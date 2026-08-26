//! serde support, behind the `serde` feature.
//!
//! Both directions go through [`Value`](crate::Value) rather than straight to and from text.
//! The formatter already knows how to write a `Value` well — block strings, bare keys,
//! indentation — and the parser already knows how to read one, so a streaming implementation
//! would be a second copy of both with nothing to show for it. Config documents are small.
//!
//! ```
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! struct Config {
//!     name: String,
//!     listen: Listen,
//! }
//!
//! #[derive(Debug, PartialEq, Serialize, Deserialize)]
//! struct Listen {
//!     host: String,
//!     port: u16,
//! }
//!
//! let src = r#"
//!     name "svc"
//!     listen { host "0.0.0.0" port 8080 }
//! "#;
//!
//! let config: Config = tot::from_str(src).unwrap();
//! assert_eq!(config.listen.port, 8080);
//!
//! // Writing back gives block form, as every converter does.
//! assert_eq!(
//!     tot::to_string(&config).unwrap(),
//!     "name \"svc\"\nlisten {\n  host \"0.0.0.0\"\n  port 8080\n}\n"
//! );
//! ```
//!
//! A *document* is not what round-trips here — a *value* is. Comments, blank lines, and the
//! author's inline-versus-block choices live in the CST, which serde never sees. Use
//! [`format`](crate::format) when the text itself has to survive.

pub mod de;
pub mod ser;

pub use de::{Deserializer, from_str, from_value};
pub use ser::{Serializer, to_string, to_value};

use std::fmt;

use crate::path::as_segment;

/// A serialization or deserialization failure.
///
/// Where it can, the error names the value it failed on, spelled the way a path is spelled —
/// so the location in `expected an integer at \`listen.port\`` can be pasted straight into
/// `tot get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    message: String,
    /// The route to the offending value, collected leaf-first as the error unwinds.
    path: Vec<Part>,
    /// Set only when the document did not parse at all, so a caller can still get a caret.
    parse: Option<crate::Error>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Key(String),
    Index(usize),
}

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Error {
            message: message.into(),
            path: Vec::new(),
            parse: None,
        }
    }

    /// Records that the failing value was the member `key` of the value being worked on.
    pub(crate) fn at_key(mut self, key: &str) -> Self {
        self.path.push(Part::Key(key.to_string()));
        self
    }

    /// Records that the failing value was element `index` of the value being worked on.
    pub(crate) fn at_index(mut self, index: usize) -> Self {
        self.path.push(Part::Index(index));
        self
    }

    /// What went wrong, without the location.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The path to the offending value, or `None` at the root.
    ///
    /// Spelled the way [`Path`](crate::Path) spells it, so it can be handed straight to
    /// `tot get` or [`Path::parse`](crate::Path::parse).
    pub fn path(&self) -> Option<String> {
        if self.path.is_empty() {
            return None;
        }
        let mut out = String::new();
        for part in self.path.iter().rev() {
            match part {
                Part::Key(key) => {
                    if !out.is_empty() {
                        out.push('.');
                    }
                    out.push_str(&as_segment(key));
                }
                Part::Index(index) => out.push_str(&format!("[{index}]")),
            }
        }
        Some(out)
    }

    /// The underlying parse failure, when [`from_str`] could not read the document at all.
    ///
    /// Only this kind of error has a span, so only this kind can be rendered with a caret.
    pub fn parse_error(&self) -> Option<&crate::Error> {
        self.parse.as_ref()
    }
}

impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        Error {
            message: e.to_string(),
            path: Vec::new(),
            parse: Some(e),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(path) = self.path() {
            write!(f, " at `{path}`")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.parse
            .as_ref()
            .map(|e| e as &(dyn std::error::Error + 'static))
    }
}

impl serde::ser::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Error::new(message.to_string())
    }
}

impl serde::de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Error::new(message.to_string())
    }
}
