import subprocess

from helpers import EXAMPLE_PLUGIN, GIT_WT, git, register_hook


def wt(cwd, *args, env=None):
    return subprocess.run(
        [str(GIT_WT), *args], cwd=cwd, env=env, text=True, capture_output=True
    )


def configure(repo):
    register_hook(repo)
    git(repo, "config", "worktree-init.plugin", str(EXAMPLE_PLUGIN))
    git(repo, "config", "worktree-init.reservedSlots", "0")


def test_flags_become_options(repo):
    configure(repo)

    result = wt(repo, "add", "--slot", "7", "--port-base", "9000", "--", "-b", "a", "../a")

    assert result.returncode == 0
    assert (repo.parent / "a" / ".env").read_text() == "SLOT=7\nPORT=9007\n"


def test_unknown_flag_creates_nothing(repo):
    configure(repo)

    result = wt(repo, "add", "--bogus", "--", "-b", "a", "../a")

    assert result.returncode == 2
    assert not (repo.parent / "a").exists()
    assert git(repo, "branch", "--list", "a").stdout == ""


def test_abbreviated_flag_is_rejected(repo):
    configure(repo)

    result = wt(repo, "add", "--sl", "3", "--", "-b", "a", "../a")

    assert result.returncode == 2
    assert not (repo.parent / "a").exists()


def test_missing_separator_is_usage_error(repo):
    configure(repo)

    assert wt(repo, "add", "-b", "a", "../a").returncode == 2


def test_help_lists_plugin_flags(repo):
    configure(repo)

    result = wt(repo, "add", "--help")

    assert result.returncode == 0
    assert "--port-base" in result.stdout


def test_git_exit_code_is_passed_through(repo):
    configure(repo)
    (repo.parent / "busy").mkdir()
    (repo.parent / "busy" / "file").write_text("x")

    assert wt(repo, "add", "--", "../busy").returncode == 128


def test_alias_resolves_paths_from_subdirectory(repo):
    configure(repo)
    git(repo, "config", "--global", "alias.wt", f"!{GIT_WT}")
    (repo / "sub").mkdir()

    result = git(repo / "sub", "wt", "add", "--yes", "--", "-b", "p", "../../prefixed", check=False)

    assert result.returncode == 0
    assert (repo.parent / "prefixed" / ".env").is_file()


def test_relative_source_is_made_absolute(repo):
    configure(repo)
    (repo / "sub").mkdir()

    result = wt(repo / "sub", "add", "--source", "..", "--", "-b", "s", "../../s")

    assert result.returncode == 0
    assert f"source {repo})" in result.stderr


def test_refresh_reads_flags_from_target_repository(repo, tmp_path):
    configure(repo)
    wt(repo, "add", "--yes", "--", "-b", "a", "../a")
    elsewhere = tmp_path / "elsewhere"
    elsewhere.mkdir()

    result = wt(elsewhere, "refresh", "--port-base", "9100", str(repo.parent / "a"))

    assert result.returncode == 0
    assert (repo.parent / "a" / ".env").read_text() == "SLOT=1\nPORT=9101\n"
