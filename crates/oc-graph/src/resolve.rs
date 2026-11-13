//! Re-finding stored references on a rebuilt body.
//!
//! Until persistent naming (E6, `docs/OVERHAUL_PLAN.md` section 3) exists, references are
//! found the way the TypeScript engine finds them: by geometric signature, nearest match.
//! Features call the [`Resolver`] trait only, so E6 can replace the strategy without
//! touching them.

use oc_doc::{EdgeSig, FaceSig};
use oc_kernel::{Curve, EdgeId, FaceId, Surface, Topology};

use crate::math::v3;

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

pub fn face_sig(t: &Topology, f: FaceId) -> FaceSig {
    let x = &t.faces[f.index()];
    FaceSig { surface: surface_name(&x.surface).into(), center: x.centroid, normal: x.normal, area: x.area, name: None }
}

pub fn edge_sig(t: &Topology, e: EdgeId) -> EdgeSig {
    let x = &t.edges[e.index()];
    EdgeSig { curve: curve_name(&x.curve).into(), mid: x.mid, a: x.start, b: x.end, length: x.length, name: None }
}

/// Finds stored face and edge references on a body.
pub trait Resolver {
    /// The face best matching `sig` with a score below `max_score`.
    fn find_face(&self, t: &Topology, sig: &FaceSig, max_score: f64) -> Option<FaceId>;
    /// One edge per signature that has a match below `max_score` (default: the larger of
    /// 5 mm and the edge's length); signatures without a match are left out.
    fn resolve_edges(&self, t: &Topology, sigs: &[EdgeSig], max_score: Option<f64>) -> Vec<EdgeId>;
}

/// Default `max_score` of [`Resolver::find_face`] (`findFace` in the TypeScript engine).
pub const FACE_SCORE: f64 = 1e3;

/// Nearest-signature matching (`findFace` / `resolveEdges` in `engine.ts`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Signatures;

impl Resolver for Signatures {
    fn find_face(&self, t: &Topology, sig: &FaceSig, max_score: f64) -> Option<FaceId> {
        // Stored normals of curved faces are not unit length (replicad reports the surface
        // derivative cross product); the kernel's are. Compare directions only.
        let n = v3::norm(sig.normal);
        let mut best = None;
        let mut best_score = f64::INFINITY;
        for (i, f) in t.faces.iter().enumerate() {
            if surface_name(&f.surface) != sig.surface {
                continue;
            }
            let score = v3::dist(f.centroid, sig.center) + 10.0 * (1.0 - v3::dot(f.normal, n)) + (f.area.sqrt() - sig.area.sqrt()).abs();
            if score < best_score {
                best_score = score;
                best = Some(FaceId(i as u32));
            }
        }
        best.filter(|_| best_score < max_score)
    }

    fn resolve_edges(&self, t: &Topology, sigs: &[EdgeSig], max_score: Option<f64>) -> Vec<EdgeId> {
        let mut out = vec![];
        for sig in sigs {
            let mut best = None;
            let mut best_score = f64::INFINITY;
            for (i, e) in t.edges.iter().enumerate() {
                if curve_name(&e.curve) != sig.curve {
                    continue;
                }
                let ends = (v3::dist(e.start, sig.a) + v3::dist(e.end, sig.b)).min(v3::dist(e.start, sig.b) + v3::dist(e.end, sig.a));
                let score = v3::dist(e.mid, sig.mid) + 0.25 * ends + 0.5 * (e.length - sig.length).abs();
                if score < best_score {
                    best_score = score;
                    best = Some(EdgeId(i as u32));
                }
            }
            if let Some(b) = best {
                if best_score < max_score.unwrap_or(sig.length.max(5.0)) {
                    out.push(b);
                }
            }
        }
        out
    }
}
