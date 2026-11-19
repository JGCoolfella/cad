/**
 * Mate animation (G1d), as SOLIDWORKS animates mates: when a mate is previewed or applied
 * (the Mate PropertyManager, its pop-up toolbar, Flip Mate Alignment, a Distance or Angle
 * value, Edit Mate, SmartMates) the components that move ease from where they were drawn
 * to the solved placement instead of jumping. Only the drawn placement animates: the
 * document and the build are updated at once (one undo step; undo restores at once), and
 * picks find the components at their solved placement. A newer solve starts from the pose
 * being drawn. Drags, insertion, undo and opening do not animate.
 *
 * Options > Assemblies > Mate animation speed (`asm.mateAnimation`, 0 = Off … 10 = Fast).
 */
import type { Id } from "../core/types";
import type { Transform } from "./types";

/** How long an animation takes at `speed` (0..10; 0: none), in milliseconds (🔍 SOLIDWORKS'
 * timing): 1 s at the slowest, 0.1 s at the fastest. */
export function animationMs(speed: number): number {
  const s = Math.max(0, Math.min(10, Math.round(speed)));
  return s === 0 ? 0 : 100 * (11 - s);
}

/** Smoothstep easing: starts and ends at rest. */
export const ease = (t: number) => (t <= 0 ? 0 : t >= 1 ? 1 : t * t * (3 - 2 * t));

/** `a` to `b` at `t`: the translation lerped, the rotation slerped (shortest way). */
export function interpolate(a: Transform, b: Transform, t: number): Transform {
  const p = a.position.map((x, k) => x + (b.position[k] - x) * t) as Transform["position"];
  let [w1, x1, y1, z1] = b.rotation;
  const [w0, x0, y0, z0] = a.rotation;
  let d = w0 * w1 + x0 * x1 + y0 * y1 + z0 * z1;
  if (d < 0) {
    [w1, x1, y1, z1] = [-w1, -x1, -y1, -z1];
    d = -d;
  }
  let k0: number, k1: number;
  if (d > 0.9995) {
    k0 = 1 - t;
    k1 = t;
  } else {
    const th = Math.acos(Math.min(1, d));
    const s = Math.sin(th);
    k0 = Math.sin((1 - t) * th) / s;
    k1 = Math.sin(t * th) / s;
  }
  const q = [k0 * w0 + k1 * w1, k0 * x0 + k1 * x1, k0 * y0 + k1 * y1, k0 * z0 + k1 * z1];
  const n = Math.hypot(...q) || 1;
  return { position: p, rotation: q.map((x) => x / n) as Transform["rotation"] };
}

/** A running animation: each moving instance's placement when it started, to its solved one. */
export interface MateAnimation {
  from: Map<Id, Transform>;
  to: Map<Id, Transform>;
  t0: number;
  ms: number;
}

const same = (a: Transform, b: Transform) => a.position.every((x, k) => Math.abs(x - b.position[k]) < 1e-9) && a.rotation.every((x, k) => Math.abs(x - b.rotation[k]) < 1e-12);

/** Starts an animation from the drawn placements `from` to `to` (instances that did not
 * move are left out), or null when nothing moves or the speed is Off. */
export function startAnimation(from: Map<Id, Transform>, to: Map<Id, Transform>, ms: number, now: number): MateAnimation | null {
  if (ms <= 0) return null;
  const f = new Map<Id, Transform>();
  const t = new Map<Id, Transform>();
  for (const [id, b] of to) {
    const a = from.get(id);
    if (a && !same(a, b)) {
      f.set(id, a);
      t.set(id, b);
    }
  }
  return f.size ? { from: f, to: t, t0: now, ms } : null;
}

/** Where instance `id` is drawn at `now` (null: not animating, draw it where it is). */
export function drawnAt(a: MateAnimation | null, id: Id, now: number): Transform | null {
  if (!a) return null;
  const f = a.from.get(id);
  const t = a.to.get(id);
  if (!f || !t) return null;
  const u = (now - a.t0) / a.ms;
  return u >= 1 ? null : interpolate(f, t, ease(u));
}
