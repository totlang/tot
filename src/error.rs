use std::fmt;

/// A byte range into the source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset one past the last character.
    pub end: usize,
}

impl Span {
    pub(crate) fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }
}

/// A lex or parse failure, carrying the source range that caused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The offending source range.
    pub span: Span,
    /// What went wrong.
    pub message: String,
    /// How to fix it, when there is an obvious answer.
    pub help: Option<String>,
}

impl Error {
    pub(crate) fn new(span: Span, message: impl Into<String>) -> Self {
        Error {
            span,
            message: message.into(),
            help: None,
        }
    }

    pub(crate) fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// The 1-based line and column (counted in characters) where the span starts.
    pub fn line_col(&self, src: &str) -> (usize, usize) {
        line_col(src, self.span.start)
    }

    /// Render a multi-line diagnostic with the offending line and a caret under the span.
    pub fn render(&self, src: &str) -> String {
        render("error", self.span, &self.message, self.help.as_deref(), src)
    }
}

/// The 1-based line and column (counted in characters) of a byte offset.
pub(crate) fn line_col(src: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(src.len());
    let mut line = 1;
    let mut line_start = 0;
    for (i, b) in src.as_bytes()[..offset].iter().enumerate() {
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    (line, src[line_start..offset].chars().count() + 1)
}

/// Shared by errors and lint warnings, so the two read alike.
pub(crate) fn render(
    label: &str,
    span: Span,
    message: &str,
    help: Option<&str>,
    src: &str,
) -> String {
    let (line, col) = line_col(src, span.start);
    let text = src.lines().nth(line - 1).unwrap_or("");
    // Clamp the caret to the first line of the span so a multi-line string doesn't
    // underline the whole document.
    let width = src
        .get(span.start..span.end.min(src.len()))
        .and_then(|s| s.lines().next())
        .map(|s| s.chars().count())
        .unwrap_or(1)
        .max(1);
    let gutter = line.to_string().len();
    let pad = " ".repeat(gutter);

    let mut out = format!("{label}: {message}\n");
    out.push_str(&format!("{pad}--> {line}:{col}\n"));
    out.push_str(&format!("{pad} |\n"));
    out.push_str(&format!("{line} | {text}\n"));
    out.push_str(&format!(
        "{pad} | {}{}",
        " ".repeat(col - 1),
        "^".repeat(width)
    ));
    if let Some(help) = help {
        out.push_str(&format!(" help: {help}"));
    }
    out.push('\n');
    out
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(help) = &self.help {
            write!(f, " (help: {help})")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}
