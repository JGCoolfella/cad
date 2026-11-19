//! Evaluate in an assembly (G1c): Interference Detection, and the occurrence bodies at
//! their placements (Measure, Mass Properties).
//!
//! Interference Detection follows SOLIDWORKS' PropertyManager:
//! - the whole assembly, or the selected components: one selected component checks it
//!   against everything else; two or more check them against one another only;
//! - every candidate pair is pre-screened by world bounding boxes, then the exact B-rep
//!   common of the two bodies at their placements is computed on the engine thread and
//!   its volume reported;
//! - "Treat coincidence as interference": bodies that touch without overlapping (a pin
//!   seated on a plate, a shaft in a hole of its own size) are reported with volume 0;
//! - "Treat subassemblies as components": parts inside one sub-assembly are not checked
//!   against one another;
//! - "Ignore hidden bodies/components".
//!
//! GPU screening of mesh pairs is a later item (G2 / H1): the box pre-screen is enough for
//! assemblies of a few hundred components.
use std::collections::HashMap;

use oc_doc::{Id, Transform};
use oc_kernel::{BBox, BooleanOp, Kernel, KernelError, Target, P3};

use crate::build::{Assembler, Built};

/// What to check, and how (the PropertyManager's Selected Components and Options).
#[derive(Clone, Debug, Default)]
pub struct InterferenceOptions {
    /// Selected components as instance paths (a top-level instance, or a component inside
    /// a sub-assembly). Empty: the whole assembly.
    pub selected: Vec<Vec<Id>>,
    /// Treat coincidence as interference.
    pub coincidence: bool,
    /// Treat subassemblies as components.
    pub subassemblies_as_components: bool,
    /// Include hidden components (SOLIDWORKS: "Ignore hidden bodies/components" cleared).
    pub include_hidden: bool,
}

/// One interference: occurrences `a` and `b` (indices into [`Built::occurrences`]).
#[derive(Clone, Debug)]
pub struct Interference<B> {
    pub a: usize,
    pub b: usize,
    /// Volume of the common body (mm³); 0 for a coincidence.
    pub volume: f64,
    /// The bodies touch but do not overlap ("Treat coincidence as interference").
    pub coincident: bool,
    /// The common body in assembly coordinates (none for a coincidence).
    pub body: Option<B>,
    /// Centre of the common body's box, or the point where the bodies touch.
    pub center: P3,
}

#[derive(Clone, Debug, Default)]
pub struct InterferenceReport<B> {
    pub interferences: Vec<Interference<B>>,
    /// Pairs whose boxes overlapped (exact checks run).
    pub candidates: usize,
    /// Pairs considered before the box pre-screen.
    pub pairs: usize,
    /// Occurrences that could not be checked (a part shown from a cache whose file is
    /// missing, or with no body).
    pub skipped: Vec<usize>,
}

/// The kernel transform of a placement.
pub fn kernel_transform(t: &Transform) -> oc_kernel::Transform {
    let c = [t.rotate([1.0, 0.0, 0.0]), t.rotate([0.0, 1.0, 0.0]), t.rotate([0.0, 0.0, 1.0])];
    let p = t.position;
    oc_kernel::Transform { m: [[c[0][0], c[1][0], c[2][0], p[0]], [c[0][1], c[1][1], c[2][1], p[1]], [c[0][2], c[1][2], c[2][2], p[2]]] }
}

/// The box of `b` (part coordinates) placed by `t`.
pub fn placed_box(b: &BBox, t: &Transform) -> BBox {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for k in 0..8 {
        let c = [if k & 1 == 0 { b.min[0] } else { b.max[0] }, if k & 2 == 0 { b.min[1] } else { b.max[1] }, if k & 4 == 0 { b.min[2] } else { b.max[2] }];
        let w = t.apply(c);
        for i in 0..3 {
            lo[i] = lo[i].min(w[i]);
            hi[i] = hi[i].max(w[i]);
        }
    }
    BBox { min: lo, max: hi }
}

fn overlap(a: &BBox, b: &BBox, gap: f64) -> bool {
    (0..3).all(|i| a.min[i] <= b.max[i] + gap && b.min[i] <= a.max[i] + gap)
}

fn under(path: &[Id], prefix: &[Id]) -> bool {
    path.len() >= prefix.len() && path[..prefix.len()] == *prefix
}

impl<K: Kernel> Assembler<K> {
    /// The body of occurrence `k` of `built` at its placement (regenerating its part if it
    /// is lightweight). `None` when the part has no body or its document is unknown.
    pub fn placed_body(&mut self, built: &Built, k: usize, quality: f64) -> Option<K::Body> {
        let o = built.occurrences.get(k)?;
        if !self.resolve(&o.part, quality) {
            return None;
        }
        let shape = self.parts.get(&o.part)?.live.as_ref()?.shape.clone()?;
        if o.world == Transform::IDENTITY {
            return Some(shape.body.clone());
        }
        self.regen.kernel().transform(&shape.body, &kernel_transform(&o.world)).ok().map(|op| op.body)
    }

    /// Entity `index` (`"face"`, `"edge"` or `"vertex"`) of occurrence `k`'s part, found on
    /// the occurrence's placed body `placed` (from [`Self::placed_body`]). The kernel may
    /// number a transformed body's entities differently from the part's (G1c: OCCT did for
    /// a translated pin), so the entity is matched by geometry: a face by its centroid and
    /// area, an edge by its middle and length, a vertex by its point, all placed.
    pub fn placed_entity(&mut self, built: &Built, k: usize, placed: &K::Body, kind: &str, index: u32, quality: f64) -> Option<u32> {
        let o = built.occurrences.get(k)?;
        if !self.resolve(&o.part, quality) {
            return None;
        }
        let shape = self.parts.get(&o.part)?.live.as_ref()?.shape.clone()?;
        let kernel = self.regen.kernel();
        let part = shape.topo(kernel).ok()?;
        let world = kernel.topology(placed).ok()?;
        let w = &o.world;
        let d = |a: P3, b: P3| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let nearest = |n: usize, score: &dyn Fn(usize) -> f64| (0..n).map(|i| (i, score(i))).min_by(|x, y| x.1.total_cmp(&y.1)).map(|(i, _)| i as u32);
        match kind {
            "face" => {
                let f = part.faces.get(index as usize)?;
                let (c, a) = (w.apply(f.centroid), f.area);
                nearest(world.faces.len(), &|i| d(world.faces[i].centroid, c) + (world.faces[i].area - a).abs())
            }
            "edge" => {
                let e = part.edges.get(index as usize)?;
                let (m, l) = (w.apply(e.mid), e.length);
                nearest(world.edges.len(), &|i| d(world.edges[i].mid, m) + (world.edges[i].length - l).abs())
            }
            "vertex" => {
                let p = w.apply(*part.vertices.get(index as usize)?);
                nearest(world.vertices.len(), &|i| d(world.vertices[i], p))
            }
            _ => None,
        }
    }

    /// Interference Detection over `built` (see the module notes).
    pub fn interferences(&mut self, built: &Built, opts: &InterferenceOptions, quality: f64) -> Result<InterferenceReport<K::Body>, String> {
        let mut report = InterferenceReport { interferences: vec![], candidates: 0, pairs: 0, skipped: vec![] };
        let occ = &built.occurrences;
        // Which selected item each occurrence falls under (None: not selected).
        let item = |k: usize| opts.selected.iter().position(|s| under(&occ[k].path, s));
        let considered: Vec<usize> = (0..occ.len()).filter(|&k| (opts.include_hidden || !occ[k].hidden) && (opts.selected.is_empty() || opts.selected.len() == 1 || item(k).is_some())).collect();
        // Part boxes (one per distinct part), placed per occurrence.
        let mut part_box: HashMap<String, Option<BBox>> = HashMap::new();
        let mut boxes: HashMap<usize, BBox> = HashMap::new();
        for &k in &considered {
            let h = occ[k].part.clone();
            if !part_box.contains_key(&h) {
                let b = if self.resolve(&h, quality) {
                    self.parts[&h].live.as_ref().and_then(|l| l.shape.as_ref()).and_then(|s| self.regen.kernel().bounding_box(&s.body).ok())
                } else {
                    None
                };
                part_box.insert(h.clone(), b);
            }
            match &part_box[&h] {
                Some(b) => {
                    boxes.insert(k, placed_box(b, &occ[k].world));
                }
                None => report.skipped.push(k),
            }
        }
        let tol = self.regen.kernel().tolerances().confusion.max(1e-6);
        let mut bodies: HashMap<usize, K::Body> = HashMap::new();
        for (x, &a) in considered.iter().enumerate() {
            for &b in &considered[x + 1..] {
                let (pa, pb) = (&occ[a].path, &occ[b].path);
                // Parts of one rigid component are not checked against each other when
                // sub-assemblies are treated as components.
                if opts.subassemblies_as_components && pa[0] == pb[0] {
                    continue;
                }
                // One selected component: pairs involving it. Several: pairs among them.
                let (ia, ib) = (item(a), item(b));
                match opts.selected.len() {
                    0 => {}
                    1 => {
                        if ia.is_none() && ib.is_none() {
                            continue;
                        }
                    }
                    _ => {
                        if ia.is_none() || ib.is_none() {
                            continue;
                        }
                    }
                }
                report.pairs += 1;
                let (Some(ba), Some(bb)) = (boxes.get(&a), boxes.get(&b)) else { continue };
                if !overlap(ba, bb, tol) {
                    continue;
                }
                report.candidates += 1;
                for k in [a, b] {
                    if let std::collections::hash_map::Entry::Vacant(e) = bodies.entry(k) {
                        let body = self.placed_body(built, k, quality).ok_or_else(|| format!("{} has no body", self.parts[&occ[k].part].name))?;
                        e.insert(body);
                    }
                }
                let kernel = self.regen.kernel();
                let (body_a, body_b) = (&bodies[&a], &bodies[&b]);
                // Bodies that only touch have an empty common.
                let common = match kernel.boolean(BooleanOp::Common, body_a, &[body_b]) {
                    Ok(op) => Some(op.body),
                    Err(KernelError::Empty { .. }) => None,
                    Err(e) => return Err(e.to_string()),
                };
                let volume = match &common {
                    Some(c) => kernel.mass_properties(c).map_err(|e| e.to_string())?.volume,
                    None => 0.0,
                };
                let size = [ba, bb].iter().map(|x| (0..3).map(|i| x.max[i] - x.min[i]).fold(0.0f64, f64::max)).fold(0.0f64, f64::max).max(1.0);
                // Coincident faces leave no volume; rounding may leave a sliver.
                let min_volume = 1e-9 * size * size * size;
                if let Some(common) = common.filter(|_| volume > min_volume) {
                    let props = kernel.mass_properties(&common).map_err(|e| e.to_string())?;
                    let center = kernel.bounding_box(&common).map(|bx| [0, 1, 2].map(|i| (bx.min[i] + bx.max[i]) / 2.0)).unwrap_or(props.center);
                    report.interferences.push(Interference { a, b, volume, coincident: false, body: Some(common), center });
                } else if opts.coincidence {
                    let d = kernel.distance(Target::Body(body_a), Target::Body(body_b)).map_err(|e| e.to_string())?;
                    if d.distance <= tol * size {
                        report.interferences.push(Interference { a, b, volume: 0.0, coincident: true, body: None, center: [0, 1, 2].map(|i| (d.a[i] + d.b[i]) / 2.0) });
                    }
                }
            }
        }
        Ok(report)
    }
}
