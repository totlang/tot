//! Lint tests. Everything here is legal tot — the question is only whether `--strict` should
//! say something about it.

use tot::{lint, lint_template};

/// The keys the lint complained about, in order.
fn flagged(src: &str) -> Vec<String> {
    lint(src)
        .unwrap_or_else(|e| panic!("expected a valid document:\n{}", e.render(src)))
        .iter()
        .map(|warning| warning.message.clone())
        .collect()
}

#[test]
fn a_member_split_across_lines_is_flagged() {
    let warnings = flagged("timeout\n30\n");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("`timeout`"), "{warnings:?}");
}

#[test]
fn members_on_one_line_are_fine() {
    assert!(flagged("timeout 30\nretries 3\n").is_empty());
    assert!(flagged("a 1 b 2").is_empty());
    assert!(flagged("{a 1 b 2}").is_empty());
    assert!(flagged("").is_empty());
}

/// A value only has to *start* on the key's line. Blocks are free to run on — that is the
/// whole shape of the language.
#[test]
fn a_value_may_run_past_the_key_line() {
    assert!(flagged("listen {\n  port 8080\n}\n").is_empty());
    assert!(flagged("tags [\n  \"a\"\n  \"b\"\n]\n").is_empty());
    assert!(flagged("motd \"\"\"\n  hello\n  \"\"\"\n").is_empty());
}

#[test]
fn nested_members_are_checked_too() {
    let warnings = flagged("outer {\n  inner\n  1\n}\n");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("`inner`"), "{warnings:?}");

    // Inside an array of objects as well.
    let warnings = flagged("routes [\n  {\n    path\n    \"/x\"\n  }\n]\n");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("`path`"), "{warnings:?}");
}

#[test]
fn every_split_member_is_reported_not_just_the_first() {
    assert_eq!(flagged("a\n1\nb\n2\n").len(), 2);
}

/// A comment between a key and its value puts them on different lines, which is the same
/// shape and the same hazard.
#[test]
fn a_comment_between_key_and_value_counts_as_a_split() {
    assert_eq!(flagged("a # why\n1\n").len(), 1);
}

/// Array elements have no key, so there is no pairing to shift and nothing to warn about.
#[test]
fn array_elements_are_not_members() {
    assert!(flagged("[\n  1\n  2\n  3\n]\n").is_empty());
}

#[test]
fn a_value_root_is_walked() {
    assert_eq!(flagged("{\n  a\n  1\n}\n").len(), 1);
}

#[test]
fn lint_reports_a_parse_error_rather_than_warnings() {
    let error = lint("kind curly").expect_err("should not parse");
    assert!(error.message.contains("string values must be quoted"));
}

// --- templates ------------------------------------------------------------------------------

/// The keys the lint complained about in a template.
fn flagged_template(src: &str) -> Vec<String> {
    lint_template(src)
        .unwrap_or_else(|e| panic!("expected a valid template:\n{}", e.render(src)))
        .iter()
        .map(|warning| warning.message.clone())
        .collect()
}

/// The hazard is the language's, not the document's: a template has no separator between
/// members either, and a form is one more kind of value that can land on the wrong line.
#[test]
fn a_template_member_split_across_lines_is_flagged() {
    let warnings = flagged_template("image\n(str \"x\")\n");
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("`image`"), "{warnings:?}");

    assert!(flagged_template(r#"image (str "x")"#).is_empty());
}

/// A form may run past its key's line, the same as a `{`, `[`, or `"""` — only the start has
/// to sit beside the key.
#[test]
fn a_form_may_run_past_its_keys_line() {
    assert!(flagged_template("image (str\n  \"x\"\n)\n").is_empty());
}

/// An argument has no key, so no argument is ever warned about. A member *inside* one is a
/// member like any other.
#[test]
fn the_rule_reaches_inside_a_form() {
    assert!(flagged_template("a (param \"p\"\n  1)\n").is_empty());

    let warnings = flagged_template("a (param \"p\" {inner\n1})\n");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("`inner`"), "{warnings:?}");
}

/// Reading a template validates its forms, so the lint refuses one that is not a template at
/// all rather than reporting warnings about it.
#[test]
fn linting_a_template_reports_a_bad_form_rather_than_warnings() {
    let error = lint_template("a (nope)").expect_err("not a form");
    assert!(error.message.contains("`nope` is not a form"), "{error}");

    // And the two dialects stay apart: parens are data in one and a form in the other.
    assert!(lint("(a) 1").is_ok());
    assert!(lint_template("(a) 1").is_err());
}

#[test]
fn warnings_render_like_errors() {
    let src = "outer {\n  inner\n  1\n}\n";
    let warnings = lint(src).unwrap();
    let rendered = warnings[0].render(src);

    assert!(rendered.starts_with("warning: "), "{rendered}");
    assert!(rendered.contains("2:3"), "{rendered}");
    assert!(rendered.contains("^^^^^"), "{rendered}");
    assert!(rendered.contains("help:"), "{rendered}");
    assert_eq!(warnings[0].line_col(src), (2, 3));
}
