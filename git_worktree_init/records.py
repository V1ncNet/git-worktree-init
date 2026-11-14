import json
import os
import secrets
from dataclasses import dataclass
from pathlib import Path
from typing import Any

TOKEN = "worktree-init-record"


@dataclass(frozen=True)
class Record:
    key: str
    admin: str
    slot: int | None
    target: Path
    options: dict[str, Any]


def _directory(common_dir: Path) -> Path:
    return common_dir / "worktree-init" / "records"


def _admin_dir(common_dir: Path, admin: str) -> Path:
    return common_dir / "worktrees" / admin


def _key_of(admin_dir: Path) -> str | None:
    token = admin_dir / TOKEN
    return token.read_text(encoding="utf-8").strip() if token.is_file() else None


def _read(path: Path) -> Record:
    data = json.loads(path.read_text(encoding="utf-8"))
    return Record(path.stem, data["admin"], data["slot"], Path(data["target"]), data["options"])


def all_records(common_dir: Path) -> list[Record]:
    directory = _directory(common_dir)
    return [_read(path) for path in sorted(directory.glob("*.json"))] if directory.is_dir() else []


def find(common_dir: Path, admin_dir: Path) -> Record | None:
    key = _key_of(admin_dir)
    path = _directory(common_dir) / f"{key}.json"
    return _read(path) if key and path.is_file() else None


def write(
    common_dir: Path, admin_dir: Path, slot: int | None, target: Path, options: dict[str, Any]
) -> None:
    key = _key_of(admin_dir) or secrets.token_hex(8)
    directory = _directory(common_dir)
    directory.mkdir(parents=True, exist_ok=True)
    data = {"admin": admin_dir.name, "slot": slot, "target": str(target), "options": options}
    staged = directory / f"{key}.json.tmp"
    staged.write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    os.replace(staged, directory / f"{key}.json")
    (admin_dir / TOKEN).write_text(key + "\n", encoding="utf-8")


def discard(common_dir: Path, record: Record) -> None:
    (_directory(common_dir) / f"{record.key}.json").unlink(missing_ok=True)


def alive(common_dir: Path, record: Record) -> bool:
    admin_dir = _admin_dir(common_dir, record.admin)
    if _key_of(admin_dir) != record.key:
        return False
    if (admin_dir / "locked").exists():
        return True
    gitdir = admin_dir / "gitdir"
    return gitdir.is_file() and (admin_dir / gitdir.read_text(encoding="utf-8").strip()).exists()
