import os
import pty
import select
import signal
import time
from pathlib import Path

import pytest

from helpers import CORE, EXAMPLE_PLUGIN, git

ZEROS = "0" * 40
PROMPT = "? Slot of the worktree (1) › ".encode()
DEADLINE = 30


def binary() -> Path:
    path = Path(os.environ.get("GIT_WT_BIN") or CORE / "target" / "debug" / "git-wt")
    if not path.is_file():
        pytest.fail(f"git-wt binary not found at {path}, run cargo build first")
    return path


class Session:
    def __init__(self, args, cwd, env):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            try:
                os.chdir(cwd)
                os.execve(str(binary()), [str(binary()), *args], env)
            finally:
                os._exit(127)
        self.output = b""
        self.started = time.monotonic()

    def _read_chunk(self) -> bool:
        remaining = DEADLINE - (time.monotonic() - self.started)
        if remaining <= 0:
            os.kill(self.pid, signal.SIGKILL)
            pytest.fail(f"git-wt did not finish in time, output so far: {self.output!r}")
        ready, _, _ = select.select([self.fd], [], [], remaining)
        if not ready:
            return True
        try:
            chunk = os.read(self.fd, 4096)
        except OSError:
            return False
        self.output += chunk
        return bool(chunk)

    def expect(self, needle: bytes) -> None:
        while needle not in self.output:
            if not self._read_chunk():
                pytest.fail(f"expected {needle!r}, got {self.output!r}")

    def send(self, data: bytes) -> None:
        os.write(self.fd, data)

    def finish(self) -> int:
        while self._read_chunk():
            pass
        _, status = os.waitpid(self.pid, 0)
        os.close(self.fd)
        return os.waitstatus_to_exitcode(status)


@pytest.fixture
def configured(repo):
    git(repo, "config", "worktree-init.plugin", str(EXAMPLE_PLUGIN))
    git(repo, "config", "worktree-init.reservedSlots", "0")
    git(repo, "worktree", "add", "-q", "-b", "w", "../w")
    return repo, (repo.parent / "w").resolve()


def prompting_env(**extra):
    env = {key: value for key, value in os.environ.items() if key != "GIT_TERMINAL_PROMPT"}
    env.update(extra)
    return env


def create(target, env) -> Session:
    return Session(["hook", ZEROS, ZEROS, "1"], target, env)


def test_interactive_answers_win_over_defaults(configured):
    _, target = configured
    session = create(target, prompting_env())

    session.expect(PROMPT)
    session.send(b"4\n")
    session.expect("? First port (8000) › ".encode())
    session.send(b"9000\n")

    assert session.finish() == 0
    assert (target / ".env").read_text() == "SLOT=4\nPORT=9004\n"


def test_env_option_disables_prompt(configured):
    _, target = configured

    session = create(target, prompting_env(WT_PORT_BASE="7000"))

    assert session.finish() == 0
    assert b"?" not in session.output
    assert (target / ".env").read_text() == "SLOT=1\nPORT=7001\n"


def test_terminal_prompt_off_disables_prompt(configured):
    _, target = configured

    session = create(target, dict(os.environ, GIT_TERMINAL_PROMPT="0"))

    assert session.finish() == 0
    assert b"?" not in session.output
    assert (target / ".env").read_text() == "SLOT=1\nPORT=8001\n"


def test_local_interactive_false_disables_prompt(configured):
    repo, target = configured
    git(repo, "config", "worktree-init.interactive", "false")

    session = create(target, prompting_env())

    assert session.finish() == 0
    assert b"?" not in session.output
    assert (target / ".env").read_text() == "SLOT=1\nPORT=8001\n"


def assert_aborted(session, target):
    output = session.output.decode()
    assert "worktree-init: aborted" in output
    assert f"worktree-init: the worktree exists, repair it with: git wt refresh {target}" in output
    assert target.is_dir()


def test_ctrl_c_at_prompt_aborts_with_repair_hint(configured):
    _, target = configured
    session = create(target, prompting_env())

    session.expect(PROMPT)
    session.send(b"\x03")

    assert session.finish() == 130
    assert_aborted(session, target)
    assert not (target / ".env").exists()


def test_ctrl_c_during_apply_waits_for_plugin(configured, tmp_path):
    repo, target = configured
    unwound = tmp_path / "unwound"
    plugin = tmp_path / "slow.py"
    plugin.write_text(
        "import time\n\n"
        "def slot_of(worktree):\n    return None\n\n"
        "def apply(ctx):\n"
        "    try:\n"
        "        print('applying', flush=True)\n"
        "        time.sleep(20)\n"
        "    finally:\n"
        "        time.sleep(0.5)\n"
        f"        open({str(unwound)!r}, 'w').write('done')\n"
    )
    git(repo, "config", "worktree-init.plugin", str(plugin))
    session = Session(["refresh", str(target)], repo, dict(os.environ))

    session.expect(b"applying")
    session.send(b"\x03")

    assert session.finish() == 130
    assert_aborted(session, target)
    assert unwound.read_text() == "done"
