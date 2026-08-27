use crate::error::{Error, Span};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenKind {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    /// `(`, in a template only. In `.tot` this is an ordinary bareword character.
    LParen,
    /// `)`, in a template only.
    RParen,
    /// An unquoted run of non-delimiter characters. The text is `src[span]`.
    Bareword,
    /// A quoted string, already unescaped.
    Str(String),
}

impl TokenKind {
    pub(crate) fn describe(&self) -> &'static str {
        match self {
            TokenKind::LBrace => "`{`",
            TokenKind::RBrace => "`}`",
            TokenKind::LBracket => "`[`",
            TokenKind::RBracket => "`]`",
            TokenKind::LParen => "`(`",
            TokenKind::RParen => "`)`",
            TokenKind::Bareword => "a bareword",
            TokenKind::Str(_) => "a string",
        }
    }
}

/// Which language the text is written in.
///
/// The two share a lexer and differ in exactly one character pair. `(` and `)` are ordinary
/// bareword characters in `.tot` — `(a) 1` is the key `(a)` — and reserving them there would
/// break documents that already parse. In `.tott` they open and close a form, which is what
/// makes computation visible: parens never appear in data, so anything inside them is being
/// evaluated and anything outside them is not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dialect {
    /// `.tot` — data. No forms; every JSON document is one of these.
    #[default]
    Data,
    /// `.tott` — a template, which evaluates to data.
    Template,
}

impl Dialect {
    /// Whether `c` may appear in a bareword.
    pub(crate) fn allows_bare(self, c: char) -> bool {
        match c {
            ',' | ':' | '"' | '{' | '}' | '[' | ']' | '#' | '=' => false,
            '(' | ')' => self == Dialect::Data,
            // A literal control character is refused inside a string, and a key is a string —
            // it just happens to be one written without quotes. Allowing it here let a key
            // hold a raw `U+0001` that the same document could not write between quotes, and
            // let every emitter put one back out unquoted, since they all ask this predicate.
            c if is_control(c) => false,
            c => !c.is_whitespace(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// `,` `:` and whitespace separate tokens; `#` introduces a comment; `=` is reserved so that
/// `key = value` gets a real diagnostic instead of a confusing one.
pub(crate) fn tokenize(src: &str, dialect: Dialect) -> Result<Vec<Token>, Error> {
    Lexer {
        src,
        pos: 0,
        dialect,
    }
    .run()
}

/// Whether a key can be written without quotes. Keys are always strings, so this asks only
/// whether every character survives being unquoted. Every emitter goes through here so they
/// cannot drift apart on which keys need quoting.
///
/// The dialect matters: `"(a)"` is a bare key in `.tot`, and unquoting it in a `.tott` file
/// would turn a key into a form.
pub(crate) fn can_be_bare(key: &str, dialect: Dialect) -> bool {
    !key.is_empty() && key.chars().all(|c| dialect.allows_bare(c))
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    dialect: Dialect,
}

/// The byte offset a document's text begins at.
///
/// A leading byte-order mark is not part of it. The mark is not whitespace, so left alone it
/// would silently become the first character of the first key — and editors write them, so it
/// is skipped rather than reported.
///
/// Diagnostics ask for this too, and have to: a mark the lexer ignored must not occupy a column
/// either. It renders as nothing at all, so counting it would put every caret on the first line
/// one place to the right of what it points at. Only a mark that *leads* the file is skipped;
/// `U+FEFF` is not whitespace in Unicode, so anywhere else it is an ordinary bareword character.
pub(crate) fn body_start(src: &str) -> usize {
    if src.starts_with(BOM) {
        BOM.len_utf8()
    } else {
        0
    }
}

pub(crate) const BOM: char = '\u{feff}';

impl<'a> Lexer<'a> {
    fn run(mut self) -> Result<Vec<Token>, Error> {
        self.pos = body_start(self.src);

        let mut tokens = Vec::new();
        loop {
            self.skip_trivia()?;
            let start = self.pos;
            let Some(c) = self.peek() else { break };
            let kind = match c {
                '{' => {
                    self.pos += 1;
                    TokenKind::LBrace
                }
                '}' => {
                    self.pos += 1;
                    TokenKind::RBrace
                }
                '[' => {
                    self.pos += 1;
                    TokenKind::LBracket
                }
                ']' => {
                    self.pos += 1;
                    TokenKind::RBracket
                }
                '"' => self.lex_string()?,
                // In `.tot` these fall through to the bareword arm, which is what keeps
                // `(a) 1` the key `(a)` there.
                '(' if self.dialect == Dialect::Template => {
                    self.pos += 1;
                    TokenKind::LParen
                }
                ')' if self.dialect == Dialect::Template => {
                    self.pos += 1;
                    TokenKind::RParen
                }
                '=' => {
                    self.pos += 1;
                    return Err(Error::new(
                        Span::new(start, self.pos),
                        "tot has no assignment operator",
                    )
                    .with_help("write `key value`, not `key = value`"));
                }
                _ => {
                    while let Some(c) = self.peek() {
                        if !self.dialect.allows_bare(c) {
                            break;
                        }
                        self.pos += c.len_utf8();
                    }
                    TokenKind::Bareword
                }
            };
            tokens.push(Token {
                kind,
                span: Span::new(start, self.pos),
            });
        }
        Ok(tokens)
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn skip_trivia(&mut self) -> Result<(), Error> {
        loop {
            let Some(c) = self.peek() else { return Ok(()) };
            match c {
                ' ' | '\t' | '\n' | '\r' | ',' | ':' => self.pos += 1,
                '#' => {
                    while let Some(c) = self.peek() {
                        if c == '\n' {
                            break;
                        }
                        self.pos += c.len_utf8();
                    }
                }
                // Before the whitespace arm, so that U+000B and U+000C — which are both — are
                // named the way a string names them. A bareword cannot hold one of these, so
                // the lexer would otherwise stop dead at it and emit a token of no width.
                c if is_control(c) => return Err(control_char_outside_string(self.pos, c)),
                // Absorbing U+00A0 and friends into a bareword would be a silent surprise.
                c if c.is_whitespace() => {
                    let span = Span::new(self.pos, self.pos + c.len_utf8());
                    return Err(Error::new(
                        span,
                        format!("unexpected whitespace character U+{:04X}", c as u32),
                    )
                    .with_help(
                        "only space, tab, CR, and LF delimit tokens; quote the key if this \
                         character belongs to it",
                    ));
                }
                _ => return Ok(()),
            }
        }
    }

    fn lex_string(&mut self) -> Result<TokenKind, Error> {
        if self.src[self.pos..].starts_with("\"\"\"") {
            self.lex_multiline()
        } else {
            self.lex_single_line()
        }
    }

    fn lex_single_line(&mut self) -> Result<TokenKind, Error> {
        let start = self.pos;
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(
                    Error::new(Span::new(start, self.pos), "unterminated string")
                        .with_help("expected a closing `\"`"),
                );
            };
            match c {
                '"' => {
                    self.pos += 1;
                    return Ok(TokenKind::Str(out));
                }
                '\\' => unescape_at(self.src, &mut self.pos, &mut out)?,
                '\n' | '\r' => {
                    return Err(
                        Error::new(Span::new(start, self.pos), "unterminated string").with_help(
                            "a single-line string may not span lines; use `\"\"\"` for a \
                             multi-line string",
                        ),
                    );
                }
                c if (c as u32) < 0x20 => {
                    return Err(control_char_error(self.pos, c));
                }
                c => {
                    self.pos += c.len_utf8();
                    out.push(c);
                }
            }
        }
    }

    /// A `"""` string. Content starts on the line after the opening delimiter; the whitespace
    /// before the closing delimiter is stripped from every line, so that reindenting the
    /// block the string lives in cannot change its value.
    fn lex_multiline(&mut self) -> Result<TokenKind, Error> {
        let src = self.src;
        let bytes = src.as_bytes();
        let start = self.pos;
        self.pos += 3;

        // Only horizontal whitespace may follow the opening delimiter.
        let mut p = self.pos;
        while matches!(bytes.get(p), Some(b' ' | b'\t')) {
            p += 1;
        }
        match bytes.get(p) {
            None => return Err(unterminated_block(start)),
            Some(b'\r') if bytes.get(p + 1) == Some(&b'\n') => p += 2,
            Some(b'\n') => p += 1,
            Some(_) => {
                return Err(Error::new(
                    Span::new(p, p + 1),
                    "content may not follow the opening `\"\"\"`",
                )
                .with_help("a multi-line string begins on the line after its opening delimiter"));
            }
        }

        // The closing delimiter is the first `"""` on a line preceded only by horizontal
        // whitespace; that whitespace is the indentation prefix.
        let mut lines: Vec<(usize, usize)> = Vec::new();
        let mut line_start = p;
        let (prefix, close_end) = loop {
            let newline = src[line_start..].find('\n').map(|i| line_start + i);
            let text_end = match newline {
                Some(e) if e > line_start && bytes[e - 1] == b'\r' => e - 1,
                Some(e) => e,
                None => bytes.len(),
            };
            let line = &src[line_start..text_end];
            let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
            if line[indent..].starts_with("\"\"\"") {
                // The delimiter owns its whole line, at both ends. Anything after it is
                // almost always an unescaped `"""` inside the content; treating it as the
                // next token instead would close the string here and land the error on some
                // later line that has nothing to do with the mistake.
                let after = line_start + indent + 3;
                let rest = &src[after..text_end];
                if !rest.chars().all(|c| c == ' ' || c == '\t') {
                    return Err(Error::new(
                        Span::new(after, text_end),
                        "content after the closing `\"\"\"`",
                    )
                    .with_help(
                        "the closing delimiter ends its line; write `\\\"\"\"` for a literal \
                         `\"\"\"` at the start of a content line",
                    ));
                }
                break (&line[..indent], after);
            }
            let Some(e) = newline else {
                return Err(unterminated_block(start));
            };
            lines.push((line_start, text_end));
            line_start = e + 1;
        };

        let mut out = String::new();
        let mut pending_newline = false;
        for &(s, e) in &lines {
            let line = &src[s..e];
            let (seg_start, seg_end) = if line.bytes().all(|b| b == b' ' || b == b'\t') {
                (s, s) // a whitespace-only line contributes an empty line
            } else if line.starts_with(prefix) {
                (s + prefix.len(), e)
            } else {
                return Err(Error::new(
                    Span::new(s, e),
                    "line is not indented to match the closing `\"\"\"`",
                )
                .with_help(format!(
                    "every line must begin with the same {} character(s) of whitespace as the \
                     closing delimiter",
                    prefix.chars().count()
                )));
            };

            if pending_newline {
                out.push('\n');
            }
            pending_newline = true;

            let mut q = seg_start;
            while q < seg_end {
                let c = src[q..].chars().next().expect("q < seg_end");
                if c == '\\' {
                    if q + 1 == seg_end {
                        pending_newline = false; // trailing `\` is a line continuation
                        break;
                    }
                    unescape_at(src, &mut q, &mut out)?;
                } else if (c as u32) < 0x20 && c != '\t' {
                    return Err(control_char_error(q, c));
                } else {
                    out.push(c);
                    q += c.len_utf8();
                }
            }
        }

        self.pos = close_end;
        Ok(TokenKind::Str(out))
    }
}

fn unterminated_block(start: usize) -> Error {
    Error::new(
        Span::new(start, start + 3),
        "unterminated multi-line string",
    )
    .with_help("expected a closing `\"\"\"` preceded only by whitespace on its line")
}

/// A character tot never holds literally. JSON forbids these inside a string, and tot forbids
/// them in a bareword for the same reason plus one of its own: a key is a string that happens
/// to be written without quotes, so the same key must not be legal one way and refused the
/// other. Every emitter asks [`can_be_bare`], so this is also what stops one being written back
/// out raw.
fn is_control(c: char) -> bool {
    (c as u32) < 0x20
}

fn control_char_error(pos: usize, c: char) -> Error {
    Error::new(
        Span::new(pos, pos + c.len_utf8()),
        format!("literal control character U+{:04X} in string", c as u32),
    )
    .with_help("write it as an escape")
}

fn control_char_outside_string(pos: usize, c: char) -> Error {
    Error::new(
        Span::new(pos, pos + c.len_utf8()),
        format!("literal control character U+{:04X}", c as u32),
    )
    .with_help(format!(
        "a control character is not a bareword character; quote the key and escape it, \
         as in `\"a\\u{:04x}b\"`",
        c as u32
    ))
}

/// Consumes one escape sequence and appends its value. On entry `*pos` is at the backslash.
/// Shared by both string forms, and by path segments, so that escapes and their diagnostics
/// stay identical everywhere a quoted string can appear.
pub(crate) fn unescape_at(src: &str, pos: &mut usize, out: &mut String) -> Result<(), Error> {
    let esc_start = *pos;
    *pos += 1;
    let Some(c) = src[*pos..].chars().next() else {
        return Err(Error::new(
            Span::new(esc_start, *pos),
            "unterminated escape sequence",
        ));
    };
    *pos += c.len_utf8();
    let ch = match c {
        '"' => '"',
        '\\' => '\\',
        '/' => '/',
        'b' => '\u{8}',
        'f' => '\u{c}',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        'u' => return unicode_escape_at(src, pos, esc_start, out),
        other => {
            return Err(Error::new(
                Span::new(esc_start, *pos),
                format!("unknown escape `\\{other}`"),
            )
            .with_help(r#"valid escapes are \" \\ \/ \b \f \n \r \t and \uXXXX"#));
        }
    };
    out.push(ch);
    Ok(())
}

fn unicode_escape_at(
    src: &str,
    pos: &mut usize,
    esc_start: usize,
    out: &mut String,
) -> Result<(), Error> {
    let hi = hex4_at(src, pos, esc_start)?;
    let cp = if (0xD800..0xDC00).contains(&hi) {
        if !src[*pos..].starts_with("\\u") {
            return Err(Error::new(
                Span::new(esc_start, *pos),
                "unpaired high surrogate in string",
            )
            .with_help(
                "a `\\uD800`–`\\uDBFF` escape must be followed by a `\\uDC00`–`\\uDFFF` one",
            ));
        }
        *pos += 2;
        let lo = hex4_at(src, pos, esc_start)?;
        if !(0xDC00..0xE000).contains(&lo) {
            return Err(Error::new(
                Span::new(esc_start, *pos),
                "high surrogate is not followed by a low surrogate",
            ));
        }
        0x1_0000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
    } else if (0xDC00..0xE000).contains(&hi) {
        return Err(Error::new(
            Span::new(esc_start, *pos),
            "unpaired low surrogate in string",
        ));
    } else {
        hi
    };
    let ch = char::from_u32(cp).ok_or_else(|| {
        Error::new(
            Span::new(esc_start, *pos),
            "escape is not a valid Unicode scalar value",
        )
    })?;
    out.push(ch);
    Ok(())
}

fn hex4_at(src: &str, pos: &mut usize, esc_start: usize) -> Result<u32, Error> {
    let digits = src.get(*pos..*pos + 4).unwrap_or("");
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::new(
            Span::new(esc_start, (*pos + 4).min(src.len())),
            "`\\u` must be followed by exactly four hex digits",
        ));
    }
    *pos += 4;
    Ok(u32::from_str_radix(digits, 16).expect("validated above"))
}
