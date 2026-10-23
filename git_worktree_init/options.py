from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from typing import Any

from git_worktree_init import config

KINDS = ("confirm", "text", "choice", "slot")
TRUE = {"1", "true", "yes", "y", "on"}
FALSE = {"0", "false", "no", "n", "off"}


class OptionError(ValueError):
    pass


@dataclass(frozen=True)
class Option:
    name: str
    kind: str
    default: Any = None
    help: str = ""
    choices: Sequence[str] | Callable[[Any], Sequence[str]] | None = None
    ask: bool = True

    @property
    def env_key(self) -> str:
        return "WT_" + self.name.upper()

    @property
    def config_key(self) -> str:
        return self.name.replace("_", "-").lower()

    @property
    def flag(self) -> str:
        return "--" + self.name.replace("_", "-")


BUILTIN = (
    Option("slot", "slot", None, "slot of the worktree"),
    Option("source", "text", None, "worktree to copy files from", ask=False),
)


def parse(option: Option, raw: str) -> Any:
    if option.kind == "confirm":
        lowered = raw.strip().lower()
        if lowered in TRUE:
            return True
        if lowered in FALSE:
            return False
        raise OptionError(f"{option.name}: expected yes or no, got {raw!r}")
    if option.kind == "slot":
        if not raw.strip().isdigit():
            raise OptionError(f"{option.name}: expected a non-negative number, got {raw!r}")
        return int(raw)
    return raw


def explicit(options: Sequence[Option], env: Mapping[str, str]) -> dict[str, Any]:
    return {o.name: parse(o, env[o.env_key]) for o in options if o.env_key in env}


def from_config(options: Sequence[Option], entries: config.Entries) -> dict[str, Any]:
    values = {}
    for option in options:
        raw = config.last(entries, option.config_key)
        if raw is not None:
            values[option.name] = parse(option, raw)
    return values


def interactive_wanted(
    options: Sequence[Option], env: Mapping[str, str], entries: config.Entries
) -> bool:
    if env.get("WT_YES", "") not in ("", "0"):
        return False
    if env.get("GIT_TERMINAL_PROMPT") == "0" or env.get("CI"):
        return False
    setting = config.last(entries, "interactive")
    if setting is not None and setting.strip().lower() in FALSE:
        return False
    if any(o.env_key in env for o in options):
        return False
    command_line = config.command_line_keys(entries)
    return not any(o.config_key in command_line for o in options)


def resolve(
    options: Sequence[Option],
    explicit_values: Mapping[str, Any],
    answers: Mapping[str, Any],
    config_values: Mapping[str, Any],
) -> dict[str, Any]:
    result = {}
    for option in options:
        for layer in (explicit_values, answers, config_values):
            if option.name in layer:
                result[option.name] = layer[option.name]
                break
        else:
            result[option.name] = option.default
    return result
