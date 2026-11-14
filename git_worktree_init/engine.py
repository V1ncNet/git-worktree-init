import fcntl
import subprocess
import sys
from collections.abc import Callable, Iterator, Mapping
from contextlib import contextmanager
from pathlib import Path
from typing import TextIO

from git_worktree_init import config, gitenv, include, prompt, records, slots, worktrees
from git_worktree_init import options as opts
from git_worktree_init import plugin as plugins
from git_worktree_init.context import Context


@contextmanager
def locked(common_dir: Path) -> Iterator[None]:
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
    loaded = _load(repo, entries)
    run_env = gitenv.sanitized(env, loaded.sanitize_env if loaded else ())
    declared = opts.BUILTIN + (loaded.options if loaded else ())
    slot_of = loaded.slot_of if loaded else _no_slot
    patterns = config.ignore_patterns(entries)
    reserved = slots.parse_reserved(config.last(entries, "reservedSlots"))
    record = records.find(repo.common_dir, repo.admin_dir) if repo.admin_dir else None
    own = slot_of(target)
    if own is None and record:
        own = record.slot
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
            recommended = slots.allocate(slots.used(slot_of, scanned, target) | _pending(repo, record), reserved)
            defaults = opts.resolve(declared, {}, {}, config_values)
            if defaults["slot"] is None:
                defaults["slot"] = recommended
            with tty:
                answers = prompt.ask(declared, defaults, prompt.Prompter(tty, tty), ctx)
            if answers.get("slot") == recommended:
                answers.pop("slot")
    values = opts.resolve(declared, explicit_values, answers, config_values)
    with locked(repo.common_dir):
        repo = worktrees.discover(target, base_env)
        ctx.worktrees = repo.worktrees
        if loaded:
            sweep(repo, loaded, run_env, out)
        scanned = [w for w in repo.worktrees if not config.path_ignored(w, patterns)]
        used = slots.used(slot_of, scanned, target) | _pending(repo, record)
        if values["slot"] is None:
            values["slot"] = own if own is not None else slots.allocate(used, reserved)
        elif values["slot"] != own:
            slots.validate(values["slot"], used, reserved)
        ctx.slot = values["slot"]
        ctx.source = _source(values["source"], repo, base_env)
        ctx.options = values
        if loaded and repo.admin_dir:
            records.write(repo.common_dir, repo.admin_dir, ctx.slot, target, values)
        print(f"worktree-init: {mode} {target} (slot {ctx.slot}, source {ctx.source})", file=out)
        spec = include.include_file(ctx.source, repo.main)
        overwrite = mode == "create" and own is None
        for rel in include.included(ctx.source, run_env, spec):
            if ctx.copy(rel, overwrite=overwrite):
                ctx.info(f"copied {rel}")
        for rel in config.all_values(entries, "localInclude"):
            if ctx.copy(rel, source=repo.main, overwrite=False):
                ctx.info(f"copied {rel}")
        if loaded:
            loaded.apply(ctx)
    if loaded and loaded.next_steps:
        steps = loaded.next_steps(ctx)
        if steps:
            print("Next steps:", file=out)
            for step in steps:
                print(f"  - {step}", file=out)


def _load(repo: worktrees.Repo, entries: config.Entries) -> plugins.Plugin | None:
    path = plugins.locate(repo.main, entries)
    return plugins.load(path) if path else None


def _pending(repo: worktrees.Repo, own: records.Record | None) -> dict[int, Path]:
    return {
        record.slot: record.target
        for record in records.all_records(repo.common_dir)
        if record.slot is not None and (own is None or record.key != own.key)
    }


def teardown_context(
    repo: worktrees.Repo,
    target: Path,
    mode: str,
    slot: int | None,
    options: dict,
    env: dict[str, str],
    out: TextIO,
) -> Context:
    return Context(
        main=repo.main,
        target=target,
        common_dir=repo.common_dir,
        mode=mode,
        env=env,
        worktrees=repo.worktrees,
        slot=slot,
        options=options,
        out=out,
    )


def sweep(repo: worktrees.Repo, loaded: plugins.Plugin, env: dict[str, str], out: TextIO) -> bool:
    clean = True
    for record in records.all_records(repo.common_dir):
        if records.alive(repo.common_dir, record):
            continue
        if loaded.teardown:
            print(f"worktree-init: prune {record.target} (slot {record.slot})", file=out)
            try:
                loaded.teardown(
                    teardown_context(repo, record.target, "prune", record.slot, record.options, env, out)
                )
            except Exception as error:
                print(f"worktree-init: teardown failed: {error}", file=out)
                print("worktree-init: the record is kept, retry with: git wt prune", file=out)
                clean = False
                continue
        records.discard(repo.common_dir, record)
    return clean


def prune(cwd: Path, env: Mapping[str, str], out: TextIO) -> bool:
    base_env = gitenv.sanitized(env)
    repo = worktrees.discover(cwd, base_env)
    loaded = _load(repo, config.read(cwd, base_env))
    if not loaded:
        return True
    with locked(repo.common_dir):
        repo = worktrees.discover(cwd, base_env)
        return sweep(repo, loaded, gitenv.sanitized(env, loaded.sanitize_env), out)
