from collections.abc import Callable, Iterable, Mapping
from pathlib import Path


class SlotError(ValueError):
    pass


def parse_reserved(raw: str | None) -> set[int]:
    if raw is None:
        return {0}
    tokens = raw.split()
    if not all(token.isdigit() for token in tokens):
        raise SlotError(f"reservedSlots must list numbers, got {raw!r}")
    return {int(token) for token in tokens}


def used(
    slot_of: Callable[[Path], int | None], worktrees: Iterable[Path], target: Path
) -> dict[int, Path]:
    result = {}
    for worktree in worktrees:
        if worktree == target:
            continue
        slot = slot_of(worktree)
        if slot is not None:
            result[slot] = worktree
    return result


def allocate(used: Mapping[int, Path], reserved: set[int]) -> int:
    candidate = 0
    while candidate in used or candidate in reserved:
        candidate += 1
    return candidate


def validate(slot: int, used: Mapping[int, Path], reserved: set[int]) -> None:
    if slot in reserved:
        raise SlotError(f"slot {slot} is reserved (worktree-init.reservedSlots)")
    if slot in used:
        raise SlotError(f"slot {slot} is taken by {used[slot]}")
