//! References from the document to model geometry (`references` section of
//! `src/core/types.ts`, plus `SketchRef`).

use serde::{Deserialize, Serialize};

use crate::{Id, Vec3};

/// Geometric signature of a face, used to re-find it after a rebuild.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceSig {
    /// OCCT surface type name: `"PLANE"`, `"CYLINDRE"`, ...
    pub surface: String,
    pub center: Vec3,
    pub normal: Vec3,
    pub area: f64,
}

/// Geometric signature of an edge.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeSig {
    pub curve: String,
    pub mid: Vec3,
    pub a: Vec3,
    pub b: Vec3,
    pub length: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StdPlane {
    Front,
    Top,
    Right,
}

/// A plane: one of the three standard planes, a planar model face, or a reference plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PlaneRef {
    Std {
        plane: StdPlane,
    },
    Face {
        sig: FaceSig,
    },
    #[serde(rename = "refplane")]
    RefPlane {
        feature_id: Id,
    },
}

/// A model reference picked in the graphics area or the tree: a direction (linear edge,
/// planar face normal, plane normal), an axis (linear or circular edge, cylindrical face,
/// sketch line) or a plane (planar face, plane).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GeomRef {
    Edge {
        sig: EdgeSig,
    },
    Face {
        sig: FaceSig,
    },
    Plane {
        plane: PlaneRef,
    },
    SketchLine {
        sketch_id: Id,
        entity: Id,
    },
    /// A model vertex or the origin (position captured when picked).
    Vertex {
        p: Vec3,
    },
    /// Reference axis feature.
    Axis {
        feature_id: Id,
    },
    /// Reference point feature.
    Point {
        feature_id: Id,
    },
}

/// External reference from a sketch to model geometry, as SOLIDWORKS relations to model
/// edges and vertices. The rebuild re-finds it by signature, projects it onto the sketch
/// plane and moves the fixed sketch geometry that stands for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SketchRef {
    /// Re-found as the common end of its edges (`edges`), else the nearest vertex.
    Vertex {
        id: Id,
        p: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edges: Option<Vec<EdgeSig>>,
    },
    Edge {
        id: Id,
        sig: EdgeSig,
    },
}

impl SketchRef {
    pub fn id(&self) -> &Id {
        match self {
            SketchRef::Vertex { id, .. } | SketchRef::Edge { id, .. } => id,
        }
    }
}
