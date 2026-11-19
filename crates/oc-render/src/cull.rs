//! GPU culling (G2, plan section 4 "Phase 2"): compute passes that decide, per instance,
//! which draws it joins this frame and at which level of detail, and write the indirect
//! draws ([`crate::gpu_scene`]).
//!
//! A frame with occlusion culling runs in two phases (the usual two-pass Hi-Z scheme):
//!
//! 1. *prepass*: the opaque instances visible last frame (and in the frustum now) are drawn
//!    into the depth buffer only, and a depth pyramid (Hi-Z) is built from it;
//! 2. *main*: every instance is tested against the frustum, the section plane and the
//!    pyramid; the ones drawn in the prepass are always drawn (their depth is there), the
//!    others only when some part of their box may be in front. Visibility is kept for the
//!    next frame's prepass.
//!
//! Only instances whose whole box is behind geometry already drawn this frame are left
//! out, so the image is the one without culling. Hidden edges (Hidden Lines Visible) and
//! the section cap's stencil parity are culled by the frustum only. Picks cull by the
//! pick window's frustum.
use crate::gpu_scene::{GpuScene, KINDS, K_OPAQUE, K_PICK_FACE, K_PRE};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct CullU {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub lod: [f32; 4],
    pub clip: [f32; 4],
    pub screen: [f32; 4],
    pub mode: [u32; 4],
    pub opts: [u32; 4],
}

/// The passes a frame culls for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Prepass = 0,
    Main = 1,
    Pick = 2,
}

impl Phase {
    /// The draw kinds the phase decides: `[first, end)`.
    pub fn kinds(self) -> (u32, u32) {
        match self {
            Phase::Prepass => (K_PRE, K_PRE + 1),
            Phase::Main => (K_OPAQUE, K_PICK_FACE),
            Phase::Pick => (K_PICK_FACE, KINDS),
        }
    }
    pub fn mask(self) -> u32 {
        let (a, b) = self.kinds();
        (a..b).fold(0, |m, k| m | 1 << k)
    }
}

const COMPACT_WG: u32 = 65535;

fn e(binding: u32, resource: wgpu::BindingResource<'_>) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry { binding, resource }
}

/// The hierarchical depth buffer of the main target.
pub(crate) struct Hiz {
    /// Every level, for the culling pass.
    pub all: wgpu::TextureView,
    /// One view per level (storage), and the same as sampled views for the next level.
    levels: Vec<(wgpu::TextureView, wgpu::TextureView, (u32, u32))>,
}

impl Hiz {
    /// For a depth buffer of `w` × `h` px.
    pub fn new(device: &wgpu::Device, w: u32, h: u32) -> Hiz {
        let (w0, h0) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
        let mips = 32 - w0.max(h0).leading_zeros();
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("hi-z"),
            size: wgpu::Extent3d { width: w0, height: h0, depth_or_array_layers: 1 },
            mip_level_count: mips,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = |l: u32| {
            tex.create_view(&wgpu::TextureViewDescriptor { base_mip_level: l, mip_level_count: Some(1), ..Default::default() })
        };
        let levels = (0..mips).map(|l| (view(l), view(l), ((w0 >> l).max(1), (h0 >> l).max(1)))).collect();
        let all = tex.create_view(&Default::default());
        Hiz { all, levels }
    }

    pub fn mips(&self) -> u32 {
        self.levels.len() as u32
    }
}

pub(crate) struct Compute {
    cull: wgpu::ComputePipeline,
    compact: wgpu::ComputePipeline,
    hiz_ms: wgpu::ComputePipeline,
    hiz_one: wgpu::ComputePipeline,
    hiz_reduce: wgpu::ComputePipeline,
    cull_u: [wgpu::Buffer; 3],
    compact_u: [wgpu::Buffer; 3],
    dummy_hiz: wgpu::TextureView,
}

impl Compute {
    pub fn new(device: &wgpu::Device) -> Compute {
        let module = |name: &str, src: &str| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(name), source: wgpu::ShaderSource::Wgsl(src.into()) })
        };
        let cull_m = module("cull", include_str!("shaders/cull.wgsl"));
        let compact_m = module("compact", include_str!("shaders/compact.wgsl"));
        let hiz_m = module("hiz", include_str!("shaders/hiz.wgsl"));
        let pipe = |m: &wgpu::ShaderModule, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: m,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let uniform = |label, size: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let cs = std::mem::size_of::<CullU>();
        let dummy = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("no hi-z"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        // Compaction kinds per phase: first, end (slots and capacity are written per frame).
        let compact_u = [Phase::Prepass, Phase::Main, Phase::Pick].map(|_| uniform("compact", 16));
        Compute {
            cull: pipe(&cull_m, "cull"),
            compact: pipe(&compact_m, "compact"),
            hiz_ms: pipe(&hiz_m, "hiz_ms"),
            hiz_one: pipe(&hiz_m, "hiz_one"),
            hiz_reduce: pipe(&hiz_m, "hiz_reduce"),
            cull_u: [uniform("cull prepass", cs), uniform("cull main", cs), uniform("cull pick", cs)],
            compact_u,
            dummy_hiz: dummy.create_view(&Default::default()),
        }
    }

    /// Writes a phase's uniforms (before the submit that culls).
    pub fn write(&self, queue: &wgpu::Queue, phase: Phase, u: &CullU, gs: &GpuScene) {
        let (a, b) = phase.kinds();
        let t = &gs.tables;
        queue.write_buffer(&self.cull_u[phase as usize], 0, bytemuck::bytes_of(u));
        queue.write_buffer(&self.compact_u[phase as usize], 0, bytemuck::cast_slice(&[a, b, t.slot_count(), t.cap]));
    }

    /// Culls and compacts for `phase` (its uniforms written with [`Compute::write`]).
    /// `hiz`: the pyramid the main phase tests against.
    pub fn encode(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, phase: Phase, gs: &GpuScene, hiz: Option<&Hiz>) {
        let t = &gs.tables;
        let n = t.order.len() as u32;
        let slots = t.slot_count();
        if n == 0 || slots == 0 {
            return;
        }
        let hiz_view = hiz.map_or(&self.dummy_hiz, |h| &h.all);
        let cull_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cull"),
            layout: &self.cull.get_bind_group_layout(0),
            entries: &[
                e(0, self.cull_u[phase as usize].as_entire_binding()),
                e(1, gs.objects.buf.as_entire_binding()),
                e(2, gs.meshes.buf.as_entire_binding()),
                e(3, gs.slots.buf.as_entire_binding()),
                e(4, gs.order.buf.as_entire_binding()),
                e(5, gs.codes.buf.as_entire_binding()),
                e(6, gs.visible.buf.as_entire_binding()),
                e(7, wgpu::BindingResource::TextureView(hiz_view)),
            ],
        });
        let compact_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compact"),
            layout: &self.compact.get_bind_group_layout(0),
            entries: &[
                e(0, self.compact_u[phase as usize].as_entire_binding()),
                e(1, gs.slots.buf.as_entire_binding()),
                e(2, gs.order.buf.as_entire_binding()),
                e(3, gs.codes.buf.as_entire_binding()),
                e(4, gs.list.buf.as_entire_binding()),
                e(5, gs.face_args.buf.as_entire_binding()),
                e(6, gs.edge_args.buf.as_entire_binding()),
            ],
        });
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("cull"), timestamp_writes: None });
        pass.set_pipeline(&self.cull);
        pass.set_bind_group(0, &cull_bind, &[]);
        pass.dispatch_workgroups(n.div_ceil(64), 1, 1);
        pass.set_pipeline(&self.compact);
        pass.set_bind_group(0, &compact_bind, &[]);
        pass.dispatch_workgroups(slots.min(COMPACT_WG), slots.div_ceil(COMPACT_WG), 1);
    }

    /// Builds the pyramid from the depth buffer (`depth`: a depth-only view, `samples` per pixel).
    pub fn encode_hiz(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, hiz: &Hiz, depth: &wgpu::TextureView, samples: u32) {
        let mut binds = vec![];
        for (l, (store, _, _)) in hiz.levels.iter().enumerate() {
            let (pipe, src) = match l {
                0 if samples > 1 => (&self.hiz_ms, (1, depth)),
                0 => (&self.hiz_one, (2, depth)),
                _ => (&self.hiz_reduce, (3, &hiz.levels[l - 1].1)),
            };
            binds.push(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("hi-z"),
                layout: &pipe.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(store) },
                    wgpu::BindGroupEntry { binding: src.0, resource: wgpu::BindingResource::TextureView(src.1) },
                ],
            }));
        }
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("hi-z"), timestamp_writes: None });
        for (l, (_, _, (w, h))) in hiz.levels.iter().enumerate() {
            pass.set_pipeline(match l {
                0 if samples > 1 => &self.hiz_ms,
                0 => &self.hiz_one,
                _ => &self.hiz_reduce,
            });
            pass.set_bind_group(0, &binds[l], &[]);
            pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), 1);
        }
    }
}

/// Instance counts per draw kind summed over slots, and per level for opaque faces: what
/// the last frame (and the last pick) drew. Blocking; for tests and benchmarks.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawStats {
    /// By kind: prepass, opaque faces, transparent faces, edges, hidden edges, section
    /// parity, pick faces, pick edges, pick vertices.
    pub kinds: [u32; 9],
    /// Opaque-face instances per level of detail.
    pub opaque_levels: Vec<u32>,
    /// Part instances drawn through the GPU lists.
    pub instances: u32,
}

pub(crate) fn read_stats(device: &wgpu::Device, queue: &wgpu::Queue, gs: &GpuScene) -> DrawStats {
    let t = &gs.tables;
    let s = t.slot_count() as usize;
    let mut st = DrawStats { instances: t.order.len() as u32, ..Default::default() };
    if s == 0 {
        return st;
    }
    let read = |buf: &wgpu::Buffer, bytes: u64| -> Vec<u32> {
        let staging = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: bytes, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(buf, 0, &staging, 0, bytes);
        queue.submit([enc.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        let v = bytemuck::cast_slice(&staging.slice(..).get_mapped_range().expect("mapped")).to_vec();
        staging.unmap();
        v
    };
    let face = read(&gs.face_args.buf, (KINDS as usize * s * 20) as u64);
    let edge = read(&gs.edge_args.buf, (KINDS as usize * s * 16) as u64);
    for k in 0..KINDS as usize {
        st.kinds[k] = (0..s)
            .map(|i| if crate::gpu_scene::is_face_kind(k as u32) { face[(k * s + i) * 5 + 1] } else { edge[(k * s + i) * 4 + 1] })
            .sum();
    }
    for (i, slot) in t.slots.iter().enumerate() {
        let l = slot.info[3] as usize;
        if st.opaque_levels.len() <= l {
            st.opaque_levels.resize(l + 1, 0);
        }
        st.opaque_levels[l] += face[(K_OPAQUE as usize * s + i) * 5 + 1];
    }
    st
}
