//! Compiles the C++ shim (`cpp/*.cpp`) and links the OCCT install named by `OCCT_DIR`
//! (built by `build-occt.cmd` / `build-occt.sh`). Approach from the P1 spike.
//!
//! - Install layouts: Windows (`inc/`, `win64/vc14/lib`, DLLs in `win64/vc14/bin`) and
//!   Unix (`include/opencascade`, `lib`).
//! - **Static** install (tests, CI): archives linked dependents-first, order and system
//!   libraries taken from the `INTERFACE_LINK_LIBRARIES` of OCCT's CMake target files.
//! - **Shared** install (shipping, LGPL 2.1 dynamic linking): import libraries / shared
//!   objects; on Unix an rpath to the install is added, on Windows the `bin` directory
//!   must be on `PATH` when running.
//! - The OCCT version is pinned: the build fails on anything but [`PINNED`].
use std::collections::{BTreeMap, BTreeSet};
use std::{env, fs, path::{Path, PathBuf}};

const PINNED: &str = "8.0.1";
const SOURCES: [&str; 6] = ["body.cpp", "profile.cpp", "ops.cpp", "finish.cpp", "query.cpp", "exchange.cpp"];

fn walk(root: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}

#[derive(Default)]
struct Deps {
    tk: BTreeMap<String, Vec<String>>,
    system: BTreeSet<String>,
    frameworks: BTreeSet<String>,
}

fn parse_targets(files: &[PathBuf], apple: bool) -> Deps {
    let mut d = Deps::default();
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy();
        if !(name.starts_with("OpenCASCADE") && name.ends_with("Targets.cmake")) {
            continue;
        }
        let text = fs::read_to_string(f).unwrap();
        let mut current: Option<String> = None;
        for line in text.lines() {
            let l = line.trim();
            if let Some(rest) = l.strip_prefix("set_target_properties(") {
                current = rest.split_whitespace().next().map(str::to_owned);
            }
            let (Some(cur), Some(q)) = (&current, l.strip_prefix("INTERFACE_LINK_LIBRARIES \"")) else {
                continue;
            };
            let mut deps = vec![];
            for item in q.trim_end_matches('"').split(';') {
                let item = item.replace("\\$<LINK_ONLY:", "").replace("$<LINK_ONLY:", "").replace('>', "");
                let item = item.trim();
                if item.is_empty() || item.contains("$<") {
                    continue;
                }
                if item.starts_with("TK") {
                    deps.push(item.to_owned());
                } else if let Some(fw) = item.strip_prefix("-framework ") {
                    d.frameworks.insert(fw.trim().to_owned());
                } else if apple && (item.ends_with(".framework") || item.starts_with(|c: char| c.is_ascii_uppercase())) {
                    // OCCT lists Apple frameworks as bare names (AppKit, IOKit) or bundle paths.
                    let base = Path::new(item).file_name().unwrap().to_string_lossy().into_owned();
                    d.frameworks.insert(base.trim_end_matches(".framework").to_owned());
                } else {
                    // -lfoo, foo.lib, /usr/lib/libfoo.so, foo
                    let base = Path::new(item).file_name().unwrap().to_string_lossy().into_owned();
                    let base = base.strip_prefix("-l").unwrap_or(&base);
                    let base = base.split('.').next().unwrap();
                    let base = if item.contains('/') { base.strip_prefix("lib").unwrap_or(base) } else { base };
                    d.system.insert(base.to_owned());
                }
            }
            d.tk.insert(cur.clone(), deps);
        }
    }
    d
}

/// Dependents first: a reversed depth-first post-order.
fn link_order(deps: &BTreeMap<String, Vec<String>>, present: &BTreeSet<String>) -> Vec<String> {
    fn visit(n: &str, deps: &BTreeMap<String, Vec<String>>, seen: &mut BTreeSet<String>, out: &mut Vec<String>) {
        if !seen.insert(n.to_owned()) {
            return;
        }
        for d in deps.get(n).into_iter().flatten() {
            visit(d, deps, seen, out);
        }
        out.push(n.to_owned());
    }
    let (mut seen, mut out) = (BTreeSet::new(), vec![]);
    for n in present {
        visit(n, deps, &mut seen, &mut out);
    }
    out.retain(|n| present.contains(n));
    out.reverse();
    out
}

fn main() {
    println!("cargo:rerun-if-env-changed=OCCT_DIR");
    println!("cargo:rerun-if-changed=cpp/shim.h");
    for s in SOURCES {
        println!("cargo:rerun-if-changed=cpp/{s}");
    }
    let Some(root) = env::var_os("OCCT_DIR").map(PathBuf::from) else {
        panic!(
            "\n\noc-kernel-occt needs OCCT {PINNED}: set OCCT_DIR to an install built by \
             crates/oc-kernel-occt/build-occt.cmd (Windows) or build-occt.sh (macOS, Linux).\n\
             Example (Windows): set OCCT_DIR=C:\\occt\\install-{PINNED}\n\
             Other crates build without it: cargo test -p oc-doc -p oc-sketch -p oc-kernel\n\n"
        );
    };
    let inc = [root.join("inc"), root.join("include/opencascade")]
        .into_iter()
        .find(|p| p.join("Standard.hxx").exists())
        .unwrap_or_else(|| panic!("no OCCT headers (Standard.hxx) under OCCT_DIR={}", root.display()));
    let version = fs::read_to_string(inc.join("Standard_Version.hxx")).unwrap_or_default();
    let found = version
        .lines()
        .find_map(|l| l.trim().strip_prefix("#define OCC_VERSION_COMPLETE"))
        .map(|v| v.trim().trim_matches('"').to_owned())
        .unwrap_or_else(|| "unknown".into());
    assert_eq!(found, PINNED, "OCCT_DIR holds OCCT {found}; oc-kernel-occt is pinned to {PINNED}");

    let mut files = vec![];
    walk(&root, &mut files);
    let file_named = |names: &[&str]| files.iter().find(|p| names.contains(&p.file_name().unwrap().to_str().unwrap_or(""))).cloned();
    let shared = file_named(&["TKernel.dll", "libTKernel.so", "libTKernel.dylib"]).is_some();
    let kernel = file_named(&["TKernel.lib", "libTKernel.a", "libTKernel.so", "libTKernel.dylib"])
        .unwrap_or_else(|| panic!("no OCCT libraries under OCCT_DIR={}", root.display()));
    let lib = kernel.parent().unwrap().to_path_buf();
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let macos = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos");
    let apple = env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple");

    let mut b = cxx_build::bridge("src/ffi.rs");
    for s in SOURCES {
        b.file(format!("cpp/{s}"));
    }
    b.include(&inc).std("c++17");
    if msvc {
        b.flag("/EHsc").flag("/bigobj").flag("/utf-8").define("WNT", None);
    }
    b.compile("oc_kernel_occt_shim");

    let present: BTreeSet<String> = fs::read_dir(&lib)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.strip_suffix(".lib")
                .map(str::to_owned)
                .or_else(|| n.strip_prefix("lib").and_then(|s| s.strip_suffix(".a").or(s.strip_suffix(".so")).or(s.strip_suffix(".dylib"))).map(str::to_owned))
        })
        .filter(|n| n.starts_with("TK"))
        .collect();
    let deps = parse_targets(&files, apple);
    let order = link_order(&deps.tk, &present);
    let kind = if shared { "dylib" } else { "static" };
    println!("cargo:rustc-link-search=native={}", lib.display());
    for l in &order {
        println!("cargo:rustc-link-lib={kind}={l}");
    }
    if !shared {
        for s in &deps.system {
            println!("cargo:rustc-link-lib=dylib={s}");
        }
        for f in &deps.frameworks {
            println!("cargo:rustc-link-lib=framework={f}");
        }
    }
    if msvc {
        for l in ["user32", "advapi32", "gdi32", "ws2_32", "shell32"] {
            println!("cargo:rustc-link-lib=dylib={l}");
        }
    } else if macos {
        println!("cargo:rustc-link-lib=dylib=c++");
    } else {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
    if shared && !msvc {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }
    let linkage = if shared { "shared" } else { "static" };
    println!("cargo:rustc-env=OC_OCCT_VERSION={found}");
    println!("cargo:rustc-env=OC_OCCT_LINKAGE={linkage}");
    if shared && msvc {
        let bin = file_named(&["TKernel.dll"]).unwrap();
        println!("cargo:warning=OCCT {found} shared: put {} on PATH to run", bin.parent().unwrap().display());
    }
}
