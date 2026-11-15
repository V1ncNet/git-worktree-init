use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::bridge::{self, Ctx, Plugin};
use crate::config::{self, Entries};
use crate::error::{Error, Result};
use crate::gitenv::{self, Env};
use crate::options::{self, OptionSpec, Values};
use crate::prompt::{self, Prompter};
use crate::records::{self, Record};
use crate::worktrees::{self, Repo};
use crate::{fsops, include, interrupt, slots};

pub fn repair_hint(target: &Path) -> String {
    format!(
        "worktree-init: the worktree exists, repair it with: git wt refresh {}\n",
        target.display()
    )
}

pub fn execute_or_explain(target: &Path, mode: &str, env: &Env) -> i32 {
    match execute(target, mode, env) {
        Ok(()) => 0,
        Err(error) => {
            eprint!("worktree-init: {error}\n{}", repair_hint(target));
            1
        }
    }
}

pub fn locked(common: &Path) -> Result<File> {
    let handle = File::create(common.join("worktree-init.lock"))?;
    handle.lock()?;
    Ok(handle)
}

pub fn load(repo: &Repo, entries: &Entries, env: &Env) -> Result<Option<Plugin>> {
    let Some(path) = bridge::locate(&repo.main, entries) else {
        return Ok(None);
    };
    let python = bridge::python(env, &repo.main);
    Plugin::start(&path, &python, env).map(Some)
}

pub fn declared_options(plugin: Option<&Plugin>) -> Vec<OptionSpec> {
    let mut declared = options::builtin();
    declared.extend(
        plugin
            .map_or(&[][..], |plugin| &plugin.options)
            .iter()
            .cloned(),
    );
    declared
}

struct Setup {
    repo: Repo,
    entries: Entries,
    plugin: Option<Plugin>,
    base_env: Env,
    run_env: Env,
    declared: Vec<OptionSpec>,
    patterns: Vec<String>,
    reserved: BTreeSet<u32>,
    record: Option<Record>,
    own: Option<u32>,
}

pub fn execute(target: &Path, mode: &str, env: &Env) -> Result<()> {
    interrupt::on_interrupt(format!("\nworktree-init: aborted\n{}", repair_hint(target)));
    let mut setup = set_up(target, env)?;
    let mut ctx = Ctx {
        main: setup.repo.main.clone(),
        target: target.to_path_buf(),
        common_dir: setup.repo.common_dir.clone(),
        mode: mode.to_string(),
        env: setup.run_env.clone(),
        worktrees: setup.repo.worktrees.clone(),
        source: None,
        slot: None,
        options: Values::new(),
    };
    let explicit = options::explicit(&setup.declared, env)?;
    let config_values = options::from_config(&setup.declared, &setup.entries)?;
    let answers =
        if mode == "create" && options::interactive_wanted(&setup.declared, env, &setup.entries) {
            ask_on_tty(&mut setup, &config_values, &ctx)?
        } else {
            Values::new()
        };
    let values = options::resolve(&setup.declared, &explicit, &answers, &config_values);
    {
        let _lock = locked(&setup.repo.common_dir)?;
        prepare(&mut setup, &mut ctx, values)?;
    }
    print_next_steps(setup.plugin.as_mut())
}

fn set_up(target: &Path, env: &Env) -> Result<Setup> {
    let base_env = gitenv::sanitized(env, &[]);
    let entries = config::read(target, &base_env)?;
    let repo = worktrees::discover(target, &base_env)?;
    let mut plugin = load(&repo, &entries, &base_env)?;
    if let Some(plugin) = &plugin {
        interrupt::watch_child(plugin.pid(), plugin.request_fd());
    }
    let sanitize_env = plugin
        .as_ref()
        .map_or(&[][..], |plugin| &plugin.sanitize_env);
    let run_env = gitenv::sanitized(env, sanitize_env);
    let declared = declared_options(plugin.as_ref());
    let patterns = config::ignore_patterns(&entries);
    let reserved = slots::parse_reserved(config::last(&entries, "reservedSlots"))?;
    let record = match &repo.admin_dir {
        Some(admin_dir) => records::find(&repo.common_dir, admin_dir)?,
        None => None,
    };
    let own = slot_of(&mut plugin, target)?.or(record.as_ref().and_then(|record| record.slot));
    Ok(Setup {
        repo,
        entries,
        plugin,
        base_env,
        run_env,
        declared,
        patterns,
        reserved,
        record,
        own,
    })
}

fn slot_of(plugin: &mut Option<Plugin>, worktree: &Path) -> Result<Option<u32>> {
    match plugin {
        Some(plugin) => plugin.slot_of(worktree),
        None => Ok(None),
    }
}

fn used_slots(setup: &mut Setup, target: &Path) -> Result<BTreeMap<u32, PathBuf>> {
    let scanned: Vec<PathBuf> = setup
        .repo
        .worktrees
        .iter()
        .filter(|worktree| !config::path_ignored(worktree, &setup.patterns))
        .cloned()
        .collect();
    let plugin = &mut setup.plugin;
    let mut used = slots::used(&mut |worktree| slot_of(plugin, worktree), &scanned, target)?;
    used.extend(pending(&setup.repo, setup.record.as_ref())?);
    Ok(used)
}

fn pending(repo: &Repo, own: Option<&Record>) -> Result<BTreeMap<u32, PathBuf>> {
    Ok(records::all_records(&repo.common_dir)?
        .into_iter()
        .filter(|record| own.is_none_or(|own| own.key != record.key))
        .filter_map(|record| record.slot.map(|slot| (slot, record.target)))
        .collect())
}

fn ask_on_tty(setup: &mut Setup, config_values: &Values, ctx: &Ctx) -> Result<Values> {
    let Some((reader, writer)) = prompt::open_tty() else {
        return Ok(Values::new());
    };
    let recommended = slots::allocate(&used_slots(setup, &ctx.target)?, &setup.reserved);
    let mut defaults = options::resolve(
        &setup.declared,
        &Values::new(),
        &Values::new(),
        config_values,
    );
    if defaults["slot"].is_null() {
        defaults.insert("slot".into(), recommended.into());
    }
    let plugin = &mut setup.plugin;
    let mut choices = |option: &str| match plugin {
        Some(plugin) => plugin.choices(option, ctx),
        None => Ok(Vec::new()),
    };
    let mut prompter = Prompter::new(reader, writer);
    let mut answers = prompt::ask(&setup.declared, &defaults, &mut prompter, &mut choices)?;
    if answers.get("slot") == Some(&Value::from(recommended)) {
        answers.remove("slot");
    }
    Ok(answers)
}

fn prepare(setup: &mut Setup, ctx: &mut Ctx, mut values: Values) -> Result<()> {
    setup.repo = worktrees::discover(&ctx.target, &setup.base_env)?;
    ctx.worktrees = setup.repo.worktrees.clone();
    if let Some(plugin) = setup.plugin.as_mut() {
        sweep(&setup.repo, plugin, &setup.run_env)?;
    }
    let used = used_slots(setup, &ctx.target)?;
    let slot = choose_slot(&values["slot"], setup.own, &used, &setup.reserved)?;
    values.insert("slot".into(), slot.into());
    let source = resolve_source(&values["source"], &setup.repo, &setup.base_env)?;
    ctx.slot = Some(slot);
    ctx.source = Some(source.clone());
    ctx.options = values;
    if setup.plugin.is_some()
        && let Some(admin_dir) = &setup.repo.admin_dir
    {
        records::write(
            &setup.repo.common_dir,
            admin_dir,
            ctx.slot,
            &ctx.target,
            &ctx.options,
        )?;
    }
    eprintln!(
        "worktree-init: {} {} (slot {slot}, source {})",
        ctx.mode,
        ctx.target.display(),
        source.display()
    );
    copy_files(setup, ctx, &source)?;
    match setup.plugin.as_mut() {
        Some(plugin) => plugin.apply(ctx),
        None => Ok(()),
    }
}

fn choose_slot(
    requested: &Value,
    own: Option<u32>,
    used: &BTreeMap<u32, PathBuf>,
    reserved: &BTreeSet<u32>,
) -> Result<u32> {
    if requested.is_null() {
        return Ok(own.unwrap_or_else(|| slots::allocate(used, reserved)));
    }
    let slot = requested
        .as_u64()
        .and_then(|slot| u32::try_from(slot).ok())
        .ok_or_else(|| Error::Other(format!("slot {requested} is out of range")))?;
    if Some(slot) != own {
        slots::validate(slot, used, reserved)?;
    }
    Ok(slot)
}

fn resolve_source(raw: &Value, repo: &Repo, env: &Env) -> Result<PathBuf> {
    let Some(raw) = raw.as_str().filter(|raw| !raw.is_empty()) else {
        return Ok(repo.main.clone());
    };
    let path = bridge::expand_home(raw);
    let invalid = |reason: &str| Error::Other(format!("{reason}: {}", path.display()));
    if !path.is_absolute() {
        return Err(Error::Other(format!(
            "source must be an absolute path, e.g. WT_SOURCE=$PWD (got {raw})"
        )));
    }
    if !path.is_dir() {
        return Err(invalid("source is not a directory"));
    }
    let other =
        worktrees::discover(&path, env).map_err(|_| invalid("source is not a git worktree"))?;
    if other.common_dir != repo.common_dir {
        return Err(invalid("source belongs to another repository"));
    }
    worktrees::toplevel(&path, env)
}

fn copy_files(setup: &Setup, ctx: &Ctx, source: &Path) -> Result<()> {
    let spec = include::include_file(source, &setup.repo.main);
    let overwrite = ctx.mode == "create" && setup.own.is_none();
    for rel in include::included(source, &setup.run_env, &spec)? {
        if fsops::copy(&rel, source, &ctx.target, overwrite)? {
            eprintln!("  copied {rel}");
        }
    }
    for rel in config::all_values(&setup.entries, "localInclude") {
        if fsops::copy(&rel, &setup.repo.main, &ctx.target, false)? {
            eprintln!("  copied {rel}");
        }
    }
    Ok(())
}

fn print_next_steps(plugin: Option<&mut Plugin>) -> Result<()> {
    let Some(plugin) = plugin.filter(|plugin| plugin.has_next_steps) else {
        return Ok(());
    };
    let steps = plugin.next_steps()?;
    if !steps.is_empty() {
        eprintln!("Next steps:");
        for step in steps {
            eprintln!("  - {step}");
        }
    }
    Ok(())
}

pub fn sweep(repo: &Repo, plugin: &mut Plugin, env: &Env) -> Result<bool> {
    let mut clean = true;
    for record in records::all_records(&repo.common_dir)? {
        if records::alive(&repo.common_dir, &record) {
            continue;
        }
        if plugin.has_teardown {
            let target = record.target.display();
            let slot = record
                .slot
                .map_or_else(|| "None".to_string(), |slot| slot.to_string());
            eprintln!("worktree-init: prune {target} (slot {slot})");
            if let Err(error) = plugin.teardown(&teardown_context(repo, &record, env)) {
                eprintln!("worktree-init: teardown failed: {error}");
                eprintln!("worktree-init: the record is kept, retry with: git wt prune");
                clean = false;
                continue;
            }
        }
        records::discard(&repo.common_dir, &record)?;
    }
    Ok(clean)
}

fn teardown_context(repo: &Repo, record: &Record, env: &Env) -> Ctx {
    Ctx {
        main: repo.main.clone(),
        target: record.target.clone(),
        common_dir: repo.common_dir.clone(),
        mode: "prune".into(),
        env: env.clone(),
        worktrees: repo.worktrees.clone(),
        source: None,
        slot: record.slot,
        options: record.options.clone(),
    }
}
