//! The part document (`PartDocument` in `src/core/types.ts`).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use imbl::Vector;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::{Feature, Id, CURRENT_VERSION};

/// The value of the document's `format` field.
pub const FORMAT: &str = "opencad-part";

/// The `format` field: always `"opencad-part"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PartFormat;

impl Serialize for PartFormat {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(FORMAT)
    }
}

impl<'de> Deserialize<'de> for PartFormat {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s == FORMAT {
            Ok(PartFormat)
        } else {
            Err(D::Error::custom(format!(
                "format is {s:?}, expected {FORMAT:?}"
            )))
        }
    }
}

/// The `version` field. An in-memory document is always at [`CURRENT_VERSION`]: older
/// documents are migrated before they are typed (see [`crate::Migrations`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SchemaVersion;

impl Serialize for SchemaVersion {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(CURRENT_VERSION)
    }
}

impl<'de> Deserialize<'de> for SchemaVersion {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = u32::deserialize(d)?;
        if v == CURRENT_VERSION {
            Ok(SchemaVersion)
        } else {
            Err(D::Error::custom(format!(
                "version is {v}, expected {CURRENT_VERSION} (migrate first)"
            )))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UnitSystem {
    #[serde(rename = "MMGS")]
    Mmgs,
    #[serde(rename = "CGS")]
    Cgs,
    #[serde(rename = "MKS")]
    Mks,
    #[serde(rename = "IPS")]
    Ips,
}

/// Tools > Equations: a global variable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Global {
    pub name: String,
    pub expr: String,
}

/// Tools > Equations: an equation driving a dimension.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equation {
    pub target: String,
    pub expr: String,
}

/// View-only display state saved with the document.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocDisplay {
    /// Standard planes the user has shown (they are hidden by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shown_planes: Option<Vec<String>>,
    /// Sketches the user has hidden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_sketches: Option<Vec<String>>,
    /// Reference planes the user has hidden (they are shown by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden_planes: Option<Vec<String>>,
    /// Manually resized plane extents in plane coordinates: `[u0, u1, v0, v1]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plane_extents: Option<BTreeMap<String, [f64; 4]>>,
    /// Solid body hidden (Hide/Show Body).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparent: Option<bool>,
    /// Appearance colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// A part document. Immutable in spirit: edits go through [`crate::History`]
/// transactions, which copy only what they change. Features are shared between snapshots
/// (`Arc`), so an unchanged feature is pointer-equal across undo steps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PartDocument {
    pub format: PartFormat,
    pub version: SchemaVersion,
    pub name: String,
    pub units: UnitSystem,
    pub features: Vector<Arc<Feature>>,
    /// Index into `features`: everything at or after it is rolled back. `None` (JSON
    /// `null`, always written) = at the end.
    #[serde(deserialize_with = "Option::deserialize")]
    pub rollback: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<Arc<DocDisplay>>,
    /// Document Properties (Tools > Options > Document Properties), keyed by option id.
    /// Free-form JSON, as `Record<string, unknown>` in TypeScript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<Arc<BTreeMap<String, Value>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub globals: Option<Arc<Vec<Global>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equations: Option<Arc<Vec<Equation>>>,
}

impl PartDocument {
    /// An empty part (no features), as File > New.
    pub fn new(name: impl Into<String>, units: UnitSystem) -> Self {
        PartDocument {
            format: PartFormat,
            version: SchemaVersion,
            name: name.into(),
            units,
            features: Vector::new(),
            rollback: None,
            material: None,
            display: None,
            props: None,
            globals: None,
            equations: None,
        }
    }

    pub fn feature_index(&self, id: &str) -> Option<usize> {
        self.features.iter().position(|f| f.id == *id)
    }

    pub fn feature(&self, id: &str) -> Option<&Feature> {
        self.features.iter().find(|f| f.id == *id).map(|f| &**f)
    }

    /// Mutable access to one feature; copies it first if a snapshot shares it.
    pub fn feature_mut(&mut self, id: &str) -> Option<&mut Feature> {
        let i = self.feature_index(id)?;
        self.features.get_mut(i).map(Arc::make_mut)
    }

    /// True when `self` and `other` hold the same data. Shared features are compared by
    /// pointer, so this is cheap after a transaction that touched few features.
    pub fn same_as(&self, other: &PartDocument) -> bool {
        self.name == other.name
            && self.units == other.units
            && self.rollback == other.rollback
            && self.material == other.material
            && self.display == other.display
            && self.props == other.props
            && self.globals == other.globals
            && self.equations == other.equations
            && self.features.len() == other.features.len()
            && (self.features.ptr_eq(&other.features)
                || self
                    .features
                    .iter()
                    .zip(other.features.iter())
                    .all(|(a, b)| Arc::ptr_eq(a, b) || a == b))
    }

    /// Structural invariants the TypeScript app maintains but does not check: unique
    /// feature ids, `rollback` within range, sketch map keys equal to their values' ids,
    /// and sketch entities referring to existing points. Returns one message per problem.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let mut ids = HashSet::new();
        for f in &self.features {
            if !ids.insert(f.id.clone()) {
                issues.push(format!("feature id {} is used twice", f.id));
            }
        }
        if let Some(r) = self.rollback {
            if r as usize > self.features.len() {
                issues.push(format!(
                    "rollback {r} is past the last feature ({})",
                    self.features.len()
                ));
            }
        }
        for f in &self.features {
            let Some(s) = f.sketch() else { continue };
            let at =
                |what: &str, key: &Id, id: &Id| format!("{}: {what} key {key} holds id {id}", f.id);
            for (k, p) in &s.points {
                if *k != p.id {
                    issues.push(at("point", k, &p.id));
                }
            }
            for (k, e) in &s.entities {
                if k != e.id() {
                    issues.push(at("entity", k, e.id()));
                }
                for p in e.points() {
                    if !s.points.contains_key(p) {
                        issues.push(format!("{}: entity {k} refers to missing point {p}", f.id));
                    }
                }
            }
            for (k, r) in &s.relations {
                if k != r.id() {
                    issues.push(at("relation", k, r.id()));
                }
            }
            for (k, d) in &s.dimensions {
                if *k != d.id {
                    issues.push(at("dimension", k, &d.id));
                }
            }
            for (k, r) in s.refs.iter().flatten() {
                if k != r.id() {
                    issues.push(at("reference", k, r.id()));
                }
            }
        }
        issues
    }
}
