//! Compiles the C++ shim and links a static OCCT install named by OCCT_DIR
//! (see build-occt.cmd). Handles the Windows (inc/, win64/vc14/lib) and Unix
//! (include/opencascade, lib) install layouts.
use std::{env, fs, path::{Path, PathBuf}};

fn find_lib_dir(root: &Path) -> Option<PathBuf> {
    let probe = |d: &Path| d.join("TKernel.lib").exists() || d.join("libTKernel.a").exists();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if probe(&d) {
            return Some(d);
        }
        for e in fs::read_dir(&d).ok()?.flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            }
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-env-changed=OCCT_DIR");
    println!("cargo:rerun-if-changed=cpp/shim.h");
    println!("cargo:rerun-if-changed=cpp/shim.cpp");
    let root = PathBuf::from(env::var("OCCT_DIR").expect("set OCCT_DIR to an OCCT install"));
    let inc = [root.join("inc"), root.join("include/opencascade")]
        .into_iter()
        .find(|p| p.join("Standard.hxx").exists())
        .expect("no OCCT headers under OCCT_DIR");
    let lib = find_lib_dir(&root).expect("no OCCT libraries under OCCT_DIR");

    let mut b = cxx_build::bridge("src/lib.rs");
    b.file("cpp/shim.cpp").include(&inc).std("c++17");
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        b.flag("/EHsc").flag("/bigobj").define("WNT", None);
    }
    b.compile("p1shim");

    println!("cargo:rustc-link-search=native={}", lib.display());
    let mut libs: Vec<String> = fs::read_dir(&lib)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.strip_suffix(".lib")
                .map(str::to_owned)
                .or_else(|| n.strip_prefix("lib").and_then(|s| s.strip_suffix(".a")).map(str::to_owned))
        })
        .filter(|n| n.starts_with("TK"))
        .collect();
    libs.sort();
    let unix = env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc");
    if unix {
        // Static archives depend on each other in a cycle-free but unknown order.
        println!("cargo:rustc-link-arg=-Wl,--start-group");
    }
    for l in &libs {
        println!("cargo:rustc-link-lib=static={l}");
    }
    if unix {
        println!("cargo:rustc-link-arg=-Wl,--end-group");
        println!("cargo:rustc-link-lib=dylib=stdc++");
    } else {
        for l in ["user32", "advapi32", "gdi32", "ws2_32", "shell32"] {
            println!("cargo:rustc-link-lib=dylib={l}");
        }
    }
}
