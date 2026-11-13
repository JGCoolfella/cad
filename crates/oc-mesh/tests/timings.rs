//! Timing report (not a pass/fail test):
//! `cargo test --release -p oc-mesh --test timings -- --ignored --nocapture`.
mod common;

use common::*;
use oc_kernel::{Kernel, Mesh, MeshParams};
use oc_mesh::*;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// Median of `n` runs of `f`, in ms.
fn median(n: usize, mut f: impl FnMut()) -> f64 {
    let mut v: Vec<f64> = (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            ms(t)
        })
        .collect();
    v.sort_by(|a, b| a.total_cmp(b));
    v[n / 2]
}

fn report(k: &K, name: &str, b: &oc_kernel_occt::Body, quality: f64, runs: usize) {
    let opts = MeshOptions {
        quality,
        ..MeshOptions::default()
    };
    let m = build(k, b, &opts).unwrap();
    let topo = k.topology(b).unwrap();
    let size = oc_mesh::display_box(&k.topology(b).unwrap()).diagonal();
    let tol = *k.tolerances();
    let levels: Vec<(MeshParams, Mesh)> = (0..m.lods.len() as u32)
        .map(|l| {
            let p = tol.display_lod(quality, size, l);
            (p, k.tessellate(b, &p).unwrap())
        })
        .collect();
    let t_topo = median(runs, || drop(k.topology(b).unwrap()));
    let t_l0 = median(runs, || drop(k.tessellate(b, &levels[0].0).unwrap()));
    let t_pack = median(runs, || drop(pack(&levels, &topo, m.origin)));
    let t_all = median(runs, || drop(build(k, b, &opts).unwrap()));
    let tris: Vec<String> = m
        .lods
        .iter()
        .map(|l| l.triangle_count().to_string())
        .collect();
    let bytes = m.to_bytes().len();
    println!(
        "| {name} | {quality} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.1} |",
        m.face_count(),
        tris.join(" / "),
        t_topo,
        t_l0,
        t_pack,
        t_all,
        bytes as f64 / 1e6
    );
}

#[test]
#[ignore]
fn timings() {
    let k = kernel();
    k.set_threads(0).unwrap();
    println!("| part | quality | faces | triangles per level | topology ms | kernel mesh L0 ms | pack all levels ms | build ms | blob MB |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for (name, b) in corpus_parts(&k) {
        report(&k, name, &b, 3.0, 9);
    }
    report(&k, "plate, 432 holes", &holed_plate(&k, 24, 18), 3.0, 3);
    let t = Instant::now();
    let large = large_part(&k, 24, 18);
    println!("large part built in {:.0} ms", ms(t));
    for q in [3.0, 6.0, 8.0] {
        report(&k, "large", &large, q, 3);
    }
    // Thread scaling on the large part (kernel and packing).
    let opts = MeshOptions {
        quality: 6.0,
        ..MeshOptions::default()
    };
    let topo = k.topology(&large).unwrap();
    let size = display_box(&k.topology(&large).unwrap()).diagonal();
    for threads in [1, 4, 16] {
        k.set_threads(threads).unwrap();
        let p = k.tolerances().display_mesh(6.0, size);
        let t_mesh = median(3, || drop(k.tessellate(&large, &p).unwrap()));
        let levels = vec![(p, k.tessellate(&large, &p).unwrap())];
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let t_pack = median(3, || drop(pool.install(|| pack(&levels, &topo, [0.0; 3]))));
        let t_build = median(3, || {
            drop(build(&k, &large, &MeshOptions { threads, ..opts }).unwrap())
        });
        println!("threads {threads}: kernel mesh L0 {t_mesh:.1} ms, pack L0 {t_pack:.1} ms, build (all levels) {t_build:.1} ms");
    }
    k.set_threads(0).unwrap();
    let m = build(&k, &large, &opts).unwrap();
    let bytes = m.to_bytes();
    let t = Instant::now();
    let h = content_hash(&bytes);
    println!(
        "content hash of {:.1} MB: {:.1} ms ({h:016x})",
        bytes.len() as f64 / 1e6,
        ms(t)
    );
    let t = Instant::now();
    let back = RenderMesh::from_bytes(&bytes).unwrap();
    println!(
        "from_bytes {:.1} ms, to_bytes {:.1} ms",
        ms(t),
        median(3, || drop(back.to_bytes()))
    );
}
