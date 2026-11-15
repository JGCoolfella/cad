//! The viewport: a wgpu swapchain on the Tauri window, under the transparent webview (P2),
//! drawn by `oc-render` on its own thread.
//!
//! The webview sends a frame (scene, camera, graphics-area rectangle) whenever something
//! changes; the thread draws the newest one into an off-screen texture the size of the
//! graphics area and copies it into the swapchain at the area's position. The rest of the
//! window is cleared to the UI background (the webview paints over it anyway).
//! Rendering is on demand: no frame arrives, nothing is drawn.
use crate::engine::MeshStore;
use crate::scene_dto::{CameraDto, SceneDto};
use oc_render::{LabelId, MeshId, Renderer, Viewport};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// The graphics area in CSS pixels, and `devicePixelRatio`.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

/// An RGBA8 image (sRGB), rows top to bottom.
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub enum ViewMsg {
    Frame { scene: Box<SceneDto>, camera: CameraDto, rect: Rect },
    Resize(u32, u32),
    Label { key: u32, rgba: Vec<u8>, width: u32, height: u32 },
    /// The last viewport image drawn.
    Capture(Sender<Option<Rgba>>),
    /// Adapter name and backend.
    Info(Sender<String>),
}

/// UI background (`--bg` in styles.css), sRGB.
const CLEAR: wgpu::Color = wgpu::Color { r: 240.0 / 255.0, g: 240.0 / 255.0, b: 240.0 / 255.0, a: 1.0 };

/// The backends to try: D3D12 on Windows (WebView2 composites over it most directly, P2 and
/// F1's note), the platform default elsewhere. `OC_BACKEND=vulkan|dx12|metal` overrides.
pub fn backends() -> wgpu::Backends {
    match std::env::var("OC_BACKEND").as_deref() {
        Ok("vulkan") => wgpu::Backends::VULKAN,
        Ok("dx12") => wgpu::Backends::DX12,
        Ok("metal") => wgpu::Backends::METAL,
        _ if cfg!(windows) => wgpu::Backends::DX12,
        _ => wgpu::Backends::PRIMARY,
    }
}

struct State {
    renderer: Renderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    store: Arc<MeshStore>,
    uploaded: HashMap<u32, MeshId>,
    labels: HashMap<u32, LabelId>,
    target: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    last: Option<(Box<SceneDto>, CameraDto, Rect)>,
}

/// Runs the render loop until the sender side is dropped. Instance and surface are made
/// on the main thread by the caller (macOS needs that for the Metal layer).
pub fn run(instance: wgpu::Instance, surface: wgpu::Surface<'static>, size: (u32, u32), store: Arc<MeshStore>, rx: Receiver<ViewMsg>) {
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .expect("no GPU adapter for the viewport");
    let wanted = wgpu::Features::DEPTH32FLOAT_STENCIL8 & adapter.features();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("viewport"),
        required_features: wanted,
        required_limits: wgpu::Limits::default(),
        ..Default::default()
    }))
    .expect("viewport device");
    let caps = surface.get_capabilities(&adapter);
    let mut config = surface.get_default_config(&adapter, size.0.max(1), size.1.max(1)).expect("surface config");
    // Bytes are copied from an sRGB render target into the non-sRGB swapchain unchanged.
    config.format = config.format.remove_srgb_suffix();
    assert!(
        caps.usages.contains(wgpu::TextureUsages::COPY_DST),
        "the viewport surface does not accept copies ({:?})",
        caps.usages
    );
    config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST;
    config.present_mode = wgpu::PresentMode::Fifo;
    config.desired_maximum_frame_latency = 1;
    surface.configure(&device, &config);
    let info = adapter.get_info();
    eprintln!("viewport: {} ({:?}), surface {:?}", info.name, info.backend, config.format);
    let renderer = Renderer::new(device, queue, info, config.format.add_srgb_suffix());
    let mut s = State { renderer, surface, config, store, uploaded: HashMap::new(), labels: HashMap::new(), target: None, last: None };
    while let Ok(m) = rx.recv() {
        let mut dirty = s.handle(m);
        while let Ok(m) = rx.try_recv() {
            dirty |= s.handle(m);
        }
        if dirty {
            s.draw();
        }
    }
}

impl State {
    /// Returns whether a redraw is needed.
    fn handle(&mut self, m: ViewMsg) -> bool {
        match m {
            ViewMsg::Frame { scene, camera, rect } => {
                self.last = Some((scene, camera, rect));
                true
            }
            ViewMsg::Resize(w, h) => {
                if w > 0 && h > 0 && (w, h) != (self.config.width, self.config.height) {
                    self.config.width = w;
                    self.config.height = h;
                    self.surface.configure(self.renderer.device(), &self.config);
                }
                true
            }
            ViewMsg::Label { key, rgba, width, height } => {
                let id = self.renderer.add_label(&rgba, width, height);
                if let Some(old) = self.labels.insert(key, id) {
                    self.renderer.remove_label(old);
                }
                true
            }
            ViewMsg::Capture(reply) => {
                let _ = reply.send(self.capture());
                false
            }
            ViewMsg::Info(reply) => {
                let i = self.renderer.adapter_info();
                let _ = reply.send(format!("{} ({:?})", i.name, i.backend));
                false
            }
        }
    }

    /// Drops meshes the webview no longer holds and uploads the ones the scene needs.
    fn sync_meshes(&mut self, scene: &SceneDto) {
        let keep = |h: &u32| *h > scene.seen || scene.retain.contains(h);
        let gone: Vec<u32> = self.uploaded.keys().copied().filter(|h| !keep(h)).collect();
        for h in gone {
            if let Some(id) = self.uploaded.remove(&h) {
                self.renderer.remove_mesh(id);
            }
        }
        let mut store = self.store.meshes.lock().expect("mesh store");
        store.retain(|h, _| keep(h));
        for b in &scene.bodies {
            if !self.uploaded.contains_key(&b.mesh) {
                if let Some(m) = store.get(&b.mesh) {
                    let id = self.renderer.add_mesh(m.clone());
                    self.uploaded.insert(b.mesh, id);
                }
            }
        }
    }

    fn draw(&mut self) {
        let Some((scene_dto, cam_dto, rect)) = self.last.take() else {
            return self.present(None);
        };
        self.sync_meshes(&scene_dto);
        let scene = scene_dto.to(&self.uploaded, &self.labels);
        let camera = cam_dto.to();
        let vp = Viewport::new((rect.w * rect.scale).round() as u32, (rect.h * rect.scale).round() as u32, rect.scale);
        if self.target.as_ref().is_none_or(|t| t.2 != (vp.width, vp.height)) {
            let tex = self.renderer.device().create_texture(&wgpu::TextureDescriptor {
                label: Some("viewport"),
                size: wgpu::Extent3d { width: vp.width, height: vp.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.config.format.add_srgb_suffix(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            self.target = Some((tex, view, (vp.width, vp.height)));
        }
        let (_, view, _) = self.target.as_ref().expect("target");
        self.renderer.render(&scene, &camera, vp, view);
        self.present(Some(rect));
        self.last = Some((scene_dto, cam_dto, rect));
    }

    fn present(&mut self, rect: Option<Rect>) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.renderer.device(), &self.config);
                return;
            }
            _ => return,
        };
        let d = self.renderer.device();
        let out = frame.texture.create_view(&Default::default());
        let mut enc = d.create_command_encoder(&Default::default());
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &out,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(CLEAR), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        if let (Some(r), Some((tex, _, (w, h)))) = (rect, &self.target) {
            let x = (r.x * r.scale).round().max(0.0) as u32;
            let y = (r.y * r.scale).round().max(0.0) as u32;
            let cw = (*w).min(self.config.width.saturating_sub(x));
            let ch = (*h).min(self.config.height.saturating_sub(y));
            if cw > 0 && ch > 0 {
                enc.copy_texture_to_texture(
                    tex.as_image_copy(),
                    wgpu::TexelCopyTextureInfo { texture: &frame.texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
                    wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
                );
            }
        }
        self.renderer.queue().submit([enc.finish()]);
        self.renderer.queue().present(frame);
    }

    fn capture(&self) -> Option<Rgba> {
        let (tex, _, (w, h)) = self.target.as_ref()?;
        let d = self.renderer.device();
        let row = (w * 4).div_ceil(256) * 256;
        let buf = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = d.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            tex.as_image_copy(),
            wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(*h) } },
            wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
        );
        self.renderer.queue().submit([enc.finish()]);
        let slice = buf.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        d.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let mapped = slice.get_mapped_range().ok()?;
        let bgra = matches!(self.config.format, wgpu::TextureFormat::Bgra8Unorm);
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..*h {
            let line = &mapped[(y * row) as usize..(y * row + w * 4) as usize];
            for p in line.chunks_exact(4) {
                data.extend_from_slice(&if bgra { [p[2], p[1], p[0], p[3]] } else { [p[0], p[1], p[2], p[3]] });
            }
        }
        drop(mapped);
        buf.unmap();
        Some(Rgba { width: *w, height: *h, data })
    }
}
