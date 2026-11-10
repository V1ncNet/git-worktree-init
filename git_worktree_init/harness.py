from collections.abc import Mapping
from pathlib import Path

from git_worktree_init import gitenv

DEFAULT_BRANCH = "develop"
REMOTE = "harness"
ANCHOR = "harness-anchor"
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
    if not admin.is_dir():
        anchor = own / ANCHOR
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
