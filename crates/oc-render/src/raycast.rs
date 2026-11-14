//! CPU ray casts against display meshes, for what viewport.ts does with a three.js
//! `Raycaster`: Select Other (every face along the ray, front to back, hidden ones too)
//! and the rotate-about point under the pointer.
use crate::camera::Ray;
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
fn tri(ray: &Ray, a: DVec3, b: DVec3, c: DVec3) -> Option<f64> {
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
