//! Folding documents together: a base plus overlays.
//!
//! The rules are chosen to be predictable rather than clever, because an overlay that
//! surprises you is worse than one that makes you say a little more.

use crate::value::{Map, Value};

/// What a `null` in an overlay means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Nulls {
    /// Set the member to `null`, which is an ordinary value in tot like any other.
    #[default]
    Set,
    /// Remove the member from the base instead, so an overlay can take something away.
    Delete,
}

/// Folds documents together, left to right.
///
/// No documents is the empty object, and one document is itself.
///
/// ```
/// let documents = ["a 1 b 2", "b 3 c 4"].map(|src| tot::parse(src).unwrap());
/// let merged = tot::merge(documents, tot::Nulls::Set);
/// assert_eq!(tot::json::to_string(&merged), r#"{"a":1,"b":3,"c":4}"#);
/// ```
pub fn merge<I: IntoIterator<Item = Value>>(documents: I, nulls: Nulls) -> Value {
    let mut documents = documents.into_iter();
    let mut base = documents
        .next()
        .unwrap_or_else(|| Value::Object(Map::new()));
    for overlay in documents {
        merge_into(&mut base, overlay, nulls);
    }
    base
}

/// Folds one overlay into one base.
///
/// **Two objects merge member by member; anything else is replaced whole.** An array replaces
/// rather than appending: concatenation cannot be undone by a later overlay, so an overlay
/// that could only ever add would be a one-way door. A change of kind replaces for the same
/// reason — there is no sensible way to fold a string into an array, and guessing at one is
/// how these systems become unpredictable.
///
/// Members the base already has keep their position; members only the overlay has are
/// appended in the order the overlay wrote them.
///
/// ```
/// let mut base = tot::parse("listen {host \"::\" port 80} debug false").unwrap();
/// let overlay = tot::parse("listen {port 8080}").unwrap();
///
/// tot::merge_into(&mut base, overlay, tot::Nulls::Set);
/// assert_eq!(
///     tot::format_value(&base),
///     "listen {\n  host \"::\"\n  port 8080\n}\ndebug false\n"
/// );
/// ```
pub fn merge_into(base: &mut Value, overlay: Value, nulls: Nulls) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => {
            for (key, value) in overlay {
                // Deleting is a member-level operation, so it lives here rather than in the
                // recursive step — there is nothing for a root `null` to be removed from.
                if nulls == Nulls::Delete && matches!(value, Value::Null) {
                    base.remove(&key);
                    continue;
                }
                match base.get_mut(&key) {
                    Some(slot) => merge_into(slot, value, nulls),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}
