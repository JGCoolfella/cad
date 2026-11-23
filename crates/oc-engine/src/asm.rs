//! Assemblies on the engine thread (OVERHAUL_PLAN G1a): open, build, drag, pick a mate
//! entity, save. The engine's [`oc_asm::Assembler`] owns the kernel and the regeneration
//! cache that parts use too, so a part opened from an assembly (or one of its components
//! edited in a part window) shares regenerations with it.
//!
//! Replies are JSON for `src/assembly/session.ts`, except build replies: binary (G3d,
//! [`encode_build`]), carrying only what changed since the build the webview holds. Display
//! meshes go to the renderer through the [`MeshStore`] by handle, one per distinct part:
//! every occurrence of a part draws the same handle with its own placement.
use crate::engine::{BytesReply, MeshStore, Reply};
use crate::pack::Packer;
use oc_asm::locate::normalize;
use oc_asm::{Assembler, Built, Disk, InstanceState, MateStatus, Options, Store};
use oc_doc::container::Container;
use oc_doc::{AssemblyDocument, Transform};
use oc_kernel::Kernel;
use oc_kernel_occt::OcctKernel;
use serde_json::{json, Value};
use std::collections::HashMap;
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

/// The open assembly's last build (for dragging), and what the webview was told of it.
#[derive(Default)]
pub struct AsmState {
    pub built: Option<Built>,
    /// What the last kept build reply told the webview (G3d): the next reply sends only what
    /// changed since, when the webview names this generation as its base.
    pub sent: Option<Sent>,
    /// Generations handed out (each kept build reply gets the next).
    gens: u64,
}

/// A placement as sent: position, then the rotation quaternion.
fn placement(t: &Transform) -> [f64; 7] {
    [t.position[0], t.position[1], t.position[2], t.rotation[0], t.rotation[1], t.rotation[2], t.rotation[3]]
}

/// Placements compare by their bits: the webview must hold exactly what the engine has.
fn bits(t: &Transform) -> [u64; 7] {
    placement(t).map(f64::to_bits)
}

/// An instance's state as sent (`STATES` in `src/assembly/buildReply.ts`, in this order).
fn state_code(s: InstanceState) -> u32 {
    match s {
        InstanceState::Fixed => 0,
        InstanceState::Under => 1,
        InstanceState::Full => 2,
        InstanceState::Over => 3,
        InstanceState::Suppressed => 4,
    }
}

/// What a build reply told the webview (G3d), entry by entry, so the next reply can send only
/// the entries that differ. Components and parts are sent whole every time: there is one per
/// distinct file, and every build hands out new mesh handles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sent {
    pub gen: u64,
    inst_ids: Vec<oc_doc::Id>,
    /// Placement bits and `state | free << 8`.
    inst: Vec<([u64; 7], u32)>,
    occ_paths: Vec<Vec<oc_doc::Id>>,
    /// Part hash, world placement bits, hidden.
    occ: Vec<(String, [u64; 7], bool)>,
    mate_ids: Vec<oc_doc::Id>,
    mates: Vec<Value>,
}

impl AsmState {
    /// After a drag or Move Component by values (replies carrying every placement, which the
    /// webview writes into the build it holds): the record follows, so the next build reply
    /// is still a difference against what the webview shows.
    fn mirror_placements(&mut self) {
        let (Some(b), Some(s)) = (&self.built, &mut self.sent) else { return };
        let same = s.inst_ids.len() == b.instances.len()
            && s.inst_ids.iter().zip(&b.instances).all(|(id, i)| *id == i.id)
            && s.occ_paths.len() == b.occurrences.len()
            && s.occ_paths.iter().zip(&b.occurrences).all(|(p, o)| *p == o.path);
        if !same {
            // Not the build the webview holds: the next reply is whole.
            self.sent = None;
            return;
        }
        for (x, i) in s.inst.iter_mut().zip(&b.instances) {
            x.0 = bits(&i.transform);
        }
        for (x, o) in s.occ.iter_mut().zip(&b.occurrences) {
            x.1 = bits(&o.world);
        }
    }
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
    // Caches of another engine build are ignored (G3d).
    let cache = Some(oc_asm::Cache::read(&c)).filter(|x| a.accepts(x)).unwrap_or_default();
    // Parts without a cached mesh will be regenerated by the first build: start the geometry
    // hosts now (G3b), while the document goes to the webview and comes back.
    let cached = |id: &oc_doc::Id| cache.manifest.components.get(id.as_str()).is_some_and(|h| cache.meshes.contains_key(h));
    if loaded.doc.components.iter().filter(|c| c.kind == oc_doc::ComponentKind::Part && !cached(&c.id)).nth(1).is_some() {
        if let Some(r) = a.runner.as_mut() {
            r.warm();
        }
    }
    a.set_cache(cache);
    Ok(json!({ "doc": loaded.doc, "sourceVersion": loaded.source_version, "path": path }))
}

/// Builds `doc` (its file is `path`, or none yet): components found and read, parts
/// regenerated or taken from the cache, mates solved. `keep`: the build becomes the one
/// drags, placing and evaluation use, and the base of the next reply (false: a trial solve,
/// the SmartMates preview). `base`: the generation of the build the webview holds (G3d);
/// when it is the last one sent, the reply carries only what changed. `extra`: more header
/// fields (the geometry hosts' counts).
#[allow(clippy::too_many_arguments)]
pub fn build(a: &mut Assembler<OcctKernel>, st: &mut AsmState, store: &MeshStore, doc: &AssemblyDocument, path: Option<&Path>, lightweight: bool, quality: f64, overrides: &Overrides, keep: bool, base: Option<u64>, extra: Option<Value>) -> BytesReply {
    let t0 = Instant::now();
    let opts = Options { quality, lightweight, search: vec![] };
    let b = a.build(doc, &dir_of(path), &Overlay::new(overrides), &opts);
    // One mesh handle per distinct part: the parts of the components and, inside
    // sub-assemblies, of their occurrences.
    let mut parts = serde_json::Map::new();
    let hashes: Vec<&String> = b.components.iter().filter_map(|c| c.part.as_ref()).chain(b.occurrences.iter().map(|o| &o.part)).collect();
    for h in hashes {
        if parts.contains_key(h) {
            continue;
        }
        let Some(p) = a.parts.get(h) else { continue };
        let handle = p.mesh.as_ref().map(|m| store.add_arc(m.clone())).unwrap_or(0);
        let errors: Vec<Value> = p.live.iter().flat_map(|l| l.errors.iter().map(|(id, e)| json!({ "id": id, "message": e }))).collect();
        // The mesh's box in part coordinates (to fit the view).
        let bbox = p.mesh.as_ref().map(|m| {
            let o = m.origin;
            let [lo, hi] = m.bounds;
            [[o[0] + lo[0] as f64, o[1] + lo[1] as f64, o[2] + lo[2] as f64], [o[0] + hi[0] as f64, o[1] + hi[1] as f64, o[2] + hi[2] as f64]]
        });
        let external = p.doc.as_ref().is_some_and(|d| has_external(d));
        parts.insert(h.clone(), json!({ "name": p.name, "handle": handle, "lightweight": p.live.is_none(), "errors": errors, "bbox": bbox, "external": external }));
    }
    let mut head = json!({
        // What was regenerated, and on how many threads (G3a; native scripts).
        "regenerated": a.stats.regenerated,
        "workers": a.stats.workers,
        "regenMs": a.stats.time.as_secs_f64() * 1e3,
        // G3c: parts read from their B-rep caches, bodies evicted to keep within the budget.
        "read": a.stats.read,
        "evicted": a.stats.evicted,
        "timeMs": t0.elapsed().as_secs_f64() * 1e3,
    });
    if let Some(Value::Object(x)) = extra {
        head.as_object_mut().expect("object").extend(x);
    }
    let prev = st.sent.as_ref().filter(|s| base.is_some_and(|g| g != 0 && g == s.gen));
    let gen = if keep {
        st.gens += 1;
        st.gens
    } else {
        0
    };
    let (bytes, sent) = encode_build(&b, parts, head, prev, gen);
    if keep {
        st.sent = Some(sent);
        st.built = Some(b);
    }
    Ok(bytes)
}

fn mate_json(m: &oc_asm::MateOut) -> Value {
    let (s, msg) = match &m.status {
        MateStatus::Ok => ("ok", None),
        MateStatus::Suppressed => ("suppressed", None),
        MateStatus::Missing => ("missing", None),
        MateStatus::Redundant => ("redundant", Some("This mate is over defining the assembly (redundant).".to_string())),
        MateStatus::Conflict => ("conflict", Some("This mate cannot be solved with the mates before it.".to_string())),
        MateStatus::Invalid(e) => ("invalid", Some(e.clone())),
    };
    json!({ "id": m.id, "status": s, "message": msg, "missing": m.missing })
}

/// The binary build reply (`pack.rs` layout; `src/assembly/buildReply.ts` decodes it) and the
/// record of what it tells the webview. `head` holds the reply's other fields. With `prev`
/// (the record of the build the webview holds), a list whose ids are unchanged (instances,
/// occurrences, mates) carries only its changed entries, by index; otherwise it is whole.
///
/// ```text
/// gen, base            this reply's generation (0: a trial solve, never a base); the one it
///                      changes (0: none, every list whole)
/// components, parts    whole (an occurrence's mesh is its part's handle)
/// mates | mateChanges  whole, or [index, mate] pairs
/// instances            {ids} whole or {at: u32} indices; tr: f64 × 7 each (position,
///                      rotation w x y z); st: u32 each, state | free << 8
/// occurrences          {paths} whole or {at: u32}; part: u32 each, an index into `hashes`;
///                      w: f64 × 7 each; hidden: u32 each
/// hashes               the part hashes the occurrences above name
/// ```
pub fn encode_build(b: &Built, parts: serde_json::Map<String, Value>, mut head: Value, prev: Option<&Sent>, gen: u64) -> (Vec<u8>, Sent) {
    let mut p = Packer::default();
    let h = head.as_object_mut().expect("object");
    h.insert("gen".into(), json!(gen));
    h.insert("base".into(), json!(prev.map_or(0, |s| s.gen)));

    // Instances.
    let inst_ids: Vec<oc_doc::Id> = b.instances.iter().map(|i| i.id.clone()).collect();
    let inst: Vec<([u64; 7], u32)> = b.instances.iter().map(|i| (bits(&i.transform), state_code(i.state) | (i.free.min(0xff_ffff) as u32) << 8)).collect();
    let same = prev.filter(|s| s.inst_ids == inst_ids);
    let at: Vec<usize> = match same {
        Some(s) => (0..inst.len()).filter(|&k| s.inst[k] != inst[k]).collect(),
        None => (0..inst.len()).collect(),
    };
    let tr: Vec<f64> = at.iter().flat_map(|&k| placement(&b.instances[k].transform)).collect();
    let codes: Vec<u32> = at.iter().map(|&k| inst[k].1).collect();
    let mut i = json!({ "tr": p.f64(&tr), "st": p.u32(&codes) });
    if same.is_some() {
        i["at"] = p.u32(&at.iter().map(|&k| k as u32).collect::<Vec<_>>());
    } else {
        i["ids"] = json!(inst_ids);
    }
    h.insert("instances".into(), i);

    // Occurrences.
    let occ_paths: Vec<Vec<oc_doc::Id>> = b.occurrences.iter().map(|o| o.path.clone()).collect();
    let occ: Vec<(String, [u64; 7], bool)> = b.occurrences.iter().map(|o| (o.part.clone(), bits(&o.world), o.hidden)).collect();
    let same = prev.filter(|s| s.occ_paths == occ_paths);
    let at: Vec<usize> = match same {
        Some(s) => (0..occ.len()).filter(|&k| s.occ[k] != occ[k]).collect(),
        None => (0..occ.len()).collect(),
    };
    let mut hashes: Vec<&str> = vec![];
    let mut index: HashMap<&str, u32> = HashMap::new();
    let part: Vec<u32> = at
        .iter()
        .map(|&k| {
            let hash = b.occurrences[k].part.as_str();
            *index.entry(hash).or_insert_with(|| {
                hashes.push(hash);
                hashes.len() as u32 - 1
            })
        })
        .collect();
    let w: Vec<f64> = at.iter().flat_map(|&k| placement(&b.occurrences[k].world)).collect();
    let hidden: Vec<u32> = at.iter().map(|&k| u32::from(b.occurrences[k].hidden)).collect();
    let mut o = json!({ "part": p.u32(&part), "w": p.f64(&w), "hidden": p.u32(&hidden) });
    if same.is_some() {
        o["at"] = p.u32(&at.iter().map(|&k| k as u32).collect::<Vec<_>>());
    } else {
        o["paths"] = json!(occ_paths);
    }
    h.insert("occurrences".into(), o);
    h.insert("hashes".into(), json!(hashes));

    // Mates.
    let mate_ids: Vec<oc_doc::Id> = b.mates.iter().map(|m| m.id.clone()).collect();
    let mates: Vec<Value> = b.mates.iter().map(mate_json).collect();
    match prev.filter(|s| s.mate_ids == mate_ids) {
        Some(s) => {
            let changes: Vec<Value> = mates.iter().enumerate().filter(|(k, m)| s.mates[*k] != **m).map(|(k, m)| json!([k, m])).collect();
            h.insert("mateChanges".into(), json!(changes));
        }
        None => {
            h.insert("mates".into(), json!(mates));
        }
    }

    let components: Vec<Value> = b
        .components
        .iter()
        .map(|c| json!({ "id": c.id, "path": c.path, "found": c.found.map(|f| format!("{f:?}")), "part": c.part, "error": c.error, "warning": c.warning, "configs": c.configs }))
        .collect();
    h.insert("components".into(), json!(components));
    h.insert("parts".into(), Value::Object(parts));
    h.insert("dof".into(), json!(b.dof));
    h.insert("converged".into(), json!(b.converged));
    h.insert("newestError".into(), json!(b.newest_error));
    h.insert("heals".into(), json!(b.heals.iter().map(|x| json!({ "mate": x.mate, "side": x.side, "entity": x.entity })).collect::<Vec<_>>()));
    let bytes = p.finish(&head);
    (bytes, Sent { gen, inst_ids, inst, occ_paths, occ, mate_ids, mates })
}

/// Drags an instance of the last build (see [`oc_asm::drag`]).
pub fn drag(st: &mut AsmState, instance: &str, grab: [f64; 3], target: [f64; 3], lever: f64) -> Reply {
    let b = st.built.as_mut().ok_or("no assembly is built")?;
    let moved = oc_asm::drag(b, instance, grab, target, lever);
    let r = placements(b, moved);
    st.mirror_placements();
    Ok(r)
}

/// Puts an instance of the last build at `to` and solves again (see [`oc_asm::place`]):
/// Move and Rotate Component by values, and rotating by dragging.
pub fn place(st: &mut AsmState, instance: &str, to: Transform) -> Reply {
    let b = st.built.as_mut().ok_or("no assembly is built")?;
    let moved = oc_asm::place(b, instance, to);
    let r = placements(b, moved);
    st.mirror_placements();
    Ok(r)
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
    // Lightweight parts resolved at once (G3a).
    let hashes: Vec<String> = b.occurrences.iter().map(|o| o.part.clone()).collect();
    a.resolve_many(&hashes, quality);
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
    // What resolving them took (G3c; native scripts): parts read from B-rep caches or regenerated.
    Ok(json!({ "occurrences": out, "read": a.stats.read, "regenerated": a.stats.regenerated }))
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
        // The placed body may number its entities differently from the part.
        let at = a.placed_entity(b, p.occ, &bodies[&p.occ], &p.kind, p.index, quality).ok_or("the entity was not found on the placed component")?;
        let p = &Pick { occ: p.occ, kind: p.kind.clone(), index: at };
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
        let index = |a: &mut Assembler<OcctKernel>, x: &Pick| a.placed_entity(b, x.occ, &bodies[&x.occ], &x.kind, x.index, quality).ok_or("the entity was not found on the placed component");
        let (ip, iq) = (index(a, p)?, index(a, q)?);
        let target = |x: &Pick, i: u32| -> oc_kernel::Target<'_, oc_kernel_occt::Body> {
            let body = &bodies[&x.occ];
            match x.kind.as_str() {
                "face" => oc_kernel::Target::Face(body, oc_kernel::FaceId(i)),
                "edge" => oc_kernel::Target::Edge(body, oc_kernel::EdgeId(i)),
                _ => oc_kernel::Target::Vertex(body, oc_kernel::VertexId(i)),
            }
        };
        let d = a.regen.kernel().distance(target(p, ip), target(q, iq)).map_err(err)?;
        distance = json!({ "distance": d.distance, "p1": d.a, "p2": d.b });
    }
    Ok(json!({ "items": items, "distance": distance }))
}

/// An entity of an occurrence of the last build, found by persistent name as mate
/// references are (G1d in-context references): the entity as it is now and the
/// occurrence's placement, for the assembly to update the parts edited in its context.
pub fn reference(a: &mut Assembler<OcctKernel>, st: &AsmState, path: &[oc_doc::Id], e: &oc_doc::MateEntity, quality: f64) -> Reply {
    let b = st.built.as_ref().ok_or("no assembly is built")?;
    let (entity, world) = a.occurrence_entity(b, path, e, quality).ok_or("the referenced entity is missing")?;
    Ok(json!({ "entity": entity, "world": transform(&world) }))
}

/// A part has in-context references (G1d): a sketch on another component's face or
/// geometry converted from another component's edge.
fn has_external(doc: &oc_doc::PartDocument) -> bool {
    doc.features.iter().any(|f| match &f.kind {
        oc_doc::FeatureKind::Sketch(s) => matches!(s.plane, oc_doc::PlaneRef::External { .. }) || s.sketch.refs.iter().flatten().any(|(_, r)| matches!(r, oc_doc::SketchRef::External { .. })),
        _ => false,
    })
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


#[cfg(test)]
mod tests {
    use super::*;
    use oc_asm::{InstanceOut, MateOut, Occurrence};

    fn t(x: f64) -> Transform {
        Transform { position: [x, 0.0, 0.0], rotation: [1.0, 0.0, 0.0, 0.0] }
    }

    /// `n` free instances, one occurrence each, and a mate between the first two.
    fn built(n: usize) -> Built {
        let mut b = Built::default();
        for k in 0..n {
            let id = oc_doc::Id::from(format!("i{k}"));
            b.instances.push(InstanceOut { id: id.clone(), transform: t(k as f64), state: if k == 0 { InstanceState::Fixed } else { InstanceState::Under }, free: if k == 0 { 0 } else { 6 } });
            b.occurrences.push(Occurrence { path: vec![id], part: format!("h{}", k % 3), world: t(k as f64), local: t(0.0), hidden: false });
        }
        b.mates.push(MateOut { id: "m1".into(), status: MateStatus::Ok, missing: vec![] });
        b
    }

    fn head(bytes: &[u8]) -> Value {
        let n = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        serde_json::from_slice(&bytes[8..8 + n]).unwrap()
    }

    fn count(v: &Value) -> u64 {
        v["n"].as_u64().unwrap()
    }

    #[test]
    fn a_first_reply_is_whole() {
        let (bytes, sent) = encode_build(&built(5), Default::default(), json!({}), None, 1);
        let h = head(&bytes);
        assert_eq!((h["gen"].as_u64(), h["base"].as_u64()), (Some(1), Some(0)));
        assert_eq!(h["instances"]["ids"].as_array().unwrap().len(), 5);
        assert_eq!(count(&h["instances"]["tr"]), 35);
        assert_eq!(count(&h["occurrences"]["w"]), 35);
        assert_eq!(h["occurrences"]["paths"].as_array().unwrap().len(), 5);
        assert_eq!(h["hashes"], json!(["h0", "h1", "h2"]));
        assert_eq!(h["mates"].as_array().unwrap().len(), 1);
        assert_eq!(sent.gen, 1);
    }

    #[test]
    fn a_reply_against_the_build_held_carries_only_changes() {
        let b = built(10_000);
        let (_, sent) = encode_build(&b, Default::default(), json!({}), None, 1);
        // Nothing changed: no entries.
        let (bytes, again) = encode_build(&b, Default::default(), json!({}), Some(&sent), 2);
        let h = head(&bytes);
        assert_eq!(h["base"].as_u64(), Some(1));
        assert!(h["instances"].get("ids").is_none() && h["occurrences"].get("paths").is_none());
        assert_eq!((count(&h["instances"]["at"]), count(&h["occurrences"]["at"])), (0, 0));
        assert_eq!(h["mateChanges"], json!([]));
        assert!(bytes.len() < 4096, "{} bytes", bytes.len());
        assert_eq!(again.inst, sent.inst);
        // One instance moved, another's state changed, an occurrence hidden, the mate broke.
        let mut c = b.clone();
        c.instances[7].transform = t(70.5);
        c.occurrences[7].world = t(70.5);
        c.instances[9].state = InstanceState::Full;
        c.instances[9].free = 0;
        c.occurrences[3].hidden = true;
        c.mates[0].status = MateStatus::Conflict;
        let (bytes, _) = encode_build(&c, Default::default(), json!({}), Some(&sent), 2);
        let h = head(&bytes);
        let base = 8 + u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let u32s = |v: &Value| -> Vec<u32> {
            let at = base + v["$b"].as_u64().unwrap() as usize;
            (0..count(v) as usize).map(|k| u32::from_le_bytes(bytes[at + 4 * k..at + 4 * k + 4].try_into().unwrap())).collect()
        };
        assert_eq!(u32s(&h["instances"]["at"]), [7, 9]);
        assert_eq!(u32s(&h["instances"]["st"]), [1 | 6 << 8, 2]);
        assert_eq!(u32s(&h["occurrences"]["at"]), [3, 7]);
        assert_eq!(u32s(&h["occurrences"]["hidden"]), [1, 0]);
        assert_eq!(h["hashes"], json!(["h0", "h1"]));
        assert_eq!(h["mateChanges"][0][0], 0);
        assert_eq!(h["mateChanges"][0][1]["status"], "conflict");
    }

    #[test]
    fn a_list_whose_ids_changed_is_whole() {
        let b = built(4);
        let (_, sent) = encode_build(&b, Default::default(), json!({}), None, 1);
        let mut c = built(5);
        c.mates.push(MateOut { id: "m2".into(), status: MateStatus::Ok, missing: vec![] });
        let (bytes, _) = encode_build(&c, Default::default(), json!({}), Some(&sent), 2);
        let h = head(&bytes);
        assert_eq!(h["instances"]["ids"].as_array().unwrap().len(), 5);
        assert_eq!(h["occurrences"]["paths"].as_array().unwrap().len(), 5);
        assert_eq!(h["mates"].as_array().unwrap().len(), 2);
    }

    /// A drag's reply carries every placement and the webview writes them into the build it
    /// holds; the record follows, so the next build reply sends only what the solve changed
    /// from the dragged placements.
    #[test]
    fn drags_keep_the_record_in_step() {
        let mut st = AsmState::default();
        let b = built(3);
        let (_, sent) = encode_build(&b, Default::default(), json!({}), None, 1);
        st.sent = Some(sent);
        let mut dragged = b.clone();
        dragged.instances[1].transform = t(5.0);
        dragged.occurrences[1].world = t(5.0);
        st.built = Some(dragged.clone());
        st.mirror_placements();
        let (bytes, _) = encode_build(&dragged, Default::default(), json!({}), st.sent.as_ref(), 2);
        assert_eq!(count(&head(&bytes)["instances"]["at"]), 0, "the webview has the dragged placement already");
        // Another build than the one the webview holds: the record is dropped (next reply whole).
        st.built = Some(built(4));
        st.mirror_placements();
        assert!(st.sent.is_none());
    }
}
