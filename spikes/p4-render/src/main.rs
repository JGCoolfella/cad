//! P4 spike: wgpu renderer baseline, headless, run on every hardware adapter.
//!
//! Scene: 100 procedurally generated meshes (spheres and tori, 256–3456 triangles each),
//! 10 000 instances sorted by mesh, feature edges drawn as screen-space wide lines.
//! Measured per adapter at 2560×1440 with 4× MSAA:
//!   * GPU time per frame (timestamp queries) and sustained wall time over 200 frames,
//!     shaded + edges, then with a section plane (stencil parity + cap);
//!   * pick latency: render a 16×16 ID window (instance id + face id) and read it back;
//!   * precision: ID image of the scene at offset 0 vs offset 1e7 mm, with camera-relative
//!     f32 (the design) and with naive absolute f32 (what three.js does today).
//!
//! Run: cargo run --release -p p4-render
use bytemuck::{Pod, Zeroable};
use glam::{DMat4, DQuat, DVec3, Mat4, Vec3};
use std::f64::consts::PI;
use std::time::Instant;
use wgpu::util::DeviceExt;

const W: u32 = 2560;
const H: u32 = 1440;
const MSAA: u32 = 4;
const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DS: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;
const ID: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Uint;
const ID_DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const PICK: u32 = 16;
const FRAMES: u32 = 200;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    nrm: [f32; 3],
    face: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Inst {
    m: [[f32; 4]; 3],
    color: u32,
    id: u32,
    _pad: [u32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Seg {
    a: [f32; 4],
    b: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MeshParam {
    seg_base: u32,
    nseg: u32,
    inst_base: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Cam {
    view_proj: [[f32; 4]; 4],
    clip: [f32; 4],
    viewport: [f32; 4],
    flags: [u32; 4],
}

struct MeshRange {
    first_index: u32,
    index_count: u32,
    base_vertex: i32,
}

/// CPU-side scene. Instance transforms are f64; the GPU only ever sees them relative to
/// the camera (or, for the naive comparison, as absolute f32).
struct Scene {
    verts: Vec<Vertex>,
    indices: Vec<u32>,
    segs: Vec<Seg>,
    meshes: Vec<MeshRange>,
    params: Vec<MeshParam>,
    inst_count: Vec<u32>,
    xf: Vec<DMat4>,
    colors: Vec<u32>,
}

fn hash(i: u32) -> u32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^ (x >> 12)
}

fn build_scene(offset: DVec3) -> Scene {
    let mut s = Scene {
        verts: vec![],
        indices: vec![],
        segs: vec![],
        meshes: vec![],
        params: vec![],
        inst_count: vec![],
        xf: vec![],
        colors: vec![],
    };
    for i in 0..100usize {
        let nu = 16 + (i % 8) * 8;
        let nv = 8 + (i % 5) * 4;
        let torus = i % 2 == 1;
        let (big, small) = (12.0 + (i % 7) as f32, 4.0 + (i % 3) as f32);
        let rad = 15.0 + (i % 6) as f32;
        let point = |u: usize, v: usize| -> ([f32; 3], [f32; 3]) {
            let phi = u as f32 / nu as f32 * std::f32::consts::TAU;
            if torus {
                let th = v as f32 / nv as f32 * std::f32::consts::TAU;
                let n = [th.cos() * phi.cos(), th.cos() * phi.sin(), th.sin()];
                let rr = big + small * th.cos();
                ([rr * phi.cos(), rr * phi.sin(), small * th.sin()], n)
            } else {
                let th = v as f32 / nv as f32 * std::f32::consts::PI;
                let n = [th.sin() * phi.cos(), th.sin() * phi.sin(), th.cos()];
                ([rad * n[0], rad * n[1], rad * n[2]], n)
            }
        };
        let base = s.verts.len() as i32;
        let first = s.indices.len() as u32;
        let row = nu + 1;
        for v in 0..=nv {
            for u in 0..=nu {
                let (p, n) = point(u, v);
                s.verts.push(Vertex { pos: p, nrm: n, face: ((v * 4 / nv).min(3)) as u32, _pad: 0 });
            }
        }
        for v in 0..nv {
            for u in 0..nu {
                let a = (v * row + u) as u32;
                let b = a + 1;
                let c = a + row as u32;
                let d = c + 1;
                // Wind so the geometric normal agrees with the vertex normal.
                let pa = Vec3::from(s.verts[base as usize + a as usize].pos);
                let pc = Vec3::from(s.verts[base as usize + c as usize].pos);
                let pd = Vec3::from(s.verts[base as usize + d as usize].pos);
                let pb = Vec3::from(s.verts[base as usize + b as usize].pos);
                let mut gn = (pc - pa).cross(pd - pa);
                if gn.length_squared() < 1e-12 {
                    gn = (pd - pa).cross(pb - pa);
                }
                let vn = Vec3::from(s.verts[base as usize + a as usize].nrm)
                    + Vec3::from(s.verts[base as usize + d as usize].nrm);
                if gn.dot(vn) >= 0.0 {
                    s.indices.extend_from_slice(&[a, c, d, a, d, b]);
                } else {
                    s.indices.extend_from_slice(&[a, d, c, a, b, d]);
                }
            }
        }
        let seg_base = s.segs.len() as u32;
        let p = |u: usize, v: usize| {
            let q = point(u % nu, v).0;
            [q[0], q[1], q[2], 1.0]
        };
        for v in (0..=nv).step_by(4) {
            for u in 0..nu {
                s.segs.push(Seg { a: p(u, v), b: p(u + 1, v) });
            }
        }
        for u in (0..nu).step_by(4) {
            for v in 0..nv {
                s.segs.push(Seg { a: p(u, v), b: p(u, v + 1) });
            }
        }
        s.meshes.push(MeshRange { first_index: first, index_count: s.indices.len() as u32 - first, base_vertex: base });
        s.params.push(MeshParam {
            seg_base,
            nseg: s.segs.len() as u32 - seg_base,
            inst_base: (i * 100) as u32,
            _pad: 0,
        });
        s.inst_count.push(100);
        // 100 instances per mesh on a 100×100 grid, 60 mm pitch.
        for k in 0..100u32 {
            let id = (i as u32) * 100 + k;
            let h = hash(id);
            let pos = offset + DVec3::new(i as f64 * 60.0, k as f64 * 60.0, (h % 20) as f64);
            let rot = DQuat::from_euler(glam::EulerRot::ZXY, (h % 360) as f64 * PI / 180.0, ((h >> 9) % 90) as f64 * PI / 180.0, 0.0);
            let scale = 0.8 + ((h >> 17) % 40) as f64 / 100.0;
            s.xf.push(DMat4::from_scale_rotation_translation(DVec3::splat(scale), rot, pos));
            let c = hash(id ^ 0xABCD);
            s.colors.push(0xFF00_0000 | (0x60 + (c & 0x7F)) | ((0x60 + ((c >> 8) & 0x7F)) << 8) | ((0x60 + ((c >> 16) & 0x7F)) << 16));
        }
    }
    s
}

/// Instance data for the GPU. `rel` = camera-relative (translation − eye in f64, then f32);
/// otherwise absolute f32 world coordinates.
fn instances(s: &Scene, eye: DVec3, rel: bool) -> Vec<Inst> {
    s.xf
        .iter()
        .zip(&s.colors)
        .enumerate()
        .map(|(i, (m, &color))| {
            let t = if rel { m.w_axis.truncate() - eye } else { m.w_axis.truncate() };
            let r = |row: usize| {
                [m.col(0)[row] as f32, m.col(1)[row] as f32, m.col(2)[row] as f32, t[row] as f32]
            };
            Inst { m: [r(0), r(1), r(2)], color, id: i as u32, _pad: [0; 2] }
        })
        .collect()
}

struct View {
    eye: DVec3,
    target: DVec3,
    width: u32,
    height: u32,
}

impl View {
    fn proj(&self) -> Mat4 {
        let dist = (self.target - self.eye).length() as f32;
        Mat4::perspective_rh(40f32.to_radians(), self.width as f32 / self.height as f32, dist * 0.02, dist * 4.0)
    }
    /// Camera-relative: the view matrix is a pure rotation.
    fn view_proj_rel(&self) -> Mat4 {
        let dir = (self.target - self.eye).normalize().as_vec3(); // look_to_rh needs a unit vector
        self.proj() * Mat4::look_to_rh(Vec3::ZERO, dir, Vec3::Z)
    }
    /// Naive: absolute f32 eye position in the view matrix.
    fn view_proj_abs(&self) -> Mat4 {
        self.proj() * Mat4::look_at_rh(self.eye.as_vec3(), self.target.as_vec3(), Vec3::Z)
    }
}

/// Clip-space crop: maps a PICK×PICK pixel window centred on (px, py) to the full NDC range.
fn pick_crop(view: &View, px: u32, py: u32) -> Mat4 {
    let sx = view.width as f32 / PICK as f32;
    let sy = view.height as f32 / PICK as f32;
    let nx = (px as f32 + 0.5) / view.width as f32 * 2.0 - 1.0;
    let ny = 1.0 - (py as f32 + 0.5) / view.height as f32 * 2.0;
    Mat4::from_cols_array(&[sx, 0.0, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -nx * sx, -ny * sy, 0.0, 1.0])
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    timestamps: bool,
    bgl: wgpu::BindGroupLayout,
    shaded_cull: wgpu::RenderPipeline,
    shaded_nocull: wgpu::RenderPipeline,
    lines: wgpu::RenderPipeline,
    parity: wgpu::RenderPipeline,
    cap: wgpu::RenderPipeline,
    id_mesh: wgpu::RenderPipeline,
    id_lines: wgpu::RenderPipeline,
}

fn vertex_layouts() -> [wgpu::VertexBufferLayout<'static>; 2] {
    const V: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 7 => Uint32];
    const I: [wgpu::VertexAttribute; 5] =
        wgpu::vertex_attr_array![2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Uint32, 6 => Uint32];
    [
        wgpu::VertexBufferLayout { array_stride: 32, step_mode: wgpu::VertexStepMode::Vertex, attributes: &V },
        wgpu::VertexBufferLayout { array_stride: 64, step_mode: wgpu::VertexStepMode::Instance, attributes: &I },
    ]
}

impl Gpu {
    fn new(adapter: &wgpu::Adapter) -> Gpu {
        let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: if timestamps { wgpu::Features::TIMESTAMP_QUERY } else { wgpu::Features::empty() },
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .expect("device");
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1),
                storage(2),
                storage(3),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let vl = vertex_layouts();
        let cap_vl = [wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0 => Float32x3],
        }];
        let depth = |compare, write, stencil: wgpu::StencilState, bias: wgpu::DepthBiasState, fmt| {
            Some(wgpu::DepthStencilState {
                format: fmt,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil,
                bias,
            })
        };
        let no_stencil = wgpu::StencilState::default();
        let invert = wgpu::StencilFaceState {
            compare: wgpu::CompareFunction::Always,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Invert,
        };
        let parity_stencil = wgpu::StencilState { front: invert, back: invert, read_mask: 0xFF, write_mask: 0xFF };
        let nonzero = wgpu::StencilFaceState { compare: wgpu::CompareFunction::NotEqual, ..wgpu::StencilFaceState::IGNORE };
        let cap_stencil = wgpu::StencilState { front: nonzero, back: nonzero, read_mask: 0xFF, write_mask: 0 };
        let line_bias = wgpu::DepthBiasState { constant: -40, slope_scale: -1.0, clamp: 0.0 };
        let pipeline = |vs: &str,
                        fs: &str,
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        cull: Option<wgpu::Face>,
                        ds: Option<wgpu::DepthStencilState>,
                        target: wgpu::TextureFormat,
                        samples: u32,
                        mask: wgpu::ColorWrites| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(fs),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState { cull_mode: cull, ..Default::default() },
                depth_stencil: ds,
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: target, blend: None, write_mask: mask })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_bufs = [Some(vl[0].clone()), Some(vl[1].clone())];
        let none = wgpu::DepthBiasState::default();
        let less = wgpu::CompareFunction::Less;
        let all = wgpu::ColorWrites::ALL;
        Gpu {
            shaded_cull: pipeline("vs_mesh", "fs_shaded", &mesh_bufs, Some(wgpu::Face::Back), depth(less, true, no_stencil.clone(), none, DS), COLOR, MSAA, all),
            shaded_nocull: pipeline("vs_mesh", "fs_shaded", &mesh_bufs, None, depth(less, true, no_stencil.clone(), none, DS), COLOR, MSAA, all),
            lines: pipeline("vs_line", "fs_line", &[], None, depth(wgpu::CompareFunction::LessEqual, true, no_stencil.clone(), line_bias, DS), COLOR, MSAA, all),
            parity: pipeline("vs_mesh", "fs_clip_only", &mesh_bufs, None, depth(wgpu::CompareFunction::Always, false, parity_stencil, none, DS), COLOR, MSAA, wgpu::ColorWrites::empty()),
            cap: pipeline("vs_cap", "fs_cap", &[Some(cap_vl[0].clone())], None, depth(less, true, cap_stencil, none, DS), COLOR, MSAA, all),
            id_mesh: pipeline("vs_mesh", "fs_id", &mesh_bufs, None, depth(less, true, no_stencil.clone(), none, ID_DEPTH), ID, 1, all),
            id_lines: pipeline("vs_line", "fs_line_id", &[], None, depth(wgpu::CompareFunction::LessEqual, true, no_stencil, line_bias, ID_DEPTH), ID, 1, all),
            device,
            queue,
            timestamps,
            bgl,
        }
    }

    fn read(&self, buf: &wgpu::Buffer) -> Vec<u8> {
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.unwrap());
        self.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let v = slice.get_mapped_range().expect("mapped").to_vec();
        buf.unmap();
        v
    }

    fn texture(&self, w: u32, h: u32, format: wgpu::TextureFormat, samples: u32, copy: bool) -> wgpu::TextureView {
        let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
        if copy {
            usage |= wgpu::TextureUsages::COPY_SRC;
        }
        self.device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }
}

/// GPU-resident scene buffers plus one camera uniform.
struct SceneGpu {
    vb: wgpu::Buffer,
    ib: wgpu::Buffer,
    inst: wgpu::Buffer,
    cam: wgpu::Buffer,
    cap: wgpu::Buffer,
    bind: wgpu::BindGroup,
}

impl SceneGpu {
    fn new(g: &Gpu, s: &Scene) -> SceneGpu {
        let d = &g.device;
        let init = |contents: &[u8], usage| {
            d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents, usage })
        };
        let vb = init(bytemuck::cast_slice(&s.verts), wgpu::BufferUsages::VERTEX);
        let ib = init(bytemuck::cast_slice(&s.indices), wgpu::BufferUsages::INDEX);
        let segs = init(bytemuck::cast_slice(&s.segs), wgpu::BufferUsages::STORAGE);
        let params = init(bytemuck::cast_slice(&s.params), wgpu::BufferUsages::STORAGE);
        let inst = d.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (s.xf.len() * 64) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cam = d.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<Cam>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let cap = d.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 6 * 12,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &g.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: cam.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: inst.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: segs.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: params.as_entire_binding() },
            ],
        });
        SceneGpu { vb, ib, inst, cam, cap, bind }
    }

    fn draw_meshes(&self, s: &Scene, pass: &mut wgpu::RenderPass, p: &wgpu::RenderPipeline) {
        pass.set_pipeline(p);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.set_vertex_buffer(0, self.vb.slice(..));
        pass.set_vertex_buffer(1, self.inst.slice(..));
        pass.set_index_buffer(self.ib.slice(..), wgpu::IndexFormat::Uint32);
        for (m, prm) in s.meshes.iter().zip(&s.params) {
            let n = s.inst_count[prm.inst_base as usize / 100];
            pass.draw_indexed(m.first_index..m.first_index + m.index_count, m.base_vertex, prm.inst_base..prm.inst_base + n);
        }
    }

    fn draw_lines(&self, s: &Scene, pass: &mut wgpu::RenderPass, p: &wgpu::RenderPipeline) {
        pass.set_pipeline(p);
        pass.set_bind_group(0, &self.bind, &[]);
        for (i, prm) in s.params.iter().enumerate() {
            let n = s.inst_count[i];
            let v = i as u32 * 6;
            pass.draw(v..v + 6, 0..n * prm.nseg);
        }
    }
}

struct Frame {
    view: View,
    section: Option<(DVec3, f64)>, // world plane: n, d
    line_px: f32,
}

fn cam_uniform(f: &Frame, vp: Mat4, viewport: (f32, f32)) -> Cam {
    let (clip, on) = match f.section {
        Some((n, d)) => {
            let dr = d - n.dot(f.view.eye); // plane in camera-relative space
            ([n.x as f32, n.y as f32, n.z as f32, dr as f32], 1)
        }
        None => ([0.0; 4], 0),
    };
    Cam { view_proj: vp.to_cols_array_2d(), clip, viewport: [viewport.0, viewport.1, f.line_px, 0.0], flags: [on, 0, 0, 0] }
}

/// Section cap quad on the plane x = d, covering the scene, camera-relative.
fn cap_quad(f: &Frame, lo: DVec3, hi: DVec3) -> [[f32; 3]; 6] {
    let (_, d) = f.section.unwrap();
    let e = f.view.eye;
    let c = |y: f64, z: f64| {
        let p = DVec3::new(d, y, z) - e;
        [p.x as f32, p.y as f32, p.z as f32]
    };
    let (a, b, cc, dd) = (c(lo.y, lo.z), c(hi.y, lo.z), c(hi.y, hi.z), c(lo.y, hi.z));
    [a, b, cc, a, cc, dd]
}

fn encode_frame(
    g: &Gpu,
    sg: &SceneGpu,
    s: &Scene,
    f: &Frame,
    color: &wgpu::TextureView,
    resolve: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    ts: Option<(&wgpu::QuerySet, u32)>,
    bounds: (DVec3, DVec3),
) -> wgpu::CommandBuffer {
    let cpu = instances(s, f.view.eye, true);
    g.queue.write_buffer(&sg.inst, 0, bytemuck::cast_slice(&cpu));
    let cam = cam_uniform(f, f.view.view_proj_rel(), (W as f32, H as f32));
    g.queue.write_buffer(&sg.cam, 0, bytemuck::bytes_of(&cam));
    if f.section.is_some() {
        g.queue.write_buffer(&sg.cap, 0, bytemuck::cast_slice(&cap_quad(f, bounds.0, bounds.1)));
    }
    let mut enc = g.device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: Some(resolve),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.82, g: 0.85, b: 0.9, a: 1.0 }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
            }),
            timestamp_writes: ts.map(|(q, i)| wgpu::RenderPassTimestampWrites {
                query_set: q,
                beginning_of_pass_write_index: Some(i),
                end_of_pass_write_index: Some(i + 1),
            }),
            ..Default::default()
        });
        if f.section.is_some() {
            sg.draw_meshes(s, &mut pass, &g.shaded_nocull);
            sg.draw_lines(s, &mut pass, &g.lines);
            sg.draw_meshes(s, &mut pass, &g.parity);
            pass.set_pipeline(&g.cap);
            pass.set_bind_group(0, &sg.bind, &[]);
            pass.set_stencil_reference(0);
            pass.set_vertex_buffer(0, sg.cap.slice(..));
            pass.draw(0..6, 0..1);
        } else {
            sg.draw_meshes(s, &mut pass, &g.shaded_cull);
            sg.draw_lines(s, &mut pass, &g.lines);
        }
    }
    enc.finish()
}

/// Renders an ID image (w×h, or a PICK window when `crop` is set) and reads it back.
fn render_ids(g: &Gpu, sg: &SceneGpu, s: &Scene, f: &Frame, rel: bool, crop: Option<(u32, u32)>, w: u32, h: u32) -> Vec<[u32; 2]> {
    let (tw, th) = if crop.is_some() { (PICK, PICK) } else { (w, h) };
    let cpu = instances(s, f.view.eye, rel);
    g.queue.write_buffer(&sg.inst, 0, bytemuck::cast_slice(&cpu));
    let mut vp = if rel { f.view.view_proj_rel() } else { f.view.view_proj_abs() };
    let mut viewport = (w as f32, h as f32);
    if let Some((px, py)) = crop {
        vp = pick_crop(&f.view, px, py) * vp;
        viewport = (PICK as f32, PICK as f32);
    }
    let mut cam = cam_uniform(f, vp, viewport);
    if !rel {
        cam.flags[0] = 0; // the naive path has no camera-relative clip plane
    }
    g.queue.write_buffer(&sg.cam, 0, bytemuck::bytes_of(&cam));
    let target = g.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ID,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    let depth = g.texture(tw, th, ID_DEPTH, 1, false);
    let row = (tw * 8).div_ceil(256) * 256;
    let out = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * th) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = g.device.create_command_encoder(&Default::default());
    {
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        sg.draw_meshes(s, &mut pass, &g.id_mesh);
        sg.draw_lines(s, &mut pass, &g.id_lines);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: &target, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &out,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(th) },
        },
        wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
    );
    g.queue.submit([enc.finish()]);
    let bytes = g.read(&out);
    let mut ids = Vec::with_capacity((tw * th) as usize);
    for y in 0..th {
        let r = &bytes[(y * row) as usize..(y * row + tw * 8) as usize];
        ids.extend(r.chunks_exact(8).map(|c| [u32::from_le_bytes(c[0..4].try_into().unwrap()), u32::from_le_bytes(c[4..8].try_into().unwrap())]));
    }
    ids
}

#[allow(clippy::too_many_arguments)]
fn save_frame(
    g: &Gpu,
    sg: &SceneGpu,
    s: &Scene,
    f: &Frame,
    color: &wgpu::TextureView,
    resolve_tex: &wgpu::Texture,
    resolve: &wgpu::TextureView,
    depth: &wgpu::TextureView,
    bounds: (DVec3, DVec3),
    path: &str,
) {
    g.queue.submit([encode_frame(g, sg, s, f, color, resolve, depth, None, bounds)]);
    let row = W * 4;
    let buf = g.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * H) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = g.device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: resolve_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(H) },
        },
        wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
    );
    g.queue.submit([enc.finish()]);
    let px = g.read(&buf);
    let file = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(file), W, H);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&px).unwrap();
    println!("  screenshot: {path}");
}

fn orbit(i: u32, offset: DVec3, w: u32, h: u32) -> View {
    let center = offset + DVec3::new(2970.0, 2970.0, 0.0);
    // Stay on the +x side so the section plane (x = centre) faces the camera.
    let a = (i as f64 / FRAMES as f64 - 0.5) * 1.0;
    let eye = center + DVec3::new(a.cos(), a.sin(), 0.8) * 5200.0;
    View { eye, target: center, width: w, height: h }
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

fn bench(adapter: &wgpu::Adapter, shot: Option<&str>) {
    let info = adapter.get_info();
    println!("\n== {} ({:?}, {:?}, driver {})", info.name, info.backend, info.device_type, info.driver_info);
    let t = Instant::now();
    let g = Gpu::new(adapter);
    let s = build_scene(DVec3::ZERO);
    let sg = SceneGpu::new(&g, &s);
    let tris: u64 = s.meshes.iter().map(|m| m.index_count as u64 / 3 * 100).sum();
    let segs: u64 = s.params.iter().map(|p| p.nseg as u64 * 100).sum();
    println!(
        "setup {:.0} ms; {} unique triangles, {} instances, {:.1} M triangles + {:.1} M edge segments per frame",
        t.elapsed().as_secs_f64() * 1e3,
        s.indices.len() / 3,
        s.xf.len(),
        tris as f64 / 1e6,
        segs as f64 / 1e6
    );
    let color = g.texture(W, H, COLOR, MSAA, false);
    let resolve_tex = g.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: W, height: H, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let resolve = resolve_tex.create_view(&Default::default());
    let depth = g.texture(W, H, DS, MSAA, false);
    let lo = DVec3::new(-30.0, -30.0, -30.0);
    let hi = DVec3::new(100.0 * 60.0, 100.0 * 60.0, 60.0);
    let period = g.queue.get_timestamp_period() as f64;

    for section in [false, true] {
        let frame = |i| Frame {
            view: orbit(i, DVec3::ZERO, W, H),
            section: section.then_some((DVec3::X, 2970.0)),
            line_px: 1.5,
        };
        // Warm up.
        for i in 0..10 {
            g.queue.submit([encode_frame(&g, &sg, &s, &frame(i), &color, &resolve, &depth, None, (lo, hi))]);
        }
        g.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let qs = g.timestamps.then(|| {
            g.device.create_query_set(&wgpu::QuerySetDescriptor { label: None, ty: wgpu::QueryType::Timestamp, count: FRAMES * 2 })
        });
        let mut cpu_update = vec![];
        let t = Instant::now();
        for i in 0..FRAMES {
            let tc = Instant::now();
            let cb = encode_frame(&g, &sg, &s, &frame(i), &color, &resolve, &depth, qs.as_ref().map(|q| (q, i * 2)), (lo, hi));
            cpu_update.push(tc.elapsed().as_secs_f64() * 1e3);
            g.queue.submit([cb]);
        }
        g.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let wall = t.elapsed().as_secs_f64() * 1e3 / FRAMES as f64;
        let gpu = qs.map(|q| {
            let resolve_buf = g.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (FRAMES * 2 * 8) as u64,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let read = g.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (FRAMES * 2 * 8) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut enc = g.device.create_command_encoder(&Default::default());
            enc.resolve_query_set(&q, 0..FRAMES * 2, &resolve_buf, 0);
            enc.copy_buffer_to_buffer(&resolve_buf, 0, &read, 0, (FRAMES * 2 * 8) as u64);
            g.queue.submit([enc.finish()]);
            let raw: Vec<u64> = bytemuck::cast_slice(&g.read(&read)).to_vec();
            let mut ms: Vec<f64> = raw.chunks_exact(2).map(|c| c[1].wrapping_sub(c[0]) as f64 * period / 1e6).collect();
            median(&mut ms)
        });
        println!(
            "  {}: GPU {} ms/frame (median), sustained {:.2} ms/frame, CPU encode+instance update {:.2} ms",
            if section { "section + cap   " } else { "shaded + edges  " },
            gpu.map_or("n/a".into(), |v| format!("{v:.2}")),
            wall,
            median(&mut cpu_update)
        );
        if section {
            if let Some(path) = shot {
                save_frame(&g, &sg, &s, &frame(FRAMES - 1), &color, &resolve_tex, &resolve, &depth, (lo, hi), path);
                // Close-up: plane x = 1500 cuts through the 26th column of parts.
                let close = Frame {
                    view: View {
                        eye: DVec3::new(1700.0, 1650.0, 120.0),
                        target: DVec3::new(1500.0, 1500.0, 0.0),
                        width: W,
                        height: H,
                    },
                    section: Some((DVec3::X, 1500.0)),
                    line_px: 1.5,
                };
                save_frame(&g, &sg, &s, &close, &color, &resolve_tex, &resolve, &depth, (lo, hi), &path.replace(".png", "-close.png"));
            }
        }
    }

    // Pick latency: 16×16 window, full ID pass, readback.
    let f = Frame { view: orbit(FRAMES / 2, DVec3::ZERO, W, H), section: None, line_px: 3.0 };
    let mut lat = vec![];
    let mut hits = 0;
    for i in 0..50u32 {
        let (px, py) = (W / 4 + (hash(i) % (W / 2)), H / 4 + (hash(i ^ 77) % (H / 2)));
        let t = Instant::now();
        let ids = render_ids(&g, &sg, &s, &f, true, Some((px, py)), W, H);
        lat.push(t.elapsed().as_secs_f64() * 1e3);
        let centre = ids[(PICK / 2 * PICK + PICK / 2) as usize];
        if centre[0] != 0 {
            hits += 1;
        }
    }
    println!("  pick 16×16 window: median {:.2} ms, max {:.2} ms ({hits}/50 hit an object)", median(&mut lat), lat[lat.len() - 1]);

    // Precision: the same view, far from the origin.
    let (pw, ph) = (640, 360);
    let near = |off: DVec3| {
        let c = off + DVec3::new(1500.0, 1500.0, 0.0);
        View { eye: c + DVec3::new(140.0, 90.0, 70.0), target: c, width: pw, height: ph }
    };
    let far = DVec3::splat(1.0e7);
    let s_far = build_scene(far);
    let sg_far = SceneGpu::new(&g, &s_far);
    for (name, rel) in [("camera-relative f32", true), ("naive absolute f32 ", false)] {
        let a = render_ids(&g, &sg, &s, &Frame { view: near(DVec3::ZERO), section: None, line_px: 1.5 }, rel, None, pw, ph);
        let b = render_ids(&g, &sg_far, &s_far, &Frame { view: near(far), section: None, line_px: 1.5 }, rel, None, pw, ph);
        let diff = a.iter().zip(&b).filter(|(x, y)| x != y).count();
        let covered = a.iter().filter(|x| x[0] != 0).count();
        println!(
            "  precision at 1e7 mm, {name}: {diff} of {} pixels differ ({:.2}%), {covered} covered",
            a.len(),
            diff as f64 * 100.0 / a.len() as f64
        );
    }
}

fn main() {
    if std::env::var("P4_DEBUG").is_ok() {
        let s = build_scene(DVec3::ZERO);
        let v = orbit(FRAMES / 2, DVec3::ZERO, W, H);
        for (name, rel) in [("rel", true), ("abs", false)] {
            let inst = instances(&s, v.eye, rel);
            let vp = if rel { v.view_proj_rel() } else { v.view_proj_abs() };
            for k in [0usize, 5050, 9999] {
                let m = inst[k].m;
                let w = Vec3::new(m[0][3], m[1][3], m[2][3]);
                let c = vp * w.extend(1.0);
                println!("{name} inst {k}: world {w:?} clip {:?} ndc {:?}", c, c.truncate() / c.w);
            }
        }
        return;
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::PRIMARY));
    let shot = std::env::args().nth(1);
    let mut first = true;
    for a in adapters {
        let info = a.get_info();
        if info.device_type == wgpu::DeviceType::Cpu {
            println!("\n(skipping software adapter {} / {:?})", info.name, info.backend);
            continue;
        }
        bench(&a, if first { shot.as_deref() } else { None });
        first = false;
    }
}
