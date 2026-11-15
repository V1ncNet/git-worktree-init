mod common;

use std::fs;
use std::path::Path;

use common::Sandbox;
use git_wt::config::{self, Entry};
use git_wt::gitenv::{self, Env};
use git_wt::glob;
use git_wt::worktrees;

const LEAKING: [&str; 3] = ["GIT_CONFIG_PARAMETERS", "COMPOSE_PROJECT_NAME", "CI"];

fn env_of(sandbox: &Sandbox) -> Env {
    let extra: Vec<String> = LEAKING.iter().map(|key| key.to_string()).collect();
    let mut env = gitenv::sanitized(&gitenv::process_env(), &extra);
    env.extend(sandbox.env());
    env
}

fn env_map(pairs: &[(&str, &str)]) -> Env {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn git_ok(sandbox: &Sandbox, cwd: &Path, args: &[&str]) {
    let output = sandbox.git(cwd, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn sanitized_drops_git_variables_and_extras() {
    let env = env_map(&[
        ("GIT_DIR", "x"),
        ("GIT_WORK_TREE", "x"),
        ("GIT_INDEX_FILE", "x"),
        ("GIT_PREFIX", "sub/"),
        ("COMPOSE_PROJECT_NAME", "leak"),
        ("GIT_CONFIG_PARAMETERS", "'a.b'='c'"),
        ("PATH", "/bin"),
    ]);

    let result = gitenv::sanitized(&env, &["COMPOSE_PROJECT_NAME".to_string()]);

    assert_eq!(
        result,
        env_map(&[("GIT_CONFIG_PARAMETERS", "'a.b'='c'"), ("PATH", "/bin")])
    );
}

#[test]
fn git_returns_stdout() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();

    let branch = gitenv::git(
        &repo,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        &env_of(&sandbox),
    )
    .unwrap();

    assert_eq!(branch.trim(), "main");
}

#[test]
fn git_fails_on_non_zero_exit() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();

    let error = gitenv::git(&repo, &["rev-parse", "no-such-ref"], &env_of(&sandbox)).unwrap_err();

    assert!(matches!(
        error,
        git_wt::error::Error::Git {
            code: Some(128),
            ..
        }
    ));
}

#[test]
fn git_status_reports_code_without_failing() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();

    let (code, _, stderr) =
        gitenv::git_status(&repo, &["rev-parse", "no-such-ref"], &env_of(&sandbox)).unwrap();

    assert_eq!(code, 128);
    assert!(!stderr.is_empty());
}

#[test]
fn run_passes_input() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();

    let output = gitenv::run(&["cat"], &repo, &env_of(&sandbox), Some(b"hello")).unwrap();

    assert_eq!(output.stdout, b"hello");
}

#[test]
fn run_uses_only_the_given_environment() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let env = env_map(&[("PATH", "/usr/bin:/bin"), ("ONLY", "this")]);

    let output = gitenv::run(&["env"], &repo, &env, None).unwrap();
    let printed = String::from_utf8(output.stdout).unwrap();

    assert!(printed.lines().any(|line| line == "ONLY=this"));
    assert!(!printed.contains("HOME="));
}

#[test]
fn read_returns_scoped_lowercase_entries() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    git_ok(
        &sandbox,
        &repo,
        &["config", "worktree-init.reservedSlots", "0 2"],
    );
    git_ok(
        &sandbox,
        &repo,
        &["config", "--add", "worktree-init.ignorePaths", "*/a/*"],
    );
    git_ok(
        &sandbox,
        &repo,
        &["config", "--add", "worktree-init.ignorePaths", "*/b/*"],
    );
    let mut env = env_of(&sandbox);
    env.insert(
        "GIT_CONFIG_PARAMETERS".into(),
        "'worktree-init.clone-db'='x'".into(),
    );

    let entries = config::read(&repo, &env).unwrap();

    assert_eq!(config::last(&entries, "reservedSlots"), Some("0 2"));
    assert_eq!(
        config::all_values(&entries, "ignorePaths"),
        vec!["*/a/*", "*/b/*"]
    );
    assert_eq!(config::last(&entries, "clone-db"), Some("x"));
    assert_eq!(
        config::command_line_keys(&entries),
        ["clone-db".to_string()].into()
    );
}

#[test]
fn read_without_entries_is_empty() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();

    assert!(config::read(&repo, &env_of(&sandbox)).unwrap().is_empty());
}

#[test]
fn ignore_patterns_default_to_claude_worktrees() {
    assert_eq!(
        config::ignore_patterns(&Default::default()),
        vec!["*/.claude/worktrees/*"]
    );
}

#[test]
fn path_ignored_matches_directory_with_spaces() {
    let patterns = config::ignore_patterns(&Default::default());

    assert!(config::path_ignored(
        Path::new("/r/.claude/worktrees/my agent"),
        &patterns
    ));
    assert!(!config::path_ignored(Path::new("/r/wt"), &patterns));
}

#[test]
fn read_failure_is_raised() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    fs::write(sandbox.root.join("home/.gitconfig"), "[broken\n").unwrap();

    let error = config::read(&repo, &env_of(&sandbox)).unwrap_err();

    assert!(
        error.to_string().starts_with("git config failed (128): "),
        "{error}"
    );
}

#[test]
fn empty_ignore_paths_ignore_nothing() {
    let mut entries = config::Entries::new();
    entries.insert(
        "ignorepaths".into(),
        vec![Entry {
            scope: "local".into(),
            value: String::new(),
        }],
    );

    assert!(config::ignore_patterns(&entries).is_empty());
}

#[test]
fn discover_from_linked_worktree_finds_main_and_all() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    git_ok(
        &sandbox,
        &repo,
        &["worktree", "add", "-q", "-b", "a", "../with space"],
    );
    let linked = sandbox.root.join("with space");

    let found = worktrees::discover(&linked, &env_of(&sandbox)).unwrap();

    assert_eq!(found.main, repo);
    assert_eq!(found.common_dir, repo.join(".git"));
    assert_eq!(found.worktrees, vec![repo, linked]);
    assert!(found.admin_dir.is_some());
}

#[test]
fn discover_skips_prunable() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    git_ok(
        &sandbox,
        &repo,
        &["worktree", "add", "-q", "-b", "gone", "../gone"],
    );
    fs::remove_dir_all(sandbox.root.join("gone")).unwrap();

    let found = worktrees::discover(&repo, &env_of(&sandbox)).unwrap();

    assert_eq!(found.worktrees, vec![repo]);
    assert!(found.admin_dir.is_none());
}

#[test]
fn toplevel_from_subdirectory() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    fs::create_dir(repo.join("sub")).unwrap();

    assert_eq!(
        worktrees::toplevel(&repo.join("sub"), &env_of(&sandbox)).unwrap(),
        repo
    );
}

#[test]
fn resolve_keeps_missing_tail() {
    let sandbox = Sandbox::new();
    let missing = sandbox.root.join("missing/./deeper/../leaf");

    assert_eq!(
        worktrees::resolve(&missing),
        sandbox.root.join("missing/leaf")
    );
}

#[test]
fn resolve_follows_symlinks_of_existing_ancestor() {
    let sandbox = Sandbox::new();
    fs::create_dir(sandbox.root.join("real")).unwrap();
    std::os::unix::fs::symlink(sandbox.root.join("real"), sandbox.root.join("link")).unwrap();

    assert_eq!(
        worktrees::resolve(&sandbox.root.join("link/new")),
        sandbox.root.join("real/new")
    );
}

#[test]
fn glob_matches_like_fnmatchcase() {
    assert!(glob::matches(
        "*/.claude/worktrees/*",
        "/a/.claude/worktrees/x/"
    ));
    assert!(glob::matches("a?c", "a/c"));
    assert!(glob::matches("[!x]b", "ab"));
    assert!(!glob::matches("[!a]b", "ab"));
    assert!(glob::matches("[a-c]", "b"));
    assert!(glob::matches("[", "["));
    assert!(!glob::matches("*.log", "x.LOG"));
    assert!(glob::matches("[^a]", "^"));
    assert!(!glob::matches("[^a]", "b"));
}
