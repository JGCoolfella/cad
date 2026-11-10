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
  FeatureStatus,
  Frame,
  PlaneRef,
  RebuildResult,
  RevolveFeature,
  Vec3,
} from "../core/types";
import { STD_FRAMES, frameForPlane, frameToWorld, offsetFrame, v3 } from "../core/math";
import { extractProfiles, type Region } from "../sketch/model";
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

interface StepState {
  owners: Owners;
  shape: Shape3D | null;
  frames: Record<string, Frame>;
  planes: Record<string, { frame: Frame; size: number }>;
}
const cache = new Map<string, { state: StepState; status: FeatureStatus }>();

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
  let state: StepState = { shape: null, frames: {}, planes: {}, owners: {} };
  let key = "root";
  const statuses: FeatureStatus[] = [];
  const all = preview ? [...features, preview] : features;
  const used = new Set<string>();
  for (const f of all) {
    if (f.suppressed) {
      statuses.push({ id: f.id });
      continue;
    }
    key = stepKey(key, f);
    used.add(key);
    let hit = cache.get(key);
    if (!hit) {
      const next: StepState = { shape: state.shape, frames: { ...state.frames }, planes: { ...state.planes }, owners: state.owners };
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
  }
  // Evict stale cache entries (keep memory bounded).
  if (cache.size > 200) for (const k of cache.keys()) if (!used.has(k)) cache.delete(k);
  const mesh = state.shape ? meshShape(state.shape, quality, state.owners) : null;
  return { mesh, statuses, frames: state.frames, planes: state.planes, timeMs: performance.now() - t0 };
}

function errorText(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "number") return "Geometry kernel error (OCCT exception)";
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
  }
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

function extrude(f: ExtrudeFeature, st: StepState, features: Feature[]) {
  const sk = sketchOf(features, f.sketchId);
  const frame = st.frames[sk.id];
  if (!frame) throw new Error("Sketch plane unresolved");
  const prof = extractProfiles(sk.sketch);
  if (!prof.regions.length) throw new Error(prof.openEnds ? "The sketch contains open contours" : "The sketch has no closed contours");
  const faces = regionFaces(prof.regions, frame);
  const n = frame.normal;
  // For cuts, extrude away from the material by default (into the part), like SOLIDWORKS.
  const dirSign = (f.reverse ? -1 : 1) * (f.cut ? -1 : 1);
  const big = bigDepth(st.shape, frame);
  const solids: Shape3D[] = [];
  const depthOf = (e: ExtrudeFeature["end1"]) => (e.type === "throughAll" ? big : e.depth);
  for (const face of faces) {
    if (f.end1.type === "midPlane") {
      const d = f.end1.depth;
      const moved = face.translate(v3.scale(n, (-dirSign * d) / 2)) as Face;
      solids.push(prism(moved, v3.scale(n, dirSign * d)));
    } else {
      solids.push(prism(face.clone(), v3.scale(n, dirSign * depthOf(f.end1))));
      if (f.dir2) solids.push(prism(face.clone(), v3.scale(n, -dirSign * depthOf(f.dir2))));
    }
  }
  let tool: Shape3D = solids[0];
  for (let i = 1; i < solids.length; i++) tool = tool.fuse(solids[i]);
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
  let st: StepState = { shape: null, frames: {}, planes: {}, owners: {} };
  for (const f of features) {
    if (f.suppressed) continue;
    const next = { ...st, frames: { ...st.frames }, planes: { ...st.planes } };
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
        if (result.mesh) {
          const m = result.mesh;
          tr.push(m.positions.buffer, m.normals.buffer, m.indices.buffer, m.edgePositions.buffer);
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
