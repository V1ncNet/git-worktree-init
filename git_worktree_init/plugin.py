import hashlib
import importlib.util
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from git_worktree_init import config
from git_worktree_init.options import BUILTIN, KINDS, Option


class PluginError(RuntimeError):
    pass


@dataclass(frozen=True)
class Plugin:
    path: Path
    options: tuple[Option, ...]
    sanitize_env: tuple[str, ...]
    slot_of: Callable[[Path], int | None]
    apply: Callable[[Any], None]
    next_steps: Callable[[Any], list[str]] | None


def locate(main: Path, entries: config.Entries) -> Path | None:
    raw = config.last(entries, "plugin")
    if not raw:
        return None
    path = Path(raw).expanduser()
    return path if path.is_absolute() else (main / path).resolve()


def load(path: Path) -> Plugin:
    if not path.is_file():
        raise PluginError(f"plugin not found: {path}")
    name = "worktree_init_plugin_" + hashlib.sha1(str(path).encode()).hexdigest()[:12]
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    try:
        spec.loader.exec_module(module)
    except Exception as error:
        sys.modules.pop(name, None)
        raise PluginError(f"{path}: {error}") from error
    for required in ("slot_of", "apply"):
        if not callable(getattr(module, required, None)):
            raise PluginError(f"{path}: missing function {required}")
    options = tuple(getattr(module, "OPTIONS", ()))
    builtin_names = {option.name for option in BUILTIN}
    for option in options:
        if not isinstance(option, Option):
            raise PluginError(f"{path}: OPTIONS must contain Option instances")
        if option.kind not in KINDS:
            raise PluginError(f"{path}: option {option.name} has unknown kind {option.kind}")
        if option.name in builtin_names:
            raise PluginError(f"{path}: option {option.name} is reserved")
    next_steps = getattr(module, "next_steps", None)
    return Plugin(
        path=path,
        options=options,
        sanitize_env=tuple(getattr(module, "SANITIZE_ENV", ())),
        slot_of=module.slot_of,
        apply=module.apply,
        next_steps=next_steps if callable(next_steps) else None,
    )
