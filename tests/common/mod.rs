#![allow(dead_code)]

use std::ffi::OsStr;
use std::fs;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const LEAKING: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_CONFIG_PARAMETERS",
    "GIT_PREFIX",
    "COMPOSE_PROJECT_NAME",
    "CI",
];

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct Sandbox {
    pub root: PathBuf,
}

impl Sandbox {
    pub fn new() -> Sandbox {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("git-wt-{}-{unique}", std::process::id()));
        fs::create_dir_all(root.join("home")).unwrap();
        Sandbox {
            root: root.canonicalize().unwrap(),
        }
    }

    pub fn repo(&self) -> PathBuf {
        let repo = self.root.join("proj");
        fs::create_dir(&repo).unwrap();
        self.git_ok(&repo, &["init", "-q", "-b", "main"]);
        fs::write(
            repo.join(".gitignore"),
            ".env\nlocal/\n.worktreeinclude\n*.log\n",
        )
        .unwrap();
        self.git_ok(&repo, &["add", ".gitignore"]);
        self.git_ok(&repo, &["commit", "-q", "-m", "Init"]);
        repo
    }

    pub fn git(&self, cwd: &Path, args: &[&str]) -> Output {
        self.run("git", cwd, args)
    }

    pub fn wt(&self, cwd: &Path, args: &[&str]) -> Output {
        self.run(wt_bin(), cwd, args)
    }

    pub fn env(&self) -> Vec<(String, String)> {
        let home = self.root.join("home");
        let pair = |key: &str, value: &str| (key.to_string(), value.to_string());
        vec![
            pair("HOME", &home.to_string_lossy()),
            pair(
                "GIT_CONFIG_GLOBAL",
                &home.join(".gitconfig").to_string_lossy(),
            ),
            pair("GIT_CONFIG_NOSYSTEM", "1"),
            pair("GIT_TERMINAL_PROMPT", "0"),
            pair("GIT_AUTHOR_NAME", "Test"),
            pair("GIT_COMMITTER_NAME", "Test"),
            pair("GIT_AUTHOR_EMAIL", "test@example.invalid"),
            pair("GIT_COMMITTER_EMAIL", "test@example.invalid"),
        ]
    }

    fn git_ok(&self, cwd: &Path, args: &[&str]) {
        let output = self.git(cwd, args);
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub fn command(&self, program: impl AsRef<OsStr>, cwd: &Path) -> Command {
        let mut command = Command::new(program);
        command.current_dir(cwd);
        for (key, _) in std::env::vars_os() {
            let key = key.to_string_lossy();
            if key.starts_with("WT_") || LEAKING.contains(&key.as_ref()) {
                command.env_remove(key.as_ref());
            }
        }
        command.envs(self.env());
        without_terminal(&mut command);
        command
    }

    pub fn wt_command(&self, cwd: &Path) -> Command {
        self.command(env!("CARGO_BIN_EXE_git-wt"), cwd)
    }

    fn run(&self, program: &str, cwd: &Path, args: &[&str]) -> Output {
        self.command(program, cwd).args(args).output().unwrap()
    }
}

pub fn without_terminal(command: &mut Command) {
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

pub fn wt_bin() -> &'static str {
    env!("CARGO_BIN_EXE_git-wt")
}

pub fn register_hook(sandbox: &Sandbox, main: &Path) {
    register_hook_with(sandbox, main, wt_bin());
}

pub fn register_hook_with(sandbox: &Sandbox, main: &Path, binary: &str) {
    let command = format!(r#"f="{binary}"; [ -x "$f" ] || exit 0; exec "$f" hook"#);
    let output = sandbox.git(main, &["config", "hook.worktree-init.command", &command]);
    assert!(output.status.success());
    let event = [
        "config",
        "--add",
        "hook.worktree-init.event",
        "post-checkout",
    ];
    assert!(sandbox.git(main, &event).status.success());
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
