//! Display tessellation for the renderer (`docs/OVERHAUL_PLAN.md`, E4).
//!
//! [`build`] meshes a body through any [`oc_kernel::Kernel`] (which meshes faces in
//! parallel) at the user's Image Quality and at coarser levels of detail, and [`pack`]
//! turns those meshes into a [`RenderMesh`]: per level, a vertex buffer, an index buffer
//! and an edge-segment buffer in the byte layout `oc-render` uploads directly
//! ([`layout`]), plus per-face and per-edge ranges for picking and highlighting.
//! Packing runs one face (and one edge) per task on rayon.
//!
//! Determinism (plan section 2): the same body and options give the same bytes
//! ([`RenderMesh::to_bytes`]) run after run, with any thread count, whatever was meshed
//! before.

mod blob;
pub mod layout;
mod pack;

pub use blob::{content_hash, BlobError};
pub use layout::{IndexFormat, Segment, Vertex, SEGMENT_STRIDE, VERTEX_STRIDE};

use oc_kernel::{v3, BBox, KResult, Kernel, Mesh, MeshParams, Topology, P3};

/// Vertex flag: the vertex is not a selectable corner. Every visible edge at it is closed
/// (a full circle starts and ends there) or there is none (a vertex only on a seam or
/// pole). SOLIDWORKS (Parasolid) has no vertex on a full circle; the current viewer
/// shows one.
pub const POINT_HIDDEN: u32 = 1;
/// Edge flag: the edge closes a periodic face on itself (cylinder seam). Not drawn.
pub const EDGE_SEAM: u32 = 1;
/// Edge flag: the edge has no length (sphere pole, cone apex). Not drawn.
pub const EDGE_DEGENERATE: u32 = 2;

/// A coarser level is kept only if it has at most this fraction (3/5) of the previous
/// level's triangles; otherwise it would cost memory without saving work.
pub const LOD_KEEP: (usize, usize) = (3, 5);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshOptions {
    /// SOLIDWORKS Image Quality slider, 0..=10 (default 3); sets level 0
    /// ([`oc_kernel::Tolerances::display_mesh`]).
    pub quality: f64,
    /// Most levels of detail (≥ 1). Level `k` uses
    /// [`oc_kernel::Tolerances::display_lod`]`(quality, size, k)`.
    pub max_lods: u32,
    /// Threads for packing: 0 = the current rayon pool (the global one by default),
    /// `n` = a pool of `n` threads for this call. The kernel's own threads are set on the
    /// kernel ([`Kernel::set_threads`]).
    pub threads: usize,
}

impl Default for MeshOptions {
    fn default() -> Self {
        MeshOptions {
            quality: 3.0,
            max_lods: 4,
            threads: 0,
        }
    }
}

/// Where face `f`'s vertices and indices sit in a level's buffers (in elements).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct FaceRange {
    pub first_vertex: u32,
    pub vertex_count: u32,
    pub first_index: u32,
    pub index_count: u32,
}

/// Where edge `e`'s segments sit in a level's segment buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct EdgeRange {
    pub first_segment: u32,
    pub segment_count: u32,
}

/// One level of detail: GPU-ready buffers ([`layout`]) and their per-entity ranges.
#[derive(Clone, Debug, PartialEq)]
pub struct Lod {
    /// The tessellation tolerances; `params.linear` bounds the chordal deviation (mm) and
    /// drives [`RenderMesh::select_lod`].
    pub params: MeshParams,
    pub vertex_count: u32,
    /// `vertex_count` × [`VERTEX_STRIDE`] bytes.
    pub vertices: Vec<u8>,
    pub index_format: IndexFormat,
    pub index_count: u32,
    /// `index_count` indices, padded with zero bytes to a multiple of 4.
    pub indices: Vec<u8>,
    /// Indexed by face ([`oc_kernel::FaceId`]).
    pub faces: Vec<FaceRange>,
    pub segment_count: u32,
    /// `segment_count` × [`SEGMENT_STRIDE`] bytes.
    pub segments: Vec<u8>,
    /// Indexed by edge ([`oc_kernel::EdgeId`]).
    pub edges: Vec<EdgeRange>,
}

impl Lod {
    pub fn triangle_count(&self) -> usize {
        self.index_count as usize / 3
    }
    pub fn vertex(&self, i: usize) -> Vertex {
        layout::get_vertex(&self.vertices[i * VERTEX_STRIDE..(i + 1) * VERTEX_STRIDE])
    }
    pub fn index(&self, i: usize) -> u32 {
        layout::get_index(&self.indices, self.index_format, i)
    }
    pub fn triangle(&self, t: usize) -> [u32; 3] {
        [0, 1, 2].map(|k| self.index(3 * t + k))
    }
    pub fn segment(&self, i: usize) -> Segment {
        layout::get_segment(&self.segments[i * SEGMENT_STRIDE..(i + 1) * SEGMENT_STRIDE])
    }
}

/// A body ready for display.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderMesh {
    /// Every position in the buffers is relative to this point (the centre of
    /// [`display_box`]).
    pub origin: P3,
    /// Box around every vertex of every level, relative to `origin`.
    pub bounds: [[f32; 3]; 2],
    /// Radius of a sphere about `origin` that contains `bounds`.
    pub radius: f32,
    /// Model vertices ([`oc_kernel::VertexId`] order), relative to `origin`, for vertex
    /// picking, with [`POINT_HIDDEN`] flags.
    pub points: Vec<[f32; 3]>,
    pub point_flags: Vec<u32>,
    /// [`EDGE_SEAM`], [`EDGE_DEGENERATE`] per edge.
    pub edge_flags: Vec<u32>,
    /// Finest first; level 0 is the Image Quality. Each level has fewer triangles.
    pub lods: Vec<Lod>,
}

impl RenderMesh {
    pub fn face_count(&self) -> usize {
        self.lods.first().map_or(0, |l| l.faces.len())
    }
    pub fn edge_count(&self) -> usize {
        self.edge_flags.len()
    }

    /// The coarsest level whose chordal deviation is at most `max_error_px` pixels when
    /// one pixel spans `mm_per_px` millimetres at the body's nearest point. Never finer
    /// than level 0 (the Image Quality).
    pub fn select_lod(&self, mm_per_px: f64, max_error_px: f64) -> usize {
        let limit = mm_per_px * max_error_px;
        self.lods
            .iter()
            .rposition(|l| l.params.linear <= limit)
            .unwrap_or(0)
    }
}

/// The box that sizes a display mesh: the union of the faces' boxes (tight for planes,
/// OCCT's fast box for curved faces, like the current viewer's) and the vertices. Cheap,
/// unlike an exact box ([`Kernel::bounding_box`] costs seconds on hundreds of fillets).
pub fn display_box(topo: &Topology) -> BBox {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    let boxes = topo
        .faces
        .iter()
        .map(|f| (f.bbox.min, f.bbox.max))
        .chain(topo.vertices.iter().map(|&p| (p, p)));
    for (a, b) in boxes {
        for k in 0..3 {
            lo[k] = lo[k].min(a[k]);
            hi[k] = hi[k].max(b[k]);
        }
    }
    if lo[0] > hi[0] {
        return BBox {
            min: [0.0; 3],
            max: [0.0; 3],
        };
    }
    BBox { min: lo, max: hi }
}

/// Meshes `body` at `opts.quality` and coarser levels and packs the result.
pub fn build<K: Kernel>(kernel: &K, body: &K::Body, opts: &MeshOptions) -> KResult<RenderMesh> {
    let topo = kernel.topology(body)?;
    let bb = display_box(&topo);
    let origin = v3::scale(v3::add(bb.min, bb.max), 0.5);
    let size = bb.diagonal();
    let tol = kernel.tolerances();
    let mut levels: Vec<(MeshParams, Mesh)> = vec![];
    for level in 0..opts.max_lods.max(1) {
        let p = tol.display_lod(opts.quality, size, level);
        let m = kernel.tessellate(body, &p)?;
        if let Some((_, prev)) = levels.last() {
            let (n, before) = (m.triangles.len(), prev.triangles.len());
            if n >= before || n * LOD_KEEP.1 > before * LOD_KEEP.0 {
                break;
            }
        }
        levels.push((p, m));
    }
    Ok(match opts.threads {
        0 => pack(&levels, &topo, origin),
        n => rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .map_err(|e| oc_kernel::KernelError::Failed {
                op: oc_kernel::OpKind::Tessellate,
                detail: e.to_string(),
            })?
            .install(|| pack(&levels, &topo, origin)),
    })
}

/// Packs kernel meshes (finest first) of a body with topology `topo`, positions relative
/// to `origin`. Runs on the current rayon pool.
pub fn pack(levels: &[(MeshParams, Mesh)], topo: &Topology, origin: P3) -> RenderMesh {
    let edge_flags: Vec<u32> = topo
        .edges
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let short = levels
                .first()
                .is_some_and(|(_, m)| m.edges.get(i).is_none_or(|l| l.len() < 2));
            (if e.seam { EDGE_SEAM } else { 0 })
                | (if short || e.length <= 0.0 {
                    EDGE_DEGENERATE
                } else {
                    0
                })
        })
        .collect();
    let visible: Vec<bool> = edge_flags.iter().map(|&f| f == 0).collect();
    let mut corner = vec![false; topo.vertices.len()];
    for (e, info) in topo.edges.iter().enumerate() {
        let open = info.vertices.len() == 2 && info.vertices[0] != info.vertices[1];
        if visible[e] && open {
            for v in &info.vertices {
                corner[v.index()] = true;
            }
        }
    }
    let points: Vec<[f32; 3]> = topo
        .vertices
        .iter()
        .map(|&p| v3::sub(p, origin).map(|c| c as f32))
        .collect();
    let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
    let mut lods = Vec::with_capacity(levels.len());
    for (p, m) in levels {
        let (lod, b) = pack::pack_level(*p, m, &visible, origin);
        let [lo, hi] = &mut bounds;
        pack::grow(lo, hi, b[0], b[1]);
        lods.push(lod);
    }
    for q in &points {
        let [lo, hi] = &mut bounds;
        pack::grow(lo, hi, *q, *q);
    }
    if bounds[0][0] > bounds[1][0] {
        bounds = [[0.0; 3]; 2];
    }
    let far = [0, 1, 2].map(|k| bounds[0][k].abs().max(bounds[1][k].abs()) as f64);
    // Rounded up, so the sphere still contains the box after the conversion to f32.
    let radius = (v3::len(far) * (1.0 + 1e-6)) as f32;
    RenderMesh {
        origin,
        bounds,
        radius,
        points,
        point_flags: corner
            .iter()
            .map(|&c| if c { 0 } else { POINT_HIDDEN })
            .collect(),
        edge_flags,
        lods,
    }
}
