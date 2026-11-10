/**
 * Multi-click sketch shapes (3 point rectangles, parallelogram, perimeter circle,
 * slots, polygon, midpoint line). Each shape knows how many clicks it needs, how to
 * preview itself, and how to create its entities and relations.
 *
 * Relations added mirror what SOLIDWORKS adds for the same tool, so the result has
 * the same degrees of freedom (e.g. a straight slot: tangent arcs, equal radii).
 */
import type { Id, NewRelation, SketchEntity, Vec2 } from "../core/types";
import { v2 } from "../core/math";
import { uid } from "../core/ids";
import type { SnapResult } from "./inference";
import { circumcenter } from "./model";

export interface ShapeOpts {
  construction?: boolean;
  /** Polygon */
  sides?: number;
  inscribed?: boolean;
  /** Rectangles: add construction lines, from corners or from midpoints. */
  constructionLines?: boolean;
  fromMidpoints?: boolean;
}

export interface BuildCtx {
  newPoint(p: Vec2): Id;
  pointFor(s: SnapResult): Id;
  addEntity(e: SketchEntity): Id;
  addRel(r: NewRelation): Id;
  autoRel: boolean;
}

export interface Preview {
  solid: Vec2[][];
  construction: Vec2[][];
  info?: string;
}

export interface Shape {
  clicks: number;
  /** pts = placed points followed by the current cursor point. */
  preview(pts: Vec2[], o: ShapeOpts): Preview;
  create(b: BuildCtx, snaps: SnapResult[], o: ShapeOpts): void;
  prompts: string[];
}

const line = (b: BuildCtx, p1: Id, p2: Id, construction?: boolean) => b.addEntity({ id: uid("e"), type: "line", p1, p2, ...(construction ? { construction: true } : {}) });
const loop = (pts: Vec2[]) => [...pts, pts[0]];
const fmt = (n: number) => (+n.toFixed(2)).toString();

function arcPts(c: Vec2, r: number, a0: number, sweep: number, n = 32): Vec2[] {
  const out: Vec2[] = [];
  for (let i = 0; i <= n; i++) out.push(v2.add(c, v2.fromAngle(a0 + (sweep * i) / n, r)));
  return out;
}

/** Four corners of a rectangle given one edge (p1→p2) and a third point setting the width. */
function rect3(p1: Vec2, p2: Vec2, p3: Vec2): Vec2[] {
  const u = v2.norm(v2.sub(p2, p1));
  const n = v2.perp(u);
  const w = v2.dot(v2.sub(p3, p1), n);
  return [p1, p2, v2.add(p2, v2.scale(n, w)), v2.add(p1, v2.scale(n, w))];
}

function rectCenter3(c: Vec2, m: Vec2, k: Vec2): Vec2[] {
  const u = v2.sub(m, c);
  const n = v2.perp(v2.norm(u));
  const b = v2.dot(v2.sub(k, c), n);
  const nb = v2.scale(n, b);
  return [v2.sub(v2.sub(c, u), nb), v2.sub(v2.add(c, u), nb), v2.add(v2.add(c, u), nb), v2.add(v2.sub(c, u), nb)];
}

/** Build 4 lines through 4 point ids; returns line ids. */
function quad(b: BuildCtx, ids: Id[], o: ShapeOpts): Id[] {
  return [0, 1, 2, 3].map((i) => line(b, ids[i], ids[(i + 1) % 4], o.construction));
}

/** Construction lines for rectangles: diagonals (from corners) or a cross (from midpoints). */
export function rectConstruction(b: BuildCtx, corners: Id[], sides: Id[], pos: Vec2[], o: ShapeOpts, centerSnap?: SnapResult) {
  const mid = (i: number): Vec2 => v2.lerp(pos[i], pos[(i + 1) % 4], 0.5);
  let first: Id;
  if (o.fromMidpoints) {
    const m = [0, 1, 2, 3].map((i) => {
      const id = b.newPoint(mid(i));
      b.addRel({ type: "midpoint", p: id, e: sides[i] });
      return id;
    });
    first = line(b, m[0], m[2], true);
    line(b, m[1], m[3], true);
  } else {
    first = line(b, corners[0], corners[2], true);
    line(b, corners[1], corners[3], true);
  }
  if (centerSnap) {
    const cp = b.pointFor(centerSnap);
    b.addRel({ type: "midpoint", p: cp, e: first });
  }
}

function slotGeom(c1: Vec2, c2: Vec2, w: Vec2) {
  const u = v2.norm(v2.sub(c2, c1));
  const n = v2.perp(u);
  const r = Math.max(1e-6, Math.abs(v2.dot(v2.sub(w, c1), n)));
  return { u, n, r, a1: v2.add(c1, v2.scale(n, r)), b1: v2.sub(c1, v2.scale(n, r)), a2: v2.add(c2, v2.scale(n, r)), b2: v2.sub(c2, v2.scale(n, r)) };
}

function slotPreview(c1: Vec2, c2: Vec2, w: Vec2): Preview {
  const g = slotGeom(c1, c2, w);
  const a = v2.angle(g.n);
  return {
    solid: [[g.a1, g.a2], arcPts(c2, g.r, a - Math.PI, Math.PI), [g.b2, g.b1], arcPts(c1, g.r, a, Math.PI)],
    construction: [[c1, c2]],
    info: `L=${fmt(v2.dist(c1, c2))}  W=${fmt(2 * g.r)}`,
  };
}

function slotCreate(b: BuildCtx, c1: Id, c2: Id, P1: Vec2, P2: Vec2, w: Vec2, o: ShapeOpts) {
  const g = slotGeom(P1, P2, w);
  const a1 = b.newPoint(g.a1), b1 = b.newPoint(g.b1), a2 = b.newPoint(g.a2), b2 = b.newPoint(g.b2);
  const top = line(b, a1, a2, o.construction);
  const bottom = line(b, b2, b1, o.construction);
  const cons = o.construction ? { construction: true } : {};
  // Arcs run counter-clockwise from p1 to p2.
  const arc1 = b.addEntity({ id: uid("e"), type: "arc", c: c1, p1: a1, p2: b1, ...cons });
  const arc2 = b.addEntity({ id: uid("e"), type: "arc", c: c2, p1: b2, p2: a2, ...cons });
  const centerline = line(b, c1, c2, true);
  for (const [l, a] of [[top, arc1], [top, arc2], [bottom, arc1], [bottom, arc2]]) b.addRel({ type: "tangent", e1: l, e2: a });
  b.addRel({ type: "equal", e1: arc1, e2: arc2 });
  return centerline;
}

function polygonPts(c: Vec2, v: Vec2, n: number): Vec2[] {
  const R = v2.dist(c, v), a0 = v2.angle(v2.sub(v, c));
  return Array.from({ length: n }, (_, i) => v2.add(c, v2.fromAngle(a0 + (2 * Math.PI * i) / n, R)));
}

export const SHAPES: Record<string, Shape> = {
  midpointLine: {
    clicks: 2,
    prompts: ["Click to place the midpoint of the line.", "Click to place an end of the line."],
    preview: ([m, e]) => (e ? { solid: [[v2.sub(v2.scale(m, 2), e), e]], construction: [], info: `L=${fmt(2 * v2.dist(m, e))}` } : { solid: [], construction: [] }),
    create(b, [m, e], o) {
      const M = m.point, E = e.point;
      const p1 = b.newPoint(v2.sub(v2.scale(M, 2), E));
      const p2 = b.pointFor(e);
      const l = line(b, p1, p2, o.construction);
      const mp = b.pointFor(m);
      b.addRel({ type: "midpoint", p: mp, e: l });
      if (b.autoRel && e.hv) b.addRel({ type: e.hv, e: l });
    },
  },
  threePointRectangle: {
    clicks: 3,
    prompts: ["Click to set the first corner.", "Click to set the direction and length of the first edge.", "Click to set the width."],
    preview(pts, o) {
      if (pts.length < 2) return { solid: [], construction: [] };
      if (pts.length === 2) return { solid: [[pts[0], pts[1]]], construction: [], info: `L=${fmt(v2.dist(pts[0], pts[1]))}` };
      const q = rect3(pts[0], pts[1], pts[2]);
      return { solid: [loop(q)], construction: o.constructionLines ? [[q[0], q[2]], [q[1], q[3]]] : [], info: `${fmt(v2.dist(q[0], q[1]))} x ${fmt(v2.dist(q[1], q[2]))}` };
    },
    create(b, s, o) {
      const q = rect3(s[0].point, s[1].point, s[2].point);
      if (v2.dist(q[1], q[2]) < 1e-9) return;
      const ids = [b.pointFor(s[0]), b.pointFor(s[1]), b.newPoint(q[2]), b.newPoint(q[3])];
      const l = quad(b, ids, o);
      b.addRel({ type: "perpendicular", e1: l[0], e2: l[1] });
      b.addRel({ type: "parallel", e1: l[0], e2: l[2] });
      b.addRel({ type: "parallel", e1: l[1], e2: l[3] });
      if (o.constructionLines) rectConstruction(b, ids, l, q, o);
    },
  },
  threePointCenterRectangle: {
    clicks: 3,
    prompts: ["Click to set the center.", "Click to set the midpoint of an edge.", "Click to set a corner."],
    preview(pts) {
      if (pts.length < 2) return { solid: [], construction: [] };
      if (pts.length === 2) return { solid: [], construction: [[v2.sub(v2.scale(pts[0], 2), pts[1]), pts[1]]] };
      const q = rectCenter3(pts[0], pts[1], pts[2]);
      return { solid: [loop(q)], construction: [[q[0], q[2]], [q[1], q[3]]], info: `${fmt(v2.dist(q[0], q[1]))} x ${fmt(v2.dist(q[1], q[2]))}` };
    },
    create(b, s, o) {
      const q = rectCenter3(s[0].point, s[1].point, s[2].point);
      if (v2.dist(q[1], q[2]) < 1e-9 || v2.dist(q[0], q[1]) < 1e-9) return;
      const ids = q.map((p) => b.newPoint(p));
      const l = quad(b, ids, o);
      b.addRel({ type: "perpendicular", e1: l[0], e2: l[1] });
      b.addRel({ type: "parallel", e1: l[0], e2: l[2] });
      b.addRel({ type: "parallel", e1: l[1], e2: l[3] });
      rectConstruction(b, ids, l, q, o, s[0]);
    },
  },
  parallelogram: {
    clicks: 3,
    prompts: ["Click to set the first corner.", "Click to set the first edge.", "Click to set the adjacent edge."],
    preview(pts, o) {
      if (pts.length < 2) return { solid: [], construction: [] };
      if (pts.length === 2) return { solid: [[pts[0], pts[1]]], construction: [] };
      const q = [pts[0], pts[1], pts[2], v2.add(pts[0], v2.sub(pts[2], pts[1]))];
      return { solid: [loop(q)], construction: o.constructionLines ? [[q[0], q[2]], [q[1], q[3]]] : [] };
    },
    create(b, s, o) {
      const q = [s[0].point, s[1].point, s[2].point, v2.add(s[0].point, v2.sub(s[2].point, s[1].point))];
      if (Math.abs(v2.cross(v2.sub(q[1], q[0]), v2.sub(q[2], q[1]))) < 1e-9) return;
      const ids = [b.pointFor(s[0]), b.pointFor(s[1]), b.pointFor(s[2]), b.newPoint(q[3])];
      const l = quad(b, ids, o);
      b.addRel({ type: "parallel", e1: l[0], e2: l[2] });
      b.addRel({ type: "parallel", e1: l[1], e2: l[3] });
      if (o.constructionLines) rectConstruction(b, ids, l, q, o);
    },
  },
  perimeterCircle: {
    clicks: 3,
    prompts: ["Click to set the first point on the perimeter.", "Click to set the second point.", "Click to set the third point."],
    preview(pts) {
      if (pts.length < 2) return { solid: [], construction: [] };
      if (pts.length === 2) {
        const c = v2.lerp(pts[0], pts[1], 0.5);
        return { solid: [arcPts(c, v2.dist(c, pts[0]), 0, 2 * Math.PI, 72)], construction: [] };
      }
      const c = circumcenter(pts[0], pts[1], pts[2]);
      if (!c) return { solid: [[pts[0], pts[2]]], construction: [] };
      const r = v2.dist(c, pts[0]);
      return { solid: [arcPts(c, r, 0, 2 * Math.PI, 72)], construction: [], info: `R=${fmt(r)}` };
    },
    create(b, s, o) {
      const c = circumcenter(s[0].point, s[1].point, s[2].point);
      if (!c) return;
      const cid = b.newPoint(c);
      const id = b.addEntity({ id: uid("e"), type: "circle", c: cid, r: v2.dist(c, s[0].point), ...(o.construction ? { construction: true } : {}) });
      if (b.autoRel) for (const sn of s) if (sn.kind === "point" && sn.pointId) b.addRel({ type: "coincident", p: sn.pointId, e: id });
    },
  },
  straightSlot: {
    clicks: 3,
    prompts: ["Click to set the center of the first arc.", "Click to set the center of the second arc.", "Click to set the slot width."],
    preview(pts) {
      if (pts.length < 2) return { solid: [], construction: [] };
      if (pts.length === 2) return { solid: [], construction: [[pts[0], pts[1]]], info: `L=${fmt(v2.dist(pts[0], pts[1]))}` };
      return slotPreview(pts[0], pts[1], pts[2]);
    },
    create(b, s, o) {
      if (v2.dist(s[0].point, s[1].point) < 1e-9) return;
      slotCreate(b, b.pointFor(s[0]), b.pointFor(s[1]), s[0].point, s[1].point, s[2].point, o);
    },
  },
  centerSlot: {
    clicks: 3,
    prompts: ["Click to set the center of the slot.", "Click to set the center of an arc.", "Click to set the slot width."],
    preview(pts) {
      if (pts.length < 2) return { solid: [], construction: [] };
      const c1 = v2.sub(v2.scale(pts[0], 2), pts[1]);
      if (pts.length === 2) return { solid: [], construction: [[c1, pts[1]]], info: `L=${fmt(v2.dist(c1, pts[1]))}` };
      return slotPreview(c1, pts[1], pts[2]);
    },
    create(b, s, o) {
      if (v2.dist(s[0].point, s[1].point) < 1e-9) return;
      const P1 = v2.sub(v2.scale(s[0].point, 2), s[1].point);
      const c1 = b.newPoint(P1), c2 = b.pointFor(s[1]);
      const cl = slotCreate(b, c1, c2, P1, s[1].point, s[2].point, o);
      const mid = b.pointFor(s[0]);
      b.addRel({ type: "midpoint", p: mid, e: cl });
    },
  },
  polygon: {
    clicks: 2,
    prompts: ["Click to set the center of the polygon.", "Drag or click to set the size and rotation."],
    preview(pts, o) {
      if (pts.length < 2) return { solid: [], construction: [] };
      const n = Math.max(3, Math.min(40, o.sides ?? 6));
      const q = polygonPts(pts[0], pts[1], n);
      const R = v2.dist(pts[0], pts[1]);
      const r = o.inscribed === false ? R : R * Math.cos(Math.PI / n);
      return { solid: [loop(q)], construction: [arcPts(pts[0], r, 0, 2 * Math.PI, 72)], info: `Ø=${fmt(2 * r)}` };
    },
    create(b, s, o) {
      const n = Math.max(3, Math.min(40, o.sides ?? 6));
      const C = s[0].point, V = s[1].point;
      const R = v2.dist(C, V);
      if (R < 1e-9) return;
      const q = polygonPts(C, V, n);
      const c = b.pointFor(s[0]);
      const ids = q.map((p, i) => (i === 0 ? b.pointFor(s[1]) : b.newPoint(p)));
      const sides = ids.map((id, i) => line(b, id, ids[(i + 1) % n], o.construction));
      for (let i = 1; i < n; i++) b.addRel({ type: "equal", e1: sides[0], e2: sides[i] });
      const inscribed = o.inscribed !== false;
      const circle = b.addEntity({ id: uid("e"), type: "circle", c, r: inscribed ? R * Math.cos(Math.PI / n) : R, construction: true });
      if (inscribed) {
        for (let i = 1; i < n; i++) b.addRel({ type: "equidistant", c, p1: ids[0], p2: ids[i] });
        b.addRel({ type: "tangent", e1: sides[0], e2: circle });
      } else {
        for (const id of ids) b.addRel({ type: "coincident", p: id, e: circle });
      }
    },
  },
};

/** Geometry for a sketch fillet at the corner V between directions to A and B. */
export function filletGeom(V: Vec2, A: Vec2, B: Vec2, r: number) {
  const u1 = v2.norm(v2.sub(A, V)), u2 = v2.norm(v2.sub(B, V));
  const cos = Math.max(-1, Math.min(1, v2.dot(u1, u2)));
  const theta = Math.acos(cos);
  if (theta < 1e-4 || Math.PI - theta < 1e-4) return null;
  const d = r / Math.tan(theta / 2);
  if (d > v2.dist(A, V) - 1e-9 || d > v2.dist(B, V) - 1e-9) return null;
  const T1 = v2.add(V, v2.scale(u1, d)), T2 = v2.add(V, v2.scale(u2, d));
  const C = v2.add(V, v2.scale(v2.norm(v2.add(u1, u2)), r / Math.sin(theta / 2)));
  // Arc must run counter-clockwise from p1 to p2 through the short side.
  const ccw = v2.cross(v2.sub(T1, C), v2.sub(T2, C)) > 0;
  return { T1, T2, C, ccw };
}
