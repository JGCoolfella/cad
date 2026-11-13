//! Determinism under thread counts (`docs/OVERHAUL_PLAN.md` sections 2 and 3.4): every
//! corpus part and torture document rebuilt with 1, 4 and 16 kernel threads (and all of
//! them) gives the same names, statuses, cache keys, metrics and face and edge
//! signatures, bit for bit. A test binary of its own, with one test: OCCT's thread pool
//! is process-wide and refuses to be resized while another test is in a kernel call.
mod common;

use common::*;
use oc_doc::PartDocument;
use oc_graph::apply_heals;
use oc_kernel::Kernel;

/// Everything a rebuild produces that must not vary, for the legacy document and for the
/// document with its names written back (so both resolution paths are covered).
fn fingerprints(threads: usize) -> Vec<(String, Vec<String>)> {
    let mut docs: Vec<(String, PartDocument)> = PARTS.iter().map(|p| (p.to_string(), load(p))).collect();
    let dir = corpus_dir().join("torture");
    let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    for f in files {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
        docs.push((f.file_stem().unwrap().to_string_lossy().into_owned(), oc_doc::load(&v["doc"].to_string()).unwrap().doc));
    }
    let mut out = vec![];
    for (name, mut doc) in docs {
        let mut r = regen();
        r.kernel().set_threads(threads).unwrap();
        let mut lines = vec![];
        for pass in ["legacy", "named"] {
            let reb = rebuild(&mut r, &doc);
            assert_eq!(reb.untracked, 0, "{name}");
            for s in &reb.steps {
                lines.push(format!("{pass} {:?} {:?}", s.status, s.key));
                let Some(b) = &s.body else { continue };
                let t = b.topo(r.kernel()).unwrap();
                lines.push(body_json(r.kernel(), &b.body).to_string());
                for (f, n) in t.faces.iter().zip(&b.names.faces) {
                    lines.push(format!("{n:?} {:?} {:?} {:?} {:?}", f.surface, f.centroid.map(f64::to_bits), f.normal.map(f64::to_bits), f.area.to_bits()));
                }
                for e in &t.edges {
                    lines.push(format!("{:?} {:?} {:?} {:?}", e.curve, e.start.map(f64::to_bits), e.end.map(f64::to_bits), e.length.to_bits()));
                }
            }
            lines.push(format!("{} heals", reb.heals.len()));
            apply_heals(&mut doc, &reb.heals);
            // The second pass must resolve by name, not from the cache.
            r.clear_cache();
        }
        out.push((name, lines));
    }
    out
}

#[test]
fn same_names_and_geometry_with_any_thread_count() {
    let reference = fingerprints(1);
    let lines: usize = reference.iter().map(|(_, l)| l.len()).sum();
    assert!(reference.len() == 26 && lines > 10_000, "{} documents, {lines} lines", reference.len());
    for threads in [1, 4, 16, 0] {
        let again = fingerprints(threads);
        for ((name, a), (_, b)) in reference.iter().zip(&again) {
            if a != b {
                let at = a.iter().zip(b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
                panic!("{name}: differs with {threads} threads at line {at}:\n  {:?}\n  {:?}", a.get(at), b.get(at));
            }
        }
    }
}
