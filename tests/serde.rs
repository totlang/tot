//! serde tests. Only runs with `--features serde`.
#![cfg(feature = "serde")]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tot::Value;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Config {
    name: String,
    version: u32,
    #[serde(default)]
    tags: Vec<String>,
    listen: Listen,
    retries: Option<u8>,
    mode: Mode,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Listen {
    host: String,
    port: u16,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Off,
    Retry(u8),
    Window(u32, u32),
    Backoff { base: f64, cap: f64 },
}

fn config() -> Config {
    Config {
        name: "svc".to_string(),
        version: 3,
        tags: vec!["a".to_string(), "b".to_string()],
        listen: Listen {
            host: "0.0.0.0".to_string(),
            port: 8080,
        },
        retries: None,
        mode: Mode::Retry(5),
    }
}

/// `T -> tot -> T`, the property everything else is a special case of.
fn round_trip<T>(value: &T) -> T
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let text = tot::to_string(value).expect("serialize");
    tot::from_str(&text).unwrap_or_else(|e| panic!("deserialize `{text}`: {e}"))
}

// --- the ordinary case ----------------------------------------------------------------------

#[test]
fn a_struct_round_trips() {
    assert_eq!(round_trip(&config()), config());
}

#[test]
fn a_struct_serializes_to_the_document_you_would_have_written() {
    let text = tot::to_string(&config().listen).unwrap();
    assert_eq!(text, "host \"0.0.0.0\"\nport 8080\n");
}

#[test]
fn a_document_deserializes() {
    let config: Config = tot::from_str(
        r#"
        name "svc"
        version 3
        tags ["a" "b"]
        listen { host "0.0.0.0" port 8080 }
        retries null
        mode {retry 5}
        "#,
    )
    .unwrap();
    assert_eq!(config, self::config());
}

/// Every value the serializer writes has to be readable by the parser, including the ones
/// the formatter writes as blocks.
#[test]
fn a_serialized_document_parses() {
    let text = tot::to_string(&config()).unwrap();
    assert_eq!(
        tot::parse(&text).unwrap(),
        tot::to_value(&config()).unwrap()
    );
    assert_eq!(tot::format(&text).unwrap(), text);
}

#[test]
fn a_multi_line_string_survives_the_round_trip() {
    let banner = "line one\n\nline three";
    let text = tot::to_string(&BTreeMap::from([("motd", banner)])).unwrap();
    assert!(text.contains("\"\"\""), "{text}");

    let back: BTreeMap<String, String> = tot::from_str(&text).unwrap();
    assert_eq!(back["motd"], banner);
}

// --- numbers ---------------------------------------------------------------------------------

#[test]
fn integers_and_floats_stay_apart() {
    assert_eq!(tot::to_string(&1i32).unwrap(), "1\n");
    assert_eq!(tot::to_string(&1.0f64).unwrap(), "1.0\n");
    assert_eq!(tot::to_string(&0.1f32).unwrap(), "0.1\n");

    // An integer is an acceptable float, but not the reverse — the language keeps `1` and
    // `1.0` distinct, and so does this.
    assert_eq!(tot::from_str::<f64>("1").unwrap(), 1.0);
    let e = tot::from_str::<u32>("1.0").unwrap_err();
    assert!(e.to_string().contains("invalid type"), "{e}");
}

#[test]
fn wide_integers_survive() {
    assert_eq!(round_trip(&u64::MAX), u64::MAX);
    assert_eq!(round_trip(&i64::MIN), i64::MIN);
    assert_eq!(round_trip(&u128::MAX), u128::MAX);
    assert_eq!(round_trip(&i128::MIN), i128::MIN);
    assert_eq!(
        tot::to_string(&u128::MAX).unwrap(),
        "340282366920938463463374607431768211455\n"
    );
}

#[test]
fn an_integer_too_big_for_the_field_is_refused() {
    let e = tot::from_str::<u8>("300").unwrap_err();
    assert!(e.to_string().contains("expected u8"), "{e}");
}

/// tot has no way to write these, and saying so beats silently writing `null` the way JSON
/// encoders do.
#[test]
fn infinity_and_nan_are_refused() {
    for bad in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        let e = tot::to_string(&bad).expect_err("should not serialize");
        assert!(e.to_string().contains("no infinity and no NaN"), "{e}");
    }
    assert!(tot::to_string(&f32::NAN).is_err());
}

// --- enums, options, and the rest of the model ------------------------------------------------

#[test]
fn every_variant_shape_round_trips() {
    for mode in [
        Mode::Off,
        Mode::Retry(5),
        Mode::Window(1, 2),
        Mode::Backoff {
            base: 0.5,
            cap: 30.0,
        },
    ] {
        assert_eq!(round_trip(&mode), mode);
    }
}

/// A unit variant is a bare string; everything else is one member naming the variant.
#[test]
fn variants_are_externally_tagged() {
    assert_eq!(tot::to_string(&Mode::Off).unwrap(), "\"off\"\n");
    assert_eq!(tot::to_string(&Mode::Retry(5)).unwrap(), "retry 5\n");
    assert_eq!(
        tot::to_string(&Mode::Window(1, 2)).unwrap(),
        "window [\n  1\n  2\n]\n"
    );
}

#[test]
fn an_option_is_null_or_the_value() {
    assert_eq!(tot::to_string(&None::<u8>).unwrap(), "null\n");
    assert_eq!(tot::to_string(&Some(0u8)).unwrap(), "0\n");

    // `null` is absent; `false` and `0` are present.
    assert_eq!(tot::from_str::<Option<u8>>("null").unwrap(), None);
    assert_eq!(tot::from_str::<Option<u8>>("0").unwrap(), Some(0));
    assert_eq!(tot::from_str::<Option<bool>>("false").unwrap(), Some(false));
}

#[test]
fn a_missing_member_and_a_null_one_are_not_the_same() {
    // `Option` accepts either, which is what `#[serde(default)]` is for elsewhere.
    let with_null: Option<u8> = tot::from_str("null").unwrap();
    assert_eq!(with_null, None);

    let e = tot::from_str::<Listen>("host \"h\"").unwrap_err();
    assert!(e.to_string().contains("missing field `port`"), "{e}");
}

#[test]
fn maps_with_non_string_keys_get_string_keys() {
    let map = BTreeMap::from([(80u16, "http".to_string()), (443, "https".to_string())]);
    assert_eq!(
        tot::to_string(&map).unwrap(),
        "80 \"http\"\n443 \"https\"\n"
    );
    assert_eq!(round_trip(&map), map);
}

#[test]
fn bytes_are_an_array_of_integers() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Blob(Vec<u8>);

    assert_eq!(
        tot::to_string(&Blob(vec![1, 2])).unwrap(),
        "[\n  1\n  2\n]\n"
    );
    assert_eq!(round_trip(&Blob(vec![0, 255])), Blob(vec![0, 255]));
}

// --- Value itself ------------------------------------------------------------------------------

/// A `Value` field takes whatever was there, so part of a document can stay untyped.
#[test]
fn a_value_can_sit_inside_a_typed_struct() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Envelope {
        kind: String,
        body: Value,
    }

    let src = "kind \"event\"\nbody {a 1 b [true null \"x\"] c 1.5}\n";
    let envelope: Envelope = tot::from_str(src).unwrap();

    assert_eq!(
        envelope
            .body
            .get("a")
            .unwrap()
            .as_integer()
            .unwrap()
            .as_str(),
        "1"
    );
    assert_eq!(
        envelope.body.get("c").unwrap().as_float().unwrap().as_str(),
        "1.5"
    );

    // Serializing writes block form, as every converter does — there is no author's
    // inline/block choice for it to preserve — so the value is what round-trips, not the text.
    let out = tot::to_string(&envelope).unwrap();
    assert_eq!(tot::parse(&out).unwrap(), tot::parse(src).unwrap());
}

#[test]
fn a_value_round_trips_through_serde() {
    let value = tot::parse("a 1 b [1.5 true null] c {d \"x\"}").unwrap();
    assert_eq!(tot::to_value(&value).unwrap(), value);
    assert_eq!(round_trip(&value), value);
}

/// A float's *lexeme* does not survive the trip, because serde only carries the number. The
/// value is unchanged; the spelling is normalized, and `Float` equality is lexical.
#[test]
fn serde_normalizes_float_lexemes() {
    let value = tot::parse("a 1. b .5 c 1.00").unwrap();
    let back = tot::to_value(&value).unwrap();

    assert_ne!(back, value);
    assert_eq!(
        value.get("a").unwrap().as_f64(),
        back.get("a").unwrap().as_f64()
    );
    assert_eq!(tot::to_string(&back).unwrap(), "a 1.0\nb 0.5\nc 1.0\n");
}

/// The example file is the one document with every feature in it at once.
#[test]
fn the_example_config_round_trips() {
    let src = include_str!("../examples/config.tot");
    let value: Value = tot::from_str(src).unwrap();

    assert_eq!(value, tot::parse(src).unwrap());
    assert_eq!(tot::to_value(&value).unwrap(), value);
    assert_eq!(tot::parse(&tot::to_string(&value).unwrap()).unwrap(), value);
}

/// A borrowed field points into the document rather than copying out of it.
#[test]
fn strings_can_borrow_from_the_document() {
    #[derive(Debug, PartialEq, Deserialize)]
    struct Borrowed<'a> {
        name: &'a str,
    }

    let value = tot::parse(r#"name "svc""#).unwrap();
    let borrowed: Borrowed<'_> = tot::from_value(&value).unwrap();
    assert_eq!(borrowed.name, "svc");
}

// --- diagnostics ---------------------------------------------------------------------------------

/// The point of tracking a path: the error names a place you can look, spelled the way
/// `tot get` spells it.
#[test]
fn an_error_names_the_value_that_failed() {
    let e = tot::from_str::<Config>(
        r#"
        name "svc"
        version 3
        listen { host "0.0.0.0" port "8080" }
        retries null
        mode "off"
        "#,
    )
    .unwrap_err();

    assert_eq!(e.path().as_deref(), Some("listen.port"));
    assert!(e.to_string().ends_with("at `listen.port`"), "{e}");
    assert!(e.message().contains("invalid type"), "{}", e.message());
}

#[test]
fn an_error_inside_an_array_names_the_element() {
    let e = tot::from_str::<Vec<Listen>>("[{host \"h\" port 1} {host 2 port 2}]").unwrap_err();
    assert_eq!(e.path().as_deref(), Some("[1].host"));
}

/// A path that names a key needing quotes has to quote it, or it cannot be used.
#[test]
fn an_error_path_is_a_usable_path() {
    let e = tot::from_str::<BTreeMap<String, u8>>(r#""log level" "debug""#).unwrap_err();
    let path = e.path().expect("a path");
    assert_eq!(path, "\"log level\"");
    assert!(tot::Path::parse(&path).is_ok(), "{path}");
}

/// A document that does not parse keeps its span, so a caller can still draw a caret.
#[test]
fn a_parse_failure_keeps_its_span() {
    let src = "kind curly";
    let e = tot::from_str::<Config>(src).unwrap_err();

    let parse = e.parse_error().expect("a parse error");
    assert!(parse.message.contains("string values must be quoted"));
    assert!(parse.render(src).contains("^^^^^"), "{}", parse.render(src));
    assert!(e.path().is_none());
}

#[test]
fn a_serialization_error_names_the_value_too() {
    #[derive(Serialize)]
    struct Metrics {
        rate: f64,
    }
    #[derive(Serialize)]
    struct Top {
        metrics: Metrics,
    }

    let e = tot::to_string(&Top {
        metrics: Metrics { rate: f64::NAN },
    })
    .unwrap_err();
    assert_eq!(e.path().as_deref(), Some("metrics.rate"));
}
