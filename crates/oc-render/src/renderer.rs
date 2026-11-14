//! The renderer: device, pipelines, uploaded meshes and labels, per-body state buffers and
//! render targets. Drawing is in `frame.rs`, picking in `gpu_pick.rs`.
use crate::mesh::MeshGpu;
use crate::pipelines::Pipelines;
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

/// Per scene body: its object uniform and highlight state buffers, reused frame to frame.
pub(crate) struct Slot {
    pub object: wgpu::Buffer,
    pub faces: wgpu::Buffer,
    pub edges: wgpu::Buffer,
    pub face_cap: u64,
    pub edge_cap: u64,
}

pub(crate) struct Targets {
    pub size: (u32, u32),
    pub samples: u32,
    pub msaa: Option<wgpu::TextureView>,
    pub depth: wgpu::TextureView,
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
    pub(crate) labels: HashMap<u32, LabelGpu>,
    next_label: u32,
    pub(crate) slots: Vec<Slot>,
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
    /// `OC_RENDER_FALLBACK=1` asks for the software adapter only.
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
        let wanted = wgpu::Features::DEPTH32FLOAT_STENCIL8 & adapter.features();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("oc-render"),
            required_features: wanted,
            required_limits: wgpu::Limits::default(),
            ..Default::default()
        }))
        .map_err(|e| RenderError::Device(e.to_string()))?;
        Ok(Renderer::new(
            device,
            queue,
            adapter.get_info(),
            wgpu::TextureFormat::Rgba8UnormSrgb,
        ))
    }

    /// A renderer on the caller's device (F2: the Tauri window's surface). `color_format`
    /// is the format of the views passed to [`Renderer::render`]; use an sRGB format.
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
        Renderer {
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
            slots: vec![],
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

    /// Uploads a body's display mesh (every level) as is.
    pub fn add_mesh(&mut self, mesh: Arc<RenderMesh>) -> MeshId {
        let id = MeshId(self.next_mesh);
        self.next_mesh += 1;
        self.meshes.insert(id, MeshGpu::new(&self.device, mesh));
        id
    }

    pub fn remove_mesh(&mut self, id: MeshId) {
        self.meshes.remove(&id);
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
        let tex = |format, samples| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let msaa = (self.samples > 1).then(|| tex(self.color_format, self.samples));
        let depth = tex(self.depth_format, self.samples);
        self.targets = Some(Targets { size: (w, h), samples: self.samples, msaa, depth });
    }

    /// Grows the slot list and each slot's state buffers to fit `n` bodies.
    pub(crate) fn ensure_slot(&mut self, i: usize, faces: u64, edges: u64) {
        let d = &self.device;
        let state = |n: u64| {
            d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("state"),
                size: (n.max(4) * 4).next_multiple_of(16),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        while self.slots.len() <= i {
            self.slots.push(Slot {
                object: d.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("object"),
                    size: std::mem::size_of::<crate::batch::ObjectU>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                faces: state(4),
                edges: state(4),
                face_cap: 4,
                edge_cap: 4,
            });
        }
        let s = &mut self.slots[i];
        if s.face_cap < faces {
            s.faces = state(faces);
            s.face_cap = faces.max(4);
        }
        if s.edge_cap < edges {
            s.edges = state(edges);
            s.edge_cap = edges.max(4);
        }
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
