//! TOML input and output.
//!
//! Reading turns datetimes into strings — tot has no date type — and reports the path of every
//! one it turned. Writing drops nulls, reporting the path of each (TOML has none), or refuses
//! instead per [`NullPolicy`]; the root has to be an object, and sub-tables land below plain
//! values because TOML's syntax demands it. Behind the `toml` feature because it pulls the one
//! third-party parser involved.

use crate::convert::{at, child, display, index};
use crate::error::ConvertError;
use crate::value::{Float, Integer, Map, Value};

/// What to do with a `null` on the way into TOML, which has no such value.
///
/// `non_exhaustive`, so that a third answer — writing the empty string, say — can be added
/// without breaking anyone. Naming a variant is unaffected; only matching on one needs a `_`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum NullPolicy {
    /// Drop the member or element, reporting the path.
    #[default]
    Omit,
    /// Refuse to convert.
    Error,
}

/// What reading TOML produced.
///
/// `non_exhaustive`: this is a report, never something a caller builds, and the lossy steps a
/// conversion has to own up to are the thing most likely to grow. Read the fields, or destructure
/// with a trailing `..`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FromToml {
    /// The document.
    pub value: Value,
    /// The paths of any datetimes, which became strings.
    pub datetimes: Vec<String>,
}

/// What writing TOML produced.
///
/// `non_exhaustive`, for the reasons on [`FromToml`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ToToml {
    /// The document as TOML text.
    pub text: String,
    /// The paths of any nulls that were dropped.
    pub dropped: Vec<String>,
}

/// Reads a TOML document as a [`Value`], reporting any datetimes it had to turn into strings.
pub fn from_str(src: &str) -> Result<FromToml, ConvertError> {
    let parsed: toml::Value = toml::from_str(src).map_err(|e| ConvertError::new(e.to_string()))?;
    let mut datetimes = Vec::new();
    let value = toml_to_tot(&parsed, "", &mut datetimes)?;
    Ok(FromToml { value, datetimes })
}

/// Writes a [`Value`] as TOML, reporting any nulls the policy dropped.
pub fn to_string(value: &Value, nulls: NullPolicy) -> Result<ToToml, ConvertError> {
    let mut dropped = Vec::new();
    let table = match tot_to_toml(value, "", &mut dropped, nulls)? {
        Some(toml::Value::Table(table)) => table,
        // `Ok(None)` is the root itself being a null that `NullPolicy::Omit` dropped, which
        // leaves no document at all rather than one of the wrong shape. Calling that a root
        // that "is not an object" names a cause the reader cannot act on: they would go
        // looking for the object they wrote, when what they have is a policy question.
        None => {
            return Err(ConvertError::new(
                "TOML needs a table at the root, and this document is a single null",
            ));
        }
        Some(_) => {
            return Err(ConvertError::new(
                "TOML needs a table at the root, and this document's root is not an object",
            ));
        }
    };
    let text = toml::to_string_pretty(&table).map_err(|e| ConvertError::new(e.to_string()))?;
    Ok(ToToml { text, dropped })
}

fn toml_to_tot(
    value: &toml::Value,
    path: &str,
    datetimes: &mut Vec<String>,
) -> Result<Value, ConvertError> {
    use toml::Value as T;
    Ok(match value {
        T::String(s) => Value::String(s.clone()),
        T::Boolean(b) => Value::Bool(*b),
        T::Integer(i) => Value::Integer(Integer::from_i64(*i)),
        T::Float(f) => Value::Float(
            Float::from_f64(*f)
                .ok_or_else(|| at(path, &format!("tot cannot write the float `{f}`")))?,
        ),
        // tot has no date type by design, so this is the one lossy step.
        T::Datetime(datetime) => {
            datetimes.push(display(path));
            Value::String(datetime.to_string())
        }
        T::Array(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| toml_to_tot(item, &index(path, i), datetimes))
                .collect::<Result<_, _>>()?,
        ),
        T::Table(table) => {
            let mut map = Map::new();
            for (key, member) in table {
                let value = toml_to_tot(member, &child(path, key), datetimes)?;
                if !map.insert(key.clone(), value) {
                    return Err(at(path, &format!("duplicate key `{key}`")));
                }
            }
            Value::Object(map)
        }
    })
}

/// `Ok(None)` means the value was a null that the policy says to drop.
fn tot_to_toml(
    value: &Value,
    path: &str,
    dropped: &mut Vec<String>,
    nulls: NullPolicy,
) -> Result<Option<toml::Value>, ConvertError> {
    use toml::Value as T;
    Ok(Some(match value {
        Value::Null => {
            if matches!(nulls, NullPolicy::Error) {
                return Err(at(path, "TOML has no null"));
            }
            dropped.push(display(path));
            return Ok(None);
        }
        Value::Bool(b) => T::Boolean(*b),
        Value::String(s) => T::String(s.clone()),
        Value::Integer(i) => T::Integer(i.as_i64().ok_or_else(|| {
            at(
                path,
                &format!(
                    "TOML integers are 64-bit signed, and `{}` does not fit",
                    i.as_str()
                ),
            )
        })?),
        Value::Float(f) => T::Float(f.as_f64()),
        Value::Array(items) => {
            let mut out = Vec::new();
            for (i, item) in items.iter().enumerate() {
                if let Some(value) = tot_to_toml(item, &index(path, i), dropped, nulls)? {
                    out.push(value);
                }
            }
            T::Array(out)
        }
        Value::Object(map) => {
            let mut table = toml::Table::new();
            for (key, member) in map.iter() {
                if let Some(value) = tot_to_toml(member, &child(path, key), dropped, nulls)? {
                    table.insert(key.to_string(), value);
                }
            }
            T::Table(table)
        }
    }))
}
