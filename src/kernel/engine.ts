/**
 * Geometry kernel engine: regenerates the feature tree with OpenCascade (B-rep).
 *
 * Runs inside the kernel worker (kernel.worker.ts) in the app and under Node in tests
 * (testing.ts). OCCT must be initialised with replicad's `setOC` before any call; this
 * module never initialises it, so it has no browser- or bundler-specific imports.
 *
 * Results of each feature are memoised by a key of (feature JSON + input key), so
 * editing a late feature only recomputes from that feature onwards.
 */
import {
  makeLine,
  makeThreePointArc,
  makeCircle,
  assembleWire,
  makeFace,
  addHolesInFace,
  getOC,
  Face,
  basicFaceExtrusion,
  revolution,
  measureArea,
  measureLength,
  measureShapeVolumeProperties,
  measureShapeSurfaceProperties,
  makeCompound,
  makePolygon,
  loft,
  genericSweep,
  makeBSplineApproximation,
  importSTEP,
  makeEllipse,
  makeVertex,
  measureDistanceBetween,
  DistanceTool,
  type Wire,
  Plane,
  Vector,
  type Shape3D,
  type Edge,
  type AnyShape,
} from "replicad";
import type {
  BodyMesh,
  EdgeInfo,
  EdgeSig,
  ExtrudeFeature,
  FaceInfo,
  FaceSig,
  Feature,
  EndCondition,
  FeatureStatus,
  Frame,
  GeomRef,
  SketchData,
  PlaneRef,
  RebuildResult,
  RevolveFeature,
  SketchFeature,
  SweepFeature,
  LoftFeature,
  HoleFeature,
  RibFeature,
  PatternInstance,
  FilletFeature,
  ChamferFeature,
  Vec3,
} from "../core/types";
import { STD_FRAMES, frameForPlane, frameToWorld, offsetFrame, v3 } from "../core/math";
import { extractProfiles, regionAtPoint, splineThrough, type Region } from "../sketch/model";
import { offsetEntities, orderChains } from "../sketch/ops";
import { updateRefGeometry } from "../sketch/refs";
import { solveSketch } from "../sketch/solver";
import { holeDims } from "../core/holes";
import { uid } from "../core/ids";
import type { KernelRequest } from "./protocol";

// ------------------------------------------------------------------ memo cache

/** Rounded face signature keys → id of the feature that created the face. */
type Owners = Record<string, string>;

function ownerKeys(sig: FaceSig): [exact: string, plane: string | null] {
  const r = (x: number, d: number) => (Math.abs(x) < 0.5 * Math.pow(10, -d) ? 0 : x).toFixed(d);
  const exact = `${sig.surface}|${sig.center.map((x) => r(x, 3)).join(",")}|${sig.normal.map((x) => r(x, 3)).join(",")}|${r(sig.area, 2)}`;
  const d = sig.center[0] * sig.normal[0] + sig.center[1] * sig.normal[1] + sig.center[2] * sig.normal[2];
  const plane = sig.surface === "PLANE" ? `P|${sig.normal.map((x) => r(x, 3)).join(",")}|${r(d, 3)}` : null;
  return [exact, plane];
}

/** Faces that already existed keep their owner; new faces belong to `featureId`. */
function computeOwners(shape: Shape3D | null, prev: Owners, featureId: string): Owners {
  const out: Owners = {};
  if (!shape) return out;
  for (const f of shape.faces) {
    const [exact, plane] = ownerKeys(faceSig(f));
    const owner = prev[exact] ?? (plane ? prev[plane] : undefined) ?? featureId;
    out[exact] = owner;
    if (plane && !(plane in out)) out[plane] = owner;
  }
  return out;
}

/** Body a feature added (boss) or removed (cut): patterns and mirrors repeat these. */
interface FeatureTool {
  shape: Shape3D;
  cut: boolean;
}

/**
 * A fillet or chamfer as applied (edges and faces in world position), so patterns and
 * mirrors can re-apply it to the matching edges of each instance.
 */
interface Dressup {
  f: FilletFeature | ChamferFeature;
  edges: EdgeSig[];
  faces: FaceSig[];
  /** Pattern instance: its edges must exist where the transform puts them (no nearest-match fallback). */
  exact?: boolean;
}

/** One pattern/mirror instance: the transform of shapes, points and directions. */
interface Xf {
  shape: (s: Shape3D) => Shape3D;
  pt: (p: Vec3) => Vec3;
  dir: (v: Vec3) => Vec3;
  /** Instances to Skip: the instance's key ("i,j" / "i") and whether it is skipped. */
  key?: string;
  skipped?: boolean;
}

interface StepState {
  owners: Owners;
  shape: Shape3D | null;
  tools: Record<string, FeatureTool[]>;
  /** Fillets/chamfers each feature applied (patterns: their dress-up instances). */
  dress: Record<string, Dressup[]>;
  frames: Record<string, Frame>;
  planes: Record<string, { frame: Frame; size: number }>;
  axes: Record<string, { p: Vec3; d: Vec3 }>;
  points: Record<string, Vec3>;
  csys: Record<string, Frame>;
  /** Sketches with model references, as resolved against the body at that step. */
  sketches: Record<string, SketchFeature>;
}
const cache = new Map<string, { state: StepState; status: FeatureStatus }>();
/** Imported STEP bodies by content hash (importing is asynchronous, rebuilding is not). */
const imported = new Map<string, Shape3D>();
export async function ensureImports(features: Feature[]) {
  for (const f of features) {
    if (f.type !== "imported") continue;
    const k = hash(f.data);
    if (imported.has(k)) continue;
    try {
      const shape = (await importSTEP(new Blob([f.data]))) as Shape3D;
      imported.set(k, shape);
    } catch {
      /* reported by applyFeature */
    }
  }
}

/** Body made by the last extrude/revolve applied (before it was combined with the part). */
let lastTool: Shape3D | null = null;
/** Instance positions of the last linear/circular pattern applied (for its preview). */
let lastInstances: PatternInstance[] | undefined;

function stepKey(prevKey: string, f: Feature) {
  return prevKey + "|" + hash(JSON.stringify(f));
}
function hash(s: string) {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0).toString(36) + s.length.toString(36);
}

// ------------------------------------------------------------------ rebuild

/** Called after each feature of a rebuild with the body as it stands after that feature. */
export type StepObserver = (f: Feature, shape: Shape3D | null, status: FeatureStatus) => void;

export function rebuild(features: Feature[], preview?: Feature, quality = 3, onStep?: StepObserver): RebuildResult {
  const t0 = performance.now();
  let state: StepState = { shape: null, frames: {}, planes: {}, owners: {}, tools: {}, dress: {}, axes: {}, points: {}, csys: {}, sketches: {} };
  let key = "root";
  const statuses: FeatureStatus[] = [];
  const all = preview ? [...features, preview] : features;
  const used = new Set<string>();
  let before: StepState | null = null;
  let tool: Shape3D | null = null;
  let instances: PatternInstance[] | undefined;
  for (const f of all) {
    if (f.suppressed) {
      statuses.push({ id: f.id });
      onStep?.(f, state.shape, { id: f.id });
      continue;
    }
    if (f === preview) {
      before = state;
      lastTool = null;
      lastInstances = undefined;
    }
    key = stepKey(key, f);
    used.add(key);
    let hit = cache.get(key);
    if (!hit) {
      const next: StepState = { shape: state.shape, frames: { ...state.frames }, planes: { ...state.planes }, owners: state.owners, tools: { ...state.tools }, dress: { ...state.dress }, axes: { ...state.axes }, points: { ...state.points }, csys: { ...state.csys }, sketches: { ...state.sketches } };
      const status: FeatureStatus = { id: f.id };
      stepWarning = null;
      try {
        applyFeature(f, next, features);
        if (stepWarning) status.warning = stepWarning;
        // Committed features only: previews are throwaway and must stay fast while dragging.
        if (next.shape && next.shape !== state.shape && f !== preview) checkResult(next.shape);
        if (next.shape !== state.shape) next.owners = computeOwners(next.shape, state.owners, f.id);
      } catch (e) {
        status.error = errorText(e);
        next.shape = state.shape;
      }
      hit = { state: next, status };
      if (f !== preview) cache.set(key, hit);
    }
    state = hit.state;
    statuses.push(hit.status);
    onStep?.(f, state.shape, hit.status);
    if (f === preview && !hit.status.error) tool = lastTool;
    if (f === preview) instances = lastInstances;
  }
  // Evict stale cache entries (keep memory bounded).
  if (cache.size > 200) for (const k of cache.keys()) if (!used.has(k)) cache.delete(k);
  // Extrude/revolve previews: the part as it was, plus the added (boss) or removed (cut) body.
  let toolShape: Shape3D | null = null;
  const previewCut = preview && "cut" in preview ? preview.cut : preview && (preview.type === "linearPattern" || preview.type === "circularPattern" || preview.type === "mirror") ? !!state.tools[preview.id]?.every((t) => t.cut) : false;
  if (preview && before && tool && ["extrude", "revolve", "sweep", "loft", "hole", "rib", "linearPattern", "circularPattern", "mirror"].includes(preview.type)) {
    toolShape = tool;
    if (previewCut && before.shape) {
      try {
        toolShape = before.shape.intersect(tool);
      } catch {
        /* keep the full tool */
      }
    }
  }
  const base = toolShape && before ? before : state;
  const mesh = base.shape ? meshShape(base.shape, quality, base.owners) : null;
  let toolMesh: BodyMesh | null = null;
  if (toolShape) {
    try {
      toolMesh = toolShape.faces.length ? meshShape(toolShape, quality, {}) : null;
    } catch {
      toolMesh = null;
    }
  }
  const sketches: Record<string, SketchData> = {};
  for (const [id, f] of Object.entries(state.sketches)) sketches[id] = f.sketch;
  return { mesh, toolMesh, statuses, frames: state.frames, planes: state.planes, axes: state.axes, points: state.points, csys: state.csys, sketches, instances, timeMs: performance.now() - t0 };
}

export function errorText(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "number" || (typeof WebAssembly !== "undefined" && "Exception" in WebAssembly && e instanceof (WebAssembly as unknown as { Exception: new () => object }).Exception))
    return "The geometry could not be built with these settings";
  return String(e);
}

function resolvePlane(ref: PlaneRef, st: StepState): Frame {
  if (ref.kind === "std") return STD_FRAMES[ref.plane];
  if (ref.kind === "refplane") {
    const p = st.planes[ref.featureId];
    if (!p) throw new Error("Reference plane is missing");
    return p.frame;
  }
  // In-context (G1d, made by the native app): another component's face as last updated.
  if (ref.kind === "external") return frameForPlane(ref.sig.center, ref.sig.normal);
  if (!st.shape) throw new Error("Sketch face no longer exists");
  const face = findFace(st.shape, ref.sig);
  if (!face) throw new Error("Sketch plane face could not be found");
  const n = face.normalAt(face.center);
  return frameForPlane(face.center.toTuple() as Vec3, n.toTuple() as Vec3);
}

function applyFeature(f: Feature, st: StepState, features: Feature[]) {
  switch (f.type) {
    case "sketch":
      st.frames[f.id] = resolvePlane(f.plane, st);
      if (f.sketch.refs && Object.keys(f.sketch.refs).length) resolveSketchRefs(f, st);
      return;
    case "refplane": {
      if (!f.refs?.length) {
        const base = resolvePlane(f.base, st);
        st.planes[f.id] = { frame: offsetFrame(base, f.offset, f.flip), size: 60 };
        return;
      }
      st.planes[f.id] = { frame: refPlaneFrame(f, st, features), size: 60 };
      return;
    }
    case "refaxis":
      st.axes[f.id] = refAxisDef(f.refs, st, features);
      return;
    case "refpoint":
      st.points[f.id] = refPointDef(f, st, features);
      return;
    case "coordsys": {
      const o = f.origin ? refPointOf(f.origin, st, features) : ([0, 0, 0] as Vec3);
      const x = f.xAxis ? refDirection(f.xAxis, st, features) : ([1, 0, 0] as Vec3);
      let y = f.yAxis ? refDirection(f.yAxis, st, features) : v3.norm(v3.cross([0, 0, 1], x));
      if (v3.len(y) < 1e-9) y = [0, 1, 0];
      // Make Y perpendicular to X; Z completes a right-handed frame.
      y = v3.norm(v3.sub(y, v3.scale(x, v3.dot(y, x))));
      st.csys[f.id] = { origin: o, x: v3.norm(x), y, normal: v3.cross(v3.norm(x), y) };
      return;
    }
    case "extrude":
      return extrude(f, st, features);
    case "revolve":
      return revolve(f, st, features);
    case "fillet":
    case "chamfer": {
      st.dress[f.id] = [applyDressup({ f, edges: f.edges, faces: f.type === "fillet" ? f.faces : [] }, st)];
      return;
    }
    case "hole":
      return hole(f, st, features);
    case "imported": {
      const s = imported.get(hash(f.data));
      if (!s) throw new Error("The imported file could not be read");
      const body = s.clone() as Shape3D;
      st.shape = st.shape ? st.shape.fuse(body) : body;
      return;
    }
    case "rib":
      return rib(f, st, features);
    case "draft": {
      if (!st.shape) throw new Error("There is no body to draft");
      const pl = refPlaneOf(f.neutral, st, features);
      const faces = f.faces.map((sig) => findFace(st.shape!, sig)).filter((x): x is Face => !!x);
      if (!faces.length) throw new Error("Select the faces to draft");
      const n = f.reverse ? v3.neg(pl.n) : pl.n;
      const fr = frameForPlane(pl.o, n);
      const plane = new Plane(pl.o as never, fr.x as never, n as never);
      st.shape = st.shape.draft((f.angle * 180) / Math.PI, (finder) => finder.inList(faces), plane) as Shape3D;
      return;
    }
    case "sweep":
      return sweep(f, st, features);
    case "loft":
      return loftFeature(f, st, features);
    case "linearPattern": {
      const xfs: Xf[] = [];
      const d1 = v3.scale(refDirection(f.dir1.ref, st, features), f.dir1.reverse ? -1 : 1);
      const d2 = f.dir2?.ref ? v3.scale(refDirection(f.dir2.ref, st, features), f.dir2.reverse ? -1 : 1) : null;
      const skip = new Set((f.skip ?? []).map(([i, j]) => `${i},${j}`));
      for (let i = 0; i < Math.max(1, f.dir1.count); i++) {
        for (let j = 0; j < (d2 ? Math.max(1, f.dir2!.count) : 1); j++) {
          if (i === 0 && j === 0) continue;
          let v = v3.scale(d1, f.dir1.spacing * i);
          if (d2) v = v3.add(v, v3.scale(d2, f.dir2!.spacing * j));
          const key = `${i + 1},${j + 1}`;
          xfs.push({ shape: (s) => s.translate(v) as Shape3D, pt: (p) => v3.add(p, v), dir: (d) => d, key, skipped: skip.has(key) });
        }
      }
      return repeatSeeds(f.id, f.seeds, st, features, xfs);
    }
    case "circularPattern": {
      const ax = refAxis(f.axis, st, features);
      const n = Math.max(1, f.count);
      const deg = (f.angle * 180) / Math.PI;
      const full = Math.abs(Math.abs(deg) - 360) < 1e-6;
      const step = f.equalSpacing ? deg / (full ? n : Math.max(1, n - 1)) : deg;
      const s = f.reverse ? -1 : 1;
      const xfs: Xf[] = [];
      for (let i = 1; i < n; i++) {
        const a = s * step * i;
        const rad = (a * Math.PI) / 180;
        const k = v3.norm(ax.d);
        const key = String(i + 1);
        xfs.push({ shape: (x) => x.rotate(a, ax.p, ax.d) as Shape3D, pt: (p) => v3.add(ax.p, rotateVec(v3.sub(p, ax.p), k, rad)), dir: (d) => rotateVec(d, k, rad), key, skipped: !!f.skip?.includes(i + 1) });
      }
      return repeatSeeds(f.id, f.seeds, st, features, xfs);
    }
    case "mirror": {
      const pl = refPlaneOf(f.plane, st, features);
      if (f.body || !f.seeds.length) {
        if (!st.shape) throw new Error("There is no body to mirror");
        const m = st.shape.clone().mirror(pl.n, pl.o) as Shape3D;
        lastTool = m;
        st.shape = st.shape.fuse(m);
        st.tools[f.id] = [{ shape: m, cut: false }];
        return;
      }
      const n = v3.norm(pl.n);
      const refl = (v: Vec3) => v3.sub(v, v3.scale(n, 2 * v3.dot(v, n)));
      return repeatSeeds(f.id, f.seeds, st, features, [{ shape: (x) => x.clone().mirror(pl.n, pl.o) as Shape3D, pt: (p) => v3.add(pl.o, refl(v3.sub(p, pl.o))), dir: refl }]);
    }
    case "shell": {
      if (!st.shape) throw new Error("There is no body to shell");
      const faces = f.faces.map((sig) => findFace(st.shape!, sig)).filter((x): x is Face => !!x);
      const t = f.outward ? -f.thickness : f.thickness;
      if (!faces.length) throw new Error("Select at least one face to remove");
      st.shape = st.shape.shell(t, (finder) => finder.inList(faces)) as Shape3D;
      return;
    }
  }
}

/**
 * Pattern/mirror instances of `seeds`, in FeatureManager order (as SOLIDWORKS does,
 * whatever order they were picked in). Features that add or remove material repeat
 * their body; fillets and chamfers are re-applied to the matching edges of each instance,
 * so they pattern together with their parent feature. An instance whose edges are missing
 * is skipped with a warning.
 */
function repeatSeeds(fid: string, seeds: string[], st: StepState, features: Feature[], all: Xf[]) {
  if (!seeds.length) throw new Error("Select the features to pattern");
  const order = (id: string) => features.findIndex((x) => x.id === id);
  const sorted = [...seeds].sort((a, b) => order(a) - order(b));
  // Instance markers sit where each instance puts the first seed's centre.
  const anchor = seedAnchor(sorted[0], st);
  if (anchor && all.some((x) => x.key)) lastInstances = all.map((x) => ({ key: x.key!, p: x.pt(anchor), skipped: !!x.skipped }));
  const xfs = all.filter((x) => !x.skipped);
  if (!xfs.length) {
    st.tools[fid] = [];
    st.dress[fid] = [];
    return;
  }
  const out: FeatureTool[] = [];
  const dress: Dressup[] = [];
  let failed = 0;
  for (const sid of sorted) {
    const tools = st.tools[sid];
    const dressups = st.dress[sid];
    if (!tools?.length && !dressups?.length) {
      const name = features.find((x) => x.id === sid)?.name ?? "A feature";
      throw new Error(`${name} cannot be patterned. Select features that add or remove material, fillets or chamfers.`);
    }
    for (const t of tools ?? []) {
      let acc: Shape3D | null = null;
      for (const x of xfs) {
        const c = x.shape(t.shape.clone() as Shape3D);
        acc = acc ? (acc.fuse(c) as Shape3D) : c;
      }
      if (!acc) continue;
      combine(st, acc, t.cut, true);
      out.push({ shape: acc, cut: t.cut });
    }
    for (const d of dressups ?? []) {
      for (const x of xfs) {
        const inst: Dressup = {
          f: d.f,
          edges: d.edges.map((e) => ({ ...e, a: x.pt(e.a), b: x.pt(e.b), mid: x.pt(e.mid) })),
          faces: d.faces.map((s) => ({ ...s, center: x.pt(s.center), normal: x.dir(s.normal) })),
          exact: true,
        };
        try {
          dress.push(applyDressup(inst, st));
        } catch {
          failed++;
        }
      }
    }
  }
  if (failed) stepWarning = failed === 1 ? "One fillet/chamfer instance could not be created (pattern its parent feature too)" : `${failed} fillet/chamfer instances could not be created (pattern their parent features too)`;
  st.tools[fid] = out;
  st.dress[fid] = dress;
  // With fillet/chamfer instances the preview shows the whole result, so they are seen too.
  lastTool = out.length === 1 && !dress.length ? out[0].shape : null;
}

/** Centre of what a seed feature added or removed (or of its fillet/chamfer edges). */
function seedAnchor(id: string, st: StepState): Vec3 | null {
  const t = st.tools[id]?.[0];
  if (t) {
    const [lo, hi] = t.shape.boundingBox.bounds;
    return [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2];
  }
  const e = st.dress[id]?.[0]?.edges[0];
  return e ? e.mid : null;
}

/**
 * Apply a fillet or chamfer to the edges (and fillet faces) its signatures find. Returns
 * it as applied: the edges actually used, in their position before the fillet, which is
 * what patterns transform.
 */
function applyDressup(d: Dressup, st: StepState): Dressup {
  const f = d.f;
  if (!st.shape) throw new Error(f.type === "fillet" ? "Nothing to fillet" : "Nothing to chamfer");
  // Instances: transformed copies are exact to modelling precision, so allow only a small error.
  const edges = resolveEdges(st.shape, d.edges, d.exact ? 0.05 : undefined);
  if (d.exact && edges.length < d.edges.length) throw new Error("Instance edges not found");
  if (f.type === "fillet") {
    for (const sig of d.faces) {
      const face = findFace(st.shape, sig, d.exact ? 0.5 : undefined);
      if (d.exact && !face) throw new Error("Instance face not found");
      if (face) edges.push(...face.edges);
    }
    if (!edges.length) throw new Error("Fillet edges could not be found");
    const set = new Set(edges.map((e) => e.hashCode));
    const r: number | [number, number] = f.radius2 !== undefined ? [f.radius, f.radius2] : f.radius;
    st.shape = st.shape.fillet((e: Edge) => (set.has(e.hashCode) && edges.some((x) => x.isSame(e)) ? r : null) as number);
    return { f, edges: edges.map(edgeSig), faces: [] };
  }
  if (!edges.length) throw new Error("Chamfer edges could not be found");
  const shape = st.shape;
  const t = f.chamferType ?? "equal";
  st.shape = shape.chamfer(((e: Edge) => {
    if (!edges.some((x) => x.isSame(e))) return null;
    if (t === "equal") return f.distance;
    // The first distance is measured on one of the edge's two faces ("Flip direction" swaps).
    const adj = shape.faces.filter((fc) => fc.edges.some((x) => x.isSame(e)));
    const face = adj[f.flip ? 1 : 0] ?? adj[0];
    const selectedFace = (finder: { when: (fn: (x: { element: Face }) => boolean) => unknown }) => finder.when(({ element }) => element.isSame(face));
    if (t === "distanceDistance") return { distances: [f.distance, f.distance2 ?? f.distance], selectedFace };
    return { distance: f.distance, angle: ((f.angle ?? Math.PI / 4) * 180) / Math.PI, selectedFace };
  }) as never);
  return { f, edges: edges.map(edgeSig), faces: [] };
}


/** A point from a reference: vertex, reference point, arc centre, face centre. */
function refPointOf(ref: GeomRef, st: StepState, features: Feature[]): Vec3 {
  if (ref.kind === "vertex") return ref.p;
  if (ref.kind === "origin") return [0, 0, 0];
  if (ref.kind === "point") {
    const p = st.points[ref.featureId];
    if (!p) throw new Error("The reference point no longer exists");
    return p;
  }
  if (ref.kind === "edge") {
    if (ref.sig.curve === "CIRCLE") return refAxis(ref, st, features).p;
    return ref.sig.mid;
  }
  if (ref.kind === "face") return ref.sig.center;
  if (ref.kind === "plane") return resolvePlane(ref.plane, st).origin;
  return refAxis(ref, st, features).p;
}

const isPlaneRef = (r: GeomRef) => r.kind === "plane" || (r.kind === "face" && r.sig.surface === "PLANE");
const isLineRef = (r: GeomRef) => (r.kind === "edge" && r.sig.curve === "LINE") || r.kind === "axis" || r.kind === "sketchLine";
const isPointRef = (r: GeomRef) => r.kind === "vertex" || r.kind === "origin" || r.kind === "point";

/** Reference plane from its references (the plane type follows from what was picked). */
function refPlaneFrame(f: { refs?: GeomRef[]; offset: number; flip: boolean; angle?: number }, st: StepState, features: Feature[]): Frame {
  const refs = f.refs ?? [];
  const planes = refs.filter(isPlaneRef), lines = refs.filter(isLineRef), pts = refs.filter(isPointRef);
  const curves = refs.filter((r) => r.kind === "edge" && r.sig.curve !== "LINE");
  if (pts.length >= 3) {
    const [a, b, c] = pts.map((r) => refPointOf(r, st, features));
    const n = v3.norm(v3.cross(v3.sub(b, a), v3.sub(c, a)));
    if (v3.len(n) < 1e-9) throw new Error("The three points are on one line");
    return frameForPlane(a, f.flip ? v3.neg(n) : n);
  }
  if (planes.length >= 2) {
    // Mid plane between two (parallel) planes or faces.
    const A = refPlaneOf(planes[0], st, features), B = refPlaneOf(planes[1], st, features);
    const nb = v3.dot(A.n, B.n) < 0 ? v3.neg(B.n) : B.n;
    const n = v3.norm(v3.add(A.n, nb));
    const o = v3.scale(v3.add(A.o, v3.add(B.o, v3.scale(n, v3.dot(v3.sub(A.o, B.o), n)))), 0.5);
    return frameForPlane(o, f.flip ? v3.neg(n) : n);
  }
  if (planes.length === 1 && lines.length >= 1) {
    // At angle: rotate the plane about the line.
    const P = refPlaneOf(planes[0], st, features);
    const L = refAxis(lines[0], st, features);
    const ang = (f.angle ?? 0) * (f.flip ? -1 : 1);
    const n = rotateVec(P.n, L.d, ang);
    return frameForPlane(L.p, n);
  }
  if (planes.length === 1 && pts.length >= 1) {
    const P = refPlaneOf(planes[0], st, features);
    return frameForPlane(refPointOf(pts[0], st, features), f.flip ? v3.neg(P.n) : P.n);
  }
  if ((curves.length || lines.length) && pts.length) {
    // Normal to curve at a point.
    const e = (curves[0] ?? lines[0]) as GeomRef;
    const p = refPointOf(pts[0], st, features);
    let t: Vec3;
    if (e.kind === "edge" && e.sig.curve === "CIRCLE") {
      const ax = refAxis(e, st, features);
      t = v3.norm(v3.cross(ax.d, v3.sub(p, ax.p)));
    } else t = refAxis(e, st, features).d;
    return frameForPlane(p, f.flip ? v3.neg(t) : t);
  }
  if (planes.length === 1) {
    const P = refPlaneOf(planes[0], st, features);
    const fr = frameForPlane(P.o, P.n);
    return offsetFrame(fr, f.offset, f.flip);
  }
  throw new Error("Select references that define a plane (a plane or face, plus a line, point or second plane)");
}

/** Rotate v about unit axis k by angle (Rodrigues). */
function rotateVec(v: Vec3, k: Vec3, a: number): Vec3 {
  const c = Math.cos(a), s = Math.sin(a);
  return v3.add(v3.add(v3.scale(v, c), v3.scale(v3.cross(k, v), s)), v3.scale(k, v3.dot(k, v) * (1 - c)));
}

/** Reference axis: a line/edge, two planes, two points, a cylindrical face, or a point and a plane. */
function refAxisDef(refs: GeomRef[], st: StepState, features: Feature[]): { p: Vec3; d: Vec3 } {
  const planes = refs.filter(isPlaneRef), pts = refs.filter(isPointRef);
  if (planes.length >= 2) {
    const A = refPlaneOf(planes[0], st, features), B = refPlaneOf(planes[1], st, features);
    const d = v3.norm(v3.cross(A.n, B.n));
    if (v3.len(d) < 1e-9) throw new Error("The planes are parallel");
    // A point on both planes: solve in the plane spanned by the normals.
    const n1 = A.n, n2 = B.n, h1 = v3.dot(n1, A.o), h2 = v3.dot(n2, B.o);
    const k = v3.dot(n1, n2), det = 1 - k * k;
    const c1 = (h1 - h2 * k) / det, c2 = (h2 - h1 * k) / det;
    return { p: v3.add(v3.scale(n1, c1), v3.scale(n2, c2)), d };
  }
  if (pts.length >= 2) {
    const a = refPointOf(pts[0], st, features), b = refPointOf(pts[1], st, features);
    return { p: a, d: v3.norm(v3.sub(b, a)) };
  }
  if (planes.length === 1 && pts.length === 1) return { p: refPointOf(pts[0], st, features), d: refPlaneOf(planes[0], st, features).n };
  if (refs[0]) return refAxis(refs[0], st, features);
  throw new Error("Select a line or edge, two planes, two points, or a cylindrical face");
}

function refPointDef(f: { refs: GeomRef[]; xyz?: Vec3 }, st: StepState, features: Feature[]): Vec3 {
  if (!f.refs.length) return f.xyz ?? [0, 0, 0];
  return refPointOf(f.refs[0], st, features);
}

/** Unit direction of a reference (linear edge, plane/face normal, sketch line). */
function refDirection(ref: GeomRef | null, st: StepState, features: Feature[]): Vec3 {
  if (!ref) throw new Error("Select a direction (a linear edge, face, plane or sketch line)");
  if (ref.kind === "axis") return refAxis(ref, st, features).d;
  if (ref.kind === "vertex" || ref.kind === "origin" || ref.kind === "point") throw new Error("A point does not define a direction");
  if (ref.kind === "edge") return v3.norm(v3.sub(ref.sig.b, ref.sig.a));
  if (ref.kind === "face") return v3.norm(ref.sig.normal);
  if (ref.kind === "plane") return resolvePlane(ref.plane, st).normal;
  const ax = refAxis(ref, st, features);
  return ax.d;
}

/** Axis (point + unit direction) of a reference. */
function refAxis(ref: GeomRef | null, st: StepState, features: Feature[]): { p: Vec3; d: Vec3 } {
  if (!ref) throw new Error("Select an axis (a circular or straight edge, a cylindrical face or a sketch line)");
  if (ref.kind === "axis") {
    const a = st.axes[ref.featureId];
    if (!a) throw new Error("The reference axis no longer exists");
    return a;
  }
  if (ref.kind === "vertex" || ref.kind === "origin" || ref.kind === "point") throw new Error("A point does not define an axis");
  if (ref.kind === "sketchLine") {
    const sk = sketchOf(features, ref.sketchId, st);
    const e = sk.sketch.entities[ref.entity];
    const frame = st.frames[sk.id];
    if (!e || e.type !== "line" || !frame) throw new Error("The axis line could not be found");
    const a = frameToWorld(frame, [sk.sketch.points[e.p1].x, sk.sketch.points[e.p1].y]);
    const b = frameToWorld(frame, [sk.sketch.points[e.p2].x, sk.sketch.points[e.p2].y]);
    return { p: a, d: v3.norm(v3.sub(b, a)) };
  }
  if (ref.kind === "plane") {
    const f = resolvePlane(ref.plane, st);
    return { p: f.origin, d: f.normal };
  }
  if (!st.shape) throw new Error("The axis reference no longer exists");
  if (ref.kind === "edge") {
    if (ref.sig.curve === "LINE") return { p: ref.sig.a, d: v3.norm(v3.sub(ref.sig.b, ref.sig.a)) };
    const e = resolveEdges(st.shape, [ref.sig])[0];
    if (!e) throw new Error("The axis edge could not be found");
    return circleAxis(e);
  }
  const face = findFace(st.shape, ref.sig);
  if (!face) throw new Error("The axis face could not be found");
  const circ = face.edges.find((e) => e.geomType === "CIRCLE");
  if (circ) return circleAxis(circ);
  return { p: ref.sig.center, d: v3.norm(ref.sig.normal) };
}

/** Centre and normal of a circular edge from three points on it. */
function circleAxis(e: Edge): { p: Vec3; d: Vec3 } {
  const a = e.pointAt(0).toTuple() as Vec3, b = e.pointAt(1 / 3).toTuple() as Vec3, c = e.pointAt(2 / 3).toTuple() as Vec3;
  const ab = v3.sub(b, a), ac = v3.sub(c, a);
  const n = v3.cross(ab, ac);
  const nn = v3.dot(n, n);
  if (nn < 1e-18) throw new Error("The edge is not circular");
  const t1 = v3.scale(v3.cross(n, ab), v3.dot(ac, ac));
  const t2 = v3.scale(v3.cross(ac, n), v3.dot(ab, ab));
  const center = v3.add(a, v3.scale(v3.add(t1, t2), 1 / (2 * nn)));
  return { p: center, d: v3.norm(n) };
}

/** Plane (origin + normal) of a reference: planar face, standard or reference plane. */
function refPlaneOf(ref: GeomRef | null, st: StepState, features: Feature[]): { o: Vec3; n: Vec3 } {
  void features;
  if (!ref) throw new Error("Select a plane or planar face to mirror about");
  if (ref.kind === "plane") {
    const f = resolvePlane(ref.plane, st);
    return { o: f.origin, n: f.normal };
  }
  if (ref.kind === "face") return { o: ref.sig.center, n: v3.norm(ref.sig.normal) };
  throw new Error("Select a plane or planar face");
}

/** B-spline edge through sampled spline points (matches the sketch curve closely). */
function splineEdge(pts: Vec3[]): Edge {
  return makeBSplineApproximation(pts as never, { tolerance: 1e-5, degMax: 3, degMin: 3 }) as Edge;
}

/** World wire of a sketch's (open or closed) chain, for sweep paths. */
function pathWire(sk: SketchFeature, frame: Frame): Wire {
  const s = sk.sketch;
  const ids = Object.values(s.entities).filter((e) => e.type !== "point" && !(e as { construction?: boolean }).construction && !(e as { ext?: string }).ext).map((e) => e.id);
  const chains = orderChains(s, ids);
  if (chains.length !== 1) throw new Error("The sweep path must be a single connected chain of sketch entities");
  const W = (id: string) => frameToWorld(frame, [s.points[id].x, s.points[id].y]);
  const edges: Edge[] = chains[0].segs.map((g) => {
    const e = g.e;
    if (e.type === "line") return makeLine(W(g.from), W(g.to));
    if (e.type === "circle") return makeCircle(e.r, W(e.c), frame.normal);
    if (e.type === "spline") {
      const pts = splineThrough(e.pts.map((id) => [s.points[id].x, s.points[id].y] as [number, number]), 16);
      return splineEdge((g.from === e.pts[0] ? pts : pts.slice().reverse()).map((p) => frameToWorld(frame, p)));
    }
    const c = [s.points[e.c].x, s.points[e.c].y];
    const a = [s.points[g.from].x - c[0], s.points[g.from].y - c[1]];
    const b = [s.points[g.to].x - c[0], s.points[g.to].y - c[1]];
    let t0 = Math.atan2(a[1], a[0]), t1 = Math.atan2(b[1], b[0]);
    // Arcs are stored CCW p1→p2; traversed backwards when g.from is p2.
    if (g.from === e.p1) {
      while (t1 <= t0) t1 += 2 * Math.PI;
    } else while (t1 >= t0) t1 -= 2 * Math.PI;
    const r = Math.hypot(a[0], a[1]);
    const tm = (t0 + t1) / 2;
    return makeThreePointArc(W(g.from), frameToWorld(frame, [c[0] + r * Math.cos(tm), c[1] + r * Math.sin(tm)]), W(g.to));
  });
  return assembleWire(edges);
}

/** Closed loops of a profile sketch as world wires: outer loops and their holes. */
function profileWires(sk: SketchFeature, frame: Frame): { outer: Wire; holes: Wire[] }[] {
  const faces = regionFaces(profileRegions(sk.sketch, { type: "extrude" } as ExtrudeFeature), frame);
  return faces.map((f) => ({ outer: f.clone().outerWire(), holes: f.clone().innerWires() }));
}

function sweep(f: SweepFeature, st: StepState, features: Feature[]) {
  const path = sketchOf(features, f.path, st);
  const pf = st.frames[path.id];
  if (!pf) throw new Error("The path sketch plane is unresolved");
  const spine = pathWire(path, pf);
  const opts = { frenet: f.orientation !== "keepNormal", transitionMode: "right" as const };
  let tool: Shape3D | null = null;
  const run = (w: Wire) => genericSweep(w, spine.clone(), opts) as Shape3D;
  if (f.profileType === "circular") {
    // A circle of the given diameter, normal to the path at its start.
    const e0 = spine.clone().edges[0];
    const p0 = e0.startPoint.toTuple() as Vec3;
    const t = v3.norm(v3.sub(e0.pointAt(0.001).toTuple() as Vec3, p0));
    tool = run(assembleWire([makeCircle(f.diameter / 2, p0, t)]));
  } else {
    const prof = sketchOf(features, f.profile, st);
    const fr = st.frames[prof.id];
    if (!fr) throw new Error("The profile sketch plane is unresolved");
    for (const r of profileWires(prof, fr)) {
      let s = run(r.outer);
      for (const h of r.holes) s = s.cut(run(h));
      tool = tool ? tool.fuse(s) : s;
    }
  }
  if (!tool) throw new Error("The sweep profile is empty");
  lastTool = tool;
  st.tools[f.id] = [{ shape: tool, cut: f.cut }];
  combine(st, tool, f.cut, f.merge);
}

/** Hole Wizard: revolved hole bodies at each position, cut from the part. */
function hole(f: HoleFeature, st: StepState, features: Feature[]) {
  if (!st.shape) throw new Error("There is no body to put holes in");
  let frame: Frame;
  let pts: [number, number][];
  if (f.sketchId) {
    const sk = sketchOf(features, f.sketchId, st);
    frame = st.frames[sk.id];
    if (!frame) throw new Error("The hole positions sketch is unresolved");
    pts = Object.values(sk.sketch.entities)
      .filter((e) => e.type === "point")
      .map((e) => (e.type === "point" ? [sk.sketch.points[e.p].x, sk.sketch.points[e.p].y] : [0, 0]) as [number, number]);
  } else {
    if (!f.face) throw new Error("Select a planar face for the hole positions");
    frame = frameForPlane(f.face.center, f.face.normal);
    pts = f.positions ?? [];
  }
  if (!pts.length) throw new Error("Click on the face to place the holes");
  const d0 = holeDims(f.holeType, f.size, f.fit);
  const c = f.custom ?? {};
  const R = (c.dia ?? d0.dia) / 2;
  const Rcb = (c.cboreDia ?? d0.cboreDia) / 2, cbD = c.cboreDepth ?? d0.cboreDepth;
  const Rcsk = (c.cskDia ?? d0.cskDia) / 2, cskA = c.cskAngle ?? d0.cskAngle;
  const axis = v3.neg(frame.normal);
  const big = bigDepth(st.shape, frame);
  let tool: Shape3D | null = null;
  for (const p of pts) {
    const P = frameToWorld(frame, p);
    let L = f.end.type === "blind" ? f.end.depth : big;
    if (f.end.type === "upToNext") L = holeExitDepth(st.shape, P, axis) ?? big;
    // Half profile in (radius, depth) starting just above the face.
    const z0 = -0.01;
    const prof: [number, number][] = [[0, z0]];
    if (f.holeType === "cbore" && Rcb > R) prof.push([Rcb, z0], [Rcb, cbD], [R, cbD]);
    else if (f.holeType === "csk" && Rcsk > R) {
      const k = Math.tan(cskA / 2);
      prof.push([Rcsk - z0 * k, z0], [R, (Rcsk - R) / k]);
    } else prof.push([R, z0]);
    prof.push([R, L]);
    // Blind holes end in a 118° drill point, as SOLIDWORKS draws them.
    prof.push([0, f.end.type === "blind" ? L + R / Math.tan((59 * Math.PI) / 180) : L]);
    const radial = frame.x;
    const pts3 = prof.map(([r, z]) => v3.add(P, v3.add(v3.scale(radial, r), v3.scale(axis, z))));
    const face = makePolygon(pts3 as never);
    const s = revolution(face, P as never, axis as never, 360) as Shape3D;
    tool = tool ? tool.fuse(s) : s;
  }
  if (!tool) return;
  lastTool = tool;
  st.tools[f.id] = [{ shape: tool, cut: true }];
  combine(st, tool, true, true);
}

/**
 * Rib: a slab along each sketch line, grown (in the sketch plane or normal to it) until it
 * meets the body; the part of the slab between the sketch and the body is added.
 */
function rib(f: RibFeature, st: StepState, features: Feature[]) {
  if (!st.shape) throw new Error("A rib needs an existing body");
  const sk = sketchOf(features, f.sketchId, st);
  const fr = st.frames[sk.id];
  if (!fr) throw new Error("The rib sketch plane is unresolved");
  const lines = Object.values(sk.sketch.entities).filter((e): e is Extract<typeof e, { type: "line" }> => e.type === "line" && !e.construction && !e.ext);
  if (!lines.length) throw new Error("The rib sketch needs at least one line");
  const big = bigDepth(st.shape, fr) * 2;
  const t = f.thickness;
  const [n0, n1] = f.side === "both" ? [-t / 2, t / 2] : f.side === "side1" ? [0, t] : [-t, 0];
  const P2 = (id: string): [number, number] => [sk.sketch.points[id].x, sk.sketch.points[id].y];
  const W = (p: [number, number], h = 0) => v3.add(frameToWorld(fr, p), v3.scale(fr.normal, h));
  const slab = (side: number): Shape3D | null => {
    let tool: Shape3D | null = null;
    for (const l of lines) {
      const a = P2(l.p1), b = P2(l.p2);
      const u: [number, number] = [b[0] - a[0], b[1] - a[1]];
      const len = Math.hypot(u[0], u[1]);
      if (len < 1e-9) continue;
      const m: [number, number] = [(-u[1] / len) * side, (u[0] / len) * side];
      let s: Shape3D;
      if (f.direction === "parallel") {
        // Rectangle from the line towards the body (in the sketch plane), thickened along the normal.
        const q = [a, b, [b[0] + m[0] * big, b[1] + m[1] * big], [a[0] + m[0] * big, a[1] + m[1] * big]] as [number, number][];
        s = basicFaceExtrusion(makePolygon(q.map((p) => W(p, n0)) as never), new Vector(v3.scale(fr.normal, n1 - n0))) as Shape3D;
      } else {
        // Thin strip around the line (in the plane), extruded along the normal towards the body.
        const k = len;
        const w0 = f.side === "both" ? -t / 2 : f.side === "side1" ? 0 : -t, w1 = w0 + t;
        void k;
        const nn: [number, number] = [-u[1] / len, u[0] / len];
        const q = [
          [a[0] + nn[0] * w0, a[1] + nn[1] * w0],
          [b[0] + nn[0] * w0, b[1] + nn[1] * w0],
          [b[0] + nn[0] * w1, b[1] + nn[1] * w1],
          [a[0] + nn[0] * w1, a[1] + nn[1] * w1],
        ] as [number, number][];
        s = basicFaceExtrusion(makePolygon(q.map((p) => W(p)) as never), new Vector(v3.scale(fr.normal, big * side))) as Shape3D;
      }
      tool = tool ? tool.fuse(s) : s;
    }
    if (!tool) return null;
    // Keep only the pieces touching the sketch (between the sketch and the body).
    const rest = tool.cut(st.shape!.clone());
    const touch = rest.solids.filter((so) => lines.some((l) => {
      const e = makeLine(W(P2(l.p1)), W(P2(l.p2)));
      return measureDistanceBetween(so, e) < 1e-4;
    }));
    if (!touch.length) return null;
    let keep = touch[0] as unknown as Shape3D;
    for (let i = 1; i < touch.length; i++) keep = keep.fuse(touch[i] as unknown as Shape3D);
    // A piece reaching the far end never met the body: not a rib on that side.
    const [lo, hi] = keep.boundingBox.bounds;
    if (Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) > big * 0.9) return null;
    return keep;
  };
  const tool = f.flip ? slab(-1) ?? slab(1) : slab(1) ?? slab(-1);
  if (!tool) throw new Error("The rib does not meet the body. Draw the rib sketch on a plane through the part.");
  lastTool = tool;
  st.tools[f.id] = [{ shape: tool, cut: false }];
  combine(st, tool, false, true);
}

/** Depth along `dir` from P to where material ends (Up To Next for holes). */
function holeExitDepth(shape: Shape3D, P: Vec3, dir: Vec3): number | null {
  let best: number | null = null;
  for (const f of shape.faces) {
    if (f.geomType !== "PLANE") continue;
    const s = faceSig(f);
    if (v3.dot(s.normal, dir) < 0.999) continue;
    const d = v3.dot(v3.sub(s.center, P), dir);
    if (d <= 1e-6 || (best !== null && d >= best)) continue;
    const hit = v3.add(P, v3.scale(dir, d));
    const [lo, hi] = f.boundingBox.bounds;
    if (hit.every((x, i) => x >= lo[i] - 1e-4 && x <= hi[i] + 1e-4)) best = d;
  }
  return best;
}

function loftFeature(f: LoftFeature, st: StepState, features: Feature[]) {
  if (f.profiles.length < 2) throw new Error("Select at least two profiles");
  const wires = f.profiles.map((id) => {
    const sk = sketchOf(features, id, st);
    const fr = st.frames[sk.id];
    if (!fr) throw new Error(`${sk.name}: sketch plane unresolved`);
    const loops = profileWires(sk, fr);
    if (loops.length !== 1 || loops[0].holes.length) throw new Error(`${sk.name}: each loft profile must be a single closed contour`);
    return loops[0].outer;
  });
  const tool = loft(wires, { ruled: false }) as Shape3D;
  lastTool = tool;
  st.tools[f.id] = [{ shape: tool, cut: f.cut }];
  combine(st, tool, f.cut, f.merge);
}

/** A sketch as this step sees it: resolved against the model when it has model references. */
function sketchOf(features: Feature[], id: string, st?: StepState): SketchFeature {
  const r = st?.sketches[id];
  if (r) return r;
  const s = features.find((x) => x.id === id);
  if (!s || s.type !== "sketch") throw new Error("Sketch not found");
  return s;
}

/** Warning for the feature being applied (shown in the FeatureManager tree). */
let stepWarning: string | null = null;

/**
 * Sketch relations to model vertices and edges (sketch/refs.ts): re-find each reference on
 * the body as it stands before the sketch, move the sketch geometry standing for it and
 * re-solve, so the sketch follows model changes. References that cannot be found keep their
 * last position and the sketch shows a warning (SOLIDWORKS "dangling" relations).
 */
function resolveSketchRefs(f: SketchFeature, st: StepState) {
  const sk = structuredClone(f.sketch);
  const frame = st.frames[f.id];
  const found = new Set<string>();
  const shape = st.shape;
  const edges = shape ? shape.edges : [];
  const ends: Vec3[] = [];
  for (const e of edges) ends.push(e.startPoint.toTuple() as Vec3, e.endPoint.toTuple() as Vec3);
  for (const r of Object.values(sk.refs ?? {})) {
    if (r.kind === "vertex") {
      // The end shared by most of the vertex's re-found edges (nearest to where it was on
      // ties); without edges, the nearest vertex.
      const own = shape && r.edges?.length ? resolveEdges(shape, r.edges) : [];
      const cand: Vec3[] = [];
      for (const e of own) cand.push(e.startPoint.toTuple() as Vec3, e.endPoint.toTuple() as Vec3);
      let best: Vec3 | null = null, bScore = -Infinity;
      for (const p of cand.length ? cand : ends) {
        const shared = cand.filter((q) => v3.dist(p, q) < 1e-6).length;
        const score = shared * 1e9 - v3.dist(p, r.p);
        if (score > bScore) [best, bScore] = [p, score];
      }
      if (best) {
        sk.refs![r.id] = { ...r, p: best };
        found.add(r.id);
      }
    } else if (shape) {
      const [e] = resolveEdges(shape, [r.sig]);
      if (e) {
        sk.refs![r.id] = { ...r, sig: edgeSig(e) };
        found.add(r.id);
      }
    }
  }
  const bad = updateRefGeometry(sk, frame, found);
  const missing = Object.keys(sk.refs ?? {}).length - found.size + bad.length;
  solveSketch(sk);
  st.sketches[f.id] = { ...f, sketch: sk };
  if (missing) stepWarning = missing === 1 ? "A model reference of this sketch could not be found" : `${missing} model references of this sketch could not be found`;
}

function regionFaces(regions: Region[], frame: Frame): Face[] {
  const W = (p: [number, number]) => frameToWorld(frame, p);
  const wireOf = (loop: Region["outer"]) => {
    const edges: Edge[] = [];
    for (const s of loop.segs) {
      if (s.type === "line") edges.push(makeLine(W(s.a), W(s.b)));
      else if (s.type === "arc") edges.push(makeThreePointArc(W(s.a), W(s.mid), W(s.b)));
      else if (s.type === "spline") edges.push(splineEdge(s.pts.map(W)));
      else if (s.type === "ellipse") {
        // OCCT needs the major radius first: swap axes when the second is longer.
        const major = s.ra >= s.rb;
        const ax = major ? s.u : ([-s.u[1], s.u[0]] as [number, number]);
        const xDir = v3.sub(W([s.c[0] + ax[0], s.c[1] + ax[1]]), W(s.c));
        edges.push(makeEllipse(Math.max(s.ra, s.rb), Math.min(s.ra, s.rb), W(s.c) as never, frame.normal as never, xDir as never));
      }
      else edges.push(makeCircle(s.r, W([s.c[0], s.c[1]]), frame.normal));
    }
    return assembleWire(edges);
  };
  return regions.map((r) => {
    const outer = makeFace(wireOf(r.outer));
    return r.holes.length ? addHolesInFace(outer, r.holes.map(wireOf)) : outer;
  });
}

/** Face oriented so its normal agrees with `dir` (prisms of reversed faces come out inside-out). */
function orientFace(face: Face, dir: Vec3): Face {
  const n = face.normalAt(face.center).toTuple() as Vec3;
  if (v3.dot(n, dir) >= 0) return face;
  return new Face(face.wrapped.Reversed() as never);
}

function prism(face: Face, vec: Vec3): Shape3D {
  return basicFaceExtrusion(orientFace(face, vec), new Vector(vec));
}

/** Regions of a sketch to extrude: Selected Contours, Thin Feature walls, or every closed region. */
function profileRegions(sk: SketchData, f: ExtrudeFeature): Region[] {
  if (f.thin) return thinRegions(sk, f.thin);
  const prof = extractProfiles(sk);
  if (!prof.regions.length) throw new Error(prof.openEnds ? "The sketch contains open contours. Use Thin Feature, or close the sketch." : "The sketch has no closed contours");
  if (!f.contours?.length) return prof.regions;
  // Each picked point selects the smallest closed area around it (with its holes).
  const picked: Region[] = [];
  for (const p of f.contours) {
    const r = regionAtPoint(prof.faces, p);
    if (r && !picked.some((x) => x.outer === r.outer)) picked.push(r);
  }
  if (!picked.length) throw new Error("The selected contours no longer exist");
  return picked;
}

/** Thin Feature: walls of the given thickness along the sketch (open or closed). */
function thinRegions(src: SketchData, o: NonNullable<ExtrudeFeature["thin"]>): Region[] {
  const sk = structuredClone(src);
  const ids = Object.values(sk.entities).filter((e) => e.type !== "point" && !(e as { construction?: boolean }).construction && !(e as { ext?: string }).ext).map((e) => e.id);
  if (!ids.length) throw new Error("The sketch is empty");
  const t1 = o.thickness, t2 = o.thickness2 ?? o.thickness;
  const rev = !!o.reverse;
  const run = (d: number, reverse: boolean) => {
    const r = offsetEntities(sk, ids, { distance: d, reverse, addDimensions: false, selectChain: true });
    if (r.error) throw new Error(r.error);
    return r.created;
  };
  let a: string[], b: string[];
  if (o.type === "midPlane") {
    a = run(t1 / 2, rev);
    b = run(t1 / 2, !rev);
  } else if (o.type === "twoDirection") {
    a = run(t1, rev);
    b = run(t2, !rev);
  } else {
    a = run(t1, rev);
    b = ids;
  }
  // The walls are bounded by the two offset copies (or the original and one copy).
  const keep = new Set([...a, ...b]);
  for (const e of Object.values(sk.entities)) if (!keep.has(e.id)) delete sk.entities[e.id];
  // Open chains: close each end of the wall with a straight line.
  const ends = (list: string[]) => {
    const count = new Map<string, number>();
    for (const id of list) {
      const e = sk.entities[id];
      if (e?.type === "line" || e?.type === "arc") for (const p of [e.p1, e.p2]) count.set(p, (count.get(p) ?? 0) + 1);
    }
    return [...count].filter(([, c]) => c === 1).map(([p]) => p);
  };
  const ea = ends(a), eb = ends(b);
  const P2 = (id: string): [number, number] => [sk.points[id].x, sk.points[id].y];
  for (const p of ea) {
    let best = "", bd = Infinity;
    for (const q of eb) {
      const d = Math.hypot(P2(p)[0] - P2(q)[0], P2(p)[1] - P2(q)[1]);
      if (d < bd) (bd = d), (best = q);
    }
    if (best) {
      const id = uid("cap");
      sk.entities[id] = { id, type: "line", p1: p, p2: best };
    }
  }
  const prof = extractProfiles(sk);
  if (!prof.regions.length) throw new Error("The thin feature could not be built from this sketch");
  return prof.regions;
}

interface ExtrudeCtx {
  st: StepState;
  features: Feature[];
  frame: Frame;
  /** Start plane origin (after From: Offset/Surface). */
  start: Vec3;
  big: number;
  cut: boolean;
}

/** Depth along `dir` for an end condition, or a clipping plane when it is not parallel. */
function endDepth(ec: EndCondition, dir: Vec3, face: Face, c: ExtrudeCtx): { depth: number; clip?: { o: Vec3; n: Vec3 } } {
  switch (ec.type) {
    case "blind":
    case "midPlane":
      return { depth: ec.depth };
    case "throughAll":
    case "throughAllBoth":
    case "upToBody":
      return { depth: c.big };
    case "upToVertex": {
      if (!ec.vertex) throw new Error("Select a vertex for Up To Vertex");
      return { depth: v3.dot(v3.sub(ec.vertex, c.start), dir) };
    }
    case "upToSurface":
    case "offsetFromSurface": {
      const pl = refPlaneOf(ec.ref ?? null, c.st, c.features);
      const off = ec.type === "offsetFromSurface" ? ec.depth * (ec.reverseOffset ? -1 : 1) : 0;
      const denom = v3.dot(dir, pl.n);
      if (Math.abs(denom) < 1e-6) throw new Error("The surface is parallel to the extrude direction");
      const d = v3.dot(v3.sub(pl.o, c.start), pl.n) / denom - off;
      if (Math.abs(Math.abs(denom) - 1) < 1e-9) return { depth: d };
      // Inclined plane: extrude past it and cut away what lies beyond.
      const o = v3.sub(pl.o, v3.scale(dir, off));
      return { depth: c.big, clip: { o, n: v3.scale(pl.n, Math.sign(denom)) } };
    }
    case "upToNext":
      return { depth: nextFaceDepth(face, dir, c) };
  }
}

/** Up To Next: the nearest face that covers the profile, parallel to the sketch. */
function nextFaceDepth(face: Face, dir: Vec3, c: ExtrudeCtx): number {
  if (!c.st.shape) throw new Error("Up To Next needs an existing body");
  const center = face.center.toTuple() as Vec3;
  let best = Infinity;
  for (const f of c.st.shape.faces) {
    if (f.geomType !== "PLANE") continue;
    const s = faceSig(f);
    const k = v3.dot(s.normal, dir);
    // Bosses grow until they meet a face looking back at them; cuts until material ends.
    if (c.cut ? k < 0.999 : k > -0.999) continue;
    const d = v3.dot(v3.sub(s.center, c.start), dir);
    if (d <= 1e-6 || d >= best) continue;
    const hit = v3.add(center, v3.scale(dir, d));
    const [lo, hi] = f.boundingBox.bounds;
    const tol = 1e-4;
    if (hit.every((x, i) => x >= lo[i] - tol && x <= hi[i] + tol)) best = d;
  }
  if (!Number.isFinite(best)) throw new Error("Up To Next: no face found in the extrude direction");
  return best;
}

/** Big oriented box on the +n side of a plane (used to clip at inclined surfaces). */
function halfSpace(o: Vec3, n: Vec3, size: number): Shape3D {
  const fr = frameForPlane(o, n);
  const L = size;
  const pts = [[-L, -L], [L, -L], [L, L], [-L, L]].map(([u, v]) => v3.add(o, v3.add(v3.scale(fr.x, u), v3.scale(fr.y, v))));
  return basicFaceExtrusion(makePolygon(pts as never), new Vector(v3.scale(n, L))) as Shape3D;
}

/** Side faces (not parallel to the sketch) of an extrusion get the draft angle. */
function applyDraft(s: Shape3D, angle: number, outward: boolean, dir: Vec3, start: Vec3): Shape3D {
  const sides = s.faces.filter((f) => {
    if (f.geomType !== "PLANE") return true;
    return Math.abs(v3.dot(faceSig(f).normal, dir)) < 0.999;
  });
  if (!sides.length) return s;
  const fr = frameForPlane(start, dir);
  const plane = new Plane(start as never, fr.x as never, dir as never);
  // Inward (default) tapers the far end smaller.
  const deg = ((outward ? -1 : 1) * angle * 180) / Math.PI;
  return s.draft(deg, (finder) => finder.inList(sides), plane) as Shape3D;
}

function extrudeOne(face: Face, dir: Vec3, ec: EndCondition, c: ExtrudeCtx): Shape3D {
  const e = endDepth(ec, dir, face, c);
  if (e.depth <= 1e-6) throw new Error("The end condition lies behind the sketch plane. Try Reverse Direction.");
  const vec = v3.scale(dir, e.depth);
  let s = ec.draft && ec.draft > 1e-9 ? draftedPrism(face, vec, e.depth, ec.draft, !!ec.draftOutward) : prism(face.clone(), vec);
  if (e.clip) s = s.cut(halfSpace(e.clip.o, e.clip.n, c.big * 4));
  return s;
}

/**
 * Drafted extrusion: a ruled loft from each loop of the profile to its offset copy at
 * the far end (exact planar/conical walls). Holes taper the opposite way, so the
 * material shrinks (draft inward, the default) or grows (draft outward).
 */
function draftedPrism(face: Face, vec: Vec3, depth: number, angle: number, outward: boolean): Shape3D {
  const d = depth * Math.tan(angle) * (outward ? 1 : -1);
  const loop = (w: ReturnType<Face["outerWire"]>, grow: number): Shape3D => {
    const far = w.clone().offset2D(grow, "intersection").translate(vec);
    return loft([w, far as never], { ruled: true }) as Shape3D;
  };
  // replicad's outerWire()/innerWires() consume the face they are called on.
  let s = loop(face.clone().outerWire(), d);
  for (const h of face.clone().innerWires()) s = s.cut(loop(h, -d));
  return s;
}

function extrude(f: ExtrudeFeature, st: StepState, features: Feature[]) {
  const sk = sketchOf(features, f.sketchId, st);
  const frame = st.frames[sk.id];
  if (!frame) throw new Error("Sketch plane unresolved");
  const n = frame.normal;
  // For cuts, extrude away from the material by default (into the part), like SOLIDWORKS.
  const dirSign = (f.reverse ? -1 : 1) * (f.cut ? -1 : 1);
  const dir = v3.scale(n, dirSign);
  // From: start at an offset or at a surface instead of the sketch plane.
  let shift = 0;
  if (f.from?.type === "offset") shift = (f.from.offset ?? 0) * (f.from.reverse ? -1 : 1);
  else if (f.from?.type === "surface") {
    const pl = refPlaneOf(f.from.ref ?? null, st, features);
    const denom = v3.dot(n, pl.n);
    if (Math.abs(denom) < 1e-6) throw new Error("The From surface is perpendicular to the sketch");
    shift = v3.dot(v3.sub(pl.o, frame.origin), pl.n) / denom;
  }
  let faces = regionFaces(profileRegions(sk.sketch, f), frame);
  if (Math.abs(shift) > 1e-12) faces = faces.map((fc) => fc.translate(v3.scale(n, shift)) as Face);
  const c: ExtrudeCtx = { st, features, frame, start: v3.add(frame.origin, v3.scale(n, shift)), big: bigDepth(st.shape, frame), cut: f.cut };
  const solids: Shape3D[] = [];
  for (const face of faces) {
    if (f.end1.type === "midPlane") {
      const d = f.end1.depth;
      const moved = face.translate(v3.scale(dir, -d / 2)) as Face;
      solids.push(extrudeOne(moved, dir, { ...f.end1, type: "blind", depth: d }, { ...c, start: v3.sub(c.start, v3.scale(dir, d / 2)) }));
    } else {
      solids.push(extrudeOne(face, dir, f.end1, c));
      const both = f.end1.type === "throughAllBoth" ? { type: "throughAll" as const, depth: 0 } : f.dir2;
      if (both) solids.push(extrudeOne(face, v3.scale(dir, -1), both, c));
    }
  }
  let tool: Shape3D = solids[0];
  for (let i = 1; i < solids.length; i++) tool = tool.fuse(solids[i]);
  lastTool = tool;
  st.tools[f.id] = [{ shape: tool, cut: f.cut }];
  combine(st, tool, f.cut, f.merge);
}

function revolve(f: RevolveFeature, st: StepState, features: Feature[]) {
  const sk = sketchOf(features, f.sketchId, st);
  const frame = st.frames[sk.id];
  let A: Vec3, dir: Vec3;
  if (f.axisRef) {
    const ax = refAxis(f.axisRef, st, features);
    A = ax.p;
    dir = ax.d;
  } else {
    const axis = sk.sketch.entities[f.axis];
    if (!axis || axis.type !== "line") throw new Error("Select a line, centerline, edge or axis as the axis of revolution");
    const a = sk.sketch.points[axis.p1], b = sk.sketch.points[axis.p2];
    A = frameToWorld(frame, [a.x, a.y]);
    dir = v3.norm(v3.sub(frameToWorld(frame, [b.x, b.y]), A));
  }
  // A solid line used as the axis is not part of the profile.
  const src = f.axisRef || !sk.sketch.entities[f.axis] || (sk.sketch.entities[f.axis] as { construction?: boolean }).construction ? sk.sketch : (() => {
    const c = structuredClone(sk.sketch);
    delete c.entities[f.axis];
    return c;
  })();
  const faces = regionFaces(profileRegions(src, { type: "extrude", thin: f.thin, contours: f.contours } as ExtrudeFeature), frame);
  const deg = (f.angle * 180) / Math.PI;
  const sgn = f.reverse ? -1 : 1;
  let tool: Shape3D | null = null;
  for (const face of faces) {
    let fc: Face = face;
    if (f.revType === "midPlane") fc = face.clone().rotate((-sgn * deg) / 2, A, dir) as Face;
    let s = revolution(fc, A, v3.scale(dir, sgn), Math.min(360, deg));
    if (f.angle2 && f.revType !== "midPlane" && deg < 360) s = s.fuse(revolution(face.clone(), A, v3.scale(dir, -sgn), Math.min(360 - deg, (f.angle2 * 180) / Math.PI)));
    tool = tool ? tool.fuse(s) : s;
  }
  if (!tool) return;
  lastTool = tool;
  st.tools[f.id] = [{ shape: tool, cut: f.cut }];
  combine(st, tool, f.cut, f.merge);
}

function combine(st: StepState, tool: Shape3D, cut: boolean, merge: boolean) {
  if (cut) {
    if (!st.shape) throw new Error("There is no body to cut");
    st.shape = st.shape.cut(tool);
  } else if (!st.shape) st.shape = tool;
  else if (merge) st.shape = st.shape.fuse(tool);
  else st.shape = makeCompound([st.shape, tool]) as Shape3D;
}

function bigDepth(shape: Shape3D | null, frame: Frame) {
  if (!shape) return 1000;
  const [lo, hi] = shape.boundingBox.bounds;
  const diag = Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]);
  const c: Vec3 = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2];
  return diag * 2 + Math.abs(v3.dot(v3.sub(c, frame.origin), frame.normal)) + 1;
}

// ------------------------------------------------------------------ references

export function faceSig(face: Face): FaceSig {
  const c = face.center;
  let normal: Vec3 = [0, 0, 1];
  try {
    normal = face.normalAt(c).toTuple() as Vec3;
  } catch {
    /* degenerate */
  }
  return { surface: face.geomType, center: c.toTuple() as Vec3, normal, area: measureArea(face) };
}

export function edgeSig(e: Edge): EdgeSig {
  return {
    curve: e.geomType,
    mid: e.pointAt(0.5).toTuple() as Vec3,
    a: e.startPoint.toTuple() as Vec3,
    b: e.endPoint.toTuple() as Vec3,
    length: measureLength(e),
  };
}

function findFace(shape: AnyShape, sig: FaceSig, maxScore = 1e3): Face | null {
  let best: Face | null = null;
  let bestScore = Infinity;
  for (const f of shape.faces) {
    if (f.geomType !== sig.surface) continue;
    const s = faceSig(f);
    const score =
      v3.dist(s.center, sig.center) + 10 * (1 - v3.dot(s.normal, sig.normal)) + Math.abs(Math.sqrt(s.area) - Math.sqrt(sig.area));
    if (score < bestScore) {
      bestScore = score;
      best = f;
    }
  }
  return best && bestScore < maxScore ? best : null;
}

function resolveEdges(shape: AnyShape, sigs: EdgeSig[], maxScore?: number): Edge[] {
  const edges = shape.edges;
  const sigsOf = edges.map(edgeSig);
  const out: Edge[] = [];
  for (const sig of sigs) {
    let best = -1, bestScore = Infinity;
    sigsOf.forEach((s, i) => {
      if (s.curve !== sig.curve) return;
      const ends = Math.min(v3.dist(s.a, sig.a) + v3.dist(s.b, sig.b), v3.dist(s.a, sig.b) + v3.dist(s.b, sig.a));
      const score = v3.dist(s.mid, sig.mid) + 0.25 * ends + 0.5 * Math.abs(s.length - sig.length);
      if (score < bestScore) {
        bestScore = score;
        best = i;
      }
    });
    if (best >= 0 && bestScore < (maxScore ?? Math.max(5, sig.length))) out.push(edges[best]);
  }
  return out;
}

// ------------------------------------------------------------------ meshing

function meshShape(shape: Shape3D, quality = 3, owners: Owners = {}): BodyMesh {
  const [lo, hi] = shape.boundingBox.bounds;
  const size = Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) || 1;
  // Image Quality slider: each step roughly halves (or doubles) the chordal deviation.
  const q = Math.max(0, Math.min(10, quality));
  const tol = Math.max(1e-4, size * 4e-4 * Math.pow(2, (3 - q) * 0.8));
  const ang = Math.max(0.03, Math.min(0.5, 0.15 * Math.pow(2, (3 - q) * 0.35)));
  const m = shape.mesh({ tolerance: tol, angularTolerance: ang });
  const em = shape.meshEdges({ tolerance: tol, angularTolerance: ang });
  const faces = shape.faces;
  const faceIdx = new Map<number, number>();
  const faceInfos: FaceInfo[] = faces.map((f, i) => {
    faceIdx.set(f.hashCode, i);
    const sig = faceSig(f);
    return { id: i, sig, planar: f.geomType === "PLANE", owner: owners[ownerKeys(sig)[0]] };
  });
  // Seam edges lie on the closing curve of a periodic face (two p-curves on the same face).
  const oc = getOC() as unknown as { BRep_Tool: { IsClosed: (e: unknown, f: unknown) => boolean } };
  const seam = new Set<number>();
  for (const f of faces) {
    if (f.geomType === "PLANE") continue;
    for (const e of f.edges) {
      try {
        if (oc.BRep_Tool.IsClosed(e.wrapped, f.wrapped)) seam.add(e.hashCode);
      } catch {
        /* binding mismatch: leave visible */
      }
    }
  }
  const edges = shape.edges;
  const edgeIdx = new Map<number, number>();
  const edgeInfos: EdgeInfo[] = edges.map((e, i) => {
    edgeIdx.set(e.hashCode, i);
    return { id: i, sig: edgeSig(e), seam: seam.has(e.hashCode), linear: e.geomType === "LINE" };
  });
  const vset = new Map<string, Vec3>();
  for (const e of edgeInfos) {
    if (e.seam) continue;
    for (const p of [e.sig.a, e.sig.b]) vset.set(p.map((x) => x.toFixed(6)).join(","), p);
  }
  return {
    positions: Float32Array.from(m.vertices),
    normals: Float32Array.from(m.normals),
    indices: Uint32Array.from(m.triangles),
    faceGroups: m.faceGroups.map((g) => ({ start: g.start, count: g.count, faceId: faceIdx.get(g.faceId) ?? -1 })),
    edgePositions: Float32Array.from(em.lines),
    edgeGroups: em.edgeGroups.map((g) => ({ start: g.start, count: g.count, edgeId: edgeIdx.get(g.edgeId) ?? -1 })),
    faces: faceInfos,
    edges: edgeInfos,
    vertices: [...vset.values()],
    bbox: [lo as Vec3, hi as Vec3],
  };
}

// ------------------------------------------------------------------ export

let lastShape: { features: string; shape: Shape3D | null } | null = null;
export function finalShape(features: Feature[]): Shape3D | null {
  const k = JSON.stringify(features);
  if (lastShape?.features === k) return lastShape.shape;
  let st: StepState = { shape: null, frames: {}, planes: {}, owners: {}, tools: {}, dress: {}, axes: {}, points: {}, csys: {}, sketches: {} };
  for (const f of features) {
    if (f.suppressed) continue;
    const next = { ...st, frames: { ...st.frames }, planes: { ...st.planes }, tools: { ...st.tools }, dress: { ...st.dress }, axes: { ...st.axes }, points: { ...st.points }, csys: { ...st.csys }, sketches: { ...st.sketches } };
    try {
      applyFeature(f, next, features);
      // Same rule as rebuild(): a feature with an invalid result is skipped.
      if (next.shape && next.shape !== st.shape) checkResult(next.shape);
      st = next;
    } catch {
      /* skip failing */
    }
  }
  lastShape = { features: k, shape: st.shape };
  return st.shape;
}

// ------------------------------------------------------------------ validity

export const INVALID = "The geometry kernel produced an invalid solid for this feature. Try different settings.";

function isValidShape(s: unknown): boolean {
  const oc = getOC() as unknown as { BRepCheck_Analyzer: new (s: unknown, geom: boolean, par: boolean, exact: boolean) => { IsValid(): boolean; delete(): void } };
  const a = new oc.BRepCheck_Analyzer(s, true, false, false);
  try {
    return a.IsValid();
  } finally {
    a.delete();
  }
}

/**
 * Every body a feature produces must be a valid solid: OCCT occasionally returns broken
 * solids without raising an error. An invalid result fails the feature, so a broken body
 * never reaches later features, the graphics area or an export.
 *
 * Deliberately no automatic repair: ShapeFix_Solid "fixed" a shell of a rounded box by
 * leaving the removed face in as a zero-thickness lid, which passes the checker but is
 * the wrong part (hard.ts H10). A clean error is better than a plausible wrong body.
 */
export function checkResult(shape: Shape3D): Shape3D {
  if (!isValidShape(shape.wrapped)) throw new Error(INVALID);
  return shape;
}

// ------------------------------------------------------------------ requests

/** Result of a request plus the buffers that can be transferred (not copied) to the caller. */
export interface Handled {
  result: unknown;
  transfer: ArrayBuffer[];
}

/**
 * Handle one kernel request. OCCT must already be initialised. Throws on failure;
 * use `errorText` to turn the error into a user-facing message.
 */
export async function handle(req: KernelRequest): Promise<Handled> {
  const done = (result: unknown, transfer: ArrayBuffer[] = []): Handled => ({ result, transfer });
  switch (req.op) {
    case "init":
      return done(null);
    case "rebuild": {
      await ensureImports([...req.features, ...(req.preview ? [req.preview] : [])]);
      const result = rebuild(req.features, req.preview, req.quality);
      const tr: ArrayBuffer[] = [];
      for (const m of [result.mesh, result.toolMesh]) {
        if (m) tr.push(m.positions.buffer as ArrayBuffer, m.normals.buffer as ArrayBuffer, m.indices.buffer as ArrayBuffer, m.edgePositions.buffer as ArrayBuffer);
      }
      return done(result, tr);
    }
    case "export": {
      const shape = finalShape(req.features);
      if (!shape) throw new Error("Nothing to export");
      const blob = req.format === "step" ? shape.blobSTEP() : shape.blobSTL({ binary: true, tolerance: 0.01, angularTolerance: 0.1 });
      return done(await blob.arrayBuffer());
    }
    case "convert": {
      const shape = finalShape(req.features);
      if (!shape) return done([]);
      const edges: Edge[] = [...resolveEdges(shape, req.edges)];
      for (const sig of req.faces) {
        const f = findFace(shape, sig);
        if (f) edges.push(...f.edges);
      }
      // Each model edge once (a face's edges may repeat picked edges).
      const uniq: Edge[] = [];
      for (const e of edges) if (!uniq.some((x) => x.isSame(e))) uniq.push(e);
      return done(uniq.map((e) => projectEdge(e, req.frame)).filter(Boolean));
    }
    case "measure": {
      const shape = finalShape(req.features);
      if (!shape || req.refs.length < 2) return done(null);
      const toShape = (r: (typeof req.refs)[number]) =>
        r.kind === "vertex" ? makeVertex(r.p as never) : r.kind === "face" ? findFace(shape, r.sig) : (resolveEdges(shape, [r.sig])[0] ?? null);
      const a = toShape(req.refs[0]), b = toShape(req.refs[1]);
      if (!a || !b) return done(null);
      const tool = new DistanceTool();
      const distance = tool.distanceBetween(a, b);
      const w = tool.wrapped as unknown as { PointOnShape1: (i: number) => { X(): number; Y(): number; Z(): number }; PointOnShape2: (i: number) => { X(): number; Y(): number; Z(): number } };
      const q1 = w.PointOnShape1(1), q2 = w.PointOnShape2(1);
      return done({ distance, p1: [q1.X(), q1.Y(), q1.Z()], p2: [q2.X(), q2.Y(), q2.Z()] });
    }
    case "clearCache":
      clearCache();
      return done(null);
    case "massProps": {
      const shape = finalShape(req.features);
      return done(shape ? massProps(shape) : null);
    }
  }
}

/** Drop every memoised result (feature steps and the last final shape). */
export function clearCache() {
  cache.clear();
  lastShape = null;
}

/** Project a model edge onto a sketch frame: exact lines/arcs/circles when the projection keeps their shape. */
function projectEdge(e: Edge, fr: Frame) {
  const P2 = (p: Vec3): [number, number] => {
    const d = v3.sub(p, fr.origin);
    return [v3.dot(d, fr.x), v3.dot(d, fr.y)];
  };
  const at = (t: number) => e.pointAt(t).toTuple() as Vec3;
  if (e.geomType === "LINE") {
    const a = P2(at(0)), b = P2(at(1));
    if (Math.hypot(a[0] - b[0], a[1] - b[1]) < 1e-9) return null; // edge seen end-on
    return { kind: "line" as const, a, b };
  }
  if (e.geomType === "CIRCLE") {
    const ax = circleAxis(e);
    if (Math.abs(Math.abs(v3.dot(ax.d, fr.normal)) - 1) < 1e-6) {
      const r = v3.dist(at(0), ax.p);
      if (v3.dist(at(0), at(1)) < 1e-7) return { kind: "circle" as const, c: P2(ax.p), r };
      return { kind: "arc" as const, a: P2(at(0)), m: P2(at(0.5)), b: P2(at(1)) };
    }
  }
  const pts: [number, number][] = [];
  for (let i = 0; i <= 48; i++) pts.push(P2(at(i / 48)));
  return { kind: "poly" as const, pts };
}

export function massProps(shape: Shape3D) {
  const v = measureShapeVolumeProperties(shape);
  const s = measureShapeSurfaceProperties(shape);
  return { volume: v.volume, area: s.area, centerOfMass: v.centerOfMass };
}
