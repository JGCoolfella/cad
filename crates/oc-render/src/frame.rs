//! Drawing a frame. Pass order follows three.js' order in the current viewer (opaque by
//! render order, then transparent), with everything drawn "over everything" last:
//!
//! 1. background;
//! 2. opaque part faces (not in Wireframe), section stencil parity and cap;
//! 3. part edges (all, or only highlighted ones in Shaded), hidden edges in HLV;
//! 4. depth-tested lines: inactive sketches, reference axes;
//! 5. transparent part faces, preview bodies, plane fills, preview edges, plane borders,
//!    plane labels;
//! 6. on top: coordinate systems, the previewed profile, reference points, vertex markers;
//! 7. the origin and the triad.
use crate::batch::{self, Batches, Ctx, FrameU, ObjectU, OverV, PtG};
use crate::camera::{Aabb, Camera, ORTHO_DEPTH};
use crate::color::Rgb;
use crate::overlay::{self, Painter};
use crate::renderer::{Image, Renderer, Viewport};
use crate::scene::{Background, BodyRole, DisplayStyle, Hit, Scene};
use glam::{DMat4, DVec3};

/// Light set of the current viewer: a hemisphere light (sky white, ground #8a8f98, 1.25)
/// and two white directional lights fixed to the camera (1.6 and 0.5).
const LIGHT1: (DVec3, f64) = (DVec3::new(0.35, 0.55, 1.0), 1.6);
const LIGHT2: (DVec3, f64) = (DVec3::new(-0.6, -0.3, 0.5), 0.5);

fn v4(v: [f32; 3], w: f32) -> [f32; 4] {
    [v[0], v[1], v[2], w]
}

pub(crate) struct Prepared {
    pub cam: Camera,
    pub model: Aabb,
    pub view_proj: DMat4,
}

/// Bodies' box (part bodies only) and everything's box (for the depth range).
fn boxes(r: &Renderer, s: &Scene) -> (Aabb, Aabb) {
    let mut model = Aabb::EMPTY;
    let mut all = Aabb::EMPTY;
    for b in &s.bodies {
        if let Some(m) = r.meshes.get(&b.mesh) {
            let wb = m.world_box().placed(&b.placement);
            all.union(&wb);
            if b.role == BodyRole::Part {
                model.union(&wb);
            }
        }
    }
    for p in &s.planes {
        let [u0, u1, v0, v1] = p.extents;
        for (u, v) in [(u0, v0), (u1, v1), (u0, v1), (u1, v0)] {
            all.add(p.frame.to_world(u, v));
        }
    }
    for a in &s.axes {
        let d = a.dir.normalize_or_zero() * (a.length / 2.0);
        all.add(a.point - d);
        all.add(a.point + d);
    }
    s.points.iter().for_each(|p| all.add(p.point));
    s.csys.iter().for_each(|c| all.add(c.frame.origin));
    for sk in &s.sketches {
        sk.segments.iter().flatten().for_each(|p| all.add(*p));
    }
    all.add(DVec3::ZERO);
    (model, all)
}

pub(crate) fn prepare(r: &Renderer, s: &Scene, cam: &Camera, vp: Viewport) -> Prepared {
    let mut cam = cam.clone();
    let (lw, lh) = vp.logical();
    cam.resize(lw, lh);
    let (model, all) = boxes(r, s);
    let (c, rad) = if all.is_empty() {
        (cam.target, ORTHO_DEPTH / 2.0)
    } else {
        (all.center(), all.diagonal() / 2.0 + 1.0)
    };
    let (near, far) = cam.depth_range(c, rad);
    let view_proj = cam.projection(near, far) * cam.view_rel();
    Prepared { cam, model, view_proj }
}

pub(crate) fn frame_uniform(s: &Scene, p: &Prepared, view_proj: DMat4, size: (f32, f32), scale: f64, pick: bool) -> FrameU {
    let o = &s.options;
    let cam = &p.cam;
    let q = cam.quat;
    let l1 = (q * LIGHT1.0.normalize()).as_vec3().to_array();
    let l2 = (q * LIGHT2.0.normalize()).as_vec3().to_array();
    let (clip, on) = match s.section {
        Some(sec) => {
            let n = sec.normal.normalize_or_zero();
            let d = n.dot(sec.origin - cam.target);
            (v4(n.as_vec3().to_array(), d as f32), 1)
        }
        None => ([0.0; 4], 0),
    };
    let (bg_mode, top, mid, bottom) = match o.background {
        Background::Scene { top, mid, bottom } => (0, top, mid, bottom),
        Background::Gradient { top, bottom } => (1, top, top, bottom),
        Background::Plain(c) => (2, c, c, c),
        Background::Transparent => (3, Rgb::BLACK, Rgb::BLACK, Rgb::BLACK),
    };
    let srgb = |c: Rgb| v4(c.to_srgb(), 1.0);
    let sc = scale as f32;
    FrameU {
        view_proj: view_proj.as_mat4().to_cols_array_2d(),
        eye: v4(cam.eye_rel().as_vec3().to_array(), if cam.perspective { 1.0 } else { 0.0 }),
        view_dir: v4(cam.view_dir().as_vec3().to_array(), 0.0),
        viewport: [size.0, size.1, sc, cam.world_per_pixel() as f32],
        clip,
        light1: v4(l1, LIGHT1.1 as f32),
        light2: v4(l2, LIGHT2.1 as f32),
        sky: v4(Rgb::WHITE.0, 1.25),
        ground: v4(Rgb::hex("#8a8f98").0, 1.25),
        hover: o.hover.with_alpha(1.0),
        selected: o.selected.with_alpha(1.0),
        preview: Rgb::hex("#f0dc3c").with_alpha(1.0),
        hidden_edge: Rgb::hex("#8a8a8a").with_alpha(1.0),
        amounts: [0.5, 0.65, 0.75, 0.0],
        // Plain edges: `Options::line_width` device px (the weight of three.js'
        // LineBasicMaterial lines in Electron); highlighted edges 3 logical px
        // (LineMaterial). In the ID pass edges are 1 px and vertex squares 9 logical px.
        widths: if pick { [1.0, 1.0, 9.0, 0.0] } else { [o.line_width, 3.0 * sc, 9.0 * sc, 0.0] },
        dash: [6.0 * sc, 4.0 * sc, 0.0, 0.0],
        bg_top: srgb(top),
        bg_mid: srgb(mid),
        bg_bottom: srgb(bottom),
        flags: [on, bg_mode, o.closed_edge_vertices as u32, 0],
    }
}

/// Face and edge highlight states of body `i` (0 none, 1 hover, 2 selected, 3 preview).
fn states(s: &Scene, i: usize, faces: u32, edges: u32) -> (Vec<u32>, Vec<u32>) {
    let b = &s.bodies[i];
    let o = &s.options;
    let mut fs = vec![0u32; faces as usize];
    let mut es = vec![0u32; edges as usize];
    for &f in &b.preview_faces {
        if let Some(x) = fs.get_mut(f as usize) {
            *x = 3;
        }
    }
    for h in &s.selected {
        match *h {
            Hit::Face { body, index } if body == b.id && o.shaded_face_highlight => {
                if let Some(x) = fs.get_mut(index as usize) {
                    *x = 2;
                }
            }
            Hit::Edge { body, index } if body == b.id => {
                if let Some(x) = es.get_mut(index as usize) {
                    *x = 2;
                }
            }
            _ => {}
        }
    }
    if o.dynamic_highlight {
        match s.hover {
            Some(h @ Hit::Face { body, index }) if body == b.id && o.shaded_face_highlight && !s.is_selected(h) => {
                if let Some(x) = fs.get_mut(index as usize) {
                    *x = 1;
                }
            }
            Some(Hit::Edge { body, index }) if body == b.id => {
                if let Some(x) = es.get_mut(index as usize) {
                    *x = 1;
                }
            }
            _ => {}
        }
    }
    (fs, es)
}

/// The object uniform of body `i`.
fn object(s: &Scene, i: usize, origin: DVec3, cam: &Camera, scale: f64, slot: u32) -> ObjectU {
    use crate::batch::*;
    let b = &s.bodies[i];
    let line_style = matches!(
        s.style,
        DisplayStyle::HiddenLinesRemoved | DisplayStyle::HiddenLinesVisible | DisplayStyle::Wireframe
    );
    let pl = &b.placement;
    let offset = v4((pl.apply(origin) - cam.target).as_vec3().to_array(), 0.0);
    let m = glam::DMat3::from_quat(pl.rotation).as_mat3();
    let rot = [v4(m.x_axis.to_array(), 0.0), v4(m.y_axis.to_array(), 0.0), v4(m.z_axis.to_array(), 0.0)];
    match b.role {
        BodyRole::Part => {
            let hlr = matches!(s.style, DisplayStyle::HiddenLinesRemoved | DisplayStyle::HiddenLinesVisible);
            let mut flags = OBJ_CLIP | OBJ_EDGE_STATES;
            if hlr {
                flags |= OBJ_UNLIT;
            }
            if !line_style || s.options.face_highlight_in_line_styles {
                flags |= OBJ_TINT;
            }
            if s.style == DisplayStyle::Shaded {
                flags |= OBJ_ONLY_HI_EDGES;
            }
            let color = if hlr { Rgb::WHITE } else { b.color };
            ObjectU {
                offset,
                rot,
                color: color.with_alpha(if b.transparent { 0.45 } else { 1.0 }),
                edge: v4(s.options.edges.0, 0.0),
                material: [0.42, 0.08, 0.0, 0.0],
                flags: [flags, slot, 0, 0],
            }
        }
        BodyRole::Highlight => ObjectU {
            offset,
            rot,
            color: b.color.with_alpha(0.7),
            edge: v4(b.color.lerp(Rgb::hex("#000000"), 0.35).0, (1.6 * scale) as f32),
            material: [0.6, 0.0, 0.0, 0.0],
            flags: [OBJ_CLIP, 0, 0, 0],
        },
        BodyRole::Preview { cut } => {
            let c = s.options.temp_shaded.lerp(Rgb::hex("#c8b400"), 0.25);
            ObjectU {
                offset,
                rot,
                color: c.with_alpha(if cut { 0.55 } else { 0.42 }),
                edge: v4(Rgb::hex("#f0e000").0, (1.6 * scale) as f32),
                material: [0.6, 0.0, 0.0, 0.0],
                flags: [OBJ_CLIP, 0, 0, 0],
            }
        }
    }
}

/// Body draw data for one frame.
pub(crate) struct BodyDraw {
    pub level: usize,
    pub bind: wgpu::BindGroup,
}

impl Renderer {
    /// Writes object uniforms and states and makes the bind groups of every body.
    pub(crate) fn body_draws(&mut self, s: &Scene, p: &Prepared, scale: f64) -> Vec<Option<BodyDraw>> {
        let mut out = vec![];
        for (i, b) in s.bodies.iter().enumerate() {
            let Some(m) = self.meshes.get(&b.mesh) else {
                out.push(None);
                continue;
            };
            let (faces, edges) = (m.face_count, m.edge_count);
            let origin = m.origin;
            let level = m.level(b.lod, p.cam.world_per_pixel() / scale);
            self.ensure_slot(i, faces as u64, edges as u64);
            let m = &self.meshes[&b.mesh];
            let slot = &self.slots[i];
            let obj = object(s, i, origin, &p.cam, scale, i as u32);
            let (fs, es) = states(s, i, faces, edges);
            self.queue.write_buffer(&slot.object, 0, bytemuck::bytes_of(&obj));
            if !fs.is_empty() {
                self.queue.write_buffer(&slot.faces, 0, bytemuck::cast_slice(&fs));
            }
            if !es.is_empty() {
                self.queue.write_buffer(&slot.edges, 0, bytemuck::cast_slice(&es));
            }
            let lv = &m.levels[level];
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("body"),
                layout: &self.pipes.body_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: slot.object.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: slot.faces.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: slot.edges.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: lv.segments.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: lv.seg_dist.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: m.points.as_entire_binding() },
                ],
            });
            out.push(Some(BodyDraw { level, bind }));
        }
        out
    }

    pub(crate) fn batch_bind(&self, b: &Batches) -> wgpu::BindGroup {
        let lines = self.storage("lines", bytemuck::cast_slice(&b.lines));
        let points = self.storage("points", bytemuck::cast_slice(&b.points));
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("batch"),
            layout: &self.pipes.batch_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: lines.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: points.as_entire_binding() },
            ],
        })
    }

    /// Draws `scene` into `target` (a view of the renderer's colour format, `vp` sized).
    pub fn render(&mut self, scene: &Scene, camera: &Camera, vp: Viewport, target: &wgpu::TextureView) {
        let cmd = self.encode(scene, camera, vp, target);
        self.queue.submit([cmd]);
    }

    /// Renders off screen and reads the image back.
    pub fn render_image(&mut self, scene: &Scene, camera: &Camera, vp: Viewport) -> Image {
        let (w, h) = (vp.width, vp.height);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.color_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let draw = self.encode(scene, camera, vp, &view);
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image readback"),
            size: (row * h) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([draw, enc.finish()]);
        let raw = self.read_buffer(&buf);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let start = (y * row) as usize;
            rgba.extend_from_slice(&raw[start..start + (w * 4) as usize]);
        }
        Image { width: w, height: h, rgba }
    }

    fn encode(&mut self, s: &Scene, camera: &Camera, vp: Viewport, target: &wgpu::TextureView) -> wgpu::CommandBuffer {
        self.set_samples(s.options.msaa);
        self.ensure_targets(vp.width, vp.height);
        let p = prepare(self, s, camera, vp);
        let fu = frame_uniform(s, &p, p.view_proj, (vp.width as f32, vp.height as f32), vp.scale, false);
        self.queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&fu));
        let cx = Ctx { scene: s, cam: &p.cam, scale: vp.scale, model: p.model };
        let mut bt = batch::visual(&cx, |id| self.labels.get(&id.0).map(|l| l.aspect));
        let mut markers: Vec<PtG> = vec![];
        batch::vertex_markers(
            &cx,
            |body, index| {
                let b = s.bodies.iter().find(|b| b.id == body && b.role == BodyRole::Part)?;
                let m = self.meshes.get(&b.mesh)?;
                let q = m.point_pos.get(index as usize)?;
                Some(b.placement.apply(m.origin + DVec3::new(q[0] as f64, q[1] as f64, q[2] as f64)))
            },
            &mut markers,
        );
        bt.points.extend(markers);
        let mut painter = Painter { scale: vp.scale, out: vec![] };
        // The View Selector is a 3D overlay over the model; the origin and triad are drawn
        // on the 2D canvas above it in viewport.ts.
        if let Some(sel) = &s.view_selector {
            crate::selector::paint(&mut painter, &p.cam, sel.hover, s.options.line_width);
        }
        overlay::origin(&mut painter, s, &p.cam);
        if s.options.triad {
            overlay::triad(&mut painter, &p.cam);
        }
        let draws = self.body_draws(s, &p, vp.scale);
        let batch_bind = self.batch_bind(&bt);
        let frame_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &self.pipes.frame_bgl,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: self.frame_buf.as_entire_binding() }],
        });
        let fills = self.vertex_buffer(bytemuck::cast_slice(&bt.fills));
        let cap = self.vertex_buffer(bytemuck::cast_slice(&bt.cap));
        let over = self.vertex_buffer(bytemuck::cast_slice::<OverV, u8>(&painter.out));
        let labels: Vec<(wgpu::Buffer, &wgpu::BindGroup)> = bt
            .labels
            .iter()
            .filter_map(|(id, v)| {
                let l = self.labels.get(&id.0)?;
                Some((self.vertex_buffer(bytemuck::cast_slice(v))?, &l.bind))
            })
            .collect();

        let t = self.targets.as_ref().expect("targets");
        let (view, resolve) = match &t.msaa {
            Some(m) => (m, Some(target)),
            None => (target, None),
        };
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: resolve,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Discard }),
                }),
                ..Default::default()
            });
            let pp = &self.pipes;
            pass.set_bind_group(0, &frame_bind, &[]);
            pass.set_pipeline(&pp.bg);
            pass.draw(0..3, 0..1);

            let parts: Vec<(&BodyDraw, &crate::scene::BodyItem)> = draws
                .iter()
                .zip(&s.bodies)
                .filter_map(|(d, b)| d.as_ref().map(|d| (d, b)))
                .collect();
            let faces = |pass: &mut wgpu::RenderPass, d: &BodyDraw, b: &crate::scene::BodyItem, pipe: &wgpu::RenderPipeline| {
                let lv = &self.meshes[&b.mesh].levels[d.level];
                pass.set_pipeline(pipe);
                pass.set_bind_group(1, &d.bind, &[]);
                pass.set_vertex_buffer(0, lv.vertices.slice(..));
                pass.set_index_buffer(lv.indices.slice(..), lv.index_format);
                pass.draw_indexed(0..lv.index_count, 0, 0..1);
            };
            let edges = |pass: &mut wgpu::RenderPass, d: &BodyDraw, b: &crate::scene::BodyItem, pipe: &wgpu::RenderPipeline| {
                let lv = &self.meshes[&b.mesh].levels[d.level];
                if lv.segment_count > 0 {
                    pass.set_pipeline(pipe);
                    pass.set_bind_group(1, &d.bind, &[]);
                    pass.draw(0..6, 0..lv.segment_count);
                }
            };
            let is_part = |b: &crate::scene::BodyItem| b.role == BodyRole::Part;
            let shows_faces = s.style != DisplayStyle::Wireframe;
            // 2. Opaque part faces, section cap.
            if shows_faces {
                for (d, b) in parts.iter().filter(|(_, b)| is_part(b) && !b.transparent) {
                    faces(&mut pass, d, b, &pp.face);
                }
            }
            if let (Some(_), Some(cap)) = (s.section, &cap) {
                for (d, b) in parts.iter().filter(|(_, b)| is_part(b)) {
                    faces(&mut pass, d, b, &pp.parity);
                }
                pass.set_pipeline(&pp.cap);
                pass.set_stencil_reference(0);
                pass.set_vertex_buffer(0, cap.slice(..));
                pass.draw(0..bt.cap.len() as u32, 0..1);
            }
            // 3. Edges.
            for (d, b) in parts.iter().filter(|(_, b)| is_part(b)) {
                edges(&mut pass, d, b, &pp.edge);
            }
            if s.style == DisplayStyle::HiddenLinesVisible {
                for (d, b) in parts.iter().filter(|(_, b)| is_part(b)) {
                    edges(&mut pass, d, b, &pp.edge_hidden);
                }
            }
            // 4. Depth-tested lines.
            pass.set_bind_group(1, &batch_bind, &[]);
            if !bt.ranges.depth.is_empty() {
                pass.set_pipeline(&pp.line);
                pass.draw(0..6, bt.ranges.depth.clone());
            }
            // 5. Transparent.
            if shows_faces {
                for (d, b) in parts.iter().filter(|(_, b)| is_part(b) && b.transparent) {
                    faces(&mut pass, d, b, &pp.face_blend);
                }
            }
            for (d, b) in &parts {
                match b.role {
                    BodyRole::Preview { cut } => faces(&mut pass, d, b, if cut { &pp.preview_cut } else { &pp.preview }),
                    BodyRole::Highlight => faces(&mut pass, d, b, &pp.preview_cut),
                    BodyRole::Part => {}
                }
            }
            if let Some(f) = &fills {
                pass.set_pipeline(&pp.fill);
                pass.set_vertex_buffer(0, f.slice(..));
                pass.draw(0..bt.fills.len() as u32, 0..1);
            }
            for (d, b) in parts.iter().filter(|(_, b)| !is_part(b)) {
                edges(&mut pass, d, b, &pp.edge_top);
            }
            pass.set_bind_group(1, &batch_bind, &[]);
            if !bt.ranges.blend.is_empty() {
                pass.set_pipeline(&pp.line_blend);
                pass.draw(0..6, bt.ranges.blend.clone());
            }
            for (vb, bind) in &labels {
                pass.set_pipeline(&pp.label);
                pass.set_bind_group(1, *bind, &[]);
                pass.set_vertex_buffer(0, vb.slice(..));
                pass.draw(0..6, 0..1);
            }
            // 6. On top.
            pass.set_bind_group(1, &batch_bind, &[]);
            if !bt.ranges.top.is_empty() {
                pass.set_pipeline(&pp.line_top);
                pass.draw(0..6, bt.ranges.top.clone());
            }
            if !bt.points.is_empty() {
                pass.set_pipeline(&pp.point_top);
                pass.draw(0..6, 0..bt.points.len() as u32);
            }
            // 7. View Selector, origin and triad.
            if let Some(o) = &over {
                pass.set_pipeline(&pp.overlay);
                pass.set_vertex_buffer(0, o.slice(..));
                pass.draw(0..painter.out.len() as u32, 0..1);
            }
        }
        enc.finish()
    }
}
