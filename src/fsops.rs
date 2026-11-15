use std::fs::{self, File};
use std::path::Path;

use crate::error::Result;

pub fn copy(rel: &str, source_root: &Path, target_root: &Path, overwrite: bool) -> Result<bool> {
    let origin = source_root.join(rel);
    if origin.is_dir() {
        return copy_tree(&origin, &target_root.join(rel));
    }
    if !origin.is_file() {
        return Ok(false);
    }
    copy_file(&origin, &target_root.join(rel), overwrite)
}

fn copy_tree(origin: &Path, destination: &Path) -> Result<bool> {
    let mut changed = false;
    for entry in fs::read_dir(origin)? {
        let entry = entry?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            changed |= copy_tree(&from, &to)?;
        } else if !from.is_dir() {
            changed |= copy_file(&from, &to, true)?;
        }
    }
    Ok(changed)
}

fn copy_file(origin: &Path, destination: &Path, overwrite: bool) -> Result<bool> {
    if destination.is_file() && (!overwrite || same_content(origin, destination)?) {
        return Ok(false);
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(origin, destination)?;
    File::open(destination)?.set_modified(fs::metadata(origin)?.modified()?)?;
    Ok(true)
}

fn same_content(first: &Path, second: &Path) -> Result<bool> {
    let (first_meta, second_meta) = (fs::metadata(first)?, fs::metadata(second)?);
    Ok(first_meta.len() == second_meta.len() && fs::read(first)? == fs::read(second)?)
}
