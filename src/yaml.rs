//! YAML input and output.
//!
//! Reading resolves aliases and inlines them, and refuses the three things tot has no
//! equivalent for: tags, non-string keys, and multi-document streams. Writing loses nothing
//! but a float's spelling — `1.00` comes back `1.0`, and `Float` equality is lexical. Behind
//! the `yaml` feature because it pulls the one third-party parser involved.

use crate::convert::{at, child, index};
use crate::error::ConvertError;
use crate::value::{Float, Integer, Map, Value};

/// Reads a YAML document as a [`Value`].
pub fn from_str(src: &str) -> Result<Value, ConvertError> {
    let yaml: yaml_serde::Value =
        yaml_serde::from_str(src).map_err(|e| ConvertError::new(e.to_string()))?;
    yaml_to_tot(&yaml, "")
}

/// Writes a [`Value`] as YAML.
pub fn to_string(value: &Value) -> Result<String, ConvertError> {
    let yaml = tot_to_yaml(value, "")?;
    yaml_serde::to_string(&yaml).map_err(|e| ConvertError::new(e.to_string()))
}

fn yaml_to_tot(value: &yaml_serde::Value, path: &str) -> Result<Value, ConvertError> {
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

fn tot_to_yaml(value: &Value, path: &str) -> Result<yaml_serde::Value, ConvertError> {
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
