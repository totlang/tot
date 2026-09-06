//! YAML converter tests. Only runs with `--features yaml`.
#![cfg(feature = "yaml")]

use tot::yaml;

#[test]
fn a_document_round_trips() {
    let value =
        tot::parse("name \"svc\" ports [80 443] tls true timeout 1.5 retries null").unwrap();
    let text = yaml::to_string(&value).unwrap();
    assert_eq!(yaml::from_str(&text).unwrap(), value);
}

#[test]
fn aliases_resolve_and_inline() {
    let value = yaml::from_str("defaults: &d\n  port: 8080\nserver: *d\n").unwrap();
    assert_eq!(
        value,
        tot::parse("defaults {port 8080} server {port 8080}").unwrap()
    );
}

#[test]
fn a_non_string_key_is_refused() {
    let error = yaml::from_str("1: one").unwrap_err();
    assert!(
        error.message.contains("tot keys are always strings"),
        "{error:?}"
    );
}

#[test]
fn a_tag_is_refused() {
    let error = yaml::from_str("a: !custom 1").unwrap_err();
    assert!(
        error
            .message
            .contains("tot has no equivalent for the YAML tag"),
        "{error:?}"
    );
}

#[test]
fn a_refusal_names_the_path_the_way_a_path_spells_it() {
    // The converters share one speller so that a refusal can be handed straight to `tot get`.
    // A key holding a dot or a space is where a bare `.` join would name something that does
    // not exist, so the refusal is provoked under exactly those keys.
    let error = yaml::from_str("\"com.example\":\n  \"log level\": !custom 1\n").unwrap_err();
    let (path, _) = error
        .message
        .split_once(": ")
        .expect("a refusal names its path first");
    assert_eq!(path, "\"com.example\".\"log level\"");
    tot::Path::parse(path).expect("and names it in a form `tot get` accepts");
}

#[test]
fn a_multi_document_stream_is_refused() {
    assert!(yaml::from_str("a: 1\n---\nb: 2\n").is_err());
}

#[test]
fn an_integer_that_needs_u64_survives() {
    let value = yaml::from_str("n: 18446744073709551615\n").unwrap();
    assert_eq!(value, tot::parse("n 18446744073709551615").unwrap());
}

#[test]
fn an_integer_wider_than_64_bits_is_refused_on_the_way_out() {
    let value = tot::parse("n 670390396497129854978701249910389023").unwrap();
    let error = yaml::to_string(&value).unwrap_err();
    assert!(
        error
            .message
            .contains("does not fit in a 64-bit YAML integer"),
        "{error:?}"
    );
}

#[test]
fn integers_and_floats_stay_distinct() {
    let value = yaml::from_str("one: 1.0\ntwo: 2\n").unwrap();
    assert_eq!(value, tot::parse("one 1.0 two 2").unwrap());
}

#[test]
fn non_finite_floats_are_refused() {
    // YAML spells them `.inf` and `.nan`; tot has no way to write either, so reading one
    // is a refusal that names where it was.
    let error = yaml::from_str("a: .inf\n").unwrap_err();
    assert_eq!(error.message, "a: tot cannot write the float `inf`");
    let error = yaml::from_str("a: .nan\n").unwrap_err();
    assert_eq!(error.message, "a: tot cannot write the float `NaN`");
}
