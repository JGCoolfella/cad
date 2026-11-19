//! The assembly document (`AssemblyDocument` in `src/assembly/types.ts`; work package G1).
//!
//! An assembly refers to part (and sub-assembly) files; it never copies their geometry:
//! - [`Component`]: one referenced document (a file, and the configuration used);
//! - [`Instance`]: one occurrence of a component in the assembly, with its placement
//!   ([`Transform`]) and whether it is fixed or floating (SOLIDWORKS' (f) and (-) prefixes
//!   belong to occurrences, so the flag lives here);
//! - [`Mate`]: a relation between geometry of two occurrences (or of one occurrence and
//!   the assembly's own planes and origin).
//!
//! Mate references follow section 3.3 of `docs/OVERHAUL_PLAN.md`: a face or edge is
//! stored as its persistent name plus signature (the same [`FaceSig`] / [`EdgeSig`] parts
//! store), so a mate finds its face again after the part is edited. Each reference also
//! keeps its analytic geometry in the part's own coordinates ([`MateGeom`]), which is what
//! a lightweight open (no part regenerated) solves with.
//!
//! Its schema version line is its own ([`ASM_CURRENT_VERSION`], [`ASM_MIGRATIONS`]); part
//! documents are unchanged.

use std::collections::{HashMap, HashSet};

use imbl::Vector;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{EdgeSig, FaceSig, Id, Migrations, StdPlane, UnitSystem, Vec3, VertexName};

/// The value of an assembly document's `format` field.
pub const ASM_FORMAT: &str = "opencad-assembly";

/// Assembly schema version written by this build.
///
/// - 1: components, instances with transforms, mates (G1a).
pub const ASM_CURRENT_VERSION: u32 = 1;

/// The assembly migrations of this build (none yet; `check()` keeps it a chain).
pub static ASM_MIGRATIONS: Migrations<'static> = Migrations { current: ASM_CURRENT_VERSION, steps: &[] };

/// The `format` field: always `"opencad-assembly"`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AsmFormat;

impl Serialize for AsmFormat {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(ASM_FORMAT)
    }
}

impl<'de> Deserialize<'de> for AsmFormat {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s == ASM_FORMAT {
            Ok(AsmFormat)
        } else {
            Err(D::Error::custom(format!("format is {s:?}, expected {ASM_FORMAT:?}")))
        }
    }
}

/// The `version` field of an assembly; in memory always [`ASM_CURRENT_VERSION`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AsmSchemaVersion;

impl Serialize for AsmSchemaVersion {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u32(ASM_CURRENT_VERSION)
    }
}

impl<'de> Deserialize<'de> for AsmSchemaVersion {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = u32::deserialize(d)?;
        if v == ASM_CURRENT_VERSION {
            Ok(AsmSchemaVersion)
        } else {
            Err(D::Error::custom(format!("version is {v}, expected {ASM_CURRENT_VERSION} (migrate first)")))
        }
    }
}

/// What a component's file holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComponentKind {
    Part,
    Assembly,
}

/// A referenced document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub id: Id,
    pub kind: ComponentKind,
    /// Path relative to the assembly file's folder, `/`-separated (`parts/pin.ocpart`).
    pub path: String,
    /// Absolute path the file was last found at; the second place the search looks
    /// (SOLIDWORKS stores absolute paths and searches from there).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abs_path: Option<String>,
    /// Configuration used. Parts have one, `"Default"`, until configurations exist.
    pub config: String,
}

/// A rigid placement: `world = rotation · local + position`. The rotation is a unit
/// quaternion `[w, x, y, z]` (normalised when read).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: [f64; 4],
}

impl Default for Transform {
    fn default() -> Self {
        Transform::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Transform = Transform { position: [0.0; 3], rotation: [1.0, 0.0, 0.0, 0.0] };

    /// The rotation as a unit quaternion (a zero quaternion reads as the identity).
    pub fn unit_rotation(&self) -> [f64; 4] {
        let q = self.rotation;
        let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        if n > 0.0 && n.is_finite() {
            [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
        } else {
            [1.0, 0.0, 0.0, 0.0]
        }
    }

    /// Rotates a direction.
    pub fn rotate(&self, v: Vec3) -> Vec3 {
        quat_rotate(self.unit_rotation(), v)
    }

    /// Maps a point from local to parent coordinates.
    pub fn apply(&self, p: Vec3) -> Vec3 {
        let r = self.rotate(p);
        [r[0] + self.position[0], r[1] + self.position[1], r[2] + self.position[2]]
    }

    /// `self ∘ inner`: first `inner`, then `self`.
    pub fn compose(&self, inner: &Transform) -> Transform {
        let q = quat_mul(self.unit_rotation(), inner.unit_rotation());
        Transform { position: self.apply(inner.position), rotation: q }
    }

    /// The inverse placement.
    pub fn inverse(&self) -> Transform {
        let q = self.unit_rotation();
        let qi = [q[0], -q[1], -q[2], -q[3]];
        let p = quat_rotate(qi, self.position);
        Transform { position: [-p[0], -p[1], -p[2]], rotation: qi }
    }

    /// Row-major 3×3 rotation matrix.
    pub fn matrix(&self) -> [[f64; 3]; 3] {
        let [w, x, y, z] = self.unit_rotation();
        [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
            [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
            [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
        ]
    }
}

/// Hamilton product `a · b` of quaternions `[w, x, y, z]`.
pub fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

/// Rotates `v` by the unit quaternion `q`.
pub fn quat_rotate(q: [f64; 4], v: Vec3) -> Vec3 {
    let (w, u) = (q[0], [q[1], q[2], q[3]]);
    let c1 = cross(u, v);
    let c2 = cross(u, c1);
    [v[0] + 2.0 * (w * c1[0] + c2[0]), v[1] + 2.0 * (w * c1[1] + c2[1]), v[2] + 2.0 * (w * c1[2] + c2[2])]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// One occurrence of a component.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub id: Id,
    /// The [`Component`] it is an occurrence of.
    pub component: Id,
    /// Display name in the FeatureManager, SOLIDWORKS style: `pin<1>`.
    pub name: String,
    /// Fixed in place ((f)); otherwise floating and placed by mates and dragging.
    pub fixed: bool,
    pub transform: Transform,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppressed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    /// Appearance colour of this occurrence (`#rrggbb`), over the part's own (G1b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Component Display (G1d): this occurrence's display mode instead of the view's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<DisplayMode>,
    /// Change Transparency (G1d): drawn transparent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transparent: Option<bool>,
}

/// A component's display mode (SOLIDWORKS: right-click a component > Component Display).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayMode {
    Wireframe,
    /// Hidden Lines Visible.
    Hlv,
    /// Hidden Lines Removed.
    Hlr,
    ShadedEdges,
    Shaded,
}

impl Instance {
    pub fn is_suppressed(&self) -> bool {
        self.suppressed == Some(true)
    }
}

/// Mate types of the Mate PropertyManager's Standard Mates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MateType {
    Coincident,
    Parallel,
    Perpendicular,
    Tangent,
    Concentric,
    Lock,
    Distance,
    Angle,
}

/// Mate alignment: the two references' directions (face normals, axis directions) point
/// the same way, or opposite ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    Aligned,
    AntiAligned,
}

/// Analytic geometry of a mate reference, in the coordinates of the part it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MateGeom {
    /// A planar face or a plane: a point on it and its normal (the face's outward normal).
    Plane { p: Vec3, n: Vec3 },
    /// A straight edge or an axis: a point on it and its direction.
    Line { p: Vec3, d: Vec3 },
    Point { p: Vec3 },
    /// A cylindrical face: a point on its axis, the axis direction, the radius.
    Cylinder { p: Vec3, d: Vec3, r: f64 },
    /// A conical face: apex-side point on its axis, the direction, the radius at `p` and the
    /// half angle.
    Cone { p: Vec3, d: Vec3, r: f64, half_angle: f64 },
    Sphere { c: Vec3, r: f64 },
    /// A circular edge: centre, normal of its plane, radius.
    Circle { c: Vec3, n: Vec3, r: f64 },
}

/// What a mate reference picks inside its occurrence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum MateEntity {
    /// A model face: persistent name and signature, plus its geometry when picked.
    Face { sig: FaceSig, geom: MateGeom },
    Edge { sig: EdgeSig, geom: MateGeom },
    Vertex {
        p: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<VertexName>,
    },
    /// A standard plane of the part (or of the assembly when the occurrence path is empty).
    Plane { plane: StdPlane },
    /// A reference plane, axis or point feature of the part, with its geometry when picked.
    RefPlane { feature_id: Id, geom: MateGeom },
    RefAxis { feature_id: Id, geom: MateGeom },
    RefPoint { feature_id: Id, geom: MateGeom },
    Origin,
    /// The whole occurrence (Lock mates).
    Component,
}

/// One side of a mate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MateRef {
    /// Instance ids from this assembly down to the occurrence that owns the entity; empty =
    /// the assembly itself (its planes and origin). The first id is the rigid body the mate
    /// moves (a sub-assembly moves as one).
    pub occurrence: Vec<Id>,
    pub entity: MateEntity,
}

/// A mate.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mate {
    pub id: Id,
    /// Display name: `Coincident1`, `Distance2`, ...
    pub name: String,
    #[serde(rename = "type")]
    pub kind: MateType,
    /// Exactly two references.
    pub refs: Vec<MateRef>,
    /// Required by the mates that orient directions (all but Lock and Perpendicular).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    /// Distance (mm) or angle (radians) of Distance and Angle mates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// Flip Dimension: measure the distance to the other side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip: Option<bool>,
    /// Lock mates: the placement of the second occurrence in the first's coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative: Option<Transform>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppressed: Option<bool>,
}

impl Mate {
    pub fn is_suppressed(&self) -> bool {
        self.suppressed == Some(true)
    }
}

/// An assembly document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssemblyDocument {
    pub format: AsmFormat,
    pub version: AsmSchemaVersion,
    pub name: String,
    pub units: UnitSystem,
    pub components: Vector<Component>,
    pub instances: Vector<Instance>,
    pub mates: Vector<Mate>,
}

impl AssemblyDocument {
    /// An empty assembly (File > New > Assembly).
    pub fn new(name: impl Into<String>, units: UnitSystem) -> Self {
        AssemblyDocument {
            format: AsmFormat,
            version: AsmSchemaVersion,
            name: name.into(),
            units,
            components: Vector::new(),
            instances: Vector::new(),
            mates: Vector::new(),
        }
    }

    pub fn component(&self, id: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.id == *id)
    }

    pub fn instance(&self, id: &str) -> Option<&Instance> {
        self.instances.iter().find(|c| c.id == *id)
    }

    /// Same data (for [`crate::History`]).
    pub fn same_as(&self, other: &Self) -> bool {
        self == other
    }

    /// Structural invariants: unique ids, instances naming existing components, mates with
    /// two references to existing instances, the fields each mate type needs. One message
    /// per problem.
    pub fn validate(&self) -> Vec<String> {
        let mut issues = Vec::new();
        let mut ids: HashSet<Id> = HashSet::new();
        let mut dup = |id: &Id, what: &str, issues: &mut Vec<String>| {
            if !ids.insert(id.clone()) {
                issues.push(format!("{what} id {id} is used twice"));
            }
        };
        for c in &self.components {
            dup(&c.id, "component", &mut issues);
            if c.path.is_empty() {
                issues.push(format!("component {} has no path", c.id));
            }
        }
        for i in &self.instances {
            dup(&i.id, "instance", &mut issues);
            if self.component(&i.component).is_none() {
                issues.push(format!("instance {} refers to missing component {}", i.id, i.component));
            }
            if i.transform.rotation.iter().all(|v| *v == 0.0) {
                issues.push(format!("instance {} has a zero rotation", i.id));
            }
        }
        let inst: HashMap<&str, &Instance> = self.instances.iter().map(|i| (i.id.as_str(), i)).collect();
        for m in &self.mates {
            dup(&m.id, "mate", &mut issues);
            if m.refs.len() != 2 {
                issues.push(format!("mate {} has {} references, expected 2", m.id, m.refs.len()));
            }
            for r in &m.refs {
                if let Some(first) = r.occurrence.first() {
                    if !inst.contains_key(first.as_str()) {
                        issues.push(format!("mate {} refers to missing instance {first}", m.id));
                    }
                }
                if r.occurrence.is_empty() && matches!(r.entity, MateEntity::Component) {
                    issues.push(format!("mate {} locks the assembly itself", m.id));
                }
            }
            let needs_align = !matches!(m.kind, MateType::Lock | MateType::Perpendicular);
            if needs_align && m.align.is_none() {
                issues.push(format!("mate {} ({:?}) has no alignment", m.id, m.kind));
            }
            if matches!(m.kind, MateType::Distance | MateType::Angle) && m.value.is_none() {
                issues.push(format!("mate {} ({:?}) has no value", m.id, m.kind));
            }
            if m.kind == MateType::Lock && m.relative.is_none() {
                issues.push(format!("lock mate {} has no relative placement", m.id));
            }
        }
        issues
    }
}
