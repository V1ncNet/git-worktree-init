mod common;

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use common::{Sandbox, text, wt_bin};
use git_wt::gitenv::{self, Env};
use git_wt::harness;

const ZEROS: &str = "0000000000000000000000000000000000000000";
const ONES: &str = "1111111111111111111111111111111111111111";
const RUNTIME_BLOCK: &str = "# claude-code-runtime\n**/.claude/worktrees/\n**/.claude/scheduled_tasks.lock\n\n# other\nfoo/\n";
const LEAKING: [&str; 3] = ["GIT_CONFIG_PARAMETERS", "COMPOSE_PROJECT_NAME", "CI"];
const HARNESS_USAGE: &str = "usage: git wt harness init --remote <url>
   or: git wt harness sync
   or: git wt harness merge <branch>

";

fn env_of(sandbox: &Sandbox) -> Env {
    let extra: Vec<String> = LEAKING.iter().map(|key| key.to_string()).collect();
    let mut env = gitenv::sanitized(&gitenv::process_env(), &extra);
    env.retain(|key, _| !key.starts_with("WT_"));
    env.extend(sandbox.env());
    env
}

fn git_ok(sandbox: &Sandbox, cwd: &Path, args: &[&str]) -> String {
    let output = sandbox.git(cwd, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        text(&output.stderr)
    );
    text(&output.stdout)
}

fn hgit(sandbox: &Sandbox, target: &Path, args: &[&str]) -> String {
    harness::git(target, args, &env_of(sandbox)).unwrap()
}

fn path(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|item| item.to_string()).collect()
}

fn write_executable(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

fn code(sandbox: &Sandbox) -> PathBuf {
    let repo = sandbox.repo();
    let exclude = repo.join(".git/info/exclude");
    let mut content = read(&exclude);
    content.push_str("CLAUDE.md\n.claude/\n.superpowers/\ndocs/superpowers/\n");
    content.push_str(RUNTIME_BLOCK);
    fs::write(&exclude, content).unwrap();
    git_ok(
        sandbox,
        &repo,
        &["config", harness::DEFAULT_BRANCH_KEY, "develop"],
    );
    repo
}

fn harnessed(sandbox: &Sandbox) -> PathBuf {
    let repo = code(sandbox);
    harness::init(&repo, "/nonexistent/harness.git", &env_of(sandbox)).unwrap();
    repo
}

fn harness_root(main: &Path) -> PathBuf {
    main.join(".git/harness.git")
}

fn ignored(sandbox: &Sandbox, target: &Path, paths: &[&str]) -> BTreeSet<String> {
    let mut args = vec!["check-ignore", "--no-index"];
    args.extend(paths);
    let output = harness::git_unchecked(target, &args, &env_of(sandbox)).unwrap();
    output.lines().map(str::to_string).collect()
}

fn register_harness_hook(sandbox: &Sandbox, main: &Path, binary: &str, events: &[&str]) {
    let command = format!(r#"f="{binary}"; [ -x "$f" ] || exit 0; exec "$f" harness-hook"#);
    git_ok(sandbox, main, &["config", "hook.harness.command", &command]);
    add_events(sandbox, main, events);
}

fn add_events(sandbox: &Sandbox, main: &Path, events: &[&str]) {
    for event in events {
        git_ok(
            sandbox,
            main,
            &["config", "--add", "hook.harness.event", event],
        );
    }
}

fn run_harness_hook(
    sandbox: &Sandbox,
    cwd: &Path,
    args: &[&str],
    input: &str,
    extra: &[(&str, &str)],
) -> Output {
    let mut child = sandbox
        .wt_command(cwd)
        .arg("harness-hook")
        .args(args)
        .envs(extra.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    child.wait_with_output().unwrap()
}

fn seeded(sandbox: &Sandbox) -> PathBuf {
    let main = harnessed(sandbox);
    fs::write(main.join("CLAUDE.md"), "develop").unwrap();
    hgit(sandbox, &main, &["add", "CLAUDE.md"]);
    hgit(sandbox, &main, &["commit", "-q", "-m", "Seed"]);
    register_harness_hook(sandbox, &main, wt_bin(), &["post-checkout"]);
    main
}

fn harness_branch_with(sandbox: &Sandbox, main: &Path, branch: &str, rel: &str, content: &str) {
    hgit(sandbox, main, &["checkout", "-q", "-b", branch]);
    write(&main.join(rel), content);
    hgit(sandbox, main, &["add", rel]);
    hgit(
        sandbox,
        main,
        &["commit", "-q", "-m", &format!("Add {rel}")],
    );
    hgit(sandbox, main, &["checkout", "-q", "develop"]);
}

fn current(sandbox: &Sandbox, target: &Path) -> String {
    hgit(sandbox, target, &["symbolic-ref", "--short", "HEAD"])
        .trim()
        .to_string()
}

fn pushing(sandbox: &Sandbox) -> (PathBuf, PathBuf) {
    let main = seeded(sandbox);
    let origin = sandbox.root.join("origin.git");
    let remote = sandbox.root.join("harness-remote.git");
    for bare in [&origin, &remote] {
        git_ok(
            sandbox,
            &sandbox.root,
            &["init", "-q", "--bare", path(bare)],
        );
    }
    git_ok(sandbox, &main, &["remote", "add", "origin", path(&origin)]);
    let root = harness_root(&main);
    git_ok(
        sandbox,
        &root,
        &["remote", "set-url", "harness", path(&remote)],
    );
    add_events(sandbox, &main, &["pre-push"]);
    git_ok(
        sandbox,
        &main,
        &["worktree", "add", "-q", "-b", "feature/foo", "../foo"],
    );
    let target = sandbox.root.join("foo");
    write(&target.join(".claude/h.md"), "handoff");
    (target, remote)
}

fn fake_ssh(sandbox: &Sandbox) -> (PathBuf, PathBuf) {
    let log = sandbox.root.join("ssh-args");
    let bin = sandbox.root.join("bin");
    write_executable(
        &bin.join("ssh"),
        &format!(
            "#!/bin/sh\necho \"$@\" >> \"{}\"\nexit 255\n",
            log.display()
        ),
    );
    (bin, log)
}

fn search_path(bin: &Path) -> String {
    format!("{}:{}", bin.display(), std::env::var("PATH").unwrap())
}

#[test]
fn init_creates_bare_repo_with_allowlist() {
    let sandbox = Sandbox::new();
    let main = harnessed(&sandbox);
    let hidden = ignored(
        &sandbox,
        &main,
        &[
            "src/Main.java",
            ".idea/misc.xml",
            "CLAUDE.md",
            "CLAUDE.local.md",
            ".claude/handoffs/x.md",
            "docs/superpowers/specs/a.md",
            "docs/other.md",
            ".claude/settings.local.json",
            ".claude/worktrees/agent/x",
            ".claude/scheduled_tasks.lock",
            ".claude/hooks/__pycache__/a.pyc",
            "foo/bar",
        ],
    );
    let root = harness_root(&main);

    assert_eq!(harness::git_dir(&main.join(".git")), root);
    assert!(harness::enabled(&main.join(".git")));
    assert_eq!(
        git_ok(&sandbox, &root, &["config", "core.bare"]).trim(),
        "true"
    );
    assert_eq!(
        git_ok(&sandbox, &root, &["config", "remote.harness.url"]).trim(),
        "/nonexistent/harness.git"
    );
    assert_eq!(
        hidden,
        set(&[
            "src/Main.java",
            ".idea/misc.xml",
            "CLAUDE.local.md",
            "docs/other.md",
            ".claude/settings.local.json",
            ".claude/worktrees/agent/x",
            ".claude/scheduled_tasks.lock",
            ".claude/hooks/__pycache__/a.pyc",
            "foo/bar",
        ])
    );
}

#[test]
fn init_sets_overrides() {
    let sandbox = Sandbox::new();
    let root = harness_root(&harnessed(&sandbox));
    let expected = [
        ("commit.gpgsign", "false"),
        ("hook.beams.enabled", "false"),
        ("hook.worktree-init.enabled", "false"),
        ("hook.harness.enabled", "false"),
        ("core.excludesFile", "/dev/null"),
    ];

    for (key, value) in expected {
        assert_eq!(
            git_ok(&sandbox, &root, &["config", "--get", key]).trim(),
            value
        );
    }
}

#[test]
fn enabled_is_false_without_harness() {
    let sandbox = Sandbox::new();
    let main = code(&sandbox);

    assert!(!harness::enabled(&main.join(".git")));
}

#[test]
fn harness_works_for_worktree_outside_parent() {
    let sandbox = Sandbox::new();
    let main = harnessed(&sandbox);
    let target = sandbox.root.join("elsewhere/with space/wt");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "-b", "feature/x", path(&target)],
    );
    fs::write(target.join("CLAUDE.md"), "x").unwrap();

    let status = hgit(&sandbox, &target, &["status", "--porcelain"]);

    assert_eq!(status, "?? CLAUDE.md\n");
}

#[test]
fn global_excludes_do_not_hide_harness_files() {
    let sandbox = Sandbox::new();
    let main = harnessed(&sandbox);
    let excludes = sandbox.root.join("global-excludes");
    fs::write(&excludes, ".claude\nCLAUDE.md\n").unwrap();
    git_ok(
        &sandbox,
        &main,
        &["config", "--global", "core.excludesFile", path(&excludes)],
    );

    assert_eq!(
        ignored(&sandbox, &main, &["CLAUDE.md", ".claude/handoffs/x.md"]),
        set(&[])
    );
}

#[test]
fn code_status_unaffected() {
    let sandbox = Sandbox::new();
    let main = harnessed(&sandbox);
    fs::write(main.join("CLAUDE.md"), "x").unwrap();
    hgit(&sandbox, &main, &["add", "CLAUDE.md"]);
    hgit(&sandbox, &main, &["commit", "-q", "-m", "Add instructions"]);

    assert_eq!(git_ok(&sandbox, &main, &["status", "--porcelain"]), "");
    assert_eq!(
        hgit(&sandbox, &main, &["log", "--format=%s"]),
        "Add instructions\n"
    );
}

#[test]
fn harness_hook_exits_without_harness() {
    let sandbox = Sandbox::new();
    let main = code(&sandbox);

    let result = run_harness_hook(&sandbox, &main, &[ZEROS, ONES, "1"], "", &[]);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(text(&result.stderr), "");
    assert!(!harness_root(&main).exists());
}

#[test]
fn harness_hook_exits_inside_harness_operation() {
    let sandbox = Sandbox::new();
    let main = harnessed(&sandbox);
    let root = harness_root(&main);

    let result = run_harness_hook(
        &sandbox,
        &main,
        &[ZEROS, ONES, "1"],
        "",
        &[("GIT_DIR", path(&root))],
    );

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(text(&result.stderr), "");
    assert_eq!(read(&root.join("HEAD")), "ref: refs/heads/develop\n");
}

#[test]
fn harness_hook_runs_with_harness() {
    let sandbox = Sandbox::new();
    let main = harnessed(&sandbox);

    let result = run_harness_hook(&sandbox, &main, &[ZEROS, ONES, "1"], "", &[]);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(text(&result.stderr), "");
    assert_eq!(
        read(&harness_root(&main).join("HEAD")),
        "ref: refs/heads/main\n"
    );
}

#[test]
fn pre_push_survives_missing_binary() {
    let sandbox = Sandbox::new();
    let (target, _) = pushing(&sandbox);
    let main = sandbox.root.join("proj");
    let command = r#"f="/nonexistent/git-wt"; [ -x "$f" ] || exit 0; exec "$f" harness-hook"#;
    git_ok(
        &sandbox,
        &main,
        &["config", "hook.harness.command", command],
    );

    let result = sandbox.git(&target, &["push", "-q", "origin", "feature/foo"]);

    assert!(result.status.success(), "{}", text(&result.stderr));
    assert!(!git_ok(&sandbox, &target, &["ls-remote", "origin", "feature/foo"]).is_empty());
}

#[test]
fn harness_hook_exits_outside_repository() {
    let sandbox = Sandbox::new();

    for args in [
        &["committed"][..],
        &["origin", "/r.git"],
        &[ZEROS, ONES, "1"],
    ] {
        let input = format!("{ZEROS} {ONES} refs/remotes/origin/develop\n");
        let result = run_harness_hook(&sandbox, &sandbox.root, args, &input, &[]);

        assert_eq!(result.status.code(), Some(0), "{args:?}");
        assert_eq!(text(&result.stderr), "", "{args:?}");
    }
}

#[test]
fn cli_init_creates_harness() {
    let sandbox = Sandbox::new();
    let main = code(&sandbox);

    let result = sandbox.wt(&main, &["harness", "init", "--remote", "/r.git"]);

    assert_eq!(result.status.code(), Some(0), "{}", text(&result.stderr));
    assert!(harness::enabled(&main.join(".git")));
    assert_eq!(
        text(&result.stderr),
        format!(
            "worktree-init: harness created in {}\n",
            harness_root(&main).display()
        )
    );
}

#[test]
fn cli_harness_without_subcommand_is_usage_error() {
    let sandbox = Sandbox::new();
    let main = code(&sandbox);

    let result = sandbox.wt(&main, &["harness"]);

    assert_eq!(result.status.code(), Some(129));
    assert_eq!(
        text(&result.stderr),
        format!("error: need the subcommand init, sync or merge\n{HARNESS_USAGE}")
    );
}

#[test]
fn cli_harness_usage_errors_match_python() {
    let sandbox = Sandbox::new();
    let main = code(&sandbox);
    let cases: [(&[&str], &str); 4] = [
        (
            &["harness", "init"],
            "the following arguments are required: --remote",
        ),
        (
            &["harness", "init", "--remote"],
            "argument --remote: expected one argument",
        ),
        (
            &["harness", "init", "--remote", "/r.git", "x"],
            "unrecognized arguments: x",
        ),
        (
            &["harness", "merge"],
            "need the subcommand init, sync or merge",
        ),
    ];

    for (args, message) in cases {
        let result = sandbox.wt(&main, args);

        assert_eq!(result.status.code(), Some(129), "{args:?}");
        assert_eq!(
            text(&result.stderr),
            format!("error: {message}\n{HARNESS_USAGE}")
        );
    }
    let help = sandbox.wt(&main, &["harness", "-h"]);
    assert_eq!(help.status.code(), Some(129));
    assert_eq!(text(&help.stdout), HARNESS_USAGE);
}

#[test]
fn reused_worktree_path_does_not_snapshot_stale_admin() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "-b", "feature/old", "../wt"],
    );
    let target = sandbox.root.join("wt");
    git_ok(
        &sandbox,
        &main,
        &["worktree", "remove", "--force", path(&target)],
    );
    let before = hgit(&sandbox, &main, &["rev-parse", "feature/old"]);

    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "-b", "feature/new", "../wt"],
    );

    assert_eq!(hgit(&sandbox, &main, &["rev-parse", "feature/old"]), before);
    assert_eq!(current(&sandbox, &target), "feature/new");
    assert_eq!(read(&target.join("CLAUDE.md")), "develop");
}

#[test]
fn branch_of_removed_worktree_attaches_elsewhere() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "-b", "feature/x", "../a"],
    );
    git_ok(
        &sandbox,
        &main,
        &[
            "worktree",
            "remove",
            "--force",
            path(&sandbox.root.join("a")),
        ],
    );

    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "../b", "feature/x"],
    );

    let target = sandbox.root.join("b");
    assert_eq!(current(&sandbox, &target), "feature/x");
    assert_eq!(read(&target.join("CLAUDE.md")), "develop");
}

#[test]
fn snapshot_refuses_when_every_harness_file_is_gone() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    let head = hgit(&sandbox, &main, &["rev-parse", "HEAD"]);
    fs::remove_file(main.join("CLAUDE.md")).unwrap();

    assert!(!harness::snapshot(&main, &env_of(&sandbox)).unwrap());
    assert_eq!(hgit(&sandbox, &main, &["rev-parse", "HEAD"]), head);
}

#[test]
fn snapshot_honours_harness_gitignore_and_skips_ds_store() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    let plugin = main.join(".claude/plugin");
    write(&plugin.join(".gitignore"), ".pytest_cache/\n");
    write(&plugin.join(".pytest_cache/x"), "cache");
    write(&main.join(".claude/.DS_Store"), "finder");

    assert!(harness::snapshot(&main, &env_of(&sandbox)).unwrap());

    let listing = hgit(&sandbox, &main, &["ls-files"]);
    let files: Vec<&str> = listing.lines().collect();
    assert!(files.contains(&".claude/plugin/.gitignore"));
    assert!(!files.contains(&".claude/plugin/.pytest_cache/x"));
    assert!(!files.contains(&".claude/.DS_Store"));
}

#[test]
fn new_branch_forks_from_t3_merge_base() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    harness_branch_with(
        &sandbox,
        &main,
        "release/x",
        ".claude/release.md",
        "release",
    );
    git_ok(&sandbox, &main, &["branch", "feature/a"]);
    git_ok(
        &sandbox,
        &main,
        &["config", "branch.feature/a.t3-merge-base", "release/x"],
    );

    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "../a", "feature/a"],
    );

    let target = sandbox.root.join("a");
    assert_eq!(current(&sandbox, &target), "feature/a");
    assert_eq!(read(&target.join(".claude/release.md")), "release");
    assert_eq!(read(&target.join("CLAUDE.md")), "develop");
}

#[test]
fn new_branch_forks_from_renamed_upstream() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    harness_branch_with(
        &sandbox,
        &main,
        "release/x",
        ".claude/release.md",
        "release",
    );
    git_ok(&sandbox, &main, &["branch", "feature/b"]);
    git_ok(
        &sandbox,
        &main,
        &["config", "branch.feature/b.remote", "origin"],
    );
    git_ok(
        &sandbox,
        &main,
        &["config", "branch.feature/b.merge", "refs/heads/release/x"],
    );

    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "../b", "feature/b"],
    );

    assert_eq!(read(&sandbox.root.join("b/.claude/release.md")), "release");
}

#[test]
fn branch_tracking_itself_forks_from_develop() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    harness_branch_with(&sandbox, &main, "feature/c", ".claude/other.md", "other");
    git_ok(&sandbox, &main, &["branch", "-q", "feature/c-code"]);
    git_ok(
        &sandbox,
        &main,
        &["config", "branch.feature/c-code.remote", "origin"],
    );
    git_ok(
        &sandbox,
        &main,
        &[
            "config",
            "branch.feature/c-code.merge",
            "refs/heads/feature/c-code",
        ],
    );

    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "../c", "feature/c-code"],
    );

    let target = sandbox.root.join("c");
    assert_eq!(current(&sandbox, &target), "feature/c-code");
    assert_eq!(read(&target.join("CLAUDE.md")), "develop");
    assert!(!target.join(".claude/other.md").exists());
}

#[test]
fn existing_remote_harness_branch_is_resumed() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    let remote = sandbox.root.join("remote.git");
    git_ok(
        &sandbox,
        &sandbox.root,
        &["init", "-q", "--bare", path(&remote)],
    );
    let root = harness_root(&main);
    git_ok(
        &sandbox,
        &root,
        &["remote", "set-url", "harness", path(&remote)],
    );
    harness_branch_with(
        &sandbox,
        &main,
        "feature/foo",
        ".claude/handoffs/a.md",
        "handoff",
    );
    git_ok(&sandbox, &root, &["push", "-q", "harness", "feature/foo"]);
    git_ok(&sandbox, &root, &["branch", "-q", "-D", "feature/foo"]);

    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "-b", "feature/foo", "../foo"],
    );

    assert_eq!(
        read(&sandbox.root.join("foo/.claude/handoffs/a.md")),
        "handoff"
    );
}

#[test]
fn switch_snapshots_dirty_harness_first() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "-b", "feature/a", "../a"],
    );
    let target = sandbox.root.join("a");
    write(&target.join(".claude/h.md"), "dirty");

    git_ok(&sandbox, &target, &["checkout", "-q", "-b", "feature/b"]);

    let log = hgit(&sandbox, &target, &["log", "--format=%s", "feature/a"]);
    assert_eq!(log.lines().next(), Some("Snapshot harness state"));
    assert_eq!(current(&sandbox, &target), "feature/b");
}

#[test]
fn new_worktree_does_not_snapshot_deletions() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "-b", "feature/a", "../a"],
    );

    let log = hgit(&sandbox, &main, &["log", "--format=%s", "develop"]);

    assert_eq!(log, "Seed\n");
}

fn unattached(sandbox: &Sandbox, main: &Path) -> PathBuf {
    git_ok(
        sandbox,
        main,
        &[
            "worktree",
            "add",
            "-q",
            "--no-checkout",
            "-b",
            "tmp/x",
            "../x",
        ],
    );
    let target = sandbox.root.join("x");
    git_ok(sandbox, &target, &["reset", "-q", "--hard"]);
    git_ok(sandbox, &target, &["branch", "-m", "feature/x"]);
    target
}

fn admin_head(main: &Path, name: &str) -> String {
    read(&harness_root(main).join("worktrees").join(name).join("HEAD"))
}

#[test]
fn new_admin_dir_holds_no_real_branch() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    let target = unattached(&sandbox, &main);

    assert!(!harness::snapshot(&target, &env_of(&sandbox)).unwrap());

    assert_eq!(admin_head(&main, "x"), "ref: refs/heads/wt-unattached/x\n");
}

#[test]
fn unattached_worktree_does_not_block_develop() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    let target = unattached(&sandbox, &main);
    harness::snapshot(&target, &env_of(&sandbox)).unwrap();
    git_ok(&sandbox, &main, &["branch", "develop"]);
    git_ok(&sandbox, &main, &["checkout", "-q", "-b", "feature/m"]);

    let result = sandbox.git(&main, &["checkout", "develop"]);

    assert!(
        !text(&result.stderr).contains("already used"),
        "{}",
        text(&result.stderr)
    );
    assert_eq!(current(&sandbox, &main), "develop");
}

#[test]
fn stale_admin_dir_on_develop_is_released() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    let target = unattached(&sandbox, &main);
    harness::snapshot(&target, &env_of(&sandbox)).unwrap();
    let admin = harness_root(&main).join("worktrees/x");
    fs::write(admin.join("HEAD"), "ref: refs/heads/develop\n").unwrap();
    git_ok(&sandbox, &main, &["branch", "develop"]);
    git_ok(&sandbox, &main, &["checkout", "-q", "-b", "feature/m"]);

    git_ok(&sandbox, &main, &["checkout", "-q", "develop"]);

    assert_eq!(current(&sandbox, &main), "develop");
    assert_eq!(admin_head(&main, "x"), "ref: refs/heads/wt-unattached/x\n");
}

#[test]
fn push_attaches_unattached_worktree_to_its_branch() {
    let sandbox = Sandbox::new();
    let (_, remote) = pushing(&sandbox);
    let main = sandbox.root.join("proj");
    let target = unattached(&sandbox, &main);

    let result = sandbox.git(&target, &["push", "-q", "origin", "feature/x"]);

    assert!(result.status.success(), "{}", text(&result.stderr));
    assert_eq!(current(&sandbox, &target), "feature/x");
    assert_eq!(read(&target.join("CLAUDE.md")), "develop");
    assert_eq!(
        git_ok(&sandbox, &remote, &["show", "feature/x:CLAUDE.md"]),
        "develop"
    );
}

#[test]
fn late_attach_keeps_existing_files() {
    let sandbox = Sandbox::new();
    let (_, remote) = pushing(&sandbox);
    let main = sandbox.root.join("proj");
    let target = unattached(&sandbox, &main);
    write(&target.join("CLAUDE.md"), "local");
    write(&target.join(".claude/handoffs/h.md"), "handoff");

    let result = sandbox.git(&target, &["push", "-q", "origin", "feature/x"]);

    assert!(result.status.success(), "{}", text(&result.stderr));
    assert_eq!(read(&target.join("CLAUDE.md")), "local");
    assert_eq!(
        git_ok(
            &sandbox,
            &remote,
            &["show", "feature/x:.claude/handoffs/h.md"]
        ),
        "handoff"
    );
    assert_eq!(
        hgit(&sandbox, &main, &["show", "develop:CLAUDE.md"]),
        "develop"
    );
}

#[test]
fn unfinished_late_attach_resumes_before_snapshot() {
    let sandbox = Sandbox::new();
    pushing(&sandbox);
    let main = sandbox.root.join("proj");
    let target = unattached(&sandbox, &main);
    let env = env_of(&sandbox);
    assert!(harness::attach_late(&target, &env).unwrap());
    let head = hgit(&sandbox, &target, &["rev-parse", "HEAD"]);
    let marker = harness_root(&main)
        .join("worktrees/x")
        .join(harness::ATTACH_PENDING);
    fs::write(&marker, "").unwrap();
    fs::remove_file(target.join("CLAUDE.md")).unwrap();
    write(&target.join(".claude/h.md"), "handoff");

    assert!(!harness::snapshot(&target, &env).unwrap());
    assert!(harness::attach_late(&target, &env).unwrap());

    assert_eq!(read(&target.join("CLAUDE.md")), "develop");
    assert!(!marker.exists());
    assert_eq!(hgit(&sandbox, &target, &["rev-parse", "HEAD"]), head);
}

#[test]
fn pending_attach_without_index_resets_before_resume() {
    let sandbox = Sandbox::new();
    pushing(&sandbox);
    let main = sandbox.root.join("proj");
    let target = unattached(&sandbox, &main);
    let env = env_of(&sandbox);
    assert!(harness::attach_late(&target, &env).unwrap());
    let admin = harness_root(&main).join("worktrees/x");
    fs::remove_file(admin.join("index")).unwrap();
    fs::write(admin.join(harness::ATTACH_PENDING), "").unwrap();
    write(&target.join("CLAUDE.md"), "local");

    assert!(harness::attach_late(&target, &env).unwrap());
    assert!(harness::snapshot(&target, &env).unwrap());

    assert!(admin.join("index").exists());
    assert!(!admin.join(harness::ATTACH_PENDING).exists());
    assert_eq!(
        hgit(&sandbox, &target, &["show", "HEAD:CLAUDE.md"]),
        "local"
    );
}

#[test]
fn switch_resumes_pending_attach_before_snapshot() {
    let sandbox = Sandbox::new();
    pushing(&sandbox);
    let main = sandbox.root.join("proj");
    let target = unattached(&sandbox, &main);
    let env = env_of(&sandbox);
    assert!(harness::attach_late(&target, &env).unwrap());
    let admin = harness_root(&main).join("worktrees/x");
    fs::write(admin.join(harness::ATTACH_PENDING), "").unwrap();
    fs::remove_file(target.join("CLAUDE.md")).unwrap();
    write(&target.join(".claude/h.md"), "handoff");

    git_ok(&sandbox, &target, &["checkout", "-q", "-b", "feature/y"]);

    assert_eq!(current(&sandbox, &target), "feature/y");
    assert!(!admin.join(harness::ATTACH_PENDING).exists());
    let tree = hgit(
        &sandbox,
        &target,
        &["ls-tree", "-r", "--name-only", "feature/x"],
    );
    assert!(tree.lines().any(|path| path == "CLAUDE.md"));
    assert!(tree.lines().any(|path| path == ".claude/h.md"));
}

#[test]
fn release_keeps_branch_of_pending_attach() {
    let sandbox = Sandbox::new();
    pushing(&sandbox);
    let main = sandbox.root.join("proj");
    let target = unattached(&sandbox, &main);
    let env = env_of(&sandbox);
    assert!(harness::attach_late(&target, &env).unwrap());
    let admin = harness_root(&main).join("worktrees/x");
    fs::remove_file(admin.join("index")).unwrap();
    fs::write(admin.join(harness::ATTACH_PENDING), "").unwrap();

    git_ok(&sandbox, &main, &["checkout", "-q", "-b", "feature/m"]);

    assert_eq!(admin_head(&main, "x"), "ref: refs/heads/feature/x\n");
    assert!(harness::attach_late(&target, &env).unwrap());
    assert_eq!(current(&sandbox, &target), "feature/x");
    assert_eq!(read(&target.join("CLAUDE.md")), "develop");
}

#[test]
fn late_attach_restores_paths_literally() {
    let sandbox = Sandbox::new();
    pushing(&sandbox);
    let main = sandbox.root.join("proj");
    write(&main.join(".claude/a.md"), "develop");
    write(&main.join(".claude/[a].md"), "glob");
    hgit(&sandbox, &main, &["add", ".claude/a.md", ".claude/[a].md"]);
    hgit(&sandbox, &main, &["commit", "-q", "-m", "Add glob name"]);
    let target = unattached(&sandbox, &main);
    write(&target.join(".claude/a.md"), "local");

    let result = sandbox.git(&target, &["push", "-q", "origin", "feature/x"]);

    assert!(result.status.success(), "{}", text(&result.stderr));
    assert_eq!(read(&target.join(".claude/a.md")), "local");
    assert_eq!(read(&target.join(".claude/[a].md")), "glob");
}

#[test]
fn push_from_detached_unattached_worktree_skips_harness() {
    let sandbox = Sandbox::new();
    let (_, remote) = pushing(&sandbox);
    let main = sandbox.root.join("proj");
    let target = unattached(&sandbox, &main);
    git_ok(&sandbox, &target, &["checkout", "-q", "--detach"]);

    let result = sandbox.git(
        &target,
        &["push", "-q", "origin", "HEAD:refs/heads/feature/x"],
    );

    assert!(result.status.success(), "{}", text(&result.stderr));
    assert!(
        !text(&result.stderr).contains("harness"),
        "{}",
        text(&result.stderr)
    );
    assert!(
        git_ok(&sandbox, &remote, &["branch", "--list"])
            .lines()
            .all(|b| !b.contains("wt-unattached"))
    );
}

#[test]
fn rebase_with_autostash_leaves_harness_untouched() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    git_ok(
        &sandbox,
        &main,
        &["worktree", "add", "-q", "-b", "feature/a", "../a"],
    );
    let target = sandbox.root.join("a");
    fs::write(target.join("f.txt"), "f").unwrap();
    git_ok(&sandbox, &target, &["add", "f.txt"]);
    git_ok(&sandbox, &target, &["commit", "-q", "-m", "Feature"]);
    fs::write(main.join("m.txt"), "m").unwrap();
    git_ok(&sandbox, &main, &["add", "m.txt"]);
    git_ok(&sandbox, &main, &["commit", "-q", "-m", "Main"]);
    fs::write(target.join(".gitignore"), "dirty\n").unwrap();
    fs::write(target.join("CLAUDE.md"), "dirty harness").unwrap();
    let head = hgit(&sandbox, &target, &["rev-parse", "HEAD"]);
    let index = hgit(&sandbox, &target, &["ls-files", "--stage"]);
    let status = hgit(&sandbox, &target, &["status", "--porcelain"]);

    let result = sandbox.git(&target, &["-c", "rebase.autostash=true", "rebase", "main"]);

    assert!(result.status.success(), "{}", text(&result.stderr));
    assert_eq!(hgit(&sandbox, &target, &["rev-parse", "HEAD"]), head);
    assert_eq!(hgit(&sandbox, &target, &["ls-files", "--stage"]), index);
    assert_eq!(hgit(&sandbox, &target, &["status", "--porcelain"]), status);
}

#[test]
fn harness_network_calls_never_prompt() {
    let sandbox = Sandbox::new();
    let main = seeded(&sandbox);
    let (bin, log) = fake_ssh(&sandbox);
    let root = harness_root(&main);
    git_ok(
        &sandbox,
        &root,
        &[
            "remote",
            "set-url",
            "harness",
            "ssh://example.invalid/h.git",
        ],
    );
    let mut env = env_of(&sandbox);
    env.remove("GIT_SSH_COMMAND");
    env.insert("PATH".into(), search_path(&bin));

    assert!(!harness::push(&main, &env).unwrap());

    assert!(read(&log).contains("BatchMode=yes"));
    assert!(read(&log).contains("ConnectTimeout="));
}

#[test]
fn pre_push_pushes_harness_branch() {
    let sandbox = Sandbox::new();
    let (target, remote) = pushing(&sandbox);

    let result = sandbox.git(&target, &["push", "-q", "origin", "feature/foo"]);

    assert!(result.status.success(), "{}", text(&result.stderr));
    assert_eq!(
        git_ok(&sandbox, &remote, &["show", "feature/foo:.claude/h.md"]),
        "handoff"
    );
}

#[test]
fn pre_push_survives_unreachable_harness() {
    let sandbox = Sandbox::new();
    let (target, _) = pushing(&sandbox);
    let root = harness_root(&sandbox.root.join("proj"));
    git_ok(
        &sandbox,
        &root,
        &["remote", "set-url", "harness", "/nonexistent"],
    );

    let result = sandbox.git(&target, &["push", "-q", "origin", "feature/foo"]);

    assert!(result.status.success());
    assert!(text(&result.stderr).contains("harness push failed"));
    assert!(!git_ok(&sandbox, &target, &["ls-remote", "origin", "feature/foo"]).is_empty());
}

#[test]
fn harness_hook_never_fails_push() {
    let sandbox = Sandbox::new();
    let (target, _) = pushing(&sandbox);
    let (bin, _) = fake_ssh(&sandbox);
    let root = harness_root(&sandbox.root.join("proj"));
    git_ok(
        &sandbox,
        &root,
        &[
            "remote",
            "set-url",
            "harness",
            "ssh://example.invalid/h.git",
        ],
    );

    let result = sandbox
        .command("git", &target)
        .args(["push", "-q", "origin", "feature/foo"])
        .env("PATH", search_path(&bin))
        .env_remove("GIT_SSH_COMMAND")
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(0), "{}", text(&result.stderr));
    assert!(text(&result.stderr).contains("worktree-init: harness push failed"));
    assert!(!git_ok(&sandbox, &target, &["ls-remote", "origin", "feature/foo"]).is_empty());
}

#[test]
fn harness_hook_survives_closed_stderr() {
    let sandbox = Sandbox::new();
    let (target, _) = pushing(&sandbox);
    let root = harness_root(&sandbox.root.join("proj"));
    git_ok(
        &sandbox,
        &root,
        &["remote", "set-url", "harness", "/nonexistent"],
    );
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);

    let status = sandbox
        .wt_command(&target)
        .args(["harness-hook", "origin", "/x"])
        .stdin(Stdio::null())
        .stderr(writer)
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(0));
}

#[test]
fn sync_exits_one_on_failure() {
    let sandbox = Sandbox::new();
    let (target, _) = pushing(&sandbox);
    let root = harness_root(&sandbox.root.join("proj"));
    git_ok(
        &sandbox,
        &root,
        &["remote", "set-url", "harness", "/nonexistent"],
    );

    let result = sandbox.wt(&target, &["harness", "sync"]);

    assert_eq!(result.status.code(), Some(1));
    assert!(text(&result.stderr).contains("harness push failed"));
}

#[test]
fn sync_pushes_snapshot() {
    let sandbox = Sandbox::new();
    let (target, remote) = pushing(&sandbox);

    let result = sandbox.wt(&target, &["harness", "sync"]);

    assert_eq!(result.status.code(), Some(0), "{}", text(&result.stderr));
    assert_eq!(
        git_ok(&sandbox, &remote, &["show", "feature/foo:.claude/h.md"]),
        "handoff"
    );
}

#[test]
fn push_never_forces() {
    let sandbox = Sandbox::new();
    let (target, remote) = pushing(&sandbox);
    let env = env_of(&sandbox);
    assert!(harness::snapshot(&target, &env).unwrap());
    assert!(harness::push(&target, &env).unwrap());
    let other = sandbox.root.join("other");
    git_ok(
        &sandbox,
        &sandbox.root,
        &[
            "clone",
            "-q",
            "-b",
            "feature/foo",
            path(&remote),
            path(&other),
        ],
    );
    fs::write(other.join("x.md"), "remote").unwrap();
    git_ok(&sandbox, &other, &["add", "x.md"]);
    git_ok(&sandbox, &other, &["commit", "-q", "-m", "Remote change"]);
    git_ok(&sandbox, &other, &["push", "-q", "origin", "feature/foo"]);
    let advanced = git_ok(&sandbox, &remote, &["rev-parse", "feature/foo"]);
    fs::write(target.join(".claude/h.md"), "local").unwrap();
    harness::snapshot(&target, &env).unwrap();

    assert!(!harness::push(&target, &env).unwrap());
    assert_eq!(
        git_ok(&sandbox, &remote, &["rev-parse", "feature/foo"]),
        advanced
    );
}

#[test]
fn missing_binary_never_blocks_git() {
    let sandbox = Sandbox::new();
    let main = harnessed(&sandbox);
    let events = ["post-checkout", "pre-push", "reference-transaction"];
    register_harness_hook(&sandbox, &main, "/nonexistent/git-wt", &events);

    let checkout = sandbox.git(&main, &["checkout", "-q", "-b", "x"]);
    let commit = sandbox.git(&main, &["commit", "-q", "--allow-empty", "-m", "Empty"]);

    assert!(checkout.status.success(), "{}", text(&checkout.stderr));
    assert!(commit.status.success(), "{}", text(&commit.stderr));
    assert_eq!(text(&checkout.stderr) + &text(&commit.stderr), "");
}

fn submitting(sandbox: &Sandbox) -> (PathBuf, PathBuf) {
    let main = seeded(sandbox);
    let origin = sandbox.root.join("origin.git");
    git_ok(
        sandbox,
        &sandbox.root,
        &["init", "-q", "--bare", path(&origin)],
    );
    git_ok(sandbox, &main, &["remote", "add", "origin", path(&origin)]);
    git_ok(sandbox, &main, &["push", "-q", "origin", "main:develop"]);
    git_ok(sandbox, &main, &["fetch", "-q", "origin"]);
    add_events(sandbox, &main, &["pre-push", "reference-transaction"]);
    write(&main.join(".superpowers/sdd/other/progress.md"), "other");
    hgit(sandbox, &main, &["add", "-f", ".superpowers"]);
    hgit(sandbox, &main, &["commit", "-q", "-m", "Add other ledger"]);
    (main, origin)
}

fn feature(sandbox: &Sandbox, main: &Path, branch: &str, own_commit: bool) -> PathBuf {
    let dashed = branch.replace('/', "-");
    let target = sandbox.root.join(&dashed);
    git_ok(
        sandbox,
        main,
        &["worktree", "add", "-q", "-b", branch, path(&target)],
    );
    if own_commit {
        fs::write(target.join(format!("{dashed}.txt")), branch).unwrap();
        git_ok(sandbox, &target, &["add", "."]);
        git_ok(
            sandbox,
            &target,
            &["commit", "-q", "-m", &format!("Work on {branch}")],
        );
    }
    let name = branch.rsplit('/').next().unwrap();
    write(
        &target.join(format!(".superpowers/sdd/2026-11-10-{name}/progress.md")),
        "ledger",
    );
    write(
        &target.join(format!(".claude/handoffs/2026-11-10-{name}.md")),
        "handoff",
    );
    write(&target.join(format!(".claude/notes-{name}.md")), "feature");
    assert!(harness::snapshot(&target, &env_of(sandbox)).unwrap());
    target
}

fn submit(sandbox: &Sandbox, main: &Path, origin: &Path, branch: &str) {
    submit_to(sandbox, main, origin, branch, "develop");
}

fn submit_to(sandbox: &Sandbox, main: &Path, origin: &Path, branch: &str, base: &str) {
    let clone = sandbox
        .root
        .join(format!("submit-{}", branch.replace('/', "-")));
    git_ok(
        sandbox,
        &sandbox.root,
        &["clone", "-q", "-b", base, path(origin), path(&clone)],
    );
    git_ok(
        sandbox,
        &clone,
        &["fetch", "-q", path(main), &format!("{branch}:{branch}")],
    );
    git_ok(
        sandbox,
        &clone,
        &[
            "merge",
            "-q",
            "--no-ff",
            "-m",
            &format!("Merge {branch}"),
            branch,
        ],
    );
    git_ok(sandbox, &clone, &["push", "-q", "origin", base]);
}

fn advance(sandbox: &Sandbox, origin: &Path, name: &str) {
    let clone = sandbox.root.join(format!("advance-{name}"));
    git_ok(
        sandbox,
        &sandbox.root,
        &["clone", "-q", "-b", "develop", path(origin), path(&clone)],
    );
    fs::write(clone.join(format!("{name}.txt")), name).unwrap();
    git_ok(sandbox, &clone, &["add", "."]);
    git_ok(sandbox, &clone, &["commit", "-q", "-m", name]);
    git_ok(sandbox, &clone, &["push", "-q", "origin", "develop"]);
}

fn remove(sandbox: &Sandbox, main: &Path, target: &Path) {
    git_ok(
        sandbox,
        main,
        &["worktree", "remove", "--force", path(target)],
    );
}

fn develop_files(sandbox: &Sandbox, main: &Path) -> BTreeSet<String> {
    hgit(sandbox, main, &["ls-tree", "-r", "--name-only", "develop"])
        .lines()
        .map(str::to_string)
        .collect()
}

fn merges(sandbox: &Sandbox, main: &Path) -> Vec<String> {
    merges_into(sandbox, main, "develop")
}

fn merges_into(sandbox: &Sandbox, main: &Path, base: &str) -> Vec<String> {
    hgit(sandbox, main, &["rev-list", "--merges", base])
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn commit_conflicting_note(sandbox: &Sandbox, main: &Path) {
    write(&main.join(".claude/notes-foo.md"), "develop");
    hgit(sandbox, main, &["add", ".claude/notes-foo.md"]);
    hgit(sandbox, main, &["commit", "-q", "-m", "Conflicting note"]);
}

#[test]
fn merge_keeps_existing_ledger_directory() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "feature/foo", true);
    write(&target.join(".superpowers/sdd/other/extra.md"), "extra");
    assert!(harness::snapshot(&target, &env_of(&sandbox)).unwrap());
    remove(&sandbox, &main, &target);
    submit(&sandbox, &main, &origin, "feature/foo");

    git_ok(&sandbox, &main, &["fetch", "-q", "origin"]);

    let files = develop_files(&sandbox, &main);
    assert!(files.contains(".superpowers/sdd/other/progress.md"));
    assert!(!files.contains(".superpowers/sdd/other/extra.md"));
}

#[test]
fn submitted_feature_is_merged_no_ff() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "feature/foo", true);
    remove(&sandbox, &main, &target);
    submit(&sandbox, &main, &origin, "feature/foo");

    let result = sandbox.git(&main, &["fetch", "-q", "origin"]);

    assert!(result.status.success(), "{}", text(&result.stderr));
    assert_eq!(merges(&sandbox, &main).len(), 1);
    let parents = hgit(&sandbox, &main, &["rev-list", "--parents", "-1", "develop"]);
    assert_eq!(parents.split_whitespace().count(), 3);
    assert!(develop_files(&sandbox, &main).contains(".claude/notes-foo.md"));
    assert_eq!(read(&main.join(".claude/notes-foo.md")), "feature");
}

#[test]
fn merge_removes_feature_ledger_and_handoffs() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "feature/foo", true);
    remove(&sandbox, &main, &target);
    submit(&sandbox, &main, &origin, "feature/foo");

    git_ok(&sandbox, &main, &["fetch", "-q", "origin"]);

    let files = develop_files(&sandbox, &main);
    assert!(!files.contains(".superpowers/sdd/2026-11-10-foo/progress.md"));
    assert!(!files.contains(".claude/handoffs/2026-11-10-foo.md"));
    assert!(files.contains(".superpowers/sdd/other/progress.md"));
    assert!(!main.join(".superpowers/sdd/2026-11-10-foo").exists());
}

#[test]
fn checked_out_branch_is_not_merged() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    feature(&sandbox, &main, "feature/foo", true);
    submit(&sandbox, &main, &origin, "feature/foo");

    git_ok(&sandbox, &main, &["fetch", "-q", "origin"]);

    assert!(merges(&sandbox, &main).is_empty());
}

#[test]
fn branch_without_own_commits_is_not_merged() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "feature/empty", false);
    remove(&sandbox, &main, &target);
    advance(&sandbox, &origin, "later");

    git_ok(&sandbox, &main, &["fetch", "-q", "origin"]);

    assert!(merges(&sandbox, &main).is_empty());
}

#[test]
fn merge_is_not_repeated() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "feature/foo", true);
    remove(&sandbox, &main, &target);
    submit(&sandbox, &main, &origin, "feature/foo");
    git_ok(&sandbox, &main, &["fetch", "-q", "origin"]);
    advance(&sandbox, &origin, "later");

    git_ok(&sandbox, &main, &["fetch", "-q", "origin"]);

    assert_eq!(merges(&sandbox, &main).len(), 1);
}

#[test]
fn merge_waits_until_worktree_is_gone() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "feature/foo", true);
    submit(&sandbox, &main, &origin, "feature/foo");
    git_ok(&sandbox, &main, &["fetch", "-q", "origin"]);
    git_ok(
        &sandbox,
        &main,
        &["branch", "-q", "-f", "develop", "origin/develop"],
    );
    remove(&sandbox, &main, &target);
    advance(&sandbox, &origin, "later");

    git_ok(&sandbox, &main, &["fetch", "-q", "origin"]);

    assert_eq!(merges(&sandbox, &main).len(), 1);
}

#[test]
fn open_conflict_blocks_second_merge() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "feature/foo", true);
    remove(&sandbox, &main, &target);
    commit_conflicting_note(&sandbox, &main);
    submit(&sandbox, &main, &origin, "feature/foo");
    let first = sandbox.git(&main, &["fetch", "-q", "origin"]);
    assert!(
        text(&first.stderr).contains("conflict"),
        "{}",
        text(&first.stderr)
    );
    let target = feature(&sandbox, &main, "feature/bar", true);
    remove(&sandbox, &main, &target);
    submit(&sandbox, &main, &origin, "feature/bar");

    let second = sandbox.git(&main, &["fetch", "-q", "origin"]);

    assert!(merges(&sandbox, &main).is_empty());
    assert!(text(&second.stderr).contains("still open"));
    assert!(harness_root(&main).join("MERGE_HEAD").is_file());
}

#[test]
fn snapshot_refuses_during_open_merge() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "feature/foo", true);
    remove(&sandbox, &main, &target);
    commit_conflicting_note(&sandbox, &main);
    submit(&sandbox, &main, &origin, "feature/foo");
    git_ok(&sandbox, &main, &["fetch", "-q", "origin"]);
    let head = hgit(&sandbox, &main, &["rev-parse", "HEAD"]);

    let result = sandbox.git(&main, &["push", "-q", "origin", "main:refs/heads/side"]);

    assert!(result.status.success());
    assert!(text(&result.stderr).contains("merge is still open"));
    assert_eq!(hgit(&sandbox, &main, &["rev-parse", "HEAD"]), head);
}

#[test]
fn manual_merge_ignores_ancestry() {
    let sandbox = Sandbox::new();
    let (main, _) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "fix/x", true);
    remove(&sandbox, &main, &target);

    let result = sandbox.wt(&main, &["harness", "merge", "fix/x"]);

    assert_eq!(result.status.code(), Some(0), "{}", text(&result.stderr));
    assert_eq!(
        text(&result.stderr),
        "worktree-init: merged harness branch fix/x into develop\n"
    );
    assert_eq!(merges(&sandbox, &main).len(), 1);
    assert!(!develop_files(&sandbox, &main).contains(".claude/handoffs/2026-11-10-x.md"));
}

#[test]
fn harness_hook_ignores_unrelated_reference_transactions() {
    let sandbox = Sandbox::new();
    let (main, origin) = submitting(&sandbox);
    let target = feature(&sandbox, &main, "feature/foo", true);
    remove(&sandbox, &main, &target);
    submit(&sandbox, &main, &origin, "feature/foo");
    git_ok(
        &sandbox,
        &main,
        &["-c", "hook.harness.enabled=false", "fetch", "-q", "origin"],
    );
    let line = |reference: &str| format!("{ZEROS} {ONES} {reference}\n");

    for (state, reference) in [
        ("prepared", "refs/remotes/origin/develop"),
        ("committed", "refs/heads/main"),
        ("committed", "refs/remotes/origin/feature/foo"),
    ] {
        run_harness_hook(&sandbox, &main, &[state], &line(reference), &[]);
        assert!(merges(&sandbox, &main).is_empty(), "{state} {reference}");
    }
    let input = line("refs/heads/main") + &line("refs/remotes/origin/develop");
    let result = run_harness_hook(&sandbox, &main, &["committed"], &input, &[]);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(merges(&sandbox, &main).len(), 1);
}

fn on_default_branch(sandbox: &Sandbox, default: &str, configured: bool) -> (PathBuf, PathBuf) {
    let main = sandbox.repo();
    if default == "master" {
        git_ok(sandbox, &main, &["branch", "-m", "master"]);
    }
    if configured {
        git_ok(
            sandbox,
            &main,
            &["config", harness::DEFAULT_BRANCH_KEY, default],
        );
    }
    let origin = sandbox.root.join("origin.git");
    let remote = sandbox.root.join("harness-remote.git");
    for bare in [&origin, &remote] {
        git_ok(
            sandbox,
            &sandbox.root,
            &["init", "-q", "--bare", path(bare)],
        );
    }
    git_ok(sandbox, &main, &["remote", "add", "origin", path(&origin)]);
    git_ok(
        sandbox,
        &main,
        &["push", "-q", "origin", &format!("HEAD:{default}")],
    );
    git_ok(sandbox, &main, &["fetch", "-q", "origin"]);
    let env = env_of(sandbox);
    harness::init(&main, path(&remote), &env).unwrap();
    assert_eq!(harness::default_branch(&main, &env).unwrap(), default);
    write(&main.join("CLAUDE.md"), default);
    hgit(sandbox, &main, &["add", "-f", "CLAUDE.md"]);
    hgit(sandbox, &main, &["commit", "-q", "-m", "Seed"]);
    let events = ["post-checkout", "pre-push", "reference-transaction"];
    register_harness_hook(sandbox, &main, wt_bin(), &events);
    (main, origin)
}

fn late_attach_and_merge_on(default: &str, configured: bool) {
    let sandbox = Sandbox::new();
    let (main, origin) = on_default_branch(&sandbox, default, configured);
    let target = unattached(&sandbox, &main);
    fs::write(target.join("x.txt"), "x").unwrap();
    git_ok(&sandbox, &target, &["add", "x.txt"]);
    git_ok(&sandbox, &target, &["commit", "-q", "-m", "Work on x"]);
    write(&target.join(".claude/notes-x.md"), "feature");

    let pushed = sandbox.git(&target, &["push", "-q", "origin", "feature/x"]);

    assert!(pushed.status.success(), "{}", text(&pushed.stderr));
    let remote = sandbox.root.join("harness-remote.git");
    assert_eq!(
        git_ok(&sandbox, &remote, &["show", "feature/x:CLAUDE.md"]),
        default
    );
    assert_eq!(
        git_ok(&sandbox, &remote, &["show", "feature/x:.claude/notes-x.md"]),
        "feature"
    );
    remove(&sandbox, &main, &target);
    submit_to(&sandbox, &main, &origin, "feature/x", default);

    let fetched = sandbox.git(&main, &["fetch", "-q", "origin"]);

    assert!(fetched.status.success(), "{}", text(&fetched.stderr));
    assert_eq!(merges_into(&sandbox, &main, default).len(), 1);
    assert_eq!(read(&main.join(".claude/notes-x.md")), "feature");
}

#[test]
fn main_repository_needs_no_configuration() {
    late_attach_and_merge_on("main", false);
}

#[test]
fn master_repository_needs_no_configuration() {
    late_attach_and_merge_on("master", false);
}

#[test]
fn develop_repository_uses_configured_default_branch() {
    late_attach_and_merge_on("develop", true);
}

const README: &str = include_str!("../README.md");

fn readme_registrations() -> Vec<&'static str> {
    README
        .lines()
        .filter(|line| line.starts_with("git config --global") && line.contains(".command "))
        .collect()
}

fn readme_install_lines() -> Vec<&'static str> {
    let install = &README[README.find("## Install").unwrap()..README.find("### Nix").unwrap()];
    install
        .lines()
        .filter(|line| line.starts_with("git config --global") && line.contains(" hook."))
        .collect()
}

fn install_from_readme(sandbox: &Sandbox, binary: &str) {
    for line in readme_install_lines() {
        let command = line.replace("<bin>", binary);
        let output = sandbox
            .command("sh", &sandbox.root)
            .args(["-c", &command])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{command}: {}",
            text(&output.stderr)
        );
    }
}

#[test]
fn readme_registrations_guard_missing_binary() {
    let registrations = readme_registrations();
    let guard = r#"'f="<bin>"; [ -x "$f" ] || exit 0; exec "$f" "#;

    assert!(registrations.len() >= 2, "{registrations:?}");
    for line in &registrations {
        let hook = line.ends_with(&format!("{guard}hook'"));
        let harness_hook = line.ends_with(&format!("{guard}harness-hook'"));
        assert!(hook || harness_hook, "{line}");
    }
    assert_eq!(readme_install_lines().len(), 6);
}

#[test]
fn readme_install_with_missing_binary_never_blocks_git() {
    let sandbox = Sandbox::new();
    let main = harnessed(&sandbox);
    fs::write(main.join(".worktreeinclude"), ".env\n").unwrap();
    install_from_readme(&sandbox, "/nonexistent/git-wt");

    let add = sandbox.git(&main, &["worktree", "add", "-q", "-b", "w", "../w"]);
    let commit = sandbox.git(&main, &["commit", "-q", "--allow-empty", "-m", "Empty"]);

    assert!(add.status.success(), "{}", text(&add.stderr));
    assert!(commit.status.success(), "{}", text(&commit.stderr));
    assert_eq!(text(&add.stderr) + &text(&commit.stderr), "");
}

#[test]
fn readme_install_runs_binary() {
    let sandbox = Sandbox::new();
    let main = harnessed(&sandbox);
    fs::write(main.join(".worktreeinclude"), ".env\n").unwrap();
    fs::write(main.join(".env"), "SECRET=1\n").unwrap();
    install_from_readme(&sandbox, wt_bin());

    let add = sandbox.git(&main, &["worktree", "add", "-q", "-b", "w", "../w"]);

    assert!(add.status.success(), "{}", text(&add.stderr));
    assert_eq!(read(&sandbox.root.join("w/.env")), "SECRET=1\n");
    assert!(harness_root(&main).join("worktrees/w/HEAD").is_file());
}
