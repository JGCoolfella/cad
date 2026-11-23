//! The viewport: a wgpu swapchain on the Tauri window, under the transparent webview (P2),
//! drawn by `oc-render` on its own thread.
//!
//! The webview sends a frame (scene, camera, graphics-area rectangle) whenever something
//! changes; the thread draws the newest one into an off-screen texture the size of the
//! graphics area ([`oc_view::View`], shared with the browser's render worker, W1c) and
//! copies it into the swapchain at the area's position. The rest of the window is cleared
//! to the UI background (the webview paints over it anyway). Rendering is on demand: no
//! frame arrives, nothing is drawn.
//!
//! Picks and box selections ([`oc_view::pick`]) are answered here too, against the scene of
//! the frame they name; hover picks run on the GPU while the thread goes on.
use crate::engine::MeshStore;
use crate::pick::{BoxReply, BoxReq, PickReply, PickReq};
use crate::scene_dto::{CameraDto, SceneDto};
use oc_render::Renderer;
use oc_view::{Hooks, View};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

pub use oc_view::{FrameStats, Rect, Rgba};

impl ViewMsg {
    /// The message's name (the stuck-state dump of check runs, `crate::diag`).
    pub fn name(&self) -> &'static str {
        match self {
            ViewMsg::Frame { .. } => "Frame",
            ViewMsg::Resize(..) => "Resize",
            ViewMsg::Label { .. } => "Label",
            ViewMsg::Capture(_) => "Capture",
            ViewMsg::Info(_) => "Info",
            ViewMsg::Pick(..) => "Pick",
            ViewMsg::Box(..) => "Box",
            ViewMsg::Stats(_) => "Stats",
        }
    }
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
    view: View,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    store: Arc<MeshStore>,
}

/// Runs the render loop until the sender side is dropped. Instance and surface are made
/// on the main thread by the caller (macOS needs that for the Metal layer).
pub fn run(instance: wgpu::Instance, surface: wgpu::Surface<'static>, size: (u32, u32), store: Arc<MeshStore>, rx: Receiver<ViewMsg>) {
    crate::diag::view("request_adapter");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
        ..Default::default()
    }))
    .expect("no GPU adapter for the viewport");
    // oc-render's features (indirect draws from an instance, G2) and pool-sized buffers.
    crate::diag::view("request_device");
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
    crate::diag::event(format!("viewport: {} ({:?})", info.name, info.backend));
    // Its pipelines: every shader compiled (seconds on a loaded machine).
    crate::diag::view("Renderer::new");
    let renderer = Renderer::new(device, queue, info, config.format.add_srgb_suffix());
    crate::diag::event("viewport: renderer ready");
    let hooks = Hooks { phase: crate::diag::view, drawn: crate::diag::view_drawn };
    let view = View::new(renderer, config.format.add_srgb_suffix(), hooks);
    let mut s = State { view, surface, config, store };
    loop {
        crate::diag::view_loop(s.view.waiting() + s.view.pending());
        crate::diag::view("wait");
        // Block while idle. While a hover pick is on the GPU (well under a millisecond) spin:
        // a timed wait sleeps at least one Windows timer tick (15.6 ms), which made hover
        // answers that slow. While a query only waits for its frame, a frame wakes the wait.
        let first = if s.view.pending() > 0 {
            match rx.try_recv() {
                Ok(m) => Some(m),
                Err(TryRecvError::Empty) => {
                    std::thread::yield_now();
                    None
                }
                Err(TryRecvError::Disconnected) => break,
            }
        } else if s.view.waiting() > 0 {
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
            crate::diag::view("handle");
            dirty = s.handle(m);
            while let Ok(m) = rx.try_recv() {
                dirty |= s.handle(m);
            }
        }
        if dirty {
            crate::diag::view("prepare");
            s.view.prepare(&s.store.meshes);
        }
        // Answer picks before presenting (which may wait for the display).
        crate::diag::view("serve");
        s.view.serve();
        if dirty {
            crate::diag::view("draw");
            let rect = s.view.draw();
            s.present(rect);
        }
        crate::diag::view("poll");
        s.view.poll_pending();
    }
}

/// A reply sent back to the IPC command waiting for it.
fn to<T: 'static>(tx: Sender<T>) -> oc_view::Reply<T> {
    Box::new(move |r| {
        let _ = tx.send(r);
    })
}

impl State {
    /// Returns whether a redraw is needed.
    fn handle(&mut self, m: ViewMsg) -> bool {
        match m {
            ViewMsg::Frame { scene, camera, rect } => {
                self.view.frame(scene, camera, rect);
                true
            }
            ViewMsg::Resize(w, h) => {
                if w > 0 && h > 0 && (w, h) != (self.config.width, self.config.height) {
                    self.config.width = w;
                    self.config.height = h;
                    self.surface.configure(self.view.renderer().device(), &self.config);
                }
                true
            }
            ViewMsg::Label { key, rgba, width, height } => {
                self.view.label(key, &rgba, width, height);
                true
            }
            ViewMsg::Capture(reply) => {
                crate::diag::view("capture");
                let _ = reply.send(self.view.capture());
                false
            }
            ViewMsg::Info(reply) => {
                let _ = reply.send(self.view.info());
                false
            }
            ViewMsg::Pick(q, reply) => {
                self.view.pick(q, to(reply));
                false
            }
            ViewMsg::Box(q, reply) => {
                self.view.box_select(q, to(reply));
                false
            }
            ViewMsg::Stats(reply) => {
                let _ = reply.send(self.view.stats());
                false
            }
        }
    }

    fn present(&mut self, rect: Option<Rect>) {
        crate::diag::view("get_current_texture");
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(self.view.renderer().device(), &self.config);
                return;
            }
            _ => return,
        };
        let r = self.view.renderer();
        let out = frame.texture.create_view(&Default::default());
        let mut enc = r.device().create_command_encoder(&Default::default());
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &out,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(CLEAR), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        if let (Some(rc), Some((tex, (w, h)))) = (rect, self.view.target()) {
            let x = (rc.x * rc.scale).round().max(0.0) as u32;
            let y = (rc.y * rc.scale).round().max(0.0) as u32;
            let cw = w.min(self.config.width.saturating_sub(x));
            let ch = h.min(self.config.height.saturating_sub(y));
            if cw > 0 && ch > 0 {
                enc.copy_texture_to_texture(
                    tex.as_image_copy(),
                    wgpu::TexelCopyTextureInfo { texture: &frame.texture, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
                    wgpu::Extent3d { width: cw, height: ch, depth_or_array_layers: 1 },
                );
            }
        }
        r.queue().submit([enc.finish()]);
        crate::diag::view("present");
        r.queue().present(frame);
    }
}
