//! Formatter tests.
//!
//! Every fixture goes through [`f`], which checks the two properties a formatter owes you —
//! the value is unchanged, and formatting again is a no-op — before returning the output.

use tot::{format, parse};

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

#[test]
fn invalid_documents_are_refused() {
    assert!(format("kind curly").is_err());
    assert!(format("a 1 a 2").is_err());
}
