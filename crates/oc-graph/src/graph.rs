//! The feature DAG: what each feature reads, so a node's cache key covers exactly its
//! inputs.
//!
//! Edges come from the references stored in the feature (sketch → extrude, reference
//! plane → sketch, seeds → pattern, ...) plus the **body chain**: every feature that
//! changes the part body depends on the previous one, and features that read the body
//! (sketches on faces, references to model edges and faces) depend on the body as it
//! stands before them. A reference to a feature that is later in the tree, suppressed or
//! missing is not an edge; the feature sees it as absent, as the TypeScript engine does.

use std::collections::HashMap;
use std::sync::Arc;

use oc_doc::{EndCondition, Feature, FeatureKind, GeomRef, PlaneRef};

/// One unsuppressed feature in the DAG.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// Position in the feature list.
    pub index: usize,
    /// Features this one reads (positions, ascending, each once).
    pub deps: Vec<usize>,
    /// Ids the feature refers to, in the order it names them (resolved or not).
    pub refs: Vec<String>,
    /// Changes the part body.
    pub writes_body: bool,
    /// Reads the part body as it stands before this feature.
    pub reads_body: bool,
    /// The body-changing feature this one's body comes from (the previous body writer).
    pub body_from: Option<usize>,
}

fn plane_ref(p: &PlaneRef, ids: &mut Vec<String>) -> bool {
    match p {
        PlaneRef::RefPlane { feature_id } => {
            ids.push(feature_id.as_str().into());
            false
        }
        PlaneRef::Face { .. } => true,
        PlaneRef::Std { .. } => false,
    }
}

/// Collect the ids a reference names; true if it needs the body.
fn geom_ref(r: &GeomRef, ids: &mut Vec<String>) -> bool {
    match r {
        GeomRef::Edge { .. } | GeomRef::Face { .. } => true,
        GeomRef::Plane { plane } => plane_ref(plane, ids),
        GeomRef::SketchLine { sketch_id, .. } => {
            ids.push(sketch_id.as_str().into());
            false
        }
        GeomRef::Axis { feature_id } | GeomRef::Point { feature_id } => {
            ids.push(feature_id.as_str().into());
            false
        }
        // A model vertex is followed by name, and a legacy one is looked for on the body
        // so it can be named.
        GeomRef::Vertex { .. } => true,
        GeomRef::Origin => false,
    }
}

fn end_ref(e: &EndCondition, ids: &mut Vec<String>) {
    if let Some(Some(r)) = &e.r#ref {
        geom_ref(r, ids);
    }
}

/// `(writes_body, reads_body, referenced ids)` of a feature.
pub fn inputs(f: &Feature) -> (bool, bool, Vec<String>) {
    let mut ids = vec![];
    let mut read = false;
    let mut g = |r: &GeomRef, ids: &mut Vec<String>| read |= geom_ref(r, ids);
    let writes = match &f.kind {
        FeatureKind::Sketch(s) => {
            let face = plane_ref(&s.plane, &mut ids);
            let refs = s.sketch.refs.as_ref().is_some_and(|r| !r.is_empty());
            return (false, face || refs, ids);
        }
        FeatureKind::RefPlane(p) => {
            match &p.refs {
                Some(r) if !r.is_empty() => r.iter().for_each(|x| g(x, &mut ids)),
                _ => {
                    let face = plane_ref(&p.base, &mut ids);
                    return (false, face, ids);
                }
            }
            false
        }
        FeatureKind::RefAxis(a) => {
            a.refs.iter().for_each(|x| g(x, &mut ids));
            false
        }
        FeatureKind::RefPoint(p) => {
            p.refs.iter().for_each(|x| g(x, &mut ids));
            false
        }
        FeatureKind::CoordSys(c) => {
            for r in [&c.origin, &c.x_axis, &c.y_axis].into_iter().flatten() {
                g(r, &mut ids);
            }
            false
        }
        FeatureKind::Extrude(e) => {
            ids.push(e.sketch_id.as_str().into());
            if let Some(Some(r)) = e.from.as_ref().and_then(|x| x.r#ref.as_ref()) {
                geom_ref(r, &mut ids);
            }
            end_ref(&e.end1, &mut ids);
            if let Some(d) = &e.dir2 {
                end_ref(d, &mut ids);
            }
            true
        }
        FeatureKind::Revolve(r) => {
            ids.push(r.sketch_id.as_str().into());
            if let Some(Some(a)) = &r.axis_ref {
                geom_ref(a, &mut ids);
            }
            true
        }
        FeatureKind::Sweep(s) => {
            ids.push(s.path.as_str().into());
            ids.push(s.profile.as_str().into());
            true
        }
        FeatureKind::Loft(l) => {
            ids.extend(l.profiles.iter().map(|p| p.as_str().into()));
            true
        }
        FeatureKind::LinearPattern(p) => {
            ids.extend(p.seeds.iter().map(|s| s.as_str().into()));
            for d in std::iter::once(&p.dir1).chain(&p.dir2) {
                if let Some(r) = &d.r#ref {
                    geom_ref(r, &mut ids);
                }
            }
            true
        }
        FeatureKind::CircularPattern(p) => {
            ids.extend(p.seeds.iter().map(|s| s.as_str().into()));
            if let Some(a) = &p.axis {
                geom_ref(a, &mut ids);
            }
            true
        }
        FeatureKind::Mirror(m) => {
            ids.extend(m.seeds.iter().map(|s| s.as_str().into()));
            if let Some(p) = &m.plane {
                geom_ref(p, &mut ids);
            }
            true
        }
        FeatureKind::Hole(h) => {
            ids.extend(h.sketch_id.iter().map(|s| s.as_str().into()));
            true
        }
        FeatureKind::Rib(r) => {
            ids.push(r.sketch_id.as_str().into());
            true
        }
        FeatureKind::Draft(d) => {
            if let Some(n) = &d.neutral {
                geom_ref(n, &mut ids);
            }
            true
        }
        FeatureKind::Fillet(_) | FeatureKind::Chamfer(_) | FeatureKind::Shell(_) | FeatureKind::Imported(_) => true,
    };
    (writes, writes || read, ids)
}

/// The DAG of a feature list: one node per unsuppressed feature (`None` for suppressed
/// ones), in tree order. Every edge points backwards, so tree order is a topological order.
pub fn analyse(features: &[Arc<Feature>]) -> Vec<Option<Node>> {
    let mut pos: HashMap<&str, usize> = HashMap::new();
    let mut body_from = None;
    let mut out = Vec::with_capacity(features.len());
    for (i, f) in features.iter().enumerate() {
        if f.is_suppressed() {
            out.push(None);
            continue;
        }
        let (writes_body, reads_body, refs) = inputs(f);
        let mut deps: Vec<usize> = refs.iter().filter_map(|id| pos.get(id.as_str()).copied()).collect();
        deps.sort_unstable();
        deps.dedup();
        out.push(Some(Node { index: i, deps, refs, writes_body, reads_body, body_from: if reads_body { body_from } else { None } }));
        if writes_body {
            body_from = Some(i);
        }
        pos.entry(f.id.as_str()).or_insert(i);
    }
    out
}

/// Nodes that need nothing from the body chain, directly or through their dependencies:
/// their results stay valid whatever happens to the body.
pub fn body_free(nodes: &[Option<Node>]) -> Vec<bool> {
    let mut free = vec![false; nodes.len()];
    for n in nodes.iter().flatten() {
        free[n.index] = !n.reads_body && n.deps.iter().all(|&d| free[d]);
    }
    free
}

/// 128-bit FNV-1a: a fixed, documented hash (section 3.4) for cache keys.
#[derive(Clone, Copy)]
pub struct Fnv(u128);

impl Default for Fnv {
    fn default() -> Self {
        Fnv(0x6c62272e07bb014262b821756295c58d)
    }
}

impl Fnv {
    pub fn bytes(mut self, b: &[u8]) -> Self {
        for &x in b {
            self.0 ^= x as u128;
            self.0 = self.0.wrapping_mul(0x0000000001000000000000000000013B);
        }
        self
    }
    pub fn u128(self, x: u128) -> Self {
        self.bytes(&x.to_le_bytes())
    }
    pub fn finish(self) -> u128 {
        self.0
    }
}
