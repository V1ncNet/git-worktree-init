mod common;

use common::Sandbox;

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
