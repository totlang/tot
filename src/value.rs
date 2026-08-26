use std::collections::HashMap;
use std::fmt;
use std::fmt::Write as _;

/// A tot value. JSON's data model, with JSON's single number type split into integers and
/// floats — a number is a float if it has a `.` or an exponent, and an integer otherwise.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// `null`
    Null,
    /// `true` or `false`
    Bool(bool),
    /// A number with no `.` and no exponent: `42`, `-7`.
    Integer(Integer),
    /// A number with a `.` or an exponent: `1.5`, `1.`, `.1`, `6e23`.
    Float(Float),
    /// A string.
    String(String),
    /// An ordered list of values.
    Array(Vec<Value>),
    /// An insertion-ordered map of string keys to values.
    Object(Map),
}

impl Value {
    /// The string contents, if this is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The boolean, if this is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The integer, if this is one. A float is not an integer even if it is integral:
    /// `1.0` is a [`Value::Float`].
    pub fn as_integer(&self) -> Option<&Integer> {
        match self {
            Value::Integer(i) => Some(i),
            _ => None,
        }
    }

    /// The float, if this is one.
    pub fn as_float(&self) -> Option<&Float> {
        match self {
            Value::Float(f) => Some(f),
            _ => None,
        }
    }

    /// The numeric value of either number variant, for callers that don't care which it is.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Integer(i) => Some(i.as_f64()),
            Value::Float(f) => Some(f.as_f64()),
            _ => None,
        }
    }

    /// The elements, if this is an array.
    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// The members, if this is an object.
    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Value::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Look up a member, if this is an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?.get(key)
    }
}

/// An integer, stored as its original lexeme.
///
/// Keeping the text means values outside `i64` — `u64` snowflake ids, bignums that arrived in
/// a JSON file — survive a round trip instead of being clamped or silently turned into floats.
/// Integer lexemes are canonical (leading zeros are a parse error), so equality is also value
/// equality, apart from `-0`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Integer(String);

impl Integer {
    pub(crate) fn from_lexeme(lexeme: impl Into<String>) -> Self {
        Integer(lexeme.into())
    }

    /// Build from a machine integer.
    pub fn from_i64(value: i64) -> Self {
        Integer(value.to_string())
    }

    /// Build from an unsigned machine integer, including values above `i64::MAX`.
    pub fn from_u64(value: u64) -> Self {
        Integer(value.to_string())
    }

    /// Build from a wide machine integer.
    pub fn from_i128(value: i128) -> Self {
        Integer(value.to_string())
    }

    /// Build from a wide unsigned machine integer.
    pub fn from_u128(value: u128) -> Self {
        Integer(value.to_string())
    }

    /// The integer exactly as it was written.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The value as an `i64`, if it fits.
    pub fn as_i64(&self) -> Option<i64> {
        self.0.parse().ok()
    }

    /// The value as a `u64`, if it fits and is non-negative.
    pub fn as_u64(&self) -> Option<u64> {
        self.0.parse().ok()
    }

    /// The value as an `i128`, if it fits.
    pub fn as_i128(&self) -> Option<i128> {
        self.0.parse().ok()
    }

    /// The value as a `u128`, if it fits and is non-negative.
    pub fn as_u128(&self) -> Option<u128> {
        self.0.parse().ok()
    }

    /// The value as an `f64`, which may lose precision.
    pub fn as_f64(&self) -> f64 {
        self.0.parse().unwrap_or(f64::NAN)
    }
}

impl fmt::Display for Integer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A float, stored as its original lexeme.
///
/// Equality is lexical, so `1.0`, `1.00`, and `1e0` are three different `Float`s even though
/// they denote the same number. Compare [`as_f64`](Float::as_f64) for value equality.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Float(String);

impl Float {
    pub(crate) fn from_lexeme(lexeme: impl Into<String>) -> Self {
        Float(lexeme.into())
    }

    /// Build from a machine float. Returns `None` for infinities and NaN, which tot has no
    /// way to write.
    pub fn from_f64(value: f64) -> Option<Self> {
        value
            .is_finite()
            .then(|| Float::from_text(value.to_string()))
    }

    /// Build from a single-precision float. Returns `None` for infinities and NaN.
    ///
    /// Goes through the `f32`'s own shortest spelling, so `0.1f32` stays `0.1` rather than
    /// becoming the `0.10000000149011612` that widening it to `f64` would produce.
    pub fn from_f32(value: f32) -> Option<Self> {
        value
            .is_finite()
            .then(|| Float::from_text(value.to_string()))
    }

    /// A finite float's text, made into a float lexeme: `1.0f64` renders as `1`, which would
    /// come back as an integer.
    fn from_text(mut text: String) -> Self {
        if !text.contains(['.', 'e', 'E']) {
            text.push_str(".0");
        }
        Float(text)
    }

    /// The float exactly as it was written, which may be a form JSON does not accept
    /// (`1.`, `.1`). [`Display`](std::fmt::Display) gives the normalized form.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The value as an `f64`.
    pub fn as_f64(&self) -> f64 {
        self.0.parse().unwrap_or(f64::NAN)
    }
}

/// Normalizes the two tot-only forms into valid JSON: `1.` becomes `1.0` and `.1` becomes
/// `0.1`. Every other lexeme is already valid JSON and is written verbatim.
impl fmt::Display for Float {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = &self.0;
        let Some(dot) = s.find('.') else {
            return f.write_str(s); // exponent-only form, e.g. `6e23`
        };
        let (head, tail) = (&s[..dot], &s[dot + 1..]);
        f.write_str(head)?;
        if !head.ends_with(|c: char| c.is_ascii_digit()) {
            f.write_str("0")?;
        }
        f.write_str(".")?;
        if !tail.starts_with(|c: char| c.is_ascii_digit()) {
            f.write_str("0")?;
        }
        f.write_str(tail)
    }
}

/// An insertion-ordered map with unique keys.
///
/// Key order is preserved so that documents round-trip in the order they were written.
#[derive(Debug, Clone, Default)]
pub struct Map {
    entries: Vec<(String, Value)>,
    index: HashMap<String, usize>,
}

impl Map {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of members.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no members.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Whether `key` is present.
    pub fn contains_key(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    /// The value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.index.get(key).map(|&i| &self.entries[i].1)
    }

    /// Members in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Members in insertion order, as a slice. serde's `MapAccess` asks for a key and its
    /// value in two calls, so the deserializer needs to index rather than iterate.
    #[cfg(feature = "serde")]
    pub(crate) fn entries(&self) -> &[(String, Value)] {
        &self.entries
    }

    /// Keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    /// Appends a member. Returns `false` without modifying the map if `key` is already
    /// present — duplicate keys are a parse error in tot, so the caller reports it.
    pub fn insert(&mut self, key: String, value: Value) -> bool {
        if self.index.contains_key(&key) {
            return false;
        }
        self.index.insert(key.clone(), self.entries.len());
        self.entries.push((key, value));
        true
    }
}

/// Order-sensitive: two maps with the same members in a different order are not equal.
impl PartialEq for Map {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

/// Writes the contents of a string with escapes applied, without the surrounding quotes.
/// tot's escapes are JSON's, so both emitters share this.
pub(crate) fn write_escaped(out: &mut String, s: &str) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
}
