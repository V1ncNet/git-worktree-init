mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::Sandbox;
use git_wt::bridge::{self, Ctx, Plugin};
use git_wt::error::Error;
use git_wt::gitenv::{self, Env};
use git_wt::options::Values;
use serde_json::json;

fn env_of(sandbox: &Sandbox) -> Env {
    let mut env = gitenv::sanitized(&gitenv::process_env(), &[]);
    env.retain(|key, _| !key.starts_with("WT_"));
    env.extend(sandbox.env());
    env
}

fn example_plugin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/minimal_plugin.py")
}

fn write_plugin(sandbox: &Sandbox, text: &str) -> PathBuf {
    let path = sandbox.root.join("plugin.py");
    fs::write(&path, text).unwrap();
    path
}

fn context(sandbox: &Sandbox, env: &Env, slot: Option<u32>, options: Values) -> Ctx {
    let target = sandbox.root.join("target");
    fs::create_dir_all(&target).unwrap();
    Ctx {
        main: sandbox.root.join("main"),
        target,
        common_dir: sandbox.root.join("main/.git"),
        mode: "add".into(),
        env: env.clone(),
        worktrees: vec![sandbox.root.join("main")],
        source: None,
        slot,
        options,
    }
}

fn start(sandbox: &Sandbox, path: &Path) -> Plugin {
    Plugin::start(path, "python3", &env_of(sandbox)).unwrap()
}

fn plugin_error(result: Result<(), Error>) -> String {
    match result {
        Err(Error::Plugin(message)) => message,
        other => panic!("expected a plugin error, got {other:?}"),
    }
}

const NOISY: &str = r#"
import sys
print("noise")

def slot_of(worktree):
    return None

def apply(ctx):
    print("noise")
    ctx.run(["echo", "child"])
"#;

#[test]
fn loads_example_plugin_and_applies() {
    let sandbox = Sandbox::new();
    let env = env_of(&sandbox);
    let mut plugin = start(&sandbox, &example_plugin());
    let options: Values = json!({"port_base": "8000"}).as_object().unwrap().clone();
    let ctx = context(&sandbox, &env, Some(2), options);

    plugin.apply(&ctx).unwrap();

    let written = fs::read_to_string(ctx.target.join(".env")).unwrap();
    assert_eq!(written, "SLOT=2\nPORT=8002\n");
    assert_eq!(
        plugin.next_steps().unwrap(),
        vec!["Start the app on port 8002".to_string()]
    );
    assert_eq!(plugin.options[0].name, "port_base");
    assert!(plugin.has_next_steps && plugin.has_teardown);
}

#[test]
fn slot_of_reads_the_worktree() {
    let sandbox = Sandbox::new();
    let mut plugin = start(&sandbox, &example_plugin());
    let worktree = sandbox.root.join("tree");
    fs::create_dir_all(&worktree).unwrap();
    assert_eq!(plugin.slot_of(&worktree).unwrap(), None);

    fs::write(worktree.join(".env"), "SLOT=4\n").unwrap();

    assert_eq!(plugin.slot_of(&worktree).unwrap(), Some(4));
}

#[test]
fn plugin_stdout_does_not_break_protocol() {
    let sandbox = Sandbox::new();
    let env = env_of(&sandbox);
    let mut plugin = start(&sandbox, &write_plugin(&sandbox, NOISY));
    let ctx = context(&sandbox, &env, None, Values::new());

    plugin.apply(&ctx).unwrap();

    assert_eq!(plugin.slot_of(&ctx.target).unwrap(), None);
}

#[test]
fn plugin_exit_in_apply_is_error() {
    let sandbox = Sandbox::new();
    let env = env_of(&sandbox);
    let text = "import sys\ndef slot_of(w): return None\ndef apply(ctx): sys.exit(3)\n";
    let mut plugin = start(&sandbox, &write_plugin(&sandbox, text));

    let message = plugin_error(plugin.apply(&context(&sandbox, &env, None, Values::new())));

    assert_eq!(message, "plugin exited during apply");
}

#[test]
fn plugin_exception_becomes_error() {
    let sandbox = Sandbox::new();
    let env = env_of(&sandbox);
    let text = "def slot_of(w): return None\ndef apply(ctx): raise RuntimeError('apply failed')\n";
    let mut plugin = start(&sandbox, &write_plugin(&sandbox, text));

    let message = plugin_error(plugin.apply(&context(&sandbox, &env, None, Values::new())));

    assert_eq!(message, "apply failed");
    assert_eq!(plugin.slot_of(&sandbox.root).unwrap(), None);
}

#[test]
fn broken_plugin_fails_to_load_with_its_message() {
    let sandbox = Sandbox::new();
    let path = write_plugin(&sandbox, "def apply(ctx): pass\n");

    let error = Plugin::start(&path, "python3", &env_of(&sandbox))
        .err()
        .unwrap();

    assert!(error.to_string().contains("missing function slot_of"));
}

#[test]
fn missing_plugin_file_is_reported_before_python_starts() {
    let sandbox = Sandbox::new();
    let path = sandbox.root.join("nope.py");

    let error = Plugin::start(&path, "/nonexistent/python3", &env_of(&sandbox))
        .err()
        .unwrap();

    assert_eq!(
        error.to_string(),
        format!("plugin not found: {}", path.display())
    );
}

#[test]
fn plugin_children_do_not_hold_pipes() {
    let sandbox = Sandbox::new();
    let env = env_of(&sandbox);
    let mut plugin = start(&sandbox, &write_plugin(&sandbox, FD_PROBE));
    let ctx = context(&sandbox, &env, None, Values::new());

    plugin.apply(&ctx).unwrap();

    assert_eq!(fs::read_to_string(ctx.target.join("open-fds")).unwrap(), "");
}

const FD_PROBE: &str = r#"
import subprocess, sys

PROBE = """
import os
for fd in (3, 4):
    try:
        os.fstat(fd)
        print(fd)
    except OSError:
        pass
"""

def slot_of(worktree):
    return None

def apply(ctx):
    child = subprocess.run([sys.executable, "-c", PROBE], close_fds=False, capture_output=True, text=True)
    (ctx.target / "open-fds").write_text(child.stdout)
"#;

#[test]
fn old_python_reports_version_message() {
    let sandbox = Sandbox::new();
    let fake = sandbox.root.join("fake-python");
    let message = "worktree-init: needs python >= 3.12, got 3.11.0 at /fake; set git config worktree-init.python";
    fs::write(&fake, format!("#!/bin/sh\necho '{message}' >&2\nexit 1\n")).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();

    let error = Plugin::start(&example_plugin(), fake.to_str().unwrap(), &env_of(&sandbox))
        .err()
        .unwrap();

    assert!(matches!(&error, Error::Plugin(text) if text.contains("did not start")));
}

#[test]
fn missing_python_fails_cleanly() {
    let sandbox = Sandbox::new();

    let error = Plugin::start(&example_plugin(), "/nonexistent/python3", &env_of(&sandbox))
        .err()
        .unwrap();

    assert!(matches!(&error, Error::Plugin(text) if text.contains("/nonexistent/python3")));
}

#[test]
fn python_resolution_order() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let mut env = env_of(&sandbox);
    let fallback = option_env!("WT_DEFAULT_PYTHON").unwrap_or("python3");
    assert_eq!(bridge::python(&env, &repo), fallback);

    sandbox.git(&repo, &["config", "worktree-init.python", "/from/config"]);
    assert_eq!(bridge::python(&env, &repo), "/from/config");

    env.insert("WT_PYTHON".into(), "/from/env".into());
    assert_eq!(bridge::python(&env, &repo), "/from/env");
}

#[test]
fn locate_resolves_relative_absolute_and_home_paths() {
    let main = Path::new("/srv/main");
    let entries = |value: &str| {
        let mut entries = git_wt::config::Entries::new();
        entries.insert(
            "plugin".into(),
            vec![git_wt::config::Entry {
                scope: "local".into(),
                value: value.into(),
            }],
        );
        entries
    };

    assert_eq!(bridge::locate(main, &git_wt::config::Entries::new()), None);
    assert_eq!(
        bridge::locate(main, &entries("/abs/p.py")),
        Some(PathBuf::from("/abs/p.py"))
    );
    assert_eq!(
        bridge::locate(main, &entries("tools/p.py")),
        Some(PathBuf::from("/srv/main/tools/p.py"))
    );
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    assert_eq!(
        bridge::locate(main, &entries("~/p.py")),
        Some(home.join("p.py"))
    );
}
