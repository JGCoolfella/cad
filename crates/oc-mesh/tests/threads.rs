//! Determinism under thread counts (plan section 2): the same bytes run after run and
//! with 1, 4 or 16 threads in the kernel (OCCT's pool) and in packing. A test binary of
//! its own, with one test: OCCT refuses to resize its pool while another test meshes.
mod common;

use common::*;
use oc_kernel::Kernel;
use oc_mesh::*;

fn blobs(k: &K, threads: usize) -> Vec<(&'static str, Vec<u8>)> {
    k.set_threads(threads).unwrap();
    let opts = MeshOptions {
        threads,
        ..MeshOptions::default()
    };
    let mut parts = corpus_parts(k);
    parts.push(("sphere", sphere(k, 15.0)));
    parts.push(("large", large_part(k, 6, 5)));
    parts
        .iter()
        .map(|(name, b)| (*name, build(k, b, &opts).unwrap().to_bytes()))
        .collect()
}

#[test]
#[cfg_attr(target_family = "wasm", ignore = "thread pools: WebAssembly has no threads (W1 design point 1)")]
fn same_bytes_run_after_run_and_with_any_thread_count() {
    let reference = blobs(&kernel(), 1);
    for threads in [1, 4, 16, 0] {
        let again = blobs(&kernel(), threads);
        for ((name, a), (_, b)) in reference.iter().zip(&again) {
            assert!(a == b, "{name}: bytes differ with {threads} threads");
        }
    }
}
