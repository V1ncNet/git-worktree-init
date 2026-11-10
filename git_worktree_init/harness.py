import shutil
import subprocess
import sys
from collections.abc import Mapping
from pathlib import Path
from typing import TextIO

from git_worktree_init import engine, gitenv

DEFAULT_BRANCH = "develop"
REMOTE = "harness"
ANCHOR = "harness-anchor"
SNAPSHOT_SUBJECT = "Snapshot harness state"
NON_INTERACTIVE_SSH = "ssh -o BatchMode=yes -o ConnectTimeout=10"
UPSTREAM = f"refs/remotes/origin/{DEFAULT_BRANCH}"
FEATURE_DIRS = (".superpowers/sdd/", ".claude/handoffs/")
RUNTIME_MARKER = "# claude-code-runtime"
ALLOWLIST = (
    "/*",
    "!/CLAUDE.md",
    "!/.claude/",
    "!/.superpowers/",
    "!/docs/",
    "/docs/*",
    "!/docs/superpowers/",
    "/.claude/settings.local.json",
    "__pycache__/",
    ".DS_Store",
)
OVERRIDES = (
    ("commit.gpgsign", "false"),
    ("hook.beams.enabled", "false"),
    ("hook.worktree-init.enabled", "false"),
    ("hook.harness.enabled", "false"),
    ("core.excludesFile", "/dev/null"),
)


def git_dir(common_dir: Path) -> Path:
    return common_dir / "harness.git"


def enabled(common_dir: Path) -> bool:
    return git_dir(common_dir).is_dir()


def git(target: Path, *args: str, env: Mapping[str, str], check: bool = True) -> str:
    clean = gitenv.sanitized(env)
    admin = _admin_dir(target, clean)
    return gitenv.git(target, f"--git-dir={admin}", f"--work-tree={target}", *args, env=clean, check=check)


def init(main: Path, remote: str, env: Mapping[str, str]) -> None:
    clean = gitenv.sanitized(env)
    _, common = _code_dirs(main, clean)
    root = git_dir(common)
    gitenv.git(main, "init", "-q", "--bare", "-b", DEFAULT_BRANCH, str(root), env=clean)
    gitenv.git(root, "remote", "add", REMOTE, remote, env=clean)
    for key, value in OVERRIDES:
        gitenv.git(root, "config", key, value, env=clean)
    lines = ALLOWLIST + _runtime_patterns(common / "info" / "exclude")
    (root / "info").mkdir(exist_ok=True)
    (root / "info" / "exclude").write_text("\n".join(lines) + "\n", encoding="utf-8")


def base_of(target: Path, branch: str, env: Mapping[str, str]) -> str:
    clean = gitenv.sanitized(env)
    merge_base = _config(target, f"branch.{branch}.t3-merge-base", clean)
    if merge_base:
        return merge_base
    upstream = _config(target, f"branch.{branch}.merge", clean).removeprefix("refs/heads/")
    if upstream and upstream != branch:
        return upstream
    return DEFAULT_BRANCH


def attach(target: Path, branch: str, env: Mapping[str, str]) -> None:
    fresh = not _index(target, env).exists()
    if not _exists(target, f"refs/heads/{branch}", env):
        _fetch(target, branch, env)
        start = _first_existing(target, (f"refs/remotes/{REMOTE}/{branch}",), env)
        if start is None:
            base = base_of(target, branch, env)
            start = _first_existing(target, _candidates(base) + _candidates(DEFAULT_BRANCH), env)
        if start is None:
            git(target, "symbolic-ref", "HEAD", f"refs/heads/{branch}", env=env)
            return
        git(target, "branch", "--no-track", branch, start, env=env)
    git(target, "checkout", "-q", *(["-f"] if fresh else []), branch, env=env)


def snapshot(target: Path, env: Mapping[str, str]) -> bool:
    if not _index(target, env).exists():
        return False
    if (_index(target, env).parent / "MERGE_HEAD").exists():
        print(f"worktree-init: harness merge is still open in {target}, snapshot skipped", file=sys.stderr)
        return False
    tracked = git(target, "ls-files", env=env).splitlines()
    if tracked and len(git(target, "ls-files", "--deleted", env=env).splitlines()) == len(tracked):
        print(f"worktree-init: every harness file is missing in {target}, snapshot skipped", file=sys.stderr)
        return False
    git(target, "add", "-A", env=env)
    hidden = git(
        target, "ls-files", "-z", "--others", f"--exclude-from={_exclude(target, env)}",
        "--exclude-per-directory=.gitignore", env=env,
    )
    paths = [path for path in hidden.split("\0") if path]
    if paths:
        git(target, "add", "-f", "--", *paths, env=env)
    if not git(target, "status", "--porcelain", "--untracked-files=no", env=env).strip():
        return False
    git(target, "commit", "-q", "--no-verify", "-m", SNAPSHOT_SUBJECT, env=env)
    return True


def switch(target: Path, branch: str, env: Mapping[str, str]) -> None:
    current = git(target, "symbolic-ref", "-q", "--short", "HEAD", env=env, check=False).strip()
    if current == branch and _index(target, env).exists():
        return
    snapshot(target, env)
    attach(target, branch, env)


def push(target: Path, env: Mapping[str, str], err: TextIO | None = None) -> bool:
    branch = git(target, "symbolic-ref", "-q", "--short", "HEAD", env=env, check=False).strip()
    try:
        git(target, "push", "-q", REMOTE, f"refs/heads/{branch}:refs/heads/{branch}", env=_offline_safe(env))
    except subprocess.CalledProcessError as error:
        lines = (error.stderr or "").strip().splitlines()
        reason = lines[-1] if lines else f"exit {error.returncode}"
        print(f"worktree-init: harness push failed: {reason}", file=err or sys.stderr)
        return False
    return True


def submitted(main: Path, env: Mapping[str, str]) -> list[str]:
    clean = gitenv.sanitized(env)
    upstream = gitenv.git(main, "rev-parse", "-q", "--verify", UPSTREAM, env=clean, check=False).strip()
    if not upstream:
        return []
    landed = set(gitenv.git(main, "rev-list", "--first-parent", upstream, env=clean).split())
    busy = _checked_out(main, clean)
    result = []
    for branch in git(main, "for-each-ref", "--format=%(refname:short)", "refs/heads", env=env).split():
        if branch == DEFAULT_BRANCH or branch in busy:
            continue
        tip = gitenv.git(main, "rev-parse", "-q", "--verify", f"refs/heads/{branch}^{{commit}}", env=clean, check=False).strip()
        if not tip or tip in landed or not _is_ancestor(main, tip, upstream, clean):
            continue
        if git(main, "rev-list", "-1", f"{DEFAULT_BRANCH}..{branch}", env=env).strip():
            result.append(branch)
    return result


def feature_paths(main: Path, branch: str, env: Mapping[str, str]) -> list[str]:
    base = git(main, "merge-base", DEFAULT_BRANCH, branch, env=env).strip()
    output = git(main, "diff", "-z", "--name-only", "--diff-filter=A", base, branch, "--", *FEATURE_DIRS, env=env)
    existing = set(git(main, "ls-tree", "-d", "--name-only", base, "--", FEATURE_DIRS[0], env=env).split())
    paths = set()
    for rel in filter(None, output.split("\0")):
        ledger = "/".join(rel.split("/")[:3])
        paths.add(ledger if rel.startswith(FEATURE_DIRS[0]) and ledger not in existing else rel)
    return sorted(paths)


def merge(main: Path, branch: str, env: Mapping[str, str], err: TextIO | None = None) -> bool:
    err = err or sys.stderr
    _, common = _code_dirs(main, gitenv.sanitized(env))
    with engine.locked(common):
        root = git_dir(common)
        if (root / "MERGE_HEAD").exists():
            print(f"worktree-init: harness merge is still open in {main}, resolve it before merging {branch}", file=err)
            return False
        current = git(main, "symbolic-ref", "-q", "--short", "HEAD", env=env, check=False).strip()
        if current != DEFAULT_BRANCH:
            print(f"worktree-init: harness of {main} is on {current or 'a detached HEAD'}, not merging {branch}", file=err)
            return False
        paths = feature_paths(main, branch, env)
        try:
            git(main, "merge", "-q", "--no-ff", "--no-commit", branch, env=env)
        except subprocess.CalledProcessError as error:
            if (root / "MERGE_HEAD").exists():
                print(f"worktree-init: harness merge of {branch} has a conflict, resolve it in {main}", file=err)
            else:
                lines = (error.stderr or "").strip().splitlines()
                print(f"worktree-init: harness merge of {branch} failed: {lines[-1] if lines else error.returncode}", file=err)
            return False
        if paths:
            git(main, "rm", "-r", "-q", "-f", "--ignore-unmatch", "--", *paths, env=env)
        git(main, "commit", "-q", "--no-verify", "-m", f"Merge harness branch '{branch}'", env=env)
    print(f"worktree-init: merged harness branch {branch} into {DEFAULT_BRANCH}", file=err)
    return True


def _checked_out(main: Path, env: Mapping[str, str]) -> set[str]:
    listing = gitenv.git(main, "worktree", "list", "--porcelain", env=env)
    return {line.removeprefix("branch refs/heads/") for line in listing.splitlines() if line.startswith("branch ")}


def _is_ancestor(main: Path, commit: str, ref: str, env: Mapping[str, str]) -> bool:
    return gitenv.run(["git", "merge-base", "--is-ancestor", commit, ref], main, env, check=False).returncode == 0


def _candidates(branch: str) -> tuple[str, ...]:
    return f"refs/heads/{branch}", f"refs/remotes/{REMOTE}/{branch}"


def _first_existing(target: Path, refs: tuple[str, ...], env: Mapping[str, str]) -> str | None:
    return next((ref for ref in refs if _exists(target, ref, env)), None)


def _exists(target: Path, ref: str, env: Mapping[str, str]) -> bool:
    return bool(git(target, "rev-parse", "-q", "--verify", f"{ref}^{{commit}}", env=env, check=False).strip())


def _fetch(target: Path, branch: str, env: Mapping[str, str]) -> None:
    git(
        target, "fetch", "-q", REMOTE, f"refs/heads/{branch}:refs/remotes/{REMOTE}/{branch}",
        env=_offline_safe(env), check=False,
    )


def _offline_safe(env: Mapping[str, str]) -> dict[str, str]:
    return {"GIT_SSH_COMMAND": NON_INTERACTIVE_SSH, **env}


def _index(target: Path, env: Mapping[str, str]) -> Path:
    return _admin_dir(target, gitenv.sanitized(env)) / "index"


def _exclude(target: Path, env: Mapping[str, str]) -> Path:
    _, common = _code_dirs(target, gitenv.sanitized(env))
    return git_dir(common) / "info" / "exclude"


def _config(target: Path, key: str, env: Mapping[str, str]) -> str:
    return gitenv.git(target, "config", "--get", key, env=env, check=False).strip()


def _code_dirs(target: Path, env: Mapping[str, str]) -> tuple[Path, Path]:
    output = gitenv.git(
        target, "rev-parse", "--path-format=absolute", "--git-dir", "--git-common-dir", env=env
    )
    own, common = output.splitlines()
    return Path(own).resolve(), Path(common).resolve()


def _admin_dir(target: Path, env: Mapping[str, str]) -> Path:
    own, common = _code_dirs(target, env)
    root = git_dir(common)
    if own == common:
        return root
    admin = root / "worktrees" / own.name
    anchor = own / ANCHOR
    if admin.is_dir() and not anchor.exists():
        shutil.rmtree(admin)
    if not admin.is_dir():
        gitenv.git(root, "worktree", "prune", env=env)
        anchor.touch()
        admin.mkdir(parents=True)
        (admin / "gitdir").write_text(f"{anchor}\n", encoding="utf-8")
        (admin / "commondir").write_text("../..\n", encoding="utf-8")
        (admin / "HEAD").write_text(f"ref: refs/heads/{DEFAULT_BRANCH}\n", encoding="utf-8")
    return admin


def _runtime_patterns(exclude: Path) -> tuple[str, ...]:
    if not exclude.is_file():
        return ()
    lines = exclude.read_text(encoding="utf-8").splitlines()
    if RUNTIME_MARKER not in lines:
        return ()
    block = []
    for line in lines[lines.index(RUNTIME_MARKER) + 1:]:
        if not line.strip():
            break
        if not line.startswith("#"):
            block.append(line)
    return tuple(block)
