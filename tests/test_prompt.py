import io
import os
import pty
import subprocess
import sys

from git_worktree_init import prompt
from git_worktree_init.options import Option
from helpers import CORE


def prompter(text):
    return prompt.Prompter(io.StringIO(text), io.StringIO())


def test_enter_takes_defaults():
    p = prompter("\n\n\n\n")

    assert p.confirm("Graph", True) is True
    assert p.number("Slot", 9) == 9
    assert p.choice("DB", ["none", "a"], "none") == "none"
    assert p.text("Name", "x") == "x"


def test_invalid_answers_ask_again():
    p = prompter("maybe\nn\nabc\n12\n7\n2\n")

    assert p.confirm("Graph", True) is False
    assert p.number("Slot", 9) == 12
    assert p.choice("DB", ["none", "a"], "none") == "a"


def test_choice_accepts_name_and_rejects_out_of_range():
    assert prompter("3\na\n").choice("DB", ["none", "a"], "none") == "a"


def test_ask_skips_hidden_and_stops_at_eof():
    declared = (
        Option("slot", "slot", help="Slot"),
        Option("source", "text", ask=False),
        Option("graph", "confirm", True),
        Option("clone_db", "choice", "none", choices=lambda ctx: ["none", ctx]),
    )
    defaults = {"slot": 3, "source": None, "graph": True, "clone_db": "none"}

    assert prompt.ask(declared, defaults, prompter("5\n"), "b") == {"slot": 5}


def test_ask_uses_callable_choices_with_context():
    declared = (Option("clone_db", "choice", "none", choices=lambda ctx: ["none", ctx]),)

    assert prompt.ask(declared, {"clone_db": "none"}, prompter("2\n"), "b") == {"clone_db": "b"}


def test_prompt_text_goes_to_output():
    out = io.StringIO()

    prompt.Prompter(io.StringIO("\n"), out).number("Slot", 9)

    assert out.getvalue() == "? Slot (9) › "


def test_open_tty_works_under_pseudo_terminal():
    pid, fd = pty.fork()
    if pid == 0:
        tty = prompt.open_tty()
        ok = tty is not None and tty.reader.isatty() and tty.writer.isatty()
        os._exit(0 if ok else 3)
    _, status = os.waitpid(pid, 0)
    os.close(fd)

    assert os.waitstatus_to_exitcode(status) == 0


def test_open_tty_without_terminal_is_none():
    code = subprocess.run(
        [
            sys.executable,
            "-c",
            "import sys; from git_worktree_init import prompt; sys.exit(0 if prompt.open_tty() is None else 3)",
        ],
        cwd=CORE,
        start_new_session=True,
    ).returncode

    assert code == 0
