//! Chains and offsets (`expandChain`, `orderChains`, `offsetEntities` in
//! `src/sketch/ops.ts`), as far as the engine uses them: Thin Feature walls and sweep
//! paths. Only geometry is produced; the relations and dimensions the sketch tool adds
//! do not change the walls.

use std::collections::HashSet;

use super::geo::{signed_area, Geo, Kind};
use crate::math::{circle_circle, line_circle, line_line, v2, P2};

/// One entity of a chain, traversed from `from` to `to` (circles: both empty).
#[derive(Clone, Debug, PartialEq)]
pub struct ChainSeg {
    pub ent: String,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    pub segs: Vec<ChainSeg>,
    pub closed: bool,
}

fn is_curve(k: &Kind) -> bool {
    matches!(k, Kind::Line { .. } | Kind::Arc { .. } | Kind::Circle { .. } | Kind::Spline { .. })
}

/// Entities connected end to end with `ids` (SOLIDWORKS "Select chain").
pub fn expand_chain(g: &Geo, ids: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for id in ids {
        if g.entities.get(id).is_some_and(|e| is_curve(&e.kind)) && !out.contains(id) {
            out.push(id.clone());
        }
    }
    let mut seen: HashSet<String> = out.iter().cloned().collect();
    let mut stack = out.clone();
    while let Some(id) = stack.pop() {
        let Some(e) = g.entities.get(&id) else { continue };
        let (Kind::Line { p1, p2 } | Kind::Arc { p1, p2, .. }) = &e.kind else { continue };
        for p in [p1, p2] {
            for n in g.using_point(p) {
                if matches!(n.kind, Kind::Point { .. } | Kind::Circle { .. }) || seen.contains(&n.id) || n.id == e.id {
                    continue;
                }
                if n.construction != e.construction {
                    continue;
                }
                if matches!(&n.kind, Kind::Arc { c, .. } if c == p) {
                    continue;
                }
                seen.insert(n.id.clone());
                out.push(n.id.clone());
                stack.push(n.id.clone());
            }
        }
    }
    out
}

/// Order entities into chains: each circle on its own, then paths and closed loops.
pub fn order_chains(g: &Geo, ids: &[String]) -> Vec<Chain> {
    let ents: Vec<&super::geo::Ent> = ids.iter().filter_map(|id| g.entities.get(id)).filter(|e| is_curve(&e.kind)).collect();
    let mut chains = vec![];
    for e in &ents {
        if matches!(e.kind, Kind::Circle { .. }) {
            chains.push(Chain { segs: vec![ChainSeg { ent: e.id.clone(), from: String::new(), to: String::new() }], closed: true });
        }
    }
    let mut left: Vec<&str> = ents.iter().filter(|e| !matches!(e.kind, Kind::Circle { .. })).map(|e| e.id.as_str()).collect();
    let ends = |e: &super::geo::Ent| -> [String; 2] {
        let (a, b) = e.ends().unwrap_or(("", ""));
        [a.to_string(), b.to_string()]
    };
    while !left.is_empty() {
        let remaining: Vec<&super::geo::Ent> = ents.iter().copied().filter(|e| left.contains(&e.id.as_str())).collect();
        let mut start = remaining[0];
        let mut from = ends(start)[0].clone();
        'search: for e in &remaining {
            for p in ends(e) {
                if remaining.iter().filter(|x| ends(x).contains(&p)).count() == 1 {
                    start = e;
                    from = p;
                    break 'search;
                }
            }
        }
        let mut segs = vec![];
        let mut cur = Some(start);
        let mut p = from;
        while let Some(c) = cur {
            left.retain(|x| *x != c.id);
            let [e1, e2] = ends(c);
            let to = if e1 == p { e2 } else { e1 };
            segs.push(ChainSeg { ent: c.id.clone(), from: p.clone(), to: to.clone() });
            p = to;
            cur = remaining.iter().copied().find(|x| left.contains(&x.id.as_str()) && ends(x).contains(&p));
        }
        let closed = segs.len() > 1 && segs[segs.len() - 1].to == segs[0].from;
        chains.push(Chain { segs, closed });
    }
    chains
}

#[derive(Clone, Copy, Debug)]
enum Off {
    Line { a: P2, b: P2 },
    /// Arcs carry their offset ends.
    Circle { c: P2, r: f64, ends: Option<(P2, P2)> },
}

/// Offset curve of one segment: positive `d` is to the left of the traversal direction
/// (circles: outward).
fn offset_seg(g: &Geo, s: &ChainSeg, d: f64) -> Option<Off> {
    match &g.entities[&s.ent].kind {
        Kind::Line { .. } => {
            let (a, b) = (g.p(&s.from), g.p(&s.to));
            let n = v2::perp(v2::norm(v2::sub(b, a)));
            Some(Off::Line { a: v2::add(a, v2::scale(n, d)), b: v2::add(b, v2::scale(n, d)) })
        }
        Kind::Circle { c, r } => (r + d > 1e-9).then(|| Off::Circle { c: g.p(c), r: r + d, ends: None }),
        Kind::Arc { c, p1, p2 } => {
            let a = g.arc(c, p1, p2);
            let cc = g.p(c);
            let r = if s.from == *p1 { a.r - d } else { a.r + d };
            if r <= 1e-9 {
                return None;
            }
            let at = |p: &str| v2::add(cc, v2::scale(v2::norm(v2::sub(g.p(p), cc)), r));
            Some(Off::Circle { c: cc, r, ends: Some((at(&s.from), at(&s.to))) })
        }
        _ => None,
    }
}

fn intersect(a: Off, b: Off, near: P2) -> P2 {
    let pts = match (a, b) {
        (Off::Line { a, b }, Off::Line { a: c, b: d }) => line_line(a, b, c, d).into_iter().collect(),
        (Off::Line { a, b }, Off::Circle { c, r, .. }) | (Off::Circle { c, r, .. }, Off::Line { a, b }) => line_circle(a, b, c, r),
        (Off::Circle { c, r, .. }, Off::Circle { c: c2, r: r2, .. }) => circle_circle(c, r, c2, r2),
    };
    pts.into_iter().reduce(|best, p| if v2::dist(p, near) < v2::dist(best, near) { p } else { best }).unwrap_or(near)
}

/// Signed area of a closed chain (positive = counter-clockwise traversal).
fn chain_area(g: &Geo, segs: &[ChainSeg]) -> f64 {
    let mut poly = vec![];
    for s in segs {
        let e = &g.entities[&s.ent];
        let mut pts = g.sample(e, 16);
        let fwd = match &e.kind {
            Kind::Line { p1, .. } | Kind::Arc { p1, .. } => s.from == *p1,
            _ => true,
        };
        if !fwd {
            pts.reverse();
        }
        poly.extend_from_slice(&pts[..pts.len() - 1]);
    }
    signed_area(&poly)
}

/// Offset Entities with chain selection (`offsetEntities(..., { selectChain: true })`).
/// Adds the offset geometry to `g` and returns the new entity ids.
pub fn offset_entities(g: &mut Geo, ids: &[String], distance: f64, reverse: bool) -> Result<Vec<String>, String> {
    let sel = expand_chain(g, ids);
    let chains = order_chains(g, &sel);
    if chains.is_empty() {
        return Err("Select sketch entities to offset.".into());
    }
    let mut created = vec![];
    for ch in &chains {
        let mut d = distance * if reverse { -1.0 } else { 1.0 };
        if ch.closed && !matches!(g.entities[&ch.segs[0].ent].kind, Kind::Circle { .. }) {
            d *= if chain_area(g, &ch.segs) > 0.0 { -1.0 } else { 1.0 };
        }
        created.extend(offset_chain(g, ch, d)?);
    }
    Ok(created)
}

fn offset_chain(g: &mut Geo, ch: &Chain, d: f64) -> Result<Vec<String>, String> {
    let segs = &ch.segs;
    if segs.iter().any(|s| matches!(g.entities[&s.ent].kind, Kind::Spline { .. })) {
        return Err("Offsetting splines is not supported yet.".into());
    }
    let curves: Vec<Off> = segs
        .iter()
        .map(|s| offset_seg(g, s, d))
        .collect::<Option<_>>()
        .ok_or("The offset distance is too large for the selected arcs or circles.")?;
    let construction = Some(false);
    if segs.len() == 1 {
        if let Off::Circle { c, r, ends: None } = curves[0] {
            let cid = g.add_point(c);
            return Ok(vec![g.add_entity(Kind::Circle { c: cid, r }, construction)]);
        }
    }
    let n = segs.len();
    let start = |i: usize| match curves[i] {
        Off::Line { a, .. } => a,
        Off::Circle { ends, .. } => ends.map_or([f64::NAN; 2], |e| e.0),
    };
    let end = |i: usize| match curves[i] {
        Off::Line { b, .. } => b,
        Off::Circle { ends, .. } => ends.map_or([f64::NAN; 2], |e| e.1),
    };
    let mut joint: Vec<String> = vec![];
    for i in 0..=n {
        if i == 0 && !ch.closed {
            joint.push(g.add_point(start(0)));
        } else if i == n && !ch.closed {
            joint.push(g.add_point(end(n - 1)));
        } else if i == n {
            joint.push(joint[0].clone());
        } else {
            let prev = (i + n - 1) % n;
            let near = v2::lerp(end(prev), start(i), 0.5);
            let pt = if v2::dist(end(prev), start(i)) < 1e-9 { near } else { intersect(curves[prev], curves[i], near) };
            joint.push(g.add_point(pt));
        }
    }
    let mut created = vec![];
    for (i, s) in segs.iter().enumerate() {
        let (a, b) = (joint[i].clone(), joint[i + 1].clone());
        let kind = match g.entities[&s.ent].kind.clone() {
            Kind::Line { p1, .. } => {
                let fwd = s.from == p1;
                Kind::Line { p1: if fwd { a.clone() } else { b.clone() }, p2: if fwd { b } else { a } }
            }
            Kind::Arc { c, p1, .. } => {
                let cid = g.add_point(g.p(&c));
                let ccw = s.from == p1;
                Kind::Arc { c: cid, p1: if ccw { a.clone() } else { b.clone() }, p2: if ccw { b } else { a } }
            }
            _ => continue,
        };
        created.push(g.add_entity(kind, construction));
    }
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::geo::Ent;

    #[test]
    fn closed_square_offsets_outward() {
        let mut g = Geo::default();
        for (id, p) in [("a", [0.0, 0.0]), ("b", [10.0, 0.0]), ("c", [10.0, 10.0]), ("d", [0.0, 10.0])] {
            g.points.insert(id.into(), p);
        }
        for (id, a, b) in [("l1", "a", "b"), ("l2", "b", "c"), ("l3", "c", "d"), ("l4", "d", "a")] {
            g.entities.insert(id.into(), Ent { id: id.into(), kind: Kind::Line { p1: a.into(), p2: b.into() }, construction: None, ext: false });
        }
        let ch = order_chains(&g, &expand_chain(&g, &["l1".to_string()]));
        assert_eq!(ch.len(), 1);
        assert!(ch[0].closed && ch[0].segs.len() == 4);
        let made = offset_entities(&mut g, &["l1".to_string()], 1.0, false).unwrap();
        assert_eq!(made.len(), 4);
        let xs: Vec<P2> = g.points.values().skip(4).copied().collect();
        assert!(xs.iter().all(|p| (p[0] == -1.0 || p[0] == 11.0) && (p[1] == -1.0 || p[1] == 11.0)));
    }
}
