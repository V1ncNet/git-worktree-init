"""Line-based JSON bridge between the git-wt binary and a Python plugin."""

import hashlib
import importlib.util
import json
import os
import sys
from pathlib import Path
from typing import Any

from git_worktree_init.context import Context
from git_worktree_init.options import BUILTIN, KINDS, Option

PATH_FIELDS = ("main", "target", "common_dir")


class BridgeError(RuntimeError):
    pass


def import_plugin(path: Path) -> Any:
    if not path.is_file():
        raise BridgeError(f"plugin not found: {path}")
    name = "worktree_init_plugin_" + hashlib.sha1(str(path).encode()).hexdigest()[:12]
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    try:
        spec.loader.exec_module(module)
    except Exception as error:
        sys.modules.pop(name, None)
        raise BridgeError(f"{path}: {error}") from error
    return module


def validated_options(module: Any, path: Path) -> tuple[Option, ...]:
    for required in ("slot_of", "apply"):
        if not callable(getattr(module, required, None)):
            raise BridgeError(f"{path}: missing function {required}")
    options = tuple(getattr(module, "OPTIONS", ()))
    builtin_names = {option.name for option in BUILTIN}
    for option in options:
        if not isinstance(option, Option):
            raise BridgeError(f"{path}: OPTIONS must contain Option instances")
        if option.kind not in KINDS:
            raise BridgeError(f"{path}: option {option.name} has unknown kind {option.kind}")
        if option.name in builtin_names:
            raise BridgeError(f"{path}: option {option.name} is reserved")
        try:
            json.dumps(option.default)
        except (TypeError, ValueError) as error:
            raise BridgeError(
                f"{path}: option {option.name} has a default that is not JSON"
            ) from error
    return options


def describe(option: Option) -> dict[str, Any]:
    dynamic = callable(option.choices)
    return {
        "name": option.name,
        "kind": option.kind,
        "default": option.default,
        "help": option.help,
        "choices": None if option.choices is None or dynamic else list(option.choices),
        "dynamic": dynamic,
        "ask": option.ask,
    }


def context_fields(raw: dict[str, Any]) -> dict[str, Any]:
    fields = dict(raw)
    for key in PATH_FIELDS:
        fields[key] = Path(fields[key])
    fields["worktrees"] = tuple(Path(entry) for entry in fields["worktrees"])
    if fields.get("source") is not None:
        fields["source"] = Path(fields["source"])
    return fields


class Bridge:
    def __init__(self) -> None:
        self.module: Any = None
        self.options: dict[str, Option] = {}
        self.context: Context | None = None

    def handle(self, request: dict[str, Any]) -> Any:
        return getattr(self, "op_" + request["op"])(request)

    def op_load(self, request: dict[str, Any]) -> dict[str, Any]:
        path = Path(request["path"])
        module = import_plugin(path)
        options = validated_options(module, path)
        self.module = module
        self.options = {option.name: option for option in options}
        return {
            "options": [describe(option) for option in options],
            "sanitize_env": list(getattr(module, "SANITIZE_ENV", ())),
            "next_steps": callable(getattr(module, "next_steps", None)),
            "teardown": callable(getattr(module, "teardown", None)),
        }

    def op_slot_of(self, request: dict[str, Any]) -> int | None:
        return self.module.slot_of(Path(request["worktree"]))

    def op_choices(self, request: dict[str, Any]) -> list[str]:
        choices = self.options[request["option"]].choices
        return [str(choice) for choice in choices(self.run_context(request["ctx"]))]

    def op_apply(self, request: dict[str, Any]) -> None:
        self.module.apply(self.run_context(request["ctx"]))

    def op_next_steps(self, request: dict[str, Any]) -> list[str]:
        return [str(step) for step in self.module.next_steps(self.context)]

    def op_teardown(self, request: dict[str, Any]) -> None:
        self.module.teardown(Context(**context_fields(request["ctx"])))

    def run_context(self, raw: dict[str, Any]) -> Context:
        fields = context_fields(raw)
        if self.context is None:
            self.context = Context(**fields)
        else:
            self.context.__dict__.update(fields)
        return self.context


def reply(request: dict[str, Any], bridge: Bridge) -> dict[str, Any]:
    try:
        return {"ok": bridge.handle(request)}
    except Exception as error:
        return {"error": str(error)}


def serve(source, sink) -> int:
    bridge = Bridge()
    for line in source:
        request = json.loads(line)
        if request["op"] == "exit":
            return 0
        sink.write(json.dumps(reply(request, bridge)) + "\n")
        sink.flush()
    return 0


def main(fd_in: int = 3, fd_out: int = 4) -> int:
    os.set_inheritable(fd_in, False)
    os.set_inheritable(fd_out, False)
    with (
        os.fdopen(fd_in, "r", encoding="utf-8", closefd=False) as source,
        os.fdopen(fd_out, "w", encoding="utf-8", closefd=False) as sink,
    ):
        try:
            return serve(source, sink)
        except KeyboardInterrupt:
            return 130
