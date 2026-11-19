//! Resolution (`docs/OVERHAUL_PLAN.md` section 3.3): finding a stored reference on the
//! live body. A reference is its persistent name plus a geometric signature:
//!
//! 1. Candidates are the live entities whose name shares a tag with the stored name (for
//!    an edge or vertex: whose faces pair up with the stored faces that way). One: done.
//! 2. Several (the entity was split): the best signature score among them.
//!
//!    None, but exactly one live face is of the same feature and role as the stored name
//!    (same tag up to its key, e.g. the one end cap of an extrusion whose profile was
//!    redrawn with new entities): that face, reported as [`How::Role`]. No warning; the
//!    caller writes the face's current name back. For an edge or vertex, a stored face
//!    with no tag-sharing face is stood in for by such a face. (Section 3.3 step 2a.)
//!    A stand-in that moved by more than the size tolerance ([`Rules::size`]) is not the
//!    same entity (owner decision 2026-11-16, G1c): with two holes cut from one sketch,
//!    deleting the circle of one leaves the other as the only face of that feature and
//!    role, and a reference to the deleted hole must go missing, as in SOLIDWORKS, not
//!    move onto its neighbour.
//!    End caps are exempt (owner decision 2026-11-19, G1d): a `start`, `end` or `mid` cap
//!    whose feature still has exactly one cap of that role is that cap wherever it moved
//!    or however it was resized, since SOLIDWORKS names caps by feature, not by loop. A
//!    redrawn profile that also moves or grows keeps its references to the cap.
//! 3. Else a strict signature match, reported as [`How::Position`] so the feature can
//!    warn "re-attached by position"; else the reference is missing.
//! 4. A legacy reference (no name): the loose nearest-signature search of the TypeScript
//!    engine, reported as [`How::Legacy`] so the name found can be written back.
//!
//! A reference always resolves to live topology; nothing downstream reads geometry out
//! of a stored signature.

use std::cell::OnceCell;

use oc_doc::{EdgeSig, FaceSig};
use oc_kernel::{v3, Curve, EdgeId, FaceId, Surface, Topology, VertexId, P3};

use crate::name::{vertex_faces, BodyNames, EdgeName, FaceName, VertexName};
use crate::signature::{best, curve_name, edge_score, face_score, nearest_edge, nearest_face, surface_name};

/// How a reference was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum How {
    /// By name, one candidate.
    Name,
    /// By name, several candidates (a split), told apart by signature.
    Split,
    /// The key of the name is gone, but one face of the same feature and role exists: found
    /// by that. The current name should be written back.
    Role,
    /// The name is gone; an entity at the stored position was accepted. Worth a warning.
    Position,
    /// The reference had no name; found by signature. The name should be written back.
    Legacy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found<I> {
    pub id: I,
    pub how: How,
}

/// Matching limits. Naming's own, not modelling tolerances.
#[derive(Clone, Copy, Debug)]
pub struct Rules {
    /// Step 3: points and planes within this distance coincide (mm, scaled by the
    /// coordinates' magnitude above 1).
    pub linear: f64,
    /// Step 3: directions within this angle are the same (radians).
    pub angular: f64,
    /// Step 3, entities other than planes and lines: allowed move of the centre and change
    /// of size, as a fraction of the stored size (√area or length). Step 2a: how far a
    /// stand-in of the same feature and role may have moved.
    pub size: f64,
    /// Step 4, faces: the TypeScript engine's `findFace` limit.
    pub legacy_face_score: f64,
    /// Step 4, edges: the limit is the edge's length, but at least this (mm).
    pub legacy_edge_min: f64,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { linear: 1e-6, angular: 1e-6, size: 0.25, legacy_face_score: 1e3, legacy_edge_min: 5.0 }
    }
}

/// A body to resolve references on: its topology and names.
pub struct Live<'a> {
    pub topo: &'a Topology,
    pub names: &'a BodyNames,
    around: OnceCell<Vec<Vec<FaceId>>>,
}

impl<'a> Live<'a> {
    pub fn new(topo: &'a Topology, names: &'a BodyNames) -> Self {
        Live { topo, names, around: OnceCell::new() }
    }

    fn around(&self) -> &[Vec<FaceId>] {
        self.around.get_or_init(|| vertex_faces(self.topo))
    }

    pub fn edge_name(&self, e: EdgeId) -> EdgeName {
        self.names.edge(self.topo, e)
    }

    pub fn vertex_name(&self, v: VertexId) -> VertexName {
        self.names.vertex(self.around(), v)
    }

    fn scale(&self, p: P3) -> f64 {
        p.iter().fold(1.0f64, |m, x| m.max(x.abs()))
    }

    /// The live faces a stored face name stands for: those sharing a tag with it, or, when
    /// there is none, the one face of the same feature and role (step 2a). The second
    /// value says whether, and how, the second rule was used.
    fn targets(&self, stored: &FaceName) -> (Vec<FaceId>, Stand) {
        let ids = |by: &dyn Fn(&FaceName) -> bool| -> Vec<FaceId> {
            self.names.faces.iter().enumerate().filter(|(_, n)| by(n)).map(|(i, _)| FaceId(i as u32)).collect()
        };
        let by_tag = ids(&|n| n.shares_tag(stored));
        if !by_tag.is_empty() {
            return (by_tag, Stand::No);
        }
        let by_role = ids(&|n| n.shares_stem(stored));
        let [one] = by_role.as_slice() else { return (vec![], Stand::No) };
        // The stem the stand-in shares: a cap's (start, end, mid) is exempt from the
        // distance check.
        let live = &self.names.faces[one.index()];
        let cap = stored.tags().iter().any(|a| CAPS.contains(&a.role()) && live.tags().iter().any(|b| a.stem() == b.stem()));
        (by_role, if cap { Stand::Cap } else { Stand::Role })
    }
}

/// The roles of an extrusion's, revolution's or sweep's caps.
const CAPS: [&str; 3] = ["start", "end", "mid"];

/// How a stored face found its live face (see [`Live::targets`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stand {
    /// By tag, or not at all.
    No,
    /// By the one face of the same feature and role; it must not have moved far.
    Role,
    /// By the one cap of the same feature and role; wherever it is (caps are named by
    /// feature).
    Cap,
}

impl Stand {
    /// Over the faces naming an edge or a vertex: whether any stand-in was used, and
    /// whether all those used were caps (then the distance check is waived).
    fn over(all: impl IntoIterator<Item = Stand>) -> (bool, bool) {
        all.into_iter().fold((false, true), |(used, caps), s| (used || s != Stand::No, caps && s != Stand::Role))
    }
}

/// A match that needed a stand-in face (step 2a) is reported as found by role, unless it
/// moved too far from where the reference was (`stayed`), when it is not the same entity.
/// Caps alone (`caps`) are exempt from that.
fn by_role<I: Copy>(found: Option<Found<I>>, (used, caps): (bool, bool), stayed: impl Fn(I) -> bool) -> Option<Found<I>> {
    match found {
        Some(f) if used => (caps || stayed(f.id)).then_some(Found { id: f.id, how: How::Role }),
        other => other,
    }
}

/// Where an edge is, for telling whether it moved: its middle, or for a closed edge (a full
/// circle, whose middle point depends on where its parameter starts) the point halfway
/// between its start and its middle, which is the centre of a circle.
fn edge_place(start: P3, end: P3, mid: P3, length: f64) -> P3 {
    if v3::dist(start, end) <= 1e-9 * length.max(1.0) {
        v3::scale(v3::add(start, mid), 0.5)
    } else {
        mid
    }
}

fn pick<I: Copy>(candidates: Vec<I>, score: impl Fn(I) -> Option<f64>, distance: impl Fn(I) -> f64) -> Option<Found<I>> {
    match candidates.as_slice() {
        [] => None,
        [one] => Some(Found { id: *one, how: How::Name }),
        // A candidate of another type has no score; it still beats nothing at all.
        _ => best(candidates.iter().copied(), &score)
            .or_else(|| best(candidates.iter().copied(), |c| Some(distance(c))))
            .map(|(id, _)| Found { id, how: How::Split }),
    }
}

/// Find the face a stored reference names.
pub fn face(live: &Live<'_>, sig: &FaceSig, rules: &Rules) -> Option<Found<FaceId>> {
    let t = live.topo;
    let Some(stored) = &sig.name else {
        return nearest_face(t, sig, rules.legacy_face_score).map(|id| Found { id, how: How::Legacy });
    };
    let stored = FaceName::from_stored(stored);
    let ids = || (0..t.faces.len() as u32).map(FaceId);
    let (candidates, role) = live.targets(&stored);
    let size = sig.area.max(0.0).sqrt();
    let stayed = |f: FaceId| v3::dist(t.faces[f.index()].centroid, sig.center) <= rules.size * size;
    if let Some(found) = by_role(pick(candidates, |f| face_score(t, f, sig), |f| v3::dist(t.faces[f.index()].centroid, sig.center)), Stand::over([role]), stayed) {
        return Some(found);
    }
    // Step 3. Planes: the same plane facing the same way, wherever on it the face now is.
    // Other surfaces: about the same place and size.
    let n = v3::unit(sig.normal).unwrap_or(sig.normal);
    let strict = ids().filter(|f| {
        let x = &t.faces[f.index()];
        if surface_name(&x.surface) != sig.surface {
            return false;
        }
        if matches!(x.surface, Surface::Plane { .. }) {
            v3::dot(x.normal, n) >= (rules.angular).cos() && v3::dot(v3::sub(x.centroid, sig.center), n).abs() <= rules.linear * live.scale(sig.center)
        } else {
            v3::dist(x.centroid, sig.center) <= rules.size * size && (x.area.sqrt() - size).abs() <= rules.size * size
        }
    });
    best(strict, |f| face_score(t, f, sig)).map(|(id, _)| Found { id, how: How::Position })
}

/// Find the edge a stored reference names.
pub fn edge(live: &Live<'_>, sig: &EdgeSig, rules: &Rules) -> Option<Found<EdgeId>> {
    let t = live.topo;
    let Some(stored) = &sig.name else {
        return nearest_edge(t, sig, sig.length.max(rules.legacy_edge_min)).map(|id| Found { id, how: How::Legacy });
    };
    let stored = EdgeName::from_stored(stored);
    let ids = || (0..t.edges.len() as u32).map(EdgeId);
    // The edge lies between a face standing for one stored name and a face standing for
    // the other (a seam: the same face for both).
    let ((x, rx), (y, ry)) = (live.targets(&stored.0[0]), live.targets(&stored.0[1]));
    let candidates: Vec<EdgeId> = ids()
        .filter(|e| {
            let faces = &t.edges[e.index()].faces;
            let (Some(a), Some(b)) = (faces.first(), faces.last()) else { return false };
            (x.contains(a) && y.contains(b)) || (x.contains(b) && y.contains(a))
        })
        .collect();
    let was = edge_place(sig.a, sig.b, sig.mid, sig.length);
    let stayed = |e: EdgeId| {
        let x = &t.edges[e.index()];
        v3::dist(edge_place(x.start, x.end, x.mid, x.length), was) <= rules.size * sig.length
    };
    if let Some(found) = by_role(pick(candidates, |e| edge_score(t, e, sig), |e| v3::dist(t.edges[e.index()].mid, sig.mid)), Stand::over([rx, ry]), stayed) {
        return Some(found);
    }
    // Step 3. Lines: the same line, overlapping where the edge was. Other curves: about the
    // same place and length.
    let dir = v3::unit(v3::sub(sig.b, sig.a));
    let strict = ids().filter(|e| {
        let x = &t.edges[e.index()];
        if curve_name(&x.curve) != sig.curve {
            return false;
        }
        match (x.curve, dir) {
            (Curve::Line, Some(d)) => {
                let Some(live_dir) = v3::unit(v3::sub(x.end, x.start)) else { return false };
                let tol = rules.linear * live.scale(sig.mid);
                v3::len(v3::cross(live_dir, d)) <= rules.angular
                    && v3::len(v3::cross(v3::sub(x.mid, sig.mid), d)) <= tol
                    && v3::dist(x.mid, sig.mid) <= (x.length + sig.length) / 2.0
            }
            _ => v3::dist(x.mid, sig.mid) <= rules.size * sig.length && (x.length - sig.length).abs() <= rules.size * sig.length,
        }
    });
    best(strict, |e| edge_score(t, e, sig)).map(|(id, _)| Found { id, how: How::Position })
}

/// Find the vertex a stored reference names. `p` is where it was when picked.
///
/// A legacy reference (no name) matches only a vertex at `p` itself: a stored point that
/// is not on the model (the origin, a sketch point) is not a reference to topology, and
/// stays where it is.
pub fn vertex(live: &Live<'_>, p: P3, name: Option<&[oc_doc::FaceName]>, rules: &Rules) -> Option<Found<VertexId>> {
    let t = live.topo;
    let ids = || (0..t.vertices.len() as u32).map(VertexId);
    let at_p = |how| {
        let tol = rules.linear * live.scale(p);
        best(ids(), |v| Some(v3::dist(t.vertices[v.index()], p))).filter(|(_, d)| *d <= tol).map(|(id, _)| Found { id, how })
    };
    let Some(stored) = name else { return at_p(How::Legacy) };
    let stored = VertexName::from_stored(stored);
    // Every stored face still has a face standing for it at the vertex, and the vertex has
    // no face that stands for none of them.
    let targets: Vec<(Vec<FaceId>, Stand)> = stored.0.iter().map(|s| live.targets(s)).collect();
    let around = live.around();
    let candidates: Vec<VertexId> = ids()
        .filter(|v| {
            let faces = &around[v.index()];
            !faces.is_empty() && targets.iter().all(|(t, _)| t.iter().any(|f| faces.contains(f))) && faces.iter().all(|f| targets.iter().any(|(t, _)| t.contains(f)))
        })
        .collect();
    let dist = |v: VertexId| v3::dist(t.vertices[v.index()], p);
    // A vertex has no size of its own; the smallest face around it gives the scale.
    let stayed = |v: VertexId| {
        let size = around[v.index()].iter().map(|f| t.faces[f.index()].area.max(0.0).sqrt()).fold(f64::INFINITY, f64::min);
        dist(v) <= rules.size * size
    };
    by_role(pick(candidates, |v| Some(dist(v)), dist), Stand::over(targets.iter().map(|(_, s)| *s)), stayed).or_else(|| at_p(How::Position))
}
