mod common;

use std::path::Path;

use common::Sandbox;
use git_wt::cli;
use git_wt::options::{Choices, Kind, OptionSpec};
use serde_json::{Value, json};

const USAGE: &str = "usage: git wt add [<options>] -- <git-worktree-add-args>...
   or: git wt refresh [<options>] [<worktree>]
   or: git wt remove [-f | --force] <worktree>
   or: git wt prune
   or: git wt harness init --remote <url>
   or: git wt harness sync
   or: git wt harness merge <branch>
   or: git wt --version

";

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn version_prints_crate_version() {
    let sandbox = Sandbox::new();
    let output = sandbox.wt(&sandbox.root, &["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        stdout(&output),
        format!("git-wt version {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn no_subcommand_is_usage_error() {
    let sandbox = Sandbox::new();
    let output = sandbox.wt(&sandbox.root, &[]);
    assert_eq!(output.status.code(), Some(129));
    assert!(stderr(&output).starts_with("error: need a subcommand\nusage: git wt add"));
}

#[test]
fn help_prints_usage_to_stdout() {
    let sandbox = Sandbox::new();
    let output = sandbox.wt(&sandbox.root, &["-h"]);
    assert_eq!(output.status.code(), Some(129));
    assert_eq!(stdout(&output), USAGE);
}

#[test]
fn unknown_subcommand_is_usage_error() {
    let sandbox = Sandbox::new();
    let output = sandbox.wt(&sandbox.root, &["frobnicate"]);
    assert_eq!(output.status.code(), Some(129));
    assert!(stderr(&output).starts_with("error: unknown subcommand: `frobnicate'"));
}

fn spec(name: &str, kind: Kind, help: &str, choices: Choices) -> OptionSpec {
    OptionSpec {
        name: name.into(),
        kind,
        default: Value::Null,
        help: help.into(),
        choices,
        ask: true,
    }
}

fn plugin_options() -> Vec<OptionSpec> {
    let mut options = git_wt::options::builtin();
    options.push(spec(
        "graph",
        Kind::Confirm,
        "start the graph",
        Choices::None,
    ));
    let names = vec!["none".to_string(), "a".to_string()];
    options.push(spec("clone_db", Kind::Choice, "db", Choices::Fixed(names)));
    options
}

fn parse(args: &[&str]) -> Result<cli::Flags, String> {
    let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
    cli::parse_flags(&plugin_options(), &args, true, true).map_err(|error| error.to_string())
}

#[test]
fn option_rows_follow_git() {
    let confirm = spec(
        "clone_db",
        Kind::Confirm,
        "clone the database",
        Choices::None,
    );
    let names = vec!["none".to_string(), "a".to_string()];
    let fixed = spec("db", Kind::Choice, "db", Choices::Fixed(names));
    let dynamic = spec("db", Kind::Choice, "db", Choices::Dynamic);
    let long = spec("very_long_option_name", Kind::Text, "wraps", Choices::None);

    assert_eq!(
        cli::option_row(&confirm),
        "    --[no-]clone-db       clone the database"
    );
    assert_eq!(cli::option_row(&fixed), "    --db (none|a)         db");
    assert_eq!(cli::option_row(&dynamic), "    --db <value>          db");
    assert_eq!(
        cli::option_row(&long),
        "    --very-long-option-name <value>\n                          wraps"
    );
}

#[test]
fn flags_parse_like_argparse() {
    let flags = parse(&[
        "--no-graph",
        "--clone-db=a",
        "--yes",
        "--slot",
        "-3",
        "--",
        "-x",
    ])
    .unwrap();

    assert!(flags.yes);
    assert_eq!(flags.worktree.as_deref(), Some("-x"));
    assert_eq!(flags.values["graph"], json!(false));
    assert_eq!(flags.values["clone_db"], json!("a"));
    assert_eq!(flags.values["slot"], json!(-3));
}

#[test]
fn flag_errors_use_argparse_wording() {
    let cases = [
        (
            vec!["--graph=1"],
            "argument --graph/--no-graph: ignored explicit argument '1'",
        ),
        (
            vec!["--yes=1"],
            "argument --yes: ignored explicit argument '1'",
        ),
        (
            vec!["--clone-db", "x"],
            "argument --clone-db: invalid choice: 'x' (choose from none, a)",
        ),
        (vec!["--clone", "a"], "unrecognized arguments: --clone"),
        (
            vec!["--slot", "--graph"],
            "argument --slot: expected one argument",
        ),
    ];

    for (args, message) in cases {
        assert_eq!(parse(&args).unwrap_err(), message, "{args:?}");
    }
}

#[test]
fn flag_env_maps_flags_to_variables() {
    let flags = parse(&["--graph", "--slot", "4", "--source", "sub"]).unwrap();

    let env = cli::flag_env(&plugin_options(), &flags, Path::new("/tmp"));

    let resolved = git_wt::worktrees::resolve(Path::new("/tmp/sub"));
    assert_eq!(env["WT_GRAPH"], "1");
    assert_eq!(env["WT_SLOT"], "4");
    assert_eq!(env["WT_SOURCE"], resolved.to_string_lossy());
}

#[test]
fn add_help_lists_yes_and_option_rows() {
    let help = cli::help_text(&cli::ADD, &plugin_options());

    assert_eq!(
        help,
        "usage: git wt add [<options>] -- <git-worktree-add-args>...

    --yes                 never prompt, use defaults
    --slot <n>            slot of the worktree
    --source <path>       worktree to copy files from
    --[no-]graph          start the graph
    --clone-db (none|a)   db

"
    );
}
