use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Write, pipe};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use serde_json::{Value, json};

use crate::config::{self, Entries};
use crate::error::{Error, Result};
use crate::gitenv::{self, Env};
use crate::options::{Choices, Kind, OptionSpec, Values};
use crate::worktrees;

const BOOT: &str = r#"import sys
if sys.path and sys.path[0] == "":
    del sys.path[0]
if sys.version_info < (3, 12):
    sys.stderr.write(f"worktree-init: needs python >= 3.12, got {sys.version.split()[0]} at {sys.executable}; set git config worktree-init.python\n")
    sys.exit(1)
import json, types
sources = json.loads(sys.argv[1])
package = types.ModuleType("git_worktree_init")
package.__path__ = []
sys.modules["git_worktree_init"] = package
for name in ("gitenv", "options", "context", "bridge"):
    module = types.ModuleType(f"git_worktree_init.{name}")
    module.__file__ = f"<git-wt>/git_worktree_init/{name}.py"
    sys.modules[module.__name__] = module
    setattr(package, name, module)
    exec(compile(sources[name], module.__file__, "exec"), module.__dict__)
sys.exit(sys.modules["git_worktree_init.bridge"].main())
"#;

const REQUEST_FD: RawFd = 3;
const REPLY_FD: RawFd = 4;
const FIRST_FREE_FD: RawFd = 10;

pub struct Ctx {
    pub main: PathBuf,
    pub target: PathBuf,
    pub common_dir: PathBuf,
    pub mode: String,
    pub env: Env,
    pub worktrees: Vec<PathBuf>,
    pub source: Option<PathBuf>,
    pub slot: Option<u32>,
    pub options: Values,
}

impl Ctx {
    fn to_json(&self) -> Value {
        json!({
            "main": path_text(&self.main),
            "target": path_text(&self.target),
            "common_dir": path_text(&self.common_dir),
            "mode": self.mode,
            "env": self.env,
            "worktrees": self.worktrees.iter().map(|path| path_text(path)).collect::<Vec<_>>(),
            "source": self.source.as_deref().map(path_text),
            "slot": self.slot,
            "options": self.options,
        })
    }
}

pub struct Plugin {
    pub path: PathBuf,
    pub options: Vec<OptionSpec>,
    pub sanitize_env: Vec<String>,
    pub has_next_steps: bool,
    pub has_teardown: bool,
    python: String,
    child: Child,
    requests: Option<PipeWriter>,
    replies: BufReader<PipeReader>,
}

enum Exchange {
    Reply(Value),
    Gone,
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn locate(main: &Path, entries: &Entries) -> Option<PathBuf> {
    let raw = config::last(entries, "plugin").filter(|raw| !raw.is_empty())?;
    let path = expand_home(raw);
    if path.is_absolute() {
        return Some(path);
    }
    Some(worktrees::resolve(&main.join(path)))
}

pub fn expand_home(raw: &str) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match (raw, raw.strip_prefix("~/"), home) {
        ("~", _, Some(home)) => home,
        (_, Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(raw),
    }
}

pub fn python(env: &Env, cwd: &Path) -> String {
    non_empty(env.get("WT_PYTHON").cloned())
        .or_else(|| configured_python(env, cwd))
        .unwrap_or_else(|| {
            option_env!("WT_DEFAULT_PYTHON")
                .unwrap_or("python3")
                .to_string()
        })
}

fn configured_python(env: &Env, cwd: &Path) -> Option<String> {
    let args = ["config", "--get", "--type=path", "worktree-init.python"];
    let (code, stdout, _) = gitenv::git_status(cwd, &args, env).ok()?;
    non_empty(Some(stdout.trim_end_matches('\n').to_string())).filter(|_| code == 0)
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
}

fn embedded_sources() -> String {
    json!({
        "options": include_str!("../bridge/git_worktree_init/options.py"),
        "context": include_str!("../bridge/git_worktree_init/context.py"),
        "gitenv": include_str!("../bridge/git_worktree_init/gitenv.py"),
        "bridge": include_str!("../bridge/git_worktree_init/bridge.py"),
    })
    .to_string()
}

fn did_not_start(python: &str) -> Error {
    Error::Plugin(format!(
        "plugin bridge did not start with {python}; set git config worktree-init.python"
    ))
}

fn spawn_bridge(python: &str, env: &Env) -> Result<(Child, PipeWriter, PipeReader)> {
    let (request_reader, requests) = pipe()?;
    let (replies, reply_writer) = pipe()?;
    let (request_fd, reply_fd) = (request_reader.as_raw_fd(), reply_writer.as_raw_fd());
    let mut command = Command::new(python);
    command
        .args(["-c", BOOT])
        .arg(embedded_sources())
        .env_clear()
        .envs(env);
    unsafe {
        command.pre_exec(move || install_bridge_fds(request_fd, reply_fd));
    }
    let child = command.spawn().map_err(|_| did_not_start(python))?;
    Ok((child, requests, replies))
}

fn install_bridge_fds(request_fd: RawFd, reply_fd: RawFd) -> std::io::Result<()> {
    let request_copy = move_above_target_range(request_fd)?;
    let reply_copy = move_above_target_range(reply_fd)?;
    duplicate_onto(request_copy, REQUEST_FD)?;
    duplicate_onto(reply_copy, REPLY_FD)
}

fn move_above_target_range(fd: RawFd) -> std::io::Result<RawFd> {
    let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, FIRST_FREE_FD) };
    if copy < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(copy)
}

fn duplicate_onto(from: RawFd, to: RawFd) -> std::io::Result<()> {
    if unsafe { libc::dup2(from, to) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn parse_option(raw: &Value) -> Result<OptionSpec> {
    let text = |key: &str| raw[key].as_str().unwrap_or_default().to_string();
    let kind = Kind::parse(&text("kind"))
        .ok_or_else(|| Error::Plugin(format!("unknown option kind {}", text("kind"))))?;
    Ok(OptionSpec {
        name: text("name"),
        kind,
        default: raw["default"].clone(),
        help: text("help"),
        choices: parse_choices(raw),
        ask: raw["ask"].as_bool().unwrap_or(true),
    })
}

fn parse_choices(raw: &Value) -> Choices {
    if raw["dynamic"].as_bool().unwrap_or(false) {
        return Choices::Dynamic;
    }
    match raw["choices"].as_array() {
        Some(choices) => Choices::Fixed(strings(choices)),
        None => Choices::None,
    }
}

fn strings(values: &[Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| value.as_str().map(String::from))
        .collect()
}

impl Plugin {
    pub fn start(path: &Path, python: &str, env: &Env) -> Result<Plugin> {
        if !path.is_file() {
            return Err(Error::Plugin(format!(
                "plugin not found: {}",
                path.display()
            )));
        }
        let (child, requests, replies) = spawn_bridge(python, env)?;
        let mut plugin = Plugin {
            path: path.to_path_buf(),
            options: Vec::new(),
            sanitize_env: Vec::new(),
            has_next_steps: false,
            has_teardown: false,
            python: python.to_string(),
            child,
            requests: Some(requests),
            replies: BufReader::new(replies),
        };
        plugin.load()?;
        Ok(plugin)
    }

    fn load(&mut self) -> Result<()> {
        let request = json!({"op": "load", "path": path_text(&self.path)});
        let reply = match self.exchange(&request) {
            Exchange::Reply(reply) => reply,
            Exchange::Gone => return Err(did_not_start(&self.python)),
        };
        let loaded = unwrap_reply(reply)?;
        self.options = loaded["options"]
            .as_array()
            .map(|options| options.iter().map(parse_option).collect())
            .transpose()?
            .unwrap_or_default();
        self.sanitize_env = strings(loaded["sanitize_env"].as_array().map_or(&[], Vec::as_slice));
        self.has_next_steps = loaded["next_steps"].as_bool().unwrap_or(false);
        self.has_teardown = loaded["teardown"].as_bool().unwrap_or(false);
        Ok(())
    }

    pub fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    pub fn request_fd(&self) -> RawFd {
        self.requests.as_ref().map_or(-1, AsRawFd::as_raw_fd)
    }

    pub fn slot_of(&mut self, worktree: &Path) -> Result<Option<u32>> {
        let reply = self.call("slot_of", json!({"worktree": path_text(worktree)}))?;
        Ok(reply.as_u64().and_then(|slot| u32::try_from(slot).ok()))
    }

    pub fn choices(&mut self, option: &str, ctx: &Ctx) -> Result<Vec<String>> {
        let reply = self.call("choices", json!({"option": option, "ctx": ctx.to_json()}))?;
        Ok(strings(reply.as_array().map_or(&[], Vec::as_slice)))
    }

    pub fn apply(&mut self, ctx: &Ctx) -> Result<()> {
        self.call("apply", json!({"ctx": ctx.to_json()})).map(drop)
    }

    pub fn next_steps(&mut self) -> Result<Vec<String>> {
        let reply = self.call("next_steps", json!({}))?;
        Ok(strings(reply.as_array().map_or(&[], Vec::as_slice)))
    }

    pub fn teardown(&mut self, ctx: &Ctx) -> Result<()> {
        self.call("teardown", json!({"ctx": ctx.to_json()}))
            .map(drop)
    }

    fn call(&mut self, op: &str, mut fields: Value) -> Result<Value> {
        fields["op"] = json!(op);
        match self.exchange(&fields) {
            Exchange::Reply(reply) => unwrap_reply(reply),
            Exchange::Gone => Err(Error::Plugin(format!("plugin exited during {op}"))),
        }
    }

    fn exchange(&mut self, request: &Value) -> Exchange {
        match self.send(request).and_then(|()| self.receive()) {
            Some(reply) => Exchange::Reply(reply),
            None => Exchange::Gone,
        }
    }

    fn send(&mut self, request: &Value) -> Option<()> {
        let requests = self.requests.as_mut()?;
        writeln!(requests, "{request}").ok()?;
        requests.flush().ok()
    }

    fn receive(&mut self) -> Option<Value> {
        let mut line = String::new();
        match self.replies.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => serde_json::from_str(&line).ok(),
        }
    }
}

fn unwrap_reply(reply: Value) -> Result<Value> {
    if let Some(message) = reply.get("error") {
        return Err(Error::Plugin(
            message
                .as_str()
                .map_or_else(|| message.to_string(), String::from),
        ));
    }
    Ok(reply.get("ok").cloned().unwrap_or(Value::Null))
}

impl Drop for Plugin {
    fn drop(&mut self) {
        let _ = self.send(&json!({"op": "exit"}));
        self.requests = None;
        let _ = self.child.wait();
    }
}
