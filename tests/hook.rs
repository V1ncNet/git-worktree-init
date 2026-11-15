mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use common::{Sandbox, register_hook, register_hook_with, text};

fn example_plugin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/minimal_plugin.py")
}

fn add(sandbox: &Sandbox, repo: &Path, args: &[&str]) -> Output {
    let mut full = vec!["worktree", "add"];
    full.extend(args);
    sandbox.git(repo, &full)
}

fn configure(sandbox: &Sandbox, repo: &Path, plugin: &Path) {
    register_hook(sandbox, repo);
    sandbox.git(
        repo,
        &["config", "worktree-init.plugin", plugin.to_str().unwrap()],
    );
    sandbox.git(repo, &["config", "worktree-init.reservedSlots", "0"]);
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn combined(output: &Output) -> String {
    text(&output.stdout) + &text(&output.stderr)
}

#[test]
fn unconfigured_and_include_only_repos_need_no_python() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    register_hook(&sandbox, &repo);
    let add_without_python = |args: &[&str]| {
        sandbox
            .command("git", &repo)
            .args(["worktree", "add"])
            .args(args)
            .env("WT_PYTHON", "/nonexistent/python3")
            .output()
            .unwrap()
    };

    let silent = add_without_python(&["-b", "a", "../a"]);
    fs::write(repo.join(".env"), "A=1\n").unwrap();
    fs::write(repo.join(".worktreeinclude"), ".env\n").unwrap();
    let included = add_without_python(&["-b", "b", "../b"]);

    assert!(silent.status.success());
    assert!(!combined(&silent).contains("worktree-init"));
    assert!(included.status.success(), "{}", combined(&included));
    assert_eq!(
        fs::read_to_string(sandbox.root.join("b/.env")).unwrap(),
        "A=1\n"
    );
}

#[test]
fn include_only_repo_copies_files_into_path_with_spaces() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    register_hook(&sandbox, &repo);
    fs::write(repo.join(".env"), "A=1\n").unwrap();
    fs::create_dir(repo.join("local")).unwrap();
    fs::write(repo.join("local/x.txt"), "x").unwrap();
    fs::write(repo.join(".worktreeinclude"), ".env\nlocal/\n").unwrap();

    let result = add(&sandbox, &repo, &["-b", "s", "../with space"]);

    let target = sandbox.root.join("with space");
    assert!(result.status.success(), "{}", combined(&result));
    assert_eq!(fs::read_to_string(target.join(".env")).unwrap(), "A=1\n");
    assert_eq!(fs::read_to_string(target.join("local/x.txt")).unwrap(), "x");
}

#[test]
fn local_include_only_repo_copies_files() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    register_hook(&sandbox, &repo);
    sandbox.git(&repo, &["config", "worktree-init.localInclude", ".env"]);
    fs::write(repo.join(".env"), "A=1\n").unwrap();

    let result = add(&sandbox, &repo, &["-b", "s", "../s"]);

    assert!(result.status.success(), "{}", combined(&result));
    assert_eq!(
        fs::read_to_string(sandbox.root.join("s/.env")).unwrap(),
        "A=1\n"
    );
}

#[test]
fn plugin_runs_and_allocates_consecutive_slots() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    configure(&sandbox, &repo, &example_plugin());

    let first = add(&sandbox, &repo, &["-b", "a", "../a"]);
    let second = add(&sandbox, &repo, &["-b", "b", "../b"]);

    assert!(first.status.success(), "{}", combined(&first));
    assert!(second.status.success(), "{}", combined(&second));
    let env_of = |name: &str| fs::read_to_string(sandbox.root.join(name).join(".env")).unwrap();
    assert_eq!(env_of("a"), "SLOT=1\nPORT=8001\n");
    assert_eq!(env_of("b"), "SLOT=2\nPORT=8002\n");
    assert!(text(&first.stderr).contains("Start the app on port 8001"));
}

#[test]
fn add_from_linked_worktree_uses_main_as_source() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    configure(&sandbox, &repo, &example_plugin());
    add(&sandbox, &repo, &["-b", "a", "../a"]);
    let sub = sandbox.root.join("a/sub");
    fs::create_dir(&sub).unwrap();

    let result = sandbox.git(&sub, &["worktree", "add", "-b", "c", "../../c"]);

    assert!(result.status.success(), "{}", combined(&result));
    assert!(text(&result.stderr).contains(&format!("source {})", repo.display())));
}

#[test]
fn checkout_clone_and_ignored_paths_do_nothing() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    configure(&sandbox, &repo, &example_plugin());
    add(&sandbox, &repo, &["-b", "a", "../a"]);
    let home = sandbox.root.join("home");
    let command = format!(
        r#"f="{}"; [ -x "$f" ] || exit 0; exec "$f" hook"#,
        common::wt_bin()
    );
    sandbox.git(
        &home,
        &["config", "--global", "hook.worktree-init.command", &command],
    );
    sandbox.git(
        &home,
        &[
            "config",
            "--global",
            "--add",
            "hook.worktree-init.event",
            "post-checkout",
        ],
    );
    let plugin = example_plugin();
    sandbox.git(
        &home,
        &[
            "config",
            "--global",
            "worktree-init.plugin",
            plugin.to_str().unwrap(),
        ],
    );

    let switched = sandbox.git(&sandbox.root.join("a"), &["switch", "-c", "a2"]);
    let cloned = sandbox.git(
        &sandbox.root,
        &["clone", "-q", repo.to_str().unwrap(), "clone"],
    );
    let ignored = add(
        &sandbox,
        &repo,
        &["-b", "agent", ".claude/worktrees/agent 1"],
    );

    for result in [switched, cloned, ignored] {
        assert!(result.status.success(), "{}", combined(&result));
        assert!(!combined(&result).contains("worktree-init"));
    }
}

#[test]
fn empty_ignore_paths_prepare_claude_worktrees() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    configure(&sandbox, &repo, &example_plugin());
    sandbox.git(&repo, &["config", "worktree-init.ignorePaths", ""]);

    let result = add(
        &sandbox,
        &repo,
        &["-b", "agent", ".claude/worktrees/agent 1"],
    );

    assert!(result.status.success(), "{}", combined(&result));
    assert!(repo.join(".claude/worktrees/agent 1/.env").is_file());
}

#[test]
fn failing_plugin_keeps_worktree_and_explains() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let broken = sandbox.root.join("broken.py");
    fs::write(
        &broken,
        "def slot_of(w):\n    return None\n\ndef apply(ctx):\n    raise RuntimeError('boom')\n",
    )
    .unwrap();
    configure(&sandbox, &repo, &broken);

    let result = add(&sandbox, &repo, &["-b", "a", "../a"]);

    assert_eq!(result.status.code(), Some(1));
    assert!(sandbox.root.join("a").is_dir());
    assert!(text(&result.stderr).contains("boom"));
    assert!(text(&result.stderr).contains("git wt refresh"));
}

#[test]
fn python_can_be_chosen() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    configure(&sandbox, &repo, &example_plugin());
    let marker = sandbox.root.join("marker");
    let wrapper = sandbox.root.join("python-wrapper");
    write_executable(
        &wrapper,
        &format!(
            "#!/bin/sh\necho used > \"{}\"\nexec python3 \"$@\"\n",
            marker.display()
        ),
    );
    sandbox.git(
        &repo,
        &["config", "worktree-init.python", wrapper.to_str().unwrap()],
    );

    let result = add(&sandbox, &repo, &["-b", "a", "../a"]);

    assert!(result.status.success(), "{}", combined(&result));
    assert_eq!(fs::read_to_string(marker).unwrap(), "used\n");
}

#[test]
fn refresh_twice_changes_nothing() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    configure(&sandbox, &repo, &example_plugin());
    add(&sandbox, &repo, &["-b", "a", "../a"]);
    let target = sandbox.root.join("a");
    let refresh = || sandbox.wt(&repo, &["refresh", target.to_str().unwrap()]);
    assert!(refresh().status.success());
    let modified = || {
        fs::metadata(target.join(".env"))
            .unwrap()
            .modified()
            .unwrap()
    };
    let before = modified();
    std::thread::sleep(std::time::Duration::from_millis(10));

    let second = refresh();

    assert!(second.status.success(), "{}", combined(&second));
    assert_eq!(modified(), before);
    assert!(!target.join(".env.bak").exists());
}

#[test]
fn parallel_adds_get_distinct_slots() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let slow = sandbox.root.join("slow.py");
    let source = fs::read_to_string(example_plugin()).unwrap().replace(
        "def apply(ctx) -> None:\n",
        "def apply(ctx) -> None:\n    import time\n    time.sleep(0.5)\n",
    );
    fs::write(&slow, source).unwrap();
    configure(&sandbox, &repo, &slow);

    let children: Vec<_> = ["p", "q"]
        .iter()
        .map(|name| {
            sandbox
                .command("git", &repo)
                .args(["worktree", "add", "-b", name, &format!("../{name}")])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        child.wait_with_output().unwrap();
    }

    let mut slots: Vec<String> = ["p", "q"]
        .iter()
        .map(|name| {
            let env = fs::read_to_string(sandbox.root.join(name).join(".env")).unwrap();
            env.lines().next().unwrap().to_string()
        })
        .collect();
    slots.sort();
    assert_eq!(slots, ["SLOT=1", "SLOT=2"]);
}

#[test]
fn hook_receives_three_arguments() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let log = sandbox.root.join("count");
    let stub = sandbox.root.join("stub");
    write_executable(
        &stub,
        &format!(
            "#!/bin/sh\n[ \"$1\" = hook ] && shift\necho $# > \"{}\"\n",
            log.display()
        ),
    );
    register_hook_with(&sandbox, &repo, stub.to_str().unwrap());

    let result = add(&sandbox, &repo, &["-b", "a", "../a"]);

    assert!(result.status.success(), "{}", combined(&result));
    assert_eq!(fs::read_to_string(log).unwrap(), "3\n");
}

#[test]
fn missing_binary_does_not_fail_checkout() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    register_hook_with(&sandbox, &repo, "/nonexistent/git-wt");

    let result = add(&sandbox, &repo, &["-q", "-b", "a", "../a"]);

    assert!(result.status.success());
    assert_eq!(text(&result.stderr), "");
}

#[test]
fn old_python_with_plugin_keeps_worktree() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    configure(&sandbox, &repo, &example_plugin());
    let fake = sandbox.root.join("old-python");
    write_executable(
        &fake,
        "#!/bin/sh\necho 'worktree-init: needs python >= 3.12, got 3.11.0 at /fake; set git config worktree-init.python' >&2\nexit 1\n",
    );
    sandbox.git(
        &repo,
        &["config", "worktree-init.python", fake.to_str().unwrap()],
    );

    let result = add(&sandbox, &repo, &["-b", "a", "../a"]);

    assert!(!result.status.success());
    assert!(sandbox.root.join("a").is_dir());
    assert!(text(&result.stderr).contains("set git config worktree-init.python"));
    assert!(text(&result.stderr).contains("git wt refresh"));
}

#[test]
fn hook_filters_exit_silently() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let zeros = "0000000000000000000000000000000000000000";
    let cases: [&[&str]; 4] = [
        &["hook", "abc", "def", "1"],
        &["hook", zeros, "def", "0"],
        &["hook"],
        &["hook", zeros, "def", "1"],
    ];

    for args in cases {
        let result = sandbox.wt(&repo, args);
        assert_eq!(result.status.code(), Some(0), "{args:?}");
        assert_eq!(combined(&result), "", "{args:?}");
    }
}
