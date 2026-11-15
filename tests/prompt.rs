use std::io::Cursor;

use git_wt::error::Result;
use git_wt::options::{Choices, Kind, OptionSpec, Values};
use git_wt::prompt::{Prompter, ask};
use serde_json::{Value, json};

fn prompter(input: &str) -> Prompter<Cursor<Vec<u8>>, Vec<u8>> {
    Prompter::new(Cursor::new(input.as_bytes().to_vec()), Vec::new())
}

fn spec(name: &str, kind: Kind, help: &str, choices: Choices, ask: bool) -> OptionSpec {
    OptionSpec {
        name: name.into(),
        kind,
        default: Value::Null,
        help: help.into(),
        choices,
        ask,
    }
}

fn defaults(entries: Value) -> Values {
    entries.as_object().unwrap().clone()
}

fn dynamic_choices_for(base: &'static str) -> impl FnMut(&str) -> Result<Vec<String>> {
    move |_| Ok(vec!["none".to_string(), base.to_string()])
}

fn names(choices: &[&str]) -> Vec<String> {
    choices.iter().map(|name| name.to_string()).collect()
}

#[test]
fn enter_takes_defaults() {
    let mut p = prompter("\n\n\n\n");

    assert!(p.confirm("Graph", true).unwrap());
    assert_eq!(p.number("Slot", 9).unwrap(), 9);
    assert_eq!(
        p.choice("DB", &names(&["none", "a"]), "none").unwrap(),
        "none"
    );
    assert_eq!(p.text("Name", &json!("x")).unwrap(), "x");
}

#[test]
fn invalid_answers_ask_again() {
    let mut p = prompter("maybe\nn\nabc\n12\n7\n2\n");

    assert!(!p.confirm("Graph", true).unwrap());
    assert_eq!(p.number("Slot", 9).unwrap(), 12);
    assert_eq!(p.choice("DB", &names(&["none", "a"]), "none").unwrap(), "a");
}

#[test]
fn choice_accepts_name_and_rejects_out_of_range() {
    let mut p = prompter("3\na\n");

    assert_eq!(p.choice("DB", &names(&["none", "a"]), "none").unwrap(), "a");
}

#[test]
fn text_without_default_shows_nothing_and_returns_empty() {
    let mut p = prompter("\n");

    assert_eq!(p.text("Name", &Value::Null).unwrap(), "");
}

#[test]
fn eof_is_reported_as_error() {
    let mut p = prompter("");

    assert_eq!(p.confirm("Graph", true).unwrap_err().to_string(), "eof");
}

#[test]
fn ask_skips_hidden_and_stops_at_eof() {
    let declared = vec![
        spec("slot", Kind::Slot, "Slot", Choices::None, true),
        spec("source", Kind::Text, "", Choices::None, false),
        spec("graph", Kind::Confirm, "", Choices::None, true),
        spec("clone_db", Kind::Choice, "", Choices::Dynamic, true),
    ];
    let given = defaults(json!({"slot": 3, "source": null, "graph": true, "clone_db": "none"}));

    let answers = ask(
        &declared,
        &given,
        &mut prompter("5\n"),
        &mut dynamic_choices_for("b"),
    )
    .unwrap();

    assert_eq!(Value::Object(answers), json!({"slot": 5}));
}

#[test]
fn ask_uses_dynamic_choices() {
    let declared = vec![spec("clone_db", Kind::Choice, "", Choices::Dynamic, true)];
    let given = defaults(json!({"clone_db": "none"}));

    let answers = ask(
        &declared,
        &given,
        &mut prompter("2\n"),
        &mut dynamic_choices_for("b"),
    )
    .unwrap();

    assert_eq!(Value::Object(answers), json!({"clone_db": "b"}));
}

#[test]
fn ask_uses_fixed_choices_and_skips_empty_ones() {
    let declared = vec![
        spec("empty", Kind::Choice, "", Choices::Fixed(vec![]), true),
        spec(
            "db",
            Kind::Choice,
            "",
            Choices::Fixed(names(&["x", "y"])),
            true,
        ),
    ];
    let given = defaults(json!({"empty": "", "db": "y"}));

    let answers = ask(
        &declared,
        &given,
        &mut prompter("\n"),
        &mut dynamic_choices_for("b"),
    )
    .unwrap();

    assert_eq!(Value::Object(answers), json!({"db": "y"}));
}

#[test]
fn prompt_text_goes_to_output() {
    let mut p = prompter("\n");

    p.number("Slot", 9).unwrap();

    assert_eq!(String::from_utf8(p.into_writer()).unwrap(), "? Slot (9) › ");
}

#[test]
fn choice_listing_goes_to_output() {
    let mut p = prompter("\n");

    p.choice("DB", &names(&["none", "a"]), "a").unwrap();

    assert_eq!(
        String::from_utf8(p.into_writer()).unwrap(),
        "? DB (2)\n  1) none  2) a\n  › "
    );
}

#[test]
fn confirm_hint_follows_default() {
    let mut p = prompter("\n\n");

    p.confirm("A", true).unwrap();
    p.confirm("B", false).unwrap();

    assert_eq!(
        String::from_utf8(p.into_writer()).unwrap(),
        "? A (Y/n) › ? B (y/N) › "
    );
}

#[test]
fn ask_capitalizes_help_as_label() {
    let declared = vec![spec(
        "port_base",
        Kind::Text,
        "first port",
        Choices::None,
        true,
    )];
    let given = defaults(json!({"port_base": "8000"}));
    let mut p = prompter("\n");

    ask(&declared, &given, &mut p, &mut dynamic_choices_for("b")).unwrap();

    assert!(
        String::from_utf8(p.into_writer())
            .unwrap()
            .starts_with("? First port (8000)")
    );
}
