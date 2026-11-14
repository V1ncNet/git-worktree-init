# git-worktree-init

## What it does

A `post-checkout` hook prepares every new linked worktree. It copies the files listed in `.worktreeinclude` and, optionally, calls a plugin for project-specific work such as allocating a slot, writing an `.env` or cloning a database. A plugin can also release that work again once the worktree is gone.

## Requirements

- Git 2.54 or newer (config-based hooks)
- Python 3.12 or newer, no packages

The interpreter can be set with `git config worktree-init.python <path>` or the environment variable `WT_PYTHON`, for example for IDEs that start Git with a short `PATH`.

## Install

Register the hook, an alias and the interpreter in a user-level config file that is included from your global config. `<clone>` is the directory of this repository.

```sh
git config --file ~/.config/git/local hook.worktree-init.command <clone>/hook.sh
git config --file ~/.config/git/local --add hook.worktree-init.event post-checkout
git config --file ~/.config/git/local alias.wt '!<clone>/bin/git-wt'
git config --file ~/.config/git/local --type=path worktree-init.python "$(command -v python3)"
```

To use the harness, register its shim for three events as well:

```sh
git config --file ~/.config/git/local hook.harness.command 'f="<clone>/harness-hook.sh"; [ -f "$f" ] || exit 0; exec "$f"'
git config --file ~/.config/git/local --add hook.harness.event post-checkout
git config --file ~/.config/git/local --add hook.harness.event pre-push
git config --file ~/.config/git/local --add hook.harness.event reference-transaction
```

The shim does nothing in a repository without `harness.git` and never fails the Git command it runs in. The registration checks that the file exists, because `reference-transaction` and `pre-push` abort Git when their hook cannot run. Checking out a branch of this repository that lacks the shim would otherwise block every commit, fetch and push on the machine.

The interpreter must be Python 3.12 or newer, so check that `python3` resolves to one, or pass the path of a suitable interpreter instead. The file `~/.config/git/local` takes effect only if the global config includes it, for example with `[include] path = ~/.config/git/local`. Use `--global` instead of `--file ~/.config/git/local` to skip the include.

`core.hooksPath` is deliberately not used, because it switches off every hook in `.git/hooks`. Keep `hook.jobs` at 1 so hooks of the same event run one after another.

To switch the hook off in one repository:

```sh
git config hook.worktree-init.enabled false
```

Nothing happens for a repository that has no `worktree-init.plugin`, no `worktree-init.localInclude` and no `.worktreeinclude` in its main worktree.

### Nix

The flake `github:V1ncNet/git-worktree-init` provides `packages.<system>.default` for `aarch64-darwin`, `x86_64-linux` and `aarch64-linux`. The package keeps the layout of this repository, so `<clone>` above becomes the store path of the package. It falls back to the Python of its nixpkgs input, which makes `worktree-init.python` optional. With home-manager:

```nix
let
  pkg = inputs.git-worktree-init.packages.${pkgs.stdenv.hostPlatform.system}.default;
in
{
  programs.git.settings = {
    hook.worktree-init = {
      command = "${pkg}/hook.sh";
      event = "post-checkout";
    };
    alias.wt = "!${pkg}/bin/git-wt";
  };
}
```

## Use

```sh
git worktree add -b feature/foo ../proj.foo
git wt add --slot 3 -- -b feature/foo ../proj.foo
git -c worktree-init.slot=3 worktree add -b feature/foo ../proj.foo
WT_SLOT=3 git worktree add -b feature/foo ../proj.foo
git wt refresh ../proj.foo
git wt prune
```

- `git worktree add` runs the hook and prompts for options when a terminal is available. With `--no-checkout` Git does not run `post-checkout`, so initialize such a worktree with `git wt refresh`.
- `git wt add [<options>] -- <git-worktree-add-args>...` turns the flags into `WT_*` variables for that one invocation and forwards everything after `--` to `git worktree add`. The exit code of `git worktree add` is returned. Flags cannot be abbreviated.
- `git -c worktree-init.<key>=<value>` and `WT_<NAME>` set an option without `git wt`.
- `git wt refresh [<options>] [<worktree>]` re-runs the preparation for an existing worktree, the current one by default. It keeps the existing slot and does not overwrite copied files that differ.
- `git wt prune` calls the plugin's `teardown` for every recorded worktree that no longer exists. See "Teardown".

`git wt -h`, `git wt add -h` and `git wt refresh -h` show the usage and the flags, including those of the plugin. Git answers `git wt --help` itself with the alias definition, so use `-h`. Help and usage errors exit with 129, like Git.

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
| `python` | Interpreter for the hook and `git wt` |
| `reservedSlots` | Space-separated slots never handed out. Default `0`, an empty value reserves nothing |
| `ignorePaths` | Glob patterns for worktree paths to skip. Multi-valued, default `*/.claude/worktrees/*`, an empty value adds no pattern |
| `interactive` | `false` switches the prompts off |
| `localInclude` | Path relative to the worktree root, copied once from the main worktree. Multi-valued |

Plus one key per option, for example `slot` or `source`.

## Writing a plugin

A plugin is a Python file, referenced by `worktree-init.plugin`. It may define:

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
| `slot`, `mode` | Resolved slot, and `create`, `refresh` or `prune` |
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

In mode `prune` the files of the worktree are gone, and the path may already hold a new worktree. Teardown may rely only on `ctx.slot`, `ctx.name`, the path `ctx.target` and `ctx.option(name)`. `ctx.run` starts in the main worktree. Do not use `write`, `copy`, `copy_tree`, `link` or `remove`, because they act on the target path. For a plugin without `teardown`, the sweep deletes the records and calls nothing.

Teardown may run more than once for the same worktree, for example after it failed halfway. Write it idempotent, such as `dropdb --if-exists`.

A slot whose teardown failed stays reserved, so a new worktree cannot reuse a database or port that still exists. The tool keeps the record, and the message names the repair command:

```sh
git wt prune
```

During create or refresh, a failed teardown prints a warning but does not fail the new worktree.

## Failure and repair

When the preparation fails, the worktree stays, the exit code is not 0 and the message names the repair command:

```sh
git wt refresh <worktree>
```

Ctrl-C at a prompt aborts the same way and leaves the worktree in place.
