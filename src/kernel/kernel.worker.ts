/// <reference lib="webworker" />
/**
 * Geometry kernel worker. Owns the OpenCascade (B-rep) instance and regenerates the
 * feature tree off the UI thread, so the interface never blocks on a rebuild.
 *
 * Results of each feature are memoised by a key of (feature JSON + input key), so
 * editing a late feature only recomputes from that feature onwards.
 */
import opencascade from "replicad-opencascadejs";
import wasmUrl from "replicad-opencascadejs/wasm?url";
import {
  setOC,
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
  Vec3,
} from "../core/types";
import { STD_FRAMES, frameForPlane, frameToWorld, offsetFrame, v3 } from "../core/math";
import { extractProfiles, regionAtPoint, type Region } from "../sketch/model";
import { offsetEntities } from "../sketch/ops";
import { uid } from "../core/ids";
import type { KernelRequest, KernelResponse } from "./protocol";

let ready: Promise<void> | null = null;
function init() {
  if (!ready) {
    ready = (async () => {
      const oc = await (opencascade as unknown as (o: object) => Promise<unknown>)({
        locateFile: () => wasmUrl,
      });
      setOC(oc as never);
    })();
  }
  return ready;
}

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

interface StepState {
  owners: Owners;
  shape: Shape3D | null;
  tools: Record<string, FeatureTool[]>;
  frames: Record<string, Frame>;
  planes: Record<string, { frame: Frame; size: number }>;
}
const cache = new Map<string, { state: StepState; status: FeatureStatus }>();
/** Body made by the last extrude/revolve applied (before it was combined with the part). */
let lastTool: Shape3D | null = null;

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

function rebuild(features: Feature[], preview?: Feature, quality = 3): RebuildResult {
  const t0 = performance.now();
  let state: StepState = { shape: null, frames: {}, planes: {}, owners: {}, tools: {} };
  let key = "root";
  const statuses: FeatureStatus[] = [];
  const all = preview ? [...features, preview] : features;
  const used = new Set<string>();
  let before: StepState | null = null;
  let tool: Shape3D | null = null;
  for (const f of all) {
    if (f.suppressed) {
      statuses.push({ id: f.id });
      continue;
    }
    if (f === preview) {
      before = state;
      lastTool = null;
    }
    key = stepKey(key, f);
    used.add(key);
    let hit = cache.get(key);
    if (!hit) {
      const next: StepState = { shape: state.shape, frames: { ...state.frames }, planes: { ...state.planes }, owners: state.owners, tools: { ...state.tools } };
      const status: FeatureStatus = { id: f.id };
      try {
        applyFeature(f, next, features);
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
    if (f === preview && !hit.status.error) tool = lastTool;
  }
  // Evict stale cache entries (keep memory bounded).
  if (cache.size > 200) for (const k of cache.keys()) if (!used.has(k)) cache.delete(k);
  // Extrude/revolve previews: the part as it was, plus the added (boss) or removed (cut) body.
  let toolShape: Shape3D | null = null;
  const previewCut = preview && "cut" in preview ? preview.cut : preview && (preview.type === "linearPattern" || preview.type === "circularPattern" || preview.type === "mirror") ? !!state.tools[preview.id]?.every((t) => t.cut) : false;
  if (preview && before && tool && (preview.type === "extrude" || preview.type === "revolve" || preview.type === "linearPattern" || preview.type === "circularPattern" || preview.type === "mirror")) {
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
  return { mesh, toolMesh, statuses, frames: state.frames, planes: state.planes, timeMs: performance.now() - t0 };
}

function errorText(e: unknown): string {
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
      return;
    case "refplane": {
      const base = resolvePlane(f.base, st);
      st.planes[f.id] = { frame: offsetFrame(base, f.offset, f.flip), size: 60 };
      return;
    }
    case "extrude":
      return extrude(f, st, features);
    case "revolve":
      return revolve(f, st, features);
    case "fillet": {
      if (!st.shape) throw new Error("Nothing to fillet");
      const edges = resolveEdges(st.shape, f.edges);
      for (const sig of f.faces) {
        const face = findFace(st.shape, sig);
        if (face) edges.push(...face.edges);
      }
      if (!edges.length) throw new Error("Fillet edges could not be found");
      const set = new Set(edges.map((e) => e.hashCode));
      st.shape = st.shape.fillet((e: Edge) => (set.has(e.hashCode) && edges.some((x) => x.isSame(e)) ? f.radius : null) as number);
      return;
    }
    case "chamfer": {
      if (!st.shape) throw new Error("Nothing to chamfer");
      const edges = resolveEdges(st.shape, f.edges);
      if (!edges.length) throw new Error("Chamfer edges could not be found");
      st.shape = st.shape.chamfer((e: Edge) => (edges.some((x) => x.isSame(e)) ? f.distance : null) as number);
      return;
    }
    case "linearPattern": {
      const offsets: Vec3[] = [];
      const d1 = v3.scale(refDirection(f.dir1.ref, st, features), f.dir1.reverse ? -1 : 1);
      const d2 = f.dir2?.ref ? v3.scale(refDirection(f.dir2.ref, st, features), f.dir2.reverse ? -1 : 1) : null;
      for (let i = 0; i < Math.max(1, f.dir1.count); i++) {
        for (let j = 0; j < (d2 ? Math.max(1, f.dir2!.count) : 1); j++) {
          if (i === 0 && j === 0) continue;
          let v = v3.scale(d1, f.dir1.spacing * i);
          if (d2) v = v3.add(v, v3.scale(d2, f.dir2!.spacing * j));
          offsets.push(v);
        }
      }
      return repeatTools(f.id, f.seeds, st, offsets.map((v) => (s: Shape3D) => s.translate(v) as Shape3D));
    }
    case "circularPattern": {
      const ax = refAxis(f.axis, st, features);
      const n = Math.max(1, f.count);
      const deg = (f.angle * 180) / Math.PI;
      const full = Math.abs(Math.abs(deg) - 360) < 1e-6;
      const step = f.equalSpacing ? deg / (full ? n : Math.max(1, n - 1)) : deg;
      const s = f.reverse ? -1 : 1;
      const xfs: ((x: Shape3D) => Shape3D)[] = [];
      for (let i = 1; i < n; i++) xfs.push((x) => x.rotate(s * step * i, ax.p, ax.d) as Shape3D);
      return repeatTools(f.id, f.seeds, st, xfs);
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
      return repeatTools(f.id, f.seeds, st, [(x) => x.clone().mirror(pl.n, pl.o) as Shape3D]);
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

/** Repeat the bodies of `seeds` through each transform and combine them with the part. */
function repeatTools(fid: string, seeds: string[], st: StepState, xfs: ((s: Shape3D) => Shape3D)[]) {
  if (!seeds.length) throw new Error("Select the features to pattern");
  if (!xfs.length) return;
  const out: FeatureTool[] = [];
  for (const sid of seeds) {
    const tools = st.tools[sid];
    if (!tools?.length) throw new Error("Only extruded, revolved, patterned and mirrored features can be patterned yet");
    for (const t of tools) {
      let acc: Shape3D | null = null;
      for (const x of xfs) {
        const c = x(t.shape.clone() as Shape3D);
        acc = acc ? (acc.fuse(c) as Shape3D) : c;
      }
      if (!acc) continue;
      combine(st, acc, t.cut, true);
      out.push({ shape: acc, cut: t.cut });
    }
  }
  st.tools[fid] = out;
  lastTool = out.length === 1 ? out[0].shape : null;
}

/** Unit direction of a reference (linear edge, plane/face normal, sketch line). */
function refDirection(ref: GeomRef | null, st: StepState, features: Feature[]): Vec3 {
  if (!ref) throw new Error("Select a direction (a linear edge, face, plane or sketch line)");
  if (ref.kind === "edge") return v3.norm(v3.sub(ref.sig.b, ref.sig.a));
  if (ref.kind === "face") return v3.norm(ref.sig.normal);
  if (ref.kind === "plane") return resolvePlane(ref.plane, st).normal;
  const ax = refAxis(ref, st, features);
  return ax.d;
}

/** Axis (point + unit direction) of a reference. */
function refAxis(ref: GeomRef | null, st: StepState, features: Feature[]): { p: Vec3; d: Vec3 } {
  if (!ref) throw new Error("Select an axis (a circular or straight edge, a cylindrical face or a sketch line)");
  if (ref.kind === "sketchLine") {
    const sk = sketchOf(features, ref.sketchId);
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
  throw new Error("Select a plane or planar face to mirror about");
}

function sketchOf(features: Feature[], id: string) {
  const s = features.find((x) => x.id === id);
  if (!s || s.type !== "sketch") throw new Error("Sketch not found");
  return s;
}

function regionFaces(regions: Region[], frame: Frame): Face[] {
  const W = (p: [number, number]) => frameToWorld(frame, p);
  const wireOf = (loop: Region["outer"]) => {
    const edges: Edge[] = [];
    for (const s of loop.segs) {
      if (s.type === "line") edges.push(makeLine(W(s.a), W(s.b)));
      else if (s.type === "arc") edges.push(makeThreePointArc(W(s.a), W(s.mid), W(s.b)));
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
  const ids = Object.values(sk.entities).filter((e) => e.type !== "point" && !(e as { construction?: boolean }).construction).map((e) => e.id);
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
  const sk = sketchOf(features, f.sketchId);
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
  const sk = sketchOf(features, f.sketchId);
  const frame = st.frames[sk.id];
  const axis = sk.sketch.entities[f.axis];
  if (!axis || axis.type !== "line") throw new Error("Select a line or centerline as the axis of revolution");
  const a = sk.sketch.points[axis.p1], b = sk.sketch.points[axis.p2];
  const A = frameToWorld(frame, [a.x, a.y]);
  const B = frameToWorld(frame, [b.x, b.y]);
  const dir = v3.norm(v3.sub(B, A));
  // Exclude the axis itself from the profile if it is a solid line.
  const prof = extractProfiles(sk.sketch);
  if (!prof.regions.length) throw new Error("The sketch has no closed contours");
  const faces = regionFaces(prof.regions, frame);
  const deg = (f.angle * 180) / Math.PI;
  const sgn = f.reverse ? -1 : 1;
  let tool: Shape3D | null = null;
  for (const face of faces) {
    let fc: Face = face;
    if (f.revType === "midPlane") fc = face.rotate((-sgn * deg) / 2, A, dir) as Face;
    const s = revolution(fc, A, v3.scale(dir, sgn), Math.min(360, deg));
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

function findFace(shape: AnyShape, sig: FaceSig): Face | null {
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
  return best && bestScore < 1e3 ? best : null;
}

function resolveEdges(shape: AnyShape, sigs: EdgeSig[]): Edge[] {
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
    if (best >= 0 && bestScore < Math.max(5, sig.length)) out.push(edges[best]);
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
function finalShape(features: Feature[]): Shape3D | null {
  const k = JSON.stringify(features);
  if (lastShape?.features === k) return lastShape.shape;
  let st: StepState = { shape: null, frames: {}, planes: {}, owners: {}, tools: {} };
  for (const f of features) {
    if (f.suppressed) continue;
    const next = { ...st, frames: { ...st.frames }, planes: { ...st.planes }, tools: { ...st.tools } };
    try {
      applyFeature(f, next, features);
      st = next;
    } catch {
      /* skip failing */
    }
  }
  lastShape = { features: k, shape: st.shape };
  return st.shape;
}

// ------------------------------------------------------------------ messaging

self.onmessage = async (ev: MessageEvent<KernelRequest>) => {
  const req = ev.data;
  const reply = (r: KernelResponse, transfer: Transferable[] = []) => (self as DedicatedWorkerGlobalScope).postMessage(r, transfer);
  try {
    await init();
    switch (req.op) {
      case "init":
        reply({ id: req.id, ok: true, result: null });
        break;
      case "rebuild": {
        const result = rebuild(req.features, req.preview, req.quality);
        const tr: Transferable[] = [];
        for (const m of [result.mesh, result.toolMesh]) {
          if (m) tr.push(m.positions.buffer, m.normals.buffer, m.indices.buffer, m.edgePositions.buffer);
        }
        reply({ id: req.id, ok: true, result }, tr);
        break;
      }
      case "export": {
        const shape = finalShape(req.features);
        if (!shape) throw new Error("Nothing to export");
        const blob = req.format === "step" ? shape.blobSTEP() : shape.blobSTL({ binary: true, tolerance: 0.01, angularTolerance: 0.1 });
        reply({ id: req.id, ok: true, result: await blob.arrayBuffer() });
        break;
      }
      case "clearCache":
        cache.clear();
        lastShape = null;
        reply({ id: req.id, ok: true, result: null });
        break;
      case "massProps": {
        const shape = finalShape(req.features);
        reply({ id: req.id, ok: true, result: shape ? massProps(shape) : null });
        break;
      }
    }
  } catch (e) {
    reply({ id: req.id, ok: false, error: errorText(e) });
  }
};

function massProps(shape: Shape3D) {
  const v = measureShapeVolumeProperties(shape);
  const s = measureShapeSurfaceProperties(shape);
  return { volume: v.volume, area: s.area, centerOfMass: v.centerOfMass };
}
