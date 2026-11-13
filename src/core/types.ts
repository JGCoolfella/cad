/**
 * Core document model.
 *
 * Everything in here is plain, JSON-serialisable data. The document is the single
 * source of truth; geometry (B-rep) is always derived from it by the kernel worker.
 * Units: millimetres and radians internally, regardless of display units.
 */

export type Id = string;
export type Vec2 = [number, number];
export type Vec3 = [number, number, number];

// ---------------------------------------------------------------- sketch data

export interface SketchPoint {
  id: Id;
  x: number;
  y: number;
  /** Points owned by an entity (endpoints, centres) vs. a free-standing sketch point. */
  standalone?: boolean;
  /**
   * A line's midpoint picked for a relation or dimension (SOLIDWORKS lets you select a
   * line's midpoint directly). Tied by a midpoint relation; drawn only when highlighted.
   */
  virtual?: boolean;
  /**
   * Owned by external reference `ext` (a model vertex or edge projected into the sketch):
   * fixed for the solver, moved by the rebuild when the model changes, not part of profiles.
   */
  ext?: Id;
}

/** `ext`: the entity stands for a model edge (external reference id); see SketchRef. */
export type SketchEntity =
  | { id: Id; type: "line"; p1: Id; p2: Id; construction?: boolean; ext?: Id }
  | { id: Id; type: "circle"; c: Id; r: number; construction?: boolean; ext?: Id }
  /** Arc runs counter-clockwise (in sketch coordinates) from p1 to p2 about c. */
  | { id: Id; type: "arc"; c: Id; p1: Id; p2: Id; construction?: boolean; ext?: Id }
  | { id: Id; type: "point"; p: Id }
  /** Ellipse: centre, a point at the end of one axis and one at the end of the other (kept perpendicular). */
  | { id: Id; type: "ellipse"; c: Id; a: Id; q: Id; construction?: boolean }
  /** Spline through points (open; centripetal Catmull-Rom between them). */
  | { id: Id; type: "spline"; pts: Id[]; construction?: boolean };

export type SketchEntityType = SketchEntity["type"];

/** Geometric relations, named as in the SOLIDWORKS Add Relations PropertyManager. */
export type Relation =
  | { id: Id; type: "horizontal"; e: Id }
  | { id: Id; type: "vertical"; e: Id }
  | { id: Id; type: "horizontalPoints"; p1: Id; p2: Id }
  | { id: Id; type: "verticalPoints"; p1: Id; p2: Id }
  | { id: Id; type: "coincident"; p: Id; e: Id }
  | { id: Id; type: "midpoint"; p: Id; e: Id }
  | { id: Id; type: "parallel"; e1: Id; e2: Id }
  | { id: Id; type: "perpendicular"; e1: Id; e2: Id }
  | { id: Id; type: "collinear"; e1: Id; e2: Id }
  | { id: Id; type: "equal"; e1: Id; e2: Id }
  | { id: Id; type: "tangent"; e1: Id; e2: Id; inner?: boolean }
  | { id: Id; type: "concentric"; e1: Id; e2: Id }
  | { id: Id; type: "coradial"; e1: Id; e2: Id }
  | { id: Id; type: "symmetric"; p1: Id; p2: Id; axis: Id }
  | { id: Id; type: "intersection"; p: Id; e1: Id; e2: Id }
  /** p1 and p2 are the same distance from c (keeps polygons regular; shown as "Pattern"). */
  | { id: Id; type: "equidistant"; c: Id; p1: Id; p2: Id }
  /**
   * Offset: e2 stays at the same offset from e1 as m2 is from m1 (the pair carrying the
   * offset dimension). Created by Offset Entities.
   */
  | { id: Id; type: "offset"; e1: Id; e2: Id; m1: Id; m2: Id }
  | { id: Id; type: "fix"; target: Id }
  /** Converted from a model edge (Convert Entities): fixed in place, shown as "On Edge". */
  | { id: Id; type: "onEdge"; target: Id };

export type RelationType = Relation["type"];

export type DimensionKind =
  | "distance" // point-point aligned
  | "horizontal" // point-point horizontal (signed)
  | "vertical" // point-point vertical (signed)
  | "pointLine" // perpendicular point-line distance
  | "lineLine" // distance between parallel lines
  | "angle" // angle between two lines
  | "radius"
  | "diameter"
  | "arcLength"
  /** Distance between an arc/circle and its offset (difference of radii). */
  | "offset";

export interface Dimension {
  id: Id;
  name: string; // e.g. "D1"
  kind: DimensionKind;
  /** Referenced point ids / entity ids, meaning depends on kind. */
  refs: Id[];
  value: number; // mm or rad
  /** Driven (reference) dimensions are shown grey and do not constrain. */
  driven?: boolean;
  /** Label position in sketch coordinates. */
  label: Vec2;
  /** Orientation bookkeeping captured at creation so signed residuals never flip. */
  sign?: number;
  /** For angles: which of the 4 angle quadrants was picked (bit 0: flip e1, bit 1: flip e2). */
  quadrant?: number;
}

export interface SketchData {
  points: Record<Id, SketchPoint>;
  entities: Record<Id, SketchEntity>;
  relations: Record<Id, Relation>;
  dimensions: Record<Id, Dimension>;
  nextDim: number;
  /** Model vertices and edges the sketch refers to (relations, dimensions). */
  refs?: Record<Id, SketchRef>;
}

/**
 * External reference from a sketch to model geometry, as SOLIDWORKS relations to model
 * edges and vertices. The rebuild re-finds it (by name in the native engine; by signature,
 * as fillet edges are, in this one), projects it onto the sketch plane and moves the fixed
 * sketch geometry that stands for it.
 */
export type SketchRef =
  /**
   * A vertex is re-found by `name`; without one, as the common end of its edges (`edges`),
   * else the nearest vertex.
   */
  | { id: Id; kind: "vertex"; p: Vec3; edges?: EdgeSig[]; name?: VertexName }
  | { id: Id; kind: "edge"; sig: EdgeSig };

// ---------------------------------------------------------------- references

/**
 * Persistent names (schema version 2, `docs/OVERHAUL_PLAN.md` section 3). A face is named
 * by where it came from in the feature history: its tags, `"<featureId>/<role>[/<key>]"`,
 * sorted and without duplicates (a merged face has several). An edge is named by the two
 * faces it lies between and a vertex by the faces around it. The native engine
 * (`crates/oc-naming`) produces and reads them; this engine ignores them and finds
 * references by signature. A reference without a name is a legacy one.
 */
export type FaceName = string[];
export type EdgeName = [FaceName, FaceName];
export type VertexName = FaceName[];

/** Reference to a face: its geometric signature as picked, and its persistent name. */
export interface FaceSig {
  surface: string; // "PLANE" | "CYLINDRE" | ...
  center: Vec3;
  normal: Vec3;
  area: number;
  name?: FaceName;
}
/** Reference to an edge: its geometric signature as picked, and its persistent name. */
export interface EdgeSig {
  curve: string;
  mid: Vec3;
  a: Vec3;
  b: Vec3;
  length: number;
  name?: EdgeName;
}

export type PlaneRef =
  | { kind: "std"; plane: "front" | "top" | "right" }
  | { kind: "face"; sig: FaceSig }
  | { kind: "refplane"; featureId: Id };

/** An orthonormal frame. normal = x × y. */
export interface Frame {
  origin: Vec3;
  x: Vec3;
  y: Vec3;
  normal: Vec3;
}

// ---------------------------------------------------------------- features

export type EndConditionType =
  | "blind"
  | "throughAll"
  | "throughAllBoth"
  | "upToNext"
  | "upToVertex"
  | "upToSurface"
  | "offsetFromSurface"
  | "upToBody"
  | "midPlane";

export interface EndCondition {
  type: EndConditionType;
  /** Depth (Blind, Mid Plane) or offset distance (Offset From Surface). */
  depth: number;
  /** Face/plane for Up To Surface and Offset From Surface. */
  ref?: GeomRef | null;
  /** Vertex position for Up To Vertex. */
  vertex?: Vec3 | null;
  /** Persistent name of that vertex when it is a model vertex. */
  vertexName?: VertexName;
  reverseOffset?: boolean;
  draft?: number; // radians, 0 = none
  draftOutward?: boolean;
}

/** Where an extrusion starts (Extrude PropertyManager "From"). */
export interface ExtrudeFrom {
  type: "sketchPlane" | "offset" | "surface";
  offset?: number;
  reverse?: boolean;
  ref?: GeomRef | null;
}

export interface ThinOptions {
  type: "oneDirection" | "midPlane" | "twoDirection";
  thickness: number;
  thickness2?: number;
  reverse?: boolean;
}

interface FeatureBase {
  id: Id;
  name: string;
  suppressed?: boolean;
}

export interface SketchFeature extends FeatureBase {
  type: "sketch";
  plane: PlaneRef;
  sketch: SketchData;
}

export interface ExtrudeFeature extends FeatureBase {
  type: "extrude";
  cut: boolean;
  sketchId: Id;
  reverse: boolean;
  end1: EndCondition;
  dir2?: EndCondition;
  merge: boolean;
  from?: ExtrudeFrom;
  thin?: ThinOptions;
  /** Selected Contours: points (sketch coordinates) inside the regions to use. Empty = all. */
  contours?: Vec2[];
  flipSideToCut?: boolean;
}

export interface RevolveFeature extends FeatureBase {
  type: "revolve";
  cut: boolean;
  sketchId: Id;
  /** Sketch line or centerline entity used as axis. */
  axis: Id;
  angle: number;
  revType: "oneDirection" | "midPlane";
  reverse: boolean;
  merge: boolean;
  /** Axis picked in the graphics area (edge, reference axis); overrides the sketch line. */
  axisRef?: GeomRef | null;
  /** Direction 2 angle (radians). */
  angle2?: number;
  thin?: ThinOptions;
  contours?: Vec2[];
}

export interface FilletFeature extends FeatureBase {
  type: "fillet";
  radius: number;
  /** Variable Size Fillet: radius at the end of each edge (start = radius). */
  radius2?: number;
  edges: EdgeSig[];
  faces: FaceSig[];
}

export interface ChamferFeature extends FeatureBase {
  type: "chamfer";
  /** Chamfer type (Equal distance is Distance-Distance with "Equal distance" on). */
  chamferType?: "equal" | "distanceDistance" | "angleDistance";
  distance: number;
  distance2?: number;
  /** Angle-Distance: angle in radians. */
  angle?: number;
  /** Measure the first distance on the other face of each edge. */
  flip?: boolean;
  edges: EdgeSig[];
}

export interface RefPlaneFeature extends FeatureBase {
  type: "refplane";
  /** Legacy single reference (offset planes); `refs` takes precedence when set. */
  base: PlaneRef;
  offset: number;
  flip: boolean;
  /** Up to three references; the plane type follows from them (see kernel refPlaneFrame). */
  refs?: GeomRef[];
  /** At Angle: angle in radians. */
  angle?: number;
}

export interface RefAxisFeature extends FeatureBase {
  type: "refaxis";
  refs: GeomRef[];
}

export interface RefPointFeature extends FeatureBase {
  type: "refpoint";
  refs: GeomRef[];
  /** Coordinates when no reference is picked. */
  xyz?: Vec3;
}

export interface CoordSysFeature extends FeatureBase {
  type: "coordsys";
  /** Origin, X axis, Y axis references (origin defaults to the part origin). */
  origin: GeomRef | null;
  xAxis: GeomRef | null;
  yAxis: GeomRef | null;
}

/**
 * A model reference picked in the graphics area or the tree: a direction (linear
 * edge, planar face normal, plane normal), an axis (linear or circular edge,
 * cylindrical face, sketch line) or a plane (planar face, plane).
 */
export type GeomRef =
  | { kind: "edge"; sig: EdgeSig }
  | { kind: "face"; sig: FaceSig }
  | { kind: "plane"; plane: PlaneRef }
  | { kind: "sketchLine"; sketchId: Id; entity: Id }
  /**
   * A model vertex or the origin (position captured when picked). A model vertex carries
   * its persistent name; the origin never has one.
   */
  | { kind: "vertex"; p: Vec3; name?: VertexName }
  /** Reference geometry features. */
  | { kind: "axis"; featureId: Id }
  | { kind: "point"; featureId: Id };

export interface PatternDirection {
  ref: GeomRef | null;
  reverse: boolean;
  spacing: number;
  /** Number of instances including the seed. */
  count: number;
}

export interface LinearPatternFeature extends FeatureBase {
  type: "linearPattern";
  /** Features to pattern (their added/removed bodies are repeated). */
  seeds: Id[];
  dir1: PatternDirection;
  dir2?: PatternDirection;
  /** Instances to Skip: [i, j], 1-based along Direction 1 and 2 (the seed is [1, 1]). */
  skip?: [number, number][];
}

export interface CircularPatternFeature extends FeatureBase {
  type: "circularPattern";
  seeds: Id[];
  axis: GeomRef | null;
  reverse: boolean;
  /** Total angle (equal spacing) or the angle between instances. */
  angle: number;
  count: number;
  equalSpacing: boolean;
  /** Instances to Skip: 1-based instance numbers (the seed is 1). */
  skip?: number[];
}

/** A pattern instance as the preview shows it (Instances to Skip picks these). */
export interface PatternInstance {
  /** "i,j" for linear patterns, "i" for circular ones (1-based; the seed is not listed). */
  key: string;
  p: Vec3;
  skipped: boolean;
}

export interface MirrorFeature extends FeatureBase {
  type: "mirror";
  seeds: Id[];
  plane: GeomRef | null;
  /** Mirror the whole body instead of features ("Bodies to Mirror"). */
  body?: boolean;
}

export interface SweepFeature extends FeatureBase {
  type: "sweep";
  cut: boolean;
  /** Sketch Profile (a closed sketch) or Circular Profile (diameter along the path). */
  profileType: "sketch" | "circular";
  profile: Id;
  diameter: number;
  path: Id;
  orientation: "followPath" | "keepNormal";
  merge: boolean;
}

export interface LoftFeature extends FeatureBase {
  type: "loft";
  cut: boolean;
  /** Profile sketches in loft order. */
  profiles: Id[];
  merge: boolean;
}

export interface HoleFeature extends FeatureBase {
  type: "hole";
  holeType: "cbore" | "csk" | "hole" | "tap";
  standard: "ISO";
  size: string;
  fit: "close" | "normal" | "loose";
  /** Show custom sizing: overrides of the standard values (mm, radians). */
  custom?: { dia?: number; cboreDia?: number; cboreDepth?: number; cskDia?: number; cskAngle?: number };
  end: { type: "blind" | "throughAll" | "upToNext"; depth: number };
  /** Placement face and hole centres (face sketch coordinates) while the PropertyManager is open. */
  face?: FaceSig;
  positions?: Vec2[];
  /** Positions sketch (sketch points), created on OK and absorbed under the hole. */
  sketchId?: Id;
}

export interface DraftFeature extends FeatureBase {
  type: "draft";
  /** Draft angle (radians). */
  angle: number;
  /** Neutral plane (planar face or plane); its normal is the direction of pull. */
  neutral: GeomRef | null;
  reverse: boolean;
  faces: FaceSig[];
}

export interface RibFeature extends FeatureBase {
  type: "rib";
  /** Open sketch (lines) drawn on a plane through the body. */
  sketchId: Id;
  thickness: number;
  /** Both sides (mid-plane), or first / second side of the sketch. */
  side: "both" | "side1" | "side2";
  /** Parallel to sketch (grows in the sketch plane) or normal to sketch. */
  direction: "parallel" | "normal";
  /** Material side; by default the side where the rib meets the body. */
  flip: boolean;
}

/** A body imported from a neutral file (STEP), stored in the document. */
export interface ImportedFeature extends FeatureBase {
  type: "imported";
  format: "step";
  data: string;
}

export interface ShellFeature extends FeatureBase {
  type: "shell";
  thickness: number;
  /** Faces removed (open faces). */
  faces: FaceSig[];
  outward: boolean;
}

export type Feature =
  | SketchFeature
  | ExtrudeFeature
  | RevolveFeature
  | FilletFeature
  | ChamferFeature
  | RefPlaneFeature
  | LinearPatternFeature
  | CircularPatternFeature
  | MirrorFeature
  | ShellFeature
  | ImportedFeature
  | RibFeature
  | DraftFeature
  | HoleFeature
  | SweepFeature
  | LoftFeature
  | RefAxisFeature
  | RefPointFeature
  | CoordSysFeature;

export type FeatureType = Feature["type"];

/**
 * Schema version this app writes. Version 1 had no persistent names; a version 1 document
 * is a version 2 document whose references have none (`migrateDocument`).
 */
export const DOC_VERSION = 2;

export interface PartDocument {
  format: "opencad-part";
  /** `DOC_VERSION`, or 1 for a document not yet saved by a version 2 app. */
  version: 1 | 2;
  name: string;
  units: UnitSystem;
  features: Feature[];
  /** Index into features: everything at or after this index is rolled back. null = at end. */
  rollback: number | null;
  material?: string;
  /** View-only display state saved with the document (plane visibility, plane sizes). */
  display?: DocDisplay;
  /** Document Properties (Tools > Options > Document Properties), keyed by option id. */
  props?: Record<string, unknown>;
  /** Tools > Equations: global variables and equations driving dimensions. */
  globals?: { name: string; expr: string }[];
  equations?: { target: string; expr: string }[];
}

export interface DocDisplay {
  /** Standard planes the user has shown (they are hidden by default). */
  shownPlanes?: string[];
  /** Sketches the user has hidden. */
  hiddenSketches?: string[];
  /** Reference planes the user has hidden (they are shown by default). */
  hiddenPlanes?: string[];
  /** Manually resized plane extents in plane coordinates: [u0, u1, v0, v1]. */
  planeExtents?: Record<string, [number, number, number, number]>;
  /** Solid body hidden (Hide/Show Body), shown transparent, or given an appearance colour. */
  bodyHidden?: boolean;
  transparent?: boolean;
  color?: string;
}

export type UnitSystem = "MMGS" | "CGS" | "MKS" | "IPS";

// ---------------------------------------------------------------- kernel output

export interface FaceInfo {
  id: number;
  sig: FaceSig;
  /** Planar faces carry a frame so they can be sketched on / used for Normal To. */
  planar: boolean;
  /** Feature that created this face (for Edit Feature from the graphics area). */
  owner?: Id;
}
export interface EdgeInfo {
  id: number;
  sig: EdgeSig;
  /** Seam edges of periodic faces are hidden, as in Parasolid-based SOLIDWORKS. */
  seam: boolean;
  /** Edge is a straight line (usable as revolve axis / direction reference). */
  linear: boolean;
}

export interface BodyMesh {
  positions: Float32Array;
  normals: Float32Array;
  indices: Uint32Array;
  /** Triangle ranges per face id. */
  faceGroups: { start: number; count: number; faceId: number }[];
  edgePositions: Float32Array;
  edgeGroups: { start: number; count: number; edgeId: number }[];
  faces: FaceInfo[];
  edges: EdgeInfo[];
  vertices: Vec3[];
  bbox: [Vec3, Vec3] | null;
}

export interface FeatureStatus {
  id: Id;
  error?: string;
  warning?: string;
}

export interface RebuildResult {
  mesh: BodyMesh | null;
  statuses: FeatureStatus[];
  /** Resolved sketch frames (needed by the UI to draw and edit sketches). */
  frames: Record<Id, Frame>;
  /** Resolved reference plane frames + sizes. */
  planes: Record<Id, { frame: Frame; size: number }>;
  /** Reference axes (point + unit direction), points and coordinate systems. */
  axes?: Record<Id, { p: Vec3; d: Vec3 }>;
  points?: Record<Id, Vec3>;
  csys?: Record<Id, Frame>;
  /** Sketches with model references, re-solved against the model as rebuilt. */
  sketches?: Record<Id, SketchData>;
  timeMs: number;
  /**
   * Feature preview (extrude/revolve PropertyManager open): `mesh` is the part before the
   * feature and `toolMesh` the body the feature adds (boss) or removes (cut), drawn as
   * SOLIDWORKS draws its yellow transparent preview.
   */
  toolMesh?: BodyMesh | null;
  /** Linear/circular pattern preview: where each instance goes, for Instances to Skip. */
  instances?: PatternInstance[];
}

/** A relation before it has been given an id. */
export type NewRelation = Relation extends infer R ? (R extends unknown ? Omit<R, "id"> : never) : never;
