import io
import os
import shutil

import pytest

from git_worktree_init import cli, engine
from helpers import git

PLUGIN = '''
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
        log.write(f"{ctx.mode} {ctx.name} {ctx.slot} {ctx.target.exists()}\\n")
'''


@pytest.fixture
def log(tmp_path, monkeypatch):
    path = tmp_path / "teardown.log"
    path.touch()
    monkeypatch.setenv("TEARDOWN_LOG", str(path))
    return path


@pytest.fixture
def configured(repo, tmp_path, log):
    plugin = tmp_path / "plugin.py"
    plugin.write_text(PLUGIN)
    git(repo, "config", "worktree-init.plugin", str(plugin))
    return repo


def create(repo, name):
    git(repo, "worktree", "add", "-q", "-b", name, f"../{name}")
    target = (repo.parent / name).resolve()
    engine.execute(target, "create", dict(os.environ), out=io.StringIO())
    return target


def prune(repo, monkeypatch):
    monkeypatch.chdir(repo)
    return cli.main(["prune"], out=io.StringIO(), err=io.StringIO())


def test_prune_tears_down_removed_worktree(configured, log, monkeypatch):
    create(configured, "a")
    git(configured, "worktree", "remove", "../a")

    assert prune(configured, monkeypatch) == 0
    assert prune(configured, monkeypatch) == 0
    assert log.read_text() == "prune a 1 False\n"


def test_prune_tears_down_deleted_directory(configured, log, monkeypatch):
    create(configured, "a")
    shutil.rmtree(configured.parent / "a")

    assert prune(configured, monkeypatch) == 0
    assert log.read_text() == "prune a 1 False\n"


def test_live_and_moved_worktrees_are_kept(configured, log, monkeypatch):
    create(configured, "a")
    git(configured, "worktree", "move", "../a", "../b")

    assert prune(configured, monkeypatch) == 0
    assert log.read_text() == ""


def test_failed_teardown_keeps_slot_reserved(configured, log, monkeypatch):
    create(configured, "a")
    git(configured, "worktree", "remove", "../a")
    monkeypatch.setenv("FAIL_TEARDOWN", "1")

    assert prune(configured, monkeypatch) == 1
    b = create(configured, "b")

    assert (b / ".env").read_text() == "2"
    monkeypatch.delenv("FAIL_TEARDOWN")
    assert prune(configured, monkeypatch) == 0
    assert log.read_text() == "prune a 1 False\n"


def test_create_sweeps_and_frees_slot(configured, log):
    create(configured, "a")
    git(configured, "worktree", "remove", "../a")

    b = create(configured, "b")

    assert log.read_text() == "prune a 1 False\n"
    assert (b / ".env").read_text() == "1"


def test_reused_path_tears_down_old_record(configured, log):
    create(configured, "a")
    shutil.rmtree(configured.parent / "a")
    git(configured, "worktree", "prune")
    git(configured, "worktree", "add", "-q", "../a", "a")
    target = (configured.parent / "a").resolve()

    engine.execute(target, "create", dict(os.environ), out=io.StringIO())

    assert log.read_text() == "prune a 1 True\n"
    assert (target / ".env").read_text() == "1"


def test_partial_apply_keeps_slot_for_refresh(configured, log):
    target = create(configured, "a")
    (target / ".env").unlink()

    engine.execute(target, "refresh", dict(os.environ), out=io.StringIO())

    assert (target / ".env").read_text() == "1"
