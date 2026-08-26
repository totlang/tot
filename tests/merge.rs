//! Merge tests. The interesting cases are all about what *doesn't* merge: an array, a change
//! of kind, and a null under each of the two policies.

use tot::{Nulls, Value, merge, merge_into, parse};

/// Merges a sequence of documents and renders the result as compact JSON.
fn fold(nulls: Nulls, documents: &[&str]) -> String {
    let parsed: Vec<Value> = documents
        .iter()
        .map(|src| parse(src).unwrap_or_else(|e| panic!("invalid document:\n{}", e.render(src))))
        .collect();
    tot::json::to_string(&merge(parsed, nulls))
}

fn set(documents: &[&str]) -> String {
    fold(Nulls::Set, documents)
}

fn delete(documents: &[&str]) -> String {
    fold(Nulls::Delete, documents)
}

// --- the ordinary case ----------------------------------------------------------------------

#[test]
fn later_documents_win() {
    assert_eq!(set(&["a 1 b 2", "b 3 c 4"]), r#"{"a":1,"b":3,"c":4}"#);
    assert_eq!(set(&["a 1", "a 2", "a 3"]), r#"{"a":3}"#);
}

#[test]
fn objects_merge_member_by_member() {
    assert_eq!(
        set(&[
            "listen {host \"::\" port 80} debug false",
            "listen {port 8080}",
        ]),
        r#"{"listen":{"host":"::","port":8080},"debug":false}"#
    );
}

#[test]
fn merging_reaches_all_the_way_down() {
    assert_eq!(
        set(&["a {b {c {d 1 e 2}}}", "a {b {c {e 9}}}"]),
        r#"{"a":{"b":{"c":{"d":1,"e":9}}}}"#
    );
}

/// A base member keeps its place; a new one goes on the end. Order is part of the value here,
/// so a merge that reshuffled it would be changing the document.
#[test]
fn order_follows_the_base_then_the_overlay() {
    assert_eq!(
        set(&["a 1 b 2 c 3", "c 9 z 8 a 7"]),
        r#"{"a":7,"b":2,"c":9,"z":8}"#
    );
}

#[test]
fn no_documents_is_the_empty_object_and_one_is_itself() {
    assert_eq!(set(&[]), "{}");
    assert_eq!(set(&["a 1 b [1 2]"]), r#"{"a":1,"b":[1,2]}"#);
}

// --- what does not merge ----------------------------------------------------------------------

/// The choice that matters most: appending cannot be undone by a later layer, so an overlay
/// that could only ever add would be a one-way door.
#[test]
fn arrays_replace_rather_than_appending() {
    assert_eq!(
        set(&["tags [\"a\" \"b\"]", "tags [\"c\"]"]),
        r#"{"tags":["c"]}"#
    );
    assert_eq!(set(&["tags [\"a\"]", "tags []"]), r#"{"tags":[]}"#);
    // Objects inside an array are not reached either — the array is one value.
    assert_eq!(set(&["r [{a 1 b 2}]", "r [{a 9}]"]), r#"{"r":[{"a":9}]}"#);
}

#[test]
fn a_change_of_kind_replaces() {
    assert_eq!(set(&["a {b 1}", "a \"scalar\""]), r#"{"a":"scalar"}"#);
    assert_eq!(set(&["a \"scalar\"", "a {b 1}"]), r#"{"a":{"b":1}}"#);
    assert_eq!(set(&["a {b 1}", "a [1]"]), r#"{"a":[1]}"#);
    assert_eq!(set(&["a 1", "a 1.0"]), r#"{"a":1.0}"#);
}

/// Only two objects have members to reconcile, and that holds at the root as well.
#[test]
fn a_root_that_is_not_an_object_replaces() {
    assert_eq!(set(&["[1 2]", "[3]"]), "[3]");
    assert_eq!(set(&["a 1", "[1]"]), "[1]");
    assert_eq!(set(&["[1]", "a 1"]), r#"{"a":1}"#);
    assert_eq!(set(&["a 1", "\"just a string\""]), r#""just a string""#);
}

// --- nulls ------------------------------------------------------------------------------------

#[test]
fn a_null_sets_by_default() {
    assert_eq!(set(&["a 1 b 2", "a null"]), r#"{"a":null,"b":2}"#);
    assert_eq!(set(&["a {b 1}", "a null"]), r#"{"a":null}"#);
}

#[test]
fn a_null_deletes_when_asked() {
    assert_eq!(delete(&["a 1 b 2", "a null"]), r#"{"b":2}"#);
    // Nested, because deletion falls out of the same recursion.
    assert_eq!(delete(&["a {b 1 c 2}", "a {b null}"]), r#"{"a":{"c":2}}"#);
    // Deleting something that was never there is not an error.
    assert_eq!(delete(&["a 1", "nope null"]), r#"{"a":1}"#);
    // The rest of the overlay still applies.
    assert_eq!(delete(&["a 1 b 2", "a null c 3"]), r#"{"b":2,"c":3}"#);
}

/// Deleting is a member-level operation. A root `null` has nothing to be removed from, so it
/// replaces like any other value of a different kind.
#[test]
fn a_root_null_replaces_under_either_policy() {
    assert_eq!(set(&["a 1", "null"]), "null");
    assert_eq!(delete(&["a 1", "null"]), "null");
}

/// Arrays are values, so a null inside one is data and never a deletion.
#[test]
fn a_null_inside_an_array_is_data() {
    assert_eq!(delete(&["a [1]", "a [null 2]"]), r#"{"a":[null,2]}"#);
}

// --- the mutating form ------------------------------------------------------------------------

#[test]
fn merge_into_changes_the_base_in_place() {
    let mut base = parse("a 1 b {c 2}").unwrap();
    merge_into(&mut base, parse("b {d 3}").unwrap(), Nulls::Set);
    assert_eq!(tot::json::to_string(&base), r#"{"a":1,"b":{"c":2,"d":3}}"#);

    // And the result is a document in its own right.
    let text = tot::format_value(&base);
    assert_eq!(parse(&text).unwrap(), base);
}
