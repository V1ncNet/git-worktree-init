mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use common::Sandbox;
use git_wt::fsops;
use git_wt::gitenv::{self, Env};
use git_wt::include;

fn env_of(sandbox: &Sandbox) -> Env {
    let mut env = gitenv::sanitized(&gitenv::process_env(), &[]);
    env.extend(sandbox.env());
    env
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn selected(source: &Path, env: &Env) -> Vec<String> {
    include::included(source, env, &source.join(".worktreeinclude")).unwrap()
}

#[test]
fn only_ignored_matches_are_included() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    write(&repo.join(".env"), "A=1");
    write(&repo.join("notes.txt"), "untracked, not ignored");
    write(
        &repo.join(".worktreeinclude"),
        ".env\nnotes.txt\n.gitignore\n",
    );

    assert_eq!(selected(&repo, &env_of(&sandbox)), [".env"]);
}

#[test]
fn negations_apply_at_any_depth() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let files = [
        "a.xml",
        "workspace.xml",
        "shelf/s.xml",
        "sub/deep/x.corrupted.1",
        "sub/deep/keep.xml",
        "sub/.DS_Store",
    ];
    for file in files {
        write(&repo.join("local").join(file), file);
    }
    write(
        &repo.join(".worktreeinclude"),
        "# comment\nlocal/\n!local/workspace.xml\n!local/shelf/\n!*.corrupted.*\n!.DS_Store\n",
    );

    assert_eq!(
        selected(&repo, &env_of(&sandbox)),
        ["local/a.xml", "local/sub/deep/keep.xml"]
    );
}

#[test]
fn negation_wins_regardless_of_order() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    write(&repo.join("local/a.xml"), "a");
    write(&repo.join("local/b.xml"), "b");
    write(&repo.join(".worktreeinclude"), "!local/a.xml\nlocal/\n");

    assert_eq!(selected(&repo, &env_of(&sandbox)), ["local/b.xml"]);
}

#[test]
fn missing_file_includes_nothing() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();

    assert!(selected(&repo, &env_of(&sandbox)).is_empty());
}

#[test]
fn many_candidates_with_negations() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    for number in 0..3000 {
        let name = format!("local/many/file-with-a-long-name-{number:05}.xml");
        write(&repo.join(name), "x");
    }
    write(&repo.join("local/many/drop.corrupted.1"), "c");
    write(&repo.join(".worktreeinclude"), "local/\n!*.corrupted.*\n");

    let result = selected(&repo, &env_of(&sandbox));

    assert_eq!(result.len(), 3000);
    assert!(!result.contains(&"local/many/drop.corrupted.1".to_string()));
}

#[test]
fn source_without_list_uses_main_list() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    write(&repo.join(".worktreeinclude"), ".env\n");
    sandbox.git(&repo, &["worktree", "add", "-q", "-b", "src", "../src"]);
    let source = sandbox.root.join("src");
    write(&source.join(".env"), "FROM_SOURCE=1");

    let spec = include::include_file(&source, &repo);

    assert_eq!(spec, repo.join(".worktreeinclude"));
    let result = include::included(&source, &env_of(&sandbox), &spec).unwrap();
    assert_eq!(result, [".env"]);
}

#[test]
fn own_list_wins_over_main_list() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    let source = sandbox.root.join("src");
    write(&source.join(".worktreeinclude"), ".env\n");

    assert_eq!(
        include::include_file(&source, &repo),
        source.join(".worktreeinclude")
    );
}

#[test]
fn check_ignore_failure_is_raised() {
    let sandbox = Sandbox::new();
    let repo = sandbox.repo();
    write(&repo.join(".env"), "A=1");
    write(&repo.join(".worktreeinclude"), ".env\n");
    let mut env = env_of(&sandbox);
    env.insert("PATH".into(), failing_check_ignore_path(&sandbox, &env));

    let error = include::included(&repo, &env, &repo.join(".worktreeinclude")).unwrap_err();

    assert!(error.to_string().contains("boom"), "{error}");
}

fn failing_check_ignore_path(sandbox: &Sandbox, env: &Env) -> String {
    let which = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real_git_binary = String::from_utf8(which.stdout).unwrap().trim().to_string();
    let bin: PathBuf = sandbox.root.join("fake-bin");
    fs::create_dir_all(&bin).unwrap();
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = check-ignore ]; then echo boom >&2; exit 128; fi\nexec {real_git_binary} \"$@\"\n"
    );
    let fake = bin.join("git");
    fs::write(&fake, script).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    format!(
        "{}:{}",
        bin.display(),
        env.get("PATH").cloned().unwrap_or_default()
    )
}

#[test]
fn copy_skips_identical_and_respects_overwrite() {
    let sandbox = Sandbox::new();
    let source = sandbox.root.join("from");
    let target = sandbox.root.join("to");
    write(&source.join("a.log"), "1");

    assert!(fsops::copy("a.log", &source, &target, true).unwrap());
    assert!(!fsops::copy("a.log", &source, &target, true).unwrap());
    write(&source.join("a.log"), "2");
    assert!(!fsops::copy("a.log", &source, &target, false).unwrap());
    assert!(fsops::copy("a.log", &source, &target, true).unwrap());
    assert_eq!(fs::read_to_string(target.join("a.log")).unwrap(), "2");
}

#[test]
fn copy_of_directory_copies_tree() {
    let sandbox = Sandbox::new();
    let source = sandbox.root.join("from");
    let target = sandbox.root.join("to");
    write(&source.join("local/sub/b.xml"), "b");
    write(&source.join("local/a.xml"), "a");

    assert!(fsops::copy("local", &source, &target, false).unwrap());
    assert_eq!(
        fs::read_to_string(target.join("local/sub/b.xml")).unwrap(),
        "b"
    );
    assert!(!fsops::copy("local", &source, &target, false).unwrap());
}

#[test]
fn copy_of_missing_path_reports_no_change() {
    let sandbox = Sandbox::new();
    let source = sandbox.root.join("from");
    let target = sandbox.root.join("to");

    assert!(!fsops::copy("nothing", &source, &target, true).unwrap());
}

#[test]
fn copy_keeps_mode_and_mtime() {
    let sandbox = Sandbox::new();
    let source = sandbox.root.join("from");
    let target = sandbox.root.join("to");
    write(&source.join("run.sh"), "#!/bin/sh\n");
    fs::set_permissions(source.join("run.sh"), fs::Permissions::from_mode(0o750)).unwrap();
    let past = SystemTime::now() - Duration::from_secs(86_400);
    fs::File::options()
        .write(true)
        .open(source.join("run.sh"))
        .unwrap()
        .set_modified(past)
        .unwrap();

    fsops::copy("run.sh", &source, &target, true).unwrap();

    let copied = fs::metadata(target.join("run.sh")).unwrap();
    assert_eq!(copied.permissions().mode() & 0o777, 0o750);
    assert_eq!(copied.modified().unwrap(), past);
}

#[test]
fn copy_tree_does_not_follow_self_referencing_link() {
    let sandbox = Sandbox::new();
    let source = sandbox.root.join("from");
    let target = sandbox.root.join("to");
    write(&source.join("local/a.xml"), "a");
    std::os::unix::fs::symlink(".", source.join("local/link")).unwrap();

    assert!(fsops::copy("local", &source, &target, true).unwrap());

    assert!(target.join("local/a.xml").is_file());
    assert!(!target.join("local/link").exists());
}

#[test]
fn copy_tree_does_not_copy_linked_foreign_directory() {
    let sandbox = Sandbox::new();
    let source = sandbox.root.join("from");
    let target = sandbox.root.join("to");
    write(&source.join("local/a.xml"), "a");
    write(&sandbox.root.join("foreign/secret.txt"), "s");
    std::os::unix::fs::symlink(sandbox.root.join("foreign"), source.join("local/out")).unwrap();

    fsops::copy("local", &source, &target, true).unwrap();

    assert!(target.join("local/a.xml").is_file());
    assert!(!target.join("local/out").exists());
}

#[test]
fn copy_tree_still_copies_symlinked_file_content() {
    let sandbox = Sandbox::new();
    let source = sandbox.root.join("from");
    let target = sandbox.root.join("to");
    write(&source.join("local/real.xml"), "r");
    std::os::unix::fs::symlink("real.xml", source.join("local/alias.xml")).unwrap();

    fsops::copy("local", &source, &target, true).unwrap();

    assert_eq!(
        fs::read_to_string(target.join("local/alias.xml")).unwrap(),
        "r"
    );
}
