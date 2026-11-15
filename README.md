# git-worktree-init

## What it does

A `post-checkout` hook prepares every new linked worktree. It copies the files listed in `.worktreeinclude` and, optionally, calls a plugin for project-specific work such as allocating a slot, writing an `.env` or cloning a database. A plugin can also release that work again once the worktree is gone.

## Requirements

- Git 2.54 or newer (config-based hooks)
- Python 3.12 or newer, only for a plugin. The plugin needs no packages.

The interpreter for a plugin can be set with `git config worktree-init.python <path>` or the environment variable `WT_PYTHON`, for example for IDEs that start Git with a short `PATH`.

## Install

`git-wt` is a single binary. Each release on GitHub has an archive `git-wt-<version>-<target>.tar.gz` for `aarch64-apple-darwin`, `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`, with a `.sha256` file next to it. The archive also holds `LICENSE` and `README.md`. Copy only `git-wt` onto `PATH`. Git then runs it for `git wt`:

```sh
tar -xzf git-wt-<version>-<target>.tar.gz
install -m 755 git-wt ~/.local/bin/
```

With Homebrew, the repository acts as its own tap. The formula installs the release binary for macOS arm64 and Linux and prints the hook registration as caveats.

```sh
brew tap V1ncNet/git-worktree-init https://github.com/V1ncNet/git-worktree-init
brew install V1ncNet/git-worktree-init/git-wt
```

macOS quarantines an archive downloaded in a browser and refuses to start the binary. Clear the flag once:

```sh
xattr -d com.apple.quarantine git-wt
```

Register the hook with the absolute path of the binary. `<bin>` stands for that path, for example `/Users/me/.local/bin/git-wt`.

```sh
git config --global hook.worktree-init.command 'f="<bin>"; [ -x "$f" ] || exit 0; exec "$f" hook'
git config --global --add hook.worktree-init.event post-checkout
```

To use the harness, register its hook for three events as well:

```sh
git config --global hook.harness.command 'f="<bin>"; [ -x "$f" ] || exit 0; exec "$f" harness-hook'
git config --global --add hook.harness.event post-checkout
git config --global --add hook.harness.event pre-push
git config --global --add hook.harness.event reference-transaction
```

The path is absolute, because IDEs start Git with a short `PATH`. A hook that cannot start prints an error on every checkout of every repository. Both commands check that the binary exists and exit with 0 if it does not. `reference-transaction` and `pre-push` abort Git when their hook cannot run. The exit code of `post-checkout` becomes the exit code of `git checkout`. Without the check, a deleted or moved binary would block every commit, fetch and push on the machine. The commands do not pass `"$@"`, because Git appends the hook arguments itself.

The harness hook does nothing in a repository without `harness.git` and never fails the Git command it runs in.

Any config file that the global config includes works as well, for example `--file ~/.config/git/local` with `[include] path = ~/.config/git/local` in the global config.

`core.hooksPath` is deliberately not used, because it switches off every hook in `.git/hooks`. Keep `hook.jobs` at 1 so hooks of the same event run one after another.

To switch the hook off in one repository:

```sh
git config hook.worktree-init.enabled false
```

Nothing happens for a repository that has no `worktree-init.plugin`, no `worktree-init.localInclude` and no `.worktreeinclude` in its main worktree.

### Nix

The flake `github:V1ncNet/git-worktree-init` provides `packages.<system>.default` for `aarch64-darwin`, `x86_64-linux` and `aarch64-linux`. The package builds the binary from source. A plugin falls back to the Python of its nixpkgs input, which makes `worktree-init.python` optional. With home-manager:

```nix
let
  pkg = inputs.git-worktree-init.packages.${pkgs.stdenv.hostPlatform.system}.default;
in
{
  home.packages = [ pkg ];

  programs.git.settings.hook = {
    worktree-init = {
      command = ''f="${pkg}/bin/git-wt"; [ -x "$f" ] || exit 0; exec "$f" hook'';
      event = "post-checkout";
    };
    harness = {
      command = ''f="${pkg}/bin/git-wt"; [ -x "$f" ] || exit 0; exec "$f" harness-hook'';
      event = [ "post-checkout" "pre-push" "reference-transaction" ];
    };
  };
}
```

`home.packages` puts `git-wt` on `PATH`, so `git wt` works without an alias.

## Upgrading from 0.1

Version 0.1 ran from a clone of this repository. Its registration points at `hook.sh` and `harness-hook.sh`, which no longer exist, so every checkout fails until the hooks are registered again. No compatibility shims ship.

A Nix or home-manager config that still uses `${pkg}/hook.sh` points at a file that no longer exists after the upgrade. Every checkout fails until you switch to the example under "Nix".

Install the binary as described under "Install". Then replace both commands in the config file that holds the old registration. Version 0.1 registered into `~/.config/git/local`:

```sh
git config --file ~/.config/git/local hook.worktree-init.command 'f="<bin>"; [ -x "$f" ] || exit 0; exec "$f" hook'
git config --file ~/.config/git/local hook.harness.command 'f="<bin>"; [ -x "$f" ] || exit 0; exec "$f" harness-hook'
```

The events stay as they are. Drop the alias, because Git finds `git-wt` on `PATH`. Drop `worktree-init.python` too, unless a repository uses a plugin and `python3` on `PATH` is older than 3.12.

```sh
git config --file ~/.config/git/local --unset alias.wt
git config --file ~/.config/git/local --unset worktree-init.python
```

If you registered with `--global` instead, use `--global` in place of `--file ~/.config/git/local` in all four commands. `--unset` exits with 5 when the key is not set in that file. That exit code only means there is nothing to drop.

## Use

```sh
git worktree add -b feature/foo ../proj.foo
git wt add --slot 3 -- -b feature/foo ../proj.foo
git -c worktree-init.slot=3 worktree add -b feature/foo ../proj.foo
WT_SLOT=3 git worktree add -b feature/foo ../proj.foo
git wt refresh ../proj.foo
git wt remove ../proj.foo
git wt prune
```

- `git worktree add` runs the hook and prompts for options when a terminal is available. With `--no-checkout` Git does not run `post-checkout`, so initialize such a worktree with `git wt refresh`.
- `git wt add [<options>] -- <git-worktree-add-args>...` turns the flags into `WT_*` variables for that one invocation and forwards everything after `--` to `git worktree add`. The exit code of `git worktree add` is returned. Flags cannot be abbreviated.
- `git -c worktree-init.<key>=<value>` and `WT_<NAME>` set an option without `git wt`.
- `git wt refresh [<options>] [<worktree>]` re-runs the preparation for an existing worktree, the current one by default. It keeps the existing slot and does not overwrite copied files that differ.
- `git wt remove [-f | --force] <worktree>` calls the plugin's `teardown` while the files still exist, then forwards to `git worktree remove`. See "Teardown".
- `git wt prune` calls the plugin's `teardown` for every recorded worktree that no longer exists. See "Teardown".

`git wt -h`, `git wt add -h` and `git wt refresh -h` show the usage and the flags, including those of the plugin. Git answers `git wt --help` itself by looking for a man page, so use `-h`. Help and usage errors exit with 129, like Git. `git wt --version` prints the version.

To copy from the current worktree instead of the main worktree, pass `--source .` or set `WT_SOURCE=$PWD`. A relative `--source` is made absolute by `git wt`.

## Options

Every option has a flag, an environment variable and a config key. Underscores in the option name become hyphens in flag and config key, so `clone_db` is `--clone-db`, `WT_CLONE_DB` and `worktree-init.clone-db`.

| Option | Flag | Env | Config | Prompt |
| --- | --- | --- | --- | --- |
| `slot` | `--slot <n>` | `WT_SLOT` | `worktree-init.slot` | yes |
| `source` | `--source <path>` | `WT_SOURCE` | `worktree-init.source` | no |

Plugins add their own options. Boolean options also accept `--no-<name>`.

Resolution order, highest first:

1. Flag or environment variable
2. Answer at the prompt
3. Config
4. Default of the option

`--slot` rejects a slot that another worktree already holds and a slot listed in `reservedSlots`. Without a slot, the lowest free one is taken. `source` must be an absolute path to a worktree of the same repository and defaults to the main worktree.

## Interactive mode

Options are asked for at a terminal, through `/dev/tty`, when a worktree is created. No prompt appears and the defaults apply silently when any of these holds:

- `--yes` or `WT_YES=1`
- `worktree-init.interactive` is `false`
- `GIT_TERMINAL_PROMPT=0`
- `CI` is set
- any option is already given as `WT_<NAME>` variable or through `git -c`
- no terminal can be opened

## `.worktreeinclude`

The format is the one of Claude Code. Each line is a pattern in `.gitignore` syntax, blank lines and lines starting with `#` are skipped. A file is copied only if it is untracked, ignored by Git and matched by a pattern.

- Negation (`!pattern`) applies at any depth and independent of the order of the lines. This differs from `.gitignore`.
- The file is read from the source worktree. If it is missing there, the one of the main worktree is used.
- To carry `.worktreeinclude` itself into every worktree, list it in the file.

Private files that must not be named in a tracked file go into the local config instead, one path per value:

```sh
git config --add worktree-init.localInclude CLAUDE.local.md
```

Each listed path is copied from the main worktree, even with `--source`. An existing file in the new worktree is never overwritten, and a path missing in the main worktree is skipped.

## Harness

The harness is a second repository under `<git-common-dir>/harness.git` for files that must never reach the code remote, such as `CLAUDE.md`, `.claude/`, `.superpowers/` and `docs/superpowers/`. Every worktree gets its own harness branch and index on top of the same files. Create it once in the main worktree:

```sh
git wt harness init --remote <url>
```

This adds the remote `harness`, writes the allowlist to `harness.git/info/exclude` and turns signing, the global hooks and the global excludes file off for harness operations. The lines of a `# claude-code-runtime` block in `.git/info/exclude` are excluded as well.

With the hook registered (see Install), the harness follows branch checkouts. A branch that exists in the harness, locally or on the remote `harness`, is resumed. A new one forks from the harness branch of its base: `branch.<name>.t3-merge-base`, else the upstream branch if it has another name, else `develop`. Before switching, uncommitted harness changes are committed as `Snapshot harness state`. A snapshot skips files ignored by a `.gitignore` inside the harness and `.DS_Store`, and it refuses to run when every harness file is missing, for example after `git clean -xfd`. A checkout that leaves HEAD detached, such as a rebase, changes nothing in the harness.

Every `git push` takes a snapshot and pushes the current harness branch to `harness`, never with `--force`. Harness fetch and push use ssh in batch mode with a ten second connect timeout unless `GIT_SSH_COMMAND` is set, so a missing network or a locked key fails fast instead of hanging. If that fails, a warning `worktree-init: harness push failed: <reason>` appears and the code push continues. `git wt harness sync` does the same by hand and exits with 1 if the push fails.

When a fetch moves `origin/develop`, every submitted feature is merged into the harness `develop` of the main worktree with `--no-ff`. A feature counts as submitted when its code branch tip is contained in `origin/develop` but is not on its first-parent chain, its code branch is checked out in no worktree, and its harness branch has commits that harness `develop` lacks. The merge drops the ledgers under `.superpowers/sdd/` and the handoffs under `.claude/handoffs/` that the feature added. A conflict stops the merge in the main worktree, and no further merge or snapshot happens there until it is resolved. `git wt harness merge <branch>` merges a harness branch by hand without these checks, for example for a fix pushed directly to `develop`. Never run `git worktree` against `harness.git`, because `remove` and `move` would act on the Git directory of a code worktree.

## Configuration keys

All keys live under `worktree-init.`.

| Key | Meaning |
| --- | --- |
| `plugin` | Path to the plugin, absolute or relative to the main worktree |
| `python` | Interpreter for the plugin |
| `reservedSlots` | Space-separated slots never handed out. Default `0`, an empty value reserves nothing |
| `ignorePaths` | Glob patterns for worktree paths to skip. Multi-valued, default `*/.claude/worktrees/*`, an empty value adds no pattern |
| `interactive` | `false` switches the prompts off |
| `localInclude` | Path relative to the worktree root, copied once from the main worktree. Multi-valued |

Plus one key per option, for example `slot` or `source`.

## Writing a plugin

A plugin is a Python file, referenced by `worktree-init.plugin`. It needs Python 3.12 or newer. `git-wt` starts the interpreter with an embedded package `git_worktree_init`, so `from git_worktree_init.options import Option` works without an install. The plugin's output and the output of `ctx.run` go to the same stdout and stderr as in 0.1. It may define:

| Name | Required | Purpose |
| --- | --- | --- |
| `OPTIONS` | no | List of `Option` objects, imported from `git_worktree_init.options` |
| `SANITIZE_ENV` | no | Environment variable names removed from every subprocess |
| `slot_of(worktree: Path) -> int \| None` | yes | The slot a worktree holds, or `None` |
| `apply(ctx)` | yes | Does the project-specific work |
| `next_steps(ctx) -> list[str]` | no | Lines printed under "Next steps:" |
| `teardown(ctx)` | no | Releases what `apply` set up, see "Teardown" |

`Option(name, kind, default=None, help="", choices=None, ask=True)` with `kind` one of `confirm`, `text`, `choice`, `slot`. `help` is a short lowercase phrase without a final period, as in Git. `git wt -h` shows it as is and the prompt capitalizes it. `choices` is a list, or a function taking the `Context` and returning one. The names `slot` and `source` are reserved.

`ctx` is a `Context`.

| Member | Meaning |
| --- | --- |
| `main`, `target`, `source`, `common_dir` | Paths of the main worktree, the new worktree, the copy source and the common git dir |
| `name` | Directory name of the target |
| `slot`, `mode` | Resolved slot, and `create`, `refresh`, `remove` or `prune` |
| `worktrees` | Paths of all usable worktrees |
| `option(name)` | Resolved value of an option |
| `info(message)` | Prints an indented line to stderr |
| `run(cmd, cwd=None, check=True, capture=False)` | Runs a command in the target with the sanitized environment, in mode `prune` in the main worktree |
| `link(rel, anchor) -> bool` | Symlinks `anchor/rel` to the target, keeps existing files, relinks dangling links |
| `copy(rel, source=None, overwrite=True) -> bool` | Copies a file or directory from the source |
| `copy_tree(rel, excludes=(), source=None) -> bool` | Copies a directory, skipping names matching `excludes` |
| `write(rel, text, backup=False) -> bool` | Writes a file, optionally saving the old one as `.bak` |
| `remove(rel) -> bool` | Removes a file or symlink |
| `ensure_ignored(rel) -> bool` | Adds `rel` to `info/exclude` unless Git already ignores it |

The methods that change files return whether they changed anything. `examples/minimal_plugin.py` is a working template. All messages go to stderr.

## Teardown

Git runs no hook when a worktree goes away, whether through `git worktree remove`, `git worktree prune` or a deleted directory. The tool therefore keeps a record for each worktree it prepares with a plugin, under `<git-common-dir>/worktree-init/records/`. A record holds the slot, the path and the resolved options. The tool writes it before `apply`, so teardown also works for a worktree whose preparation failed halfway.

The sweep calls `teardown(ctx)` with `ctx.mode` set to `prune` for every record whose worktree no longer exists. It deletes the record once teardown returns. `git wt prune` runs the sweep, and so does every create and refresh. A worktree moved with `git worktree move` counts as existing, and so does a locked one.

`git wt remove` calls teardown with `ctx.mode` set to `remove` before Git deletes anything. The files are still there, so teardown may read them or run `docker compose down` in the target. Slot and options come from the record. A worktree without a record falls back to `slot_of` and the configured options, for example one created before the plugin had a teardown. If neither gives a slot, `git wt remove` skips teardown, so `ctx.slot` is always a number. Git refuses to remove a dirty or locked worktree. In that case `git wt remove` skips teardown and Git prints its usual error. `git wt remove` passes `--force` and `-f -f` on to Git.

A failing teardown blocks the removal. The worktree and its record stay unchanged, and the message names the retry:

```sh
git wt remove <worktree>
```

To give up on teardown, remove the worktree with `git worktree remove` and run `git wt prune` later. If Git refuses the removal after teardown ran, the worktree stays without its resources. `git wt refresh <worktree>` sets them up again.

In mode `prune` the files of the worktree are gone, and the path may already hold a new worktree. Teardown may rely only on `ctx.slot`, `ctx.name`, the path `ctx.target` and `ctx.option(name)`. `ctx.run` starts in the main worktree. Do not use `write`, `copy`, `copy_tree`, `link` or `remove`, because they act on the target path. For a plugin without `teardown`, the sweep deletes the records and calls nothing.

Teardown may run more than once for the same worktree, for example after it failed halfway. Write it idempotent, such as `dropdb --if-exists`.

A slot whose teardown failed stays reserved, so a new worktree cannot reuse a database or port that still exists. The tool keeps the record, and the message names the repair command:

```sh
git wt prune
```

During create or refresh, a failed teardown prints a warning but does not fail the new worktree.

## Release

The version lives in `Cargo.toml`. To release, bump `version` there and run `cargo build` so `Cargo.lock` follows. Commit the change as `Release <version>` on `main`, tag it `v<version>` and push the tag.

The workflow `release.yml` then checks that the tag matches `Cargo.toml` and sits on `main`. It builds the three targets and publishes the GitHub release with archives and checksums. Then it commits the new version and checksums to `Formula/git-wt.rb` on `main`. A run started by hand builds the archives but publishes nothing.

## Failure and repair

When the preparation fails, the worktree stays, the exit code is not 0 and the message names the repair command:

```sh
git wt refresh <worktree>
```

Ctrl-C at a prompt aborts the same way and leaves the worktree in place.
