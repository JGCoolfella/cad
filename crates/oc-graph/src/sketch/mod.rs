//! Sketch → kernel input: regions to extrude or revolve (all closed areas, Selected
//! Contours, Thin Feature walls), sweep paths, and their world-space curves.

pub mod chain;
pub mod geo;
pub mod profile;
pub mod refs;

use oc_doc::{ThinOptions, ThinType, Vec2};
use oc_kernel::{Segment, Wire};

use crate::math::{to_world, v2, v3, Frame, P3};
use crate::FeatureError;
use geo::{spline_through, Geo, Kind};
use profile::{Region, Seg};

/// Regions of a sketch to build from (`profileRegions`): Thin Feature walls, the
/// Selected Contours, or every closed region.
pub fn profile_regions(g: &Geo, thin: Option<&ThinOptions>, contours: Option<&[Vec2]>) -> Result<Vec<Region>, FeatureError> {
    if let Some(t) = thin {
        return thin_regions(g, t);
    }
    let prof = profile::extract(g);
    if prof.regions.is_empty() {
        return Err(if prof.open_ends > 0 {
            "The sketch contains open contours. Use Thin Feature, or close the sketch."
        } else {
            "The sketch has no closed contours"
        }
        .into());
    }
    let Some(points) = contours.filter(|c| !c.is_empty()) else { return Ok(prof.regions) };
    // Each picked point selects the smallest closed area around it (with its holes).
    let mut picked: Vec<(usize, Region)> = vec![];
    for p in points {
        if let Some((i, r)) = profile::region_at_point(&prof.faces, *p) {
            if !picked.iter().any(|(j, _)| *j == i) {
                picked.push((i, r));
            }
        }
    }
    if picked.is_empty() {
        return Err("The selected contours no longer exist".into());
    }
    Ok(picked.into_iter().map(|(_, r)| r).collect())
}

/// Thin Feature: walls of the given thickness along the sketch (`thinRegions`).
fn thin_regions(src: &Geo, o: &ThinOptions) -> Result<Vec<Region>, FeatureError> {
    let mut g = src.clone();
    let ids: Vec<String> = g.entities.values().filter(|e| !matches!(e.kind, Kind::Point { .. }) && e.solid()).map(|e| e.id.clone()).collect();
    if ids.is_empty() {
        return Err("The sketch is empty".into());
    }
    let t1 = o.thickness;
    let t2 = o.thickness2.unwrap_or(o.thickness);
    let rev = o.reverse == Some(true);
    let run = |g: &mut Geo, d: f64, reverse: bool| chain::offset_entities(g, &ids, d, reverse).map_err(FeatureError::from);
    let (a, b) = match o.kind {
        ThinType::MidPlane => (run(&mut g, t1 / 2.0, rev)?, run(&mut g, t1 / 2.0, !rev)?),
        ThinType::TwoDirection => (run(&mut g, t1, rev)?, run(&mut g, t2, !rev)?),
        ThinType::OneDirection => (run(&mut g, t1, rev)?, ids.clone()),
    };
    // The walls are bounded by the two offset copies (or the original and one copy).
    g.entities.retain(|id, _| a.contains(id) || b.contains(id));
    // Open chains: close each end of the wall with a straight line.
    let ends = |g: &Geo, list: &[String]| -> Vec<String> {
        let mut count: indexmap::IndexMap<String, usize> = indexmap::IndexMap::new();
        for id in list {
            if let Some(e) = g.entities.get(id) {
                if let Kind::Line { p1, p2 } | Kind::Arc { p1, p2, .. } = &e.kind {
                    for p in [p1, p2] {
                        *count.entry(p.clone()).or_default() += 1;
                    }
                }
            }
        }
        count.into_iter().filter(|(_, c)| *c == 1).map(|(p, _)| p).collect()
    };
    let (ea, eb) = (ends(&g, &a), ends(&g, &b));
    for p in ea {
        let mut best: Option<&String> = None;
        let mut bd = f64::INFINITY;
        for q in &eb {
            let d = v2::dist(g.p(&p), g.p(q));
            if d < bd {
                bd = d;
                best = Some(q);
            }
        }
        if let Some(q) = best.cloned() {
            g.add_entity(Kind::Line { p1: p, p2: q }, None);
        }
    }
    let prof = profile::extract(&g);
    if prof.regions.is_empty() {
        return Err("The thin feature could not be built from this sketch".into());
    }
    Ok(prof.regions)
}

fn seg_world(s: &Seg, f: &Frame) -> Segment {
    let w = |p: [f64; 2]| to_world(f, p);
    match s {
        Seg::Line { a, b } => Segment::Line { a: w(*a), b: w(*b) },
        Seg::Arc { a, b, mid } => Segment::Arc { a: w(*a), mid: w(*mid), b: w(*b) },
        Seg::Spline { pts, .. } => Segment::Spline { points: pts.iter().map(|p| w(*p)).collect() },
        Seg::Circle { c, r } => Segment::Circle { center: w(*c), normal: f.normal, radius: *r },
        Seg::Ellipse { c, u, ra, rb } => {
            // The kernel takes the major radius first: swap axes when the second is longer.
            let ax = if ra >= rb { *u } else { [-u[1], u[0]] };
            let major_dir = v3::sub(w(v2::add(*c, ax)), w(*c));
            Segment::Ellipse { center: w(*c), normal: f.normal, major_dir, major: ra.max(*rb), minor: ra.min(*rb) }
        }
    }
}

/// A region in world coordinates on frame `f`.
pub fn region_world(r: &Region, f: &Frame) -> oc_kernel::Region {
    let wire = |l: &profile::Loop| Wire::new(l.segs.iter().map(|s| seg_world(s, f)).collect());
    oc_kernel::Region { outer: wire(&r.outer), holes: r.holes.iter().map(wire).collect() }
}

/// A sweep path: the sketch's single connected chain, in world coordinates, plus its
/// start point and the direction from there to the point at 0.1 % of the first curve's
/// parameter range (where the TypeScript engine places a circular profile).
pub struct Path {
    pub wire: Wire,
    pub start: P3,
    pub start_dir: P3,
}

pub fn path_wire(g: &Geo, f: &Frame) -> Result<Path, FeatureError> {
    let ids: Vec<String> = g.entities.values().filter(|e| !matches!(e.kind, Kind::Point { .. }) && e.solid()).map(|e| e.id.clone()).collect();
    let chains = chain::order_chains(g, &ids);
    if chains.len() != 1 {
        return Err("The sweep path must be a single connected chain of sketch entities".into());
    }
    let w = |p: [f64; 2]| to_world(f, p);
    let mut segs = vec![];
    let mut first: Option<(P3, P3)> = None;
    for s in &chains[0].segs {
        let e = &g.entities[&s.ent];
        let (seg, start, near) = match &e.kind {
            Kind::Line { .. } => {
                let (a, b) = (w(g.p(&s.from)), w(g.p(&s.to)));
                (Segment::Line { a, b }, a, b)
            }
            Kind::Circle { c, r } => {
                let x = occt_x_dir(f.normal);
                let y = v3::cross(f.normal, x);
                let center = w(g.p(c));
                let at = |t: f64| v3::add(center, v3::add(v3::scale(x, r * t.cos()), v3::scale(y, r * t.sin())));
                (Segment::Circle { center, normal: f.normal, radius: *r }, at(0.0), at(0.001 * std::f64::consts::TAU))
            }
            Kind::Spline { pts } => {
                let mut p: Vec<[f64; 2]> = spline_through(&pts.iter().map(|x| g.p(x)).collect::<Vec<_>>(), 16);
                if pts.first() != Some(&s.from) {
                    p.reverse();
                }
                let (a, b) = (w(p[0]), w(p[1]));
                (Segment::Spline { points: p.into_iter().map(w).collect() }, a, b)
            }
            Kind::Arc { c, p1, .. } => {
                let c = g.p(c);
                let (a, b) = (v2::sub(g.p(&s.from), c), v2::sub(g.p(&s.to), c));
                let t0 = a[1].atan2(a[0]);
                let mut t1 = b[1].atan2(b[0]);
                // Arcs are stored counter-clockwise p1 → p2; traversed backwards from p2.
                if s.from == *p1 {
                    while t1 <= t0 {
                        t1 += std::f64::consts::TAU;
                    }
                } else {
                    while t1 >= t0 {
                        t1 -= std::f64::consts::TAU;
                    }
                }
                let r = a[0].hypot(a[1]);
                let at = |t: f64| w(v2::add(c, v2::from_angle(t, r)));
                let seg = Segment::Arc { a: w(g.p(&s.from)), mid: at((t0 + t1) / 2.0), b: w(g.p(&s.to)) };
                (seg, w(g.p(&s.from)), at(t0 + 0.001 * (t1 - t0)))
            }
            _ => continue,
        };
        if first.is_none() {
            let near = match &seg {
                Segment::Line { a, b } => v3::add(*a, v3::scale(v3::sub(*b, *a), 0.001)),
                _ => near,
            };
            first = Some((start, near));
        }
        segs.push(seg);
    }
    let (start, near) = first.ok_or("The sweep path must be a single connected chain of sketch entities")?;
    Ok(Path { wire: Wire::new(segs), start, start_dir: v3::norm(v3::sub(near, start)) })
}

/// The X direction OCCT's `gp_Ax2(P, N)` picks for a normal (where a full circle starts).
fn occt_x_dir(n: P3) -> P3 {
    let (a, b, c) = (n[0], n[1], n[2]);
    let (aa, ba, ca) = (a.abs(), b.abs(), c.abs());
    let d = if ba <= aa && ba <= ca {
        if aa > ca {
            [-c, 0.0, a]
        } else {
            [c, 0.0, -a]
        }
    } else if aa <= ba && aa <= ca {
        if ba > ca {
            [0.0, -c, b]
        } else {
            [0.0, c, -b]
        }
    } else if aa > ba {
        [-b, a, 0.0]
    } else {
        [b, -a, 0.0]
    };
    v3::norm(d)
}
