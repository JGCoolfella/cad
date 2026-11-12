//! 2D helpers over [`Real`], ported operation for operation from `src/sketch/solver.ts`,
//! `src/sketch/model.ts` (`splineThrough`) and `src/core/math.ts` (`wrapAngle`) so values
//! agree to rounding.

use crate::real::Real;
use std::f64::consts::PI;

pub type P<T> = [T; 2];

pub fn sub<T: Real>(a: P<T>, b: P<T>) -> P<T> {
    [a[0] - b[0], a[1] - b[1]]
}
pub fn add<T: Real>(a: P<T>, b: P<T>) -> P<T> {
    [a[0] + b[0], a[1] + b[1]]
}
pub fn scale<T: Real>(a: P<T>, s: T) -> P<T> {
    [a[0] * s, a[1] * s]
}
pub fn dot<T: Real>(a: P<T>, b: P<T>) -> T {
    a[0] * b[0] + a[1] * b[1]
}
pub fn cross<T: Real>(a: P<T>, b: P<T>) -> T {
    a[0] * b[1] - a[1] * b[0]
}

pub fn dist<T: Real>(a: P<T>, b: P<T>) -> T {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Unit vector from `a` to `b` (zero-length guarded as in TypeScript).
pub fn unit<T: Real>(a: P<T>, b: P<T>) -> P<T> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dy).or(1e-12);
    [dx / l, dy / l]
}

/// Signed distance from `p` to the infinite line through `a`, `b`.
pub fn dist_to_line<T: Real>(p: P<T>, a: P<T>, b: P<T>) -> T {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l = dx.hypot(dy).or(1e-12);
    ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx) / l
}

/// Midpoint, computed as TypeScript does: `(a + b) / 2` per axis.
pub fn mid<T: Real>(a: P<T>, b: P<T>) -> P<T> {
    [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
}

/// Wrap to (-π, π].
pub fn wrap_angle<T: Real>(mut a: T) -> T {
    while a.val() <= -PI {
        a = a + 2.0 * PI;
    }
    while a.val() > PI {
        a = a - 2.0 * PI;
    }
    a
}

/// Points along the centripetal Catmull-Rom spline through `pts`, `per` samples per span,
/// plus the last point (`splineThrough`).
pub fn spline_through<T: Real>(pts: &[P<T>], per: usize) -> Vec<P<T>> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut ext = Vec::with_capacity(n + 2);
    ext.push(add(pts[0], sub(pts[0], pts[1])));
    ext.extend_from_slice(pts);
    ext.push(add(pts[n - 1], sub(pts[n - 1], pts[n - 2])));
    let rt = |a: P<T>, b: P<T>| dist(a, b).sqrt().or(1e-6);
    let inv = |t: T| T::cst(1.0) / t;
    let mut out = Vec::with_capacity((ext.len() - 3) * per + 1);
    for i in 1..ext.len() - 2 {
        let (p0, p1, p2, p3) = (ext[i - 1], ext[i], ext[i + 1], ext[i + 2]);
        let (t01, t12, t23) = (rt(p0, p1), rt(p1, p2), rt(p2, p3));
        // Tangents of the centripetal parameterisation (Barry-Goldman), scaled to the span.
        let m1 = scale(
            add(
                sub(
                    scale(sub(p1, p0), inv(t01)),
                    scale(sub(p2, p0), inv(t01 + t12)),
                ),
                scale(sub(p2, p1), inv(t12)),
            ),
            t12,
        );
        let m2 = scale(
            add(
                sub(
                    scale(sub(p2, p1), inv(t12)),
                    scale(sub(p3, p1), inv(t12 + t23)),
                ),
                scale(sub(p3, p2), inv(t23)),
            ),
            t12,
        );
        for k in 0..per {
            let t = k as f64 / per as f64;
            let (t2, t3) = (t * t, t * t * t);
            let (h00, h10, h01, h11) = (
                2.0 * t3 - 3.0 * t2 + 1.0,
                t3 - 2.0 * t2 + t,
                -2.0 * t3 + 3.0 * t2,
                t3 - t2,
            );
            let comb = |a: usize| p1[a] * h00 + m1[a] * h10 + p2[a] * h01 + m2[a] * h11;
            out.push([comb(0), comb(1)]);
        }
    }
    out.push(pts[n - 1]);
    out
}
