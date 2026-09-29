from collections.abc import Mapping, Sequence
from pathlib import Path

from git_worktree_init.gitenv import git, run


def include_file(source: Path, main: Path) -> Path:
    own = source / ".worktreeinclude"
    return own if own.is_file() else main / ".worktreeinclude"


def patterns(spec: Path) -> tuple[list[str], list[str]]:
    positive, negative = [], []
    for raw in spec.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("!"):
            negative.append(line[1:])
        else:
            positive.append(line)
    return positive, negative


CHUNK = 500


def _matching(
    source: Path, env: Mapping[str, str], selectors: Sequence[str], paths: Sequence[str] = ()
) -> list[str]:
    args = ["ls-files", "-z", "--others", "--ignored", *(f"--exclude={p}" for p in selectors)]
    if not paths:
        return [path for path in git(source, *args, env=env).split("\0") if path]
    result = []
    for start in range(0, len(paths), CHUNK):
        chunk = paths[start:start + CHUNK]
        output = git(source, *args, "--", *(f":(literal){p}" for p in chunk), env=env)
        result += [path for path in output.split("\0") if path]
    return result


def included(source: Path, env: Mapping[str, str], spec: Path | None = None) -> list[str]:
    spec = spec or source / ".worktreeinclude"
    if not spec.is_file():
        return []
    positive, negative = patterns(spec)
    if not positive:
        return []
    candidates = _matching(source, env, positive)
    if candidates and negative:
        dropped = set(_matching(source, env, negative, candidates))
        candidates = [path for path in candidates if path not in dropped]
    if not candidates:
        return []
    checked = run(
        ["git", "check-ignore", "-z", "--stdin"],
        source,
        env,
        check=False,
        input="\0".join(candidates) + "\0",
    )
    return sorted(path for path in checked.stdout.split("\0") if path)
