import sys

if sys.version_info < (3, 12):
    sys.stderr.write(
        f"worktree-init: needs python >= 3.12, got {sys.version.split()[0]} at {sys.executable}; "
        "set git config worktree-init.python\n"
    )
    sys.exit(1)

from git_worktree_init.cli import main

sys.exit(main(sys.argv[1:]))
