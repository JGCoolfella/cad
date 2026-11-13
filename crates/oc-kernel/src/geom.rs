//! Geometry passed *into* the kernel: points, planes, axes, transforms and profile curves.
//!
//! Everything is plain `f64` data in model units (millimetres) and world coordinates, so
//! any kernel can build its own curves from it. Profiles are already resolved from the
//! sketch (regions found, splines sampled) by the caller; the kernel only builds them.

/// A point or vector in model space (mm).
pub type P3 = [f64; 3];

/// Vector helpers on [`P3`] (kept tiny on purpose; the engine's maths crate is not
/// needed by the trait).
pub mod v3 {
    use super::P3;
    pub fn add(a: P3, b: P3) -> P3 {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    }
    pub fn sub(a: P3, b: P3) -> P3 {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }
    pub fn scale(a: P3, s: f64) -> P3 {
        [a[0] * s, a[1] * s, a[2] * s]
    }
    pub fn dot(a: P3, b: P3) -> f64 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    pub fn cross(a: P3, b: P3) -> P3 {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }
    pub fn len(a: P3) -> f64 {
        dot(a, a).sqrt()
    }
    pub fn dist(a: P3, b: P3) -> f64 {
        len(sub(a, b))
    }
    /// Unit vector, or `None` for a (near) zero vector.
    pub fn unit(a: P3) -> Option<P3> {
        let l = len(a);
        (l > 1e-300 && l.is_finite()).then(|| scale(a, 1.0 / l))
    }
}

/// An oriented plane. `normal` need not be unit length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    pub origin: P3,
    pub normal: P3,
}

/// A directed line. `dir` need not be unit length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    pub origin: P3,
    pub dir: P3,
}

/// An affine map `p ↦ R p + t` with `R` orthogonal (rotation or reflection, no scaling).
/// Row-major 3×4: `m[r] = [R[r][0], R[r][1], R[r][2], t[r]]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub m: [[f64; 4]; 3],
}

impl Transform {
    pub const IDENTITY: Transform = Transform { m: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]] };

    pub fn translation(t: P3) -> Self {
        let mut m = Self::IDENTITY.m;
        for (r, row) in m.iter_mut().enumerate() {
            row[3] = t[r];
        }
        Transform { m }
    }

    /// Rotation by `angle` radians about `axis` (right-handed).
    pub fn rotation(axis: Axis, angle: f64) -> Self {
        let k = v3::unit(axis.dir).unwrap_or([0.0, 0.0, 1.0]);
        let (s, c) = angle.sin_cos();
        let t = 1.0 - c;
        let r = [
            [t * k[0] * k[0] + c, t * k[0] * k[1] - s * k[2], t * k[0] * k[2] + s * k[1]],
            [t * k[0] * k[1] + s * k[2], t * k[1] * k[1] + c, t * k[1] * k[2] - s * k[0]],
            [t * k[0] * k[2] - s * k[1], t * k[1] * k[2] + s * k[0], t * k[2] * k[2] + c],
        ];
        Self::from_linear(r, axis.origin)
    }

    /// Reflection in a plane.
    pub fn mirror(plane: Plane) -> Self {
        let n = v3::unit(plane.normal).unwrap_or([0.0, 0.0, 1.0]);
        let mut r = [[0.0; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, x) in row.iter_mut().enumerate() {
                *x = if i == j { 1.0 } else { 0.0 } - 2.0 * n[i] * n[j];
            }
        }
        Self::from_linear(r, plane.origin)
    }

    /// `p ↦ R (p − c) + c`.
    fn from_linear(r: [[f64; 3]; 3], c: P3) -> Self {
        let mut m = [[0.0; 4]; 3];
        for i in 0..3 {
            m[i][..3].copy_from_slice(&r[i]);
            m[i][3] = c[i] - (r[i][0] * c[0] + r[i][1] * c[1] + r[i][2] * c[2]);
        }
        Transform { m }
    }

    pub fn apply(&self, p: P3) -> P3 {
        let m = &self.m;
        [0, 1, 2].map(|r| m[r][0] * p[0] + m[r][1] * p[1] + m[r][2] * p[2] + m[r][3])
    }

    /// Determinant of the linear part: +1 for a rigid motion, −1 for a reflection.
    pub fn det(&self) -> f64 {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }
}

/// One curve of a profile or path, in world coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum Segment {
    Line { a: P3, b: P3 },
    /// Circular arc from `a` through `mid` to `b`.
    Arc { a: P3, mid: P3, b: P3 },
    /// Full circle. Its start point is a kernel choice; it forms a loop on its own.
    Circle { center: P3, normal: P3, radius: f64 },
    /// Full ellipse, `major ≥ minor`, major axis along `major_dir`; a loop on its own.
    Ellipse { center: P3, normal: P3, major_dir: P3, major: f64, minor: f64 },
    /// Cubic B-spline approximating `points` within `Tolerances::spline_fit`, from the
    /// first point to the last.
    Spline { points: Vec<P3> },
}

impl Segment {
    /// Start and end point, `None` for closed curves (circle, ellipse).
    pub fn ends(&self) -> Option<(P3, P3)> {
        match self {
            Segment::Line { a, b } | Segment::Arc { a, b, .. } => Some((*a, *b)),
            Segment::Spline { points } => Some((*points.first()?, *points.last()?)),
            Segment::Circle { .. } | Segment::Ellipse { .. } => None,
        }
    }
}

/// Connected segments in order, each starting where the previous one ends.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Wire {
    pub segments: Vec<Segment>,
}

impl Wire {
    pub fn new(segments: Vec<Segment>) -> Self {
        Wire { segments }
    }

    /// Closed polygon through `points`.
    pub fn polygon(points: &[P3]) -> Self {
        let n = points.len();
        Wire::new((0..n).map(|i| Segment::Line { a: points[i], b: points[(i + 1) % n] }).collect())
    }

    /// Axis-aligned rectangle in the plane `z`, corners in counter-clockwise order from
    /// `(x0, y0)`.
    pub fn rect_xy(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Self {
        Wire::polygon(&[[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]])
    }

    /// Closed: a single circle or ellipse, or the last segment ends where the first starts
    /// (within `tol`).
    pub fn is_closed(&self, tol: f64) -> bool {
        match self.segments.as_slice() {
            [] => false,
            [Segment::Circle { .. } | Segment::Ellipse { .. }] => true,
            segs => match (segs[0].ends(), segs[segs.len() - 1].ends()) {
                (Some((start, _)), Some((_, end))) => v3::dist(start, end) <= tol,
                _ => false,
            },
        }
    }
}

/// A planar area: one outer boundary and the holes inside it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Region {
    pub outer: Wire,
    pub holes: Vec<Wire>,
}

impl Region {
    pub fn new(outer: Wire) -> Self {
        Region { outer, holes: vec![] }
    }
    pub fn with_hole(mut self, hole: Wire) -> Self {
        self.holes.push(hole);
        self
    }
    /// Wire `w` of the region: 0 = outer, `k` = hole `k − 1`. The numbering used by
    /// [`crate::Source::Segment`].
    pub fn wire(&self, w: usize) -> Option<&Wire> {
        if w == 0 {
            Some(&self.outer)
        } else {
            self.holes.get(w - 1)
        }
    }
}

/// Planar regions to extrude, revolve or sweep. Regions must not overlap.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Profile {
    pub regions: Vec<Region>,
}

impl Profile {
    pub fn new(regions: Vec<Region>) -> Self {
        Profile { regions }
    }
    pub fn single(outer: Wire) -> Self {
        Profile { regions: vec![Region::new(outer)] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: P3, b: P3) -> bool {
        v3::dist(a, b) < 1e-12
    }

    #[test]
    fn transforms() {
        let r = Transform::rotation(Axis { origin: [1.0, 0.0, 0.0], dir: [0.0, 0.0, 2.0] }, std::f64::consts::FRAC_PI_2);
        assert!(close(r.apply([2.0, 0.0, 5.0]), [1.0, 1.0, 5.0]));
        assert!((r.det() - 1.0).abs() < 1e-12);
        let m = Transform::mirror(Plane { origin: [0.0, 0.0, 3.0], normal: [0.0, 0.0, -1.0] });
        assert!(close(m.apply([1.0, 2.0, 5.0]), [1.0, 2.0, 1.0]));
        assert!((m.det() + 1.0).abs() < 1e-12);
        assert!(close(Transform::translation([1.0, 2.0, 3.0]).apply([1.0, 1.0, 1.0]), [2.0, 3.0, 4.0]));
    }

    #[test]
    fn wires() {
        let w = Wire::rect_xy(0.0, 0.0, 2.0, 1.0, 0.0);
        assert_eq!(w.segments.len(), 4);
        assert!(w.is_closed(1e-9));
        let open = Wire::new(w.segments[..3].to_vec());
        assert!(!open.is_closed(1e-9));
        assert!(Wire::new(vec![Segment::Circle { center: [0.0; 3], normal: [0.0, 0.0, 1.0], radius: 1.0 }]).is_closed(0.0));
        let r = Region::new(w.clone()).with_hole(Wire::rect_xy(0.5, 0.25, 1.0, 0.5, 0.0));
        assert_eq!(r.wire(0), Some(&w));
        assert!(r.wire(1).is_some() && r.wire(2).is_none());
    }
}
