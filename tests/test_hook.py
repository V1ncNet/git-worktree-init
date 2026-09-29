import os
import subprocess
import sys
import time
from pathlib import Path

from helpers import EXAMPLE_PLUGIN, GIT_WT, HOOK, git, register_hook


def add(repo, *args, env=None):
    return git(repo, "worktree", "add", *args, env=env, check=False)


def configure(repo, plugin=EXAMPLE_PLUGIN):
    register_hook(repo)
    git(repo, "config", "worktree-init.plugin", str(plugin))
    git(repo, "config", "worktree-init.reservedSlots", "0")


def test_unconfigured_repo_starts_no_python(repo):
    register_hook(repo)

    result = add(repo, "-b", "a", "../a")

    assert result.returncode == 0
    assert "worktree-init" not in result.stdout + result.stderr


def test_include_only_repo_copies_files_into_path_with_spaces(repo):
    register_hook(repo)
    (repo / ".env").write_text("A=1\n")
    (repo / "local").mkdir()
    (repo / "local" / "x.txt").write_text("x")
    (repo / ".worktreeinclude").write_text(".env\nlocal/\n")

    result = add(repo, "-b", "s", "../with space")

    target = repo.parent / "with space"
    assert result.returncode == 0
    assert (target / ".env").read_text() == "A=1\n"
    assert (target / "local" / "x.txt").read_text() == "x"


def test_plugin_runs_and_allocates_consecutive_slots(repo):
    configure(repo)

    first = add(repo, "-b", "a", "../a")
    second = add(repo, "-b", "b", "../b")

    assert first.returncode == second.returncode == 0
    assert (repo.parent / "a" / ".env").read_text() == "SLOT=1\nPORT=8001\n"
    assert (repo.parent / "b" / ".env").read_text() == "SLOT=2\nPORT=8002\n"
    assert "Start the app on port 8001" in first.stderr


def test_add_from_linked_worktree_uses_main_as_source(repo):
    configure(repo)
    add(repo, "-b", "a", "../a")
    linked = repo.parent / "a"
    (linked / "sub").mkdir()

    result = git(linked / "sub", "worktree", "add", "-b", "c", "../../c", check=False)

    assert result.returncode == 0
    assert f"source {repo})" in result.stderr


def test_checkout_clone_and_ignored_paths_do_nothing(repo, tmp_path):
    configure(repo)
    add(repo, "-b", "a", "../a")
    home = Path(os.environ["HOME"])
    git(home, "config", "--global", "hook.worktree-init.command", str(HOOK))
    git(home, "config", "--global", "--add", "hook.worktree-init.event", "post-checkout")
    git(home, "config", "--global", "worktree-init.plugin", str(EXAMPLE_PLUGIN))

    switched = git(repo.parent / "a", "switch", "-c", "a2", check=False)
    cloned = git(tmp_path, "clone", "-q", str(repo), "clone", check=False)
    ignored = add(repo, "-b", "agent", ".claude/worktrees/agent 1")

    for result in (switched, cloned, ignored):
        assert result.returncode == 0
        assert "worktree-init" not in result.stdout + result.stderr


def test_failing_plugin_keeps_worktree_and_explains(repo, tmp_path):
    broken = tmp_path / "broken.py"
    broken.write_text("def slot_of(w):\n    return None\n\ndef apply(ctx):\n    raise RuntimeError('boom')\n")
    configure(repo, broken)

    result = add(repo, "-b", "a", "../a")

    assert result.returncode == 1
    assert (repo.parent / "a").is_dir()
    assert "boom" in result.stderr
    assert "git wt refresh" in result.stderr


def test_python_can_be_chosen(repo, tmp_path):
    configure(repo)
    marker = tmp_path / "marker"
    wrapper = tmp_path / "python-wrapper"
    wrapper.write_text(f'#!/bin/sh\necho used > "{marker}"\nexec "{sys.executable}" "$@"\n')
    wrapper.chmod(0o755)
    git(repo, "config", "worktree-init.python", str(wrapper))

    result = add(repo, "-b", "a", "../a")

    assert result.returncode == 0
    assert marker.read_text() == "used\n"


def test_refresh_twice_changes_nothing(repo):
    configure(repo)
    add(repo, "-b", "a", "../a")
    target = repo.parent / "a"
    subprocess.run([str(GIT_WT), "refresh", str(target)], cwd=repo, check=True, capture_output=True)
    before = (target / ".env").stat().st_mtime_ns
    time.sleep(0.01)

    subprocess.run([str(GIT_WT), "refresh", str(target)], cwd=repo, check=True, capture_output=True)

    assert (target / ".env").stat().st_mtime_ns == before
    assert not (target / ".env.bak").exists()


def test_parallel_adds_get_distinct_slots(repo, tmp_path):
    slow = tmp_path / "slow.py"
    slow.write_text(EXAMPLE_PLUGIN.read_text().replace(
        "def apply(ctx) -> None:\n", "def apply(ctx) -> None:\n    import time\n    time.sleep(0.5)\n"
    ))
    configure(repo, slow)

    processes = [
        subprocess.Popen(
            ["git", "worktree", "add", "-b", name, f"../{name}"],
            cwd=repo,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        for name in ("p", "q")
    ]
    for process in processes:
        process.communicate()

    slots = {(repo.parent / name / ".env").read_text().splitlines()[0] for name in ("p", "q")}
    assert slots == {"SLOT=1", "SLOT=2"}
