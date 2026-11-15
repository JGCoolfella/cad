//! The 3D View Selector (SOLIDWORKS: View Orientation > View Selector, Ctrl+Space), ported
//! from `src/render/viewSelector.ts`.
//!
//! A translucent bevelled cube drawn at the centre of the viewport with the view's
//! rotation, 0.17 × the short side of the viewport per cube unit. It has 26 facets: 6
//! faces, 12 edge chamfers and 8 corner cuts. A face pointing away from the viewer is
//! repeated as a floating panel outside the cube, so every direction can be picked from
//! any view. The hovered facet is tinted light blue; clicking one orients the view along
//! its normal (the caller does that with [`up_for`]).
//!
//! The cube is screen-space overlay geometry (no depth test), drawn in the order three.js
//! drew it: every object is transparent at the same distance, so creation order.
use crate::camera::{Camera, Ray};
use crate::color::Rgb;
use crate::overlay::Painter;
use crate::raycast::tri;
use glam::{DQuat, DVec2, DVec3};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FacetKind {
    Face,
    Edge,
    Corner,
}

/// One clickable facet.
#[derive(Clone, Debug, PartialEq)]
pub struct Facet {
    /// Unit normal: the view looks along it (from this side) when the facet is clicked.
    pub normal: DVec3,
    pub kind: FacetKind,
    /// "Front", "Top", … for the six faces (the view name shown after clicking).
    pub name: Option<&'static str>,
    /// Polygon in cube units (convex, counter-clockwise or clockwise).
    pub poly: Vec<DVec3>,
    /// Faces only: the floating panel shown while the face points away from the viewer.
    pub panel: Option<Vec<DVec3>>,
}

/// Edge chamfer size along each axis and corner cut along the diagonal (viewSelector.ts).
const B: f64 = 0.34;
const CC: f64 = 0.78;
/// Pixels per cube unit, as a fraction of the viewport's short side.
const SIZE: f64 = 0.17;
const FILL: &str = "#c9cbd0";
const HOVER: &str = "#aebdfa";
const OUTLINE: &str = "#1b1b1b";
const OUTLINE_HOVER: &str = "#3048e0";

struct HalfSpace {
    n: DVec3,
    d: f64,
    kind: FacetKind,
}

fn face_name(n: DVec3) -> Option<&'static str> {
    let s = |x: f64| if x > 0.5 { 1 } else if x < -0.5 { -1 } else { 0 };
    Some(match (s(n.x), s(n.y), s(n.z)) {
        (1, 0, 0) => "Right",
        (-1, 0, 0) => "Left",
        (0, 1, 0) => "Top",
        (0, -1, 0) => "Bottom",
        (0, 0, 1) => "Front",
        (0, 0, -1) => "Back",
        _ => return None,
    })
}

/// A big square in the plane of `all[i]`, clipped by every other half-space n·x ≤ d.
fn clip_facet(i: usize, all: &[HalfSpace]) -> Vec<DVec3> {
    let p = &all[i];
    let o = p.n * p.d;
    let a = if p.n.x.abs() < 0.9 { DVec3::X } else { DVec3::Y };
    let u = (a - p.n * a.dot(p.n)).normalize();
    let v = p.n.cross(u);
    let mut poly = vec![o - u * 4.0 - v * 4.0, o + u * 4.0 - v * 4.0, o + u * 4.0 + v * 4.0, o - u * 4.0 + v * 4.0];
    for (j, q) in all.iter().enumerate() {
        if j == i {
            continue;
        }
        let mut out = vec![];
        for k in 0..poly.len() {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            let (da, db) = (q.n.dot(a) - q.d, q.n.dot(b) - q.d);
            if da <= 1e-9 {
                out.push(a);
            }
            if (da < -1e-9 && db > 1e-9) || (da > 1e-9 && db < -1e-9) {
                out.push(a.lerp(b, da / (da - db)));
            }
        }
        poly = out;
        if poly.len() < 3 {
            return vec![];
        }
    }
    poly
}

fn build() -> Vec<Facet> {
    let mut planes = vec![];
    for x in -1i32..=1 {
        for y in -1i32..=1 {
            for z in -1i32..=1 {
                let k = x.abs() + y.abs() + z.abs();
                if k == 0 {
                    continue;
                }
                let n = DVec3::new(x as f64, y as f64, z as f64).normalize();
                let (d, kind) = match k {
                    1 => (1.0, FacetKind::Face),
                    2 => ((2.0 - B) / std::f64::consts::SQRT_2, FacetKind::Edge),
                    _ => ((3.0 - CC) / 3f64.sqrt(), FacetKind::Corner),
                };
                planes.push(HalfSpace { n, d, kind });
            }
        }
    }
    let mut out = vec![];
    for (i, p) in planes.iter().enumerate() {
        let poly = clip_facet(i, &planes);
        if poly.len() < 3 {
            continue;
        }
        let (name, panel) = if p.kind == FacetKind::Face {
            let shrink = poly.iter().map(|&v| (v - p.n * p.d) * 0.92 + p.n * (p.d + 0.95)).collect();
            (face_name(p.n), Some(shrink))
        } else {
            (None, None)
        };
        out.push(Facet { normal: p.n, kind: p.kind, name, poly, panel });
    }
    out
}

/// The 26 facets, in the order `viewSelector.ts` makes them (x, y, z from −1 to 1).
pub fn facets() -> &'static [Facet] {
    static F: OnceLock<Vec<Facet>> = OnceLock::new();
    F.get_or_init(build)
}

/// Camera up vector when looking from a facet's direction (`ViewSelector.upFor`).
pub fn up_for(n: DVec3) -> DVec3 {
    if n.y.abs() > 0.9 && n.x.abs() < 0.2 && n.z.abs() < 0.2 {
        DVec3::new(0.0, 0.0, if n.y > 0.0 { -1.0 } else { 1.0 })
    } else {
        DVec3::Y
    }
}

/// The selector's own orthographic view: the camera's rotation, centred in a viewport of
/// `w` × `h` logical px.
struct View {
    q: DQuat,
    inv: DQuat,
    c: DVec2,
    s: f64,
}

impl View {
    fn new(q: DQuat, w: f64, h: f64) -> View {
        View { q, inv: q.inverse(), c: DVec2::new(w / 2.0, h / 2.0), s: w.min(h) * SIZE }
    }
    fn project(&self, p: DVec3) -> DVec2 {
        let v = self.inv * p;
        DVec2::new(self.c.x + v.x * self.s, self.c.y - v.y * self.s)
    }
    /// Panels are shown for faces turned away from the viewer.
    fn panel_shown(&self, f: &Facet) -> bool {
        f.panel.is_some() && f.normal.dot(self.q * DVec3::Z) < 0.15
    }
}

fn fan(poly: &[DVec3]) -> impl Iterator<Item = [DVec3; 3]> + '_ {
    (1..poly.len().saturating_sub(1)).map(move |i| [poly[0], poly[i], poly[i + 1]])
}

/// Facet under logical pixel (sx, sy) of the viewport `camera` describes (its rotation and
/// pixel size), nearest the viewer; panels count while shown.
pub fn pick(camera: &Camera, sx: f64, sy: f64) -> Option<usize> {
    let v = View::new(camera.quat, camera.width_px, camera.height_px);
    let local = DVec3::new((sx - v.c.x) / v.s, (v.c.y - sy) / v.s, 10.0);
    let ray = Ray { origin: v.q * local, dir: v.q * DVec3::NEG_Z };
    let mut best: Option<(f64, usize)> = None;
    for (i, f) in facets().iter().enumerate() {
        let panel = f.panel.as_deref().filter(|_| v.panel_shown(f));
        for poly in std::iter::once(f.poly.as_slice()).chain(panel) {
            for [a, b, c] in fan(poly) {
                if let Some(t) = tri(&ray, a, b, c) {
                    if best.is_none_or(|(bt, _)| t < bt) {
                        best = Some((t, i));
                    }
                }
            }
        }
    }
    best.map(|(_, i)| i)
}

/// Draws the cube into the overlay: per facet its fill, its outline, then its panel.
pub(crate) fn paint(p: &mut Painter, camera: &Camera, hover: Option<u32>, line_width_px: f32) {
    let v = View::new(camera.quat, camera.width_px, camera.height_px);
    let width = line_width_px as f64 / p.scale;
    for (i, f) in facets().iter().enumerate() {
        let on = hover == Some(i as u32);
        let opacity = if on {
            0.75
        } else if f.kind == FacetKind::Face {
            0.32
        } else {
            0.42
        };
        let fill = Rgb::hex(if on { HOVER } else { FILL }).with_alpha(opacity);
        let line = Rgb::hex(if on { OUTLINE_HOVER } else { OUTLINE }).with_alpha(0.85);
        let panel = f.panel.as_deref().filter(|_| v.panel_shown(f));
        for poly in std::iter::once(f.poly.as_slice()).chain(panel) {
            let s: Vec<DVec2> = poly.iter().map(|&q| v.project(q)).collect();
            for k in 1..s.len().saturating_sub(1) {
                p.tri(s[0], s[k], s[k + 1], fill);
            }
            for k in 0..s.len() {
                p.line(s[k], s[(k + 1) % s.len()], width, line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(view: &str) -> Camera {
        let mut c = Camera::default();
        c.resize(800.0, 600.0);
        c.set_view(view, None);
        c
    }

    #[test]
    fn twenty_six_facets() {
        let f = facets();
        assert_eq!(f.len(), 26);
        let count = |k| f.iter().filter(|x| x.kind == k).count();
        assert_eq!((count(FacetKind::Face), count(FacetKind::Edge), count(FacetKind::Corner)), (6, 12, 8));
        let mut names: Vec<_> = f.iter().filter_map(|x| x.name).collect();
        names.sort();
        assert_eq!(names, ["Back", "Bottom", "Front", "Left", "Right", "Top"]);
        // Facets of a kind are congruent (same vertex count), inside the unit cube, and lie
        // in their plane.
        for k in [FacetKind::Face, FacetKind::Edge, FacetKind::Corner] {
            let n: Vec<usize> = f.iter().filter(|x| x.kind == k).map(|x| x.poly.len()).collect();
            assert!(n[0] >= 3 && n.iter().all(|&m| m == n[0]), "{k:?} {n:?}");
        }
        for x in f {
            assert!(x.poly.iter().all(|p| p.abs().max_element() <= 1.0 + 1e-9));
            let d = x.normal.dot(x.poly[0]);
            assert!(x.poly.iter().all(|p| (x.normal.dot(*p) - d).abs() < 1e-9));
        }
    }

    #[test]
    fn picks_the_facet_facing_the_viewer() {
        // Front view: the centre is the Front face; the cube's half side is 0.17 × 600 px.
        let c = cam("front");
        let i = pick(&c, 400.0, 300.0).unwrap();
        assert_eq!(facets()[i].name, Some("Front"));
        // Just inside the right side: the Right-Front chamfer (x and z), not a face.
        let s = 600.0 * SIZE;
        let i = pick(&c, 400.0 + s * 0.95, 300.0).unwrap();
        assert_eq!(facets()[i].kind, FacetKind::Edge);
        assert!((facets()[i].normal - DVec3::new(1.0, 0.0, 1.0).normalize()).length() < 1e-9);
        // The Back face is a panel behind the cube, 1.95 units out: seen from the front it is
        // hidden by the cube; outside the cube (beyond 1 unit) nothing is there.
        assert_eq!(pick(&c, 400.0 + s * 1.5, 300.0), None);
        assert_eq!(pick(&c, 10.0, 10.0), None);
    }

    #[test]
    fn hidden_faces_show_panels() {
        // Isometric: Front, Top and Right face the viewer; Back, Bottom and Left are panels.
        let c = cam("isometric");
        let v = View::new(c.quat, c.width_px, c.height_px);
        let shown: Vec<_> = facets().iter().filter(|f| v.panel_shown(f)).filter_map(|f| f.name).collect();
        assert_eq!(shown.len(), 3);
        for n in ["Back", "Bottom", "Left"] {
            assert!(shown.contains(&n), "{shown:?}");
        }
        // Each panel can be picked on its far side from the cube (its centre may lie under
        // the cube's silhouette).
        for f in facets().iter().filter(|f| v.panel_shown(f)) {
            let pts: Vec<DVec2> = f.panel.as_ref().unwrap().iter().map(|&q| v.project(q)).collect();
            let centre = pts.iter().fold(DVec2::ZERO, |a, &b| a + b) / pts.len() as f64;
            let far = pts.iter().copied().max_by(|a, b| a.distance(v.c).total_cmp(&b.distance(v.c))).unwrap();
            let s = centre.lerp(far, 0.8);
            assert_eq!(pick(&c, s.x, s.y).map(|i| facets()[i].name), Some(f.name));
        }
    }

    #[test]
    fn up_vectors() {
        assert_eq!(up_for(DVec3::Y), DVec3::NEG_Z);
        assert_eq!(up_for(DVec3::NEG_Y), DVec3::Z);
        assert_eq!(up_for(DVec3::new(1.0, 1.0, 1.0).normalize()), DVec3::Y);
    }

    #[test]
    fn paints_hover_in_light_blue() {
        let c = cam("front");
        let mut p = Painter { scale: 1.25, out: vec![] };
        let front = facets().iter().position(|f| f.name == Some("Front")).unwrap() as u32;
        paint(&mut p, &c, Some(front), 1.5);
        let blue = Rgb::hex(HOVER).with_alpha(0.75);
        assert!(p.out.iter().any(|v| v.color == blue));
        // Physical pixels: the cube centre lands at the scaled viewport centre.
        let xs: Vec<f32> = p.out.iter().filter(|v| v.color == blue).map(|v| v.pos[0]).collect();
        let mid = (xs.iter().cloned().fold(f32::MAX, f32::min) + xs.iter().cloned().fold(f32::MIN, f32::max)) / 2.0;
        assert!((mid - 400.0 * 1.25).abs() < 0.5, "{mid}");
    }
}
