//! Path tests. A path is a separate little language from tot itself — most of what is worth
//! checking is where the two differ, and what a path that misses has to say for itself.

use tot::{Path, Value};

const DOC: &str = r#"
name "svc"
listen { host "0.0.0.0" port 8080 }
regions ["us-west-2" "eu-central-1"]
"com.example.level" "debug"
routes [{path "/a"} {path "/b"}]
empty {}
nothing null
"#;

fn parse(src: &str) -> Value {
    tot::parse(src).unwrap_or_else(|e| panic!("expected a valid document:\n{}", e.render(src)))
}

/// The value at `path`, as tot text — a compact stand-in for comparing `Value`s.
fn get_in(src: &str, path: &str) -> Result<String, String> {
    let document = parse(src);
    let path = Path::parse(path).map_err(|e| e.to_string())?;
    path.get(&document)
        .map(|value| tot::format_value(value).trim_end().to_string())
        .map_err(|e| e.to_string())
}

fn get(path: &str) -> Result<String, String> {
    get_in(DOC, path)
}

/// The message of a lookup or parse failure, for a path that is expected to fail.
fn fails(path: &str) -> String {
    get(path).expect_err("should not resolve")
}

// --- finding things -----------------------------------------------------------------------

#[test]
fn a_member_is_found() {
    assert_eq!(get("name").unwrap(), "\"svc\"");
    assert_eq!(get("listen.port").unwrap(), "8080");
}

#[test]
fn an_element_is_found() {
    assert_eq!(get("regions[1]").unwrap(), "\"eu-central-1\"");
    assert_eq!(get("routes[0].path").unwrap(), "\"/a\"");
    assert_eq!(get("routes[1].path").unwrap(), "\"/b\"");
}

/// A collection comes back as a document in its own right, so it can be piped back in.
#[test]
fn a_collection_comes_back_as_tot() {
    assert_eq!(get("listen").unwrap(), "host \"0.0.0.0\"\nport 8080");
    assert_eq!(get("empty").unwrap(), "{}");
}

/// `null` is a value the document has, not a value it lacks.
#[test]
fn null_is_found_rather_than_missing() {
    assert_eq!(get("nothing").unwrap(), "null");
}

#[test]
fn a_root_that_is_not_an_object_is_still_reachable() {
    assert_eq!(get_in("[10 20 30]", "[1]").unwrap(), "20");
    assert_eq!(get_in(r#"[{a 1}]"#, "[0].a").unwrap(), "1");
}

// --- where paths and documents disagree about `.` -------------------------------------------

/// The one real trap: a `.` nests in a path but is an ordinary character in a key.
#[test]
fn a_dotted_key_has_to_be_quoted() {
    assert!(fails("com.example.level").contains("no member `com`"));
    assert_eq!(get("\"com.example.level\"").unwrap(), "\"debug\"");
}

#[test]
fn a_quoted_segment_takes_the_usual_escapes() {
    let src = r#""a\"b" 1 "c\td" 2 "" 3"#;
    assert_eq!(get_in(src, r#""a\"b""#).unwrap(), "1");
    assert_eq!(get_in(src, r#""c\td""#).unwrap(), "2");
    assert_eq!(get_in(src, r#""""#).unwrap(), "3");
}

#[test]
fn a_bare_segment_takes_anything_a_bare_key_takes() {
    let src = "path/to/thing 1 my-name 2 123 3 ünïcode 4";
    for (path, want) in [("path/to/thing", "1"), ("my-name", "2"), ("123", "3")] {
        assert_eq!(get_in(src, path).unwrap(), want);
    }
    assert_eq!(get_in(src, "ünïcode").unwrap(), "4");
}

// --- when the document does not have it -----------------------------------------------------

#[test]
fn a_missing_member_names_the_ones_that_were_there() {
    let message = fails("listen.prot");
    assert!(
        message.contains("no member `prot` in `listen`"),
        "{message}"
    );
    assert!(message.contains("members are host, port"), "{message}");
}

#[test]
fn a_missing_member_at_the_root_says_so() {
    assert!(fails("nope").contains("in the document"));
}

/// A suggested name has to be typeable as a path, or it sends the reader straight back into
/// the trap the suggestion was meant to get them out of.
#[test]
fn suggested_names_are_spelled_the_way_a_path_spells_them() {
    let message =
        get_in(r#"a 1 "log level" 2 "com.example.x" 3"#, "nope").expect_err("should not resolve");
    assert!(
        message.contains(r#"members are a, "log level", "com.example.x""#),
        "{message}"
    );
}

#[test]
fn an_empty_object_says_it_has_no_members() {
    assert!(fails("empty.x").contains("no members"));
}

#[test]
fn walking_into_a_scalar_is_reported_as_a_type_error() {
    let message = fails("name.first");
    assert!(
        message.contains("cannot look up `first`: `name` is a string, not an object"),
        "{message}"
    );
    assert!(fails("listen.port.x").contains("is an integer"));
}

#[test]
fn indexing_something_that_is_not_an_array_is_a_type_error() {
    let message = fails("listen[0]");
    assert!(message.contains("not an array"), "{message}");
    assert!(message.contains("by name, not by position"), "{message}");
    assert!(fails("[0]").contains("the document"));
}

#[test]
fn an_index_past_the_end_reports_the_length() {
    let message = fails("regions[2]");
    assert!(message.contains("index 2 is out of range"), "{message}");
    assert!(message.contains("`regions` has 2 elements"), "{message}");
}

/// The span covers the segment that failed, not the whole path — that is the point of
/// keeping spans at all.
#[test]
fn the_span_covers_the_failing_segment() {
    let document = parse(DOC);
    let path = Path::parse("listen.prot").unwrap();
    let e = path.get(&document).unwrap_err();

    assert_eq!(&path.text()[e.span.start..e.span.end], "prot");
    let rendered = e.render(path.text());
    assert!(rendered.contains("       ^^^^"), "{rendered}");
}

// --- paths that are not paths ---------------------------------------------------------------

#[test]
fn malformed_paths_are_rejected() {
    for path in [
        "",      // nothing to look up
        "a.",    // trailing separator
        "a..b",  // empty segment
        "a[",    // unclosed
        "a[]",   // no index
        "a[x]",  // not a number
        "a[-1]", // no negative indices
        "a[0]b", // no separator
        "a.[0]", // an index does not follow a `.`
        "a=b",   // reserved outside a string
        "a b",   // whitespace is not a separator here
        "\"a",   // unterminated
        "a.\"b", // unterminated, later
    ] {
        assert!(
            Path::parse(path).is_err(),
            "`{path}` should not parse as a path"
        );
    }
}

#[test]
fn a_malformed_path_explains_itself() {
    assert!(Path::parse("").unwrap_err().message.contains("empty path"));
    assert!(
        Path::parse("a.")
            .unwrap_err()
            .message
            .contains("expected a member name")
    );
    assert!(
        Path::parse("a.[0]")
            .unwrap_err()
            .help
            .unwrap()
            .contains("a[0]")
    );
    assert!(
        Path::parse("a[]")
            .unwrap_err()
            .message
            .contains("expected an index")
    );
    assert!(Path::parse("a[0").unwrap_err().message.contains("unclosed"));
    assert!(
        Path::parse("a,b")
            .unwrap_err()
            .message
            .contains("unexpected `,`")
    );
}

#[test]
fn an_index_too_large_to_hold_is_an_error() {
    let e = Path::parse("a[99999999999999999999999999]").unwrap_err();
    assert!(e.message.contains("too large"), "{}", e.message);
}

/// Path parse errors point into the path, so rendering them against it lines up.
#[test]
fn a_parse_error_spans_the_path_it_was_given() {
    let text = "listen=port";
    let e = Path::parse(text).unwrap_err();
    assert_eq!(&text[e.span.start..e.span.end], "=");
    assert!(e.render(text).contains("      ^"), "{}", e.render(text));
}
