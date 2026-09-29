import io
import os

import pytest

from git_worktree_init import cli, engine
from helpers import EXAMPLE_PLUGIN, git, prompting_env


class FakeTty:
    def __init__(self, answers):
        self.input = io.StringIO(answers)
        self.output = io.StringIO()

    def readline(self):
        return self.input.readline()

    def write(self, text):
        self.output.write(text)

    def flush(self):
        pass

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


@pytest.fixture
def configured(repo):
    git(repo, "config", "worktree-init.plugin", str(EXAMPLE_PLUGIN))
    git(repo, "config", "worktree-init.reservedSlots", "0")
    git(repo, "worktree", "add", "-q", "-b", "w", "../w")
    return repo, (repo.parent / "w").resolve()


def test_interactive_answers_win_over_defaults(configured):
    _, target = configured
    tty = FakeTty("4\n9000\n")

    engine.execute(target, "create", prompting_env(), out=io.StringIO(), tty_opener=lambda: tty)

    assert (target / ".env").read_text() == "SLOT=4\nPORT=9004\n"
    assert "? Slot (1) › " in tty.output.getvalue()


def test_env_option_disables_prompt(configured):
    _, target = configured

    engine.execute(target, "create", prompting_env(WT_PORT_BASE="7000"), out=io.StringIO(), tty_opener=pytest.fail)

    assert (target / ".env").read_text() == "SLOT=1\nPORT=7001\n"


def test_terminal_prompt_off_disables_prompt(configured):
    _, target = configured

    engine.execute(target, "create", dict(os.environ), out=io.StringIO(), tty_opener=pytest.fail)

    assert (target / ".env").read_text() == "SLOT=1\nPORT=8001\n"


def test_no_tty_uses_defaults(configured):
    _, target = configured

    engine.execute(target, "create", prompting_env(), out=io.StringIO(), tty_opener=lambda: None)

    assert (target / ".env").read_text() == "SLOT=1\nPORT=8001\n"


def test_taken_slot_is_rejected(configured):
    repo, target = configured
    (repo / ".env").write_text("SLOT=5\n")

    with pytest.raises(ValueError, match="slot 5 is taken"):
        engine.execute(target, "create", dict(os.environ, WT_SLOT="5"), out=io.StringIO())


def test_reserved_slot_is_rejected(configured):
    _, target = configured

    with pytest.raises(ValueError, match="reserved"):
        engine.execute(target, "create", dict(os.environ, WT_SLOT="0"), out=io.StringIO())


def test_refresh_keeps_own_slot(configured):
    _, target = configured
    (target / ".env").write_text("SLOT=6\n")

    engine.execute(target, "refresh", dict(os.environ), out=io.StringIO())

    assert (target / ".env").read_text() == "SLOT=6\nPORT=8006\n"


def test_create_on_prepared_worktree_keeps_slot_and_files(configured):
    repo, target = configured
    (repo / ".worktreeinclude").write_text("*.log\n")
    (repo / "a.log").write_text("new")
    (target / "a.log").write_text("old")
    (target / ".env").write_text("SLOT=6\n")

    engine.execute(target, "create", dict(os.environ), out=io.StringIO())

    assert (target / ".env").read_text() == "SLOT=6\nPORT=8006\n"
    assert (target / "a.log").read_text() == "old"


def test_relative_source_is_rejected(configured):
    _, target = configured

    with pytest.raises(ValueError, match="absolute"):
        engine.execute(target, "create", dict(os.environ, WT_SOURCE="."), out=io.StringIO())


def test_source_from_other_repository_is_rejected(configured, tmp_path):
    _, target = configured
    other = tmp_path / "other"
    other.mkdir()
    git(other, "init", "-q")

    with pytest.raises(ValueError, match="another repository"):
        engine.execute(target, "create", dict(os.environ, WT_SOURCE=str(other)), out=io.StringIO())


def test_keyboard_interrupt_returns_130(configured, monkeypatch):
    _, target = configured

    def interrupted(*args, **kwargs):
        raise KeyboardInterrupt

    monkeypatch.setattr(engine, "execute", interrupted)
    err = io.StringIO()

    code = cli.main(["refresh", str(target)], env=dict(os.environ), out=io.StringIO(), err=err)

    assert code == 130
    assert "git wt refresh" in err.getvalue()


def test_add_keyboard_interrupt_returns_130(configured, monkeypatch):
    repo, _ = configured

    real_run = cli.subprocess.run

    def interrupted(command, *args, **kwargs):
        if command[:3] == ["git", "worktree", "add"]:
            raise KeyboardInterrupt
        return real_run(command, *args, **kwargs)

    monkeypatch.setattr(cli.subprocess, "run", interrupted)
    monkeypatch.chdir(repo)

    code = cli.main(["add", "--", "../x"], env=dict(os.environ), out=io.StringIO(), err=io.StringIO())

    assert code == 130


def test_refresh_reports_setup_errors(configured, monkeypatch):
    _, target = configured

    def failing(*args, **kwargs):
        raise RuntimeError("boom")

    monkeypatch.setattr(cli, "declared", failing)
    err = io.StringIO()

    code = cli.main(["refresh", str(target)], env=dict(os.environ), out=io.StringIO(), err=err)

    assert code == 2
    assert "worktree-init: boom" in err.getvalue()
