//! The protocol between the app and a geometry host: binary frames on the host's standard
//! input (requests) and, on Windows, a pipe of its own (replies; G3d), named by the first
//! frame. The host's standard output goes to `NUL`, so nothing the kernel prints can break
//! the protocol. Elsewhere the replies still use standard output (no Output frame).
//!
//! ```text
//! frame    = magic "OCHP" | kind u32 | length u64 | payload        (little-endian)
//! Output   app → host, first (Windows): the reply pipe's handle in the host, u64
//! Hello    host → app, once at start: protocol u32 | kernel name
//! Job      app → host: id u64 | quality f64 | rules 5 × f64 | part document (JSON)
//! Done     host → app: id u64 | seconds f64 | body? | geometry | mesh?
//!          body     = 1 | native blob | face count u32, per face: tag count u32, tags
//!          geometry = planes, axes, points (JSON maps of MateGeom) | errors: count, (id, message)
//!          mesh     = 1 | RenderMesh::to_bytes
//! Failed   host → app: id u64 | message (the job could not run: an unreadable document)
//! ```
//! Strings and byte strings are a u64 length and the bytes. A frame with the wrong magic, or
//! a payload that does not decode, means the stream is broken: the app restarts the host.
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::time::Duration;

use oc_asm::parallel::{PartOut, RefGeometry};
use oc_doc::MateGeom;
use oc_naming::{BodyNames, FaceName, Rules};

pub const MAGIC: [u8; 4] = *b"OCHP";
/// Changes with any change of the frames below.
pub const PROTOCOL: u32 = 2;
/// Largest payload accepted (a mesh of a huge part is a few hundred MB).
pub const MAX_PAYLOAD: u64 = 4 << 30;

pub const HELLO: u32 = 1;
pub const JOB: u32 = 2;
pub const DONE: u32 = 3;
pub const FAILED: u32 = 4;
pub const OUTPUT: u32 = 5;

pub fn write_frame(w: &mut impl Write, kind: u32, payload: &[u8]) -> io::Result<()> {
    w.write_all(&MAGIC)?;
    w.write_all(&kind.to_le_bytes())?;
    w.write_all(&(payload.len() as u64).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// The next frame; `Ok(None)` at the end of the stream.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<(u32, Vec<u8>)>> {
    let mut head = [0u8; 16];
    let mut got = 0;
    while got < head.len() {
        match r.read(&mut head[got..])? {
            0 if got == 0 => return Ok(None),
            0 => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "the stream ended inside a frame")),
            n => got += n,
        }
    }
    if head[..4] != MAGIC {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a geometry-host frame"));
    }
    let kind = u32::from_le_bytes(head[4..8].try_into().expect("4 bytes"));
    let len = u64::from_le_bytes(head[8..16].try_into().expect("8 bytes"));
    if len > MAX_PAYLOAD {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload)?;
    Ok(Some((kind, payload)))
}

/// Payload writer.
#[derive(Default)]
pub struct Enc(pub Vec<u8>);

impl Enc {
    pub fn u32(&mut self, x: u32) -> &mut Self {
        self.0.extend_from_slice(&x.to_le_bytes());
        self
    }
    pub fn u64(&mut self, x: u64) -> &mut Self {
        self.0.extend_from_slice(&x.to_le_bytes());
        self
    }
    pub fn f64(&mut self, x: f64) -> &mut Self {
        self.0.extend_from_slice(&x.to_le_bytes());
        self
    }
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.u64(b.len() as u64);
        self.0.extend_from_slice(b);
        self
    }
    pub fn str(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes())
    }
}

/// Payload reader; every read checks the length left.
pub struct Dec<'a>(pub &'a [u8]);

fn short() -> String {
    "the frame is shorter than its contents".into()
}

impl<'a> Dec<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        if self.0.len() < n {
            return Err(short());
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    pub fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4 bytes")))
    }
    pub fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }
    pub fn f64(&mut self) -> Result<f64, String> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().expect("8 bytes")))
    }
    pub fn bytes(&mut self) -> Result<&'a [u8], String> {
        let n = self.u64()?;
        self.take(usize::try_from(n).map_err(|_| short())?)
    }
    pub fn str(&mut self) -> Result<String, String> {
        String::from_utf8(self.bytes()?.to_vec()).map_err(|e| e.to_string())
    }
    pub fn end(&self) -> Result<(), String> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err("the frame has trailing bytes".into())
        }
    }
}

/// A part to regenerate, as it crosses to a host.
#[derive(Clone, Debug)]
pub struct Job {
    pub id: u64,
    pub quality: f64,
    pub rules: Rules,
    /// The (configured) part document, `oc_doc::save`'s JSON.
    pub doc: String,
}

pub fn encode_job(j: &Job) -> Vec<u8> {
    let mut e = Enc::default();
    let r = &j.rules;
    e.u64(j.id).f64(j.quality).f64(r.linear).f64(r.angular).f64(r.size).f64(r.legacy_face_score).f64(r.legacy_edge_min).str(&j.doc);
    e.0
}

pub fn decode_job(b: &[u8]) -> Result<Job, String> {
    let mut d = Dec(b);
    let id = d.u64()?;
    let quality = d.f64()?;
    let rules = Rules { linear: d.f64()?, angular: d.f64()?, size: d.f64()?, legacy_face_score: d.f64()?, legacy_edge_min: d.f64()? };
    let doc = d.str()?;
    d.end()?;
    Ok(Job { id, quality, rules, doc })
}

fn geom_json(m: &HashMap<String, MateGeom>) -> String {
    // Sorted, so the bytes do not depend on hash-map order.
    let sorted: std::collections::BTreeMap<&String, &MateGeom> = m.iter().collect();
    serde_json::to_string(&sorted).expect("geometry is plain data")
}

fn geom_from(s: &str) -> Result<HashMap<String, MateGeom>, String> {
    serde_json::from_str(s).map_err(|e| e.to_string())
}

/// A regenerated part, as it crosses back.
pub fn encode_done(id: u64, o: &PartOut) -> Vec<u8> {
    let mut e = Enc::default();
    e.u64(id).f64(o.time.as_secs_f64());
    match &o.body {
        Some((blob, names)) => {
            e.u32(1).bytes(blob).u32(names.faces.len() as u32);
            for f in &names.faces {
                let tags = f.to_stored();
                e.u32(tags.len() as u32);
                for t in &tags {
                    e.str(t);
                }
            }
        }
        None => {
            e.u32(0);
        }
    }
    let g = &o.geometry;
    e.str(&geom_json(&g.planes)).str(&geom_json(&g.axes)).str(&geom_json(&g.points)).u32(g.errors.len() as u32);
    for (id, msg) in &g.errors {
        e.str(id).str(msg);
    }
    match &o.mesh {
        Some(m) => e.u32(1).bytes(&m.to_bytes()),
        None => e.u32(0),
    };
    e.0
}

pub fn decode_done(b: &[u8]) -> Result<(u64, PartOut), String> {
    let mut d = Dec(b);
    let id = d.u64()?;
    let time = Duration::from_secs_f64(d.f64()?.max(0.0));
    let body = match d.u32()? {
        0 => None,
        _ => {
            let blob = d.bytes()?.to_vec();
            let n = d.u32()?;
            let mut faces = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let k = d.u32()?;
                let tags = (0..k).map(|_| d.str()).collect::<Result<Vec<_>, _>>()?;
                faces.push(FaceName::from_stored(&tags));
            }
            Some((blob, BodyNames { faces }))
        }
    };
    let mut geometry = RefGeometry { planes: geom_from(&d.str()?)?, axes: geom_from(&d.str()?)?, points: geom_from(&d.str()?)?, errors: vec![] };
    for _ in 0..d.u32()? {
        geometry.errors.push((d.str()?, d.str()?));
    }
    let mesh = match d.u32()? {
        0 => None,
        _ => Some(oc_mesh::RenderMesh::from_bytes(d.bytes()?).map_err(|e| format!("{e:?}"))?),
    };
    d.end()?;
    Ok((id, PartOut { body, geometry, mesh, time }))
}

pub fn encode_failed(id: u64, message: &str) -> Vec<u8> {
    let mut e = Enc::default();
    e.u64(id).str(message);
    e.0
}

pub fn decode_failed(b: &[u8]) -> Result<(u64, String), String> {
    let mut d = Dec(b);
    let id = d.u64()?;
    let m = d.str()?;
    d.end()?;
    Ok((id, m))
}

pub fn encode_hello(kernel: &str) -> Vec<u8> {
    let mut e = Enc::default();
    e.u32(PROTOCOL).str(kernel);
    e.0
}

pub fn decode_hello(b: &[u8]) -> Result<(u32, String), String> {
    let mut d = Dec(b);
    let p = d.u32()?;
    let k = d.str()?;
    d.end()?;
    Ok((p, k))
}
