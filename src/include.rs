use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::gitenv::{self, Env};

const CHUNK: usize = 500;

pub fn include_file(source: &Path, main: &Path) -> PathBuf {
    let own = source.join(".worktreeinclude");
    if own.is_file() {
        own
    } else {
        main.join(".worktreeinclude")
    }
}

fn patterns(spec: &Path) -> Result<(Vec<String>, Vec<String>)> {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for raw in fs::read_to_string(spec)?.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line.strip_prefix('!') {
            Some(pattern) => negative.push(pattern.to_string()),
            None => positive.push(line.to_string()),
        }
    }
    Ok((positive, negative))
}

fn split_paths(output: &str) -> Vec<String> {
    output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_string)
        .collect()
}

fn matching(
    source: &Path,
    env: &Env,
    selectors: &[String],
    paths: &[String],
) -> Result<Vec<String>> {
    let excludes: Vec<String> = selectors.iter().map(|p| format!("--exclude={p}")).collect();
    let mut args = vec!["ls-files", "-z", "--others", "--ignored"];
    args.extend(excludes.iter().map(String::as_str));
    if paths.is_empty() {
        return Ok(split_paths(&gitenv::git(source, &args, env)?));
    }
    let mut result = Vec::new();
    for chunk in paths.chunks(CHUNK) {
        let literals: Vec<String> = chunk.iter().map(|p| format!(":(literal){p}")).collect();
        let mut chunk_args = args.clone();
        chunk_args.push("--");
        chunk_args.extend(literals.iter().map(String::as_str));
        result.extend(split_paths(&gitenv::git(source, &chunk_args, env)?));
    }
    Ok(result)
}

fn candidates(
    source: &Path,
    env: &Env,
    positive: &[String],
    negative: &[String],
) -> Result<Vec<String>> {
    let found = matching(source, env, positive, &[])?;
    if found.is_empty() || negative.is_empty() {
        return Ok(found);
    }
    let dropped: HashSet<String> = matching(source, env, negative, &found)?
        .into_iter()
        .collect();
    Ok(found
        .into_iter()
        .filter(|path| !dropped.contains(path))
        .collect())
}

fn ignored_among(source: &Path, env: &Env, candidates: &[String]) -> Result<Vec<String>> {
    let input = format!("{}\0", candidates.join("\0"));
    let checked = gitenv::run(
        &["git", "check-ignore", "-z", "--stdin"],
        source,
        env,
        Some(input.as_bytes()),
    )?;
    let code = checked.status.code().unwrap_or(-1);
    if code != 0 && code != 1 {
        let stderr = String::from_utf8_lossy(&checked.stderr);
        return Err(Error::Other(format!(
            "git check-ignore failed ({code}): {}",
            stderr.trim()
        )));
    }
    let mut paths = split_paths(&String::from_utf8_lossy(&checked.stdout));
    paths.sort();
    Ok(paths)
}

pub fn included(source: &Path, env: &Env, spec: &Path) -> Result<Vec<String>> {
    if !spec.is_file() {
        return Ok(Vec::new());
    }
    let (positive, negative) = patterns(spec)?;
    if positive.is_empty() {
        return Ok(Vec::new());
    }
    let found = candidates(source, env, &positive, &negative)?;
    if found.is_empty() {
        return Ok(Vec::new());
    }
    ignored_among(source, env, &found)
}
