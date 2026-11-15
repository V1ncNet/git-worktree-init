use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};
use crate::gitenv::{self, Env};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub main: PathBuf,
    pub common_dir: PathBuf,
    pub worktrees: Vec<PathBuf>,
    pub admin_dir: Option<PathBuf>,
}

pub fn resolve(path: &Path) -> PathBuf {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let mut existing = absolute.as_path();
    let mut tail = Vec::new();
    let mut resolved = loop {
        if let Ok(canonical) = existing.canonicalize() {
            break canonical;
        }
        match (existing.file_name(), existing.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name.to_owned());
                existing = parent;
            }
            _ => break existing.to_path_buf(),
        }
    };
    for name in tail.iter().rev() {
        resolved.push(name);
    }
    normalize_missing_tail(&resolved)
}

fn normalize_missing_tail(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized
}

pub fn discover(cwd: &Path, env: &Env) -> Result<Repo> {
    let dirs = gitenv::git(
        cwd,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--git-dir",
        ],
        env,
    )?;
    let [common, own] = dirs
        .lines()
        .map(|line| resolve(Path::new(line)))
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| Error::Other("unexpected output of git rev-parse".into()))?;
    let listing = gitenv::git(cwd, &["worktree", "list", "--porcelain"], env)?;
    let blocks: Vec<Vec<&str>> = listing
        .trim()
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .map(|block| block.lines().collect())
        .collect();
    let paths: Vec<PathBuf> = blocks.iter().map(|block| worktree_path(block)).collect();
    let main = paths
        .first()
        .cloned()
        .ok_or_else(|| Error::Other("git worktree list printed no worktrees".into()))?;
    let worktrees = paths
        .into_iter()
        .zip(&blocks)
        .filter(|(_, block)| is_usable(block))
        .map(|(path, _)| path)
        .collect();
    let admin_dir = (own != common).then_some(own);
    Ok(Repo {
        main,
        common_dir: common,
        worktrees,
        admin_dir,
    })
}

fn worktree_path(block: &[&str]) -> PathBuf {
    let first = block.first().copied().unwrap_or_default();
    resolve(Path::new(first.strip_prefix("worktree ").unwrap_or(first)))
}

fn is_usable(block: &[&str]) -> bool {
    !block.contains(&"bare") && !block.iter().any(|line| line.starts_with("prunable"))
}

pub fn toplevel(cwd: &Path, env: &Env) -> Result<PathBuf> {
    let output = gitenv::git(cwd, &["rev-parse", "--show-toplevel"], env)?;
    Ok(resolve(Path::new(output.trim())))
}
