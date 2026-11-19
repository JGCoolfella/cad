//! The ID pass: the scene's pick geometry rendered through a cropped projection into a
//! [`PICK`]×[`PICK`] `Rg32Uint` window (one texel per logical pixel) and read back.
//! [`Renderer::pick`] waits for the result (clicks); [`Renderer::pick_async`] returns at
//! once and [`PendingPick::poll`] collects it on a later frame (hover).
use crate::batch::{self, Ctx};
use crate::cull::Phase;
use crate::draw::BodyDraws;
use crate::frame::{frame_uniform, prepare};
use crate::gpu_scene::{K_PICK_EDGE, K_PICK_FACE, K_PICK_VERTEX};
use crate::instances;
use crate::pick::{crop, nearest, resolve, to_hit, PickFilter, PICK};
use crate::pipelines::{ID_DEPTH, ID_FORMAT};
use crate::renderer::{Renderer, Viewport};
use crate::scene::{Hit, Scene};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const ROW: u32 = 256; // PICK × 8 bytes, padded to wgpu's row alignment

pub(crate) fn targets(device: &wgpu::Device) -> (wgpu::Texture, wgpu::TextureView, wgpu::TextureView) {
    let tex = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("pick"),
            size: wgpu::Extent3d { width: PICK, height: PICK, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let id = tex(ID_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
    let depth = tex(ID_DEPTH, wgpu::TextureUsages::RENDER_ATTACHMENT);
    let (iv, dv) = (id.create_view(&Default::default()), depth.create_view(&Default::default()));
    (id, iv, dv)
}

/// A pick submitted to the GPU and not read yet.
pub struct PendingPick {
    buf: Option<wgpu::Buffer>,
    ready: Arc<AtomicBool>,
    body_ids: Vec<u32>,
    filter: PickFilter,
    /// Decided before the ID pass (the origin).
    early: Option<Option<Hit>>,
}

impl PendingPick {
    /// `Some(result)` once the GPU has finished, `None` while it has not. Never blocks.
    pub fn poll(&mut self, r: &Renderer) -> Option<Option<Hit>> {
        if let Some(e) = self.early {
            return Some(e);
        }
        let _ = r.device.poll(wgpu::PollType::Poll);
        if !self.ready.load(Ordering::Acquire) {
            return None;
        }
        let buf = self.buf.take()?;
        let ids = decode(&buf.slice(..).get_mapped_range().expect("mapped"));
        buf.unmap();
        let hit = resolve(&nearest(&ids, PICK), &self.filter).and_then(|raw| to_hit(raw, &self.body_ids));
        self.early = Some(hit);
        Some(hit)
    }
}

fn decode(bytes: &[u8]) -> Vec<[u32; 2]> {
    let mut ids = Vec::with_capacity((PICK * PICK) as usize);
    for y in 0..PICK {
        for x in 0..PICK {
            let o = (y * ROW + x * 8) as usize;
            let w = |k: usize| u32::from_le_bytes([bytes[o + k], bytes[o + k + 1], bytes[o + k + 2], bytes[o + k + 3]]);
            ids.push([w(0), w(4)]);
        }
    }
    ids
}

impl Renderer {
    /// What is under logical pixel (sx, sy), by the rules of `viewport.ts` `pick()`
    /// (see [`crate::pick`]). Blocks until the GPU answers (well under a frame, P4).
    pub fn pick(&mut self, scene: &Scene, camera: &crate::Camera, vp: Viewport, sx: f64, sy: f64, filter: &PickFilter) -> Option<Hit> {
        let mut p = self.pick_async(scene, camera, vp, sx, sy, filter);
        if let Some(e) = p.early {
            return e;
        }
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        p.poll(self).flatten()
    }

    /// Submits a pick and returns without waiting (hover).
    pub fn pick_async(&mut self, scene: &Scene, camera: &crate::Camera, vp: Viewport, sx: f64, sy: f64, filter: &PickFilter) -> PendingPick {
        let p = prepare(self, scene, camera, vp);
        let body_ids: Vec<u32> = scene.bodies.iter().map(|b| b.id).collect();
        let pending = |early| PendingPick { buf: None, ready: Arc::new(AtomicBool::new(true)), body_ids: body_ids.clone(), filter: filter.clone(), early };
        // The origin, by screen distance, before anything else (shown origins, no sketch).
        if scene.origin.pickable {
            let [ox, oy] = p.cam.project(glam::DVec3::ZERO);
            if (ox - sx).hypot(oy - sy) <= 5.0 {
                return pending(Some(Some(Hit::Origin)));
            }
        }
        let (lw, lh) = vp.logical();
        let half = (PICK / 2) as f64;
        let vpm = crop(lw, lh, sx.floor() - half, sy.floor() - half) * p.view_proj;
        let fu = frame_uniform(scene, &p, vpm, (PICK as f32, PICK as f32), 1.0, true);
        self.queue.write_buffer(&self.pick_frame_buf, 0, bytemuck::bytes_of(&fu));
        let cx = Ctx { scene, cam: &p.cam, scale: vp.scale, model: p.model };
        let bt = batch::pick(&cx);
        // The same GPU scene as the frame (objects are camera relative, so a pick of the
        // drawn scene uploads nothing), culled by the pick window.
        let built = instances::build(self, scene, &p.cam, vp.scale);
        let c = self.culling();
        self.gs.upload(&self.device, &self.queue, built.objects, built.states, built.tables);
        if c.gpu {
            let n = self.gs.tables.order.len() as u32;
            let u = instances::cull_uniform(scene, &p, vpm, (PICK as f32, PICK as f32), 8.0, Phase::Pick, 0, c.frustum, n, vp.scale);
            self.compute.write(&self.queue, Phase::Pick, &u, &self.gs);
        }
        let kinds = built.kinds;
        let batch_bind = self.batch_bind(&bt);
        let frame_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pick frame"),
            layout: &self.pipes.frame_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.pick_frame_buf.as_entire_binding() }],
        });
        let planes = self.vertex_buffer(bytemuck::cast_slice(&bt.fills));
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("pick readback"),
            size: (ROW * PICK) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let d = BodyDraws::new(self, scene, &kinds, &p.cam, vp.scale, c.gpu);
        let mut enc = self.device.create_command_encoder(&Default::default());
        if c.gpu {
            self.compute.encode(&self.device, &mut enc, Phase::Pick, &self.gs, None);
        }
        {
            let (_, idv, dv) = &self.pick_targets;
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pick"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: idv,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: dv,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            let pp = &self.pipes;
            pass.set_bind_group(0, &frame_bind, &[]);
            // Part bodies: faces (not in Wireframe), edges, vertices. Previews are not pickable.
            d.kind(&mut pass, K_PICK_FACE, &pp.id_face);
            d.kind(&mut pass, K_PICK_EDGE, &pp.id_edge);
            d.kind(&mut pass, K_PICK_VERTEX, &pp.id_vertex);
            if let Some(vb) = &planes {
                pass.set_pipeline(&pp.id_tri);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(0..bt.fills.len() as u32, 0..1);
            }
            pass.set_bind_group(1, &batch_bind, &[]);
            if !bt.lines.is_empty() {
                pass.set_pipeline(&pp.id_line);
                pass.draw(0..6, 0..bt.lines.len() as u32);
            }
            if !bt.points.is_empty() {
                pass.set_pipeline(&pp.id_point);
                pass.draw(0..6, 0..bt.points.len() as u32);
            }
        }
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &self.pick_targets.0, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(ROW), rows_per_image: Some(PICK) },
            },
            wgpu::Extent3d { width: PICK, height: PICK, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);
        let ready = Arc::new(AtomicBool::new(false));
        let flag = ready.clone();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            if r.is_ok() {
                flag.store(true, Ordering::Release);
            }
        });
        PendingPick { buf: Some(buf), ready, body_ids, filter: filter.clone(), early: None }
    }
}
