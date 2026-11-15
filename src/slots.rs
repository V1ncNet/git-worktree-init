use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::options::python_repr;

pub fn parse_reserved(raw: Option<&str>) -> Result<BTreeSet<u32>> {
    let Some(raw) = raw else {
        return Ok(BTreeSet::from([0]));
    };
    raw.split_whitespace()
        .map(|token| {
            token.parse::<u32>().map_err(|_| {
                Error::Other(format!(
                    "reservedSlots must list numbers, got {}",
                    python_repr(raw)
                ))
            })
        })
        .collect()
}

pub fn used(
    slot_of: &mut dyn FnMut(&Path) -> Result<Option<u32>>,
    worktrees: &[PathBuf],
    target: &Path,
) -> Result<BTreeMap<u32, PathBuf>> {
    let mut result = BTreeMap::new();
    for worktree in worktrees.iter().filter(|worktree| *worktree != target) {
        if let Some(slot) = slot_of(worktree)? {
            result.insert(slot, worktree.clone());
        }
    }
    Ok(result)
}

pub fn allocate(used: &BTreeMap<u32, PathBuf>, reserved: &BTreeSet<u32>) -> u32 {
    (0..)
        .find(|candidate| !used.contains_key(candidate) && !reserved.contains(candidate))
        .expect("an unbounded range has a free slot")
}

pub fn validate(slot: u32, used: &BTreeMap<u32, PathBuf>, reserved: &BTreeSet<u32>) -> Result<()> {
    if reserved.contains(&slot) {
        return Err(Error::Other(format!(
            "slot {slot} is reserved (worktree-init.reservedSlots)"
        )));
    }
    if let Some(holder) = used.get(&slot) {
        return Err(Error::Other(format!(
            "slot {slot} is taken by {}",
            holder.display()
        )));
    }
    Ok(())
}
