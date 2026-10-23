import argparse
import os
import subprocess
import sys
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import TextIO

from git_worktree_init import config, engine, gitenv, worktrees
from git_worktree_init import options as opts
from git_worktree_init import plugin as plugins


@dataclass(frozen=True)
class Command:
    synopsis: str
    with_yes: bool
    with_target: bool


ADD = Command("git wt add [<options>] -- <git-worktree-add-args>...", with_yes=True, with_target=False)
REFRESH = Command("git wt refresh [<options>] [<worktree>]", with_yes=False, with_target=True)
USAGE = f"usage: {ADD.synopsis}\n   or: {REFRESH.synopsis}\n\n"
HELP_FLAGS = ("-h", "--help")
USAGE_ERROR = 129
HELP_COLUMN = 26
YES_HELP = "never prompt, use defaults"


class UsageError(Exception):
    pass


class Parser(argparse.ArgumentParser):
    def error(self, message: str):
        raise UsageError(message)


def main(
    argv: Sequence[str],
    env: Mapping[str, str] | None = None,
    out: TextIO | None = None,
    err: TextIO | None = None,
    help_out: TextIO | None = None,
) -> int:
    env = dict(os.environ if env is None else env)
    out = out or sys.stderr
    err = err or sys.stderr
    help_out = help_out or sys.stdout
    if not argv:
        return _usage_error(err, "need a subcommand", USAGE)
    command, rest = argv[0], list(argv[1:])
    if command in HELP_FLAGS:
        help_out.write(USAGE)
        return USAGE_ERROR
    if command == "hook":
        return _hook(env, out, err)
    if command == "add":
        return _add(rest, env, err, help_out)
    if command == "refresh":
        return _refresh(rest, env, out, err, help_out)
    return _usage_error(err, f"unknown subcommand: `{command}'", USAGE)


def _usage_error(err: TextIO, message: str, usage: str) -> int:
    err.write(f"error: {message}\n{usage}")
    return USAGE_ERROR


def _repair(target: Path) -> str:
    return f"worktree-init: the worktree exists, repair it with: git wt refresh {target}\n"


def _execute(target: Path, mode: str, env: dict[str, str], out: TextIO, err: TextIO) -> int:
    try:
        engine.execute(target, mode, env, out=out)
    except KeyboardInterrupt:
        err.write(f"\nworktree-init: aborted\n{_repair(target)}")
        return 130
    except Exception as error:
        err.write(f"worktree-init: {error}\n{_repair(target)}")
        return 1
    return 0


def _hook(env: dict[str, str], out: TextIO, err: TextIO) -> int:
    try:
        target = worktrees.toplevel(Path.cwd(), gitenv.sanitized(env))
    except (subprocess.CalledProcessError, RuntimeError) as error:
        err.write(f"worktree-init: {error}\n")
        return 1
    return _execute(target, "create", env, out, err)


def declared(cwd: Path, env: Mapping[str, str]) -> tuple[opts.Option, ...]:
    base = gitenv.sanitized(env)
    try:
        repo = worktrees.discover(cwd, base)
    except subprocess.CalledProcessError:
        return opts.BUILTIN
    path = plugins.locate(repo.main, config.read(cwd, base))
    return opts.BUILTIN + (plugins.load(path).options if path else ())


def parser(command: Command, options: Sequence[opts.Option]) -> argparse.ArgumentParser:
    result = Parser(add_help=False, allow_abbrev=False)
    if command.with_yes:
        result.add_argument("--yes", action="store_true")
    for option in options:
        keywords = {"dest": option.name, "default": None}
        if option.kind == "confirm":
            result.add_argument(option.flag, action=argparse.BooleanOptionalAction, **keywords)
        elif option.kind == "slot":
            result.add_argument(option.flag, type=int, **keywords)
        elif option.kind == "choice" and option.choices and not callable(option.choices):
            result.add_argument(option.flag, choices=list(option.choices), **keywords)
        else:
            result.add_argument(option.flag, **keywords)
    if command.with_target:
        result.add_argument("worktree", nargs="?")
    return result


def help_text(command: Command, options: Sequence[opts.Option]) -> str:
    rows = [_row("--yes", YES_HELP)] if command.with_yes else []
    rows += [option_row(option) for option in options]
    return f"usage: {command.synopsis}\n\n" + "\n".join(rows) + "\n\n"


def option_row(option: opts.Option) -> str:
    return _row(_flag_spec(option), option.help)


def _flag_spec(option: opts.Option) -> str:
    if option.kind == "confirm":
        return "--[no-]" + option.flag.removeprefix("--")
    if option.kind == "slot":
        return f"{option.flag} <n>"
    if option.name == "source":
        return f"{option.flag} <path>"
    if option.kind == "choice" and option.choices and not callable(option.choices):
        return f"{option.flag} ({'|'.join(option.choices)})"
    return f"{option.flag} <value>"


def _row(spec: str, text: str) -> str:
    left = "    " + spec
    if len(left) < HELP_COLUMN:
        return (left.ljust(HELP_COLUMN) + text).rstrip()
    return f"{left}\n{' ' * HELP_COLUMN}{text}".rstrip()


def flag_env(options: Sequence[opts.Option], args: argparse.Namespace, cwd: Path) -> dict[str, str]:
    result = {}
    for option in options:
        value = getattr(args, option.name)
        if value is None:
            continue
        if option.kind == "confirm":
            result[option.env_key] = "1" if value else "0"
        elif option.name == "source":
            result[option.env_key] = str((cwd / value).resolve())
        else:
            result[option.env_key] = str(value)
    return result


def _wants_help(argv: Sequence[str]) -> bool:
    options = argv[:argv.index("--")] if "--" in argv else argv
    return any(arg in HELP_FLAGS for arg in options)


def _parse(command: Command, options, argv: Sequence[str], err: TextIO, help_out: TextIO):
    if _wants_help(argv):
        help_out.write(help_text(command, options))
        return None, USAGE_ERROR
    try:
        return parser(command, options).parse_args(argv), 0
    except UsageError as error:
        return None, _usage_error(err, str(error), help_text(command, options))


def _refresh(argv: list[str], env: dict[str, str], out: TextIO, err: TextIO, help_out: TextIO) -> int:
    cwd = Path.cwd()
    guess = cwd
    if argv and not argv[-1].startswith("-") and (cwd / argv[-1]).is_dir():
        guess = cwd / argv[-1]
    try:
        options = declared(guess, env)
    except (plugins.PluginError, RuntimeError, subprocess.CalledProcessError) as error:
        err.write(f"worktree-init: {error}\n")
        return 2
    args, code = _parse(REFRESH, options, argv, err, help_out)
    if args is None:
        return code
    location = (cwd / args.worktree) if args.worktree else cwd
    try:
        target = worktrees.toplevel(location, gitenv.sanitized(env))
    except (subprocess.CalledProcessError, FileNotFoundError, NotADirectoryError):
        err.write(f"worktree-init: not a git worktree: {location}\n")
        return 2
    return _execute(target, "refresh", env | flag_env(options, args, cwd), out, err)


def _add(argv: list[str], env: dict[str, str], err: TextIO, help_out: TextIO) -> int:
    cwd = Path.cwd()
    if "--" in argv:
        split = argv.index("--")
        own, forwarded = argv[:split], argv[split + 1:]
    else:
        own, forwarded = argv, None
    try:
        options = declared(cwd, env)
    except (plugins.PluginError, RuntimeError, subprocess.CalledProcessError) as error:
        err.write(f"worktree-init: {error}\n")
        return 2
    if forwarded is None and not _wants_help(own):
        return _usage_error(err, "missing -- before the git worktree add arguments", help_text(ADD, options))
    args, code = _parse(ADD, options, own, err, help_out)
    if args is None:
        return code
    child = env | flag_env(options, args, cwd)
    if args.yes:
        child["WT_YES"] = "1"
    try:
        return subprocess.run(["git", "worktree", "add", *forwarded], cwd=cwd, env=child).returncode
    except KeyboardInterrupt:
        return 130
