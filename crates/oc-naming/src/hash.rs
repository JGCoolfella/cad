//! The one hash behind every name key (`docs/OVERHAUL_PLAN.md` section 3.4).
//!
//! **64-bit FNV-1a**: offset basis `0xcbf29ce484222325`, prime `0x100000001b3`, one
//! byte at a time. It has no seed and no dependence on platform, pointer values or map
//! order, so a key is the same on every machine and in every run. A key is written as 16
//! lower-case hexadecimal digits.
//!
//! What is hashed (the encoding is part of the name format; changing it renames faces):
//! - a string: its UTF-8 bytes, then the byte `0x00`;
//! - a face name: each tag as a string, in the name's (sorted) order, then `0x01`;
//! - an edge or vertex name: its face names in their (sorted) order.

/// Running FNV-1a 64 state.
#[derive(Clone, Copy, Debug)]
pub struct Fnv64(u64);

impl Default for Fnv64 {
    fn default() -> Self {
        Fnv64(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv64 {
    pub fn bytes(mut self, b: &[u8]) -> Self {
        for &x in b {
            self.0 ^= u64::from(x);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        self
    }

    /// A string and its terminator.
    pub fn str(self, s: &str) -> Self {
        self.bytes(s.as_bytes()).bytes(&[0])
    }

    pub fn finish(self) -> u64 {
        self.0
    }

    /// The key as it appears in a tag.
    pub fn key(self) -> String {
        format!("{:016x}", self.0)
    }
}

/// Key of a list of strings, order-independent: they are sorted (and repeated ones dropped)
/// before hashing. Used for the entity ids of a profile loop.
pub fn key_of_set<S: AsRef<str>>(items: &[S]) -> String {
    let mut v: Vec<&str> = items.iter().map(AsRef::as_ref).collect();
    v.sort_unstable();
    v.dedup();
    v.into_iter().fold(Fnv64::default(), Fnv64::str).key()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_64_reference_vectors() {
        // Published FNV-1a 64 test vectors.
        assert_eq!(Fnv64::default().finish(), 0xcbf29ce484222325);
        assert_eq!(Fnv64::default().bytes(b"a").finish(), 0xaf63dc4c8601ec8c);
        assert_eq!(Fnv64::default().bytes(b"foobar").finish(), 0x85944171f73967e8);
        assert_eq!(Fnv64::default().bytes(b"a").key(), "af63dc4c8601ec8c");
    }

    #[test]
    fn set_keys_ignore_order_and_repeats_but_not_boundaries() {
        assert_eq!(key_of_set(&["l2", "l1", "l10"]), key_of_set(&["l10", "l1", "l2", "l1"]));
        assert_ne!(key_of_set(&["l1", "l2"]), key_of_set(&["l1", "l3"]));
        // The terminator keeps ("ab", "c") apart from ("a", "bc").
        assert_ne!(key_of_set(&["ab", "c"]), key_of_set(&["a", "bc"]));
    }
}
