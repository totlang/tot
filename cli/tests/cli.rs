//! End-to-end tests: they run the real binary over stdin and check stdout, stderr, and the
//! exit code.

use std::io::Write;
use std::process::{Command, Stdio};

const EXE: &str = env!("CARGO_BIN_EXE_tot");

const DOC: &str = "\
name \"svc\"
port 8080
ratio 0.5
on true
tags [\"a\" \"b\"]
nested {
  x 1
  y [2 3]
}
";

struct Output {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(EXE)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn tot");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(stdin.as_bytes())
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait for tot");
    Output {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// The compact JSON of a document, as a comparable canonical form.
fn json(src: &str) -> String {
    let out = run(&["to", "json", "--compact"], src);
    assert_eq!(out.code, 0, "{}", out.stderr);
    out.stdout
}

// --- fmt and check ------------------------------------------------------------------------

#[test]
fn fmt_reads_stdin_and_writes_stdout() {
    let out = run(&["fmt"], "a:1,b:2");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "a 1\nb 2\n");
}

#[test]
fn fmt_check_reports_unformatted_input() {
    assert_eq!(run(&["fmt", "--check"], "a 1\nb 2\n").code, 0);

    let out = run(&["fmt", "--check"], "a:1");
    assert_eq!(out.code, 1);
    assert!(out.stdout.is_empty(), "--check must write nothing");
    assert!(out.stderr.contains("not formatted"), "{}", out.stderr);
}

#[test]
fn check_renders_a_diagnostic() {
    let out = run(&["check"], "address {\n  kind curly\n}");
    assert_eq!(out.code, 1);
    assert!(
        out.stderr.contains("string values must be quoted"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains("^^^^^"), "{}", out.stderr);
}

// --- JSON ---------------------------------------------------------------------------------

#[test]
fn to_json_is_indented_unless_asked_otherwise() {
    let out = run(&["to", "json"], "a 1 b [2 3]");
    assert_eq!(
        out.stdout,
        "{\n  \"a\": 1,\n  \"b\": [\n    2,\n    3\n  ]\n}\n"
    );
    assert_eq!(json("a 1 b [2 3]"), "{\"a\":1,\"b\":[2,3]}\n");
}

#[test]
fn from_json_only_reformats() {
    let out = run(&["from", "json"], r#"{"a": 1, "b": [2, 3]}"#);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "a 1\nb [\n  2\n  3\n]\n");
}

#[test]
fn json_keeps_integers_and_floats_apart() {
    assert_eq!(
        json("a 1 b 1.0 c 1. d .5"),
        "{\"a\":1,\"b\":1.0,\"c\":1.0,\"d\":0.5}\n"
    );
}

// --- YAML ---------------------------------------------------------------------------------

#[test]
fn yaml_round_trips() {
    let yaml = run(&["to", "yaml"], DOC);
    assert_eq!(yaml.code, 0, "{}", yaml.stderr);

    let back = run(&["from", "yaml"], &yaml.stdout);
    assert_eq!(back.code, 0, "{}", back.stderr);
    assert_eq!(json(&back.stdout), json(DOC));
}

#[test]
fn yaml_mappings_with_non_string_keys_are_refused() {
    let out = run(&["from", "yaml"], "1: one\n");
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("keys are always strings"),
        "{}",
        out.stderr
    );
}

// --- TOML ---------------------------------------------------------------------------------

#[test]
fn toml_round_trips_apart_from_nulls() {
    let toml = run(&["to", "toml"], DOC);
    assert_eq!(toml.code, 0, "{}", toml.stderr);

    let back = run(&["from", "toml"], &toml.stdout);
    assert_eq!(back.code, 0, "{}", back.stderr);
    assert_eq!(json(&back.stdout), json(DOC));
}

#[test]
fn toml_drops_nulls_and_reports_each_one() {
    let out = run(&["to", "toml"], "a 1 b null nested { c null }");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("a = 1"), "{}", out.stdout);
    assert!(!out.stdout.contains("b ="), "{}", out.stdout);
    assert!(out.stderr.contains("dropped null at b"), "{}", out.stderr);
    assert!(
        out.stderr.contains("dropped null at nested.c"),
        "{}",
        out.stderr
    );
}

#[test]
fn toml_null_error_policy_refuses_instead() {
    let out = run(&["to", "toml", "--null=error"], "a 1 b null");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("TOML has no null"), "{}", out.stderr);
}

#[test]
fn toml_needs_an_object_at_the_root() {
    let out = run(&["to", "toml"], "[1 2 3]");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("table at the root"), "{}", out.stderr);
}

#[test]
fn toml_datetimes_become_strings_with_a_warning() {
    let out = run(&["from", "toml"], "when = 1979-05-27T07:32:00Z\n");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout.contains("when \"1979-05-27T07:32:00Z\""),
        "{}",
        out.stdout
    );
    assert!(out.stderr.contains("datetime at when"), "{}", out.stderr);
}

// --- argument handling --------------------------------------------------------------------

#[test]
fn unknown_commands_flags_and_formats_exit_two() {
    assert_eq!(run(&["frobnicate"], "").code, 2);
    assert_eq!(run(&["fmt", "--sideways"], "a 1").code, 2);

    let out = run(&["to", "xml"], "a 1");
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("expected json, yaml, or toml"),
        "{}",
        out.stderr
    );
}

#[test]
fn help_is_printed_with_no_arguments() {
    for args in [&[][..], &["help"][..]] {
        let out = run(args, "");
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("USAGE"), "{}", out.stdout);
    }
}
