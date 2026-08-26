//! Schema tests. A schema is a document shaped like the ones it describes, so most of these
//! read as a pair: the shape, and something that does or does not fit it.

use tot::Schema;

const SHAPE: &str = r#"
name    "string"
version "int"
listen {
  host  "string"
  port  "int"
  tls?  "bool"
}
regions ["string"]
labels  {* "string"}
retries "int|null"
"#;

const GOOD: &str = r#"
name "svc"
version 3
listen {host "0.0.0.0" port 8080}
regions ["us-west-2"]
labels {team "core" tier "1"}
retries null
"#;

/// The violations, rendered as `message at path`, in the order they were found.
fn check(schema: &str, document: &str) -> Vec<String> {
    Schema::parse(schema)
        .unwrap_or_else(|e| panic!("invalid schema:\n{}", e.render(schema)))
        .check(document)
        .unwrap_or_else(|e| panic!("invalid document:\n{}", e.render(document)))
        .iter()
        .map(|violation| violation.to_string())
        .collect()
}

fn none() -> Vec<String> {
    Vec::new()
}

// --- the shape holds --------------------------------------------------------------------------

/// A schema lines up with the document beside it: same keys, same shape, values replaced.
#[test]
fn a_document_that_fits_says_nothing() {
    assert_eq!(check(SHAPE, GOOD), none());
}

#[test]
fn an_optional_member_may_be_absent_or_present() {
    assert_eq!(check(SHAPE, GOOD), none());
    let with_tls = GOOD.replace("port 8080", "port 8080 tls true");
    assert_eq!(check(SHAPE, &with_tls), none());
}

// --- types ------------------------------------------------------------------------------------

#[test]
fn a_wrong_type_is_reported_with_its_path() {
    assert_eq!(
        check(r#"port "int""#, r#"port "8080""#),
        ["expected int, found a string at `port`"]
    );
    assert_eq!(
        check(r#"a {b {c "bool"}}"#, "a {b {c 1}}"),
        ["expected bool, found an integer at `a.b.c`"]
    );
}

/// The integer/float split is the language's, so a schema keeps it too.
#[test]
fn int_and_float_stay_apart() {
    assert_eq!(check(r#"a "int""#, "a 1"), none());
    assert_eq!(check(r#"a "float""#, "a 1.0"), none());
    assert_eq!(
        check(r#"a "int""#, "a 1.0"),
        ["expected int, found a float at `a`"]
    );
    assert_eq!(check(r#"a "int|float""#, "a 1.5"), none());
}

#[test]
fn a_union_accepts_any_of_its_names() {
    assert_eq!(check(r#"a "string|null""#, r#"a "x""#), none());
    assert_eq!(check(r#"a "string|null""#, "a null"), none());
    assert_eq!(
        check(r#"a "string|null""#, "a 1"),
        ["expected string or null, found an integer at `a`"]
    );
}

#[test]
fn any_accepts_anything_including_a_collection() {
    for value in ["1", r#""x""#, "null", "[1 2]", "{b 1}", "true"] {
        assert_eq!(
            check(r#"a "any""#, &format!("a {value}")),
            none(),
            "{value}"
        );
    }
}

// --- arrays and objects -------------------------------------------------------------------------

#[test]
fn every_element_of_an_array_is_checked() {
    assert_eq!(check(r#"a ["int"]"#, "a [1 2 3]"), none());
    assert_eq!(check(r#"a ["int"]"#, "a []"), none());
    assert_eq!(
        check(r#"a ["int"]"#, r#"a [1 "x" 3 true]"#),
        [
            "expected int, found a string at `a[1]`",
            "expected int, found a boolean at `a[3]`",
        ]
    );
    assert_eq!(
        check(r#"a ["int"]"#, "a 1"),
        ["expected an array, found an integer at `a`"]
    );
}

#[test]
fn an_array_of_objects_is_reached_all_the_way_down() {
    assert_eq!(
        check(
            r#"routes [{path "string" public "bool"}]"#,
            r#"routes [{path "/a" public true} {path 1 public "yes"}]"#
        ),
        [
            "expected string, found an integer at `routes[1].path`",
            "expected bool, found a string at `routes[1].public`",
        ]
    );
}

#[test]
fn a_missing_member_is_reported_against_its_object() {
    assert_eq!(
        check(
            r#"listen {host "string" port "int"}"#,
            r#"listen {host "::"}"#
        ),
        ["missing member `port` at `listen`"]
    );
    assert_eq!(check(r#"a "int""#, ""), ["missing member `a`"]);
}

// --- unknown members ------------------------------------------------------------------------------

/// Catching a typo is most of what this is for, so an undeclared member is an error by
/// default and the message names what the schema does have.
#[test]
fn an_unknown_member_is_an_error_and_names_the_alternatives() {
    let found = Schema::parse(r#"listen {host "string" port "int"}"#)
        .unwrap()
        .check(r#"listen {host "::" port 80 prot 8080}"#)
        .unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].to_string(),
        "unknown member `prot` at `listen.prot`"
    );
    assert_eq!(found[0].help.as_deref(), Some("the schema has host, port"));
}

/// A typo is two problems at once — the name that is not there and the one that is — and
/// both get said, because either alone would send you looking in the wrong place.
#[test]
fn a_typo_is_reported_from_both_sides() {
    assert_eq!(
        check(
            r#"listen {host "string" port "int"}"#,
            r#"listen {host "::" prot 8080}"#
        ),
        [
            "missing member `port` at `listen`",
            "unknown member `prot` at `listen.prot`",
        ]
    );
}

#[test]
fn a_star_member_covers_every_other_key() {
    assert_eq!(
        check(r#"labels {* "string"}"#, r#"labels {a "1" b "2"}"#),
        none()
    );
    assert_eq!(
        check(r#"labels {* "string"}"#, r#"labels {a "1" b 2}"#),
        ["expected string, found an integer at `labels.b`"]
    );

    // Declared members and a catch-all together.
    let schema = r#"listen {host "string" * "any"}"#;
    assert_eq!(
        check(schema, r#"listen {host "::" anything 1 more true}"#),
        none()
    );
    assert_eq!(
        check(schema, "listen {host 1 other 2}"),
        ["expected string, found an integer at `listen.host`"]
    );
}

/// A key needing quotes has to be spelled so the path can be used.
#[test]
fn paths_are_spelled_the_way_tot_get_spells_them() {
    let found = check(r#"a {* "int"}"#, r#"a {"log level" "x"}"#);
    assert_eq!(
        found,
        [r#"expected int, found a string at `a."log level"`"#]
    );
    assert!(tot::Path::parse(r#"a."log level""#).is_ok());
}

// --- the schema itself --------------------------------------------------------------------------

/// A bare word is never a value in tot, and a schema does not get an exception — which is
/// also what makes a schema readable beside the document it describes.
#[test]
fn a_type_has_to_be_quoted_because_a_schema_is_tot() {
    let e = Schema::parse("port int").expect_err("a bare type is not tot");
    assert!(e.message.contains("string values must be quoted"), "{e}");
    assert!(Schema::parse(r#"port "int""#).is_ok());
}

#[test]
fn a_schema_that_is_not_a_schema_is_refused() {
    for (schema, expected) in [
        (r#"a "strng""#, "is not a type"),
        ("a 1", "is a value"),
        ("a true", "is a value"),
        ("a null", "is a value"),
        (r#"a ["int" "string"]"#, "exactly one"),
        ("a []", "exactly one"),
        (r#"a {b?* "int"}"#, "is not a member name"),
        (r#"a "int|int""#, "listed twice"),
        (r#"a "int|nope""#, "is not a type"),
    ] {
        let e = Schema::parse(schema).expect_err(schema);
        assert!(e.message.contains(expected), "`{schema}`: {}", e.message);
    }
}

/// A schema error points at the key it went wrong at, the same as any other diagnostic.
#[test]
fn a_schema_error_carries_a_caret() {
    let schema = "name \"string\"\nlisten {\n  port \"intt\"\n}\n";
    let e = Schema::parse(schema).expect_err("should not compile");

    assert_eq!(e.line_col(schema), (3, 3));
    let rendered = e.render(schema);
    assert!(rendered.contains("^^^^"), "{rendered}");
    assert!(rendered.contains("`intt` is not a type"), "{rendered}");
}

/// The whole schema may be one type, because a document may be one value.
#[test]
fn a_schema_can_describe_a_root_that_is_not_an_object() {
    assert_eq!(check(r#"["int"]"#, "[1 2]"), none());
    assert_eq!(
        check(r#"["int"]"#, r#"[1 "x"]"#),
        ["expected int, found a string at `[1]`"]
    );
    assert_eq!(check(r#""string""#, r#""just a string""#), none());
    assert_eq!(
        check(r#""string""#, "42"),
        ["expected string, found an integer"]
    );
}

// --- the caret against the document ---------------------------------------------------------------

/// A violation the document has a place for gets a span, so `check` can point at it.
#[test]
fn a_violation_points_at_the_offending_key() {
    let document = "name \"svc\"\nlisten {\n  port \"8080\"\n}\n";
    let found = Schema::parse(r#"name "string"  listen {port "int"}"#)
        .unwrap()
        .check(document)
        .unwrap();

    assert_eq!(found.len(), 1);
    let rendered = found[0].render(document);
    assert!(rendered.contains("3:3"), "{rendered}");
    assert!(rendered.contains("^^^^"), "{rendered}");
}

/// A missing member has no place of its own in the text, so it is reported against the
/// object that should have had it — which is where you would go to add it.
#[test]
fn a_missing_member_points_at_the_object_that_wants_it() {
    let document = "listen {host \"::\"}\n";
    let found = Schema::parse(r#"listen {host "string" port "int"}"#)
        .unwrap()
        .check(document)
        .unwrap();

    assert_eq!(found[0].to_string(), "missing member `port` at `listen`");
    let rendered = found[0].render(document);
    assert!(rendered.contains("1:1"), "{rendered}");
    assert!(rendered.contains("^^^^^^"), "{rendered}");
}

/// At the root there is no enclosing key, so there is genuinely nothing to point at and the
/// violation renders as one line rather than a caret somewhere misleading.
#[test]
fn a_missing_member_at_the_root_has_no_caret() {
    let document = "host \"::\"\n";
    let found = Schema::parse(r#"port "int"  * "any""#)
        .unwrap()
        .check(document)
        .unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].span, None);
    assert_eq!(found[0].render(document), "error: missing member `port`\n");
}
