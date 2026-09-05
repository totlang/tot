//! Path spelling shared by the converters, for their diagnostics.
//!
//! Every path a converter reports is a real path, so it can be handed to `tot get`. A key is
//! spelled the way a path spells one — joining with a bare `.` would turn a key holding a dot
//! into a chain of members that do not exist, and one holding a space into nothing at all.

use crate::error::ConvertError;
use crate::path::Path;

pub(crate) fn child(path: &str, key: &str) -> String {
    let segment = Path::segment(key);
    if path.is_empty() {
        segment
    } else {
        format!("{path}.{segment}")
    }
}

pub(crate) fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

pub(crate) fn display(path: &str) -> String {
    if path.is_empty() {
        "the document root".to_string()
    } else {
        path.to_string()
    }
}

pub(crate) fn at(path: &str, message: &str) -> ConvertError {
    ConvertError::new(format!("{}: {message}", display(path)))
}
