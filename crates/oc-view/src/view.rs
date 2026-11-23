//! The viewport's frames: the webview sends a frame (scene, camera, graphics-area rectangle)
//! whenever something changes; [`View`] keeps the newest, uploads the meshes it needs,
//! converts its scene and draws it into an off-screen texture the size of the graphics area
//! ([`View::draw`]), which the caller copies to its surface. Rendering is on demand: no frame
//! arrives, nothing is drawn.
//!
//! Picks and box selections ([`crate::pick`]) are answered against the scene of the frame
//! they name; hover picks run on the GPU while the caller goes on ([`View::poll_pending`]).
use crate::pick::{self, Answer, BoxReply, BoxReq, PickReply, PickReq};
use crate::scene_dto::{CameraDto, SceneDto};
use oc_mesh::RenderMesh;
use oc_render::{LabelId, MeshId, PendingPick, Renderer, Scene, Viewport};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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

/// Display meshes by the handle the engine gave them (the engine adds, the view drops the
/// ones the webview no longer holds).
pub type Meshes = Mutex<HashMap<u32, Arc<RenderMesh>>>;

/// Where a query's answer goes.
pub type Reply<T> = Box<dyn FnOnce(T)>;

/// What the view is doing, for the caller's diagnostics (`oc-app`'s stuck-state dump).
#[derive(Clone, Copy)]
pub struct Hooks {
    /// The view enters a phase of its work.
    pub phase: fn(&'static str),
    /// A frame (its `seq`) was drawn.
    pub drawn: fn(u32),
}

impl Default for Hooks {
    fn default() -> Self {
        Hooks { phase: |_| {}, drawn: |_| {} }
    }
}

/// A pick or box selection waiting for its frame.
enum Query {
    Pick(Box<PickReq>, Reply<PickReply>),
    Box(Box<BoxReq>, Reply<BoxReply>),
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

pub struct View {
    renderer: Renderer,
    /// The target's format (sRGB).
    format: wgpu::TextureFormat,
    hooks: Hooks,
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
    pending: Vec<(PendingPick, PickReply, Reply<PickReply>)>,
    /// Draw counts are read back after each frame (a script asked for them).
    count: bool,
    stats: Option<FrameStats>,
    /// When the frame being drawn started (prepared).
    started: Instant,
}

impl View {
    /// A view drawing with `renderer` into targets of `format` (an sRGB format, the one the
    /// renderer was made for).
    pub fn new(renderer: Renderer, format: wgpu::TextureFormat, hooks: Hooks) -> View {
        View {
            renderer,
            format,
            hooks,
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
        }
    }

    pub fn renderer(&self) -> &Renderer {
        &self.renderer
    }

    /// Adapter name and backend.
    pub fn info(&self) -> String {
        let i = self.renderer.adapter_info();
        format!("{} ({:?})", i.name, i.backend)
    }

    /// Queries waiting for their frame.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Hover picks on the GPU.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// A new frame from the webview (needs a redraw).
    pub fn frame(&mut self, mut scene: Box<SceneDto>, camera: CameraDto, rect: Rect) {
        scene.keep_bodies(self.last.as_mut().map(|l| &mut *l.0));
        self.last = Some((scene, camera, rect));
    }

    /// A plane name image under the webview's `key` (needs a redraw).
    pub fn label(&mut self, key: u32, rgba: &[u8], width: u32, height: u32) {
        let id = self.renderer.add_label(rgba, width, height);
        if let Some(old) = self.labels.insert(key, id) {
            self.renderer.remove_label(old);
        }
    }

    pub fn pick(&mut self, q: Box<PickReq>, reply: Reply<PickReply>) {
        self.waiting.push((Instant::now(), Query::Pick(q, reply)));
    }

    pub fn box_select(&mut self, q: Box<BoxReq>, reply: Reply<BoxReply>) {
        self.waiting.push((Instant::now(), Query::Box(q, reply)));
    }

    /// What the last frame drew; the first call turns the counting on.
    pub fn stats(&mut self) -> Option<FrameStats> {
        self.count = true;
        self.stats.clone()
    }

    /// Answers the queries whose frame has arrived (or that waited too long).
    pub fn serve(&mut self) {
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
                    Answer::Now(r) => reply(r),
                    Answer::Later(p, r) => self.pending.push((p, r, reply)),
                },
                Query::Box(q, reply) => reply(pick::box_select(&self.renderer, scene, seq, &q)),
            }
        }
    }

    /// Sends the hover picks the GPU has finished.
    pub fn poll_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        for (mut p, mut r, reply) in pending {
            match p.poll(&self.renderer) {
                Some(hit) => {
                    r.hit = hit.map(Into::into);
                    reply(r);
                }
                None => self.pending.push((p, r, reply)),
            }
        }
    }

    /// Drops meshes the webview no longer holds and uploads the ones the scene needs. An
    /// assembly build hands out new handles for unchanged parts; their meshes are the same
    /// `Arc`s, which the renderer does not upload again (G2).
    fn sync_meshes(&mut self, scene: &SceneDto, meshes: &Meshes) {
        let keep = |h: &u32| *h > scene.seen || scene.retain.contains(h);
        let gone: Vec<u32> = self.uploaded.keys().copied().filter(|h| !keep(h)).collect();
        for h in gone {
            if let Some(id) = self.uploaded.remove(&h) {
                self.renderer.remove_mesh(id);
            }
        }
        let mut store = meshes.lock().expect("mesh store");
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
    /// Starts the frame's clock.
    pub fn prepare(&mut self, meshes: &Meshes) {
        self.started = Instant::now();
        let Some(last) = self.last.take() else { return };
        self.sync_meshes(&last.0, meshes);
        let scene = last.0.to(&self.uploaded, &self.labels);
        self.pick_scene = last.0.pick_scene(&scene);
        self.current = Some((last.0.seq, scene));
        self.last = Some(last);
    }

    /// Draws the newest frame into the target and returns the graphics area it covers;
    /// `None` when there is no frame yet (the caller clears its surface).
    pub fn draw(&mut self) -> Option<Rect> {
        let (Some((dto, cam_dto, rect)), Some((seq, scene))) = (&self.last, &self.current) else {
            return None;
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
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            self.target = Some((tex, view, (vp.width, vp.height)));
        }
        let (_, view, _) = self.target.as_ref().expect("target");
        self.renderer.render(scene, &camera, vp, view);
        (self.hooks.drawn)(seq);
        if self.count {
            (self.hooks.phase)("draw_stats");
            let st = self.renderer.draw_stats();
            self.stats = Some(FrameStats { seq, moving, lod, levels: st.opaque_levels, instances: st.instances, render_ms: self.started.elapsed().as_secs_f64() * 1e3 });
        }
        Some(rect)
    }

    /// The target the last frame was drawn into, and its size in physical pixels.
    pub fn target(&self) -> Option<(&wgpu::Texture, (u32, u32))> {
        self.target.as_ref().map(|(t, _, size)| (t, *size))
    }

    /// The last viewport image drawn (waits for the GPU).
    pub fn capture(&self) -> Option<Rgba> {
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
        let bgra = matches!(self.format.remove_srgb_suffix(), wgpu::TextureFormat::Bgra8Unorm);
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
