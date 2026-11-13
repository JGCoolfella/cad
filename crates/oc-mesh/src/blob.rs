//! One contiguous little-endian blob per [`RenderMesh`]: what the geometry host writes to
//! shared memory (P3) and a tessellation cache stores. The GPU buffers are copied in
//! unchanged, each starting at a multiple of 4 bytes.
//!
//! ```text
//! "OCMESH\0\0" u32 version=1
//! u32 lods, faces, edges, points
//! f64×3 origin, f32×6 bounds, f32 radius
//! points × (f32×3 position, u32 flags), edges × u32 flags
//! per level: f64 linear, f64 angular, u32 vertices, u32 index bytes (2|4), u32 indices,
//!            u32 segments, faces × u32×4 ranges, edges × u32×2 ranges,
//!            vertex bytes, index bytes (padded to 4), segment bytes
//! ```

use crate::layout::{IndexFormat, SEGMENT_STRIDE, VERTEX_STRIDE};
use crate::{EdgeRange, FaceRange, Lod, RenderMesh};
use oc_kernel::MeshParams;

const MAGIC: &[u8; 8] = b"OCMESH\0\0";
const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlobError {
    NotAMesh,
    Version(u32),
    Truncated,
    Invalid(&'static str),
}

impl std::fmt::Display for BlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlobError::NotAMesh => write!(f, "not a mesh blob"),
            BlobError::Version(v) => write!(f, "mesh blob version {v} is not supported"),
            BlobError::Truncated => write!(f, "mesh blob is truncated"),
            BlobError::Invalid(what) => write!(f, "mesh blob is invalid: {what}"),
        }
    }
}

impl std::error::Error for BlobError {}

/// FNV-1a (64-bit, standard offset basis and prime) of `bytes`: a fixed, documented hash
/// for deduplicating meshes (plan section 3.4).
pub fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

struct W(Vec<u8>);

impl W {
    fn u32(&mut self, x: u32) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn f32(&mut self, x: f32) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn f64(&mut self, x: f64) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u32(n as u32);
    }
}

struct R<'a>(&'a [u8]);

impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BlobError> {
        if self.0.len() < n {
            return Err(BlobError::Truncated);
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u32(&mut self) -> Result<u32, BlobError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn f32(&mut self) -> Result<f32, BlobError> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn f64(&mut self) -> Result<f64, BlobError> {
        let b = self.take(8)?;
        Ok(f64::from_le_bytes(
            b.try_into().map_err(|_| BlobError::Truncated)?,
        ))
    }
    fn n(&mut self) -> Result<usize, BlobError> {
        Ok(self.u32()? as usize)
    }
}

impl RenderMesh {
    /// The blob described in the module docs.
    pub fn to_bytes(&self) -> Vec<u8> {
        let payload: usize = self
            .lods
            .iter()
            .map(|l| l.vertices.len() + l.indices.len() + l.segments.len())
            .sum();
        let mut w = W(Vec::with_capacity(payload + 1024));
        w.0.extend_from_slice(MAGIC);
        w.u32(VERSION);
        w.len(self.lods.len());
        w.len(self.face_count());
        w.len(self.edge_flags.len());
        w.len(self.points.len());
        self.origin.iter().for_each(|&x| w.f64(x));
        self.bounds.iter().flatten().for_each(|&x| w.f32(x));
        w.f32(self.radius);
        for (p, &f) in self.points.iter().zip(&self.point_flags) {
            p.iter().for_each(|&x| w.f32(x));
            w.u32(f);
        }
        self.edge_flags.iter().for_each(|&f| w.u32(f));
        for l in &self.lods {
            w.f64(l.params.linear);
            w.f64(l.params.angular);
            w.u32(l.vertex_count);
            w.len(l.index_format.bytes());
            w.u32(l.index_count);
            w.u32(l.segment_count);
            for r in &l.faces {
                [r.first_vertex, r.vertex_count, r.first_index, r.index_count]
                    .iter()
                    .for_each(|&x| w.u32(x));
            }
            for r in &l.edges {
                w.u32(r.first_segment);
                w.u32(r.segment_count);
            }
            w.0.extend_from_slice(&l.vertices);
            w.0.extend_from_slice(&l.indices);
            w.0.extend_from_slice(&l.segments);
        }
        w.0
    }

    /// Reads a blob written by [`Self::to_bytes`].
    pub fn from_bytes(bytes: &[u8]) -> Result<RenderMesh, BlobError> {
        let mut r = R(bytes);
        if r.take(8).map_err(|_| BlobError::NotAMesh)? != MAGIC {
            return Err(BlobError::NotAMesh);
        }
        match r.u32()? {
            VERSION => {}
            v => return Err(BlobError::Version(v)),
        }
        let (nl, nf, ne, np) = (r.n()?, r.n()?, r.n()?, r.n()?);
        let origin = [r.f64()?, r.f64()?, r.f64()?];
        let mut bounds = [[0f32; 3]; 2];
        for b in bounds.iter_mut().flatten() {
            *b = r.f32()?;
        }
        let radius = r.f32()?;
        let (mut points, mut point_flags) = (vec![], vec![]);
        for _ in 0..np {
            points.push([r.f32()?, r.f32()?, r.f32()?]);
            point_flags.push(r.u32()?);
        }
        let edge_flags = (0..ne).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
        let mut lods = vec![];
        for _ in 0..nl {
            let params = MeshParams {
                linear: r.f64()?,
                angular: r.f64()?,
            };
            let vertex_count = r.u32()?;
            let index_format = match r.u32()? {
                2 => IndexFormat::U16,
                4 => IndexFormat::U32,
                _ => return Err(BlobError::Invalid("index width")),
            };
            let (index_count, segment_count) = (r.u32()?, r.u32()?);
            let faces = (0..nf)
                .map(|_| {
                    Ok(FaceRange {
                        first_vertex: r.u32()?,
                        vertex_count: r.u32()?,
                        first_index: r.u32()?,
                        index_count: r.u32()?,
                    })
                })
                .collect::<Result<Vec<_>, BlobError>>()?;
            let edges = (0..ne)
                .map(|_| {
                    Ok(EdgeRange {
                        first_segment: r.u32()?,
                        segment_count: r.u32()?,
                    })
                })
                .collect::<Result<Vec<_>, BlobError>>()?;
            let vertices = r.take(vertex_count as usize * VERTEX_STRIDE)?.to_vec();
            let indices = r
                .take((index_count as usize * index_format.bytes()).next_multiple_of(4))?
                .to_vec();
            let segments = r.take(segment_count as usize * SEGMENT_STRIDE)?.to_vec();
            let within = |first: u32, n: u32, len: u32| first as u64 + n as u64 <= len as u64;
            let in_range = faces.iter().all(|f| {
                within(f.first_vertex, f.vertex_count, vertex_count)
                    && within(f.first_index, f.index_count, index_count)
            }) && edges
                .iter()
                .all(|e| within(e.first_segment, e.segment_count, segment_count));
            if !in_range {
                return Err(BlobError::Invalid("range outside its buffer"));
            }
            lods.push(Lod {
                params,
                vertex_count,
                vertices,
                index_format,
                index_count,
                indices,
                faces,
                segment_count,
                segments,
                edges,
            });
        }
        if !r.0.is_empty() {
            return Err(BlobError::Invalid("trailing bytes"));
        }
        Ok(RenderMesh {
            origin,
            bounds,
            radius,
            points,
            point_flags,
            edge_flags,
            lods,
        })
    }
}
