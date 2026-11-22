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
use oc_mesh::RenderMesh;
use oc_naming::{resolve, How, Live, Rules};

use crate::cache::{self, part_hash, Cache};
use crate::geom::*;
use crate::locate::{locate, normalize, Found, Store};
use crate::parallel::{self, Memo, PartJob, RefGeometry};

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

impl<B> LivePart<B> {
    pub(crate) fn from_geometry(g: RefGeometry) -> Self {
        LivePart { shape: None, planes: g.planes, axes: g.axes, points: g.points, errors: g.errors }
    }
}

/// A distinct part (by content hash).
pub struct Part<B> {
    pub hash: String,
    pub name: String,
    pub doc: Option<Arc<PartDocument>>,
    pub mesh: Option<Arc<RenderMesh>>,
    /// `None` while lightweight (shown from the cache, not regenerated) or evicted to keep
    /// within the body budget.
    pub live: Option<LivePart<B>>,
    /// The part's B-rep cache entry (G3c, [`cache::encode_brep`]): read from the container,
    /// or made when the part was evicted or saved. Resolving reads it instead of regenerating.
    pub brep: Option<Arc<Vec<u8>>>,
    /// When it was last used (the assembler's clock), for evicting the least recently used.
    pub(crate) used: std::cell::Cell<u64>,
}

impl<B> Part<B> {
    pub(crate) fn new(hash: String, name: String, doc: Option<Arc<PartDocument>>, mesh: Option<Arc<RenderMesh>>, live: Option<LivePart<B>>) -> Self {
        Part { hash, name, doc, mesh, live, brep: None, used: std::cell::Cell::new(0) }
    }
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
    /// The configurations of the component's document, by name (G1e; `Default` for one
    /// without any), for Component Properties' Referenced configuration.
    pub configs: Vec<String>,
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

/// A component's file as read.
enum Src {
    /// The document and the content hash of its file's bytes.
    Part(Arc<PartDocument>, String),
    Assembly(Box<AssemblyDocument>, Box<Cache>, PathBuf),
    /// Not found, shown from the assembly's cache (this hash).
    Cached(String),
    Missing,
}

/// Configuration names of a document (one, `Default`, without configurations).
fn names_of(c: Option<&oc_doc::Configurations>) -> Vec<String> {
    c.map_or_else(|| vec![oc_doc::DEFAULT_CONFIGURATION.to_string()], |c| c.list.iter().map(|x| x.name.clone()).collect())
}

/// The key of an instance's component as built: the component and the configuration the
/// instance references.
fn loaded_key(doc: &AssemblyDocument, i: &oc_doc::Instance) -> (Id, String) {
    (i.component.clone(), oc_doc::config::referenced_configuration(doc, i).to_string())
}

/// A part file as one configuration has it: the document, its part hash and the warning when
/// the part has no such configuration.
type Configured = (Arc<PartDocument>, String, Option<String>);

/// What the last build (or [`Assembler::resolve_many`]) regenerated.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BuildStats {
    /// Parts regenerated.
    pub regenerated: usize,
    /// Threads they were regenerated on: 1 when on the assembler's own thread, 0 when nothing
    /// was regenerated.
    pub workers: usize,
    /// Wall time spent regenerating, meshing and taking the bodies over.
    pub time: std::time::Duration,
    /// Of that, reading the workers' bodies back on the assembler's thread.
    pub take_over: std::time::Duration,
    /// Parts read from their B-rep caches instead of being regenerated (G3c).
    pub read: usize,
    /// Parts whose bodies were dropped to keep within the body budget (G3c).
    pub evicted: usize,
}

/// Builds assemblies on one kernel; keeps every part it regenerated, by hash, so a
/// rebuild after a mate edit regenerates nothing. When several parts need regenerating they
/// are regenerated at once on worker threads ([`crate::parallel`], G3a); one alone is
/// regenerated on the assembler's thread, where its results stay in the shared
/// regeneration cache (a part window opened from the assembly finds them).
pub struct Assembler<K: Kernel> {
    pub regen: Regenerator<K>,
    pub parts: HashMap<String, Part<K::Body>>,
    cache: Cache,
    pub rules: Rules,
    /// Parts regenerated so far (shared and cached parts are not regenerated again).
    pub regenerations: usize,
    /// Worker threads for regenerating several parts at once: 0 half the logical processors,
    /// 1 none (every part on the assembler's thread). Used when there is no `runner`.
    pub workers: usize,
    /// Where several parts regenerate at once instead of this process' worker threads
    /// (geometry-host processes, G3b).
    pub runner: Option<Box<dyn parallel::Runner>>,
    /// What the last build regenerated.
    pub stats: BuildStats,
    /// While planning a build: the parts it will regenerate, instead of regenerating them.
    plan: Option<Vec<PartJob>>,
    /// Most memory the parts' bodies may take, estimated (G3c, [`crate::outofcore`]); `None`:
    /// no limit. The least recently used bodies beyond it are dropped and read back from their
    /// B-rep caches when needed.
    pub body_budget: Option<usize>,
    pub(crate) clock: std::cell::Cell<u64>,
    /// Part files read before, by the content hash of their bytes, and each one as a
    /// configuration has it with its part hash (and the warning when the configuration is
    /// missing), so unchanged files are neither parsed nor hashed again (a rebuild reads
    /// every component; G3a's planning pass reads them twice).
    docs: HashMap<String, Arc<PartDocument>>,
    configured: HashMap<(String, String, u64), Configured>,
}

impl<K: Kernel + Clone + Send> Assembler<K> {
    pub fn new(kernel: K) -> Self {
        Self::with(Regenerator::new(kernel))
    }

    pub fn with(regen: Regenerator<K>) -> Self {
        Assembler { regen, parts: HashMap::new(), cache: Cache::default(), rules: Rules::default(), regenerations: 0, workers: 0, runner: None, stats: BuildStats::default(), plan: None, body_budget: None, clock: std::cell::Cell::new(0), docs: HashMap::new(), configured: HashMap::new() }
    }

    /// Uses the caches of an opened container: meshes for lightweight builds, B-reps for
    /// resolving parts (G3c) and for resolved builds (G3d). Caches written by another engine
    /// build are ignored ([`Assembler::accepts`]).
    pub fn set_cache(&mut self, cache: Cache) {
        self.cache = if self.accepts(&cache) { cache } else { Cache::default() };
    }

    /// The engine build this assembler's caches are keyed to (G3d): the app's version and
    /// the kernel's identity.
    pub fn engine(&self) -> cache::Engine {
        cache::Engine { app: cache::APP_VERSION.to_string(), kernel: self.regen.kernel().identity() }
    }

    /// Whether caches were written by this engine build (G3d, owner decision 3 on G3c: they
    /// are trusted only then; another app version or kernel, or a file saved before caches
    /// named their engine, regenerates its parts).
    pub fn accepts(&self, cache: &Cache) -> bool {
        cache.manifest.engine.as_ref() == Some(&self.engine())
    }

    /// Builds `doc`, whose file is in folder `dir`. The parts that need regenerating, in
    /// sub-assemblies too, are found first by a pass that reads the components without
    /// regenerating anything; when there are several they are regenerated at once.
    pub fn build(&mut self, doc: &AssemblyDocument, dir: &Path, store: &dyn Store, opts: &Options) -> Built {
        self.stats = BuildStats::default();
        let store = Memo::new(store);
        if self.parallel() {
            self.plan = Some(vec![]);
            let _ = self.build_inner(doc, dir, &store, opts, &mut vec![]);
            let jobs = self.plan.take().unwrap_or_default();
            if jobs.len() > 1 {
                self.regenerate_all(jobs);
            }
        }
        let t = std::time::Instant::now();
        let before = self.regenerations;
        let b = self.build_inner(doc, dir, &store, opts, &mut vec![]);
        if self.regenerations > before {
            self.stats.regenerated += self.regenerations - before;
            self.stats.workers = self.stats.workers.max(1);
            self.stats.time += t.elapsed();
        }
        self.keep_budget();
        b
    }

    fn build_inner(&mut self, doc: &AssemblyDocument, dir: &Path, store: &dyn Store, opts: &Options, stack: &mut Vec<PathBuf>) -> Built {
        let mut out = Built::default();
        // Each component's document, read once.
        let mut sources: HashMap<Id, Src> = HashMap::new();
        let mut found_dirs: Vec<PathBuf> = vec![];
        for c in &doc.components {
            let mut co = ComponentOut { id: c.id.clone(), kind: c.kind, path: None, found: None, part: None, error: None, warning: None, configs: vec![] };
            let hit = locate(store, dir, &c.path, c.abs_path.as_deref(), &opts.search, &found_dirs);
            let src = match (hit, c.kind) {
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
                        ComponentKind::Part => {
                            let key = oc_doc::container::content_hash(&[&bytes]);
                            let parsed = match self.docs.get(&key) {
                                Some(d) => Ok(d.clone()),
                                None => std::str::from_utf8(&bytes).map_err(|e| e.to_string()).and_then(|t| oc_doc::load(t).map_err(|e| e.to_string())).map(|l| Arc::new(l.doc)),
                            };
                            match parsed {
                                Ok(d) => {
                                    if self.docs.len() > 4096 {
                                        self.docs.clear();
                                        self.configured.clear();
                                    }
                                    self.docs.insert(key.clone(), d.clone());
                                    co.configs = names_of(d.configurations.as_deref());
                                    Src::Part(d, key)
                                }
                                Err(e) => {
                                    co.error = Some(format!("{}: {e}", path.display()));
                                    Src::Missing
                                }
                            }
                        }
                        ComponentKind::Assembly => {
                            let key = normalize(&path);
                            if stack.contains(&key) {
                                co.error = Some("the assembly contains itself".into());
                                Src::Missing
                            } else {
                                match Container::from_bytes(&bytes).map_err(|e| e.to_string()).and_then(|c| {
                                    let d = oc_doc::load_assembly(c.document().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                                    let cache = Some(Cache::read(&c)).filter(|x| self.accepts(x)).unwrap_or_default();
                                    Ok((d.doc, cache))
                                }) {
                                    Ok((sub, cache)) => {
                                        co.configs = names_of(sub.configurations.as_ref());
                                        Src::Assembly(Box::new(sub), Box::new(cache), key)
                                    }
                                    Err(e) => {
                                        co.error = Some(format!("{}: {e}", path.display()));
                                        Src::Missing
                                    }
                                }
                            }
                        }
                    }
                }
                (None, ComponentKind::Part) => match self.cache.manifest.components.get(c.id.as_str()).cloned().filter(|h| self.cache.meshes.contains_key(h)) {
                    Some(h) => {
                        co.warning = Some(format!("{} was not found; shown from the assembly's cache", c.path));
                        Src::Cached(h)
                    }
                    None => {
                        co.error = Some(format!("{} was not found", c.path));
                        Src::Missing
                    }
                },
                (None, ComponentKind::Assembly) => {
                    co.error = Some(format!("{} was not found", c.path));
                    Src::Missing
                }
            };
            sources.insert(c.id.clone(), src);
            out.components.push(co);
        }
        // Each (component, configuration) the instances reference (G1e), built once: the
        // component's own configuration first, so its part hash is the component's.
        let mut loaded: HashMap<(Id, String), Loaded> = HashMap::new();
        let mut wanted: Vec<(Id, String)> = doc.components.iter().map(|c| (c.id.clone(), c.config.clone())).collect();
        let mut listed: std::collections::HashSet<(Id, String)> = wanted.iter().cloned().collect();
        for i in &doc.instances {
            let k = (i.component.clone(), oc_doc::config::referenced_configuration(doc, i).to_string());
            if listed.insert(k.clone()) {
                wanted.push(k);
            }
        }
        for (cid, config) in wanted {
            let Some(src) = sources.get(&cid) else { continue };
            let ci = doc.components.iter().position(|c| c.id == cid).expect("listed above");
            let first = config == doc.components[ci].config;
            let l = match src {
                Src::Part(d, key) => {
                    let mk = (key.clone(), config.clone(), opts.quality.to_bits());
                    let (configured, h, warning) = match self.configured.get(&mk) {
                        Some(x) => x.clone(),
                        None => {
                            let (c, w) = match oc_graph::config::configured(d, &config) {
                                Ok(x) => (Arc::new(x), None),
                                // SOLIDWORKS shows the part in its active configuration and warns.
                                Err(e) => (d.clone(), Some(format!("{e}; the part's active configuration is used"))),
                            };
                            let h = part_hash(&c, opts.quality);
                            self.configured.insert(mk, (c.clone(), h.clone(), w.clone()));
                            (c, h, w)
                        }
                    };
                    if let Some(w) = warning {
                        out.components[ci].warning.get_or_insert(w);
                    }
                    let h = self.ensure_part(configured, h, opts);
                    if first {
                        out.components[ci].part = Some(h.clone());
                    }
                    Loaded::Part(h)
                }
                Src::Assembly(sub, cache, key) => {
                    let mut sub = (**sub).clone();
                    if sub.configurations.is_some() {
                        oc_doc::config::capture_assembly(&mut sub);
                        if let Err(e) = oc_doc::config::apply_assembly(&mut sub, &config) {
                            out.components[ci].warning.get_or_insert(format!("{e}; the sub-assembly's active configuration is used"));
                        }
                    }
                    let outer = std::mem::replace(&mut self.cache, (**cache).clone());
                    stack.push(key.clone());
                    let sub_dir = out.components[ci].path.as_ref().and_then(|p| p.parent()).map(Path::to_path_buf).unwrap_or_default();
                    let b = self.build_inner(&sub, &sub_dir, store, opts, stack);
                    stack.pop();
                    self.cache = outer;
                    Loaded::Assembly(Box::new(b))
                }
                Src::Cached(h) => {
                    self.ensure_cached(h);
                    if first {
                        out.components[ci].part = Some(h.clone());
                    }
                    Loaded::Part(h.clone())
                }
                Src::Missing => Loaded::Missing,
            };
            loaded.insert((cid, config), l);
        }
        if self.plan.is_some() {
            // Planning: the parts to regenerate are known; nothing else is needed.
            return out;
        }
        // Bodies: the top-level instances that take part.
        let mut body_of: HashMap<Id, usize> = HashMap::new();
        let mut bodies = vec![];
        for i in doc.instances.iter().filter(|i| !i.is_suppressed()) {
            body_of.insert(i.id.clone(), bodies.len());
            bodies.push(oc_mate::Body { pose: pose(&i.transform), fixed: i.fixed });
        }
        // Mates (instances found through a map: G3c, 10 000 instances).
        let instance_of: HashMap<&Id, &oc_doc::Instance> = doc.instances.iter().map(|i| (&i.id, i)).collect();
        let mut mates = vec![];
        let mut pre: Vec<Option<MateStatus>> = vec![];
        let mut missing_of: Vec<Vec<usize>> = vec![];
        for m in &doc.mates {
            let mut sides = vec![];
            let mut status = m.is_suppressed().then_some(MateStatus::Suppressed);
            let mut missing = vec![];
            if status.is_none() {
                for (k, r) in m.refs.iter().enumerate() {
                    match self.side(doc, &instance_of, &loaded, &body_of, r) {
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
            match loaded.get(&loaded_key(doc, i)) {
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
    fn ensure_part(&mut self, doc: Arc<PartDocument>, h: String, opts: &Options) -> String {
        let light_ok = opts.lightweight && self.cache.meshes.contains_key(&h);
        // An evicted part (G3c) is lightweight with its B-rep in memory: kept as it is by a
        // lightweight build, read back by another.
        let evicted = self.parts.get(&h).is_some_and(|p| p.live.is_none() && p.brep.is_some() && p.mesh.is_some());
        let known = self.parts.get(&h).is_some_and(|p| p.live.is_some() || light_ok || (opts.lightweight && evicted));
        if let Some(p) = self.parts.get_mut(&h) {
            p.doc.get_or_insert(doc.clone());
        }
        self.touch(&h);
        if known {
            return h;
        }
        if evicted && self.read_brep(&h) {
            return h;
        }
        // A resolved build of a freshly opened file reads the part's B-rep cache, written by
        // this engine build, instead of regenerating it (G3d, owner decision 3 on G3c).
        if !opts.lightweight && !self.parts.contains_key(&h) && self.cache.meshes.contains_key(&h) && self.cache.breps.contains_key(&h) {
            self.ensure_cached(&h);
            if let Some(p) = self.parts.get_mut(&h) {
                p.doc = Some(doc.clone());
            }
            if self.read_brep(&h) {
                return h;
            }
            // It did not read (dropped): regenerated below.
            self.parts.remove(&h);
        }
        if light_ok {
            self.ensure_cached(&h);
            if let Some(p) = self.parts.get_mut(&h) {
                p.doc = Some(doc);
            }
            return h;
        }
        if let Some(plan) = &mut self.plan {
            if !plan.iter().any(|j| j.hash == h) {
                plan.push(PartJob { hash: h.clone(), doc, quality: opts.quality });
            }
            return h;
        }
        let name = doc.name.clone();
        let (live, mesh) = self.regenerate(&doc, opts.quality);
        self.parts.insert(h.clone(), Part::new(h.clone(), name, Some(doc), mesh, Some(live)));
        self.touch(&h);
        h
    }

    fn ensure_cached(&mut self, h: &str) {
        if self.parts.contains_key(h) {
            return;
        }
        let name = self.cache.manifest.parts.get(h).map(|p| p.name.clone()).unwrap_or_default();
        let mesh = self.cache.meshes.get(h).cloned();
        let mut p = Part::new(h.to_string(), name, None, mesh, None);
        p.brep = self.cache.breps.get(h).cloned();
        self.parts.insert(h.to_string(), p);
    }

    /// Regenerates a part on this thread (its results stay in the shared regeneration cache).
    pub(crate) fn regenerate(&mut self, doc: &PartDocument, quality: f64) -> (LivePart<K::Body>, Option<Arc<RenderMesh>>) {
        self.regenerations += 1;
        let Ok(r) = self.regen.regenerate_doc(doc, &CancelToken::new()) else { return (LivePart::from_geometry(RefGeometry::default()), None) };
        let mut live = LivePart::from_geometry(RefGeometry::of(&r));
        let mesh = r.body.as_ref().and_then(|b| oc_mesh::build(self.regen.kernel(), &b.body, &parallel::mesh_options(quality)).ok()).map(Arc::new);
        live.shape = r.body;
        (live, mesh)
    }

    /// Regenerates a lightweight part (to pick its geometry). False if its document is
    /// not known (shown from a cache whose file is missing).
    pub fn resolve(&mut self, hash: &str, quality: f64) -> bool {
        let Some(p) = self.parts.get(hash) else { return false };
        p.used.set(self.tick());
        if p.live.is_some() {
            return true;
        }
        // Its B-rep cache (G3c): no regeneration; also for a part whose file is missing.
        if p.brep.is_some() && self.read_brep(hash) {
            self.keep_budget();
            return true;
        }
        let Some(p) = self.parts.get(hash) else { return false };
        let Some(doc) = p.doc.clone() else { return false };
        let (live, mesh) = self.regenerate(&doc, quality);
        let p = self.parts.get_mut(hash).expect("known");
        p.live = Some(live);
        if mesh.is_some() {
            p.mesh = mesh;
        }
        self.keep_budget();
        true
    }

    /// An entity of an occurrence's part, found as mate references are found (by persistent
    /// name, then position; G1d in-context references): the entity as it is now, renamed or
    /// moved, in its part's coordinates, and the occurrence's placement. `None` when the
    /// occurrence or the entity is gone. A lightweight part is regenerated first.
    pub fn occurrence_entity(&mut self, built: &Built, path: &[Id], e: &MateEntity, quality: f64) -> Option<(MateEntity, Transform)> {
        let o = built.occurrences.iter().find(|o| o.path == path)?;
        let (h, world) = (o.part.clone(), o.world);
        if !self.resolve(&h, quality) {
            return None;
        }
        let (_, heal) = self.part_side(&h, e).ok()?;
        Some((heal.unwrap_or_else(|| e.clone()), world))
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
    fn side(&self, doc: &AssemblyDocument, instance_of: &HashMap<&Id, &oc_doc::Instance>, loaded: &HashMap<(Id, String), Loaded>, body_of: &HashMap<Id, usize>, r: &MateRef) -> Result<(oc_mate::Owner, MateGeom, Option<MateEntity>), SideError> {
        let Some((first, rest)) = r.occurrence.split_first() else {
            return match &r.entity {
                MateEntity::Plane { plane } => Ok((oc_mate::Owner::Ground, std_plane(*plane), None)),
                MateEntity::Origin => Ok((oc_mate::Owner::Ground, MateGeom::Point { p: [0.0; 3] }, None)),
                _ => Err(SideError::Invalid("the assembly has only its planes and origin".into())),
            };
        };
        let inst = *instance_of.get(first).ok_or(SideError::Missing)?;
        let &body = body_of.get(first).ok_or(SideError::Off)?;
        let owner = oc_mate::Owner::Body(body);
        if matches!(r.entity, MateEntity::Component) {
            return Ok((owner, MateGeom::Point { p: [0.0; 3] }, None));
        }
        let at = loaded.get(&loaded_key(doc, inst)).ok_or(SideError::Missing)?;
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
        part.used.set(self.tick());
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

    /// The container to save: `doc` plus the cached meshes and B-reps (G3c) of the parts
    /// `built` used.
    pub fn container(&self, doc: &AssemblyDocument, built: &Built, quality: f64) -> Result<Container, String> {
        let mut comps = BTreeMap::new();
        let mut parts = BTreeMap::new();
        let mut breps = BTreeMap::new();
        let mut mesh_of = |h: &String| {
            // Once per part (occurrences repeat parts: 10 000 of 100 in the scale assembly).
            if parts.contains_key(h) {
                return;
            }
            if let Some(m) = self.parts.get(h).and_then(|p| p.mesh.clone().map(|m| (p.name.clone(), m))) {
                parts.insert(h.clone(), m);
                if let Some(b) = self.brep_of(h) {
                    breps.insert(h.clone(), b);
                }
            }
        };
        for c in &built.components {
            let Some(h) = &c.part else { continue };
            comps.insert(c.id.to_string(), h.clone());
            mesh_of(h);
        }
        // Occurrences of other configurations of a component (G1e: a mesh per part and
        // configuration in use).
        for o in &built.occurrences {
            mesh_of(&o.part);
        }
        cache::write(doc, quality, &self.engine(), &comps, &parts, &breps)
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
    let solved: HashMap<&Id, &InstanceOut> = built.instances.iter().filter(|o| o.state != InstanceState::Suppressed).map(|o| (&o.id, o)).collect();
    for i in doc.instances.iter_mut() {
        if let Some(out) = solved.get(&i.id) {
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
    // By id through a map (G3c: 10 000 instances made this quadratic).
    let at: HashMap<&Id, usize> = built.bodies.iter().enumerate().map(|(k, id)| (id, k)).collect();
    for i in built.instances.iter_mut() {
        if let Some(&k) = at.get(&i.id) {
            i.transform = transform(&poses[k]);
        }
    }
    for o in built.occurrences.iter_mut() {
        if let Some(&k) = o.path.first().and_then(|id| at.get(id)) {
            o.world = transform(&poses[k]).compose(&o.local);
        }
    }
}
