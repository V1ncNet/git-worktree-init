import fcntl
import subprocess
import sys
from collections.abc import Callable, Iterator, Mapping
from contextlib import contextmanager
from pathlib import Path
from typing import TextIO

from git_worktree_init import config, gitenv, include, prompt, slots, worktrees
from git_worktree_init import options as opts
from git_worktree_init import plugin as plugins
from git_worktree_init.context import Context


@contextmanager
def _locked(common_dir: Path) -> Iterator[None]:
    with open(common_dir / "worktree-init.lock", "w") as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        yield


def _no_slot(_: Path) -> None:
    return None


def _source(raw: str | None, repo: worktrees.Repo, env: Mapping[str, str]) -> Path:
    if not raw:
        return repo.main
    path = Path(raw).expanduser()
    if not path.is_absolute():
        raise ValueError(f"source must be an absolute path, e.g. WT_SOURCE=$PWD (got {raw})")
    if not path.is_dir():
        raise ValueError(f"source is not a directory: {path}")
    try:
        other = worktrees.discover(path, env)
    except subprocess.CalledProcessError:
        raise ValueError(f"source is not a git worktree: {path}") from None
    if other.common_dir != repo.common_dir:
        raise ValueError(f"source belongs to another repository: {path}")
    return worktrees.toplevel(path, env)


def execute(
    target: Path,
    mode: str,
    env: Mapping[str, str],
    out: TextIO | None = None,
    tty_opener: Callable[[], prompt.Tty | None] = prompt.open_tty,
) -> None:
    out = out or sys.stderr
    base_env = gitenv.sanitized(env)
    entries = config.read(target, base_env)
    repo = worktrees.discover(target, base_env)
    plugin_path = plugins.locate(repo.main, entries)
    loaded = plugins.load(plugin_path) if plugin_path else None
    run_env = gitenv.sanitized(env, loaded.sanitize_env if loaded else ())
    declared = opts.BUILTIN + (loaded.options if loaded else ())
    slot_of = loaded.slot_of if loaded else _no_slot
    patterns = config.ignore_patterns(entries)
    reserved = slots.parse_reserved(config.last(entries, "reservedSlots"))
    own = slot_of(target) if mode == "refresh" else None
    ctx = Context(
        main=repo.main,
        target=target,
        common_dir=repo.common_dir,
        mode=mode,
        env=run_env,
        worktrees=repo.worktrees,
        out=out,
    )
    explicit_values = opts.explicit(declared, env)
    config_values = opts.from_config(declared, entries)
    answers = {}
    if mode == "create" and opts.interactive_wanted(declared, env, entries):
        tty = tty_opener()
        if tty is not None:
            scanned = [w for w in repo.worktrees if not config.path_ignored(w, patterns)]
            recommended = slots.allocate(slots.used(slot_of, scanned, target), reserved)
            defaults = opts.resolve(declared, {}, {}, config_values)
            if defaults["slot"] is None:
                defaults["slot"] = recommended
            with tty:
                answers = prompt.ask(declared, defaults, prompt.Prompter(tty, tty), ctx)
            if answers.get("slot") == recommended:
                answers.pop("slot")
    values = opts.resolve(declared, explicit_values, answers, config_values)
    with _locked(repo.common_dir):
        repo = worktrees.discover(target, base_env)
        ctx.worktrees = repo.worktrees
        scanned = [w for w in repo.worktrees if not config.path_ignored(w, patterns)]
        used = slots.used(slot_of, scanned, target)
        if values["slot"] is None:
            values["slot"] = own if own is not None else slots.allocate(used, reserved)
        elif values["slot"] != own:
            slots.validate(values["slot"], used, reserved)
        ctx.slot = values["slot"]
        ctx.source = _source(values["source"], repo, base_env)
        ctx.options = values
        print(f"worktree-init: {mode} {target} (slot {ctx.slot}, source {ctx.source})", file=out)
        spec = include.include_file(ctx.source, repo.main)
        for rel in include.included(ctx.source, run_env, spec):
            if ctx.copy(rel, overwrite=mode == "create"):
                ctx.info(f"copied {rel}")
        if loaded:
            loaded.apply(ctx)
    if loaded and loaded.next_steps:
        steps = loaded.next_steps(ctx)
        if steps:
            print("Next steps:", file=out)
            for step in steps:
                print(f"  - {step}", file=out)
