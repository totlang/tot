//! The canonical formatter.
//!
//! Two rules do most of the work. Inline versus block is the author's choice and is preserved
//! — the formatter never reflows, so there is no line-length budget. And a multi-line string
//! is re-indented along with the block it lives in, which is well defined precisely because
//! its indentation is anchored to its closing delimiter.

use crate::cst::{self, Body, Collection, Document, Form, Item, Lead, Node};
use crate::error::Error;
use crate::lex::{Dialect, can_be_bare};
use crate::value::{Value, write_escaped};
use std::fmt::Write as _;

/// Format a tot document into its canonical form.
///
/// ```
/// let out = tot::format(r#"{"address": {"zip": 94102}}"#).unwrap();
/// assert_eq!(out, "{address {zip 94102}}\n");
/// ```
pub fn format(src: &str) -> Result<String, Error> {
    format_in(src, Dialect::Data)
}

/// Format a `.tott` template into its canonical form.
///
/// The same formatter, with one more shape to write: a form is bracketed like a collection and
/// follows the same rule, so the author's choice of inline or block is preserved and nothing is
/// ever reflowed.
///
/// ```
/// let out = tot::format_template("a (str   \"x\"  (param  \"n\"))").unwrap();
/// assert_eq!(out, "a (str \"x\" (param \"n\"))\n");
/// ```
pub fn format_template(src: &str) -> Result<String, Error> {
    format_in(src, Dialect::Template)
}

fn format_in(src: &str, dialect: Dialect) -> Result<String, Error> {
    // Validate first, so the tree walk below can assume a well-formed document. Both walks
    // read the same tokens, so the source is lexed once.
    let tokens = crate::lex::tokenize(src, dialect)?;
    match dialect {
        Dialect::Data => {
            crate::parse::from_tokens(src, &tokens)?;
        }
        Dialect::Template => crate::template::validate(src, &tokens)?,
    }

    let document = cst::from_tokens(src, &tokens, dialect)?;
    let mut printer = Printer { out: String::new() };
    printer.document(&document);
    Ok(printer.finish())
}

struct Printer {
    out: String,
}

impl Printer {
    fn document(&mut self, document: &Document<'_>) {
        self.leads(&document.lead, 0);
        match &document.body {
            // The brace-less root has no brackets to keep an inline form inside, so its
            // members are always one per line. Write `{a 1 b 2}` if you want one line.
            Body::Members(items) => {
                for item in items {
                    self.item(item, 0);
                }
            }
            Body::Value(node) => {
                self.value(node, 0);
                if let Some(comment) = document.trailing {
                    self.out.push(' ');
                    self.out.push_str(comment);
                }
                self.out.push('\n');
            }
        }
        self.leads(&document.tail, 0);
    }

    fn finish(mut self) -> String {
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        self.out
    }

    fn indent(&mut self, level: usize) {
        write_indent(&mut self.out, level);
    }

    fn leads(&mut self, leads: &[Lead<'_>], level: usize) {
        let mut after_blank = false;
        for lead in leads {
            match lead {
                Lead::Blank => {
                    if !after_blank {
                        self.out.push('\n');
                    }
                    after_blank = true;
                }
                Lead::Comment(text) => {
                    self.indent(level);
                    self.out.push_str(text);
                    self.out.push('\n');
                    after_blank = false;
                }
            }
        }
    }

    fn item(&mut self, item: &Item<'_>, level: usize) {
        self.leads(&item.lead, level);
        self.indent(level);
        if let Some(key) = &item.key {
            self.out.push_str(&key.text);
            self.out.push(' ');
        }
        self.value(&item.value, level);
        if let Some(comment) = item.trailing {
            self.out.push(' ');
            self.out.push_str(comment);
        }
        self.out.push('\n');
    }

    fn value(&mut self, node: &Node<'_>, level: usize) {
        match node {
            Node::Scalar(raw) => self.scalar(raw, level),
            Node::Object(collection) => self.collection(collection, level, '{', '}'),
            Node::Array(collection) => self.collection(collection, level, '[', ']'),
            Node::Form(form) => self.form(form, level),
        }
    }

    fn collection(&mut self, collection: &Collection<'_>, level: usize, open: char, close: char) {
        self.out.push(open);
        self.bracketed(collection, level, close, false);
    }

    /// A form is bracketed like a collection, so it gets the same rule: the author's choice of
    /// inline or block is preserved and nothing is reflowed. The head belongs to the opening,
    /// so `(str` stays together and only the arguments move.
    fn form(&mut self, form: &Form<'_>, level: usize) {
        self.out.push('(');
        self.out.push_str(form.head);
        // The one difference from a collection: `(str "a")` needs a space after the head,
        // where `{a 1}` needs nothing after the brace.
        self.bracketed(&form.args, level, ')', true);
    }

    /// Writes the items of a bracketed shape and its closing bracket. The opening has already
    /// been written; `spaced` is whether an inline first item needs a space before it.
    fn bracketed(&mut self, collection: &Collection<'_>, level: usize, close: char, spaced: bool) {
        if collection.items.is_empty() && collection.tail.is_empty() {
            self.out.push(close);
            return;
        }

        // A comment forces a line of its own, so anything carrying trivia has to be block
        // form regardless of how it was written. In practice a comment implies a newline
        // inside the brackets anyway, so this only guards the renderer.
        let has_trivia = !collection.tail.is_empty()
            || collection
                .items
                .iter()
                .any(|item| !item.lead.is_empty() || item.trailing.is_some());

        if !collection.block && !has_trivia {
            for (i, item) in collection.items.iter().enumerate() {
                if i > 0 || spaced {
                    self.out.push(' ');
                }
                if let Some(key) = &item.key {
                    self.out.push_str(&key.text);
                    self.out.push(' ');
                }
                self.value(&item.value, level);
            }
            self.out.push(close);
            return;
        }

        self.out.push('\n');
        for item in &collection.items {
            self.item(item, level + 1);
        }
        self.leads(&collection.tail, level + 1);
        self.indent(level);
        self.out.push(close);
    }

    fn scalar(&mut self, raw: &str, level: usize) {
        if raw.starts_with("\"\"\"") {
            self.multiline(raw, level);
        } else {
            self.out.push_str(raw);
        }
    }

    /// Re-indents a `"""` string to sit one level inside its member. The value is unchanged:
    /// the old prefix comes off every content line and the new one goes on, and the closing
    /// delimiter — which is what defines the prefix — moves with them.
    fn multiline(&mut self, raw: &str, level: usize) {
        let prefix = "  ".repeat(level + 1);
        let mut lines = raw.split('\n');
        lines.next(); // the opening delimiter, plus any whitespace we are about to drop

        let lines: Vec<&str> = lines
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect();
        let Some((closing, content)) = lines.split_last() else {
            self.out.push_str(raw); // not reachable for a lexed multi-line string
            return;
        };
        let old = &closing[..closing.len() - closing.trim_start_matches([' ', '\t']).len()];

        self.out.push_str("\"\"\"\n");
        for line in content {
            if line.trim_matches([' ', '\t']).is_empty() {
                self.out.push('\n'); // a blank line, with no trailing whitespace
            } else {
                self.out.push_str(&prefix);
                self.out.push_str(line.strip_prefix(old).unwrap_or(line));
                self.out.push('\n');
            }
        }
        self.out.push_str(&prefix);
        self.out.push_str("\"\"\"");
    }
}

/// Render a [`Value`] as tot.
///
/// This is the converters' entry point, and it differs from [`format`] in the one way that
/// matters: there is no source text, so there is no author intent to preserve. Everything is
/// written in block form except empty collections.
///
/// ```
/// let value = tot::parse(r#"{"a": [1, 2]}"#).unwrap();
/// assert_eq!(tot::format_value(&value), "a [\n  1\n  2\n]\n");
/// ```
pub fn format_value(value: &Value) -> String {
    let mut out = String::new();
    match value {
        // A root object is written without its braces — the whole point of the language. An
        // empty one keeps them, because a file of nothing is a poor way to say `{}`. This is
        // the one place the two emitters differ: `format` leaves an empty file empty, having
        // source to preserve and no reason to put anything into it.
        Value::Object(map) if !map.is_empty() => {
            for (key, member) in map.iter() {
                write_member(&mut out, key, member, 0);
            }
        }
        other => {
            write_value(&mut out, other, 0);
            out.push('\n');
        }
    }
    out
}

fn write_member(out: &mut String, key: &str, value: &Value, level: usize) {
    write_indent(out, level);
    write_key(out, key);
    out.push(' ');
    write_value(out, value, level);
    out.push('\n');
}

fn write_value(out: &mut String, value: &Value, level: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Integer(i) => out.push_str(i.as_str()),
        // Both lexemes are valid tot as they stand; only JSON needs `1.` normalized.
        Value::Float(f) => out.push_str(f.as_str()),
        Value::String(s) => write_string(out, s, level),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push_str("[\n");
            for item in items {
                write_indent(out, level + 1);
                write_value(out, item, level + 1);
                out.push('\n');
            }
            write_indent(out, level);
            out.push(']');
        }
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Object(map) => {
            out.push_str("{\n");
            for (key, member) in map.iter() {
                write_member(out, key, member, level + 1);
            }
            write_indent(out, level);
            out.push('}');
        }
    }
}

/// Writes a string, as a `"""` block where that round-trips and as a quoted literal
/// otherwise. Config files hold shell snippets, PEM blocks, and banners; a converter that
/// emits those as one `\n`-laden line is correct but useless.
fn write_string(out: &mut String, s: &str, level: usize) {
    match block_lines(s) {
        Some(lines) => write_block_string(out, &lines, level),
        None => {
            out.push('"');
            write_escaped(out, s);
            out.push('"');
        }
    }
}

/// The lines of a string that can be written as a block, or `None` if it cannot be.
///
/// A line ending in a space or tab rules the whole string out. The reader turns a
/// whitespace-only line into an empty one, so such a line would come back changed, and even
/// where it would survive, trailing whitespace is the first thing an editor or a pre-commit
/// hook strips. Everything else is handled by escaping.
fn block_lines(s: &str) -> Option<Vec<&str>> {
    if !s.contains('\n') {
        return None;
    }
    let lines: Vec<&str> = s.split('\n').collect();
    if lines.iter().any(|line| line.ends_with([' ', '\t'])) {
        return None;
    }
    Some(lines)
}

fn write_block_string(out: &mut String, lines: &[&str], level: usize) {
    // The closing delimiter defines the indentation, so content sits at the same level.
    let prefix = "  ".repeat(level + 1);
    out.push_str("\"\"\"\n");
    for line in lines {
        // Blank lines are written empty rather than indented: trailing whitespace would
        // both read back wrong and invite an editor to change the value.
        if line.is_empty() {
            out.push('\n');
            continue;
        }
        out.push_str(&prefix);
        write_block_line(out, line);
        out.push('\n');
    }
    out.push_str(&prefix);
    out.push_str("\"\"\"");
}

/// Escapes one line of block content.
///
/// `"` is left alone almost everywhere: a `"""` run only closes the string when it opens a
/// line, so that is the one position needing an escape. Leaving the rest bare is what keeps
/// an embedded shell script readable. A newline is structural here, and a tab is legal and
/// reads better than `\t`.
fn write_block_line(out: &mut String, line: &str) {
    let blanks = line.len() - line.trim_start_matches([' ', '\t']).len();
    let rest = if line[blanks..].starts_with("\"\"\"") {
        out.push_str(&line[..blanks]);
        out.push_str("\\\"");
        &line[blanks + 1..]
    } else {
        line
    };

    for c in rest.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            // A literal CR would be eaten as a line ending by the reader.
            '\r' => out.push_str("\\r"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 && c != '\t' => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
}

fn write_key(out: &mut String, key: &str) {
    // `format_value` writes `.tot` — it is what the converters and `tot build` emit — so a
    // paren in a key is an ordinary character here.
    if can_be_bare(key, Dialect::Data) {
        out.push_str(key);
    } else {
        out.push('"');
        write_escaped(out, key);
        out.push('"');
    }
}

fn write_indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}
