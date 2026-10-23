# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

- Tests: `uvx pytest` (pytest is not installed globally). Single test: `uvx pytest tests/test_engine.py -k <name>`.
- Requires Git 2.54+ (config-based hooks) and Python 3.12+.

## Constraints

- Runtime code uses the Python standard library only. Never add a dependency to `pyproject.toml`.
- `hook.sh` runs on every checkout of every repository. It stays POSIX `sh` and must exit before starting Python unless all filters pass. Don't add process spawns to the early filters (see "Shim filters" in `docs/architecture.md`).
- The hook entry point is `python3 wti.py`, never `python -m`, to keep the caller's working directory off `sys.path`.
- All messages go to stderr. Only help output (`-h`) goes to stdout.
- Help and usage errors follow Git. Help phrases are lowercase without a final period, and the exit code is 129.
- The tool's own Git calls and every plugin subprocess run without `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE` and `GIT_PREFIX` (see `gitenv.py`). The `git worktree add` forwarded by `git wt add` keeps the caller's environment on purpose.

## Docs

User-facing behavior is specified in `README.md` and design decisions in `docs/architecture.md`. Update them in the same commit as a behavior change.
