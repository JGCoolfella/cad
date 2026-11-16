//! What each mate means as equations. A mate between two pieces of geometry becomes a
//! [`Rule`], chosen once from the geometry types; the rule's residuals are written once over
//! [`Real`] so the same code gives values and exact derivatives.
//!
//! Directions that must be parallel are written two ways ([`Mode`]):
//! - for solving, `b − s·a` (three rows), which vanishes only at the wanted alignment `s`,
//!   so Gauss-Newton converges to the aligned or anti-aligned solution the mate asks for
//!   from any start except the exact opposite one;
//! - for rank analysis, the two components of `b` across `a` (`u₁·b`, `u₂·b` with `u₁`, `u₂`
//!   perpendicular to `a`): the same two degrees of freedom without the third, always
//!   dependent row, so a mate is not reported redundant against itself.
use oc_sketch::real::Real;

use crate::geom::*;
use crate::{Align, MateKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Solve,
    Analyse,
}

/// The geometry class a mate sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Class {
    Plane,
    /// A true line (straight edge, reference axis).
    Line,
    /// A cylinder or cone: an axis with a radius.
    Round,
    Circle,
    Sphere,
    Point,
}

pub(crate) fn class(g: &Geom) -> Class {
    match g {
        Geom::Plane { .. } => Class::Plane,
        Geom::Line { .. } => Class::Line,
        Geom::Cylinder { .. } | Geom::Cone { .. } => Class::Round,
        Geom::Circle { .. } => Class::Circle,
        Geom::Sphere { .. } => Class::Sphere,
        Geom::Point { .. } => Class::Point,
    }
}

fn radius(g: &Geom) -> f64 {
    match *g {
        Geom::Cylinder { r, .. } | Geom::Cone { r, .. } | Geom::Sphere { r, .. } | Geom::Circle { r, .. } => r,
        _ => 0.0,
    }
}

/// Equations of one mate. `a` and `b` are the sides after ordering (see [`build`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Rule {
    /// Directions parallel with sign `s`, then `b`'s anchor at `off` along `a`'s normal.
    PlanePlane { s: f64, off: f64 },
    /// `b`'s direction across `a`'s normal, `b`'s anchor at `off` along it.
    PlaneAxisIn { off: f64 },
    /// `b`'s anchor at `off` along `a`'s normal.
    PlanePoint { off: f64 },
    /// Directions parallel with sign `s`, `b`'s anchor at `off` along `a`'s normal (a circle
    /// lying in a plane).
    PlaneCircle { s: f64, off: f64 },
    /// Directions parallel (sign `s`) and `b`'s anchor on `a`'s line: coaxial.
    Coaxial { s: f64 },
    /// `b`'s anchor on `a`'s line.
    PointOnAxis,
    /// Anchors equal.
    PointPoint,
    /// Directions parallel (sign `s`), anchors equal (coincident circles).
    CircleCircle { s: f64 },
    /// Directions parallel with sign `s` (parallel planes or axes).
    Parallel { s: f64 },
    /// Directions perpendicular.
    Perpendicular,
    /// Parallel directions, then `b`'s anchor `gap` from `a`'s line.
    AxisAxisGap { s: f64, gap: f64 },
    /// `b`'s anchor `gap` from `a`'s line.
    PointAxisGap { gap: f64 },
    /// Anchors `gap` apart.
    PointPointGap { gap: f64 },
    /// `dot(a, b) = c` of the directions.
    DotIs { c: f64 },
    /// `b`'s placement in `a`'s coordinates equals `rel`.
    Lock { rel: Pose },
}

/// The inputs a rule is chosen from.
pub(crate) struct Spec<'a> {
    pub kind: MateKind,
    pub ga: &'a Geom,
    pub gb: &'a Geom,
    pub align: Align,
    pub value: f64,
    pub flip: bool,
    pub relative: Pose,
}

/// Chooses the rule. Returns it with `swap` (true when the sides were exchanged so that
/// the plane, then the axis, comes first), or the reason the pair cannot be mated so.
pub(crate) fn build(m: &Spec) -> Result<(Rule, bool), String> {
    use Class::*;
    let (ca, cb) = (class(m.ga), class(m.gb));
    let swap = cb < ca;
    let (ca, cb, ga, gb) = if swap { (cb, ca, m.gb, m.ga) } else { (ca, cb, m.ga, m.gb) };
    let s = if m.align == Align::Aligned { 1.0 } else { -1.0 };
    let sigma = if m.flip { -1.0 } else { 1.0 };
    let v = m.value;
    let (ra, rb) = (radius(ga), radius(gb));
    let no = || Err(format!("{:?} cannot be mated between a {} and a {}", m.kind, name(ca), name(cb)));
    let axis = |c: Class| matches!(c, Line | Round | Circle);
    let rule = match m.kind {
        MateKind::Coincident => match (ca, cb) {
            (Plane, Plane) => Rule::PlanePlane { s, off: 0.0 },
            (Plane, Line) => Rule::PlaneAxisIn { off: 0.0 },
            (Plane, Point) => Rule::PlanePoint { off: 0.0 },
            (Plane, Circle) => Rule::PlaneCircle { s, off: 0.0 },
            (Line, Line) => Rule::Coaxial { s },
            (Line, Point) => Rule::PointOnAxis,
            (Point, Point) => Rule::PointPoint,
            (Circle, Circle) => Rule::CircleCircle { s },
            _ => return no(),
        },
        MateKind::Parallel => match (ca, cb) {
            (Plane, Plane) => Rule::Parallel { s },
            (Plane, c) if axis(c) => Rule::Perpendicular,
            (a, b) if axis(a) && axis(b) => Rule::Parallel { s },
            _ => return no(),
        },
        MateKind::Perpendicular => match (ca, cb) {
            (Plane, Plane) => Rule::Perpendicular,
            (Plane, c) if axis(c) => Rule::Parallel { s: closest(ga, gb) },
            (a, b) if axis(a) && axis(b) => Rule::Perpendicular,
            _ => return no(),
        },
        MateKind::Concentric => match (ca, cb) {
            (a, b) if axis(a) && axis(b) => Rule::Coaxial { s },
            (a, Point | Sphere) if axis(a) => Rule::PointOnAxis,
            (Sphere, Sphere | Point) => Rule::PointPoint,
            _ => return no(),
        },
        MateKind::Tangent => {
            // Plane and round: the round side lies on the side of the normal for
            // anti-aligned (outside the face's material), behind it for aligned (🔍).
            let side = -s;
            match (ca, cb) {
                (Plane, Round) if matches!(gb, Geom::Cylinder { .. }) => Rule::PlaneAxisIn { off: side * rb },
                (Plane, Sphere) => Rule::PlanePoint { off: side * rb },
                (Round, Round) if matches!((ga, gb), (Geom::Cylinder { .. }, Geom::Cylinder { .. })) => {
                    // Anti-aligned: outside each other; aligned: one inside the other.
                    let gap = if s < 0.0 { ra + rb } else { (ra - rb).abs() };
                    Rule::AxisAxisGap { s: closest(ga, gb), gap }
                }
                (Round, Sphere) if matches!(ga, Geom::Cylinder { .. }) => Rule::PointAxisGap { gap: if s < 0.0 { ra + rb } else { (ra - rb).abs() } },
                (Sphere, Sphere) => Rule::PointPointGap { gap: if s < 0.0 { ra + rb } else { (ra - rb).abs() } },
                _ => return no(),
            }
        }
        MateKind::Distance => {
            if v < 0.0 {
                return Err("a distance cannot be negative".into());
            }
            match (ca, cb) {
                (Plane, Plane) => Rule::PlanePlane { s, off: sigma * v },
                (Plane, Point | Sphere) => Rule::PlanePoint { off: sigma * v },
                (Plane, Line | Round) => Rule::PlaneAxisIn { off: sigma * v },
                (a, b) if axis(a) && axis(b) && a != Circle && b != Circle => Rule::AxisAxisGap { s, gap: v },
                (a, Point | Sphere) if axis(a) => Rule::PointAxisGap { gap: v },
                (Point | Sphere | Circle, Point | Sphere | Circle) => Rule::PointPointGap { gap: v },
                _ => return no(),
            }
        }
        MateKind::Angle => {
            if ga.direction().is_none() || gb.direction().is_none() {
                return no();
            }
            let c = v.cos();
            Rule::DotIs { c: s * c }
        }
        MateKind::Lock => Rule::Lock { rel: if swap { m.relative.inverse() } else { m.relative } },
    };
    Ok((rule, swap))
}

fn name(c: Class) -> &'static str {
    match c {
        Class::Plane => "plane",
        Class::Line => "line",
        Class::Round => "cylinder or cone",
        Class::Circle => "circle",
        Class::Sphere => "sphere",
        Class::Point => "point",
    }
}

/// Sign that keeps two directions as they are now (for mates without an alignment option).
fn closest(a: &Geom, b: &Geom) -> f64 {
    match (a.direction(), b.direction()) {
        (Some(x), Some(y)) if dot(x, y) < 0.0 => -1.0,
        _ => 1.0,
    }
}

/// A side placed in the world: anchor, direction (zero for points), placement.
pub(crate) struct Placed<T> {
    pub p: R3<T>,
    pub d: R3<T>,
    /// World rotation and translation of the owning body (for Lock).
    pub q: [T; 4],
    pub t: R3<T>,
}

/// Places constant body-local geometry with a base pose and parameters `x = (δ, ω)`.
pub(crate) fn place<T: Real>(g: &Geom, base: &Pose, x: Option<&[T; 6]>) -> Placed<T> {
    let (p, d) = (g.anchor(), g.direction().unwrap_or([0.0; 3]));
    let (bp, bd) = (base.dir(p), base.dir(d));
    let qb = base.q.map(T::cst);
    match x {
        None => Placed { p: cst3(add(bp, base.t)), d: cst3(bd), q: qb, t: cst3(base.t) },
        Some(x) => {
            let qi = gincrement([x[3], x[4], x[5]]);
            let t = gadd(cst3(base.t), [x[0], x[1], x[2]]);
            Placed { p: gadd(grot(qi, bp), t), d: grot(qi, bd), q: gqmul(qi, qb), t }
        }
    }
}

fn dir_eq<T: Real>(a: R3<T>, b: R3<T>, s: f64, mode: Mode, out: &mut Vec<T>) {
    match mode {
        Mode::Solve => out.extend(gsub(b, gscale(a, s))),
        Mode::Analyse => {
            let (u1, u2) = gbasis(a);
            out.push(gdot(u1, b));
            out.push(gdot(u2, b));
        }
    }
}

fn across<T: Real>(p: R3<T>, origin: R3<T>, d: R3<T>) -> (T, T) {
    let (u1, u2) = gbasis(d);
    let v = gsub(p, origin);
    (gdot(u1, v), gdot(u2, v))
}

/// Conjugate rotation of a generic vector.
fn grot_inv<T: Real>(q: [T; 4], v: R3<T>) -> R3<T> {
    let u = [-q[1], -q[2], -q[3]];
    let c1 = gcross(u, v);
    let c2 = gcross(u, c1);
    [v[0] + (q[0] * c1[0] + c2[0]) * 2.0, v[1] + (q[0] * c1[1] + c2[1]) * 2.0, v[2] + (q[0] * c1[2] + c2[2]) * 2.0]
}

/// Appends the residuals of `rule` for placed sides `a` and `b`.
pub(crate) fn residuals<T: Real>(rule: &Rule, a: &Placed<T>, b: &Placed<T>, mode: Mode, out: &mut Vec<T>) {
    let on_plane = |p: R3<T>| gdot(gsub(p, a.p), gunit(a.d));
    match *rule {
        Rule::PlanePlane { s, off } | Rule::PlaneCircle { s, off } => {
            dir_eq(gunit(a.d), gunit(b.d), s, mode, out);
            out.push(on_plane(b.p) - off);
        }
        Rule::PlaneAxisIn { off } => {
            out.push(gdot(gunit(a.d), gunit(b.d)));
            out.push(on_plane(b.p) - off);
        }
        Rule::PlanePoint { off } => out.push(on_plane(b.p) - off),
        Rule::Coaxial { s } => {
            dir_eq(gunit(a.d), gunit(b.d), s, mode, out);
            let (x, y) = across(b.p, a.p, a.d);
            out.push(x);
            out.push(y);
        }
        Rule::PointOnAxis => {
            let (x, y) = across(b.p, a.p, a.d);
            out.push(x);
            out.push(y);
        }
        Rule::PointPoint => out.extend(gsub(b.p, a.p)),
        Rule::CircleCircle { s } => {
            dir_eq(gunit(a.d), gunit(b.d), s, mode, out);
            out.extend(gsub(b.p, a.p));
        }
        Rule::Parallel { s } => dir_eq(gunit(a.d), gunit(b.d), s, mode, out),
        Rule::Perpendicular => out.push(gdot(gunit(a.d), gunit(b.d))),
        Rule::AxisAxisGap { s, gap } => {
            dir_eq(gunit(a.d), gunit(b.d), s, mode, out);
            let (x, y) = across(b.p, a.p, a.d);
            out.push(x.hypot(y) - gap);
        }
        Rule::PointAxisGap { gap } => {
            let (x, y) = across(b.p, a.p, a.d);
            out.push(x.hypot(y) - gap);
        }
        Rule::PointPointGap { gap } => out.push(gnorm(gsub(b.p, a.p)) - gap),
        Rule::DotIs { c } => out.push(gdot(gunit(a.d), gunit(b.d)) - c),
        Rule::Lock { rel } => {
            // b's placement in a's coordinates minus `rel`: translation, then the vector
            // part of the remaining rotation (sign chosen so it vanishes only at identity).
            let local = grot_inv(a.q, gsub(b.t, a.t));
            out.extend(gsub(local, cst3(rel.t)));
            let qa_inv = [a.q[0], -a.q[1], -a.q[2], -a.q[3]];
            let rel_inv = [rel.q[0], -rel.q[1], -rel.q[2], -rel.q[3]].map(T::cst);
            let e = gqmul(rel_inv, gqmul(qa_inv, b.q));
            let k = if e[0].val() < 0.0 { -2.0 } else { 2.0 };
            out.extend([e[1] * k, e[2] * k, e[3] * k]);
        }
    }
}
