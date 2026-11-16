//! Assemblies on the engine thread (OVERHAUL_PLAN G1a): open, build, drag, pick a mate
//! entity, save. The engine's [`oc_asm::Assembler`] owns the kernel and the regeneration
//! cache that parts use too, so a part opened from an assembly (or one of its components
//! edited in a part window) shares regenerations with it.
//!
//! Replies are JSON for `src/assembly/session.ts`. Display meshes go to the renderer
//! through the [`MeshStore`] by handle, one per distinct part: every occurrence of a part
//! draws the same handle with its own placement.
use crate::engine::{MeshStore, Reply};
use oc_asm::{Assembler, Built, Disk, InstanceState, MateStatus, Options};
use oc_doc::container::Container;
use oc_doc::{AssemblyDocument, Transform};
use oc_kernel_occt::OcctKernel;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

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
pub fn build(a: &mut Assembler<OcctKernel>, st: &mut AsmState, store: &MeshStore, doc: &AssemblyDocument, path: Option<&Path>, lightweight: bool, quality: f64) -> Reply {
    let t0 = Instant::now();
    let opts = Options { quality, lightweight, search: vec![] };
    let b = a.build(doc, &dir_of(path), &Disk, &opts);
    // One mesh handle per distinct part.
    let mut parts = serde_json::Map::new();
    let mut handles: BTreeMap<String, u32> = BTreeMap::new();
    for c in &b.components {
        let Some(h) = &c.part else { continue };
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
    Ok(json!({
        "moved": moved,
        "instances": b.instances.iter().map(|i| json!({ "id": i.id, "transform": transform(&i.transform) })).collect::<Vec<_>>(),
        "occurrences": b.occurrences.iter().map(|o| transform(&o.world)).collect::<Vec<_>>(),
        "dof": b.dof,
    }))
}

/// The mate entity for a face, edge or vertex of a part (regenerating it if lightweight).
pub fn entity(a: &mut Assembler<OcctKernel>, part: &str, kind: &str, index: u32, quality: f64) -> Reply {
    a.entity(part, kind, index, quality).map(|e| json!(e)).ok_or_else(|| format!("this {kind} cannot be used in a mate"))
}

/// Writes `doc` to `path` with the caches of the last build, the component paths made
/// relative to the new folder (absolute ones kept as the fallback). Returns the document
/// as saved.
pub fn save(a: &mut Assembler<OcctKernel>, st: &mut AsmState, doc: &AssemblyDocument, path: &Path, quality: f64) -> Reply {
    let mut doc = doc.clone();
    let dir = dir_of(Some(path));
    if let Some(b) = &st.built {
        for c in doc.components.iter_mut() {
            let Some(found) = b.components.iter().find(|o| o.id == c.id).and_then(|o| o.path.clone()) else { continue };
            let abs = oc_asm::locate::normalize(&found);
            c.path = oc_asm::locate::relative(&abs, &oc_asm::locate::normalize(&dir)).unwrap_or_else(|| abs.to_string_lossy().into_owned());
            c.abs_path = Some(abs.to_string_lossy().into_owned());
        }
    }
    // Build with the paths as saved, so the caches name the right parts.
    let b = a.build(&doc, &dir, &Disk, &Options { quality, lightweight: true, search: vec![] });
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
    std::fs::write(path, text).map(|_| Value::Null).map_err(|e| format!("{}: {e}", path.display()))
}

