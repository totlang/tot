//! A [`Value`] into `T: Deserialize`, and tot text into one through the parser.

use serde::de::value::StrDeserializer;
use serde::de::{
    self, Deserialize, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use serde::forward_to_deserialize_any;

use super::Error;
use crate::value::{Integer, Map, Value};

/// Read a tot document into a value.
///
/// ```
/// let ports: Vec<u16> = tot::from_str("[80 443]").unwrap();
/// assert_eq!(ports, [80, 443]);
/// ```
pub fn from_str<T: de::DeserializeOwned>(src: &str) -> Result<T, Error> {
    let value = crate::parse(src)?;
    from_value(&value)
}

/// Read an already-parsed document into a value.
///
/// Borrowing means a `&'de str` field can point straight into the document instead of
/// allocating a copy.
pub fn from_value<'de, T: Deserialize<'de>>(value: &'de Value) -> Result<T, Error> {
    T::deserialize(Deserializer::new(value))
}

/// Reads one [`Value`]. See [`from_value`].
pub struct Deserializer<'de> {
    value: &'de Value,
}

impl<'de> Deserializer<'de> {
    pub fn new(value: &'de Value) -> Self {
        Deserializer { value }
    }
}

impl<'de> de::Deserializer<'de> for Deserializer<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(*b),
            Value::Integer(i) => visit_integer(i, visitor),
            Value::Float(f) => visitor.visit_f64(f.as_f64()),
            Value::String(s) => visitor.visit_borrowed_str(s),
            Value::Array(items) => visitor.visit_seq(Elements { items, index: 0 }),
            Value::Object(map) => visitor.visit_map(Members {
                entries: map.entries(),
                index: 0,
            }),
        }
    }

    /// `null` is the missing value; everything else is present, including `false` and `0`.
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.value {
            Value::Null => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    /// Externally tagged, the way the serializer writes them: a bare string for a unit
    /// variant, and a one-member object for every other kind.
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let variant = match self.value {
            Value::String(s) => Variant {
                name: s,
                payload: None,
            },
            Value::Object(map) if map.len() == 1 => {
                let (name, payload) = map.iter().next().expect("exactly one member");
                Variant {
                    name,
                    payload: Some(payload),
                }
            }
            Value::Object(map) => {
                return Err(Error::new(format!(
                    "expected an enum variant, which is one member naming it, but found {} of them",
                    map.len()
                )));
            }
            other => {
                return Err(Error::new(format!(
                    "expected an enum variant, but found {}",
                    crate::path::kind(other)
                )));
            }
        };
        visitor.visit_enum(variant)
    }

    /// A field the target does not want. Skipping it beats walking it.
    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    // Everything else is the shape of the value itself: an integer is an integer whether the
    // target wants a `u8` or an `i64`, and serde's own visitors do the range checking and
    // write the "invalid type" message when the shape is wrong.
    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string bytes byte_buf
        unit unit_struct seq tuple tuple_struct map struct identifier
    }
}

/// Integers keep their lexeme, so widen until the value fits. serde narrows from here, and
/// reports the overflow itself if the target is smaller.
fn visit_integer<'de, V: Visitor<'de>>(i: &Integer, visitor: V) -> Result<V::Value, Error> {
    if let Some(n) = i.as_i64() {
        visitor.visit_i64(n)
    } else if let Some(n) = i.as_u64() {
        visitor.visit_u64(n)
    } else if let Some(n) = i.as_i128() {
        visitor.visit_i128(n)
    } else if let Some(n) = i.as_u128() {
        visitor.visit_u128(n)
    } else {
        Err(Error::new(format!(
            "integer `{i}` is too wide for serde, which stops at 128 bits"
        )))
    }
}

struct Elements<'de> {
    items: &'de [Value],
    index: usize,
}

impl<'de> SeqAccess<'de> for Elements<'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        let Some(value) = self.items.get(self.index) else {
            return Ok(None);
        };
        let index = self.index;
        self.index += 1;
        seed.deserialize(Deserializer::new(value))
            .map(Some)
            .map_err(|e| e.at_index(index))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.items.len() - self.index)
    }
}

struct Members<'de> {
    entries: &'de [(String, Value)],
    index: usize,
}

impl<'de> MapAccess<'de> for Members<'de> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some((key, _)) = self.entries.get(self.index) else {
            return Ok(None);
        };
        // A key that will not deserialize is located the same way a value is: a map with a
        // typed key fails here, and "expected u16" with nothing to point at is no better a
        // message on this side than it would be on the other.
        seed.deserialize(Key { key })
            .map(Some)
            .map_err(|e| e.at_key(key))
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let (key, value) = self
            .entries
            .get(self.index)
            .expect("serde calls next_key_seed before next_value_seed");
        self.index += 1;
        seed.deserialize(Deserializer::new(value))
            .map_err(|e| e.at_key(key))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.entries.len() - self.index)
    }
}

/// A key on its way to a map's key type.
///
/// A tot key is always a string, but the serializer writes an integer or boolean key as its
/// text, so `BTreeMap<u16, _>` has to read that text back. Anything that does not parse is
/// handed over as the string it is, which lets serde write the type mismatch itself.
struct Key<'de> {
    key: &'de str,
}

macro_rules! key_as {
    ($($method:ident -> $visit:ident as $ty:ty,)*) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
                match self.key.parse::<$ty>() {
                    Ok(parsed) => visitor.$visit(parsed),
                    Err(_) => visitor.visit_borrowed_str(self.key),
                }
            }
        )*
    };
}

impl<'de> de::Deserializer<'de> for Key<'de> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_borrowed_str(self.key)
    }

    /// A key is never absent — it would not be a member otherwise.
    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }

    /// A unit-only enum is a perfectly good key type, and the serializer writes one as its
    /// variant name. Reading it back means offering the name as a variant rather than as the
    /// plain string an enum's visitor will not accept.
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_enum(Variant {
            name: self.key,
            payload: None,
        })
    }

    key_as! {
        deserialize_i8 -> visit_i8 as i8,
        deserialize_i16 -> visit_i16 as i16,
        deserialize_i32 -> visit_i32 as i32,
        deserialize_i64 -> visit_i64 as i64,
        deserialize_i128 -> visit_i128 as i128,
        deserialize_u8 -> visit_u8 as u8,
        deserialize_u16 -> visit_u16 as u16,
        deserialize_u32 -> visit_u32 as u32,
        deserialize_u64 -> visit_u64 as u64,
        deserialize_u128 -> visit_u128 as u128,
        deserialize_bool -> visit_bool as bool,
    }

    // Floats are left out on purpose: the serializer refuses a float key, because a lexeme
    // like `1.0` is a poor name, so nothing round-trips through one.
    forward_to_deserialize_any! {
        f32 f64 char str string bytes byte_buf unit unit_struct newtype_struct seq
        tuple tuple_struct map struct identifier ignored_any
    }
}

struct Variant<'de> {
    name: &'de str,
    /// `None` for a unit variant, which is written as a bare string.
    payload: Option<&'de Value>,
}

impl<'de> EnumAccess<'de> for Variant<'de> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
        // Annotated because `?` would otherwise leave the error type — and so the
        // deserializer's — open.
        let name: StrDeserializer<'de, Error> = self.name.into_deserializer();
        Ok((seed.deserialize(name)?, self))
    }
}

impl<'de> VariantAccess<'de> for Variant<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        match self.payload {
            None => Ok(()),
            Some(_) => Err(Error::new(format!(
                "`{}` is a unit variant, so it is written as a bare string",
                self.name
            ))),
        }
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        let payload = self.payload()?;
        seed.deserialize(Deserializer::new(payload))
            .map_err(|e| e.at_key(self.name))
    }

    fn tuple_variant<V: Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, Error> {
        let payload = self.payload()?;
        de::Deserializer::deserialize_seq(Deserializer::new(payload), visitor)
            .map_err(|e| e.at_key(self.name))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let payload = self.payload()?;
        de::Deserializer::deserialize_map(Deserializer::new(payload), visitor)
            .map_err(|e| e.at_key(self.name))
    }
}

impl<'de> Variant<'de> {
    fn payload(&self) -> Result<&'de Value, Error> {
        self.payload.ok_or_else(|| {
            Error::new(format!(
                "`{}` carries a value, so it is written as `{{{} …}}`",
                self.name, self.name
            ))
        })
    }
}

/// tot's own model, so that a `Value` can sit inside a `Deserialize` type and hold whatever
/// was there.
impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
        deserializer.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;

impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("any tot value")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Integer(Integer::from_i64(v)))
    }

    fn visit_i128<E>(self, v: i128) -> Result<Value, E> {
        Ok(Value::Integer(Integer::from_i128(v)))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Integer(Integer::from_u64(v)))
    }

    fn visit_u128<E>(self, v: u128) -> Result<Value, E> {
        Ok(Value::Integer(Integer::from_u128(v)))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        crate::value::Float::from_f64(v)
            .map(Value::Float)
            .ok_or_else(|| E::custom(format!("tot cannot hold `{v}`")))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_string()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_some<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        Value::deserialize(deserializer)
    }

    fn visit_newtype_struct<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Value, D::Error> {
        Value::deserialize(deserializer)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(Value::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Value, A::Error> {
        let mut map = Map::new();
        while let Some((key, value)) = access.next_entry::<String, Value>()? {
            // Duplicate keys are a parse error in the language, so they are one here too.
            if !map.insert(key.clone(), value) {
                return Err(de::Error::custom(format!("duplicate key `{key}`")));
            }
        }
        Ok(Value::Object(map))
    }
}
