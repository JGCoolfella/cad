//! Picking rules of `Viewport.pick()` in viewport.ts.
//!
//! The scene's pick geometry is rendered into a [`PICK`]×[`PICK`] logical-pixel ID window
//! around the pointer (R32G32 = pick word, index); for each kind the hit nearest the
//! centre is kept; then kinds are tried in priority order, each within its own radius:
//!
//! | order | kind | radius (px) | |
//! |---|---|---|---|
//! | 0 | origin | 5 | screen distance to the projected origin, before the ID pass, when origins are shown and no sketch is open |
//! | 1 | vertex | 5 | not when only edges are wanted (Chamfer) |
//! | 2 | edge | 4 | |
//! | 3 | sketch | 4 | |
//! | 4 | face | 1.5 | not when only edges are wanted |
//! | 5 | reference axis, point, coordinate system | 4 | |
//! | 6 | plane | 1.5 | |
//!
//! With an active selection filter only the filtered kinds are tried and every radius
//! doubles. Occlusion comes from the depth buffer: faces sit slightly behind their edges
//! and vertices (depth bias), so an edge on a visible face wins, a hidden one cannot.
use crate::batch::{K_EDGE, K_FACE, K_PLANE, K_REF, K_SKETCH, K_VERTEX};
use crate::scene::Hit;
use glam::DMat4;

/// Side of the pick window in logical pixels.
pub const PICK: u32 = 13;

/// Entity kinds for selection filters (`selFilter`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PickKind {
    Face,
    Edge,
    Vertex,
    Plane,
    Sketch,
    Ref,
}

/// What may be picked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickFilter {
    /// The selection filter when it is on and not empty (`app.filterOn && selFilter.size`).
    pub only: Option<Vec<PickKind>>,
    /// Only edges (the Chamfer PropertyManager): no vertices, no faces.
    pub edges_only: bool,
}

impl PickFilter {
    fn ok(&self, k: PickKind) -> bool {
        self.only.as_ref().is_none_or(|v| v.contains(&k))
    }
}

/// Nearest hit of one kind in the window: distance from the centre (px), word, index.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Near {
    pub d: f64,
    pub word: u32,
    pub index: u32,
}

/// Nearest hit per kind (indexed by kind, 0 unused) from an ID window, row-major from the
/// top-left, the first strictly nearest pixel winning, as in viewport.ts.
pub(crate) fn nearest(ids: &[[u32; 2]], side: u32) -> [Option<Near>; 7] {
    let half = (side / 2) as f64;
    let mut best: [Option<Near>; 7] = [None; 7];
    for y in 0..side {
        for x in 0..side {
            let [w, index] = ids[(y * side + x) as usize];
            let kind = (w >> 24) as usize;
            if w == 0 || kind >= best.len() {
                continue;
            }
            let d = (x as f64 - half).hypot(y as f64 - half);
            if best[kind].is_none_or(|b| d < b.d) {
                best[kind] = Some(Near { d, word: w, index });
            }
        }
    }
    best
}

/// A picked entity before ids are mapped: kind, slot (body index for model topology),
/// index (sub-entity or the caller's id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Raw {
    pub kind: u32,
    pub slot: u32,
    pub index: u32,
}

/// The priority rule.
pub(crate) fn resolve(best: &[Option<Near>; 7], f: &PickFilter) -> Option<Raw> {
    let w = if f.only.is_some() { 2.0 } else { 1.0 };
    let order = [
        (K_VERTEX, PickKind::Vertex, 5.0, !f.edges_only),
        (K_EDGE, PickKind::Edge, 4.0, true),
        (K_SKETCH, PickKind::Sketch, 4.0, true),
        (K_FACE, PickKind::Face, 1.5, !f.edges_only),
        (K_REF, PickKind::Ref, 4.0, true),
        (K_PLANE, PickKind::Plane, 1.5, true),
    ];
    for (k, kind, radius, allowed) in order {
        if !allowed || !f.ok(kind) {
            continue;
        }
        if let Some(n) = best[k as usize].filter(|n| n.d <= radius * w) {
            return Some(Raw {
                kind: k,
                slot: n.word & 0x00ff_ffff,
                index: n.index,
            });
        }
    }
    None
}

/// Maps a raw hit to the caller's ids (`body_ids[slot]` for model topology).
pub(crate) fn to_hit(r: Raw, body_ids: &[u32]) -> Option<Hit> {
    let body = || body_ids.get(r.slot as usize).copied();
    Some(match r.kind {
        K_FACE => Hit::Face { body: body()?, index: r.index },
        K_EDGE => Hit::Edge { body: body()?, index: r.index },
        K_VERTEX => Hit::Vertex { body: body()?, index: r.index },
        K_PLANE => Hit::Plane(r.index),
        K_SKETCH | K_REF => Hit::Feature(r.index),
        _ => return None,
    })
}

/// Clip-space crop mapping the `PICK`-pixel window whose top-left logical pixel is
/// (x0, y0) onto the whole render target (three.js `setViewOffset`).
pub(crate) fn crop(width: f64, height: f64, x0: f64, y0: f64) -> DMat4 {
    let n = PICK as f64;
    let (sx, sy) = (width / n, height / n);
    let tx = sx - 1.0 - 2.0 * x0 / n;
    let ty = 1.0 - sy + 2.0 * y0 / n;
    DMat4::from_cols_array(&[
        sx, 0.0, 0.0, 0.0, 0.0, sy, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, tx, ty, 0.0, 1.0,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::word;

    fn window(hits: &[(u32, u32, u32, u32, u32)]) -> Vec<[u32; 2]> {
        // (x, y, kind, slot, index)
        let mut v = vec![[0u32; 2]; (PICK * PICK) as usize];
        for &(x, y, k, s, i) in hits {
            v[(y * PICK + x) as usize] = [word(k, s), i];
        }
        v
    }

    fn pick(hits: &[(u32, u32, u32, u32, u32)], f: &PickFilter) -> Option<Raw> {
        resolve(&nearest(&window(hits), PICK), f)
    }

    #[test]
    fn priority_and_radii() {
        let none = PickFilter::default();
        // Face under the pointer, an edge 3 px away: the edge wins.
        let r = pick(&[(6, 6, K_FACE, 0, 7), (9, 6, K_EDGE, 0, 2)], &none).unwrap();
        assert_eq!((r.kind, r.index), (K_EDGE, 2));
        // A vertex 5 px away beats an edge under the pointer.
        let r = pick(&[(6, 6, K_EDGE, 0, 2), (6, 1, K_VERTEX, 0, 4)], &none).unwrap();
        assert_eq!((r.kind, r.index), (K_VERTEX, 4));
        // 5 px diagonally (4.24) is not within the face radius; only the centre is.
        assert_eq!(pick(&[(3, 3, K_FACE, 0, 7)], &none), None);
        assert_eq!(pick(&[(7, 7, K_FACE, 0, 7)], &none).unwrap().kind, K_FACE);
        // Sketch before face, face before reference geometry, reference before plane.
        let r = pick(&[(6, 6, K_FACE, 0, 1), (6, 9, K_SKETCH, 0, 11)], &none).unwrap();
        assert_eq!(r.kind, K_SKETCH);
        let r = pick(&[(6, 6, K_FACE, 0, 1), (6, 8, K_REF, 0, 12)], &none).unwrap();
        assert_eq!(r.kind, K_FACE);
        let r = pick(&[(6, 6, K_PLANE, 0, 3), (6, 8, K_REF, 0, 12)], &none).unwrap();
        assert_eq!(r.kind, K_REF);
        // Nearest pixel of a kind is the one used.
        let r = pick(&[(0, 0, K_EDGE, 0, 1), (8, 6, K_EDGE, 0, 2)], &none).unwrap();
        assert_eq!(r.index, 2);
    }

    #[test]
    fn filters_double_the_radius_and_restrict_kinds() {
        let faces = PickFilter { only: Some(vec![PickKind::Face]), edges_only: false };
        let hits = [(6, 3, K_FACE, 0, 1), (6, 6, K_EDGE, 0, 2)];
        let r = pick(&hits, &faces).unwrap();
        assert_eq!((r.kind, r.index), (K_FACE, 1));
        let chamfer = PickFilter { only: None, edges_only: true };
        let r = pick(&[(6, 6, K_VERTEX, 0, 1), (6, 6 + 3, K_EDGE, 0, 2)], &chamfer).unwrap();
        assert_eq!(r.kind, K_EDGE);
        assert_eq!(pick(&[(6, 6, K_FACE, 0, 1)], &chamfer), None);
    }

    #[test]
    fn hits_map_to_caller_ids() {
        let r = Raw { kind: K_FACE, slot: 1, index: 9 };
        assert_eq!(to_hit(r, &[40, 41]), Some(Hit::Face { body: 41, index: 9 }));
        assert_eq!(to_hit(Raw { kind: K_PLANE, slot: 0, index: 5 }, &[]), Some(Hit::Plane(5)));
        assert_eq!(to_hit(Raw { kind: K_REF, slot: 0, index: 6 }, &[]), Some(Hit::Feature(6)));
        assert_eq!(to_hit(r, &[]), None);
    }

    #[test]
    fn crop_maps_the_window_to_ndc() {
        let m = crop(800.0, 600.0, 100.0, 50.0);
        let ndc = |px: f64, py: f64| glam::DVec4::new(px / 800.0 * 2.0 - 1.0, 1.0 - py / 600.0 * 2.0, 0.0, 1.0);
        let a = m * ndc(100.0, 50.0);
        let b = m * ndc(113.0, 63.0);
        assert!((a.x + 1.0).abs() < 1e-12 && (a.y - 1.0).abs() < 1e-12);
        assert!((b.x - 1.0).abs() < 1e-12 && (b.y + 1.0).abs() < 1e-12);
    }
}
