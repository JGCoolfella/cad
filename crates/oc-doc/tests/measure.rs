//! Measurements for the plan's session log; ignored by default. Run with
//! `cargo test --release -p oc-doc --test measure -- --ignored --nocapture --test-threads=1`.
//!
//! `export_canonical` writes the canonical form of every corpus document to
//! `$OC_DOC_CANON_DIR` so it can be compared byte for byte with what JavaScript produces.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use oc_doc::*;

/// Counts live heap bytes, to measure what an undo step retains.
struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size() as isize, Ordering::Relaxed);
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

#[test]
#[ignore]
fn export_canonical() {
    let dir = std::env::var("OC_DOC_CANON_DIR").expect("set OC_DOC_CANON_DIR");
    let mut docs = common::corpus_documents();
    docs.push(common::CorpusDoc {
        label: "all-fields".into(),
        text: common::fixture("all-fields.ocpart"),
    });
    for d in docs {
        let name = d.label.replace(['/', ' ', '(', ')'], "_");
        let doc = load(&d.text).unwrap().doc;
        std::fs::write(format!("{dir}/{name}.rust.jcs"), canonical(&doc).unwrap()).unwrap();
        std::fs::write(format!("{dir}/{name}.input.json"), &d.text).unwrap();
    }
}

#[test]
#[ignore]
fn corpus_load_and_save_times() {
    let docs = common::corpus_documents();
    let (mut bytes, mut load_ms, mut save_ms, mut worst) = (0usize, 0.0, 0.0, (0.0, String::new()));
    for d in &docs {
        let t = Instant::now();
        let doc = load(&d.text).unwrap().doc;
        let l = ms(t);
        let t = Instant::now();
        let s = save(&doc).unwrap();
        save_ms += ms(t);
        load_ms += l;
        bytes += d.text.len();
        if l > worst.0 {
            worst = (l, d.label.clone());
        }
        assert!(!s.is_empty());
    }
    println!(
        "corpus: {} documents, {:.1} KB; load {load_ms:.2} ms total (slowest {:.2} ms, {}); save {save_ms:.2} ms total",
        docs.len(),
        bytes as f64 / 1024.0,
        worst.0,
        worst.1
    );
}

/// A large part: `copies` copies of the `patterns` part's features (fresh ids), plus a
/// sketch of `n` lines (2n points).
fn large(copies: usize, n: usize) -> PartDocument {
    let base =
        load(&std::fs::read_to_string(common::corpus_dir().join("patterns.ocpart")).unwrap())
            .unwrap()
            .doc;
    let mut doc = PartDocument::new("large", UnitSystem::Mmgs);
    for c in 0..copies {
        for f in &base.features {
            let mut f = (**f).clone();
            f.id = format!("{}_{c}", f.id).into();
            doc.features.push_back(Arc::new(f));
        }
    }
    let mut sketch = SketchData {
        next_dim: 1,
        ..SketchData::default()
    };
    for i in 0..n {
        let (a, b, l): (Id, Id, Id) = (
            format!("p{i}a").into(),
            format!("p{i}b").into(),
            format!("l{i}").into(),
        );
        sketch
            .points
            .insert(a.clone(), SketchPoint::new(a.clone(), i as f64, 0.0));
        sketch
            .points
            .insert(b.clone(), SketchPoint::new(b.clone(), i as f64, 1.0));
        sketch.entities.insert(
            l.clone(),
            SketchEntity::Line {
                id: l,
                p1: a,
                p2: b,
                construction: None,
                ext: None,
            },
        );
    }
    let plane = PlaneRef::Std {
        plane: StdPlane::Front,
    };
    doc.features.push_back(Arc::new(Feature::new(
        "big",
        "BigSketch",
        FeatureKind::Sketch(SketchFeature { plane, sketch }),
    )));
    doc
}

#[test]
#[ignore]
fn undo_step_cost_on_a_large_document() {
    let doc = large(200, 10_000);
    let text = save(&doc).unwrap();
    let t = Instant::now();
    let canon = canonical(&doc).unwrap();
    let snapshot_ms = ms(t);
    let t = Instant::now();
    let reloaded = load(&text).unwrap().doc;
    let load_ms = ms(t);
    assert_eq!(reloaded, doc);
    println!(
        "large part: {} features, sketch of 10k lines; JSON {:.1} MB (canonical {:.1} MB); load {load_ms:.0} ms; one full canonical snapshot (the TypeScript undo approach) {snapshot_ms:.1} ms",
        doc.features.len(),
        text.len() as f64 / 1e6,
        canon.len() as f64 / 1e6
    );
    drop((text, canon, reloaded));

    let mut h = History::new(doc);
    let steps = 200;
    let mid = h.doc().features[h.doc().features.len() / 2].id.clone();
    for (label, edit) in [
        ("feature rename", 0),
        ("sketch point move", 1),
        ("sketch point add", 2),
    ] {
        let before = live();
        let t = Instant::now();
        for i in 0..steps {
            h.transact(label, |d| -> Result<(), ()> {
                if edit == 0 {
                    d.feature_mut(&mid).unwrap().name = format!("n{i}");
                } else if edit == 1 {
                    let s = d.feature_mut("big").unwrap().sketch_mut().unwrap();
                    s.points
                        .get_mut(format!("p{}a", i * 37 % 10_000).as_str())
                        .unwrap()
                        .x += 1.0;
                } else {
                    let s = d.feature_mut("big").unwrap().sketch_mut().unwrap();
                    let id = Id::from(format!("q{i}"));
                    s.points
                        .insert(id.clone(), SketchPoint::new(id, i as f64, 0.0));
                }
                Ok(())
            })
            .unwrap();
        }
        let per_step_us = ms(t) * 1e3 / steps as f64;
        let per_step_bytes = (live() - before) as f64 / steps as f64;
        let t = Instant::now();
        for _ in 0..steps {
            h.undo();
        }
        for _ in 0..steps {
            h.redo();
        }
        let swap_us = ms(t) * 1e3 / (2 * steps) as f64;
        println!(
            "{label}: commit {per_step_us:.1} us, undo/redo {swap_us:.2} us, retained {:.1} KB per undo step",
            per_step_bytes / 1024.0
        );
    }
}
