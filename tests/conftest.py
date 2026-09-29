import os
import sys

import pytest

from helpers import CORE, git

sys.path.insert(0, str(CORE))

LEAKING = (
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_CONFIG_PARAMETERS",
    "GIT_PREFIX",
    "COMPOSE_PROJECT_NAME",
    "CI",
)


@pytest.fixture(autouse=True)
def isolated_git(monkeypatch, tmp_path):
    home = tmp_path / "home"
    home.mkdir()
    monkeypatch.setenv("HOME", str(home))
    monkeypatch.setenv("GIT_CONFIG_GLOBAL", str(home / ".gitconfig"))
    monkeypatch.setenv("GIT_CONFIG_NOSYSTEM", "1")
    monkeypatch.setenv("GIT_TERMINAL_PROMPT", "0")
    for key in ("GIT_AUTHOR_NAME", "GIT_COMMITTER_NAME"):
        monkeypatch.setenv(key, "Test")
    for key in ("GIT_AUTHOR_EMAIL", "GIT_COMMITTER_EMAIL"):
        monkeypatch.setenv(key, "test@example.invalid")
    for key in list(os.environ):
        if key.startswith("WT_") or key in LEAKING:
            monkeypatch.delenv(key)


@pytest.fixture
def repo(tmp_path):
    main = (tmp_path / "proj").resolve()
    main.mkdir()
    git(main, "init", "-q", "-b", "main")
    (main / ".gitignore").write_text(".env\nlocal/\n.worktreeinclude\n*.log\n")
    git(main, "add", ".gitignore")
    git(main, "commit", "-q", "-m", "Init")
    return main
