import subprocess
from collections.abc import Iterable, Mapping, Sequence
from pathlib import Path

GIT_VARIABLES = ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX")


def sanitized(env: Mapping[str, str], extra: Iterable[str] = ()) -> dict[str, str]:
    dropped = set(GIT_VARIABLES) | set(extra)
    return {key: value for key, value in env.items() if key not in dropped}


def run(
    cmd: Sequence[str],
    cwd: Path,
    env: Mapping[str, str],
    check: bool = True,
    capture: bool = True,
    input: str | None = None,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(cmd),
        cwd=cwd,
        env=dict(env),
        check=check,
        text=True,
        capture_output=capture,
        input=input,
    )


def git(cwd: Path, *args: str, env: Mapping[str, str], check: bool = True) -> str:
    return run(["git", *args], cwd, env, check=check).stdout
