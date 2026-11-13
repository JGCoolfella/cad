//! Geometric signatures: what a reference stores beside its name, how a live face or edge
//! is scored against one, and the nearest-signature search the TypeScript engine used
//! (`findFace` / `resolveEdges` in `engine.ts`), which survives as the legacy fallback.

use oc_doc::{EdgeSig, FaceSig};
use oc_kernel::{v3, Curve, EdgeId, FaceId, Surface, Topology};

use crate::name::BodyNames;

/// Surface type names as replicad reports them (and as signatures store them).
pub fn surface_name(s: &Surface) -> &'static str {
    match s {
        Surface::Plane { .. } => "PLANE",
        Surface::Cylinder { .. } => "CYLINDRE",
        Surface::Cone { .. } => "CONE",
        Surface::Sphere { .. } => "SPHERE",
        Surface::Torus { .. } => "TORUS",
        Surface::BSpline => "BSPLINE_SURFACE",
        Surface::Bezier => "BEZIER_SURFACE",
        Surface::Revolution => "REVOLUTION_SURFACE",
        Surface::Extrusion => "EXTRUSION_SURFACE",
        Surface::Offset => "OFFSET_SURFACE",
        Surface::Other => "OTHER_SURFACE",
    }
}

/// Curve type names as replicad reports them.
pub fn curve_name(c: &Curve) -> &'static str {
    match c {
        Curve::Line => "LINE",
        Curve::Circle { .. } => "CIRCLE",
        Curve::Ellipse { .. } => "ELLIPSE",
        Curve::BSpline => "BSPLINE_CURVE",
        Curve::Bezier => "BEZIER_CURVE",
        Curve::Other => "OTHER_CURVE",
    }
}

/// The reference to face `f` as a document stores it: signature, and name when the
/// body's names are given.
pub fn face_sig(t: &Topology, names: Option<&BodyNames>, f: FaceId) -> FaceSig {
    let x = &t.faces[f.index()];
    FaceSig {
        surface: surface_name(&x.surface).into(),
        center: x.centroid,
        normal: x.normal,
        area: x.area,
        name: names.and_then(|n| n.faces.get(f.index())).map(|n| n.to_stored()),
    }
}

/// The reference to edge `e` as a document stores it.
pub fn edge_sig(t: &Topology, names: Option<&BodyNames>, e: EdgeId) -> EdgeSig {
    let x = &t.edges[e.index()];
    EdgeSig { curve: curve_name(&x.curve).into(), mid: x.mid, a: x.start, b: x.end, length: x.length, name: names.map(|n| n.edge(t, e).to_stored()) }
}

/// How far live face `f` is from `sig` (0 = identical): distance between centres, plus
/// the turn between normals, plus the difference in size. Faces of another surface type
/// do not compare (`None`).
pub fn face_score(t: &Topology, f: FaceId, sig: &FaceSig) -> Option<f64> {
    let x = &t.faces[f.index()];
    if surface_name(&x.surface) != sig.surface {
        return None;
    }
    // Stored normals of curved faces are not unit length (replicad reports the surface
    // derivative cross product); the kernel's are. Compare directions only.
    let n = v3::unit(sig.normal).unwrap_or(sig.normal);
    Some(v3::dist(x.centroid, sig.center) + 10.0 * (1.0 - v3::dot(x.normal, n)) + (x.area.sqrt() - sig.area.sqrt()).abs())
}

/// How far live edge `e` is from `sig`: mid point, end points (either way round), length.
pub fn edge_score(t: &Topology, e: EdgeId, sig: &EdgeSig) -> Option<f64> {
    let x = &t.edges[e.index()];
    if curve_name(&x.curve) != sig.curve {
        return None;
    }
    let ends = (v3::dist(x.start, sig.a) + v3::dist(x.end, sig.b)).min(v3::dist(x.start, sig.b) + v3::dist(x.end, sig.a));
    Some(v3::dist(x.mid, sig.mid) + 0.25 * ends + 0.5 * (x.length - sig.length).abs())
}

/// The best-scoring of `candidates` (the first wins ties, so pass them in index order).
pub fn best<I: Copy>(candidates: impl IntoIterator<Item = I>, score: impl Fn(I) -> Option<f64>) -> Option<(I, f64)> {
    let mut best: Option<(I, f64)> = None;
    for c in candidates {
        if let Some(s) = score(c) {
            if best.is_none_or(|(_, b)| s < b) {
                best = Some((c, s));
            }
        }
    }
    best
}

/// The face nearest `sig` among all faces, if its score is below `max_score`.
pub fn nearest_face(t: &Topology, sig: &FaceSig, max_score: f64) -> Option<FaceId> {
    let all = (0..t.faces.len() as u32).map(FaceId);
    best(all, |f| face_score(t, f, sig)).filter(|(_, s)| *s < max_score).map(|(f, _)| f)
}

/// The edge nearest `sig` among all edges, if its score is below `max_score`.
pub fn nearest_edge(t: &Topology, sig: &EdgeSig, max_score: f64) -> Option<EdgeId> {
    let all = (0..t.edges.len() as u32).map(EdgeId);
    best(all, |e| edge_score(t, e, sig)).filter(|(_, s)| *s < max_score).map(|(e, _)| e)
}
