//! TOML converter tests. Only runs with `--features toml`.
#![cfg(feature = "toml")]

use tot::toml::{self, FromToml, NullPolicy, ToToml};

#[test]
fn a_document_round_trips_apart_from_nulls() {
    // Plain members before the table: TOML hoists sub-tables below them, so a document in any
    // other order does not come back in its own — the one reordering the conversion makes.
    let value = tot::parse(
        "name \"svc\"\nratio 0.5\nregions [\"us\" \"eu\"]\nlisten {host \"0.0.0.0\" port 8080}\n",
    )
    .unwrap();
    let ToToml { text, dropped, .. } = toml::to_string(&value, NullPolicy::Omit).unwrap();
    assert!(dropped.is_empty());
    assert_eq!(toml::from_str(&text).unwrap().value, value);
}

#[test]
fn nulls_are_dropped_and_their_paths_reported() {
    let value = tot::parse("a 1 retries null nested {b null}").unwrap();
    let ToToml { text, dropped, .. } = toml::to_string(&value, NullPolicy::Omit).unwrap();
    assert_eq!(dropped, ["retries", "nested.b"]);
    assert!(!text.contains("retries"), "{text}");
}

#[test]
fn an_error_policy_refuses_a_null_instead() {
    let value = tot::parse("retries null").unwrap();
    let error = toml::to_string(&value, NullPolicy::Error).unwrap_err();
    assert_eq!(error.message, "retries: TOML has no null");
}

#[test]
fn the_root_must_be_an_object() {
    let value = tot::parse("[1 2]").unwrap();
    let error = toml::to_string(&value, NullPolicy::Omit).unwrap_err();
    assert!(
        error
            .message
            .contains("this document's root is not an object"),
        "{error:?}"
    );
}

#[test]
fn a_null_root_is_told_apart_from_a_root_of_the_wrong_shape() {
    // Under `Omit` the root null is dropped, which leaves nothing to write rather than
    // something unwritable. Both refusals are about the root, so only the cause differs — and
    // a reader told the root "is not an object" would go looking for the object they wrote.
    let value = tot::parse_value("null").unwrap();
    let error = toml::to_string(&value, NullPolicy::Omit).unwrap_err();
    assert_eq!(
        error.message,
        "TOML needs a table at the root, and this document is a single null"
    );

    // The other policy refuses the same document before the root is ever in question, and
    // names the path the way every other refusal does.
    let error = toml::to_string(&value, NullPolicy::Error).unwrap_err();
    assert_eq!(error.message, "the document root: TOML has no null");
}

#[test]
fn datetimes_become_strings_and_are_reported() {
    let FromToml {
        value, datetimes, ..
    } = toml::from_str("when = 1979-05-27\n").unwrap();
    assert_eq!(value, tot::parse("when \"1979-05-27\"").unwrap());
    assert_eq!(datetimes, ["when"]);
}

#[test]
fn an_integer_wider_than_64_bits_is_refused() {
    let value = tot::parse("n 670390396497129854978701249910389023").unwrap();
    let error = toml::to_string(&value, NullPolicy::Omit).unwrap_err();
    assert_eq!(
        error.message,
        "n: TOML integers are 64-bit signed, and `670390396497129854978701249910389023` \
         does not fit"
    );
}

#[test]
fn sub_tables_land_below_plain_values() {
    // TOML's syntax puts a sub-table after the plain members of its parent, so a document
    // written the other way round is hoisted on the way out. Feed it the order that makes the
    // hoist visible: one written table-first is the only case that can show it happening.
    let value = tot::parse("listen {host \"h\"} port 8080").unwrap();
    let text = toml::to_string(&value, NullPolicy::Omit).unwrap().text;

    let plain = text.find("port = 8080").expect("the plain member survived");
    let table = text.find("[listen]").expect("the sub-table survived");
    assert!(plain < table, "the sub-table was not hoisted:\n{text}");

    // And the consequence, which is the whole reason this is documented rather than silent:
    // `Map` equality is order-sensitive, so the document does *not* come back as itself. This
    // is the one reordering the conversion makes.
    let back = toml::from_str(&text).unwrap().value;
    assert_ne!(back, value, "the hoist somehow round-tripped");
    assert_eq!(back, tot::parse("port 8080 listen {host \"h\"}").unwrap());
}

#[test]
fn a_reported_path_is_spelled_the_way_a_path_spells_one() {
    // The keys that most need this are the ones a bare `.` join gets wrong: one holding a dot
    // would name a chain of members that do not exist, and one holding a space would not parse
    // at all. Both side channels go through the same speller, so both are checked here — and
    // checked by handing the result back to `Path`, not by eyeballing the quotes.
    let value = tot::parse("\"com.example\" {\"log level\" null}").unwrap();
    let dropped = toml::to_string(&value, NullPolicy::Omit).unwrap().dropped;
    assert_eq!(dropped, ["\"com.example\".\"log level\""]);
    let path = tot::Path::parse(&dropped[0]).expect("a reported path parses");
    assert_eq!(path.get(&value).expect("and resolves"), &tot::Value::Null);

    let read = toml::from_str("[\"com.example\"]\n\"log level\" = 1979-05-27\n").unwrap();
    assert_eq!(read.datetimes, ["\"com.example\".\"log level\""]);
    let path = tot::Path::parse(&read.datetimes[0]).expect("a reported path parses");
    assert_eq!(
        path.get(&read.value).expect("and resolves"),
        &tot::Value::String("1979-05-27".into())
    );
}

#[test]
fn non_finite_floats_are_refused() {
    // TOML spells them `inf` and `nan`; tot has no way to write either, so reading one
    // is a refusal that names where it was.
    let error = toml::from_str("a = inf\n").unwrap_err();
    assert_eq!(error.message, "a: tot cannot write the float `inf`");
    let error = toml::from_str("a = nan\n").unwrap_err();
    assert_eq!(error.message, "a: tot cannot write the float `NaN`");
}
