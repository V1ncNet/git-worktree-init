mod common;

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use common::{Sandbox, register_hook, text, wt_bin};

const ADD_HELP: &str = "usage: git wt add [<options>] -- <git-worktree-add-args>...

    --yes                 never prompt, use defaults
    --slot <n>            slot of the worktree
    --source <path>       worktree to copy files from
    --port-base <value>   first port

";

fn example_plugin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/minimal_plugin.py")
}

fn configured(sandbox: &Sandbox) -> PathBuf {
    let repo = sandbox.repo();
    register_hook(sandbox, &repo);
    let plugin = example_plugin();
    sandbox.git(
        &repo,
        &["config", "worktree-init.plugin", plugin.to_str().unwrap()],
    );
    sandbox.git(&repo, &["config", "worktree-init.reservedSlots", "0"]);
    repo
}

fn sibling(repo: &Path, name: &str) -> PathBuf {
    repo.parent().unwrap().join(name)
}

fn path_with_binary_directory() -> OsString {
    let binary_directory = Path::new(wt_bin()).parent().unwrap().to_path_buf();
    let mut paths = vec![binary_directory];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    std::env::join_paths(paths).unwrap()
}

#[test]
fn flags_become_options() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);

    let output = sandbox.wt(
        &repo,
        &[
            "add",
            "--slot",
            "7",
            "--port-base",
            "9000",
            "--",
            "-b",
            "a",
            "../a",
        ],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let env_file = fs::read_to_string(sibling(&repo, "a").join(".env")).unwrap();
    assert_eq!(env_file, "SLOT=7\nPORT=9007\n");
}

#[test]
fn unknown_flag_creates_nothing() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);

    let output = sandbox.wt(&repo, &["add", "--bogus", "--", "-b", "a", "../a"]);

    assert_eq!(output.status.code(), Some(129));
    assert!(!sibling(&repo, "a").exists());
    assert_eq!(
        text(&sandbox.git(&repo, &["branch", "--list", "a"]).stdout),
        ""
    );
}

#[test]
fn abbreviated_flag_is_rejected() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);

    let output = sandbox.wt(&repo, &["add", "--sl", "3", "--", "-b", "a", "../a"]);

    assert_eq!(output.status.code(), Some(129));
    assert!(!sibling(&repo, "a").exists());
}

#[test]
fn missing_separator_is_usage_error() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);

    let unparsed = sandbox.wt(&repo, &["add", "-b", "a", "../a"]);
    let missing = sandbox.wt(&repo, &["add", "--yes"]);

    for output in [unparsed, missing] {
        assert_eq!(output.status.code(), Some(129));
        assert!(text(&output.stderr).starts_with(
            "error: missing -- before the git worktree add arguments\nusage: git wt add"
        ));
    }
}

#[test]
fn help_follows_git_layout_and_lists_plugin_flags() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);

    let output = sandbox.wt(&repo, &["add", "-h"]);

    assert_eq!(output.status.code(), Some(129));
    assert_eq!(text(&output.stdout), ADD_HELP);
}

#[test]
fn long_help_matches_short_help() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);

    let long = sandbox.wt(&repo, &["add", "--help"]);
    let short = sandbox.wt(&repo, &["add", "-h"]);

    assert_eq!(text(&long.stdout), text(&short.stdout));
}

#[test]
fn help_wins_over_invalid_arguments() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();

    let output = sandbox.wt(&repo, &["add", "-h", "--bogus"]);

    assert_eq!(output.status.code(), Some(129));
    assert!(text(&output.stdout).starts_with("usage: git wt add"));
    assert_eq!(text(&output.stderr), "");
}

#[test]
fn help_flag_after_separator_is_no_help() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();

    let output = sandbox.wt(&repo, &["refresh", "--", "-h"]);

    assert_eq!(text(&output.stdout), "");
    assert!(text(&output.stderr).contains("not a git worktree"));
}

#[test]
fn refresh_help_shows_worktree_argument_without_yes() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();

    let output = sandbox.wt(&repo, &["refresh", "-h"]);

    assert_eq!(output.status.code(), Some(129));
    assert!(
        text(&output.stdout)
            .starts_with("usage: git wt refresh [<options>] [<worktree>]\n\n    --slot <n>")
    );
}

#[test]
fn usage_error_explains_and_prints_help_to_stderr() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);

    let output = sandbox.wt(&repo, &["add", "--slot", "x", "--", "../a"]);

    assert_eq!(output.status.code(), Some(129));
    assert!(
        text(&output.stderr)
            .starts_with("error: argument --slot: invalid int value: 'x'\nusage: git wt add")
    );
    assert_eq!(text(&output.stdout), "");
}

#[test]
fn git_exit_code_is_passed_through() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);
    let busy = sibling(&repo, "busy");
    fs::create_dir(&busy).unwrap();
    fs::write(busy.join("file"), "x").unwrap();

    let output = sandbox.wt(&repo, &["add", "--", "../busy"]);

    assert_eq!(output.status.code(), Some(128));
}

#[test]
fn runs_from_subdirectory_through_git() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);
    let subdirectory = repo.join("sub");
    fs::create_dir(&subdirectory).unwrap();

    let output = sandbox
        .command("git", &subdirectory)
        .env("PATH", path_with_binary_directory())
        .args(["wt", "add", "--yes", "--", "-b", "p", "../../prefixed"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(sibling(&repo, "prefixed").join(".env").is_file());
}

#[test]
fn relative_source_is_made_absolute() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);
    let subdirectory = repo.join("sub");
    fs::create_dir(&subdirectory).unwrap();

    let output = sandbox.wt(
        &subdirectory,
        &["add", "--source", "..", "--", "-b", "s", "../../s"],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains(&format!("source {})", repo.display())));
}

#[test]
fn refresh_reads_flags_from_target_repository() {
    let sandbox = Sandbox::new();
    let repo = configured(&sandbox);
    sandbox.wt(&repo, &["add", "--yes", "--", "-b", "a", "../a"]);
    let elsewhere = sandbox.root.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let target = sibling(&repo, "a");

    let output = sandbox.wt(
        &elsewhere,
        &["refresh", "--port-base", "9100", target.to_str().unwrap()],
    );

    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let env_file = fs::read_to_string(target.join(".env")).unwrap();
    assert_eq!(env_file, "SLOT=1\nPORT=9101\n");
}
