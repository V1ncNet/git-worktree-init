import io
import os
import shutil
import subprocess
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


def test_shim_starts_python_with_harness(harnessed):
    result = shim(harnessed, "0" * 40, "1" * 40, "1")

    assert result.returncode == 1


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
