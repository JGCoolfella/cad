import type { Frame, Vec2, Vec3 } from "./types";

export const EPS = 1e-9;

export const v2 = {
  add: (a: Vec2, b: Vec2): Vec2 => [a[0] + b[0], a[1] + b[1]],
  sub: (a: Vec2, b: Vec2): Vec2 => [a[0] - b[0], a[1] - b[1]],
  scale: (a: Vec2, s: number): Vec2 => [a[0] * s, a[1] * s],
  dot: (a: Vec2, b: Vec2) => a[0] * b[0] + a[1] * b[1],
  cross: (a: Vec2, b: Vec2) => a[0] * b[1] - a[1] * b[0],
  len: (a: Vec2) => Math.hypot(a[0], a[1]),
  dist: (a: Vec2, b: Vec2) => Math.hypot(a[0] - b[0], a[1] - b[1]),
  norm: (a: Vec2): Vec2 => {
    const l = Math.hypot(a[0], a[1]);
    return l < EPS ? [0, 0] : [a[0] / l, a[1] / l];
  },
  lerp: (a: Vec2, b: Vec2, t: number): Vec2 => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
  perp: (a: Vec2): Vec2 => [-a[1], a[0]],
  angle: (a: Vec2) => Math.atan2(a[1], a[0]),
  fromAngle: (t: number, r = 1): Vec2 => [Math.cos(t) * r, Math.sin(t) * r],
};

export const v3 = {
  add: (a: Vec3, b: Vec3): Vec3 => [a[0] + b[0], a[1] + b[1], a[2] + b[2]],
  sub: (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]],
  scale: (a: Vec3, s: number): Vec3 => [a[0] * s, a[1] * s, a[2] * s],
  dot: (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2],
  cross: (a: Vec3, b: Vec3): Vec3 => [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ],
  len: (a: Vec3) => Math.hypot(a[0], a[1], a[2]),
  dist: (a: Vec3, b: Vec3) => Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]),
  norm: (a: Vec3): Vec3 => {
    const l = Math.hypot(a[0], a[1], a[2]);
    return l < EPS ? [0, 0, 0] : [a[0] / l, a[1] / l, a[2] / l];
  },
  neg: (a: Vec3): Vec3 => [-a[0], -a[1], -a[2]],
};

/** Normalise an angle to (-PI, PI]. */
export function wrapAngle(a: number) {
  while (a <= -Math.PI) a += 2 * Math.PI;
  while (a > Math.PI) a -= 2 * Math.PI;
  return a;
}

/** Normalise an angle to [0, 2PI). */
export function wrap2Pi(a: number) {
  const t = a % (2 * Math.PI);
  return t < 0 ? t + 2 * Math.PI : t;
}

/**
 * Standard planes, matching SOLIDWORKS: Front = XY (normal +Z), Top = XZ (normal +Y,
 * sketch Y points to -Z), Right = YZ (normal +X, sketch X points to -Z).
 */
export const STD_FRAMES: Record<"front" | "top" | "right", Frame> = {
  front: { origin: [0, 0, 0], x: [1, 0, 0], y: [0, 1, 0], normal: [0, 0, 1] },
  top: { origin: [0, 0, 0], x: [1, 0, 0], y: [0, 0, -1], normal: [0, 1, 0] },
  right: { origin: [0, 0, 0], x: [0, 0, -1], y: [0, 1, 0], normal: [1, 0, 0] },
};

export function frameToWorld(f: Frame, p: Vec2): Vec3 {
  return [
    f.origin[0] + f.x[0] * p[0] + f.y[0] * p[1],
    f.origin[1] + f.x[1] * p[0] + f.y[1] * p[1],
    f.origin[2] + f.x[2] * p[0] + f.y[2] * p[1],
  ];
}

export function worldToFrame(f: Frame, p: Vec3): Vec2 {
  const d = v3.sub(p, f.origin);
  return [v3.dot(d, f.x), v3.dot(d, f.y)];
}

/**
 * Build a sketch frame for an arbitrary plane (e.g. a planar face), choosing axes the
 * way SOLIDWORKS does: inherit the orientation of the most-parallel standard plane.
 */
export function frameForPlane(origin: Vec3, normal: Vec3): Frame {
  const n = v3.norm(normal);
  const cands = [STD_FRAMES.front, STD_FRAMES.top, STD_FRAMES.right];
  let best = cands[0];
  let bestDot = -1;
  for (const c of cands) {
    const d = Math.abs(v3.dot(c.normal, n));
    if (d > bestDot) {
      bestDot = d;
      best = c;
    }
  }
  // Project the standard plane's X axis into the new plane. If the face points the
  // other way, mirror X so that the sketch is seen un-mirrored from the face's front.
  const flip = v3.dot(best.normal, n) < 0;
  let x = v3.sub(best.x, v3.scale(n, v3.dot(best.x, n)));
  if (v3.len(x) < 1e-6) x = v3.sub(best.y, v3.scale(n, v3.dot(best.y, n)));
  x = v3.norm(x);
  if (flip) x = v3.neg(x);
  const y = v3.norm(v3.cross(n, x));
  // Origin: projection of world origin onto the plane (keeps sketch origin meaningful).
  const o = v3.sub([0, 0, 0], v3.scale(n, v3.dot(v3.sub([0, 0, 0], origin), n)));
  return { origin: o, x, y, normal: n };
}

export function offsetFrame(f: Frame, d: number, flip = false): Frame {
  const o = v3.add(f.origin, v3.scale(f.normal, d));
  if (!flip) return { ...f, origin: o };
  return { origin: o, x: v3.neg(f.x), y: f.y, normal: v3.neg(f.normal) };
}

/** Closest point on segment ab to p, with parameter t in [0,1]. */
export function closestOnSegment(p: Vec2, a: Vec2, b: Vec2) {
  const ab = v2.sub(b, a);
  const l2 = v2.dot(ab, ab);
  let t = l2 < EPS ? 0 : v2.dot(v2.sub(p, a), ab) / l2;
  t = Math.max(0, Math.min(1, t));
  return { t, point: v2.add(a, v2.scale(ab, t)) };
}

/** Is angle t inside CCW arc from a0 to a1? */
export function angleInArc(t: number, a0: number, a1: number) {
  const sweep = wrap2Pi(a1 - a0);
  const rel = wrap2Pi(t - a0);
  return rel <= sweep + 1e-9;
}

export function lineLineIntersection(a: Vec2, b: Vec2, c: Vec2, d: Vec2): Vec2 | null {
  const r = v2.sub(b, a);
  const s = v2.sub(d, c);
  const den = v2.cross(r, s);
  if (Math.abs(den) < 1e-12) return null;
  const t = v2.cross(v2.sub(c, a), s) / den;
  return v2.add(a, v2.scale(r, t));
}

export function lineCircleIntersections(a: Vec2, b: Vec2, c: Vec2, r: number): Vec2[] {
  const d = v2.sub(b, a);
  const f = v2.sub(a, c);
  const A = v2.dot(d, d);
  const B = 2 * v2.dot(f, d);
  const C = v2.dot(f, f) - r * r;
  const disc = B * B - 4 * A * C;
  if (A < EPS || disc < 0) return [];
  const s = Math.sqrt(disc);
  return [(-B - s) / (2 * A), (-B + s) / (2 * A)].map((t) => v2.add(a, v2.scale(d, t)));
}

export function circleCircleIntersections(c1: Vec2, r1: number, c2: Vec2, r2: number): Vec2[] {
  const d = v2.dist(c1, c2);
  if (d < EPS || d > r1 + r2 || d < Math.abs(r1 - r2)) return [];
  const a = (r1 * r1 - r2 * r2 + d * d) / (2 * d);
  const h = Math.sqrt(Math.max(0, r1 * r1 - a * a));
  const u = v2.scale(v2.sub(c2, c1), 1 / d);
  const m = v2.add(c1, v2.scale(u, a));
  const p = v2.perp(u);
  return [v2.add(m, v2.scale(p, h)), v2.sub(m, v2.scale(p, h))];
}
