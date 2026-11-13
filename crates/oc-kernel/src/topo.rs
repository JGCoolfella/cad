//! What the kernel reports *about* a body: topology with geometry, mass properties,
//! distances, validity and tessellations.
//!
//! Faces, edges and vertices are addressed by their index in the body's own enumeration
//! ([`FaceId`] etc.). The order is deterministic for a given body (same inputs, same
//! operations → same order), but it is *not* stable across edits; persistent names
//! (`oc-naming`) are built from operation history, never from indices.

use crate::geom::{Axis, P3};

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u32);
        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}
id_type!(
    /// Index of a face in its body.
    FaceId
);
id_type!(
    /// Index of an edge in its body.
    EdgeId
);
id_type!(
    /// Index of a vertex in its body.
    VertexId
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TopoKind {
    Face,
    Edge,
    Vertex,
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub min: P3,
    pub max: P3,
}

impl BBox {
    pub fn diagonal(&self) -> f64 {
        crate::geom::v3::dist(self.min, self.max)
    }
}

/// Underlying surface of a face. Analytic surfaces carry their parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Surface {
    Plane { origin: P3, normal: P3 },
    Cylinder { axis: Axis, radius: f64 },
    /// `radius` is the radius at `axis.origin`; the radius grows by `tan(half_angle)` per
    /// unit along `axis.dir`.
    Cone { axis: Axis, radius: f64, half_angle: f64 },
    Sphere { center: P3, radius: f64 },
    Torus { axis: Axis, major: f64, minor: f64 },
    BSpline,
    Bezier,
    Revolution,
    Extrusion,
    Offset,
    Other,
}

impl Surface {
    pub fn name(&self) -> &'static str {
        match self {
            Surface::Plane { .. } => "plane",
            Surface::Cylinder { .. } => "cylinder",
            Surface::Cone { .. } => "cone",
            Surface::Sphere { .. } => "sphere",
            Surface::Torus { .. } => "torus",
            Surface::BSpline => "bspline",
            Surface::Bezier => "bezier",
            Surface::Revolution => "revolution",
            Surface::Extrusion => "extrusion",
            Surface::Offset => "offset",
            Surface::Other => "other",
        }
    }
}

/// Underlying curve of an edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Line,
    Circle { center: P3, normal: P3, radius: f64 },
    Ellipse { center: P3, normal: P3, major_dir: P3, major: f64, minor: f64 },
    BSpline,
    Bezier,
    Other,
}

impl Curve {
    pub fn name(&self) -> &'static str {
        match self {
            Curve::Line => "line",
            Curve::Circle { .. } => "circle",
            Curve::Ellipse { .. } => "ellipse",
            Curve::BSpline => "bspline",
            Curve::Bezier => "bezier",
            Curve::Other => "other",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FaceInfo {
    pub surface: Surface,
    pub area: f64,
    /// Centre of area of the face.
    pub centroid: P3,
    /// Outward unit normal of the surface at the point nearest the centroid (the
    /// "face signature" normal; for curved faces it depends on that point).
    pub normal: P3,
    /// Contains the face; tight for planar faces (curved faces may get a looser box).
    pub bbox: BBox,
    /// Boundary edges, ascending, each once.
    pub edges: Vec<EdgeId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EdgeInfo {
    pub curve: Curve,
    /// Start and end point (equal for a closed edge such as a full circle).
    pub start: P3,
    pub end: P3,
    /// Point at the middle of the parameter range.
    pub mid: P3,
    pub length: f64,
    /// Faces bounded by the edge, ascending. Two for a manifold solid; one entry for a
    /// seam (the same face on both sides).
    pub faces: Vec<FaceId>,
    /// The edge closes a periodic face on itself (cylinder seam); hidden in displays.
    pub seam: bool,
    pub vertices: Vec<VertexId>,
}

/// Faces, edges and vertices of a body with their geometry, in the body's index order.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Topology {
    pub solids: u32,
    pub shells: u32,
    pub faces: Vec<FaceInfo>,
    pub edges: Vec<EdgeInfo>,
    pub vertices: Vec<P3>,
}

impl Topology {
    /// Edges shared by faces `a` and `b`.
    pub fn edges_between(&self, a: FaceId, b: FaceId) -> Vec<EdgeId> {
        let fa = &self.faces[a.index()].edges;
        self.faces[b.index()].edges.iter().copied().filter(|e| fa.contains(e)).collect()
    }
}

/// Mass properties for unit density (mass = volume).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProps {
    pub volume: f64,
    pub area: f64,
    pub center: P3,
    /// Inertia tensor about the centre of mass, axes parallel to the world axes.
    pub inertia: [[f64; 3]; 3],
    /// Principal moments, ascending, and their unit axes.
    pub principal_moments: [f64; 3],
    pub principal_axes: [P3; 3],
}

/// Minimum distance between two targets and a pair of points realising it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distance {
    pub distance: f64,
    pub a: P3,
    pub b: P3,
}

/// Something to measure from or to.
#[derive(Clone, Copy, Debug)]
pub enum Target<'a, B> {
    Point(P3),
    Body(&'a B),
    Face(&'a B, FaceId),
    Edge(&'a B, EdgeId),
    Vertex(&'a B, VertexId),
}

/// One problem found by a validity check.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    /// Entity concerned, if the check names one.
    pub entity: Option<(TopoKind, u32)>,
    /// Kernel-specific code (e.g. OCCT's `BRepCheck_NotClosed`) or one of the generic
    /// codes `NegativeVolume`, `NoSolid`.
    pub code: String,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Check {
    pub valid: bool,
    pub problems: Vec<Problem>,
}

/// Triangles of a body, grouped by face, plus a polyline per edge.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Mesh {
    pub positions: Vec<P3>,
    /// Unit normals per position, pointing out of the material: the exact surface normal,
    /// or an average of the adjacent facets' where the surface has none (apex, pole).
    pub normals: Vec<[f32; 3]>,
    /// Counter-clockwise seen from outside.
    pub triangles: Vec<[u32; 3]>,
    /// `faces[i]` = triangle range of face `i` (half-open, in triangles).
    pub faces: Vec<std::ops::Range<u32>>,
    /// Polyline of edge `i`.
    pub edges: Vec<Vec<P3>>,
}

impl Mesh {
    /// Sum of triangle areas.
    pub fn area(&self) -> f64 {
        use crate::geom::v3;
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.positions[i as usize]);
                0.5 * v3::len(v3::cross(v3::sub(b, a), v3::sub(c, a)))
            })
            .sum()
    }
}
