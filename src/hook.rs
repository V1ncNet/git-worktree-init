use std::path::{Path, PathBuf};

use crate::config::{self, Entries};
use crate::engine;
use crate::gitenv::{self, Env};
use crate::worktrees;

pub fn run(args: &[String], env: &Env) -> i32 {
    match new_worktree(args, env) {
        Some(target) => engine::execute_or_explain(&target, "create", env),
        None => 0,
    }
}

fn new_worktree(args: &[String], env: &Env) -> Option<PathBuf> {
    if !is_fresh_checkout(args) || !is_branch_checkout(args) {
        return None;
    }
    let clean = gitenv::sanitized(env, &[]);
    let cwd = std::env::current_dir().ok()?;
    let repo = worktrees::discover(&cwd, &clean).ok()?;
    repo.admin_dir.as_ref()?;
    let here = worktrees::toplevel(&cwd, &clean).ok()?;
    let entries = config::read(&here, &clean).ok()?;
    if config::path_ignored(&here, &config::ignore_patterns(&entries)) {
        return None;
    }
    wants_preparation(&repo.main, &entries).then_some(here)
}

fn is_fresh_checkout(args: &[String]) -> bool {
    args.first()
        .is_none_or(|previous| previous.chars().all(|character| character == '0'))
}

fn is_branch_checkout(args: &[String]) -> bool {
    args.get(2).is_some_and(|flag| flag == "1")
}

fn wants_preparation(main: &Path, entries: &Entries) -> bool {
    let plugin = config::last(entries, "plugin").is_some_and(|value| !value.is_empty());
    let local_include = config::all_values(entries, "localInclude")
        .iter()
        .any(|value| !value.is_empty());
    plugin || main.join(".worktreeinclude").is_file() || local_include
}
