//! Independent parts regenerated at once (G3a). Each worker thread owns its own kernel and
//! regenerator (bodies are not `Send`: a kernel and its bodies stay on one thread, as on a
//! geometry host) and hands back plain data only: the part's final body as the kernel's
//! native blob ([`oc_kernel::Kernel::export_native`]) with its persistent names, its
//! reference geometry, its feature errors and its display mesh. The assembler's thread
//! reads the body back ([`oc_kernel::Kernel::import_native`]). These jobs and results are
//! what a geometry-host process will exchange (G3b).
//!
//! A part's own feature chain stays serial (E5, owner decision 2026-11-13); parts are
//! independent of each other, so a part's result does not depend on which worker made it or
//! on what else ran at the same time (`tests/parallel.rs`).
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use oc_doc::{MateGeom, PartDocument};
use oc_graph::{CancelToken, Rebuild, Regenerator, Shape};
use oc_kernel::Kernel;
use oc_mesh::{MeshOptions, RenderMesh};
use oc_naming::{BodyNames, Rules};

use crate::build::{Assembler, BuildStats, LivePart, Part};
use crate::locate::Store;

/// Reference geometry and feature errors of a regenerated part (part coordinates).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefGeometry {
    pub planes: HashMap<String, MateGeom>,
    pub axes: HashMap<String, MateGeom>,
    pub points: HashMap<String, MateGeom>,
    /// Features with an error or warning: (id, message).
    pub errors: Vec<(String, String)>,
}

impl RefGeometry {
    pub fn of<B>(r: &Rebuild<B>) -> RefGeometry {
        let mut g = RefGeometry::default();
        for s in &r.steps {
            if let Some(e) = s.status.error.as_ref().or(s.status.warning.as_ref()) {
                g.errors.push((s.status.id.clone(), e.clone()));
            }
        }
        g.planes = r.planes.iter().map(|(k, f)| (k.clone(), MateGeom::Plane { p: f.origin, n: f.normal })).collect();
        g.axes = r.axes.iter().map(|(k, a)| (k.clone(), MateGeom::Line { p: a.p, d: a.d })).collect();
        g.points = r.points.iter().map(|(k, p)| (k.clone(), MateGeom::Point { p: *p })).collect();
        g
    }
}

/// A part to regenerate: its (configured) document and the Image Quality of its mesh.
#[derive(Clone)]
pub struct PartJob {
    pub hash: String,
    pub doc: Arc<PartDocument>,
    pub quality: f64,
}

/// A part regenerated on a worker: plain data only.
pub struct PartOut {
    /// The final body: the kernel's native blob and the persistent name of each face, in the
    /// kernel's face order. `None` when the part has no body (or the rebuild was cancelled).
    pub body: Option<(Vec<u8>, BodyNames)>,
    pub geometry: RefGeometry,
    pub mesh: Option<RenderMesh>,
    /// Time on the worker: regeneration, meshing and export.
    pub time: Duration,
}

/// Display mesh options of an assembly part.
pub fn mesh_options(quality: f64) -> MeshOptions {
    MeshOptions { quality, max_lods: 4, threads: 0 }
}

/// Regenerates one part on `regen` (a worker's) and exports its body.
pub fn work<K: Kernel>(regen: &mut Regenerator<K>, job: &PartJob) -> Result<PartOut, String> {
    let t = std::time::Instant::now();
    let Ok(r) = regen.regenerate_doc(&job.doc, &CancelToken::new()) else {
        return Ok(PartOut { body: None, geometry: RefGeometry::default(), mesh: None, time: t.elapsed() });
    };
    let geometry = RefGeometry::of(&r);
    let k = regen.kernel();
    let mut body = None;
    let mut mesh = None;
    if let Some(s) = &r.body {
        mesh = oc_mesh::build(k, &s.body, &mesh_options(job.quality)).ok();
        let blob = k.export_native(&s.body).map_err(|e| format!("{}: {e}", job.doc.name))?;
        body = Some((blob, s.names.clone()));
    }
    Ok(PartOut { body, geometry, mesh, time: t.elapsed() })
}

/// How many workers for `jobs` parts: `workers` (0: one per logical processor), at most one
/// per part.
pub fn worker_count(workers: usize, jobs: usize) -> usize {
    let n = if workers == 0 { std::thread::available_parallelism().map_or(1, |n| n.get()) } else { workers };
    n.min(jobs).max(1)
}

/// Regenerates `jobs` on `workers` threads, each with a clone of `kernel` and a regenerator
/// with `rules`; results in job order. A part whose regeneration panics is an `Err` (the
/// others are unaffected). Each worker drops its regeneration cache after every part, so
/// at most one part per worker is held in memory besides the results.
pub fn run<K: Kernel + Clone + Send>(kernel: &K, rules: Rules, jobs: &[PartJob], workers: usize) -> Vec<Result<PartOut, String>> {
    let workers = worker_count(workers, jobs.len());
    let next = AtomicUsize::new(0);
    let mut results: Vec<Option<Result<PartOut, String>>> = (0..jobs.len()).map(|_| None).collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|i| {
                let k = kernel.clone();
                let next = &next;
                std::thread::Builder::new()
                    .name(format!("part-worker-{i}"))
                    .spawn_scoped(s, move || {
                        let mut regen = Regenerator::with_rules(k, rules);
                        let mut done = vec![];
                        loop {
                            let j = next.fetch_add(1, Ordering::Relaxed);
                            let Some(job) = jobs.get(j) else { break };
                            let r = catch_unwind(AssertUnwindSafe(|| work(&mut regen, job))).unwrap_or_else(|_| Err(format!("{}: the regeneration failed", job.doc.name)));
                            regen.clear_cache();
                            done.push((j, r));
                        }
                        done
                    })
                    .expect("part worker thread")
            })
            .collect();
        for h in handles {
            for (j, r) in h.join().unwrap_or_default() {
                results[j] = Some(r);
            }
        }
    });
    results.into_iter().map(|r| r.unwrap_or_else(|| Err("the part worker stopped".into()))).collect()
}

/// Reads each file once per build (the plan pass and the build read the same files).
pub struct Memo<'a> {
    inner: &'a dyn Store,
    files: RefCell<HashMap<PathBuf, Option<Vec<u8>>>>,
}

impl<'a> Memo<'a> {
    pub fn new(inner: &'a dyn Store) -> Self {
        Memo { inner, files: RefCell::new(HashMap::new()) }
    }
}

impl Store for Memo<'_> {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        if let Some(b) = self.files.borrow().get(path) {
            return b.clone();
        }
        let b = self.inner.read(path);
        self.files.borrow_mut().insert(path.to_path_buf(), b.clone());
        b
    }
    fn exists(&self, path: &Path) -> bool {
        match self.files.borrow().get(path) {
            Some(b) => b.is_some(),
            None => self.inner.exists(path),
        }
    }
}

/// Regenerating several parts at once from a build or for evaluation.
impl<K: Kernel + Clone + Send> Assembler<K> {
    /// Regenerates `jobs` on worker threads and takes their results over (in job order).
    pub(crate) fn regenerate_all(&mut self, jobs: Vec<PartJob>) {
        let t = Instant::now();
        let workers = worker_count(self.workers, jobs.len());
        let outs = run(self.regen.kernel(), self.regen.rules(), &jobs, workers);
        for (job, out) in jobs.into_iter().zip(outs) {
            let (live, mesh) = self.taken_or_regenerated(&job, out);
            let name = job.doc.name.clone();
            self.parts.insert(job.hash.clone(), Part { hash: job.hash, name, doc: Some(job.doc), mesh, live: Some(live) });
        }
        self.stats.workers = workers;
        self.stats.time += t.elapsed();
    }

    /// A worker's result on this thread, its body read back from the native blob. A part the
    /// worker could not regenerate (it panicked) or whose body cannot be read back is
    /// regenerated here instead.
    fn taken_or_regenerated(&mut self, job: &PartJob, out: Result<PartOut, String>) -> (LivePart<K::Body>, Option<Arc<RenderMesh>>) {
        let taken = out.and_then(|o| {
            let mut live = LivePart::from_geometry(o.geometry);
            if let Some((blob, names)) = o.body {
                let t = Instant::now();
                let body = self.regen.kernel().import_native(&blob).map_err(|e| e.to_string());
                self.stats.take_over += t.elapsed();
                live.shape = Some(Shape::new(body?, names));
            }
            Ok((live, o.mesh.map(Arc::new)))
        });
        self.stats.regenerated += 1;
        match taken {
            Ok(x) => {
                self.regenerations += 1;
                x
            }
            Err(_) => {
                self.stats.regenerated -= 1;
                let before = self.regenerations;
                let x = self.regenerate(&job.doc, job.quality);
                self.stats.regenerated += self.regenerations - before;
                x
            }
        }
    }


    /// Resolves lightweight parts (see [`Self::resolve`]), several at once on worker threads
    /// (Mass Properties and Interference Detection of a lightweight assembly). False if some
    /// part's document is not known (shown from a cache whose file is missing).
    pub fn resolve_many(&mut self, hashes: &[String], quality: f64) -> bool {
        self.stats = BuildStats::default();
        let mut jobs: Vec<PartJob> = vec![];
        let mut known = true;
        for h in hashes {
            match self.parts.get(h) {
                Some(p) if p.live.is_some() => {}
                Some(Part { doc: Some(doc), .. }) => {
                    if !jobs.iter().any(|j| j.hash == *h) {
                        jobs.push(PartJob { hash: h.clone(), doc: doc.clone(), quality });
                    }
                }
                _ => known = false,
            }
        }
        let t = Instant::now();
        if jobs.len() > 1 && self.workers != 1 {
            let workers = worker_count(self.workers, jobs.len());
            let outs = run(self.regen.kernel(), self.regen.rules(), &jobs, workers);
            for (job, out) in jobs.iter().zip(outs) {
                let (live, mesh) = self.taken_or_regenerated(job, out);
                let p = self.parts.get_mut(&job.hash).expect("listed above");
                p.live = Some(live);
                if mesh.is_some() {
                    p.mesh = mesh;
                }
            }
            self.stats.workers = workers;
        } else if !jobs.is_empty() {
            for j in &jobs {
                self.resolve(&j.hash, quality);
            }
            self.stats.regenerated = jobs.len();
            self.stats.workers = 1;
        }
        self.stats.time = t.elapsed();
        known
    }
}
