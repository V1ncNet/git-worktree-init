import os
import shutil

from git_worktree_init import worktrees
from helpers import git


def test_discover_from_linked_worktree_finds_main_and_all(repo):
    git(repo, "worktree", "add", "-q", "-b", "a", "../with space")
    linked = (repo.parent / "with space").resolve()

    found = worktrees.discover(linked, os.environ)

    assert found.main == repo
    assert found.common_dir == (repo / ".git").resolve()
    assert found.worktrees == (repo, linked)


def test_discover_skips_prunable(repo):
    git(repo, "worktree", "add", "-q", "-b", "gone", "../gone")
    shutil.rmtree(repo.parent / "gone")

    assert worktrees.discover(repo, os.environ).worktrees == (repo,)


def test_toplevel_from_subdirectory(repo):
    (repo / "sub").mkdir()

    assert worktrees.toplevel(repo / "sub", os.environ) == repo
