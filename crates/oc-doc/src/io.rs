//! Reading and writing documents.
//!
//! **Loading is lossless or fails.** After a document is read into the typed model, the
//! model is written back to canonical JSON and compared with the canonical form of the
//! (migrated) file. Any difference (an unknown field, a `null` where the schema has an
//! optional field, a fractional count) is an error naming the JSON path, so saving never
//! drops or changes data the user's file held. Unknown fields are rejected rather than
//! carried along: opaque data cannot be kept consistent by edits (it may name features
//! that an edit deletes), and a newer schema is caught earlier by `version`.

use std::fmt::{self, Display};

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

use crate::canon::{self, CanonError, J};
use crate::{MigrationError, Migrations, PartDocument, CURRENT_VERSION, FORMAT, MIGRATIONS};

/// Why a document could not be loaded.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadError {
    /// Not well-formed JSON, or an object with a repeated key.
    Syntax(String),
    /// Well-formed JSON, but not an OpenCAD part (`format`, `version`).
    NotAPart(String),
    Migration(MigrationError),
    /// Does not match the schema (missing field, wrong type, unknown enum value).
    Schema {
        path: String,
        message: String,
    },
    /// Matches the schema only by losing or changing data at `path`.
    Lossy {
        path: String,
        file: String,
        model: String,
    },
}

impl Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Syntax(m) => write!(f, "not valid JSON: {m}"),
            LoadError::NotAPart(m) => write!(f, "not an OpenCAD part: {m}"),
            LoadError::Migration(e) => e.fmt(f),
            LoadError::Schema { path, message } => write!(f, "{path}: {message}"),
            LoadError::Lossy { path, file, model } => {
                write!(
                    f,
                    "{path}: the file has {file}, which this schema reads as {model}"
                )
            }
        }
    }
}

impl std::error::Error for LoadError {}

/// A loaded document and the schema version its file had.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub doc: PartDocument,
    /// Version in the file; below [`CURRENT_VERSION`] when it was migrated.
    pub source_version: u32,
}

/// Reads a document saved by any supported version.
pub fn load(text: &str) -> Result<Loaded, LoadError> {
    load_with(text, &MIGRATIONS)
}

/// [`load`] with another migration registry (tests; must end at [`CURRENT_VERSION`]).
pub fn load_with(text: &str, migrations: &Migrations<'_>) -> Result<Loaded, LoadError> {
    assert_eq!(
        migrations.current, CURRENT_VERSION,
        "a migration registry must end at the current version"
    );
    let mut root = match parse(text)? {
        Value::Object(m) => m,
        _ => return Err(LoadError::NotAPart("the top level is not an object".into())),
    };
    let source_version = header(&root)?;
    migrations
        .upgrade(&mut root, source_version)
        .map_err(LoadError::Migration)?;
    let value = Value::Object(root);
    let doc: PartDocument =
        serde_path_to_error::deserialize(&value).map_err(|e| LoadError::Schema {
            path: display_path(&e.path().to_string()),
            message: e.inner().to_string(),
        })?;
    let file = canon::tree(&value).map_err(|e| LoadError::Syntax(e.0))?;
    let model = canon::tree(&doc).map_err(|e| LoadError::Schema {
        path: "$".into(),
        message: e.0,
    })?;
    if let Some((path, file, model)) = first_difference(&file, &model, "$".into()) {
        return Err(LoadError::Lossy { path, file, model });
    }
    Ok(Loaded {
        doc,
        source_version,
    })
}

/// The document as saved: canonical numbers, one-space indent (the TypeScript app's
/// layout), canonical member order except in sketch records ([`crate::Record`]), which
/// keep insertion order because the solver depends on it. Fails only on values JSON
/// cannot hold (NaN, infinities).
pub fn save(doc: &PartDocument) -> Result<String, CanonError> {
    canon::to_pretty_string(doc, 1)
}

/// RFC 8785 canonical form of the document: the bytes to compare or hash.
pub fn canonical(doc: &PartDocument) -> Result<String, CanonError> {
    canon::to_canonical_string(doc)
}

/// RFC 8785 canonical form of any JSON text (no schema), with the same strict parsing
/// as [`load`].
pub fn canonicalize_json(text: &str) -> Result<String, LoadError> {
    let v = parse(text)?;
    canon::to_canonical_string(&v).map_err(|e| LoadError::Syntax(e.0))
}

fn header(root: &Map<String, Value>) -> Result<u32, LoadError> {
    match root.get("format") {
        Some(Value::String(s)) if s == FORMAT => {}
        Some(other) => {
            return Err(LoadError::NotAPart(format!(
                "format is {other}, expected \"{FORMAT}\""
            )))
        }
        None => return Err(LoadError::NotAPart("no format field".into())),
    }
    match root.get("version") {
        Some(v) => v
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| {
                LoadError::NotAPart(format!("version is {v}, expected a schema version number"))
            }),
        None => Err(LoadError::NotAPart("no version field".into())),
    }
}

/// `features[3].sketch` style paths (serde_path_to_error prints `features[3].sketch` already;
/// the root is shown as `$`).
fn display_path(p: &str) -> String {
    if p == "." || p.is_empty() {
        "$".into()
    } else {
        format!("$.{p}")
    }
}

// ---------------------------------------------------------------- strict parsing

/// Parses JSON into a `Value`, rejecting repeated object keys (which `JSON.parse` and
/// serde_json both resolve silently, last one wins).
fn parse(text: &str) -> Result<Value, LoadError> {
    let mut de = serde_json::Deserializer::from_str(text);
    let v = Strict::deserialize(&mut de).map_err(|e| LoadError::Syntax(e.to_string()))?;
    de.end().map_err(|e| LoadError::Syntax(e.to_string()))?;
    Ok(v.0)
}

struct Strict(Value);

impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(StrictVisitor).map(Strict)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::from(v))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("number out of range"))
    }
    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }
    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(Strict(v)) = seq.next_element()? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some(k) = map.next_key::<String>()? {
            if out.contains_key(&k) {
                return Err(de::Error::custom(format!(
                    "key {k:?} appears twice in one object"
                )));
            }
            let Strict(v) = map.next_value()?;
            out.insert(k, v);
        }
        Ok(Value::Object(out))
    }
}

// ---------------------------------------------------------------- lossless check

fn brief(j: Option<&J>) -> String {
    let Some(j) = j else { return "no value".into() };
    let mut s = String::new();
    canon::write_compact(j, &mut s);
    if s.chars().count() > 60 {
        s = s.chars().take(57).collect::<String>() + "...";
    }
    s
}

/// First place where two canonical trees differ: (path, file value, model value).
fn first_difference(file: &J, model: &J, path: String) -> Option<(String, String, String)> {
    match (file, model) {
        (J::Obj(a), J::Obj(b)) => {
            let (mut i, mut j) = (0, 0);
            while i < a.len() || j < b.len() {
                let order = match (a.get(i), b.get(j)) {
                    (Some((ka, _)), Some((kb, _))) => ka.encode_utf16().cmp(kb.encode_utf16()),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    _ => std::cmp::Ordering::Greater,
                };
                match order {
                    std::cmp::Ordering::Less => {
                        return Some((
                            format!("{path}.{}", a[i].0),
                            brief(Some(&a[i].1)),
                            brief(None),
                        ));
                    }
                    std::cmp::Ordering::Greater => {
                        return Some((
                            format!("{path}.{}", b[j].0),
                            brief(None),
                            brief(Some(&b[j].1)),
                        ));
                    }
                    std::cmp::Ordering::Equal => {
                        if let Some(d) =
                            first_difference(&a[i].1, &b[j].1, format!("{path}.{}", a[i].0))
                        {
                            return Some(d);
                        }
                        i += 1;
                        j += 1;
                    }
                }
            }
            None
        }
        (J::Arr(a), J::Arr(b)) => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                if let Some(d) = first_difference(x, y, format!("{path}[{i}]")) {
                    return Some(d);
                }
            }
            let n = a.len().min(b.len());
            (a.len() != b.len()).then(|| (format!("{path}[{n}]"), brief(a.get(n)), brief(b.get(n))))
        }
        (J::Num(x), J::Num(y)) if x == y => None,
        _ if file == model => None,
        _ => Some((path, brief(Some(file)), brief(Some(model)))),
    }
}
