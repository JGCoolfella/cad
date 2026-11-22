//! The caches an `.ocasm` container carries beside `document.json`: one display mesh per
//! distinct part, named by the part's content hash, and a manifest saying which component
//! had which hash when the assembly was saved. They are only ever caches: deleting them
//! makes the next open regenerate the parts.
//!
//! ```text
//! document.json
//! cache/manifest.json      {"cache": 1, "engine": {"app": "0.8.0", "kernel": "OCCT 8.0.1"}, "quality": 3,
//!                           "components": {"c1": "<hash>"}, "parts": {"<hash>": {"name": "pin"}}}
//! cache/mesh/<hash>.ocm    oc_mesh::RenderMesh::to_bytes
//! cache/brep/<hash>.ocb    the body (G3c): encode_brep
//! ```
//!
//! A part whose hash matches opens from its cached mesh without being regenerated
//! (lightweight). Picking its geometry for a new mate resolves it: since G3c from its B-rep
//! entry (the kernel's native blob with the faces' persistent names, the reference geometry
//! and the feature errors), regenerating only when there is none or it does not read; since
//! G3d a resolved build of a freshly opened file reads them too.
//!
//! The caches are trusted only from the engine build that wrote them (G3d, owner decision 3
//! on G3c): the manifest names it ([`Engine`]: the app's version and the kernel's name and
//! version), and caches of another build, or of a file saved before they named it, are
//! ignored ([`crate::Assembler::set_cache`]), so a new release or kernel never shows or reads
//! what an older one made. [`CACHE_VERSION`] still changes with the layout. The native blob is not canonical (OCCT writes shared locations in memory
//! order), so two saves of the same assembly may differ in these entries (owner decision
//! 2026-11-20); everything else is written deterministically.
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use oc_doc::container::{content_hash, Container};
use oc_doc::{AssemblyDocument, MateGeom, PartDocument};
use oc_mesh::RenderMesh;
use oc_naming::{BodyNames, FaceName};
use serde::{Deserialize, Serialize};

use crate::parallel::RefGeometry;

/// Changes with the layout of the caches (engine changes are covered by [`Engine`]).
pub const CACHE_VERSION: u32 = 1;
pub const MANIFEST: &str = "cache/manifest.json";

/// The app's version (`crates/oc-app/Cargo.toml`, read by this crate's build script): the
/// engine build the caches are keyed to, with the kernel's identity.
pub const APP_VERSION: &str = env!("OC_APP_VERSION");

/// The engine build that wrote a container's caches (G3d).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Engine {
    /// The app's version ([`APP_VERSION`]).
    pub app: String,
    /// The kernel's name and version ([`oc_kernel::Kernel::identity`]).
    pub kernel: String,
}

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

pub fn brep_entry(hash: &str) -> String {
    format!("cache/brep/{hash}.ocb")
}

const BREP_MAGIC: &[u8; 4] = b"OCBC";
/// Changes with the layout below (the native blob carries its own format and version).
const BREP_FORMAT: u32 = 1;

#[derive(Serialize, Deserialize)]
struct BrepHead {
    names: Vec<Vec<String>>,
    planes: BTreeMap<String, MateGeom>,
    axes: BTreeMap<String, MateGeom>,
    points: BTreeMap<String, MateGeom>,
    errors: Vec<(String, String)>,
}

/// A part's B-rep entry: `OCBC`, format, the length of a JSON head (the faces' persistent
/// names in the kernel's face order, the reference geometry, the feature errors), the head,
/// then the kernel's native blob (`Kernel::export_native`).
pub fn encode_brep(blob: &[u8], names: &BodyNames, g: &RefGeometry) -> Vec<u8> {
    let head = BrepHead {
        names: names.faces.iter().map(FaceName::to_stored).collect(),
        planes: g.planes.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        axes: g.axes.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        points: g.points.iter().map(|(k, v)| (k.clone(), *v)).collect(),
        errors: g.errors.clone(),
    };
    let json = serde_json::to_vec(&head).expect("plain data");
    let mut out = BREP_MAGIC.to_vec();
    out.extend_from_slice(&BREP_FORMAT.to_le_bytes());
    out.extend_from_slice(&(json.len() as u64).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(blob);
    out
}

/// The native blob, names and reference geometry of a B-rep entry.
pub fn decode_brep(b: &[u8]) -> Result<(&[u8], BodyNames, RefGeometry), String> {
    if b.len() < 16 || &b[..4] != BREP_MAGIC || b[4..8] != BREP_FORMAT.to_le_bytes() {
        return Err("not a B-rep cache entry of this version".into());
    }
    let n = usize::try_from(u64::from_le_bytes(b[8..16].try_into().expect("8 bytes"))).map_err(|e| e.to_string())?;
    let json = b.get(16..16 + n).ok_or("the B-rep cache entry is truncated")?;
    let head: BrepHead = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    let names = BodyNames { faces: head.names.iter().map(|t| FaceName::from_stored(t)).collect() };
    let g = RefGeometry { planes: head.planes.into_iter().collect(), axes: head.axes.into_iter().collect(), points: head.points.into_iter().collect(), errors: head.errors };
    Ok((&b[16 + n..], names, g))
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PartInfo {
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub cache: u32,
    /// The engine build that wrote the caches (G3d; none in files saved before).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine: Option<Engine>,
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
    /// B-rep entries by part hash, as stored (decoded when a part is resolved).
    pub breps: HashMap<String, Arc<Vec<u8>>>,
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
        let breps = m.parts.keys().filter_map(|h| c.get(&brep_entry(h)).map(|b| (h.clone(), Arc::new(b.to_vec())))).collect();
        Cache { manifest: m, meshes, breps }
    }
}

/// The container for `doc` with the meshes of `parts` (hash → name and mesh) and the B-rep
/// entries `breps` (hash → entry) as caches, written by `engine`.
pub fn write(doc: &AssemblyDocument, quality: f64, engine: &Engine, components: &BTreeMap<String, String>, parts: &BTreeMap<String, (String, Arc<RenderMesh>)>, breps: &BTreeMap<String, Arc<Vec<u8>>>) -> Result<Container, String> {
    let mut c = Container::new();
    c.insert(oc_doc::container::DOCUMENT_ENTRY, oc_doc::save_assembly(doc).map_err(|e| e.to_string())?.into_bytes());
    let manifest = Manifest {
        cache: CACHE_VERSION,
        engine: Some(engine.clone()),
        quality,
        components: components.clone(),
        parts: parts.iter().map(|(h, (name, _))| (h.clone(), PartInfo { name: name.clone() })).collect(),
    };
    c.insert(MANIFEST, serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?);
    for (h, (_, mesh)) in parts {
        c.insert(mesh_entry(h), mesh.to_bytes());
    }
    for (h, b) in breps {
        c.insert(brep_entry(h), b.to_vec());
    }
    Ok(c)
}
