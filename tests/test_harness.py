import io
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from git_worktree_init import cli, harness
from helpers import HARNESS_HOOK, git

RUNTIME_BLOCK = "# claude-code-runtime\n**/.claude/worktrees/\n**/.claude/scheduled_tasks.lock\n\n# other\nfoo/\n"


@pytest.fixture
def code(repo):
    exclude = repo / ".git" / "info" / "exclude"
    exclude.write_text(exclude.read_text() + "CLAUDE.md\n.claude/\n.superpowers/\ndocs/superpowers/\n" + RUNTIME_BLOCK)
    return repo


@pytest.fixture
def harnessed(code):
    harness.init(code, "/nonexistent/harness.git", dict(os.environ))
    return code


def ignored(target, *paths):
    output = harness.git(target, "check-ignore", "--no-index", *paths, env=dict(os.environ), check=False)
    return set(output.splitlines())


def shim(cwd, *args, **extra):
    env = {k: v for k, v in os.environ.items() if k != "GIT_DIR"} | {"WT_PYTHON": shutil.which("false")} | extra
    return subprocess.run(["sh", str(HARNESS_HOOK), *args], cwd=cwd, env=env, capture_output=True, text=True)


def test_init_creates_bare_repo_with_allowlist(harnessed):
    hidden = ignored(
        harnessed,
        "src/Main.java", ".idea/misc.xml", "CLAUDE.md", "CLAUDE.local.md", ".claude/handoffs/x.md",
        "docs/superpowers/specs/a.md", "docs/other.md", ".claude/settings.local.json",
        ".claude/worktrees/agent/x", ".claude/scheduled_tasks.lock", ".claude/hooks/__pycache__/a.pyc",
        "foo/bar",
    )

    assert harness.git_dir(harnessed / ".git") == harnessed / ".git" / "harness.git"
    assert harness.enabled(harnessed / ".git")
    assert git(harnessed / ".git" / "harness.git", "config", "core.bare").stdout.strip() == "true"
    assert git(harnessed / ".git" / "harness.git", "config", "remote.harness.url").stdout.strip() == "/nonexistent/harness.git"
    assert hidden == {
        "src/Main.java", ".idea/misc.xml", "CLAUDE.local.md", "docs/other.md", ".claude/settings.local.json",
        ".claude/worktrees/agent/x", ".claude/scheduled_tasks.lock", ".claude/hooks/__pycache__/a.pyc",
        "foo/bar",
    }


def test_init_sets_overrides(harnessed):
    config = harnessed / ".git" / "harness.git"

    def value(key):
        return git(config, "config", "--get", key).stdout.strip()

    assert value("commit.gpgsign") == "false"
    assert value("hook.beams.enabled") == "false"
    assert value("hook.worktree-init.enabled") == "false"
    assert value("hook.harness.enabled") == "false"
    assert value("core.excludesFile") == "/dev/null"


def test_enabled_is_false_without_harness(code):
    assert not harness.enabled(code / ".git")


def test_harness_works_for_worktree_outside_parent(harnessed, tmp_path):
    target = tmp_path / "elsewhere" / "with space" / "wt"
    target.parent.mkdir(parents=True)
    git(harnessed, "worktree", "add", "-q", "-b", "feature/x", str(target))
    (target / "CLAUDE.md").write_text("x")

    status = harness.git(target, "status", "--porcelain", env=dict(os.environ))

    assert status == "?? CLAUDE.md\n"


def test_global_excludes_do_not_hide_harness_files(harnessed, tmp_path):
    excludes = tmp_path / "global-excludes"
    excludes.write_text(".claude\nCLAUDE.md\n")
    git(harnessed, "config", "--global", "core.excludesFile", str(excludes))

    assert ignored(harnessed, "CLAUDE.md", ".claude/handoffs/x.md") == set()


def test_code_status_unaffected(harnessed):
    env = dict(os.environ)
    (harnessed / "CLAUDE.md").write_text("x")
    harness.git(harnessed, "add", "CLAUDE.md", env=env)
    harness.git(harnessed, "commit", "-q", "-m", "Add instructions", env=env)

    assert git(harnessed, "status", "--porcelain").stdout == ""
    assert harness.git(harnessed, "log", "--format=%s", env=env) == "Add instructions\n"


def test_shim_exits_without_harness(code):
    result = shim(code, "0" * 40, "1" * 40, "1")

    assert result.returncode == 0


def test_shim_exits_inside_harness_operation(harnessed):
    result = shim(harnessed, "0" * 40, "1" * 40, "1", GIT_DIR=str(harnessed / ".git" / "harness.git"))

    assert result.returncode == 0


def test_shim_starts_python_with_harness(harnessed, tmp_path):
    marker = tmp_path / "marker"
    fake = tmp_path / "fake-python"
    fake.write_text(f'#!/bin/sh\necho "$@" > "{marker}"\nexit 3\n')
    fake.chmod(0o755)

    result = shim(harnessed, "0" * 40, "1" * 40, "1", WT_PYTHON=str(fake))

    assert result.returncode == 0
    assert marker.read_text().split()[1:3] == ["harness-hook", "post-checkout"]


def test_pre_push_survives_broken_python(pushing):
    target, _ = pushing
    git(target, "config", "worktree-init.python", "/nonexistent/python")

    result = git(target, "push", "-q", "origin", "feature/foo", check=False)

    assert result.returncode == 0
    assert git(target, "ls-remote", "origin", "feature/foo").stdout


def test_shim_exits_outside_repository(tmp_path):
    result = shim(tmp_path, "committed")

    assert result.returncode == 0


def test_cli_init_creates_harness(code, monkeypatch):
    monkeypatch.chdir(code)

    result = cli.main(["harness", "init", "--remote", "/r.git"], env=dict(os.environ), err=io.StringIO())

    assert result == 0
    assert harness.enabled(code / ".git")


def test_cli_harness_without_subcommand_is_usage_error(code, monkeypatch):
    monkeypatch.chdir(code)
    err = io.StringIO()

    result = cli.main(["harness"], env=dict(os.environ), err=err)

    assert result == 129
    assert "git wt harness init --remote <url>" in err.getvalue()


@pytest.fixture
def seeded(harnessed):
    env = dict(os.environ)
    (harnessed / "CLAUDE.md").write_text("develop")
    harness.git(harnessed, "add", "CLAUDE.md", env=env)
    harness.git(harnessed, "commit", "-q", "-m", "Seed", env=env)
    git(harnessed, "config", "hook.harness.command", str(HARNESS_HOOK))
    git(harnessed, "config", "--add", "hook.harness.event", "post-checkout")
    git(harnessed, "config", "worktree-init.python", sys.executable)
    return harnessed


def harness_branch_with(main, branch, rel, text):
    env = dict(os.environ)
    harness.git(main, "checkout", "-q", "-b", branch, env=env)
    (main / rel).parent.mkdir(parents=True, exist_ok=True)
    (main / rel).write_text(text)
    harness.git(main, "add", rel, env=env)
    harness.git(main, "commit", "-q", "-m", f"Add {rel}", env=env)
    harness.git(main, "checkout", "-q", "develop", env=env)


def current(target):
    return harness.git(target, "symbolic-ref", "--short", "HEAD", env=dict(os.environ)).strip()


def test_reused_worktree_path_does_not_snapshot_stale_admin(seeded):
    env = dict(os.environ)
    git(seeded, "worktree", "add", "-q", "-b", "feature/old", "../wt")
    target = seeded.parent / "wt"
    git(seeded, "worktree", "remove", "--force", str(target))
    before = harness.git(seeded, "rev-parse", "feature/old", env=env)

    git(seeded, "worktree", "add", "-q", "-b", "feature/new", "../wt")

    assert harness.git(seeded, "rev-parse", "feature/old", env=env) == before
    assert current(target) == "feature/new"
    assert (target / "CLAUDE.md").read_text() == "develop"


def test_branch_of_removed_worktree_attaches_elsewhere(seeded):
    git(seeded, "worktree", "add", "-q", "-b", "feature/x", "../a")
    git(seeded, "worktree", "remove", "--force", str(seeded.parent / "a"))

    git(seeded, "worktree", "add", "-q", "../b", "feature/x")

    target = seeded.parent / "b"
    assert current(target) == "feature/x"
    assert (target / "CLAUDE.md").read_text() == "develop"


def test_snapshot_refuses_when_every_harness_file_is_gone(seeded):
    env = dict(os.environ)
    head = harness.git(seeded, "rev-parse", "HEAD", env=env)
    (seeded / "CLAUDE.md").unlink()

    assert not harness.snapshot(seeded, env)
    assert harness.git(seeded, "rev-parse", "HEAD", env=env) == head


def test_snapshot_honours_harness_gitignore_and_skips_ds_store(seeded):
    env = dict(os.environ)
    plugin = seeded / ".claude" / "plugin"
    (plugin / ".pytest_cache").mkdir(parents=True)
    (plugin / ".gitignore").write_text(".pytest_cache/\n")
    (plugin / ".pytest_cache" / "x").write_text("cache")
    (seeded / ".claude" / ".DS_Store").write_text("finder")

    assert harness.snapshot(seeded, env)

    files = harness.git(seeded, "ls-files", env=env).splitlines()
    assert ".claude/plugin/.gitignore" in files
    assert ".claude/plugin/.pytest_cache/x" not in files
    assert ".claude/.DS_Store" not in files


def test_new_branch_forks_from_t3_merge_base(seeded):
    harness_branch_with(seeded, "release/x", ".claude/release.md", "release")
    git(seeded, "branch", "feature/a")
    git(seeded, "config", "branch.feature/a.t3-merge-base", "release/x")

    git(seeded, "worktree", "add", "-q", "../a", "feature/a")

    target = seeded.parent / "a"
    assert current(target) == "feature/a"
    assert (target / ".claude" / "release.md").read_text() == "release"
    assert (target / "CLAUDE.md").read_text() == "develop"


def test_new_branch_forks_from_renamed_upstream(seeded):
    harness_branch_with(seeded, "release/x", ".claude/release.md", "release")
    git(seeded, "branch", "feature/b")
    git(seeded, "config", "branch.feature/b.remote", "origin")
    git(seeded, "config", "branch.feature/b.merge", "refs/heads/release/x")

    git(seeded, "worktree", "add", "-q", "../b", "feature/b")

    assert (seeded.parent / "b" / ".claude" / "release.md").read_text() == "release"


def test_branch_tracking_itself_forks_from_develop(seeded):
    harness_branch_with(seeded, "feature/c", ".claude/other.md", "other")
    git(seeded, "branch", "-q", "feature/c-code")
    git(seeded, "config", "branch.feature/c-code.remote", "origin")
    git(seeded, "config", "branch.feature/c-code.merge", "refs/heads/feature/c-code")

    git(seeded, "worktree", "add", "-q", "../c", "feature/c-code")

    target = seeded.parent / "c"
    assert current(target) == "feature/c-code"
    assert (target / "CLAUDE.md").read_text() == "develop"
    assert not (target / ".claude" / "other.md").exists()


def test_existing_remote_harness_branch_is_resumed(seeded, tmp_path):
    remote = tmp_path / "remote.git"
    git(tmp_path, "init", "-q", "--bare", str(remote))
    root = seeded / ".git" / "harness.git"
    git(root, "remote", "set-url", "harness", str(remote))
    harness_branch_with(seeded, "feature/foo", ".claude/handoffs/a.md", "handoff")
    git(root, "push", "-q", "harness", "feature/foo")
    git(root, "branch", "-q", "-D", "feature/foo")

    git(seeded, "worktree", "add", "-q", "-b", "feature/foo", "../foo")

    assert (seeded.parent / "foo" / ".claude" / "handoffs" / "a.md").read_text() == "handoff"


def test_switch_snapshots_dirty_harness_first(seeded):
    git(seeded, "worktree", "add", "-q", "-b", "feature/a", "../a")
    target = seeded.parent / "a"
    (target / ".claude").mkdir()
    (target / ".claude" / "h.md").write_text("dirty")

    git(target, "checkout", "-q", "-b", "feature/b")

    log = harness.git(target, "log", "--format=%s", "feature/a", env=dict(os.environ))
    assert log.splitlines()[0] == "Snapshot harness state"
    assert current(target) == "feature/b"


def test_new_worktree_does_not_snapshot_deletions(seeded):
    git(seeded, "worktree", "add", "-q", "-b", "feature/a", "../a")

    log = harness.git(seeded, "log", "--format=%s", "develop", env=dict(os.environ))
    assert log == "Seed\n"


def test_rebase_with_autostash_leaves_harness_untouched(seeded):
    git(seeded, "worktree", "add", "-q", "-b", "feature/a", "../a")
    target = seeded.parent / "a"
    (target / "f.txt").write_text("f")
    git(target, "add", "f.txt")
    git(target, "commit", "-q", "-m", "Feature")
    (seeded / "m.txt").write_text("m")
    git(seeded, "add", "m.txt")
    git(seeded, "commit", "-q", "-m", "Main")
    (target / ".gitignore").write_text("dirty\n")
    (target / "CLAUDE.md").write_text("dirty harness")
    env = dict(os.environ)
    head = harness.git(target, "rev-parse", "HEAD", env=env)
    index = harness.git(target, "ls-files", "--stage", env=env)
    status = harness.git(target, "status", "--porcelain", env=env)

    result = git(target, "-c", "rebase.autostash=true", "rebase", "main", check=False)

    assert result.returncode == 0, result.stderr
    assert harness.git(target, "rev-parse", "HEAD", env=env) == head
    assert harness.git(target, "ls-files", "--stage", env=env) == index
    assert harness.git(target, "status", "--porcelain", env=env) == status


@pytest.fixture
def pushing(seeded, tmp_path):
    origin = tmp_path / "origin.git"
    remote = tmp_path / "harness-remote.git"
    for bare in (origin, remote):
        git(tmp_path, "init", "-q", "--bare", str(bare))
    git(seeded, "remote", "add", "origin", str(origin))
    git(seeded / ".git" / "harness.git", "remote", "set-url", "harness", str(remote))
    git(seeded, "config", "--add", "hook.harness.event", "pre-push")
    git(seeded, "worktree", "add", "-q", "-b", "feature/foo", "../foo")
    target = seeded.parent / "foo"
    (target / ".claude").mkdir()
    (target / ".claude" / "h.md").write_text("handoff")
    return target, remote


def test_harness_network_calls_never_prompt(seeded, tmp_path, monkeypatch):
    log = tmp_path / "ssh-args"
    fake = tmp_path / "bin" / "ssh"
    fake.parent.mkdir()
    fake.write_text(f'#!/bin/sh\necho "$@" >> "{log}"\nexit 255\n')
    fake.chmod(0o755)
    monkeypatch.setenv("PATH", f"{fake.parent}:{os.environ['PATH']}")
    git(seeded / ".git" / "harness.git", "remote", "set-url", "harness", "ssh://example.invalid/h.git")

    assert not harness.push(seeded, dict(os.environ), err=io.StringIO())

    assert "BatchMode=yes" in log.read_text()
    assert "ConnectTimeout=" in log.read_text()



def test_pre_push_pushes_harness_branch(pushing):
    target, remote = pushing

    result = git(target, "push", "-q", "origin", "feature/foo", check=False)

    assert result.returncode == 0, result.stderr
    assert git(remote, "show", "feature/foo:.claude/h.md").stdout == "handoff"


def test_pre_push_survives_unreachable_harness(pushing):
    target, _ = pushing
    git(target / ".." / "proj" / ".git" / "harness.git", "remote", "set-url", "harness", "/nonexistent")

    result = git(target, "push", "-q", "origin", "feature/foo", check=False)

    assert result.returncode == 0
    assert "harness push failed" in result.stderr
    assert git(target, "ls-remote", "origin", "feature/foo").stdout


def test_sync_exits_one_on_failure(pushing, monkeypatch):
    target, _ = pushing
    git(target / ".." / "proj" / ".git" / "harness.git", "remote", "set-url", "harness", "/nonexistent")
    monkeypatch.chdir(target)
    err = io.StringIO()

    result = cli.main(["harness", "sync"], env=dict(os.environ), err=err)

    assert result == 1
    assert "harness push failed" in err.getvalue()


def test_sync_pushes_snapshot(pushing, monkeypatch):
    target, remote = pushing
    monkeypatch.chdir(target)

    result = cli.main(["harness", "sync"], env=dict(os.environ), err=io.StringIO())

    assert result == 0
    assert git(remote, "show", "feature/foo:.claude/h.md").stdout == "handoff"


def test_push_never_forces(pushing, tmp_path):
    target, remote = pushing
    env = dict(os.environ)
    assert harness.snapshot(target, env)
    assert harness.push(target, env)
    other = tmp_path / "other"
    git(tmp_path, "clone", "-q", "-b", "feature/foo", str(remote), str(other))
    (other / "x.md").write_text("remote")
    git(other, "add", "x.md")
    git(other, "commit", "-q", "-m", "Remote change")
    git(other, "push", "-q", "origin", "feature/foo")
    advanced = git(remote, "rev-parse", "feature/foo").stdout
    (target / ".claude" / "h.md").write_text("local")
    harness.snapshot(target, env)

    assert not harness.push(target, env)
    assert git(remote, "rev-parse", "feature/foo").stdout == advanced
