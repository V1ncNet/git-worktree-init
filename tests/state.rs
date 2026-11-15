mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use common::Sandbox;
use git_wt::config::{Entries, Entry};
use git_wt::gitenv::Env;
use git_wt::options::{self, Choices, Kind, OptionSpec, Values};
use git_wt::records::{self, Record};
use git_wt::slots;
use serde_json::{Value, json};

fn spec(name: &str, kind: Kind, default: Value, choices: Choices) -> OptionSpec {
    OptionSpec {
        name: name.into(),
        kind,
        default,
        help: String::new(),
        choices,
        ask: true,
    }
}

fn graph() -> OptionSpec {
    spec("graph", Kind::Confirm, json!(true), Choices::None)
}

fn clone_db() -> OptionSpec {
    let choices = Choices::Fixed(vec!["none".into(), "a".into()]);
    spec("clone_db", Kind::Choice, json!("none"), choices)
}

fn declared() -> Vec<OptionSpec> {
    let mut specs = options::builtin();
    specs.push(graph());
    specs.push(clone_db());
    specs
}

fn env_map(pairs: &[(&str, &str)]) -> Env {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn entries(pairs: &[(&str, &str, &str)]) -> Entries {
    let mut entries = Entries::new();
    for (key, scope, value) in pairs {
        entries.entry(key.to_string()).or_default().push(Entry {
            scope: scope.to_string(),
            value: value.to_string(),
        });
    }
    entries
}

fn values(pairs: &[(&str, Value)]) -> Values {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.clone()))
        .collect()
}

#[test]
fn keys_are_derived_from_name() {
    let option = clone_db();
    assert_eq!(option.env_key(), "WT_CLONE_DB");
    assert_eq!(option.config_key(), "clone-db");
    assert_eq!(option.flag(), "--clone-db");
}

#[test]
fn kind_parses_known_names_only() {
    assert_eq!(Kind::parse("confirm"), Some(Kind::Confirm));
    assert_eq!(Kind::parse("slot"), Some(Kind::Slot));
    assert_eq!(Kind::parse("number"), None);
}

#[test]
fn parse_confirm() {
    for (raw, expected) in [("yes", true), ("1", true), ("off", false), ("N", false)] {
        assert_eq!(options::parse(&graph(), raw).unwrap(), json!(expected));
    }
}

#[test]
fn parse_rejects_bad_values() {
    let error = options::parse(&graph(), "maybe").unwrap_err();
    assert_eq!(error.to_string(), "graph: expected yes or no, got 'maybe'");
    let slot = &options::builtin()[0];
    let error = options::parse(slot, "-1").unwrap_err();
    assert_eq!(
        error.to_string(),
        "slot: expected a non-negative number, got '-1'"
    );
}

#[test]
fn parse_quotes_like_python_repr() {
    let error = options::parse(&graph(), "it's").unwrap_err();
    assert_eq!(error.to_string(), "graph: expected yes or no, got \"it's\"");
}

#[test]
fn parse_slot_and_text() {
    let slot = &options::builtin()[0];
    assert_eq!(options::parse(slot, "07").unwrap(), json!(7));
    assert_eq!(options::parse(&clone_db(), "a b").unwrap(), json!("a b"));
}

#[test]
fn builtin_declares_slot_and_source() {
    let specs = options::builtin();
    assert_eq!(specs[0].name, "slot");
    assert_eq!(specs[0].help, "slot of the worktree");
    assert_eq!(specs[1].name, "source");
    assert_eq!(specs[1].help, "worktree to copy files from");
    assert!(!specs[1].ask);
}

#[test]
fn resolve_order_is_explicit_answer_config_default() {
    let specs = declared();
    let explicit = options::explicit(&specs, &env_map(&[("WT_GRAPH", "0")])).unwrap();
    let config = options::from_config(
        &specs,
        &entries(&[("clone-db", "local", "a"), ("graph", "local", "true")]),
    )
    .unwrap();

    let resolved = options::resolve(
        &specs,
        &explicit,
        &values(&[("clone_db", json!("none"))]),
        &config,
    );

    assert_eq!(
        resolved,
        values(&[
            ("slot", Value::Null),
            ("source", Value::Null),
            ("graph", json!(false)),
            ("clone_db", json!("none")),
        ])
    );
}

#[test]
fn interactive_wanted_without_any_setting() {
    assert!(options::interactive_wanted(
        &declared(),
        &Env::new(),
        &Entries::new()
    ));
}

#[test]
fn interactive_not_wanted() {
    let cases = [
        (env_map(&[("WT_YES", "1")]), Entries::new()),
        (env_map(&[("WT_GRAPH", "1")]), Entries::new()),
        (Env::new(), entries(&[("clone-db", "command", "a")])),
        (Env::new(), entries(&[("interactive", "global", "false")])),
        (env_map(&[("GIT_TERMINAL_PROMPT", "0")]), Entries::new()),
        (env_map(&[("CI", "true")]), Entries::new()),
    ];
    for (env, entries) in cases {
        assert!(!options::interactive_wanted(&declared(), &env, &entries));
    }
}

#[test]
fn local_config_value_keeps_interactive() {
    let entries = entries(&[("clone-db", "local", "a")]);
    assert!(options::interactive_wanted(
        &declared(),
        &Env::new(),
        &entries
    ));
}

fn used_slots(pairs: &[(u32, &str)]) -> BTreeMap<u32, PathBuf> {
    pairs
        .iter()
        .map(|(slot, path)| (*slot, PathBuf::from(path)))
        .collect()
}

fn reserved(raw: &str) -> BTreeSet<u32> {
    slots::parse_reserved(Some(raw)).unwrap()
}

#[test]
fn allocate_takes_smallest_free_outside_reserved() {
    let used = used_slots(&[(1, "/a"), (3, "/b")]);
    assert_eq!(slots::allocate(&used, &reserved("0 2")), 4);
}

#[test]
fn reserved_defaults_to_zero_and_can_be_emptied() {
    assert_eq!(slots::parse_reserved(None).unwrap(), BTreeSet::from([0]));
    assert!(slots::parse_reserved(Some("")).unwrap().is_empty());
}

#[test]
fn used_skips_target_and_unknown() {
    let table: BTreeMap<PathBuf, Option<u32>> = BTreeMap::from([
        (PathBuf::from("/a"), Some(1)),
        (PathBuf::from("/b"), None),
        (PathBuf::from("/t"), Some(5)),
    ]);
    let worktrees: Vec<PathBuf> = table.keys().cloned().collect();
    let mut slot_of = |path: &Path| Ok(table[path]);

    let result = slots::used(&mut slot_of, &worktrees, Path::new("/t")).unwrap();

    assert_eq!(result, used_slots(&[(1, "/a")]));
}

#[test]
fn validate_rejects_taken_and_reserved_slots() {
    let taken = slots::validate(1, &used_slots(&[(1, "/a")]), &BTreeSet::new()).unwrap_err();
    assert_eq!(taken.to_string(), "slot 1 is taken by /a");
    let held = slots::validate(2, &BTreeMap::new(), &reserved("0 2")).unwrap_err();
    assert_eq!(
        held.to_string(),
        "slot 2 is reserved (worktree-init.reservedSlots)"
    );
    assert!(slots::validate(3, &BTreeMap::new(), &reserved("0 2")).is_ok());
}

#[test]
fn parse_reserved_rejects_garbage() {
    let error = slots::parse_reserved(Some("0 x")).unwrap_err();
    assert_eq!(
        error.to_string(),
        "reservedSlots must list numbers, got '0 x'"
    );
}

fn common_dir(sandbox: &Sandbox) -> PathBuf {
    let common = sandbox.root.join("common");
    fs::create_dir_all(common.join("worktrees")).unwrap();
    common
}

fn admin_dir(common: &Path, name: &str) -> PathBuf {
    let admin = common.join("worktrees").join(name);
    fs::create_dir_all(&admin).unwrap();
    admin
}

#[test]
fn reads_record_written_by_python() {
    let sandbox = Sandbox::new();
    let common = common_dir(&sandbox);
    let directory = common.join("worktree-init").join("records");
    fs::create_dir_all(&directory).unwrap();
    let python_output = "{\n  \"admin\": \"wt1\",\n  \"slot\": 3,\n  \"target\": \"/x/wt1\",\n  \"options\": {\n    \"slot\": 3,\n    \"source\": null,\n    \"graph\": true,\n    \"port_base\": \"8000\"\n  }\n}\n";
    fs::write(directory.join("abc.json"), python_output).unwrap();

    let found = records::all_records(&common).unwrap();

    let expected = Record {
        key: "abc".into(),
        admin: "wt1".into(),
        slot: Some(3),
        target: PathBuf::from("/x/wt1"),
        options: values(&[
            ("slot", json!(3)),
            ("source", Value::Null),
            ("graph", json!(true)),
            ("port_base", json!("8000")),
        ]),
    };
    assert_eq!(found, vec![expected]);
}

#[test]
fn written_record_round_trips_and_sets_token() {
    let sandbox = Sandbox::new();
    let common = common_dir(&sandbox);
    let admin = admin_dir(&common, "wt2");
    let options = values(&[("slot", json!(null)), ("graph", json!(false))]);

    records::write(&common, &admin, None, Path::new("/x/wt2"), &options).unwrap();
    let found = records::find(&common, &admin).unwrap().unwrap();

    assert_eq!(found.admin, "wt2");
    assert_eq!(found.slot, None);
    assert_eq!(found.target, PathBuf::from("/x/wt2"));
    assert_eq!(found.options, options);
    assert_eq!(found.key.len(), 16);
    let token = fs::read_to_string(admin.join(records::TOKEN)).unwrap();
    assert_eq!(token, format!("{}\n", found.key));
    assert_eq!(records::all_records(&common).unwrap(), vec![found.clone()]);

    records::write(&common, &admin, Some(2), Path::new("/x/wt2"), &options).unwrap();
    let rewritten = records::find(&common, &admin).unwrap().unwrap();
    assert_eq!(
        (rewritten.key.as_str(), rewritten.slot),
        (found.key.as_str(), Some(2))
    );

    records::discard(&common, &rewritten).unwrap();
    assert!(records::find(&common, &admin).unwrap().is_none());
    records::discard(&common, &rewritten).unwrap();
}

#[test]
fn alive_follows_token_lock_and_gitdir() {
    let sandbox = Sandbox::new();
    let common = common_dir(&sandbox);
    let admin = admin_dir(&common, "wt3");
    records::write(&common, &admin, None, Path::new("/x/wt3"), &Values::new()).unwrap();
    let record = records::find(&common, &admin).unwrap().unwrap();

    assert!(!records::alive(&common, &record));

    fs::write(
        admin.join("gitdir"),
        format!("{}\n", sandbox.root.join("missing").display()),
    )
    .unwrap();
    assert!(!records::alive(&common, &record));

    fs::write(
        admin.join("gitdir"),
        format!("{}\n", sandbox.root.display()),
    )
    .unwrap();
    assert!(records::alive(&common, &record));

    fs::remove_file(admin.join("gitdir")).unwrap();
    fs::write(admin.join("locked"), "").unwrap();
    assert!(records::alive(&common, &record));

    fs::write(admin.join(records::TOKEN), "other\n").unwrap();
    assert!(!records::alive(&common, &record));
}
