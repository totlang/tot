use crate::error::{Error, Span};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TokenKind {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
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
            TokenKind::Bareword => "a bareword",
            TokenKind::Str(_) => "a string",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub(crate) fn tokenize(src: &str) -> Result<Vec<Token>, Error> {
    Lexer { src, pos: 0 }.run()
}

/// `,` `:` and whitespace separate tokens; `#` introduces a comment; `=` is reserved so that
/// `key = value` gets a real diagnostic instead of a confusing one.
pub(crate) fn is_bareword_char(c: char) -> bool {
    !matches!(c, ',' | ':' | '"' | '{' | '}' | '[' | ']' | '#' | '=') && !c.is_whitespace()
}

/// Whether a key can be written without quotes. Keys are always strings, so this asks only
/// whether every character survives being unquoted. Both emitters go through here so they
/// cannot drift apart on which keys need quoting.
pub(crate) fn can_be_bare(key: &str) -> bool {
    !key.is_empty() && key.chars().all(is_bareword_char)
}

struct Lexer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn run(mut self) -> Result<Vec<Token>, Error> {
        // A byte-order mark is not whitespace, so left alone it would silently become the
        // first character of the first key. Editors write them; skip one.
        if self.src.starts_with('\u{feff}') {
            self.pos += '\u{feff}'.len_utf8();
        }

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
                        if !is_bareword_char(c) {
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
                break (&line[..indent], line_start + indent + 3);
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

fn control_char_error(pos: usize, c: char) -> Error {
    Error::new(
        Span::new(pos, pos + c.len_utf8()),
        format!("literal control character U+{:04X} in string", c as u32),
    )
    .with_help("write it as an escape")
}

/// Consumes one escape sequence and appends its value. On entry `*pos` is at the backslash.
/// Shared by both string forms so that escapes and their diagnostics stay identical.
fn unescape_at(src: &str, pos: &mut usize, out: &mut String) -> Result<(), Error> {
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
