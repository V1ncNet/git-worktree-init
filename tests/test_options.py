import pytest

from git_worktree_init import options
from git_worktree_init.config import Entry
from git_worktree_init.options import Option

GRAPH = Option("graph", "confirm", True, "Copy graph")
CLONE = Option("clone_db", "choice", "none", "Clone from", choices=["none", "a"])
DECLARED = options.BUILTIN + (GRAPH, CLONE)


def test_keys_are_derived_from_name():
    assert (CLONE.env_key, CLONE.config_key, CLONE.flag) == (
        "WT_CLONE_DB",
        "clone-db",
        "--clone-db",
    )


@pytest.mark.parametrize("raw, expected", [("yes", True), ("1", True), ("off", False), ("N", False)])
def test_parse_confirm(raw, expected):
    assert options.parse(GRAPH, raw) is expected


def test_parse_rejects_bad_values():
    with pytest.raises(options.OptionError):
        options.parse(GRAPH, "maybe")
    with pytest.raises(options.OptionError):
        options.parse(options.BUILTIN[0], "-1")


def test_resolve_order_is_explicit_answer_config_default():
    explicit = options.explicit(DECLARED, {"WT_GRAPH": "0"})
    config_values = options.from_config(
        DECLARED, {"clone-db": [Entry("local", "a")], "graph": [Entry("local", "true")]}
    )

    values = options.resolve(DECLARED, explicit, {"clone_db": "none"}, config_values)

    assert values == {"slot": None, "source": None, "graph": False, "clone_db": "none"}


def test_interactive_wanted_without_any_setting():
    assert options.interactive_wanted(DECLARED, {}, {})


@pytest.mark.parametrize(
    "env, entries",
    [
        ({"WT_YES": "1"}, {}),
        ({"WT_GRAPH": "1"}, {}),
        ({}, {"clone-db": [Entry("command", "a")]}),
        ({}, {"interactive": [Entry("global", "false")]}),
        ({"GIT_TERMINAL_PROMPT": "0"}, {}),
        ({"CI": "true"}, {}),
    ],
)
def test_interactive_not_wanted(env, entries):
    assert not options.interactive_wanted(DECLARED, env, entries)


def test_local_config_value_keeps_interactive():
    assert options.interactive_wanted(DECLARED, {}, {"clone-db": [Entry("local", "a")]})
