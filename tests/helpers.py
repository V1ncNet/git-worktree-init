import os
import subprocess
import sys
from pathlib import Path

CORE = Path(__file__).resolve().parent.parent
HOOK = CORE / "hook.sh"
GIT_WT = CORE / "bin" / "git-wt"
WTI = CORE / "wti.py"
EXAMPLE_PLUGIN = CORE / "examples" / "minimal_plugin.py"


def git(cwd, *args, env=None, check=True):
    return subprocess.run(
        ["git", *args], cwd=cwd, env=env, check=check, text=True, capture_output=True
    )


def register_hook(main):
    git(main, "config", "hook.worktree-init.command", str(HOOK))
    git(main, "config", "--add", "hook.worktree-init.event", "post-checkout")
    git(main, "config", "worktree-init.python", sys.executable)


def prompting_env(**extra):
    env = {key: value for key, value in os.environ.items() if key != "GIT_TERMINAL_PROMPT"}
    env.update(extra)
    return env
