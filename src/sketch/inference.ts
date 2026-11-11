/**
 * Sketch inferencing / snapping, modelled on SOLIDWORKS Sketch Snaps:
 *  - snap to endpoints & sketch points, centres, midpoints, quadrants, intersections,
 *    nearest (on curve);
 *  - horizontal / vertical inference from the anchor (adds a relation, blue guide);
 *  - horizontal / vertical alignment to "woken" points (guide only, orange);
 *  - perpendicular / parallel inference to the previous chained line;
 *  - optional grid and angle snaps.
 * Holding Ctrl suppresses snapping and automatic relations.
 */
import type { Id, SketchData, Vec2 } from "../core/types";
import { v2, lineLineIntersection, lineCircleIntersections, circleCircleIntersections } from "../core/math";
import { P, arcGeom, closestOnEntity, sampleEntity } from "./model";
import type { SystemOptions } from "../app/settings";

export type SnapKind = "point" | "midpoint" | "quadrant" | "intersection" | "onEntity" | "grid" | "free";

export interface Guide {
  from: Vec2;
  to: Vec2;
  /** true = blue (relation will be added), false = orange guide only. */
  relation: boolean;
}

export interface SnapResult {
  point: Vec2;
  kind: SnapKind;
  pointId?: Id;
  entityId?: Id;
  entity2?: Id;
  hv?: "horizontal" | "vertical";
  perpendicularTo?: Id;
  parallelTo?: Id;
  guides: Guide[];
  /** Relation glyphs shown next to the cursor. */
  badges: string[];
}

export interface InferContext {
  /** Anchor point (start of the line being drawn) for H/V/perp inference. */
  anchor?: Vec2;
  /** Previous chained line, for perpendicular / parallel inference. */
  prevLine?: Id;
  /** Snap tolerance in sketch units (~ pick radius in pixels × world/px). */
  tol: number;
  woken: Id[];
  /** Points that must not be snapped to (e.g. the rubber-band's own points). */
  exclude?: Set<Id>;
  ctrl: boolean;
  options: SystemOptions["sketch"];
  /** Inference of H/V from the anchor applies (lines, rectangle diagonals don't). */
  hvFromAnchor?: boolean;
}

export function infer(sk: SketchData, raw: Vec2, ctx: InferContext): SnapResult {
  const free: SnapResult = { point: raw, kind: "free", guides: [], badges: [] };
  const o = ctx.options;
  if (ctx.ctrl || !o.enableSnapping) return free;
  const s = o.snaps;
  const tol = ctx.tol;
  const ex = ctx.exclude ?? new Set<Id>();

  // 1. Existing points (endpoints, sketch points, centres, origin).
  if (s.endpoints || s.centers) {
    let best: { id: Id; d: number } | null = null;
    for (const p of Object.values(sk.points)) {
      if (ex.has(p.id)) continue;
      const d = Math.hypot(p.x - raw[0], p.y - raw[1]);
      if (d < tol && (!best || d < best.d)) best = { id: p.id, d };
    }
    if (best) {
      const sp = sk.points[best.id];
      return { point: [sp.x, sp.y], kind: "point", pointId: best.id, guides: [], badges: ["coincident"] };
    }
  }

  const ents = Object.values(sk.entities).filter((e) => e.type !== "point" && !entityUsesAny(e, ex));

  // 2. Midpoints and quadrants.
  for (const e of ents) {
    if (s.midpoints && e.type === "line") {
      const m = v2.lerp(P(sk, e.p1), P(sk, e.p2), 0.5);
      if (v2.dist(m, raw) < tol) return { point: m, kind: "midpoint", entityId: e.id, guides: [], badges: ["midpoint"] };
    }
    if (s.midpoints && e.type === "arc") {
      const m = arcGeom(sk, e).mid;
      if (v2.dist(m, raw) < tol) return { point: m, kind: "midpoint", entityId: e.id, guides: [], badges: ["midpoint"] };
    }
    if (s.quadrants && (e.type === "circle" || e.type === "arc")) {
      const c = P(sk, e.c);
      const r = e.type === "circle" ? e.r : arcGeom(sk, e).r;
      for (let k = 0; k < 4; k++) {
        const q = v2.add(c, v2.fromAngle((k * Math.PI) / 2, r));
        if (e.type === "arc" && closestOnEntity(sk, e, q)!.dist > 1e-6) continue;
        if (v2.dist(q, raw) < tol) return { point: q, kind: "quadrant", entityId: e.id, guides: [], badges: ["coincident"] };
      }
    }
  }

  // 3. Intersections among entities near the cursor.
  const near = ents.filter((e) => (closestOnEntity(sk, e, raw)?.dist ?? Infinity) < tol * 2);
  if (s.intersections && near.length >= 2) {
    for (let i = 0; i < near.length; i++)
      for (let j = i + 1; j < near.length; j++) {
        for (const x of intersectEntities(sk, near[i].id, near[j].id)) {
          if (v2.dist(x, raw) < tol)
            return { point: x, kind: "intersection", entityId: near[i].id, entity2: near[j].id, guides: [], badges: ["intersection"] };
        }
      }
  }

  // 4. Inference lines.
  let pt: Vec2 = [raw[0], raw[1]];
  const res: SnapResult = { point: pt, kind: "free", guides: [], badges: [] };
  let lockX = false, lockY = false;
  const a = ctx.anchor;
  if (a && s.hvLines && ctx.hvFromAnchor !== false && v2.dist(a, raw) > tol) {
    if (Math.abs(raw[1] - a[1]) < tol * 0.75) {
      pt = [raw[0], a[1]];
      res.hv = "horizontal";
      lockY = true;
      res.badges.push("horizontal");
    } else if (Math.abs(raw[0] - a[0]) < tol * 0.75) {
      pt = [a[0], raw[1]];
      res.hv = "vertical";
      lockX = true;
      res.badges.push("vertical");
    }
  }
  // Perpendicular / parallel to the previous chained line.
  if (a && !res.hv && ctx.prevLine && (s.perpendicular || s.parallel)) {
    const pl = sk.entities[ctx.prevLine];
    if (pl && pl.type === "line") {
      const u = v2.norm(v2.sub(P(sk, pl.p2), P(sk, pl.p1)));
      const d = v2.sub(raw, a);
      const len = v2.len(d);
      if (len > tol) {
        const along = v2.dot(d, u);
        const across = v2.cross(u, d);
        if (s.perpendicular && Math.abs(along) < tol * 0.75) {
          pt = v2.add(a, v2.scale(v2.perp(u), across));
          res.perpendicularTo = pl.id;
          res.badges.push("perpendicular");
          res.guides.push({ from: a, to: pt, relation: true });
        } else if (s.parallel && Math.abs(across) < tol * 0.75) {
          pt = v2.add(a, v2.scale(u, along));
          res.parallelTo = pl.id;
          res.badges.push("parallel");
          res.guides.push({ from: a, to: pt, relation: true });
        }
      }
    }
  }
  // Alignment with woken points (guide only).
  if (s.hvPoints) {
    for (const id of ctx.woken) {
      const p = sk.points[id];
      if (!p || ex.has(id)) continue;
      if (!lockX && Math.abs(raw[0] - p.x) < tol * 0.75 && Math.abs(raw[1] - p.y) > tol) {
        pt = [p.x, pt[1]];
        lockX = true;
        res.guides.push({ from: [p.x, p.y], to: pt, relation: false });
      } else if (!lockY && Math.abs(raw[1] - p.y) < tol * 0.75 && Math.abs(raw[0] - p.x) > tol) {
        pt = [pt[0], p.y];
        lockY = true;
        res.guides.push({ from: [p.x, p.y], to: pt, relation: false });
      }
    }
  }
  if (res.hv && a) res.guides.push({ from: a, to: pt, relation: true });

  // 5. Nearest point on an entity (coincident) — combined with an H/V lock if any.
  {
    let best: { id: Id; d: number; p: Vec2 } | null = null;
    for (const e of ents) {
      const c = closestOnEntity(sk, e, raw);
      if (c && c.dist < tol && (!best || c.dist < best.d)) best = { id: e.id, d: c.dist, p: c.point };
    }
    if (best) {
      if ((lockX || lockY) && res.guides.length) {
        // Intersect the locked guide with the entity.
        const g = res.guides[0];
        const dir: Vec2 = lockY && !lockX ? [1, 0] : !lockY && lockX ? [0, 1] : v2.sub(g.to, g.from);
        const xs = intersectLineWithEntity(sk, best.id, pt, v2.add(pt, dir));
        const x = xs.sort((m, n) => v2.dist(m, raw) - v2.dist(n, raw))[0];
        if (x && v2.dist(x, raw) < tol * 1.5) {
          res.point = x;
          res.kind = "onEntity";
          res.entityId = best.id;
          res.badges.push("coincident");
          return res;
        }
      }
      return { point: best.p, kind: "onEntity", entityId: best.id, guides: [], badges: ["coincident"] };
    }
  }

  // 6. Grid.
  if (s.grid && !lockX && !lockY) {
    const g = o.gridSpacing;
    pt = [Math.round(pt[0] / g) * g, Math.round(pt[1] / g) * g];
    res.kind = "grid";
  }
  res.point = pt;
  return res;
}

function entityUsesAny(e: SketchData["entities"][string], ex: Set<Id>) {
  if (!ex.size) return false;
  switch (e.type) {
    case "line": return ex.has(e.p1) || ex.has(e.p2);
    case "arc": return ex.has(e.p1) || ex.has(e.p2) || ex.has(e.c);
    case "circle": return ex.has(e.c);
    case "point": return ex.has(e.p);
  }
}

/** Intersections between two sketch entities (bounded to their extents). */
/** Intersection of two segments, if any. */
function segSeg(a: Vec2, b: Vec2, c: Vec2, d: Vec2): Vec2 | null {
  const r: Vec2 = [b[0] - a[0], b[1] - a[1]], s: Vec2 = [d[0] - c[0], d[1] - c[1]];
  const den = r[0] * s[1] - r[1] * s[0];
  if (Math.abs(den) < 1e-15) return null;
  const t = ((c[0] - a[0]) * s[1] - (c[1] - a[1]) * s[0]) / den;
  const u = ((c[0] - a[0]) * r[1] - (c[1] - a[1]) * r[0]) / den;
  if (t < -1e-9 || t > 1 + 1e-9 || u < -1e-9 || u > 1 + 1e-9) return null;
  return [a[0] + r[0] * t, a[1] + r[1] * t];
}

export function intersectEntities(sk: SketchData, a: Id, b: Id): Vec2[] {
  const ea = sk.entities[a], eb = sk.entities[b];
  if (!ea || !eb) return [];
  const pts: Vec2[] = [];
  if (ea.type === "spline" || eb.type === "spline") {
    // Splines: intersect the sampled curves.
    if (ea.type === "point" || eb.type === "point") return [];
    const A = sampleEntity(sk, ea, 128), B = sampleEntity(sk, eb, 128);
    for (let i = 0; i + 1 < A.length; i++)
      for (let j = 0; j + 1 < B.length; j++) {
        const x = segSeg(A[i], A[i + 1], B[j], B[j + 1]);
        if (x && !pts.some((q) => Math.hypot(q[0] - x[0], q[1] - x[1]) < 1e-6)) pts.push(x);
      }
    return pts;
  }
  const onBoth = (p: Vec2) =>
    (closestOnEntity(sk, ea, p)?.dist ?? 1) < 1e-6 && (closestOnEntity(sk, eb, p)?.dist ?? 1) < 1e-6;
  if (ea.type === "line" && eb.type === "line") {
    const x = lineLineIntersection(P(sk, ea.p1), P(sk, ea.p2), P(sk, eb.p1), P(sk, eb.p2));
    if (x) pts.push(x);
  } else if (ea.type === "line" || eb.type === "line") {
    const L = ea.type === "line" ? ea : (eb as typeof ea & { type: "line" });
    const C = ea.type === "line" ? eb : ea;
    if (L.type === "line" && (C.type === "circle" || C.type === "arc")) {
      const r = C.type === "circle" ? C.r : arcGeom(sk, C).r;
      pts.push(...lineCircleIntersections(P(sk, L.p1), P(sk, L.p2), P(sk, C.c), r));
    }
  } else if ((ea.type === "circle" || ea.type === "arc") && (eb.type === "circle" || eb.type === "arc")) {
    const ra = ea.type === "circle" ? ea.r : arcGeom(sk, ea).r;
    const rb = eb.type === "circle" ? eb.r : arcGeom(sk, eb).r;
    pts.push(...circleCircleIntersections(P(sk, ea.c), ra, P(sk, eb.c), rb));
  }
  return pts.filter(onBoth);
}

/** Intersections of an infinite line (through a, b) with an entity. */
export function intersectLineWithEntity(sk: SketchData, id: Id, a: Vec2, b: Vec2): Vec2[] {
  const e = sk.entities[id];
  if (!e) return [];
  if (e.type === "line") {
    const x = lineLineIntersection(a, b, P(sk, e.p1), P(sk, e.p2));
    return x ? [x] : [];
  }
  if (e.type === "circle" || e.type === "arc") {
    const r = e.type === "circle" ? e.r : arcGeom(sk, e).r;
    return lineCircleIntersections(a, b, P(sk, e.c), r).filter((p) => (closestOnEntity(sk, e, p)?.dist ?? 1) < 1e-6);
  }
  return [];
}
