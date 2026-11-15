mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use common::{Sandbox, text};

const ZEROS: &str = "0000000000000000000000000000000000000000";

const REFRESH_HELP: &str = "usage: git wt refresh [<options>] [<worktree>]

    --slot <n>            slot of the worktree
    --source <path>       worktree to copy files from
    --port-base <value>   first port

";

fn example_plugin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/minimal_plugin.py")
}

fn configured(sandbox: &Sandbox) -> (PathBuf, PathBuf) {
    let repo = sandbox.repo();
    use_plugin(sandbox, &repo, &example_plugin());
    sandbox.git(&repo, &["config", "worktree-init.reservedSlots", "0"]);
    sandbox.git(&repo, &["worktree", "add", "-q", "-b", "w", "../w"]);
    (repo, sandbox.root.join("w"))
}

fn use_plugin(sandbox: &Sandbox, repo: &Path, plugin: &Path) {
    sandbox.git(
        repo,
        &["config", "worktree-init.plugin", plugin.to_str().unwrap()],
    );
}

fn write_plugin(sandbox: &Sandbox, text: &str) -> PathBuf {
    let path = sandbox.root.join("plugin.py");
    fs::write(&path, text).unwrap();
    path
}

fn create(sandbox: &Sandbox, target: &Path, env: &[(&str, &str)]) -> Output {
    sandbox
        .wt_command(target)
        .args(["hook", ZEROS, ZEROS, "1"])
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

fn refresh(sandbox: &Sandbox, cwd: &Path, args: &[&str]) -> Output {
    let mut full = vec!["refresh"];
    full.extend(args);
    sandbox.wt(cwd, &full)
}

fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path).unwrap()
}

fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "exit {:?}: {}",
        output.status.code(),
        text(&output.stderr)
    );
}

fn assert_fails_with(output: &Output, message: &str) {
    assert_eq!(output.status.code(), Some(1));
    let stderr = text(&output.stderr);
    assert!(stderr.contains(message), "{stderr}");
    assert!(
        stderr.contains("repair it with: git wt refresh"),
        "{stderr}"
    );
}

#[test]
fn no_tty_uses_defaults() {
    let sandbox = Sandbox::new();
    let (_, target) = configured(&sandbox);

    let output = sandbox
        .wt_command(&target)
        .args(["hook", ZEROS, ZEROS, "1"])
        .env_remove("GIT_TERMINAL_PROMPT")
        .output()
        .unwrap();

    assert_ok(&output);
    assert_eq!(read(target.join(".env")), "SLOT=1\nPORT=8001\n");
}

#[test]
fn taken_slot_is_rejected() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    fs::write(repo.join(".env"), "SLOT=5\n").unwrap();

    let output = create(&sandbox, &target, &[("WT_SLOT", "5")]);

    assert_fails_with(&output, "slot 5 is taken");
}

#[test]
fn reserved_slot_is_rejected() {
    let sandbox = Sandbox::new();
    let (_, target) = configured(&sandbox);

    let output = create(&sandbox, &target, &[("WT_SLOT", "0")]);

    assert_fails_with(&output, "reserved");
}

#[test]
fn refresh_keeps_own_slot() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    fs::write(target.join(".env"), "SLOT=6\n").unwrap();

    assert_ok(&refresh(&sandbox, &repo, &[target.to_str().unwrap()]));

    assert_eq!(read(target.join(".env")), "SLOT=6\nPORT=8006\n");
}

#[test]
fn create_on_prepared_worktree_keeps_slot_and_files() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    fs::write(repo.join(".worktreeinclude"), "*.log\n").unwrap();
    fs::write(repo.join("a.log"), "new").unwrap();
    fs::write(target.join("a.log"), "old").unwrap();
    fs::write(target.join(".env"), "SLOT=6\n").unwrap();

    assert_ok(&create(&sandbox, &target, &[]));

    assert_eq!(read(target.join(".env")), "SLOT=6\nPORT=8006\n");
    assert_eq!(read(target.join("a.log")), "old");
}

#[test]
fn local_include_copies_from_main_once() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    sandbox.git(
        &repo,
        &["config", "worktree-init.localInclude", "CLAUDE.local.md"],
    );
    fs::write(repo.join("CLAUDE.local.md"), "private").unwrap();

    assert_ok(&create(&sandbox, &target, &[]));

    assert_eq!(read(target.join("CLAUDE.local.md")), "private");
}

#[test]
fn local_include_never_overwrites() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    sandbox.git(
        &repo,
        &["config", "worktree-init.localInclude", "CLAUDE.local.md"],
    );
    fs::write(repo.join("CLAUDE.local.md"), "private").unwrap();
    fs::write(target.join("CLAUDE.local.md"), "changed").unwrap();

    assert_ok(&refresh(&sandbox, &repo, &[target.to_str().unwrap()]));

    assert_eq!(read(target.join("CLAUDE.local.md")), "changed");
}

#[test]
fn local_include_uses_main_even_with_source() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    sandbox.git(
        &repo,
        &["config", "worktree-init.localInclude", "CLAUDE.local.md"],
    );
    sandbox.git(&repo, &["worktree", "add", "-q", "-b", "s", "../s"]);
    let source = sandbox.root.join("s");
    fs::write(repo.join("CLAUDE.local.md"), "from main").unwrap();
    fs::write(source.join("CLAUDE.local.md"), "from source").unwrap();

    let output = create(
        &sandbox,
        &target,
        &[("WT_SOURCE", source.to_str().unwrap())],
    );

    assert_ok(&output);
    assert_eq!(read(target.join("CLAUDE.local.md")), "from main");
}

#[test]
fn local_include_skips_missing() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    sandbox.git(
        &repo,
        &["config", "worktree-init.localInclude", "CLAUDE.local.md"],
    );

    assert_ok(&create(&sandbox, &target, &[]));

    assert!(!target.join("CLAUDE.local.md").exists());
}

#[test]
fn relative_source_is_rejected() {
    let sandbox = Sandbox::new();
    let (_, target) = configured(&sandbox);

    let output = create(&sandbox, &target, &[("WT_SOURCE", ".")]);

    assert_fails_with(&output, "source must be an absolute path");
}

#[test]
fn source_from_other_repository_is_rejected() {
    let sandbox = Sandbox::new();
    let (_, target) = configured(&sandbox);
    let other = sandbox.root.join("other");
    fs::create_dir(&other).unwrap();
    sandbox.git(&other, &["init", "-q"]);

    let output = create(&sandbox, &target, &[("WT_SOURCE", other.to_str().unwrap())]);

    assert_fails_with(&output, "another repository");
}

#[test]
fn refresh_reports_setup_errors() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    let broken = write_plugin(&sandbox, "raise RuntimeError('boom')\n");
    use_plugin(&sandbox, &repo, &broken);

    let output = refresh(&sandbox, &repo, &[target.to_str().unwrap()]);

    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).starts_with("worktree-init: "));
    assert!(text(&output.stderr).contains("boom"));
}

#[test]
fn sweep_releases_port_of_deleted_worktree() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    sandbox.git(&repo, &["config", "worktree-init.port-base", "9000"]);
    assert_ok(&create(&sandbox, &target, &[]));
    fs::remove_dir_all(&target).unwrap();
    sandbox.git(&repo, &["worktree", "add", "-q", "-b", "x", "../x"]);

    let output = refresh(&sandbox, &repo, &["../x"]);

    assert_ok(&output);
    assert!(text(&output.stderr).contains(&format!(
        "worktree-init: prune {} (slot 1)",
        target.display()
    )));
    assert!(text(&output.stderr).contains("released port 9001"));
}

#[test]
fn plugin_output_reaches_stdout() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    let noisy = write_plugin(
        &sandbox,
        "def slot_of(worktree):\n    return None\n\ndef apply(ctx):\n    print('noise')\n    ctx.run(['echo', 'child'])\n",
    );
    use_plugin(&sandbox, &repo, &noisy);

    let output = refresh(&sandbox, &repo, &[target.to_str().unwrap()]);

    assert_ok(&output);
    assert!(text(&output.stdout).contains("noise"));
    assert!(text(&output.stdout).contains("child"));
}

#[test]
fn sanitize_env_reaches_plugin_children() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);
    let dump = sandbox.root.join("env.txt");
    let plugin = write_plugin(
        &sandbox,
        &format!(
            "SANITIZE_ENV = ['VIRTUAL_ENV']\n\ndef slot_of(worktree):\n    return None\n\ndef apply(ctx):\n    open({:?}, 'w').write(ctx.run(['env'], capture=True).stdout)\n",
            dump.to_str().unwrap()
        ),
    );
    use_plugin(&sandbox, &repo, &plugin);
    let git_dir = repo.join(".git");

    let output = sandbox
        .wt_command(&repo)
        .args(["refresh", target.to_str().unwrap()])
        .env("VIRTUAL_ENV", "/x")
        .env("GIT_DIR", &git_dir)
        .output()
        .unwrap();

    assert_ok(&output);
    let env = read(&dump);
    assert!(env.contains("HOME="), "{env}");
    assert!(!env.contains("VIRTUAL_ENV"), "{env}");
    assert!(!env.contains("GIT_DIR"), "{env}");
}

#[test]
fn refresh_flags_override_config() {
    let sandbox = Sandbox::new();
    let (repo, target) = configured(&sandbox);

    let output = refresh(
        &sandbox,
        &repo,
        &["--slot", "3", "--port-base=7000", target.to_str().unwrap()],
    );

    assert_ok(&output);
    assert_eq!(read(target.join(".env")), "SLOT=3\nPORT=7003\n");
}

#[test]
fn refresh_help_lists_plugin_options() {
    let sandbox = Sandbox::new();
    let (repo, _) = configured(&sandbox);

    let output = refresh(&sandbox, &repo, &["--port-base", "1", "-h"]);

    assert_eq!(output.status.code(), Some(129));
    assert_eq!(text(&output.stdout), REFRESH_HELP);
    assert_eq!(text(&output.stderr), "");
}

#[test]
fn refresh_usage_errors_follow_argparse() {
    let sandbox = Sandbox::new();
    let (repo, _) = configured(&sandbox);
    let cases: [(&[&str], &str); 5] = [
        (&["--slot", "x"], "argument --slot: invalid int value: 'x'"),
        (
            &["--port-base"],
            "argument --port-base: expected one argument",
        ),
        (&["--port", "1"], "unrecognized arguments: --port"),
        (&["a", "b", "--zz"], "unrecognized arguments: b --zz"),
        (
            &["--slot", "1", "--slot"],
            "argument --slot: expected one argument",
        ),
    ];

    for (args, message) in cases {
        let output = refresh(&sandbox, &repo, args);
        assert_eq!(output.status.code(), Some(129), "{args:?}");
        assert_eq!(
            text(&output.stderr),
            format!("error: {message}\n{REFRESH_HELP}"),
            "{args:?}"
        );
    }
}

#[test]
fn refresh_outside_worktree_is_reported() {
    let sandbox = Sandbox::new();
    let (repo, _) = configured(&sandbox);

    let output = refresh(&sandbox, &repo, &["/nonexistent"]);

    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        text(&output.stderr),
        "worktree-init: not a git worktree: /nonexistent\n"
    );
}
