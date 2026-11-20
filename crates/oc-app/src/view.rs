//! The viewport: a wgpu swapchain on the Tauri window, under the transparent webview (P2),
//! drawn by `oc-render` on its own thread.
//!
//! The webview sends a frame (scene, camera, graphics-area rectangle) whenever something
//! changes; the thread draws the newest one into an off-screen texture the size of the
//! graphics area and copies it into the swapchain at the area's position. The rest of the
//! window is cleared to the UI background (the webview paints over it anyway).
//! Rendering is on demand: no frame arrives, nothing is drawn.
//!
//! Picks and box selections ([`crate::pick`]) are answered here too, against the scene of
//! the frame they name; hover picks run on the GPU while the thread goes on.
use crate::engine::MeshStore;
use crate::pick::{self, Answer, BoxReply, BoxReq, PickReply, PickReq};
use crate::scene_dto::{CameraDto, SceneDto};
use oc_render::{LabelId, MeshId, PendingPick, Renderer, Scene, Viewport};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The graphics area in CSS pixels, and `devicePixelRatio`.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

impl Rect {
    /// The graphics area in physical pixels.
    pub fn viewport(&self) -> Viewport {
        Viewport::new((self.w * self.scale).round() as u32, (self.h * self.scale).round() as u32, self.scale)
    }
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
    Pick(Box<PickReq>, Sender<PickReply>),
    Box(Box<BoxReq>, Sender<BoxReply>),
    /// What the last frame drew (scripts). The first request turns the counting on: from
    /// then on each frame reads its draw counts back from the GPU.
    Stats(Sender<Option<FrameStats>>),
}

/// What a frame drew (G2b: the native scripts check the levels of detail during and after
/// a turn of the view).
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct FrameStats {
    /// The frame's number (`SceneDto::seq`).
    pub seq: u32,
    /// The frame said the view was moving.
    pub moving: bool,
    /// Its Level of detail slider (0: Off or not applicable).
    pub lod: u32,
    /// Part instances with opaque faces drawn, per level of detail (0 the finest).
    pub levels: Vec<u32>,
    /// Part instances in the GPU lists.
    pub instances: u32,
    /// The render thread's time for the frame (G3a, scripts): the scene converted, meshes
    /// uploaded, the frame encoded and the GPU done (the counts read back), before presenting.
    #[serde(rename = "renderMs")]
    pub render_ms: f64,
}

/// A pick or box selection waiting for its frame.
enum Query {
    Pick(Box<PickReq>, Sender<PickReply>),
    Box(Box<BoxReq>, Sender<BoxReply>),
}

impl Query {
    fn seq(&self) -> u32 {
        match self {
            Query::Pick(q, _) => q.seq,
            Query::Box(q, _) => q.seq,
        }
    }
}

/// How long a query waits for the frame it names before the newest frame answers it (the
/// webview coalesces frames, but one always follows a pick's).
const FRAME_WAIT: Duration = Duration::from_millis(250);

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
    /// The newest frame's scene, converted: its `seq` and the scene.
    current: Option<(u32, Scene)>,
    /// The newest frame's scene as picks see it, when some body is drawn away from where it
    /// is picked (a mate animation, G1d).
    pick_scene: Option<Scene>,
    waiting: Vec<(Instant, Query)>,
    /// Hover picks on the GPU.
    pending: Vec<(PendingPick, PickReply, Sender<PickReply>)>,
    /// Draw counts are read back after each frame (a script asked for them).
    count: bool,
    stats: Option<FrameStats>,
    /// When the frame being drawn started (prepared).
    started: Instant,
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
    // oc-render's features (indirect draws from an instance, G2) and pool-sized buffers.
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("viewport"),
        required_features: Renderer::features(adapter.features()),
        required_limits: Renderer::limits(&adapter.limits()),
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
    let mut s = State {
        renderer,
        surface,
        config,
        store,
        uploaded: HashMap::new(),
        labels: HashMap::new(),
        target: None,
        last: None,
        current: None,
        pick_scene: None,
        waiting: vec![],
        pending: vec![],
        count: false,
        stats: None,
        started: Instant::now(),
    };
    loop {
        // Block while idle. While a hover pick is on the GPU (well under a millisecond) spin:
        // a timed wait sleeps at least one Windows timer tick (15.6 ms), which made hover
        // answers that slow. While a query only waits for its frame, a frame wakes the wait.
        let first = if !s.pending.is_empty() {
            match rx.try_recv() {
                Ok(m) => Some(m),
                Err(TryRecvError::Empty) => {
                    std::thread::yield_now();
                    None
                }
                Err(TryRecvError::Disconnected) => break,
            }
        } else if !s.waiting.is_empty() {
            match rx.recv_timeout(Duration::from_millis(1)) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => break,
            }
        };
        let mut dirty = false;
        if let Some(m) = first {
            dirty = s.handle(m);
            while let Ok(m) = rx.try_recv() {
                dirty |= s.handle(m);
            }
        }
        if dirty {
            s.started = Instant::now();
            s.prepare();
        }
        // Answer picks before presenting (which may wait for the display).
        s.serve();
        if dirty {
            s.draw();
        }
        s.poll_pending();
    }
}

impl State {
    /// Returns whether a redraw is needed.
    fn handle(&mut self, m: ViewMsg) -> bool {
        match m {
            ViewMsg::Frame { mut scene, camera, rect } => {
                scene.keep_bodies(self.last.as_mut().map(|l| &mut *l.0));
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
            ViewMsg::Pick(q, reply) => {
                self.waiting.push((Instant::now(), Query::Pick(q, reply)));
                false
            }
            ViewMsg::Box(q, reply) => {
                self.waiting.push((Instant::now(), Query::Box(q, reply)));
                false
            }
            ViewMsg::Stats(reply) => {
                self.count = true;
                let _ = reply.send(self.stats.clone());
                false
            }
        }
    }

    /// Answers the queries whose frame has arrived (or that waited too long).
    fn serve(&mut self) {
        let have = self.current.as_ref().map(|c| c.0);
        let waiting = std::mem::take(&mut self.waiting);
        for (t, q) in waiting {
            let ready = match have {
                Some(seq) => seq >= q.seq() || t.elapsed() > FRAME_WAIT,
                None => q.seq() == 0 || t.elapsed() > FRAME_WAIT,
            };
            if !ready {
                self.waiting.push((t, q));
                continue;
            }
            let empty = Scene::default();
            let (seq, scene) = self.current.as_ref().map_or((0, &empty), |(n, s)| (*n, s));
            let scene = self.pick_scene.as_ref().unwrap_or(scene);
            match q {
                Query::Pick(q, reply) => match pick::pick(&mut self.renderer, scene, seq, &q) {
                    Answer::Now(r) => {
                        let _ = reply.send(r);
                    }
                    Answer::Later(p, r) => self.pending.push((p, r, reply)),
                },
                Query::Box(q, reply) => {
                    let _ = reply.send(pick::box_select(&self.renderer, scene, seq, &q));
                }
            }
        }
    }

    /// Sends the hover picks the GPU has finished.
    fn poll_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for (mut p, mut r, reply) in pending {
            match p.poll(&self.renderer) {
                Some(hit) => {
                    r.hit = hit.map(Into::into);
                    let _ = reply.send(r);
                }
                None => self.pending.push((p, r, reply)),
            }
        }
    }

    /// Drops meshes the webview no longer holds and uploads the ones the scene needs. An
    /// assembly build hands out new handles for unchanged parts; their meshes are the same
    /// `Arc`s, which the renderer does not upload again (G2).
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
        for b in scene.bodies() {
            if !self.uploaded.contains_key(&b.mesh) {
                if let Some(m) = store.get(&b.mesh) {
                    let id = self.renderer.add_mesh(m.clone());
                    self.uploaded.insert(b.mesh, id);
                }
            }
        }
    }

    /// Uploads the newest frame's meshes and converts its scene (for drawing and picks).
    fn prepare(&mut self) {
        let Some(last) = self.last.take() else { return };
        self.sync_meshes(&last.0);
        let scene = last.0.to(&self.uploaded, &self.labels);
        self.pick_scene = last.0.pick_scene(&scene);
        self.current = Some((last.0.seq, scene));
        self.last = Some(last);
    }

    fn draw(&mut self) {
        let (Some((dto, cam_dto, rect)), Some((seq, scene))) = (&self.last, &self.current) else {
            return self.present(None);
        };
        let (seq, moving, lod) = (*seq, dto.moving, dto.lod);
        let (camera, rect) = (cam_dto.to(), *rect);
        let vp = rect.viewport();
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
        self.renderer.render(scene, &camera, vp, view);
        if self.count {
            let st = self.renderer.draw_stats();
            self.stats = Some(FrameStats { seq, moving, lod, levels: st.opaque_levels, instances: st.instances, render_ms: self.started.elapsed().as_secs_f64() * 1e3 });
        }
        self.present(Some(rect));
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
