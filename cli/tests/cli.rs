//! End-to-end tests: they run the real binary over stdin and check stdout, stderr, and the
//! exit code.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const EXE: &str = env!("CARGO_BIN_EXE_tot");

/// A directory of this test's own, removed when the test ends.
///
/// The name carries the process id and a counter, so two runs at once — a watch process
/// alongside a manual one, or two CI jobs sharing a temp dir — cannot collide, and nothing
/// is left behind to be read by the next run.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("tot-cli-{name}-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        TempDir(dir)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A path as the CLI wants it. Tests pass real files, so this must not fail.
fn arg(path: &Path) -> &str {
    path.to_str().expect("temp paths are UTF-8")
}

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
    run_in(None, args, stdin)
}

/// Runs the binary, optionally from a given working directory — which is the only way to give
/// it a relative path, and so the only way to test one that starts with `--`.
fn run_in(dir: Option<&Path>, args: &[&str], stdin: &str) -> Output {
    let mut command = Command::new(EXE);
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let mut child = command
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
fn fmt_reports_an_unparseable_document_as_exit_one() {
    let out = run(&["fmt"], "kind curly");
    assert_eq!(out.code, 1);
    assert!(out.stdout.is_empty(), "nothing should be written");
    assert!(
        out.stderr.contains("string values must be quoted"),
        "{}",
        out.stderr
    );
}

/// One bad file must not leave the rest of a directory unformatted and unreported.
#[test]
fn fmt_processes_every_file_even_when_one_fails() {
    let dir = TempDir::new("fmt-continues");
    let (first, bad, last) = (
        dir.file("first.tot"),
        dir.file("bad.tot"),
        dir.file("last.tot"),
    );
    std::fs::write(&first, "a:1").expect("write");
    std::fs::write(&bad, "kind curly").expect("write");
    std::fs::write(&last, "b:2").expect("write");

    let out = run(&["fmt", arg(&first), arg(&bad), arg(&last)], "");

    assert_eq!(out.code, 1, "{}", out.stderr);
    assert!(
        out.stderr.contains("string values must be quoted"),
        "{}",
        out.stderr
    );
    assert_eq!(std::fs::read_to_string(&first).expect("read"), "a 1\n");
    assert_eq!(std::fs::read_to_string(&last).expect("read"), "b 2\n");
}

#[test]
fn formatting_a_file_in_place_succeeds() {
    let dir = TempDir::new("fmt-in-place");
    let path = dir.file("one.tot");
    std::fs::write(&path, "a:1").expect("write");

    let out = run(&["fmt", arg(&path)], "");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "a 1\n");
}

#[test]
fn an_unreadable_file_is_exit_two() {
    let out = run(&["check", "definitely-not-a-real-file.tot"], "");
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("definitely-not-a-real-file"),
        "{}",
        out.stderr
    );
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

#[test]
fn check_strict_flags_members_split_across_lines() {
    let split = "timeout\n30\n";

    // The document is legal, so plain check says nothing.
    assert_eq!(run(&["check"], split).code, 0);

    let out = run(&["check", "--strict"], split);
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("warning: "), "{}", out.stderr);
    assert!(out.stderr.contains("`timeout`"), "{}", out.stderr);

    assert_eq!(run(&["check", "--strict"], "timeout 30\n").code, 0);
    // A block value still only has to start on the key's line.
    assert_eq!(
        run(&["check", "--strict"], "listen {\n  port 8080\n}\n").code,
        0
    );
}

// --- merge --------------------------------------------------------------------------------

#[test]
fn merge_folds_files_left_to_right() {
    let dir = TempDir::new("merge");
    let base = dir.file("base.tot");
    let overlay = dir.file("prod.tot");
    std::fs::write(&base, "name \"svc\"\nlisten {host \"::\" port 80}\n").expect("write");
    std::fs::write(&overlay, "listen {port 8080}\nreplicas 3\n").expect("write");

    let out = run(&["merge", arg(&base), arg(&overlay)], "");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "name \"svc\"\nlisten {\n  host \"::\"\n  port 8080\n}\nreplicas 3\n"
    );
    // The output is a document, so it goes straight back into the CLI.
    assert_eq!(
        json(&out.stdout),
        r#"{"name":"svc","listen":{"host":"::","port":8080},"replicas":3}"#.to_string() + "\n"
    );
}

/// A `-` layer is how a merge takes part in a pipeline.
#[test]
fn merge_reads_stdin_as_a_layer() {
    let dir = TempDir::new("merge-stdin");
    let base = dir.file("base.tot");
    std::fs::write(&base, "a 1 b 2\n").expect("write");

    assert_eq!(
        json(&run(&["merge", arg(&base), "-"], "b 9 c 3").stdout),
        "{\"a\":1,\"b\":9,\"c\":3}\n"
    );
    // And on the other side, where the file wins.
    assert_eq!(
        json(&run(&["merge", "-", arg(&base)], "b 9 c 3").stdout),
        "{\"b\":2,\"c\":3,\"a\":1}\n"
    );
}

#[test]
fn merge_null_deletes_only_when_asked() {
    let doc = "a null\n";
    let dir = TempDir::new("merge-null");
    let base = dir.file("base.tot");
    std::fs::write(&base, "a 1 b 2\n").expect("write");

    assert_eq!(
        json(&run(&["merge", arg(&base), "-"], doc).stdout),
        "{\"a\":null,\"b\":2}\n"
    );
    assert_eq!(
        json(&run(&["merge", "--null=delete", arg(&base), "-"], doc).stdout),
        "{\"b\":2}\n"
    );
}

/// One output, so one bad layer is the end of it — a document merged from only some of its
/// layers would be worse than nothing.
#[test]
fn merge_stops_at_a_bad_layer() {
    let dir = TempDir::new("merge-bad");
    let bad = dir.file("bad.tot");
    std::fs::write(&bad, "kind curly").expect("write");

    let out = run(&["merge", arg(&bad), "-"], "a 1");
    assert_eq!(out.code, 1);
    assert!(out.stdout.is_empty(), "{}", out.stdout);
    assert!(
        out.stderr.contains("string values must be quoted"),
        "{}",
        out.stderr
    );

    let missing = run(&["merge", "definitely-not-a-real-file.tot"], "");
    assert_eq!(missing.code, 2);
}

#[test]
fn merge_with_one_input_is_that_document() {
    assert_eq!(
        run(&["merge"], "a:1,b:[2,3]").stdout,
        "a 1\nb [\n  2\n  3\n]\n"
    );
}

// --- get ----------------------------------------------------------------------------------

#[test]
fn get_prints_the_value_at_a_path() {
    assert_eq!(run(&["get", "port"], DOC).stdout, "8080\n");
    assert_eq!(run(&["get", "nested.y[1]"], DOC).stdout, "3\n");
    assert_eq!(run(&["get", "name"], DOC).stdout, "\"svc\"\n");
}

/// The default output is tot, so a value can go straight back into the next command.
#[test]
fn get_output_is_a_tot_document() {
    let out = run(&["get", "nested"], DOC);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "x 1\ny [\n  2\n  3\n]\n");
    assert_eq!(json(&out.stdout), "{\"x\":1,\"y\":[2,3]}\n");
}

#[test]
fn get_raw_drops_the_quotes_on_a_string() {
    assert_eq!(run(&["get", "--raw", "name"], DOC).stdout, "svc\n");
    // Only strings are affected; everything else is already unquoted.
    assert_eq!(run(&["get", "--raw", "port"], DOC).stdout, "8080\n");
}

#[test]
fn get_reads_a_file_as_well_as_stdin() {
    let dir = TempDir::new("get");
    let path = dir.file("config.tot");
    std::fs::write(&path, DOC).expect("write");

    let out = run(&["get", "--raw", "name", arg(&path)], "");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "svc\n");
}

/// `-` is a bareword character, so a key, a path, and a file may all begin with `--`. A bare
/// `--` is what makes them reachable.
#[test]
fn a_bare_double_dash_ends_the_flags() {
    let doc = "--foo 1\n";
    assert_eq!(run(&["get", "--", "--foo"], doc).stdout, "1\n");
    // Without it, the path is read as a flag.
    let out = run(&["get", "--foo"], doc);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("unknown flag"), "{}", out.stderr);

    // Flags before the `--` still apply.
    assert_eq!(
        run(&["get", "--raw", "--", "--foo"], "--foo \"x\"\n").stdout,
        "x\n"
    );

    // And a file whose own name starts with `--`, which needs a relative path to reproduce.
    let dir = TempDir::new("double-dash");
    let path = dir.file("--odd.tot");
    std::fs::write(&path, "a:1").expect("write");

    let out = run_in(Some(&dir.0), &["fmt", "--", "--odd.tot"], "");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "a 1\n");
}

/// A path the document does not have is exit 1: the command line was fine, the document just
/// did not answer it. A path that is not a path is exit 2.
#[test]
fn get_separates_a_missing_path_from_a_bad_one() {
    let missing = run(&["get", "nested.z"], DOC);
    assert_eq!(missing.code, 1);
    assert!(missing.stdout.is_empty(), "{}", missing.stdout);
    assert!(
        missing.stderr.contains("no member `z`"),
        "{}",
        missing.stderr
    );
    assert!(
        missing.stderr.contains("members are x, y"),
        "{}",
        missing.stderr
    );

    let malformed = run(&["get", "nested..z"], DOC);
    assert_eq!(malformed.code, 2);
    assert!(
        malformed.stderr.contains("expected a member name"),
        "{}",
        malformed.stderr
    );
}

/// The `.` in a key is the trap `get` has to get right: it nests in a path but not in a
/// document, so a key holding one is only reachable quoted.
#[test]
fn get_reaches_a_key_that_needs_quoting() {
    let doc = "com.example.owner \"platform-team\"\n\"log level\" \"debug\"\n";

    assert_eq!(
        run(&["get", "--raw", "\"com.example.owner\""], doc).stdout,
        "platform-team\n"
    );
    assert_eq!(
        run(&["get", "--raw", "\"log level\""], doc).stdout,
        "debug\n"
    );

    // Unquoted, the same key reads as three nested ones — and the miss says what was there,
    // spelled the way a path would have to spell it.
    let out = run(&["get", "com.example.owner"], doc);
    assert_eq!(out.code, 1);
    assert!(
        out.stderr.contains(r#""com.example.owner", "log level""#),
        "{}",
        out.stderr
    );
}

/// `-` means stdin everywhere a FILE is taken, not only in a merge.
#[test]
fn a_dash_is_stdin_for_every_command() {
    assert_eq!(run(&["fmt", "-"], "a:1").stdout, "a 1\n");
    assert_eq!(run(&["check", "-"], "a 1").code, 0);
    assert_eq!(run(&["get", "a", "-"], "a 1").stdout, "1\n");
    assert_eq!(
        run(&["to", "json", "--compact", "-"], "a 1").stdout,
        "{\"a\":1}\n"
    );

    // The diagnostic calls it stdin rather than `-`.
    let out = run(&["check", "-"], "kind curly");
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("in <stdin>"), "{}", out.stderr);
}

#[test]
fn get_needs_a_path() {
    let out = run(&["get"], DOC);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("needs a path"), "{}", out.stderr);
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

/// A document that does not parse is exit 1 wherever it is read — a converter must not
/// report it as a bad command line.
#[test]
fn a_document_that_does_not_parse_is_exit_one_everywhere() {
    for args in [
        &["to", "json"][..],
        &["to", "yaml"][..],
        &["from", "json"][..],
        &["get", "a"][..],
    ] {
        let out = run(args, "kind curly");
        assert_eq!(out.code, 1, "{args:?}: {}", out.stderr);
        assert!(
            out.stderr.contains("string values must be quoted"),
            "{args:?}: {}",
            out.stderr
        );
    }
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

/// The case the converter exists for: a YAML block scalar comes out as a tot block, not as
/// one long escaped line.
#[test]
fn from_yaml_emits_block_strings() {
    let out = run(&["from", "yaml"], "motd: |-\n  hello\n\n  world\n");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "motd \"\"\"\n  hello\n\n  world\n  \"\"\"\n");
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

/// A flag that cannot apply to the chosen format is an error, not a silent no-op — someone
/// passing `--null=error` as a guard has to hear about it if it is doing nothing.
#[test]
fn flags_are_rejected_for_formats_they_do_not_apply_to() {
    let out = run(&["to", "yaml", "--compact"], "a 1");
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("applies only to `tot to json`"),
        "{}",
        out.stderr
    );

    let out = run(&["to", "json", "--null=error"], "a 1");
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("applies only to `tot to toml`"),
        "{}",
        out.stderr
    );

    // They still work where they belong.
    assert_eq!(run(&["to", "json", "--compact"], "a 1").code, 0);
    assert_eq!(run(&["to", "toml", "--null=error"], "a 1").code, 0);
}

#[test]
fn help_is_printed_with_no_arguments() {
    for args in [&[][..], &["help"][..]] {
        let out = run(args, "");
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("USAGE"), "{}", out.stdout);
    }
}
