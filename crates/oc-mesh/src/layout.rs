//! The GPU buffer formats. Every buffer is little-endian bytes that `oc-render` uploads
//! as is (`queue.write_buffer`); the structs here only decode them for tests and tools.
//!
//! Positions are `f32` relative to [`crate::RenderMesh::origin`] (an `f64` point), so the
//! renderer can place a body camera-relative without precision loss (P4: absolute `f32`
//! at 10 km changed 24 % of ID pixels, camera-relative none).
//!
//! **Vertex** ([`VERTEX_STRIDE`] = 20 bytes), one vertex buffer per level of detail:
//!
//! | offset | wgpu format | content |
//! |---|---|---|
//! | 0 | `Float32x3` | position |
//! | 12 | `Snorm16x2` | outward unit normal, octahedral ([`oct_encode`]) |
//! | 16 | `Uint32` | face index ([`oc_kernel::FaceId`]), for ID picking and highlights |
//!
//! Vertices are grouped by face (a face's vertices are contiguous, and never shared with
//! another face, so creases stay sharp). Decoding the normal in WGSL:
//!
//! ```wgsl
//! fn oct_decode(e: vec2f) -> vec3f {
//!   var n = vec3f(e, 1.0 - abs(e.x) - abs(e.y));
//!   let t = max(-n.z, 0.0);
//!   n.x += select(t, -t, n.x >= 0.0);
//!   n.y += select(t, -t, n.y >= 0.0);
//!   return normalize(n);
//! }
//! ```
//!
//! **Index** buffer: triangle list, counter-clockwise seen from outside, grouped by face;
//! `Uint16` when the level has at most 65 536 vertices, else `Uint32` ([`IndexFormat`]).
//! The byte length is padded to a multiple of 4 (wgpu's copy alignment).
//!
//! **Edge segment** ([`SEGMENT_STRIDE`] = 32 bytes), read from a storage buffer and
//! expanded to screen-space quads, as in P4 (`struct Seg { a: vec4f, b: vec4f }`):
//!
//! | offset | type | content |
//! |---|---|---|
//! | 0 | `f32 × 3` | start |
//! | 12 | `u32` | edge index ([`oc_kernel::EdgeId`]), `bitcast<u32>(a.w)` |
//! | 16 | `f32 × 3` | end |
//! | 28 | `u32` | reserved, 0 |
//!
//! Segments are grouped by edge. Seam and degenerate edges have none.

/// Bytes per vertex.
pub const VERTEX_STRIDE: usize = 20;
/// Bytes per edge segment.
pub const SEGMENT_STRIDE: usize = 32;
/// Most vertices a level can have and still use 16-bit indices.
pub const MAX_U16_VERTICES: usize = 1 << 16;

/// Width of the index buffer's entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndexFormat {
    U16,
    U32,
}

impl IndexFormat {
    pub fn for_vertex_count(n: usize) -> Self {
        if n <= MAX_U16_VERTICES {
            IndexFormat::U16
        } else {
            IndexFormat::U32
        }
    }
    pub fn bytes(self) -> usize {
        match self {
            IndexFormat::U16 => 2,
            IndexFormat::U32 => 4,
        }
    }
}

/// A decoded vertex.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub face: u32,
}

/// A decoded edge segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub a: [f32; 3],
    pub b: [f32; 3],
    pub edge: u32,
}

fn sign_not_zero(x: f32) -> f32 {
    if x >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

fn snorm16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

/// Octahedral encoding of a unit vector into two signed 16-bit values (angular error
/// below 1e-4 rad).
pub fn oct_encode(n: [f32; 3]) -> [i16; 2] {
    let l1 = n[0].abs() + n[1].abs() + n[2].abs();
    let (mut x, mut y) = (n[0] / l1, n[1] / l1);
    if n[2] < 0.0 {
        (x, y) = (
            (1.0 - y.abs()) * sign_not_zero(x),
            (1.0 - x.abs()) * sign_not_zero(y),
        );
    }
    [snorm16(x), snorm16(y)]
}

/// Inverse of [`oct_encode`] (what the shader computes).
pub fn oct_decode(e: [i16; 2]) -> [f32; 3] {
    let (x, y) = (
        (e[0] as f32 / 32767.0).max(-1.0),
        (e[1] as f32 / 32767.0).max(-1.0),
    );
    let mut n = [x, y, 1.0 - x.abs() - y.abs()];
    let t = (-n[2]).max(0.0);
    n[0] += if n[0] >= 0.0 { -t } else { t };
    n[1] += if n[1] >= 0.0 { -t } else { t };
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    n.map(|c| c / l)
}

pub(crate) fn put_vertex(out: &mut [u8], position: [f32; 3], normal: [i16; 2], face: u32) {
    for (k, c) in position.iter().enumerate() {
        out[4 * k..4 * k + 4].copy_from_slice(&c.to_le_bytes());
    }
    out[12..14].copy_from_slice(&normal[0].to_le_bytes());
    out[14..16].copy_from_slice(&normal[1].to_le_bytes());
    out[16..20].copy_from_slice(&face.to_le_bytes());
}

pub(crate) fn put_segment(out: &mut [u8], a: [f32; 3], b: [f32; 3], edge: u32) {
    for k in 0..3 {
        out[4 * k..4 * k + 4].copy_from_slice(&a[k].to_le_bytes());
        out[16 + 4 * k..20 + 4 * k].copy_from_slice(&b[k].to_le_bytes());
    }
    out[12..16].copy_from_slice(&edge.to_le_bytes());
    out[28..32].copy_from_slice(&0u32.to_le_bytes());
}

fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

pub(crate) fn get_vertex(b: &[u8]) -> Vertex {
    let n = [
        i16::from_le_bytes([b[12], b[13]]),
        i16::from_le_bytes([b[14], b[15]]),
    ];
    Vertex {
        position: [0, 4, 8].map(|o| f32_at(b, o)),
        normal: oct_decode(n),
        face: u32_at(b, 16),
    }
}

pub(crate) fn get_segment(b: &[u8]) -> Segment {
    Segment {
        a: [0, 4, 8].map(|o| f32_at(b, o)),
        b: [16, 20, 24].map(|o| f32_at(b, o)),
        edge: u32_at(b, 12),
    }
}

pub(crate) fn get_index(b: &[u8], format: IndexFormat, i: usize) -> u32 {
    match format {
        IndexFormat::U16 => u16::from_le_bytes([b[2 * i], b[2 * i + 1]]) as u32,
        IndexFormat::U32 => u32_at(b, 4 * i),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octahedral_normals_round_trip() {
        let mut worst = 0f32;
        for i in 0..64 {
            for j in 0..32 {
                let (phi, theta) = (i as f32 * 0.1, j as f32 * 0.1);
                let n = [
                    theta.sin() * phi.cos(),
                    theta.sin() * phi.sin(),
                    theta.cos(),
                ];
                let d = oct_decode(oct_encode(n)).map(|c| c as f64);
                let n = n.map(|c| c as f64);
                // Angle from the cross product (acos near 1 is noise in f32).
                let c = [
                    n[1] * d[2] - n[2] * d[1],
                    n[2] * d[0] - n[0] * d[2],
                    n[0] * d[1] - n[1] * d[0],
                ];
                worst = worst.max((c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt() as f32);
            }
        }
        for n in [
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
            [0.0, -1.0, 0.0],
        ] {
            assert_eq!(oct_decode(oct_encode(n)), n);
        }
        assert!(worst < 1e-4, "{worst}");
    }

    #[test]
    fn records_round_trip() {
        let mut v = [0u8; VERTEX_STRIDE];
        put_vertex(&mut v, [1.5, -2.0, 3.25], oct_encode([0.0, 1.0, 0.0]), 7);
        assert_eq!(
            get_vertex(&v),
            Vertex {
                position: [1.5, -2.0, 3.25],
                normal: [0.0, 1.0, 0.0],
                face: 7
            }
        );
        let mut s = [0u8; SEGMENT_STRIDE];
        put_segment(&mut s, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0], 9);
        assert_eq!(
            get_segment(&s),
            Segment {
                a: [1.0, 2.0, 3.0],
                b: [4.0, 5.0, 6.0],
                edge: 9
            }
        );
        assert_eq!(IndexFormat::for_vertex_count(65536), IndexFormat::U16);
        assert_eq!(IndexFormat::for_vertex_count(65537), IndexFormat::U32);
    }
}
