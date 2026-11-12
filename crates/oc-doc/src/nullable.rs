//! Serde adapter for TypeScript fields declared `field?: T | null`.
//!
//! Such a field has three states in a saved document (absent, `null`, a value), and a
//! lossless round trip must keep all three apart, so the Rust field is
//! `Option<Option<T>>`: `None` = absent, `Some(None)` = `null`, `Some(Some(v))` = value.
//! Use with `#[serde(default, skip_serializing_if = "Option::is_none", with = "crate::nullable")]`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn serialize<S: Serializer, T: Serialize>(
    v: &Option<Option<T>>,
    s: S,
) -> Result<S::Ok, S::Error> {
    match v {
        Some(Some(x)) => x.serialize(s),
        _ => s.serialize_none(),
    }
}

pub fn deserialize<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}
