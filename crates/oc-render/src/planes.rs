//! Reference geometry sizing, ported from `src/render/planes.ts` and `buildRefGeometry`:
//! plane extents from the model box, the in-plane name label's rectangle, plane border
//! widths and the reference axis length.
use crate::camera::Aabb;
use crate::scene::Frame;

/// Auto plane size: the model's footprint on the plane plus a margin, always including
/// the plane's origin and never tiny (`autoExtents`, `minHalf` = 50).
pub fn auto_extents(frame: &Frame, model: Option<&Aabb>, min_half: f64) -> [f64; 4] {
    let Some(b) = model.filter(|b| !b.is_empty()) else {
        return [-min_half, min_half, -min_half, min_half];
    };
    // Footprint of the box's corners in the plane; planes through the origin always
    // include it (the range starts at 0).
    let (mut a, mut bb, mut c, mut d) = (0f64, 0f64, 0f64, 0f64);
    for p in b.corners() {
        let rel = p - frame.origin;
        let (u, v) = (rel.dot(frame.x), rel.dot(frame.y));
        (a, bb, c, d) = (a.min(u), bb.max(u), c.min(v), d.max(v));
    }
    let m = (bb - a).max(d - c) * 0.12 + 2.0;
    let grow = |lo: f64, hi: f64| {
        let mid = (lo + hi) / 2.0;
        let half = ((hi - lo) / 2.0).max(min_half * 0.1);
        (mid - half, mid + half)
    };
    let (u0, u1) = grow(a - m, bb + m);
    let (v0, v1) = grow(c - m, d + m);
    [u0, u1, v0, v1]
}

/// Rectangle `[u0, u1, v0, v1]` of a plane's name label: at the top-left corner, height
/// 7.5 % of the plane's smaller side, width from the label image's aspect ratio.
pub fn label_rect(ext: [f64; 4], aspect: f64) -> [f64; 4] {
    let [u0, u1, v0, v1] = ext;
    let span = (u1 - u0).min(v1 - v0);
    let h = (span * 0.075).max(1e-3);
    let w = h * aspect;
    let pad = h * 0.35;
    let (x0, y1) = (u0 + pad, v1 - pad * 0.6);
    [x0, x0 + w, y1 - h, y1]
}

/// Plane border width in logical pixels: hovered 2.4, selected 1.6, else 1.1.
pub fn border_width(hover: bool, selected: bool) -> f64 {
    if hover {
        2.4
    } else if selected {
        1.6
    } else {
        1.1
    }
}

/// Plane fill opacity: `(100 − transparency) / 100`, at least 0.03; 0 when shaded planes
/// are off (the fill is not drawn).
pub fn fill_opacity(shaded: bool, transparency: f64) -> f64 {
    if shaded {
        ((100.0 - transparency) / 100.0).max(0.03)
    } else {
        0.0
    }
}

/// Length of reference axes: 0.6 × the model box diagonal, at least 10 mm.
pub fn ref_axis_length(model: Option<&Aabb>) -> f64 {
    (model.map_or(0.0, |b| b.diagonal()) * 0.6).max(10.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::DVec3;

    #[test]
    fn extents_cover_the_model_and_the_origin() {
        let f = Frame::front();
        assert_eq!(auto_extents(&f, None, 50.0), [-50.0, 50.0, -50.0, 50.0]);
        let b = Aabb {
            min: DVec3::new(10.0, 10.0, 0.0),
            max: DVec3::new(30.0, 20.0, 5.0),
        };
        // Footprint u 0..30 (origin included), v 0..20; margin 30 × 0.12 + 2 = 5.6.
        let e = auto_extents(&f, Some(&b), 50.0);
        let want = [-5.6, 35.6, -5.6, 25.6];
        for k in 0..4 {
            assert!((e[k] - want[k]).abs() < 1e-12, "{e:?}");
        }
        // A tiny model is grown to half-size ≥ 5 (minHalf × 0.1).
        let t = Aabb {
            min: DVec3::ZERO,
            max: DVec3::new(0.1, 0.1, 0.1),
        };
        let e = auto_extents(&f, Some(&t), 50.0);
        assert!((e[1] - e[0] - 10.0).abs() < 1e-12);
    }

    #[test]
    fn label_sits_in_the_top_left_corner() {
        let r = label_rect([-50.0, 50.0, -50.0, 50.0], 4.0);
        // h = 7.5, pad = 2.625, top = 50 − 0.6 pad.
        let want = [-47.375, -17.375, 40.925, 48.425];
        for k in 0..4 {
            assert!((r[k] - want[k]).abs() < 1e-9, "{r:?}");
        }
        assert_eq!(border_width(true, true), 2.4);
        assert!((fill_opacity(true, 82.0) - 0.18).abs() < 1e-12);
        assert_eq!(fill_opacity(true, 100.0), 0.03);
        assert_eq!(ref_axis_length(None), 10.0);
    }
}
