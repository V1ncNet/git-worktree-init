use serde_json::{Map, Value};

use crate::config::{self, Entries};
use crate::error::{Error, Result};
use crate::gitenv::Env;

pub type Values = Map<String, Value>;

const TRUE: [&str; 5] = ["1", "true", "yes", "y", "on"];
const FALSE: [&str; 5] = ["0", "false", "no", "n", "off"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Confirm,
    Text,
    Choice,
    Slot,
}

impl Kind {
    pub fn parse(name: &str) -> Option<Kind> {
        match name {
            "confirm" => Some(Kind::Confirm),
            "text" => Some(Kind::Text),
            "choice" => Some(Kind::Choice),
            "slot" => Some(Kind::Slot),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choices {
    None,
    Fixed(Vec<String>),
    Dynamic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionSpec {
    pub name: String,
    pub kind: Kind,
    pub default: Value,
    pub help: String,
    pub choices: Choices,
    pub ask: bool,
}

impl OptionSpec {
    pub fn env_key(&self) -> String {
        format!("WT_{}", self.name.to_uppercase())
    }

    pub fn config_key(&self) -> String {
        self.name.replace('_', "-").to_lowercase()
    }

    pub fn flag(&self) -> String {
        format!("--{}", self.name.replace('_', "-"))
    }
}

pub fn builtin() -> Vec<OptionSpec> {
    vec![
        OptionSpec {
            name: "slot".into(),
            kind: Kind::Slot,
            default: Value::Null,
            help: "slot of the worktree".into(),
            choices: Choices::None,
            ask: true,
        },
        OptionSpec {
            name: "source".into(),
            kind: Kind::Text,
            default: Value::Null,
            help: "worktree to copy files from".into(),
            choices: Choices::None,
            ask: false,
        },
    ]
}

pub fn python_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut repr = String::from(quote);
    for character in text.chars() {
        match character {
            '\\' => repr.push_str("\\\\"),
            '\n' => repr.push_str("\\n"),
            '\r' => repr.push_str("\\r"),
            '\t' => repr.push_str("\\t"),
            other if other == quote => {
                repr.push('\\');
                repr.push(other);
            }
            other => repr.push(other),
        }
    }
    repr.push(quote);
    repr
}

pub fn parse(option: &OptionSpec, raw: &str) -> Result<Value> {
    match option.kind {
        Kind::Confirm => parse_confirm(option, raw),
        Kind::Slot => parse_slot(option, raw),
        Kind::Text | Kind::Choice => Ok(Value::String(raw.to_string())),
    }
}

fn parse_confirm(option: &OptionSpec, raw: &str) -> Result<Value> {
    let lowered = raw.trim().to_lowercase();
    if TRUE.contains(&lowered.as_str()) {
        return Ok(Value::Bool(true));
    }
    if FALSE.contains(&lowered.as_str()) {
        return Ok(Value::Bool(false));
    }
    Err(Error::Other(format!(
        "{}: expected yes or no, got {}",
        option.name,
        python_repr(raw)
    )))
}

fn parse_slot(option: &OptionSpec, raw: &str) -> Result<Value> {
    let digits = raw.trim();
    let number = digits
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| digits.parse::<u64>().ok())
        .flatten();
    number.map(Value::from).ok_or_else(|| {
        Error::Other(format!(
            "{}: expected a non-negative number, got {}",
            option.name,
            python_repr(raw)
        ))
    })
}

pub fn explicit(options: &[OptionSpec], env: &Env) -> Result<Values> {
    let mut values = Values::new();
    for option in options {
        if let Some(raw) = env.get(&option.env_key()) {
            values.insert(option.name.clone(), parse(option, raw)?);
        }
    }
    Ok(values)
}

pub fn from_config(options: &[OptionSpec], entries: &Entries) -> Result<Values> {
    let mut values = Values::new();
    for option in options {
        if let Some(raw) = config::last(entries, &option.config_key()) {
            values.insert(option.name.clone(), parse(option, raw)?);
        }
    }
    Ok(values)
}

pub fn interactive_wanted(options: &[OptionSpec], env: &Env, entries: &Entries) -> bool {
    let set = |key: &str| env.get(key).is_some_and(|value| !value.is_empty());
    if env
        .get("WT_YES")
        .is_some_and(|value| !value.is_empty() && value != "0")
    {
        return false;
    }
    if env
        .get("GIT_TERMINAL_PROMPT")
        .is_some_and(|value| value == "0")
        || set("CI")
    {
        return false;
    }
    let disabled = config::last(entries, "interactive")
        .is_some_and(|value| FALSE.contains(&value.trim().to_lowercase().as_str()));
    if disabled
        || options
            .iter()
            .any(|option| env.contains_key(&option.env_key()))
    {
        return false;
    }
    let command_line = config::command_line_keys(entries);
    !options
        .iter()
        .any(|option| command_line.contains(&option.config_key()))
}

pub fn resolve(
    options: &[OptionSpec],
    explicit: &Values,
    answers: &Values,
    config: &Values,
) -> Values {
    options
        .iter()
        .map(|option| {
            let value = [explicit, answers, config]
                .into_iter()
                .find_map(|layer| layer.get(&option.name))
                .unwrap_or(&option.default);
            (option.name.clone(), value.clone())
        })
        .collect()
}
