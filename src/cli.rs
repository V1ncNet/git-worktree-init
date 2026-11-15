use std::io::Write;
use std::iter::Peekable;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command as Process;
use std::slice::Iter;

use serde_json::Value;

use crate::config;
use crate::engine;
use crate::error::{Error, Result};
use crate::gitenv::{self, Env};
use crate::hook;
use crate::interrupt;
use crate::options::{self, Choices, Kind, OptionSpec, Values, python_repr};
use crate::worktrees;

pub const USAGE_ERROR: i32 = 129;

pub const USAGE: &str = "usage: git wt add [<options>] -- <git-worktree-add-args>...
   or: git wt refresh [<options>] [<worktree>]
   or: git wt remove [-f | --force] <worktree>
   or: git wt prune
   or: git wt harness init --remote <url>
   or: git wt harness sync
   or: git wt harness merge <branch>
   or: git wt --version

";

const HELP_FLAGS: [&str; 2] = ["-h", "--help"];
const NOT_YET_PORTED: [&str; 2] = ["harness", "harness-hook"];
const HELP_COLUMN: usize = 26;
const YES_HELP: &str = "never prompt, use defaults";
const FORCE_HELP: &str = "remove a dirty worktree, twice for a locked one";
const REMOVE_SYNOPSIS: &str = "git wt remove [-f | --force] <worktree>";
const PRUNE_USAGE: &str = "usage: git wt prune\n\n";
const SETUP_FAILED: i32 = 2;

pub struct Command {
    pub synopsis: &'static str,
    pub with_yes: bool,
    pub with_target: bool,
}

pub const ADD: Command = Command {
    synopsis: "git wt add [<options>] -- <git-worktree-add-args>...",
    with_yes: true,
    with_target: false,
};

pub const REFRESH: Command = Command {
    synopsis: "git wt refresh [<options>] [<worktree>]",
    with_yes: false,
    with_target: true,
};

#[derive(Debug, Default, PartialEq)]
pub struct Flags {
    pub values: Values,
    pub yes: bool,
    pub worktree: Option<String>,
}

pub fn main(argv: Vec<String>) -> i32 {
    let Some((command, rest)) = argv.split_first() else {
        return usage_error("need a subcommand", USAGE);
    };
    let command = command.as_str();
    if HELP_FLAGS.contains(&command) {
        print!("{USAGE}");
        return USAGE_ERROR;
    }
    match command {
        "--version" => {
            println!("git-wt version {}", env!("CARGO_PKG_VERSION"));
            0
        }
        "hook" => hook::run(rest, &gitenv::process_env()),
        "add" => add(rest, &gitenv::process_env()),
        "refresh" => refresh(rest, &gitenv::process_env()),
        "remove" => remove(rest, &gitenv::process_env()),
        "prune" => prune(rest, &gitenv::process_env()),
        _ if NOT_YET_PORTED.contains(&command) => usage_error("not implemented yet", USAGE),
        _ => usage_error(&format!("unknown subcommand: `{command}'"), USAGE),
    }
}

pub fn usage_error(message: &str, usage: &str) -> i32 {
    let _ = write!(std::io::stderr(), "error: {message}\n{usage}");
    USAGE_ERROR
}

fn add(args: &[String], env: &Env) -> i32 {
    let Ok(cwd) = std::env::current_dir() else {
        eprintln!("worktree-init: the current directory is gone");
        return SETUP_FAILED;
    };
    let (own, forwarded) = match args.iter().position(|arg| arg == "--") {
        Some(split) => (&args[..split], Some(&args[split + 1..])),
        None => (args, None),
    };
    let options = match declared(&cwd, env) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("worktree-init: {error}");
            return SETUP_FAILED;
        }
    };
    if wants_help(own) {
        print!("{}", help_text(&ADD, &options));
        return USAGE_ERROR;
    }
    let Some(forwarded) = forwarded else {
        let message = "missing -- before the git worktree add arguments";
        return usage_error(message, &help_text(&ADD, &options));
    };
    match parse_flags(&options, own, ADD.with_yes, ADD.with_target) {
        Ok(flags) => run_worktree_add(forwarded, &add_env(&options, &flags, &cwd), &cwd),
        Err(error) => usage_error(&error.to_string(), &help_text(&ADD, &options)),
    }
}

fn add_env(options: &[OptionSpec], flags: &Flags, cwd: &Path) -> Env {
    let mut extra = flag_env(options, flags, cwd);
    if flags.yes {
        extra.insert("WT_YES".to_string(), "1".to_string());
    }
    extra
}

fn run_worktree_add(forwarded: &[String], extra: &Env, cwd: &Path) -> i32 {
    let spawned = Process::new("git")
        .args(["worktree", "add"])
        .args(forwarded)
        .envs(extra)
        .current_dir(cwd)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            eprintln!("worktree-init: cannot run git: {error}");
            return SETUP_FAILED;
        }
    };
    interrupt::on_interrupt(String::new());
    interrupt::watch_child(child.id() as i32, -1);
    match child.wait() {
        Ok(status) => status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
        Err(_) => SETUP_FAILED,
    }
}

fn refresh(args: &[String], env: &Env) -> i32 {
    let Ok(cwd) = std::env::current_dir() else {
        eprintln!("worktree-init: the current directory is gone");
        return SETUP_FAILED;
    };
    let guess = args
        .last()
        .filter(|last| !last.starts_with('-') && cwd.join(last).is_dir())
        .map_or_else(|| cwd.clone(), |last| cwd.join(last));
    let options = match declared(&guess, env) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("worktree-init: {error}");
            return SETUP_FAILED;
        }
    };
    if wants_help(args) {
        print!("{}", help_text(&REFRESH, &options));
        return USAGE_ERROR;
    }
    let flags = match parse_flags(&options, args, REFRESH.with_yes, REFRESH.with_target) {
        Ok(flags) => flags,
        Err(error) => return usage_error(&error.to_string(), &help_text(&REFRESH, &options)),
    };
    let location = flags
        .worktree
        .as_ref()
        .map_or_else(|| cwd.clone(), |worktree| cwd.join(worktree));
    let Ok(target) = worktrees::toplevel(&location, &gitenv::sanitized(env, &[])) else {
        eprintln!("worktree-init: not a git worktree: {}", location.display());
        return SETUP_FAILED;
    };
    let mut child_env = env.clone();
    child_env.extend(flag_env(&options, &flags, &cwd));
    engine::execute_or_explain(&target, "refresh", &child_env)
}

fn remove_usage() -> String {
    format!(
        "usage: {REMOVE_SYNOPSIS}\n\n{}\n\n",
        row("-f, --force", FORCE_HELP)
    )
}

fn parse_remove(args: &[String]) -> Result<(u8, String)> {
    let mut force: u8 = 0;
    let mut worktree: Vec<&str> = Vec::new();
    let mut unrecognized: Vec<&str> = Vec::new();
    let mut only_positional = false;
    for arg in args {
        if only_positional || !is_option_like(arg) {
            worktree.push(arg);
        } else if arg == "--" {
            only_positional = true;
        } else if arg == "--force" {
            force = force.saturating_add(1);
        } else if let Some(value) = arg.strip_prefix("--force=") {
            reject_inline("-f/--force", Some(value))?;
        } else if is_short_force(arg) {
            let flags = u8::try_from(arg.len() - 1).unwrap_or(u8::MAX);
            force = force.saturating_add(flags);
        } else {
            unrecognized.push(arg);
        }
    }
    let Some((first, extra)) = worktree.split_first() else {
        return Err(usage(
            "the following arguments are required: worktree".into(),
        ));
    };
    unrecognized.extend(extra);
    if !unrecognized.is_empty() {
        return Err(usage(format!(
            "unrecognized arguments: {}",
            unrecognized.join(" ")
        )));
    }
    Ok((force, (*first).to_string()))
}

fn is_short_force(arg: &str) -> bool {
    arg.len() > 1 && arg.starts_with('-') && arg[1..].chars().all(|flag| flag == 'f')
}

fn remove(args: &[String], env: &Env) -> i32 {
    if wants_help(args) {
        print!("{}", remove_usage());
        return USAGE_ERROR;
    }
    let (force, worktree) = match parse_remove(args) {
        Ok(parsed) => parsed,
        Err(error) => return usage_error(&error.to_string(), &remove_usage()),
    };
    let Ok(cwd) = std::env::current_dir() else {
        eprintln!("worktree-init: the current directory is gone");
        return SETUP_FAILED;
    };
    let location = cwd.join(worktree);
    let Ok(target) = worktrees::toplevel(&location, &gitenv::sanitized(env, &[])) else {
        eprintln!("worktree-init: not a git worktree: {}", location.display());
        return SETUP_FAILED;
    };
    match engine::remove(&target, force, env) {
        Ok(code) => code,
        Err(Error::Teardown(message)) => {
            eprintln!("worktree-init: teardown failed: {message}");
            eprintln!(
                "worktree-init: the worktree is kept, retry with: git wt remove {}",
                target.display()
            );
            1
        }
        Err(error) => {
            eprintln!("worktree-init: {error}");
            1
        }
    }
}

fn prune(args: &[String], env: &Env) -> i32 {
    if wants_help(args) {
        print!("{PRUNE_USAGE}");
        return USAGE_ERROR;
    }
    if !args.is_empty() {
        return usage_error("prune takes no arguments", PRUNE_USAGE);
    }
    let Ok(cwd) = std::env::current_dir() else {
        eprintln!("worktree-init: the current directory is gone");
        return SETUP_FAILED;
    };
    match engine::prune(&cwd, env) {
        Ok(clean) => i32::from(!clean),
        Err(error) => {
            eprintln!("worktree-init: {error}");
            1
        }
    }
}

pub fn declared(cwd: &Path, env: &Env) -> Result<Vec<OptionSpec>> {
    let base = gitenv::sanitized(env, &[]);
    let Ok(repo) = worktrees::discover(cwd, &base) else {
        return Ok(options::builtin());
    };
    let entries = config::read(cwd, &base)?;
    let plugin = engine::load(&repo, &entries, &base)?;
    Ok(engine::declared_options(plugin.as_ref()))
}

pub fn wants_help(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| HELP_FLAGS.contains(&arg.as_str()))
}

pub fn help_text(command: &Command, options: &[OptionSpec]) -> String {
    let mut rows = Vec::new();
    if command.with_yes {
        rows.push(row("--yes", YES_HELP));
    }
    rows.extend(options.iter().map(option_row));
    format!("usage: {}\n\n{}\n\n", command.synopsis, rows.join("\n"))
}

pub fn option_row(option: &OptionSpec) -> String {
    row(&flag_spec(option), &option.help)
}

fn fixed_choices(option: &OptionSpec) -> Option<&[String]> {
    match &option.choices {
        Choices::Fixed(names) if option.kind == Kind::Choice && !names.is_empty() => Some(names),
        _ => None,
    }
}

fn flag_spec(option: &OptionSpec) -> String {
    let flag = option.flag();
    if option.kind == Kind::Confirm {
        return format!("--[no-]{}", &flag[2..]);
    }
    if option.kind == Kind::Slot {
        return format!("{flag} <n>");
    }
    if option.name == "source" {
        return format!("{flag} <path>");
    }
    match fixed_choices(option) {
        Some(names) => format!("{flag} ({})", names.join("|")),
        None => format!("{flag} <value>"),
    }
}

fn row(spec: &str, text: &str) -> String {
    let left = format!("    {spec}");
    let width = left.chars().count();
    let joined = if width < HELP_COLUMN {
        format!("{left}{}{text}", " ".repeat(HELP_COLUMN - width))
    } else {
        format!("{left}\n{}{text}", " ".repeat(HELP_COLUMN))
    };
    joined.trim_end().to_string()
}

fn usage(message: String) -> Error {
    Error::Usage(message)
}

fn is_option_like(arg: &str) -> bool {
    arg.starts_with('-') && arg != "-" && !is_negative_number(arg)
}

fn is_negative_number(arg: &str) -> bool {
    let Some(digits) = arg.strip_prefix('-') else {
        return false;
    };
    let all_digits = |part: &str| part.chars().all(|character| character.is_ascii_digit());
    match digits.split_once('.') {
        None => !digits.is_empty() && all_digits(digits),
        Some((whole, fraction)) => {
            !fraction.is_empty() && all_digits(whole) && all_digits(fraction)
        }
    }
}

pub fn parse_flags(
    options: &[OptionSpec],
    args: &[String],
    with_yes: bool,
    with_target: bool,
) -> Result<Flags> {
    let mut flags = Flags::default();
    let mut unrecognized: Vec<&str> = Vec::new();
    let mut rest = args.iter().peekable();
    let mut only_positional = false;
    while let Some(arg) = rest.next() {
        if only_positional || !is_option_like(arg) {
            if with_target && flags.worktree.is_none() {
                flags.worktree = Some(arg.clone());
            } else {
                unrecognized.push(arg);
            }
            continue;
        }
        if arg == "--" {
            only_positional = true;
            continue;
        }
        let (name, inline) = match arg.split_once('=').filter(|_| arg.starts_with("--")) {
            Some((name, value)) => (name, Some(value)),
            None => (arg.as_str(), None),
        };
        if with_yes && name == "--yes" {
            reject_inline("--yes", inline)?;
            flags.yes = true;
            continue;
        }
        let Some((option, negated)) = find_option(options, name) else {
            unrecognized.push(arg);
            continue;
        };
        let value = if option.kind == Kind::Confirm {
            reject_inline(
                &format!("{0}/--no-{1}", option.flag(), &option.flag()[2..]),
                inline,
            )?;
            Value::Bool(!negated)
        } else {
            let raw = match inline {
                Some(value) => value.to_string(),
                None => take_value(&mut rest, &option.flag())?,
            };
            typed_value(option, &raw)?
        };
        flags.values.insert(option.name.clone(), value);
    }
    if !unrecognized.is_empty() {
        return Err(usage(format!(
            "unrecognized arguments: {}",
            unrecognized.join(" ")
        )));
    }
    Ok(flags)
}

fn reject_inline(label: &str, inline: Option<&str>) -> Result<()> {
    match inline {
        Some(value) => Err(usage(format!(
            "argument {label}: ignored explicit argument {}",
            python_repr(value)
        ))),
        None => Ok(()),
    }
}

fn find_option<'a>(options: &'a [OptionSpec], name: &str) -> Option<(&'a OptionSpec, bool)> {
    options.iter().find_map(|option| {
        let flag = option.flag();
        if name == flag {
            return Some((option, false));
        }
        let negated = format!("--no-{}", &flag[2..]);
        (option.kind == Kind::Confirm && name == negated).then_some((option, true))
    })
}

fn take_value(rest: &mut Peekable<Iter<'_, String>>, flag: &str) -> Result<String> {
    rest.next_if(|next| !is_option_like(next))
        .cloned()
        .ok_or_else(|| usage(format!("argument {flag}: expected one argument")))
}

fn typed_value(option: &OptionSpec, raw: &str) -> Result<Value> {
    let flag = option.flag();
    if option.kind == Kind::Slot {
        return raw.trim().parse::<i64>().map(Value::from).map_err(|_| {
            usage(format!(
                "argument {flag}: invalid int value: {}",
                python_repr(raw)
            ))
        });
    }
    if let Some(names) = fixed_choices(option)
        && !names.iter().any(|name| name == raw)
    {
        return Err(usage(format!(
            "argument {flag}: invalid choice: {} (choose from {})",
            python_repr(raw),
            names.join(", ")
        )));
    }
    Ok(Value::String(raw.to_string()))
}

pub fn flag_env(options: &[OptionSpec], flags: &Flags, cwd: &Path) -> Env {
    options
        .iter()
        .filter_map(|option| {
            let value = flags.values.get(&option.name)?;
            let text = match value {
                Value::Bool(set) => if *set { "1" } else { "0" }.to_string(),
                Value::String(raw) if option.name == "source" => worktrees::resolve(&cwd.join(raw))
                    .to_string_lossy()
                    .into_owned(),
                Value::String(raw) => raw.clone(),
                other => other.to_string(),
            };
            Some((option.env_key(), text))
        })
        .collect()
}
