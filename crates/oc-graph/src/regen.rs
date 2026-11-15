//! Regeneration: evaluate the DAG in tree order with a cache keyed by each node's inputs,
//! cancellation between nodes, and per-node timings.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use oc_doc::{Feature, SketchData};
use oc_kernel::Kernel;
use oc_naming::Rules;

use crate::eval::{AxisDef, Ctx, Instance, NodeOut, Shape};
use crate::graph::{analyse, Fnv, Node};
use crate::math::{Frame, P3};
use crate::resolve::{healed, Heal};

/// Status of one feature after a rebuild (`FeatureStatus`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub id: String,
    pub error: Option<String>,
    pub warning: Option<String>,
}

/// One feature of a rebuild.
pub struct Step<B> {
    pub status: Status,
    /// The part body after this feature.
    pub body: Option<Rc<Shape<B>>>,
    /// Time spent evaluating the feature in this rebuild (zero when cached or suppressed).
    pub time: Duration,
    /// The result came from the cache.
    pub cached: bool,
    /// Cache key: equal keys mean equal inputs (feature data and everything it reads).
    pub key: Option<u128>,
}

/// Result of a rebuild (`RebuildResult` without meshes, which E4 provides).
pub struct Rebuild<B> {
    pub steps: Vec<Step<B>>,
    /// Final body.
    pub body: Option<Rc<Shape<B>>>,
    pub frames: IndexMap<String, Frame>,
    pub planes: IndexMap<String, Frame>,
    pub axes: IndexMap<String, AxisDef>,
    pub points: IndexMap<String, P3>,
    pub csys: IndexMap<String, Frame>,
    /// Sketches with model references, re-solved against the model.
    pub sketches: IndexMap<String, Arc<SketchData>>,
    /// Instance markers of the last pattern that produced any.
    pub instances: IndexMap<String, Vec<Instance>>,
    /// Every evaluated feature's outputs, in tree order (the app's preview reads the tool
    /// bodies and fillet faces of the feature being edited from here).
    pub outs: IndexMap<String, Rc<NodeOut<B>>>,
    /// Features whose legacy references (no persistent name) were found by signature,
    /// with the names written in: apply with [`crate::apply_heals`] inside
    /// `oc_doc::History::heal`. Rebuilding the healed document is a cache hit.
    pub heals: Vec<Heal>,
    /// Faces named `/new/` because no history led to them, over every body the rebuild
    /// made (intermediate ones included). The target is zero.
    pub untracked: u32,
    pub time: Duration,
}

impl<B> Rebuild<B> {
    pub fn statuses(&self) -> Vec<Status> {
        self.steps.iter().map(|s| s.status.clone()).collect()
    }
}

/// A newer edit cancels stale rebuilds: set it from any thread; the rebuild stops before
/// its next feature. (A kernel call in progress runs to its end.)
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// The rebuild was cancelled after `done` features; their results are cached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled {
    pub done: usize,
}

struct Entry<B> {
    out: Option<Rc<NodeOut<B>>>,
    status: Status,
    body: Option<Rc<Shape<B>>>,
    /// The feature with the names of its legacy references written in.
    heal: Option<Arc<Feature>>,
    untracked: u32,
}

impl<B> Clone for Entry<B> {
    fn clone(&self) -> Self {
        Entry { out: self.out.clone(), status: self.status.clone(), body: self.body.clone(), heal: self.heal.clone(), untracked: self.untracked }
    }
}

/// Regenerates feature lists on one kernel. Keeps its cache between rebuilds; lives on
/// the kernel's (geometry host) thread.
pub struct Regenerator<K: Kernel> {
    kernel: K,
    rules: Rules,
    cache: HashMap<u128, Entry<K::Body>>,
    /// Key of a feature with legacy references → key of the same feature once healed. The
    /// entry is stored under the healed key, so the rebuild after the names are written
    /// back finds it, and so does every feature after it.
    alias: HashMap<u128, u128>,
    /// Content hash per feature allocation (unchanged features are pointer-equal across
    /// undo steps, so they are hashed once).
    hashes: HashMap<usize, (Arc<Feature>, u128)>,
    /// Entries kept beyond the last rebuild's (the TypeScript engine keeps 200).
    pub cache_limit: usize,
}

impl<K: Kernel> Regenerator<K> {
    pub fn new(kernel: K) -> Self {
        Self::with_rules(kernel, Rules::default())
    }

    /// With other limits for finding stored references (`oc_naming::Rules`).
    pub fn with_rules(kernel: K, rules: Rules) -> Self {
        Regenerator { kernel, rules, cache: HashMap::new(), alias: HashMap::new(), hashes: HashMap::new(), cache_limit: 200 }
    }

    pub fn kernel(&self) -> &K {
        &self.kernel
    }

    pub fn clear_cache(&mut self) {
        self.cache.clear();
        self.alias.clear();
        self.hashes.clear();
    }

    pub fn cache_len(&self) -> usize {
        self.cache.len()
    }

    fn content_hash(&mut self, f: &Arc<Feature>) -> u128 {
        let ptr = Arc::as_ptr(f) as usize;
        if let Some((_, h)) = self.hashes.get(&ptr) {
            return *h;
        }
        let text = oc_doc::to_canonical_string(&**f).unwrap_or_else(|e| format!("unhashable {e} {:p}", Arc::as_ptr(f)));
        let h = Fnv::default().bytes(text.as_bytes()).finish();
        self.hashes.insert(ptr, (f.clone(), h));
        h
    }

    /// Rebuild a document's features (respecting its rollback bar).
    pub fn regenerate_doc(&mut self, doc: &oc_doc::PartDocument, cancel: &CancelToken) -> Result<Rebuild<K::Body>, Cancelled> {
        let n = doc.rollback.map_or(doc.features.len(), |r| (r as usize).min(doc.features.len()));
        let features: Vec<Arc<Feature>> = doc.features.iter().take(n).cloned().collect();
        self.regenerate(&features, cancel)
    }

    /// Rebuild `features` in order. A failing feature reports an error, its input passes
    /// through unchanged and later features continue.
    pub fn regenerate(&mut self, features: &[Arc<Feature>], cancel: &CancelToken) -> Result<Rebuild<K::Body>, Cancelled> {
        self.regenerate_observed(features, cancel, &mut |_| {})
    }

    /// [`Regenerator::regenerate`], calling `on_step` after each feature (progress, or the
    /// TypeScript engine's `onStep` observer).
    pub fn regenerate_observed(
        &mut self,
        features: &[Arc<Feature>],
        cancel: &CancelToken,
        on_step: &mut dyn FnMut(&Step<K::Body>),
    ) -> Result<Rebuild<K::Body>, Cancelled> {
        let t0 = Instant::now();
        let nodes = analyse(features);
        let mut keys: Vec<Option<u128>> = vec![None; features.len()];
        let mut outs: HashMap<String, Rc<NodeOut<K::Body>>> = HashMap::new();
        let mut body: Option<Rc<Shape<K::Body>>> = None;
        let mut steps: Vec<Step<K::Body>> = Vec::with_capacity(features.len());
        let mut used: HashSet<u128> = HashSet::new();
        let mut heals = vec![];
        let mut untracked = 0;
        for (i, f) in features.iter().enumerate() {
            let Some(node) = &nodes[i] else {
                steps.push(Step { status: Status { id: f.id.as_str().into(), ..Status::default() }, body: body.clone(), time: Duration::ZERO, cached: false, key: None });
                on_step(&steps[i]);
                continue;
            };
            if cancel.is_cancelled() {
                self.evict(&used);
                return Err(Cancelled { done: i });
            }
            let content = self.content_hash(f);
            let mut key = Self::key(content, features, node, &keys);
            if let Some(k) = self.alias.get(&key).filter(|k| self.cache.contains_key(k)) {
                key = *k;
            }
            let start = Instant::now();
            let (entry, cached) = match self.cache.get(&key) {
                Some(e) => (e.clone(), true),
                None => {
                    let e = self.evaluate(features, f, node, &outs, body.clone());
                    if let Some(h) = &e.heal {
                        // Stored under the key the healed feature will have.
                        let to = Self::key(self.content_hash(h), features, node, &keys);
                        self.alias.insert(key, to);
                        key = to;
                    }
                    self.cache.insert(key, e.clone());
                    (e, false)
                }
            };
            keys[i] = Some(key);
            used.insert(key);
            let time = if cached { Duration::ZERO } else { start.elapsed() };
            // Nothing to write back once the document holds the healed feature.
            if let Some(to) = entry.heal.as_ref().filter(|to| !Arc::ptr_eq(f, to) && **f != ***to) {
                heals.push(Heal { from: f.clone(), to: to.clone() });
            }
            untracked += entry.untracked;
            if let Some(o) = &entry.out {
                outs.insert(f.id.as_str().into(), o.clone());
            }
            // Only body writers have the body in their key; everything else passes it on.
            if node.writes_body {
                body = entry.body.clone();
            }
            steps.push(Step { status: entry.status, body: body.clone(), time, cached, key: Some(key) });
            on_step(&steps[i]);
        }
        self.evict(&used);
        let mut r = Rebuild {
            steps,
            body,
            frames: IndexMap::new(),
            planes: IndexMap::new(),
            axes: IndexMap::new(),
            points: IndexMap::new(),
            csys: IndexMap::new(),
            sketches: IndexMap::new(),
            instances: IndexMap::new(),
            outs: IndexMap::new(),
            heals,
            untracked,
            time: Duration::ZERO,
        };
        for f in features {
            let Some(o) = outs.get(f.id.as_str()) else { continue };
            let id = || f.id.as_str().to_string();
            o.frame.map(|x| r.frames.insert(id(), x));
            o.plane.map(|x| r.planes.insert(id(), x));
            o.axis.map(|x| r.axes.insert(id(), x));
            o.point.map(|x| r.points.insert(id(), x));
            o.csys.map(|x| r.csys.insert(id(), x));
            if let Some(s) = &o.sketch {
                r.sketches.insert(id(), s.clone());
            }
            if let Some(x) = &o.instances {
                r.instances.insert(id(), x.clone());
            }
            r.outs.insert(id(), o.clone());
        }
        r.time = t0.elapsed();
        Ok(r)
    }

    /// Key of a node: its feature's content, the keys of the features it reads (or, for
    /// references that are not edges, their position and name), and the body it starts from.
    fn key(content: u128, features: &[Arc<Feature>], node: &Node, keys: &[Option<u128>]) -> u128 {
        let mut h = Fnv::default().u128(content);
        if node.reads_body {
            h = h.bytes(b"body").u128(node.body_from.and_then(|b| keys[b]).unwrap_or(0));
        }
        for id in &node.refs {
            h = h.bytes(id.as_bytes()).bytes(&[0]);
            match features.iter().position(|f| f.id.as_str() == id) {
                Some(p) if node.deps.contains(&p) => h = h.bytes(b"dep").u128(keys[p].unwrap_or(0)),
                Some(p) => h = h.bytes(b"absent").bytes(&(p as u64).to_le_bytes()).bytes(features[p].name.as_bytes()),
                None => h = h.bytes(b"missing"),
            }
        }
        h.finish()
    }

    fn evaluate(
        &self,
        features: &[Arc<Feature>],
        f: &Feature,
        node: &Node,
        outs: &HashMap<String, Rc<NodeOut<K::Body>>>,
        body: Option<Rc<Shape<K::Body>>>,
    ) -> Entry<K::Body> {
        let ctx = Ctx::new(&self.kernel, self.rules, features, f, outs, body.clone());
        let res = crate::features::apply(&ctx, f);
        if cfg!(debug_assertions) {
            check_reads(features, node, &ctx, outs);
        }
        let log = ctx.log.take();
        let heal = healed(f, &log.found).map(Arc::new);
        let mut status = Status { id: f.id.as_str().into(), ..Status::default() };
        match res {
            Ok(mut out) => {
                out.body = ctx.result_body();
                // The feature's own warning, then what its references ran into.
                let warnings: Vec<String> = out.warning.iter().cloned().chain(log.warnings()).collect();
                status.warning = (!warnings.is_empty()).then(|| warnings.join("; "));
                let after = out.body.clone().or(body);
                Entry { out: Some(Rc::new(out)), status, body: after, heal, untracked: log.untracked }
            }
            Err(e) => {
                status.error = Some(e.message);
                Entry { out: None, status, body, heal, untracked: log.untracked }
            }
        }
    }

    fn evict(&mut self, used: &HashSet<u128>) {
        if self.cache.len() > self.cache_limit {
            self.cache.retain(|k, _| used.contains(k));
            self.alias.retain(|_, to| used.contains(to));
            if self.hashes.len() > self.cache_limit * 4 {
                self.hashes.clear();
            }
        }
    }
}

/// Debug builds: every output a feature read and the body must be among its declared
/// inputs, or the cache key would miss a dependency.
fn check_reads<K: Kernel>(features: &[Arc<Feature>], node: &Node, ctx: &Ctx<'_, K>, outs: &HashMap<String, Rc<NodeOut<K::Body>>>) {
    for id in ctx.reads.borrow().iter() {
        if !outs.contains_key(id) {
            continue;
        }
        let p = features.iter().position(|f| f.id.as_str() == id);
        assert!(
            p.is_some_and(|p| node.deps.contains(&p)),
            "{} read {id}, which is not among its dependencies {:?}",
            features[node.index].name,
            node.deps
        );
    }
    assert!(!ctx.read_body.get() || node.reads_body, "{} read the body without declaring it", features[node.index].name);
}
