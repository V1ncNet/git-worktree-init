use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;

use crate::error::{Error, Result};

pub const GIT_VARIABLES: [&str; 4] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_PREFIX"];

pub type Env = BTreeMap<String, String>;

pub fn process_env() -> Env {
    std::env::vars_os()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect()
}

pub fn sanitized(env: &Env, extra: &[String]) -> Env {
    env.iter()
        .filter(|(key, _)| !GIT_VARIABLES.contains(&key.as_str()) && !extra.contains(key))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn run(cmd: &[&str], cwd: &Path, env: &Env, input: Option<&[u8]>) -> Result<Output> {
    let (program, args) = cmd
        .split_first()
        .ok_or_else(|| Error::Other("empty command".into()))?;
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match input {
        Some(bytes) => run_with_input(command, bytes),
        None => Ok(command.output()?),
    }
}

fn run_with_input(mut command: Command, input: &[u8]) -> Result<Output> {
    let mut child = command.stdin(Stdio::piped()).spawn()?;
    let mut stdin = child.stdin.take().expect("stdin was piped");
    thread::scope(|scope| {
        scope.spawn(move || {
            let _ = stdin.write_all(input);
        });
        Ok(child.wait_with_output()?)
    })
}

pub fn git_status(cwd: &Path, args: &[&str], env: &Env) -> Result<(i32, String, String)> {
    let cmd: Vec<&str> = std::iter::once("git").chain(args.iter().copied()).collect();
    let output = run(&cmd, cwd, env, None)?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

pub fn git(cwd: &Path, args: &[&str], env: &Env) -> Result<String> {
    let (code, stdout, stderr) = git_status(cwd, args, env)?;
    if code == 0 {
        return Ok(stdout);
    }
    Err(Error::Git {
        args: args.iter().map(|arg| arg.to_string()).collect(),
        code: Some(code).filter(|code| *code >= 0),
        stderr,
    })
}
