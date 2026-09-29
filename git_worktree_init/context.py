import filecmp
import os
import shutil
import subprocess
import sys
from collections.abc import Sequence
from dataclasses import dataclass, field
from fnmatch import fnmatchcase
from pathlib import Path
from typing import Any, TextIO

from git_worktree_init import gitenv


@dataclass
class Context:
    main: Path
    target: Path
    common_dir: Path
    mode: str
    env: dict[str, str]
    worktrees: tuple[Path, ...] = ()
    source: Path | None = None
    slot: int | None = None
    options: dict[str, Any] = field(default_factory=dict)
    out: TextIO = field(default_factory=lambda: sys.stderr)

    @property
    def name(self) -> str:
        return self.target.name

    def option(self, name: str) -> Any:
        return self.options[name]

    def info(self, message: str) -> None:
        print(f"  {message}", file=self.out)

    def run(
        self,
        cmd: Sequence[str],
        cwd: Path | None = None,
        check: bool = True,
        capture: bool = False,
    ) -> subprocess.CompletedProcess[str]:
        return gitenv.run(cmd, cwd or self.target, self.env, check=check, capture=capture)

    def link(self, rel: str, anchor: Path) -> bool:
        source = anchor / rel
        destination = self.target / rel
        if not source.exists():
            self.info(f"skipped {rel} (missing in {anchor})")
            return False
        if destination.is_symlink():
            current = Path(os.readlink(destination))
            if current == source:
                return False
            if destination.exists():
                self.info(f"kept {rel} (links to {current})")
                return False
            destination.unlink()
            destination.symlink_to(source)
            self.info(f"relinked {rel} -> {source} (was dangling)")
            return True
        if destination.exists():
            self.info(f"kept {rel} (already present)")
            return False
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.symlink_to(source)
        self.info(f"linked {rel} -> {source}")
        return True

    def copy(self, rel: str, source: Path | None = None, overwrite: bool = True) -> bool:
        origin = (source or self.source) / rel
        if origin.is_dir():
            return self.copy_tree(rel, source=source)
        if not origin.is_file():
            return False
        return self._copy_file(origin, self.target / rel, overwrite)

    def copy_tree(
        self, rel: str, excludes: Sequence[str] = (), source: Path | None = None
    ) -> bool:
        root = (source or self.source) / rel
        changed = False
        for directory, subdirectories, files in root.walk():
            relative = directory.relative_to(root)
            subdirectories[:] = [
                d for d in subdirectories if not _excluded(relative / d, excludes)
            ]
            for name in files:
                if _excluded(relative / name, excludes):
                    continue
                changed |= self._copy_file(
                    directory / name, self.target / rel / relative / name, True
                )
        return changed

    def write(self, rel: str, text: str, backup: bool = False) -> bool:
        destination = self.target / rel
        if destination.is_file() and destination.read_text(encoding="utf-8") == text:
            return False
        if backup and destination.is_file():
            shutil.copy2(destination, destination.with_name(destination.name + ".bak"))
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_text(text, encoding="utf-8")
        return True

    def remove(self, rel: str) -> bool:
        path = self.target / rel
        if not path.is_file() and not path.is_symlink():
            return False
        path.unlink()
        return True

    def ensure_ignored(self, rel: str) -> bool:
        checked = self.run(["git", "check-ignore", "-q", rel], check=False, capture=True)
        if checked.returncode == 0:
            return False
        exclude = self.common_dir / "info" / "exclude"
        existing = exclude.read_text(encoding="utf-8").splitlines() if exclude.exists() else []
        if rel in existing:
            return False
        exclude.parent.mkdir(parents=True, exist_ok=True)
        with exclude.open("a", encoding="utf-8") as handle:
            handle.write(f"\n{rel}\n")
        self.info(f"added {rel} to info/exclude")
        return True

    def _copy_file(self, origin: Path, destination: Path, overwrite: bool) -> bool:
        if destination.is_file():
            if not overwrite or filecmp.cmp(origin, destination, shallow=False):
                return False
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(origin, destination)
        return True


def _excluded(relative: Path, patterns: Sequence[str]) -> bool:
    for pattern in patterns:
        if pattern.startswith("/"):
            if fnmatchcase(relative.as_posix(), pattern.strip("/")):
                return True
        elif fnmatchcase(relative.name, pattern.rstrip("/")):
            return True
    return False
