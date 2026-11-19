//! The CPU side of a frame's GPU scene (G2): every body's [`ObjectU`] (placement relative
//! to the camera target, colours, flags, the draw kinds it joins), the highlight states of
//! the bodies that have any, the grouping into draw slots, and the culling uniforms.
use crate::batch::{ObjectU, OBJ_CLIP, OBJ_EDGE_STATES, OBJ_ONLY_HI_EDGES, OBJ_TINT, OBJ_UNLIT};
use crate::camera::Camera;
use crate::color::Rgb;
use crate::cull::{CullU, Phase};
use crate::frame::Prepared;
use crate::gpu_scene::*;
use crate::mesh::MeshGpu;
use crate::renderer::Renderer;
use crate::scene::{BodyItem, BodyRole, DisplayStyle, Hit, LodChoice, MeshId, Scene};
use glam::{DMat4, DVec3};
use std::collections::HashSet;

fn v4(v: [f32; 3], w: f32) -> [f32; 4] {
    [v[0], v[1], v[2], w]
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

/// Bodies (by caller id) that some hover or selection names.
fn highlighted(s: &Scene) -> HashSet<u32> {
    let mut ids = HashSet::new();
    for h in s.selected.iter().chain(s.hover.iter()) {
        if let Hit::Face { body, .. } | Hit::Edge { body, .. } = *h {
            ids.insert(body);
        }
    }
    ids
}

/// Millimetres per physical pixel for the automatic level of detail: a constant in an
/// orthographic view; per millimetre of distance from the eye in perspective.
pub(crate) fn lod_scale(cam: &Camera, scale: f64) -> f64 {
    let mm = cam.world_per_pixel() / scale;
    if cam.perspective {
        mm / cam.distance()
    } else {
        mm
    }
}

/// The level a body is drawn at: the GPU's rule (`level_of` in cull.wgsl) in `f64`, for
/// bodies drawn one by one.
pub(crate) fn cpu_level(m: &MeshGpu, b: &BodyItem, cam: &Camera, scale: f64) -> usize {
    let mut mm = lod_scale(cam, scale);
    if cam.perspective {
        let [lo, hi] = m.bounds;
        let (lo, hi) = (DVec3::from(lo.map(f64::from)), DVec3::from(hi.map(f64::from)));
        let center = b.placement.apply(m.origin + (lo + hi) * 0.5);
        let eye = cam.target + cam.eye_rel();
        mm *= ((center - eye).length() - (hi - lo).length() * 0.5).max(1e-3);
    }
    m.level(b.lod, mm)
}

/// The draw kinds of a part body (bits of [`KINDS`]).
fn kinds(s: &Scene, b: &BodyItem, m: &MeshGpu, edge_highlight: bool) -> u32 {
    let style = b.style.unwrap_or(s.style);
    let faces = style != DisplayStyle::Wireframe;
    let mut k = 0;
    if faces {
        k |= if b.transparent { 1 << K_TRANSP } else { 1 << K_PRE | 1 << K_OPAQUE };
    }
    // Shaded: only highlighted edges are drawn.
    if style != DisplayStyle::Shaded || edge_highlight {
        k |= 1 << K_EDGE;
    }
    if style == DisplayStyle::HiddenLinesVisible {
        k |= 1 << K_HIDDEN;
    }
    if s.section.is_some() {
        k |= 1 << K_PARITY;
    }
    if b.pickable {
        k |= 1 << K_PICK_EDGE;
        if faces {
            k |= 1 << K_PICK_FACE;
        }
        if m.point_count > 0 {
            k |= 1 << K_PICK_VERTEX;
        }
    }
    k
}

/// The object of body `i`.
fn object(s: &Scene, i: usize, m: &MeshGpu, cam: &Camera, scale: f64) -> ObjectU {
    let b = &s.bodies[i];
    let style = b.style.unwrap_or(s.style);
    let line_style = matches!(style, DisplayStyle::HiddenLinesRemoved | DisplayStyle::HiddenLinesVisible | DisplayStyle::Wireframe);
    let pl = &b.placement;
    let offset = v4((pl.apply(m.origin) - cam.target).as_vec3().to_array(), 0.0);
    let r = glam::DMat3::from_quat(pl.rotation).as_mat3();
    let rot = [v4(r.x_axis.to_array(), 0.0), v4(r.y_axis.to_array(), 0.0), v4(r.z_axis.to_array(), 0.0)];
    let (lod_mode, lod_level, lod_err) = match b.lod {
        LodChoice::Finest => (0, 0, 0.0),
        LodChoice::Auto { max_error_px } => (1, 0, max_error_px as f32),
        LodChoice::Level(k) => (2, k as u32, 0.0),
    };
    let base = ObjectU {
        offset,
        rot,
        flags: [0, i as u32, NONE, NONE],
        cull: [NONE, 0, lod_mode, lod_level],
        extra: [m.point_base, lod_err.to_bits(), 0, 0],
        ..Default::default()
    };
    match b.role {
        BodyRole::Part => {
            let hlr = matches!(style, DisplayStyle::HiddenLinesRemoved | DisplayStyle::HiddenLinesVisible);
            let mut flags = OBJ_CLIP | OBJ_EDGE_STATES;
            if hlr {
                flags |= OBJ_UNLIT;
            }
            if !line_style || s.options.face_highlight_in_line_styles {
                flags |= OBJ_TINT;
            }
            if style == DisplayStyle::Shaded {
                flags |= OBJ_ONLY_HI_EDGES;
            }
            let color = if hlr { Rgb::WHITE } else { b.color };
            ObjectU {
                color: color.with_alpha(if b.transparent { 0.45 } else { 1.0 }),
                edge: v4(s.options.edges.0, 0.0),
                material: [0.42, 0.08, 0.0, 0.0],
                flags: [flags, i as u32, NONE, NONE],
                ..base
            }
        }
        BodyRole::Highlight => ObjectU {
            color: b.color.with_alpha(0.7),
            edge: v4(b.color.lerp(Rgb::hex("#000000"), 0.35).0, (1.6 * scale) as f32),
            material: [0.6, 0.0, 0.0, 0.0],
            flags: [OBJ_CLIP, 0, NONE, NONE],
            ..base
        },
        BodyRole::Preview { cut } => {
            let c = s.options.temp_shaded.lerp(Rgb::hex("#c8b400"), 0.25);
            ObjectU {
                color: c.with_alpha(if cut { 0.55 } else { 0.42 }),
                edge: v4(Rgb::hex("#f0e000").0, (1.6 * scale) as f32),
                material: [0.6, 0.0, 0.0, 0.0],
                flags: [OBJ_CLIP, 0, NONE, NONE],
                ..base
            }
        }
    }
}

/// A frame's GPU scene before upload.
pub(crate) struct Built {
    pub objects: Vec<ObjectU>,
    pub states: Vec<u32>,
    pub tables: Tables,
    /// Draw kind bits per body (0 for bodies that are not part bodies or have no mesh).
    pub kinds: Vec<u32>,
}

/// Objects, states and slots of `s` seen by `cam`.
pub(crate) fn build(r: &Renderer, s: &Scene, cam: &Camera, scale: f64) -> Built {
    let hi = highlighted(s);
    let mut objects = Vec::with_capacity(s.bodies.len());
    let mut states_buf: Vec<u32> = vec![];
    let mut inst: Vec<Option<MeshId>> = Vec::with_capacity(s.bodies.len());
    let mut kind_bits = Vec::with_capacity(s.bodies.len());
    for (i, b) in s.bodies.iter().enumerate() {
        let Some(m) = r.meshes.get(&b.mesh) else {
            objects.push(ObjectU { cull: [NONE, 0, 0, 0], flags: [0, i as u32, NONE, NONE], ..Default::default() });
            inst.push(None);
            kind_bits.push(0);
            continue;
        };
        let mut o = object(s, i, m, cam, scale);
        let mut edge_hi = false;
        if !b.preview_faces.is_empty() || hi.contains(&b.id) {
            let (fs, es) = states(s, i, m.face_count, m.edge_count);
            edge_hi = es.iter().any(|&x| x != 0);
            if fs.iter().any(|&x| x != 0) {
                o.flags[2] = states_buf.len() as u32;
                states_buf.extend_from_slice(&fs);
            }
            if edge_hi {
                o.flags[3] = states_buf.len() as u32;
                states_buf.extend_from_slice(&es);
            }
        }
        let part = b.role == BodyRole::Part;
        let k = if part { kinds(s, b, m, edge_hi) } else { 0 };
        o.cull[1] = k;
        objects.push(o);
        inst.push(part.then_some(b.mesh));
        kind_bits.push(k);
    }
    let tables = tables(&inst, &r.meshes);
    for (i, o) in objects.iter_mut().enumerate() {
        o.cull[0] = tables.mesh_of[i];
    }
    Built { objects, states: states_buf, tables, kinds: kind_bits }
}

/// The culling uniform of a phase. `view_proj` is the phase's (the pick window's for
/// picks), `size` its target in px, `margin` px around boxes (line widths), `hiz` the
/// pyramid's levels (0: no occlusion test), `on` culling at all.
#[allow(clippy::too_many_arguments)]
pub(crate) fn cull_uniform(s: &Scene, p: &Prepared, view_proj: DMat4, size: (f32, f32), margin: f32, phase: Phase, hiz: u32, on: bool, count: u32, scale: f64) -> CullU {
    let cam = &p.cam;
    let (clip, section) = match s.section {
        Some(sec) => {
            let n = sec.normal.normalize_or_zero();
            (v4(n.as_vec3().to_array(), n.dot(sec.origin - cam.target) as f32), 1)
        }
        None => ([0.0; 4], 0),
    };
    CullU {
        view_proj: view_proj.as_mat4().to_cols_array_2d(),
        eye: v4(cam.eye_rel().as_vec3().to_array(), if cam.perspective { 1.0 } else { 0.0 }),
        lod: [lod_scale(cam, scale) as f32, 0.0, 0.0, 0.0],
        clip,
        // Occluders for the prepass: at least a fortieth of the view's smaller side across
        // (27 px at 1080 p). Every visible instance as an occluder cost more than it saved
        // in the 10k benchmark's open layouts; large ones only saved nothing in its dense one.
        screen: [size.0, size.1, margin, size.0.min(size.1) / 40.0],
        mode: [phase as u32, hiz, section, count],
        opts: [on as u32, phase.mask(), 0, 0],
    }
}
