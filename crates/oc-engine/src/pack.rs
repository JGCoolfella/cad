//! The binary reply of `engine_rebuild`: a JSON header plus typed arrays, so meshes reach
//! the webview without JSON-encoding millions of floats.
//!
//! Layout: `b"OCR1"`, the header length (`u32` LE), the header (UTF-8 JSON, padded with
//! spaces to a multiple of 8 bytes), then the arrays, each starting on a boundary of its
//! element size (4 bytes, 8 for `f64`). In the header an array is
//! `{"$b": offset, "n": count, "t": "f32" | "u32" | "f64"}`, the offset counted in bytes from
//! the start of the array section (`src/native/engine.ts` decodes it). Assembly build
//! replies (G3d) use the same layout with `f64` placements.
use serde_json::{json, Value};

#[derive(Default)]
pub struct Packer {
    data: Vec<u8>,
}

impl Packer {
    pub fn f32(&mut self, v: &[f32]) -> Value {
        let at = self.data.len();
        for x in v {
            self.data.extend_from_slice(&x.to_le_bytes());
        }
        json!({ "$b": at, "n": v.len(), "t": "f32" })
    }

    pub fn u32(&mut self, v: &[u32]) -> Value {
        let at = self.data.len();
        for x in v {
            self.data.extend_from_slice(&x.to_le_bytes());
        }
        json!({ "$b": at, "n": v.len(), "t": "u32" })
    }

    /// `f64` values, 8-byte aligned (the array section starts on an 8-byte boundary).
    pub fn f64(&mut self, v: &[f64]) -> Value {
        while self.data.len() % 8 != 0 {
            self.data.push(0);
        }
        let at = self.data.len();
        self.data.reserve(v.len() * 8);
        for x in v {
            self.data.extend_from_slice(&x.to_le_bytes());
        }
        json!({ "$b": at, "n": v.len(), "t": "f64" })
    }

    pub fn finish(self, header: &Value) -> Vec<u8> {
        let mut text = serde_json::to_vec(header).expect("header serialises");
        while text.len() % 8 != 0 {
            text.push(b' ');
        }
        let mut out = Vec::with_capacity(8 + text.len() + self.data.len());
        out.extend_from_slice(b"OCR1");
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(&text);
        out.extend_from_slice(&self.data);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let mut p = Packer::default();
        let a = p.f32(&[1.0, 2.0]);
        let b = p.u32(&[7]);
        let bytes = p.finish(&json!({ "a": a, "b": b }));
        assert_eq!(&bytes[..4], b"OCR1");
        let n = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        assert_eq!(n % 4, 0);
        let header: Value = serde_json::from_slice(&bytes[8..8 + n]).unwrap();
        assert_eq!(header["b"]["$b"], 8);
        let base = 8 + n;
        assert_eq!(f32::from_le_bytes(bytes[base + 4..base + 8].try_into().unwrap()), 2.0);
        assert_eq!(u32::from_le_bytes(bytes[base + 8..base + 12].try_into().unwrap()), 7);
    }

    #[test]
    fn f64_arrays_are_aligned() {
        let mut p = Packer::default();
        let a = p.u32(&[1]);
        let b = p.f64(&[0.1, -2.5]);
        let bytes = p.finish(&json!({ "a": a, "b": b, "pad": "x" }));
        let n = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        let base = 8 + n;
        assert_eq!(base % 8, 0, "the array section starts on an 8-byte boundary");
        let header: Value = serde_json::from_slice(&bytes[8..8 + n]).unwrap();
        let at = header["b"]["$b"].as_u64().unwrap() as usize;
        assert_eq!((base + at) % 8, 0, "Float64Array needs it");
        assert_eq!(header["b"]["t"], "f64");
        assert_eq!(f64::from_le_bytes(bytes[base + at + 8..base + at + 16].try_into().unwrap()), -2.5);
    }
}
