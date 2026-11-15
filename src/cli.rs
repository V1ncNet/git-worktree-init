use std::io::Write;

pub const USAGE_ERROR: i32 = 129;

pub const USAGE: &str = "usage: git wt add [<options>] -- <git-worktree-add-args>...
   or: git wt refresh [<options>] [<worktree>]
   or: git wt remove [-f | --force] <worktree>
   or: git wt prune
   or: git wt harness init --remote <url>
   or: git wt harness sync
   or: git wt harness merge <branch>
   or: git wt --version

";

const HELP_FLAGS: [&str; 2] = ["-h", "--help"];
const KNOWN_SUBCOMMANDS: [&str; 7] = [
    "hook",
    "add",
    "refresh",
    "remove",
    "prune",
    "harness",
    "harness-hook",
];

pub fn main(argv: Vec<String>) -> i32 {
    let Some(command) = argv.first().map(String::as_str) else {
        return usage_error("need a subcommand", USAGE);
    };
    if HELP_FLAGS.contains(&command) {
        print!("{USAGE}");
        return USAGE_ERROR;
    }
    if command == "--version" {
        println!("git-wt version {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if KNOWN_SUBCOMMANDS.contains(&command) {
        return usage_error("not implemented yet", USAGE);
    }
    usage_error(&format!("unknown subcommand: `{command}'"), USAGE)
}

pub fn usage_error(message: &str, usage: &str) -> i32 {
    let _ = write!(std::io::stderr(), "error: {message}\n{usage}");
    USAGE_ERROR
}
