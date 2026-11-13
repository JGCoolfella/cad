//! Conversions between `oc-kernel` types and the bridge's plain structs.

use crate::ffi;
use oc_kernel::{
    BBox, CapEnd, Check, Curve, EdgeId, EdgeInfo, FaceId, FaceInfo, Fate, History, Image, KernelError, MassProps,
    Mesh, OpKind, Problem, Region, Relation, Segment, Source, Surface, Tolerances, TopoKind, Topology, VertexId, Wire,
    Axis, P3,
};

pub fn tol(t: &Tolerances) -> ffi::Tol {
    ffi::Tol {
        confusion: t.confusion,
        angular: t.angular,
        fuzzy: t.boolean_fuzzy,
        parallel: t.boolean_parallel,
        unify_angular: t.unify_angular,
        offset: t.offset,
        loft: t.loft,
        spline_fit: t.spline_fit,
        heal: t.heal,
        heal_acceptance: t.heal_acceptance,
    }
}

/// Maps a bridge error ("category|detail") onto a `KernelError`.
pub fn error(op: OpKind, e: cxx::Exception) -> KernelError {
    let msg = e.what();
    let (cat, detail) = msg.split_once('|').unwrap_or(("failed", msg));
    let detail = detail.to_string();
    match cat {
        "input" => KernelError::InvalidInput { op, detail },
        "invalid" => KernelError::InvalidResult { op, problems: detail.split(", ").map(str::to_string).collect() },
        "empty" => KernelError::Empty { op },
        "io" => KernelError::Io { op, detail },
        _ => KernelError::Failed { op, detail },
    }
}

fn push3(v: &mut Vec<f64>, p: P3) {
    v.extend_from_slice(&p);
}

fn segment(s: &Segment) -> ffi::SegIn {
    let mut v = vec![];
    let kind = match s {
        Segment::Line { a, b } => {
            push3(&mut v, *a);
            push3(&mut v, *b);
            0
        }
        Segment::Arc { a, mid, b } => {
            for p in [a, mid, b] {
                push3(&mut v, *p);
            }
            1
        }
        Segment::Circle { center, normal, radius } => {
            push3(&mut v, *center);
            push3(&mut v, *normal);
            v.push(*radius);
            2
        }
        Segment::Ellipse { center, normal, major_dir, major, minor } => {
            for p in [center, normal, major_dir] {
                push3(&mut v, *p);
            }
            v.extend_from_slice(&[*major, *minor]);
            3
        }
        Segment::Spline { points } => {
            for p in points {
                push3(&mut v, *p);
            }
            4
        }
    };
    ffi::SegIn { kind, v }
}

pub fn wire(w: &Wire) -> ffi::WireIn {
    ffi::WireIn { segs: w.segments.iter().map(segment).collect() }
}

pub fn regions(rs: &[Region]) -> Vec<ffi::RegionIn> {
    rs.iter().map(|r| ffi::RegionIn { outer: wire(&r.outer), holes: r.holes.iter().map(wire).collect() }).collect()
}

pub fn history(fates: &[ffi::FateOut], image_kind: u8) -> History {
    let fates = fates
        .iter()
        .map(|f| {
            let source = match f.src {
                0 => Source::Face { arg: f.arg, face: FaceId(f.a) },
                1 => Source::Edge { arg: f.arg, edge: EdgeId(f.a) },
                2 => Source::Vertex { arg: f.arg, vertex: VertexId(f.a) },
                3 => Source::Segment { region: f.a, wire: f.b, segment: f.c },
                _ => Source::Cap { region: f.a, end: if f.b == 0 { CapEnd::Start } else { CapEnd::End } },
            };
            // One image per index; Same wins over Modified over Generated.
            let mut images: Vec<Image> = vec![];
            for (list, relation) in [(&f.same, Relation::Same), (&f.modified, Relation::Modified), (&f.generated, Relation::Generated)] {
                for &index in list {
                    if !images.iter().any(|i| i.index == index) {
                        images.push(Image { index, relation });
                    }
                }
            }
            images.sort();
            Fate { source, deleted: f.deleted && images.is_empty(), images }
        })
        .collect();
    History { image_kind: if image_kind == 0 { TopoKind::Face } else { TopoKind::Edge }, fates }
}

fn p3(v: &[f64], i: usize) -> P3 {
    [v[i], v[i + 1], v[i + 2]]
}

fn bbox6(v: &[f64]) -> BBox {
    BBox { min: p3(v, 0), max: p3(v, 3) }
}

pub fn bbox(v: [f64; 6]) -> BBox {
    bbox6(&v)
}

fn surface(kind: u8, p: &[f64]) -> Surface {
    let axis = || Axis { origin: p3(p, 0), dir: p3(p, 3) };
    match kind {
        0 => Surface::Plane { origin: p3(p, 0), normal: p3(p, 3) },
        1 => Surface::Cylinder { axis: axis(), radius: p[6] },
        2 => Surface::Cone { axis: axis(), radius: p[6], half_angle: p[7] },
        3 => Surface::Sphere { center: p3(p, 0), radius: p[3] },
        4 => Surface::Torus { axis: axis(), major: p[6], minor: p[7] },
        5 => Surface::BSpline,
        6 => Surface::Bezier,
        7 => Surface::Revolution,
        8 => Surface::Extrusion,
        9 => Surface::Offset,
        _ => Surface::Other,
    }
}

fn curve(kind: u8, p: &[f64]) -> Curve {
    match kind {
        0 => Curve::Line,
        1 => Curve::Circle { center: p3(p, 0), normal: p3(p, 3), radius: p[6] },
        2 => Curve::Ellipse { center: p3(p, 0), normal: p3(p, 3), major_dir: p3(p, 6), major: p[9], minor: p[10] },
        3 => Curve::BSpline,
        4 => Curve::Bezier,
        _ => Curve::Other,
    }
}

pub fn topology(t: &ffi::TopoOut) -> Topology {
    Topology {
        solids: t.solids,
        shells: t.shells,
        faces: t
            .faces
            .iter()
            .map(|f| FaceInfo {
                surface: surface(f.surface, &f.params),
                area: f.area,
                centroid: p3(&f.centroid, 0),
                normal: p3(&f.normal, 0),
                bbox: bbox6(&f.bbox),
                edges: f.edges.iter().map(|&e| EdgeId(e)).collect(),
            })
            .collect(),
        edges: t
            .edges
            .iter()
            .map(|e| EdgeInfo {
                curve: curve(e.curve, &e.params),
                start: p3(&e.pts, 0),
                end: p3(&e.pts, 3),
                mid: p3(&e.pts, 6),
                length: e.length,
                faces: e.faces.iter().map(|&f| FaceId(f)).collect(),
                seam: e.seam,
                vertices: e.vertices.iter().map(|&v| VertexId(v)).collect(),
            })
            .collect(),
        vertices: t.vertices.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
    }
}

pub fn mass(m: &ffi::MassOut) -> MassProps {
    let i = &m.inertia;
    let inertia = [[i[0], i[1], i[2]], [i[3], i[4], i[5]], [i[6], i[7], i[8]]];
    let mut principal: Vec<(f64, P3)> = (0..3).map(|k| (m.moments[k], p3(&m.axes, 3 * k))).collect();
    principal.sort_by(|a, b| a.0.total_cmp(&b.0));
    MassProps {
        volume: m.volume,
        area: m.area,
        center: p3(&m.center, 0),
        inertia,
        principal_moments: [principal[0].0, principal[1].0, principal[2].0],
        principal_axes: [principal[0].1, principal[1].1, principal[2].1],
    }
}

pub fn check(c: &ffi::CheckOut) -> Check {
    Check {
        valid: c.valid,
        problems: c
            .problems
            .iter()
            .map(|p| Problem {
                entity: match p.kind {
                    0 => Some((TopoKind::Face, p.index)),
                    1 => Some((TopoKind::Edge, p.index)),
                    2 => Some((TopoKind::Vertex, p.index)),
                    _ => None,
                },
                code: p.code.clone(),
            })
            .collect(),
    }
}

pub fn mesh(m: &ffi::MeshOut) -> Mesh {
    let mut faces = Vec::with_capacity(m.face_ends.len());
    let mut start = 0;
    for &end in &m.face_ends {
        faces.push(start..end);
        start = end;
    }
    let pts: Vec<P3> = m.edge_points.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
    let mut edges = Vec::with_capacity(m.edge_ends.len());
    let mut s = 0usize;
    for &end in &m.edge_ends {
        edges.push(pts[s..end as usize].to_vec());
        s = end as usize;
    }
    Mesh {
        positions: m.positions.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        normals: m.normals.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        triangles: m.triangles.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
        faces,
        edges,
    }
}
