//! Behavioural tests for the lexer and parser, written against SPEC.md.
//!
//! Parsed documents are compared as compact JSON, which is what `Value`'s `Display` produces.

use tot::{Value, parse};

/// Parse and render as compact JSON, printing a full diagnostic if it fails.
fn j(src: &str) -> String {
    match parse(src) {
        Ok(value) => tot::json::to_string(&value),
        Err(e) => panic!("expected a successful parse:\n{}", e.render(src)),
    }
}

/// The error message plus its help text.
fn err(src: &str) -> String {
    parse(src).expect_err("expected a parse error").to_string()
}

#[test]
fn spec_example() {
    let src = r#"
my-name "tim"

address {
  street "100 main st"
  zip 123456
  country "united states"
}

"favorite food" [
  "tacos"
  {
    name "fries"
    kind "curly"
    rating 10
  }
]
"#;
    assert_eq!(
        j(src),
        r#"{"my-name":"tim","address":{"street":"100 main st","zip":123456,"country":"united states"},"favorite food":["tacos",{"name":"fries","kind":"curly","rating":10}]}"#
    );
}

// --- goal #2: every JSON document is a valid tot document ---------------------------------

#[test]
fn json_parses_verbatim() {
    let json = r#"{"a": 1, "b": [1, 2, {"c": null}], "d": {"e": true}, "f": ""}"#;
    assert_eq!(
        j(json),
        r#"{"a":1,"b":[1,2,{"c":null}],"d":{"e":true},"f":""}"#
    );
    assert_eq!(j("[]"), "[]");
    assert_eq!(j("{}"), "{}");
}

#[test]
fn commas_and_colons_are_whitespace() {
    assert_eq!(j("a 1 b 2"), j(r#"{"a": 1, "b": 2}"#));
    assert_eq!(j("[1, 2, 3,]"), "[1,2,3]"); // trailing commas fall out for free
    assert_eq!(j(":::a,,,1:::"), r#"{"a":1}"#);
}

// --- document roots -----------------------------------------------------------------------

#[test]
fn document_roots() {
    assert_eq!(j(""), "{}");
    assert_eq!(j("  \n # nothing here\n"), "{}");
    assert_eq!(j("a 1"), r#"{"a":1}"#);
    assert_eq!(j("{a 1}"), r#"{"a":1}"#);
    assert_eq!(j("[1 2]"), "[1,2]");
    assert_eq!(j(r#""just a string""#), r#""just a string""#);
    assert_eq!(j("42"), "42");
    assert_eq!(j("true"), "true");
    assert_eq!(j("null"), "null");
}

#[test]
fn trailing_content_after_a_collection_root_is_rejected() {
    assert!(err("{a 1} b 2").contains("unexpected"));
    assert!(err("[1] 2").contains("unexpected"));
}

// --- barewords ----------------------------------------------------------------------------

#[test]
fn bareword_keys() {
    assert_eq!(j("path/to/thing 1"), r#"{"path/to/thing":1}"#);
    assert_eq!(j("com.example.setting 1"), r#"{"com.example.setting":1}"#);
    assert_eq!(j("my-name 1"), r#"{"my-name":1}"#);
    assert_eq!(j("café 1"), r#"{"café":1}"#);
    // A key is always a string, so literals and digits are ordinary keys.
    assert_eq!(j(r#"123 "x""#), r#"{"123":"x"}"#);
    assert_eq!(j(r#"true "x""#), r#"{"true":"x"}"#);
}

/// Editors write them, and a BOM is not whitespace, so leaving it alone would make it the
/// first character of the first key.
#[test]
fn a_leading_byte_order_mark_is_skipped() {
    assert_eq!(j("\u{feff}a 1"), r#"{"a":1}"#);
}

/// A key is a string that happens to be written without quotes, so it may not hold a character
/// a string may not hold. Left legal, a bare key could carry a raw `U+0001` the same document
/// could not write between quotes — and every emitter asks the same predicate, so `from json`
/// wrote one straight back out unquoted.
#[test]
fn a_control_character_is_not_a_bareword_character() {
    let message = err("a\u{1}b 1");
    assert!(
        message.contains("literal control character U+0001"),
        "{message}"
    );
    assert!(message.contains("quote the key"), "{message}");

    // In value position, and standing alone between two tokens.
    assert!(err("a \u{1}").contains("U+0001"));
    assert!(err("a 1 \u{2} b 2").contains("U+0002"));

    // `U+000B` and `U+000C` are control characters that are also whitespace. A string calls
    // them control characters, so a bareword does too rather than blaming the whitespace rule.
    assert!(err("a\u{b}b 1").contains("literal control character U+000B"));
    assert!(err("a\u{c}b 1").contains("literal control character U+000C"));

    // Escaped, it is an ordinary key — this is the spelling the diagnostic asks for.
    assert_eq!(j(r#""a\u0001b" 1"#), "{\"a\\u0001b\":1}");
}

/// It is skipped for diagnostics too, and has to be: a mark that renders as nothing at all must
/// not occupy a column, or every caret on the first line lands one place to the right of what
/// it points at.
#[test]
fn a_byte_order_mark_takes_up_no_column() {
    let marked = "\u{feff}kind curly\n";
    let plain = "kind curly\n";
    let (marked_error, plain_error) = (
        tot::parse(marked).unwrap_err(),
        tot::parse(plain).unwrap_err(),
    );

    assert_eq!(marked_error.line_col(marked), (1, 6));
    assert_eq!(plain_error.line_col(plain), (1, 6));

    // The strongest statement of it: the same document with and without the mark renders the
    // same diagnostic, which holds only if the mark is off the printed line as well as out of
    // the count.
    let rendered = marked_error.render(marked);
    assert_eq!(rendered, plain_error.render(plain));
    assert!(!rendered.contains('\u{feff}'), "the mark is still printed");

    // Only a *leading* mark is skipped. `U+FEFF` is not whitespace in Unicode, so anywhere else
    // it is an ordinary bareword character and the lines below the first are untouched.
    let two = "\u{feff}a 1\nkind curly\n";
    assert_eq!(tot::parse(two).unwrap_err().line_col(two), (2, 6));
}

#[test]
fn tokens_are_self_delimiting() {
    assert_eq!(j(r#"a"b""#), r#"{"a":"b"}"#);
    assert_eq!(j("a{b 1}"), r#"{"a":{"b":1}}"#);
}

// --- comments -----------------------------------------------------------------------------

#[test]
fn comments() {
    assert_eq!(j("a 1 # trailing\nb 2"), r#"{"a":1,"b":2}"#);
    assert_eq!(j("a 1 #no space needed"), r#"{"a":1}"#);
    assert_eq!(j("# whole line\na 1"), r#"{"a":1}"#);
    // There is no block comment form: `#` always runs to the end of the line.
    assert_eq!(j("#* not a block *# a 1"), "{}");
    assert_eq!(j("a 1 #* still just a comment *#\nb 2"), r#"{"a":1,"b":2}"#);
}

// --- strings ------------------------------------------------------------------------------

#[test]
fn string_escapes() {
    assert_eq!(j(r#"a "tab\there""#), r#"{"a":"tab\there"}"#);
    assert_eq!(j(r#"a "Aé""#), r#"{"a":"Aé"}"#);
    assert_eq!(j(r#"a "😀""#), "{\"a\":\"\u{1f600}\"}");
    assert_eq!(j(r#"a "q \" b \\ s \/""#), r#"{"a":"q \" b \\ s /"}"#);
}

#[test]
fn string_errors() {
    assert!(err(r#"a "unterminated"#).contains("unterminated string"));
    assert!(err("a \"one\ntwo\"").contains("multi-line"));
    assert!(err(r#"a "\q""#).contains(r"unknown escape `\q`"));
    assert!(err(r#"a "\u00""#).contains("four hex digits"));
    assert!(err(r#"a "\ud83d""#).contains("unpaired high surrogate"));
    assert!(err("a \"\u{7}\"").contains("literal control character U+0007"));
}

// --- multi-line strings -------------------------------------------------------------------

#[test]
fn multiline_dedents_to_the_closing_delimiter() {
    let src = "motd \"\"\"\n  hello\n    indented\n  world\n  \"\"\"";
    assert_eq!(j(src), r#"{"motd":"hello\n  indented\nworld"}"#);
}

#[test]
fn multiline_has_no_trailing_newline() {
    assert_eq!(j("a \"\"\"\nx\n\"\"\""), r#"{"a":"x"}"#);
    assert_eq!(j("a \"\"\"\nx\\n\n\"\"\""), r#"{"a":"x\n"}"#);
}

#[test]
fn multiline_normalizes_crlf() {
    let src = "a \"\"\"\r\n  one\r\n  two\r\n  \"\"\"";
    assert_eq!(j(src), r#"{"a":"one\ntwo"}"#);
}

#[test]
fn multiline_blank_lines_and_continuations() {
    assert_eq!(
        j("a \"\"\"\n  one\n\n  two\n  \"\"\""),
        r#"{"a":"one\n\ntwo"}"#
    );
    assert_eq!(
        j("a \"\"\"\n  one \\\n  two\n  \"\"\""),
        r#"{"a":"one two"}"#
    );
}

/// The point of anchoring indentation to the closing delimiter: reindenting the block a
/// string lives in must not change the string.
#[test]
fn multiline_value_survives_reindentation() {
    let tight = "a {\n  b \"\"\"\n    x\n      y\n    \"\"\"\n}";
    let loose = "a {\n        b \"\"\"\n          x\n            y\n          \"\"\"\n}";
    assert_eq!(j(tight), r#"{"a":{"b":"x\n  y"}}"#);
    assert_eq!(j(tight), j(loose));
}

#[test]
fn multiline_errors() {
    assert!(err("a \"\"\" oops\n  x\n  \"\"\"").contains("content may not follow"));
    assert!(err("a \"\"\"\nx\n").contains("unterminated multi-line string"));
    assert!(err("a \"\"\"\n  x\ny\n  \"\"\"").contains("not indented to match"));
}

/// The closing delimiter owns its whole line. Without that rule an unescaped `"""` in the
/// content closes the string early and the error lands on some later line instead.
#[test]
fn a_closing_delimiter_ends_its_line() {
    let src = "motd \"\"\"\n  hello\n  \"\"\" oops\n  \"\"\"\n";
    let e = parse(src).expect_err("should not parse");
    assert!(e.message.contains("content after the closing"), "{e}");
    assert_eq!(e.line_col(src), (3, 6));

    // Escaping the quote keeps the line as content, which is the fix the help suggests.
    let ok = parse("motd \"\"\"\n  hello\n  \\\"\"\" oops\n  \"\"\"\n").unwrap();
    assert_eq!(ok.get("motd").unwrap().as_str(), Some("hello\n\"\"\" oops"));
}

// --- reading text that sits where a value goes ------------------------------------------------

/// Same grammar, different guess about what a lone bareword means. In a file it is most
/// likely a key that lost its value; in a value position there is no key to lose one.
#[test]
fn parse_value_blames_the_value_not_a_missing_key() {
    assert!(err("svc").contains("has no value"));

    let e = tot::parse_value("svc").expect_err("should not parse");
    assert!(e.message.contains("string values must be quoted"), "{e}");
    assert!(e.help.unwrap().contains(r#"write `"svc"`"#));
}

/// The grammar really is a document's, so everything `format_value` writes reads back —
/// including the brace-less object that `tot get` prints for a collection.
#[test]
fn parse_value_takes_whatever_a_document_takes() {
    for src in [
        "8080",
        r#""svc""#,
        "true",
        "null",
        "1.5",
        "[1 2]",
        "{a 1}",
        r#"host "::" port 80"#,
        "",
    ] {
        let value = tot::parse_value(src)
            .unwrap_or_else(|e| panic!("`{src}` should parse as a value: {e}"));
        assert_eq!(value, parse(src).unwrap(), "`{src}`");
    }

    // A key that really is missing its value still says so, because there is a key.
    assert!(
        tot::parse_value("a 1 b")
            .unwrap_err()
            .message
            .contains("key `b` has no value")
    );
}

// --- editing a parsed document --------------------------------------------------------------

/// Read, change one thing, write it back — the reason a `Value` needs to be mutable at all.
#[test]
fn a_parsed_document_can_be_edited() {
    let mut value = parse("name \"svc\"\nlisten {host \"::\" port 8080}\nstale true").unwrap();

    // Replace an existing member, which `insert` deliberately refuses to do.
    *value.get_mut("listen").unwrap().get_mut("port").unwrap() =
        Value::Integer(tot::Integer::from_i64(9090));
    assert!(
        !value
            .as_object_mut()
            .unwrap()
            .insert("name".to_string(), Value::Null)
    );

    // Add one, and drop one.
    assert!(
        value
            .as_object_mut()
            .unwrap()
            .insert("added".to_string(), Value::Bool(true))
    );
    assert_eq!(
        value.as_object_mut().unwrap().remove("stale"),
        Some(Value::Bool(true))
    );
    assert_eq!(value.as_object_mut().unwrap().remove("stale"), None);

    assert_eq!(
        j(&tot::format_value(&value)),
        r#"{"name":"svc","listen":{"host":"::","port":9090},"added":true}"#
    );
}

/// Removing a member has to leave the index agreeing with the order, or a later lookup
/// silently returns the wrong value.
#[test]
fn removing_a_member_keeps_the_rest_addressable() {
    let mut value = parse("a 1 b 2 c 3 d 4").unwrap();
    let map = value.as_object_mut().unwrap();

    map.remove("a");
    map.remove("c");

    assert_eq!(map.len(), 2);
    assert_eq!(map.keys().collect::<Vec<_>>(), ["b", "d"]);
    for (key, want) in [("b", "2"), ("d", "4")] {
        let found = map.get(key).and_then(Value::as_integer).map(|i| i.as_str());
        assert_eq!(found, Some(want), "`{key}` after removals");
    }
    assert!(map.get("a").is_none() && map.get("c").is_none());

    // The index has to accept the names back, too.
    assert!(map.insert("a".to_string(), Value::Null));
    assert_eq!(map.keys().collect::<Vec<_>>(), ["b", "d", "a"]);
}

// --- numbers ------------------------------------------------------------------------------

#[test]
fn number_lexemes_are_preserved() {
    assert_eq!(
        j("a 0 b -0 c 1.5 d -2.5e+10 e 1E-3"),
        r#"{"a":0,"b":-0,"c":1.5,"d":-2.5e+10,"e":1E-3}"#
    );
}

#[test]
fn integers_and_floats_are_distinct() {
    let value = parse("i 1 f 1.0 z 0 e 6e23 n 1e-5").unwrap();
    assert!(matches!(value.get("i"), Some(Value::Integer(_))));
    assert!(matches!(value.get("z"), Some(Value::Integer(_))));
    assert!(matches!(value.get("f"), Some(Value::Float(_))));
    // An exponent makes a float too — `1e-5` could not be an integer.
    assert!(matches!(value.get("e"), Some(Value::Float(_))));
    assert!(matches!(value.get("n"), Some(Value::Float(_))));
}

/// An integer keeps its lexeme and so has no range limit, but a float has to denote a real
/// `f64`. tot cannot write an infinity, so a lexeme that means one has no value here — and
/// letting it through would make a document that parses but that no converter can write.
#[test]
fn a_float_that_no_f64_can_hold_is_rejected() {
    for src in ["a 1e999", "a -1e999", "a 1.5e400"] {
        assert!(err(src).contains("outside the range of a float"), "{src}");
    }
    // As the whole document, too, where a lone bareword is otherwise read as a missing value.
    assert!(err("1e999").contains("outside the range of a float"));

    // Underflow is a real value — it is zero, and the lexeme still survives.
    let value = parse("a 1e-999").unwrap();
    assert_eq!(value.get("a").unwrap().as_f64(), Some(0.0));
    assert_eq!(
        value.get("a").unwrap().as_float().unwrap().as_str(),
        "1e-999"
    );

    // The largest finite float is fine; an integer of any width still is too.
    assert!(parse("a 1.7976931348623157e308").is_ok());
    assert!(parse(&format!("a {}", "9".repeat(400))).is_ok());
}

/// `1.` and `.1` are tot-only forms: legal here, not legal JSON, normalized on the way out.
#[test]
fn dot_forms_resolve_and_normalize() {
    assert_eq!(
        j("a 1. b .1 c -.5 d 0."),
        r#"{"a":1.0,"b":0.1,"c":-0.5,"d":0.0}"#
    );

    let value = parse("a 1. b .1 c -.5").unwrap();
    assert_eq!(value.get("a").unwrap().as_f64(), Some(1.0));
    assert_eq!(value.get("b").unwrap().as_f64(), Some(0.1));
    assert_eq!(value.get("c").unwrap().as_f64(), Some(-0.5));
    // The lexeme survives; only Display normalizes.
    assert_eq!(value.get("a").unwrap().as_float().unwrap().as_str(), "1.");
}

/// Lexemes are preserved, so integers outside `i64` survive a round trip.
#[test]
fn wide_integers_keep_their_precision() {
    let value = parse("a 9007199254740993 b 18446744073709551615").unwrap();

    let a = value.get("a").unwrap().as_integer().unwrap();
    assert_eq!(a.as_i64(), Some(9007199254740993));

    let b = value.get("b").unwrap().as_integer().unwrap();
    assert_eq!(b.as_i64(), None);
    assert_eq!(b.as_u64(), Some(18446744073709551615));
    assert_eq!(b.as_str(), "18446744073709551615");
}

#[test]
fn malformed_numbers_name_themselves() {
    for bad in ["01234", "+5", "0x1f", "1_000", "1.2.3", "1e", "."] {
        let message = err(&format!("zip {bad}"));
        assert!(message.contains(bad), "{bad}: {message}");
    }
    assert!(err("zip 01234").contains("not a valid number"));
    // A zip code with a leading zero has to be a string, which is what you wanted anyway.
    assert_eq!(j(r#"zip "01234""#), r#"{"zip":"01234"}"#);
}

// --- the load-bearing rules ---------------------------------------------------------------

#[test]
fn string_values_must_be_quoted() {
    let message = err("kind curly");
    assert!(
        message.contains("string values must be quoted"),
        "{message}"
    );
    assert!(message.contains(r#"write `"curly"`"#), "{message}");
    assert!(err("zip NaN").contains("must be quoted"));
}

/// The parity hazard: a member with a missing value shifts the ones after it. Requiring
/// quotes on string values means the error lands on the shifted token, not at EOF.
#[test]
fn parity_hazard_is_caught_at_the_shifted_token() {
    let src = "debug\nport 8080";
    let e = parse(src).unwrap_err();
    assert!(e.message.contains("string values must be quoted"), "{e}");
    assert_eq!(e.line_col(src), (2, 1));
}

#[test]
fn missing_value_blames_the_key() {
    let src = "a { b 1 c }";
    let e = parse(src).unwrap_err();
    assert!(e.message.contains("key `c` has no value"), "{e}");
    assert_eq!(e.line_col(src), (1, 9));

    assert!(err("a 1 dangling").contains("key `dangling` has no value"));
}

#[test]
fn duplicate_keys_are_rejected() {
    assert!(err("a 1 a 2").contains("duplicate key `a`"));
    assert!(err(r#"{"a": 1, "a": 2}"#).contains("duplicate key `a`"));
    // Same key in different scopes is fine.
    assert_eq!(j("a { x 1 } b { x 2 }"), r#"{"a":{"x":1},"b":{"x":2}}"#);
}

#[test]
fn reserved_characters() {
    assert!(err("a = 1").contains("no assignment operator"));
    assert_eq!(j(r#""a=b" 1"#), r#"{"a=b":1}"#);
    assert_eq!(j(r#""has # hash" 1"#), r#"{"has # hash":1}"#);
    assert!(err("a\u{00a0}1").contains("U+00A0"));
}

#[test]
fn bracket_errors() {
    assert!(err("a { b 1").contains("unclosed `{`"));
    assert!(err("a [ 1 2").contains("unclosed `[`"));
    assert!(err("a 1 }").contains("unexpected `}`"));
    assert!(err("{ [1] 2 }").contains("expected a key"));
    // Arrays hold values, not pairs.
    assert!(err("[ a 1 ]").contains("string values must be quoted"));
}

#[test]
fn nesting_is_depth_limited() {
    assert!(err(&"[".repeat(200)).contains("maximum nesting depth"));
    assert!(parse(&format!("{}{}", "[".repeat(100), "]".repeat(100))).is_ok());
}

// --- diagnostics and the value API --------------------------------------------------------

#[test]
fn errors_render_with_a_caret() {
    let src = "address {\n  kind curly\n}";
    let rendered = parse(src).unwrap_err().render(src);
    assert!(rendered.contains("2:8"), "{rendered}");
    assert!(rendered.contains("kind curly"), "{rendered}");
    assert!(rendered.contains("^^^^^"), "{rendered}");
}

#[test]
fn key_order_is_preserved() {
    let value = parse(r#"z 1 a 2 m { inner "x" }"#).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.keys().collect::<Vec<_>>(), ["z", "a", "m"]);
    assert_eq!(
        value
            .get("m")
            .and_then(|m| m.get("inner"))
            .and_then(Value::as_str),
        Some("x")
    );
}
