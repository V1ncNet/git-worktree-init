use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::{Error, Result};
use crate::gitenv::{self, Env};
use crate::glob;

const PREFIX: &str = "worktree-init.";
const DEFAULT_IGNORE_PATHS: [&str; 1] = ["*/.claude/worktrees/*"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub scope: String,
    pub value: String,
}

pub type Entries = BTreeMap<String, Vec<Entry>>;

pub fn read(cwd: &Path, env: &Env) -> Result<Entries> {
    let args = [
        "config",
        "--show-scope",
        "--get-regexp",
        r"^worktree-init\.",
    ];
    let (code, stdout, stderr) = gitenv::git_status(cwd, &args, env)?;
    if code != 0 && code != 1 {
        return Err(Error::Other(format!(
            "git config failed ({code}): {}",
            stderr.trim()
        )));
    }
    let mut entries = Entries::new();
    for line in stdout.lines() {
        let (scope, rest) = line.split_once('\t').unwrap_or((line, ""));
        let (key, value) = rest.split_once(' ').unwrap_or((rest, ""));
        let key = key.strip_prefix(PREFIX).unwrap_or(key);
        entries.entry(key.to_string()).or_default().push(Entry {
            scope: scope.to_string(),
            value: value.to_string(),
        });
    }
    Ok(entries)
}

pub fn last<'a>(entries: &'a Entries, key: &str) -> Option<&'a str> {
    let values = entries.get(&key.to_lowercase())?;
    values.last().map(|entry| entry.value.as_str())
}

pub fn all_values(entries: &Entries, key: &str) -> Vec<String> {
    entries
        .get(&key.to_lowercase())
        .map(|values| values.iter().map(|entry| entry.value.clone()).collect())
        .unwrap_or_default()
}

pub fn command_line_keys(entries: &Entries) -> BTreeSet<String> {
    entries
        .iter()
        .filter(|(_, values)| values.iter().any(|entry| entry.scope == "command"))
        .map(|(key, _)| key.clone())
        .collect()
}

pub fn ignore_patterns(entries: &Entries) -> Vec<String> {
    if !entries.contains_key("ignorepaths") {
        return DEFAULT_IGNORE_PATHS.map(String::from).to_vec();
    }
    all_values(entries, "ignorePaths")
        .into_iter()
        .filter(|pattern| !pattern.is_empty())
        .collect()
}

pub fn path_ignored(path: &Path, patterns: &[String]) -> bool {
    let candidate = format!("{}/", path.to_string_lossy());
    patterns
        .iter()
        .any(|pattern| glob::matches(pattern, &candidate))
}
