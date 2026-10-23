import io

import pytest

from git_worktree_init import cli
from git_worktree_init.options import Option

TOP_LEVEL_USAGE = (
    "usage: git wt add [<options>] -- <git-worktree-add-args>...\n"
    "   or: git wt refresh [<options>] [<worktree>]\n"
    "\n"
)


def test_top_level_help_lists_subcommands():
    help_out, err = io.StringIO(), io.StringIO()

    code = cli.main(["-h"], err=err, help_out=help_out)

    assert code == 129
    assert help_out.getvalue() == TOP_LEVEL_USAGE
    assert err.getvalue() == ""


@pytest.mark.parametrize(
    ("args", "error"),
    [((), "error: need a subcommand\n"), (("bogus",), "error: unknown subcommand: `bogus'\n")],
)
def test_bad_subcommand_prints_usage_to_stderr(args, error):
    help_out, err = io.StringIO(), io.StringIO()

    code = cli.main(list(args), err=err, help_out=help_out)

    assert code == 129
    assert err.getvalue() == error + TOP_LEVEL_USAGE
    assert help_out.getvalue() == ""


@pytest.mark.parametrize(
    ("option", "row"),
    [
        (Option("clone_db", "confirm", help="clone the database"), "    --[no-]clone-db       clone the database"),
        (Option("db", "choice", choices=["none", "a"], help="db"), "    --db (none|a)         db"),
        (Option("db", "choice", choices=lambda ctx: [], help="db"), "    --db <value>          db"),
        (
            Option("very_long_option_name", "text", help="wraps"),
            "    --very-long-option-name <value>\n                          wraps",
        ),
    ],
)
def test_option_rows_follow_git(option, row):
    assert cli.option_row(option) == row
