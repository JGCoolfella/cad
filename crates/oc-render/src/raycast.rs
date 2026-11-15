//! CPU queries against display meshes (level 0), for what viewport.ts does on the CPU:
//! ray casts (its three.js `Raycaster`) for Select Other (every face along the ray, front
//! to back, hidden ones too), the rotate-about point and Hole Wizard positions; and box
//! selection of model edges (`boxSelectModel`).
use crate::camera::{Camera, Ray};
use crate::renderer::Renderer;
use crate::scene::{BodyRole, Hit, Scene};
use glam::DVec3;

/// A face crossed by a ray, `t` millimetres from the ray's origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub hit: Hit,
    pub t: f64,
    pub point: DVec3,
}

/// Möller–Trumbore, both sides.
pub(crate) fn tri(ray: &Ray, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
    let (e1, e2) = (b - a, c - a);
    let p = ray.dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = ray.origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = ray.dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some(t)
}

impl Renderer {
    /// Faces of the part bodies along `ray`, nearest first, each face once (at its
    /// nearest crossing). Uses level 0. Section clipping is not applied (as today).
    pub fn raycast_faces(&self, scene: &Scene, ray: &Ray) -> Vec<RayHit> {
        let mut out: Vec<RayHit> = vec![];
        for b in scene.bodies.iter().filter(|b| b.role == BodyRole::Part) {
            let Some(m) = self.meshes.get(&b.mesh) else { continue };
            let mesh = &m.cpu;
            let Some(l) = mesh.lods.first() else { continue };
            // Bounding sphere rejection.
            let o = DVec3::from(mesh.origin);
            let to = o - ray.origin;
            let along = to.dot(ray.dir);
            if (to - ray.dir * along).length() > mesh.radius as f64 + 1e-6 {
                continue;
            }
            let pos = |i: u32| {
                let v = l.vertex(i as usize).position;
                o + DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64)
            };
            for (fi, f) in l.faces.iter().enumerate() {
                let mut best: Option<f64> = None;
                for t in (f.first_index / 3)..((f.first_index + f.index_count) / 3) {
                    let [i, j, k] = l.triangle(t as usize);
                    if let Some(d) = tri(ray, pos(i), pos(j), pos(k)) {
                        best = Some(best.map_or(d, |x: f64| x.min(d)));
                    }
                }
                if let Some(t) = best {
                    out.push(RayHit {
                        hit: Hit::Face { body: b.id, index: fi as u32 },
                        t,
                        point: ray.origin + ray.dir * t,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.t.total_cmp(&b.t));
        out
    }

    /// Model edges of the part bodies in the screen box between corners `from` and `to`
    /// (logical px, in drag order). A window box (`crossing` false: dragged left to right in
    /// SOLIDWORKS) takes edges entirely inside; a crossing box (right to left) edges any of
    /// whose segments touches the box. (viewport.ts tests only the segments' end points,
    /// so its crossing box misses a long straight edge crossed in the middle.) Seams and
    /// degenerate edges are left out. Level 0, by edge index.
    pub fn box_edges(&self, scene: &Scene, camera: &Camera, from: [f64; 2], to: [f64; 2], crossing: bool) -> Vec<Hit> {
        let (xa, xb, ya, yb) = (from[0].min(to[0]), from[0].max(to[0]), from[1].min(to[1]), from[1].max(to[1]));
        let inside = |[x, y]: [f64; 2]| x >= xa && x <= xb && y >= ya && y <= yb;
        // Liang–Barsky: does the screen segment a–b meet the box?
        let crosses = |a: [f64; 2], b: [f64; 2]| {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let (mut t0, mut t1) = (0.0f64, 1.0f64);
            for (p, q) in [(-dx, a[0] - xa), (dx, xb - a[0]), (-dy, a[1] - ya), (dy, yb - a[1])] {
                if p == 0.0 {
                    if q < 0.0 {
                        return false;
                    }
                } else {
                    let t = q / p;
                    if p < 0.0 {
                        t0 = t0.max(t);
                    } else {
                        t1 = t1.min(t);
                    }
                }
            }
            t0 <= t1
        };
        let mut out = vec![];
        for b in scene.bodies.iter().filter(|b| b.role == BodyRole::Part) {
            let Some(m) = self.meshes.get(&b.mesh) else { continue };
            let mesh = &m.cpu;
            let Some(l) = mesh.lods.first() else { continue };
            let o = DVec3::from(mesh.origin);
            let at = |v: [f32; 3]| o + DVec3::new(v[0] as f64, v[1] as f64, v[2] as f64);
            for (e, r) in l.edges.iter().enumerate() {
                let flags = mesh.edge_flags.get(e).copied().unwrap_or(0);
                if flags & (oc_mesh::EDGE_SEAM | oc_mesh::EDGE_DEGENERATE) != 0 || r.segment_count == 0 {
                    continue;
                }
                let mut segs = (r.first_segment..r.first_segment + r.segment_count).map(|i| {
                    let s = l.segment(i as usize);
                    (camera.project(at(s.a)), camera.project(at(s.b)))
                });
                let ok = if crossing {
                    segs.any(|(a, b)| crosses(a, b))
                } else {
                    segs.all(|(a, b)| inside(a) && inside(b))
                };
                if ok {
                    out.push(Hit::Edge { body: b.id, index: e as u32 });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangle_hits_from_both_sides() {
        let (a, b, c) = (DVec3::ZERO, DVec3::X, DVec3::Y);
        let down = Ray { origin: DVec3::new(0.2, 0.2, 5.0), dir: DVec3::NEG_Z };
        assert_eq!(tri(&down, a, b, c), Some(5.0));
        let up = Ray { origin: DVec3::new(0.2, 0.2, -2.0), dir: DVec3::Z };
        assert_eq!(tri(&up, a, b, c), Some(2.0));
        let miss = Ray { origin: DVec3::new(0.8, 0.8, 5.0), dir: DVec3::NEG_Z };
        assert_eq!(tri(&miss, a, b, c), None);
    }
}
