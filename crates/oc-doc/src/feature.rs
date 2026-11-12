//! Features (`features` section of `src/core/types.ts`).
//!
//! TypeScript's `FeatureBase` (`id`, `name`, `suppressed`) becomes the fields of
//! [`Feature`]; the per-type data is [`FeatureKind`], tagged by `type` in JSON.

use serde::{Deserialize, Serialize};

use crate::{EdgeSig, FaceSig, GeomRef, Id, PlaneRef, SketchData, Vec2, Vec3};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub id: Id,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppressed: Option<bool>,
    #[serde(flatten)]
    pub kind: FeatureKind,
}

impl Feature {
    pub fn new(id: impl Into<Id>, name: impl Into<String>, kind: FeatureKind) -> Self {
        Feature {
            id: id.into(),
            name: name.into(),
            suppressed: None,
            kind,
        }
    }

    pub fn is_suppressed(&self) -> bool {
        self.suppressed == Some(true)
    }

    /// The sketch, for sketch features.
    pub fn sketch(&self) -> Option<&SketchData> {
        match &self.kind {
            FeatureKind::Sketch(s) => Some(&s.sketch),
            _ => None,
        }
    }

    pub fn sketch_mut(&mut self) -> Option<&mut SketchData> {
        match &mut self.kind {
            FeatureKind::Sketch(s) => Some(&mut s.sketch),
            _ => None,
        }
    }
}

/// Per-type feature data. The JSON tag (`type`) of each variant is [`FeatureKind::type_name`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(
    clippy::large_enum_variant,
    reason = "each feature is its own Arc allocation (about 0.5 KB) and is never moved by value in bulk"
)]
pub enum FeatureKind {
    #[serde(rename = "sketch")]
    Sketch(SketchFeature),
    #[serde(rename = "extrude")]
    Extrude(ExtrudeFeature),
    #[serde(rename = "revolve")]
    Revolve(RevolveFeature),
    #[serde(rename = "fillet")]
    Fillet(FilletFeature),
    #[serde(rename = "chamfer")]
    Chamfer(ChamferFeature),
    #[serde(rename = "refplane")]
    RefPlane(RefPlaneFeature),
    #[serde(rename = "linearPattern")]
    LinearPattern(LinearPatternFeature),
    #[serde(rename = "circularPattern")]
    CircularPattern(CircularPatternFeature),
    #[serde(rename = "mirror")]
    Mirror(MirrorFeature),
    #[serde(rename = "shell")]
    Shell(ShellFeature),
    #[serde(rename = "imported")]
    Imported(ImportedFeature),
    #[serde(rename = "rib")]
    Rib(RibFeature),
    #[serde(rename = "draft")]
    Draft(DraftFeature),
    #[serde(rename = "hole")]
    Hole(HoleFeature),
    #[serde(rename = "sweep")]
    Sweep(SweepFeature),
    #[serde(rename = "loft")]
    Loft(LoftFeature),
    #[serde(rename = "refaxis")]
    RefAxis(RefAxisFeature),
    #[serde(rename = "refpoint")]
    RefPoint(RefPointFeature),
    #[serde(rename = "coordsys")]
    CoordSys(CoordSysFeature),
}

impl FeatureKind {
    /// Every feature type's JSON tag (`FeatureType` in TypeScript).
    pub const TYPE_NAMES: [&'static str; 19] = [
        "sketch",
        "extrude",
        "revolve",
        "fillet",
        "chamfer",
        "refplane",
        "linearPattern",
        "circularPattern",
        "mirror",
        "shell",
        "imported",
        "rib",
        "draft",
        "hole",
        "sweep",
        "loft",
        "refaxis",
        "refpoint",
        "coordsys",
    ];

    /// The JSON tag (`type`), as in TypeScript's `FeatureType`.
    pub fn type_name(&self) -> &'static str {
        use FeatureKind::*;
        match self {
            Sketch(_) => "sketch",
            Extrude(_) => "extrude",
            Revolve(_) => "revolve",
            Fillet(_) => "fillet",
            Chamfer(_) => "chamfer",
            RefPlane(_) => "refplane",
            LinearPattern(_) => "linearPattern",
            CircularPattern(_) => "circularPattern",
            Mirror(_) => "mirror",
            Shell(_) => "shell",
            Imported(_) => "imported",
            Rib(_) => "rib",
            Draft(_) => "draft",
            Hole(_) => "hole",
            Sweep(_) => "sweep",
            Loft(_) => "loft",
            RefAxis(_) => "refaxis",
            RefPoint(_) => "refpoint",
            CoordSys(_) => "coordsys",
        }
    }
}

// ---------------------------------------------------------------- end conditions

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EndConditionType {
    Blind,
    ThroughAll,
    ThroughAllBoth,
    UpToNext,
    UpToVertex,
    UpToSurface,
    OffsetFromSurface,
    UpToBody,
    MidPlane,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndCondition {
    #[serde(rename = "type")]
    pub kind: EndConditionType,
    /// Depth (Blind, Mid Plane) or offset distance (Offset From Surface).
    pub depth: f64,
    /// Face/plane for Up To Surface and Offset From Surface.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    pub r#ref: Option<Option<GeomRef>>,
    /// Vertex position for Up To Vertex.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    pub vertex: Option<Option<Vec3>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse_offset: Option<bool>,
    /// Radians; 0 = none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_outward: Option<bool>,
}

impl EndCondition {
    pub fn blind(depth: f64) -> Self {
        EndCondition {
            kind: EndConditionType::Blind,
            depth,
            r#ref: None,
            vertex: None,
            reverse_offset: None,
            draft: None,
            draft_outward: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExtrudeFromType {
    SketchPlane,
    Offset,
    Surface,
}

/// Where an extrusion starts (Extrude PropertyManager "From").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExtrudeFrom {
    #[serde(rename = "type")]
    pub kind: ExtrudeFromType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    pub r#ref: Option<Option<GeomRef>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThinType {
    OneDirection,
    MidPlane,
    TwoDirection,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThinOptions {
    #[serde(rename = "type")]
    pub kind: ThinType,
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness2: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reverse: Option<bool>,
}

// ---------------------------------------------------------------- feature types

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SketchFeature {
    pub plane: PlaneRef,
    pub sketch: SketchData,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtrudeFeature {
    pub cut: bool,
    pub sketch_id: Id,
    pub reverse: bool,
    pub end1: EndCondition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir2: Option<EndCondition>,
    pub merge: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<ExtrudeFrom>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin: Option<ThinOptions>,
    /// Selected Contours: points (sketch coordinates) inside the regions to use. Empty = all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contours: Option<Vec<Vec2>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip_side_to_cut: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RevolveType {
    OneDirection,
    MidPlane,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevolveFeature {
    pub cut: bool,
    pub sketch_id: Id,
    /// Sketch line or centerline entity used as axis.
    pub axis: Id,
    pub angle: f64,
    pub rev_type: RevolveType,
    pub reverse: bool,
    pub merge: bool,
    /// Axis picked in the graphics area (edge, reference axis); overrides the sketch line.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "crate::nullable"
    )]
    pub axis_ref: Option<Option<GeomRef>>,
    /// Direction 2 angle (radians).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle2: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thin: Option<ThinOptions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contours: Option<Vec<Vec2>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilletFeature {
    pub radius: f64,
    /// Variable Size Fillet: radius at the end of each edge (start = radius).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radius2: Option<f64>,
    pub edges: Vec<EdgeSig>,
    pub faces: Vec<FaceSig>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChamferType {
    Equal,
    DistanceDistance,
    AngleDistance,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChamferFeature {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chamfer_type: Option<ChamferType>,
    pub distance: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distance2: Option<f64>,
    /// Angle-Distance: angle in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    /// Measure the first distance on the other face of each edge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flip: Option<bool>,
    pub edges: Vec<EdgeSig>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefPlaneFeature {
    /// Legacy single reference (offset planes); `refs` takes precedence when set.
    pub base: PlaneRef,
    pub offset: f64,
    pub flip: bool,
    /// Up to three references; the plane type follows from them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refs: Option<Vec<GeomRef>>,
    /// At Angle: angle in radians.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefAxisFeature {
    pub refs: Vec<GeomRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RefPointFeature {
    pub refs: Vec<GeomRef>,
    /// Coordinates when no reference is picked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xyz: Option<Vec3>,
}

/// Origin, X axis, Y axis references (origin defaults to the part origin). Each is
/// required in the JSON and may be `null`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordSysFeature {
    #[serde(deserialize_with = "Option::deserialize")]
    pub origin: Option<GeomRef>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub x_axis: Option<GeomRef>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub y_axis: Option<GeomRef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternDirection {
    #[serde(deserialize_with = "Option::deserialize")]
    pub r#ref: Option<GeomRef>,
    pub reverse: bool,
    pub spacing: f64,
    /// Number of instances including the seed.
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinearPatternFeature {
    /// Features to pattern (their added/removed bodies are repeated).
    pub seeds: Vec<Id>,
    pub dir1: PatternDirection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir2: Option<PatternDirection>,
    /// Instances to Skip: `[i, j]`, 1-based along Direction 1 and 2 (the seed is `[1, 1]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<Vec<[u32; 2]>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CircularPatternFeature {
    pub seeds: Vec<Id>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub axis: Option<GeomRef>,
    pub reverse: bool,
    /// Total angle (equal spacing) or the angle between instances.
    pub angle: f64,
    pub count: u32,
    pub equal_spacing: bool,
    /// Instances to Skip: 1-based instance numbers (the seed is 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip: Option<Vec<u32>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MirrorFeature {
    pub seeds: Vec<Id>,
    #[serde(deserialize_with = "Option::deserialize")]
    pub plane: Option<GeomRef>,
    /// Mirror the whole body instead of features ("Bodies to Mirror").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SweepProfileType {
    Sketch,
    Circular,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SweepOrientation {
    FollowPath,
    KeepNormal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SweepFeature {
    pub cut: bool,
    /// Sketch Profile (a closed sketch) or Circular Profile (diameter along the path).
    pub profile_type: SweepProfileType,
    pub profile: Id,
    pub diameter: f64,
    pub path: Id,
    pub orientation: SweepOrientation,
    pub merge: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoftFeature {
    pub cut: bool,
    /// Profile sketches in loft order.
    pub profiles: Vec<Id>,
    pub merge: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HoleType {
    Cbore,
    Csk,
    Hole,
    Tap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HoleStandard {
    #[serde(rename = "ISO")]
    Iso,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HoleFit {
    Close,
    Normal,
    Loose,
}

/// Show custom sizing: overrides of the standard values (mm, radians).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoleCustom {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dia: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cbore_dia: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cbore_depth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csk_dia: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csk_angle: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HoleEndType {
    Blind,
    ThroughAll,
    UpToNext,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HoleEnd {
    #[serde(rename = "type")]
    pub kind: HoleEndType,
    pub depth: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoleFeature {
    pub hole_type: HoleType,
    pub standard: HoleStandard,
    pub size: String,
    pub fit: HoleFit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom: Option<HoleCustom>,
    pub end: HoleEnd,
    /// Placement face while the PropertyManager is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<FaceSig>,
    /// Hole centres (face sketch coordinates) while the PropertyManager is open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positions: Option<Vec<Vec2>>,
    /// Positions sketch (sketch points), created on OK and absorbed under the hole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sketch_id: Option<Id>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DraftFeature {
    /// Draft angle (radians).
    pub angle: f64,
    /// Neutral plane (planar face or plane); its normal is the direction of pull.
    #[serde(deserialize_with = "Option::deserialize")]
    pub neutral: Option<GeomRef>,
    pub reverse: bool,
    pub faces: Vec<FaceSig>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RibSide {
    Both,
    Side1,
    Side2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RibDirection {
    Parallel,
    Normal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RibFeature {
    /// Open sketch (lines) drawn on a plane through the body.
    pub sketch_id: Id,
    pub thickness: f64,
    pub side: RibSide,
    pub direction: RibDirection,
    /// Material side; by default the side where the rib meets the body.
    pub flip: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImportFormat {
    #[serde(rename = "step")]
    Step,
}

/// A body imported from a neutral file (STEP), stored in the document.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportedFeature {
    pub format: ImportFormat,
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShellFeature {
    pub thickness: f64,
    /// Faces removed (open faces).
    pub faces: Vec<FaceSig>,
    pub outward: bool,
}
