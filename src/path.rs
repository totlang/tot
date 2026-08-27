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
use crate::lex::{Dialect, can_be_bare, unescape_at};
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

    /// Spells one key the way a path spells it.
    ///
    /// A message that names a key should name it in a form the reader can type straight back,
    /// and the keys that most need this — one holding a `.` or a space — are exactly the ones
    /// a reader would otherwise get wrong. Anything building a path out of keys should go
    /// through here rather than joining them with `.` itself.
    ///
    /// ```
    /// assert_eq!(tot::Path::segment("port"), "port");
    /// assert_eq!(tot::Path::segment("com.example"), r#""com.example""#);
    /// assert!(tot::Path::parse(&tot::Path::segment("log level")).is_ok());
    /// ```
    pub fn segment(key: &str) -> String {
        as_segment(key)
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
        match self.resolve(document) {
            Found::Value(value) => Ok(value),
            Found::Absent(error) | Found::Wrong(error) => Err(error),
        }
    }

    /// Look this path up, telling *nothing there* apart from *the wrong shape*.
    ///
    /// `Ok(None)` is a member the object does not have or an index past the end of an array:
    /// the two ways a document can simply not answer. `Err` is a step that ran into the wrong
    /// **kind** of value, which is a different problem — the document is not shaped the way
    /// the caller thought, and a caller with a fallback should not quietly use it for that.
    ///
    /// ```
    /// let doc = tot::parse("listen {port 8080}").unwrap();
    ///
    /// // Absent: the object simply has no `tls`.
    /// assert!(tot::Path::parse("listen.tls").unwrap().find(&doc).unwrap().is_none());
    ///
    /// // Wrong shape: `port` is an integer, so looking inside it is a mistake, not a miss.
    /// assert!(tot::Path::parse("listen.port.x").unwrap().find(&doc).is_err());
    /// ```
    pub fn find<'v>(&self, document: &'v Value) -> Result<Option<&'v Value>, Error> {
        match self.resolve(document) {
            Found::Value(value) => Ok(Some(value)),
            Found::Absent(_) => Ok(None),
            Found::Wrong(error) => Err(error),
        }
    }

    /// The one walk both of the above read, so they cannot drift apart on what they find or on
    /// what they call it.
    fn resolve<'v>(&self, document: &'v Value) -> Found<'v> {
        let mut current = document;
        for (i, segment) in self.segments.iter().enumerate() {
            current = match (&segment.step, current) {
                (Step::Key(key), Value::Object(map)) => match map.get(key) {
                    Some(value) => value,
                    None => return Found::Absent(self.no_member(i, key, map)),
                },
                (Step::Key(key), other) => return Found::Wrong(self.not_an_object(i, key, other)),
                (Step::Index(n), Value::Array(items)) => match items.get(*n) {
                    Some(value) => value,
                    None => return Found::Absent(self.out_of_range(i, *n, items.len())),
                },
                (Step::Index(n), other) => return Found::Wrong(self.not_an_array(i, *n, other)),
            };
        }
        Found::Value(current)
    }

    /// The value at this path, for changing it in place.
    ///
    /// Every segment has to be there already. Use [`set`](Path::set) to put a value somewhere
    /// the document does not have yet.
    pub fn get_mut<'v>(&self, document: &'v mut Value) -> Result<&'v mut Value, Error> {
        self.walk(document, self.segments.len(), Missing::Reject)
    }

    /// Replaces the value at this path.
    ///
    /// **The last segment does not have to exist** — adding a member is what setting is for.
    /// Every segment before it does, unless `missing` is [`Missing::Create`]; a mistyped path
    /// should not quietly build a branch nobody asked for.
    ///
    /// An array element is never created, under either setting. The index would have to be
    /// in range already, and there is no answer to what would fill the gap if it were not.
    ///
    /// **A failed `set` leaves the document alone.** The whole path is checked before
    /// anything is written, so `Missing::Create` cannot half-build a branch and then report
    /// an error against the document it just changed.
    ///
    /// ```
    /// let mut doc = tot::parse("listen {port 8080}").unwrap();
    /// let value = tot::Value::Bool(true);
    ///
    /// tot::Path::parse("listen.tls").unwrap()
    ///     .set(&mut doc, value, tot::Missing::Reject)
    ///     .unwrap();
    /// assert_eq!(tot::format_value(&doc), "listen {\n  port 8080\n  tls true\n}\n");
    /// ```
    pub fn set(&self, document: &mut Value, value: Value, missing: Missing) -> Result<(), Error> {
        // Nothing is written until the whole path is known to work. `walk` builds objects as
        // it descends, so a failure further along would otherwise leave behind the branch it
        // had already made — a document changed by a call that reported it had failed.
        self.preflight(document, missing)?;

        // A path always has at least one segment; `parse` rejects an empty one.
        let last = self.segments.len() - 1;
        let parent = self.walk(document, last, missing)?;

        match (&self.segments[last].step, parent) {
            (Step::Key(key), Value::Object(map)) => {
                // `insert` refuses a key that is already there, so an existing member is
                // replaced through its slot, which also keeps its position.
                match map.get_mut(key) {
                    Some(slot) => *slot = value,
                    None => {
                        map.insert(key.clone(), value);
                    }
                }
                Ok(())
            }
            (Step::Key(key), other) => Err(self.not_an_object(last, key, other)),
            (Step::Index(n), Value::Array(items)) => {
                let len = items.len();
                match items.get_mut(*n) {
                    Some(slot) => {
                        *slot = value;
                        Ok(())
                    }
                    None => Err(self.out_of_range(last, *n, len)),
                }
            }
            (Step::Index(n), other) => Err(self.not_an_array(last, *n, other)),
        }
    }

    /// Decides whether [`set`](Path::set) will succeed, without touching the document.
    ///
    /// This walks the same steps as [`walk`](Path::walk) and reports the same four failures,
    /// reading only. Once a member is missing, everything below it is an object this call
    /// would be creating, which is tracked as `None` rather than by making one.
    fn preflight(&self, document: &Value, missing: Missing) -> Result<(), Error> {
        let last = self.segments.len() - 1;
        let mut current = Some(document);
        for (i, segment) in self.segments.iter().enumerate() {
            let Some(value) = current else {
                // Inside something that does not exist yet. A key lands in a fresh object;
                // an index never can, and the last step is no different.
                if let Step::Index(n) = &segment.step {
                    return Err(self.not_an_array(i, *n, &Value::Object(Map::new())));
                }
                continue;
            };
            current = match (&segment.step, value) {
                (Step::Key(key), Value::Object(map)) => match map.get(key) {
                    Some(next) => Some(next),
                    // The last step may name something new — that is what setting is for.
                    None if i == last => return Ok(()),
                    None if missing == Missing::Create => None,
                    None => return Err(self.no_member(i, key, map)),
                },
                (Step::Key(key), other) => return Err(self.not_an_object(i, key, other)),
                (Step::Index(n), Value::Array(items)) => match items.get(*n) {
                    Some(next) => Some(next),
                    None => return Err(self.out_of_range(i, *n, items.len())),
                },
                (Step::Index(n), other) => return Err(self.not_an_array(i, *n, other)),
            };
        }
        Ok(())
    }

    /// Resolves the first `upto` segments, mutably, for the two callers above.
    fn walk<'v>(
        &self,
        document: &'v mut Value,
        upto: usize,
        missing: Missing,
    ) -> Result<&'v mut Value, Error> {
        let mut current = document;
        for (i, segment) in self.segments[..upto].iter().enumerate() {
            current = match (&segment.step, current) {
                (Step::Key(key), Value::Object(map)) => {
                    if !map.contains_key(key) {
                        if missing == Missing::Reject {
                            return Err(self.no_member(i, key, map));
                        }
                        map.insert(key.clone(), Value::Object(Map::new()));
                    }
                    map.get_mut(key).expect("present, or just added")
                }
                (Step::Key(key), other) => return Err(self.not_an_object(i, key, other)),
                (Step::Index(n), Value::Array(items)) => {
                    // An index is never filled in: `Missing::Create` makes objects, because
                    // there is no sensible value to pad an array with.
                    let len = items.len();
                    match items.get_mut(*n) {
                        Some(value) => value,
                        None => return Err(self.out_of_range(i, *n, len)),
                    }
                }
                (Step::Index(n), other) => return Err(self.not_an_array(i, *n, other)),
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

    // The four ways a path can fail to resolve. Shared so that reading and writing cannot
    // drift apart on what they call the same problem.

    fn no_member(&self, i: usize, key: &str, map: &Map) -> Error {
        Error::new(
            self.segments[i].span,
            format!("no member `{key}` in {}", self.container(i)),
        )
        .with_help(members(map))
    }

    fn not_an_object(&self, i: usize, key: &str, found: &Value) -> Error {
        Error::new(
            self.segments[i].span,
            format!(
                "cannot look up `{key}`: {} is {}, not an object",
                self.container(i),
                kind(found)
            ),
        )
    }

    fn out_of_range(&self, i: usize, n: usize, len: usize) -> Error {
        Error::new(
            self.segments[i].span,
            format!(
                "index {n} is out of range: {} has {}",
                self.container(i),
                count(len)
            ),
        )
    }

    fn not_an_array(&self, i: usize, n: usize, found: &Value) -> Error {
        let error = Error::new(
            self.segments[i].span,
            format!(
                "cannot take element {n} of {}: it is {}, not an array",
                self.container(i),
                kind(found)
            ),
        );
        match found {
            Value::Object(_) => error.with_help("an object is looked up by name, not by position"),
            _ => error,
        }
    }
}

/// What a lookup ran into, keeping the diagnostic either way.
enum Found<'v> {
    Value(&'v Value),
    /// Nothing at this path, which is a fact about the document.
    Absent(Error),
    /// A step hit the wrong kind of value, which is a fact about the path.
    Wrong(Error),
}

/// Whether [`Path::set`] may build the objects on the way to its destination.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Missing {
    /// A member that is not there is an error. This is the default because a mistyped path is
    /// far more likely than a genuinely missing branch, and the typo is invisible if it
    /// silently succeeds.
    #[default]
    Reject,
    /// Objects along the way are created as needed. Nothing that *is* there is replaced: a
    /// scalar where an object is needed is still an error, since overwriting it would be
    /// throwing away a value nobody asked to lose.
    Create,
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
    // A path names a value in a `.tot` document, so it spells keys the way `.tot` does.
    if can_be_bare(key, Dialect::Data) && !key.contains('.') {
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
    // A path names a value inside a `.tot` document, so it spells keys the way `.tot` does:
    // a paren is an ordinary character here even when the document was built from a template.
    Dialect::Data.allows_bare(c) && c != '.'
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
