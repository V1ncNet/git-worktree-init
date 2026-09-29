from collections.abc import Mapping
from dataclasses import dataclass
from fnmatch import fnmatchcase
from pathlib import Path

from git_worktree_init.gitenv import run

PREFIX = "worktree-init."
DEFAULT_IGNORE_PATHS = ("*/.claude/worktrees/*",)


@dataclass(frozen=True)
class Entry:
    scope: str
    value: str


type Entries = dict[str, list[Entry]]


def read(cwd: Path, env: Mapping[str, str]) -> Entries:
    result = run(
        ["git", "config", "--show-scope", "--get-regexp", r"^worktree-init\."],
        cwd,
        env,
        check=False,
    )
    if result.returncode not in (0, 1):
        raise RuntimeError(f"git config failed ({result.returncode}): {result.stderr.strip()}")
    entries: Entries = {}
    for line in result.stdout.splitlines():
        scope, _, rest = line.partition("\t")
        key, _, value = rest.partition(" ")
        entries.setdefault(key[len(PREFIX):], []).append(Entry(scope, value))
    return entries


def last(entries: Entries, key: str) -> str | None:
    values = entries.get(key.lower())
    return values[-1].value if values else None


def all_values(entries: Entries, key: str) -> list[str]:
    return [entry.value for entry in entries.get(key.lower(), [])]


def command_line_keys(entries: Entries) -> set[str]:
    return {
        key for key, values in entries.items() if any(e.scope == "command" for e in values)
    }


def ignore_patterns(entries: Entries) -> list[str]:
    return [p for p in all_values(entries, "ignorePaths") if p] or list(DEFAULT_IGNORE_PATHS)


def path_ignored(path: Path, patterns: list[str]) -> bool:
    candidate = f"{path}/"
    return any(fnmatchcase(candidate, pattern) for pattern in patterns)
