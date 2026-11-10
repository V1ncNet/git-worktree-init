#!/bin/sh
case "${GIT_DIR:-}" in */harness.git|*/harness.git/*) exit 0 ;; esac
common_dir=$(git rev-parse --path-format=absolute --git-common-dir 2>/dev/null) || exit 0
[ -d "$common_dir/harness.git" ] || exit 0
case $# in
    3) [ "$3" = 1 ] || exit 0; event=post-checkout ;;
    2) event=pre-push ;;
    1) event=reference-transaction ;;
    *) exit 0 ;;
esac
python=${WT_PYTHON:-$(git config --get --type=path worktree-init.python)}
root=$(cd "$(dirname "$0")" && pwd -P)
exec "${python:-python3}" "$root/wti.py" harness-hook "$event" "$@"
