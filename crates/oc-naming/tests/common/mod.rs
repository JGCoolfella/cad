//! A cuboid's topology built by hand, with its faces in any order, so tests can check
//! that names never depend on the kernel's numbering.
#![allow(dead_code)]

use oc_kernel::{BBox, Curve, EdgeId, EdgeInfo, FaceId, FaceInfo, Surface, Topology, VertexId};
use oc_naming::{BodyNames, FaceName, Tag};

/// Face `k` of the canonical cuboid: 0 −x, 1 +x, 2 −y, 3 +y, 4 −z, 5 +z.
pub const SIDES: [&str; 6] = ["-x", "+x", "-y", "+y", "-z", "+z"];

pub fn name(tags: &[&str]) -> FaceName {
    FaceName::new(tags.iter().map(|t| Tag::new(t)))
}

/// A cuboid from `min` to `max`. `order[i]` is the canonical face stored at index `i`.
pub fn cuboid(min: [f64; 3], max: [f64; 3], order: [usize; 6]) -> Topology {
    let at = |canon: usize| order.iter().position(|&c| c == canon).unwrap() as u32;
    let mid = [0, 1, 2].map(|i| (min[i] + max[i]) / 2.0);
    let size = [0, 1, 2].map(|i| max[i] - min[i]);
    let mut vertices = vec![];
    for i in 0..8 {
        vertices.push([0, 1, 2].map(|a| if i >> a & 1 == 1 { max[a] } else { min[a] }));
    }
    // Edges: for each axis, the four edges parallel to it.
    let mut edges = vec![];
    for axis in 0..3 {
        let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
        for (su, sv) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
            let mut a = min;
            a[u] = if su == 1 { max[u] } else { min[u] };
            a[v] = if sv == 1 { max[v] } else { min[v] };
            let mut b = a;
            b[axis] = max[axis];
            let vid = |p: [f64; 3]| VertexId(vertices.iter().position(|q| *q == p).unwrap() as u32);
            let mut faces = vec![FaceId(at(2 * u + su)), FaceId(at(2 * v + sv))];
            faces.sort();
            edges.push(EdgeInfo {
                curve: Curve::Line,
                start: a,
                end: b,
                mid: [0, 1, 2].map(|i| (a[i] + b[i]) / 2.0),
                length: size[axis],
                faces,
                seam: false,
                vertices: vec![vid(a), vid(b)],
            });
        }
    }
    let faces = (0..6)
        .map(|i| {
            let canon = order[i];
            let (axis, plus) = (canon / 2, canon % 2 == 1);
            let mut normal = [0.0; 3];
            normal[axis] = if plus { 1.0 } else { -1.0 };
            let mut c = mid;
            c[axis] = if plus { max[axis] } else { min[axis] };
            let (mut lo, mut hi) = (min, max);
            lo[axis] = c[axis];
            hi[axis] = c[axis];
            FaceInfo {
                surface: Surface::Plane { origin: c, normal },
                area: size[(axis + 1) % 3] * size[(axis + 2) % 3],
                centroid: c,
                normal,
                bbox: BBox { min: lo, max: hi },
                edges: (0..edges.len() as u32).map(EdgeId).filter(|e| edges[e.index()].faces.contains(&FaceId(i as u32))).collect(),
            }
        })
        .collect();
    Topology { solids: 1, shells: 1, faces, edges, vertices }
}

/// Names for a cuboid extruded by feature `f` from a rectangle l0..l3 (sides −y, +x, +y,
/// −x in profile order) with caps at −z (start) and +z (end), in the cuboid's face order.
pub fn extruded(f: &str, order: [usize; 6]) -> BodyNames {
    let tag = |canon: usize| match canon {
        0 => format!("{f}/side/l3"),
        1 => format!("{f}/side/l1"),
        2 => format!("{f}/side/l0"),
        3 => format!("{f}/side/l2"),
        4 => format!("{f}/start/k"),
        _ => format!("{f}/end/k"),
    };
    BodyNames { faces: order.iter().map(|&c| name(&[&tag(c)])).collect() }
}

/// Index of the face with outward normal along `side` ("+z", ...).
pub fn face_at(order: [usize; 6], side: &str) -> FaceId {
    let canon = SIDES.iter().position(|s| *s == side).unwrap();
    FaceId(order.iter().position(|&c| c == canon).unwrap() as u32)
}

/// The edge between two sides.
pub fn edge_between(t: &Topology, a: FaceId, b: FaceId) -> EdgeId {
    let e = t.edges_between(a, b);
    assert_eq!(e.len(), 1);
    e[0]
}
