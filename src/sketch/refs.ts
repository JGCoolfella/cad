/**
 * External references: model vertices and edges used by a sketch (SOLIDWORKS relations to
 * model geometry). Each reference owns fixed sketch geometry (a point, line, arc or circle,
 * flagged `ext`) that ordinary relations and dimensions attach to. The rebuild re-finds the
 * reference by signature, and `updateRefGeometry` moves that geometry before the sketch is
 * solved, so the sketch follows the model.
 *
 * Pure data code: used by the UI and by the kernel engine.
 */
import type { EdgeSig, ExternalRef, Frame, Id, SketchData, SketchEntity, SketchRef, Vec2, Vec3 } from "../core/types";
import { v2, v3, worldToFrame } from "../core/math";
import { uid } from "../core/ids";
import { circumcenter } from "./model";

export type RefShape =
  | { kind: "point"; p: Vec2 }
  | { kind: "line"; a: Vec2; b: Vec2 }
  | { kind: "circle"; c: Vec2; r: number }
  | { kind: "arc"; c: Vec2; a: Vec2; b: Vec2 };

type NewRef = { kind: "vertex"; p: Vec3; edges?: EdgeSig[] } | { kind: "edge"; sig: EdgeSig } | ExternalEdge;

/** Another component's edge in this part's coordinates (G1d, Edit Part in an assembly). */
export type ExternalEdge = { kind: "external"; sig: EdgeSig; ext: ExternalRef };

const height = (f: Frame, p: Vec3) => v3.dot(v3.sub(p, f.origin), f.normal);

/**
 * The reference projected onto the sketch plane, or null when it has no sketch
 * counterpart: an edge seen end-on, a circle not parallel to the plane, or a curve that is
 * neither a line nor a circle (those can still be snapped to, without a relation).
 */
export function refShape(ref: NewRef, f: Frame): RefShape | null {
  if (ref.kind === "vertex") return { kind: "point", p: worldToFrame(f, ref.p) };
  const s = ref.sig;
  const a = worldToFrame(f, s.a), b = worldToFrame(f, s.b), m = worldToFrame(f, s.mid);
  const scale = Math.max(1, s.length);
  if (s.curve === "LINE") return v2.dist(a, b) > 1e-9 * scale ? { kind: "line", a, b } : null;
  if (s.curve !== "CIRCLE") return null;
  // A circle keeps its shape when projected only if it is parallel to the sketch plane.
  const h = height(f, s.a);
  if (Math.abs(height(f, s.mid) - h) > 1e-7 * scale || Math.abs(height(f, s.b) - h) > 1e-7 * scale) return null;
  if (v3.dist(s.a, s.b) < 1e-7 * scale) {
    // Closed: the parameter midpoint is diametrically opposite the start.
    return { kind: "circle", c: v2.lerp(a, m, 0.5), r: v2.dist(a, m) / 2 };
  }
  const c = circumcenter(a, m, b);
  if (!c) return null;
  // Stored counter-clockwise: swap the ends of a clockwise arc (as Convert Entities does).
  const ccw = (m[0] - a[0]) * (b[1] - a[1]) - (m[1] - a[1]) * (b[0] - a[0]) < 0;
  return { kind: "arc", c, a: ccw ? a : b, b: ccw ? b : a };
}

function sameRef(r: SketchRef, n: NewRef): boolean {
  if (r.kind === "vertex" && n.kind === "vertex") return v3.dist(r.p, n.p) < 1e-6;
  const sameComponent = r.kind !== "external" || (n.kind === "external" && r.ext.component.join("/") === n.ext.component.join("/"));
  if (r.kind !== "vertex" && n.kind !== "vertex" && (r.kind === "external") === (n.kind === "external") && sameComponent) {
    const s = r.sig, t = n.sig;
    return s.curve === t.curve && v3.dist(s.mid, t.mid) < 1e-6 && Math.min(v3.dist(s.a, t.a) + v3.dist(s.b, t.b), v3.dist(s.a, t.b) + v3.dist(s.b, t.a)) < 1e-6;
  }
  return false;
}

/** The sketch geometry standing for reference `refId`: its entity (edges) or point (vertices). */
export function refGeometry(sk: SketchData, refId: Id): { entity?: Id; point?: Id } | null {
  const e = Object.values(sk.entities).find((x) => "ext" in x && x.ext === refId);
  if (e) return { entity: e.id };
  const p = Object.values(sk.points).find((x) => x.ext === refId);
  return p ? { point: p.id } : null;
}

/**
 * Reference a model vertex or edge from the sketch (reusing an existing reference to the
 * same geometry). Returns the sketch point (vertex) or entity (edge) to attach relations to,
 * or null if the geometry has no sketch counterpart.
 */
export function addRef(sk: SketchData, ref: NewRef, f: Frame): { entity?: Id; point?: Id } | null {
  const existing = Object.values(sk.refs ?? {}).find((r) => sameRef(r, ref));
  if (existing) {
    const g = refGeometry(sk, existing.id);
    if (g) return g;
  }
  const shape = refShape(ref, f);
  if (!shape) return null;
  const id = existing?.id ?? uid("x");
  sk.refs = { ...(sk.refs ?? {}), [id]: { ...ref, id } as SketchRef };
  const pt = (p: Vec2): Id => {
    const pid = uid("p");
    sk.points[pid] = { id: pid, x: p[0], y: p[1], ext: id };
    return pid;
  };
  const eid = uid("e");
  let e: SketchEntity;
  switch (shape.kind) {
    case "point":
      return { point: pt(shape.p) };
    case "line":
      e = { id: eid, type: "line", p1: pt(shape.a), p2: pt(shape.b), ext: id };
      break;
    case "circle":
      e = { id: eid, type: "circle", c: pt(shape.c), r: shape.r, ext: id };
      break;
    case "arc":
      e = { id: eid, type: "arc", c: pt(shape.c), p1: pt(shape.a), p2: pt(shape.b), ext: id };
      break;
  }
  sk.entities[eid] = e;
  return { entity: eid };
}

/**
 * Convert Entities of another component's edge (G1d, Edit Part in an assembly): a sketch
 * entity on the edge's projection that follows it when the assembly updates the reference.
 * It shares the points of the reference's fixed geometry (and, for a circle, has an Equal
 * relation to it), so profiles can use it. Returns the new entity, or null when the edge
 * has no sketch counterpart (seen end-on, or a circle not parallel to the sketch).
 */
export function convertExternal(sk: SketchData, ref: ExternalEdge, f: Frame): Id | null {
  const g = addRef(sk, ref, f);
  const r = g?.entity ? sk.entities[g.entity] : undefined;
  if (!r) return null;
  const id = uid("e");
  if (r.type === "line") sk.entities[id] = { id, type: "line", p1: r.p1, p2: r.p2 };
  else if (r.type === "arc") sk.entities[id] = { id, type: "arc", c: r.c, p1: r.p1, p2: r.p2 };
  else if (r.type === "circle") {
    sk.entities[id] = { id, type: "circle", c: r.c, r: r.r };
    const rid = uid("r");
    sk.relations[rid] = { id: rid, type: "equal", e1: id, e2: r.id };
  } else return null;
  return id;
}

/**
 * Move every reference's sketch geometry to where its (re-found) model geometry projects.
 * `only`: restrict to these references (the ones the rebuild found). Returns the ids of
 * references whose geometry no longer fits (an edge that changed type, say); those keep
 * their last position.
 */
export function updateRefGeometry(sk: SketchData, f: Frame, only?: Set<Id>): Id[] {
  const bad: Id[] = [];
  for (const r of Object.values(sk.refs ?? {})) {
    if (only && !only.has(r.id)) continue;
    const g = refGeometry(sk, r.id);
    const shape = refShape(r, f);
    if (!g || !shape) {
      bad.push(r.id);
      continue;
    }
    const set = (pid: Id, p: Vec2) => {
      const q = sk.points[pid];
      if (q) [q.x, q.y] = p;
    };
    const e = g.entity ? sk.entities[g.entity] : null;
    if (shape.kind === "point" && g.point) set(g.point, shape.p);
    else if (shape.kind === "line" && e?.type === "line") {
      set(e.p1, shape.a);
      set(e.p2, shape.b);
    } else if (shape.kind === "circle" && e?.type === "circle") {
      set(e.c, shape.c);
      e.r = shape.r;
    } else if (shape.kind === "arc" && e?.type === "arc") {
      set(e.c, shape.c);
      set(e.p1, shape.a);
      set(e.p2, shape.b);
    } else bad.push(r.id);
  }
  return bad;
}

/**
 * Drop references nothing uses: no relation, dimension or (non-reference) entity refers to
 * their geometry. References are created as soon as model geometry is picked or snapped to,
 * so the unused ones are cleaned up when the sketch is closed.
 */
export function pruneUnusedRefs(sk: SketchData) {
  if (!sk.refs) return;
  const used = new Set<Id>();
  for (const r of Object.values(sk.relations)) for (const [k, v] of Object.entries(r)) if (k !== "id" && k !== "type" && typeof v === "string") used.add(v);
  for (const d of Object.values(sk.dimensions)) d.refs.forEach((x) => used.add(x));
  for (const e of Object.values(sk.entities)) {
    if ("ext" in e && e.ext) continue;
    for (const v of Object.values(e)) if (typeof v === "string") used.add(v);
    if (e.type === "spline") e.pts.forEach((x) => used.add(x));
  }
  for (const r of Object.values(sk.refs)) {
    const ents = Object.values(sk.entities).filter((e) => "ext" in e && e.ext === r.id);
    const pts = Object.values(sk.points).filter((p) => p.ext === r.id);
    if (ents.some((e) => used.has(e.id)) || pts.some((p) => used.has(p.id))) continue;
    for (const e of ents) delete sk.entities[e.id];
    for (const p of pts) delete sk.points[p.id];
    delete sk.refs[r.id];
  }
  if (!Object.keys(sk.refs).length) delete sk.refs;
}

/** True for sketch geometry owned by an external reference. */
export function isExt(sk: SketchData, id: Id): boolean {
  const e = sk.entities[id];
  if (e) return "ext" in e && !!e.ext;
  return !!sk.points[id]?.ext;
}
