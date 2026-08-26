use crate::error::{Error, Span};
use crate::lex::{Token, TokenKind, tokenize};
use crate::value::{Float, Integer, Map, Value};

/// Recursion limit, so that pathological nesting is a diagnostic rather than a stack overflow.
const MAX_DEPTH: usize = 128;

/// Parse a tot document.
///
/// The top level is an object body with the braces left off, unless the entire input is a
/// single value — which is what lets JSON documents with a scalar or array root parse.
pub fn parse(src: &str) -> Result<Value, Error> {
    from_tokens(src, &tokenize(src)?)
}

/// Parses an already-tokenized document, for callers that walk the same tokens twice.
pub(crate) fn from_tokens(src: &str, tokens: &[Token]) -> Result<Value, Error> {
    Parser {
        src,
        tokens,
        pos: 0,
        depth: 0,
    }
    .document()
}

struct Parser<'a, 't> {
    src: &'a str,
    tokens: &'t [Token],
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a, '_> {
    fn document(&mut self) -> Result<Value, Error> {
        if self.tokens.is_empty() {
            return Ok(Value::Object(Map::new()));
        }

        // `{` and `[` can never begin a key, so a document that opens with one is that value.
        if matches!(self.tokens[0].kind, TokenKind::LBrace | TokenKind::LBracket) {
            let value = self.value()?;
            if let Some(token) = self.tokens.get(self.pos) {
                return Err(Error::new(
                    token.span,
                    format!(
                        "unexpected {} after the top-level value",
                        token.kind.describe()
                    ),
                ));
            }
            return Ok(value);
        }

        // A lone scalar is a scalar document. A lone bareword that isn't a literal is much
        // more likely a key whose value was forgotten, so say that instead.
        if self.tokens.len() == 1 {
            let span = self.tokens[0].span;
            return match &self.tokens[0].kind {
                TokenKind::Str(s) => Ok(Value::String(s.clone())),
                TokenKind::Bareword => match literal(self.text(span)) {
                    Some(value) => Ok(value),
                    // A number-shaped lexeme that produced no value is out of range, not a
                    // key whose value was forgotten.
                    None if number_lexeme(self.text(span)).is_some() => {
                        Err(bad_value(span, self.text(span)))
                    }
                    None => Err(missing_value(span, self.text(span))),
                },
                kind => Err(Error::new(span, format!("unexpected {}", kind.describe()))),
            };
        }

        Ok(Value::Object(self.members(None)?))
    }

    fn text(&self, span: Span) -> &'a str {
        &self.src[span.start..span.end]
    }

    /// Parses `key value` pairs. `open` is the span of the `{` when parsing an object body,
    /// and `None` at the top level, where EOF terminates instead of `}`.
    fn members(&mut self, open: Option<Span>) -> Result<Map, Error> {
        let mut map = Map::new();
        loop {
            match self.tokens.get(self.pos) {
                None => {
                    return match open {
                        Some(open) => {
                            Err(Error::new(open, "unclosed `{`")
                                .with_help("expected a matching `}`"))
                        }
                        None => Ok(map),
                    };
                }
                Some(token) if matches!(token.kind, TokenKind::RBrace) => {
                    if open.is_none() {
                        return Err(Error::new(token.span, "unexpected `}`")
                            .with_help("there is no open `{` to close"));
                    }
                    self.pos += 1;
                    return Ok(map);
                }
                _ => {}
            }

            let key_span = self.tokens[self.pos].span;
            let key = match &self.tokens[self.pos].kind {
                TokenKind::Str(s) => s.clone(),
                TokenKind::Bareword => self.text(key_span).to_string(),
                kind => {
                    return Err(Error::new(
                        key_span,
                        format!("expected a key, found {}", kind.describe()),
                    ));
                }
            };
            self.pos += 1;

            // Report a missing value against its key. Blaming EOF is what makes an
            // unseparated syntax miserable to debug.
            let at_end = match self.tokens.get(self.pos) {
                None => true,
                Some(token) => open.is_some() && matches!(token.kind, TokenKind::RBrace),
            };
            if at_end {
                return Err(missing_value(key_span, &key));
            }

            let value = self.value()?;
            if !map.insert(key.clone(), value) {
                return Err(Error::new(key_span, format!("duplicate key `{key}`"))
                    .with_help("tot rejects duplicate keys rather than picking a winner"));
            }
        }
    }

    fn value(&mut self) -> Result<Value, Error> {
        let Some(token) = self.tokens.get(self.pos) else {
            let end = Span::new(self.src.len(), self.src.len());
            return Err(Error::new(end, "expected a value, found end of input"));
        };
        let span = token.span;
        // Cloning the kind keeps the borrow checker out of the way; string values are cloned
        // once, which is not worth optimizing at config-file sizes.
        match token.kind.clone() {
            TokenKind::Str(s) => {
                self.pos += 1;
                Ok(Value::String(s))
            }
            TokenKind::Bareword => match literal(self.text(span)) {
                Some(value) => {
                    self.pos += 1;
                    Ok(value)
                }
                None => Err(bad_value(span, self.text(span))),
            },
            TokenKind::LBrace => {
                self.pos += 1;
                self.enter(span)?;
                let map = self.members(Some(span))?;
                self.depth -= 1;
                Ok(Value::Object(map))
            }
            TokenKind::LBracket => {
                self.pos += 1;
                self.enter(span)?;
                let mut items = Vec::new();
                loop {
                    match self.tokens.get(self.pos) {
                        None => {
                            return Err(Error::new(span, "unclosed `[`")
                                .with_help("expected a matching `]`"));
                        }
                        Some(token) if matches!(token.kind, TokenKind::RBracket) => {
                            self.pos += 1;
                            break;
                        }
                        _ => items.push(self.value()?),
                    }
                }
                self.depth -= 1;
                Ok(Value::Array(items))
            }
            kind => Err(Error::new(
                span,
                format!("expected a value, found {}", kind.describe()),
            )),
        }
    }

    fn enter(&mut self, span: Span) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Error::new(
                span,
                format!("maximum nesting depth of {MAX_DEPTH} exceeded"),
            ));
        }
        Ok(())
    }
}

fn missing_value(span: Span, key: &str) -> Error {
    Error::new(span, format!("key `{key}` has no value"))
        .with_help("every key must be followed by a value")
}

fn bad_value(span: Span, text: &str) -> Error {
    // Grammatically a number, so the only way it reached here is by being out of range.
    if matches!(number_lexeme(text), Some(Value::Float(_))) {
        return Error::new(span, format!("`{text}` is outside the range of a float")).with_help(
            "tot has no infinity, so it has no way to write this; the largest finite \
                 value is about 1.8e308",
        );
    }
    if text.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.')) {
        Error::new(span, format!("`{text}` is not a valid number")).with_help(format!(
            "tot uses the JSON number grammar: no leading zeros, no leading `+`, no hex, no \
             underscores. Write `\"{text}\"` to keep it as a string"
        ))
    } else {
        Error::new(span, "expected a value; string values must be quoted")
            .with_help(format!("write `\"{text}\"`"))
    }
}

fn literal(text: &str) -> Option<Value> {
    match text {
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        "null" => Some(Value::Null),
        _ => number(text),
    }
}

/// Validates and classifies a number lexeme, rejecting a float no `f64` can hold.
///
/// An integer keeps its lexeme and so has no range limit, but a float has to denote a real
/// `f64` — tot has no way to write an infinity, so a lexeme that means one has no value in
/// the language. Letting `1e999` through would produce a document that parses and formats but
/// that no converter can write.
fn number(text: &str) -> Option<Value> {
    let value = number_lexeme(text)?;
    if let Value::Float(f) = &value
        && !f.as_str().parse::<f64>().is_ok_and(f64::is_finite)
    {
        return None;
    }
    Some(value)
}

/// The grammar alone, with no range check.
///
/// ```text
/// number = "-"? ( digits ("." [0-9]*)? | "." [0-9]+ ) ([eE] [+-]? [0-9]+)?
/// digits = "0" | [1-9][0-9]*
/// ```
///
/// A superset of the JSON grammar: `1.` and `.1` are also accepted. Leading zeros stay a
/// parse error so that `01234` has to be a string. The result is a [`Value::Float`] if there
/// was a `.` or an exponent and a [`Value::Integer`] otherwise.
fn number_lexeme(text: &str) -> Option<Value> {
    let b = text.as_bytes();
    let mut i = 0;
    let mut is_float = false;

    if b.first() == Some(&b'-') {
        i += 1;
    }

    let int_start = i;
    if b.get(i) == Some(&b'0') {
        i += 1;
        if b.get(i).is_some_and(u8::is_ascii_digit) {
            return None; // leading zero
        }
    } else {
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
    }
    let has_int = i > int_start;

    if b.get(i) == Some(&b'.') {
        is_float = true;
        i += 1;
        let frac_start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if !has_int && i == frac_start {
            return None; // a `.` needs digits on at least one side
        }
    } else if !has_int {
        return None;
    }

    if matches!(b.get(i), Some(b'e' | b'E')) {
        is_float = true;
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == exp_start {
            return None;
        }
    }

    if i != b.len() {
        return None;
    }
    Some(if is_float {
        Value::Float(Float::from_lexeme(text))
    } else {
        Value::Integer(Integer::from_lexeme(text))
    })
}
