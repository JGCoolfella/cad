//! Residual equations. Each relation or dimension becomes one or more scalar equations over
//! resolved references ([`Pt`], [`Rad`]); each equation is evaluated generically over
//! [`Real`], so the same code gives values (`f64`) and exact gradients ([`Dual`]).
//!
//! The formulas are those of `src/sketch/solver.ts`, operation for operation.

use crate::geom::{self, cross, dist, dist_to_line, dot, mid, unit, wrap_angle, P};
use crate::real::{Dual, Real};
use std::f64::consts::PI;

/// A point: free (`x[i]`, `x[i + 1]`) or a constant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pt {
    Var(usize),
    Fixed([f64; 2]),
}

/// A circle radius: free (`x[i]`) or a constant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rad {
    Var(usize),
    Fixed(f64),
}

/// The radius of an entity (`rad(e)` in TypeScript): a circle's radius parameter, an arc's
/// centre-to-start distance, zero for anything else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadOf {
    Circle(Rad),
    Arc { c: Pt, p1: Pt },
    Zero,
}

/// Size compared by an Equal relation: line length or radius.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    Len(Pt, Pt),
    Rad(RadOf),
}

/// Operands of `signedOffset(x, y)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OffsetPair {
    /// Two lines: perpendicular distance of y's midpoint from x, signed.
    Lines { x1: Pt, x2: Pt, y1: Pt, y2: Pt },
    /// Anything else: `rad(y) - rad(x)`.
    Radii { x: RadOf, y: RadOf },
}

/// What a dimension measures (`measureFn`).
#[derive(Clone, Debug, PartialEq)]
pub enum Measure {
    Distance(Pt, Pt),
    /// `sign · (q - p)` along `axis` (0 horizontal, 1 vertical).
    Delta {
        sign: f64,
        axis: usize,
        p: Pt,
        q: Pt,
    },
    PointLine {
        p: Pt,
        a: Pt,
        b: Pt,
    },
    LineLine {
        a1: Pt,
        a2: Pt,
        b1: Pt,
        b2: Pt,
    },
    Angle {
        sign: f64,
        quadrant: u32,
        a1: Pt,
        a2: Pt,
        b1: Pt,
        b2: Pt,
    },
    /// `k · rad` (1 radius, 2 diameter).
    Radius {
        k: f64,
        r: RadOf,
    },
    Offset(OffsetPair),
    ArcLength {
        c: Pt,
        p1: Pt,
        p2: Pt,
    },
}

/// One scalar residual.
#[derive(Clone, Debug, PartialEq)]
pub enum EqKind {
    /// `coord(a) - coord(b)` along `axis`.
    Diff {
        axis: usize,
        a: Pt,
        b: Pt,
    },
    /// Signed distance of `p` from line `a`-`b`.
    PointOnLine {
        p: Pt,
        a: Pt,
        b: Pt,
    },
    /// `|p - c| - r`.
    PointOnCircle {
        p: Pt,
        c: Pt,
        r: RadOf,
    },
    PointOnEllipse {
        p: Pt,
        c: Pt,
        a: Pt,
        q: Pt,
    },
    PointOnSpline {
        p: Pt,
        pts: Vec<Pt>,
    },
    /// Internal: ellipse axes perpendicular.
    EllipseAxes {
        c: Pt,
        a: Pt,
        q: Pt,
    },
    /// `|a - c| - |b - c|`: arc ends equidistant from the centre (internal), equidistant
    /// relation, arc midpoint.
    EqualDist {
        c: Pt,
        a: Pt,
        b: Pt,
    },
    /// `p - (a + b) / 2` along `axis`.
    Midpoint {
        axis: usize,
        p: Pt,
        a: Pt,
        b: Pt,
    },
    /// Cross (parallel) or dot (perpendicular) of the two lines' unit directions.
    Angle {
        cross: bool,
        a1: Pt,
        a2: Pt,
        b1: Pt,
        b2: Pt,
    },
    Equal {
        a: Size,
        b: Size,
    },
    /// Line tangent to an arc ending on it: radius at the shared point ⟂ line.
    TangentLineShared {
        l1: Pt,
        l2: Pt,
        c: Pt,
        s: Pt,
    },
    /// `|distance of centre from line| - r`.
    TangentLineCircle {
        l1: Pt,
        l2: Pt,
        c: Pt,
        r: RadOf,
    },
    /// Arcs sharing an end point: both centres in line with it.
    TangentArcsShared {
        s: Pt,
        c1: Pt,
        c2: Pt,
    },
    TangentCircles {
        c1: Pt,
        r1: RadOf,
        c2: Pt,
        r2: RadOf,
        inner: bool,
    },
    RadDiff(RadOf, RadOf),
    /// Symmetric: midpoint of p-q on the axis.
    SymmetricMid {
        p: Pt,
        q: Pt,
        a: Pt,
        b: Pt,
    },
    /// Symmetric: p-q perpendicular to the axis.
    SymmetricAlong {
        p: Pt,
        q: Pt,
        a: Pt,
        b: Pt,
    },
    Offset {
        s0: f64,
        sm: f64,
        ab: OffsetPair,
        m: OffsetPair,
    },
    /// A driving dimension: `measure - value` (wrapped for angles).
    Dim {
        measure: Measure,
        value: f64,
        angle: bool,
    },
}

/// Reads parameter values as scalars of type `T`.
pub trait Params<T: Real> {
    fn get(&self, i: usize) -> T;
}

/// Plain values.
pub struct Values<'a>(pub &'a [f64]);
impl Params<f64> for Values<'_> {
    #[inline]
    fn get(&self, i: usize) -> f64 {
        self.0[i]
    }
}

/// Values with up to `N` parameters seeded as dual inputs.
pub struct Seeded<'a, const N: usize> {
    pub x: &'a [f64],
    pub seeds: &'a [usize],
}
impl<const N: usize> Params<Dual<N>> for Seeded<'_, N> {
    #[inline]
    fn get(&self, i: usize) -> Dual<N> {
        match self.seeds.iter().position(|&s| s == i) {
            Some(k) => Dual::var(self.x[i], k),
            None => Dual::cst(self.x[i]),
        }
    }
}

#[inline]
fn pt<T: Real>(x: &impl Params<T>, p: Pt) -> P<T> {
    match p {
        Pt::Var(i) => [x.get(i), x.get(i + 1)],
        Pt::Fixed(c) => [T::cst(c[0]), T::cst(c[1])],
    }
}

fn rad<T: Real>(x: &impl Params<T>, r: RadOf) -> T {
    match r {
        RadOf::Circle(Rad::Var(i)) => x.get(i),
        RadOf::Circle(Rad::Fixed(c)) => T::cst(c),
        RadOf::Arc { c, p1 } => {
            let (c, p) = (pt(x, c), pt(x, p1));
            (p[0] - c[0]).hypot(p[1] - c[1])
        }
        RadOf::Zero => T::cst(0.0),
    }
}

/// `signedOffset(x, y)`.
pub fn signed_offset<T: Real>(x: &impl Params<T>, o: OffsetPair) -> T {
    match o {
        OffsetPair::Lines { x1, x2, y1, y2 } => {
            let (a, b) = (pt(x, x1), pt(x, x2));
            let m = mid(pt(x, y1), pt(x, y2));
            let u = unit(a, b);
            u[0] * (m[1] - a[1]) - u[1] * (m[0] - a[0])
        }
        OffsetPair::Radii { x: rx, y: ry } => rad(x, ry) - rad(x, rx),
    }
}

impl Measure {
    pub fn eval<T: Real>(&self, x: &impl Params<T>) -> T {
        match *self {
            Measure::Distance(p, q) => dist(pt(x, p), pt(x, q)),
            Measure::Delta { sign, axis, p, q } => (pt(x, q)[axis] - pt(x, p)[axis]) * sign,
            Measure::PointLine { p, a, b } => dist_to_line(pt(x, p), pt(x, a), pt(x, b)).abs(),
            Measure::LineLine { a1, a2, b1, b2 } => {
                let m = mid(pt(x, b1), pt(x, b2));
                dist_to_line(m, pt(x, a1), pt(x, a2)).abs()
            }
            Measure::Angle {
                sign,
                quadrant,
                a1,
                a2,
                b1,
                b2,
            } => {
                let mut u = unit(pt(x, a1), pt(x, a2));
                let mut v = unit(pt(x, b1), pt(x, b2));
                if quadrant & 1 != 0 {
                    u = [-u[0], -u[1]];
                }
                if quadrant & 2 != 0 {
                    v = [-v[0], -v[1]];
                }
                cross(u, v).atan2(dot(u, v)) * sign
            }
            Measure::Radius { k, r } => rad(x, r) * k,
            Measure::Offset(o) => signed_offset(x, o).abs(),
            Measure::ArcLength { c, p1, p2 } => {
                let (c, a, b) = (pt(x, c), pt(x, p1), pt(x, p2));
                let mut sw = (b[1] - c[1]).atan2(b[0] - c[0]) - (a[1] - c[1]).atan2(a[0] - c[0]);
                while sw.val() <= 0.0 {
                    sw = sw + 2.0 * PI;
                }
                sw * (a[0] - c[0]).hypot(a[1] - c[1])
            }
        }
    }
}

impl EqKind {
    pub fn eval<T: Real>(&self, x: &impl Params<T>) -> T {
        let p = |q: Pt| pt(x, q);
        match self {
            EqKind::Diff { axis, a, b } => p(*a)[*axis] - p(*b)[*axis],
            EqKind::PointOnLine { p: q, a, b } => dist_to_line(p(*q), p(*a), p(*b)),
            EqKind::PointOnCircle { p: q, c, r } => {
                let (c, q) = (p(*c), p(*q));
                (q[0] - c[0]).hypot(q[1] - c[1]) - rad(x, *r)
            }
            EqKind::PointOnEllipse { p: pp, c, a, q } => {
                let (c, a, q, pt) = (p(*c), p(*a), p(*q), p(*pp));
                let ra = (a[0] - c[0]).hypot(a[1] - c[1]).or(1e-9);
                let u = [(a[0] - c[0]) / ra, (a[1] - c[1]) / ra];
                let rb = (q[0] - c[0]).hypot(q[1] - c[1]).or(1e-9);
                let d = [pt[0] - c[0], pt[1] - c[1]];
                let ex = (d[0] * u[0] + d[1] * u[1]) / ra;
                let ey = (-d[0] * u[1] + d[1] * u[0]) / rb;
                (ex.hypot(ey) - 1.0) * ra.min(rb)
            }
            EqKind::PointOnSpline { p: q, pts } => {
                let q = p(*q);
                let ctrl: Vec<P<T>> = pts.iter().map(|&s| p(s)).collect();
                let poly = geom::spline_through(&ctrl, 24);
                let mut best: Option<T> = None;
                for w in poly.windows(2) {
                    let (a, b) = (w[0], w[1]);
                    let ab = [b[0] - a[0], b[1] - a[1]];
                    let l2 = (ab[0] * ab[0] + ab[1] * ab[1]).or(1e-18);
                    let t = (((q[0] - a[0]) * ab[0] + (q[1] - a[1]) * ab[1]) / l2)
                        .min(T::cst(1.0))
                        .max(T::cst(0.0));
                    let d = (q[0] - a[0] - ab[0] * t).hypot(q[1] - a[1] - ab[1] * t);
                    best = Some(match best {
                        Some(b) => b.min(d),
                        None => d,
                    });
                }
                best.unwrap_or(T::cst(f64::INFINITY))
            }
            EqKind::EllipseAxes { c, a, q } => {
                let (c, a, q) = (p(*c), p(*a), p(*q));
                let u = unit(c, a);
                let w = [q[0] - c[0], q[1] - c[1]];
                u[0] * w[0] + u[1] * w[1]
            }
            EqKind::EqualDist { c, a, b } => {
                let (c, a, b) = (p(*c), p(*a), p(*b));
                (a[0] - c[0]).hypot(a[1] - c[1]) - (b[0] - c[0]).hypot(b[1] - c[1])
            }
            EqKind::Midpoint { axis, p: q, a, b } => {
                p(*q)[*axis] - (p(*a)[*axis] + p(*b)[*axis]) / 2.0
            }
            EqKind::Angle {
                cross: is_cross,
                a1,
                a2,
                b1,
                b2,
            } => {
                let u = unit(p(*a1), p(*a2));
                let v = unit(p(*b1), p(*b2));
                if *is_cross {
                    cross(u, v)
                } else {
                    dot(u, v)
                }
            }
            EqKind::Equal { a, b } => size(x, *a) - size(x, *b),
            EqKind::TangentLineShared { l1, l2, c, s } => {
                dot(unit(p(*l1), p(*l2)), unit(p(*c), p(*s)))
            }
            EqKind::TangentLineCircle { l1, l2, c, r } => {
                dist_to_line(p(*c), p(*l1), p(*l2)).abs() - rad(x, *r)
            }
            EqKind::TangentArcsShared { s, c1, c2 } => {
                cross(unit(p(*s), p(*c1)), unit(p(*s), p(*c2)))
            }
            EqKind::TangentCircles {
                c1,
                r1,
                c2,
                r2,
                inner,
            } => {
                let d = dist(p(*c1), p(*c2));
                let (ra, rb) = (rad(x, *r1), rad(x, *r2));
                if *inner {
                    d - (ra - rb).abs()
                } else {
                    d - (ra + rb)
                }
            }
            EqKind::RadDiff(a, b) => rad(x, *a) - rad(x, *b),
            EqKind::SymmetricMid { p: pp, q, a, b } => {
                let (pv, qv) = (p(*pp), p(*q));
                dist_to_line(mid(pv, qv), p(*a), p(*b))
            }
            EqKind::SymmetricAlong { p: pp, q, a, b } => {
                let (pv, qv) = (p(*pp), p(*q));
                let u = unit(p(*a), p(*b));
                (qv[0] - pv[0]) * u[0] + (qv[1] - pv[1]) * u[1]
            }
            EqKind::Offset { s0, sm, ab, m } => {
                signed_offset(x, *ab) * *s0 - signed_offset(x, *m) * *sm
            }
            EqKind::Dim {
                measure,
                value,
                angle,
            } => {
                let g = measure.eval(x) - *value;
                if *angle {
                    wrap_angle(g)
                } else {
                    g
                }
            }
        }
    }

    /// The free parameters the equation reads, sorted, without duplicates.
    pub fn vars(&self) -> Vec<usize> {
        let mut v = VarSink(Vec::new());
        match self {
            EqKind::Diff { a, b, .. } => v.pts(&[a, b]),
            EqKind::PointOnLine { p, a, b }
            | EqKind::EqualDist { c: p, a, b }
            | EqKind::Midpoint { p, a, b, .. }
            | EqKind::EllipseAxes { c: p, a, q: b }
            | EqKind::TangentArcsShared { s: p, c1: a, c2: b } => v.pts(&[p, a, b]),
            EqKind::PointOnCircle { p, c, r } => {
                v.pts(&[p, c]);
                v.rad(r);
            }
            EqKind::PointOnEllipse { p, c, a, q } => v.pts(&[p, c, a, q]),
            EqKind::PointOnSpline { p, pts } => {
                v.pt(p);
                pts.iter().for_each(|q| v.pt(q));
            }
            EqKind::Angle { a1, a2, b1, b2, .. }
            | EqKind::TangentLineShared {
                l1: a1,
                l2: a2,
                c: b1,
                s: b2,
            }
            | EqKind::SymmetricMid {
                p: a1,
                q: a2,
                a: b1,
                b: b2,
            }
            | EqKind::SymmetricAlong {
                p: a1,
                q: a2,
                a: b1,
                b: b2,
            } => v.pts(&[a1, a2, b1, b2]),
            EqKind::Equal { a, b } => {
                for s in [a, b] {
                    match s {
                        Size::Len(p, q) => v.pts(&[p, q]),
                        Size::Rad(r) => v.rad(r),
                    }
                }
            }
            EqKind::TangentLineCircle { l1, l2, c, r } => {
                v.pts(&[l1, l2, c]);
                v.rad(r);
            }
            EqKind::TangentCircles { c1, r1, c2, r2, .. } => {
                v.pts(&[c1, c2]);
                v.rad(r1);
                v.rad(r2);
            }
            EqKind::RadDiff(a, b) => {
                v.rad(a);
                v.rad(b);
            }
            EqKind::Offset { ab, m, .. } => {
                v.pair(ab);
                v.pair(m);
            }
            EqKind::Dim { measure, .. } => match measure {
                Measure::Distance(p, q) | Measure::Delta { p, q, .. } => v.pts(&[p, q]),
                Measure::PointLine { p, a, b } | Measure::ArcLength { c: p, p1: a, p2: b } => {
                    v.pts(&[p, a, b])
                }
                Measure::LineLine { a1, a2, b1, b2 } | Measure::Angle { a1, a2, b1, b2, .. } => {
                    v.pts(&[a1, a2, b1, b2])
                }
                Measure::Radius { r, .. } => v.rad(r),
                Measure::Offset(o) => v.pair(o),
            },
        }
        let mut v = v.0;
        v.sort_unstable();
        v.dedup();
        v
    }
}

/// Collects the free parameters an equation reads.
struct VarSink(Vec<usize>);

impl VarSink {
    fn pt(&mut self, p: &Pt) {
        if let Pt::Var(i) = *p {
            self.0.extend([i, i + 1]);
        }
    }
    fn pts(&mut self, ps: &[&Pt]) {
        ps.iter().for_each(|p| self.pt(p));
    }
    fn rad(&mut self, r: &RadOf) {
        match r {
            RadOf::Circle(Rad::Var(i)) => self.0.push(*i),
            RadOf::Circle(Rad::Fixed(_)) | RadOf::Zero => {}
            RadOf::Arc { c, p1 } => self.pts(&[c, p1]),
        }
    }
    fn pair(&mut self, o: &OffsetPair) {
        match o {
            OffsetPair::Lines { x1, x2, y1, y2 } => self.pts(&[x1, x2, y1, y2]),
            OffsetPair::Radii { x, y } => {
                self.rad(x);
                self.rad(y);
            }
        }
    }
}

fn size<T: Real>(x: &impl Params<T>, s: Size) -> T {
    match s {
        Size::Len(a, b) => dist(pt(x, a), pt(x, b)),
        Size::Rad(r) => rad(x, r),
    }
}

/// Number of derivatives carried per dual evaluation; equations with more variables are
/// differentiated in chunks.
pub const CHUNK: usize = 8;

/// Writes the gradient of `eq` with respect to `vars` (as returned by [`EqKind::vars`])
/// into `out`, and returns the value.
pub fn gradient(eq: &EqKind, x: &[f64], vars: &[usize], out: &mut [f64]) -> f64 {
    let mut value = 0.0;
    if vars.is_empty() {
        return eq.eval(&Values(x));
    }
    for (chunk, seeds) in vars.chunks(CHUNK).enumerate() {
        let d: Dual<CHUNK> = eq.eval(&Seeded::<CHUNK> { x, seeds });
        value = d.v;
        let base = chunk * CHUNK;
        out[base..base + seeds.len()].copy_from_slice(&d.d[..seeds.len()]);
    }
    value
}
