import os

from git_worktree_init import gitenv


def test_sanitized_drops_git_variables_and_extras():
    env = {
        "GIT_DIR": "x",
        "GIT_WORK_TREE": "x",
        "GIT_INDEX_FILE": "x",
        "GIT_PREFIX": "sub/",
        "COMPOSE_PROJECT_NAME": "leak",
        "GIT_CONFIG_PARAMETERS": "'a.b'='c'",
        "PATH": "/bin",
    }

    result = gitenv.sanitized(env, ["COMPOSE_PROJECT_NAME"])

    assert result == {"GIT_CONFIG_PARAMETERS": "'a.b'='c'", "PATH": "/bin"}


def test_git_returns_stdout(repo):
    assert gitenv.git(repo, "rev-parse", "--abbrev-ref", "HEAD", env=os.environ).strip() == "main"


def test_run_passes_input(repo):
    assert gitenv.run(["cat"], repo, os.environ, input="hello").stdout == "hello"
