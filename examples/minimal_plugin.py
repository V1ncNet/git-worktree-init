from pathlib import Path

from git_worktree_init.options import Option

OPTIONS = [Option("port_base", "text", "8000", "first port")]


def slot_of(worktree: Path) -> int | None:
    env = worktree / ".env"
    if not env.is_file():
        return None
    for line in env.read_text(encoding="utf-8").splitlines():
        if line.startswith("SLOT="):
            return int(line.split("=", 1)[1])
    return None


def apply(ctx) -> None:
    port = int(ctx.option("port_base")) + ctx.slot
    ctx.write(".env", f"SLOT={ctx.slot}\nPORT={port}\n", backup=True)
    ctx.ensure_ignored(".env.bak")


def next_steps(ctx) -> list[str]:
    return [f"Start the app on port {int(ctx.option('port_base')) + ctx.slot}"]
