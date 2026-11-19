use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File, OpenOptions};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use crate::engine;
use crate::error::{Error, Result};
use crate::gitenv::{self, Env};
use crate::worktrees;

pub const DEFAULT_BRANCH: &str = "develop";
pub const REMOTE: &str = "harness";
pub const ANCHOR: &str = "harness-anchor";
pub const UNATTACHED: &str = "refs/heads/wt-unattached";
pub const ATTACH_PENDING: &str = "wt-attach-pending";
pub const ATTACH_LOCK: &str = "attach.lock";
pub const SNAPSHOT_SUBJECT: &str = "Snapshot harness state";
pub const NON_INTERACTIVE_SSH: &str = "ssh -o BatchMode=yes -o ConnectTimeout=10";
pub const UPSTREAM: &str = "refs/remotes/origin/develop";
pub const FEATURE_DIRS: [&str; 2] = [".superpowers/sdd/", ".claude/handoffs/"];
pub const RUNTIME_MARKER: &str = "# claude-code-runtime";
pub const ALLOWLIST: [&str; 10] = [
    "/*",
    "!/CLAUDE.md",
    "!/.claude/",
    "!/.superpowers/",
    "!/docs/",
    "/docs/*",
    "!/docs/superpowers/",
    "/.claude/settings.local.json",
    "__pycache__/",
    ".DS_Store",
];
pub const OVERRIDES: [(&str, &str); 5] = [
    ("commit.gpgsign", "false"),
    ("hook.beams.enabled", "false"),
    ("hook.worktree-init.enabled", "false"),
    ("hook.harness.enabled", "false"),
    ("core.excludesFile", "/dev/null"),
];

pub fn git_dir(common: &Path) -> PathBuf {
    common.join("harness.git")
}

pub fn enabled(common: &Path) -> bool {
    git_dir(common).is_dir()
}

pub fn git(target: &Path, args: &[&str], env: &Env) -> Result<String> {
    let (full, clean) = harness_call(target, args, env)?;
    gitenv::git(target, &as_strs(&full), &clean)
}

pub fn git_unchecked(target: &Path, args: &[&str], env: &Env) -> Result<String> {
    let (full, clean) = harness_call(target, args, env)?;
    gitenv::git_status(target, &as_strs(&full), &clean).map(|(_, stdout, _)| stdout)
}

pub fn init(main: &Path, remote: &str, env: &Env) -> Result<()> {
    let clean = gitenv::sanitized(env, &[]);
    let (_, common) = code_dirs(main, &clean)?;
    let root = git_dir(&common);
    let root_arg = root.to_string_lossy();
    let create = ["init", "-q", "--bare", "-b", DEFAULT_BRANCH, &root_arg];
    gitenv::git(main, &create, &clean)?;
    gitenv::git(&root, &["remote", "add", REMOTE, remote], &clean)?;
    for (key, value) in OVERRIDES {
        gitenv::git(&root, &["config", key, value], &clean)?;
    }
    let mut lines: Vec<String> = ALLOWLIST.iter().map(|line| line.to_string()).collect();
    lines.extend(runtime_patterns(&common.join("info").join("exclude"))?);
    fs::create_dir_all(root.join("info"))?;
    fs::write(root.join("info").join("exclude"), lines.join("\n") + "\n")?;
    Ok(())
}

pub fn base_of(target: &Path, branch: &str, env: &Env) -> Result<String> {
    let clean = gitenv::sanitized(env, &[]);
    let merge_base = config(target, &format!("branch.{branch}.t3-merge-base"), &clean)?;
    if !merge_base.is_empty() {
        return Ok(merge_base);
    }
    let merge = config(target, &format!("branch.{branch}.merge"), &clean)?;
    let upstream = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
    if !upstream.is_empty() && upstream != branch {
        return Ok(upstream.to_string());
    }
    Ok(DEFAULT_BRANCH.to_string())
}

pub fn attach(target: &Path, branch: &str, env: &Env) -> Result<()> {
    let fresh = !index(target, env)?.exists();
    let local = format!("refs/heads/{branch}");
    if !exists(target, &local, env)? {
        let Some(start) = start_point(target, branch, env)? else {
            git(target, &["symbolic-ref", "HEAD", &local], env)?;
            return Ok(());
        };
        git(target, &["branch", "--no-track", branch, &start], env)?;
    }
    let _lock = attach_lock(target, env)?;
    release_unattached(target, env)?;
    let mut checkout = vec!["checkout", "-q"];
    if fresh {
        checkout.push("-f");
    }
    checkout.push(branch);
    git(target, &checkout, env).map(drop)
}

pub fn attach_late(target: &Path, env: &Env) -> Result<bool> {
    let pending = pending(target, env)?;
    if pending.exists() {
        resume_attach(target, &pending, env)?;
        return Ok(true);
    }
    if index(target, env)?.exists() {
        return Ok(true);
    }
    let args = ["symbolic-ref", "-q", "--short", "HEAD"];
    let clean = gitenv::sanitized(env, &[]);
    let (_, branch, _) = gitenv::git_status(target, &args, &clean)?;
    let branch = branch.trim();
    if branch.is_empty() {
        return Ok(false);
    }
    let local = format!("refs/heads/{branch}");
    if !exists(target, &local, env)? {
        let Some(start) = start_point(target, branch, env)? else {
            return Ok(false);
        };
        git(target, &["branch", "--no-track", branch, &start], env)?;
    }
    {
        let _lock = attach_lock(target, env)?;
        release_unattached(target, env)?;
        git(target, &["symbolic-ref", "HEAD", &local], env)?;
        fs::write(&pending, "")?;
        git(target, &["reset", "-q"], env)?;
    }
    finish_attach(target, &pending, env)?;
    Ok(true)
}

pub fn snapshot(target: &Path, env: &Env) -> Result<bool> {
    let index = index(target, env)?;
    if !index.exists() {
        return Ok(false);
    }
    if index.with_file_name("MERGE_HEAD").exists() {
        let shown = target.display();
        eprintln!("worktree-init: harness merge is still open in {shown}, snapshot skipped");
        return Ok(false);
    }
    if index.with_file_name(ATTACH_PENDING).exists() {
        let shown = target.display();
        eprintln!("worktree-init: harness attach is unfinished in {shown}, snapshot skipped");
        return Ok(false);
    }
    if every_file_missing(target, env)? {
        let shown = target.display();
        eprintln!("worktree-init: every harness file is missing in {shown}, snapshot skipped");
        return Ok(false);
    }
    git(target, &["add", "-A"], env)?;
    add_hidden(target, env)?;
    let status = git(
        target,
        &["status", "--porcelain", "--untracked-files=no"],
        env,
    )?;
    if status.trim().is_empty() {
        return Ok(false);
    }
    git(
        target,
        &["commit", "-q", "--no-verify", "-m", SNAPSHOT_SUBJECT],
        env,
    )?;
    Ok(true)
}

pub fn switch(target: &Path, branch: &str, env: &Env) -> Result<()> {
    let pending = pending(target, env)?;
    if pending.exists() {
        resume_attach(target, &pending, env)?;
    }
    if current_branch(target, env)? == branch && index(target, env)?.exists() {
        return Ok(());
    }
    snapshot(target, env)?;
    attach(target, branch, env)
}

pub fn push(target: &Path, env: &Env) -> Result<bool> {
    let branch = current_branch(target, env)?;
    let refspec = format!("refs/heads/{branch}:refs/heads/{branch}");
    match git(
        target,
        &["push", "-q", REMOTE, &refspec],
        &offline_safe(env),
    ) {
        Ok(_) => Ok(true),
        Err(Error::Git { code, stderr, .. }) => {
            let reason = last_line(&stderr)
                .map_or_else(|| format!("exit {}", exit_code(code)), str::to_string);
            eprintln!("worktree-init: harness push failed: {reason}");
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub fn submitted(main: &Path, env: &Env) -> Result<Vec<String>> {
    let clean = gitenv::sanitized(env, &[]);
    let upstream = verified(main, UPSTREAM, &clean)?;
    if upstream.is_empty() {
        return Ok(Vec::new());
    }
    let first_parents = gitenv::git(main, &["rev-list", "--first-parent", &upstream], &clean)?;
    let landed: HashSet<&str> = first_parents.split_whitespace().collect();
    let busy = checked_out(main, &clean)?;
    let heads = git(
        main,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        env,
    )?;
    let mut result = Vec::new();
    for branch in heads.split_whitespace() {
        if branch == DEFAULT_BRANCH || busy.contains(branch) {
            continue;
        }
        let tip = verified(main, &format!("refs/heads/{branch}^{{commit}}"), &clean)?;
        if tip.is_empty()
            || landed.contains(tip.as_str())
            || !is_ancestor(main, &tip, &upstream, &clean)?
        {
            continue;
        }
        if has_own_commits(main, branch, env)? {
            result.push(branch.to_string());
        }
    }
    Ok(result)
}

pub fn feature_paths(main: &Path, branch: &str, env: &Env) -> Result<Vec<String>> {
    let base = git(main, &["merge-base", DEFAULT_BRANCH, branch], env)?;
    let base = base.trim();
    let mut diff = vec![
        "diff",
        "-z",
        "--name-only",
        "--diff-filter=A",
        base,
        branch,
        "--",
    ];
    diff.extend(FEATURE_DIRS);
    let added = git(main, &diff, env)?;
    let ledgers = FEATURE_DIRS[0];
    let tree = git(
        main,
        &["ls-tree", "-d", "--name-only", base, "--", ledgers],
        env,
    )?;
    let existing: HashSet<&str> = tree.split_whitespace().collect();
    let paths: BTreeSet<String> = added
        .split('\0')
        .filter(|rel| !rel.is_empty())
        .map(|rel| {
            let ledger = rel.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
            if rel.starts_with(ledgers) && !existing.contains(ledger.as_str()) {
                ledger
            } else {
                rel.to_string()
            }
        })
        .collect();
    Ok(paths.into_iter().collect())
}

pub fn merge(main: &Path, branch: &str, env: &Env) -> Result<bool> {
    let (_, common) = code_dirs(main, &gitenv::sanitized(env, &[]))?;
    let merged = {
        let _lock = engine::locked(&common)?;
        merge_locked(main, branch, &git_dir(&common), env)?
    };
    if merged {
        eprintln!("worktree-init: merged harness branch {branch} into {DEFAULT_BRANCH}");
    }
    Ok(merged)
}

fn merge_locked(main: &Path, branch: &str, root: &Path, env: &Env) -> Result<bool> {
    let shown = main.display();
    if root.join("MERGE_HEAD").exists() {
        eprintln!(
            "worktree-init: harness merge is still open in {shown}, resolve it before merging {branch}"
        );
        return Ok(false);
    }
    let current = current_branch(main, env)?;
    if current != DEFAULT_BRANCH {
        let on = if current.is_empty() {
            "a detached HEAD"
        } else {
            &current
        };
        eprintln!("worktree-init: harness of {shown} is on {on}, not merging {branch}");
        return Ok(false);
    }
    let paths = feature_paths(main, branch, env)?;
    match git(
        main,
        &["merge", "-q", "--no-ff", "--no-commit", branch],
        env,
    ) {
        Ok(_) => {}
        Err(Error::Git { code, stderr, .. }) => {
            if root.join("MERGE_HEAD").exists() {
                eprintln!(
                    "worktree-init: harness merge of {branch} has a conflict, resolve it in {shown}"
                );
            } else {
                let reason = last_line(&stderr).map_or_else(|| exit_code(code), str::to_string);
                eprintln!("worktree-init: harness merge of {branch} failed: {reason}");
            }
            return Ok(false);
        }
        Err(error) => return Err(error),
    }
    if !paths.is_empty() {
        let mut remove = vec!["rm", "-r", "-q", "-f", "--ignore-unmatch", "--"];
        remove.extend(paths.iter().map(String::as_str));
        git(main, &remove, env)?;
    }
    let message = format!("Merge harness branch '{branch}'");
    git(main, &["commit", "-q", "--no-verify", "-m", &message], env)?;
    Ok(true)
}

fn has_own_commits(main: &Path, branch: &str, env: &Env) -> Result<bool> {
    let range = format!("{DEFAULT_BRANCH}..{branch}");
    Ok(!git(main, &["rev-list", "-1", &range], env)?
        .trim()
        .is_empty())
}

fn verified(cwd: &Path, reference: &str, env: &Env) -> Result<String> {
    let (_, stdout, _) = gitenv::git_status(cwd, &["rev-parse", "-q", "--verify", reference], env)?;
    Ok(stdout.trim().to_string())
}

fn checked_out(main: &Path, env: &Env) -> Result<HashSet<String>> {
    let listing = gitenv::git(main, &["worktree", "list", "--porcelain"], env)?;
    Ok(listing
        .lines()
        .filter(|line| line.starts_with("branch "))
        .map(|line| line.trim_start_matches("branch refs/heads/").to_string())
        .collect())
}

fn is_ancestor(main: &Path, commit: &str, reference: &str, env: &Env) -> Result<bool> {
    let args = ["merge-base", "--is-ancestor", commit, reference];
    Ok(gitenv::git_status(main, &args, env)?.0 == 0)
}

fn last_line(stderr: &str) -> Option<&str> {
    stderr.trim().lines().last()
}

fn exit_code(code: Option<i32>) -> String {
    code.map_or_else(|| "signal".to_string(), |code| code.to_string())
}

fn every_file_missing(target: &Path, env: &Env) -> Result<bool> {
    let tracked = git(target, &["ls-files"], env)?.lines().count();
    let deleted = git(target, &["ls-files", "--deleted"], env)?
        .lines()
        .count();
    Ok(tracked > 0 && deleted == tracked)
}

fn add_hidden(target: &Path, env: &Env) -> Result<()> {
    let exclude_from = format!("--exclude-from={}", exclude(target, env)?.display());
    let listing = [
        "ls-files",
        "-z",
        "--others",
        &exclude_from,
        "--exclude-per-directory=.gitignore",
    ];
    let hidden = git(target, &listing, env)?;
    let paths: Vec<&str> = hidden.split('\0').filter(|path| !path.is_empty()).collect();
    if paths.is_empty() {
        return Ok(());
    }
    let mut add = vec!["add", "-f", "--"];
    add.extend(paths);
    git(target, &add, env).map(drop)
}

fn attach_lock(target: &Path, env: &Env) -> Result<File> {
    let (_, common) = code_dirs(target, &gitenv::sanitized(env, &[]))?;
    let handle = File::create(git_dir(&common).join(ATTACH_LOCK))?;
    handle.lock()?;
    Ok(handle)
}

fn pending(target: &Path, env: &Env) -> Result<PathBuf> {
    Ok(index(target, env)?.with_file_name(ATTACH_PENDING))
}

fn resume_attach(target: &Path, pending: &Path, env: &Env) -> Result<()> {
    if !index(target, env)?.exists() {
        let _lock = attach_lock(target, env)?;
        git(target, &["reset", "-q"], env)?;
    }
    finish_attach(target, pending, env)
}

fn finish_attach(target: &Path, pending: &Path, env: &Env) -> Result<()> {
    restore_missing(target, env)?;
    fs::remove_file(pending)?;
    Ok(())
}

fn restore_missing(target: &Path, env: &Env) -> Result<()> {
    let listing = git(target, &["ls-files", "-z", "--deleted"], env)?;
    let paths: Vec<&str> = listing
        .split('\0')
        .filter(|path| !path.is_empty())
        .collect();
    if paths.is_empty() {
        return Ok(());
    }
    let mut checkout = vec!["--literal-pathspecs", "checkout", "-q", "--"];
    checkout.extend(paths);
    git(target, &checkout, env).map(drop)
}

fn release_unattached(target: &Path, env: &Env) -> Result<()> {
    let (_, common) = code_dirs(target, &gitenv::sanitized(env, &[]))?;
    let Ok(entries) = fs::read_dir(git_dir(&common).join("worktrees")) else {
        return Ok(());
    };
    for entry in entries {
        let admin = entry?.path();
        if is_unattached(&admin) {
            write_unattached_head(&admin)?;
        }
    }
    Ok(())
}

fn is_unattached(admin: &Path) -> bool {
    admin.join("HEAD").is_file()
        && !admin.join("index").exists()
        && !admin.join(ATTACH_PENDING).exists()
}

fn write_unattached_head(admin: &Path) -> Result<()> {
    let name = admin.file_name().unwrap_or_default().to_string_lossy();
    fs::write(admin.join("HEAD"), format!("ref: {UNATTACHED}/{name}\n"))?;
    Ok(())
}

fn start_point(target: &Path, branch: &str, env: &Env) -> Result<Option<String>> {
    fetch(target, branch, env)?;
    let fetched = format!("refs/remotes/{REMOTE}/{branch}");
    if exists(target, &fetched, env)? {
        return Ok(Some(fetched));
    }
    let mut refs = candidates(&base_of(target, branch, env)?).to_vec();
    refs.extend(candidates(DEFAULT_BRANCH));
    first_existing(target, &refs, env)
}

fn current_branch(target: &Path, env: &Env) -> Result<String> {
    let output = git_unchecked(target, &["symbolic-ref", "-q", "--short", "HEAD"], env)?;
    Ok(output.trim().to_string())
}

fn candidates(branch: &str) -> [String; 2] {
    [
        format!("refs/heads/{branch}"),
        format!("refs/remotes/{REMOTE}/{branch}"),
    ]
}

fn first_existing(target: &Path, refs: &[String], env: &Env) -> Result<Option<String>> {
    for reference in refs {
        if exists(target, reference, env)? {
            return Ok(Some(reference.clone()));
        }
    }
    Ok(None)
}

fn exists(target: &Path, reference: &str, env: &Env) -> Result<bool> {
    let commit = format!("{reference}^{{commit}}");
    let output = git_unchecked(target, &["rev-parse", "-q", "--verify", &commit], env)?;
    Ok(!output.trim().is_empty())
}

fn fetch(target: &Path, branch: &str, env: &Env) -> Result<()> {
    let refspec = format!("refs/heads/{branch}:refs/remotes/{REMOTE}/{branch}");
    git_unchecked(
        target,
        &["fetch", "-q", REMOTE, &refspec],
        &offline_safe(env),
    )
    .map(drop)
}

fn offline_safe(env: &Env) -> Env {
    let mut safe = env.clone();
    safe.entry("GIT_SSH_COMMAND".into())
        .or_insert_with(|| NON_INTERACTIVE_SSH.into());
    safe
}

fn index(target: &Path, env: &Env) -> Result<PathBuf> {
    Ok(admin_dir(target, &gitenv::sanitized(env, &[]))?.join("index"))
}

fn exclude(target: &Path, env: &Env) -> Result<PathBuf> {
    let (_, common) = code_dirs(target, &gitenv::sanitized(env, &[]))?;
    Ok(git_dir(&common).join("info").join("exclude"))
}

fn config(target: &Path, key: &str, env: &Env) -> Result<String> {
    let (_, stdout, _) = gitenv::git_status(target, &["config", "--get", key], env)?;
    Ok(stdout.trim().to_string())
}

fn harness_call(target: &Path, args: &[&str], env: &Env) -> Result<(Vec<String>, Env)> {
    let clean = gitenv::sanitized(env, &[]);
    let admin = admin_dir(target, &clean)?;
    let mut full = vec![
        format!("--git-dir={}", admin.to_string_lossy()),
        format!("--work-tree={}", target.to_string_lossy()),
    ];
    full.extend(args.iter().map(|arg| arg.to_string()));
    Ok((full, clean))
}

fn as_strs(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

fn code_dirs(target: &Path, env: &Env) -> Result<(PathBuf, PathBuf)> {
    let args = [
        "rev-parse",
        "--path-format=absolute",
        "--git-dir",
        "--git-common-dir",
    ];
    let output = gitenv::git(target, &args, env)?;
    let mut lines = output
        .lines()
        .map(|line| worktrees::resolve(Path::new(line)));
    match (lines.next(), lines.next()) {
        (Some(own), Some(common)) => Ok((own, common)),
        _ => Err(Error::Other("unexpected output of git rev-parse".into())),
    }
}

fn admin_dir(target: &Path, env: &Env) -> Result<PathBuf> {
    let (own, common) = code_dirs(target, env)?;
    let root = git_dir(&common);
    if own == common {
        return Ok(root);
    }
    let admin = root
        .join("worktrees")
        .join(own.file_name().unwrap_or_default());
    let anchor = own.join(ANCHOR);
    if admin.is_dir() && !anchor.exists() {
        fs::remove_dir_all(&admin)?;
    }
    if !admin.is_dir() {
        gitenv::git(&root, &["worktree", "prune"], env)?;
        create_admin(&admin, &anchor)?;
    }
    Ok(admin)
}

fn create_admin(admin: &Path, anchor: &Path) -> Result<()> {
    OpenOptions::new().create(true).append(true).open(anchor)?;
    fs::create_dir_all(admin)?;
    let mut gitdir = anchor.as_os_str().as_bytes().to_vec();
    gitdir.push(b'\n');
    fs::write(admin.join("gitdir"), gitdir)?;
    fs::write(admin.join("commondir"), "../..\n")?;
    write_unattached_head(admin)
}

fn runtime_patterns(exclude: &Path) -> Result<Vec<String>> {
    if !exclude.is_file() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(exclude)?;
    Ok(content
        .lines()
        .skip_while(|line| *line != RUNTIME_MARKER)
        .skip(1)
        .take_while(|line| !line.trim().is_empty())
        .filter(|line| !line.starts_with('#'))
        .map(str::to_string)
        .collect())
}
