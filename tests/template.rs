//! Template tests. A `.tott` file is the data language plus one production — a form, wherever
//! a value goes — so most of what is worth checking is that production and the guarantee that
//! adding it changed nothing about `.tot`.

use std::collections::HashMap;

use tot::template::{Imports, Loaded, NoImports, Params, Template};
use tot::{Dialect, Value};

/// Builds `src` with the given parameters and no imports, as compact JSON.
fn build(src: &str, params: &[(&str, Value)]) -> Result<String, String> {
    let template = Template::parse(src).map_err(|e| e.render(src))?;
    let mut set = Params::new();
    for (name, value) in params {
        set.set(*name, value.clone());
    }
    template
        .evaluate(&set)
        .map(|value| tot::json::to_string(&value))
        .map_err(|e| e.render())
}

fn plain(src: &str) -> Result<String, String> {
    build(src, &[])
}

/// The message of a build that is expected to fail.
fn fails(src: &str) -> String {
    plain(src).expect_err("should not build")
}

fn string(s: &str) -> Value {
    Value::String(s.to_string())
}

fn int(n: i64) -> Value {
    Value::Integer(tot::Integer::from_i64(n))
}

/// An importer backed by a map, so the evaluator can be tested without touching a disk.
///
/// It counts what it was asked for, which is how a test can tell a file that was built once
/// from one that was built over and over.
struct Files {
    files: HashMap<String, String>,
    loads: usize,
}

impl Files {
    fn new(files: &[(&str, &str)]) -> Self {
        Files {
            files: files
                .iter()
                .map(|(name, src)| (name.to_string(), src.to_string()))
                .collect(),
            loads: 0,
        }
    }
}

impl Imports for Files {
    fn load(&mut self, _from: &str, target: &str) -> Result<Loaded, String> {
        self.loads += 1;
        let source = self
            .files
            .get(target)
            .ok_or_else(|| format!("cannot import `{target}`: no such file"))?;
        Ok(Loaded {
            name: target.to_string(),
            source: source.clone(),
            // The dialect follows the extension, which is what keeps a `.tot` file data even
            // when a template is what pulled it in.
            dialect: if target.ends_with(".tott") {
                Dialect::Template
            } else {
                Dialect::Data
            },
        })
    }
}

fn build_with(src: &str, files: &[(&str, &str)]) -> Result<String, String> {
    let template = Template::parse_named(src, "main.tott").map_err(|e| e.render(src))?;
    template
        .build(&Params::new(), &mut Files::new(files))
        .map(|value| tot::json::to_string(&value))
        .map_err(|e| e.render())
}

// --- the data language is unchanged -------------------------------------------------------

/// The whole design rests on this: reserving parens for forms must not reach `.tot`, where
/// `(` and `)` are ordinary bareword characters and documents already rely on it.
#[test]
fn parens_are_still_data_in_a_tot_document() {
    let document = tot::parse("(a) 1  @type 2  $ref 3").expect("legal tot");
    assert_eq!(
        tot::json::to_string(&document),
        r#"{"(a)":1,"@type":2,"$ref":3}"#
    );

    // The formatter still unquotes such a key, because `.tot` is what it writes.
    assert_eq!(tot::format(r#""(a)" 1"#).unwrap(), "(a) 1\n");

    // In a template the same text is a form, and `a` is not one.
    assert!(Template::parse("(a) 1").is_err());
}

/// A template with no forms in it is just a document, and builds to itself.
#[test]
fn a_template_without_forms_is_a_document() {
    let src = "name \"svc\"\nlisten {host \"::\" port 80}\nregions [\"a\" \"b\"]\n";
    let template = Template::parse(src).unwrap();

    assert!(template.is_data(), "no forms, so nothing to evaluate");
    assert_eq!(
        plain(src).unwrap(),
        tot::json::to_string(&tot::parse(src).unwrap())
    );
}

/// Every JSON document is a valid template too, since it is a valid document.
#[test]
fn json_is_still_a_valid_template() {
    assert_eq!(
        plain(r#"{"a": [1, 2], "b": null}"#).unwrap(),
        r#"{"a":[1,2],"b":null}"#
    );
}

// --- param --------------------------------------------------------------------------------

#[test]
fn a_parameter_is_substituted() {
    assert_eq!(
        build(r#"env (param "env")"#, &[("env", string("prod"))]).unwrap(),
        r#"{"env":"prod"}"#
    );
    // A parameter carries a whole value, not just a string.
    assert_eq!(
        build(
            r#"limits (param "limits")"#,
            &[("limits", tot::parse("{cpu 2}").unwrap())]
        )
        .unwrap(),
        r#"{"limits":{"cpu":2}}"#
    );
}

#[test]
fn a_parameter_that_was_not_set_is_an_error_naming_the_ones_that_were() {
    let message =
        build(r#"a (param "missing")"#, &[("env", string("prod"))]).expect_err("should not build");
    assert!(
        message.contains("no value for parameter `missing`"),
        "{message}"
    );
    assert!(message.contains("the parameters set are env"), "{message}");

    assert!(fails(r#"a (param "x")"#).contains("no parameters were set"));
}

/// A default is what makes a template usable without spelling out every parameter every time.
#[test]
fn a_parameter_may_have_a_default() {
    assert_eq!(
        plain(r#"replicas (param "replicas" 1)"#).unwrap(),
        r#"{"replicas":1}"#
    );
    assert_eq!(
        build(r#"replicas (param "replicas" 1)"#, &[("replicas", int(5))]).unwrap(),
        r#"{"replicas":5}"#
    );
}

/// A parameter's name is written down, so a reader can see what a template needs without
/// running it.
#[test]
fn a_parameter_name_may_not_be_computed() {
    let message = fails(r#"a (param (str "en" "v"))"#);
    assert!(
        message.contains("a param's name has to be written down"),
        "{message}"
    );
}

// --- if -----------------------------------------------------------------------------------

#[test]
fn a_condition_picks_a_branch() {
    let src = r#"replicas (if (param "prod") 5 1)"#;
    assert_eq!(
        build(src, &[("prod", Value::Bool(true))]).unwrap(),
        r#"{"replicas":5}"#
    );
    assert_eq!(
        build(src, &[("prod", Value::Bool(false))]).unwrap(),
        r#"{"replicas":1}"#
    );
}

/// tot has no truthiness in a document, and a template does not get its own rules.
#[test]
fn a_condition_has_to_be_a_boolean() {
    let message = fails("a (if 1 \"y\" \"n\")");
    assert!(
        message.contains("the condition of `if` is a boolean, but this is an integer"),
        "{message}"
    );
    assert!(message.contains("no truthiness"), "{message}");
}

/// Only the branch taken is evaluated, so the other may name a file this configuration does
/// not have.
#[test]
fn the_branch_not_taken_is_not_evaluated() {
    assert_eq!(
        build_with(
            r#"a (if true (import "there.tot") (import "gone.tot"))"#,
            &[("there.tot", "x 1")]
        )
        .unwrap(),
        r#"{"a":{"x":1}}"#
    );
    // And the same template the other way round does reach for the missing file.
    let message = build_with(
        r#"a (if false (import "there.tot") (import "gone.tot"))"#,
        &[("there.tot", "x 1")],
    )
    .expect_err("gone.tot is not there");
    assert!(message.contains("no such file"), "{message}");
}

// --- str ----------------------------------------------------------------------------------

#[test]
fn str_joins_what_has_an_obvious_spelling() {
    assert_eq!(
        build(
            r#"image (str "registry/" (param "name") ":" (param "tag"))"#,
            &[("name", string("svc")), ("tag", int(3))]
        )
        .unwrap(),
        r#"{"image":"registry/svc:3"}"#
    );
    assert_eq!(
        plain(r#"a (str "n=" 1 " f=" 1. " b=" true)"#).unwrap(),
        r#"{"a":"n=1 f=1.0 b=true"}"#
    );
    assert_eq!(plain("a (str)").unwrap(), r#"{"a":""}"#);
}

#[test]
fn str_refuses_what_would_be_a_guess() {
    for (src, what) in [
        ("a (str null)", "null"),
        ("a (str [1 2])", "an array"),
        ("a (str {b 1})", "an object"),
    ] {
        let message = fails(src);
        assert!(
            message.contains(&format!("`str` has no spelling for {what}")),
            "`{src}`: {message}"
        );
    }
}

// --- import -------------------------------------------------------------------------------

#[test]
fn importing_a_document_embeds_its_value() {
    assert_eq!(
        build_with(
            r#"regions (import "regions.tot")  name "svc""#,
            &[("regions.tot", r#"["us-west-2" "eu-central-1"]"#)]
        )
        .unwrap(),
        r#"{"regions":["us-west-2","eu-central-1"],"name":"svc"}"#
    );
}

/// A `.tot` file is data even when a template imported it, so its parens stay ordinary.
#[test]
fn an_imported_document_is_read_as_data() {
    assert_eq!(
        build_with(r#"a (import "d.tot")"#, &[("d.tot", "(x) 1")]).unwrap(),
        r#"{"a":{"(x)":1}}"#
    );
}

/// An imported template is evaluated, and sees the same parameters — they belong to the build,
/// not to a file.
#[test]
fn an_imported_template_is_evaluated() {
    let template = Template::parse_named(r#"a (import "part.tott")"#, "main.tott").unwrap();
    let mut params = Params::new();
    params.set("env", string("prod"));

    let built = template
        .build(
            &params,
            &mut Files::new(&[("part.tott", r#"env (param "env")"#)]),
        )
        .unwrap();
    assert_eq!(tot::json::to_string(&built), r#"{"a":{"env":"prod"}}"#);
}

/// An import graph has to be acyclic: a file that imports itself has no value to be replaced by.
#[test]
fn a_cycle_is_reported_as_one() {
    // The root is called main.tott, so the importer has to be able to hand it back.
    let message = build_with(
        r#"a (import "b.tott")"#,
        &[
            ("b.tott", r#"c (import "main.tott")"#),
            ("main.tott", r#"a (import "b.tott")"#),
        ],
    )
    .expect_err("a cycle");

    assert!(message.contains("is a cycle"), "{message}");
    assert!(
        message.contains("main.tott → b.tott → main.tott"),
        "{message}"
    );

    // Direct self-import too.
    let message = build_with(
        r#"a (import "s.tott")"#,
        &[("s.tott", r#"b (import "s.tott")"#)],
    )
    .expect_err("a cycle");
    assert!(message.contains("is a cycle"), "{message}");
}

/// Importing the same file twice is not a cycle — only a file that is still open is.
#[test]
fn the_same_file_may_be_imported_twice() {
    assert_eq!(
        build_with(
            r#"a (import "d.tot")  b (import "d.tot")"#,
            &[("d.tot", "x 1")]
        )
        .unwrap(),
        r#"{"a":{"x":1},"b":{"x":1}}"#
    );
}

/// An import's path is written down, so the graph is visible without running the build.
#[test]
fn an_import_path_may_not_be_computed() {
    let message = fails(r#"a (import (str "reg" ".tot"))"#);
    assert!(
        message.contains("a import's path has to be written down"),
        "{message}"
    );
}

#[test]
fn a_template_with_no_importer_says_so() {
    assert!(fails(r#"a (import "x.tot")"#).contains("no importer"));
    let _ = NoImports; // the type is public, for a caller that wants to be explicit
}

/// A shared fragment is the ordinary reason to have one, so a graph that shares must not cost
/// time exponential in its depth. Each file is built once and its value reused.
#[test]
fn a_file_is_built_once_however_many_times_it_is_imported() {
    // Each level joins the level below it with itself. The joined value is the empty string,
    // so the document stays tiny while the number of *evaluations* is what the shape decides:
    // one per level with reuse, 2^depth without.
    const DEPTH: usize = 20;
    let mut files: Vec<(String, String)> = (0..DEPTH)
        .map(|i| {
            (
                format!("f{i}.tott"),
                format!(
                    r#"(str (import "f{n}.tott") (import "f{n}.tott"))"#,
                    n = i + 1
                ),
            )
        })
        .collect();
    files.push((format!("f{DEPTH}.tott"), "\"\"".to_string()));

    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, src)| (name.as_str(), src.as_str()))
        .collect();
    let mut importer = Files::new(&borrowed);

    let built = Template::parse_named(r#"root (import "f0.tott")"#, "main.tott")
        .unwrap()
        .build(&Params::new(), &mut importer)
        .expect("builds");

    assert_eq!(built.get("root").and_then(Value::as_str), Some(""));

    // One load per import written, not per path through the graph. Without reuse this is
    // 2^20 — over a million — and the assertion is what says so rather than the clock.
    let edges = 2 * DEPTH + 1;
    assert!(
        importer.loads <= edges,
        "{} loads for {edges} imports: a file is being rebuilt",
        importer.loads
    );
}

/// Reuse must not swallow a cycle: a file in the cache has finished building, so it cannot
/// also be open on the stack.
#[test]
fn reuse_does_not_hide_a_cycle() {
    // `shared.tot` is imported twice before the cycle is reached, so the cache is warm.
    let message = build_with(
        r#"a (import "shared.tot") b (import "shared.tot") c (import "loop.tott")"#,
        &[
            ("shared.tot", "x 1"),
            ("loop.tott", r#"d (import "main.tott")"#),
            ("main.tott", "unused 1"),
        ],
    )
    .expect_err("a cycle");
    assert!(message.contains("is a cycle"), "{message}");
}

/// A failure inside an imported file draws its caret in that file, and says how the build
/// got there.
#[test]
fn an_error_inside_an_import_names_the_file_and_the_chain() {
    let message = build_with(
        r#"a (import "mid.tott")"#,
        &[
            ("mid.tott", "b (import \"leaf.tott\")"),
            ("leaf.tott", "c 1\nd (str null)\n"),
        ],
    )
    .expect_err("leaf fails");

    assert!(message.starts_with("in leaf.tott\n"), "{message}");
    // The caret is on the offending argument in leaf.tott, not on the import that reached it.
    assert!(message.contains("--> 2:8"), "{message}");
    assert!(message.contains("d (str null)"), "{message}");
    assert!(message.contains("  imported from mid.tott\n"), "{message}");
    assert!(message.contains("  imported from main.tott\n"), "{message}");
}

/// An imported file that does not parse is reported against itself, not against the importer.
#[test]
fn an_imported_file_that_does_not_parse_is_reported_in_that_file() {
    let message = build_with(r#"a (import "bad.tot")"#, &[("bad.tot", "kind curly")])
        .expect_err("bad.tot does not parse");
    assert!(message.starts_with("in bad.tot\n"), "{message}");
    assert!(
        message.contains("string values must be quoted"),
        "{message}"
    );
}

/// The pieces a caller needs to render a build failure some other way than `render` does.
/// They are the public surface of a `BuildError`, so what they promise is worth pinning.
#[test]
fn a_build_error_carries_the_parts_of_its_diagnostic() {
    let template = Template::parse_named(r#"a (import "mid.tott")"#, "main.tott").unwrap();
    assert_eq!(template.name(), "main.tott");

    let e = template
        .build(
            &Params::new(),
            &mut Files::new(&[
                ("mid.tott", r#"b (import "leaf.tott")"#),
                ("leaf.tott", "c (str null)"),
            ]),
        )
        .expect_err("leaf fails");

    // The failure belongs to the file it happened in, not to the one the build started at.
    assert_eq!(e.file(), "leaf.tott");
    assert_eq!(e.text(), "c (str null)");
    // The chain reads from the root down to whatever imported the failing file.
    assert_eq!(e.chain(), ["main.tott", "mid.tott"]);

    // The span indexes that file's text, which is what makes a caret land in the right place.
    let span = e.error().span;
    assert_eq!(&e.text()[span.start..span.end], "null");
    assert!(e.error().message.contains("no spelling for null"));
}

/// `Params` reports whether it has anything, which is what a caller checks before deciding a
/// build needs arguments it was not given.
#[test]
fn params_knows_whether_it_is_empty() {
    let mut params = Params::new();
    assert!(params.is_empty());
    params.set("x", int(1));
    assert!(!params.is_empty());
    assert_eq!(params.get("x"), Some(&int(1)));
    assert_eq!(params.get("y"), None);
}

// --- the shape of a form --------------------------------------------------------------------

#[test]
fn a_form_that_is_not_a_form_is_refused() {
    for (src, expected) in [
        ("a (nope 1)", "`nope` is not a form"),
        ("a ()", "a form needs a name"),
        (r#"a ("str" 1)"#, "a form begins with its name"),
        ("a (str", "unclosed `("),
        (
            "a (param)",
            "`param` takes 1 or 2 arguments, but was given none",
        ),
        (
            r#"a (param "x" 1 2)"#,
            "`param` takes 1 or 2 arguments, but was given 3",
        ),
        (
            r#"a (if true 1)"#,
            "`if` takes 3 arguments, but was given 2",
        ),
        (
            r#"a (import "x.tot" "y.tot")"#,
            "`import` takes 1 argument, but was given 2",
        ),
    ] {
        let message = fails(src);
        assert!(message.contains(expected), "`{src}`: {message}");
    }
}

/// A computed key would make the shape of a document depend on evaluating it, and the shape
/// is what a reader most needs to see without running anything.
#[test]
fn a_form_cannot_be_a_key() {
    let message = fails(r#"(str "a" "b") 1"#);
    assert!(message.contains("a form cannot be a key"), "{message}");
}

#[test]
fn forms_nest() {
    assert_eq!(
        build(
            r#"a (str "x" (if (param "p") (str "-" (param "e")) ""))"#,
            &[("p", Value::Bool(true)), ("e", string("prod"))]
        )
        .unwrap(),
        r#"{"a":"x-prod"}"#
    );
}

/// A form goes wherever a value goes, including as an array element and as a whole document.
#[test]
fn a_form_goes_anywhere_a_value_goes() {
    assert_eq!(
        build(r#"xs [1 (param "n") 3]"#, &[("n", int(2))]).unwrap(),
        r#"{"xs":[1,2,3]}"#
    );
    assert_eq!(
        build(
            r#"(param "whole")"#,
            &[("whole", tot::parse("a 1").unwrap())]
        )
        .unwrap(),
        r#"{"a":1}"#
    );
    assert_eq!(
        build_with(r#"(import "d.tot")"#, &[("d.tot", "a 1")]).unwrap(),
        r#"{"a":1}"#
    );
}

/// The parity hazard is the language's, so a template inherits both the trap and the
/// diagnostic that makes it survivable.
#[test]
fn a_missing_value_is_still_blamed_on_its_key() {
    assert!(fails("debug\n").contains("key `debug` has no value"));
    assert!(fails("a 1 b").contains("key `b` has no value"));
}

#[test]
fn duplicate_keys_are_still_an_error() {
    assert!(fails(r#"a 1 a (param "x")"#).contains("duplicate key `a`"));
    assert!(fails(r#"o {a (str "x") a 2}"#).contains("duplicate key `a`"));
}

/// The list a diagnostic gives is the list of forms.
///
/// Help that has drifted from the tool is the defect this codebase keeps finding, and three
/// separate spellings of the same list is how it happens. This checks the direction that can be
/// checked from outside: every name the help offers really is a form.
#[test]
fn the_forms_a_diagnostic_lists_are_the_forms() {
    let message = fails("a (nope)");
    let (_, rest) = message.split_once("the forms are ").expect("names them");
    let names: Vec<&str> = rest
        .split(';')
        .next()
        .expect("a first part")
        .split(", ")
        .map(|name| name.trim_start_matches("and "))
        .collect();
    assert_eq!(names.len(), 7, "{names:?}");

    for name in &names {
        // A real form gets past the head check, so whatever it fails on next is not that.
        // `(str)` is legal with no arguments at all, so a success is fine too.
        if let Err(refused) = plain(&format!("a ({name})")) {
            assert!(
                !refused.contains(&format!("`{name}` is not a form")),
                "the help offers `{name}`, which is not a form"
            );
        }
    }
    for name in ["param", "if", "str", "import", "get", "map", "it"] {
        assert!(names.contains(&name), "the help omits `{name}`: {names:?}");
    }
}

// --- get --------------------------------------------------------------------------------------

/// **What `get` reads from is an argument**, which is the decision this form needed. Reaching
/// into the document being built would make a template's meaning depend on the order its
/// members happen to be evaluated in.
#[test]
fn get_reads_one_value_out_of_another() {
    assert_eq!(
        plain(r#"a (get "listen.port" {listen {host "::" port 80}})"#).unwrap(),
        r#"{"a":80}"#
    );
    assert_eq!(
        plain(r#"a (get "xs[1]" {xs [10 20 30]})"#).unwrap(),
        r#"{"a":20}"#
    );
    // A collection comes back whole, the way `tot get` prints one.
    assert_eq!(
        plain(r#"a (get "listen" {listen {port 80}})"#).unwrap(),
        r#"{"a":{"port":80}}"#
    );
    // The ordinary case: reaching into a file rather than a literal written beside it.
    assert_eq!(
        build_with(
            r#"port (get "listen.port" (import "base.tot"))"#,
            &[("base.tot", "listen {host \"::\" port 8080}")]
        )
        .unwrap(),
        r#"{"port":8080}"#
    );
}

/// A path is *not* a build input, so unlike a `param`'s name and an `import`'s path it may be
/// computed. Selecting by parameter is the ordinary reason to want one.
#[test]
fn a_get_path_can_be_computed() {
    assert_eq!(
        build(
            r#"a (get (param "env") {prod {n 3} dev {n 1}})"#,
            &[("env", string("prod"))]
        )
        .unwrap(),
        r#"{"a":{"n":3}}"#
    );
    assert_eq!(
        build(
            r#"a (get (str (param "env") ".n") {prod {n 3}})"#,
            &[("env", string("prod"))]
        )
        .unwrap(),
        r#"{"a":3}"#
    );
}

/// The default is the only way to reach a member that may not be there: `if` needs a boolean,
/// and there is no `has`.
#[test]
fn get_takes_a_default_only_on_a_miss() {
    assert_eq!(
        plain(r#"a (get "listen.port" {listen {}} 8080)"#).unwrap(),
        r#"{"a":8080}"#
    );
    // Evaluated only when it is needed, the way a `param`'s default is. This one cannot be
    // built, so a hit that evaluated it would fail rather than answer.
    assert_eq!(
        plain(r#"a (get "n" {n 1} (param "not-set"))"#).unwrap(),
        r#"{"a":1}"#
    );
    // A default reaches a missing branch as well as a missing leaf.
    assert_eq!(
        plain(r#"a (get "x.y.z" {} null)"#).unwrap(),
        r#"{"a":null}"#
    );
}

/// A default answers "there is nothing there" and only that.
///
/// A step that ran into the wrong *kind* of value is the imported document being shaped
/// differently than this template expects — a template bug, and the one thing a build is for.
/// Defaulting past it reports success and writes the fallback.
#[test]
fn a_get_default_does_not_hide_a_shape_mismatch() {
    // The two ways a document can simply not answer: no such member, and past the end.
    assert_eq!(
        plain(r#"a (get "listen.port" {listen {}} 8080)"#).unwrap(),
        r#"{"a":8080}"#
    );
    assert_eq!(
        plain(r#"a (get "xs[9]" {xs [1]} 0)"#).unwrap(),
        r#"{"a":0}"#
    );

    // Neither of these is a miss, so neither takes the default.
    for (src, expected) in [
        (
            r#"a (get "listen.port" {listen 5} 8080)"#,
            "cannot look up `port`: `listen` is an integer, not an object",
        ),
        (
            r#"a (get "xs[0]" {xs {a 1}} 0)"#,
            "cannot take element 0 of `xs`: it is an object, not an array",
        ),
    ] {
        let message = fails(src);
        assert!(message.contains(expected), "`{src}`: {message}");
    }
}

/// Without a default a miss is a build failure, carrying the diagnostic `tot get` gives —
/// which names what *was* there, spelled the way a path spells it.
#[test]
fn a_get_miss_says_what_was_there() {
    let message = fails(r#"a (get "listen.prot" {listen {host "::" port 80}})"#);
    assert!(
        message.contains("no member `prot` in `listen`"),
        "{message}"
    );
    assert!(message.contains("members are host, port"), "{message}");
    // The caret goes on the argument that asked, since the miss's own span indexes the path.
    assert!(message.contains(r#"^^^^^^^^^^^^^"#), "{message}");

    for (src, expected) in [
        (
            r#"a (get "xs[9]" {xs [1]})"#,
            "index 9 is out of range: `xs` has 1 element",
        ),
        (
            r#"a (get "a.b" {a 1})"#,
            "cannot look up `b`: `a` is an integer, not an object",
        ),
        (
            r#"a (get 1 {a 1})"#,
            "a path is a string, but this is an integer",
        ),
        (
            r#"a (get "a..b" {a 1})"#,
            "`a..b` is not a path: expected a member name",
        ),
    ] {
        let message = fails(src);
        assert!(message.contains(expected), "`{src}`: {message}");
    }
}

// --- map and it -------------------------------------------------------------------------------

#[test]
fn map_evaluates_its_body_once_per_element() {
    assert_eq!(
        plain(r#"a (map (str "x-" (it)) ["p" "q"])"#).unwrap(),
        r#"{"a":["x-p","x-q"]}"#
    );
    // The body is a value like any other, so it can be a collection — which is what turns a
    // list of names into a list of objects.
    assert_eq!(
        plain(r#"a (map {name (it) up true} ["p" "q"])"#).unwrap(),
        r#"{"a":[{"name":"p","up":true},{"name":"q","up":true}]}"#
    );
    // `(it)` is the whole element, so it needs no unwrapping to be one.
    assert_eq!(
        plain(r#"a (map (it) [1 "two" null])"#).unwrap(),
        r#"{"a":[1,"two",null]}"#
    );
}

/// The two new forms are most of the point together: `map` walks a list of objects and `get`
/// reaches into each one.
#[test]
fn map_and_get_compose() {
    assert_eq!(
        plain(r#"ports (map (get "port" (it)) [{port 80} {port 443}])"#).unwrap(),
        r#"{"ports":[80,443]}"#
    );
    assert_eq!(
        build_with(
            r#"hosts (map (str (it) ".example.net") (import "regions.tot"))"#,
            &[("regions.tot", r#"["eu" "us"]"#)]
        )
        .unwrap(),
        r#"{"hosts":["eu.example.net","us.example.net"]}"#
    );
}

#[test]
fn map_over_nothing_evaluates_nothing() {
    // The body would fail if it were reached, which is how this says it was not.
    assert_eq!(
        plain(r#"a (map (param "not-set") [])"#).unwrap(),
        r#"{"a":[]}"#
    );
}

#[test]
fn map_needs_an_array() {
    let message = fails(r#"a (map (it) {b 1})"#);
    assert!(
        message.contains("`map` works over an array, but this is an object"),
        "{message}"
    );
    assert!(fails(r#"a (map (it) "xs")"#).contains("but this is a string"));
}

/// `(it)` is refused where there is no element, and this is a *parse* error — so `tot check`
/// catches it without anything being built.
#[test]
fn it_outside_a_map_body_is_refused() {
    for src in [
        "a (it)",
        r#"a (str "x" (it))"#,
        // The list argument is evaluated in the scope the `map` itself sits in, not in its body.
        r#"a (map (it) [(it)])"#,
    ] {
        let error = Template::parse(src).expect_err("no element here");
        assert!(
            error
                .message
                .contains("`it` is only meaningful inside a `(map …)`"),
            "`{src}`: {error}"
        );
    }
}

/// A `map` may not appear inside another `map`'s body, which is what keeps `(it)` free of a
/// shadowing rule: it names the element of exactly one `map`, and there is only ever one.
#[test]
fn a_map_may_not_nest_in_a_body_but_may_in_the_list() {
    let error = Template::parse(r#"a (map (map (it) [1]) [2])"#).expect_err("ambiguous");
    assert!(
        error
            .message
            .contains("a `map` cannot appear inside another `map`'s body"),
        "{error}"
    );
    // It hides no better inside a collection.
    assert!(Template::parse(r#"a (map {b (map (it) [1])} [2])"#).is_err());

    // In the list argument there is no ambiguity: that `map` finishes before the body runs.
    assert_eq!(
        plain(r#"a (map (str "<" (it) ">") (map (str "x" (it)) ["p" "q"]))"#).unwrap(),
        r#"{"a":["<xp>","<xq>"]}"#
    );
}

/// An import is a wall. An imported file is parsed on its own, so `(it)` in one is an error
/// rather than a reach into whatever imported it — which is also what keeps the build cache
/// sound, since a file's value cannot depend on the body it was imported inside.
#[test]
fn it_does_not_cross_an_import() {
    let message = build_with(
        r#"a (map (import "frag.tott") ["p" "q"])"#,
        &[("frag.tott", "(it)")],
    )
    .expect_err("no element in frag.tott");
    assert!(
        message.contains("`it` is only meaningful inside a `(map …)`"),
        "{message}"
    );
    assert!(message.contains("in frag.tott"), "{message}");

    // A `map` of its own is fine there, and reads its own element.
    assert_eq!(
        build_with(
            r#"a (map (import "frag.tott") ["p" "q"])"#,
            &[("frag.tott", r#"(map (str "y" (it)) ["1" "2"])"#)],
        )
        .unwrap(),
        r#"{"a":[["y1","y2"],["y1","y2"]]}"#
    );
}

#[test]
fn map_and_get_arities_are_checked() {
    for (src, expected) in [
        (
            r#"a (map (it))"#,
            "`map` takes 2 arguments, but was given 1",
        ),
        (
            r#"a (map (it) [1] [2])"#,
            "`map` takes 2 arguments, but was given 3",
        ),
        (
            r#"a (get "b")"#,
            "`get` takes 2 or 3 arguments, but was given 1",
        ),
        (
            r#"a (get "b" {} 1 2)"#,
            "`get` takes 2 or 3 arguments, but was given 4",
        ),
        (
            r#"a (map (it 1) [2])"#,
            "`it` takes no arguments, but was given 1",
        ),
    ] {
        let message = fails(src);
        assert!(message.contains(expected), "`{src}`: {message}");
    }
}
