//! The zip container of assembly files (`.ocasm`; `docs/OVERHAUL_PLAN.md` section 1, "File
//! format"): `document.json`, the authoritative and versioned document, plus caches named
//! by content hash (`cache/…`) that can always be regenerated and are never the source of
//! truth.
//!
//! Only the subset of zip this needs is written and read: entries stored (method 0) or
//! deflated (method 8), UTF-8 names, no encryption, no zip64 (an entry and the archive
//! stay below 4 GiB; a G2-scale assembly cache that outgrows that needs zip64). Written
//! archives are byte-for-byte deterministic: fixed timestamps (1980-01-01), entries in
//! insertion order, fixed compression level. Any zip tool can list and unpack them.

use std::fmt;

/// Name of the document entry.
pub const DOCUMENT_ENTRY: &str = "document.json";

/// Why a container could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContainerError {
    /// Not a zip archive, or a damaged one.
    Corrupt(String),
    /// A zip feature this reader does not support (zip64, encryption, other methods).
    Unsupported(String),
    /// No `document.json`.
    NoDocument,
}

impl fmt::Display for ContainerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContainerError::Corrupt(m) => write!(f, "damaged file: {m}"),
            ContainerError::Unsupported(m) => write!(f, "unsupported zip content: {m}"),
            ContainerError::NoDocument => write!(f, "the file has no {DOCUMENT_ENTRY}"),
        }
    }
}

impl std::error::Error for ContainerError {}

/// Entries of a container, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Container {
    entries: Vec<(String, Vec<u8>)>,
}

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
/// 1980-01-01 00:00:00 in MS-DOS date format (the earliest zip can hold).
const DOS_DATE: u16 = (1 << 5) | 1;
const UTF8_NAMES: u16 = 1 << 11;
const DEFLATE_LEVEL: u8 = 6;

impl Container {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces an entry (a replaced entry keeps its place).
    pub fn insert(&mut self, name: impl Into<String>, bytes: Vec<u8>) {
        let name = name.into();
        match self.entries.iter_mut().find(|(n, _)| *n == name) {
            Some(e) => e.1 = bytes,
            None => self.entries.push((name, bytes)),
        }
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries.iter().find(|(n, _)| n == name).map(|(_, b)| b.as_slice())
    }

    pub fn remove(&mut self, name: &str) -> Option<Vec<u8>> {
        let i = self.entries.iter().position(|(n, _)| n == name)?;
        Some(self.entries.remove(i).1)
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(n, _)| n.as_str())
    }

    /// The `document.json` text.
    pub fn document(&self) -> Result<&str, ContainerError> {
        let b = self.get(DOCUMENT_ENTRY).ok_or(ContainerError::NoDocument)?;
        std::str::from_utf8(b).map_err(|_| ContainerError::Corrupt(format!("{DOCUMENT_ENTRY} is not UTF-8")))
    }

    /// The zip archive.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in &self.entries {
            let crc = crc32fast::hash(data);
            let deflated = miniz_oxide::deflate::compress_to_vec(data, DEFLATE_LEVEL);
            let (method, stored): (u16, &[u8]) = if deflated.len() < data.len() { (8, &deflated) } else { (0, data) };
            let offset = out.len() as u32;
            let header = |sig: u32, buf: &mut Vec<u8>| {
                put32(buf, sig);
                if sig == CENTRAL {
                    put16(buf, 20); // made by: zip 2.0, MS-DOS attributes
                }
                put16(buf, 20); // needed to extract: 2.0 (deflate)
                put16(buf, UTF8_NAMES);
                put16(buf, method);
                put16(buf, 0); // time
                put16(buf, DOS_DATE);
                put32(buf, crc);
                put32(buf, stored.len() as u32);
                put32(buf, data.len() as u32);
                put16(buf, name.len() as u16);
                put16(buf, 0); // extra field
            };
            header(LOCAL, &mut out);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(stored);
            header(CENTRAL, &mut central);
            put16(&mut central, 0); // comment
            put16(&mut central, 0); // disk
            put16(&mut central, 0); // internal attributes
            put32(&mut central, 0); // external attributes
            put32(&mut central, offset);
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        put32(&mut out, END);
        put16(&mut out, 0);
        put16(&mut out, 0);
        put16(&mut out, self.entries.len() as u16);
        put16(&mut out, self.entries.len() as u16);
        put32(&mut out, central.len() as u32);
        put32(&mut out, cd_offset);
        put16(&mut out, 0);
        out
    }

    /// Reads an archive. Every entry is decompressed and its CRC checked.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ContainerError> {
        let bad = |m: &str| ContainerError::Corrupt(m.into());
        // End of central directory: the last END signature within the comment's reach.
        let min = bytes.len().saturating_sub(22 + 0xffff);
        let end = (min..=bytes.len().saturating_sub(22))
            .rev()
            .find(|&i| get32(bytes, i) == Some(END))
            .ok_or_else(|| bad("not a zip archive"))?;
        let count = get16(bytes, end + 10).ok_or_else(|| bad("truncated"))? as usize;
        let cd_size = get32(bytes, end + 12).ok_or_else(|| bad("truncated"))? as usize;
        let cd_offset = get32(bytes, end + 16).ok_or_else(|| bad("truncated"))? as usize;
        if count == 0xffff || cd_offset == 0xffff_ffff {
            return Err(ContainerError::Unsupported("zip64".into()));
        }
        if cd_offset.checked_add(cd_size).is_none_or(|e| e > end) {
            return Err(bad("central directory out of range"));
        }
        let mut entries: Vec<(String, Vec<u8>)> = Vec::with_capacity(count);
        let mut at = cd_offset;
        for _ in 0..count {
            let f = |o: usize| get32(bytes, at + o).ok_or_else(|| bad("truncated central directory"));
            let h = |o: usize| get16(bytes, at + o).ok_or_else(|| bad("truncated central directory"));
            if f(0)? != CENTRAL {
                return Err(bad("central directory entry expected"));
            }
            let (flags, method) = (h(8)?, h(10)?);
            let (crc, csize, usize_) = (f(16)?, f(20)? as usize, f(24)? as usize);
            let (nlen, xlen, clen) = (h(28)? as usize, h(30)? as usize, h(32)? as usize);
            let local = f(42)? as usize;
            if csize == 0xffff_ffff || usize_ == 0xffff_ffff || local == 0xffff_ffff {
                return Err(ContainerError::Unsupported("zip64".into()));
            }
            if flags & 1 != 0 {
                return Err(ContainerError::Unsupported("encrypted entry".into()));
            }
            let name_bytes = bytes.get(at + 46..at + 46 + nlen).ok_or_else(|| bad("truncated name"))?;
            let name = String::from_utf8(name_bytes.to_vec()).map_err(|_| bad("entry name is not UTF-8"))?;
            at += 46 + nlen + xlen + clen;
            if name.ends_with('/') {
                continue; // a folder entry written by another zip tool
            }
            if entries.iter().any(|(n, _)| *n == name) {
                return Err(bad(&format!("entry {name} appears twice")));
            }
            if get32(bytes, local) != Some(LOCAL) {
                return Err(bad(&format!("entry {name}: local header expected")));
            }
            let lnlen = get16(bytes, local + 26).ok_or_else(|| bad("truncated"))? as usize;
            let lxlen = get16(bytes, local + 28).ok_or_else(|| bad("truncated"))? as usize;
            let start = local + 30 + lnlen + lxlen;
            let raw = bytes.get(start..start + csize).ok_or_else(|| bad(&format!("entry {name} is truncated")))?;
            let data = match method {
                0 => raw.to_vec(),
                8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, usize_).map_err(|e| bad(&format!("entry {name}: {e:?}")))?,
                m => return Err(ContainerError::Unsupported(format!("entry {name}: compression method {m}"))),
            };
            if data.len() != usize_ || crc32fast::hash(&data) != crc {
                return Err(bad(&format!("entry {name}: checksum mismatch")));
            }
            entries.push((name, data));
        }
        Ok(Container { entries })
    }
}

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn get16(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn get32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// A content hash for cache names: 128-bit FNV-1a, as 32 lowercase hex digits. Fixed and
/// documented (section 3.4), so equal content gets the same name on every machine. Not
/// collision-resistant against an adversary; a cache entry is only ever a cache.
pub fn content_hash(parts: &[&[u8]]) -> String {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut h = OFFSET;
    for p in parts {
        // Length-prefixed, so ("ab", "c") and ("a", "bc") differ.
        for b in (p.len() as u64).to_le_bytes().iter().chain(p.iter()) {
            h ^= *b as u128;
            h = h.wrapping_mul(PRIME);
        }
    }
    format!("{h:032x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_determinism() {
        let mut c = Container::new();
        c.insert(DOCUMENT_ENTRY, b"{\"a\": 1}".to_vec());
        c.insert("cache/x.bin", vec![7u8; 10_000]);
        c.insert("cache/empty", vec![]);
        let bytes = c.to_bytes();
        assert_eq!(bytes, c.to_bytes(), "deterministic");
        let back = Container::from_bytes(&bytes).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.document().unwrap(), "{\"a\": 1}");
        assert!(bytes.len() < 1000, "the repeated bytes are deflated ({} bytes)", bytes.len());
    }

    #[test]
    fn damage_is_detected() {
        let mut c = Container::new();
        c.insert(DOCUMENT_ENTRY, b"hello hello hello hello".to_vec());
        let mut bytes = c.to_bytes();
        let i = 30 + DOCUMENT_ENTRY.len() + 2;
        bytes[i] ^= 0x55;
        assert!(matches!(Container::from_bytes(&bytes), Err(ContainerError::Corrupt(_))));
        assert!(matches!(Container::from_bytes(b"not a zip"), Err(ContainerError::Corrupt(_))));
        assert_eq!(Container::from_bytes(&Container::new().to_bytes()).unwrap().document(), Err(ContainerError::NoDocument));
    }

    #[test]
    fn hash_is_fixed() {
        assert_eq!(content_hash(&[]), "6c62272e07bb014262b821756295c58d");
        assert_ne!(content_hash(&[b"ab", b"c"]), content_hash(&[b"a", b"bc"]));
    }
}
