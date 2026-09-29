import os

from git_worktree_init import include
from helpers import git


def test_only_ignored_matches_are_included(repo):
    (repo / ".env").write_text("A=1")
    (repo / "notes.txt").write_text("untracked, not ignored")
    (repo / ".worktreeinclude").write_text(".env\nnotes.txt\n.gitignore\n")

    assert include.included(repo, os.environ) == [".env"]


def test_negations_apply_at_any_depth(repo):
    deep = repo / "local" / "sub" / "deep"
    (repo / "local" / "shelf").mkdir(parents=True)
    deep.mkdir(parents=True)
    for rel in ("a.xml", "workspace.xml", "shelf/s.xml", "sub/deep/x.corrupted.1", "sub/deep/keep.xml", "sub/.DS_Store"):
        (repo / "local" / rel).write_text(rel)
    (repo / ".worktreeinclude").write_text(
        "# comment\nlocal/\n!local/workspace.xml\n!local/shelf/\n!*.corrupted.*\n!.DS_Store\n"
    )

    assert include.included(repo, os.environ) == ["local/a.xml", "local/sub/deep/keep.xml"]


def test_negation_wins_regardless_of_order(repo):
    (repo / "local").mkdir()
    (repo / "local" / "a.xml").write_text("a")
    (repo / "local" / "b.xml").write_text("b")
    (repo / ".worktreeinclude").write_text("!local/a.xml\nlocal/\n")

    assert include.included(repo, os.environ) == ["local/b.xml"]


def test_missing_file_includes_nothing(repo):
    assert include.included(repo, os.environ) == []


def test_many_candidates_with_negations(repo):
    many = repo / "local" / "many"
    many.mkdir(parents=True)
    for number in range(3000):
        (many / f"file-with-a-long-name-{number:05d}.xml").write_text("x")
    (many / "drop.corrupted.1").write_text("c")
    (repo / ".worktreeinclude").write_text("local/\n!*.corrupted.*\n")

    result = include.included(repo, os.environ)

    assert len(result) == 3000
    assert "local/many/drop.corrupted.1" not in result


def test_source_without_list_uses_main_list(repo):
    (repo / ".worktreeinclude").write_text(".env\n")
    git(repo, "worktree", "add", "-q", "-b", "src", "../src")
    source = (repo.parent / "src").resolve()
    (source / ".env").write_text("FROM_SOURCE=1")

    spec = include.include_file(source, repo)

    assert spec == repo / ".worktreeinclude"
    assert include.included(source, os.environ, spec) == [".env"]
