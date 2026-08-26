//! A formatting-oriented syntax tree.
//!
//! [`parse`](crate::parse) discards comments, blank lines, and whether a collection was
//! written inline — all of which the formatter has to preserve. This module re-walks the
//! token stream and keeps them, reading trivia straight out of the gaps between token spans:
//! everything between two tokens is whitespace, `,`/`:`, or a comment, so no second lexer is
//! needed.

use crate::error::{Error, Span};
use crate::lex::{Token, TokenKind, can_be_bare};

/// One line of the run-up to an item.
#[derive(Debug)]
pub(crate) enum Lead<'a> {
    Blank,
    /// A whole-line comment, including its `#`, with trailing whitespace removed.
    Comment(&'a str),
}

#[derive(Debug)]
pub(crate) enum Node<'a> {
    /// A string or a number/`true`/`false`/`null` bareword, as written in the source.
    Scalar(&'a str),
    Object(Collection<'a>),
    Array(Collection<'a>),
}

#[derive(Debug)]
pub(crate) struct Collection<'a> {
    /// Whether the source had a newline between the brackets. The author's choice of inline
    /// or block is preserved, so this is the only thing that decides the shape.
    pub block: bool,
    pub items: Vec<Item<'a>>,
    /// Comments and blank lines between the last item and the closing bracket.
    pub tail: Vec<Lead<'a>>,
}

/// A member's key, carrying what the formatter and the lints each need.
#[derive(Debug)]
pub(crate) struct Key {
    /// The key as it should be written: unquoted where that is legal.
    pub text: String,
    /// Where the key sits in the source, for diagnostics.
    pub span: Span,
    /// Whether a newline separates the key from the first token of its value.
    pub split_from_value: bool,
}

#[derive(Debug)]
pub(crate) struct Item<'a> {
    pub lead: Vec<Lead<'a>>,
    /// `None` for array elements.
    pub key: Option<Key>,
    pub value: Node<'a>,
    /// A comment that followed the value on the same line.
    pub trailing: Option<&'a str>,
}

#[derive(Debug)]
pub(crate) enum Body<'a> {
    Members(Vec<Item<'a>>),
    Value(Node<'a>),
}

#[derive(Debug)]
pub(crate) struct Document<'a> {
    /// Trivia before the body. Empty when the body is a member list, since the first member
    /// owns its own lead.
    pub lead: Vec<Lead<'a>>,
    pub body: Body<'a>,
    /// A comment on the same line as the end of a [`Body::Value`] document. A member list has
    /// no use for this — there the comment belongs to the last member.
    pub trailing: Option<&'a str>,
    pub tail: Vec<Lead<'a>>,
}

/// Builds the tree from an already-tokenized document. The caller has normally just validated
/// the same tokens with [`parse`](crate::parse), and tokenizing again for this walk would be
/// pure duplicate work — including re-unescaping every string.
pub(crate) fn from_tokens<'a>(src: &'a str, tokens: &[Token]) -> Result<Document<'a>, Error> {
    Builder {
        src,
        tokens,
        pos: 0,
    }
    .document()
}

struct Builder<'a, 't> {
    src: &'a str,
    tokens: &'t [Token],
    pos: usize,
}

impl<'a> Builder<'a, '_> {
    fn document(mut self) -> Result<Document<'a>, Error> {
        let (_, mut lead) = self.split_gap(false);
        trim_leading_blanks(&mut lead);

        if self.tokens.is_empty() {
            trim_trailing_blanks(&mut lead);
            return Ok(Document {
                lead,
                body: Body::Members(Vec::new()),
                trailing: None,
                tail: Vec::new(),
            });
        }

        // Mirrors `parse::document`: a document that opens with a bracket, or is a single
        // token, is one value rather than a member list.
        let single = matches!(self.tokens[0].kind, TokenKind::LBrace | TokenKind::LBracket)
            || self.tokens.len() == 1;
        if single {
            let node = self.node()?;
            // There is no member here to hang a trailing comment on, so the document keeps it.
            let (trailing, mut tail) = self.split_gap(true);
            trim_trailing_blanks(&mut tail);
            return Ok(Document {
                lead,
                body: Body::Value(node),
                trailing,
                tail,
            });
        }

        // The leading trivia belongs to the first member.
        let mut next_lead = lead;
        let mut items: Vec<Item<'a>> = Vec::new();
        loop {
            if self.pos >= self.tokens.len() {
                trim_trailing_blanks(&mut next_lead);
                return Ok(Document {
                    lead: Vec::new(),
                    body: Body::Members(items),
                    trailing: None,
                    tail: next_lead,
                });
            }
            items.push(self.item(true, next_lead)?);
            let (trailing, lead) = self.split_gap(true);
            if let Some(comment) = trailing {
                items.last_mut().expect("just pushed").trailing = Some(comment);
            }
            next_lead = lead;
        }
    }

    /// The source between the previous token and the current one.
    fn gap(&self) -> &'a str {
        let start = if self.pos == 0 {
            0
        } else {
            self.tokens[self.pos - 1].span.end
        };
        let end = self
            .tokens
            .get(self.pos)
            .map_or(self.src.len(), |token| token.span.start);
        &self.src[start..end]
    }

    /// Splits the gap into a comment trailing whatever came before it and the lead lines for
    /// whatever comes next. `after_item` says whether there is a preceding item on that first
    /// line for a comment to attach to.
    fn split_gap(&self, after_item: bool) -> (Option<&'a str>, Vec<Lead<'a>>) {
        let gap = self.gap();
        let has_next = self.pos < self.tokens.len();
        let segments: Vec<&str> = gap.split('\n').collect();

        let mut trailing = None;
        let mut first = 0;
        if after_item {
            // A comment on the first line trails the previous item — unless that line is also
            // the line the next token sits on, in which case there is no comment to find.
            if segments.len() > 1 || !has_next {
                trailing = comment_in(segments[0]);
            }
            first = 1;
        }
        // The final segment is the run-up to the next token on its own line, not a line of
        // its own — unless there is no next token.
        let last = segments.len() - usize::from(has_next);

        let mut lead = Vec::new();
        for segment in &segments[first.min(last)..last] {
            match comment_in(segment) {
                Some(comment) => lead.push(Lead::Comment(comment)),
                None => lead.push(Lead::Blank),
            }
        }
        (trailing, lead)
    }

    fn item(&mut self, keyed: bool, mut lead: Vec<Lead<'a>>) -> Result<Item<'a>, Error> {
        let key = if keyed {
            let token = self.tokens.get(self.pos).ok_or_else(internal)?;
            let span = token.span;
            let raw = &self.src[span.start..span.end];
            let text = match &token.kind {
                TokenKind::Str(cooked) => render_key(raw, cooked),
                TokenKind::Bareword => raw.to_string(),
                _ => return Err(internal()),
            };
            self.pos += 1;

            // The gap between a key and the start of its value is exactly what
            // `check --strict` asks about, so record it while we are standing on it.
            let split_from_value = self.gap().contains('\n');

            // A comment between a key and its value has no natural home. Move it above the
            // member rather than dropping it.
            let (_, between) = self.split_gap(false);
            lead.extend(
                between
                    .into_iter()
                    .filter(|line| matches!(line, Lead::Comment(_))),
            );

            Some(Key {
                text,
                span,
                split_from_value,
            })
        } else {
            None
        };

        Ok(Item {
            lead,
            key,
            value: self.node()?,
            trailing: None,
        })
    }

    fn node(&mut self) -> Result<Node<'a>, Error> {
        let token = self.tokens.get(self.pos).ok_or_else(internal)?;
        let span = token.span;
        match token.kind.clone() {
            TokenKind::LBrace => Ok(Node::Object(self.collection(true)?)),
            TokenKind::LBracket => Ok(Node::Array(self.collection(false)?)),
            TokenKind::Str(_) | TokenKind::Bareword => {
                self.pos += 1;
                Ok(Node::Scalar(&self.src[span.start..span.end]))
            }
            _ => Err(internal()),
        }
    }

    /// Builds `{ … }` when `keyed`, `[ … ]` otherwise. The opening bracket is the current
    /// token.
    fn collection(&mut self, keyed: bool) -> Result<Collection<'a>, Error> {
        let open_end = self.tokens[self.pos].span.end;
        self.pos += 1;
        let mut items: Vec<Item<'a>> = Vec::new();

        loop {
            let (trailing, mut lead) = self.split_gap(!items.is_empty());
            if let Some(comment) = trailing {
                items
                    .last_mut()
                    .expect("after_item implies an item")
                    .trailing = Some(comment);
            }

            let at_close = match self.tokens.get(self.pos).map(|token| &token.kind) {
                Some(TokenKind::RBrace) => keyed,
                Some(TokenKind::RBracket) => !keyed,
                _ => false,
            };
            if at_close {
                let close_start = self.tokens[self.pos].span.start;
                self.pos += 1;
                if items.is_empty() {
                    trim_leading_blanks(&mut lead);
                }
                trim_trailing_blanks(&mut lead);
                return Ok(Collection {
                    block: self.src[open_end..close_start].contains('\n'),
                    items,
                    tail: lead,
                });
            }

            if items.is_empty() {
                trim_leading_blanks(&mut lead);
            }
            items.push(self.item(keyed, lead)?);
        }
    }
}

/// A key is always a string, so an unquoted spelling is available whenever every character
/// is a legal bareword character. Otherwise the original quoting is kept verbatim, escapes
/// and all.
fn render_key(raw: &str, cooked: &str) -> String {
    if can_be_bare(cooked) {
        cooked.to_string()
    } else {
        raw.to_string()
    }
}

fn comment_in(segment: &str) -> Option<&str> {
    let hash = segment.find('#')?;
    Some(segment[hash..].trim_end())
}

fn trim_leading_blanks(lead: &mut Vec<Lead<'_>>) {
    let keep = lead
        .iter()
        .position(|line| matches!(line, Lead::Comment(_)))
        .unwrap_or(lead.len());
    lead.drain(..keep);
}

fn trim_trailing_blanks(lead: &mut Vec<Lead<'_>>) {
    while matches!(lead.last(), Some(Lead::Blank)) {
        lead.pop();
    }
}

/// Unreachable: `format` validates with `parse` before building, so the token stream is
/// known to be well-formed by the time we walk it.
fn internal() -> Error {
    Error::new(
        Span::new(0, 0),
        "internal error: the formatter and the parser disagree about this document",
    )
}
