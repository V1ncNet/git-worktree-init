mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use common::{Sandbox, text};

const ZEROS: &str = "0000000000000000000000000000000000000000";

const REMOVE_HELP: &str = "usage: git wt remove [-f | --force] <worktree>

    -f, --force           remove a dirty worktree, twice for a locked one

";

const PLUGIN: &str = r#"
import os
from pathlib import Path


def slot_of(worktree):
    marker = worktree / ".env"
    return int(marker.read_text()) if marker.is_file() else None


def apply(ctx):
    (ctx.target / ".env").write_text(str(ctx.slot))


def teardown(ctx):
    if os.environ.get("FAIL_TEARDOWN"):
        raise RuntimeError("database busy")
    ctx.run(["true"])
    with open(os.environ["TEARDOWN_LOG"], "a") as log:
        log.write(f"{ctx.mode} {ctx.name} {ctx.slot} {ctx.target.exists()}\n")
"#;

struct Fixture {
    sandbox: Sandbox,
    repo: PathBuf,
    log: PathBuf,
}

fn configured() -> Fixture {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let plugin = sandbox.root.join("plugin.py");
    fs::write(&plugin, PLUGIN).unwrap();
    sandbox.git(
        &repo,
        &["config", "worktree-init.plugin", plugin.to_str().unwrap()],
    );
    let log = sandbox.root.join("teardown.log");
    fs::write(&log, "").unwrap();
    Fixture { sandbox, repo, log }
}

impl Fixture {
    fn sibling(&self, name: &str) -> PathBuf {
        self.repo.parent().unwrap().join(name)
    }

    fn git(&self, args: &[&str]) {
        let output = self.sandbox.git(&self.repo, args);
        assert!(output.status.success(), "{}", text(&output.stderr));
    }

    fn wt_in(&self, cwd: &Path, args: &[&str], failing: bool) -> Output {
        let mut command = self.sandbox.wt_command(cwd);
        command.args(args).env("TEARDOWN_LOG", &self.log);
        if failing {
            command.env("FAIL_TEARDOWN", "1");
        }
        command.output().unwrap()
    }

    fn wt(&self, args: &[&str]) -> Output {
        self.wt_in(&self.repo, args, false)
    }

    fn prepare(&self, name: &str) -> PathBuf {
        self.git(&["worktree", "add", "-q", "-b", name, &format!("../{name}")]);
        let target = self.sibling(name);
        let output = self.hook(&target, "create");
        assert!(output.status.success(), "{}", text(&output.stderr));
        target
    }

    fn hook(&self, target: &Path, mode: &str) -> Output {
        self.hook_with(target, mode, false)
    }

    fn hook_with(&self, target: &Path, mode: &str, failing: bool) -> Output {
        let args = match mode {
            "create" => vec!["hook", ZEROS, ZEROS, "1"],
            _ => vec!["refresh", target.to_str().unwrap()],
        };
        self.wt_in(target, &args, failing)
    }

    fn prune(&self) -> i32 {
        self.wt(&["prune"]).status.code().unwrap()
    }

    fn prune_failing(&self) -> i32 {
        self.wt_in(&self.repo, &["prune"], true)
            .status
            .code()
            .unwrap()
    }

    fn remove(&self, args: &[&str]) -> Output {
        self.wt(&[&["remove"], args].concat())
    }

    fn remove_failing(&self, args: &[&str]) -> Output {
        self.wt_in(&self.repo, &[&["remove"], args].concat(), true)
    }

    fn logged(&self) -> String {
        fs::read_to_string(&self.log).unwrap()
    }
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

#[test]
fn prune_tears_down_removed_worktree() {
    let fixture = configured();
    fixture.prepare("a");
    fixture.git(&["worktree", "remove", "../a"]);

    assert_eq!(fixture.prune(), 0);
    assert_eq!(fixture.prune(), 0);
    assert_eq!(fixture.logged(), "prune a 1 False\n");
}

#[test]
fn prune_tears_down_deleted_directory() {
    let fixture = configured();
    fixture.prepare("a");
    fs::remove_dir_all(fixture.sibling("a")).unwrap();

    assert_eq!(fixture.prune(), 0);
    assert_eq!(fixture.logged(), "prune a 1 False\n");
}

#[test]
fn live_and_moved_worktrees_are_kept() {
    let fixture = configured();
    fixture.prepare("a");
    fixture.git(&["worktree", "move", "../a", "../b"]);

    assert_eq!(fixture.prune(), 0);
    assert_eq!(fixture.logged(), "");
}

#[test]
fn failed_teardown_keeps_slot_reserved() {
    let fixture = configured();
    fixture.prepare("a");
    fixture.git(&["worktree", "remove", "../a"]);

    assert_eq!(fixture.prune_failing(), 1);
    let b = fixture.sibling("b");
    fixture.git(&["worktree", "add", "-q", "-b", "b", "../b"]);
    let output = fixture.hook_with(&b, "create", true);
    assert_eq!(code(&output), 0, "{}", text(&output.stderr));

    assert_eq!(fs::read_to_string(b.join(".env")).unwrap(), "2");
    assert_eq!(fixture.prune(), 0);
    assert_eq!(fixture.logged(), "prune a 1 False\n");
}

#[test]
fn create_sweeps_and_frees_slot() {
    let fixture = configured();
    fixture.prepare("a");
    fixture.git(&["worktree", "remove", "../a"]);

    let b = fixture.prepare("b");

    assert_eq!(fixture.logged(), "prune a 1 False\n");
    assert_eq!(fs::read_to_string(b.join(".env")).unwrap(), "1");
}

#[test]
fn reused_path_tears_down_old_record() {
    let fixture = configured();
    fixture.prepare("a");
    fs::remove_dir_all(fixture.sibling("a")).unwrap();
    fixture.git(&["worktree", "prune"]);
    fixture.git(&["worktree", "add", "-q", "../a", "a"]);
    let target = fixture.sibling("a");

    let output = fixture.hook(&target, "create");

    assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    assert_eq!(fixture.logged(), "prune a 1 True\n");
    assert_eq!(fs::read_to_string(target.join(".env")).unwrap(), "1");
}

#[test]
fn partial_apply_keeps_slot_for_refresh() {
    let fixture = configured();
    let target = fixture.prepare("a");
    fs::remove_file(target.join(".env")).unwrap();

    let output = fixture.hook(&target, "refresh");

    assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    assert_eq!(fs::read_to_string(target.join(".env")).unwrap(), "1");
}

#[test]
fn remove_tears_down_while_files_exist() {
    let fixture = configured();
    fixture.prepare("a");

    let output = fixture.remove(&["../a"]);

    assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    assert!(!fixture.sibling("a").exists());
    assert_eq!(fixture.logged(), "remove a 1 True\n");
    assert_eq!(fixture.prune(), 0);
    assert_eq!(fixture.logged(), "remove a 1 True\n");
}

#[test]
fn failed_teardown_keeps_worktree_and_record() {
    let fixture = configured();
    let target = fixture.prepare("a");

    let output = fixture.remove_failing(&["../a"]);

    assert_eq!(code(&output), 1);
    let stderr = text(&output.stderr);
    assert!(
        stderr.contains("teardown failed: database busy"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "the worktree is kept, retry with: git wt remove {}",
            target.display()
        )),
        "{stderr}"
    );
    assert!(target.is_dir());
    assert_eq!(code(&fixture.remove(&["../a"])), 0);
    assert_eq!(fixture.logged(), "remove a 1 True\n");
}

#[test]
fn dirty_worktree_is_refused_before_teardown() {
    let fixture = configured();
    let target = fixture.prepare("a");
    fs::write(target.join("untracked"), "x").unwrap();

    let output = fixture.remove(&["../a"]);

    assert_ne!(code(&output), 0);
    assert!(target.is_dir());
    assert_eq!(fixture.logged(), "");
    assert_eq!(code(&fixture.remove(&["--force", "../a"])), 0);
    assert_eq!(fixture.logged(), "remove a 1 True\n");
}

#[test]
fn locked_worktree_needs_force_twice() {
    let fixture = configured();
    fixture.prepare("a");
    fixture.git(&["worktree", "lock", "../a"]);

    assert_ne!(code(&fixture.remove(&["-f", "../a"])), 0);
    assert_eq!(fixture.logged(), "");
    assert_eq!(code(&fixture.remove(&["-ff", "../a"])), 0);
}

#[test]
fn remove_without_record_falls_back_to_slot_of() {
    let fixture = configured();
    fixture.git(&["worktree", "add", "-q", "-b", "a", "../a"]);
    fs::write(fixture.sibling("a").join(".env"), "4").unwrap();

    let output = fixture.remove(&["../a"]);

    assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    assert_eq!(fixture.logged(), "remove a 4 True\n");
}

#[test]
fn remove_help_exits_129() {
    let fixture = configured();

    let output = fixture.remove(&["-h"]);

    assert_eq!(code(&output), 129);
    assert_eq!(text(&output.stdout), REMOVE_HELP);
    assert_eq!(text(&output.stderr), "");
}

#[test]
fn remove_without_any_slot_skips_teardown() {
    let fixture = configured();
    fixture.git(&["worktree", "add", "-q", "-b", "a", "../a"]);

    let output = fixture.remove(&["../a"]);

    assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    assert_eq!(fixture.logged(), "");
}

#[test]
fn removes_worktree_recorded_by_python_version() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/minimal_plugin.py");
    sandbox.git(
        &repo,
        &["config", "worktree-init.plugin", plugin.to_str().unwrap()],
    );
    sandbox.git(&repo, &["worktree", "add", "-q", "-b", "a", "../a"]);
    let target = repo.parent().unwrap().join("a");
    let records = repo.join(".git/worktree-init/records");
    fs::create_dir_all(&records).unwrap();
    let written_by_python = format!(
        "{{\n  \"admin\": \"a\",\n  \"slot\": 3,\n  \"target\": \"{}\",\n  \"options\": {{\n    \"slot\": 3,\n    \"source\": null,\n    \"port_base\": \"7000\"\n  }}\n}}\n",
        target.display()
    );
    fs::write(records.join("0123456789abcdef.json"), written_by_python).unwrap();
    fs::write(
        repo.join(".git/worktrees/a/worktree-init-record"),
        "0123456789abcdef\n",
    )
    .unwrap();

    let output = sandbox.wt(&repo, &["remove", "../a"]);

    assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("worktree-init: remove"));
    assert!(text(&output.stderr).contains("(slot 3)"));
    assert!(text(&output.stderr).contains("released port 7003"));
    assert!(!target.exists());
    assert!(!records.join("0123456789abcdef.json").exists());
}

#[test]
fn prune_releases_port_of_deleted_worktree() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/minimal_plugin.py");
    sandbox.git(
        &repo,
        &["config", "worktree-init.plugin", plugin.to_str().unwrap()],
    );
    sandbox.git(&repo, &["config", "worktree-init.port-base", "9000"]);
    sandbox.git(&repo, &["worktree", "add", "-q", "-b", "a", "../a"]);
    let target = repo.parent().unwrap().join("a");
    let created = sandbox
        .wt_command(&target)
        .args(["hook", ZEROS, ZEROS, "1"])
        .output()
        .unwrap();
    assert_eq!(code(&created), 0, "{}", text(&created.stderr));
    fs::remove_dir_all(&target).unwrap();

    let output = sandbox.wt(&repo, &["prune"]);

    assert_eq!(code(&output), 0, "{}", text(&output.stderr));
    assert!(text(&output.stderr).contains("released port 9001"));
}

#[test]
fn prune_help_and_arguments_follow_git() {
    let fixture = configured();

    let help = fixture.wt(&["prune", "-h"]);
    let extra = fixture.wt(&["prune", "x"]);

    assert_eq!(code(&help), 129);
    assert_eq!(text(&help.stdout), "usage: git wt prune\n\n");
    assert_eq!(code(&extra), 129);
    assert_eq!(
        text(&extra.stderr),
        "error: prune takes no arguments\nusage: git wt prune\n\n"
    );
}

#[test]
fn remove_usage_errors_follow_argparse() {
    let fixture = configured();
    let cases: [(&[&str], &str); 4] = [
        (&[], "the following arguments are required: worktree"),
        (&["a", "b"], "unrecognized arguments: b"),
        (&["--forc", "a"], "unrecognized arguments: --forc"),
        (
            &["--force=1", "a"],
            "argument -f/--force: ignored explicit argument '1'",
        ),
    ];

    for (args, message) in cases {
        let output = fixture.remove(args);
        assert_eq!(code(&output), 129, "{args:?}");
        assert_eq!(
            text(&output.stderr),
            format!("error: {message}\n{REMOVE_HELP}"),
            "{args:?}"
        );
    }
}

#[test]
fn remove_counts_long_short_force_runs_without_overflow() {
    let fixture = configured();

    for length in [255, 300] {
        let flags = format!("-{}", "f".repeat(length));
        let output = fixture.remove(&[&flags]);
        assert_eq!(code(&output), 129, "{length}");
        assert_eq!(
            text(&output.stderr),
            format!("error: the following arguments are required: worktree\n{REMOVE_HELP}")
        );
    }
}

#[test]
fn remove_outside_worktree_is_reported() {
    let fixture = configured();
    let missing = fixture.sibling("missing");

    let output = fixture.remove(&["../missing"]);

    assert_eq!(code(&output), 2);
    assert_eq!(
        text(&output.stderr),
        format!(
            "worktree-init: not a git worktree: {}\n",
            fixture.repo.join("../missing").display()
        )
    );
    assert!(!missing.exists());
}
