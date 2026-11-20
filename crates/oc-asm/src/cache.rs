//! The caches an `.ocasm` container carries beside `document.json`: one display mesh per
//! distinct part, named by the part's content hash, and a manifest saying which component
//! had which hash when the assembly was saved. They are only ever caches: deleting them
//! makes the next open regenerate the parts.
//!
//! ```text
//! document.json
//! cache/manifest.json      {"cache": 1, "quality": 3, "components": {"c1": "<hash>"}, "parts": {"<hash>": {"name": "pin"}}}
//! cache/mesh/<hash>.ocm    oc_mesh::RenderMesh::to_bytes
//! ```
//!
//! No B-rep is cached yet: the kernel trait has a native body transfer since G3a
//! (`export_native`), but its bytes are not canonical (OCCT writes shared locations in
//! memory order), so B-rep caches would break byte-identical saves; they are G3c's. A part
//! whose hash matches opens from its cached mesh without being regenerated (lightweight);
//! picking its geometry for a new mate resolves it.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use oc_doc::container::{content_hash, Container};
use oc_doc::{AssemblyDocument, PartDocument};
use oc_mesh::RenderMesh;
use serde::{Deserialize, Serialize};

/// Changes when anything that shapes cached meshes changes (engine, mesher, layout).
pub const CACHE_VERSION: u32 = 1;
pub const MANIFEST: &str = "cache/manifest.json";

/// Content hash of a part as displayed: its canonical document, the image quality and
/// [`CACHE_VERSION`]. Equal hashes mean equal geometry and equal meshes, so instances of
/// identical parts (even from different files) share one regeneration and one mesh.
pub fn part_hash(doc: &PartDocument, quality: f64) -> String {
    let canon = oc_doc::canonical(doc).unwrap_or_default();
    content_hash(&[canon.as_bytes(), format!("q{quality}").as_bytes(), &CACHE_VERSION.to_le_bytes()])
}

pub fn mesh_entry(hash: &str) -> String {
    format!("cache/mesh/{hash}.ocm")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartInfo {
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub cache: u32,
    pub quality: f64,
    /// Component id → part hash.
    pub components: BTreeMap<String, String>,
    pub parts: BTreeMap<String, PartInfo>,
}

/// Caches read from a container.
#[derive(Clone, Default)]
pub struct Cache {
    pub manifest: Manifest,
    pub meshes: HashMap<String, Arc<RenderMesh>>,
}

impl Cache {
    /// Reads the caches of a container. Caches written by another cache version, or damaged
    /// ones, are ignored (they are only caches).
    pub fn read(c: &Container) -> Cache {
        let Some(m) = c.get(MANIFEST).and_then(|b| serde_json::from_slice::<Manifest>(b).ok()) else { return Cache::default() };
        if m.cache != CACHE_VERSION {
            return Cache::default();
        }
        let meshes = m.parts.keys().filter_map(|h| c.get(&mesh_entry(h)).and_then(|b| RenderMesh::from_bytes(b).ok()).map(|mesh| (h.clone(), Arc::new(mesh)))).collect();
        Cache { manifest: m, meshes }
    }
}

/// The container for `doc` with the meshes of `parts` (hash → name and mesh) as caches.
pub fn write(doc: &AssemblyDocument, quality: f64, components: &BTreeMap<String, String>, parts: &BTreeMap<String, (String, Arc<RenderMesh>)>) -> Result<Container, String> {
    let mut c = Container::new();
    c.insert(oc_doc::container::DOCUMENT_ENTRY, oc_doc::save_assembly(doc).map_err(|e| e.to_string())?.into_bytes());
    let manifest = Manifest {
        cache: CACHE_VERSION,
        quality,
        components: components.clone(),
        parts: parts.iter().map(|(h, (name, _))| (h.clone(), PartInfo { name: name.clone() })).collect(),
    };
    c.insert(MANIFEST, serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?);
    for (h, (_, mesh)) in parts {
        c.insert(mesh_entry(h), mesh.to_bytes());
    }
    Ok(c)
}
