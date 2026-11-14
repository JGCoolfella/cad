//! Propagation (`docs/OVERHAUL_PLAN.md` section 3.2): the names of an operation's result
//! from its history and the names of its inputs.
//!
//! Each result face gets, in this order:
//! 1. its **birth tags**, when the operation created it: from a profile segment or cap
//!    (the feature supplies the tag), from an edge or vertex (the tag hashes the edge's or
//!    vertex's name), or as the offset copy of a face;
//! 2. else the **union of the tags of the input faces** history maps to it (kept, modified,
//!    split or merged), wrapped as a pattern instance when the feature says so;
//! 3. else the tags of input faces on the **same surface** with overlapping bounds;
//! 4. else `<fid>/new/<rank>`, ranked among the operation's other such faces by surface
//!    type, centre and area. This is a history gap; the corpus has none.
//!
//! Nothing here reads a face index except to look up a name: no tag depends on the
//! kernel's face order (section 3.4).

use oc_kernel::{BBox, CapEnd, History, Relation, Source, Surface, TopoKind, Topology};

use crate::name::{vertex_faces, BodyNames, FaceName, Tag};
use crate::signature::surface_name;

/// A body's topology, computed only if naming needs it (most operations do not).
pub trait Topo {
    fn topology(&self) -> Option<&Topology>;
}

impl Topo for Topology {
    fn topology(&self) -> Option<&Topology> {
        Some(self)
    }
}

/// No topology available: steps that need it are skipped.
pub struct NoTopo;

impl Topo for NoTopo {
    fn topology(&self) -> Option<&Topology> {
        None
    }
}

/// One input body of the operation (history argument `k` is `inputs[k]`).
pub struct Input<'a> {
    pub names: &'a BodyNames,
    pub topo: &'a dyn Topo,
}

/// What the feature tells naming about the operation.
#[derive(Clone, Copy)]
pub struct Births<'a> {
    /// Id of the feature doing the operation: the `<fid>` of every tag born here.
    pub feature: &'a str,
    /// Tags of faces made from profile segments and caps. A source missing here gets a
    /// tag from its position in the profile (`side/s<region>.<wire>.<segment>`).
    pub explicit: &'a [(Source, Tag)],
    /// Role of a face generated from an edge (`fillet`, `chamfer`).
    pub edge_role: &'a str,
    /// Role of a face generated from a vertex.
    pub vertex_role: &'a str,
    /// Role of a face generated from a face as a new, separate face (a shell's inner
    /// walls: `inner`). `None`: the generated face replaces its source and keeps its
    /// name (a drafted face).
    pub face_role: Option<&'a str>,
    /// Pattern or mirror instance: every inherited tag `t` becomes `<fid>/i<n>/t`.
    pub instance: Option<&'a str>,
}

impl<'a> Births<'a> {
    /// An operation that creates nothing a feature has to name itself.
    pub fn of(feature: &'a str) -> Self {
        Births { feature, explicit: &[], edge_role: "edge", vertex_role: "corner", face_role: None, instance: None }
    }
}

/// Tolerances for "the same surface" (step 3). These are naming's own matching limits,
/// not modelling tolerances: surfaces that reach the comparison were computed by the same
/// kernel from the same input, so they agree far more closely.
#[derive(Clone, Copy, Debug)]
pub struct Identity {
    /// Distances below this are zero (mm).
    pub linear: f64,
    /// Directions closer than this are parallel (radians).
    pub angular: f64,
}

impl Default for Identity {
    fn default() -> Self {
        Identity { linear: 1e-6, angular: 1e-9 }
    }
}

/// Names of an operation's result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tracked {
    pub names: BodyNames,
    /// Faces named by surface identity (step 3).
    pub by_identity: u32,
    /// Faces that got a `/new/` tag (step 4).
    pub untracked: u32,
}

/// Step-4 sort key: surface type, then centre and area rounded to this quantum, so
/// kernel noise in the last digits cannot change a rank.
const RANK_QUANTUM: f64 = 1e-5;

/// Rank faces of `t` by geometry alone: surface type, rounded centre, rounded area (and
/// normal). Returns, for each entry of `faces`, how many of the others sort before it;
/// faces that cannot be told apart share a rank, so the face order never decides.
pub fn rank_by_geometry(t: &Topology, faces: &[usize]) -> Vec<usize> {
    let q = |x: f64| (x / RANK_QUANTUM).round() as i64;
    let keys: Vec<_> = faces
        .iter()
        .map(|&i| {
            let f = &t.faces[i];
            (surface_name(&f.surface), f.centroid.map(q), q(f.area), f.normal.map(q))
        })
        .collect();
    keys.iter().map(|k| keys.iter().filter(|other| *other < k).count()).collect()
}

fn parallel(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    use oc_kernel::v3;
    v3::len(v3::cross(a, b)) <= tol * v3::len(a) * v3::len(b)
}

fn on_line(p: [f64; 3], o: [f64; 3], d: [f64; 3], tol: f64) -> bool {
    use oc_kernel::v3;
    let l = v3::len(d);
    l > 0.0 && v3::len(v3::cross(v3::sub(p, o), d)) / l <= tol
}

/// The two faces lie on one underlying surface (analytic surfaces only; free-form
/// surfaces have no parameters to compare and never match).
pub fn same_surface(a: &Surface, b: &Surface, tol: &Identity) -> bool {
    use oc_kernel::v3;
    let near = |x: f64, y: f64| (x - y).abs() <= tol.linear;
    match (a, b) {
        (Surface::Plane { origin: o1, normal: n1 }, Surface::Plane { origin: o2, normal: n2 }) => {
            parallel(*n1, *n2, tol.angular) && v3::dot(v3::sub(*o2, *o1), *n1).abs() <= tol.linear * v3::len(*n1)
        }
        (Surface::Cylinder { axis: a1, radius: r1 }, Surface::Cylinder { axis: a2, radius: r2 }) => {
            near(*r1, *r2) && parallel(a1.dir, a2.dir, tol.angular) && on_line(a2.origin, a1.origin, a1.dir, tol.linear)
        }
        (Surface::Cone { axis: a1, radius: r1, half_angle: h1 }, Surface::Cone { axis: a2, radius: r2, half_angle: h2 }) => {
            // Same axis line, and the same radius where the second cone's origin sits on the first.
            let d = v3::unit(a1.dir).unwrap_or(a1.dir);
            let along = v3::dot(v3::sub(a2.origin, a1.origin), d);
            let same_way = v3::dot(a1.dir, a2.dir) >= 0.0;
            parallel(a1.dir, a2.dir, tol.angular)
                && on_line(a2.origin, a1.origin, a1.dir, tol.linear)
                && (h1.abs() - h2.abs()).abs() <= tol.angular.max(1e-12)
                && (h1.signum() == h2.signum()) == same_way
                && near(r1 + along * h1.tan(), *r2)
        }
        (Surface::Sphere { center: c1, radius: r1 }, Surface::Sphere { center: c2, radius: r2 }) => near(*r1, *r2) && v3::dist(*c1, *c2) <= tol.linear,
        (Surface::Torus { axis: a1, major: m1, minor: n1 }, Surface::Torus { axis: a2, major: m2, minor: n2 }) => {
            near(*m1, *m2) && near(*n1, *n2) && parallel(a1.dir, a2.dir, tol.angular) && v3::dist(a1.origin, a2.origin) <= tol.linear
        }
        _ => false,
    }
}

fn overlap(a: &BBox, b: &BBox, tol: f64) -> bool {
    (0..3).all(|i| a.min[i] <= b.max[i] + tol && b.min[i] <= a.max[i] + tol)
}

fn positional(feature: &str, s: Source) -> Option<Tag> {
    match s {
        Source::Segment { region, wire, segment } => Some(Tag::birth(feature, "side", &format!("s{region}.{wire}.{segment}"))),
        Source::Cap { region, end } => Some(Tag::birth(feature, if end == CapEnd::Start { "start" } else { "end" }, &format!("r{region}"))),
        _ => None,
    }
}

/// Names of the faces of an operation's result. `result` is asked for its topology only
/// when some face has no history (steps 3 and 4), an input only when an edge or a vertex
/// generated a face.
pub fn track(b: &Births<'_>, h: &History, inputs: &[Input<'_>], result: &dyn Topo, tol: &Identity) -> Tracked {
    let n = h.count as usize;
    if h.image_kind != TopoKind::Face {
        return Tracked { names: BodyNames::default(), by_identity: 0, untracked: 0 };
    }
    let mut born: Vec<Vec<Tag>> = vec![vec![]; n];
    let mut kept: Vec<Vec<Tag>> = vec![vec![]; n];
    // Faces around each vertex of an input, computed when the first vertex source needs it.
    let mut around: Vec<Option<Vec<Vec<oc_kernel::FaceId>>>> = vec![None; inputs.len()];
    let inherit = |tags: &[Tag], into: &mut Vec<Tag>| match b.instance {
        Some(i) => into.extend(tags.iter().map(|t| t.instance(b.feature, i))),
        None => into.extend(tags.iter().cloned()),
    };
    for fate in &h.fates {
        // The tag(s) this source gives the faces it creates.
        let birth: Option<Tag> = match fate.source {
            Source::Segment { .. } | Source::Cap { .. } => b.explicit.iter().find(|(s, _)| *s == fate.source).map(|(_, t)| t.clone()).or_else(|| positional(b.feature, fate.source)),
            Source::Edge { arg, edge } => inputs.get(arg as usize).and_then(|i| {
                let t = i.topo.topology()?;
                (edge.index() < t.edges.len()).then(|| Tag::birth(b.feature, b.edge_role, &i.names.edge(t, edge).key()))
            }),
            Source::Vertex { arg, vertex } => inputs.get(arg as usize).and_then(|i| {
                let t = i.topo.topology()?;
                let faces = around[arg as usize].get_or_insert_with(|| vertex_faces(t));
                (vertex.index() < faces.len()).then(|| Tag::birth(b.feature, b.vertex_role, &i.names.vertex(faces, vertex).key()))
            }),
            Source::Face { .. } => None,
        };
        for im in &fate.images {
            let i = im.index as usize;
            if i >= n {
                continue;
            }
            match fate.source {
                Source::Face { arg, face } => {
                    let Some(name) = inputs.get(arg as usize).and_then(|x| x.names.faces.get(face.index())) else { continue };
                    match (im.relation, b.face_role) {
                        (Relation::Generated, Some(role)) => born[i].push(Tag::birth(b.feature, role, &name.key())),
                        _ => inherit(name.tags(), &mut kept[i]),
                    }
                }
                _ => born[i].extend(birth.clone()),
            }
        }
    }
    let mut names: Vec<FaceName> = Vec::with_capacity(n);
    let mut pending = vec![];
    for (i, (born, kept)) in born.into_iter().zip(kept).enumerate() {
        let tags = if born.is_empty() { kept } else { born };
        if tags.is_empty() {
            pending.push(i);
        }
        names.push(FaceName::new(tags));
    }
    let (mut by_identity, mut untracked) = (0, 0);
    if !pending.is_empty() {
        let rt = result.topology();
        let mut still = vec![];
        for &i in &pending {
            let mut tags = vec![];
            if let Some(f) = rt.and_then(|t| t.faces.get(i)) {
                for input in inputs {
                    let Some(it) = input.topo.topology() else { continue };
                    for (j, g) in it.faces.iter().enumerate() {
                        if same_surface(&f.surface, &g.surface, tol) && overlap(&f.bbox, &g.bbox, tol.linear) {
                            if let Some(name) = input.names.faces.get(j) {
                                inherit(name.tags(), &mut tags);
                            }
                        }
                    }
                }
            }
            if tags.is_empty() {
                still.push(i);
            } else {
                by_identity += 1;
                names[i] = FaceName::new(tags);
            }
        }
        untracked = still.len() as u32;
        let ranks = match rt {
            Some(t) if still.iter().all(|&i| i < t.faces.len()) => rank_by_geometry(t, &still).into_iter().map(|r| r.to_string()).collect(),
            // Without geometry there is nothing index-free to rank by: one shared tag.
            _ => vec!["x".to_string(); still.len()],
        };
        for (&i, r) in still.iter().zip(&ranks) {
            names[i] = FaceName::single(Tag::birth(b.feature, "new", r));
        }
    }
    Tracked { names: BodyNames { faces: names }, by_identity, untracked }
}

/// Names for a body that arrives without history (an imported file): every face is
/// `<fid>/import/<rank>`, ranked by geometry. The ranks are stable for as long as the
/// imported data is, which is stored in the document.
pub fn import_names(feature: &str, t: &Topology) -> BodyNames {
    let all: Vec<usize> = (0..t.faces.len()).collect();
    let faces = rank_by_geometry(t, &all).into_iter().map(|r| FaceName::single(Tag::birth(feature, "import", &r.to_string()))).collect();
    BodyNames { faces }
}
