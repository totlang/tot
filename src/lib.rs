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
mod parse;
mod value;

pub mod json;

pub use error::{Error, Span};
pub use fmt::{format, format_value};
pub use parse::parse;
pub use value::{Float, Integer, Map, Value};
