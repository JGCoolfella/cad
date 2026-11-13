//! Frames and vector helpers, as `src/core/math.ts` defines them (same formulas, so
//! sketch planes and reference geometry come out exactly as in the TypeScript engine).

pub use oc_kernel::P3;
use oc_doc::StdPlane;

/// A point in sketch coordinates.
pub type P2 = [f64; 2];

/// TypeScript's `EPS`: vectors shorter than this normalise to zero.
pub const EPS: f64 = 1e-9;

/// A sketch or reference plane frame: origin, in-plane axes and normal (unit vectors).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub origin: P3,
    pub x: P3,
    pub y: P3,
    pub normal: P3,
}

/// 3D vector helpers (`v3` in math.ts).
pub mod v3 {
    pub use oc_kernel::v3::{add, cross, dist, dot, len, scale, sub};
    use oc_kernel::P3;

    /// Unit vector, or zero for a vector shorter than [`super::EPS`] (math.ts `norm`).
    pub fn norm(a: P3) -> P3 {
        let l = len(a);
        if l < super::EPS {
            [0.0; 3]
        } else {
            [a[0] / l, a[1] / l, a[2] / l]
        }
    }
    pub fn neg(a: P3) -> P3 {
        [-a[0], -a[1], -a[2]]
    }
    pub fn mid(a: P3, b: P3) -> P3 {
        scale(add(a, b), 0.5)
    }
}

/// 2D vector helpers (`v2` in math.ts).
pub mod v2 {
    use super::P2;
    pub fn add(a: P2, b: P2) -> P2 {
        [a[0] + b[0], a[1] + b[1]]
    }
    pub fn sub(a: P2, b: P2) -> P2 {
        [a[0] - b[0], a[1] - b[1]]
    }
    pub fn scale(a: P2, s: f64) -> P2 {
        [a[0] * s, a[1] * s]
    }
    pub fn dot(a: P2, b: P2) -> f64 {
        a[0] * b[0] + a[1] * b[1]
    }
    pub fn cross(a: P2, b: P2) -> f64 {
        a[0] * b[1] - a[1] * b[0]
    }
    pub fn len(a: P2) -> f64 {
        a[0].hypot(a[1])
    }
    pub fn dist(a: P2, b: P2) -> f64 {
        (a[0] - b[0]).hypot(a[1] - b[1])
    }
    pub fn norm(a: P2) -> P2 {
        let l = len(a);
        if l < super::EPS {
            [0.0, 0.0]
        } else {
            [a[0] / l, a[1] / l]
        }
    }
    pub fn lerp(a: P2, b: P2, t: f64) -> P2 {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    }
    pub fn perp(a: P2) -> P2 {
        [-a[1], a[0]]
    }
    pub fn from_angle(t: f64, r: f64) -> P2 {
        [t.cos() * r, t.sin() * r]
    }
}

const TAU: f64 = std::f64::consts::TAU;

/// Angle in `[0, 2π)` (math.ts `wrap2Pi`; `%` has JavaScript's sign rule in Rust too).
pub fn wrap_2pi(a: f64) -> f64 {
    let t = a % TAU;
    if t < 0.0 {
        t + TAU
    } else {
        t
    }
}

/// Is angle `t` inside the counter-clockwise arc from `a0` to `a1`?
pub fn angle_in_arc(t: f64, a0: f64, a1: f64) -> bool {
    wrap_2pi(t - a0) <= wrap_2pi(a1 - a0) + 1e-9
}

/// The standard planes, as SOLIDWORKS orients them: Front = XY, Top = XZ (sketch Y
/// towards −Z), Right = YZ (sketch X towards −Z).
pub fn std_frame(p: StdPlane) -> Frame {
    match p {
        StdPlane::Front => Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], normal: [0.0, 0.0, 1.0] },
        StdPlane::Top => Frame { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, -1.0], normal: [0.0, 1.0, 0.0] },
        StdPlane::Right => Frame { origin: [0.0; 3], x: [0.0, 0.0, -1.0], y: [0.0, 1.0, 0.0], normal: [1.0, 0.0, 0.0] },
    }
}

pub fn to_world(f: &Frame, p: P2) -> P3 {
    [
        f.origin[0] + f.x[0] * p[0] + f.y[0] * p[1],
        f.origin[1] + f.x[1] * p[0] + f.y[1] * p[1],
        f.origin[2] + f.x[2] * p[0] + f.y[2] * p[1],
    ]
}

pub fn to_frame(f: &Frame, p: P3) -> P2 {
    let d = v3::sub(p, f.origin);
    [v3::dot(d, f.x), v3::dot(d, f.y)]
}

/// Sketch frame on an arbitrary plane, oriented like the most parallel standard plane
/// (math.ts `frameForPlane`). The origin is the world origin projected onto the plane.
pub fn frame_for_plane(origin: P3, normal: P3) -> Frame {
    let n = v3::norm(normal);
    let mut best = std_frame(StdPlane::Front);
    let mut best_dot = -1.0;
    for c in [StdPlane::Front, StdPlane::Top, StdPlane::Right].map(std_frame) {
        let d = v3::dot(c.normal, n).abs();
        if d > best_dot {
            best_dot = d;
            best = c;
        }
    }
    let flip = v3::dot(best.normal, n) < 0.0;
    let mut x = v3::sub(best.x, v3::scale(n, v3::dot(best.x, n)));
    if v3::len(x) < 1e-6 {
        x = v3::sub(best.y, v3::scale(n, v3::dot(best.y, n)));
    }
    x = v3::norm(x);
    if flip {
        x = v3::neg(x);
    }
    let y = v3::norm(v3::cross(n, x));
    let o = v3::sub([0.0; 3], v3::scale(n, v3::dot(v3::sub([0.0; 3], origin), n)));
    Frame { origin: o, x, y, normal: n }
}

/// Plane offset along its normal by `d`; `flip` reverses it (math.ts `offsetFrame`).
pub fn offset_frame(f: &Frame, d: f64, flip: bool) -> Frame {
    let o = v3::add(f.origin, v3::scale(f.normal, d));
    if flip {
        Frame { origin: o, x: v3::neg(f.x), y: f.y, normal: v3::neg(f.normal) }
    } else {
        Frame { origin: o, ..*f }
    }
}

/// Rotate `v` about the unit axis `k` by `a` radians (Rodrigues).
pub fn rotate_vec(v: P3, k: P3, a: f64) -> P3 {
    let (s, c) = a.sin_cos();
    v3::add(v3::add(v3::scale(v, c), v3::scale(v3::cross(k, v), s)), v3::scale(k, v3::dot(k, v) * (1.0 - c)))
}

pub fn line_line(a: P2, b: P2, c: P2, d: P2) -> Option<P2> {
    let r = v2::sub(b, a);
    let s = v2::sub(d, c);
    let den = v2::cross(r, s);
    if den.abs() < 1e-12 {
        return None;
    }
    let t = v2::cross(v2::sub(c, a), s) / den;
    Some(v2::add(a, v2::scale(r, t)))
}

pub fn line_circle(a: P2, b: P2, c: P2, r: f64) -> Vec<P2> {
    let d = v2::sub(b, a);
    let f = v2::sub(a, c);
    let (qa, qb, qc) = (v2::dot(d, d), 2.0 * v2::dot(f, d), v2::dot(f, f) - r * r);
    let disc = qb * qb - 4.0 * qa * qc;
    if qa < EPS || disc < 0.0 {
        return vec![];
    }
    let s = disc.sqrt();
    [(-qb - s) / (2.0 * qa), (-qb + s) / (2.0 * qa)].iter().map(|&t| v2::add(a, v2::scale(d, t))).collect()
}

pub fn circle_circle(c1: P2, r1: f64, c2: P2, r2: f64) -> Vec<P2> {
    let d = v2::dist(c1, c2);
    if d < EPS || d > r1 + r2 || d < (r1 - r2).abs() {
        return vec![];
    }
    let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let h = (r1 * r1 - a * a).max(0.0).sqrt();
    let u = v2::scale(v2::sub(c2, c1), 1.0 / d);
    let m = v2::add(c1, v2::scale(u, a));
    let p = v2::perp(u);
    vec![v2::add(m, v2::scale(p, h)), v2::sub(m, v2::scale(p, h))]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_follow_the_nearest_standard_plane() {
        let f = frame_for_plane([1.0, 2.0, 5.0], [0.0, 0.0, 1.0]);
        assert_eq!(f, Frame { origin: [0.0, 0.0, 5.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0], normal: [0.0, 0.0, 1.0] });
        // Facing the other way: X mirrored so the sketch is seen un-mirrored.
        let f = frame_for_plane([0.0, 0.0, -2.0], [0.0, 0.0, -1.0]);
        assert_eq!((f.x, f.y), ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
        let o = offset_frame(&std_frame(StdPlane::Top), 3.0, true);
        assert_eq!((o.origin, o.x, o.normal), ([0.0, 3.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]));
        assert!(v3::dist(rotate_vec([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2), [0.0, 1.0, 0.0]) < 1e-15);
    }

    #[test]
    fn angles_wrap_like_javascript() {
        assert!((wrap_2pi(-0.5) - (TAU - 0.5)).abs() < 1e-15);
        assert_eq!(wrap_2pi(TAU), 0.0);
        assert!(angle_in_arc(3.0, 2.0, 4.0) && !angle_in_arc(1.0, 2.0, 4.0) && angle_in_arc(0.1, 6.0, 0.5));
    }
}
