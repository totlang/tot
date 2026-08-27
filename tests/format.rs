//! Formatter tests.
//!
//! Every fixture goes through [`f`], which checks the two properties a formatter owes you —
//! the value is unchanged, and formatting again is a no-op — before returning the output.

use tot::template::{Params, Template};
use tot::{Map, Value, format, format_template, format_value, parse};

fn f(src: &str) -> String {
    let once = format(src).unwrap_or_else(|e| panic!("{}", e.render(src)));

    assert_eq!(
        parse(&once).unwrap(),
        parse(src).unwrap(),
        "formatting changed the value\n--- got ---\n{once}"
    );

    let twice = format(&once).unwrap();
    assert_eq!(
        twice, once,
        "formatting is not idempotent\n--- once ---\n{once}\n--- twice ---\n{twice}"
    );

    once
}

#[test]
fn punctuation_goes_away_and_keys_are_unquoted() {
    assert_eq!(f(r#"{"a": 1, "b": 2}"#), "{a 1 b 2}\n");
    assert_eq!(f("a:1,b:2"), "a 1\nb 2\n");
}

#[test]
fn keys_stay_quoted_only_where_they_must() {
    assert_eq!(
        f(r#""address" 1 "favorite food" 2 "a=b" 3 "" 4"#),
        "address 1\n\"favorite food\" 2\n\"a=b\" 3\n\"\" 4\n"
    );
}

#[test]
fn block_form_is_indented_two_spaces() {
    assert_eq!(
        f("a{\nb 1\nc {\nd 2\n}\n}"),
        "a {\n  b 1\n  c {\n    d 2\n  }\n}\n"
    );
}

/// Inline versus block is the author's choice; the formatter normalizes spacing but never
/// reflows.
#[test]
fn inline_collections_stay_inline() {
    assert_eq!(f("a [1  2   3]"), "a [1 2 3]\n");
    assert_eq!(f("a {b 1 c [2 3]}"), "a {b 1 c [2 3]}\n");
    assert_eq!(f("a {} b []"), "a {}\nb []\n");
    // An empty collection collapses even if it was written open.
    assert_eq!(f("a {\n}"), "a {}\n");
}

#[test]
fn block_collections_stay_block() {
    assert_eq!(f("a [\n1 2\n]"), "a [\n  1\n  2\n]\n");
}

/// The brace-less root has no brackets to hold an inline form, so it always breaks.
#[test]
fn top_level_members_are_one_per_line() {
    assert_eq!(f("a 1 b 2"), "a 1\nb 2\n");
    assert_eq!(f("{a 1 b 2}"), "{a 1 b 2}\n");
}

#[test]
fn value_roots_round_trip() {
    assert_eq!(f("[1,2,3]"), "[1 2 3]\n");
    assert_eq!(f(r#""just a string""#), "\"just a string\"\n");
    assert_eq!(f("42"), "42\n");
    assert_eq!(f(""), "");
}

// --- comments and blank lines -------------------------------------------------------------

#[test]
fn comments_keep_their_position() {
    let src = "\
# header

a 1 # about a

# about b
b {
  c 2
  # dangling
}
";
    assert_eq!(f(src), src);
}

/// A value root has no member to hang a trailing comment on, so the document itself keeps it.
#[test]
fn a_trailing_comment_on_a_value_root_survives() {
    assert_eq!(f("[1 2] # note"), "[1 2] # note\n");
    assert_eq!(f("{a 1} # note"), "{a 1} # note\n");
    assert_eq!(f("42 # note"), "42 # note\n");
    assert_eq!(f("[\n  1\n] # tail"), "[\n  1\n] # tail\n");
    assert_eq!(f("# lead\n[1 2] # tail\n"), "# lead\n[1 2] # tail\n");
}

#[test]
fn a_document_of_only_comments_survives() {
    assert_eq!(f("# just a note\n"), "# just a note\n");
    assert_eq!(f("\n\n# note\n\n\n"), "# note\n");
}

#[test]
fn blank_lines_collapse_to_one() {
    assert_eq!(f("a 1\n\n\n\nb 2\n"), "a 1\n\nb 2\n");
}

#[test]
fn leading_and_trailing_blank_lines_are_dropped() {
    assert_eq!(f("\n\n\na 1\n\n\n"), "a 1\n");
    assert_eq!(f("a {\n\n  b 1\n\n}"), "a {\n  b 1\n}\n");
}

#[test]
fn comments_are_reindented_with_their_block() {
    assert_eq!(f("a {\n# note\nb 1\n}"), "a {\n  # note\n  b 1\n}\n");
}

/// A comment between a key and its value has no natural home, so it moves above the member
/// rather than being dropped.
#[test]
fn a_comment_between_key_and_value_moves_up() {
    assert_eq!(f("a # why\n1\n"), "# why\na 1\n");
}

#[test]
fn comments_inside_arrays_survive() {
    assert_eq!(
        f("a [\n1 # one\n# about two\n2\n]"),
        "a [\n  1 # one\n  # about two\n  2\n]\n"
    );
}

// --- multi-line strings -------------------------------------------------------------------

#[test]
fn multiline_strings_are_reindented_with_their_block() {
    assert_eq!(
        f("a {\nmotd \"\"\"\nhello\n  indented\n\"\"\"\n}"),
        "a {\n  motd \"\"\"\n    hello\n      indented\n    \"\"\"\n}\n"
    );
}

#[test]
fn multiline_reindentation_preserves_escapes_and_blank_lines() {
    let src = "a \"\"\"\n    one\\tescaped\n\n    two \\\n    three\n    \"\"\"";
    let out = f(src);
    assert!(out.contains("one\\tescaped"), "{out}");
    // The blank line comes back with no trailing whitespace.
    assert!(out.contains("\n\n"), "{out}");
    assert_eq!(
        parse(&out).unwrap().get("a").unwrap().as_str(),
        Some("one\tescaped\n\ntwo three")
    );
}

#[test]
fn multiline_strings_normalize_crlf() {
    let out = f("a \"\"\"\r\n  one\r\n  two\r\n  \"\"\"");
    assert!(!out.contains('\r'), "{out:?}");
}

// --- whole documents ----------------------------------------------------------------------

#[test]
fn the_spec_example_is_already_canonical() {
    let src = "\
my-name \"tim\"

address {
  street \"100 main st\"
  zip 123456
  country \"united states\"
}

\"favorite food\" [
  \"tacos\"
  {
    name \"fries\"
    kind \"curly\"
    rating 10
  }
]
";
    assert_eq!(f(src), src);
}

#[test]
fn a_thoroughly_ugly_document_comes_out_clean() {
    let src = "  #  leading note\n\n\n\"a\":1,  \"b\":{  \"c\":[1,2,{\"d\":true},],\n\"e\":null }  # tail\n\n";
    assert_eq!(
        f(src),
        // The blank lines between the header comment and the body are kept, collapsed to one.
        "#  leading note\n\na 1\nb {\n  c [1 2 {d true}]\n  e null\n} # tail\n"
    );
}

/// The formatter pulls a value back up onto its key's line — which is exactly the shape
/// `check --strict` reports, so `tot fmt` repairs what the lint finds. That is worth pinning:
/// it is a relationship between two features, and nothing else would catch it breaking.
#[test]
fn formatting_joins_a_member_back_onto_one_line() {
    assert_eq!(f("timeout\n30\n"), "timeout 30\n");
    assert_eq!(f("a\n1\nb\n2\n"), "a 1\nb 2\n");
    assert_eq!(f("listen\n{port 8080}\n"), "listen {port 8080}\n");
    // A comment between the two has no home on that line, so it moves above the member.
    assert_eq!(f("a # why\n1\n"), "# why\na 1\n");

    for src in ["timeout\n30\n", "a\n1\nb\n2\n", "a # why\n1\n"] {
        let formatted = f(src);
        assert!(
            tot::lint(&formatted).unwrap().is_empty(),
            "`{src}` should be clean after formatting, got `{formatted}`"
        );
    }
}

// --- format_value: emitting a Value with no source to preserve -----------------------------

/// `format_value(&{k: s})`, for testing how one string is written.
fn emit(s: &str) -> String {
    let mut map = Map::new();
    map.insert("k".to_string(), Value::String(s.to_string()));
    format_value(&Value::Object(map))
}

#[test]
fn strings_become_blocks_only_when_they_have_line_breaks() {
    assert_eq!(emit("one\ntwo"), "k \"\"\"\n  one\n  two\n  \"\"\"\n");
    assert_eq!(emit("one line"), "k \"one line\"\n");
    assert_eq!(emit(""), "k \"\"\n");
    // A line ending in whitespace rules the whole string out: the reader would blank a
    // whitespace-only line, and an editor would strip the rest.
    assert_eq!(emit("one \ntwo"), "k \"one \\ntwo\"\n");
    assert_eq!(emit("   \ntwo"), "k \"   \\ntwo\"\n");
}

/// The property that makes the block form safe to choose automatically: whatever is emitted
/// must read back byte-for-byte, and must already be canonical.
#[test]
fn block_strings_survive_the_round_trip() {
    let cases = [
        "one\ntwo",
        "one\n\ntwo",
        "trailing newline\n",
        "two trailing newlines\n\n",
        "\nleading newline",
        "\n",
        "\n\n",
        "has \"quotes\" inside\nand more",
        "\"\"\"\nopens with a triple quote",
        "  \"\"\" indented triple quote\nnext",
        "ends with a backslash \\\nnext line",
        "tab\there\nand\tthere",
        "carriage \r return\nnext",
        "bell \u{7} here\nnext",
        "#not a comment\nnext",
        "}\n]\n{",
        "set -e\nif [ -f \"$1\" ]; then\n  echo \"yes\"\nfi",
        // These fall back to a quoted literal.
        "trailing space \nnext",
        "   \nwhitespace-only line",
        "no newline at all",
        "",
    ];

    for case in cases {
        let emitted = emit(case);
        let reparsed = parse(&emitted).unwrap_or_else(|e| {
            panic!(
                "{case:?} did not re-parse\n{emitted}\n{}",
                e.render(&emitted)
            )
        });
        assert_eq!(
            reparsed.get("k").and_then(Value::as_str),
            Some(case),
            "value changed\n--- emitted ---\n{emitted}"
        );
        assert_eq!(
            format(&emitted).unwrap(),
            emitted,
            "not canonical\n--- emitted ---\n{emitted}"
        );
    }
}

#[test]
fn block_strings_are_indented_with_their_member() {
    let mut inner = Map::new();
    inner.insert(
        "motd".to_string(),
        Value::String("hello\nworld".to_string()),
    );
    let mut outer = Map::new();
    outer.insert("service".to_string(), Value::Object(inner));

    let emitted = format_value(&Value::Object(outer));
    assert_eq!(
        emitted,
        "service {\n  motd \"\"\"\n    hello\n    world\n    \"\"\"\n}\n"
    );
    assert_eq!(format(&emitted).unwrap(), emitted);
}

#[test]
fn invalid_documents_are_refused() {
    assert!(format("kind curly").is_err());
    assert!(format("a 1 a 2").is_err());
}

// --- templates ------------------------------------------------------------------------------

/// The template fixture check, owing the same two properties `f` does — except that a template
/// denotes a value only once it is built, so that is what gets compared.
fn t(src: &str) -> String {
    let build = |text: &str| {
        Template::parse(text)
            .unwrap_or_else(|e| panic!("{}", e.render(text)))
            .evaluate(&Params::new())
            .map(|value| tot::json::to_string(&value))
    };

    let once = format_template(src).unwrap_or_else(|e| panic!("{}", e.render(src)));

    assert_eq!(
        build(&once).ok(),
        build(src).ok(),
        "formatting changed what the template builds\n--- got ---\n{once}"
    );

    let twice = format_template(&once).unwrap();
    assert_eq!(
        twice, once,
        "formatting is not idempotent\n--- once ---\n{once}\n--- twice ---\n{twice}"
    );

    once
}

/// A form is bracketed like a collection, so it gets the same rule: spacing is normalized and
/// nothing is reflowed.
#[test]
fn inline_forms_stay_inline() {
    assert_eq!(t(r#"a (str   "x"    "y")"#), "a (str \"x\" \"y\")\n");
    assert_eq!(t(r#"a (param  "n"  1)"#), "a (param \"n\" 1)\n");
    assert_eq!(t("a (str)"), "a (str)\n");
    // Nested forms, and a form inside a collection.
    assert_eq!(
        t(r#"a (if (param "p" false) (str "y") "n")"#),
        "a (if (param \"p\" false) (str \"y\") \"n\")\n"
    );
    assert_eq!(t(r#"xs [1 (param "n" 2) 3]"#), "xs [1 (param \"n\" 2) 3]\n");
}

/// The closing paren lands on its own line at the form's indent, the way `}` and `]` do. A
/// form is not special: it is one more bracketed shape.
#[test]
fn block_forms_stay_block() {
    assert_eq!(
        t("a (str\n\"one\"\n\"two\")"),
        "a (str\n  \"one\"\n  \"two\"\n)\n"
    );
    // The head stays on the opening line — alone it would read as an argument.
    assert!(t("a (str\n\"one\")").starts_with("a (str\n"));
    // Nested, so indentation compounds the same way.
    assert_eq!(
        t("a (str\n(param \"n\")\n(str\n\"x\")\n)"),
        "a (str\n  (param \"n\")\n  (str\n    \"x\"\n  )\n)\n"
    );
}

/// A comment forces block form and is never dropped, including one written between `(` and the
/// head, where it has no home of its own.
#[test]
fn comments_inside_a_form_survive() {
    assert_eq!(
        t("a (str \"x\" # why\n\"y\")"),
        "a (str\n  \"x\" # why\n  \"y\"\n)\n"
    );
    assert_eq!(
        t("a ( # stray\nstr \"x\")"),
        "a (str\n  # stray\n  \"x\"\n)\n"
    );
    assert_eq!(
        t("a (str\n# above\n\"x\"\n)"),
        "a (str\n  # above\n  \"x\"\n)\n"
    );
}

/// The trap the dialect exists to close: unquoting a key that holds parens would turn it into
/// a form. The same key is bare in a document and quoted in a template.
#[test]
fn a_key_holding_parens_stays_quoted_in_a_template() {
    assert_eq!(t(r#""(a)" 1"#), "\"(a)\" 1\n");
    assert_eq!(f(r#""(a)" 1"#), "(a) 1\n");

    // `@` and `$` are reserved in neither, which is what choosing parens bought.
    assert_eq!(t(r#""@type" 1 "$ref" 2"#), "@type 1\n$ref 2\n");
}

/// Everything the data formatter does, a template formatter still does — it is the same
/// formatter with one more shape.
#[test]
fn a_template_is_still_formatted_like_a_document() {
    assert_eq!(t(r#"{"a": 1, "b": [2, 3]}"#), "{a 1 b [2 3]}\n");
    assert_eq!(t("a{\nb 1\n}"), "a {\n  b 1\n}\n");
    assert_eq!(
        t("motd \"\"\"\n hello\n \"\"\""),
        "motd \"\"\"\n  hello\n  \"\"\"\n"
    );
    assert_eq!(t("# a comment\n\n\na 1"), "# a comment\n\na 1\n");
}

/// A form is a value, so a template whose whole content is one is that form.
#[test]
fn a_form_can_be_the_whole_document() {
    assert_eq!(t(r#"(param "whole" 1)"#), "(param \"whole\" 1)\n");
}

#[test]
fn invalid_templates_are_refused() {
    assert!(format_template("a (nope)").is_err());
    assert!(format_template("a ()").is_err());
    assert!(format_template("a (str").is_err());
    assert!(format_template("kind curly").is_err());
    // And a document is not read as a template unless it is asked for: here the parens are
    // data, so `format` takes it and `format_template` does not.
    assert!(format("(a) 1").is_ok());
    assert!(format_template("(a) 1").is_err());
}
