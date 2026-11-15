# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

- Rust tests: `nix develop -c cargo test`. Single test: `nix develop -c cargo test --test hook <name>`.
- Bridge tests: `uvx pytest` (pytest is not installed globally). It needs `target/debug/git-wt`, so run `cargo build` first.
- Full check: `nix develop -c sh -c 'cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo build && cargo test && uvx pytest -q'`, then `nix flake check`.
- Requires Git 2.54+ (config-based hooks). Python 3.12+ is needed only for plugins and the bridge tests.

## Constraints

- The only crates are `serde_json` and `libc`. A further crate needs a reason in the commit that adds it.
- The bridge under `bridge/git_worktree_init/` uses the Python standard library only. Never add a dependency to `pyproject.toml`.
- `git-wt hook` and `git-wt harness-hook` run on every checkout of every repository. Don't add process spawns to the early filters in `src/hook.rs` and `src/cli.rs` (see "Hook filters" in `docs/architecture.md`).
- The bridge removes the caller's working directory from `sys.path` before its first import (see "Python bridge" in `docs/architecture.md`).
- All messages go to stderr. Only help output (`-h`) goes to stdout.
- Help and usage errors follow Git. Help phrases are lowercase without a final period, and the exit code is 129.
- The tool's own Git calls and every plugin subprocess run without `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE` and `GIT_PREFIX` (see `src/gitenv.rs`). The `git worktree add` forwarded by `git wt add` keeps the caller's environment on purpose.

## Docs

User-facing behavior is specified in `README.md` and design decisions in `docs/architecture.md`. Update them in the same commit as a behavior change.
