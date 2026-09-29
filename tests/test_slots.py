from pathlib import Path

import pytest

from git_worktree_init import slots


def test_allocate_takes_smallest_free_outside_reserved():
    used = {1: Path("/a"), 3: Path("/b")}

    assert slots.allocate(used, slots.parse_reserved("0 2")) == 4


def test_reserved_defaults_to_zero_and_can_be_emptied():
    assert slots.parse_reserved(None) == {0}
    assert slots.parse_reserved("") == set()


def test_used_skips_target_and_unknown():
    table = {Path("/a"): 1, Path("/b"): None, Path("/t"): 5}

    assert slots.used(table.get, list(table), Path("/t")) == {1: Path("/a")}


def test_validate_rejects_taken_and_reserved_slots():
    with pytest.raises(slots.SlotError, match="/a"):
        slots.validate(1, {1: Path("/a")}, set())
    with pytest.raises(slots.SlotError, match="reserved"):
        slots.validate(2, {}, {0, 2})


def test_parse_reserved_rejects_garbage():
    with pytest.raises(slots.SlotError):
        slots.parse_reserved("0 x")
