from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Any

KINDS = ("confirm", "text", "choice", "slot")


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

