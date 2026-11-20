//! The body budget (G3c), in a test binary of its own so the working set is this test's: with
//! a budget of three bodies, an assembly of twelve large plates keeps at most about three
//! live, and evicting and reading bodies back over and over does not grow the process.
mod common;

use std::path::{Path, PathBuf};

use common::*;
use oc_asm::{scale, Memory};

#[cfg(windows)]
#[allow(unsafe_code)]
fn working_set_mb() -> f64 {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    // SAFETY: plain Win32 call filling a struct of the size given.
    unsafe {
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
        c.WorkingSetSize as f64 / 1048576.0
    }
}

#[cfg(windows)]
#[test]
fn bodies_stay_within_the_budget_through_evictions_and_reads() {
    let (files, doc) = scale::plates(12, 12, 9);
    let mut m = Memory::default();
    for (name, text) in &files {
        m.0.insert(PathBuf::from(format!("/w/p/{name}")), text.clone().into_bytes());
    }
    let mut a = assembler();
    let b = a.build(&doc, Path::new("/w/p"), &m, &opts(false));
    let all: Vec<String> = b.components.iter().filter_map(|c| c.part.clone()).collect();
    let one = all.iter().map(|h| a.brep_of(h).unwrap().len() * oc_asm::outofcore::BODY_PER_BLOB_BYTE).max().unwrap();
    a.body_budget = Some(one * 3);
    a.keep_budget();
    assert!(a.body_memory() <= one * 3, "{} > {}", a.body_memory(), one * 3);
    let mut sets = vec![];
    for round in 0..6 {
        // Every part used (an evaluation of the whole assembly), then back within budget.
        for h in &all {
            assert!(a.resolve(h, 3.0));
            assert!(a.body_memory() <= one * 3, "round {round}");
        }
        assert_eq!(a.regenerations, 12, "round {round}: read back, never regenerated");
        sets.push(working_set_mb());
    }
    println!("working set per round: {sets:.0?} MB");
    // After the first rounds the allocator has what it needs: no growth per round.
    assert!(sets[5] <= sets[1] + 8.0, "the working set grows: {sets:?}");
}
