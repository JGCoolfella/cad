//! Inputs of the modelling operations. Each spec states intent (what SOLIDWORKS' feature
//! asks for), not an algorithm, so another kernel can satisfy it its own way.

use crate::geom::{Axis, Plane, Profile, Wire, P3};
use crate::topo::{EdgeId, FaceId};

/// Simple solids, mostly for tests and tool bodies.
#[derive(Clone, Debug, PartialEq)]
pub enum Primitive {
    /// Axis-aligned box from `min` with edge lengths `size` (all > 0).
    Block { min: P3, size: P3 },
    /// Cylinder on `axis` from its origin, `height` along `axis.dir`.
    Cylinder { axis: Axis, radius: f64, height: f64 },
    Sphere { center: P3, radius: f64 },
}

/// Taper of an extrusion's side faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Taper {
    /// Draft angle in radians, `0 < angle < π/2`.
    pub angle: f64,
    /// `false` (SOLIDWORKS' default, "Draft outward" off): the far end is smaller; holes in
    /// the profile grow. `true`: the far end is larger.
    pub outward: bool,
}

/// Linear extrusion of planar regions.
#[derive(Clone, Debug, PartialEq)]
pub struct PrismSpec {
    pub profile: Profile,
    /// Extrusion vector: direction and length. Must not lie in the profile plane.
    pub vector: P3,
    pub taper: Option<Taper>,
}

/// Revolution of planar regions about an axis in their plane.
#[derive(Clone, Debug, PartialEq)]
pub struct RevolveSpec {
    pub profile: Profile,
    pub axis: Axis,
    /// Radians, right-handed about `axis.dir`; `2π` (or more) is a full revolution.
    pub angle: f64,
}

/// How the profile is oriented along a sweep path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepOrientation {
    /// SOLIDWORKS "Follow Path": the profile turns with the path (Frenet frame).
    FollowPath,
    /// SOLIDWORKS "Keep Normal Constant": the profile keeps its orientation.
    KeepNormal,
}

/// Sweep of planar regions along a path.
#[derive(Clone, Debug, PartialEq)]
pub struct SweepSpec {
    pub profile: Profile,
    /// Open or closed connected path; the profile should sit at its start.
    pub path: Wire,
    pub orientation: SweepOrientation,
}

/// Loft through closed sections, first to last. Section `k` is reported in history as
/// region `k` (wire 0).
#[derive(Clone, Debug, PartialEq)]
pub struct LoftSpec {
    pub sections: Vec<Wire>,
    /// Straight (ruled) faces between sections instead of smooth ones.
    pub ruled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BooleanOp {
    /// Union.
    Fuse,
    /// Target minus tools.
    Cut,
    /// Intersection.
    Common,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Radius {
    Constant(f64),
    /// Varies linearly along the edge from its start to its end (SOLIDWORKS variable
    /// size fillet with two vertices).
    Linear(f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilletEdge {
    pub edge: EdgeId,
    pub radius: Radius,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferSize {
    /// Equal distance on both faces.
    Equal(f64),
    /// `d1` measured on `face` (one of the edge's two faces), `d2` on the other.
    Distances { d1: f64, d2: f64, face: FaceId },
    /// Distance on `face`, and the chamfer face at `angle` radians to `face`.
    DistanceAngle { distance: f64, angle: f64, face: FaceId },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChamferEdge {
    pub edge: EdgeId,
    pub size: ChamferSize,
}

/// Hollow a solid, removing `open` faces.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellSpec {
    pub open: Vec<FaceId>,
    pub thickness: f64,
    /// `false`: the wall grows inwards (SOLIDWORKS default); `true`: outwards.
    pub outward: bool,
}

/// Draft (taper) faces about a neutral plane.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftSpec {
    pub faces: Vec<FaceId>,
    /// Faces keep their intersection with this plane; its normal is the pull direction.
    pub neutral: Plane,
    /// Radians. Positive tilts the faces so the part narrows along the pull direction.
    pub angle: f64,
}

/// Tessellation tolerances. See [`crate::Tolerances::display_mesh`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshParams {
    /// Maximum chordal deviation, mm.
    pub linear: f64,
    /// Maximum angle between adjacent facet normals along a curve, radians.
    pub angular: f64,
}
