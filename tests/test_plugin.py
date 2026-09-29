import pytest

from git_worktree_init import plugin
from git_worktree_init.config import Entry

VALID = '''
from dataclasses import dataclass
from git_worktree_init.options import Option

@dataclass
class Marker:
    value: int

OPTIONS = [Option("graph", "confirm", True)]
SANITIZE_ENV = ["SOME_LEAKING_VARIABLE"]

def slot_of(worktree):
    return Marker(3).value

def apply(ctx):
    pass
'''


def write(tmp_path, text):
    path = tmp_path / "p.py"
    path.write_text(text)
    return path


def test_locate_resolves_relative_to_main(tmp_path):
    main = tmp_path / "main"
    entries = {"plugin": [Entry("local", "../shared/p.py")]}

    assert plugin.locate(main, entries) == (tmp_path / "shared" / "p.py").resolve()
    assert plugin.locate(main, {}) is None


def test_load_valid_plugin(tmp_path):
    loaded = plugin.load(write(tmp_path, VALID))

    assert [o.name for o in loaded.options] == ["graph"]
    assert loaded.sanitize_env == ("SOME_LEAKING_VARIABLE",)
    assert loaded.slot_of(tmp_path) == 3
    assert loaded.next_steps is None


@pytest.mark.parametrize(
    "text, fragment",
    [
        ("def apply(ctx): pass\n", "slot_of"),
        ("def slot_of(w): return None\n", "apply"),
        ("raise RuntimeError('boom')\n", "boom"),
        (VALID.replace('"confirm"', '"bogus"'), "bogus"),
        (VALID.replace('Option("graph"', 'Option("slot"'), "slot"),
    ],
)
def test_load_rejects_broken_plugins(tmp_path, text, fragment):
    with pytest.raises(plugin.PluginError, match=fragment):
        plugin.load(write(tmp_path, text))


def test_load_missing_file(tmp_path):
    with pytest.raises(plugin.PluginError, match="not found"):
        plugin.load(tmp_path / "nope.py")
