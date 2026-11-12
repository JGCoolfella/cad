//! Canonical JSON.
//!
//! The canonical form is RFC 8785 (JSON Canonicalization Scheme): object keys sorted by
//! UTF-16 code units, numbers printed as ECMAScript's `Number.prototype.toString` prints
//! them (shortest round-trip digits; `5`, not `5.0`), strings escaped as `JSON.stringify`
//! escapes them, no whitespace. Two documents are equivalent iff their canonical forms
//! are byte-equal, whatever key order, indentation, line endings or number spelling the
//! files used. Because numbers print as JavaScript prints them, the TypeScript app and
//! the native engine agree on the bytes.
//!
//! Serialization goes through a small tree ([`J`]) built by a serde `Serializer`, which
//! also enforces what JSON (and so the document) cannot hold: non-finite numbers,
//! integers beyond 2^53, non-string map keys and duplicate keys are errors, never
//! silently `null` or lost.

use std::fmt::{self, Display, Write as _};

use serde::ser::{self, Serialize};

/// Why a value has no JSON form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonError(pub String);

impl Display for CanonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CanonError {}

impl ser::Error for CanonError {
    fn custom<T: Display>(msg: T) -> Self {
        CanonError(msg.to_string())
    }
}

/// A JSON value. Object members are in canonical order, except the members of a
/// [`crate::Record`] in a tree built for saving, which keep insertion order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

/// Largest integer every IEEE double between 0 and it represents exactly (2^53).
const MAX_SAFE: u64 = 1 << 53;

/// Canonical tree: every object's members sorted.
pub(crate) fn tree<T: Serialize + ?Sized>(v: &T) -> Result<J, CanonError> {
    v.serialize(TreeSer::CANONICAL)
}

/// RFC 8785 canonical JSON of any serializable value.
pub fn to_canonical_string<T: Serialize + ?Sized>(v: &T) -> Result<String, CanonError> {
    let mut out = String::new();
    write_compact(&tree(v)?, &mut out);
    Ok(out)
}

/// Canonical number spelling, indented like `JSON.stringify(v, null, indent)`. Members
/// are in canonical order, except that a [`crate::Record`] keeps its insertion order
/// (sketch points, entities, relations, dimensions and refs come out as created).
pub fn to_pretty_string<T: Serialize + ?Sized>(v: &T, indent: usize) -> Result<String, CanonError> {
    let mut out = String::new();
    let tree = v.serialize(TreeSer {
        keep_record_order: true,
        ordered: false,
    })?;
    write_pretty(&tree, indent, 0, &mut out);
    Ok(out)
}

fn object(mut members: Vec<(String, J)>, sorted: bool) -> Result<J, CanonError> {
    if !sorted {
        // A `Record`, whose keys are unique by construction.
        return Ok(J::Obj(members));
    }
    members.sort_by(|a, b| a.0.encode_utf16().cmp(b.0.encode_utf16()));
    if let Some(w) = members.windows(2).find(|w| w[0].0 == w[1].0) {
        return Err(CanonError(format!("duplicate key {:?}", w[0].0)));
    }
    Ok(J::Obj(members))
}

fn number(x: f64) -> Result<J, CanonError> {
    if x.is_finite() {
        Ok(J::Num(x))
    } else {
        Err(CanonError(format!("{x} has no JSON form")))
    }
}

fn integer(neg: bool, magnitude: u64) -> Result<J, CanonError> {
    if magnitude > MAX_SAFE {
        return Err(CanonError(format!(
            "integer {}{magnitude} is beyond 2^53 and would change in JavaScript",
            if neg { "-" } else { "" }
        )));
    }
    let x = magnitude as f64;
    Ok(J::Num(if neg { -x } else { x }))
}

// ---------------------------------------------------------------- writers

pub(crate) fn write_compact(j: &J, out: &mut String) {
    match j {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Num(x) => write_number(*x, out),
        J::Str(s) => write_string(s, out),
        J::Arr(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(x, out);
            }
            out.push(']');
        }
        J::Obj(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(k, out);
                out.push(':');
                write_compact(v, out);
            }
            out.push('}');
        }
    }
}

fn write_pretty(j: &J, indent: usize, depth: usize, out: &mut String) {
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', indent * depth));
    };
    match j {
        J::Arr(a) if !a.is_empty() => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write_pretty(x, indent, depth + 1, out);
            }
            newline(out, depth);
            out.push(']');
        }
        J::Obj(m) if !m.is_empty() => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                newline(out, depth + 1);
                write_string(k, out);
                out.push_str(": ");
                write_pretty(v, indent, depth + 1, out);
            }
            newline(out, depth);
            out.push('}');
        }
        _ => write_compact(j, out),
    }
}

/// `JSON.stringify` string escaping (RFC 8785 section 3.2.2.2).
fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// ECMAScript `Number::toString(x)` for finite `x` (ECMA-262 section 6.1.6.1.20), from
/// the shortest round-trip digits that `ryu` finds.
pub(crate) fn write_number(x: f64, out: &mut String) {
    if x == 0.0 {
        out.push('0'); // also -0, as in JavaScript
        return;
    }
    let mut buf = ryu::Buffer::new();
    let s = buf.format_finite(x);
    let (neg, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s),
    };
    let (mantissa, exp) = match s.split_once('e') {
        Some((m, e)) => (m, e.parse::<i32>().expect("ryu exponent")),
        None => (s, 0),
    };
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    // x = 0.d1 d2 ... dk × 10^n, with d1 ≠ 0 and dk ≠ 0.
    let all: String = [int_part, frac_part].concat();
    let lead = all.bytes().take_while(|&b| b == b'0').count();
    let digits = all[lead..].trim_end_matches('0');
    let n = int_part.len() as i32 + exp - lead as i32;
    let k = digits.len() as i32;

    if neg {
        out.push('-');
    }
    if k <= n && n <= 21 {
        out.push_str(digits);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        out.push_str(&digits[..n as usize]);
        out.push('.');
        out.push_str(&digits[n as usize..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(digits);
    } else {
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        let e = n - 1;
        let _ = write!(out, "e{}{}", if e < 0 { '-' } else { '+' }, e.abs());
    }
}

// ---------------------------------------------------------------- serializer

#[derive(Clone, Copy)]
struct TreeSer {
    /// Building a tree for saving: `Record` members keep their order.
    keep_record_order: bool,
    /// The value being serialized is a `Record`'s map whose order is kept.
    ordered: bool,
}

impl TreeSer {
    const CANONICAL: TreeSer = TreeSer {
        keep_record_order: false,
        ordered: false,
    };

    /// Serializer for values nested in this one.
    fn child(self) -> TreeSer {
        TreeSer {
            ordered: false,
            ..self
        }
    }
}

struct SeqSer {
    ser: TreeSer,
    items: Vec<J>,
    /// Set for tuple variants: wrap as `{variant: [...]}`.
    variant: Option<&'static str>,
}

struct MapSer {
    ser: TreeSer,
    /// Members are sorted at the end (false for a `Record` when saving).
    sorted: bool,
    members: Vec<(String, J)>,
    key: Option<String>,
    /// Set for struct variants: wrap as `{variant: {...}}`.
    variant: Option<&'static str>,
}

fn wrap(variant: Option<&'static str>, j: J) -> Result<J, CanonError> {
    match variant {
        Some(v) => object(vec![(v.to_owned(), j)], true),
        None => Ok(j),
    }
}

impl ser::Serializer for TreeSer {
    type Ok = J;
    type Error = CanonError;
    type SerializeSeq = SeqSer;
    type SerializeTuple = SeqSer;
    type SerializeTupleStruct = SeqSer;
    type SerializeTupleVariant = SeqSer;
    type SerializeMap = MapSer;
    type SerializeStruct = MapSer;
    type SerializeStructVariant = MapSer;

    fn serialize_bool(self, v: bool) -> Result<J, CanonError> {
        Ok(J::Bool(v))
    }
    fn serialize_i8(self, v: i8) -> Result<J, CanonError> {
        self.serialize_i64(v.into())
    }
    fn serialize_i16(self, v: i16) -> Result<J, CanonError> {
        self.serialize_i64(v.into())
    }
    fn serialize_i32(self, v: i32) -> Result<J, CanonError> {
        self.serialize_i64(v.into())
    }
    fn serialize_i64(self, v: i64) -> Result<J, CanonError> {
        integer(v < 0, v.unsigned_abs())
    }
    fn serialize_u8(self, v: u8) -> Result<J, CanonError> {
        self.serialize_u64(v.into())
    }
    fn serialize_u16(self, v: u16) -> Result<J, CanonError> {
        self.serialize_u64(v.into())
    }
    fn serialize_u32(self, v: u32) -> Result<J, CanonError> {
        self.serialize_u64(v.into())
    }
    fn serialize_u64(self, v: u64) -> Result<J, CanonError> {
        integer(false, v)
    }
    fn serialize_f32(self, v: f32) -> Result<J, CanonError> {
        number(v.into())
    }
    fn serialize_f64(self, v: f64) -> Result<J, CanonError> {
        number(v)
    }
    fn serialize_char(self, v: char) -> Result<J, CanonError> {
        Ok(J::Str(v.to_string()))
    }
    fn serialize_str(self, v: &str) -> Result<J, CanonError> {
        Ok(J::Str(v.to_owned()))
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<J, CanonError> {
        Err(CanonError("byte strings have no JSON form".into()))
    }
    fn serialize_none(self) -> Result<J, CanonError> {
        Ok(J::Null)
    }
    fn serialize_some<T: Serialize + ?Sized>(self, v: &T) -> Result<J, CanonError> {
        v.serialize(self)
    }
    fn serialize_unit(self) -> Result<J, CanonError> {
        Ok(J::Null)
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<J, CanonError> {
        Ok(J::Null)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<J, CanonError> {
        Ok(J::Str(variant.to_owned()))
    }
    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        v: &T,
    ) -> Result<J, CanonError> {
        let ordered = self.keep_record_order && name == crate::record::ORDERED_MAP;
        v.serialize(TreeSer {
            ordered,
            ..self.child()
        })
    }
    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        v: &T,
    ) -> Result<J, CanonError> {
        wrap(Some(variant), v.serialize(self.child())?)
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSer, CanonError> {
        Ok(SeqSer {
            ser: self.child(),
            items: Vec::with_capacity(len.unwrap_or(0)),
            variant: None,
        })
    }
    fn serialize_tuple(self, len: usize) -> Result<SeqSer, CanonError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_struct(self, _: &'static str, len: usize) -> Result<SeqSer, CanonError> {
        self.serialize_seq(Some(len))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<SeqSer, CanonError> {
        Ok(SeqSer {
            ser: self.child(),
            items: Vec::with_capacity(len),
            variant: Some(variant),
        })
    }
    fn serialize_map(self, len: Option<usize>) -> Result<MapSer, CanonError> {
        Ok(MapSer {
            ser: self.child(),
            sorted: !self.ordered,
            members: Vec::with_capacity(len.unwrap_or(0)),
            key: None,
            variant: None,
        })
    }
    fn serialize_struct(self, _: &'static str, len: usize) -> Result<MapSer, CanonError> {
        self.child().serialize_map(Some(len))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<MapSer, CanonError> {
        Ok(MapSer {
            ser: self.child(),
            sorted: true,
            members: Vec::with_capacity(len),
            key: None,
            variant: Some(variant),
        })
    }
}

impl ser::SerializeSeq for SeqSer {
    type Ok = J;
    type Error = CanonError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), CanonError> {
        self.items.push(v.serialize(self.ser)?);
        Ok(())
    }
    fn end(self) -> Result<J, CanonError> {
        wrap(self.variant, J::Arr(self.items))
    }
}

impl ser::SerializeTuple for SeqSer {
    type Ok = J;
    type Error = CanonError;
    fn serialize_element<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), CanonError> {
        ser::SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<J, CanonError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SeqSer {
    type Ok = J;
    type Error = CanonError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), CanonError> {
        ser::SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<J, CanonError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleVariant for SeqSer {
    type Ok = J;
    type Error = CanonError;
    fn serialize_field<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), CanonError> {
        ser::SerializeSeq::serialize_element(self, v)
    }
    fn end(self) -> Result<J, CanonError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeMap for MapSer {
    type Ok = J;
    type Error = CanonError;
    fn serialize_key<T: Serialize + ?Sized>(&mut self, k: &T) -> Result<(), CanonError> {
        match k.serialize(self.ser)? {
            J::Str(s) => {
                self.key = Some(s);
                Ok(())
            }
            other => Err(CanonError(format!("map key {other:?} is not a string"))),
        }
    }
    fn serialize_value<T: Serialize + ?Sized>(&mut self, v: &T) -> Result<(), CanonError> {
        let k = self
            .key
            .take()
            .ok_or_else(|| CanonError("map value without a key".into()))?;
        self.members.push((k, v.serialize(self.ser)?));
        Ok(())
    }
    fn end(self) -> Result<J, CanonError> {
        wrap(self.variant, object(self.members, self.sorted)?)
    }
}

impl ser::SerializeStruct for MapSer {
    type Ok = J;
    type Error = CanonError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        k: &'static str,
        v: &T,
    ) -> Result<(), CanonError> {
        self.members.push((k.to_owned(), v.serialize(self.ser)?));
        Ok(())
    }
    fn end(self) -> Result<J, CanonError> {
        ser::SerializeMap::end(self)
    }
}

impl ser::SerializeStructVariant for MapSer {
    type Ok = J;
    type Error = CanonError;
    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        k: &'static str,
        v: &T,
    ) -> Result<(), CanonError> {
        ser::SerializeStruct::serialize_field(self, k, v)
    }
    fn end(self) -> Result<J, CanonError> {
        ser::SerializeMap::end(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(x: f64) -> String {
        let mut s = String::new();
        write_number(x, &mut s);
        s
    }

    /// Expected strings are what `String(x)` prints in JavaScript (V8).
    #[test]
    fn numbers_print_as_javascript_prints_them() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "0"),
            (5.0, "5"),
            (-5.0, "-5"),
            (100.0, "100"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (123.456, "123.456"),
            (1e-6, "0.000001"),
            (1.5e-6, "0.0000015"),
            (1e-7, "1e-7"),
            (-1e-7, "-1e-7"),
            (1.5e-7, "1.5e-7"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1.2345e21, "1.2345e+21"),
            (123456789012345680000.0, "123456789012345680000"),
            (9007199254740993.0, "9007199254740992"),
            (f64::MAX, "1.7976931348623157e+308"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
            (5e-324, "5e-324"),
            (std::f64::consts::PI, "3.141592653589793"),
            (-124.288, "-124.288"),
            (68963.6, "68963.6"),
            (0.5, "0.5"),
        ];
        for &(x, want) in cases {
            assert_eq!(num(x), want, "{x:e}");
        }
    }

    #[test]
    fn printed_numbers_read_back_exactly() {
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let x = f64::from_bits(state);
            if !x.is_finite() {
                continue;
            }
            let s = num(x);
            let back: f64 = s.parse().unwrap();
            assert_eq!(back, if x == 0.0 { 0.0 } else { x }, "{s}");
        }
    }

    #[test]
    fn strings_escape_as_json_stringify() {
        let mut s = String::new();
        write_string(
            "a\"b\\c\u{8}\u{c}\n\r\t\u{1}\u{1f}\u{7f}é\u{2028}😀",
            &mut s,
        );
        assert_eq!(
            s,
            "\"a\\\"b\\\\c\\b\\f\\n\\r\\t\\u0001\\u001f\u{7f}é\u{2028}😀\""
        );
    }

    #[test]
    fn keys_sort_by_utf16_code_units() {
        // U+FB01 sorts after U+1F600 in UTF-16 (0xFB01 > 0xD83D) but before it by code point.
        let v = serde_json::json!({"b": 1, "a": 2, "\u{1F600}": 3, "\u{FB01}": 4, "B": 5});
        assert_eq!(
            to_canonical_string(&v).unwrap(),
            "{\"B\":5,\"a\":2,\"b\":1,\"\u{1F600}\":3,\"\u{FB01}\":4}"
        );
    }

    #[test]
    fn non_json_values_are_errors() {
        assert!(to_canonical_string(&f64::NAN).is_err());
        assert!(to_canonical_string(&[1.0, f64::INFINITY]).is_err());
        assert!(to_canonical_string(&(MAX_SAFE + 1)).is_err());
        assert!(to_canonical_string(&MAX_SAFE).is_ok());
        let mut m = std::collections::BTreeMap::new();
        m.insert(1, 2);
        assert!(to_canonical_string(&m).is_err());
    }

    #[test]
    fn pretty_matches_json_stringify_layout() {
        let v = serde_json::json!({"b": [1, {"c": []}], "a": {}});
        assert_eq!(
            to_pretty_string(&v, 1).unwrap(),
            "{\n \"a\": {},\n \"b\": [\n  1,\n  {\n   \"c\": []\n  }\n ]\n}"
        );
    }
}
