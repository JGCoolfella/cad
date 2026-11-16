//! Assemblies on the engine thread (OVERHAUL_PLAN G1a): open, build, drag, pick a mate
//! entity, save. The engine's [`oc_asm::Assembler`] owns the kernel and the regeneration
//! cache that parts use too, so a part opened from an assembly (or one of its components
//! edited in a part window) shares regenerations with it.
//!
//! Replies are JSON for `src/assembly/session.ts`. Display meshes go to the renderer
//! through the [`MeshStore`] by handle, one per distinct part: every occurrence of a part
//! draws the same handle with its own placement.
use crate::engine::{MeshStore, Reply};
use oc_asm::locate::normalize;
use oc_asm::{Assembler, Built, Disk, InstanceState, MateStatus, Options, Store};
use oc_doc::container::Container;
use oc_doc::{AssemblyDocument, Transform};
use oc_kernel::Kernel;
use oc_kernel_occt::OcctKernel;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Component documents opened from the assembly and edited, not saved yet (G1b: Open Part
/// or Open Assembly, then back to the assembly): path to document text (a part's JSON, or
/// a sub-assembly's document JSON).
pub type Overrides = HashMap<String, String>;

/// Component files read from the edited documents first, then from the disk, as
/// SOLIDWORKS shows a part's unsaved edits in the assemblies that use it.
pub struct Overlay {
    files: HashMap<String, Vec<u8>>,
}

fn key(p: &Path) -> String {
    let k = normalize(p).to_string_lossy().into_owned();
    if cfg!(windows) {
        k.to_lowercase()
    } else {
        k
    }
}

impl Overlay {
    pub fn new(overrides: &Overrides) -> Overlay {
        let mut files = HashMap::new();
        for (path, text) in overrides {
            let p = Path::new(path);
            let bytes = if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ocasm")) {
                // A sub-assembly: its document in its container (keeping the caches on disk).
                let mut c = Disk.read(p).and_then(|b| Container::from_bytes(&b).ok()).unwrap_or_default();
                c.insert(oc_doc::container::DOCUMENT_ENTRY, text.as_bytes().to_vec());
                c.to_bytes()
            } else {
                text.as_bytes().to_vec()
            };
            files.insert(key(p), bytes);
        }
        Overlay { files }
    }
}

impl Store for Overlay {
    fn read(&self, path: &Path) -> Option<Vec<u8>> {
        self.files.get(&key(path)).cloned().or_else(|| Disk.read(path))
    }
    fn exists(&self, path: &Path) -> bool {
        self.files.contains_key(&key(path)) || Disk.exists(path)
    }
}

/// The open assembly's last build (for dragging).
#[derive(Default)]
pub struct AsmState {
    pub built: Option<Built>,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn transform(t: &Transform) -> Value {
    json!({ "position": t.position, "rotation": t.rotation })
}

fn dir_of(path: Option<&Path>) -> PathBuf {
    path.and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default()
}

/// Reads an `.ocasm` file: its document (migrated, as JSON) and, for lightweight builds,
/// its caches.
pub fn open(a: &mut Assembler<OcctKernel>, path: &Path) -> Reply {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let c = Container::from_bytes(&bytes).map_err(err)?;
    let loaded = oc_doc::load_assembly(c.document().map_err(err)?).map_err(err)?;
    a.set_cache(oc_asm::Cache::read(&c));
    Ok(json!({ "doc": loaded.doc, "sourceVersion": loaded.source_version, "path": path }))
}

/// Builds `doc` (its file is `path`, or none yet): components found and read, parts
/// regenerated or taken from the cache, mates solved.
#[allow(clippy::too_many_arguments)]
pub fn build(a: &mut Assembler<OcctKernel>, st: &mut AsmState, store: &MeshStore, doc: &AssemblyDocument, path: Option<&Path>, lightweight: bool, quality: f64, overrides: &Overrides) -> Reply {
    let t0 = Instant::now();
    let opts = Options { quality, lightweight, search: vec![] };
    let b = a.build(doc, &dir_of(path), &Overlay::new(overrides), &opts);
    // One mesh handle per distinct part: the parts of the components and, inside
    // sub-assemblies, of their occurrences.
    let mut parts = serde_json::Map::new();
    let mut handles: BTreeMap<String, u32> = BTreeMap::new();
    let hashes: Vec<&String> = b.components.iter().filter_map(|c| c.part.as_ref()).chain(b.occurrences.iter().map(|o| &o.part)).collect();
    for h in hashes {
        if handles.contains_key(h) {
            continue;
        }
        let Some(p) = a.parts.get(h) else { continue };
        let handle = p.mesh.as_ref().map(|m| store.add_arc(m.clone())).unwrap_or(0);
        handles.insert(h.clone(), handle);
        let errors: Vec<Value> = p.live.iter().flat_map(|l| l.errors.iter().map(|(id, e)| json!({ "id": id, "message": e }))).collect();
        // The mesh's box in part coordinates (to fit the view).
        let bbox = p.mesh.as_ref().map(|m| {
            let o = m.origin;
            let [lo, hi] = m.bounds;
            [[o[0] + lo[0] as f64, o[1] + lo[1] as f64, o[2] + lo[2] as f64], [o[0] + hi[0] as f64, o[1] + hi[1] as f64, o[2] + hi[2] as f64]]
        });
        parts.insert(h.clone(), json!({ "name": p.name, "handle": handle, "lightweight": p.live.is_none(), "errors": errors, "bbox": bbox }));
    }
    let reply = reply(&b, &handles, parts, t0);
    st.built = Some(b);
    Ok(reply)
}

fn reply(b: &Built, handles: &BTreeMap<String, u32>, parts: serde_json::Map<String, Value>, t0: Instant) -> Value {
    let state = |s: InstanceState| match s {
        InstanceState::Fixed => "fixed",
        InstanceState::Under => "under",
        InstanceState::Full => "full",
        InstanceState::Over => "over",
        InstanceState::Suppressed => "suppressed",
    };
    let mate = |s: &MateStatus| match s {
        MateStatus::Ok => ("ok", None),
        MateStatus::Suppressed => ("suppressed", None),
        MateStatus::Missing => ("missing", None),
        MateStatus::Redundant => ("redundant", Some("This mate is over defining the assembly (redundant).".to_string())),
        MateStatus::Conflict => ("conflict", Some("This mate cannot be solved with the mates before it.".to_string())),
        MateStatus::Invalid(e) => ("invalid", Some(e.clone())),
    };
    json!({
        "components": b.components.iter().map(|c| json!({
            "id": c.id, "path": c.path, "found": c.found.map(|f| format!("{f:?}")), "part": c.part,
            "error": c.error, "warning": c.warning,
        })).collect::<Vec<_>>(),
        "instances": b.instances.iter().map(|i| json!({ "id": i.id, "transform": transform(&i.transform), "state": state(i.state), "free": i.free })).collect::<Vec<_>>(),
        "occurrences": b.occurrences.iter().map(|o| json!({
            "path": o.path, "part": o.part, "mesh": handles.get(&o.part).copied().unwrap_or(0), "world": transform(&o.world), "hidden": o.hidden,
        })).collect::<Vec<_>>(),
        "parts": parts,
        "mates": b.mates.iter().map(|m| { let (s, msg) = mate(&m.status); json!({ "id": m.id, "status": s, "message": msg, "missing": m.missing }) }).collect::<Vec<_>>(),
        "dof": b.dof,
        "converged": b.converged,
        "newestError": b.newest_error,
        "heals": b.heals.iter().map(|h| json!({ "mate": h.mate, "side": h.side, "entity": h.entity })).collect::<Vec<_>>(),
        "timeMs": t0.elapsed().as_secs_f64() * 1e3,
    })
}

/// Drags an instance of the last build (see [`oc_asm::drag`]).
pub fn drag(st: &mut AsmState, instance: &str, grab: [f64; 3], target: [f64; 3], lever: f64) -> Reply {
    let b = st.built.as_mut().ok_or("no assembly is built")?;
    let moved = oc_asm::drag(b, instance, grab, target, lever);
    Ok(placements(b, moved))
}

/// Puts an instance of the last build at `to` and solves again (see [`oc_asm::place`]):
/// Move and Rotate Component by values, and rotating by dragging.
pub fn place(st: &mut AsmState, instance: &str, to: Transform) -> Reply {
    let b = st.built.as_mut().ok_or("no assembly is built")?;
    let moved = oc_asm::place(b, instance, to);
    Ok(placements(b, moved))
}

fn placements(b: &Built, moved: bool) -> Value {
    json!({
        "moved": moved,
        "instances": b.instances.iter().map(|i| json!({ "id": i.id, "transform": transform(&i.transform) })).collect::<Vec<_>>(),
        "occurrences": b.occurrences.iter().map(|o| transform(&o.world)).collect::<Vec<_>>(),
        "dof": b.dof,
    })
}

/// Mass properties of the last build's occurrences, for unit density, at their placements
/// (Evaluate > Mass Properties in an assembly). Suppressed components are left out (they
/// are not occurrences); hidden ones count, as in SOLIDWORKS (🔍). The UI applies each
/// part's material density and combines them.
pub fn mass(a: &mut Assembler<OcctKernel>, st: &AsmState, quality: f64) -> Reply {
    let b = st.built.as_ref().ok_or("no assembly is built")?;
    let mut per_part: HashMap<String, Value> = HashMap::new();
    let mut out = vec![];
    for o in &b.occurrences {
        if !per_part.contains_key(&o.part) {
            let v = if a.resolve(&o.part, quality) {
                let p = &a.parts[&o.part];
                let material = p.doc.as_ref().and_then(|d| d.material.clone());
                match p.live.as_ref().and_then(|l| l.shape.clone()) {
                    Some(shape) => {
                        let m = a.regen.kernel().mass_properties(&shape.body).map_err(|e| e.to_string())?;
                        json!({ "volume": m.volume, "area": m.area, "center": m.center, "material": material })
                    }
                    None => Value::Null,
                }
            } else {
                Value::Null
            };
            per_part.insert(o.part.clone(), v);
        }
        let v = &per_part[&o.part];
        if v.is_null() {
            continue;
        }
        let c: [f64; 3] = serde_json::from_value(v["center"].clone()).unwrap_or_default();
        out.push(json!({ "path": o.path, "part": o.part, "name": a.parts[&o.part].name, "volume": v["volume"], "area": v["area"], "center": o.world.apply(c), "material": v["material"] }));
    }
    Ok(json!({ "occurrences": out }))
}

/// Interference Detection (G1c) over the last build: the selected components (instance
/// paths; none = the whole assembly) and the PropertyManager's options. Each interference
/// carries its occurrences, volume and, when it has volume, a mesh of the common body for
/// the graphics area (drawn red at the identity placement).
pub fn interference(a: &mut Assembler<OcctKernel>, st: &AsmState, store: &MeshStore, opts: &oc_asm::InterferenceOptions, quality: f64) -> Reply {
    let t0 = Instant::now();
    let b = st.built.as_ref().ok_or("no assembly is built")?;
    let r = a.interferences(b, opts, quality)?;
    let mut out = vec![];
    for x in &r.interferences {
        let mesh = match &x.body {
            Some(body) => oc_mesh::build(a.regen.kernel(), body, &oc_mesh::MeshOptions { quality, max_lods: 1, threads: 0 }).map(|m| store.add(m)).unwrap_or(0),
            None => 0,
        };
        out.push(json!({
            "a": x.a, "b": x.b, "pathA": b.occurrences[x.a].path, "pathB": b.occurrences[x.b].path,
            "volume": x.volume, "coincident": x.coincident, "center": x.center, "mesh": mesh,
        }));
    }
    Ok(json!({ "interferences": out, "pairs": r.pairs, "candidates": r.candidates, "skipped": r.skipped, "timeMs": t0.elapsed().as_secs_f64() * 1e3 }))
}

/// One entity picked in the graphics area of an assembly: occurrence, kind, index.
#[derive(serde::Deserialize)]
pub struct Pick {
    pub occ: usize,
    pub kind: String,
    pub index: u32,
}

/// Measure in an assembly (G1c): each picked face, edge or vertex of an occurrence at its
/// placement (area, length, circle, XYZ, a plane's normal or a line's direction, all in
/// assembly coordinates), and the minimum distance between the first two, as the part
/// Measure tool reports them.
pub fn measure(a: &mut Assembler<OcctKernel>, st: &AsmState, picks: &[Pick], quality: f64) -> Reply {
    let b = st.built.as_ref().ok_or("no assembly is built")?;
    let mut bodies: HashMap<usize, oc_kernel_occt::Body> = HashMap::new();
    let mut items = vec![];
    for p in picks.iter().take(8) {
        b.occurrences.get(p.occ).ok_or("no such occurrence")?;
        if let std::collections::hash_map::Entry::Vacant(e) = bodies.entry(p.occ) {
            e.insert(a.placed_body(b, p.occ, quality).ok_or("this component has no body")?);
        }
        let topo = a.regen.kernel().topology(&bodies[&p.occ]).map_err(err)?;
        let item = match p.kind.as_str() {
            "face" => {
                let f = topo.faces.get(p.index as usize).ok_or("no such face")?;
                let dir = matches!(f.surface, oc_kernel::Surface::Plane { .. }).then_some(f.normal);
                json!({ "kind": "face", "area": f.area, "center": f.centroid, "dir": dir })
            }
            "edge" => {
                let e = topo.edges.get(p.index as usize).ok_or("no such edge")?;
                let (circle, dir) = match e.curve {
                    oc_kernel::Curve::Circle { center, radius, .. } => (Some(json!({ "center": center, "diameter": 2.0 * radius })), None),
                    oc_kernel::Curve::Line => (None, Some([e.end[0] - e.start[0], e.end[1] - e.start[1], e.end[2] - e.start[2]])),
                    _ => (None, None),
                };
                json!({ "kind": "edge", "length": e.length, "circle": circle, "dir": dir })
            }
            "vertex" => {
                let v = topo.vertices.get(p.index as usize).ok_or("no such vertex")?;
                json!({ "kind": "vertex", "point": v })
            }
            k => return Err(format!("cannot measure a {k}")),
        };
        items.push(item);
    }
    let mut distance = Value::Null;
    if let [p, q, ..] = picks {
        let target = |x: &Pick| -> oc_kernel::Target<'_, oc_kernel_occt::Body> {
            let body = &bodies[&x.occ];
            match x.kind.as_str() {
                "face" => oc_kernel::Target::Face(body, oc_kernel::FaceId(x.index)),
                "edge" => oc_kernel::Target::Edge(body, oc_kernel::EdgeId(x.index)),
                _ => oc_kernel::Target::Vertex(body, oc_kernel::VertexId(x.index)),
            }
        };
        let d = a.regen.kernel().distance(target(p), target(q)).map_err(err)?;
        distance = json!({ "distance": d.distance, "p1": d.a, "p2": d.b });
    }
    Ok(json!({ "items": items, "distance": distance }))
}

/// The mate entity for a face, edge or vertex of a part (regenerating it if lightweight).
pub fn entity(a: &mut Assembler<OcctKernel>, part: &str, kind: &str, index: u32, quality: f64) -> Reply {
    a.entity(part, kind, index, quality).map(|e| json!(e)).ok_or_else(|| format!("this {kind} cannot be used in a mate"))
}

/// Writes `doc` to `path` with fresh caches, the component paths made relative to the new
/// folder (absolute ones kept as the fallback). `from` is where the document was (its
/// relative paths start there; none for a new assembly). Returns the document as saved.
///
/// The components are found from `from` first (G1b: not from the last build, which may be
/// another document's, as when a sub-assembly edited from its parent is saved with it).
#[allow(clippy::too_many_arguments)]
pub fn save(a: &mut Assembler<OcctKernel>, st: &mut AsmState, doc: &AssemblyDocument, path: &Path, from: Option<&Path>, quality: f64, overrides: &Overrides) -> Reply {
    let mut doc = doc.clone();
    let dir = dir_of(Some(path));
    let store = Overlay::new(overrides);
    let opts = Options { quality, lightweight: true, search: vec![] };
    let located = a.build(&doc, &dir_of(from), &store, &opts);
    for c in doc.components.iter_mut() {
        let Some(found) = located.components.iter().find(|o| o.id == c.id).and_then(|o| o.path.clone()) else { continue };
        let abs = oc_asm::locate::normalize(&found);
        c.path = oc_asm::locate::relative(&abs, &oc_asm::locate::normalize(&dir)).unwrap_or_else(|| abs.to_string_lossy().into_owned());
        c.abs_path = Some(abs.to_string_lossy().into_owned());
    }
    // Build with the paths as saved, so the caches name the right parts.
    let b = a.build(&doc, &dir, &store, &opts);
    let c = a.container(&doc, &b, quality)?;
    let bytes = c.to_bytes();
    let tmp = path.with_extension("ocasm.tmp");
    std::fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))?;
    st.built = Some(b);
    Ok(json!({ "doc": doc, "bytes": bytes.len() }))
}

/// Reads a text file (parts opened from the native file dialog or an assembly).
pub fn read_text(path: &Path) -> Reply {
    std::fs::read_to_string(path).map(Value::String).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes a text file (parts saved to a path).
pub fn write_text(path: &Path, text: &str) -> Reply {
    // The Save As dialog only gives existing folders; scripts may name a new one.
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, text).map(|_| Value::Null).map_err(|e| format!("{}: {e}", path.display()))
}

