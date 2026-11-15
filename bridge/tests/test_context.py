import io
import os

from git_worktree_init.context import Context
from helpers import git


def make(repo, tmp_path):
    target = tmp_path / "target"
    target.mkdir()
    return Context(
        main=repo,
        target=target,
        common_dir=repo / ".git",
        mode="create",
        env=dict(os.environ),
        source=repo,
        out=io.StringIO(),
    )


def test_write_is_idempotent_and_backs_up_on_change(repo, tmp_path):
    ctx = make(repo, tmp_path)

    assert ctx.write(".env", "A=1\n", backup=True)
    assert not ctx.write(".env", "A=1\n", backup=True)
    assert not (ctx.target / ".env.bak").exists()
    assert ctx.write(".env", "A=2\n", backup=True)
    assert (ctx.target / ".env.bak").read_text() == "A=1\n"


def test_link_creates_once_and_keeps_foreign_files(repo, tmp_path):
    ctx = make(repo, tmp_path)
    anchor = tmp_path / "anchor"
    anchor.mkdir()
    (anchor / "CLAUDE.md").write_text("x")
    (ctx.target / "own.md").write_text("mine")
    (anchor / "own.md").write_text("theirs")

    assert ctx.link("CLAUDE.md", anchor)
    assert not ctx.link("CLAUDE.md", anchor)
    assert not ctx.link("own.md", anchor)
    assert not ctx.link("missing.md", anchor)
    assert (ctx.target / "CLAUDE.md").resolve() == anchor / "CLAUDE.md"
    assert (ctx.target / "own.md").read_text() == "mine"


def test_link_replaces_dangling_and_keeps_live_foreign_symlink(repo, tmp_path):
    ctx = make(repo, tmp_path)
    anchor = tmp_path / "anchor"
    anchor.mkdir()
    (anchor / "a.md").write_text("a")
    (anchor / "b.md").write_text("b")
    other = tmp_path / "other.md"
    other.write_text("o")
    (ctx.target / "a.md").symlink_to(tmp_path / "vanished.md")
    (ctx.target / "b.md").symlink_to(other)

    assert ctx.link("a.md", anchor)
    assert not ctx.link("b.md", anchor)
    assert (ctx.target / "a.md").resolve() == anchor / "a.md"
    assert (ctx.target / "b.md").resolve() == other
    assert "links to" in ctx.out.getvalue()


def test_copy_skips_identical_and_respects_overwrite(repo, tmp_path):
    ctx = make(repo, tmp_path)
    (repo / "a.log").write_text("1")

    assert ctx.copy("a.log")
    assert not ctx.copy("a.log")
    (repo / "a.log").write_text("2")
    assert not ctx.copy("a.log", overwrite=False)
    assert ctx.copy("a.log")
    assert (ctx.target / "a.log").read_text() == "2"


def test_copy_tree_applies_anchored_and_floating_excludes(repo, tmp_path):
    ctx = make(repo, tmp_path)
    tree = repo / "local"
    (tree / "shelf").mkdir(parents=True)
    (tree / "sub").mkdir()
    (tree / "keep.xml").write_text("k")
    (tree / "shelf" / "x").write_text("x")
    (tree / "sub" / "a.corrupted.1").write_text("c")
    (tree / "sub" / "b.xml").write_text("b")

    ctx.copy_tree("local", excludes=("/shelf/", "*.corrupted.*"))

    copied = sorted(p.relative_to(ctx.target).as_posix() for p in ctx.target.rglob("*") if p.is_file())
    assert copied == ["local/keep.xml", "local/sub/b.xml"]


def test_ensure_ignored_appends_once(repo, tmp_path):
    git(repo, "worktree", "add", "-q", "-b", "w", "../w")
    ctx = make(repo, tmp_path)
    ctx.target = (repo.parent / "w").resolve()

    assert ctx.ensure_ignored("generated.yml")
    assert not ctx.ensure_ignored("generated.yml")
    assert not ctx.ensure_ignored(".env")
    assert (repo / ".git" / "info" / "exclude").read_text().count("generated.yml") == 1


def test_remove_reports_change(repo, tmp_path):
    ctx = make(repo, tmp_path)
    (ctx.target / "x").write_text("x")

    assert ctx.remove("x")
    assert not ctx.remove("x")
