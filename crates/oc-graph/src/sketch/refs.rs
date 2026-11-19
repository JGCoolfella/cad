//! Sketch relations to model vertices and edges (`src/sketch/refs.ts`,
//! `resolveSketchRefs` in `engine.ts`): re-find each reference on the body as it stands
//! before the sketch, move the sketch geometry standing for it, and re-solve the sketch
//! with `oc-sketch`, so the sketch follows model changes. References that cannot be found
//! keep their last position and the sketch gets a warning.

use oc_doc::{EdgeSig, SketchData, SketchEntity, SketchRef};
use oc_kernel::Kernel;

use super::geo::circumcenter;
use crate::eval::{Ctx, FResult};
use crate::math::{to_frame, v2, v3, Frame, P2};
use crate::resolve::edge_sig;

enum RefShape {
    Point(P2),
    Line(P2, P2),
    Circle(P2, f64),
    Arc { c: P2, a: P2, b: P2 },
}

fn height(f: &Frame, p: [f64; 3]) -> f64 {
    v3::dot(v3::sub(p, f.origin), f.normal)
}

/// The reference projected onto the sketch plane, or `None` when it has no sketch
/// counterpart (`refShape`).
fn ref_shape(r: &SketchRef, f: &Frame) -> Option<RefShape> {
    let s: &EdgeSig = match r {
        SketchRef::Vertex { p, .. } => return Some(RefShape::Point(to_frame(f, *p))),
        SketchRef::Edge { sig, .. } | SketchRef::External { sig, .. } => sig,
    };
    let (a, b, m) = (to_frame(f, s.a), to_frame(f, s.b), to_frame(f, s.mid));
    let scale = s.length.max(1.0);
    if s.curve == "LINE" {
        return (v2::dist(a, b) > 1e-9 * scale).then_some(RefShape::Line(a, b));
    }
    if s.curve != "CIRCLE" {
        return None;
    }
    let h = height(f, s.a);
    if (height(f, s.mid) - h).abs() > 1e-7 * scale || (height(f, s.b) - h).abs() > 1e-7 * scale {
        return None;
    }
    if v3::dist(s.a, s.b) < 1e-7 * scale {
        return Some(RefShape::Circle(v2::lerp(a, m, 0.5), v2::dist(a, m) / 2.0));
    }
    let c = circumcenter(a, m, b)?;
    let ccw = (m[0] - a[0]) * (b[1] - a[1]) - (m[1] - a[1]) * (b[0] - a[0]) < 0.0;
    Some(if ccw { RefShape::Arc { c, a, b } } else { RefShape::Arc { c, a: b, b: a } })
}

fn set(sk: &mut SketchData, pid: &oc_doc::Id, p: P2) {
    if let Some(q) = sk.points.get_mut(pid) {
        q.x = p[0];
        q.y = p[1];
    }
}

/// Move every found reference's geometry to where its model geometry projects. Returns
/// the references whose geometry no longer fits (`updateRefGeometry`).
fn update_ref_geometry(sk: &mut SketchData, f: &Frame, found: &[String]) -> usize {
    let refs: Vec<SketchRef> = sk.refs.as_ref().map(|r| r.values().cloned().collect()).unwrap_or_default();
    let mut bad = 0;
    for r in &refs {
        let rid = r.id().as_str();
        if !found.iter().any(|x| x == rid) {
            continue;
        }
        let entity = sk.entities.values().find(|e| entity_ext(e) == Some(rid)).cloned();
        let point = sk.points.values().find(|p| p.ext.as_ref().map(|x| x.as_str()) == Some(rid)).map(|p| p.id.clone());
        let shape = ref_shape(r, f);
        let ok = match (shape, &entity, &point) {
            (Some(RefShape::Point(p)), None, Some(pid)) => {
                set(sk, pid, p);
                true
            }
            (Some(RefShape::Line(a, b)), Some(SketchEntity::Line { p1, p2, .. }), _) => {
                set(sk, p1, a);
                set(sk, p2, b);
                true
            }
            (Some(RefShape::Circle(c, rad)), Some(SketchEntity::Circle { id, c: cid, .. }), _) => {
                set(sk, cid, c);
                if let Some(SketchEntity::Circle { r, .. }) = sk.entities.get_mut(id) {
                    *r = rad;
                }
                true
            }
            (Some(RefShape::Arc { c, a, b }), Some(SketchEntity::Arc { c: cid, p1, p2, .. }), _) => {
                set(sk, cid, c);
                set(sk, p1, a);
                set(sk, p2, b);
                true
            }
            _ => false,
        };
        if !ok {
            bad += 1;
        }
    }
    bad
}

fn entity_ext(e: &SketchEntity) -> Option<&str> {
    match e {
        SketchEntity::Line { ext, .. } | SketchEntity::Circle { ext, .. } | SketchEntity::Arc { ext, .. } => ext.as_ref().map(|x| x.as_str()),
        _ => None,
    }
}

/// Where a legacy vertex reference (no name) is now, as the TypeScript engine finds it:
/// the end shared by most of the vertex's re-found edges (nearest to where it was on
/// ties); without edges, the nearest vertex.
fn legacy_vertex<K: Kernel>(c: &Ctx<'_, K>, p: [f64; 3], edges: Option<&Vec<EdgeSig>>) -> FResult<Option<[f64; 3]>> {
    let Some(s) = c.shape() else { return Ok(None) };
    let t = c.topo(&s)?;
    let mut cand: Vec<[f64; 3]> = vec![];
    for sig in edges.into_iter().flatten() {
        if let Some((_, e)) = c.find_edge(sig)? {
            let x = &t.edges[e.index()];
            cand.extend([x.start, x.end]);
        }
    }
    let ends: Vec<[f64; 3]> = if cand.is_empty() { t.edges.iter().flat_map(|e| [e.start, e.end]).collect() } else { vec![] };
    let pool = if cand.is_empty() { &ends } else { &cand };
    let mut best: Option<[f64; 3]> = None;
    let mut best_score = f64::NEG_INFINITY;
    for q in pool {
        let shared = cand.iter().filter(|c| v3::dist(*q, **c) < 1e-6).count() as f64;
        let score = shared * 1e9 - v3::dist(*q, p);
        if score > best_score {
            best = Some(*q);
            best_score = score;
        }
    }
    Ok(best)
}

/// Re-find the references on the body before the sketch (if any), move their geometry
/// and re-solve. Returns the updated sketch, whose references carry their current
/// position and name, and the number of references that could not be found or no longer
/// fit.
pub fn resolve<K: Kernel>(c: &Ctx<'_, K>, src: &SketchData, frame: &Frame) -> FResult<(SketchData, usize)> {
    let mut sk = src.clone();
    let mut found: Vec<String> = vec![];
    let refs: Vec<SketchRef> = sk.refs.as_ref().map(|r| r.values().cloned().collect()).unwrap_or_default();
    for r in refs {
        let updated = match &r {
            // A named vertex follows the model by name.
            SketchRef::Vertex { id, p, edges, name: Some(name) } => {
                c.find_vertex(*p, Some(name))?.map(|(q, n)| SketchRef::Vertex { id: id.clone(), p: q, edges: edges.clone(), name: n })
            }
            // A legacy one is found the old way, then named by the model vertex it landed on.
            SketchRef::Vertex { id, p, edges, name: None } => match legacy_vertex(c, *p, edges.as_ref())? {
                Some(q) => {
                    let name = c.name_vertex_at(*p, q)?;
                    Some(SketchRef::Vertex { id: id.clone(), p: q, edges: edges.clone(), name })
                }
                None => None,
            },
            SketchRef::Edge { id, sig } => match c.find_edge(sig)? {
                Some((s, e)) => Some(SketchRef::Edge { id: id.clone(), sig: edge_sig(c.topo(&s)?, Some(&s.names), e) }),
                None => None,
            },
            // Another component's edge (G1d): its stored geometry, as the assembly last
            // updated it.
            SketchRef::External { .. } => Some(r.clone()),
        };
        if let Some(u) = updated {
            found.push(r.id().as_str().to_string());
            if let Some(refs) = sk.refs.as_mut() {
                refs.insert(r.id().clone(), u);
            }
        }
    }
    let total = sk.refs.as_ref().map_or(0, |r| r.len());
    let bad = update_ref_geometry(&mut sk, frame, &found);
    solve(&mut sk);
    Ok((sk, total - found.len() + bad))
}

/// Solve the sketch in place with `oc-sketch` (`solveSketch`): points and circle radii
/// are written back.
pub(crate) fn solve(sk: &mut SketchData) {
    let Ok(v) = serde_json::to_value(&*sk) else { return };
    let Ok(mut s) = serde_json::from_value::<oc_sketch::SketchData>(v) else { return };
    oc_sketch::solve_sketch(&mut s);
    for p in s.points.values() {
        if let Some(q) = sk.points.get_mut(p.id.as_str()) {
            q.x = p.x;
            q.y = p.y;
        }
    }
    for e in s.entities.values() {
        if let oc_sketch::SketchEntity::Circle { id, r, .. } = e {
            if let Some(SketchEntity::Circle { r: dst, .. }) = sk.entities.get_mut(id.as_str()) {
                *dst = *r;
            }
        }
    }
}

/// The warning a sketch shows for `missing` references.
pub fn warning(missing: usize) -> Option<String> {
    match missing {
        0 => None,
        1 => Some("A model reference of this sketch could not be found".into()),
        n => Some(format!("{n} model references of this sketch could not be found")),
    }
}
