/**
 * Test-only builder for corpus parts: produces real `PartDocument`s (the same JSON the
 * app saves as .ocpart) from short scripts, and computes per-step metrics with the
 * kernel engine. Faces and edges are picked from the current body exactly as the UI
 * does (copying `FaceInfo.sig` / `EdgeInfo.sig` from a rebuild), so references in the
 * documents are the ones the app would store.
 *
 * Ids are deterministic (a per-part counter), so a part always builds to the same file.
 */
import type {
  BodyMesh,
  EdgeSig,
  FaceSig,
  Feature,
  GeomRef,
  Id,
  PartDocument,
  PlaneRef,
  SketchData,
  SketchFeature,
  Vec2,
  Vec3,
} from "../../core/types";
import { emptySketch } from "../../sketch/model";
import { ORIGIN_ID } from "../../sketch/solver";
import { measureShapeSurfaceProperties, measureShapeVolumeProperties, type Shape3D } from "replicad";
import { ensureImports, rebuild } from "../engine";

type NoId<T> = T extends unknown ? Omit<T, "id" | "name"> : never;

// ------------------------------------------------------------------ sketches

/** Draws into a SketchData with deterministic ids (prefixed by the sketch id). */
export class SketchDraw {
  private n = 0;
  constructor(readonly sk: SketchData, private prefix: string) {}

  private id(kind: string) {
    return `${this.prefix}${kind}${++this.n}`;
  }

  /** A point; `ORIGIN_ID` is returned for (0, 0) when `origin` is set. */
  point(x: number, y: number, origin = false): Id {
    if (origin && x === 0 && y === 0) return ORIGIN_ID;
    const id = this.id("p");
    this.sk.points[id] = { id, x, y };
    return id;
  }

  line(a: Id, b: Id, construction = false): Id {
    const id = this.id("l");
    this.sk.entities[id] = { id, type: "line", p1: a, p2: b, ...(construction ? { construction } : {}) };
    return id;
  }

  /** Closed or open polyline through the given coordinates; returns the line ids. */
  poly(pts: Vec2[], closed = true): Id[] {
    const ids = pts.map(([x, y]) => this.point(x, y));
    const lines: Id[] = [];
    for (let i = 0; i < ids.length - (closed ? 0 : 1); i++) lines.push(this.line(ids[i], ids[(i + 1) % ids.length]));
    return lines;
  }

  rect(x0: number, y0: number, x1: number, y1: number): Id[] {
    return this.poly([[x0, y0], [x1, y0], [x1, y1], [x0, y1]]);
  }

  circle(cx: number, cy: number, r: number, construction = false): Id {
    const c = this.point(cx, cy);
    const id = this.id("c");
    this.sk.entities[id] = { id, type: "circle", c, r, ...(construction ? { construction } : {}) };
    return id;
  }

  /** Arc counter-clockwise from p1 to p2 about c (points must already be equidistant). */
  arc(c: Id, p1: Id, p2: Id): Id {
    const id = this.id("a");
    this.sk.entities[id] = { id, type: "arc", c, p1, p2 };
    return id;
  }

  ellipse(cx: number, cy: number, ra: number, rb: number): Id {
    const c = this.point(cx, cy);
    const a = this.point(cx + ra, cy);
    const q = this.point(cx, cy + rb);
    const id = this.id("e");
    this.sk.entities[id] = { id, type: "ellipse", c, a, q };
    return id;
  }

  spline(pts: Vec2[]): Id {
    const id = this.id("s");
    this.sk.entities[id] = { id, type: "spline", pts: pts.map(([x, y]) => this.point(x, y)) };
    return id;
  }

  /** A standalone sketch point (Hole Wizard positions). */
  sketchPoint(x: number, y: number): Id {
    const p = this.point(x, y);
    this.sk.points[p].standalone = true;
    const id = this.id("pt");
    this.sk.entities[id] = { id, type: "point", p };
    return id;
  }
}

// ------------------------------------------------------------------ parts

export const STD = {
  front: { kind: "std", plane: "front" } as PlaneRef,
  top: { kind: "std", plane: "top" } as PlaneRef,
  right: { kind: "std", plane: "right" } as PlaneRef,
};

/** Plane reference as a GeomRef (directions, mirror planes, neutral planes). */
export const planeRef = (p: PlaneRef): GeomRef => ({ kind: "plane", plane: p });

export class PartBuilder {
  readonly doc: PartDocument;
  private n = 0;
  private counts = new Map<string, number>();

  constructor(name: string) {
    this.doc = { format: "opencad-part", version: 1, name, units: "MMGS", features: [], rollback: null };
  }

  private nextId() {
    return `f${++this.n}`;
  }

  private nextName(base: string) {
    const k = (this.counts.get(base) ?? 0) + 1;
    this.counts.set(base, k);
    return `${base}${k}`;
  }

  /** Add a sketch; returns its feature id. */
  sketch(plane: PlaneRef, draw: (d: SketchDraw) => void): Id {
    const id = this.nextId();
    const sk = emptySketch();
    draw(new SketchDraw(sk, `${id}_`));
    const f: SketchFeature = { id, name: this.nextName("Sketch"), type: "sketch", plane, sketch: sk };
    this.doc.features.push(f);
    return id;
  }

  /** Add any other feature; returns its id. */
  add(name: string, f: NoId<Feature>): Id {
    const id = this.nextId();
    this.doc.features.push({ ...f, id, name: this.nextName(name) } as Feature);
    return id;
  }

  /** The body as the UI would see it now (fails the build if any feature errored). */
  mesh(): BodyMesh {
    const r = rebuild(this.doc.features);
    const bad = r.statuses.filter((s) => s.error);
    if (bad.length) throw new Error(`${this.doc.name}: ${bad.map((s) => `${s.id}: ${s.error}`).join("; ")}`);
    if (!r.mesh) throw new Error(`${this.doc.name}: no body yet`);
    return r.mesh;
  }

  /** The single face matching `pred` (throws if none or several match). */
  face(pred: (s: FaceSig) => boolean): FaceSig {
    const all = this.mesh().faces.map((f) => f.sig).filter(pred);
    if (all.length !== 1) throw new Error(`${this.doc.name}: expected one face, found ${all.length}`);
    return all[0];
  }

  faces(pred: (s: FaceSig) => boolean): FaceSig[] {
    const all = this.mesh().faces.map((f) => f.sig).filter(pred);
    if (!all.length) throw new Error(`${this.doc.name}: no faces matched`);
    return all;
  }

  edges(pred: (s: EdgeSig) => boolean): EdgeSig[] {
    const m = this.mesh();
    const all = m.edges.filter((e) => !e.seam).map((e) => e.sig).filter(pred);
    if (!all.length) throw new Error(`${this.doc.name}: no edges matched`);
    return all;
  }
}

// ------------------------------------------------------------------ predicates

const near = (a: number, b: number, tol = 1e-6) => Math.abs(a - b) <= tol;
export const nearV = (a: Vec3, b: Vec3, tol = 1e-6) => a.every((x, i) => near(x, b[i], tol));

/** Planar face whose normal is `n` and whose centre has coordinate `axis` = `value`. */
export const planar = (n: Vec3, axis?: 0 | 1 | 2, value?: number) => (s: FaceSig) =>
  s.surface === "PLANE" && nearV(s.normal, n, 1e-6) && (axis === undefined || near(s.center[axis], value!, 1e-6));

/** Edge whose midpoint satisfies `pred`. */
export const edgeAt = (pred: (mid: Vec3) => boolean, curve?: string) => (s: EdgeSig) => (!curve || s.curve === curve) && pred(s.mid);

// ------------------------------------------------------------------ metrics

export interface StepMetrics {
  id: Id;
  name: string;
  type: Feature["type"];
  error?: string;
  warning?: string;
  /** Body after this step (absent when there is no body yet). */
  body?: { volume: number; area: number; solids: number; faces: number; edges: number; bbox: [Vec3, Vec3] };
  /** Resolved reference geometry this feature defines (sketch frames, planes, axes, points, csys). */
  ref?: unknown;
}

export interface PartMetrics {
  part: string;
  steps: StepMetrics[];
}

/** 6 significant figures; values below 1e-7 in magnitude become 0 (no -0, no float dust). */
export function round(x: number): number {
  if (!Number.isFinite(x)) return x;
  if (Math.abs(x) < 1e-7) return 0;
  return Number(x.toPrecision(6));
}

function roundDeep<T>(v: T): T {
  if (typeof v === "number") return round(v) as T;
  if (Array.isArray(v)) return v.map(roundDeep) as T;
  if (v && typeof v === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, x] of Object.entries(v)) out[k] = roundDeep(x);
    return out as T;
  }
  return v;
}

function bodyMetrics(shape: Shape3D): StepMetrics["body"] {
  const [lo, hi] = shape.boundingBox.bounds;
  return {
    volume: measureShapeVolumeProperties(shape).volume,
    area: measureShapeSurfaceProperties(shape).area,
    solids: shape.solids.length,
    faces: shape.faces.length,
    edges: shape.edges.length,
    bbox: [lo as Vec3, hi as Vec3],
  };
}

/**
 * Rebuild a document and measure the body after every feature. The engine's memo cache
 * is keyed by content, so warm and cold rebuilds give the same values.
 */
export async function measure(doc: PartDocument): Promise<PartMetrics> {
  await ensureImports(doc.features);
  const steps: StepMetrics[] = [];
  const r = rebuild(doc.features, undefined, 3, (f, shape, status) => {
    const s: StepMetrics = { id: f.id, name: f.name, type: f.type };
    if (status.error) s.error = status.error;
    if (status.warning) s.warning = status.warning;
    if (shape) s.body = bodyMetrics(shape);
    steps.push(s);
  });
  for (const s of steps) {
    const ref = r.frames[s.id] ?? r.planes[s.id]?.frame ?? r.axes?.[s.id] ?? r.points?.[s.id] ?? r.csys?.[s.id];
    if (ref) s.ref = ref;
  }
  return roundDeep({ part: doc.name, steps });
}
