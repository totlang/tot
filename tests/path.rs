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

/// `find` tells *nothing there* apart from *the wrong shape*, which `get` deliberately does
/// not: a caller with a fallback should use it for the first and never for the second.
#[test]
fn find_tells_absent_from_the_wrong_shape() {
    let document = parse(DOC);
    let find = |text: &str| Path::parse(text).unwrap().find(&document);

    // There.
    assert!(find("listen.port").unwrap().is_some());
    // Not there: a member the object does not have, and an index past the end.
    assert_eq!(find("listen.tls").unwrap(), None);
    assert_eq!(find("regions[9]").unwrap(), None);
    // Not a miss: the step ran into the wrong kind of value, which is the path being wrong
    // about the document rather than the document being short of something.
    assert!(find("listen.port.deeper").is_err());
    assert!(find("listen[0]").is_err());

    // Both still fail through `get`, which is what reports a miss with its own diagnostic.
    for text in [
        "listen.tls",
        "regions[9]",
        "listen.port.deeper",
        "listen[0]",
    ] {
        assert!(Path::parse(text).unwrap().get(&document).is_err(), "{text}");
    }
}

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

// --- writing ----------------------------------------------------------------------------------

/// Sets `path` to `value` in `DOC` and renders the result as compact JSON.
fn set_in(src: &str, path: &str, value: &str, missing: tot::Missing) -> Result<String, String> {
    let mut document = parse(src);
    let path = Path::parse(path).map_err(|e| e.to_string())?;
    path.set(&mut document, parse(value), missing)
        .map_err(|e| e.to_string())?;
    Ok(tot::json::to_string(&document))
}

fn set(path: &str, value: &str) -> Result<String, String> {
    set_in(DOC, path, value, tot::Missing::Reject)
}

#[test]
fn setting_replaces_a_value_in_place() {
    let out = set("listen.port", "9090").unwrap();
    assert!(
        out.contains(r#""listen":{"host":"0.0.0.0","port":9090}"#),
        "{out}"
    );
    assert_eq!(
        set_in("a 1", "a", r#""x""#, tot::Missing::Reject).unwrap(),
        r#"{"a":"x"}"#
    );
}

/// Adding a member is the point of setting, so the last step may be new. It lands at the end,
/// the way any new member does.
#[test]
fn the_last_step_may_be_new() {
    let out = set("listen.tls", "true").unwrap();
    assert!(out.contains(r#""port":8080,"tls":true"#), "{out}");
    assert_eq!(
        set_in("", "a", "1", tot::Missing::Reject).unwrap(),
        r#"{"a":1}"#
    );
}

/// Replacing a member must not move it: order is part of the document.
#[test]
fn setting_keeps_a_member_where_it_was() {
    assert_eq!(
        set_in("a 1 b 2 c 3", "b", "9", tot::Missing::Reject).unwrap(),
        r#"{"a":1,"b":9,"c":3}"#
    );
}

#[test]
fn an_element_can_be_set_but_never_added() {
    let out = set("regions[0]", r#""eu-west-1""#).unwrap();
    assert!(
        out.contains(r#""regions":["eu-west-1","eu-central-1"]"#),
        "{out}"
    );
    assert!(
        set("regions[2]", r#""x""#)
            .unwrap_err()
            .contains("out of range")
    );
    assert!(set("routes[0].path", r#""/z""#).is_ok());
}

/// A path that is not there is an error by default: a mistyped path is far likelier than a
/// genuinely missing branch, and a silent success hides the typo.
#[test]
fn a_missing_step_before_the_last_is_rejected() {
    let e = set("listen.tls.enabled", "true").unwrap_err();
    assert!(e.contains("no member `tls` in `listen`"), "{e}");
    assert!(
        set("nope.deeper", "1")
            .unwrap_err()
            .contains("in the document")
    );
}

#[test]
fn create_builds_the_objects_on_the_way() {
    let out = set_in("a 1", "b.c.d", "true", tot::Missing::Create).unwrap();
    assert_eq!(out, r#"{"a":1,"b":{"c":{"d":true}}}"#);

    // What is already there is left alone.
    let out = set_in("a {x 1}", "a.y", "2", tot::Missing::Create).unwrap();
    assert_eq!(out, r#"{"a":{"x":1,"y":2}}"#);
}

/// `Create` fills in what is missing. It never replaces what is present, because that would
/// throw away a value nobody asked to lose.
#[test]
fn create_does_not_overwrite_a_value_of_the_wrong_kind() {
    let e = set_in("a \"scalar\"", "a.b", "1", tot::Missing::Create).unwrap_err();
    assert!(e.contains("`a` is a string, not an object"), "{e}");

    // Nor does it invent array elements, under either setting.
    let e = set_in("a []", "a[0].b", "1", tot::Missing::Create).unwrap_err();
    assert!(e.contains("out of range"), "{e}");
}

#[test]
fn setting_reports_a_type_error_the_way_reading_does() {
    let e = set("name.first", "1").unwrap_err();
    assert!(
        e.contains("cannot look up `first`: `name` is a string, not an object"),
        "{e}"
    );
    assert!(set("listen[0]", "1").unwrap_err().contains("not an array"));
}

/// `get_mut` is the plain dual of `get` — everything has to be there already.
#[test]
fn get_mut_reaches_an_existing_value() {
    let mut document = parse(DOC);
    let path = Path::parse("listen.port").unwrap();
    *path.get_mut(&mut document).unwrap() = tot::Value::Bool(false);
    assert_eq!(path.get(&document).unwrap().as_bool(), Some(false));

    assert!(
        Path::parse("listen.tls")
            .unwrap()
            .get_mut(&mut document)
            .is_err()
    );
}

/// What `get` prints is what `set` takes, so the pair round-trips.
#[test]
fn get_and_set_round_trip() {
    let document = parse(DOC);
    for path in [
        "name",
        "listen",
        "listen.port",
        "regions",
        "regions[1]",
        "routes[0]",
    ] {
        let path = Path::parse(path).unwrap();
        let printed = tot::format_value(path.get(&document).unwrap());

        let mut copy = parse(DOC);
        path.set(&mut copy, parse(&printed), tot::Missing::Reject)
            .unwrap_or_else(|e| panic!("setting `{}` back: {e}", path.text()));
        assert_eq!(copy, document, "round trip through `{}`", path.text());
    }
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

/// A `set` that fails changes nothing. `Missing::Create` builds objects as it descends, so
/// without checking the whole path first a failure part way along would leave the branch it
/// had already made behind — a document altered by a call that said it had failed.
#[test]
fn a_failed_set_leaves_the_document_alone() {
    let before = tot::parse("root {}").unwrap();

    // Each of these fails somewhere after the point where `Create` would have started
    // building, which is the only way a partial write could happen.
    for (path, expected) in [
        ("a.b.c[0]", "not an array"),
        ("a.b[3].c", "not an array"),
        ("x[0]", "not an array"),
    ] {
        let mut doc = before.clone();
        let e = Path::parse(path)
            .unwrap()
            .set(&mut doc, tot::Value::Bool(true), tot::Missing::Create)
            .expect_err(path);

        assert!(e.message.contains(expected), "`{path}`: {}", e.message);
        assert_eq!(doc, before, "`{path}` left the document changed");
    }

    // The same holds without `--create`, where nothing is built in the first place.
    let mut doc = before.clone();
    assert!(
        Path::parse("a.b.c")
            .unwrap()
            .set(&mut doc, tot::Value::Bool(true), tot::Missing::Reject)
            .is_err()
    );
    assert_eq!(doc, before);
}

/// The check that makes a failed `set` a no-op must not make a good one a no-op too.
#[test]
fn a_set_that_can_succeed_still_does() {
    let mut doc = tot::parse("root {}").unwrap();
    Path::parse("a.b.c")
        .unwrap()
        .set(&mut doc, tot::Value::Bool(true), tot::Missing::Create)
        .expect("every step is creatable");
    assert_eq!(
        tot::json::to_string(&doc),
        r#"{"root":{},"a":{"b":{"c":true}}}"#
    );

    // Reaching through an array that is already there, into an object that is not.
    let mut doc = tot::parse("xs [{} {}]").unwrap();
    Path::parse("xs[1].deep.leaf")
        .unwrap()
        .set(
            &mut doc,
            tot::Value::Integer(tot::Integer::from_i64(1)),
            tot::Missing::Create,
        )
        .expect("index in range, objects creatable below it");
    assert_eq!(
        tot::json::to_string(&doc),
        r#"{"xs":[{},{"deep":{"leaf":1}}]}"#
    );
}
