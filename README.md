# git-worktree-init

## What it does

A `post-checkout` hook prepares every new linked worktree. It copies the files listed in `.worktreeinclude` and, optionally, calls a plugin for project-specific work such as allocating a slot, writing an `.env` or cloning a database.

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

The interpreter must be Python 3.12 or newer, so check that `python3` resolves to one, or pass the path of a suitable interpreter instead. The file `~/.config/git/local` takes effect only if the global config includes it, for example with `[include] path = ~/.config/git/local`. Use `--global` instead of `--file ~/.config/git/local` to skip the include.

`core.hooksPath` is deliberately not used, because it switches off every hook in `.git/hooks`. Keep `hook.jobs` at 1 so hooks of the same event run one after another.

To switch the hook off in one repository:

```sh
git config hook.worktree-init.enabled false
```

Nothing happens for a repository that has neither a `worktree-init.plugin` nor a `.worktreeinclude` in its main worktree.

## Use

```sh
git worktree add -b feature/foo ../proj.foo
git wt add --slot 3 -- -b feature/foo ../proj.foo
git -c worktree-init.slot=3 worktree add -b feature/foo ../proj.foo
WT_SLOT=3 git worktree add -b feature/foo ../proj.foo
git wt refresh ../proj.foo
```

- `git worktree add` runs the hook and prompts for options when a terminal is available. With `--no-checkout` Git does not run `post-checkout`, so initialize such a worktree with `git wt refresh`.
- `git wt add [--yes] [--<option> ...] -- <git worktree add arguments>` turns the flags into `WT_*` variables for that one invocation and forwards everything after `--` to `git worktree add`. The exit code of `git worktree add` is returned, a usage error returns 2. Flags cannot be abbreviated.
- `git -c worktree-init.<key>=<value>` and `WT_<NAME>` set an option without `git wt`.
- `git wt refresh [--<option> ...] [<worktree>]` re-runs the preparation for an existing worktree, the current one by default. It keeps the existing slot and does not overwrite copied files that differ.

To copy from the current worktree instead of the main worktree, pass `--source .` or set `WT_SOURCE=$PWD`. A relative `--source` is made absolute by `git wt`.

## Options

Every option has a flag, an environment variable and a config key. Underscores in the option name become hyphens in flag and config key, so `clone_db` is `--clone-db`, `WT_CLONE_DB` and `worktree-init.clone-db`.

| Option | Flag | Env | Config | Prompt |
| --- | --- | --- | --- | --- |
| `slot` | `--slot N` | `WT_SLOT` | `worktree-init.slot` | yes |
| `source` | `--source PATH` | `WT_SOURCE` | `worktree-init.source` | no |

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

## Configuration keys

All keys live under `worktree-init.`.

| Key | Meaning |
| --- | --- |
| `plugin` | Path to the plugin, absolute or relative to the main worktree |
| `python` | Interpreter for the hook and `git wt` |
| `reservedSlots` | Space-separated slots never handed out. Default `0`, an empty value reserves nothing |
| `ignorePaths` | Glob patterns for worktree paths to skip. Multi-valued, default `*/.claude/worktrees/*`, an empty value adds no pattern |
| `interactive` | `false` switches the prompts off |

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

`Option(name, kind, default=None, help="", choices=None, ask=True)` with `kind` one of `confirm`, `text`, `choice`, `slot`. `choices` is a list, or a function taking the `Context` and returning one. The names `slot` and `source` are reserved.

`ctx` is a `Context`.

| Member | Meaning |
| --- | --- |
| `main`, `target`, `source`, `common_dir` | Paths of the main worktree, the new worktree, the copy source and the common git dir |
| `name` | Directory name of the target |
| `slot`, `mode` | Resolved slot, and `create` or `refresh` |
| `worktrees` | Paths of all usable worktrees |
| `option(name)` | Resolved value of an option |
| `info(message)` | Prints an indented line to stderr |
| `run(cmd, cwd=None, check=True, capture=False)` | Runs a command in the target with the sanitized environment |
| `link(rel, anchor) -> bool` | Symlinks `anchor/rel` to the target, keeps existing files, relinks dangling links |
| `copy(rel, source=None, overwrite=True) -> bool` | Copies a file or directory from the source |
| `copy_tree(rel, excludes=(), source=None) -> bool` | Copies a directory, skipping names matching `excludes` |
| `write(rel, text, backup=False) -> bool` | Writes a file, optionally saving the old one as `.bak` |
| `remove(rel) -> bool` | Removes a file or symlink |
| `ensure_ignored(rel) -> bool` | Adds `rel` to `info/exclude` unless Git already ignores it |

The methods that change files return whether they changed anything. `examples/minimal_plugin.py` is a working template. All messages go to stderr.

## Failure and repair

When the preparation fails, the worktree stays, the exit code is not 0 and the message names the repair command:

```sh
git wt refresh <worktree>
```

Ctrl-C at a prompt aborts the same way and leaves the worktree in place.
