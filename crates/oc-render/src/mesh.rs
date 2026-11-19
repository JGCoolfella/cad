//! Uploaded meshes: [`oc_mesh::RenderMesh`] buffers copied as they are into the shared
//! geometry pool ([`crate::pool`]; plan E4: "output layout the renderer consumes
//! directly"), plus two derived arrays: the cumulative length of each edge at each
//! segment's start (for dashes) and the model vertices as `vec4` (position, flags). Indices
//! are widened to 32 bits so every level shares one index buffer.
use crate::pool::{LevelRange, Pool};
use glam::DVec3;
use oc_mesh::RenderMesh;
use std::sync::Arc;

pub(crate) struct LevelGpu {
    pub range: LevelRange,
    /// Chordal deviation, mm (for automatic level selection).
    pub linear: f64,
}

pub(crate) struct MeshGpu {
    pub origin: DVec3,
    pub bounds: [[f32; 3]; 2],
    pub levels: Vec<LevelGpu>,
    pub point_base: u32,
    pub point_count: u32,
    /// Model vertex positions relative to `origin` (for vertex markers).
    pub point_pos: Vec<[f32; 3]>,
    pub face_count: u32,
    pub edge_count: u32,
    /// CPU copy for ray casts (Select Other, rotate-about point) and pool compaction.
    pub cpu: Arc<RenderMesh>,
    /// [`crate::Renderer::add_mesh`] calls not yet matched by `remove_mesh` (the same
    /// `Arc` added again shares the upload: an assembly rebuilt with unchanged parts).
    pub refs: u32,
}

/// Length along its edge of each segment's start point (mm).
pub(crate) fn segment_distances(level: &oc_mesh::Lod) -> Vec<f32> {
    let mut out = vec![0f32; level.segment_count as usize];
    for r in &level.edges {
        let mut run = 0f64;
        for i in r.first_segment..r.first_segment + r.segment_count {
            out[i as usize] = run as f32;
            let s = level.segment(i as usize);
            let d: f64 = (0..3).map(|k| ((s.b[k] - s.a[k]) as f64).powi(2)).sum();
            run += d.sqrt();
        }
    }
    out
}

impl MeshGpu {
    /// Uploads `mesh` into the pool; None when the pool is full.
    pub fn new(pool: &mut Pool, device: &wgpu::Device, queue: &wgpu::Queue, mesh: Arc<RenderMesh>) -> Option<MeshGpu> {
        let levels = mesh
            .lods
            .iter()
            .map(|l| Some(LevelGpu { range: pool.push_level(device, queue, l)?, linear: l.params.linear }))
            .collect::<Option<Vec<_>>>()?;
        let pts: Vec<[f32; 4]> = mesh
            .points
            .iter()
            .zip(&mesh.point_flags)
            .map(|(p, &f)| [p[0], p[1], p[2], f32::from_bits(f)])
            .collect();
        let point_base = pool.push_points(device, queue, &pts)?;
        Some(MeshGpu {
            origin: DVec3::from(mesh.origin),
            bounds: mesh.bounds,
            levels,
            point_base,
            point_count: pts.len() as u32,
            point_pos: mesh.points.clone(),
            face_count: mesh.face_count() as u32,
            edge_count: mesh.edge_count() as u32,
            cpu: mesh,
            refs: 1,
        })
    }

    pub fn release(&self, pool: &mut Pool) {
        let r: Vec<LevelRange> = self.levels.iter().map(|l| l.range).collect();
        pool.release(&r, self.point_count);
    }

    /// The level to draw for a choice, given millimetres per physical pixel at the body
    /// (bodies drawn one by one: previews and highlights; part bodies choose on the GPU,
    /// `cull.wgsl`, by the same rule).
    pub fn level(&self, choice: crate::scene::LodChoice, mm_per_px: f64) -> usize {
        let last = self.levels.len().saturating_sub(1);
        match choice {
            crate::scene::LodChoice::Finest => 0,
            crate::scene::LodChoice::Level(k) => k.min(last),
            crate::scene::LodChoice::Auto { max_error_px } => {
                let limit = mm_per_px * max_error_px;
                self.levels.iter().rposition(|l| l.linear <= limit).unwrap_or(0)
            }
        }
    }

    /// World box of the mesh.
    pub fn world_box(&self) -> crate::camera::Aabb {
        let [lo, hi] = self.bounds;
        crate::camera::Aabb {
            min: self.origin + DVec3::new(lo[0] as f64, lo[1] as f64, lo[2] as f64),
            max: self.origin + DVec3::new(hi[0] as f64, hi[1] as f64, hi[2] as f64),
        }
    }
}
