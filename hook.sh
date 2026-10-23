#!/bin/sh
case "$1" in *[!0]*) exit 0 ;; esac
[ "$3" = 1 ] || exit 0
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX
git_dir=$(git rev-parse --path-format=absolute --git-dir 2>/dev/null) || exit 0
common_dir=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || exit 0
[ "$git_dir" = "$common_dir" ] && exit 0
here=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
patterns=$(git config --get-all worktree-init.ignorePaths) || patterns='*/.claude/worktrees/*'
set -f
old_ifs=$IFS
IFS='
'
for pattern in $patterns; do
    case "$here/" in $pattern) exit 0 ;; esac
done
IFS=$old_ifs
set +f
main=$(git worktree list --porcelain | sed -n '1s/^worktree //p')
if [ -z "$(git config --get worktree-init.plugin)" ] && [ ! -f "$main/.worktreeinclude" ]; then
    exit 0
fi
python=${WT_PYTHON:-$(git config --get --type=path worktree-init.python)}
root=$(cd "$(dirname "$0")" && pwd -P)
exec "${python:-python3}" "$root/wti.py" hook "$@"
