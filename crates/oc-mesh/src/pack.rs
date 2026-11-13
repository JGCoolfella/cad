//! Kernel meshes → GPU layout ([`crate::layout`]), one face (or edge) per parallel task.
//!
//! Each face is packed on its own (compacted vertices, local indices, normals encoded),
//! then prefix sums give every face its place, and the faces write their part of the
//! shared buffers in parallel through disjoint slices. Results are concatenated in face
//! order, so the bytes never depend on the thread count or scheduling.

use crate::layout::{
    oct_encode, put_segment, put_vertex, IndexFormat, SEGMENT_STRIDE, VERTEX_STRIDE,
};
use crate::{EdgeRange, FaceRange, Lod};
use oc_kernel::{v3, Mesh, MeshParams, P3};
use rayon::prelude::*;

/// One face, packed with face-local indices.
struct FacePack {
    vertices: Vec<u8>,
    indices: Vec<u32>,
    min: [f32; 3],
    max: [f32; 3],
}

fn rel(p: P3, origin: P3) -> [f32; 3] {
    [
        (p[0] - origin[0]) as f32,
        (p[1] - origin[1]) as f32,
        (p[2] - origin[2]) as f32,
    ]
}

/// Grows the box `lo..hi` to contain `a..b`.
pub(crate) fn grow(lo: &mut [f32; 3], hi: &mut [f32; 3], a: [f32; 3], b: [f32; 3]) {
    for (k, (l, h)) in lo.iter_mut().zip(hi.iter_mut()).enumerate() {
        *l = l.min(a[k]);
        *h = h.max(b[k]);
    }
}

/// The kernel's normal if it is a usable unit vector, else the triangle's own.
fn normal_or(n: [f32; 3], tri: [P3; 3]) -> [f32; 3] {
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l.is_finite() && (l - 1.0).abs() < 1e-3 {
        return n.map(|c| c / l);
    }
    let g = v3::cross(v3::sub(tri[1], tri[0]), v3::sub(tri[2], tri[0]));
    match v3::unit(g) {
        Some(u) => u.map(|c| c as f32),
        None => [0.0, 0.0, 1.0],
    }
}

fn pack_face(face: u32, tris: &[[u32; 3]], mesh: &Mesh, origin: P3) -> FacePack {
    let mut fp = FacePack {
        vertices: vec![],
        indices: Vec::with_capacity(tris.len() * 3),
        min: [f32::MAX; 3],
        max: [f32::MIN; 3],
    };
    let Some(lo) = tris.iter().flatten().copied().min() else {
        return fp;
    };
    let hi = tris.iter().flatten().copied().max().unwrap_or(lo);
    // Compact: the face's vertices in first-use order, whatever the kernel's numbering.
    let mut local = vec![u32::MAX; (hi - lo) as usize + 1];
    let mut order: Vec<(u32, [P3; 3])> = vec![];
    for t in tris {
        let corners = t.map(|i| mesh.positions[i as usize]);
        for &i in t {
            let slot = &mut local[(i - lo) as usize];
            if *slot == u32::MAX {
                *slot = order.len() as u32;
                order.push((i, corners));
            }
            fp.indices.push(*slot);
        }
    }
    fp.vertices = vec![0; order.len() * VERTEX_STRIDE];
    for ((i, corners), out) in order
        .iter()
        .zip(fp.vertices.chunks_exact_mut(VERTEX_STRIDE))
    {
        let p = rel(mesh.positions[*i as usize], origin);
        grow(&mut fp.min, &mut fp.max, p, p);
        let n = normal_or(mesh.normals[*i as usize], *corners);
        put_vertex(out, p, oct_encode(n), face);
    }
    fp
}

/// Edge `e`'s polyline as segments, dropping repeated points.
fn edge_segments(e: u32, line: &[P3], origin: P3) -> Vec<u8> {
    let pts: Vec<[f32; 3]> = line.iter().map(|&p| rel(p, origin)).collect();
    let mut out = vec![];
    let mut seg = [0u8; SEGMENT_STRIDE];
    for w in pts.windows(2) {
        if w[0] != w[1] {
            put_segment(&mut seg, w[0], w[1], e);
            out.extend_from_slice(&seg);
        }
    }
    out
}

/// Split `buf` into consecutive pieces of the given lengths.
fn split(mut buf: &mut [u8], lens: impl Iterator<Item = usize>) -> Vec<&mut [u8]> {
    let mut out = vec![];
    for n in lens {
        let (a, b) = buf.split_at_mut(n);
        out.push(a);
        buf = b;
    }
    out
}

/// Packs one level. `visible[e]` says whether edge `e` gets segments. Returns the level
/// and the bounds of its vertices (relative to `origin`).
pub(crate) fn pack_level(
    params: MeshParams,
    mesh: &Mesh,
    visible: &[bool],
    origin: P3,
) -> (Lod, [[f32; 3]; 2]) {
    let faces: Vec<FacePack> = mesh
        .faces
        .par_iter()
        .enumerate()
        .map(|(f, r)| {
            pack_face(
                f as u32,
                &mesh.triangles[r.start as usize..r.end as usize],
                mesh,
                origin,
            )
        })
        .collect();
    let mut ranges = Vec::with_capacity(faces.len());
    let (mut nv, mut ni) = (0u32, 0u32);
    let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
    for f in &faces {
        let (v, i) = (
            (f.vertices.len() / VERTEX_STRIDE) as u32,
            f.indices.len() as u32,
        );
        ranges.push(FaceRange {
            first_vertex: nv,
            vertex_count: v,
            first_index: ni,
            index_count: i,
        });
        nv += v;
        ni += i;
        if v > 0 {
            let [lo, hi] = &mut bounds;
            grow(lo, hi, f.min, f.max);
        }
    }
    let format = IndexFormat::for_vertex_count(nv as usize);
    let w = format.bytes();
    let mut vertices = vec![0u8; nv as usize * VERTEX_STRIDE];
    let mut indices = vec![0u8; (ni as usize * w).next_multiple_of(4)];
    let vs = split(&mut vertices, faces.iter().map(|f| f.vertices.len()));
    let is = split(&mut indices, faces.iter().map(|f| f.indices.len() * w));
    vs.into_par_iter()
        .zip(is)
        .zip(faces.par_iter().zip(&ranges))
        .for_each(|((vo, io), (f, r))| {
            vo.copy_from_slice(&f.vertices);
            for (k, &i) in f.indices.iter().enumerate() {
                let g = r.first_vertex + i;
                match format {
                    IndexFormat::U16 => {
                        io[2 * k..2 * k + 2].copy_from_slice(&(g as u16).to_le_bytes())
                    }
                    IndexFormat::U32 => io[4 * k..4 * k + 4].copy_from_slice(&g.to_le_bytes()),
                }
            }
        });

    let segs: Vec<Vec<u8>> = mesh
        .edges
        .par_iter()
        .enumerate()
        .map(|(e, line)| {
            if visible.get(e).copied().unwrap_or(false) {
                edge_segments(e as u32, line, origin)
            } else {
                vec![]
            }
        })
        .collect();
    let mut edges = Vec::with_capacity(segs.len());
    let mut ns = 0u32;
    for s in &segs {
        let n = (s.len() / SEGMENT_STRIDE) as u32;
        edges.push(EdgeRange {
            first_segment: ns,
            segment_count: n,
        });
        ns += n;
    }
    let segments = segs.concat();
    let lod = Lod {
        params,
        vertex_count: nv,
        vertices,
        index_format: format,
        index_count: ni,
        indices,
        faces: ranges,
        segment_count: ns,
        segments,
        edges,
    };
    (lod, bounds)
}
