//! Building an assembly: find and read every component, regenerate each distinct part
//! once (or take its cached mesh), find every mate reference on its part by persistent
//! name, solve the mates, and list what to draw.
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use oc_doc::container::Container;
use oc_doc::{AssemblyDocument, ComponentKind, Id, MateEntity, MateGeom, MateRef, MateType, PartDocument, Transform};
use oc_graph::{CancelToken, Regenerator, Shape};
use oc_kernel::{EdgeId, FaceId, Kernel, VertexId};
use oc_mesh::{MeshOptions, RenderMesh};
use oc_naming::{resolve, How, Live, Rules};

use crate::cache::{self, part_hash, Cache};
use crate::geom::*;
use crate::locate::{locate, normalize, Found, Store};

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Image Quality of the display meshes.
    pub quality: f64,
    /// Take a part's cached mesh when its hash matches, without regenerating it.
    pub lightweight: bool,
    /// Extra folders to search for component files.
    pub search: Vec<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Options { quality: 3.0, lightweight: true, search: vec![] }
    }
}

/// A regenerated part: its final body and reference geometry, in part coordinates.
pub struct LivePart<B> {
    pub shape: Option<Rc<Shape<B>>>,
    pub planes: HashMap<String, MateGeom>,
    pub axes: HashMap<String, MateGeom>,
    pub points: HashMap<String, MateGeom>,
    /// Features with an error or warning: (id, message).
    pub errors: Vec<(String, String)>,
}

/// A distinct part (by content hash).
pub struct Part<B> {
    pub hash: String,
    pub name: String,
    pub doc: Option<Arc<PartDocument>>,
    pub mesh: Option<Arc<RenderMesh>>,
    /// `None` while lightweight (shown from the cache, not regenerated).
    pub live: Option<LivePart<B>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComponentOut {
    pub id: Id,
    pub kind: ComponentKind,
    pub path: Option<PathBuf>,
    pub found: Option<Found>,
    /// Content hash of the part (parts only).
    pub part: Option<String>,
    pub error: Option<String>,
    pub warning: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstanceState {
    Fixed,
    Under,
    Full,
    Over,
    Suppressed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstanceOut {
    pub id: Id,
    pub transform: Transform,
    pub state: InstanceState,
    /// Degrees of freedom left with every other component held.
    pub free: usize,
}

/// A part occurrence to draw: instance path from this assembly down, part hash, placement.
#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    pub path: Vec<Id>,
    pub part: String,
    pub world: Transform,
    /// Placement in the top-level instance's coordinates (identity for a part instance).
    pub local: Transform,
    /// Hidden (Hide Components): not drawn, still mated.
    pub hidden: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MateStatus {
    Ok,
    Suppressed,
    /// A reference no longer resolves on its part (shown as Missing).
    Missing,
    Redundant,
    Conflict,
    Invalid(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MateOut {
    pub id: Id,
    pub status: MateStatus,
    /// Which references are missing (0, 1).
    pub missing: Vec<usize>,
}

/// A mate reference found under another name or with changed geometry (the part was
/// edited): write it back without an undo step.
#[derive(Clone, Debug, PartialEq)]
pub struct MateHeal {
    pub mate: Id,
    pub side: usize,
    pub entity: MateEntity,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Built {
    pub components: Vec<ComponentOut>,
    pub instances: Vec<InstanceOut>,
    pub occurrences: Vec<Occurrence>,
    pub mates: Vec<MateOut>,
    pub dof: usize,
    pub converged: bool,
    pub newest_error: Option<Id>,
    pub heals: Vec<MateHeal>,
    /// The mate problem as solved (bodies are `bodies`, the top-level instances that take
    /// part), for dragging.
    pub problem: oc_mate::Problem,
    pub bodies: Vec<Id>,
}

enum Loaded {
    Part(String),
    Assembly(Box<Built>),
    Missing,
}

/// Builds assemblies on one kernel; keeps every part it regenerated, by hash, so a
/// rebuild after a mate edit regenerates nothing.
pub struct Assembler<K: Kernel> {
    pub regen: Regenerator<K>,
    pub parts: HashMap<String, Part<K::Body>>,
    cache: Cache,
    pub rules: Rules,
    /// Parts regenerated so far (shared and cached parts are not regenerated again).
    pub regenerations: usize,
}

impl<K: Kernel> Assembler<K> {
    pub fn new(kernel: K) -> Self {
        Self::with(Regenerator::new(kernel))
    }

    pub fn with(regen: Regenerator<K>) -> Self {
        Assembler { regen, parts: HashMap::new(), cache: Cache::default(), rules: Rules::default(), regenerations: 0 }
    }

    /// Uses the caches of an opened container for lightweight builds.
    pub fn set_cache(&mut self, cache: Cache) {
        self.cache = cache;
    }

    /// Builds `doc`, whose file is in folder `dir`.
    pub fn build(&mut self, doc: &AssemblyDocument, dir: &Path, store: &dyn Store, opts: &Options) -> Built {
        self.build_inner(doc, dir, store, opts, &mut vec![])
    }

    fn build_inner(&mut self, doc: &AssemblyDocument, dir: &Path, store: &dyn Store, opts: &Options, stack: &mut Vec<PathBuf>) -> Built {
        let mut out = Built::default();
        let mut loaded: HashMap<Id, Loaded> = HashMap::new();
        let mut found_dirs: Vec<PathBuf> = vec![];
        for c in &doc.components {
            let mut co = ComponentOut { id: c.id.clone(), kind: c.kind, path: None, found: None, part: None, error: None, warning: None };
            let hit = locate(store, dir, &c.path, c.abs_path.as_deref(), &opts.search, &found_dirs);
            let l = match (hit, c.kind) {
                (Some((path, how)), kind) => {
                    if let Some(parent) = path.parent() {
                        if !found_dirs.contains(&parent.to_path_buf()) {
                            found_dirs.push(parent.to_path_buf());
                        }
                    }
                    co.path = Some(path.clone());
                    co.found = Some(how);
                    let bytes = store.read(&path).unwrap_or_default();
                    match kind {
                        ComponentKind::Part => match std::str::from_utf8(&bytes).map_err(|e| e.to_string()).and_then(|t| oc_doc::load(t).map_err(|e| e.to_string())) {
                            Ok(l) => {
                                let h = self.ensure_part(Arc::new(l.doc), opts);
                                co.part = Some(h.clone());
                                Loaded::Part(h)
                            }
                            Err(e) => {
                                co.error = Some(format!("{}: {e}", path.display()));
                                Loaded::Missing
                            }
                        },
                        ComponentKind::Assembly => {
                            let key = normalize(&path);
                            if stack.contains(&key) {
                                co.error = Some("the assembly contains itself".into());
                                Loaded::Missing
                            } else {
                                match Container::from_bytes(&bytes).map_err(|e| e.to_string()).and_then(|c| {
                                    let d = oc_doc::load_assembly(c.document().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                                    Ok((d.doc, Cache::read(&c)))
                                }) {
                                    Ok((sub, cache)) => {
                                        let outer = std::mem::replace(&mut self.cache, cache);
                                        stack.push(key);
                                        let sub_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
                                        let b = self.build_inner(&sub, &sub_dir, store, opts, stack);
                                        stack.pop();
                                        self.cache = outer;
                                        Loaded::Assembly(Box::new(b))
                                    }
                                    Err(e) => {
                                        co.error = Some(format!("{}: {e}", path.display()));
                                        Loaded::Missing
                                    }
                                }
                            }
                        }
                    }
                }
                (None, ComponentKind::Part) => match self.cache.manifest.components.get(c.id.as_str()).cloned().filter(|h| self.cache.meshes.contains_key(h)) {
                    Some(h) => {
                        self.ensure_cached(&h);
                        co.warning = Some(format!("{} was not found; shown from the assembly's cache", c.path));
                        co.part = Some(h.clone());
                        Loaded::Part(h)
                    }
                    None => {
                        co.error = Some(format!("{} was not found", c.path));
                        Loaded::Missing
                    }
                },
                (None, ComponentKind::Assembly) => {
                    co.error = Some(format!("{} was not found", c.path));
                    Loaded::Missing
                }
            };
            loaded.insert(c.id.clone(), l);
            out.components.push(co);
        }
        // Bodies: the top-level instances that take part.
        let mut body_of: HashMap<Id, usize> = HashMap::new();
        let mut bodies = vec![];
        for i in doc.instances.iter().filter(|i| !i.is_suppressed()) {
            body_of.insert(i.id.clone(), bodies.len());
            bodies.push(oc_mate::Body { pose: pose(&i.transform), fixed: i.fixed });
        }
        // Mates.
        let mut mates = vec![];
        let mut pre: Vec<Option<MateStatus>> = vec![];
        let mut missing_of: Vec<Vec<usize>> = vec![];
        for m in &doc.mates {
            let mut sides = vec![];
            let mut status = m.is_suppressed().then_some(MateStatus::Suppressed);
            let mut missing = vec![];
            if status.is_none() {
                for (k, r) in m.refs.iter().enumerate() {
                    match self.side(doc, &loaded, &body_of, r) {
                        Ok((owner, geom, heal)) => {
                            if let Some(entity) = heal {
                                out.heals.push(MateHeal { mate: m.id.clone(), side: k, entity });
                            }
                            sides.push(oc_mate::Side { owner, geom: to_solver(&geom) });
                        }
                        Err(SideError::Missing) => missing.push(k),
                        Err(SideError::Off) => status = Some(MateStatus::Suppressed),
                        Err(SideError::Invalid(e)) => status = Some(MateStatus::Invalid(e)),
                    }
                }
                if !missing.is_empty() && status.is_none() {
                    status = Some(MateStatus::Missing);
                }
            }
            let ok = status.is_none() && sides.len() == 2 && m.refs.len() == 2;
            let placeholder = oc_mate::Side { owner: oc_mate::Owner::Ground, geom: oc_mate::Geom::Point { p: [0.0; 3] } };
            let (a, b) = if ok { (sides[0], sides[1]) } else { (placeholder, placeholder) };
            let mut sm = oc_mate::Mate::new(kind(m.kind), a, b, if m.align == Some(oc_doc::Align::Aligned) { oc_mate::Align::Aligned } else { oc_mate::Align::AntiAligned });
            sm.value = m.value.unwrap_or(0.0);
            sm.flip = m.flip == Some(true);
            sm.relative = m.relative.as_ref().map(pose).unwrap_or_default();
            sm.enabled = ok;
            mates.push(sm);
            pre.push(if ok { None } else { Some(status.unwrap_or(MateStatus::Invalid("a mate needs two references".into()))) });
            missing_of.push(missing);
        }
        let problem = oc_mate::Problem { bodies, mates };
        let solved = oc_mate::solve(&problem);
        out.bodies = doc.instances.iter().filter(|i| !i.is_suppressed()).map(|i| i.id.clone()).collect();
        out.problem = problem;
        out.problem.bodies.iter_mut().zip(&solved.poses).for_each(|(b, p)| b.pose = *p);
        for (k, m) in doc.mates.iter().enumerate() {
            let status = pre[k].clone().unwrap_or_else(|| match &solved.mates[k] {
                oc_mate::MateState::Ok | oc_mate::MateState::Off => MateStatus::Ok,
                oc_mate::MateState::Redundant => MateStatus::Redundant,
                oc_mate::MateState::Conflict => MateStatus::Conflict,
                oc_mate::MateState::Invalid(e) => MateStatus::Invalid(e.clone()),
            });
            out.mates.push(MateOut { id: m.id.clone(), status, missing: missing_of[k].clone() });
        }
        out.newest_error = solved.newest_error.map(|k| doc.mates[k].id.clone());
        out.dof = solved.dof;
        out.converged = solved.converged;
        for i in &doc.instances {
            let Some(&b) = body_of.get(&i.id) else {
                out.instances.push(InstanceOut { id: i.id.clone(), transform: i.transform, state: InstanceState::Suppressed, free: 0 });
                continue;
            };
            let info = &solved.bodies[b];
            let state = match info.state {
                oc_mate::BodyState::Fixed => InstanceState::Fixed,
                oc_mate::BodyState::Under => InstanceState::Under,
                oc_mate::BodyState::Full => InstanceState::Full,
                oc_mate::BodyState::Over => InstanceState::Over,
            };
            let world = transform(&solved.poses[b]);
            out.instances.push(InstanceOut { id: i.id.clone(), transform: world, state, free: info.free });
            let hidden = i.hidden == Some(true);
            match loaded.get(&i.component) {
                Some(Loaded::Part(h)) => out.occurrences.push(Occurrence { path: vec![i.id.clone()], part: h.clone(), world, local: Transform::IDENTITY, hidden }),
                Some(Loaded::Assembly(sub)) => {
                    for o in &sub.occurrences {
                        let mut path = vec![i.id.clone()];
                        path.extend(o.path.iter().cloned());
                        out.occurrences.push(Occurrence { path, part: o.part.clone(), world: world.compose(&o.world), local: o.world, hidden: hidden || o.hidden });
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Registers a part, regenerating it unless it is known or (lightweight) cached.
    fn ensure_part(&mut self, doc: Arc<PartDocument>, opts: &Options) -> String {
        let h = part_hash(&doc, opts.quality);
        let light_ok = opts.lightweight && self.cache.meshes.contains_key(&h);
        let known = self.parts.get(&h).is_some_and(|p| p.live.is_some() || light_ok);
        if let Some(p) = self.parts.get_mut(&h) {
            p.doc.get_or_insert(doc.clone());
        }
        if known {
            return h;
        }
        if light_ok {
            self.ensure_cached(&h);
            if let Some(p) = self.parts.get_mut(&h) {
                p.doc = Some(doc);
            }
            return h;
        }
        let name = doc.name.clone();
        let (live, mesh) = self.regenerate(&doc, opts.quality);
        self.parts.insert(h.clone(), Part { hash: h.clone(), name, doc: Some(doc), mesh, live: Some(live) });
        h
    }

    fn ensure_cached(&mut self, h: &str) {
        if self.parts.contains_key(h) {
            return;
        }
        let name = self.cache.manifest.parts.get(h).map(|p| p.name.clone()).unwrap_or_default();
        let mesh = self.cache.meshes.get(h).cloned();
        self.parts.insert(h.to_string(), Part { hash: h.to_string(), name, doc: None, mesh, live: None });
    }

    fn regenerate(&mut self, doc: &PartDocument, quality: f64) -> (LivePart<K::Body>, Option<Arc<RenderMesh>>) {
        self.regenerations += 1;
        let mut live = LivePart { shape: None, planes: HashMap::new(), axes: HashMap::new(), points: HashMap::new(), errors: vec![] };
        let Ok(r) = self.regen.regenerate_doc(doc, &CancelToken::new()) else { return (live, None) };
        for s in &r.steps {
            if let Some(e) = s.status.error.as_ref().or(s.status.warning.as_ref()) {
                live.errors.push((s.status.id.clone(), e.clone()));
            }
        }
        live.planes = r.planes.iter().map(|(k, f)| (k.clone(), MateGeom::Plane { p: f.origin, n: f.normal })).collect();
        live.axes = r.axes.iter().map(|(k, a)| (k.clone(), MateGeom::Line { p: a.p, d: a.d })).collect();
        live.points = r.points.iter().map(|(k, p)| (k.clone(), MateGeom::Point { p: *p })).collect();
        let mesh = r.body.as_ref().and_then(|b| oc_mesh::build(self.regen.kernel(), &b.body, &MeshOptions { quality, max_lods: 4, threads: 0 }).ok()).map(Arc::new);
        live.shape = r.body;
        (live, mesh)
    }

    /// Regenerates a lightweight part (to pick its geometry). False if its document is
    /// not known (shown from a cache whose file is missing).
    pub fn resolve(&mut self, hash: &str, quality: f64) -> bool {
        let Some(p) = self.parts.get(hash) else { return false };
        if p.live.is_some() {
            return true;
        }
        let Some(doc) = p.doc.clone() else { return false };
        let (live, mesh) = self.regenerate(&doc, quality);
        let p = self.parts.get_mut(hash).expect("known");
        p.live = Some(live);
        if mesh.is_some() {
            p.mesh = mesh;
        }
        true
    }

    /// The mate entity for face, edge or vertex `index` of a part (regenerating it if it
    /// is lightweight). `None` for an index the part does not have.
    pub fn entity(&mut self, hash: &str, kind: &str, index: u32, quality: f64) -> Option<MateEntity> {
        if !self.resolve(hash, quality) {
            return None;
        }
        let k = self.regen.kernel();
        let live = self.parts.get(hash)?.live.as_ref()?;
        let shape = live.shape.as_ref()?;
        let topo = shape.topo(k).ok()?;
        let count = match kind {
            "face" => topo.faces.len(),
            "edge" => topo.edges.len(),
            "vertex" => topo.vertices.len(),
            _ => return None,
        };
        if index as usize >= count {
            return None;
        }
        Some(match kind {
            "face" => {
                let f = FaceId(index);
                MateEntity::Face { sig: oc_naming::signature::face_sig(topo, Some(&shape.names), f), geom: face_geom(topo, f)? }
            }
            "edge" => {
                let e = EdgeId(index);
                MateEntity::Edge { sig: oc_naming::signature::edge_sig(topo, Some(&shape.names), e), geom: edge_geom(topo, e)? }
            }
            "vertex" => {
                let v = VertexId(index);
                let l = Live::new(topo, &shape.names);
                MateEntity::Vertex { p: topo.vertices[v.index()], name: Some(l.vertex_name(v).to_stored()) }
            }
            _ => return None,
        })
    }

    /// One mate side: owner, geometry in the owner's coordinates, and a heal if the
    /// reference was found under another name or its geometry changed.
    fn side(&self, doc: &AssemblyDocument, loaded: &HashMap<Id, Loaded>, body_of: &HashMap<Id, usize>, r: &MateRef) -> Result<(oc_mate::Owner, MateGeom, Option<MateEntity>), SideError> {
        let Some((first, rest)) = r.occurrence.split_first() else {
            return match &r.entity {
                MateEntity::Plane { plane } => Ok((oc_mate::Owner::Ground, std_plane(*plane), None)),
                MateEntity::Origin => Ok((oc_mate::Owner::Ground, MateGeom::Point { p: [0.0; 3] }, None)),
                _ => Err(SideError::Invalid("the assembly has only its planes and origin".into())),
            };
        };
        let inst = doc.instance(first).ok_or(SideError::Missing)?;
        let &body = body_of.get(first).ok_or(SideError::Off)?;
        let owner = oc_mate::Owner::Body(body);
        if matches!(r.entity, MateEntity::Component) {
            return Ok((owner, MateGeom::Point { p: [0.0; 3] }, None));
        }
        let at = loaded.get(&inst.component).ok_or(SideError::Missing)?;
        if !rest.is_empty() {
            // A part inside a sub-assembly: its occurrence there, placed in the top
            // instance's frame.
            let Loaded::Assembly(sub) = at else { return Err(SideError::Missing) };
            let o = sub.occurrences.iter().find(|o| o.path == rest).ok_or(SideError::Missing)?;
            return self.part_side(&o.part, &r.entity).map(|(g, h)| (owner, transformed(&g, &o.world), h));
        }
        match at {
            Loaded::Part(h) => self.part_side(h, &r.entity).map(|(g, heal)| (owner, g, heal)),
            Loaded::Assembly(_) => match &r.entity {
                MateEntity::Plane { plane } => Ok((owner, std_plane(*plane), None)),
                MateEntity::Origin => Ok((owner, MateGeom::Point { p: [0.0; 3] }, None)),
                _ => Err(SideError::Invalid("pick geometry of a part inside the sub-assembly".into())),
            },
            Loaded::Missing => Err(SideError::Missing),
        }
    }

    /// Geometry of an entity of part `h` in part coordinates.
    fn part_side(&self, h: &str, e: &MateEntity) -> Result<(MateGeom, Option<MateEntity>), SideError> {
        let part = self.parts.get(h).ok_or(SideError::Missing)?;
        let stored = |g: &MateGeom| Ok((*g, None));
        let Some(live) = &part.live else {
            // Lightweight: the geometry stored with the reference.
            return match e {
                MateEntity::Face { geom, .. } | MateEntity::Edge { geom, .. } | MateEntity::RefPlane { geom, .. } | MateEntity::RefAxis { geom, .. } | MateEntity::RefPoint { geom, .. } => stored(geom),
                MateEntity::Vertex { p, .. } => Ok((MateGeom::Point { p: *p }, None)),
                MateEntity::Plane { plane } => Ok((std_plane(*plane), None)),
                MateEntity::Origin | MateEntity::Component => Ok((MateGeom::Point { p: [0.0; 3] }, None)),
            };
        };
        let k = self.regen.kernel();
        let heal_geom = |old: &MateGeom, new: MateGeom, make: &dyn Fn(MateGeom) -> MateEntity| -> (MateGeom, Option<MateEntity>) {
            if same(old, &new, 1e-9) {
                (new, None)
            } else {
                (new, Some(make(new)))
            }
        };
        match e {
            MateEntity::Face { sig, geom } => {
                let s = live.shape.as_ref().ok_or(SideError::Missing)?;
                let topo = s.topo(k).map_err(|_| SideError::Missing)?;
                let f = resolve::face(&Live::new(topo, &s.names), sig, &self.rules).ok_or(SideError::Missing)?;
                let g = keep_sense(geom, face_geom(topo, f.id).ok_or_else(|| SideError::Invalid("this face cannot be mated".into()))?);
                let renamed = matches!(f.how, How::Role | How::Legacy | How::Position);
                let new_sig = || oc_naming::signature::face_sig(topo, Some(&s.names), f.id);
                let (g, heal) = heal_geom(geom, g, &|g| MateEntity::Face { sig: new_sig(), geom: g });
                Ok((g, heal.or_else(|| renamed.then(|| MateEntity::Face { sig: new_sig(), geom: g }))))
            }
            MateEntity::Edge { sig, geom } => {
                let s = live.shape.as_ref().ok_or(SideError::Missing)?;
                let topo = s.topo(k).map_err(|_| SideError::Missing)?;
                let f = resolve::edge(&Live::new(topo, &s.names), sig, &self.rules).ok_or(SideError::Missing)?;
                let g = keep_sense(geom, edge_geom(topo, f.id).ok_or_else(|| SideError::Invalid("this edge cannot be mated".into()))?);
                let renamed = matches!(f.how, How::Role | How::Legacy | How::Position);
                let new_sig = || oc_naming::signature::edge_sig(topo, Some(&s.names), f.id);
                let (g, heal) = heal_geom(geom, g, &|g| MateEntity::Edge { sig: new_sig(), geom: g });
                Ok((g, heal.or_else(|| renamed.then(|| MateEntity::Edge { sig: new_sig(), geom: g }))))
            }
            MateEntity::Vertex { p, name } => {
                let s = live.shape.as_ref().ok_or(SideError::Missing)?;
                let topo = s.topo(k).map_err(|_| SideError::Missing)?;
                let l = Live::new(topo, &s.names);
                let v = resolve::vertex(&l, *p, name.as_deref(), &self.rules).ok_or(SideError::Missing)?;
                let q = topo.vertices[v.id.index()];
                let heal = (q != *p || matches!(v.how, How::Role | How::Legacy | How::Position)).then(|| MateEntity::Vertex { p: q, name: Some(l.vertex_name(v.id).to_stored()) });
                Ok((MateGeom::Point { p: q }, heal))
            }
            MateEntity::Plane { plane } => Ok((std_plane(*plane), None)),
            MateEntity::Origin | MateEntity::Component => Ok((MateGeom::Point { p: [0.0; 3] }, None)),
            MateEntity::RefPlane { feature_id, geom } => {
                let g = *live.planes.get(feature_id.as_str()).ok_or(SideError::Missing)?;
                Ok(heal_geom(geom, g, &|g| MateEntity::RefPlane { feature_id: feature_id.clone(), geom: g }))
            }
            MateEntity::RefAxis { feature_id, geom } => {
                let g = *live.axes.get(feature_id.as_str()).ok_or(SideError::Missing)?;
                Ok(heal_geom(geom, g, &|g| MateEntity::RefAxis { feature_id: feature_id.clone(), geom: g }))
            }
            MateEntity::RefPoint { feature_id, geom } => {
                let g = *live.points.get(feature_id.as_str()).ok_or(SideError::Missing)?;
                Ok(heal_geom(geom, g, &|g| MateEntity::RefPoint { feature_id: feature_id.clone(), geom: g }))
            }
        }
    }

    /// The container to save: `doc` plus the cached meshes of the parts `built` used.
    pub fn container(&self, doc: &AssemblyDocument, built: &Built, quality: f64) -> Result<Container, String> {
        let mut comps = BTreeMap::new();
        let mut parts = BTreeMap::new();
        for c in &built.components {
            let Some(h) = &c.part else { continue };
            comps.insert(c.id.to_string(), h.clone());
            if let Some(p) = self.parts.get(h) {
                if let Some(m) = &p.mesh {
                    parts.insert(h.clone(), (p.name.clone(), m.clone()));
                }
            }
        }
        cache::write(doc, quality, &comps, &parts)
    }
}

/// An axis has no direction of its own: the kernel's cylinder axis, a line edge's or a
/// circle's normal can turn round when the part is edited (G1c: deleting one of two holes
/// cut together reversed the other hole's axis), which would flip the mate's alignment and
/// break it. The same entity keeps the sense it had when it was picked. A plane's normal is
/// the face's outward normal and is kept as it is; so is a cone's axis (it says where the
/// apex is).
fn keep_sense(old: &MateGeom, new: MateGeom) -> MateGeom {
    let flip = |d: [f64; 3], o: [f64; 3]| if d[0] * o[0] + d[1] * o[1] + d[2] * o[2] < 0.0 { [-d[0], -d[1], -d[2]] } else { d };
    match (old, new) {
        (MateGeom::Cylinder { d: o, .. }, MateGeom::Cylinder { p, d, r }) => MateGeom::Cylinder { p, d: flip(d, *o), r },
        (MateGeom::Line { d: o, .. }, MateGeom::Line { p, d }) => MateGeom::Line { p, d: flip(d, *o) },
        (MateGeom::Circle { n: o, .. }, MateGeom::Circle { c, n, r }) => MateGeom::Circle { c, n: flip(n, *o), r },
        (_, new) => new,
    }
}

enum SideError {
    Missing,
    /// The instance is suppressed.
    Off,
    Invalid(String),
}

fn kind(t: MateType) -> oc_mate::MateKind {
    match t {
        MateType::Coincident => oc_mate::MateKind::Coincident,
        MateType::Parallel => oc_mate::MateKind::Parallel,
        MateType::Perpendicular => oc_mate::MateKind::Perpendicular,
        MateType::Tangent => oc_mate::MateKind::Tangent,
        MateType::Concentric => oc_mate::MateKind::Concentric,
        MateType::Lock => oc_mate::MateKind::Lock,
        MateType::Distance => oc_mate::MateKind::Distance,
        MateType::Angle => oc_mate::MateKind::Angle,
    }
}

/// Writes heals into a document (no undo step: call inside `History::heal`).
pub fn apply_heals(doc: &mut AssemblyDocument, heals: &[MateHeal]) {
    for h in heals {
        if let Some(m) = doc.mates.iter_mut().find(|m| m.id == h.mate) {
            if let Some(r) = m.refs.get_mut(h.side) {
                r.entity = h.entity.clone();
            }
        }
    }
}

/// Writes solved placements into a document.
pub fn apply_placements(doc: &mut AssemblyDocument, built: &Built) {
    for out in &built.instances {
        if out.state == InstanceState::Suppressed {
            continue;
        }
        if let Some(i) = doc.instances.iter_mut().find(|i| i.id == out.id) {
            i.transform = out.transform;
        }
    }
}

/// Drags instance `id` so its point `grab` (instance coordinates) moves towards `target`
/// (assembly coordinates), within the freedom the mates leave (`oc_mate::drag`); `lever`
/// is how many mm a radian of turning counts as. Updates the instances' placements and
/// states and the occurrences' placements; returns false when the instance cannot move
/// (fixed, suppressed or unknown).
pub fn drag(built: &mut Built, id: &str, grab: [f64; 3], target: [f64; 3], lever: f64) -> bool {
    let Some(b) = built.bodies.iter().position(|x| x.as_str() == id) else { return false };
    if built.problem.bodies[b].fixed {
        return false;
    }
    let solved = oc_mate::drag(&built.problem, b, grab, target, lever);
    take_poses(built, &solved.poses);
    built.dof = solved.dof;
    true
}

/// Puts instance `id` at `to` (Move and Rotate Component by values, rotating by dragging)
/// and solves again, respecting the mates: when the mates allow it where it was put, it
/// stays there and the floating components mated to it follow; otherwise the mates take it
/// to the nearest placement they allow (as SOLIDWORKS moves a mated component only within
/// its freedom). False when the instance cannot move (fixed, suppressed or unknown).
pub fn place(built: &mut Built, id: &str, to: Transform) -> bool {
    let Some(b) = built.bodies.iter().position(|x| x.as_str() == id) else { return false };
    if built.problem.bodies[b].fixed {
        return false;
    }
    let errors = |s: &oc_mate::Solved| s.mates.iter().filter(|m| m.is_error()).count();
    let before = errors(&oc_mate::solve(&built.problem));
    let mut p = built.problem.clone();
    p.bodies[b].pose = pose(&to);
    // Held where it was put, the others follow if they can.
    p.bodies[b].fixed = true;
    let held = oc_mate::solve(&p);
    p.bodies[b].fixed = false;
    let poses = if held.converged && errors(&held) <= before { held.poses } else { oc_mate::solve(&p).poses };
    built.problem.bodies.iter_mut().zip(&poses).for_each(|(x, q)| x.pose = *q);
    // Analysed with every floating body free again.
    let solved = oc_mate::solve(&built.problem);
    take_poses(built, &solved.poses);
    built.dof = solved.dof;
    true
}

/// Writes solved poses into the problem, the instances and their occurrences.
fn take_poses(built: &mut Built, poses: &[oc_mate::Pose]) {
    built.problem.bodies.iter_mut().zip(poses).for_each(|(x, p)| x.pose = *p);
    for (k, bid) in built.bodies.iter().enumerate() {
        let world = transform(&poses[k]);
        if let Some(i) = built.instances.iter_mut().find(|i| i.id == *bid) {
            i.transform = world;
        }
        for o in built.occurrences.iter_mut().filter(|o| o.path[0] == *bid) {
            o.world = world.compose(&o.local);
        }
    }
}
