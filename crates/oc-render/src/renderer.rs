//! The renderer: device, pipelines, uploaded meshes (the geometry pool) and labels, the
//! GPU scene and render targets. Drawing is in `frame.rs`, culling in `cull.rs`, picking
//! in `gpu_pick.rs`.
use crate::cull::{Compute, Hiz, PendingStats};
#[cfg(not(target_arch = "wasm32"))]
use crate::cull::DrawStats;
use crate::gpu_scene::GpuScene;
use crate::mesh::MeshGpu;
use crate::pipelines::Pipelines;
use crate::pool::Pool;
use crate::scene::{LabelId, MeshId};
use oc_mesh::RenderMesh;
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Size of the drawing surface in physical pixels and the logical-to-physical scale
/// (`devicePixelRatio`). Sizes the app gives in pixels (line widths, the pick window,
/// the triad) are logical.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

impl Viewport {
    pub fn new(width: u32, height: u32, scale: f64) -> Viewport {
        Viewport {
            width: width.max(1),
            height: height.max(1),
            scale: if scale > 0.0 { scale } else { 1.0 },
        }
    }
    pub fn logical(&self) -> (f64, f64) {
        (self.width as f64 / self.scale, self.height as f64 / self.scale)
    }
}

/// An RGBA8 (sRGB) image, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }
}

#[derive(Debug)]
pub enum RenderError {
    NoAdapter(String),
    Device(String),
}

impl std::fmt::Display for RenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RenderError::NoAdapter(e) => write!(f, "no GPU adapter: {e}"),
            RenderError::Device(e) => write!(f, "GPU device: {e}"),
        }
    }
}

impl std::error::Error for RenderError {}

pub(crate) struct LabelGpu {
    pub view: wgpu::TextureView,
    pub bind: wgpu::BindGroup,
    pub aspect: f64,
}

pub(crate) struct Targets {
    pub size: (u32, u32),
    pub samples: u32,
    pub msaa: Option<wgpu::TextureView>,
    pub depth: wgpu::TextureView,
    /// The depth aspect, for building the Hi-Z.
    pub depth_sampled: wgpu::TextureView,
    pub hiz: Hiz,
}

/// How part bodies are culled (G2). The default culls everything it can; turning parts off
/// changes speed only, never the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Culling {
    /// Draw lists built on the GPU and drawn with indirect draws (needs
    /// `INDIRECT_FIRST_INSTANCE`); false: one draw per body, nothing culled.
    pub gpu: bool,
    /// Leave out instances outside the view (or cut away by the section).
    pub frustum: bool,
    /// Leave out instances hidden behind others (two-pass Hi-Z; needs `frustum`).
    pub occlusion: bool,
}

impl Default for Culling {
    fn default() -> Self {
        Culling { gpu: true, frustum: true, occlusion: true }
    }
}

pub struct Renderer {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) info: wgpu::AdapterInfo,
    pub(crate) color_format: wgpu::TextureFormat,
    pub(crate) depth_format: wgpu::TextureFormat,
    pub(crate) pipes: Pipelines,
    pub(crate) samples: u32,
    pub(crate) meshes: HashMap<MeshId, MeshGpu>,
    next_mesh: u32,
    /// Uploaded meshes by their `Arc`'s address: adding the same mesh again shares it.
    by_ptr: HashMap<usize, MeshId>,
    pub(crate) pool: Pool,
    pub(crate) gs: GpuScene,
    pub(crate) compute: Compute,
    pub(crate) culling: Culling,
    /// The device can start indirect draws at an instance (`INDIRECT_FIRST_INSTANCE`).
    first_instance: bool,
    pub(crate) labels: HashMap<u32, LabelGpu>,
    next_label: u32,
    pub(crate) frame_buf: wgpu::Buffer,
    pub(crate) pick_frame_buf: wgpu::Buffer,
    pub(crate) targets: Option<Targets>,
    pub(crate) pick_targets: (wgpu::Texture, wgpu::TextureView, wgpu::TextureView),
}

/// The depth format: 32-bit float with stencil where the adapter has it (reverse Z, plan
/// section 1), else 24-bit with stencil.
fn depth_format(features: wgpu::Features) -> wgpu::TextureFormat {
    if features.contains(wgpu::Features::DEPTH32FLOAT_STENCIL8) {
        wgpu::TextureFormat::Depth32FloatStencil8
    } else {
        wgpu::TextureFormat::Depth24PlusStencil8
    }
}

impl Renderer {
    /// A renderer on its own device, for off-screen rendering (tests, thumbnails). Tries a
    /// high-performance hardware adapter, then the platform's software adapter (WARP on
    /// Windows). `WGPU_BACKEND` (e.g. `dx12`, `vulkan`) restricts the backends;
    /// `OC_RENDER_FALLBACK=1` asks for the software adapter only. Not in the browser, which
    /// never blocks on its GPU: [`Renderer::on_adapter`] there.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn headless() -> Result<Renderer, RenderError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let request = |fallback| {
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: fallback,
                compatible_surface: None,
                ..Default::default()
            }))
        };
        let software = std::env::var("OC_RENDER_FALLBACK").is_ok_and(|v| v == "1");
        let adapter = request(software)
            .or_else(|_| request(true))
            .map_err(|e| RenderError::NoAdapter(e.to_string()))?;
        pollster::block_on(Renderer::on_adapter(&adapter, wgpu::TextureFormat::Rgba8UnormSrgb))
    }

    /// A renderer on a new device of `adapter`, with the [`Renderer::features`] it has and
    /// the [`Renderer::limits`] (W1c: the browser's devices are made asynchronously; the
    /// optional features fall back as [`Renderer::new`] says). `color_format` as there.
    pub async fn on_adapter(adapter: &wgpu::Adapter, color_format: wgpu::TextureFormat) -> Result<Renderer, RenderError> {
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("oc-render"),
                required_features: Renderer::features(adapter.features()),
                required_limits: Renderer::limits(&adapter.limits()),
                ..Default::default()
            })
            .await
            .map_err(|e| RenderError::Device(e.to_string()))?;
        Ok(Renderer::new(device, queue, adapter.get_info(), color_format))
    }

    /// The device features the renderer uses, of those `available`: request them when
    /// making the device passed to [`Renderer::new`].
    pub fn features(available: wgpu::Features) -> wgpu::Features {
        (wgpu::Features::DEPTH32FLOAT_STENCIL8 | wgpu::Features::INDIRECT_FIRST_INSTANCE) & available
    }

    /// The device limits to request: the defaults, with buffers as large as the adapter
    /// allows (the geometry pool holds every mesh).
    pub fn limits(adapter: &wgpu::Limits) -> wgpu::Limits {
        let d = wgpu::Limits::default();
        wgpu::Limits {
            max_buffer_size: adapter.max_buffer_size.max(d.max_buffer_size),
            max_storage_buffer_binding_size: adapter.max_storage_buffer_binding_size.max(d.max_storage_buffer_binding_size),
            ..d
        }
    }

    /// A renderer on the caller's device (F2: the Tauri window's surface). `color_format`
    /// is the format of the views passed to [`Renderer::render`]; use an sRGB format.
    /// Request [`Renderer::features`] and [`Renderer::limits`] for the device. Without
    /// `DEPTH32FLOAT_STENCIL8` depth is 24-bit; without `INDIRECT_FIRST_INSTANCE` bodies are
    /// drawn one by one, nothing culled ([`Renderer::culling`]): the same image either way.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        info: wgpu::AdapterInfo,
        color_format: wgpu::TextureFormat,
    ) -> Renderer {
        let depth_format = depth_format(device.features());
        let samples = 4;
        let pipes = Pipelines::new(&device, color_format, depth_format, samples);
        let uniform = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<crate::batch::FrameU>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let (frame_buf, pick_frame_buf) = (uniform("frame"), uniform("pick frame"));
        let pick_targets = crate::gpu_pick::targets(&device);
        let first_instance = device.features().contains(wgpu::Features::INDIRECT_FIRST_INSTANCE);
        Renderer {
            by_ptr: HashMap::new(),
            pool: Pool::new(&device),
            gs: GpuScene::new(&device),
            compute: Compute::new(&device),
            culling: Culling::default(),
            first_instance,
            device,
            queue,
            info,
            color_format,
            depth_format,
            pipes,
            samples,
            meshes: HashMap::new(),
            next_mesh: 0,
            labels: HashMap::new(),
            next_label: 0,
            frame_buf,
            pick_frame_buf,
            targets: None,
            pick_targets,
        }
    }

    pub fn adapter_info(&self) -> &wgpu::AdapterInfo {
        &self.info
    }
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }
    pub fn depth_format(&self) -> wgpu::TextureFormat {
        self.depth_format
    }

    /// Uploads a body's display mesh (every level) as is. The same `Arc` added again is
    /// not uploaded again: it returns the same id, and the mesh stays until every add is
    /// matched by a [`Renderer::remove_mesh`] (an assembly rebuilt with unchanged parts
    /// hands out new handles for the same meshes, G2). A mesh that does not fit in the
    /// device's buffers gets an id but is not drawn.
    pub fn add_mesh(&mut self, mesh: Arc<RenderMesh>) -> MeshId {
        let key = Arc::as_ptr(&mesh) as usize;
        if let Some(id) = self.by_ptr.get(&key) {
            if let Some(m) = self.meshes.get_mut(id) {
                m.refs += 1;
                return *id;
            }
        }
        let id = MeshId(self.next_mesh);
        self.next_mesh += 1;
        match MeshGpu::new(&mut self.pool, &self.device, &self.queue, mesh) {
            Some(m) => {
                self.meshes.insert(id, m);
                self.by_ptr.insert(key, id);
            }
            None => eprintln!("oc-render: mesh {id:?} does not fit in the GPU's buffers; not drawn"),
        }
        id
    }

    pub fn remove_mesh(&mut self, id: MeshId) {
        let Some(m) = self.meshes.get_mut(&id) else { return };
        m.refs -= 1;
        if m.refs > 0 {
            return;
        }
        let m = self.meshes.remove(&id).expect("mesh");
        self.by_ptr.remove(&(Arc::as_ptr(&m.cpu) as usize));
        m.release(&mut self.pool);
        let (dead, used) = self.pool.waste();
        if dead > 16 << 20 && dead * 2 > used {
            self.compact_pool();
        }
    }

    /// Re-uploads the live meshes into a fresh pool (dropping removed meshes' space).
    fn compact_pool(&mut self) {
        let generation = self.pool.generation;
        self.pool = Pool::new(&self.device);
        self.pool.generation = generation + 1;
        let mut ids: Vec<MeshId> = self.meshes.keys().copied().collect();
        ids.sort();
        for id in ids {
            let old = &self.meshes[&id];
            let (cpu, refs) = (old.cpu.clone(), old.refs);
            match MeshGpu::new(&mut self.pool, &self.device, &self.queue, cpu) {
                Some(mut m) => {
                    m.refs = refs;
                    self.meshes.insert(id, m);
                }
                None => {
                    let m = self.meshes.remove(&id).expect("mesh");
                    self.by_ptr.remove(&(Arc::as_ptr(&m.cpu) as usize));
                }
            }
        }
    }

    /// Meshes uploaded (distinct `Arc`s).
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Bytes of the geometry pool in use (live and removed meshes).
    pub fn pool_bytes(&self) -> u64 {
        self.pool.waste().1
    }

    /// How part bodies are culled; [`Culling::gpu`] reads false on a device without
    /// `INDIRECT_FIRST_INSTANCE`.
    pub fn culling(&self) -> Culling {
        let mut c = self.culling;
        c.gpu &= self.first_instance;
        c.frustum &= c.gpu;
        c.occlusion &= c.frustum;
        c
    }

    pub fn set_culling(&mut self, c: Culling) {
        self.culling = c;
    }

    /// What the last frame (and the last pick) drew through the GPU lists. Blocks until
    /// the GPU is idle; for tests and benchmarks.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn draw_stats(&self) -> DrawStats {
        crate::cull::read_stats(&self.device, &self.queue, &self.gs)
    }

    /// [`Renderer::draw_stats`] without waiting: [`PendingStats::poll`] has them once the
    /// GPU is done.
    pub fn draw_stats_async(&self) -> PendingStats {
        crate::cull::stats_async(&self.device, &self.queue, &self.gs)
    }

    pub fn mesh(&self, id: MeshId) -> Option<&Arc<RenderMesh>> {
        self.meshes.get(&id).map(|m| &m.cpu)
    }

    /// Uploads a plane name image: RGBA8, sRGB, straight alpha, rows top to bottom. The
    /// caller rasterises the text (the current viewer: Century Gothic-style, light blue
    /// `#3f86ec`, on a transparent background).
    pub fn add_label(&mut self, rgba: &[u8], width: u32, height: u32) -> LabelId {
        let (w, h) = (width.max(1), height.max(1));
        let mips = mip_chain(rgba, w, h);
        let tex = self.device.create_texture_with_data(
            &self.queue,
            &wgpu::TextureDescriptor {
                label: Some("label"),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: mips.len() as u32,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::MipMajor,
            &mips.concat(),
        );
        let view = tex.create_view(&Default::default());
        let bind = self.label_bind(&view);
        let id = self.next_label;
        self.next_label += 1;
        self.labels.insert(id, LabelGpu { view, bind, aspect: w as f64 / h as f64 });
        LabelId(id)
    }

    fn label_bind(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("label"),
            layout: &self.pipes.label_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.pipes.sampler) },
            ],
        })
    }

    pub fn remove_label(&mut self, id: LabelId) {
        self.labels.remove(&id.0);
    }

    pub(crate) fn set_samples(&mut self, samples: u32) {
        let s = if samples > 1 { 4 } else { 1 };
        if s != self.samples {
            self.samples = s;
            self.pipes = Pipelines::new(&self.device, self.color_format, self.depth_format, s);
            // Bind groups belong to the old layouts.
            let ids: Vec<u32> = self.labels.keys().copied().collect();
            for id in ids {
                let bind = self.label_bind(&self.labels[&id].view);
                self.labels.get_mut(&id).expect("label").bind = bind;
            }
            self.targets = None;
        }
    }

    /// Render targets (multisampled colour, depth-stencil) for a size.
    pub(crate) fn ensure_targets(&mut self, w: u32, h: u32) {
        let ok = self
            .targets
            .as_ref()
            .is_some_and(|t| t.size == (w, h) && t.samples == self.samples);
        if ok {
            return;
        }
        let tex = |format, samples, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let msaa = (self.samples > 1).then(|| tex(self.color_format, self.samples, rt).create_view(&Default::default()));
        let depth_tex = tex(self.depth_format, self.samples, rt | wgpu::TextureUsages::TEXTURE_BINDING);
        let depth = depth_tex.create_view(&Default::default());
        let depth_sampled = depth_tex.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() });
        let hiz = Hiz::new(&self.device, w, h);
        self.targets = Some(Targets { size: (w, h), samples: self.samples, msaa, depth, depth_sampled, hiz });
    }

    pub(crate) fn storage(&self, label: &str, bytes: &[u8]) -> wgpu::Buffer {
        let mut v = bytes.to_vec();
        if v.len() < 16 {
            v.resize(16, 0);
        }
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: &v,
            usage: wgpu::BufferUsages::STORAGE,
        })
    }

    pub(crate) fn vertex_buffer(&self, bytes: &[u8]) -> Option<wgpu::Buffer> {
        (!bytes.is_empty()).then(|| {
            self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: wgpu::BufferUsages::VERTEX,
            })
        })
    }

    /// Reads a texture-to-buffer copy back (blocking).
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn read_buffer(&self, buf: &wgpu::Buffer) -> Vec<u8> {
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map"));
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
        let v = slice.get_mapped_range().expect("mapped").to_vec();
        buf.unmap();
        v
    }
}

/// Box-filtered mip levels of an RGBA8 image (level 0 first).
fn mip_chain(rgba: &[u8], w: u32, h: u32) -> Vec<Vec<u8>> {
    let mut levels = vec![{
        let mut v = rgba.to_vec();
        v.resize((w * h * 4) as usize, 0);
        v
    }];
    let (mut cw, mut ch) = (w, h);
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let src = levels.last().expect("level");
        let mut dst = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (2 * x + dx).min(cw - 1);
                        let sy = (2 * y + dy).min(ch - 1);
                        sum += src[((sy * cw + sx) * 4 + c) as usize] as u32;
                    }
                    dst[((y * nw + x) * 4 + c) as usize] = (sum / 4) as u8;
                }
            }
        }
        levels.push(dst);
        (cw, ch) = (nw, nh);
    }
    levels
}
