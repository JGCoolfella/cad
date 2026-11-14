//! The view camera, a port of `src/render/camera.ts`: orthographic by default (SOLIDWORKS'
//! default), perspective optional with a 30° vertical field of view; free (trackball)
//! rotation about a pivot, pan, zoom about the pointer, roll, standard orientations, fit
//! and eased transitions.
//!
//! Everything is `f64`. The GPU gets matrices relative to [`Camera::target`] (the render
//! origin), so `f32` only ever holds small numbers (plan section 1, P4's precision test).
//! Depth is reversed (near = 1, far = 0) into a float buffer.
//!
//! Screen coordinates are **logical** pixels (CSS pixels in the webview), x right, y down,
//! as in the TypeScript camera.
use glam::{DMat3, DMat4, DQuat, DVec3, DVec4};
use std::f64::consts::FRAC_1_SQRT_2;

/// Vertical field of view of the perspective camera (three.js `PerspectiveCamera(30, …)`).
pub const PERSPECTIVE_FOV_DEG: f64 = 30.0;
/// Depth of the orthographic view volume when the scene is empty (`FAR` in camera.ts).
pub const ORTHO_DEPTH: f64 = 1e5;

/// `(name, view direction (from the model towards the eye), up)`, as `STANDARD_VIEWS`.
pub const STANDARD_VIEWS: [(&str, [f64; 3], [f64; 3]); 9] = [
    ("front", [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ("back", [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
    ("left", [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ("right", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ("top", [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
    ("bottom", [0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
    ("isometric", [1.0, 1.0, 1.0], [0.0, 1.0, 0.0]),
    ("trimetric", [FRAC_1_SQRT_2, 0.5, FRAC_1_SQRT_2], [0.0, 1.0, 0.0]),
    ("dimetric", [1.0, 0.8165, 1.0], [0.0, 1.0, 0.0]),
];

/// An axis-aligned box in world millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: DVec3,
    pub max: DVec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        min: DVec3::splat(f64::INFINITY),
        max: DVec3::splat(f64::NEG_INFINITY),
    };
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
    pub fn add(&mut self, p: DVec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    pub fn union(&mut self, b: &Aabb) {
        if !b.is_empty() {
            self.add(b.min);
            self.add(b.max);
        }
    }
    pub fn corners(&self) -> [DVec3; 8] {
        std::array::from_fn(|i| {
            DVec3::new(
                if i & 1 != 0 { self.max.x } else { self.min.x },
                if i & 2 != 0 { self.max.y } else { self.min.y },
                if i & 4 != 0 { self.max.z } else { self.min.z },
            )
        })
    }
    pub fn center(&self) -> DVec3 {
        (self.min + self.max) * 0.5
    }
    pub fn diagonal(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            (self.max - self.min).length()
        }
    }
}

#[derive(Clone, Debug)]
struct Anim {
    t0: f64,
    dur: f64,
    q0: DQuat,
    q1: DQuat,
    tg0: DVec3,
    tg1: DVec3,
    h0: f64,
    h1: f64,
}

/// A ray in world space; `dir` is a unit vector into the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: DVec3,
    pub dir: DVec3,
}

#[derive(Clone, Debug)]
pub struct Camera {
    /// Point looked at; also the render origin of camera-relative GPU coordinates.
    pub target: DVec3,
    /// Orientation; the camera looks along −Z of this rotation.
    pub quat: DQuat,
    /// Visible world height in millimetres (orthographic; the perspective camera is placed
    /// so that the target plane shows the same height).
    pub height: f64,
    pub perspective: bool,
    /// Viewport size in logical pixels.
    pub width_px: f64,
    pub height_px: f64,
    /// Standard view the camera is at ("Isometric"), `None` after a free rotation.
    pub view_name: Option<String>,
    anim: Option<Anim>,
}

impl Default for Camera {
    fn default() -> Self {
        let mut c = Camera {
            target: DVec3::ZERO,
            quat: DQuat::IDENTITY,
            height: 200.0,
            perspective: false,
            width_px: 1.0,
            height_px: 1.0,
            view_name: None,
            anim: None,
        };
        c.set_view("isometric", None);
        c
    }
}

fn capitalised(name: &str) -> String {
    let mut c = name.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

impl Camera {
    pub fn resize(&mut self, width_px: f64, height_px: f64) {
        self.width_px = width_px.max(1.0);
        self.height_px = height_px.max(1.0);
    }

    pub fn aspect(&self) -> f64 {
        self.width_px / self.height_px.max(1.0)
    }

    /// Millimetres per logical pixel at the target depth.
    pub fn world_per_pixel(&self) -> f64 {
        self.height / self.height_px.max(1.0)
    }

    /// Unit vector from the target towards the eye.
    pub fn view_dir(&self) -> DVec3 {
        self.quat * DVec3::Z
    }
    pub fn right(&self) -> DVec3 {
        self.quat * DVec3::X
    }
    pub fn up(&self) -> DVec3 {
        self.quat * DVec3::Y
    }

    /// Eye distance of the perspective camera.
    pub fn distance(&self) -> f64 {
        (self.height / 2.0) / (PERSPECTIVE_FOV_DEG.to_radians() / 2.0).tan()
    }

    /// Eye position relative to the target (zero for the orthographic camera, whose eye is
    /// on the target plane; its view volume extends both ways).
    pub fn eye_rel(&self) -> DVec3 {
        if self.perspective {
            self.view_dir() * self.distance()
        } else {
            DVec3::ZERO
        }
    }

    /// World → view, with world positions relative to the target.
    pub fn view_rel(&self) -> DMat4 {
        DMat4::from_quat(self.quat.inverse()) * DMat4::from_translation(-self.eye_rel())
    }

    /// Reverse-Z projection. `near`/`far` are view-space distances along the view direction
    /// (for the orthographic camera they may be negative: the volume straddles the eye);
    /// the perspective camera ignores `far` (infinite far plane) and clamps `near` as the
    /// TypeScript camera does (`max(0.01, distance / 1000)`).
    pub fn projection(&self, near: f64, far: f64) -> DMat4 {
        let a = self.aspect();
        if self.perspective {
            let near = near.max(0.01).max(self.distance() / 1000.0);
            DMat4::perspective_infinite_reverse_rh(PERSPECTIVE_FOV_DEG.to_radians(), a, near)
        } else {
            let h = self.height / 2.0;
            // orthographic_rh maps −near → 0 and −far → 1; swapped gives reverse Z.
            DMat4::orthographic_rh(-h * a, h * a, -h, h, far, near)
        }
    }

    /// Depth range (near, far) that encloses a sphere (world centre, radius) with a margin,
    /// for [`Camera::projection`].
    pub fn depth_range(&self, center: DVec3, radius: f64) -> (f64, f64) {
        let d = self.view_dir();
        let z = (center - self.target - self.eye_rel()).dot(-d);
        let r = radius.max(1e-3) * 1.02 + 1e-3;
        if self.perspective {
            ((z - r).max(self.distance() / 1000.0), z + r)
        } else {
            (z - r, z + r)
        }
    }

    /// Projects a world point to logical pixels.
    pub fn project(&self, p: DVec3) -> [f64; 2] {
        let m = self.projection(1.0, 2.0) * self.view_rel();
        let c = m * DVec4::from((p - self.target, 1.0));
        let (x, y) = (c.x / c.w, c.y / c.w);
        [(x + 1.0) / 2.0 * self.width_px, (1.0 - y) / 2.0 * self.height_px]
    }

    /// World point on the plane through the target, perpendicular to the view, under a
    /// logical pixel.
    pub fn screen_to_plane_point(&self, sx: f64, sy: f64) -> DVec3 {
        let nx = sx / self.width_px * 2.0 - 1.0;
        let ny = -(sy / self.height_px) * 2.0 + 1.0;
        self.target
            + self.right() * (nx * self.height * self.aspect() / 2.0)
            + self.up() * (ny * self.height / 2.0)
    }

    /// The pick ray under a logical pixel (three.js `Raycaster.setFromCamera`).
    pub fn ray(&self, sx: f64, sy: f64) -> Ray {
        let on_plane = self.screen_to_plane_point(sx, sy);
        if self.perspective {
            let eye = self.target + self.eye_rel();
            Ray {
                origin: eye,
                dir: (on_plane - eye).normalize(),
            }
        } else {
            Ray {
                origin: on_plane + self.view_dir() * (ORTHO_DEPTH / 2.0),
                dir: -self.view_dir(),
            }
        }
    }

    // ------------------------------------------------------------ orientations

    /// Rotation whose +Z is `dir` and whose Y is as close to `up` as possible.
    pub fn orientation_for(dir: DVec3, up: DVec3) -> DQuat {
        let z = dir.normalize();
        let mut u = up;
        if u.dot(z).abs() > 0.999 {
            u = DVec3::NEG_Z;
        }
        let x = u.cross(z).normalize();
        let y = z.cross(x).normalize();
        DQuat::from_mat3(&DMat3::from_cols(x, y, z)).normalize()
    }

    /// Jumps to a standard view, optionally fitting a box. Returns false for an unknown
    /// name.
    pub fn set_view(&mut self, name: &str, fit: Option<&Aabb>) -> bool {
        let Some(&(n, dir, up)) = STANDARD_VIEWS.iter().find(|v| v.0 == name) else {
            return false;
        };
        let q = Self::orientation_for(dir.into(), up.into());
        let (target, height) = fit
            .map(|b| self.fit_params(b, q))
            .unwrap_or((self.target, self.height));
        self.quat = q;
        self.target = target;
        self.height = height;
        self.anim = None;
        self.view_name = Some(capitalised(n));
        true
    }

    /// Target and height that frame a box seen with orientation `q` (12 % margin).
    pub fn fit_params(&self, b: &Aabb, q: DQuat) -> (DVec3, f64) {
        if b.is_empty() {
            return (DVec3::ZERO, 200.0);
        }
        let inv = q.inverse();
        let mut vb = Aabb::EMPTY;
        for c in b.corners() {
            vb.add(inv * c);
        }
        let ext = vb.max - vb.min;
        let mut h = ext.y.max(ext.x / self.aspect()) * 1.12;
        if h == 0.0 || !h.is_finite() {
            h = 100.0;
        }
        (q * vb.center(), h)
    }

    pub fn zoom_to_fit(&mut self, b: &Aabb) {
        let q = self.anim.as_ref().map_or(self.quat, |a| a.q1);
        let (t, h) = self.fit_params(b, q);
        self.quat = q;
        self.target = t;
        self.height = h;
        self.anim = None;
    }

    // ------------------------------------------------------------ manipulation

    /// Free rotation by a mouse move of (dx, dy) logical pixels about a world pivot.
    pub fn rotate(&mut self, dx: f64, dy: f64, pivot: DVec3, deg_per_px: f64) {
        let qa = DQuat::from_axis_angle(self.up(), (-dx * deg_per_px).to_radians());
        let qb = DQuat::from_axis_angle(self.right(), (-dy * deg_per_px).to_radians());
        self.view_name = None;
        self.apply_rotation(qa * qb, pivot);
    }

    /// Rotation by `angle` radians about a world axis through `pivot`.
    pub fn rotate_axis(&mut self, axis: DVec3, angle: f64, pivot: DVec3) {
        self.view_name = None;
        self.apply_rotation(DQuat::from_axis_angle(axis.normalize(), angle), pivot);
    }

    /// Roll about the screen normal.
    pub fn roll(&mut self, angle: f64) {
        let q = DQuat::from_axis_angle(self.view_dir(), angle);
        self.view_name = None;
        self.quat = (q * self.quat).normalize();
    }

    fn apply_rotation(&mut self, q: DQuat, pivot: DVec3) {
        self.quat = (q * self.quat).normalize();
        self.target = q * (self.target - pivot) + pivot;
    }

    /// Pan by a mouse move in logical pixels (the model follows the pointer).
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let w = self.world_per_pixel();
        self.target += self.right() * (-dx * w) + self.up() * (dy * w);
    }

    /// Zoom by `factor` (> 1 zooms in) keeping the world point under the pointer fixed.
    pub fn zoom_at(&mut self, factor: f64, sx: f64, sy: f64) {
        let before = self.screen_to_plane_point(sx, sy);
        self.height = (self.height / factor).clamp(1e-4, 1e7);
        let after = self.screen_to_plane_point(sx, sy);
        self.target += before - after;
    }

    // ------------------------------------------------------------ transitions

    /// Starts an eased transition (`duration` seconds from `now`, both in seconds);
    /// `duration <= 0` jumps.
    pub fn animate_to(&mut self, q: DQuat, target: DVec3, height: f64, duration: f64, now: f64) {
        if duration <= 0.0 {
            (self.quat, self.target, self.height, self.anim) = (q, target, height, None);
            return;
        }
        self.anim = Some(Anim {
            t0: now,
            dur: duration,
            q0: self.quat,
            q1: q,
            tg0: self.target,
            tg1: target,
            h0: self.height,
            h1: height,
        });
    }

    /// Advances a transition; true while one is running (the caller should redraw).
    pub fn tick(&mut self, now: f64) -> bool {
        let Some(a) = self.anim.clone() else {
            return false;
        };
        let t = ((now - a.t0) / a.dur).clamp(0.0, 1.0);
        let e = if t < 0.5 {
            2.0 * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
        };
        self.quat = a.q0.slerp(a.q1, e);
        self.target = a.tg0.lerp(a.tg1, e);
        self.height = a.h0 + (a.h1 - a.h0) * e;
        if t >= 1.0 {
            self.anim = None;
        }
        true
    }

    pub fn animating(&self) -> bool {
        self.anim.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(w: f64, h: f64) -> Camera {
        let mut c = Camera::default();
        c.resize(w, h);
        c
    }

    #[test]
    fn standard_views_look_along_their_direction() {
        let mut c = cam(800.0, 600.0);
        assert_eq!(c.view_name.as_deref(), Some("Isometric"));
        assert!((c.view_dir() - DVec3::ONE.normalize()).length() < 1e-12);
        c.set_view("top", None);
        assert!((c.view_dir() - DVec3::Y).length() < 1e-12);
        assert!((c.up() - DVec3::NEG_Z).length() < 1e-12);
        c.set_view("front", None);
        assert!((c.right() - DVec3::X).length() < 1e-12);
        assert!(!c.set_view("sideways", None));
    }

    #[test]
    fn projection_maps_target_to_centre_and_round_trips() {
        for persp in [false, true] {
            let mut c = cam(800.0, 600.0);
            c.perspective = persp;
            c.target = DVec3::new(1e7, -3.0, 2.0);
            let [x, y] = c.project(c.target);
            assert!((x - 400.0).abs() < 1e-6 && (y - 300.0).abs() < 1e-6);
            let p = c.screen_to_plane_point(120.0, 70.0);
            let [x, y] = c.project(p);
            assert!((x - 120.0).abs() < 1e-6 && (y - 70.0).abs() < 1e-6, "{persp} {x} {y}");
            let r = c.ray(120.0, 70.0);
            let along = (p - r.origin).normalize();
            assert!((along - r.dir).length() < 1e-9);
        }
    }

    #[test]
    fn reverse_z_puts_near_at_one() {
        for persp in [false, true] {
            let mut c = cam(800.0, 600.0);
            c.perspective = persp;
            let (n, f) = c.depth_range(DVec3::ZERO, 50.0);
            let m = c.projection(n, f) * c.view_rel();
            let toward = c.view_dir() * 40.0;
            let depth = |p: DVec3| {
                let v = m * DVec4::from((p, 1.0));
                v.z / v.w
            };
            let (front, back) = (depth(toward), depth(-toward));
            assert!(front > back, "{persp}: {front} {back}");
            assert!((0.0..=1.0).contains(&front) && (0.0..=1.0).contains(&back));
        }
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut c = cam(800.0, 600.0);
        let p = c.screen_to_plane_point(600.0, 100.0);
        c.zoom_at(1.7, 600.0, 100.0);
        let q = c.screen_to_plane_point(600.0, 100.0);
        assert!((p - q).length() < 1e-9);
        let before = c.target;
        c.pan(10.0, 0.0);
        assert!(((before - c.target).length() - 10.0 * c.world_per_pixel()).abs() < 1e-9);
    }

    #[test]
    fn fit_frames_the_box() {
        let mut c = cam(800.0, 400.0);
        let b = Aabb {
            min: DVec3::new(-10.0, -5.0, 0.0),
            max: DVec3::new(30.0, 5.0, 2.0),
        };
        c.set_view("front", Some(&b));
        assert!((c.target - DVec3::new(10.0, 0.0, 1.0)).length() < 1e-9);
        // Width-limited: 40 mm across an aspect of 2 → height 20 × 1.12.
        assert!((c.height - 22.4).abs() < 1e-9);
    }

    #[test]
    fn transition_eases_to_the_end() {
        let mut c = cam(800.0, 600.0);
        let q = Camera::orientation_for(DVec3::X, DVec3::Y);
        c.animate_to(q, DVec3::new(5.0, 0.0, 0.0), 50.0, 0.35, 10.0);
        assert!(c.tick(10.175));
        assert!((c.height - 125.0).abs() < 1e-9);
        assert!(c.tick(11.0));
        assert!(!c.animating());
        assert!((c.view_dir() - DVec3::X).length() < 1e-12);
        assert!(!c.tick(12.0));
    }
}
