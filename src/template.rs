//! Templates: `.tott` files, which are built into `.tot` documents.
//!
//! **The data language does not change.** A `.tot` file still denotes a value you can see by
//! reading it, and no consumer of tot has to become an evaluator. Everything here happens in
//! a separate file type, at build time, and what gets committed and read is ordinary tot.
//!
//! ```tott
//! name     "example-service"
//! replicas (if (param "prod") 5 1)
//! image    (str "registry.example/" (param "name") ":" (param "tag"))
//! regions  (import "regions.tot")
//! ```
//!
//! A form is `(head arg…)`, evaluated once and replaced by its value. Parens are the sigil
//! because **parens never appear in data**, so computation is distinguishable from data by
//! looking — the same reason a `(str …)` form is preferred to `"${name}"` interpolation, which
//! would make every string potentially computed and force a reader to scan for it.
//!
//! There are four forms and no way to define a fifth. That is the discipline the whole design
//! rests on: the moment a template file can define a function, people write libraries, and a
//! configuration file becomes a program that has to be read as one.
//!
//! | form | |
//! |---|---|
//! | `(param "name")` | a build parameter, or an error if it was not set |
//! | `(param "name" default)` | …or `default` if it was not set |
//! | `(if cond then else)` | `cond` must be a boolean; only the branch taken is evaluated |
//! | `(str a b …)` | joins strings and numbers into one string |
//! | `(import "file")` | that file's value, read relative to the file importing it |
//!
//! ```
//! use tot::template::{Params, Template};
//!
//! let template = Template::parse(r#"replicas (if (param "prod") 5 1)"#).unwrap();
//!
//! let mut params = Params::new();
//! params.set("prod", tot::Value::Bool(true));
//!
//! let built = template.evaluate(&params).unwrap();
//! assert_eq!(tot::format_value(&built), "replicas 5\n");
//! ```

use std::collections::HashMap;

use crate::error::{Error, Span};
use crate::lex::{Dialect, Token, TokenKind, tokenize};
use crate::parse::{MAX_DEPTH, bad_value, literal, lone_bareword, missing_value};
use crate::path::kind;
use crate::value::{Map, Value};

/// How deep imports may nest. A cycle is caught by name and reported as one; this is the
/// backstop for a chain that is merely absurd.
const MAX_IMPORTS: usize = 32;

// --- the tree ---------------------------------------------------------------------------------

/// A parsed template.
///
/// It carries its own source and name so that a failure anywhere in a build — including inside
/// a file three imports deep — can still draw a caret in the right place.
#[derive(Debug, Clone)]
pub struct Template {
    root: Node,
    name: String,
    source: String,
}

/// One node of a template, and where it was written.
#[derive(Debug, Clone)]
struct Node {
    kind: Kind,
    span: Span,
}

#[derive(Debug, Clone)]
enum Kind {
    /// A subtree with no form anywhere inside it. Collapsing these is what makes evaluating
    /// the untouched parts of a document a clone rather than a walk.
    Literal(Value),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
    Form(Form),
}

#[derive(Debug, Clone)]
struct Form {
    head: Head,
    args: Vec<Node>,
}

/// The forms. There are four, and adding a fifth should be a deliberate decision rather than
/// a convenience.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Head {
    Param,
    If,
    Str,
    Import,
}

impl Head {
    fn parse(name: &str) -> Option<Head> {
        match name {
            "param" => Some(Head::Param),
            "if" => Some(Head::If),
            "str" => Some(Head::Str),
            "import" => Some(Head::Import),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Head::Param => "param",
            Head::If => "if",
            Head::Str => "str",
            Head::Import => "import",
        }
    }

    /// The number of arguments allowed, as `(least, most)`. `None` is variadic.
    fn arity(self) -> (usize, Option<usize>) {
        match self {
            Head::Param => (1, Some(2)),
            Head::If => (3, Some(3)),
            Head::Str => (0, None),
            Head::Import => (1, Some(1)),
        }
    }

    fn usage(self) -> &'static str {
        match self {
            Head::Param => "write `(param \"name\")`, or `(param \"name\" default)`",
            Head::If => {
                "write `(if condition then else)`; both branches are required, \
                         because a form is replaced by its value and there is no value for \
                         a branch that was not written"
            }
            Head::Str => "write `(str a b …)`",
            Head::Import => "write `(import \"file.tot\")`",
        }
    }

    /// Whether this form's first argument has to be written down rather than computed.
    ///
    /// A parameter's name and an import's path are both statically visible on purpose: it is
    /// what lets a reader see which parameters a template needs and which files it pulls in,
    /// without running it.
    fn first_arg_is_literal(self) -> bool {
        matches!(self, Head::Param | Head::Import)
    }
}

// --- parsing ----------------------------------------------------------------------------------

impl Template {
    /// Parse a template.
    ///
    /// ```
    /// assert!(tot::template::Template::parse(r#"a (str "x" 1)"#).is_ok());
    /// assert!(tot::template::Template::parse("a (nope)").is_err());
    /// ```
    pub fn parse(src: &str) -> Result<Template, Error> {
        Template::parse_named(src, "<template>")
    }

    /// Parse a template, naming the file it came from for diagnostics.
    pub fn parse_named(src: &str, name: &str) -> Result<Template, Error> {
        let tokens = tokenize(src, Dialect::Template)?;
        let root = Parser {
            src,
            tokens: &tokens,
            pos: 0,
            depth: 0,
        }
        .document()?;
        Ok(Template {
            root,
            name: name.to_string(),
            source: src.to_string(),
        })
    }

    /// What this template is called in diagnostics.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether this template has no forms in it, and so is already a document.
    pub fn is_data(&self) -> bool {
        matches!(self.root.kind, Kind::Literal(_))
    }
}

/// The template grammar is the data grammar plus one production — a form, wherever a value
/// goes. It is a separate walk rather than a mode on [`crate::parse`] because it builds a
/// different tree: a `Value` is data, and a form is not, so a form must not be able to appear
/// in one. The pieces below the tree shape — the number grammar, escapes, and the two
/// diagnostics for a bareword in value position — are shared.
struct Parser<'a, 't> {
    src: &'a str,
    tokens: &'t [Token],
    pos: usize,
    depth: usize,
}

impl<'a> Parser<'a, '_> {
    fn document(&mut self) -> Result<Node, Error> {
        if self.tokens.is_empty() {
            return Ok(Node {
                kind: Kind::Literal(Value::Object(Map::new())),
                span: Span::new(0, 0),
            });
        }

        // A lone bareword in a file is most likely a key that lost its value, in a template
        // exactly as in a document.
        if self.tokens.len() == 1 && self.tokens[0].kind == TokenKind::Bareword {
            let span = self.tokens[0].span;
            return match literal(self.text(span)) {
                Some(value) => Ok(Node {
                    kind: Kind::Literal(value),
                    span,
                }),
                None => Err(lone_bareword(span, self.text(span))),
            };
        }

        // `{`, `[`, and now `(` can never begin a key, so a document that opens with one is
        // that value. A lone token is a scalar document, as in `.tot`.
        let opens_a_value = matches!(
            self.tokens[0].kind,
            TokenKind::LBrace | TokenKind::LBracket | TokenKind::LParen
        );
        if opens_a_value || self.tokens.len() == 1 {
            let is_form = self.tokens[0].kind == TokenKind::LParen;
            let node = self.value()?;
            if let Some(token) = self.tokens.get(self.pos) {
                // A form with a value after it is someone reaching for a computed key, or for
                // splicing an import's members into the document. Neither is a thing, and
                // saying so beats reporting the value that followed as a surprise.
                if is_form {
                    return Err(Error::new(node.span, "a form cannot be a key").with_help(
                        "a form goes where a value goes; splicing one document's members into \
                         another is what `tot merge` is for",
                    ));
                }
                return Err(Error::new(
                    token.span,
                    format!(
                        "unexpected {} after the top-level value",
                        token.kind.describe()
                    ),
                ));
            }
            return Ok(node);
        }

        let start = self.tokens[0].span.start;
        let members = self.members(None)?;
        Ok(object(members, Span::new(start, self.src.len())))
    }

    fn text(&self, span: Span) -> &'a str {
        &self.src[span.start..span.end]
    }

    fn members(&mut self, open: Option<Span>) -> Result<Vec<(String, Node)>, Error> {
        let mut members: Vec<(String, Node)> = Vec::new();
        loop {
            match self.tokens.get(self.pos) {
                None => {
                    return match open {
                        Some(open) => {
                            Err(Error::new(open, "unclosed `{`")
                                .with_help("expected a matching `}`"))
                        }
                        None => Ok(members),
                    };
                }
                Some(token) if matches!(token.kind, TokenKind::RBrace) => {
                    if open.is_none() {
                        return Err(Error::new(token.span, "unexpected `}`")
                            .with_help("there is no open `{` to close"));
                    }
                    self.pos += 1;
                    return Ok(members);
                }
                _ => {}
            }

            let key_span = self.tokens[self.pos].span;
            let key = match &self.tokens[self.pos].kind {
                TokenKind::Str(s) => s.clone(),
                TokenKind::Bareword => self.text(key_span).to_string(),
                // A computed key would make the shape of a document depend on evaluating it,
                // and the shape is what a reader most needs to see without running anything.
                TokenKind::LParen => {
                    return Err(Error::new(key_span, "a form cannot be a key")
                        .with_help("keys are written down; a form goes where a value goes"));
                }
                kind => {
                    return Err(Error::new(
                        key_span,
                        format!("expected a key, found {}", kind.describe()),
                    ));
                }
            };
            self.pos += 1;

            let at_end = match self.tokens.get(self.pos) {
                None => true,
                Some(token) => open.is_some() && matches!(token.kind, TokenKind::RBrace),
            };
            if at_end {
                return Err(missing_value(key_span, &key));
            }

            let value = self.value()?;
            if members.iter().any(|(name, _)| *name == key) {
                return Err(Error::new(key_span, format!("duplicate key `{key}`"))
                    .with_help("tot rejects duplicate keys rather than picking a winner"));
            }
            members.push((key, value));
        }
    }

    fn value(&mut self) -> Result<Node, Error> {
        let Some(token) = self.tokens.get(self.pos) else {
            let end = Span::new(self.src.len(), self.src.len());
            return Err(Error::new(end, "expected a value, found end of input"));
        };
        let span = token.span;
        match token.kind.clone() {
            TokenKind::Str(s) => {
                self.pos += 1;
                Ok(Node {
                    kind: Kind::Literal(Value::String(s)),
                    span,
                })
            }
            TokenKind::Bareword => match literal(self.text(span)) {
                Some(value) => {
                    self.pos += 1;
                    Ok(Node {
                        kind: Kind::Literal(value),
                        span,
                    })
                }
                None => Err(bad_value(span, self.text(span))),
            },
            TokenKind::LBrace => {
                self.pos += 1;
                self.enter(span)?;
                let members = self.members(Some(span))?;
                self.depth -= 1;
                let end = self.tokens[self.pos - 1].span.end;
                Ok(object(members, Span::new(span.start, end)))
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
                let end = self.tokens[self.pos - 1].span.end;
                Ok(array(items, Span::new(span.start, end)))
            }
            TokenKind::LParen => {
                self.enter(span)?;
                let node = self.form(span)?;
                self.depth -= 1;
                Ok(node)
            }
            kind => Err(Error::new(
                span,
                format!("expected a value, found {}", kind.describe()),
            )),
        }
    }

    /// A `(head arg…)` form. The current token is the `(`.
    fn form(&mut self, open: Span) -> Result<Node, Error> {
        self.pos += 1;

        let Some(token) = self.tokens.get(self.pos) else {
            return Err(Error::new(open, "unclosed `(`").with_help("expected a matching `)`"));
        };
        let head_span = token.span;
        let head = match &token.kind {
            TokenKind::Bareword => {
                let name = self.text(head_span);
                Head::parse(name).ok_or_else(|| {
                    Error::new(head_span, format!("`{name}` is not a form")).with_help(
                        "the forms are param, if, str, and import; there is no way \
                                    to define another",
                    )
                })?
            }
            TokenKind::RParen => {
                return Err(Error::new(
                    Span::new(open.start, head_span.end),
                    "a form needs a name",
                )
                .with_help("the forms are param, if, str, and import"));
            }
            kind => {
                return Err(Error::new(
                    head_span,
                    format!(
                        "a form begins with its name, but this is {}",
                        kind.describe()
                    ),
                )
                .with_help("the forms are param, if, str, and import"));
            }
        };
        self.pos += 1;

        let mut args = Vec::new();
        let close = loop {
            match self.tokens.get(self.pos) {
                None => {
                    return Err(
                        Error::new(open, "unclosed `(`").with_help("expected a matching `)`")
                    );
                }
                Some(token) if matches!(token.kind, TokenKind::RParen) => {
                    let end = token.span.end;
                    self.pos += 1;
                    break end;
                }
                _ => args.push(self.value()?),
            }
        };

        let span = Span::new(open.start, close);
        let (least, most) = head.arity();
        if args.len() < least || most.is_some_and(|most| args.len() > most) {
            // The noun is on the "takes" side, so the count alone reads better on the other.
            let given = match args.len() {
                0 => "none".to_string(),
                n => n.to_string(),
            };
            return Err(Error::new(
                span,
                format!(
                    "`{}` takes {}, but was given {given}",
                    head.name(),
                    arity(least, most),
                ),
            )
            .with_help(head.usage()));
        }

        // The one static check a form gets: a name or a path that is computed would not be
        // visible to a reader, and the whole point of these two is that they are.
        if head.first_arg_is_literal() && !matches!(args[0].kind, Kind::Literal(Value::String(_))) {
            let what = if head == Head::Param { "name" } else { "path" };
            return Err(Error::new(
                args[0].span,
                format!("a {}'s {what} has to be written down", head.name()),
            )
            .with_help(format!(
                "write a quoted string, so a reader can see which {} a template needs \
                 without running it",
                if head == Head::Param {
                    "parameters"
                } else {
                    "files"
                }
            )));
        }

        Ok(Node {
            kind: Kind::Form(Form { head, args }),
            span,
        })
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

/// Builds an array node, collapsing it to a literal when nothing inside it is computed.
fn array(items: Vec<Node>, span: Span) -> Node {
    if items.iter().all(|node| node.is_literal()) {
        let values = items.into_iter().map(Node::into_literal).collect();
        return Node {
            kind: Kind::Literal(Value::Array(values)),
            span,
        };
    }
    Node {
        kind: Kind::Array(items),
        span,
    }
}

/// Builds an object node, collapsing it to a literal when nothing inside it is computed.
fn object(members: Vec<(String, Node)>, span: Span) -> Node {
    if members.iter().all(|(_, node)| node.is_literal()) {
        let mut map = Map::new();
        for (key, node) in members {
            map.insert(key, node.into_literal());
        }
        return Node {
            kind: Kind::Literal(Value::Object(map)),
            span,
        };
    }
    Node {
        kind: Kind::Object(members),
        span,
    }
}

impl Node {
    fn is_literal(&self) -> bool {
        matches!(self.kind, Kind::Literal(_))
    }

    fn into_literal(self) -> Value {
        match self.kind {
            Kind::Literal(value) => value,
            _ => unreachable!("checked before collapsing"),
        }
    }
}

fn arity(least: usize, most: Option<usize>) -> String {
    match most {
        Some(most) if most == least => plural(least, "argument"),
        Some(most) => format!("{least} or {most} arguments"),
        None => "any number of arguments".to_string(),
    }
}

fn plural(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

// --- parameters -------------------------------------------------------------------------------

/// The values a build was given for its `(param …)` forms.
///
/// Parameters come from the command line and nothing else, so a build is a pure function of
/// its inputs and reproduces anywhere. Reading the environment would be convenient and would
/// make `tot build --check` able to pass on one machine and fail on another for a reason the
/// source does not show; anyone who wants it can write `--set env="$ENV"` and put the
/// dependency in plain sight.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Params(HashMap<String, Value>);

impl Params {
    /// No parameters.
    pub fn new() -> Self {
        Params(HashMap::new())
    }

    /// Sets one, replacing any previous value for that name.
    pub fn set(&mut self, name: impl Into<String>, value: Value) {
        self.0.insert(name.into(), value);
    }

    /// The value for `name`, if it was set.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    /// Whether nothing was set.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The names that were set, sorted, for a diagnostic.
    fn names(&self) -> String {
        if self.0.is_empty() {
            return "no parameters were set; pass one with `--set name=value`".to_string();
        }
        let mut names: Vec<&str> = self.0.keys().map(String::as_str).collect();
        names.sort_unstable();
        format!("the parameters set are {}", names.join(", "))
    }
}

// --- imports ----------------------------------------------------------------------------------

/// A file that `(import …)` asked for.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// What to call it in diagnostics. Cycle detection compares these, so it has to identify
    /// the file rather than the spelling that reached it — two paths to one file are one file.
    pub name: String,
    /// Its text.
    pub source: String,
    /// Which language to read it in, which follows the extension: `.tott` is evaluated, and
    /// anything else is data.
    pub dialect: Dialect,
}

/// Where `(import …)` gets its files.
///
/// The library does no I/O of its own, so a build can be driven from a filesystem, an archive,
/// or a map in a test without any of them being special.
pub trait Imports {
    /// Loads `target` as it was written inside the file called `from`.
    fn load(&mut self, from: &str, target: &str) -> Result<Loaded, String>;
}

/// An [`Imports`] that has none, for a template that should not be reaching for files.
pub struct NoImports;

impl Imports for NoImports {
    fn load(&mut self, _from: &str, target: &str) -> Result<Loaded, String> {
        Err(format!(
            "cannot import `{target}`: this build has no importer"
        ))
    }
}

// --- failures ---------------------------------------------------------------------------------

/// A build failure.
///
/// Unlike a parse error, this may have happened in a file other than the one the build started
/// at, so it carries that file's name and text — enough to draw the caret where it belongs
/// rather than at whatever offset the span happens to hit in the wrong document.
///
/// The contents are boxed because a whole file's text sits inside, and this is the error half
/// of every result the evaluator returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError(Box<Failure>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Failure {
    error: Error,
    file: String,
    text: String,
    chain: Vec<String>,
}

impl BuildError {
    /// What went wrong, with a span into [`text`](BuildError::text).
    pub fn error(&self) -> &Error {
        &self.0.error
    }

    /// The file the span indexes, which is not always the one the build started at.
    pub fn file(&self) -> &str {
        &self.0.file
    }

    /// That file's source, so a caller can render the span against the right document.
    pub fn text(&self) -> &str {
        &self.0.text
    }

    /// The files that imported it, starting with the one the build began at.
    pub fn chain(&self) -> &[String] {
        &self.0.chain
    }

    /// Render the diagnostic, with a caret and the import chain that reached it.
    pub fn render(&self) -> String {
        let mut out = format!("in {}\n", self.file());
        out.push_str(&self.0.error.render(&self.0.text));
        for name in self.0.chain.iter().rev() {
            out.push_str(&format!("  imported from {name}\n"));
        }
        out
    }
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "in {}: {}", self.0.file, self.0.error)
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0.error)
    }
}

// --- evaluation -------------------------------------------------------------------------------

impl Template {
    /// Build this template with no importer, which is enough for a template that has no
    /// `(import …)` in it.
    pub fn evaluate(&self, params: &Params) -> Result<Value, BuildError> {
        self.build(params, &mut NoImports)
    }

    /// Build this template, resolving imports through `imports`.
    pub fn build<I: Imports>(&self, params: &Params, imports: &mut I) -> Result<Value, BuildError> {
        let mut build = Build {
            params,
            imports,
            stack: vec![Frame {
                name: self.name.clone(),
                source: self.source.clone(),
            }],
        };
        build.node(&self.root)
    }
}

/// One file being evaluated.
struct Frame {
    name: String,
    source: String,
}

struct Build<'a, I: Imports> {
    params: &'a Params,
    imports: &'a mut I,
    /// The files currently open, for cycle detection and for the chain in a diagnostic.
    stack: Vec<Frame>,
}

impl<I: Imports> Build<'_, I> {
    /// Attaches the file the failure happened in, and how the build got there.
    fn fail(&self, error: Error) -> BuildError {
        let top = self.stack.last().expect("a frame is open while evaluating");
        BuildError(Box::new(Failure {
            error,
            file: top.name.clone(),
            text: top.source.clone(),
            chain: self.stack[..self.stack.len() - 1]
                .iter()
                .map(|frame| frame.name.clone())
                .collect(),
        }))
    }

    fn node(&mut self, node: &Node) -> Result<Value, BuildError> {
        match &node.kind {
            Kind::Literal(value) => Ok(value.clone()),
            Kind::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.node(item)?);
                }
                Ok(Value::Array(out))
            }
            Kind::Object(members) => {
                let mut map = Map::new();
                for (key, member) in members {
                    let value = self.node(member)?;
                    map.insert(key.clone(), value);
                }
                Ok(Value::Object(map))
            }
            Kind::Form(form) => self.form(form, node.span),
        }
    }

    fn form(&mut self, form: &Form, span: Span) -> Result<Value, BuildError> {
        match form.head {
            Head::Param => self.param(form, span),
            Head::If => self.conditional(form),
            Head::Str => self.join(form),
            Head::Import => self.import(form, span),
        }
    }

    fn param(&mut self, form: &Form, span: Span) -> Result<Value, BuildError> {
        let name = literal_string(&form.args[0]);
        if let Some(value) = self.params.get(name) {
            return Ok(value.clone());
        }
        match form.args.get(1) {
            Some(default) => self.node(default),
            None => Err(self.fail(
                Error::new(span, format!("no value for parameter `{name}`"))
                    .with_help(self.params.names()),
            )),
        }
    }

    /// Only the branch that is taken is evaluated, so the other one may be an import of a file
    /// that does not exist in this configuration.
    fn conditional(&mut self, form: &Form) -> Result<Value, BuildError> {
        let condition = self.node(&form.args[0])?;
        let taken = match condition {
            Value::Bool(true) => &form.args[1],
            Value::Bool(false) => &form.args[2],
            // tot has no truthiness, in a template no more than in a document.
            other => {
                return Err(self.fail(
                    Error::new(
                        form.args[0].span,
                        format!(
                            "the condition of `if` is a boolean, but this is {}",
                            kind(&other)
                        ),
                    )
                    .with_help(
                        "tot has no truthiness: write a comparison's result, or a \
                                parameter that is `true` or `false`",
                    ),
                ));
            }
        };
        self.node(taken)
    }

    fn join(&mut self, form: &Form) -> Result<Value, BuildError> {
        let mut out = String::new();
        for arg in &form.args {
            match self.node(arg)? {
                Value::String(s) => out.push_str(&s),
                Value::Integer(i) => out.push_str(i.as_str()),
                // The normalized spelling, so `1.` reads as `1.0` inside a string.
                Value::Float(f) => out.push_str(&f.to_string()),
                Value::Bool(b) => out.push_str(if b { "true" } else { "false" }),
                other => {
                    return Err(self.fail(
                        Error::new(
                            arg.span,
                            format!("`str` has no spelling for {}", kind(&other)),
                        )
                        .with_help(
                            "`str` joins strings, numbers, and booleans; anything else would \
                             be a guess at how it should read",
                        ),
                    ));
                }
            }
        }
        Ok(Value::String(out))
    }

    fn import(&mut self, form: &Form, span: Span) -> Result<Value, BuildError> {
        let target = literal_string(&form.args[0]).to_string();
        let from = self
            .stack
            .last()
            .expect("a frame is open while evaluating")
            .name
            .clone();

        if self.stack.len() >= MAX_IMPORTS {
            return Err(self.fail(Error::new(
                span,
                format!("imports are nested more than {MAX_IMPORTS} deep"),
            )));
        }

        let loaded = self
            .imports
            .load(&from, &target)
            .map_err(|message| self.fail(Error::new(span, message)))?;

        // An import graph has to be acyclic: a file that imports itself, however indirectly,
        // has no value to be replaced by.
        if let Some(at) = self
            .stack
            .iter()
            .position(|frame| frame.name == loaded.name)
        {
            let mut cycle: Vec<&str> = self.stack[at..]
                .iter()
                .map(|frame| frame.name.as_str())
                .collect();
            cycle.push(&loaded.name);
            return Err(self.fail(
                Error::new(span, format!("importing `{}` is a cycle", loaded.name))
                    .with_help(format!("the cycle is {}", cycle.join(" → "))),
            ));
        }

        self.stack.push(Frame {
            name: loaded.name,
            source: loaded.source,
        });
        let result = self.imported(loaded.dialect);
        self.stack.pop();
        result
    }

    /// Reads whatever the importer just handed us, in the dialect it named.
    fn imported(&mut self, dialect: Dialect) -> Result<Value, BuildError> {
        let frame = self.stack.last().expect("just pushed");
        let (name, source) = (frame.name.clone(), frame.source.clone());
        match dialect {
            // A `.tot` file is data, and reading it is the ordinary parser. This is the whole
            // reason importing one costs nothing: there is no evaluation to do.
            Dialect::Data => crate::parse(&source).map_err(|e| self.fail(e)),
            Dialect::Template => {
                let template = Template::parse_named(&source, &name).map_err(|e| self.fail(e))?;
                self.node(&template.root)
            }
        }
    }
}

/// The text of an argument the parser has already required to be a written-down string.
fn literal_string(node: &Node) -> &str {
    match &node.kind {
        Kind::Literal(Value::String(s)) => s,
        _ => unreachable!("checked when the form was parsed"),
    }
}
