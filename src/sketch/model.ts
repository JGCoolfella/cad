/** Sketch data helpers: construction, geometry queries and closed-profile extraction. */
import type { Id, SketchData, SketchEntity, Vec2 } from "../core/types";
import { v2, wrap2Pi, closestOnSegment, angleInArc } from "../core/math";
import { ORIGIN_ID, entityPoints } from "./solver";

export function emptySketch(): SketchData {
  return {
    points: { [ORIGIN_ID]: { id: ORIGIN_ID, x: 0, y: 0 } },
    entities: {},
    relations: {},
    dimensions: {},
    nextDim: 1,
  };
}

export function P(sk: SketchData, id: Id): Vec2 {
  const p = sk.points[id];
  return [p.x, p.y];
}

export function arcGeom(sk: SketchData, e: Extract<SketchEntity, { type: "arc" }>) {
  const c = P(sk, e.c), a = P(sk, e.p1), b = P(sk, e.p2);
  const r = v2.dist(c, a);
  const a0 = Math.atan2(a[1] - c[1], a[0] - c[0]);
  let a1 = Math.atan2(b[1] - c[1], b[0] - c[0]);
  let sweep = wrap2Pi(a1 - a0);
  if (sweep < 1e-9) sweep = 2 * Math.PI;
  a1 = a0 + sweep;
  const mid = v2.add(c, v2.fromAngle(a0 + sweep / 2, r));
  return { c, r, a0, a1, sweep, mid, start: a, end: b };
}

/**
 * Smooth curve through points (centripetal Catmull-Rom, natural ends), `per` samples
 * per span. Shared by the sketch, the solver, profiles and the kernel.
 */
export function splineThrough(pts: Vec2[], per = 16): Vec2[] {
  if (pts.length < 2) return pts.slice();
  if (pts.length === 2) return [pts[0], pts[1]];
  const ext = [v2.add(pts[0], v2.sub(pts[0], pts[1])), ...pts, v2.add(pts[pts.length - 1], v2.sub(pts[pts.length - 1], pts[pts.length - 2]))];
  const out: Vec2[] = [];
  for (let i = 1; i < ext.length - 2; i++) {
    const [p0, p1, p2, p3] = [ext[i - 1], ext[i], ext[i + 1], ext[i + 2]];
    const t01 = Math.pow(v2.dist(p0, p1), 0.5) || 1e-6, t12 = Math.pow(v2.dist(p1, p2), 0.5) || 1e-6, t23 = Math.pow(v2.dist(p2, p3), 0.5) || 1e-6;
    // Tangents of the centripetal parameterisation (Barry–Goldman), scaled to the span.
    const m1 = v2.scale(v2.add(v2.sub(v2.scale(v2.sub(p1, p0), 1 / t01), v2.scale(v2.sub(p2, p0), 1 / (t01 + t12))), v2.scale(v2.sub(p2, p1), 1 / t12)), t12);
    const m2 = v2.scale(v2.add(v2.sub(v2.scale(v2.sub(p2, p1), 1 / t12), v2.scale(v2.sub(p3, p1), 1 / (t12 + t23))), v2.scale(v2.sub(p3, p2), 1 / t23)), t12);
    for (let k = 0; k < per; k++) {
      const t = k / per, t2 = t * t, t3 = t2 * t;
      const h00 = 2 * t3 - 3 * t2 + 1, h10 = t3 - 2 * t2 + t, h01 = -2 * t3 + 3 * t2, h11 = t3 - t2;
      out.push([h00 * p1[0] + h10 * m1[0] + h01 * p2[0] + h11 * m2[0], h00 * p1[1] + h10 * m1[1] + h01 * p2[1] + h11 * m2[1]]);
    }
  }
  out.push(pts[pts.length - 1]);
  return out;
}

/** Polyline approximation of an entity in sketch coordinates. */
export function sampleEntity(sk: SketchData, e: SketchEntity, seg = 64): Vec2[] {
  switch (e.type) {
    case "spline":
      return splineThrough(e.pts.map((p) => P(sk, p)), Math.max(4, Math.round(seg / 4)));
    case "line":
      return [P(sk, e.p1), P(sk, e.p2)];
    case "point":
      return [P(sk, e.p)];
    case "circle": {
      const c = P(sk, e.c);
      const out: Vec2[] = [];
      for (let i = 0; i <= seg; i++) out.push(v2.add(c, v2.fromAngle((i / seg) * 2 * Math.PI, e.r)));
      return out;
    }
    case "arc": {
      const g = arcGeom(sk, e);
      const n = Math.max(4, Math.ceil((seg * g.sweep) / (2 * Math.PI)));
      const out: Vec2[] = [];
      for (let i = 0; i <= n; i++) out.push(v2.add(g.c, v2.fromAngle(g.a0 + (g.sweep * i) / n, g.r)));
      return out;
    }
  }
}

/** Closest point on an entity curve. */
export function closestOnEntity(sk: SketchData, e: SketchEntity, p: Vec2): { point: Vec2; dist: number; t?: number } | null {
  switch (e.type) {
    case "line": {
      const r = closestOnSegment(p, P(sk, e.p1), P(sk, e.p2));
      return { point: r.point, dist: v2.dist(p, r.point), t: r.t };
    }
    case "circle": {
      const c = P(sk, e.c);
      const d = v2.sub(p, c);
      const l = v2.len(d) || 1e-12;
      const q = v2.add(c, v2.scale(d, e.r / l));
      return { point: q, dist: Math.abs(l - e.r) };
    }
    case "arc": {
      const g = arcGeom(sk, e);
      const t = Math.atan2(p[1] - g.c[1], p[0] - g.c[0]);
      if (angleInArc(t, g.a0, g.a1)) {
        const q = v2.add(g.c, v2.fromAngle(t, g.r));
        return { point: q, dist: v2.dist(p, q) };
      }
      const da = v2.dist(p, g.start), db = v2.dist(p, g.end);
      return da < db ? { point: g.start, dist: da } : { point: g.end, dist: db };
    }
    case "point":
      return { point: P(sk, e.p), dist: v2.dist(p, P(sk, e.p)) };
    case "spline": {
      const pts = sampleEntity(sk, e, 128);
      let best: { point: Vec2; dist: number } = { point: pts[0], dist: Infinity };
      for (let i = 0; i + 1 < pts.length; i++) {
        const r = closestOnSegment(p, pts[i], pts[i + 1]);
        const d = v2.dist(p, r.point);
        if (d < best.dist) best = { point: r.point, dist: d };
      }
      return best;
    }
  }
}

/** Entities that use a given point. */
export function entitiesUsingPoint(sk: SketchData, pid: Id): SketchEntity[] {
  return Object.values(sk.entities).filter((e) => entityPoints(e).includes(pid));
}

/** Delete entities plus orphaned points, relations and dimensions that reference them. */
export function deleteFromSketch(sk: SketchData, ids: Set<Id>) {
  const pointIds = new Set<Id>();
  for (const id of ids) {
    const e = sk.entities[id];
    if (e) {
      delete sk.entities[id];
      entityPoints(e).forEach((p) => pointIds.add(p));
    }
    if (sk.relations[id]) delete sk.relations[id];
    if (sk.dimensions[id]) delete sk.dimensions[id];
    if (sk.points[id] && id !== ORIGIN_ID) pointIds.add(id);
  }
  // Remove points no longer used by any entity.
  const used = new Set<Id>([ORIGIN_ID]);
  for (const e of Object.values(sk.entities)) entityPoints(e).forEach((p) => used.add(p));
  const removed = new Set<Id>([...ids]);
  for (const p of pointIds) {
    if (!used.has(p)) {
      delete sk.points[p];
      removed.add(p);
    }
  }
  pruneRefs(sk, removed);
  pruneVirtualPoints(sk);
}

/** Remove midpoint helper points that nothing (but their own midpoint relation) uses. */
export function pruneVirtualPoints(sk: SketchData) {
  for (const p of Object.values(sk.points)) {
    if (!p.virtual) continue;
    const rels = Object.values(sk.relations).filter((r) => Object.values(r).includes(p.id));
    const own = rels.filter((r) => r.type === "midpoint" && r.p === p.id);
    const line = own[0] && sk.entities[(own[0] as { e: Id }).e];
    const usedByDim = Object.values(sk.dimensions).some((d) => d.refs.includes(p.id));
    if (line && (rels.length > own.length || usedByDim)) continue;
    for (const r of own) delete sk.relations[r.id];
    delete sk.points[p.id];
  }
}

/** Drop relations / dimensions that reference anything no longer present. */
export function pruneRefs(sk: SketchData, removed?: Set<Id>) {
  const exists = (id: Id) => !!sk.points[id] || !!sk.entities[id];
  for (const r of Object.values(sk.relations)) {
    const refs = Object.entries(r).filter(([k]) => !["id", "type", "inner"].includes(k)).map(([, v]) => v as Id);
    if (refs.some((x) => !exists(x) || removed?.has(x))) delete sk.relations[r.id];
  }
  for (const d of Object.values(sk.dimensions)) {
    if (d.refs.some((x) => !exists(x) || removed?.has(x))) delete sk.dimensions[d.id];
  }
}

// ---------------------------------------------------------------- profiles

export type ProfileSeg =
  | { type: "line"; a: Vec2; b: Vec2 }
  | { type: "arc"; a: Vec2; b: Vec2; mid: Vec2 }
  | { type: "circle"; c: Vec2; r: number }
  /** Spline as sampled points from a to b (inclusive). */
  | { type: "spline"; a: Vec2; b: Vec2; pts: Vec2[] };

export interface Loop {
  segs: ProfileSeg[];
  poly: Vec2[];
  area: number; // signed (CCW positive)
  /** Sketch entities that make up the loop. */
  ents: Id[];
}

export interface Region {
  outer: Loop;
  holes: Loop[];
}

export interface ProfileResult {
  regions: Region[];
  /** Open (non-closed) chain endpoints count, for "open profile" diagnostics. */
  openEnds: number;
  /**
   * Smallest closed areas of the sketch (each bounded face of the sketch graph, plus
   * circles), used for Shaded Sketch Contours and region selection.
   */
  faces: Loop[];
}

interface HalfEdge {
  from: Id;
  to: Id;
  ent: SketchEntity;
  /** Outgoing tangent angle at `from`. */
  angle: number;
  used: boolean;
  twin?: HalfEdge;
}

/**
 * Extract closed regions from the non-construction geometry of a sketch.
 * Each connected component contributes its outer boundary; nested boundaries
 * alternate between solid and hole (even-odd), matching default extrude behaviour.
 */
export function extractProfiles(sk: SketchData): ProfileResult {
  const loops: Loop[] = [];
  const out: HalfEdge[] = [];
  const byNode = new Map<Id, HalfEdge[]>();
  const add = (h: HalfEdge) => {
    out.push(h);
    if (!byNode.has(h.from)) byNode.set(h.from, []);
    byNode.get(h.from)!.push(h);
  };
  for (const e of Object.values(sk.entities)) {
    if ((e as { construction?: boolean }).construction) continue;
    if (e.type === "circle") {
      const c = P(sk, e.c);
      const poly = sampleEntity(sk, e, 96);
      loops.push({ segs: [{ type: "circle", c, r: e.r }], poly, area: Math.PI * e.r * e.r, ents: [e.id] });
    } else if (e.type === "line" || e.type === "arc" || e.type === "spline") {
      const e1 = e.type === "spline" ? e.pts[0] : e.p1, e2 = e.type === "spline" ? e.pts[e.pts.length - 1] : e.p2;
      if (e1 === e2) continue;
      const [ta, tb] = endTangents(sk, e);
      const h1: HalfEdge = { from: e1, to: e2, ent: e, angle: ta, used: false };
      const h2: HalfEdge = { from: e2, to: e1, ent: e, angle: tb, used: false };
      h1.twin = h2;
      h2.twin = h1;
      add(h1);
      add(h2);
    }
  }
  let openEnds = 0;
  for (const [, hs] of byNode) if (hs.length === 1) openEnds++;
  // Prune dangling chains so they don't break the loops they hang off.
  let pruned = true;
  while (pruned) {
    pruned = false;
    for (const [node, hs] of byNode) {
      if (hs.length !== 1) continue;
      const h = hs[0];
      byNode.delete(node);
      const other = byNode.get(h.to);
      if (other) other.splice(other.indexOf(h.twin!), 1);
      h.used = h.twin!.used = true;
      pruned = true;
    }
  }
  for (const [, hs] of byNode) hs.sort((a, b) => a.angle - b.angle);
  const faces: Loop[] = loops.slice();
  // Face traversal: at each node take the next edge clockwise from the reverse of the incoming edge.
  for (const h0 of out) {
    if (h0.used) continue;
    const cycle: HalfEdge[] = [];
    let h: HalfEdge | undefined = h0;
    let guard = 0;
    while (h && !h.used && guard++ < 10000) {
      h.used = true;
      cycle.push(h);
      const hs: HalfEdge[] = byNode.get(h.to) ?? [];
      const twin: HalfEdge = h.twin!;
      const i = hs.indexOf(twin);
      // next half-edge in angular order before twin (i.e. turning most to the left gives faces CCW)
      h = hs.length ? hs[(i - 1 + hs.length) % hs.length] : undefined;
      if (h === h0) break;
    }
    if (h !== h0 || cycle.length === 0) continue;
    // Dangling edges (traversed both ways in the same cycle) make the loop degenerate.
    const ents = new Set(cycle.map((c) => c.ent.id));
    if (ents.size !== cycle.length) continue;
    const segs: ProfileSeg[] = cycle.map((c) => toSeg(sk, c));
    const poly = segsToPoly(segs);
    const area = signedArea(poly);
    const entIds = [...ents];
    // Bounded faces come out CCW (positive). Negative = outer boundary of the component.
    if (area > 0) faces.push({ segs, poly, area, ents: entIds });
    else if (area < 0) {
      // Reverse to CCW for the outer loop representation.
      const rev = reverseSegs(segs);
      loops.push({ segs: rev, poly: segsToPoly(rev), area: -area, ents: entIds });
    }
  }
  // Even-odd nesting.
  loops.sort((a, b) => b.area - a.area);
  const depth = new Map<Loop, number>();
  const parent = new Map<Loop, Loop | null>();
  for (let i = 0; i < loops.length; i++) {
    const L = loops[i];
    let par: Loop | null = null;
    for (let j = i - 1; j >= 0; j--) {
      if (pointInPoly(L.poly[0], loops[j].poly) && pointInPoly(midOf(L.poly), loops[j].poly)) {
        par = loops[j];
        break;
      }
    }
    parent.set(L, par);
    depth.set(L, par ? depth.get(par)! + 1 : 0);
  }
  const regions: Region[] = [];
  for (const L of loops) {
    if (depth.get(L)! % 2 !== 0) continue;
    regions.push({ outer: L, holes: loops.filter((h) => parent.get(h) === L) });
  }
  return { regions, openEnds, faces };
}

function polyCentroid(poly: Vec2[]): Vec2 {
  let x = 0, y = 0;
  for (const p of poly) {
    x += p[0];
    y += p[1];
  }
  return [x / poly.length, y / poly.length];
}

/** Loops directly inside a closed area (its holes), among the sketch's smallest areas. */
export function faceHoles(faces: Loop[], f: Loop): Loop[] {
  const inside = faces.filter((g) => g !== f && g.area < f.area && !g.ents.some((e) => f.ents.includes(e)) && pointInPoly(polyCentroid(g.poly), f.poly));
  return inside.filter((g) => !inside.some((h) => h !== g && h.area > g.area && pointInPoly(polyCentroid(g.poly), h.poly)));
}

/** The region (smallest closed area and its holes) containing a point. */
export function regionAtPoint(faces: Loop[], p: Vec2): Region | null {
  let best: Loop | null = null;
  for (const f of faces) if ((!best || f.area < best.area) && pointInPoly(p, f.poly)) best = f;
  return best ? { outer: best, holes: faceHoles(faces, best) } : null;
}

function midOf(poly: Vec2[]): Vec2 {
  return poly[Math.floor(poly.length / 2)];
}

function endTangents(sk: SketchData, e: SketchEntity): [number, number] {
  if (e.type === "line") {
    const a = P(sk, e.p1), b = P(sk, e.p2);
    const t = Math.atan2(b[1] - a[1], b[0] - a[0]);
    return [t, Math.atan2(a[1] - b[1], a[0] - b[0])];
  }
  if (e.type === "spline") {
    const s = sampleEntity(sk, e, 64);
    const a = s[0], a2 = s[1], b = s[s.length - 1], b2 = s[s.length - 2];
    return [Math.atan2(a2[1] - a[1], a2[0] - a[0]), Math.atan2(b2[1] - b[1], b2[0] - b[0])];
  }
  if (e.type === "arc") {
    const g = arcGeom(sk, e);
    // Leaving p1 along CCW direction; leaving p2 along CW direction. Nudge by curvature so
    // tangent-continuous junctions still sort deterministically.
    const t1 = g.a0 + Math.PI / 2 + 1e-4 * Math.sign(g.sweep);
    const t2 = g.a1 - Math.PI / 2 - 1e-4;
    return [normAngle(t1), normAngle(t2)];
  }
  return [0, 0];
}

function normAngle(a: number) {
  return Math.atan2(Math.sin(a), Math.cos(a));
}

function toSeg(sk: SketchData, h: HalfEdge): ProfileSeg {
  const e = h.ent;
  const a = P(sk, h.from), b = P(sk, h.to);
  if (e.type === "arc") return { type: "arc", a, b, mid: arcGeom(sk, e).mid };
  if (e.type === "spline") {
    const s = sampleEntity(sk, e, 64);
    return { type: "spline", a, b, pts: h.from === e.pts[0] ? s : s.slice().reverse() };
  }
  return { type: "line", a, b };
}

function reverseSegs(segs: ProfileSeg[]): ProfileSeg[] {
  return segs
    .slice()
    .reverse()
    .map((s) => (s.type === "line" ? { type: "line", a: s.b, b: s.a } : s.type === "arc" ? { type: "arc", a: s.b, b: s.a, mid: s.mid } : s.type === "spline" ? { type: "spline", a: s.b, b: s.a, pts: s.pts.slice().reverse() } : s));
}

function segsToPoly(segs: ProfileSeg[]): Vec2[] {
  const out: Vec2[] = [];
  for (const s of segs) {
    if (s.type === "line") out.push(s.a);
    else if (s.type === "spline") out.push(...s.pts.slice(0, -1));
    else if (s.type === "arc") {
      // sample 3-point arc
      const pts = sampleThreePointArc(s.a, s.mid, s.b, 24);
      out.push(...pts.slice(0, -1));
    }
  }
  return out;
}

export function sampleThreePointArc(a: Vec2, m: Vec2, b: Vec2, n: number): Vec2[] {
  const c = circumcenter(a, m, b);
  if (!c) return [a, b];
  const r = v2.dist(c, a);
  const ta = Math.atan2(a[1] - c[1], a[0] - c[0]);
  const tm = Math.atan2(m[1] - c[1], m[0] - c[0]);
  const tb = Math.atan2(b[1] - c[1], b[0] - c[0]);
  let sweep = wrap2Pi(tb - ta);
  if (!angleInArc(tm, ta, ta + sweep)) sweep = sweep - 2 * Math.PI;
  const out: Vec2[] = [];
  for (let i = 0; i <= n; i++) out.push(v2.add(c, v2.fromAngle(ta + (sweep * i) / n, r)));
  return out;
}

export function circumcenter(a: Vec2, b: Vec2, c: Vec2): Vec2 | null {
  const d = 2 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
  if (Math.abs(d) < 1e-12) return null;
  const a2 = a[0] ** 2 + a[1] ** 2, b2 = b[0] ** 2 + b[1] ** 2, c2 = c[0] ** 2 + c[1] ** 2;
  return [(a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d, (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d];
}

export function signedArea(poly: Vec2[]) {
  let s = 0;
  for (let i = 0; i < poly.length; i++) {
    const a = poly[i], b = poly[(i + 1) % poly.length];
    s += a[0] * b[1] - b[0] * a[1];
  }
  return s / 2;
}

export function pointInPoly(p: Vec2, poly: Vec2[]) {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const a = poly[i], b = poly[j];
    if (a[1] > p[1] !== b[1] > p[1] && p[0] < ((b[0] - a[0]) * (p[1] - a[1])) / (b[1] - a[1]) + a[0]) inside = !inside;
  }
  return inside;
}
