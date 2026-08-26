//! Optional checks. Nothing here is part of the language — everything a lint reports is
//! legal tot, and a document that trips every one of them still parses.

use std::fmt;

use crate::cst::{self, Body, Item, Node};
use crate::error::{self, Error, Span};

/// Something legal but worth a second look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The source range the warning is about.
    pub span: Span,
    /// What was noticed.
    pub message: String,
    /// What to do about it.
    pub help: Option<String>,
}

impl Warning {
    /// The 1-based line and column (counted in characters) where the span starts.
    pub fn line_col(&self, src: &str) -> (usize, usize) {
        error::line_col(src, self.span.start)
    }

    /// Render a multi-line diagnostic with the offending line and a caret under the span.
    pub fn render(&self, src: &str) -> String {
        error::render(
            "warning",
            self.span,
            &self.message,
            self.help.as_deref(),
            src,
        )
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(help) = &self.help {
            write!(f, " (help: {help})")?;
        }
        Ok(())
    }
}

/// Check a document for things that are legal but risky.
///
/// Currently one rule: **a member's value must begin on the same line as its key.** tot has no
/// separator between members, so a missing value silently shifts every member after it.
/// Requiring quotes on string values catches nearly all of those at the offending token, but
/// only keeping each member on one line makes the error land in the right place every time.
///
/// A value may still run past that line — `{`, `[`, and `"""` only have to *start* beside the
/// key. Whitespace stays non-structural in the language; this is the tooling choosing a
/// convention, not the grammar imposing one.
///
/// ```
/// assert!(tot::lint("timeout 30").unwrap().is_empty());
/// assert_eq!(tot::lint("timeout\n30").unwrap().len(), 1);
/// ```
pub fn lint(src: &str) -> Result<Vec<Warning>, Error> {
    // Validate first, so the tree walk below can assume a well-formed document.
    crate::parse(src)?;

    let document = cst::build(src)?;
    let mut warnings = Vec::new();
    match &document.body {
        Body::Members(items) => visit_items(items, &mut warnings),
        Body::Value(node) => visit_node(node, &mut warnings),
    }
    Ok(warnings)
}

fn visit_items(items: &[Item<'_>], warnings: &mut Vec<Warning>) {
    for item in items {
        if let Some(key) = &item.key
            && key.split_from_value
        {
            warnings.push(Warning {
                span: key.span,
                message: format!(
                    "the value of `{}` is on a different line from its key",
                    key.text
                ),
                help: Some(
                    "keep a member on one line, so that a missing value cannot shift the \
                     members after it"
                        .to_string(),
                ),
            });
        }
        visit_node(&item.value, warnings);
    }
}

fn visit_node(node: &Node<'_>, warnings: &mut Vec<Warning>) {
    match node {
        Node::Scalar(_) => {}
        Node::Object(collection) | Node::Array(collection) => {
            visit_items(&collection.items, warnings);
        }
    }
}
