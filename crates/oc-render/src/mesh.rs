//! GPU copies of [`oc_mesh::RenderMesh`] buffers, uploaded as they are (plan E4: "output
//! layout the renderer consumes directly"), plus two derived buffers: the cumulative
//! length of each edge at each segment's start (for dashes) and the model vertices as
//! `vec4` (position, flags).
use glam::DVec3;
use oc_mesh::{IndexFormat, RenderMesh};
use wgpu::util::DeviceExt;

pub(crate) struct LevelGpu {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub index_format: wgpu::IndexFormat,
    pub index_count: u32,
    pub segments: wgpu::Buffer,
    pub segment_count: u32,
    pub seg_dist: wgpu::Buffer,
    /// Chordal deviation, mm (for automatic level selection).
    pub linear: f64,
}

pub(crate) struct MeshGpu {
    pub origin: DVec3,
    pub bounds: [[f32; 3]; 2],
    pub levels: Vec<LevelGpu>,
    pub points: wgpu::Buffer,
    pub point_count: u32,
    /// Model vertex positions relative to `origin` (for vertex markers).
    pub point_pos: Vec<[f32; 3]>,
    pub face_count: u32,
    pub edge_count: u32,
    /// CPU copy for ray casts (Select Other, rotate-about point).
    pub cpu: std::sync::Arc<RenderMesh>,
}

/// Storage buffers may not be empty.
fn storage(device: &wgpu::Device, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    let mut v = bytes.to_vec();
    if v.len() < 16 {
        v.resize(16, 0);
    }
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: &v,
        usage: wgpu::BufferUsages::STORAGE,
    })
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
    pub fn new(device: &wgpu::Device, mesh: std::sync::Arc<RenderMesh>) -> MeshGpu {
        let levels = mesh
            .lods
            .iter()
            .map(|l| {
                let dist = segment_distances(l);
                let mut idx = l.indices.clone();
                if idx.is_empty() {
                    idx.resize(4, 0);
                }
                let mut verts = l.vertices.clone();
                if verts.is_empty() {
                    verts.resize(oc_mesh::VERTEX_STRIDE, 0);
                }
                LevelGpu {
                    vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("vertices"),
                        contents: &verts,
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                    indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("indices"),
                        contents: &idx,
                        usage: wgpu::BufferUsages::INDEX,
                    }),
                    index_format: match l.index_format {
                        IndexFormat::U16 => wgpu::IndexFormat::Uint16,
                        IndexFormat::U32 => wgpu::IndexFormat::Uint32,
                    },
                    index_count: l.index_count,
                    segments: storage(device, "segments", &l.segments),
                    segment_count: l.segment_count,
                    seg_dist: storage(device, "seg_dist", bytemuck::cast_slice(&dist)),
                    linear: l.params.linear,
                }
            })
            .collect();
        let pts: Vec<[f32; 4]> = mesh
            .points
            .iter()
            .zip(&mesh.point_flags)
            .map(|(p, &f)| [p[0], p[1], p[2], f32::from_bits(f)])
            .collect();
        MeshGpu {
            origin: DVec3::from(mesh.origin),
            bounds: mesh.bounds,
            levels,
            points: storage(device, "points", bytemuck::cast_slice(&pts)),
            point_count: pts.len() as u32,
            point_pos: mesh.points.clone(),
            face_count: mesh.face_count() as u32,
            edge_count: mesh.edge_count() as u32,
            cpu: mesh,
        }
    }

    /// The level to draw for a choice, given millimetres per physical pixel at the body.
    pub fn level(&self, choice: crate::scene::LodChoice, mm_per_px: f64) -> usize {
        let last = self.levels.len().saturating_sub(1);
        match choice {
            crate::scene::LodChoice::Finest => 0,
            crate::scene::LodChoice::Level(k) => k.min(last),
            crate::scene::LodChoice::Auto { max_error_px } => {
                let limit = mm_per_px * max_error_px;
                self.levels
                    .iter()
                    .rposition(|l| l.linear <= limit)
                    .unwrap_or(0)
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
