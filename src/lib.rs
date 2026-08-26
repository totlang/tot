//! `tot` — JSON with the punctuation removed.
//!
//! A tot document is a sequence of `key value` pairs with no separators. Whitespace is only
//! a delimiter, never structure; `,` and `:` are treated as whitespace, which makes every
//! JSON document a valid tot document. See `SPEC.md` for the language definition.
//!
//! ```
//! let value = tot::parse(r#"
//!     name "tim"
//!     address { city "sf" zip 94102 }
//! "#).unwrap();
//!
//! assert_eq!(
//!     tot::json::to_string(&value),
//!     r#"{"name":"tim","address":{"city":"sf","zip":94102}}"#
//! );
//! ```

mod cst;
mod error;
mod fmt;
mod lex;
mod lint;
mod merge;
mod parse;
mod path;
mod value;

pub mod json;

#[cfg(feature = "serde")]
pub mod serde;

pub use error::{Error, Span};
pub use fmt::{format, format_value};
pub use lint::{Warning, lint};
pub use merge::{Nulls, merge, merge_into};
pub use parse::{parse, parse_value};
pub use path::{Missing, Path};
#[cfg(feature = "serde")]
pub use serde::{from_str, from_value, to_string, to_value};
pub use value::{Float, Integer, Map, Value};
