//! Sketch data, mirroring `SketchData` and friends in `src/core/types.ts` field for field.
//!
//! These types are local to this crate until `oc-doc` (work package E1) owns the document
//! model. They carry the same names as the TypeScript types and as the planned `oc-doc`
//! types, so switching is a change of `use` paths. Only what the solver reads is modelled;
//! unknown JSON fields are ignored and unknown relation, entity or dimension kinds parse as
//! `Other` and are skipped by the solver, like the TypeScript `switch` statements do.
//!
//! **Order matters.** Parameters and equations are created in map order, and rank analysis
//! processes equations in creation order (the newest conflicting relation is the one flagged).
//! `Record` therefore preserves insertion order, which for a JSON object is the order of its
//! keys in the text, i.e. JavaScript's `Object.values` order when the text came from
//! `JSON.stringify`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub type Id = String;
pub type Vec2 = [f64; 2];
/// `Record<Id, T>` in TypeScript: an insertion-ordered map.
pub type Record<T> = IndexMap<Id, T>;

/// The fixed sketch origin point every sketch contains.
pub const ORIGIN_ID: &str = "origin";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SketchData {
    pub points: Record<SketchPoint>,
    #[serde(default)]
    pub entities: Record<SketchEntity>,
    #[serde(default)]
    pub relations: Record<Relation>,
    #[serde(default)]
    pub dimensions: Record<Dimension>,
    #[serde(default)]
    pub next_dim: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchPoint {
    pub id: Id,
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub standalone: Option<bool>,
    #[serde(default, rename = "virtual", skip_serializing_if = "Option::is_none")]
    pub virtual_: Option<bool>,
    /// Owned by an external (model) reference: a constant for the solver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ext: Option<Id>,
}

impl SketchPoint {
    pub fn new(id: impl Into<Id>, x: f64, y: f64) -> Self {
        Self {
            id: id.into(),
            x,
            y,
            standalone: None,
            virtual_: None,
            ext: None,
        }
    }
}

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
    /// Counter-clockwise from `p1` to `p2` about `c`.
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
    /// Centre, end of one axis (`a`) and end of the other (`q`), kept perpendicular.
    Ellipse {
        id: Id,
        c: Id,
        a: Id,
        q: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction: Option<bool>,
    },
    /// Open centripetal Catmull-Rom spline through `pts`.
    Spline {
        id: Id,
        pts: Vec<Id>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        construction: Option<bool>,
    },
    #[serde(other)]
    Other,
}

impl SketchEntity {
    /// Points the entity is defined by (`entityPoints` in TypeScript).
    pub fn points(&self) -> Vec<&str> {
        match self {
            Self::Line { p1, p2, .. } => vec![p1, p2],
            Self::Circle { c, .. } => vec![c],
            Self::Arc { c, p1, p2, .. } => vec![c, p1, p2],
            Self::Point { p, .. } => vec![p],
            Self::Spline { pts, .. } => pts.iter().map(String::as_str).collect(),
            Self::Ellipse { c, a, q, .. } => vec![c, a, q],
            Self::Other => vec![],
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
    /// `p1` and `p2` are the same distance from `c`.
    Equidistant {
        id: Id,
        c: Id,
        p1: Id,
        p2: Id,
    },
    /// `e2` keeps the offset from `e1` that `m2` has from `m1`.
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
    OnEdge {
        id: Id,
        target: Id,
    },
    #[serde(other)]
    Other,
}

impl Relation {
    pub fn id(&self) -> Option<&str> {
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
            | OnEdge { id, .. } => Some(id),
            Other => None,
        }
    }

    /// Ids the relation refers to (`relationRefs` in TypeScript).
    pub fn refs(&self) -> Vec<&str> {
        use Relation::*;
        match self {
            Horizontal { e, .. } | Vertical { e, .. } => vec![e],
            HorizontalPoints { p1, p2, .. } | VerticalPoints { p1, p2, .. } => vec![p1, p2],
            Coincident { p, e, .. } | Midpoint { p, e, .. } => vec![p, e],
            Symmetric { p1, p2, axis, .. } => vec![p1, p2, axis],
            Intersection { p, e1, e2, .. } => vec![p, e1, e2],
            Fix { target, .. } | OnEdge { target, .. } => vec![target],
            Equidistant { c, p1, p2, .. } => vec![c, p1, p2],
            Offset { e1, e2, .. }
            | Parallel { e1, e2, .. }
            | Perpendicular { e1, e2, .. }
            | Collinear { e1, e2, .. }
            | Equal { e1, e2, .. }
            | Tangent { e1, e2, .. }
            | Concentric { e1, e2, .. }
            | Coradial { e1, e2, .. } => vec![e1, e2],
            Other => vec![],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    Angle,
    Radius,
    Diameter,
    ArcLength,
    /// Difference of radii between an arc/circle and its offset.
    Offset,
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dimension {
    pub id: Id,
    #[serde(default)]
    pub name: String,
    pub kind: DimensionKind,
    pub refs: Vec<Id>,
    /// Millimetres or radians.
    pub value: f64,
    /// Driven (reference) dimensions display the measured value and do not constrain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub driven: Option<bool>,
    #[serde(default)]
    pub label: Vec2,
    /// Orientation captured at creation so signed residuals never flip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign: Option<f64>,
    /// Angles: which quadrant was picked (bit 0 flips e1, bit 1 flips e2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quadrant: Option<u32>,
}

impl Dimension {
    pub fn new(id: impl Into<Id>, kind: DimensionKind, refs: &[&str], value: f64) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            kind,
            refs: refs.iter().map(|s| s.to_string()).collect(),
            value,
            driven: None,
            label: [0.0, 0.0],
            sign: None,
            quadrant: None,
        }
    }
}

impl SketchData {
    /// An empty sketch holding only the origin (`emptySketch` in TypeScript).
    pub fn empty() -> Self {
        let mut points = Record::new();
        points.insert(ORIGIN_ID.into(), SketchPoint::new(ORIGIN_ID, 0.0, 0.0));
        Self {
            points,
            next_dim: 1,
            ..Default::default()
        }
    }

    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    pub fn add_point(&mut self, id: &str, x: f64, y: f64) {
        self.points.insert(id.into(), SketchPoint::new(id, x, y));
    }

    pub fn add_line(&mut self, id: &str, p1: &str, p2: &str) {
        let e = SketchEntity::Line {
            id: id.into(),
            p1: p1.into(),
            p2: p2.into(),
            construction: None,
            ext: None,
        };
        self.entities.insert(id.into(), e);
    }

    pub fn add_circle(&mut self, id: &str, c: &str, r: f64) {
        let e = SketchEntity::Circle {
            id: id.into(),
            c: c.into(),
            r,
            construction: None,
            ext: None,
        };
        self.entities.insert(id.into(), e);
    }

    pub fn add_arc(&mut self, id: &str, c: &str, p1: &str, p2: &str) {
        let e = SketchEntity::Arc {
            id: id.into(),
            c: c.into(),
            p1: p1.into(),
            p2: p2.into(),
            construction: None,
            ext: None,
        };
        self.entities.insert(id.into(), e);
    }

    pub fn add_relation(&mut self, r: Relation) {
        let id = r.id().expect("relation needs an id").to_string();
        self.relations.insert(id, r);
    }

    pub fn add_dimension(&mut self, d: Dimension) {
        self.dimensions.insert(d.id.clone(), d);
    }
}
