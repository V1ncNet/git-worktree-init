from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

from git_worktree_init.gitenv import git


@dataclass(frozen=True)
class Repo:
    main: Path
    common_dir: Path
    worktrees: tuple[Path, ...]
    admin_dir: Path | None = None


def discover(cwd: Path, env: Mapping[str, str]) -> Repo:
    dirs = git(cwd, "rev-parse", "--path-format=absolute", "--git-common-dir", "--git-dir", env=env)
    common, own = (Path(line).resolve() for line in dirs.splitlines())
    listing = git(cwd, "worktree", "list", "--porcelain", env=env)
    blocks = [block.splitlines() for block in listing.strip().split("\n\n") if block.strip()]
    paths = [Path(block[0].removeprefix("worktree ")).resolve() for block in blocks]
    usable = tuple(
        path
        for path, block in zip(paths, blocks)
        if "bare" not in block and not any(line.startswith("prunable") for line in block)
    )
    admin_dir = None if own == common else own
    return Repo(main=paths[0], common_dir=common, worktrees=usable, admin_dir=admin_dir)


def toplevel(cwd: Path, env: Mapping[str, str]) -> Path:
    return Path(git(cwd, "rev-parse", "--show-toplevel", env=env).strip()).resolve()
