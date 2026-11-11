/**
 * Sketch editing operations (SOLIDWORKS Sketch toolbar): Offset, Mirror, Move/Copy,
 * Rotate, Scale, Linear/Circular Sketch Pattern, Extend and Sketch Chamfer.
 *
 * Every operation mutates the given SketchData (callers pass a clone for previews and
 * call app.sketchCheckpoint() before applying for real) and returns the ids it created.
 */
import type { Id, Relation, SketchData, SketchEntity, Vec2 } from "../core/types";
import { v2, lineLineIntersection, lineCircleIntersections, circleCircleIntersections } from "../core/math";
import { uid } from "../core/ids";
import { P, arcGeom, signedArea, sampleEntity, entitiesUsingPoint } from "./model";
import { entityPoints } from "./solver";
import { intersectEntities } from "./inference";

type Line = Extract<SketchEntity, { type: "line" }>;
type Arc = Extract<SketchEntity, { type: "arc" }>;
type Circle = Extract<SketchEntity, { type: "circle" }>;

const isCurve = (e: SketchEntity | undefined): e is Line | Arc | Circle => !!e && (e.type === "line" || e.type === "arc" || e.type === "circle");

function addPoint(sk: SketchData, p: Vec2): Id {
  const id = uid("p");
  sk.points[id] = { id, x: p[0], y: p[1] };
  return id;
}

function addRel(sk: SketchData, r: Omit<Relation, "id"> & { type: Relation["type"] }): Id {
  const id = uid("r");
  sk.relations[id] = { ...(r as object), id } as Relation;
  return id;
}

// ================================================================ chains

/** Entities connected end-to-end with the given ones (SOLIDWORKS "Select chain"). */
export function expandChain(sk: SketchData, ids: Id[]): Id[] {
  const out = new Set(ids.filter((id) => isCurve(sk.entities[id])));
  const stack = [...out];
  while (stack.length) {
    const e = sk.entities[stack.pop()!];
    if (!e || e.type === "circle" || e.type === "point") continue;
    for (const p of [e.p1, e.p2]) {
      for (const n of entitiesUsingPoint(sk, p)) {
        if (n.type === "point" || out.has(n.id) || n.id === e.id) continue;
        if (n.type === "circle") continue;
        if ((n as { construction?: boolean }).construction !== (e as { construction?: boolean }).construction) continue;
        if (n.type === "arc" && n.c === p) continue;
        out.add(n.id);
        stack.push(n.id);
      }
    }
  }
  return [...out];
}

interface ChainSeg {
  e: Line | Arc | Circle;
  /** Traversal from `from` to `to` (circles: from === to === ""). */
  from: Id;
  to: Id;
}

/** Order entities into chains (each chain a path or a closed loop). */
export function orderChains(sk: SketchData, ids: Id[]): { segs: ChainSeg[]; closed: boolean }[] {
  const ents = ids.map((id) => sk.entities[id]).filter(isCurve);
  const chains: { segs: ChainSeg[]; closed: boolean }[] = [];
  const left = new Set(ents.filter((e) => e.type !== "circle").map((e) => e.id));
  for (const e of ents) if (e.type === "circle") chains.push({ segs: [{ e, from: "", to: "" }], closed: true });
  const ends = (e: Line | Arc) => [e.p1, e.p2];
  const at = (p: Id) => ents.filter((e): e is Line | Arc => e.type !== "circle" && left.has(e.id) && ends(e).includes(p));
  while (left.size) {
    // Start at a path end if there is one (a point used once), else anywhere.
    const remaining = ents.filter((e): e is Line | Arc => e.type !== "circle" && left.has(e.id));
    let start = remaining[0];
    let from = start.p1;
    search: for (const e of remaining) {
      for (const p of ends(e)) {
        if (remaining.filter((x) => ends(x).includes(p)).length === 1) {
          start = e;
          from = p;
          break search;
        }
      }
    }
    const segs: ChainSeg[] = [];
    let cur: Line | Arc | undefined = start;
    let p = from;
    while (cur) {
      left.delete(cur.id);
      const to = cur.p1 === p ? cur.p2 : cur.p1;
      segs.push({ e: cur, from: p, to });
      p = to;
      cur = at(p)[0];
    }
    const closed = segs.length > 1 && segs[segs.length - 1].to === segs[0].from;
    chains.push({ segs, closed });
  }
  return chains;
}

// ================================================================ offset

export interface OffsetOptions {
  distance: number;
  reverse?: boolean;
  /** Offset to both sides. */
  bidirectional?: boolean;
  /** Close bi-directional offsets of open chains with arcs ("Cap ends: Arcs"). */
  capEnds?: boolean;
  /** Convert the original entities to construction geometry. */
  baseConstruction?: boolean;
  /** Add the offset dimension (default on). */
  addDimensions?: boolean;
  /** Pick the whole connected chain (default on). */
  selectChain?: boolean;
  /** Make the offset geometry construction. */
  offsetConstruction?: boolean;
}

/** Offset curve of one segment: positive d is to the left of the traversal direction. */
function offsetSeg(sk: SketchData, s: ChainSeg, d: number): { kind: "line"; a: Vec2; b: Vec2 } | { kind: "circle"; c: Vec2; r: number; a?: Vec2; b?: Vec2 } | null {
  const e = s.e;
  if (e.type === "line") {
    const a = P(sk, s.from), b = P(sk, s.to);
    const n = v2.perp(v2.norm(v2.sub(b, a)));
    return { kind: "line", a: v2.add(a, v2.scale(n, d)), b: v2.add(b, v2.scale(n, d)) };
  }
  if (e.type === "circle") {
    const r = e.r + d; // circles: positive = outward
    return r > 1e-9 ? { kind: "circle", c: P(sk, e.c), r } : null;
  }
  const g = arcGeom(sk, e);
  const c = P(sk, e.c);
  // Arc stored CCW p1→p2. Travelling CCW, left is towards the centre.
  const ccw = s.from === e.p1;
  const r = ccw ? g.r - d : g.r + d;
  if (r <= 1e-9) return null;
  const a = v2.add(c, v2.scale(v2.norm(v2.sub(P(sk, s.from), c)), r));
  const b = v2.add(c, v2.scale(v2.norm(v2.sub(P(sk, s.to), c)), r));
  return { kind: "circle", c, r, a, b };
}

function intersectOffset(A: NonNullable<ReturnType<typeof offsetSeg>>, B: NonNullable<ReturnType<typeof offsetSeg>>, near: Vec2): Vec2 {
  let pts: Vec2[] = [];
  if (A.kind === "line" && B.kind === "line") {
    const x = lineLineIntersection(A.a, A.b, B.a, B.b);
    pts = x ? [x] : [];
  } else if (A.kind === "line" && B.kind === "circle") pts = lineCircleIntersections(A.a, A.b, B.c, B.r);
  else if (A.kind === "circle" && B.kind === "line") pts = lineCircleIntersections(B.a, B.b, A.c, A.r);
  else if (A.kind === "circle" && B.kind === "circle") pts = circleCircleIntersections(A.c, A.r, B.c, B.r);
  if (!pts.length) return near;
  return pts.reduce((best, p) => (v2.dist(p, near) < v2.dist(best, near) ? p : best));
}

/** Signed area of a closed chain (positive = counter-clockwise traversal). */
function chainArea(sk: SketchData, segs: ChainSeg[]) {
  const poly: Vec2[] = [];
  for (const s of segs) {
    const pts = sampleEntity(sk, s.e, 16);
    const fwd = s.e.type === "line" ? s.from === s.e.p1 : s.e.type === "arc" ? s.from === s.e.p1 : true;
    poly.push(...(fwd ? pts : pts.slice().reverse()).slice(0, -1));
  }
  return signedArea(poly);
}

/**
 * Offset Entities. Returns the new entity ids, or an error message when the offset
 * cannot be built (e.g. an arc radius would become zero).
 */
export function offsetEntities(sk: SketchData, ids: Id[], o: OffsetOptions): { created: Id[]; error?: string } {
  const sel = o.selectChain === false ? ids : expandChain(sk, ids);
  const chains = orderChains(sk, sel);
  if (!chains.length) return { created: [], error: "Select sketch entities to offset." };
  const created: Id[] = [];
  const sides = o.bidirectional ? [1, -1] : [1];
  for (const side of sides) {
    for (const ch of chains) {
      // Positive offsets go outward on closed chains and to the left on open ones.
      let d = o.distance * side * (o.reverse ? -1 : 1);
      if (ch.closed && ch.segs[0].e.type !== "circle") d *= chainArea(sk, ch.segs) > 0 ? -1 : 1;
      const res = offsetChain(sk, ch.segs, ch.closed, d, o);
      if ("error" in res) return { created, error: res.error };
      created.push(...res.created);
      if (o.bidirectional && o.capEnds && !ch.closed && side === -1) created.push(...capEnds(sk, ch.segs, res.ends, o.distance));
    }
  }
  if (o.baseConstruction) for (const id of sel) (sk.entities[id] as { construction?: boolean }).construction = true;
  return { created };
}

function offsetChain(sk: SketchData, segs: ChainSeg[], closed: boolean, d: number, o: OffsetOptions): { created: Id[]; ends: [Id, Id] } | { error: string } {
  const curves = segs.map((s) => offsetSeg(sk, s, d));
  if (curves.some((c) => !c)) return { error: "The offset distance is too large for the selected arcs or circles." };
  const created: Id[] = [];
  const construction = !!o.offsetConstruction;
  // Circles stand alone.
  if (segs.length === 1 && segs[0].e.type === "circle") {
    const src = segs[0].e as Circle;
    const cur = curves[0] as { c: Vec2; r: number };
    const cid = addPoint(sk, cur.c);
    const id = uid("e");
    sk.entities[id] = { id, type: "circle", c: cid, r: cur.r, construction };
    addRel(sk, { type: "concentric", e1: src.id, e2: id } as never);
    if (o.addDimensions !== false) addOffsetDim(sk, src.id, id, Math.abs(d));
    created.push(id);
    return { created, ends: ["", ""] };
  }
  // Joint points: intersection of neighbouring offset curves near the shifted original joint.
  const n = segs.length;
  const joint: Id[] = [];
  const startPt = (i: number): Vec2 => {
    const c = curves[i]!;
    return c.kind === "line" ? c.a : c.a!;
  };
  const endPt = (i: number): Vec2 => {
    const c = curves[i]!;
    return c.kind === "line" ? c.b : c.b!;
  };
  for (let i = 0; i <= n; i++) {
    if (i === 0 && !closed) joint.push(addPoint(sk, startPt(0)));
    else if (i === n && !closed) joint.push(addPoint(sk, endPt(n - 1)));
    else if (i === n) joint.push(joint[0]);
    else {
      const prev = (i - 1 + n) % n;
      const near = v2.lerp(endPt(prev), startPt(i), 0.5);
      const pt = v2.dist(endPt(prev), startPt(i)) < 1e-9 ? near : intersectOffset(curves[prev]!, curves[i]!, near);
      joint.push(addPoint(sk, pt));
    }
  }
  let master: { src: Id; dst: Id } | null = null;
  const pairs: { src: Id; dst: Id; kind: "line" | "arc" }[] = [];
  segs.forEach((s, i) => {
    const a = joint[i], b = joint[i + 1];
    const id = uid("e");
    if (s.e.type === "line") {
      sk.entities[id] = { id, type: "line", p1: s.from === s.e.p1 ? a : b, p2: s.from === s.e.p1 ? b : a, construction };
      addRel(sk, { type: "parallel", e1: s.e.id, e2: id } as never);
      pairs.push({ src: s.e.id, dst: id, kind: "line" });
    } else if (s.e.type === "arc") {
      const cid = addPoint(sk, P(sk, s.e.c));
      const ccw = s.from === s.e.p1;
      sk.entities[id] = { id, type: "arc", c: cid, p1: ccw ? a : b, p2: ccw ? b : a, construction };
      addRel(sk, { type: "concentric", e1: s.e.id, e2: id } as never);
      pairs.push({ src: s.e.id, dst: id, kind: "arc" });
    }
    created.push(id);
  });
  // One dimension drives the offset; the rest follow through offset relations.
  const m = pairs.find((p) => p.kind === "line") ?? pairs[0];
  master = { src: m.src, dst: m.dst };
  if (o.addDimensions !== false) addOffsetDim(sk, master.src, master.dst, Math.abs(d));
  for (const p of pairs) {
    if (p === m) continue;
    addRel(sk, { type: "offset", e1: p.src, e2: p.dst, m1: master.src, m2: master.dst } as never);
  }
  return { created, ends: [joint[0], joint[n]] };
}

function addOffsetDim(sk: SketchData, src: Id, dst: Id, value: number) {
  const a = sk.entities[src], b = sk.entities[dst];
  const id = uid("d");
  const name = `D${sk.nextDim++}`;
  if (a.type === "line" && b.type === "line") {
    const m = v2.lerp(P(sk, a.p1), P(sk, a.p2), 0.5);
    const mb = v2.lerp(P(sk, b.p1), P(sk, b.p2), 0.5);
    sk.dimensions[id] = { id, name, kind: "lineLine", refs: [src, dst], value, label: v2.lerp(m, mb, 0.5) };
  } else {
    const c = a.type === "circle" || a.type === "arc" ? P(sk, a.c) : [0, 0];
    const r = b.type === "circle" ? b.r : b.type === "arc" ? arcGeom(sk, b).r : 0;
    sk.dimensions[id] = { id, name, kind: "offset", refs: [src, dst], value, label: v2.add(c as Vec2, [r + 2, r * 0.3]) };
  }
}

/** Arcs joining the two sides of a bi-directional offset of an open chain. */
function capEnds(sk: SketchData, segs: ChainSeg[], ends: [Id, Id], dist: number): Id[] {
  void dist;
  const out: Id[] = [];
  // The positive side was created just before this call: find its open ends by position.
  const first = segs[0], last = segs[segs.length - 1];
  for (const [orig, endId] of [[first.from, ends[0]], [last.to, ends[1]]] as [Id, Id][]) {
    const c = P(sk, orig);
    const q = P(sk, endId);
    // Partner point: mirror of q through the original end.
    const partner = Object.values(sk.points).find((p) => p.id !== endId && v2.dist([p.x, p.y], v2.sub(v2.scale(c, 2), q)) < 1e-6);
    if (!partner) continue;
    const cid = addPoint(sk, c);
    const id = uid("e");
    // Choose orientation so the cap bulges away from the chain.
    const away = orig === first.from ? v2.sub(P(sk, orig), P(sk, first.to)) : v2.sub(P(sk, orig), P(sk, last.from));
    const ccwMid = v2.add(c, v2.scale(v2.norm(v2.perp(v2.sub(q, c))), v2.dist(q, c)));
    const ccw = v2.dot(v2.sub(ccwMid, c), away) > 0;
    sk.entities[id] = { id, type: "arc", c: cid, p1: ccw ? endId : partner.id, p2: ccw ? partner.id : endId };
    out.push(id);
  }
  return out;
}

// ================================================================ copy / transform

/**
 * Copy (or move) entities through a point mapping. `copy` keeps the originals;
 * otherwise the originals' points are moved in place. Returns the ids of the
 * resulting entities (copies, or the moved originals) and the point map.
 */
export function transformEntities(sk: SketchData, ids: Id[], f: (p: Vec2) => Vec2, copy: boolean, opts: { scale?: number; mirror?: boolean } = {}): { created: Id[]; pointMap: Map<Id, Id> } {
  const ents = ids.map((id) => sk.entities[id]).filter((e): e is SketchEntity => !!e);
  const pts = new Set<Id>();
  for (const e of ents) entityPoints(e).forEach((p) => pts.add(p));
  const map = new Map<Id, Id>();
  const created: Id[] = [];
  for (const p of pts) {
    const q = f(P(sk, p));
    if (copy) map.set(p, addPoint(sk, q));
    else {
      if (p === "origin") continue;
      sk.points[p].x = q[0];
      sk.points[p].y = q[1];
      map.set(p, p);
    }
  }
  const scale = opts.scale ?? 1;
  for (const e of ents) {
    const M = (p: Id) => map.get(p) ?? p;
    if (!copy) {
      if (e.type === "circle") e.r *= scale;
      if (e.type === "arc" && opts.mirror) [e.p1, e.p2] = [e.p2, e.p1];
      created.push(e.id);
      continue;
    }
    const id = uid("e");
    const c = (e as { construction?: boolean }).construction;
    if (e.type === "line") sk.entities[id] = { id, type: "line", p1: M(e.p1), p2: M(e.p2), construction: c };
    else if (e.type === "circle") sk.entities[id] = { id, type: "circle", c: M(e.c), r: e.r * scale, construction: c };
    else if (e.type === "arc") sk.entities[id] = opts.mirror ? { id, type: "arc", c: M(e.c), p1: M(e.p2), p2: M(e.p1), construction: c } : { id, type: "arc", c: M(e.c), p1: M(e.p1), p2: M(e.p2), construction: c };
    else if (e.type === "point") {
      sk.points[M(e.p)].standalone = true;
      sk.entities[id] = { id, type: "point", p: M(e.p) };
    }
    created.push(id);
  }
  return { created, pointMap: map };
}

/** Move Entities: drop relations that tie moved geometry to fixed geometry (unless kept). */
export function detachMoved(sk: SketchData, ids: Id[]) {
  const moving = new Set<Id>(ids);
  for (const id of ids) {
    const e = sk.entities[id];
    if (e) entityPoints(e).forEach((p) => moving.add(p));
  }
  for (const r of Object.values(sk.relations)) {
    const refs = Object.entries(r).filter(([k]) => !["id", "type", "inner"].includes(k)).map(([, v]) => v as Id);
    const inside = refs.filter((x) => moving.has(x)).length;
    if (inside > 0 && inside < refs.length) delete sk.relations[r.id];
  }
  for (const d of Object.values(sk.dimensions)) {
    const inside = d.refs.filter((x) => moving.has(x)).length;
    if (inside > 0 && inside < d.refs.length) delete sk.dimensions[d.id];
  }
  // A moved endpoint shared with an unmoved entity is split off.
  for (const p of [...moving]) {
    if (!sk.points[p] || p === "origin") continue;
    const users = entitiesUsingPoint(sk, p);
    const out = users.filter((e) => !moving.has(e.id));
    if (out.length && users.some((e) => moving.has(e.id))) {
      const nid = addPoint(sk, P(sk, p));
      for (const e of out) {
        const ee = e as Record<string, unknown>;
        for (const k of ["p1", "p2", "c", "p"]) if (ee[k] === p) ee[k] = nid;
      }
    }
  }
}

export const xf = {
  translate: (d: Vec2) => (p: Vec2): Vec2 => v2.add(p, d),
  rotate: (c: Vec2, ang: number) => (p: Vec2): Vec2 => {
    const s = Math.sin(ang), k = Math.cos(ang);
    const q = v2.sub(p, c);
    return [c[0] + q[0] * k - q[1] * s, c[1] + q[0] * s + q[1] * k];
  },
  scale: (c: Vec2, f: number) => (p: Vec2): Vec2 => v2.add(c, v2.scale(v2.sub(p, c), f)),
  mirror: (a: Vec2, b: Vec2) => (p: Vec2): Vec2 => {
    const u = v2.norm(v2.sub(b, a));
    const q = v2.sub(p, a);
    const along = v2.scale(u, v2.dot(q, u));
    return v2.add(a, v2.sub(v2.scale(along, 2), q));
  },
};

// ================================================================ mirror

/**
 * Mirror Entities about a line (centerline or line). With `copy` (SOLIDWORKS default)
 * the originals stay and symmetric relations tie each copy to its original.
 */
export function mirrorEntities(sk: SketchData, ids: Id[], axis: Id, copy = true): { created: Id[]; error?: string } {
  const ax = sk.entities[axis];
  if (!ax || ax.type !== "line") return { created: [], error: "Select a line or centerline to mirror about." };
  const a = P(sk, ax.p1), b = P(sk, ax.p2);
  const list = ids.filter((id) => id !== axis && sk.entities[id]);
  const { created, pointMap } = transformEntities(sk, list, xf.mirror(a, b), copy, { mirror: true });
  if (copy) {
    for (const [src, dst] of pointMap) {
      if (src === dst) continue;
      // Points lying on the axis stay shared instead of being doubled.
      const onAxis = Math.abs(v2.cross(v2.norm(v2.sub(b, a)), v2.sub(P(sk, src), a))) < 1e-9;
      if (onAxis) {
        mergePoint(sk, dst, src);
        continue;
      }
      addRel(sk, { type: "symmetric", p1: src, p2: dst, axis } as never);
    }
    list.forEach((id, i) => {
      const s = sk.entities[id], c = sk.entities[created[i]];
      if (s?.type === "circle" && c?.type === "circle") addRel(sk, { type: "equal", e1: id, e2: c.id } as never);
    });
  }
  return { created };
}

function mergePoint(sk: SketchData, from: Id, into: Id) {
  for (const e of Object.values(sk.entities)) {
    const ee = e as Record<string, unknown>;
    for (const k of ["p1", "p2", "c", "p"]) if (ee[k] === from) ee[k] = into;
  }
  delete sk.points[from];
}

// ================================================================ patterns

export interface LinearPatternOptions {
  /** Direction 1 angle (radians) and spacing / count (count includes the seed). */
  angle1: number;
  spacing1: number;
  count1: number;
  angle2?: number;
  spacing2?: number;
  count2?: number;
  /** Instance indices to skip, as "i,j". */
  skip?: string[];
}

export function linearPattern(sk: SketchData, ids: Id[], o: LinearPatternOptions): { created: Id[] } {
  const created: Id[] = [];
  const n2 = Math.max(1, o.count2 ?? 1);
  const d1 = v2.fromAngle(o.angle1, o.spacing1);
  const d2 = v2.fromAngle(o.angle2 ?? o.angle1 + Math.PI / 2, o.spacing2 ?? o.spacing1);
  for (let i = 0; i < Math.max(1, o.count1); i++) {
    for (let j = 0; j < n2; j++) {
      if (i === 0 && j === 0) continue;
      if (o.skip?.includes(`${i},${j}`)) continue;
      const off = v2.add(v2.scale(d1, i), v2.scale(d2, j));
      const r = transformEntities(sk, ids, xf.translate(off), true);
      tieCopies(sk, ids, r.created);
      created.push(...r.created);
    }
  }
  return { created };
}

export interface CircularPatternOptions {
  center: Vec2;
  /** Total angle (radians). With `equalSpacing` instances are spread over it. */
  angle: number;
  count: number;
  equalSpacing?: boolean;
  reverse?: boolean;
  skip?: number[];
}

export function circularPattern(sk: SketchData, ids: Id[], o: CircularPatternOptions): { created: Id[] } {
  const created: Id[] = [];
  const n = Math.max(1, o.count);
  const full = Math.abs(Math.abs(o.angle) - 2 * Math.PI) < 1e-9;
  const step = o.equalSpacing === false ? o.angle : o.angle / (full ? n : Math.max(1, n - 1));
  const s = o.reverse ? -1 : 1;
  for (let i = 1; i < n; i++) {
    if (o.skip?.includes(i)) continue;
    const r = transformEntities(sk, ids, xf.rotate(o.center, s * step * i), true);
    tieCopies(sk, ids, r.created);
    created.push(...r.created);
  }
  return { created };
}

/** Pattern instances keep the size of their seed (equal relations), as SOLIDWORKS adds. */
function tieCopies(sk: SketchData, seeds: Id[], copies: Id[]) {
  seeds.forEach((id, i) => {
    const a = sk.entities[id], b = sk.entities[copies[i]];
    if (!a || !b || a.type === "point") return;
    if ((a.type === "circle" || a.type === "arc" || a.type === "line") && a.type === b.type) addRel(sk, { type: "equal", e1: a.id, e2: b.id } as never);
  });
}

// ================================================================ extend

/**
 * Extend Entities: lengthen a line or arc from the end nearest `near` to the closest
 * intersection with another entity. Returns false if nothing is in the way.
 */
export function extendEntity(sk: SketchData, id: Id, near: Vec2): boolean {
  const e = sk.entities[id];
  if (!e || (e.type !== "line" && e.type !== "arc")) return false;
  const end = v2.dist(P(sk, e.p1), near) <= v2.dist(P(sk, e.p2), near) ? e.p1 : e.p2;
  const other = end === e.p1 ? e.p2 : e.p1;
  // A shared endpoint is detached so only this entity grows.
  if (entitiesUsingPoint(sk, end).length > 1) return false;
  const E = P(sk, end);
  let best: Vec2 | null = null;
  let bestD = Infinity;
  if (e.type === "line") {
    const O = P(sk, other);
    const u = v2.norm(v2.sub(E, O));
    const far = v2.add(E, v2.scale(u, 1e6));
    const probe = uid("tmp");
    const pa = uid("tmp"), pb = uid("tmp");
    sk.points[pa] = { id: pa, x: E[0], y: E[1] };
    sk.points[pb] = { id: pb, x: far[0], y: far[1] };
    sk.entities[probe] = { id: probe, type: "line", p1: pa, p2: pb };
    for (const x of Object.values(sk.entities)) {
      if (x.id === probe || x.id === id || x.type === "point") continue;
      for (const q of intersectEntities(sk, probe, x.id)) {
        const t = v2.dot(v2.sub(q, E), u);
        if (t > 1e-6 && t < bestD) {
          bestD = t;
          best = q;
        }
      }
    }
    delete sk.entities[probe];
    delete sk.points[pa];
    delete sk.points[pb];
  } else {
    const g = arcGeom(sk, e);
    const c = P(sk, e.c);
    const circ = uid("tmp"), cc = uid("tmp");
    sk.points[cc] = { id: cc, x: c[0], y: c[1] };
    sk.entities[circ] = { id: circ, type: "circle", c: cc, r: g.r };
    const a0 = Math.atan2(E[1] - c[1], E[0] - c[0]);
    // Extending p2 continues counter-clockwise; extending p1 continues clockwise.
    const dir = end === e.p2 ? 1 : -1;
    for (const x of Object.values(sk.entities)) {
      if (x.id === circ || x.id === id || x.type === "point") continue;
      for (const q of intersectEntities(sk, circ, x.id)) {
        let t = (Math.atan2(q[1] - c[1], q[0] - c[0]) - a0) * dir;
        while (t <= 1e-9) t += 2 * Math.PI;
        // Never sweep past the other end.
        if (t < bestD && t < 2 * Math.PI - Math.abs(g.sweep) - 1e-9) {
          bestD = t;
          best = q;
        }
      }
    }
    delete sk.entities[circ];
    delete sk.points[cc];
  }
  if (!best) return false;
  sk.points[end].x = best[0];
  sk.points[end].y = best[1];
  return true;
}

// ================================================================ chamfer

export interface ChamferOptions {
  type: "angleDistance" | "distanceDistance";
  d1: number;
  /** Second distance (distance-distance) or angle in radians (angle-distance). */
  d2: number;
  equal?: boolean;
}

/** Sketch Chamfer at the corner `v` shared by lines l1 and l2. */
export function chamferCorner(sk: SketchData, l1: Id, l2: Id, v: Id, o: ChamferOptions): { created?: Id; error?: string } {
  const a = sk.entities[l1], b = sk.entities[l2];
  if (a?.type !== "line" || b?.type !== "line") return { error: "Select two lines that meet at a corner." };
  const V = P(sk, v);
  const A = P(sk, a.p1 === v ? a.p2 : a.p1), B = P(sk, b.p1 === v ? b.p2 : b.p1);
  const ua = v2.norm(v2.sub(A, V)), ub = v2.norm(v2.sub(B, V));
  const da = o.d1;
  let db = o.type === "distanceDistance" ? (o.equal ? o.d1 : o.d2) : 0;
  if (o.type === "angleDistance") {
    const corner = Math.acos(Math.max(-1, Math.min(1, v2.dot(ua, ub))));
    const third = Math.PI - corner - o.d2;
    if (third <= 0) return { error: "The chamfer angle is too large for this corner." };
    db = (da * Math.sin(o.d2)) / Math.sin(third);
  }
  if (da >= v2.dist(A, V) - 1e-9 || db >= v2.dist(B, V) - 1e-9) return { error: "The chamfer is too large for the selected entities." };
  const pa = addPoint(sk, v2.add(V, v2.scale(ua, da)));
  const pb = addPoint(sk, v2.add(V, v2.scale(ub, db)));
  const shared = entitiesUsingPoint(sk, v).length > 2;
  for (const [ln, np] of [[a, pa], [b, pb]] as [Line, Id][]) {
    if (ln.p1 === v) ln.p1 = np;
    else ln.p2 = np;
  }
  // Relations/dimensions on the corner point would now dangle.
  if (!shared && v !== "origin") {
    for (const r of Object.values(sk.relations)) if (Object.values(r).includes(v)) delete sk.relations[r.id];
    for (const d of Object.values(sk.dimensions)) if (d.refs.includes(v)) delete sk.dimensions[d.id];
    if (!entitiesUsingPoint(sk, v).length) delete sk.points[v];
  }
  const id = uid("e");
  sk.entities[id] = { id, type: "line", p1: pa, p2: pb };
  return { created: id };
}
