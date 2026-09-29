import os
import subprocess
from pathlib import Path

import pytest

from git_worktree_init import config
from helpers import git


def test_read_returns_scoped_lowercase_entries(repo):
    git(repo, "config", "worktree-init.reservedSlots", "0 2")
    git(repo, "config", "--add", "worktree-init.ignorePaths", "*/a/*")
    git(repo, "config", "--add", "worktree-init.ignorePaths", "*/b/*")
    env = dict(os.environ, GIT_CONFIG_PARAMETERS="'worktree-init.clone-db'='x'")

    entries = config.read(repo, env)

    assert config.last(entries, "reservedSlots") == "0 2"
    assert config.all_values(entries, "ignorePaths") == ["*/a/*", "*/b/*"]
    assert config.last(entries, "clone-db") == "x"
    assert config.command_line_keys(entries) == {"clone-db"}


def test_read_without_entries_is_empty(repo):
    assert config.read(repo, os.environ) == {}


def test_ignore_patterns_default_to_claude_worktrees():
    assert config.ignore_patterns({}) == ["*/.claude/worktrees/*"]


def test_path_ignored_matches_directory_with_spaces():
    patterns = config.ignore_patterns({})

    assert config.path_ignored(Path("/r/.claude/worktrees/my agent"), patterns)
    assert not config.path_ignored(Path("/r/wt"), patterns)


def test_read_failure_is_raised(repo, monkeypatch):
    monkeypatch.setattr(
        config,
        "run",
        lambda *args, **kwargs: subprocess.CompletedProcess([], 128, "", "boom"),
    )

    with pytest.raises(RuntimeError, match="boom"):
        config.read(repo, os.environ)
