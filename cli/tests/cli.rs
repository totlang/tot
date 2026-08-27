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

#[test]
fn check_schema_reports_shape_problems() {
    let dir = TempDir::new("schema");
    let schema = dir.file("shape.tot");
    std::fs::write(
        &schema,
        "name \"string\"\nlisten {port \"int\" tls? \"bool\"}\n",
    )
    .expect("write");
    let flag = format!("--schema={}", arg(&schema));

    let good = "name \"svc\"\nlisten {port 8080}\n";
    assert_eq!(run(&["check", &flag], good).code, 0);

    let out = run(&["check", &flag], "name \"svc\"\nlisten {prot 8080}\n");
    assert_eq!(out.code, 1);
    assert!(
        out.stderr.contains("missing member `port`"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("unknown member `prot`"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("the schema has port, tls"),
        "{}",
        out.stderr
    );
    // The offending key gets a caret, like every other diagnostic.
    assert!(out.stderr.contains("^^^^"), "{}", out.stderr);
}

/// A schema that is not a schema is a bad command line, not a bad document.
#[test]
fn check_rejects_a_bad_schema_before_reading_anything() {
    let dir = TempDir::new("schema-bad");
    let schema = dir.file("shape.tot");
    std::fs::write(&schema, "port \"intt\"\n").expect("write");

    let out = run(
        &["check", &format!("--schema={}", arg(&schema))],
        "port 1\n",
    );
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("`intt` is not a type"),
        "{}",
        out.stderr
    );

    // And the flag needs its file attached, since a bare one would look like an input.
    let out = run(&["check", "--schema", "shape.tot"], "a 1\n");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("with an `=`"), "{}", out.stderr);
}

/// The schema and the lint are separate questions, and a document can fail both at once.
#[test]
fn check_schema_and_strict_compose() {
    let dir = TempDir::new("schema-strict");
    let schema = dir.file("shape.tot");
    std::fs::write(&schema, "timeout \"string\"\n").expect("write");
    let flag = format!("--schema={}", arg(&schema));

    let out = run(&["check", "--strict", &flag], "timeout\n30\n");
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("expected string"), "{}", out.stderr);
    assert!(out.stderr.contains("warning: "), "{}", out.stderr);
}

/// The example is the document that exercises every feature, so the schema beside it has to
/// describe all of them.
#[test]
fn the_example_config_matches_its_schema() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root");
    let flag = format!("--schema={}", arg(&root.join("examples/config.schema.tot")));
    let config = root.join("examples/config.tot");

    let out = run(&["check", "--strict", &flag, arg(&config)], "");
    assert_eq!(out.code, 0, "{}", out.stderr);
}

// --- build --------------------------------------------------------------------------------

/// The workspace root, for reaching the committed examples.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root")
        .to_path_buf()
}

const TEMPLATE: &str = "\
name \"svc\"
replicas (if (param \"prod\") 5 1)
image (str \"reg/\" (param \"name\" \"svc\") \":\" (param \"tag\"))
";

#[test]
fn build_writes_the_document_to_stdout() {
    let dir = TempDir::new("build");
    let template = dir.file("config.tott");
    std::fs::write(&template, TEMPLATE).expect("write");

    let out = run(
        &[
            "build",
            "--set=prod=true",
            "--set-raw=tag=v1",
            arg(&template),
        ],
        "",
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout,
        "name \"svc\"\nreplicas 5\nimage \"reg/svc:v1\"\n"
    );

    // The output is an ordinary document, so it goes straight back into the CLI.
    assert_eq!(
        json(&out.stdout),
        "{\"name\":\"svc\",\"replicas\":5,\"image\":\"reg/svc:v1\"}\n"
    );
}

/// `--set` takes a tot value and `--set-raw` takes a string, the same split `tot set` uses —
/// so a value means one thing across the whole CLI.
#[test]
fn build_parameters_are_values_unless_raw() {
    let dir = TempDir::new("build-params");
    let template = dir.file("c.tott");
    std::fs::write(&template, "a (param \"x\")\n").expect("write");
    let build = |flag: &str| run(&["build", flag, arg(&template)], "").stdout;

    assert_eq!(build("--set=x=1"), "a 1\n");
    assert_eq!(build("--set=x=true"), "a true\n");
    assert_eq!(build("--set=x=[1 2]"), "a [\n  1\n  2\n]\n");
    assert_eq!(build("--set-raw=x=1"), "a \"1\"\n");
    assert_eq!(build("--set-raw=x=prod"), "a \"prod\"\n");

    // A bare string is the same parse error it would be anywhere else.
    let out = run(&["build", "--set=x=prod", arg(&template)], "");
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("string values must be quoted"),
        "{}",
        out.stderr
    );
}

#[test]
fn build_out_writes_a_file_and_check_compares_it() {
    let dir = TempDir::new("build-check");
    let template = dir.file("c.tott");
    let document = dir.file("c.tot");
    std::fs::write(&template, TEMPLATE).expect("write");
    let params = ["--set=prod=true", "--set-raw=tag=v1"];

    let out = run(
        &[
            "build",
            params[0],
            params[1],
            &format!("--out={}", arg(&document)),
            arg(&template),
        ],
        "",
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.is_empty(), "--out writes the file, not stdout");

    // `--check` infers the same path from the template's name.
    let out = run(
        &["build", params[0], params[1], "--check", arg(&template)],
        "",
    );
    assert_eq!(out.code, 0, "{}", out.stderr);

    // A parameter that changes the output is a drift `--check` has to catch.
    let out = run(
        &[
            "build",
            "--set=prod=false",
            params[1],
            "--check",
            arg(&template),
        ],
        "",
    );
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("is not what"), "{}", out.stderr);
}

/// A missing parameter is the build failing, not a guess.
#[test]
fn build_reports_a_missing_parameter_with_a_caret() {
    let dir = TempDir::new("build-missing");
    let template = dir.file("c.tott");
    std::fs::write(&template, TEMPLATE).expect("write");

    let out = run(&["build", "--set=prod=true", arg(&template)], "");
    assert_eq!(out.code, 1);
    assert!(out.stdout.is_empty(), "{}", out.stdout);
    assert!(
        out.stderr.contains("no value for parameter `tag`"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains("^^^^"), "{}", out.stderr);
    assert!(
        out.stderr.contains("the parameters set are prod"),
        "{}",
        out.stderr
    );
}

/// Imports resolve relative to the file doing the importing, so a directory of templates keeps
/// working wherever it is and whatever directory the build runs from.
#[test]
fn build_imports_relative_to_the_importing_file() {
    let dir = TempDir::new("build-import");
    std::fs::create_dir_all(dir.file("parts")).expect("mkdir");
    std::fs::write(dir.file("parts/regions.tot"), "[\"us\" \"eu\"]\n").expect("write");
    std::fs::write(
        dir.file("parts/inner.tott"),
        "regions (import \"regions.tot\")\n",
    )
    .expect("write");
    let template = dir.file("top.tott");
    std::fs::write(&template, "block (import \"parts/inner.tott\")\n").expect("write");

    // Run from somewhere else entirely, to prove the paths are not relative to the invocation.
    let out = run_in(Some(&root()), &["build", arg(&template)], "");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        json(&out.stdout),
        "{\"block\":{\"regions\":[\"us\",\"eu\"]}}\n"
    );
}

/// A file that imports itself, however indirectly, has no value to be replaced by.
#[test]
fn build_refuses_an_import_cycle() {
    let dir = TempDir::new("build-cycle");
    std::fs::write(dir.file("a.tott"), "x (import \"b.tott\")\n").expect("write");
    std::fs::write(dir.file("b.tott"), "y (import \"a.tott\")\n").expect("write");

    let out = run(&["build", arg(&dir.file("a.tott"))], "");
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("is a cycle"), "{}", out.stderr);
    assert!(out.stderr.contains("a.tott"), "{}", out.stderr);
}

/// A failure inside an imported file is reported in that file, with the chain that reached it.
#[test]
fn build_reports_a_failure_in_the_file_it_happened_in() {
    let dir = TempDir::new("build-chain");
    std::fs::write(dir.file("top.tott"), "a (import \"leaf.tott\")\n").expect("write");
    std::fs::write(dir.file("leaf.tott"), "b 1\nc (str null)\n").expect("write");

    let out = run(&["build", arg(&dir.file("top.tott"))], "");
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("in "), "{}", out.stderr);
    assert!(out.stderr.contains("leaf.tott"), "{}", out.stderr);
    assert!(
        out.stderr.contains("`str` has no spelling for null"),
        "{}",
        out.stderr
    );
    assert!(
        out.stderr.contains("imported from"),
        "the chain: {}",
        out.stderr
    );
}

/// The committed example must still be what its template builds — which is the whole point of
/// `--check`, tested on the pair the repository ships.
#[test]
fn the_example_template_still_builds_its_document() {
    let out = run_in(
        Some(&root()),
        &[
            "build",
            "--set=prod=true",
            "--set-raw=tag=v1.4.2",
            "--check",
            "examples/service.tott",
        ],
        "",
    );
    assert_eq!(out.code, 0, "{}", out.stderr);

    // What it builds is canonical tot, not merely parseable tot — and the template itself is
    // canonical too, now that `fmt` can read one.
    let out = run_in(
        Some(&root()),
        &[
            "fmt",
            "--check",
            "examples/service.tot",
            "examples/service.tott",
            "examples/regions.tot",
        ],
        "",
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
}

/// `fmt` reads a `.tott` file as a template, the same way `(import …)` decides by extension.
#[test]
fn fmt_formats_a_template_by_its_extension() {
    let dir = TempDir::new("fmt-tott");
    let path = dir.file("c.tott");
    std::fs::write(&path, "a:(str   \"x\"    (param  \"n\"))\n").expect("write");

    let out = run(&["fmt", arg(&path)], "");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "a (str \"x\" (param \"n\"))\n"
    );

    // And `--check` reports one that has drifted, so a template is kept honest like anything
    // else in the repository.
    std::fs::write(&path, "a (str  \"x\")\n").expect("write");
    let out = run(&["fmt", "--check", arg(&path)], "");
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("not formatted"), "{}", out.stderr);
}

/// A `.tot` file is still data even next to templates, so its parens stay ordinary — which is
/// the guarantee the whole dialect split exists to keep.
#[test]
fn fmt_still_reads_a_tot_file_as_data() {
    let dir = TempDir::new("fmt-tot-parens");
    let path = dir.file("c.tot");
    std::fs::write(&path, "\"(a)\" 1\n").expect("write");

    let out = run(&["fmt", arg(&path)], "");
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "(a) 1\n");

    // The same key in a template keeps its quotes, or it would become a form.
    assert_eq!(
        run(&["fmt", "--template"], "\"(a)\" 1\n").stdout,
        "\"(a)\" 1\n"
    );
}

/// Stdin has no extension, so it is a document unless `--template` says otherwise. Guessing
/// from the contents would be the implicit typing the language exists to avoid.
#[test]
fn fmt_needs_to_be_told_when_stdin_is_a_template() {
    let template = "a (str  \"x\")\n";

    let out = run(&["fmt"], template);
    assert_eq!(out.code, 1, "a form is not a value in a document");
    assert!(
        out.stderr.contains("string values must be quoted"),
        "{}",
        out.stderr
    );

    let out = run(&["fmt", "--template"], template);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "a (str \"x\")\n");
}

#[test]
fn build_needs_a_template_and_takes_only_one() {
    let out = run(&["build"], "");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("needs a template"), "{}", out.stderr);

    let out = run(&["build", "a.tott", "b.tott"], "");
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("takes one template"), "{}", out.stderr);

    // Each value-taking flag needs its `=`, for the reason `--schema` does.
    for flag in ["--out", "--set", "--set-raw"] {
        let out = run(&["build", flag, "x.tot", "c.tott"], "");
        assert_eq!(out.code, 2, "{flag}");
        assert!(out.stderr.contains("with an `=`"), "{}", out.stderr);
    }
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

// --- set ----------------------------------------------------------------------------------

#[test]
fn set_writes_a_value_and_prints_the_document() {
    let out = json(&run(&["set", "port", "9090"], DOC).stdout);
    assert!(out.contains("\"port\":9090"), "{out}");
    assert_eq!(run(&["set", "a", "2"], "a 1\n").stdout, "a 2\n");
    // A new member lands at the end.
    assert_eq!(run(&["set", "b", "2"], "a 1\n").stdout, "a 1\nb 2\n");
}

/// The value is spelled the way `get` prints one, so the pair round-trips — including the
/// quotes on a string, which is what `--raw` exists to avoid typing.
#[test]
fn set_takes_the_value_get_prints() {
    assert_eq!(run(&["set", "a", "\"svc\""], "a 1\n").stdout, "a \"svc\"\n");
    assert_eq!(
        run(&["set", "--raw", "a", "svc"], "a 1\n").stdout,
        "a \"svc\"\n"
    );
    assert_eq!(
        run(&["set", "a", "[1 2]"], "a 1\n").stdout,
        "a [\n  1\n  2\n]\n"
    );
    assert_eq!(
        run(&["set", "a", "{b 1}"], "a 1\n").stdout,
        "a {\n  b 1\n}\n"
    );

    // An unquoted string is the same parse error it would be in a document.
    let out = run(&["set", "a", "svc"], "a 1\n");
    assert_eq!(out.code, 2);
    assert!(
        out.stderr.contains("string values must be quoted"),
        "{}",
        out.stderr
    );
}

#[test]
fn set_needs_the_path_to_exist_unless_told_otherwise() {
    let out = run(&["set", "a.b.c", "1"], "a {}\n");
    assert_eq!(out.code, 1);
    assert!(out.stdout.is_empty(), "{}", out.stdout);
    assert!(out.stderr.contains("no member `b`"), "{}", out.stderr);

    assert_eq!(
        json(&run(&["set", "--create", "a.b.c", "1"], "a {}\n").stdout),
        "{\"a\":{\"b\":{\"c\":1}}}\n"
    );
}

#[test]
fn set_needs_a_path_and_a_value() {
    for args in [&["set"][..], &["set", "a"][..]] {
        let out = run(args, "a 1\n");
        assert_eq!(out.code, 2, "{args:?}");
        assert!(
            out.stderr.contains("needs a path and a value"),
            "{}",
            out.stderr
        );
    }
}

/// merge, get, and set all read and write documents, so they compose.
#[test]
fn set_chains_with_the_other_commands() {
    let first = run(
        &["set", "listen.port", "9090"],
        "listen {host \"::\" port 80}\n",
    );
    assert_eq!(first.code, 0, "{}", first.stderr);
    let second = run(&["set", "--raw", "listen.host", "0.0.0.0"], &first.stdout);
    assert_eq!(second.code, 0, "{}", second.stderr);

    assert_eq!(
        json(&second.stdout),
        "{\"listen\":{\"host\":\"0.0.0.0\",\"port\":9090}}\n"
    );
    assert_eq!(
        run(&["get", "--raw", "listen.host"], &second.stdout).stdout,
        "0.0.0.0\n"
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

/// The help's schema example, run as a schema.
///
/// It once showed unquoted type names — the design the parser refuses — and nothing caught it,
/// because the only test of the help asserted that it contained the word USAGE. Documentation
/// that can be executed should be.
#[test]
fn the_help_teaches_a_schema_that_works() {
    let help = run(&["help"], "").stdout;

    // The examples are indented one step further than the section body they sit in, and the
    // block ends at the blank line before the prose picks up again.
    let section = help
        .split_once("SCHEMA\n")
        .expect("a SCHEMA section")
        .1
        .lines()
        .skip_while(|line| !line.starts_with("        "))
        .take_while(|line| line.starts_with("        "))
        .map(|line| line.trim_start())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(section.contains("regions"), "extracted:\n{section}");

    let dir = TempDir::new("help-schema");
    let schema = dir.file("from-help.tot");
    std::fs::write(&schema, &section).expect("write");
    let flag = format!("--schema={}", arg(&schema));

    // It compiles, and it describes the document it appears to describe.
    let good = "name \"svc\"\nlisten {host \"::\" port 80}\nregions [\"us-west-2\"]\n\
                labels {team \"core\"}\nretries null\n";
    let out = run(&["check", &flag], good);
    assert_eq!(out.code, 0, "the help's own schema: {}", out.stderr);

    // And it is a real check, not one that accepts anything.
    let out = run(&["check", &flag], "name 1\nlisten {host \"::\" port 80}\n");
    assert_eq!(out.code, 1);
    assert!(out.stderr.contains("expected string"), "{}", out.stderr);
}
