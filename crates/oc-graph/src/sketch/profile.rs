//! Closed regions of a sketch (`extractProfiles`, `regionAtPoint` in
//! `src/sketch/model.ts`): a half-edge graph of the non-construction lines, arcs and
//! splines, dangling chains pruned, faces walked with angular ordering, the outer
//! boundary of each connected component kept, and boundaries nested even-odd. Circles and
//! ellipses are loops on their own. No intersections are computed: curves must meet at
//! shared sketch points, as in the TypeScript engine.

use indexmap::IndexMap;

use super::geo::{point_in_poly, sample_three_point_arc, signed_area, Geo, Kind};
use crate::math::{v2, P2};

/// One curve of a loop, in sketch coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Line { a: P2, b: P2 },
    Arc { a: P2, b: P2, mid: P2 },
    Circle { c: P2, r: f64 },
    /// Full ellipse: centre, unit first axis, radius along it and perpendicular to it.
    Ellipse { c: P2, u: P2, ra: f64, rb: f64 },
    /// Sampled spline from `a` to `b` (both included in `pts`).
    Spline { a: P2, b: P2, pts: Vec<P2> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Loop {
    pub segs: Vec<Seg>,
    pub poly: Vec<P2>,
    /// Counter-clockwise loops are positive.
    pub area: f64,
    /// Entities the loop is made of.
    pub ents: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub outer: Loop,
    pub holes: Vec<Loop>,
}

#[derive(Clone, Debug, Default)]
pub struct Profiles {
    pub regions: Vec<Region>,
    /// Number of open chain ends (for the "open contours" message).
    pub open_ends: usize,
    /// Smallest closed areas (bounded faces of the graph, plus circles and ellipses), for
    /// Selected Contours.
    pub faces: Vec<Loop>,
}

struct HalfEdge {
    from: String,
    to: String,
    ent: String,
    angle: f64,
    used: bool,
    twin: usize,
}

pub fn extract(g: &Geo) -> Profiles {
    let mut loops: Vec<Loop> = vec![];
    let mut out: Vec<HalfEdge> = vec![];
    let mut by_node: IndexMap<String, Vec<usize>> = IndexMap::new();
    for e in g.entities.values() {
        if !e.solid() {
            continue;
        }
        match &e.kind {
            Kind::Ellipse { c, a, q } => {
                let el = g.ellipse(c, a, q);
                loops.push(Loop {
                    segs: vec![Seg::Ellipse { c: el.c, u: el.u, ra: el.ra, rb: el.rb }],
                    poly: g.sample(e, 96),
                    area: std::f64::consts::PI * el.ra * el.rb,
                    ents: vec![e.id.clone()],
                });
            }
            Kind::Circle { c, r } => loops.push(Loop {
                segs: vec![Seg::Circle { c: g.p(c), r: *r }],
                poly: g.sample(e, 96),
                area: std::f64::consts::PI * r * r,
                ents: vec![e.id.clone()],
            }),
            Kind::Line { .. } | Kind::Arc { .. } | Kind::Spline { .. } => {
                let Some((e1, e2)) = e.ends() else { continue };
                if e1 == e2 {
                    continue;
                }
                let (ta, tb) = end_tangents(g, e);
                let i = out.len();
                for (from, to, angle, twin) in [(e1, e2, ta, i + 1), (e2, e1, tb, i)] {
                    by_node.entry(from.to_string()).or_default().push(out.len());
                    out.push(HalfEdge { from: from.into(), to: to.into(), ent: e.id.clone(), angle, used: false, twin });
                }
            }
            Kind::Point { .. } => {}
        }
    }
    let open_ends = by_node.values().filter(|h| h.len() == 1).count();
    // Prune dangling chains so they don't break the loops they hang off.
    let mut pruned = true;
    while pruned {
        pruned = false;
        let nodes: Vec<String> = by_node.keys().cloned().collect();
        for node in nodes {
            let Some(hs) = by_node.get(&node) else { continue };
            if hs.len() != 1 {
                continue;
            }
            let h = hs[0];
            by_node.shift_remove(&node);
            let (to, twin) = (out[h].to.clone(), out[h].twin);
            if let Some(other) = by_node.get_mut(&to) {
                // JavaScript's splice(indexOf(x), 1) removes the last element when x is absent.
                let at = other.iter().position(|&x| x == twin).unwrap_or(other.len().wrapping_sub(1));
                if at < other.len() {
                    other.remove(at);
                }
            }
            out[h].used = true;
            out[twin].used = true;
            pruned = true;
        }
    }
    for hs in by_node.values_mut() {
        hs.sort_by(|&a, &b| out[a].angle.partial_cmp(&out[b].angle).unwrap_or(std::cmp::Ordering::Equal));
    }
    let mut faces = loops.clone();
    // Faces: at each node take the next edge clockwise from the reverse of the incoming one.
    for h0 in 0..out.len() {
        if out[h0].used {
            continue;
        }
        let mut cycle = vec![];
        let mut h = Some(h0);
        let mut guard = 0;
        while let Some(cur) = h {
            if out[cur].used || guard >= 10000 {
                break;
            }
            guard += 1;
            out[cur].used = true;
            cycle.push(cur);
            let hs = by_node.get(&out[cur].to).map(Vec::as_slice).unwrap_or(&[]);
            let i = hs.iter().position(|&x| x == out[cur].twin).map_or(-1, |i| i as isize);
            h = (!hs.is_empty()).then(|| hs[(i - 1 + hs.len() as isize).rem_euclid(hs.len() as isize) as usize]);
            if h == Some(h0) {
                break;
            }
        }
        if h != Some(h0) || cycle.is_empty() {
            continue;
        }
        let mut ents: Vec<String> = vec![];
        for &c in &cycle {
            if !ents.contains(&out[c].ent) {
                ents.push(out[c].ent.clone());
            }
        }
        if ents.len() != cycle.len() {
            continue;
        }
        let segs: Vec<Seg> = cycle.iter().map(|&c| to_seg(g, &out[c])).collect();
        let poly = segs_to_poly(&segs);
        let area = signed_area(&poly);
        if area > 0.0 {
            faces.push(Loop { segs, poly, area, ents });
        } else if area < 0.0 {
            let rev = reverse_segs(&segs);
            loops.push(Loop { poly: segs_to_poly(&rev), segs: rev, area: -area, ents });
        }
    }
    // Even-odd nesting (stable sort by area, largest first).
    loops.sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap_or(std::cmp::Ordering::Equal));
    let mut parent: Vec<Option<usize>> = vec![None; loops.len()];
    let mut depth = vec![0usize; loops.len()];
    for i in 0..loops.len() {
        let l = &loops[i];
        let mid = l.poly[l.poly.len() / 2];
        if let Some(j) = (0..i).rev().find(|&j| point_in_poly(l.poly[0], &loops[j].poly) && point_in_poly(mid, &loops[j].poly)) {
            parent[i] = Some(j);
            depth[i] = depth[j] + 1;
        }
    }
    let regions = (0..loops.len())
        .filter(|&i| depth[i] % 2 == 0)
        .map(|i| Region {
            outer: loops[i].clone(),
            holes: (0..loops.len()).filter(|&h| parent[h] == Some(i)).map(|h| loops[h].clone()).collect(),
        })
        .collect();
    Profiles { regions, open_ends, faces }
}

fn poly_centroid(poly: &[P2]) -> P2 {
    let s = poly.iter().fold([0.0, 0.0], |a, p| v2::add(a, *p));
    v2::scale(s, 1.0 / poly.len() as f64)
}

/// Loops directly inside closed area `f` (its holes), among the sketch's smallest areas.
fn face_holes(faces: &[Loop], f: usize) -> Vec<Loop> {
    let fl = &faces[f];
    let inside: Vec<usize> = (0..faces.len())
        .filter(|&g| {
            let gl = &faces[g];
            g != f && gl.area < fl.area && !gl.ents.iter().any(|e| fl.ents.contains(e)) && point_in_poly(poly_centroid(&gl.poly), &fl.poly)
        })
        .collect();
    inside
        .iter()
        .filter(|&&g| {
            let c = poly_centroid(&faces[g].poly);
            !inside.iter().any(|&h| h != g && faces[h].area > faces[g].area && point_in_poly(c, &faces[h].poly))
        })
        .map(|&g| faces[g].clone())
        .collect()
}

/// The region (smallest closed area containing `p`, with its holes), and the index of
/// that area in `faces`.
pub fn region_at_point(faces: &[Loop], p: P2) -> Option<(usize, Region)> {
    let mut best: Option<usize> = None;
    for (i, f) in faces.iter().enumerate() {
        if best.is_none_or(|b| f.area < faces[b].area) && point_in_poly(p, &f.poly) {
            best = Some(i);
        }
    }
    best.map(|b| (b, Region { outer: faces[b].clone(), holes: face_holes(faces, b) }))
}

fn norm_angle(a: f64) -> f64 {
    a.sin().atan2(a.cos())
}

fn end_tangents(g: &Geo, e: &super::geo::Ent) -> (f64, f64) {
    match &e.kind {
        Kind::Line { p1, p2 } => {
            let (a, b) = (g.p(p1), g.p(p2));
            ((b[1] - a[1]).atan2(b[0] - a[0]), (a[1] - b[1]).atan2(a[0] - b[0]))
        }
        Kind::Spline { .. } => {
            let s = g.sample(e, 64);
            let n = s.len();
            ((s[1][1] - s[0][1]).atan2(s[1][0] - s[0][0]), (s[n - 2][1] - s[n - 1][1]).atan2(s[n - 2][0] - s[n - 1][0]))
        }
        Kind::Arc { c, p1, p2 } => {
            let a = g.arc(c, p1, p2);
            // Nudged by curvature so tangent-continuous junctions still sort deterministically.
            let t1 = a.a0 + std::f64::consts::FRAC_PI_2 + 1e-4 * a.sweep.signum();
            let t2 = a.a1 - std::f64::consts::FRAC_PI_2 - 1e-4;
            (norm_angle(t1), norm_angle(t2))
        }
        _ => (0.0, 0.0),
    }
}

fn to_seg(g: &Geo, h: &HalfEdge) -> Seg {
    let e = &g.entities[&h.ent];
    let (a, b) = (g.p(&h.from), g.p(&h.to));
    match &e.kind {
        Kind::Arc { c, p1, p2 } => Seg::Arc { a, b, mid: g.arc(c, p1, p2).mid },
        Kind::Spline { pts } => {
            let mut s = g.sample(e, 64);
            if pts.first() != Some(&h.from) {
                s.reverse();
            }
            Seg::Spline { a, b, pts: s }
        }
        _ => Seg::Line { a, b },
    }
}

fn reverse_segs(segs: &[Seg]) -> Vec<Seg> {
    segs.iter()
        .rev()
        .map(|s| match s {
            Seg::Line { a, b } => Seg::Line { a: *b, b: *a },
            Seg::Arc { a, b, mid } => Seg::Arc { a: *b, b: *a, mid: *mid },
            Seg::Spline { a, b, pts } => Seg::Spline { a: *b, b: *a, pts: pts.iter().rev().copied().collect() },
            other => other.clone(),
        })
        .collect()
}

fn segs_to_poly(segs: &[Seg]) -> Vec<P2> {
    let mut out = vec![];
    for s in segs {
        match s {
            Seg::Line { a, .. } => out.push(*a),
            Seg::Spline { pts, .. } => out.extend_from_slice(&pts[..pts.len() - 1]),
            Seg::Arc { a, b, mid } => {
                let p = sample_three_point_arc(*a, *mid, *b, 24);
                out.extend_from_slice(&p[..p.len() - 1]);
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::geo::Ent;

    fn geo(points: &[(&str, P2)], lines: &[(&str, &str)]) -> Geo {
        let mut g = Geo::default();
        for (id, p) in points {
            g.points.insert(id.to_string(), *p);
        }
        for (i, (a, b)) in lines.iter().enumerate() {
            let id = format!("l{i}");
            g.entities.insert(id.clone(), Ent { id, kind: Kind::Line { p1: a.to_string(), p2: b.to_string() }, construction: None, ext: false });
        }
        g
    }

    #[test]
    fn square_with_dangling_line_and_hole() {
        let mut g = geo(
            &[("a", [0.0, 0.0]), ("b", [10.0, 0.0]), ("c", [10.0, 10.0]), ("d", [0.0, 10.0]), ("x", [20.0, 20.0])],
            &[("a", "b"), ("b", "c"), ("c", "d"), ("d", "a"), ("c", "x")],
        );
        g.points.insert("o".into(), [5.0, 5.0]);
        g.entities.insert("k".into(), Ent { id: "k".into(), kind: Kind::Circle { c: "o".into(), r: 2.0 }, construction: None, ext: false });
        let p = extract(&g);
        assert_eq!(p.open_ends, 1);
        assert_eq!(p.regions.len(), 1);
        assert!((p.regions[0].outer.area - 100.0).abs() < 1e-9);
        assert_eq!(p.regions[0].holes.len(), 1);
        // Outer loops are counter-clockwise.
        assert!(signed_area(&p.regions[0].outer.poly) > 0.0);
        let (_, r) = region_at_point(&p.faces, [1.0, 1.0]).unwrap();
        assert_eq!(r.holes.len(), 1);
        let (_, r) = region_at_point(&p.faces, [5.0, 5.0]).unwrap();
        assert!(r.holes.is_empty() && matches!(r.outer.segs[0], Seg::Circle { .. }));
    }
}
