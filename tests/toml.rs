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
    let ToToml { text, dropped } = toml::to_string(&value, NullPolicy::Omit).unwrap();
    assert!(dropped.is_empty());
    assert_eq!(toml::from_str(&text).unwrap().value, value);
}

#[test]
fn nulls_are_dropped_and_their_paths_reported() {
    let value = tot::parse("a 1 retries null nested {b null}").unwrap();
    let ToToml { text, dropped } = toml::to_string(&value, NullPolicy::Omit).unwrap();
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
fn datetimes_become_strings_and_are_reported() {
    let FromToml { value, datetimes } = toml::from_str("when = 1979-05-27\n").unwrap();
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
    // TOML's syntax puts a sub-table after the plain members of its parent, so a document in
    // any other order has to be hoisted on the way out — and still reads back the same.
    let value = tot::parse("port 8080 listen {host \"h\"}").unwrap();
    let text = toml::to_string(&value, NullPolicy::Omit).unwrap().text;
    assert_eq!(toml::from_str(&text).unwrap().value, value);
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
