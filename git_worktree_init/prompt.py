from collections.abc import Mapping, Sequence
from typing import Any, TextIO

from git_worktree_init.options import Option


class Tty:
    def __init__(self, reader: TextIO, writer: TextIO):
        self.reader = reader
        self.writer = writer

    def readline(self) -> str:
        return self.reader.readline()

    def write(self, text: str) -> None:
        self.writer.write(text)

    def flush(self) -> None:
        self.writer.flush()

    def __enter__(self) -> "Tty":
        return self

    def __exit__(self, *exc: object) -> bool:
        self.reader.close()
        self.writer.close()
        return False


def open_tty() -> Tty | None:
    try:
        reader = open("/dev/tty", "r", encoding="utf-8")
    except OSError:
        return None
    try:
        writer = open("/dev/tty", "w", encoding="utf-8")
    except OSError:
        reader.close()
        return None
    return Tty(reader, writer)


class Prompter:
    def __init__(self, stream_in, stream_out):
        self.stream_in = stream_in
        self.stream_out = stream_out

    def _read(self, text: str) -> str:
        self.stream_out.write(text)
        self.stream_out.flush()
        line = self.stream_in.readline()
        if line == "":
            raise EOFError
        return line.strip()

    def confirm(self, label: str, default: bool) -> bool:
        hint = "Y/n" if default else "y/N"
        while True:
            answer = self._read(f"? {label} ({hint}) › ").lower()
            if not answer:
                return default
            if answer in ("y", "yes"):
                return True
            if answer in ("n", "no"):
                return False

    def number(self, label: str, default: int) -> int:
        while True:
            answer = self._read(f"? {label} ({default}) › ")
            if not answer:
                return default
            if answer.isdigit():
                return int(answer)

    def text(self, label: str, default: Any) -> str:
        shown = "" if default is None else str(default)
        return self._read(f"? {label} ({shown}) › ") or shown

    def choice(self, label: str, choices: Sequence[str], default: str) -> str:
        index = choices.index(default) + 1 if default in choices else 1
        listing = "  ".join(f"{number}) {name}" for number, name in enumerate(choices, 1))
        while True:
            answer = self._read(f"? {label} ({index})\n  {listing}\n  › ")
            if not answer:
                return choices[index - 1]
            if answer.isdigit() and 1 <= int(answer) <= len(choices):
                return choices[int(answer) - 1]
            if answer in choices:
                return answer


def ask(
    options: Sequence[Option], defaults: Mapping[str, Any], prompter: Prompter, ctx: Any
) -> dict[str, Any]:
    answers: dict[str, Any] = {}
    try:
        for option in options:
            if not option.ask:
                continue
            label = option.help or option.name
            default = defaults[option.name]
            if option.kind == "confirm":
                answers[option.name] = prompter.confirm(label, bool(default))
            elif option.kind == "slot":
                answers[option.name] = prompter.number(label, default)
            elif option.kind == "choice":
                source = option.choices(ctx) if callable(option.choices) else option.choices
                choices = list(source or [])
                if choices:
                    answers[option.name] = prompter.choice(label, choices, default)
            else:
                answers[option.name] = prompter.text(label, default)
    except EOFError:
        pass
    return answers
