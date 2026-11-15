# Rust port of git-worktree-init

## Intent

The tool moves from Python to Rust and ships as one binary `git-wt` per platform, attached to GitHub Releases. Three reasons drive it.

- Installing it today means a clone, two shell shims, an alias and an interpreter setting. Installing it afterwards means downloading one file and registering the hooks.
- The hook runs on every checkout of every repository. A native binary starts in milliseconds, so the shell filters that exist only to avoid a Python start can go.
- Rust as a language is a goal of its own.

Releases get real versions. A version lives in `Cargo.toml`, a tag `vX.Y.Z` produces the release, and `git wt --version` prints it.

## Goals

- `git-wt` covers everything the README describes today: `add`, `refresh`, `remove`, `prune`, the `post-checkout` hook, `.worktreeinclude`, `localInclude`, slots, records and sweep, prompts on `/dev/tty`, and the harness with `init`, `sync`, `merge` and its hook.
- Existing Python plugins run unchanged. The file keeps `OPTIONS`, `SANITIZE_ENV`, `slot_of`, `apply`, `next_steps` and `teardown`, and `from git_worktree_init.options import Option` keeps working.
- A repository without `worktree-init.plugin` needs no Python at all.
- A tag push builds and publishes the binaries for `aarch64-darwin`, `x86_64-linux` and `aarch64-linux`.
- `brew install` installs the binary from a formula in this repository.
- The Nix flake keeps working and builds the binary from source.

## Non-goals

- New features or changed behavior. The README stays the contract, apart from the install section and the requirements.
- Windows and `x86_64-darwin`.
- A separate Homebrew tap repository, crates.io, an installer script or self-update.
- Automatic version bumps from commit messages. Commits in this repository follow Chris Beams, not Conventional Commits.

## Anti-goals

The port fails, even if it works, when:

- an existing plugin needs an edit to keep working,
- an ordinary checkout or clone in an unrelated repository gets slower or prints anything,
- a harness hook failure aborts a `git push` or a `reference-transaction`,
- a message moves from stderr to stdout.

## Constraints

- Rust edition 2024, at least Rust 1.89, because `std::fs::File::lock` replaces `flock`.
- The only crates are `serde_json` for records and the bridge protocol, and `libc` for the two things the standard library cannot do. One is handing the bridge extra file descriptors through `pre_exec`, because Rust opens every descriptor with `CLOEXEC`. The other is a `SIGINT` handler, so Ctrl-C at a prompt prints the repair message as today. Argument parsing, glob matching, the lock and the terminal prompt use the standard library. A further crate needs a reason in the commit that adds it.
- Help and usage errors follow Git, as today: lowercase help phrases without a final period, exit code 129, help on stdout and everything else on stderr.
- The tool's own Git calls and every plugin subprocess run without `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE` and `GIT_PREFIX`. The forwarded `git worktree add` and `git worktree remove` keep the caller's environment.
- Every commit compiles and passes `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check` and the remaining pytest suite.
- `README.md`, `docs/architecture.md` and `CLAUDE.md` change in the same commit as the behavior they describe.

## Approaches

### Plugin bridge

Plugins are Python files loaded in-process today. A Rust binary cannot do that, and every existing plugin must keep working.

- **A. Embedded Python bridge with `ctx` in Python.** Chosen. The binary embeds a small Python package and starts it with the configured interpreter. Lock, slots, records, options and prompts stay in Rust. The `ctx` helpers stay in Python, nearly unchanged from today's `context.py`. This needs the least code. About 200 lines of Python stay in the repository.
- **B. Bridge as a thin proxy with `ctx` in Rust.** Every `ctx` call goes back to the binary over the protocol. Almost everything is Rust, but the protocol grows by one message per helper without changing any behavior.
- **Rejected.** PyO3 links libpython, so the binary would depend on one Python build. An embedded scripting language such as Lua breaks every plugin.

### Releases

- **1. Tag-driven workflow.** Chosen. A push of `vX.Y.Z` starts a GitHub Actions workflow that checks the tag against `Cargo.toml`, builds three targets and publishes archives with SHA-256 checksums. The maintainer decides when a release happens.
- **2. cargo-dist.** It generates the workflow, an installer script and a Homebrew formula. The formula is the only part needed, it fits in about 30 lines, and the generated CI code would need maintenance of its own.
- **Rejected.** release-please and semantic-release need Conventional Commits.

## Design

### One binary

`git-wt` replaces `wti.py`, `hook.sh`, `harness-hook.sh` and `bin/git-wt`. On `PATH`, Git runs it for `git wt` without an alias, and the working directory is already the caller's, so the `GIT_PREFIX` handling of the alias goes away. Git answers `git wt --help` by looking for a man page, so `-h` stays the documented help.

Subcommands visible to users: `add`, `refresh`, `remove`, `prune`, `harness init`, `harness sync`, `harness merge` and `--version`. Subcommands for hooks: `hook` for `post-checkout` and `harness-hook` for the three harness events. The hook subcommands do not appear in the usage text, as today.

Registration in the README becomes, with `<bin>` the absolute path of the binary:

```sh
git config --global hook.worktree-init.command 'f="<bin>"; [ -x "$f" ] || exit 0; exec "$f" hook'
git config --global --add hook.worktree-init.event post-checkout
git config --global hook.harness.command 'f="<bin>"; [ -x "$f" ] || exit 0; exec "$f" harness-hook'
git config --global --add hook.harness.event post-checkout
git config --global --add hook.harness.event pre-push
git config --global --add hook.harness.event reference-transaction
```

- The path is absolute, because IDEs start Git with a short `PATH`, and a hook that cannot start would print an error on every checkout of every repository.
- Both commands guard against a missing binary. `reference-transaction` and `pre-push` abort Git when their hook cannot run, and the exit code of `post-checkout` becomes the exit code of `git checkout`.
- The commands do not pass `"$@"`. Git runs a command with shell metacharacters as `sh -c '<command> "$@"'` and appends the arguments itself. A probe with Git 2.56 confirmed this. An explicit `"$@"` would double them and break the event detection by argument count.

### Hook filters

`git-wt hook` runs the same filters as `hook.sh`, in the same order, and exits 0 silently when one matches. The first two filters only look at the arguments, so an ordinary checkout costs one process start and no Git call. The binary may read files such as `.git` directly instead of calling `git rev-parse` where the result is identical. `docs/architecture.md` "Shim filters" describes the new cost.

`git-wt harness-hook` keeps the rules of `harness-hook.sh`. It derives the event from the number of arguments, reads stdin for `reference-transaction`, exits early when `GIT_DIR` points into a `harness.git` or when no `harness.git` exists, and always exits 0. A failure prints `worktree-init: harness hook failed, <event> continues`.

### Python bridge

The bridge runs only when `worktree-init.plugin` is set. The interpreter comes from `WT_PYTHON`, then `worktree-init.python`, then `python3`, and must be 3.12 or newer, as today. The binary starts one bridge process per command and keeps it for the whole run, because option `choices` can be a function of the `Context` and is called during the prompt.

- The bridge sources are embedded in the binary. The caller's working directory must not reach `sys.path`, as today's `wti.py` guarantees. `PYTHONPATH` and user site-packages keep working as today, because a plugin may import a package installed there. The bridge therefore runs without `-I` and removes the working directory from `sys.path` before its first import.
- `git_worktree_init.options.Option` keeps its constructor and attributes. `git_worktree_init.context.Context` keeps every member listed in the README.
- Requests and replies are JSON lines. The requests are `load`, `choices`, `slot_of`, `apply`, `next_steps` and `teardown`. `load` returns the options and `SANITIZE_ENV`. Each call that takes a `ctx` receives its fields.
- The bridge keeps one `Context` instance for the whole run and only updates its fields per request. Today `apply` and `next_steps` see the same object, and a plugin may set attributes in `apply` that `next_steps` reads.
- Option values cross the protocol as JSON, typed as today: `confirm` as boolean, `slot` as integer, `text` and `choice` as string, an unset value as `null`. A default that JSON cannot represent fails `load` with an error naming the option. Today such a default already fails when the record is written.
- The bridge runs with the sanitized environment, and `ctx.run` sanitizes as today, including `SANITIZE_ENV`.
- The protocol uses its own file descriptors, not fd 0 and fd 1. Plugin `print()` and the output of `ctx.run` children keep going to the inherited stdout and stderr, exactly as today.
- An exception in the plugin becomes the same error message and exit code as today. Ctrl-C at a prompt still aborts and leaves the worktree in place.
- `apply` and `teardown` run while Rust holds the lock.

`examples/minimal_plugin.py` keeps working without a change and is the bridge's end-to-end fixture. The two plugins known to be in use today use `SANITIZE_ENV`, `ctx.target`, `ctx.main`, `ctx.info` and `ctx.run`, and catch `subprocess.CalledProcessError` from `ctx.run`. They import nothing from `git_worktree_init`.

### State on disk

The binary reads and writes the same files as the Python version: the records under `<git-common-dir>/worktree-init/records/` with the token file `worktree-init-record`, the lock `<git-common-dir>/worktree-init.lock`, and everything under `harness.git`. A worktree prepared by the Python version gets its teardown from the binary after an upgrade. `File::lock` uses `flock` on Unix, so both versions exclude each other during a port that runs them side by side. A test writes a record in today's format and checks that the binary tears it down.

### Versions and releases

- `Cargo.toml` holds the version. `git wt --version` prints `git-wt version <version>`, in the format of `git --version`.
- A tag `vX.Y.Z` on `main` starts `.github/workflows/release.yml`. It fails when the tag does not match `Cargo.toml`, builds `aarch64-apple-darwin`, `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`, and creates a GitHub Release with `git-wt-<version>-<target>.tar.gz` and a `.sha256` file per archive.
- The musl targets give static Linux binaries that run on any distribution.
- A release is a commit that bumps `Cargo.toml` and `Cargo.lock`, followed by a tag. The README describes the steps in a short "Release" section.
- Upgrading from 0.1 means registering the hooks again. The README gets a section "Upgrading from 0.1" with the commands. No compatibility shims for `hook.sh` or `harness-hook.sh` ship.
- The first Rust release is `v0.2.0`. The Python version was `0.1.0` and was never tagged, and the changed installation is a breaking change that a `0.x` minor bump allows.

### Homebrew

`Formula/git-wt.rb` in this repository installs the release binary for macOS arm64 and Linux. This repository acts as its own tap:

```sh
brew tap <owner>/git-worktree-init https://github.com/<owner>/git-worktree-init
brew install <owner>/git-worktree-init/git-wt
```

- The formula downloads the release archive and checks its SHA-256. It does not build from source and does not depend on Python, because Python is only needed for a plugin.
- The release workflow writes version and checksums into the formula once the release exists, and commits it to `main` as `Update Homebrew formula to <version>`. The workflow needs `contents: write`. A protected `main` that rejects this push fails the job, while the release itself stays published.
- The formula prints the hook registration as `caveats`, with `<bin>` replaced by its own path under `HOMEBREW_PREFIX`.
- `brew test` runs `git-wt --version`.

### CI

`.github/workflows/ci.yml` runs on every push and pull request, on `macos-latest` (arm64) and `ubuntu-latest` (x86_64):

- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`, including the end-to-end tests against real Git repositories, with Git 2.54 or newer
- the bridge's pytest suite with Python 3.12
- `nix flake check` on Linux, so the flake keeps building

`ubuntu-latest` ships an older Git, so the job installs a current one before testing. `aarch64-linux` is built in the release workflow but not tested in CI.

### Nix

`flake.nix` builds with `rustPlatform.buildRustPackage` from the committed `Cargo.lock`. `mainProgram` stays `git-wt`, and the version comes from `Cargo.toml`.

- The README promises today that the Nix package falls back to the Python of its nixpkgs input, so `worktree-init.python` is optional. The promise stays. The build passes the interpreter path through a compile-time environment variable, and the binary uses it in place of `python3` as the last fallback.
- The home-manager example adds the package to `home.packages`, so `git wt` finds `git-wt` on `PATH`, and points the hooks at `${pkg}/bin/git-wt` instead of `${pkg}/hook.sh`. `alias.wt` is gone.
- A `devShells.default` provides `cargo`, `rustc`, `clippy`, `rustfmt` and Python 3.12 for development. Development runs in `nix develop`, so it does not depend on a global `rustup` setup.

### Tests

Most of today's pytest files import the Python modules directly, so they go along with the modules. Their cases become Rust tests.

- Tests of internal modules run against the library `git_wt` from `tests/`, or sit next to the code where that is simpler.
- End-to-end tests in `tests/` drive the built binary against real Git repositories in temporary directories, with the same isolation as `conftest.py` today: own `HOME`, `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM=1` and no `WT_*` or leaking Git variables.
- The prompt test needs a pseudo terminal. The builder may keep it in pytest with `pty` if Rust needs a crate for it.
- The bridge keeps a pytest suite for `Context` and plugin loading.

Every behavior covered by a test today stays covered.

### Repository after the port

- `Cargo.toml`, `Cargo.lock`, `src/` and `tests/` for Rust.
- The bridge under one directory, for example `bridge/git_worktree_init/`, with its tests.
- `examples/minimal_plugin.py`, `flake.nix`, `README.md`, `docs/architecture.md`, `CLAUDE.md` and `.github/workflows/`.
- `wti.py`, `hook.sh`, `harness-hook.sh`, `bin/` and the rest of the Python package are gone.

`CLAUDE.md` names the new commands and constraints. "Runtime code uses the Python standard library only" applies to the bridge, and the crate rule above applies to Rust.

## Left to the builder

- The module layout of `src/` and the order of the port, as long as each commit leaves the tool working end to end. Porting module by module behind the Python CLI, or building the Rust CLI next to it and switching over in one commit, are both fine.
- How the bridge sources reach the interpreter, for example `-c` with an import hook or a version-keyed cache directory.
- Which file descriptors carry the protocol.
- The glob matcher for `ignorePaths` and the `copy_tree` excludes, as long as it matches `fnmatch` and shell `case` semantics on the cases the tests cover.
- The exact wording of new messages, following the existing `worktree-init:` prefix.
