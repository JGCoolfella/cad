//! Times the geometry host's part job (`parallel::work`: regenerate, export the body, mesh)
//! over the corpus parts, the same program natively and in WebAssembly (W1a):
//!
//! `cargo run --release -p oc-asm --example regen -- corpus [rounds] [kernel threads]`
//! `node scripts/wasm.mjs cargo run --release --target wasm32-unknown-emscripten -p oc-asm --example regen -- corpus [rounds]`
//!
//! Each round runs every part once on a fresh regenerator (no cache), as a host does; the
//! first round includes OCCT's first-use costs. Prints one line per part (the best round) and
//! a `REGEN {...}` summary with the totals per round.
use oc_asm::parallel::{work, PartJob};
use oc_graph::Regenerator;
use oc_kernel_occt::OcctKernel;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().unwrap_or_else(|| "corpus".into());
    let rounds: usize = args.next().map_or(3, |s| s.parse().expect("rounds"));
    // OCCT's pool (0: one thread per logical processor; WebAssembly always has one).
    let threads: usize = args.next().map_or(0, |s| s.parse().expect("threads"));
    oc_kernel::Kernel::set_threads(&OcctKernel::default(), threads).expect("threads");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{dir}: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "ocpart"))
        .collect();
    files.sort();
    let jobs: Vec<PartJob> = files
        .iter()
        .map(|p| {
            let text = std::fs::read_to_string(p).expect("part file");
            let doc = oc_doc::load(&text).unwrap_or_else(|e| panic!("{}: {e:?}", p.display())).doc;
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            PartJob { hash: name, doc: Arc::new(doc), quality: 0.5 }
        })
        .collect();
    let mut best = vec![f64::INFINITY; jobs.len()];
    let mut totals = vec![];
    let mut checks = vec![(0usize, 0usize, 0usize); jobs.len()];
    for _ in 0..rounds {
        let t = Instant::now();
        for (i, job) in jobs.iter().enumerate() {
            let mut regen = Regenerator::new(OcctKernel::default());
            let s = Instant::now();
            let out = work(&mut regen, job).unwrap_or_else(|e| panic!("{}: {e}", job.hash));
            best[i] = best[i].min(s.elapsed().as_secs_f64() * 1e3);
            let mesh = out.mesh.as_ref();
            checks[i] = (out.body.as_ref().map_or(0, |b| b.0.len()), mesh.map_or(0, |m| m.lods.len()), mesh.map_or(0, |m| m.lods[0].triangle_count()));
        }
        totals.push(t.elapsed().as_secs_f64() * 1e3);
    }
    for ((job, ms), (blob, lods, tris)) in jobs.iter().zip(&best).zip(&checks) {
        println!("{:>18} {ms:>9.2} ms  blob {blob:>8} B  levels {lods}  triangles {tris}", job.hash);
    }
    let rounds: Vec<String> = totals.iter().map(|t| format!("{t:.1}")).collect();
    println!(
        "REGEN {{\"target\":\"{}\",\"threads\":{threads},\"parts\":{},\"best_sum_ms\":{:.1},\"rounds_ms\":[{}]}}",
        std::env::consts::ARCH,
        jobs.len(),
        best.iter().sum::<f64>(),
        rounds.join(",")
    );
}
