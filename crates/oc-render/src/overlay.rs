//! Screen-space markers that `viewport.ts` draws on its 2D canvas: the part origin
//! (`drawOrigin`) and the reference triad (`drawTriad`). They are tessellated here into
//! triangles in physical pixels and drawn last, over everything. The triad's X, Y and Z
//! letters are stroked glyphs (no font dependency).
use crate::batch::OverV;
use crate::camera::Camera;
use crate::color::Rgb;
use crate::scene::{Hit, Scene};
use glam::{DVec2, DVec3};

pub(crate) struct Painter {
    pub scale: f64,
    pub out: Vec<OverV>,
}

impl Painter {
    fn v(&self, p: DVec2, c: [f32; 4]) -> OverV {
        OverV {
            pos: (p * self.scale).as_vec2().to_array(),
            color: c,
        }
    }
    pub fn tri(&mut self, a: DVec2, b: DVec2, c: DVec2, col: [f32; 4]) {
        let v = [self.v(a, col), self.v(b, col), self.v(c, col)];
        self.out.extend(v);
    }
    /// A stroke `width` logical px wide, butt ends (canvas default).
    pub fn line(&mut self, a: DVec2, b: DVec2, width: f64, col: [f32; 4]) {
        let d = b - a;
        if d.length() < 1e-9 {
            return;
        }
        let n = d.perp().normalize() * (width / 2.0);
        self.tri(a - n, b - n, b + n, col);
        self.tri(a - n, b + n, a + n, col);
    }
    pub fn disc(&mut self, c: DVec2, r: f64, col: [f32; 4]) {
        let k = 16;
        for i in 0..k {
            let t = |j: usize| std::f64::consts::TAU * j as f64 / k as f64;
            let p = |j: usize| c + DVec2::new(t(j).cos(), t(j).sin()) * r;
            self.tri(c, p(i), p(i + 1), col);
        }
    }
    /// Arrowhead whose base is at `t`, pointing along unit `u`: tip `t + u·len`, half base
    /// width `half` (the canvas paths in viewport.ts).
    fn arrow(&mut self, t: DVec2, u: DVec2, len: f64, half: f64, col: [f32; 4]) {
        let tip = t + u * len;
        let a = DVec2::new(t.x - u.y * half, t.y + u.x * half);
        let b = DVec2::new(t.x + u.y * half, t.y - u.x * half);
        self.tri(tip, a, b, col);
    }
    /// Bold 11 px X, Y or Z centred at `c` (stroked; about 7 × 8 px).
    fn glyph(&mut self, ch: char, c: DVec2, col: [f32; 4]) {
        let (w, h, s) = (3.4, 4.0, 1.7);
        let p = |x: f64, y: f64| c + DVec2::new(x * w, y * h);
        let strokes: &[(DVec2, DVec2)] = match ch {
            'X' => &[(p(-1.0, -1.0), p(1.0, 1.0)), (p(1.0, -1.0), p(-1.0, 1.0))],
            'Y' => &[(p(-1.0, -1.0), p(0.0, 0.0)), (p(1.0, -1.0), p(0.0, 0.0)), (p(0.0, 0.0), p(0.0, 1.0))],
            'Z' => &[(p(-1.0, -1.0), p(1.0, -1.0)), (p(1.0, -1.0), p(-1.0, 1.0)), (p(-1.0, 1.0), p(1.0, 1.0))],
            _ => &[],
        };
        for &(a, b) in strokes {
            self.line(a, b, s, col);
        }
    }
}

fn xy(p: [f64; 2]) -> DVec2 {
    DVec2::new(p[0], p[1])
}

/// The part origin: three short arrows (24 logical px of model length) and a dot, blue,
/// orange on hover, selection blue when selected.
pub(crate) fn origin(p: &mut Painter, scene: &Scene, cam: &Camera) {
    let (hov, sel) = (scene.is_hovered(Hit::Origin), scene.is_selected(Hit::Origin));
    if !(scene.origin.shown || hov || sel) {
        return;
    }
    let o = &scene.options;
    let col = if sel {
        o.selected
    } else if hov {
        o.hover
    } else {
        Rgb::hex("#1c3fd6")
    }
    .with_alpha(1.0);
    let width = if hov || sel { 2.0 } else { 1.4 };
    let c = xy(cam.project(DVec3::ZERO));
    let wpp = cam.world_per_pixel();
    for axis in [DVec3::X, DVec3::Y, DVec3::Z] {
        let t = xy(cam.project(axis * (wpp * 24.0)));
        let d = t - c;
        let len = d.length();
        if len < 3.0 {
            continue;
        }
        p.line(c, t, width, col);
        p.arrow(t, d / len, 5.0, 2.6, col);
    }
    p.disc(c, 2.2, col);
}

/// The reference triad at the bottom left: red X, green Y, blue Z, back to front.
pub(crate) fn triad(p: &mut Painter, cam: &Camera) {
    let (cx, cy, l) = (46.0, cam.height_px - 52.0, 30.0);
    let inv = cam.quat.inverse();
    let mut axes: Vec<(DVec3, &str, char)> = [(DVec3::X, "#e01010", 'X'), (DVec3::Y, "#10a010", 'Y'), (DVec3::Z, "#1030e0", 'Z')]
        .into_iter()
        .map(|(v, c, n)| (inv * v, c, n))
        .collect();
    axes.sort_by(|a, b| a.0.z.total_cmp(&b.0.z));
    let centre = DVec2::new(cx, cy);
    for (v, hex, name) in axes {
        let col = Rgb::hex(hex).with_alpha(1.0);
        let end = DVec2::new(cx + v.x * l, cy - v.y * l);
        p.line(centre, end, 2.0, col);
        let d = end - centre;
        let len = d.length();
        if len > 4.0 {
            p.arrow(end, d / len, 6.0, 3.0, col);
        }
        p.glyph(name, DVec2::new(cx + v.x * (l + 13.0), cy - v.y * (l + 13.0)), col);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triad_points_x_right_in_front_view() {
        let mut cam = Camera::default();
        cam.resize(400.0, 300.0);
        cam.set_view("front", None);
        let mut p = Painter { scale: 1.0, out: vec![] };
        triad(&mut p, &cam);
        // The red X arrow reaches 30 px right of the centre (46, 248), plus the arrowhead.
        let red = Rgb::hex("#e01010").with_alpha(1.0);
        let max_x = p.out.iter().filter(|v| v.color == red).map(|v| v.pos[0]).fold(0.0, f32::max);
        assert!((max_x - (46.0 + 30.0 + 13.0 + 3.4 + 0.9)).abs() < 1.0, "{max_x}");
        // Z points at the viewer: its arrow is a dot at the centre (no arrowhead, len 0).
        let mut q = Painter { scale: 2.0, out: vec![] };
        triad(&mut q, &cam);
        assert_eq!(q.out.len(), p.out.len());
        assert!((q.out[0].pos[0] - 2.0 * p.out[0].pos[0]).abs() < 1e-4);
    }
}
