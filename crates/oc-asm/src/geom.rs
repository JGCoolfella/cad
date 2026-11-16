//! Mate geometry: what a picked face, edge or vertex is to a mate, read from live
//! topology; standard planes; conversions between the document's and the solver's types.
use oc_doc::{MateGeom, StdPlane, Transform};
use oc_kernel::{Curve, EdgeId, FaceId, Surface, Topology, VertexId};
use oc_mate::{Geom, Pose};

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = dot(a, a).sqrt();
    if n > 0.0 {
        [a[0] / n, a[1] / n, a[2] / n]
    } else {
        a
    }
}
/// The point of the line `(o, d)` nearest `c` (a stable anchor on an axis).
fn foot(o: [f64; 3], d: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let d = unit(d);
    let t = dot(sub(c, o), d);
    [o[0] + d[0] * t, o[1] + d[1] * t, o[2] + d[2] * t]
}

/// A face as mate geometry; `None` for surfaces mates do not use (free-form, torus).
pub fn face_geom(t: &Topology, f: FaceId) -> Option<MateGeom> {
    let x = &t.faces[f.index()];
    Some(match x.surface {
        // The face's outward normal (the surface's may point inward on a reversed face).
        Surface::Plane { .. } => MateGeom::Plane { p: x.centroid, n: unit(x.normal) },
        Surface::Cylinder { axis, radius } => MateGeom::Cylinder { p: foot(axis.origin, axis.dir, x.centroid), d: unit(axis.dir), r: radius },
        Surface::Cone { axis, radius, half_angle } => MateGeom::Cone { p: axis.origin, d: unit(axis.dir), r: radius, half_angle },
        Surface::Sphere { center, radius } => MateGeom::Sphere { c: center, r: radius },
        _ => return None,
    })
}

/// An edge as mate geometry; `None` for curves mates do not use.
pub fn edge_geom(t: &Topology, e: EdgeId) -> Option<MateGeom> {
    let x = &t.edges[e.index()];
    match x.curve {
        Curve::Line => {
            let d = sub(x.end, x.start);
            (dot(d, d) > 0.0).then(|| MateGeom::Line { p: x.mid, d: unit(d) })
        }
        Curve::Circle { center, normal, radius } => Some(MateGeom::Circle { c: center, n: unit(normal), r: radius }),
        _ => None,
    }
}

pub fn vertex_geom(t: &Topology, v: VertexId) -> MateGeom {
    MateGeom::Point { p: t.vertices[v.index()] }
}

/// A standard plane (Front = XY, Top = XZ, Right = YZ, normals +Z, +Y, +X as the app's
/// `STD_FRAMES`).
pub fn std_plane(p: StdPlane) -> MateGeom {
    let n = match p {
        StdPlane::Front => [0.0, 0.0, 1.0],
        StdPlane::Top => [0.0, 1.0, 0.0],
        StdPlane::Right => [1.0, 0.0, 0.0],
    };
    MateGeom::Plane { p: [0.0; 3], n }
}

pub fn to_solver(g: &MateGeom) -> Geom {
    match *g {
        MateGeom::Plane { p, n } => Geom::Plane { p, n },
        MateGeom::Line { p, d } => Geom::Line { p, d },
        MateGeom::Point { p } => Geom::Point { p },
        MateGeom::Cylinder { p, d, r } => Geom::Cylinder { p, d, r },
        MateGeom::Cone { p, d, r, half_angle } => Geom::Cone { p, d, r, half_angle },
        MateGeom::Sphere { c, r } => Geom::Sphere { c, r },
        MateGeom::Circle { c, n, r } => Geom::Circle { c, n, r },
    }
}

/// The same geometry mapped by `t`.
pub fn transformed(g: &MateGeom, t: &Transform) -> MateGeom {
    match *g {
        MateGeom::Plane { p, n } => MateGeom::Plane { p: t.apply(p), n: t.rotate(n) },
        MateGeom::Line { p, d } => MateGeom::Line { p: t.apply(p), d: t.rotate(d) },
        MateGeom::Point { p } => MateGeom::Point { p: t.apply(p) },
        MateGeom::Cylinder { p, d, r } => MateGeom::Cylinder { p: t.apply(p), d: t.rotate(d), r },
        MateGeom::Cone { p, d, r, half_angle } => MateGeom::Cone { p: t.apply(p), d: t.rotate(d), r, half_angle },
        MateGeom::Sphere { c, r } => MateGeom::Sphere { c: t.apply(c), r },
        MateGeom::Circle { c, n, r } => MateGeom::Circle { c: t.apply(c), n: t.rotate(n), r },
    }
}

/// Equal within `tol` (positions mm, directions and radii).
pub fn same(a: &MateGeom, b: &MateGeom, tol: f64) -> bool {
    let v = |x: [f64; 3], y: [f64; 3]| dot(sub(x, y), sub(x, y)).sqrt() <= tol;
    let s = |x: f64, y: f64| (x - y).abs() <= tol;
    match (*a, *b) {
        (MateGeom::Plane { p, n }, MateGeom::Plane { p: q, n: m }) => v(n, m) && dot(sub(q, p), n).abs() <= tol,
        (MateGeom::Line { p, d }, MateGeom::Line { p: q, d: e }) => v(d, e) && v(foot(p, d, q), q),
        (MateGeom::Point { p }, MateGeom::Point { p: q }) => v(p, q),
        (MateGeom::Cylinder { p, d, r }, MateGeom::Cylinder { p: q, d: e, r: s2 }) => v(d, e) && v(foot(p, d, q), q) && s(r, s2),
        (MateGeom::Cone { p, d, r, half_angle }, MateGeom::Cone { p: q, d: e, r: r2, half_angle: h2 }) => v(p, q) && v(d, e) && s(r, r2) && s(half_angle, h2),
        (MateGeom::Sphere { c, r }, MateGeom::Sphere { c: d, r: r2 }) => v(c, d) && s(r, r2),
        (MateGeom::Circle { c, n, r }, MateGeom::Circle { c: d, n: m, r: r2 }) => v(c, d) && v(n, m) && s(r, r2),
        _ => false,
    }
}

pub fn pose(t: &Transform) -> Pose {
    Pose::new(t.unit_rotation(), t.position)
}

pub fn transform(p: &Pose) -> Transform {
    Transform { position: p.t, rotation: p.q }
}
