//! CPU side of a frame: the uniform blocks and the per-frame batches (lines, points,
//! plane fills, labels, section cap) built from a [`Scene`]. Positions are converted to
//! `f32` only after subtracting the render origin (the camera target) in `f64`.
use crate::camera::{Aabb, Camera};
use crate::color::Rgb;
use crate::planes;
use crate::scene::{Hit, LabelId, Scene, Section};
use bytemuck::{Pod, Zeroable};
use glam::DVec3;

/// ID buffer kinds (high byte of the pick word); 0 is background.
pub(crate) const K_FACE: u32 = 1;
pub(crate) const K_EDGE: u32 = 2;
pub(crate) const K_VERTEX: u32 = 3;
pub(crate) const K_PLANE: u32 = 4;
pub(crate) const K_SKETCH: u32 = 5;
pub(crate) const K_REF: u32 = 6;

pub(crate) fn word(kind: u32, slot: u32) -> u32 {
    (kind << 24) | (slot & 0x00ff_ffff)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct FrameU {
    pub view_proj: [[f32; 4]; 4],
    pub eye: [f32; 4],
    pub view_dir: [f32; 4],
    pub viewport: [f32; 4],
    pub clip: [f32; 4],
    pub light1: [f32; 4],
    pub light2: [f32; 4],
    pub sky: [f32; 4],
    pub ground: [f32; 4],
    pub hover: [f32; 4],
    pub selected: [f32; 4],
    pub preview: [f32; 4],
    pub hidden_edge: [f32; 4],
    pub amounts: [f32; 4],
    pub widths: [f32; 4],
    pub dash: [f32; 4],
    pub bg_top: [f32; 4],
    pub bg_mid: [f32; 4],
    pub bg_bottom: [f32; 4],
    pub flags: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub(crate) struct ObjectU {
    pub offset: [f32; 4],
    /// Columns of the placement's rotation (identity for a part).
    pub rot: [[f32; 4]; 3],
    pub color: [f32; 4],
    pub edge: [f32; 4],
    pub material: [f32; 4],
    pub flags: [u32; 4],
}

pub(crate) const OBJ_UNLIT: u32 = 1;
pub(crate) const OBJ_TINT: u32 = 2;
pub(crate) const OBJ_CLIP: u32 = 4;
pub(crate) const OBJ_ONLY_HI_EDGES: u32 = 8;
pub(crate) const OBJ_EDGE_STATES: u32 = 16;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct LineG {
    pub a: [f32; 4],
    pub b: [f32; 4],
    pub color: [f32; 4],
    pub id: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct PtG {
    pub p: [f32; 4],
    pub color: [f32; 4],
    pub id: [u32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct TriV {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub color: [f32; 4],
    pub back: [f32; 4],
    pub id: [u32; 2],
    pub flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct LabelV {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(crate) struct OverV {
    pub pos: [f32; 2],
    pub color: [f32; 4],
}

/// Ranges of [`Batches::lines`] drawn with different depth behaviour.
#[derive(Default, Debug)]
pub(crate) struct LineRanges {
    /// Depth-tested and written (sketches, reference axes).
    pub depth: std::ops::Range<u32>,
    /// Over everything (coordinate systems, the previewed profile).
    pub top: std::ops::Range<u32>,
    /// Depth-tested, blended, not written (plane borders).
    pub blend: std::ops::Range<u32>,
}

#[derive(Default)]
pub(crate) struct Batches {
    pub lines: Vec<LineG>,
    pub ranges: LineRanges,
    pub points: Vec<PtG>,
    pub fills: Vec<TriV>,
    pub cap: Vec<TriV>,
    pub labels: Vec<(LabelId, [LabelV; 6])>,
}

/// Rendering context shared by the builders.
pub(crate) struct Ctx<'a> {
    pub scene: &'a Scene,
    pub cam: &'a Camera,
    /// Physical px per logical px.
    pub scale: f64,
    /// Model box (bodies), for the cap size.
    pub model: Aabb,
}

impl Ctx<'_> {
    pub fn rel(&self, p: DVec3) -> [f32; 3] {
        (p - self.cam.target).as_vec3().to_array()
    }
    fn state_color(&self, h: Hit, normal: Rgb) -> Rgb {
        let o = &self.scene.options;
        if self.scene.is_selected(h) {
            o.selected
        } else if self.scene.is_hovered(h) {
            o.hover
        } else {
            normal
        }
    }
    fn px(&self, logical: f64) -> f32 {
        (logical * self.scale) as f32
    }
}

fn line(a: [f32; 3], b: [f32; 3], width: f32, color: [f32; 4], id: [u32; 2]) -> LineG {
    LineG {
        a: [a[0], a[1], a[2], width],
        b: [b[0], b[1], b[2], 0.0],
        color,
        id: [id[0], id[1], 0, 0],
    }
}

fn dashed(mut l: LineG, dash: f32, gap: f32) -> LineG {
    l.b[3] = dash;
    l.id[2] = gap.to_bits();
    l
}

const AXIS_BLUE: &str = "#2a62c9";
const PLANE_BORDER: &str = "#6c9be0";
const PROFILE: &str = "#141414";

/// The colour pass batches.
pub(crate) fn visual(cx: &Ctx, label_aspect: impl Fn(LabelId) -> Option<f64>) -> Batches {
    let s = cx.scene;
    let o = &s.options;
    let wpp = cx.cam.world_per_pixel();
    let mut b = Batches::default();
    let one = o.line_width; // three.js LineBasicMaterial lines: `Options::line_width` device px
    let nid = [0u32, 0u32];
    // Depth-tested lines: inactive sketches, reference axes.
    for sk in s.sketches.iter().filter(|k| !k.profile) {
        let c = cx.state_color(Hit::Feature(sk.id), o.sketch_inactive).with_alpha(1.0);
        for [p, q] in &sk.segments {
            b.lines.push(line(cx.rel(*p), cx.rel(*q), one, c, nid));
        }
    }
    for a in &s.axes {
        let c = cx.state_color(Hit::Feature(a.id), Rgb::hex(AXIS_BLUE)).with_alpha(1.0);
        let d = a.dir.normalize_or_zero() * (a.length / 2.0);
        let l = line(cx.rel(a.point - d), cx.rel(a.point + d), one, c, nid);
        b.lines.push(dashed(l, cx.px(14.0), cx.px(5.0)));
    }
    let n_depth = b.lines.len() as u32;
    // Over everything: coordinate systems, then the previewed profile.
    for c in &s.csys {
        let sel = s.is_selected(Hit::Feature(c.id));
        let len = wpp * 40.0;
        for (d, hex) in [(c.frame.x, "#d01818"), (c.frame.y, "#18a018"), (c.frame.normal, "#1830d0")] {
            let col = if sel { o.selected } else { Rgb::hex(hex) };
            let (p, q) = (cx.rel(c.frame.origin), cx.rel(c.frame.origin + d * len));
            b.lines.push(line(p, q, one, col.with_alpha(1.0), nid));
        }
    }
    for sk in s.sketches.iter().filter(|k| k.profile) {
        let c = cx.state_color(Hit::Feature(sk.id), Rgb::hex(PROFILE)).with_alpha(1.0);
        for [p, q] in &sk.segments {
            b.lines.push(line(cx.rel(*p), cx.rel(*q), one, c, nid));
        }
    }
    let n_top = b.lines.len() as u32;
    // Planes: fills (two-sided colours), borders, labels.
    let opacity = planes::fill_opacity(o.shaded_planes, o.plane_transparency) as f32;
    for p in &s.planes {
        let h = Hit::Plane(p.id);
        let (hov, sel) = (s.is_hovered(h), s.is_selected(h));
        let [u0, u1, v0, v1] = p.extents;
        let c = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)].map(|(u, v)| cx.rel(p.frame.to_world(u, v)));
        if opacity > 0.0 {
            let n = p.frame.normal.as_vec3().to_array();
            let (f, bk) = (o.plane_front.with_alpha(opacity), o.plane_back.with_alpha(opacity));
            for i in [0, 1, 2, 0, 2, 3] {
                b.fills.push(TriV { pos: c[i], nrm: n, color: f, back: bk, id: nid, flags: 0 });
            }
        }
        let col = if sel {
            o.selected
        } else if hov {
            o.hover
        } else {
            Rgb::hex(PLANE_BORDER)
        };
        let w = cx.px(planes::border_width(hov, sel));
        for i in 0..4 {
            b.lines.push(line(c[i], c[(i + 1) % 4], w, col.with_alpha(1.0), nid));
        }
        if let Some((id, aspect)) = p.label.and_then(|id| label_aspect(id).map(|a| (id, a))) {
            let [x0, x1, y0, y1] = planes::label_rect(p.extents, aspect);
            let q = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(u, v)| cx.rel(p.frame.to_world(u, v)));
            let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
            let v = [0, 1, 2, 0, 2, 3].map(|i| LabelV { pos: q[i], uv: uv[i] });
            b.labels.push((id, v));
        }
    }
    b.ranges = LineRanges {
        depth: 0..n_depth,
        top: n_depth..n_top,
        blend: n_top..b.lines.len() as u32,
    };
    // Points over everything: reference points (7 px), then vertex markers (8 px).
    for pt in &s.points {
        let c = cx.state_color(Hit::Feature(pt.id), Rgb::hex(AXIS_BLUE));
        let r = cx.rel(pt.point);
        b.points.push(PtG { p: [r[0], r[1], r[2], cx.px(7.0)], color: c.with_alpha(1.0), id: [0; 4] });
    }
    if let Some(sec) = s.section {
        b.cap = cap(cx, &sec);
    }
    b
}

/// World positions of the hovered and selected model vertices, with their marker colour.
pub(crate) fn vertex_markers(
    cx: &Ctx,
    point_of: impl Fn(u32, u32) -> Option<DVec3>,
    out: &mut Vec<PtG>,
) {
    let s = cx.scene;
    let items = s.selected.iter().chain(s.hover.iter());
    for h in items {
        if let Hit::Vertex { body, index } = *h {
            let Some(p) = point_of(body, index) else { continue };
            let c = if s.is_selected(*h) { s.options.selected } else { s.options.hover };
            let r = cx.rel(p);
            out.push(PtG { p: [r[0], r[1], r[2], cx.px(8.0)], color: c.with_alpha(1.0), id: [0; 4] });
        }
    }
}

/// The section cap: a square on the section plane, lit, model colour × 0.85, drawn where
/// the stencil parity of the bodies is odd.
fn cap(cx: &Ctx, sec: &Section) -> Vec<TriV> {
    let n = sec.normal.normalize_or_zero();
    if n == DVec3::ZERO {
        return vec![];
    }
    let size = cx.model.diagonal() * 4.0 + 10.0;
    let (u, v) = n.any_orthonormal_pair();
    let h = size / 2.0;
    let c = [(-h, -h), (h, -h), (h, h), (-h, h)].map(|(a, b)| cx.rel(sec.origin + u * a + v * b));
    let color = cx
        .scene
        .bodies
        .iter()
        .find(|b| b.role == crate::scene::BodyRole::Part)
        .map_or(Rgb::hex("#c9ccd1"), |b| b.color)
        .scale(0.85)
        .with_alpha(1.0);
    let nrm = n.as_vec3().to_array();
    [0, 1, 2, 0, 2, 3]
        .map(|i| TriV { pos: c[i], nrm, color, back: color, id: [0; 2], flags: 1 })
        .to_vec()
}

/// The ID pass batches: sketches and reference geometry as 1 px lines, reference points
/// as 11 px squares, plane quads.
pub(crate) fn pick(cx: &Ctx) -> Batches {
    let s = cx.scene;
    let wpp = cx.cam.world_per_pixel();
    let mut b = Batches::default();
    for sk in &s.sketches {
        let id = [word(K_SKETCH, 0), sk.id];
        for [p, q] in &sk.segments {
            b.lines.push(line(cx.rel(*p), cx.rel(*q), 1.0, [0.0; 4], id));
        }
    }
    for a in &s.axes {
        let d = a.dir.normalize_or_zero() * (a.length / 2.0);
        let id = [word(K_REF, 0), a.id];
        b.lines.push(line(cx.rel(a.point - d), cx.rel(a.point + d), 1.0, [0.0; 4], id));
    }
    for c in &s.csys {
        let id = [word(K_REF, 0), c.id];
        for d in [c.frame.x, c.frame.y, c.frame.normal] {
            let q = c.frame.origin + d * (wpp * 40.0);
            b.lines.push(line(cx.rel(c.frame.origin), cx.rel(q), 1.0, [0.0; 4], id));
        }
    }
    b.ranges.depth = 0..b.lines.len() as u32;
    for pt in &s.points {
        let r = cx.rel(pt.point);
        b.points.push(PtG { p: [r[0], r[1], r[2], 11.0], color: [0.0; 4], id: [word(K_REF, 0), pt.id, 0, 0] });
    }
    for p in &s.planes {
        let [u0, u1, v0, v1] = p.extents;
        let c = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)].map(|(u, v)| cx.rel(p.frame.to_world(u, v)));
        let id = [word(K_PLANE, 0), p.id];
        for i in [0, 1, 2, 0, 2, 3] {
            b.fills.push(TriV { pos: c[i], nrm: [0.0; 3], color: [0.0; 4], back: [0.0; 4], id, flags: 0 });
        }
    }
    b
}
