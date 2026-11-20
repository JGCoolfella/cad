//! The generated scale assembly (G3): its parts regenerate as intended, and (ignored) the
//! timings of opening and rebuilding the 10 000-component assembly of G2's benchmark.
//!
//! `cargo test --release -p oc-asm --test scale -- --ignored --nocapture`
mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

use common::*;
use oc_asm::{scale, Cache, Memory};
use oc_doc::container::Container;

fn memory_of(files: &[(String, String)]) -> Memory {
    let mut m = Memory::default();
    for (name, text) in files {
        m.0.insert(PathBuf::from(format!("/w/scale/{name}")), text.clone().into_bytes());
    }
    m
}

#[test]
fn scale_parts_are_distinct_blocks_with_holes() {
    let (files, doc) = scale::generate(8, 40);
    let m = memory_of(&files);
    let mut a = assembler();
    let b = a.build(&doc, Path::new("/w/scale"), &m, &opts(false));
    assert_eq!(a.regenerations, 8);
    assert_eq!(b.occurrences.len(), 40);
    assert!(b.components.iter().all(|c| c.error.is_none() && c.warning.is_none()), "{:?}", b.components);
    let k = a.regen.kernel();
    for (i, c) in b.components.iter().enumerate() {
        let p = &a.parts[c.part.as_ref().unwrap()];
        let live = p.live.as_ref().unwrap();
        assert!(live.errors.is_empty(), "{}: {:?}", p.name, live.errors);
        let topo = live.shape.as_ref().unwrap().topo(k).unwrap();
        assert_eq!(topo.faces.len(), 6 + 1 + i % 4, "{}: a block with {} holes", p.name, 1 + i % 4);
        assert!(p.mesh.is_some());
    }
    assert_eq!(a.parts.len(), 8, "eight distinct hashes");
}

/// Peak and current working set of this process, MB (Windows).
#[cfg(windows)]
#[allow(unsafe_code)]
fn memory_mb() -> (f64, f64) {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: plain Win32 call filling a struct of the size given.
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
        (c.PeakWorkingSetSize as f64 / 1048576.0, c.WorkingSetSize as f64 / 1048576.0)
    }
}

#[cfg(not(windows))]
fn memory_mb() -> (f64, f64) {
    (0.0, 0.0)
}

/// The part with its extrusion 0.5 mm deeper.
fn deeper(text: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(text).unwrap();
    let d = &mut v["features"][1]["end1"]["depth"];
    *d = serde_json::json!(d.as_f64().unwrap() + 0.5);
    serde_json::to_string(&v).unwrap()
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

#[test]
#[ignore = "timing report"]
fn scale_10k_timings() {
    let (files, doc) = scale::generate(100, 10_000);
    let m = memory_of(&files);
    let dir = Path::new("/w/scale");
    println!("threads available: {:?}", std::thread::available_parallelism());
    let (_, base) = memory_mb();
    // Cold open: every part regenerated and meshed, one by one on this thread, then on
    // workers (`OC_SCALE_SERIAL_FIRST=0` skips the serial one, for a clean peak memory).
    if std::env::var("OC_SCALE_SERIAL_FIRST").as_deref() != Ok("0") {
        let mut s = assembler();
        s.workers = 1;
        let t = Instant::now();
        let _ = s.build(&doc, dir, &m, &opts(false));
        println!("cold open, serial: {:.0} ms ({:?})", ms(t), s.stats);
    }
    // `OC_SCALE_WORKERS=2,4`: cold opens on that many workers too (scaling).
    for n in std::env::var("OC_SCALE_WORKERS").unwrap_or_default().split(',').filter_map(|x| x.parse::<usize>().ok()) {
        let mut s = assembler();
        s.workers = n;
        let t = Instant::now();
        let _ = s.build(&doc, dir, &m, &opts(false));
        println!("cold open, {n} workers: {:.0} ms", ms(t));
    }
    let mut a = assembler();
    let t = Instant::now();
    let b = a.build(&doc, dir, &m, &opts(false));
    let cold = ms(t);
    assert_eq!(a.regenerations, 100);
    assert_eq!(b.occurrences.len(), 10_000);
    let (peak, now) = memory_mb();
    println!("cold open: {cold:.0} ms, {:?}; working set {base:.0} -> {now:.0} MB, peak {peak:.0} MB", a.stats);
    // Rebuild, nothing changed (a mate edit's rebuild).
    let t = Instant::now();
    let _ = a.build(&doc, dir, &m, &opts(false));
    println!("rebuild, nothing changed: {:.1} ms", ms(t));
    // One part edited: one regeneration.
    let mut m2 = memory_of(&files);
    let edited = deeper(&files[7].1);
    assert_ne!(edited, files[7].1);
    m2.0.insert(PathBuf::from(format!("/w/scale/{}", files[7].0)), edited.into_bytes());
    let before = a.regenerations;
    let t = Instant::now();
    let _ = a.build(&doc, dir, &m2, &opts(false));
    println!("rebuild, one part edited: {:.1} ms ({} regenerated, {:?})", ms(t), a.regenerations - before, a.stats);
    // Ten parts edited.
    let mut m3 = memory_of(&files);
    for (name, text) in files.iter().take(10) {
        m3.0.insert(PathBuf::from(format!("/w/scale/{name}")), deeper(text).into_bytes());
    }
    let before = a.regenerations;
    let t = Instant::now();
    let _ = a.build(&doc, dir, &m3, &opts(false));
    println!("rebuild, ten parts edited: {:.1} ms ({} regenerated, {:?})", ms(t), a.regenerations - before, a.stats);
    // Save (container with every mesh), then a lightweight open in a fresh engine.
    let t = Instant::now();
    let c = a.container(&doc, &b, 3.0).unwrap();
    let bytes = c.to_bytes();
    println!("save: {:.0} ms, {:.1} MB", ms(t), bytes.len() as f64 / 1048576.0);
    let t = Instant::now();
    let c = Container::from_bytes(&bytes).unwrap();
    let loaded = oc_doc::load_assembly(c.document().unwrap()).unwrap();
    let mut light = assembler();
    light.set_cache(Cache::read(&c));
    let read = ms(t);
    let t = Instant::now();
    let lb = light.build(&loaded.doc, dir, &m, &opts(true));
    println!("lightweight open: read {read:.0} ms + build {:.0} ms ({} regenerated)", ms(t), light.regenerations);
    assert_eq!(lb.occurrences.len(), 10_000);
    // Resolving every lightweight part (Mass Properties, Interference Detection).
    let t = Instant::now();
    let hashes: Vec<String> = lb.components.iter().filter_map(|c| c.part.clone()).collect();
    assert!(light.resolve_many(&hashes, 3.0));
    println!("resolve 100 lightweight parts: {:.0} ms ({:?})", ms(t), light.stats);
}
