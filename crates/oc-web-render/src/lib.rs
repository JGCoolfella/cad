//! `oc-web-render`: the browser's viewport (`docs/OVERHAUL_PLAN.md` W1c, "W1 design" point 4).
//!
//! A wasm-bindgen module (`wasm32-unknown-unknown`, wgpu's WebGPU backend) run by the render
//! worker (`src/web/render.worker.ts`). It draws `oc-render` into an `OffscreenCanvas` that
//! lies under the transparent UI, as the desktop app's render thread draws into the window's
//! swapchain under the webview: the same frames from the UI (`NativeView`, unchanged), the
//! same frame logic ([`oc_view::View`]), the same picks. Meshes come from the engine worker
//! as `RenderMesh::to_bytes` blobs in transferred buffers, under the engine's handles.
//!
//! The browser's GPU is never waited for: the device is made asynchronously
//! ([`WebRender::create`]), every pick (hover and click) and the draw counts of scripts are
//! answered once the GPU is done, polled by the worker ([`WebRender::tick`]) between its
//! messages. Optional WebGPU features (`indirect-first-instance`, `depth32float-stencil8`)
//! are used when the adapter has them; without them bodies are drawn one by one (no culling)
//! and depth is 24-bit, the same image.
//!
//! On other targets the crate is empty.
#![cfg(target_arch = "wasm32")]

use oc_render::Renderer;
use oc_view::pick::{BoxReq, PickReq};
use oc_view::scene_dto::{CameraDto, SceneDto};
use oc_view::{Hooks, Meshes, Rect, Reply, View};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::*;
use web_time::Instant;

/// UI background (`--bg` in styles.css), sRGB: the canvas outside the graphics area (the UI
/// paints over it), as in the desktop app.
const CLEAR: wgpu::Color = wgpu::Color { r: 240.0 / 255.0, g: 240.0 / 255.0, b: 240.0 / 255.0, a: 1.0 };

/// `view_frame`'s arguments.
#[derive(Deserialize)]
struct FrameArgs {
    scene: Box<SceneDto>,
    camera: CameraDto,
    rect: Rect,
}

/// What the renderer runs on (`info`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Info {
    adapter: String,
    backend: String,
    /// `depth32float-stencil8`: 32-bit depth, else 24-bit.
    depth32: bool,
    /// `indirect-first-instance`: GPU lists and culling, else bodies one by one.
    indirect_first_instance: bool,
    surface: String,
}

/// The last frame drawn: its number and times (`drawn`).
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Drawn {
    seq: u32,
    /// Frames drawn.
    frames: u32,
    /// The frame's arrival to its commands submitted (CPU: the scene converted, meshes
    /// uploaded, the frame encoded).
    cpu_ms: f64,
    /// The frame's arrival to the GPU done with it (`onSubmittedWorkDone`).
    gpu_ms: Option<f64>,
}

#[wasm_bindgen]
pub struct WebRender {
    view: View,
    meshes: Meshes,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    info: String,
    /// A frame, label, mesh or resize arrived: prepare and draw on the next tick.
    dirty: bool,
    /// When the oldest undrawn change arrived.
    since: Option<Instant>,
    drawn: Drawn,
    /// Set by the queue when the GPU is done with the last frame drawn.
    gpu_done: Arc<Mutex<Option<Instant>>>,
    gpu_from: Option<Instant>,
}

fn err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

/// A reply passed to a JavaScript callback as JSON.
fn to_js<T: Serialize + 'static>(f: js_sys::Function) -> Reply<T> {
    Box::new(move |r: T| {
        let s = serde_json::to_string(&r).unwrap_or_else(|e| format!("{{\"error\":{:?}}}", e.to_string()));
        let _ = f.call1(&JsValue::NULL, &JsValue::from_str(&s));
    })
}

#[wasm_bindgen]
impl WebRender {
    /// Starts WebGPU on `canvas` (sized in device pixels). Fails without WebGPU, an adapter
    /// or a device: the UI then falls back (W1f).
    pub async fn create(canvas: web_sys::OffscreenCanvas) -> Result<WebRender, JsValue> {
        if !wgpu::util::is_browser_webgpu_supported().await {
            return Err(err("WebGPU is not available"));
        }
        let (w, h) = (canvas.width().max(1), canvas.height().max(1));
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(wgpu::SurfaceTarget::OffscreenCanvas(canvas)).map_err(err)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(err)?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| matches!(f, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm))
            .ok_or_else(|| err(format!("no 8-bit canvas format in {:?}", caps.formats)))?;
        // Bytes are copied from an sRGB render target into the canvas unchanged (formats
        // that differ only in sRGB copy into each other in WebGPU).
        let mut config = surface.get_default_config(&adapter, w, h).ok_or_else(|| err("the canvas does not suit the adapter"))?;
        config.format = format;
        config.usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST;
        config.present_mode = wgpu::PresentMode::Fifo;
        config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
        config.view_formats = vec![];
        let renderer = Renderer::on_adapter(&adapter, format.add_srgb_suffix()).await.map_err(err)?;
        surface.configure(renderer.device(), &config);
        let a = adapter.get_info();
        let f = renderer.device().features();
        let info = serde_json::to_string(&Info {
            adapter: if a.name.is_empty() { format!("{} {}", a.driver, a.driver_info) } else { a.name.clone() },
            backend: format!("{:?}", a.backend),
            depth32: f.contains(wgpu::Features::DEPTH32FLOAT_STENCIL8),
            indirect_first_instance: f.contains(wgpu::Features::INDIRECT_FIRST_INSTANCE),
            surface: format!("{format:?}"),
        })
        .map_err(err)?;
        let view = View::new(renderer, format.add_srgb_suffix(), Hooks::default());
        Ok(WebRender {
            view,
            meshes: Meshes::default(),
            surface,
            config,
            info,
            dirty: false,
            since: None,
            drawn: Drawn::default(),
            gpu_done: Arc::new(Mutex::new(None)),
            gpu_from: None,
        })
    }

    /// Adapter, backend, optional features and canvas format, as JSON.
    pub fn info(&self) -> String {
        self.info.clone()
    }

    /// The adapter as the desktop app's `app_info` gives it: "name (backend)".
    pub fn adapter(&self) -> String {
        self.view.info()
    }

    fn touch(&mut self) {
        self.dirty = true;
        self.since.get_or_insert_with(Instant::now);
    }

    /// The canvas' new size in device pixels (the window's).
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 && (width, height) != (self.config.width, self.config.height) {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(self.view.renderer().device(), &self.config);
        }
        self.touch();
    }

    /// A frame from the UI: `view_frame`'s arguments (`{scene, camera, rect}`) as JSON.
    pub fn frame(&mut self, json: &str) -> Result<(), JsValue> {
        let a: FrameArgs = serde_json::from_str(json).map_err(err)?;
        self.view.frame(a.scene, a.camera, a.rect);
        self.touch();
        Ok(())
    }

    /// A display mesh from the engine worker: its handle and `RenderMesh::to_bytes`. A frame
    /// that arrived first and names it is drawn again with it.
    pub fn mesh(&mut self, handle: u32, bytes: &[u8]) -> Result<(), JsValue> {
        let m = oc_mesh::RenderMesh::from_bytes(bytes).map_err(err)?;
        self.meshes.lock().expect("meshes").insert(handle, Arc::new(m));
        self.touch();
        Ok(())
    }

    /// A plane name rasterised by the UI (`view_add_label`): RGBA8, sRGB.
    pub fn label(&mut self, key: u32, rgba: &[u8], width: u32, height: u32) -> Result<(), JsValue> {
        if rgba.len() != (width * height * 4) as usize {
            return Err(err("label size mismatch"));
        }
        self.view.label(key, rgba, width, height);
        self.touch();
        Ok(())
    }

    /// A pick (`view_pick`'s `req` as JSON); `reply` gets the `PickReply` as JSON once the
    /// GPU has answered.
    pub fn pick(&mut self, json: &str, reply: js_sys::Function) -> Result<(), JsValue> {
        let q: Box<PickReq> = serde_json::from_str(json).map_err(err)?;
        self.view.pick(q, to_js(reply));
        Ok(())
    }

    /// A box selection (`view_box`'s `req` as JSON); `reply` gets the `BoxReply` as JSON.
    pub fn box_select(&mut self, json: &str, reply: js_sys::Function) -> Result<(), JsValue> {
        let q: Box<BoxReq> = serde_json::from_str(json).map_err(err)?;
        self.view.box_select(q, to_js(reply));
        Ok(())
    }

    /// What the last frame drew (`view_stats`), as JSON (`null` until counted: the first call
    /// turns the counting on).
    pub fn stats(&mut self) -> String {
        serde_json::to_string(&self.view.stats()).unwrap_or_else(|_| "null".into())
    }

    /// The last frame drawn and its times, as JSON.
    pub fn drawn(&mut self) -> String {
        if let (Some(from), Some(done)) = (self.gpu_from, *self.gpu_done.lock().expect("gpu done")) {
            if done >= from {
                self.drawn.gpu_ms = Some(done.duration_since(from).as_secs_f64() * 1e3);
            }
        }
        serde_json::to_string(&self.drawn).unwrap_or_default()
    }

    /// Does what is due: the newest frame prepared, picks answered, the frame drawn and
    /// presented, picks the GPU has finished sent. Returns true while picks or draw counts
    /// are still on the GPU or queries wait for their frame: call again soon (the worker
    /// yields to its event loop in between, which is when WebGPU resolves the read-backs).
    pub fn tick(&mut self) -> bool {
        let dirty = std::mem::take(&mut self.dirty);
        if dirty {
            self.view.prepare(&self.meshes);
        }
        // Answer picks before presenting, as the desktop app does.
        self.view.serve();
        if dirty {
            let rect = self.view.draw();
            self.present(rect);
            let since = self.since.take().unwrap_or_else(Instant::now);
            if let Some(seq) = self.view.seq() {
                self.drawn.seq = seq;
            }
            self.drawn.frames += 1;
            self.drawn.cpu_ms = since.elapsed().as_secs_f64() * 1e3;
            self.drawn.gpu_ms = None;
            self.gpu_from = Some(since);
            let done = self.gpu_done.clone();
            self.view.renderer().queue().on_submitted_work_done(move || {
                *done.lock().expect("gpu done") = Some(Instant::now());
            });
        }
        self.view.poll_pending();
        self.view.pending() + self.view.waiting() > 0
    }
}

impl WebRender {
    fn present(&mut self, rect: Option<Rect>) {
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
        r.queue().present(frame);
    }
}
