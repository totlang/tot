//! The canonical formatter.
//!
//! Two rules do most of the work. Inline versus block is the author's choice and is preserved
//! — the formatter never reflows, so there is no line-length budget. And a multi-line string
//! is re-indented along with the block it lives in, which is well defined precisely because
//! its indentation is anchored to its closing delimiter.

use crate::cst::{self, Body, Collection, Document, Item, Lead, Node};
use crate::error::Error;
use crate::lex::can_be_bare;
use crate::value::{Value, write_escaped};

/// Format a tot document into its canonical form.
///
/// ```
/// let out = tot::format(r#"{"address": {"zip": 94102}}"#).unwrap();
/// assert_eq!(out, "{address {zip 94102}}\n");
/// ```
pub fn format(src: &str) -> Result<String, Error> {
    // Validate first, so the tree walk below can assume a well-formed document.
    crate::parse(src)?;

    let document = cst::build(src)?;
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
            self.out.push_str(key);
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
        }
    }

    fn collection(&mut self, collection: &Collection<'_>, level: usize, open: char, close: char) {
        if collection.items.is_empty() && collection.tail.is_empty() {
            self.out.push(open);
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
            self.out.push(open);
            for (i, item) in collection.items.iter().enumerate() {
                if i > 0 {
                    self.out.push(' ');
                }
                if let Some(key) = &item.key {
                    self.out.push_str(key);
                    self.out.push(' ');
                }
                self.value(&item.value, level);
            }
            self.out.push(close);
            return;
        }

        self.out.push(open);
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
        // A root object is written without its braces — the whole point of the language.
        Value::Object(map) => {
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
        Value::String(s) => {
            out.push('"');
            write_escaped(out, s);
            out.push('"');
        }
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

fn write_key(out: &mut String, key: &str) {
    if can_be_bare(key) {
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
