import json
import os
import threading

import pytest

from git_worktree_init import bridge
from helpers import EXAMPLE_PLUGIN

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


class Client:
    def __init__(self):
        requests_read, self._requests = os.pipe()
        self._replies, replies_write = os.pipe()
        self._reader = os.fdopen(self._replies, "r", encoding="utf-8")
        self._writer = os.fdopen(self._requests, "w", encoding="utf-8")
        self.exit_code = None
        self._thread = threading.Thread(target=self._serve, args=(requests_read, replies_write))
        self._thread.start()

    def _serve(self, requests_read, replies_write):
        self.exit_code = bridge.main(requests_read, replies_write)
        os.close(requests_read)
        os.close(replies_write)

    def ask(self, **request):
        self._writer.write(json.dumps(request) + "\n")
        self._writer.flush()
        return json.loads(self._reader.readline())

    def stop(self):
        self.ask_no_reply(op="exit")
        self._thread.join(timeout=5)
        self._writer.close()
        self._reader.close()

    def ask_no_reply(self, **request):
        self._writer.write(json.dumps(request) + "\n")
        self._writer.flush()


@pytest.fixture
def client():
    started = Client()
    yield started
    started.stop()


def ctx(tmp_path, **overrides):
    fields = {
        "main": str(tmp_path / "main"),
        "target": str(tmp_path / "target"),
        "common_dir": str(tmp_path / "main" / ".git"),
        "mode": "add",
        "env": {},
        "worktrees": [str(tmp_path / "main")],
        "source": None,
        "slot": None,
        "options": {},
    }
    return {**fields, **overrides}


def write(tmp_path, text):
    path = tmp_path / "p.py"
    path.write_text(text)
    return path


def load(client, path):
    return client.ask(op="load", path=str(path))


def test_load_reports_example_plugin_options(client):
    reply = load(client, EXAMPLE_PLUGIN)

    assert reply == {
        "ok": {
            "options": [
                {
                    "name": "port_base",
                    "kind": "text",
                    "default": "8000",
                    "help": "first port",
                    "choices": None,
                    "dynamic": False,
                    "ask": True,
                }
            ],
            "sanitize_env": [],
            "next_steps": True,
            "teardown": True,
        }
    }


def test_load_valid_plugin(client, tmp_path):
    result = load(client, write(tmp_path, VALID))["ok"]

    assert [option["name"] for option in result["options"]] == ["graph"]
    assert result["sanitize_env"] == ["SOME_LEAKING_VARIABLE"]
    assert result["next_steps"] is False
    assert client.ask(op="slot_of", worktree=str(tmp_path)) == {"ok": 3}


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
def test_load_rejects_broken_plugins(client, tmp_path, text, fragment):
    path = write(tmp_path, text)

    error = load(client, path)["error"]

    assert fragment in error
    assert error.startswith(f"{path}: ")


def test_load_missing_file(client, tmp_path):
    assert "not found" in load(client, tmp_path / "nope.py")["error"]


def test_load_rejects_non_json_default(client, tmp_path):
    path = write(tmp_path, VALID.replace('"graph", "confirm", True', '"graph", "text", object()'))

    assert load(client, path) == {
        "error": f"{path}: option graph has a default that is not JSON"
    }


def test_apply_and_next_steps_share_context(client, tmp_path):
    plugin = """
def slot_of(worktree): return None
def apply(ctx): ctx.marker = 1
def next_steps(ctx): return [str(ctx.marker)]
"""
    load(client, write(tmp_path, plugin))

    assert client.ask(op="apply", ctx=ctx(tmp_path)) == {"ok": None}
    assert client.ask(op="next_steps") == {"ok": ["1"]}


def test_teardown_gets_fresh_context(client, tmp_path):
    plugin = """
def slot_of(worktree): return None
def apply(ctx): pass
def teardown(ctx):
    ctx.seen = getattr(ctx, "seen", 0) + 1
    ctx.info(str(ctx.seen))
    if ctx.seen != 1:
        raise RuntimeError("context was reused")
"""
    load(client, write(tmp_path, plugin))

    assert client.ask(op="teardown", ctx=ctx(tmp_path)) == {"ok": None}
    assert client.ask(op="teardown", ctx=ctx(tmp_path)) == {"ok": None}


def test_plugin_exception_is_reported_and_loop_continues(client, tmp_path):
    plugin = """
def slot_of(worktree): return None
def apply(ctx): raise RuntimeError("apply failed")
"""
    load(client, write(tmp_path, plugin))

    assert client.ask(op="apply", ctx=ctx(tmp_path)) == {"error": "apply failed"}
    assert client.ask(op="slot_of", worktree=str(tmp_path)) == {"ok": None}


def test_dynamic_choices_receive_context(client, tmp_path):
    plugin = """
from git_worktree_init.options import Option

def names(ctx):
    return [p.name for p in ctx.worktrees] + [ctx.mode]

OPTIONS = [Option("which", "choice", None, "pick", choices=names), Option("fixed", "choice", "a", choices=["a", "b"])]
def slot_of(worktree): return None
def apply(ctx): pass
"""
    options = load(client, write(tmp_path, plugin))["ok"]["options"]

    assert [(o["choices"], o["dynamic"]) for o in options] == [(None, True), (["a", "b"], False)]
    assert client.ask(op="choices", option="which", ctx=ctx(tmp_path)) == {"ok": ["main", "add"]}


def test_exit_code_is_zero_after_exit_request():
    started = Client()
    started.stop()

    assert started.exit_code == 0
