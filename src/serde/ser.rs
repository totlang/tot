//! `T: Serialize` into a [`Value`], and from there into tot text.

use serde::ser::{self, Serialize};

use super::Error;
use crate::value::{Float, Integer, Map, Value};

/// Serialize a value into tot text.
///
/// ```
/// let map = std::collections::BTreeMap::from([("port", 8080)]);
/// assert_eq!(tot::to_string(&map).unwrap(), "port 8080\n");
/// ```
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    Ok(crate::format_value(&to_value(value)?))
}

/// Serialize a value into a [`Value`].
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, Error> {
    value.serialize(Serializer)
}

/// Builds a [`Value`]. See [`to_value`].
pub struct Serializer;

impl ser::Serializer for Serializer {
    type Ok = Value;
    type Error = Error;

    type SerializeSeq = SerializeArray;
    type SerializeTuple = SerializeArray;
    type SerializeTupleStruct = SerializeArray;
    type SerializeTupleVariant = SerializeArray;
    type SerializeMap = SerializeObject;
    type SerializeStruct = SerializeObject;
    type SerializeStructVariant = SerializeObject;

    fn serialize_bool(self, v: bool) -> Result<Value, Error> {
        Ok(Value::Bool(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Value, Error> {
        self.serialize_i64(v.into())
    }

    fn serialize_i16(self, v: i16) -> Result<Value, Error> {
        self.serialize_i64(v.into())
    }

    fn serialize_i32(self, v: i32) -> Result<Value, Error> {
        self.serialize_i64(v.into())
    }

    fn serialize_i64(self, v: i64) -> Result<Value, Error> {
        Ok(Value::Integer(Integer::from_i64(v)))
    }

    fn serialize_i128(self, v: i128) -> Result<Value, Error> {
        Ok(Value::Integer(Integer::from_i128(v)))
    }

    fn serialize_u8(self, v: u8) -> Result<Value, Error> {
        self.serialize_u64(v.into())
    }

    fn serialize_u16(self, v: u16) -> Result<Value, Error> {
        self.serialize_u64(v.into())
    }

    fn serialize_u32(self, v: u32) -> Result<Value, Error> {
        self.serialize_u64(v.into())
    }

    fn serialize_u64(self, v: u64) -> Result<Value, Error> {
        Ok(Value::Integer(Integer::from_u64(v)))
    }

    fn serialize_u128(self, v: u128) -> Result<Value, Error> {
        Ok(Value::Integer(Integer::from_u128(v)))
    }

    fn serialize_f32(self, v: f32) -> Result<Value, Error> {
        Float::from_f32(v)
            .map(Value::Float)
            .ok_or_else(|| unwritable(v))
    }

    fn serialize_f64(self, v: f64) -> Result<Value, Error> {
        Float::from_f64(v)
            .map(Value::Float)
            .ok_or_else(|| unwritable(v))
    }

    fn serialize_char(self, v: char) -> Result<Value, Error> {
        Ok(Value::String(v.to_string()))
    }

    fn serialize_str(self, v: &str) -> Result<Value, Error> {
        Ok(Value::String(v.to_string()))
    }

    /// tot has no byte string, so bytes become an array of integers, which is what
    /// `Vec<u8>` reads back as.
    fn serialize_bytes(self, v: &[u8]) -> Result<Value, Error> {
        Ok(Value::Array(
            v.iter()
                .map(|b| Value::Integer(Integer::from_u64((*b).into())))
                .collect(),
        ))
    }

    fn serialize_none(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<Value, Error> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Value, Error> {
        Ok(Value::Null)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Value, Error> {
        Ok(Value::String(variant.to_string()))
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Value, Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Value, Error> {
        let inner = value.serialize(self).map_err(|e| e.at_key(variant))?;
        Ok(wrap(variant, inner))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SerializeArray, Error> {
        Ok(SerializeArray {
            items: Vec::with_capacity(len.unwrap_or(0)),
            variant: None,
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SerializeArray, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SerializeArray, Error> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SerializeArray, Error> {
        Ok(SerializeArray {
            items: Vec::with_capacity(len),
            variant: Some(variant),
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<SerializeObject, Error> {
        Ok(SerializeObject {
            map: Map::new(),
            key: None,
            variant: None,
        })
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<SerializeObject, Error> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<SerializeObject, Error> {
        Ok(SerializeObject {
            map: Map::new(),
            key: None,
            variant: Some(variant),
        })
    }
}

/// An externally tagged variant: `{ Variant <payload> }`, the shape serde's own
/// derive reads back.
fn wrap(variant: &str, value: Value) -> Value {
    let mut map = Map::new();
    map.insert(variant.to_string(), value);
    Value::Object(map)
}

/// The only float a `Value` cannot hold. JSON has the same hole; unlike JSON's encoders, this
/// says so rather than writing `null`.
fn unwritable(value: impl std::fmt::Display) -> Error {
    Error::new(format!(
        "cannot write `{value}`: tot has no infinity and no NaN"
    ))
}

pub struct SerializeArray {
    items: Vec<Value>,
    /// Set when this array is an enum variant's payload, and has to be wrapped at the end.
    variant: Option<&'static str>,
}

impl SerializeArray {
    fn push<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let index = self.items.len();
        self.items
            .push(value.serialize(Serializer).map_err(|e| e.at_index(index))?);
        Ok(())
    }

    fn finish(self) -> Value {
        let array = Value::Array(self.items);
        match self.variant {
            Some(variant) => wrap(variant, array),
            None => array,
        }
    }
}

impl ser::SerializeSeq for SerializeArray {
    type Ok = Value;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.push(value)
    }

    fn end(self) -> Result<Value, Error> {
        Ok(self.finish())
    }
}

impl ser::SerializeTuple for SerializeArray {
    type Ok = Value;
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.push(value)
    }

    fn end(self) -> Result<Value, Error> {
        Ok(self.finish())
    }
}

impl ser::SerializeTupleStruct for SerializeArray {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.push(value)
    }

    fn end(self) -> Result<Value, Error> {
        Ok(self.finish())
    }
}

impl ser::SerializeTupleVariant for SerializeArray {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        self.push(value)
    }

    fn end(self) -> Result<Value, Error> {
        Ok(self.finish())
    }
}

pub struct SerializeObject {
    map: Map,
    /// The key from the last `serialize_key`, waiting for its value.
    key: Option<String>,
    /// Set when this object is an enum variant's payload, and has to be wrapped at the end.
    variant: Option<&'static str>,
}

impl SerializeObject {
    fn insert<T: Serialize + ?Sized>(&mut self, key: String, value: &T) -> Result<(), Error> {
        let value = value.serialize(Serializer).map_err(|e| e.at_key(&key))?;
        // The language has no last-wins rule, so neither does this.
        if !self.map.insert(key.clone(), value) {
            return Err(Error::new(format!("duplicate key `{key}`")));
        }
        Ok(())
    }

    fn finish(self) -> Value {
        let object = Value::Object(self.map);
        match self.variant {
            Some(variant) => wrap(variant, object),
            None => object,
        }
    }
}

impl ser::SerializeMap for SerializeObject {
    type Ok = Value;
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        self.key = Some(key_string(key.serialize(Serializer)?)?);
        Ok(())
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        let key = self
            .key
            .take()
            .expect("serde calls serialize_key before serialize_value");
        self.insert(key, value)
    }

    fn end(self) -> Result<Value, Error> {
        Ok(self.finish())
    }
}

impl ser::SerializeStruct for SerializeObject {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.insert(key.to_string(), value)
    }

    fn end(self) -> Result<Value, Error> {
        Ok(self.finish())
    }
}

impl ser::SerializeStructVariant for SerializeObject {
    type Ok = Value;
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.insert(key.to_string(), value)
    }

    fn end(self) -> Result<Value, Error> {
        Ok(self.finish())
    }
}

/// A tot key is always a string, so a map key has to become one. Numbers and booleans have
/// an obvious spelling and are taken; anything else would be a guess.
fn key_string(key: Value) -> Result<String, Error> {
    match key {
        Value::String(s) => Ok(s),
        Value::Integer(i) => Ok(i.as_str().to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        other => Err(Error::new(format!(
            "a key must be a string, an integer, or a boolean, not {}",
            crate::path::kind(&other)
        ))),
    }
}

/// tot's own model, so that a `Value` can sit inside a `Serialize` type and pass through.
impl Serialize for Value {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(b) => serializer.serialize_bool(*b),
            Value::Integer(i) => serialize_integer(i, serializer),
            Value::Float(f) => serializer.serialize_f64(f.as_f64()),
            Value::String(s) => serializer.serialize_str(s),
            Value::Array(items) => serializer.collect_seq(items),
            Value::Object(map) => serializer.collect_map(map.iter()),
        }
    }
}

/// Integers keep their lexeme, so one can be wider than any machine integer. Widen until it
/// fits rather than silently truncating; past `u128` there is nothing honest left to do.
fn serialize_integer<S: ser::Serializer>(i: &Integer, serializer: S) -> Result<S::Ok, S::Error> {
    if let Some(n) = i.as_i64() {
        serializer.serialize_i64(n)
    } else if let Some(n) = i.as_u64() {
        serializer.serialize_u64(n)
    } else if let Some(n) = i.as_i128() {
        serializer.serialize_i128(n)
    } else if let Some(n) = i.as_u128() {
        serializer.serialize_u128(n)
    } else {
        Err(ser::Error::custom(format!(
            "integer `{i}` is too wide for serde, which stops at 128 bits"
        )))
    }
}
