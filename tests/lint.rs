//! Lint tests. Everything here is legal tot — the question is only whether `--strict` should
//! say something about it.

use tot::lint;

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
