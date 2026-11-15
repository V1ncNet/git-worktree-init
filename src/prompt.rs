use std::fs::File;
use std::io::{BufRead, BufReader, Write};

use serde_json::Value;

use crate::error::{Error, Result};
use crate::options::{Choices, Kind, OptionSpec, Values};

const EOF_MESSAGE: &str = "eof";

pub struct Prompter<R: BufRead, W: Write> {
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> Prompter<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Prompter { reader, writer }
    }

    pub fn into_writer(self) -> W {
        self.writer
    }

    fn read(&mut self, text: &str) -> Result<String> {
        self.writer.write_all(text.as_bytes())?;
        self.writer.flush()?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(Error::Other(EOF_MESSAGE.into()));
        }
        Ok(line.trim().to_string())
    }

    pub fn confirm(&mut self, label: &str, default: bool) -> Result<bool> {
        let hint = if default { "Y/n" } else { "y/N" };
        loop {
            let answer = self.read(&format!("? {label} ({hint}) › "))?.to_lowercase();
            match answer.as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => {}
            }
        }
    }

    pub fn number(&mut self, label: &str, default: u32) -> Result<u32> {
        loop {
            let answer = self.read(&format!("? {label} ({default}) › "))?;
            if answer.is_empty() {
                return Ok(default);
            }
            if let Some(number) = parse_digits(&answer) {
                return Ok(number);
            }
        }
    }

    pub fn text(&mut self, label: &str, default: &Value) -> Result<String> {
        let shown = match default {
            Value::Null => String::new(),
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let answer = self.read(&format!("? {label} ({shown}) › "))?;
        Ok(if answer.is_empty() { shown } else { answer })
    }

    pub fn choice(&mut self, label: &str, choices: &[String], default: &str) -> Result<String> {
        let index = choices
            .iter()
            .position(|name| name == default)
            .map_or(1, |position| position + 1);
        let listing = numbered_listing(choices);
        loop {
            let answer = self.read(&format!("? {label} ({index})\n  {listing}\n  › "))?;
            if answer.is_empty() {
                return Ok(choices[index - 1].clone());
            }
            if let Some(number) = parse_digits(&answer)
                && (1..=choices.len()).contains(&(number as usize))
            {
                return Ok(choices[number as usize - 1].clone());
            }
            if choices.contains(&answer) {
                return Ok(answer);
            }
        }
    }
}

fn parse_digits(answer: &str) -> Option<u32> {
    answer
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| answer.parse().ok())
        .flatten()
}

fn numbered_listing(choices: &[String]) -> String {
    choices
        .iter()
        .enumerate()
        .map(|(position, name)| format!("{}) {name}", position + 1))
        .collect::<Vec<_>>()
        .join("  ")
}

fn capitalized(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

fn label_of(option: &OptionSpec) -> String {
    let source = if option.help.is_empty() {
        &option.name
    } else {
        &option.help
    };
    capitalized(source)
}

fn is_eof(error: &Error) -> bool {
    matches!(error, Error::Other(message) if message == EOF_MESSAGE)
}

fn answer_option<R: BufRead, W: Write>(
    option: &OptionSpec,
    default: &Value,
    prompter: &mut Prompter<R, W>,
    choices: &mut dyn FnMut(&str) -> Result<Vec<String>>,
) -> Result<Option<Value>> {
    let label = label_of(option);
    let answer = match option.kind {
        Kind::Confirm => Value::Bool(prompter.confirm(&label, default.as_bool().unwrap_or(false))?),
        Kind::Slot => {
            let default = default.as_u64().unwrap_or(0) as u32;
            Value::from(prompter.number(&label, default)?)
        }
        Kind::Choice => {
            let available = available_choices(option, choices)?;
            if available.is_empty() {
                return Ok(None);
            }
            let default = default.as_str().unwrap_or_default();
            Value::String(prompter.choice(&label, &available, default)?)
        }
        Kind::Text => Value::String(prompter.text(&label, default)?),
    };
    Ok(Some(answer))
}

fn available_choices(
    option: &OptionSpec,
    choices: &mut dyn FnMut(&str) -> Result<Vec<String>>,
) -> Result<Vec<String>> {
    match &option.choices {
        Choices::None => Ok(Vec::new()),
        Choices::Fixed(names) => Ok(names.clone()),
        Choices::Dynamic => choices(&option.name),
    }
}

pub fn ask<R: BufRead, W: Write>(
    options: &[OptionSpec],
    defaults: &Values,
    prompter: &mut Prompter<R, W>,
    choices: &mut dyn FnMut(&str) -> Result<Vec<String>>,
) -> Result<Values> {
    let mut answers = Values::new();
    for option in options.iter().filter(|option| option.ask) {
        let default = defaults.get(&option.name).unwrap_or(&Value::Null);
        match answer_option(option, default, prompter, choices) {
            Ok(Some(answer)) => {
                answers.insert(option.name.clone(), answer);
            }
            Ok(None) => {}
            Err(error) if is_eof(&error) => break,
            Err(error) => return Err(error),
        }
    }
    Ok(answers)
}

pub fn open_tty() -> Option<(BufReader<File>, File)> {
    let reader = File::open("/dev/tty").ok()?;
    let writer = File::options().write(true).open("/dev/tty").ok()?;
    Some((BufReader::new(reader), writer))
}
