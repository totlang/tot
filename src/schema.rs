//! Checking a document against a shape.
//!
//! **A schema is a tot document that looks like the documents it describes**, with a type
//! where each value would be. That is the whole design: a schema you have to decode is worse
//! than no schema, and a shape you can read beside the config it governs is one you will keep
//! up to date.
//!
//! ```tot
//! name    "string"
//! version "int"
//! listen {
//!   host  "string"
//!   port  "int"
//!   tls?  "bool"          # `?` on the key: the member may be absent
//! }
//! regions ["string"]      # an array, every element a string
//! labels  {* "string"}    # `*`: any other key, with a string value
//! retries "int|null"
//! ```
//!
//! A type name is quoted because **a schema is tot, and in tot a bare word is never a value.**
//! That rule does not get suspended for schemas, and the quotes are what make a schema line up
//! with the document beside it: the same keys, in the same shape, with the values replaced.
//!
//! A type is `any`, `string`, `int`, `float`, `bool`, `null`, or several of them joined by
//! `|`. `{…}` describes an object and `[T]` an array. A member the schema does not name is an
//! error unless the object has a `*`, because catching a typo is most of what checking is for.
//!
//! ```
//! let schema = tot::Schema::parse(r#"port "int"  host? "string""#).unwrap();
//! assert!(schema.check("port 8080").unwrap().is_empty());
//!
//! let bad = schema.check(r#"port "8080""#).unwrap();
//! assert_eq!(bad[0].to_string(), "expected int, found a string at `port`");
//! ```

use std::collections::HashMap;
use std::fmt;

use crate::cst::{self, Body, Item, Node};
use crate::error::{Error, Span};
use crate::lex::{Dialect, Token};
use crate::path::{as_segment, kind};
use crate::value::{Map, Value};

/// A compiled schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    root: Type,
}

/// One thing wrong with a document.
///
/// The location is a path rather than a span, because that is what a shape mismatch has: a
/// missing member is nowhere in the text at all. Where the document does have somewhere to
/// point, [`span`](Violation::span) is set as well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Where in the document, spelled the way [`Path`](crate::Path) spells it, so it can be
    /// handed straight to `tot get`. Empty at the root.
    pub path: String,
    /// What is wrong.
    pub message: String,
    /// What to do about it.
    pub help: Option<String>,
    /// The key this is about, when the document has one — absent for a missing member.
    pub span: Option<Span>,
}

impl Violation {
    fn new(path: &str, message: impl Into<String>) -> Self {
        Violation {
            path: path.to_string(),
            message: message.into(),
            help: None,
            span: None,
        }
    }

    fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Render a caret diagnostic, or a single line where there is nothing to point at.
    pub fn render(&self, src: &str) -> String {
        match self.span {
            Some(span) => {
                crate::error::render("error", span, &self.to_string(), self.help.as_deref(), src)
            }
            None => match &self.help {
                Some(help) => format!("error: {self} (help: {help})\n"),
                None => format!("error: {self}\n"),
            },
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if !self.path.is_empty() {
            write!(f, " at `{}`", self.path)?;
        }
        Ok(())
    }
}

impl Schema {
    /// Read a schema from a tot document.
    ///
    /// A schema that does not parse, or that parses but is not a schema, is an [`Error`]. The
    /// second kind carries the span of the key it went wrong at, so both read the same.
    pub fn parse(src: &str) -> Result<Self, Error> {
        // Both walks read the same tokens, so the source is lexed once.
        let tokens = crate::lex::tokenize(src, Dialect::Data)?;
        let value = crate::parse::from_tokens(src, &tokens)?;
        Type::compile(&value, "")
            .map(|root| Schema { root })
            .map_err(|violation| {
                let spans = key_spans(src, &tokens);
                let span = spans
                    .get(&violation.path)
                    .copied()
                    .unwrap_or(Span::new(0, 0));
                let error = Error::new(span, violation.to_string());
                match violation.help {
                    Some(help) => error.with_help(help),
                    None => error,
                }
            })
    }

    /// Check a document, returning everything wrong with it.
    ///
    /// Every violation is reported, not just the first: a checker that stops at one turns a
    /// single pass into a guessing game.
    pub fn check(&self, src: &str) -> Result<Vec<Violation>, Error> {
        // Both walks read the same tokens, so the source is lexed once.
        let tokens = crate::lex::tokenize(src, Dialect::Data)?;
        let document = crate::parse::from_tokens(src, &tokens)?;
        let mut violations = self.check_value(&document);
        if !violations.is_empty() {
            let spans = key_spans(src, &tokens);
            for violation in &mut violations {
                violation.span = spans.get(&violation.path).copied();
            }
        }
        Ok(violations)
    }

    /// Check an already-parsed document. Violations carry no span, there being no source.
    pub fn check_value(&self, document: &Value) -> Vec<Violation> {
        let mut violations = Vec::new();
        self.root.check(document, "", &mut violations);
        violations
    }
}

/// What a value is allowed to be.
#[derive(Debug, Clone, PartialEq)]
enum Type {
    /// Anything at all, including a collection.
    Any,
    /// One of these scalar kinds. Never empty.
    Scalars(Vec<Scalar>),
    /// An array whose every element matches.
    Array(Box<Type>),
    Object(Fields),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scalar {
    String,
    Int,
    Float,
    Bool,
    Null,
}

impl Scalar {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "string" => Some(Scalar::String),
            "int" => Some(Scalar::Int),
            "float" => Some(Scalar::Float),
            "bool" => Some(Scalar::Bool),
            "null" => Some(Scalar::Null),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Scalar::String => "string",
            Scalar::Int => "int",
            Scalar::Float => "float",
            Scalar::Bool => "bool",
            Scalar::Null => "null",
        }
    }

    fn matches(self, value: &Value) -> bool {
        matches!(
            (self, value),
            (Scalar::String, Value::String(_))
                | (Scalar::Int, Value::Integer(_))
                | (Scalar::Float, Value::Float(_))
                | (Scalar::Bool, Value::Bool(_))
                | (Scalar::Null, Value::Null)
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Fields {
    /// Declared members, in the order the schema wrote them.
    members: Vec<Field>,
    /// What a key the schema did not name must match, from a `*` member. `None` makes an
    /// undeclared member an error.
    rest: Option<Box<Type>>,
}

#[derive(Debug, Clone, PartialEq)]
struct Field {
    name: String,
    optional: bool,
    ty: Type,
}

impl Type {
    /// Reads one schema value. `path` is where it sits in the schema, for the diagnostic.
    fn compile(value: &Value, path: &str) -> Result<Type, Violation> {
        match value {
            Value::String(name) => Type::names(name, path),
            Value::Array(items) => match items.as_slice() {
                [element] => Ok(Type::Array(Box::new(Type::compile(
                    element,
                    &join(path, "[0]"),
                )?))),
                _ => Err(Violation::new(
                    path,
                    format!(
                        "an array schema describes its element type, so it needs exactly one, \
                         not {}",
                        items.len()
                    ),
                )
                .with_help("write `[string]` for an array of strings, or `[any]` for any array")),
            },
            Value::Object(map) => Type::fields(map, path).map(Type::Object),
            other => Err(Violation::new(
                path,
                format!(
                    "a schema says what a value may be, and {} is a value",
                    kind(other)
                ),
            )
            .with_help("write a type: any, string, int, float, bool, null, `{…}`, or `[…]`")),
        }
    }

    /// A bareword type: one name, or several joined by `|`.
    fn names(text: &str, path: &str) -> Result<Type, Violation> {
        let mut scalars = Vec::new();
        let mut any = false;
        // Every alternative is read, `any` included. Returning as soon as `any` turned up
        // would make a typo after it legal while the same typo before it was an error, so
        // whether a schema was checked would depend on the order its author happened to
        // write the union in.
        for name in text.split('|') {
            if name == "any" {
                if any {
                    return Err(Violation::new(path, "`any` is listed twice"));
                }
                any = true;
                continue;
            }
            match Scalar::parse(name) {
                Some(scalar) if scalars.contains(&scalar) => {
                    return Err(Violation::new(path, format!("`{name}` is listed twice")));
                }
                Some(scalar) => scalars.push(scalar),
                // An empty alternative has no name to quote, so naming the `|` is the only
                // way to say what went wrong.
                None if name.is_empty() => {
                    return Err(Violation::new(path, "a `|` needs a type on both sides")
                        .with_help("the types are any, string, int, float, bool, and null"));
                }
                None => {
                    return Err(
                        Violation::new(path, format!("`{name}` is not a type")).with_help(
                            "the types are any, string, int, float, bool, and null; join them \
                         with `|`",
                        ),
                    );
                }
            }
        }
        // `any` swallows every alternative beside it — which is why they are checked first
        // rather than skipped. `Scalars` is never empty: the only way to collect nothing is
        // for every alternative to have been `any`.
        Ok(if any {
            Type::Any
        } else {
            Type::Scalars(scalars)
        })
    }

    fn fields(map: &Map, path: &str) -> Result<Fields, Violation> {
        let mut members: Vec<Field> = Vec::new();
        let mut rest = None;
        for (key, value) in map.iter() {
            // `*` stands for every key the schema does not name, so it is not a member.
            if key == "*" {
                rest = Some(Box::new(Type::compile(value, &join(path, "*"))?));
                continue;
            }
            // A catch-all already covers no key at all when the document has none, so there
            // is nothing for `?` to say. Left alone, `*?` would fall through below and
            // quietly declare a member named `*` — the one thing a schema cannot describe.
            if key == "*?" {
                return Err(Violation::new(
                    &join(path, &as_segment(key)),
                    "`*?` is not a member name",
                )
                .with_help("write `*`, which covers every other key and is satisfied by none"));
            }
            if let Some(name) = key.strip_suffix('*') {
                return Err(Violation::new(
                    &join(path, &as_segment(key)),
                    format!("`{key}` is not a member name"),
                )
                .with_help(format!(
                    "a bare `*` covers every other key; write `{name}` to name one"
                )));
            }
            let (name, optional) = match key.strip_suffix('?') {
                Some(name) => (name, true),
                None => (key, false),
            };
            // `a` and `a?` are different keys, so the language's duplicate-key rule does not
            // see this one. Left in, the member would be checked twice against two types that
            // cannot both hold, and only one of them would be reported.
            if members.iter().any(|field| field.name == name) {
                return Err(Violation::new(
                    &join(path, &as_segment(key)),
                    format!("member `{name}` is declared twice"),
                )
                .with_help("`?` goes on the key, so `a` and `a?` name the same member"));
            }
            members.push(Field {
                name: name.to_string(),
                optional,
                // Spelled with the key as the schema wrote it, `?` and all: this path is
                // looked up in the schema's own spans, and `tls` would not find `tls?`.
                // `Fields::check` builds the document's path from `name` instead.
                ty: Type::compile(value, &join(path, &as_segment(key)))?,
            });
        }
        Ok(Fields { members, rest })
    }

    /// Describes what this type accepts, for a message.
    fn describe(&self) -> String {
        match self {
            Type::Any => "any value".to_string(),
            Type::Scalars(scalars) => scalars
                .iter()
                .map(|s| s.name())
                .collect::<Vec<_>>()
                .join(" or "),
            Type::Array(_) => "an array".to_string(),
            Type::Object(_) => "an object".to_string(),
        }
    }

    fn check(&self, value: &Value, path: &str, out: &mut Vec<Violation>) {
        match self {
            Type::Any => {}
            Type::Scalars(scalars) => {
                if !scalars.iter().any(|s| s.matches(value)) {
                    out.push(Violation::new(
                        path,
                        format!("expected {}, found {}", self.describe(), kind(value)),
                    ));
                }
            }
            Type::Array(element) => match value {
                Value::Array(items) => {
                    for (i, item) in items.iter().enumerate() {
                        element.check(item, &join(path, &format!("[{i}]")), out);
                    }
                }
                other => out.push(Violation::new(
                    path,
                    format!("expected an array, found {}", kind(other)),
                )),
            },
            Type::Object(fields) => match value {
                Value::Object(map) => fields.check(map, path, out),
                other => out.push(Violation::new(
                    path,
                    format!("expected an object, found {}", kind(other)),
                )),
            },
        }
    }
}

impl Fields {
    fn check(&self, map: &Map, path: &str, out: &mut Vec<Violation>) {
        for field in &self.members {
            match map.get(&field.name) {
                Some(value) => field
                    .ty
                    .check(value, &join(path, &as_segment(&field.name)), out),
                None if field.optional => {}
                None => out.push(Violation::new(
                    path,
                    format!("missing member `{}`", field.name),
                )),
            }
        }

        for (key, value) in map.iter() {
            if self.members.iter().any(|field| field.name == key) {
                continue;
            }
            let where_ = join(path, &as_segment(key));
            match &self.rest {
                Some(ty) => ty.check(value, &where_, out),
                None => out.push(
                    Violation::new(&where_, format!("unknown member `{key}`"))
                        .with_help(self.expected()),
                ),
            }
        }
    }

    /// The members the schema does name, which is usually enough to spot the typo.
    fn expected(&self) -> String {
        const SHOWN: usize = 8;
        if self.members.is_empty() {
            return "the schema declares no members here".to_string();
        }
        let names: Vec<String> = self
            .members
            .iter()
            .take(SHOWN)
            .map(|field| as_segment(&field.name))
            .collect();
        if self.members.len() > SHOWN {
            format!(
                "the schema has {} ({} in all)",
                names.join(", "),
                self.members.len()
            )
        } else {
            format!("the schema has {}", names.join(", "))
        }
    }
}

fn join(path: &str, segment: &str) -> String {
    if path.is_empty() {
        segment.to_string()
    } else if segment.starts_with('[') {
        format!("{path}{segment}")
    } else {
        format!("{path}.{segment}")
    }
}

/// Maps each member's path to the span of its key.
///
/// A violation is found in the value tree, which has no spans, so this is how one gets back
/// to the text. Built only when something is already wrong, since it costs a second walk.
///
/// The caller has already parsed these tokens, so the tree is known to be well formed; a
/// document with no spans to offer is one where every violation renders as a plain line,
/// which is what happens anyway for a violation with nowhere to point.
fn key_spans(src: &str, tokens: &[Token]) -> HashMap<String, Span> {
    let mut spans = HashMap::new();
    let Ok(document) = cst::from_tokens(src, tokens, Dialect::Data) else {
        return spans;
    };
    match &document.body {
        Body::Members(items) => index(items, "", &mut spans),
        Body::Value(node) => index_node(node, "", &mut spans),
    }
    spans
}

fn index(items: &[Item<'_>], path: &str, spans: &mut HashMap<String, Span>) {
    for (i, item) in items.iter().enumerate() {
        let child = match &item.key {
            Some(key) => {
                let child = join(path, &as_segment(&key.name));
                spans.insert(child.clone(), key.span);
                child
            }
            None => join(path, &format!("[{i}]")),
        };
        index_node(&item.value, &child, spans);
    }
}

fn index_node(node: &Node<'_>, path: &str, spans: &mut HashMap<String, Span>) {
    match node {
        Node::Scalar(_) => {}
        Node::Object(collection) | Node::Array(collection) => {
            index(&collection.items, path, spans);
        }
        // A schema and the documents it describes are both `.tot`, so there are no forms here.
        Node::Form(_) => {}
    }
}
