import argparse
import os
import subprocess
import sys
from collections.abc import Mapping, Sequence
from pathlib import Path
from typing import TextIO

from git_worktree_init import config, engine, gitenv, worktrees
from git_worktree_init import options as opts
from git_worktree_init import plugin as plugins

USAGE = "usage: git wt {add,refresh} [options]\n"


def main(
    argv: Sequence[str],
    env: Mapping[str, str] | None = None,
    out: TextIO | None = None,
    err: TextIO | None = None,
) -> int:
    env = dict(os.environ if env is None else env)
    out = out or sys.stderr
    err = err or sys.stderr
    if not argv:
        err.write(USAGE)
        return 2
    command, rest = argv[0], list(argv[1:])
    if command == "hook":
        return _hook(env, out, err)
    if command == "add":
        return _add(rest, env, err)
    if command == "refresh":
        return _refresh(rest, env, out, err)
    err.write(USAGE)
    return 2


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


def parser(prog: str, options: Sequence[opts.Option], with_target: bool) -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(prog=prog, allow_abbrev=False)
    result.add_argument("--yes", action="store_true", help="never ask, use defaults")
    for option in options:
        keywords = {"dest": option.name, "default": None, "help": option.help}
        if option.kind == "confirm":
            result.add_argument(option.flag, action=argparse.BooleanOptionalAction, **keywords)
        elif option.kind == "slot":
            result.add_argument(option.flag, type=int, **keywords)
        elif option.kind == "choice" and option.choices and not callable(option.choices):
            result.add_argument(option.flag, choices=list(option.choices), **keywords)
        else:
            result.add_argument(option.flag, **keywords)
    if with_target:
        result.add_argument("worktree", nargs="?")
    return result


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


def _parse(prog: str, options, with_target: bool, argv: Sequence[str]):
    try:
        return parser(prog, options, with_target).parse_args(argv), 0
    except SystemExit as exit_:
        return None, int(exit_.code or 0)


def _refresh(argv: list[str], env: dict[str, str], out: TextIO, err: TextIO) -> int:
    cwd = Path.cwd()
    guess = cwd
    if argv and not argv[-1].startswith("-") and (cwd / argv[-1]).is_dir():
        guess = cwd / argv[-1]
    try:
        options = declared(guess, env)
    except (plugins.PluginError, RuntimeError, subprocess.CalledProcessError) as error:
        err.write(f"worktree-init: {error}\n")
        return 2
    args, code = _parse("git wt refresh", options, True, argv)
    if args is None:
        return code
    location = (cwd / args.worktree) if args.worktree else cwd
    try:
        target = worktrees.toplevel(location, gitenv.sanitized(env))
    except (subprocess.CalledProcessError, FileNotFoundError, NotADirectoryError):
        err.write(f"worktree-init: not a git worktree: {location}\n")
        return 2
    return _execute(target, "refresh", env | flag_env(options, args, cwd), out, err)


def _add(argv: list[str], env: dict[str, str], err: TextIO) -> int:
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
    args, code = _parse("git wt add", options, False, own)
    if args is None:
        return code
    if forwarded is None:
        err.write("usage: git wt add [options] -- <git worktree add arguments>\n")
        return 2
    child = env | flag_env(options, args, cwd)
    if args.yes:
        child["WT_YES"] = "1"
    return subprocess.run(["git", "worktree", "add", *forwarded], cwd=cwd, env=child).returncode
