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

// Puts an "Available on crate feature `yaml`" badge on everything behind a feature gate, so a
// reader on docs.rs is told what to turn on rather than finding out from a build error. Only
// docs.rs sets `docsrs` (through `rustdoc-args` in the manifest) and only docs.rs builds on
// nightly; with the cfg unset this expands to nothing, so stable and the 1.88 MSRV check never
// see a `feature` attribute. `doc_auto_cfg` was the spelling until it was removed in 1.92 and
// folded into `doc_cfg`, which now does the automatic part on its own.
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(any(feature = "yaml", feature = "toml"))]
mod convert;
mod cst;
mod error;
mod fmt;
mod lex;
mod lint;
mod merge;
mod parse;
mod path;
mod schema;
mod value;

pub mod json;
pub mod template;
#[cfg(feature = "toml")]
pub mod toml;
#[cfg(feature = "yaml")]
pub mod yaml;

#[cfg(feature = "serde")]
pub mod serde;

#[cfg(any(feature = "yaml", feature = "toml"))]
pub use error::ConvertError;
pub use error::{Error, Span};
pub use fmt::{format, format_template, format_value};
pub use lex::Dialect;
pub use lint::{Warning, lint, lint_template};
pub use merge::{Nulls, merge, merge_into};
pub use parse::{parse, parse_value};
pub use path::{Missing, Path};
pub use schema::{Schema, Violation};
#[cfg(feature = "serde")]
pub use serde::{from_str, from_value, to_string, to_value};
pub use template::{BuildError, Params, Template};
pub use value::{Float, Integer, Map, Value};
