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
}

export type SketchEntity =
  | { id: Id; type: "line"; p1: Id; p2: Id; construction?: boolean }
  | { id: Id; type: "circle"; c: Id; r: number; construction?: boolean }
  /** Arc runs counter-clockwise (in sketch coordinates) from p1 to p2 about c. */
  | { id: Id; type: "arc"; c: Id; p1: Id; p2: Id; construction?: boolean }
  | { id: Id; type: "point"; p: Id };

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
  | { id: Id; type: "fix"; target: Id };

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
}

// ---------------------------------------------------------------- references

/** Geometric signature of a face, used to re-find it after a rebuild. */
export interface FaceSig {
  surface: string; // "PLANE" | "CYLINDRE" | ...
  center: Vec3;
  normal: Vec3;
  area: number;
}
export interface EdgeSig {
  curve: string;
  mid: Vec3;
  a: Vec3;
  b: Vec3;
  length: number;
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
  /** A model vertex or the origin (position captured when picked). */
  | { kind: "vertex"; p: Vec3 }
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
}

export interface MirrorFeature extends FeatureBase {
  type: "mirror";
  seeds: Id[];
  plane: GeomRef | null;
  /** Mirror the whole body instead of features ("Bodies to Mirror"). */
  body?: boolean;
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
  | RefAxisFeature
  | RefPointFeature
  | CoordSysFeature;

export type FeatureType = Feature["type"];

export interface PartDocument {
  format: "opencad-part";
  version: 1;
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
  timeMs: number;
  /**
   * Feature preview (extrude/revolve PropertyManager open): `mesh` is the part before the
   * feature and `toolMesh` the body the feature adds (boss) or removes (cut), drawn as
   * SOLIDWORKS draws its yellow transparent preview.
   */
  toolMesh?: BodyMesh | null;
}

/** A relation before it has been given an id. */
export type NewRelation = Relation extends infer R ? (R extends unknown ? Omit<R, "id"> : never) : never;
