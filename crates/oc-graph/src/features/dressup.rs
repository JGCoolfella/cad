//! Fillet and chamfer (`applyDressup` in `engine.ts`), also re-applied per instance by
//! patterns and mirrors.

use oc_doc::{ChamferFeature, ChamferType, FilletFeature};
use oc_kernel::{ChamferEdge, ChamferSize, EdgeId, FaceId, FilletEdge, Kernel, Radius, Topology};

use crate::eval::{Ctx, DressKind, Dressup, FResult};
use crate::math::v3;
use crate::resolve::{edge_sig, FACE_SCORE};

pub fn of_fillet(f: &FilletFeature) -> Dressup {
    Dressup { kind: DressKind::Fillet(f.clone()), edges: f.edges.clone(), faces: f.faces.clone(), exact: false }
}

pub fn of_chamfer(f: &ChamferFeature) -> Dressup {
    Dressup { kind: DressKind::Chamfer(f.clone()), edges: f.edges.clone(), faces: vec![], exact: false }
}

/// The face an unequal chamfer measures its first distance on.
///
/// The TypeScript engine takes "the edge's first face" in replicad's (OCCT traversal)
/// order, an enumeration artefact the kernel trait does not reproduce (E3 finding 3).
/// Resolved by geometry instead: the face on the left of the edge as it runs from its
/// start to its end, i.e. the face whose normal turns into the other face's normal
/// counter-clockwise about the edge direction. This reproduces the corpus (Chamfer2: the
/// y = 0 face, Chamfer3: the x = 40 face). `flip` takes the other face.
fn first_face(t: &Topology, e: EdgeId, flip: bool) -> FaceId {
    let x = &t.edges[e.index()];
    let faces = &x.faces;
    if faces.len() < 2 {
        return faces[0];
    }
    let (a, b) = (faces[0], faces[1]);
    let (na, nb) = (t.faces[a.index()].normal, t.faces[b.index()].normal);
    let along = v3::sub(x.end, x.start);
    let (first, second) = if v3::dot(v3::cross(na, nb), along) >= 0.0 { (a, b) } else { (b, a) };
    if flip {
        second
    } else {
        first
    }
}

/// Apply a fillet or chamfer to the edges (and fillet faces) its signatures find. Returns
/// it as applied: the edges actually used, where they were before the operation (what
/// patterns transform).
pub fn apply_dressup<K: Kernel>(c: &Ctx<'_, K>, d: &Dressup) -> FResult<Dressup> {
    let fillet = matches!(d.kind, DressKind::Fillet(_));
    let s = c.shape().ok_or(if fillet { "Nothing to fillet" } else { "Nothing to chamfer" })?;
    let t = c.topo(&s)?;
    // Instances: transformed copies are exact to modelling precision, so allow only a small error.
    let mut edges = c.resolver.resolve_edges(t, &d.edges, d.exact.then_some(0.05));
    if d.exact && edges.len() < d.edges.len() {
        return Err("Instance edges not found".into());
    }
    if fillet {
        for sig in &d.faces {
            let face = c.resolver.find_face(t, sig, if d.exact { 0.5 } else { FACE_SCORE });
            match face {
                Some(f) => edges.extend(t.faces[f.index()].edges.iter().copied()),
                None if d.exact => return Err("Instance face not found".into()),
                None => {}
            }
        }
    }
    let mut uniq: Vec<EdgeId> = vec![];
    for e in edges {
        if !uniq.contains(&e) {
            uniq.push(e);
        }
    }
    let body = match &d.kind {
        DressKind::Fillet(f) => {
            if uniq.is_empty() {
                return Err("Fillet edges could not be found".into());
            }
            let radius = match f.radius2 {
                Some(r2) => Radius::Linear(f.radius, r2),
                None => Radius::Constant(f.radius),
            };
            let list: Vec<FilletEdge> = uniq.iter().map(|&edge| FilletEdge { edge, radius }).collect();
            c.k.fillet(&s.body, &list)?.body
        }
        DressKind::Chamfer(f) => {
            if uniq.is_empty() {
                return Err("Chamfer edges could not be found".into());
            }
            let flip = f.flip == Some(true);
            let list: Vec<ChamferEdge> = uniq
                .iter()
                .map(|&edge| {
                    let size = match f.chamfer_type.unwrap_or(ChamferType::Equal) {
                        ChamferType::Equal => ChamferSize::Equal(f.distance),
                        ChamferType::DistanceDistance => {
                            ChamferSize::Distances { d1: f.distance, d2: f.distance2.unwrap_or(f.distance), face: first_face(t, edge, flip) }
                        }
                        ChamferType::AngleDistance => ChamferSize::DistanceAngle {
                            distance: f.distance,
                            angle: f.angle.unwrap_or(std::f64::consts::FRAC_PI_4),
                            face: first_face(t, edge, flip),
                        },
                    };
                    ChamferEdge { edge, size }
                })
                .collect();
            c.k.chamfer(&s.body, &list)?.body
        }
    };
    let applied = Dressup { kind: d.kind.clone(), edges: uniq.iter().map(|&e| edge_sig(t, e)).collect(), faces: vec![], exact: false };
    c.set_body(body);
    Ok(applied)
}
