//! The app's version, for keying the `.ocasm` caches to the engine build (G3d,
//! `oc_asm::cache::APP_VERSION`). Read from `crates/oc-app/Cargo.toml`, the version the
//! release bumps, so every assembler (the app's, tools', tests') keys caches the same way;
//! `cargo test -p oc-app` checks the two agree.
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets it")).join("../oc-app/Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).unwrap_or_else(|e| panic!("{}: {e}", manifest.display()));
    // The first `version = "…"` is the package's (it precedes every table but [package]).
    let version = text
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == "version").then(|| v.trim().trim_matches('"').to_string())
        })
        .unwrap_or_else(|| panic!("{}: no version", manifest.display()));
    println!("cargo:rustc-env=OC_APP_VERSION={version}");
}
