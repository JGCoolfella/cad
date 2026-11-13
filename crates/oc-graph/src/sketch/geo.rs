//! Sketch geometry as the profile code sees it: points and entities in insertion order
//! (the order `Object.values` gives in TypeScript, which the region walk depends on),
//! plus the sampling helpers of `src/sketch/model.ts`.

use indexmap::IndexMap;
use oc_doc::{SketchData, SketchEntity};

use crate::math::{angle_in_arc, v2, wrap_2pi, P2};

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Line { p1: String, p2: String },
    Circle { c: String, r: f64 },
    /// Counter-clockwise from `p1` to `p2` about `c`.
    Arc { c: String, p1: String, p2: String },
    Point { p: String },
    Ellipse { c: String, a: String, q: String },
    Spline { pts: Vec<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Ent {
    pub id: String,
    pub kind: Kind,
    /// Kept as an option: chain selection treats "absent" and `false` differently, as
    /// TypeScript's `!==` does.
    pub construction: Option<bool>,
    /// Stands for a model edge (external reference).
    pub ext: bool,
}

impl Ent {
    /// Part of profiles and paths: not construction geometry, not a model reference.
    pub fn solid(&self) -> bool {
        self.construction != Some(true) && !self.ext
    }

    /// Sketch points the entity is defined by (`entityPoints`).
    pub fn points(&self) -> Vec<&str> {
        match &self.kind {
            Kind::Line { p1, p2 } => vec![p1, p2],
            Kind::Circle { c, .. } => vec![c],
            Kind::Arc { c, p1, p2 } => vec![c, p1, p2],
            Kind::Point { p } => vec![p],
            Kind::Ellipse { c, a, q } => vec![c, a, q],
            Kind::Spline { pts } => pts.iter().map(String::as_str).collect(),
        }
    }

    /// End points of an open curve (line, arc, spline).
    pub fn ends(&self) -> Option<(&str, &str)> {
        match &self.kind {
            Kind::Line { p1, p2 } | Kind::Arc { p1, p2, .. } => Some((p1, p2)),
            Kind::Spline { pts } => Some((pts.first()?, pts.last()?)),
            _ => None,
        }
    }
}

/// Points and entities of one sketch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Geo {
    pub points: IndexMap<String, P2>,
    pub entities: IndexMap<String, Ent>,
    next_id: u32,
}

impl Geo {
    pub fn new(sk: &SketchData) -> Self {
        let points = sk.points.values().map(|p| (p.id.as_str().to_string(), [p.x, p.y])).collect();
        let s = |id: &oc_doc::Id| id.as_str().to_string();
        let entities = sk
            .entities
            .values()
            .map(|e| {
                let (kind, construction, ext) = match e {
                    SketchEntity::Line { p1, p2, construction, ext, .. } => {
                        (Kind::Line { p1: s(p1), p2: s(p2) }, *construction, ext.is_some())
                    }
                    SketchEntity::Circle { c, r, construction, ext, .. } => {
                        (Kind::Circle { c: s(c), r: *r }, *construction, ext.is_some())
                    }
                    SketchEntity::Arc { c, p1, p2, construction, ext, .. } => {
                        (Kind::Arc { c: s(c), p1: s(p1), p2: s(p2) }, *construction, ext.is_some())
                    }
                    SketchEntity::Point { p, .. } => (Kind::Point { p: s(p) }, None, false),
                    SketchEntity::Ellipse { c, a, q, construction, .. } => {
                        (Kind::Ellipse { c: s(c), a: s(a), q: s(q) }, *construction, false)
                    }
                    SketchEntity::Spline { pts, construction, .. } => {
                        (Kind::Spline { pts: pts.iter().map(s).collect() }, *construction, false)
                    }
                };
                let id = s(e.id());
                (id.clone(), Ent { id, kind, construction, ext })
            })
            .collect();
        Geo { points, entities, next_id: 0 }
    }

    /// Point coordinates; a missing point reads as NaN (the document validator reports it).
    pub fn p(&self, id: &str) -> P2 {
        self.points.get(id).copied().unwrap_or([f64::NAN; 2])
    }

    /// A fresh id with `prefix` that no point or entity uses (TypeScript's `uid`).
    pub fn fresh(&mut self, prefix: &str) -> String {
        loop {
            self.next_id += 1;
            let id = format!("{prefix}~{}", self.next_id);
            if !self.points.contains_key(&id) && !self.entities.contains_key(&id) {
                return id;
            }
        }
    }

    pub fn add_point(&mut self, p: P2) -> String {
        let id = self.fresh("p");
        self.points.insert(id.clone(), p);
        id
    }

    pub fn add_entity(&mut self, kind: Kind, construction: Option<bool>) -> String {
        let id = self.fresh("e");
        self.entities.insert(id.clone(), Ent { id: id.clone(), kind, construction, ext: false });
        id
    }

    /// Entities that use point `pid`, in entity order.
    pub fn using_point<'a>(&'a self, pid: &'a str) -> impl Iterator<Item = &'a Ent> + 'a {
        self.entities.values().filter(move |e| e.points().contains(&pid))
    }

    pub fn arc(&self, c: &str, p1: &str, p2: &str) -> ArcGeom {
        let (c, a, b) = (self.p(c), self.p(p1), self.p(p2));
        let r = v2::dist(c, a);
        let a0 = (a[1] - c[1]).atan2(a[0] - c[0]);
        let a1 = (b[1] - c[1]).atan2(b[0] - c[0]);
        let mut sweep = wrap_2pi(a1 - a0);
        if sweep < 1e-9 {
            sweep = std::f64::consts::TAU;
        }
        let mid = v2::add(c, v2::from_angle(a0 + sweep / 2.0, r));
        ArcGeom { c, r, a0, a1: a0 + sweep, sweep, mid }
    }

    pub fn ellipse(&self, c: &str, a: &str, q: &str) -> EllipseGeom {
        let (c, a, q) = (self.p(c), self.p(a), self.p(q));
        let ra = nonzero(v2::dist(c, a));
        let u = v2::scale(v2::sub(a, c), 1.0 / ra);
        let v = v2::perp(u);
        let rb = nonzero(v2::dot(v2::sub(q, c), v).abs());
        EllipseGeom { c, u, v, ra, rb }
    }

    /// Polyline approximation of an entity (`sampleEntity`).
    pub fn sample(&self, e: &Ent, seg: usize) -> Vec<P2> {
        match &e.kind {
            Kind::Spline { pts } => spline_through(&pts.iter().map(|p| self.p(p)).collect::<Vec<_>>(), (seg as f64 / 4.0).round().max(4.0) as usize),
            Kind::Ellipse { c, a, q } => {
                let g = self.ellipse(c, a, q);
                (0..=seg)
                    .map(|i| {
                        let t = i as f64 / seg as f64 * std::f64::consts::TAU;
                        v2::add(g.c, v2::add(v2::scale(g.u, g.ra * t.cos()), v2::scale(g.v, g.rb * t.sin())))
                    })
                    .collect()
            }
            Kind::Line { p1, p2 } => vec![self.p(p1), self.p(p2)],
            Kind::Point { p } => vec![self.p(p)],
            Kind::Circle { c, r } => {
                let c = self.p(c);
                (0..=seg).map(|i| v2::add(c, v2::from_angle(i as f64 / seg as f64 * std::f64::consts::TAU, *r))).collect()
            }
            Kind::Arc { c, p1, p2 } => {
                let g = self.arc(c, p1, p2);
                let n = ((seg as f64 * g.sweep / std::f64::consts::TAU).ceil() as usize).max(4);
                (0..=n).map(|i| v2::add(g.c, v2::from_angle(g.a0 + g.sweep * i as f64 / n as f64, g.r))).collect()
            }
        }
    }
}

/// `x || 1e-9` for a length.
fn nonzero(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        1e-9
    } else {
        x
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArcGeom {
    pub c: P2,
    pub r: f64,
    pub a0: f64,
    pub a1: f64,
    pub sweep: f64,
    pub mid: P2,
}

#[derive(Clone, Copy, Debug)]
pub struct EllipseGeom {
    pub c: P2,
    pub u: P2,
    pub v: P2,
    pub ra: f64,
    pub rb: f64,
}

/// Smooth curve through points: centripetal Catmull-Rom with natural ends, `per` samples
/// per span (`splineThrough`).
pub fn spline_through(pts: &[P2], per: usize) -> Vec<P2> {
    if pts.len() <= 2 {
        return pts.to_vec();
    }
    let n = pts.len();
    let mut ext = Vec::with_capacity(n + 2);
    ext.push(v2::add(pts[0], v2::sub(pts[0], pts[1])));
    ext.extend_from_slice(pts);
    ext.push(v2::add(pts[n - 1], v2::sub(pts[n - 1], pts[n - 2])));
    let knot = |a: P2, b: P2| {
        let t = v2::dist(a, b).sqrt();
        if t == 0.0 {
            1e-6
        } else {
            t
        }
    };
    let mut out = Vec::with_capacity((n - 1) * per + 1);
    for i in 1..ext.len() - 2 {
        let (p0, p1, p2, p3) = (ext[i - 1], ext[i], ext[i + 1], ext[i + 2]);
        let (t01, t12, t23) = (knot(p0, p1), knot(p1, p2), knot(p2, p3));
        let m1 = v2::scale(
            v2::add(
                v2::sub(v2::scale(v2::sub(p1, p0), 1.0 / t01), v2::scale(v2::sub(p2, p0), 1.0 / (t01 + t12))),
                v2::scale(v2::sub(p2, p1), 1.0 / t12),
            ),
            t12,
        );
        let m2 = v2::scale(
            v2::add(
                v2::sub(v2::scale(v2::sub(p2, p1), 1.0 / t12), v2::scale(v2::sub(p3, p1), 1.0 / (t12 + t23))),
                v2::scale(v2::sub(p3, p2), 1.0 / t23),
            ),
            t12,
        );
        for k in 0..per {
            let t = k as f64 / per as f64;
            let (t2, t3) = (t * t, t * t * t);
            let (h00, h10, h01, h11) = (2.0 * t3 - 3.0 * t2 + 1.0, t3 - 2.0 * t2 + t, -2.0 * t3 + 3.0 * t2, t3 - t2);
            out.push([
                h00 * p1[0] + h10 * m1[0] + h01 * p2[0] + h11 * m2[0],
                h00 * p1[1] + h10 * m1[1] + h01 * p2[1] + h11 * m2[1],
            ]);
        }
    }
    out.push(pts[n - 1]);
    out
}

pub fn circumcenter(a: P2, b: P2, c: P2) -> Option<P2> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (v2::dot(a, a), v2::dot(b, b), v2::dot(c, c));
    Some([
        (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d,
        (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d,
    ])
}

/// `n + 1` points on the arc from `a` through `m` to `b`.
pub fn sample_three_point_arc(a: P2, m: P2, b: P2, n: usize) -> Vec<P2> {
    let Some(c) = circumcenter(a, m, b) else { return vec![a, b] };
    let r = v2::dist(c, a);
    let ang = |p: P2| (p[1] - c[1]).atan2(p[0] - c[0]);
    let (ta, tm, tb) = (ang(a), ang(m), ang(b));
    let mut sweep = wrap_2pi(tb - ta);
    if !angle_in_arc(tm, ta, ta + sweep) {
        sweep -= std::f64::consts::TAU;
    }
    (0..=n).map(|i| v2::add(c, v2::from_angle(ta + sweep * i as f64 / n as f64, r))).collect()
}

pub fn signed_area(poly: &[P2]) -> f64 {
    let n = poly.len();
    (0..n).map(|i| v2::cross(poly[i], poly[(i + 1) % n])).sum::<f64>() / 2.0
}

/// Even-odd point in polygon (`pointInPoly`).
pub fn point_in_poly(p: P2, poly: &[P2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_passes_through_its_points() {
        let pts = [[0.0, 0.0], [10.0, 5.0], [20.0, 0.0]];
        let s = spline_through(&pts, 16);
        assert_eq!(s.len(), 33);
        assert_eq!(s[0], pts[0]);
        assert!(v2::dist(s[16], pts[1]) < 1e-12);
        assert_eq!(s[32], pts[2]);
    }

    #[test]
    fn polygons() {
        let sq = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
        assert_eq!(signed_area(&sq), 4.0);
        assert!(point_in_poly([1.0, 1.0], &sq) && !point_in_poly([3.0, 1.0], &sq));
        let arc = sample_three_point_arc([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], 4);
        assert!(v2::dist(arc[2], [0.0, 1.0]) < 1e-12);
        let arc = sample_three_point_arc([1.0, 0.0], [0.0, -1.0], [-1.0, 0.0], 4);
        assert!(v2::dist(arc[2], [0.0, -1.0]) < 1e-12);
    }
}
