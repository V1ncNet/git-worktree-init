use std::fmt;

#[derive(Debug)]
pub enum Error {
    Usage(String),
    Git {
        args: Vec<String>,
        code: Option<i32>,
        stderr: String,
    },
    Plugin(String),
    Teardown(String),
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Usage(message)
            | Error::Plugin(message)
            | Error::Teardown(message)
            | Error::Other(message) => f.write_str(message),
            Error::Git { args, code, stderr } => {
                let stderr = stderr.trim();
                if !stderr.is_empty() {
                    return f.write_str(stderr);
                }
                let code = code.map_or_else(|| "signal".to_string(), |code| code.to_string());
                write!(f, "git {} failed ({code})", args.join(" "))
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Error::Other(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_error_prints_trimmed_stderr() {
        let error = Error::Git {
            args: vec!["status".into()],
            code: Some(128),
            stderr: " fatal: nope\n".into(),
        };
        assert_eq!(error.to_string(), "fatal: nope");
    }

    #[test]
    fn git_error_without_stderr_names_args_and_code() {
        let error = Error::Git {
            args: vec!["worktree".into(), "add".into()],
            code: Some(1),
            stderr: "\n".into(),
        };
        assert_eq!(error.to_string(), "git worktree add failed (1)");
    }
}
