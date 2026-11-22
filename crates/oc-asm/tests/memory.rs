//! Memory budget of regenerating parts at once (G3a), in a test binary of its own so the
//! process' peak working set is this test's. Opening the generated 10 000-component assembly
//! (100 distinct parts) cold, on one worker per logical processor, must stay within a budget:
//! each worker holds one part's regeneration at a time (it drops its cache after every
//! part), so the peak grows with the number of workers, not with the number of parts.
//! Budgets for large parts and out-of-core assemblies are G3c's.
// The measurements are Windows-only (process working set); elsewhere the file has no tests.
#![cfg_attr(not(windows), allow(unused_imports, dead_code))]

mod common;

use std::path::{Path, PathBuf};

use common::*;
use oc_asm::{scale, Memory};

/// Peak and current working set of this process, MB.
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

/// Measured 2026-11-20 (release and debug, 8 workers): the peak grew by 29 MB (on one thread,
/// before G3a: 36 MB, with another test running). The budget leaves room for allocator and thread-count
/// differences. These parts are small: a worker keeping its regeneration cache adds under
/// 1 MB here and is not caught; per-part budgets on large parts are G3c's.
const BUDGET_MB: f64 = 96.0;

#[cfg(windows)]
#[test]
#[cfg_attr(target_family = "wasm", ignore = "worker threads and process memory: WebAssembly has neither (W1 design point 1)")]
fn opening_ten_thousand_components_in_parallel_stays_within_budget() {
    let (files, doc) = scale::generate(100, 10_000);
    let mut m = Memory::default();
    for (name, text) in &files {
        m.0.insert(PathBuf::from(format!("/w/scale/{name}")), text.clone().into_bytes());
    }
    let (_, base) = memory_mb();
    let mut a = assembler();
    let b = a.build(&doc, Path::new("/w/scale"), &m, &opts(false));
    assert_eq!(b.occurrences.len(), 10_000);
    assert_eq!(a.stats.regenerated, 100);
    assert!(a.stats.workers > 1 || std::thread::available_parallelism().map_or(1, |n| n.get()) < 4, "{:?}", a.stats);
    let (peak, now) = memory_mb();
    println!("working set {base:.0} -> {now:.0} MB, peak {peak:.0} MB on {} workers", a.stats.workers);
    assert!(peak - base <= BUDGET_MB, "the peak grew by {:.0} MB (budget {BUDGET_MB} MB)", peak - base);
    // The assembler keeps the 100 parts' bodies and meshes, nothing of the workers'.
    assert!(now - base <= BUDGET_MB / 2.0, "{:.0} MB kept after the build", now - base);
}
