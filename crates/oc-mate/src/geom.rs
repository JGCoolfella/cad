//! Rigid placements, mate geometry, and 3-vector arithmetic written once over
//! [`oc_sketch::real::Real`] (values or dual numbers).
use oc_sketch::real::Real;

pub type V3 = [f64; 3];

/// A rigid placement: `world = rotate(q, local) + t`, `q` a unit quaternion `[w, x, y, z]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub q: [f64; 4],
    pub t: V3,
}

impl Default for Pose {
    fn default() -> Self {
        Pose::IDENTITY
    }
}

impl Pose {
    pub const IDENTITY: Pose = Pose { q: [1.0, 0.0, 0.0, 0.0], t: [0.0; 3] };

    pub fn new(q: [f64; 4], t: V3) -> Pose {
        Pose { q: qnorm(q), t }
    }

    pub fn point(&self, p: V3) -> V3 {
        add(qrot(self.q, p), self.t)
    }

    pub fn dir(&self, d: V3) -> V3 {
        qrot(self.q, d)
    }

    /// `self ∘ inner`.
    pub fn compose(&self, inner: &Pose) -> Pose {
        Pose { q: qnorm(qmul(self.q, inner.q)), t: self.point(inner.t) }
    }

    pub fn inverse(&self) -> Pose {
        let qi = [self.q[0], -self.q[1], -self.q[2], -self.q[3]];
        Pose { q: qi, t: scale(qrot(qi, self.t), -1.0) }
    }

    /// Rotation by `angle` about the axis through `at` with direction `axis`, applied after
    /// `self`.
    pub fn rotated_about(&self, at: V3, axis: V3, angle: f64) -> Pose {
        let a = unit(axis);
        let (s, c) = (angle / 2.0).sin_cos();
        let r = [c, a[0] * s, a[1] * s, a[2] * s];
        let q = qnorm(qmul(r, self.q));
        let t = add(qrot(r, sub(self.t, at)), at);
        Pose { q, t }
    }
}

/// Geometry of one side of a mate, in the coordinates of the body that owns it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Geom {
    /// Point on the plane, normal.
    Plane { p: V3, n: V3 },
    /// A straight edge or axis: point, direction.
    Line { p: V3, d: V3 },
    Point { p: V3 },
    Cylinder { p: V3, d: V3, r: f64 },
    Cone { p: V3, d: V3, r: f64, half_angle: f64 },
    Sphere { c: V3, r: f64 },
    /// Circular edge: centre, normal of its plane, radius.
    Circle { c: V3, n: V3, r: f64 },
}

impl Geom {
    /// The same geometry placed by `pose`.
    pub fn placed(&self, pose: &Pose) -> Geom {
        match *self {
            Geom::Plane { p, n } => Geom::Plane { p: pose.point(p), n: pose.dir(n) },
            Geom::Line { p, d } => Geom::Line { p: pose.point(p), d: pose.dir(d) },
            Geom::Point { p } => Geom::Point { p: pose.point(p) },
            Geom::Cylinder { p, d, r } => Geom::Cylinder { p: pose.point(p), d: pose.dir(d), r },
            Geom::Cone { p, d, r, half_angle } => Geom::Cone { p: pose.point(p), d: pose.dir(d), r, half_angle },
            Geom::Sphere { c, r } => Geom::Sphere { c: pose.point(c), r },
            Geom::Circle { c, n, r } => Geom::Circle { c: pose.point(c), n: pose.dir(n), r },
        }
    }

    /// The point a mate measures from (on the plane, on the axis, the centre).
    pub fn anchor(&self) -> V3 {
        match *self {
            Geom::Plane { p, .. } | Geom::Line { p, .. } | Geom::Point { p } | Geom::Cylinder { p, .. } | Geom::Cone { p, .. } => p,
            Geom::Sphere { c, .. } | Geom::Circle { c, .. } => c,
        }
    }

    /// The direction a mate aligns (plane normal, axis direction), if any.
    pub fn direction(&self) -> Option<V3> {
        match *self {
            Geom::Plane { n, .. } | Geom::Circle { n, .. } => Some(n),
            Geom::Line { d, .. } | Geom::Cylinder { d, .. } | Geom::Cone { d, .. } => Some(d),
            Geom::Point { .. } | Geom::Sphere { .. } => None,
        }
    }
}

// ------------------------------------------------------------------ f64 vectors

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}
/// `a` scaled to unit length (zero stays zero).
pub fn unit(a: V3) -> V3 {
    let n = norm(a);
    if n > 0.0 {
        scale(a, 1.0 / n)
    } else {
        a
    }
}
/// A unit vector perpendicular to `a` (deterministic: crosses the world axis least aligned
/// with `a`).
pub fn perpendicular(a: V3) -> V3 {
    let e = least_aligned_axis(a);
    unit(cross(a, e))
}

fn least_aligned_axis(a: V3) -> V3 {
    let (x, y, z) = (a[0].abs(), a[1].abs(), a[2].abs());
    if x <= y && x <= z {
        [1.0, 0.0, 0.0]
    } else if y <= z {
        [0.0, 1.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    }
}

pub fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}
pub fn qnorm(q: [f64; 4]) -> [f64; 4] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n > 0.0 && n.is_finite() {
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    } else {
        [1.0, 0.0, 0.0, 0.0]
    }
}
pub fn qrot(q: [f64; 4], v: V3) -> V3 {
    let u = [q[1], q[2], q[3]];
    let c1 = cross(u, v);
    let c2 = cross(u, c1);
    [v[0] + 2.0 * (q[0] * c1[0] + c2[0]), v[1] + 2.0 * (q[0] * c1[1] + c2[1]), v[2] + 2.0 * (q[0] * c1[2] + c2[2])]
}

/// The rotation increment of the local parameterisation: `ω` (3 parameters) maps to the
/// unit quaternion `(1, ω/2) / √(1 + |ω|²/4)` (a Cayley-type map). It is smooth everywhere,
/// the identity at `ω = 0` with derivative `½[ω]` there (so `ω` is a rotation vector to
/// first order), and covers every rotation below a half turn. Parameters are re-based
/// after every accepted step, so they stay near zero and no gimbal lock can occur.
pub fn increment(w: V3) -> [f64; 4] {
    let s = (1.0 + dot(w, w) / 4.0).sqrt();
    [1.0 / s, w[0] / (2.0 * s), w[1] / (2.0 * s), w[2] / (2.0 * s)]
}

// ------------------------------------------------------------------ generic vectors

pub type R3<T> = [T; 3];

pub fn cst3<T: Real>(a: V3) -> R3<T> {
    [T::cst(a[0]), T::cst(a[1]), T::cst(a[2])]
}
pub fn gadd<T: Real>(a: R3<T>, b: R3<T>) -> R3<T> {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
pub fn gsub<T: Real>(a: R3<T>, b: R3<T>) -> R3<T> {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub fn gscale<T: Real>(a: R3<T>, s: f64) -> R3<T> {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub fn gdot<T: Real>(a: R3<T>, b: R3<T>) -> T {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub fn gcross<T: Real>(a: R3<T>, b: R3<T>) -> R3<T> {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub fn gnorm<T: Real>(a: R3<T>) -> T {
    a[0].hypot(a[1]).hypot(a[2])
}
pub fn gunit<T: Real>(a: R3<T>) -> R3<T> {
    let n = gnorm(a);
    if n.val() > 0.0 {
        [a[0] / n, a[1] / n, a[2] / n]
    } else {
        a
    }
}

/// An orthonormal pair perpendicular to `d` that moves smoothly with `d` (the helper axis
/// is chosen on the value, so derivatives are those of the branch taken).
pub fn gbasis<T: Real>(d: R3<T>) -> (R3<T>, R3<T>) {
    let dv = [d[0].val(), d[1].val(), d[2].val()];
    let e = cst3::<T>(least_aligned_axis(dv));
    let du = gunit(d);
    let u1 = gunit(gcross(du, e));
    let u2 = gcross(du, u1);
    (u1, u2)
}

/// Rotates a constant vector by the generic quaternion `q`.
pub fn grot<T: Real>(q: [T; 4], v: V3) -> R3<T> {
    let u = [q[1], q[2], q[3]];
    let vv = cst3::<T>(v);
    let c1 = gcross(u, vv);
    let c2 = gcross(u, c1);
    [vv[0] + (q[0] * c1[0] + c2[0]) * 2.0, vv[1] + (q[0] * c1[1] + c2[1]) * 2.0, vv[2] + (q[0] * c1[2] + c2[2]) * 2.0]
}

/// [`increment`] over `T`.
pub fn gincrement<T: Real>(w: R3<T>) -> [T; 4] {
    let s = (gdot(w, w) / 4.0 + 1.0).sqrt();
    let h = s * 2.0;
    [T::cst(1.0) / s, w[0] / h, w[1] / h, w[2] / h]
}

/// Generic Hamilton product.
pub fn gqmul<T: Real>(a: [T; 4], b: [T; 4]) -> [T; 4] {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}
