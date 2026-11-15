import subprocess
from pathlib import Path

BRIDGE = Path(__file__).resolve().parent.parent
ROOT = BRIDGE.parent
EXAMPLE_PLUGIN = ROOT / "examples" / "minimal_plugin.py"


def git(cwd, *args, env=None, check=True):
    return subprocess.run(
        ["git", *args], cwd=cwd, env=env, check=check, text=True, capture_output=True
    )
