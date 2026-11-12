//! The committed WASM build (`src/sketch/wasm/`) must come from the current Rust sources.
//! `scripts/build-sketch-wasm.mjs` writes the hash of the sources it built from to
//! `src/sketch/wasm/SOURCE_HASH`; this test recomputes it. If it fails, run
//! `npm run build:sketch-wasm` and commit the result.

use std::path::{Path, PathBuf};

/// Files that determine the WASM build, as paths relative to the repository root.
fn sources(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).expect("read dir").flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    for c in ["crates/oc-sketch", "crates/oc-sketch-wasm"] {
        files.push(root.join(c).join("Cargo.toml"));
        walk(&root.join(c).join("src"), &mut files);
    }
    let mut rel: Vec<String> = files
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .expect("under root")
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
        })
        .collect();
    rel.sort();
    rel
}

/// FNV-1a 64 over `path \0 content(LF line endings) \0` for each file (same as the script).
fn source_hash(root: &Path) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for rel in sources(root) {
        let text = std::fs::read_to_string(root.join(&rel))
            .expect("read source")
            .replace("\r\n", "\n");
        feed(rel.as_bytes());
        feed(&[0]);
        feed(text.as_bytes());
        feed(&[0]);
    }
    format!("{h:016x}")
}

#[test]
fn committed_wasm_is_built_from_current_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().expect("root");
    let stored = std::fs::read_to_string(root.join("src/sketch/wasm/SOURCE_HASH"))
        .expect("src/sketch/wasm/SOURCE_HASH (run npm run build:sketch-wasm)");
    assert_eq!(
        stored.trim(),
        source_hash(&root),
        "stale WASM build: run `npm run build:sketch-wasm` and commit src/sketch/wasm/"
    );
}
