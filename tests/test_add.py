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

    assert result.returncode == 129
    assert not (repo.parent / "a").exists()
    assert git(repo, "branch", "--list", "a").stdout == ""


def test_abbreviated_flag_is_rejected(repo):
    configure(repo)

    result = wt(repo, "add", "--sl", "3", "--", "-b", "a", "../a")

    assert result.returncode == 129
    assert not (repo.parent / "a").exists()


def test_missing_separator_is_usage_error(repo):
    configure(repo)

    unparsed = wt(repo, "add", "-b", "a", "../a")
    missing = wt(repo, "add", "--yes")

    for result in (unparsed, missing):
        assert result.returncode == 129
        assert result.stderr.startswith("error: missing -- before the git worktree add arguments\nusage: git wt add")


def test_help_follows_git_layout_and_lists_plugin_flags(repo):
    configure(repo)

    result = wt(repo, "add", "-h")

    assert result.returncode == 129
    assert result.stdout == (
        "usage: git wt add [<options>] -- <git-worktree-add-args>...\n"
        "\n"
        "    --yes                 never prompt, use defaults\n"
        "    --slot <n>            slot of the worktree\n"
        "    --source <path>       worktree to copy files from\n"
        "    --port-base <value>   first port\n"
        "\n"
    )


def test_long_help_matches_short_help(repo):
    configure(repo)

    assert wt(repo, "add", "--help").stdout == wt(repo, "add", "-h").stdout


def test_help_wins_over_invalid_arguments(repo):
    result = wt(repo, "add", "-h", "--bogus")

    assert result.returncode == 129
    assert result.stdout.startswith("usage: git wt add")
    assert result.stderr == ""


def test_help_flag_after_separator_is_no_help(repo):
    result = wt(repo, "refresh", "--", "-h")

    assert result.stdout == ""
    assert "not a git worktree" in result.stderr


def test_refresh_help_shows_worktree_argument_without_yes(repo):
    result = wt(repo, "refresh", "-h")

    assert result.returncode == 129
    assert result.stdout.startswith("usage: git wt refresh [<options>] [<worktree>]\n\n    --slot <n>")


def test_usage_error_explains_and_prints_help_to_stderr(repo):
    configure(repo)

    result = wt(repo, "add", "--slot", "x", "--", "../a")

    assert result.returncode == 129
    assert result.stderr.startswith("error: argument --slot: invalid int value: 'x'\nusage: git wt add")
    assert result.stdout == ""


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
