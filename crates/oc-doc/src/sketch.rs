//! Sketch data (`sketch data` section of `src/core/types.ts`).
//!
//! Records (`Record<Id, T>`) are persistent ordered maps, so editing one point of a large
//! sketch copies a path of map nodes, not the sketch. The map key always equals the
//! value's `id` (see [`crate::PartDocument::validate`]).

use imbl::OrdMap;
use serde::{Deserialize, Serialize};

use crate::{Id, SketchRef, Vec2};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchPoint {
    pub id: Id,
    pub x: f64,
    pub y: f64,
    /// Points owned by an entity (endpoints, centres) vs. a free-standing sketch point.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standalone: Option<bool>,
    /// A line's midpoint picked for a relation or dimension. Tied by a midpoint relation;
    /// drawn only when highlighted.
    #[serde(rename = "virtual", default, skip_serializing_if = "Option::is_none")]
    pub is_virtual: Option<bool>,
    /// Owned by external reference `ext` (a model vertex or edge projected into the
    /// sketch): fixed for the solver, moved by the rebuild, not part of profiles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<Id>,
}

impl SketchPoint {
    pub fn new(id: impl Into<Id>, x: f64, y: f64) -> Self {
        SketchPoint {
            id: id.into(),
            x,
            y,
            standalone: None,
            is_virtual: None,
            ext: None,
        }
    }
}

/// A sketch entity. `ext`: the entity stands for a model edge (external reference id).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SketchEntity {
    Line {
        id: Id,
        p1: Id,
        p2: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ext: Option<Id>,
    },
    Circle {
        id: Id,
        c: Id,
        r: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ext: Option<Id>,
    },
    /// Runs counter-clockwise (in sketch coordinates) from `p1` to `p2` about `c`.
    Arc {
        id: Id,
        c: Id,
        p1: Id,
        p2: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ext: Option<Id>,
    },
    Point {
        id: Id,
        p: Id,
    },
    /// Centre, a point at the end of one axis and one at the end of the other.
    Ellipse {
        id: Id,
        c: Id,
        a: Id,
        q: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction: Option<bool>,
    },
    /// Open spline through points (centripetal Catmull-Rom between them).
    Spline {
        id: Id,
        pts: Vec<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction: Option<bool>,
    },
}

impl SketchEntity {
    pub fn id(&self) -> &Id {
        match self {
            SketchEntity::Line { id, .. }
            | SketchEntity::Circle { id, .. }
            | SketchEntity::Arc { id, .. }
            | SketchEntity::Point { id, .. }
            | SketchEntity::Ellipse { id, .. }
            | SketchEntity::Spline { id, .. } => id,
        }
    }

    /// Ids of the sketch points the entity is defined by.
    pub fn points(&self) -> Vec<&Id> {
        match self {
            SketchEntity::Line { p1, p2, .. } => vec![p1, p2],
            SketchEntity::Circle { c, .. } => vec![c],
            SketchEntity::Arc { c, p1, p2, .. } => vec![c, p1, p2],
            SketchEntity::Point { p, .. } => vec![p],
            SketchEntity::Ellipse { c, a, q, .. } => vec![c, a, q],
            SketchEntity::Spline { pts, .. } => pts.iter().collect(),
        }
    }
}

/// Geometric relations, named as in the SOLIDWORKS Add Relations PropertyManager.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Relation {
    Horizontal {
        id: Id,
        e: Id,
    },
    Vertical {
        id: Id,
        e: Id,
    },
    HorizontalPoints {
        id: Id,
        p1: Id,
        p2: Id,
    },
    VerticalPoints {
        id: Id,
        p1: Id,
        p2: Id,
    },
    Coincident {
        id: Id,
        p: Id,
        e: Id,
    },
    Midpoint {
        id: Id,
        p: Id,
        e: Id,
    },
    Parallel {
        id: Id,
        e1: Id,
        e2: Id,
    },
    Perpendicular {
        id: Id,
        e1: Id,
        e2: Id,
    },
    Collinear {
        id: Id,
        e1: Id,
        e2: Id,
    },
    Equal {
        id: Id,
        e1: Id,
        e2: Id,
    },
    Tangent {
        id: Id,
        e1: Id,
        e2: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        inner: Option<bool>,
    },
    Concentric {
        id: Id,
        e1: Id,
        e2: Id,
    },
    Coradial {
        id: Id,
        e1: Id,
        e2: Id,
    },
    Symmetric {
        id: Id,
        p1: Id,
        p2: Id,
        axis: Id,
    },
    Intersection {
        id: Id,
        p: Id,
        e1: Id,
        e2: Id,
    },
    /// `p1` and `p2` are the same distance from `c` (keeps polygons regular).
    Equidistant {
        id: Id,
        c: Id,
        p1: Id,
        p2: Id,
    },
    /// `e2` stays at the same offset from `e1` as `m2` is from `m1`.
    Offset {
        id: Id,
        e1: Id,
        e2: Id,
        m1: Id,
        m2: Id,
    },
    Fix {
        id: Id,
        target: Id,
    },
    /// Converted from a model edge (Convert Entities): fixed in place.
    OnEdge {
        id: Id,
        target: Id,
    },
}

impl Relation {
    pub fn id(&self) -> &Id {
        use Relation::*;
        match self {
            Horizontal { id, .. }
            | Vertical { id, .. }
            | HorizontalPoints { id, .. }
            | VerticalPoints { id, .. }
            | Coincident { id, .. }
            | Midpoint { id, .. }
            | Parallel { id, .. }
            | Perpendicular { id, .. }
            | Collinear { id, .. }
            | Equal { id, .. }
            | Tangent { id, .. }
            | Concentric { id, .. }
            | Coradial { id, .. }
            | Symmetric { id, .. }
            | Intersection { id, .. }
            | Equidistant { id, .. }
            | Offset { id, .. }
            | Fix { id, .. }
            | OnEdge { id, .. } => id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DimensionKind {
    /// Point-point aligned.
    Distance,
    /// Point-point horizontal (signed).
    Horizontal,
    /// Point-point vertical (signed).
    Vertical,
    /// Perpendicular point-line distance.
    PointLine,
    /// Distance between parallel lines.
    LineLine,
    /// Angle between two lines.
    Angle,
    Radius,
    Diameter,
    ArcLength,
    /// Distance between an arc/circle and its offset (difference of radii).
    Offset,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub id: Id,
    /// Display name, e.g. `"D1"`.
    pub name: String,
    pub kind: DimensionKind,
    /// Referenced point ids / entity ids; meaning depends on `kind`.
    pub refs: Vec<Id>,
    /// Millimetres or radians.
    pub value: f64,
    /// Driven (reference) dimensions are shown grey and do not constrain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driven: Option<bool>,
    /// Label position in sketch coordinates.
    pub label: Vec2,
    /// Orientation bookkeeping captured at creation so signed residuals never flip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign: Option<f64>,
    /// For angles: which of the 4 angle quadrants was picked (bit 0: flip e1, bit 1: flip e2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quadrant: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SketchData {
    pub points: OrdMap<Id, SketchPoint>,
    pub entities: OrdMap<Id, SketchEntity>,
    pub relations: OrdMap<Id, Relation>,
    pub dimensions: OrdMap<Id, Dimension>,
    /// Number for the next dimension name (`D<nextDim>`).
    pub next_dim: u32,
    /// Model vertices and edges the sketch refers to (relations, dimensions).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<OrdMap<Id, SketchRef>>,
}
