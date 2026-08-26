//! Paths: naming one value inside a document.
//!
//! A path is **not** tot syntax. In a document `.` is an ordinary bareword character, so
//! `com.example.level` is one key rather than three nested ones; in a path it is the nesting
//! operator. A key containing a `.` — or anything else a bare key may not contain — is written
//! quoted, with the same escapes strings use.
//!
//! ```text
//! path    = step ( "." member | index )*
//! step    = member | index
//! member  = bare | STRING
//! bare    = ( any character legal in a bare key, except "." )+
//! index   = "[" [0-9]+ "]"
//! ```
//!
//! ```
//! let doc = tot::parse(r#"listen { host "0.0.0.0" ports [80 443] }"#).unwrap();
//! let path = tot::Path::parse("listen.ports[1]").unwrap();
//! assert_eq!(path.get(&doc).unwrap().as_integer().unwrap().as_str(), "443");
//! ```

use crate::error::{Error, Span};
use crate::lex::{can_be_bare, is_bareword_char, unescape_at};
use crate::value::{Map, Value, write_escaped};

/// One step of a path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// A member of an object.
    Key(String),
    /// An element of an array.
    Index(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    step: Step,
    /// Where this step sits in the path text, so a failure can point at it.
    span: Span,
}

/// A path to one value inside a document.
///
/// Parsing and lookup are separate because their failures are different kinds of problem: a
/// malformed path is the caller's mistake, while a path the document does not have is an
/// answer about the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Path {
    text: String,
    segments: Vec<Segment>,
}

impl Path {
    /// Parse a path.
    ///
    /// Spans in the returned error index into `text`, not into any document.
    ///
    /// ```
    /// assert!(tot::Path::parse("listen.port").is_ok());
    /// assert!(tot::Path::parse("listen.").is_err());
    /// ```
    pub fn parse(text: &str) -> Result<Self, Error> {
        let segments = Parser { src: text, pos: 0 }.run()?;
        Ok(Path {
            text: text.to_string(),
            segments,
        })
    }

    /// The path as it was written. Spans in errors from [`get`](Path::get) index into this.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Look this path up in a document.
    ///
    /// The error's span covers the segment that failed, so a caller that wants a caret can
    /// render it against [`text`](Path::text).
    ///
    /// ```
    /// let doc = tot::parse("listen { port 8080 }").unwrap();
    /// let path = tot::Path::parse("listen.prot").unwrap();
    /// let e = path.get(&doc).unwrap_err();
    /// assert_eq!(e.message, "no member `prot` in `listen`");
    /// ```
    pub fn get<'v>(&self, document: &'v Value) -> Result<&'v Value, Error> {
        let mut current = document;
        for (i, segment) in self.segments.iter().enumerate() {
            current = match (&segment.step, current) {
                (Step::Key(key), Value::Object(map)) => match map.get(key) {
                    Some(value) => value,
                    None => {
                        return Err(Error::new(
                            segment.span,
                            format!("no member `{key}` in {}", self.container(i)),
                        )
                        .with_help(members(map)));
                    }
                },
                (Step::Key(key), other) => {
                    return Err(Error::new(
                        segment.span,
                        format!(
                            "cannot look up `{key}`: {} is {}, not an object",
                            self.container(i),
                            kind(other)
                        ),
                    ));
                }
                (Step::Index(n), Value::Array(items)) => match items.get(*n) {
                    Some(value) => value,
                    None => {
                        return Err(Error::new(
                            segment.span,
                            format!(
                                "index {n} is out of range: {} has {}",
                                self.container(i),
                                count(items.len())
                            ),
                        ));
                    }
                },
                (Step::Index(n), other) => {
                    let error = Error::new(
                        segment.span,
                        format!(
                            "cannot take element {n} of {}: it is {}, not an array",
                            self.container(i),
                            kind(other)
                        ),
                    );
                    return Err(match other {
                        Value::Object(_) => {
                            error.with_help("an object is looked up by name, not by position")
                        }
                        _ => error,
                    });
                }
            };
        }
        Ok(current)
    }

    /// What the segment at `i` was looked up in, named the way the caller wrote it.
    fn container(&self, i: usize) -> String {
        match i.checked_sub(1) {
            None => "the document".to_string(),
            Some(prev) => format!("`{}`", &self.text[..self.segments[prev].span.end]),
        }
    }
}

/// The keys that were there, which is usually enough to spot a typo. Truncated, because a
/// generated document can have hundreds and the help line is not the place for them.
fn members(map: &Map) -> String {
    const SHOWN: usize = 8;
    if map.is_empty() {
        return "that object has no members".to_string();
    }
    let names: Vec<String> = map.keys().take(SHOWN).map(as_segment).collect();
    if map.len() > SHOWN {
        format!(
            "members include {} ({} in all)",
            names.join(", "),
            map.len()
        )
    } else {
        format!("members are {}", names.join(", "))
    }
}

/// Spells a key the way a path spells it, so a name offered as a suggestion can be typed
/// straight back. The keys that most need this — a `.` or a space in them — are exactly the
/// ones a reader would otherwise get wrong.
pub(crate) fn as_segment(key: &str) -> String {
    if can_be_bare(key) && !key.contains('.') {
        return key.to_string();
    }
    let mut out = String::from("\"");
    write_escaped(&mut out, key);
    out.push('"');
    out
}

fn count(len: usize) -> String {
    match len {
        0 => "no elements".to_string(),
        1 => "1 element".to_string(),
        n => format!("{n} elements"),
    }
}

/// Names a value's type for a message, with its article.
pub(crate) fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a float",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// A bare path segment takes any character a bare key takes, except the `.` that separates
/// segments. Sharing the predicate keeps the two spellings of a key from drifting apart.
fn is_segment_char(c: char) -> bool {
    is_bareword_char(c) && c != '.'
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn run(mut self) -> Result<Vec<Segment>, Error> {
        if self.src.is_empty() {
            return Err(Error::new(Span::new(0, 0), "empty path")
                .with_help("a path names one value, like `listen.port` or `regions[0]`"));
        }
        let first = match self.peek() {
            Some('[') => self.index()?,
            _ => self.member()?,
        };
        let mut segments = vec![first];
        loop {
            match self.peek() {
                None => return Ok(segments),
                Some('[') => segments.push(self.index()?),
                Some('.') => {
                    self.pos += 1;
                    segments.push(self.member()?);
                }
                Some(c) => {
                    return Err(Error::new(
                        Span::new(self.pos, self.pos + c.len_utf8()),
                        format!("unexpected `{c}` in path"),
                    )
                    .with_help("separate members with `.`, and index elements with `[n]`"));
                }
            }
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    /// The span of whatever comes next, for pointing at it.
    fn here(&self) -> Span {
        let end = self.peek().map_or(self.pos, |c| self.pos + c.len_utf8());
        Span::new(self.pos, end)
    }

    fn member(&mut self) -> Result<Segment, Error> {
        let start = self.pos;
        match self.peek() {
            Some('"') => {
                let key = self.string()?;
                Ok(Segment {
                    step: Step::Key(key),
                    span: Span::new(start, self.pos),
                })
            }
            Some(c) if is_segment_char(c) => {
                while let Some(c) = self.peek() {
                    if !is_segment_char(c) {
                        break;
                    }
                    self.pos += c.len_utf8();
                }
                Ok(Segment {
                    step: Step::Key(self.src[start..self.pos].to_string()),
                    span: Span::new(start, self.pos),
                })
            }
            found => {
                let error = Error::new(self.here(), "expected a member name");
                Err(match found {
                    Some('[') => {
                        error.with_help("an index attaches to what it indexes, with no `.`: `a[0]`")
                    }
                    _ => error.with_help(
                        "a member name is a bare key, or a quoted one if it contains `.`",
                    ),
                })
            }
        }
    }

    fn index(&mut self) -> Result<Segment, Error> {
        let start = self.pos;
        self.pos += 1; // `[`
        let digits_start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        let digits = &self.src[digits_start..self.pos];
        if digits.is_empty() {
            return Err(Error::new(
                Span::new(start, self.here().end),
                "expected an index after `[`",
            )
            .with_help("an index is a whole number from zero up, as in `[0]`"));
        }
        if self.peek() != Some(']') {
            return Err(
                Error::new(Span::new(start, self.here().end), "unclosed index")
                    .with_help("expected a `]`"),
            );
        }
        self.pos += 1;
        let index = digits.parse::<usize>().map_err(|_| {
            Error::new(
                Span::new(digits_start, digits_start + digits.len()),
                "index is too large",
            )
        })?;
        Ok(Segment {
            step: Step::Index(index),
            span: Span::new(start, self.pos),
        })
    }

    /// A quoted segment. Escapes are the language's, but the restrictions a document puts on
    /// string contents are about reading a document, so they are not repeated here.
    fn string(&mut self) -> Result<String, Error> {
        let start = self.pos;
        self.pos += 1; // `"`
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(
                    Error::new(Span::new(start, self.pos), "unterminated string in path")
                        .with_help("expected a closing `\"`"),
                );
            };
            match c {
                '"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                '\\' => unescape_at(self.src, &mut self.pos, &mut out)?,
                c => {
                    self.pos += c.len_utf8();
                    out.push(c);
                }
            }
        }
    }
}
