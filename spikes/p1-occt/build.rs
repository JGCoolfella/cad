//! Compiles the C++ shim and links a static OCCT install named by OCCT_DIR
//! (see build-occt.cmd / build-occt.sh). Handles the Windows (inc/, win64/vc14/lib) and
//! Unix (include/opencascade, lib) install layouts.
//!
//! Static archives must be linked dependents-first on Unix, so the order (and the system
//! libraries OCCT needs) comes from the `INTERFACE_LINK_LIBRARIES` of the CMake target
//! files OCCT installs.
use std::collections::{BTreeMap, BTreeSet};
use std::{env, fs, path::{Path, PathBuf}};

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

fn parse_targets(files: &[PathBuf]) -> Deps {
    let mut d = Deps::default();
    let apple = std::env::var("CARGO_CFG_TARGET_VENDOR").as_deref() == Ok("apple");
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
    println!("cargo:rerun-if-changed=cpp/shim.cpp");
    let root = PathBuf::from(env::var("OCCT_DIR").expect("set OCCT_DIR to an OCCT install"));
    let inc = [root.join("inc"), root.join("include/opencascade")]
        .into_iter()
        .find(|p| p.join("Standard.hxx").exists())
        .expect("no OCCT headers under OCCT_DIR");
    let mut files = vec![];
    walk(&root, &mut files);
    let kernel = files
        .iter()
        .find(|p| matches!(p.file_name().unwrap().to_str(), Some("TKernel.lib" | "libTKernel.a")))
        .expect("no static OCCT libraries under OCCT_DIR");
    let lib = kernel.parent().unwrap().to_path_buf();
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let macos = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos");

    let mut b = cxx_build::bridge("src/lib.rs");
    b.file("cpp/shim.cpp").include(&inc).std("c++17");
    if msvc {
        b.flag("/EHsc").flag("/bigobj").define("WNT", None);
    }
    b.compile("p1shim");

    let present: BTreeSet<String> = fs::read_dir(&lib)
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
    let deps = parse_targets(&files);
    let order = link_order(&deps.tk, &present);
    println!("cargo:rustc-link-search=native={}", lib.display());
    for l in &order {
        println!("cargo:rustc-link-lib=static={l}");
    }
    for s in &deps.system {
        println!("cargo:rustc-link-lib=dylib={s}");
    }
    for f in &deps.frameworks {
        println!("cargo:rustc-link-lib=framework={f}");
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
    println!("cargo:warning=OCCT link order: {}", order.join(" "));
    if !deps.system.is_empty() || !deps.frameworks.is_empty() {
        println!("cargo:warning=OCCT system libs: {:?} frameworks: {:?}", deps.system, deps.frameworks);
    }
}
