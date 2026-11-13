//! Determinism (plan section 2): the same body gives the same bytes whatever was meshed
//! before, plus the blob round trip. Thread counts: `threads.rs`.
mod common;

use common::*;
use oc_mesh::*;

#[test]
fn same_bytes_whatever_was_meshed_before() {
    let k = kernel();
    let parts = corpus_parts(&k);
    let cold: Vec<Vec<u8>> = parts
        .iter()
        .map(|(_, b)| build(&k, b, &MeshOptions::default()).unwrap().to_bytes())
        .collect();
    // Mesh everything at other qualities first, ending with one 5 % off the default
    // deflection, which OCCT would otherwise reuse.
    for (_, b) in &parts {
        for quality in [6.0, 1.0, 2.9] {
            build(
                &k,
                b,
                &MeshOptions {
                    quality,
                    max_lods: 1,
                    ..MeshOptions::default()
                },
            )
            .unwrap();
        }
    }
    for ((name, b), want) in parts.iter().zip(&cold) {
        assert!(
            &build(&k, b, &MeshOptions::default()).unwrap().to_bytes() == want,
            "{name}: depends on earlier meshes"
        );
    }
}

#[test]
fn blob_round_trip() {
    let k = kernel();
    for (name, b) in corpus_parts(&k) {
        let m = build(&k, &b, &MeshOptions::default()).unwrap();
        let bytes = m.to_bytes();
        let back = RenderMesh::from_bytes(&bytes).unwrap();
        assert_eq!(back, m, "{name}");
        assert_eq!(back.to_bytes(), bytes);
        assert_eq!(content_hash(&back.to_bytes()), content_hash(&bytes));
        // Every buffer starts 4-byte aligned in the blob, and its bytes are there as is.
        for l in &m.lods {
            for buf in [&l.vertices, &l.indices, &l.segments] {
                if buf.is_empty() {
                    continue;
                }
                let at = bytes
                    .windows(buf.len())
                    .position(|w| w == &buf[..])
                    .unwrap();
                assert_eq!(at % 4, 0, "{name}");
            }
        }
        assert_eq!(
            RenderMesh::from_bytes(&bytes[..bytes.len() - 1]),
            Err(BlobError::Truncated)
        );
        assert_eq!(
            RenderMesh::from_bytes(b"not a mesh"),
            Err(BlobError::NotAMesh)
        );
        let mut v2 = bytes.clone();
        v2[8] = 2;
        assert_eq!(RenderMesh::from_bytes(&v2), Err(BlobError::Version(2)));
    }
}
