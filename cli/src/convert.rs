//! Conversions between `tot::Value` and the foreign value types.
//!
//! JSON is absent on purpose. `,` and `:` are whitespace in tot, so reading JSON is just
//! `tot::parse`, and writing it is `tot::json` — neither needs a dependency or a line of
//! code here.

use tot::{Float, Integer, Map, Value};

/// What to do with a `null` on the way into TOML, which has no such value.
#[derive(Clone, Copy)]
pub enum NullPolicy {
    /// Drop the member or element, reporting the path.
    Omit,
    /// Refuse to convert.
    Error,
}

// --- YAML ---------------------------------------------------------------------------------

pub fn from_yaml(src: &str) -> Result<Value, String> {
    let yaml: yaml_serde::Value = yaml_serde::from_str(src).map_err(|e| e.to_string())?;
    yaml_to_tot(&yaml, "")
}

pub fn to_yaml(value: &Value) -> Result<String, String> {
    let yaml = tot_to_yaml(value, "")?;
    yaml_serde::to_string(&yaml).map_err(|e| e.to_string())
}

fn yaml_to_tot(value: &yaml_serde::Value, path: &str) -> Result<Value, String> {
    use yaml_serde::Value as Y;
    Ok(match value {
        Y::Null => Value::Null,
        Y::Bool(b) => Value::Bool(*b),
        Y::String(s) => Value::String(s.clone()),
        Y::Number(n) => {
            if n.is_i64() {
                Value::Integer(Integer::from_i64(n.as_i64().expect("is_i64")))
            } else if n.is_u64() {
                Value::Integer(Integer::from_u64(n.as_u64().expect("is_u64")))
            } else {
                let f = n
                    .as_f64()
                    .ok_or_else(|| at(path, "unrepresentable number"))?;
                Value::Float(
                    Float::from_f64(f)
                        .ok_or_else(|| at(path, &format!("tot cannot write the float `{f}`")))?,
                )
            }
        }
        Y::Sequence(items) => Value::Array(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| yaml_to_tot(item, &index(path, i)))
                .collect::<Result<_, _>>()?,
        ),
        Y::Mapping(mapping) => {
            let mut map = Map::new();
            for (key, member) in mapping {
                let Y::String(key) = key else {
                    return Err(at(
                        path,
                        "tot keys are always strings, and this mapping has one that is not",
                    ));
                };
                let value = yaml_to_tot(member, &child(path, key))?;
                if !map.insert(key.clone(), value) {
                    return Err(at(path, &format!("duplicate key `{key}`")));
                }
            }
            Value::Object(map)
        }
        Y::Tagged(tagged) => {
            return Err(at(
                path,
                &format!("tot has no equivalent for the YAML tag `{}`", tagged.tag),
            ));
        }
    })
}

fn tot_to_yaml(value: &Value, path: &str) -> Result<yaml_serde::Value, String> {
    use yaml_serde::Value as Y;
    Ok(match value {
        Value::Null => Y::Null,
        Value::Bool(b) => Y::Bool(*b),
        Value::String(s) => Y::String(s.clone()),
        Value::Integer(i) => {
            if let Some(v) = i.as_i64() {
                Y::Number(v.into())
            } else if let Some(v) = i.as_u64() {
                Y::Number(v.into())
            } else {
                return Err(at(
                    path,
                    &format!("`{}` does not fit in a 64-bit YAML integer", i.as_str()),
                ));
            }
        }
        Value::Float(f) => Y::Number(f.as_f64().into()),
        Value::Array(items) => Y::Sequence(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| tot_to_yaml(item, &index(path, i)))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(map) => {
            let mut mapping = yaml_serde::Mapping::new();
            for (key, member) in map.iter() {
                mapping.insert(
                    Y::String(key.to_string()),
                    tot_to_yaml(member, &child(path, key))?,
                );
            }
            Y::Mapping(mapping)
        }
    })
}

// --- TOML ---------------------------------------------------------------------------------

/// Returns the document plus the paths of any datetimes, which became strings.
pub fn from_toml(src: &str) -> Result<(Value, Vec<String>), String> {
    let parsed: toml::Value = toml::from_str(src).map_err(|e| e.to_string())?;
    let mut datetimes = Vec::new();
    let value = toml_to_tot(&parsed, "", &mut datetimes)?;
    Ok((value, datetimes))
}

/// Returns the document plus the paths of any nulls that were dropped.
pub fn to_toml(value: &Value, nulls: NullPolicy) -> Result<(String, Vec<String>), String> {
    let mut dropped = Vec::new();
    let Some(toml::Value::Table(table)) = tot_to_toml(value, "", &mut dropped, nulls)? else {
        return Err(
            "TOML needs a table at the root, and this document's root is not an object".into(),
        );
    };
    let text = toml::to_string_pretty(&table).map_err(|e| e.to_string())?;
    Ok((text, dropped))
}

fn toml_to_tot(
    value: &toml::Value,
    path: &str,
    datetimes: &mut Vec<String>,
) -> Result<Value, String> {
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
) -> Result<Option<toml::Value>, String> {
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

// --- paths, for diagnostics ---------------------------------------------------------------

fn child(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

fn index(path: &str, i: usize) -> String {
    format!("{path}[{i}]")
}

fn display(path: &str) -> String {
    if path.is_empty() {
        "the document root".to_string()
    } else {
        path.to_string()
    }
}

fn at(path: &str, message: &str) -> String {
    format!("{}: {message}", display(path))
}
