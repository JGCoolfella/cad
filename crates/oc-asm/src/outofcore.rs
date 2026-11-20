//! Out-of-core parts (G3c): B-rep caches and a memory budget for bodies.
//!
//! - A part's body is kept as a B-rep entry ([`crate::cache::encode_brep`]: the kernel's
//!   native blob with the faces' persistent names, the reference geometry and the feature
//!   errors). It comes from the `.ocasm` when the assembly is opened lightweight, and is made
//!   when a body is evicted or the assembly saved. Resolving a lightweight or evicted part
//!   reads it (`import_native`) instead of regenerating the part.
//! - [`Assembler::body_budget`] bounds the memory the parts' bodies take, estimated as
//!   [`BODY_PER_BLOB_BYTE`] times their native blobs. After every build and resolve, the least
//!   recently used bodies beyond it are dropped; their parts become lightweight again (mates
//!   use the geometry stored with them, as when opened lightweight) and are read back when a
//!   pick, an evaluation or a non-lightweight build needs them. Display meshes are not
//!   evicted: the renderer holds them (and the GPU copy) while they are drawn.
use std::sync::Arc;

use oc_graph::Shape;
use oc_kernel::Kernel;

use crate::build::{Assembler, LivePart};
use crate::cache::{decode_brep, encode_brep};
use crate::parallel::RefGeometry;

/// Memory a live body takes per byte of its native blob, measured (G3c: 12 plates with 108
/// to 207 holes, 10 MB of bodies for 1.4 MB of blobs; `oc-host/tests/timings.rs`).
pub const BODY_PER_BLOB_BYTE: usize = 7;

impl<K: Kernel + Clone + Send> Assembler<K> {
    /// The assembler's clock, one tick per use.
    pub(crate) fn tick(&self) -> u64 {
        let t = self.clock.get() + 1;
        self.clock.set(t);
        t
    }

    /// Marks part `h` as used now.
    pub(crate) fn touch(&self, h: &str) {
        if let Some(p) = self.parts.get(h) {
            p.used.set(self.tick());
        }
    }

    /// Part `h`'s B-rep entry: its cache entry, or made from its live body.
    pub fn brep_of(&self, h: &str) -> Option<Arc<Vec<u8>>> {
        let p = self.parts.get(h)?;
        if let Some(b) = &p.brep {
            return Some(b.clone());
        }
        let live = p.live.as_ref()?;
        let shape = live.shape.as_ref()?;
        let blob = self.regen.kernel().export_native(&shape.body).ok()?;
        let g = RefGeometry { planes: live.planes.clone(), axes: live.axes.clone(), points: live.points.clone(), errors: live.errors.clone() };
        Some(Arc::new(encode_brep(&blob, &shape.names, &g)))
    }

    /// Makes part `h` live from its B-rep entry; false (and the entry dropped) if it does not
    /// read: another format, or a blob of another kernel version.
    pub(crate) fn read_brep(&mut self, h: &str) -> bool {
        let Some(entry) = self.parts.get(h).and_then(|p| p.brep.clone()) else { return false };
        let t = std::time::Instant::now();
        let live = decode_brep(&entry).and_then(|(blob, names, g)| {
            let body = self.regen.kernel().import_native(blob).map_err(|e| e.to_string())?;
            let mut live = LivePart::from_geometry(g);
            live.shape = Some(Shape::new(body, names));
            Ok(live)
        });
        let p = self.parts.get_mut(h).expect("checked above");
        match live {
            Ok(l) => {
                p.live = Some(l);
                self.stats.read += 1;
                self.stats.time += t.elapsed();
                self.touch(h);
                true
            }
            Err(_) => {
                p.brep = None;
                false
            }
        }
    }

    /// Estimated memory of the live bodies now.
    pub fn body_memory(&self) -> usize {
        self.parts.values().filter(|p| p.live.as_ref().is_some_and(|l| l.shape.is_some())).filter_map(|p| p.brep.as_ref()).map(|b| b.len() * BODY_PER_BLOB_BYTE).sum()
    }

    /// Drops the least recently used bodies until the live ones fit the budget (the most
    /// recently used always stays). Each keeps (or gets) its B-rep entry to be read back.
    pub fn keep_budget(&mut self) {
        let Some(budget) = self.body_budget else { return };
        // Entries for the live bodies that have none (measured by their blobs).
        let missing: Vec<String> = self.parts.iter().filter(|(_, p)| p.brep.is_none() && p.live.as_ref().is_some_and(|l| l.shape.is_some())).map(|(h, _)| h.clone()).collect();
        for h in missing {
            let b = self.brep_of(&h);
            if let Some(p) = self.parts.get_mut(&h) {
                p.brep = b;
            }
        }
        let mut live: Vec<(u64, String, usize)> = self
            .parts
            .iter()
            .filter(|(_, p)| p.live.as_ref().is_some_and(|l| l.shape.is_some()) && p.mesh.is_some())
            .filter_map(|(h, p)| p.brep.as_ref().map(|b| (p.used.get(), h.clone(), b.len() * BODY_PER_BLOB_BYTE)))
            .collect();
        let mut total: usize = live.iter().map(|x| x.2).sum();
        if total <= budget {
            return;
        }
        live.sort();
        for (_, h, size) in live.iter().take(live.len().saturating_sub(1)) {
            if total <= budget {
                break;
            }
            if let Some(p) = self.parts.get_mut(h) {
                p.live = None;
                total -= size;
                self.stats.evicted += 1;
            }
        }
    }
}
