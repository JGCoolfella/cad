//! Body draws (G2). Part bodies are drawn per draw kind: one `multi_draw_*_indirect` over
//! every draw slot when the GPU culls ([`crate::Culling::gpu`]), else one draw per body in
//! scene order. Previews and highlights are always drawn one by one. Every draw reads the
//! shared geometry pool; a body drawn alone uses the identity range of the instance list.
use crate::gpu_scene::{is_face_kind, GpuScene, K_PICK_VERTEX};
use crate::instances::cpu_level;
use crate::mesh::MeshGpu;
use crate::renderer::Renderer;
use crate::scene::Scene;

fn e(binding: u32, buf: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource: buf.as_entire_binding() }
}

pub(crate) struct BodyDraws<'a> {
    r: &'a Renderer,
    gs: &'a GpuScene,
    scene: &'a Scene,
    bind: wgpu::BindGroup,
    /// Draw lists filled by the GPU (else one draw per body).
    gpu: bool,
    kinds: &'a [u32],
    /// Level per body for bodies drawn one by one.
    levels: Vec<usize>,
}

impl<'a> BodyDraws<'a> {
    pub fn new(r: &'a Renderer, scene: &'a Scene, kinds: &'a [u32], cam: &crate::Camera, scale: f64, gpu: bool) -> BodyDraws<'a> {
        let gs = &r.gs;
        let bind = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bodies"),
            layout: &r.pipes.body_bgl,
            entries: &[
                e(0, &gs.objects.buf),
                e(1, &gs.states.buf),
                e(2, &gs.list.buf),
                e(3, &r.pool.segs.buf),
                e(4, &r.pool.seg_dist.buf),
                e(5, &r.pool.points.buf),
            ],
        });
        let levels = scene
            .bodies
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let alone = !gpu || kinds.get(i).is_none_or(|&k| k == 0);
                match r.meshes.get(&b.mesh) {
                    Some(m) if alone => cpu_level(m, b, cam, scale),
                    _ => 0,
                }
            })
            .collect();
        BodyDraws { r, gs, scene, bind, gpu, kinds, levels }
    }

    fn mesh(&self, i: usize) -> Option<&MeshGpu> {
        self.r.meshes.get(&self.scene.bodies[i].mesh)
    }

    fn geometry(&self, pass: &mut wgpu::RenderPass, pipe: &wgpu::RenderPipeline) {
        let pool = &self.r.pool;
        pass.set_pipeline(pipe);
        pass.set_bind_group(1, &self.bind, &[]);
        pass.set_vertex_buffer(0, pool.vertices.buf.slice(..));
        pass.set_index_buffer(pool.indices.buf.slice(..), wgpu::IndexFormat::Uint32);
    }

    /// Every part body of draw kind `k`.
    pub fn kind(&self, pass: &mut wgpu::RenderPass, k: u32, pipe: &wgpu::RenderPipeline) {
        if self.gpu {
            let slots = self.gs.tables.slot_count();
            if slots == 0 {
                return;
            }
            self.geometry(pass, pipe);
            if is_face_kind(k) {
                pass.multi_draw_indexed_indirect(&self.gs.face_args.buf, (k * slots) as u64 * 20, slots);
            } else {
                pass.multi_draw_indirect(&self.gs.edge_args.buf, (k * slots) as u64 * 16, slots);
            }
            return;
        }
        for (i, &bits) in self.kinds.iter().enumerate() {
            if bits & (1 << k) == 0 {
                continue;
            }
            if k == K_PICK_VERTEX {
                self.points(pass, i, pipe);
            } else if is_face_kind(k) {
                self.faces(pass, i, pipe);
            } else {
                self.edges(pass, i, pipe);
            }
        }
    }

    /// Body `i`'s faces.
    pub fn faces(&self, pass: &mut wgpu::RenderPass, i: usize, pipe: &wgpu::RenderPipeline) {
        let Some(m) = self.mesh(i) else { return };
        let r = m.levels[self.levels[i]].range;
        if r.index_count == 0 {
            return;
        }
        let id = self.gs.tables.identity_base() + i as u32;
        self.geometry(pass, pipe);
        pass.draw_indexed(r.first_index..r.first_index + r.index_count, r.base_vertex, id..id + 1);
    }

    /// Body `i`'s edges.
    pub fn edges(&self, pass: &mut wgpu::RenderPass, i: usize, pipe: &wgpu::RenderPipeline) {
        let Some(m) = self.mesh(i) else { return };
        let r = m.levels[self.levels[i]].range;
        if r.seg_count == 0 {
            return;
        }
        let id = self.gs.tables.identity_base() + i as u32;
        self.geometry(pass, pipe);
        pass.draw(6 * r.seg_base..6 * (r.seg_base + r.seg_count), id..id + 1);
    }

    /// Body `i`'s model vertices (pick squares).
    fn points(&self, pass: &mut wgpu::RenderPass, i: usize, pipe: &wgpu::RenderPipeline) {
        let Some(m) = self.mesh(i) else { return };
        if m.point_count == 0 {
            return;
        }
        let id = self.gs.tables.identity_base() + i as u32;
        self.geometry(pass, pipe);
        pass.draw(6 * m.point_base..6 * (m.point_base + m.point_count), id..id + 1);
    }
}
