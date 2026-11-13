//! Regeneration behaviour: cache reuse along the DAG, cancellation, failures passing the
//! body through, suppression, rollback, determinism and sketch model references.
mod common;

use std::collections::BTreeSet;
use std::sync::Arc;

use common::*;
use oc_doc::Feature;
use oc_graph::graph::analyse;
use oc_graph::{CancelToken, Cancelled, FAILED};
use oc_kernel::Kernel;
use serde_json::{json, Value};

fn edit(fs: &[Arc<Feature>], id: &str, f: impl FnOnce(&mut Value)) -> Vec<Arc<Feature>> {
    let mut f = Some(f);
    fs.iter()
        .map(|x| {
            if x.id.as_str() != id {
                return x.clone();
            }
            let mut v = serde_json::to_value(&**x).unwrap();
            (f.take().unwrap())(&mut v);
            Arc::new(serde_json::from_value(v).unwrap())
        })
        .collect()
}

fn ids<B>(r: &oc_graph::Rebuild<B>, cached: bool) -> BTreeSet<String> {
    r.steps.iter().filter(|s| s.cached == cached && s.key.is_some()).map(|s| s.status.id.clone()).collect()
}

fn set(x: &[&str]) -> BTreeSet<String> {
    x.iter().map(|s| s.to_string()).collect()
}

#[test]
fn warm_rebuild_is_cached_and_identical() {
    let doc = load("patterns");
    let mut r = regen();
    let a = rebuild(&mut r, &doc);
    assert!(a.steps.iter().all(|s| !s.cached));
    let b = rebuild(&mut r, &doc);
    assert!(b.steps.iter().all(|s| s.cached), "every step comes from the cache");
    assert!(b.steps.iter().all(|s| s.time.is_zero()));
    for (x, y) in a.steps.iter().zip(&b.steps) {
        assert_eq!(x.key, y.key);
        assert_eq!(x.status, y.status);
    }
    let k = r.kernel();
    assert_eq!(body_json(k, &a.body.unwrap().body), body_json(k, &b.body.unwrap().body));
}

#[test]
fn an_edit_recomputes_only_what_reads_it() {
    let doc = load("refgeom");
    let fs = features(&doc);
    let mut r = regen();
    r.regenerate(&fs, &CancelToken::new()).unwrap();
    // A reference point nothing uses: only itself (the TypeScript engine recomputes
    // everything after the edited feature).
    let moved = edit(&fs, "f16", |v| v["xyz"] = json!([6, 5, 5]));
    let b = r.regenerate(&moved, &CancelToken::new()).unwrap();
    assert_eq!(ids(&b, false), set(&["f16"]));
    assert_eq!(b.points["f16"], [6.0, 5.0, 5.0]);
    // The first boss: everything that reads the body, but not sketches on standard planes
    // or reference geometry from vertices and standard planes.
    let taller = edit(&fs, "f2", |v| v["end1"]["depth"] = json!(25));
    let c = r.regenerate(&taller, &CancelToken::new()).unwrap();
    assert_eq!(ids(&c, true), set(&["f1", "f3", "f5", "f8", "f11", "f12", "f14", "f16"]));
    assert!(c.steps.iter().all(|s| s.status.error.is_none()));
    // Plane2 (mid plane of two faces) is built from the faces' stored signatures, as in the
    // TypeScript engine, so it does not follow the taller boss. Persistent naming (E6)
    // resolves such references against the live body.
    assert!((c.planes["f6"].origin[2] - 10.0).abs() < 1e-9, "{:?}", c.planes["f6"]);
}

#[test]
fn dependencies_follow_references_and_the_body_chain() {
    let fs = features(&load("patterns"));
    let nodes = analyse(&fs);
    let pos = |id: &str| fs.iter().position(|f| f.id.as_str() == id).unwrap();
    let mirror = nodes[pos("f14")].as_ref().unwrap();
    assert_eq!(mirror.deps, vec![pos("f4"), pos("f13")]);
    assert!(mirror.writes_body && mirror.reads_body);
    assert_eq!(mirror.body_from, Some(pos("f12")));
    let sketch4 = nodes[pos("f10")].as_ref().unwrap();
    assert!(sketch4.reads_body && !sketch4.writes_body, "a sketch on a face reads the body");
    let plane1 = nodes[pos("f13")].as_ref().unwrap();
    assert!(!plane1.reads_body && plane1.deps.is_empty() && plane1.body_from.is_none());
    let free = oc_graph::graph::body_free(&nodes);
    assert!(free[pos("f13")] && free[pos("f9")] && !free[pos("f10")]);
}

#[test]
fn cancellation_stops_between_features_and_keeps_finished_work() {
    let doc = load("patterns");
    let fs = features(&doc);
    let mut r = regen();
    let pre = CancelToken::new();
    pre.cancel();
    assert_eq!(r.regenerate(&fs, &pre).err(), Some(Cancelled { done: 0 }));
    let token = CancelToken::new();
    let mut seen = 0;
    let t = token.clone();
    let res = r.regenerate_observed(&fs, &token, &mut |_| {
        seen += 1;
        if seen == 6 {
            t.cancel();
        }
    });
    assert_eq!(res.err(), Some(Cancelled { done: 6 }));
    let again = rebuild(&mut r, &doc);
    let cached: Vec<bool> = again.steps.iter().map(|s| s.cached).collect();
    assert_eq!(&cached[..6], &[true; 6]);
    assert!(cached[6..].iter().all(|c| !c));
}

#[test]
fn a_failing_feature_passes_the_body_through() {
    let doc = load("fillet-chamfer");
    let fs = features(&doc);
    let mut r = regen();
    let ok = r.regenerate(&fs, &CancelToken::new()).unwrap();
    let broken = edit(&fs, "f3", |v| v["radius"] = json!(100));
    let b = r.regenerate(&broken, &CancelToken::new()).unwrap();
    assert_eq!(b.steps[2].status.error.as_deref(), Some(FAILED));
    let k = r.kernel();
    let m = |s: &oc_graph::Step<_>| body_json(k, &s.body.as_ref().unwrap().body);
    assert_eq!(m(&b.steps[2]), m(&ok.steps[1]), "Fillet1 passes Boss-Extrude1's body on");
    assert!(b.steps[3..].iter().all(|s| s.status.error.is_none()), "later features still build");

    // Messages are the TypeScript engine's.
    let doc = load("block-hole");
    let fs = features(&doc);
    let open = edit(&fs, "f1", |v| {
        let first = v["sketch"]["entities"].as_object().unwrap().keys().next().unwrap().clone();
        v["sketch"]["entities"].as_object_mut().unwrap().shift_remove(&first);
    });
    let b = r.regenerate(&open, &CancelToken::new()).unwrap();
    assert_eq!(b.steps[1].status.error.as_deref(), Some("The sketch contains open contours. Use Thin Feature, or close the sketch."));
    assert_eq!(b.steps[3].status.error.as_deref(), Some("There is no body to cut"));
    assert!(b.body.is_none());
    let lost = edit(&fs, "f4", |v| v["sketchId"] = json!("nope"));
    let b = r.regenerate(&lost, &CancelToken::new()).unwrap();
    assert_eq!(b.steps[3].status.error.as_deref(), Some("Sketch not found"));
    // A reference to a later sketch is unresolved, as in TypeScript.
    let mut reordered = fs.clone();
    reordered.swap(2, 3);
    let b = r.regenerate(&reordered, &CancelToken::new()).unwrap();
    assert_eq!(b.steps[2].status.error.as_deref(), Some("Sketch plane unresolved"));
}

#[test]
fn suppression_and_rollback() {
    let mut doc = load("block-hole");
    let fs = features(&doc);
    let mut r = regen();
    let s = edit(&fs, "f4", |v| v["suppressed"] = json!(true));
    let b = r.regenerate(&s, &CancelToken::new()).unwrap();
    assert_eq!(b.steps[3].status, oc_graph::Status { id: "f4".into(), ..Default::default() });
    assert!(b.steps[3].key.is_none());
    let k = r.kernel();
    assert_eq!(body_json(k, &b.body.unwrap().body)["faces"], json!(6));
    doc.rollback = Some(2);
    let b = r.regenerate_doc(&doc, &CancelToken::new()).unwrap();
    assert_eq!(b.steps.len(), 2);
}

#[test]
fn cold_rebuilds_are_identical() {
    for part in ["patterns", "shell", "thin-contours"] {
        let doc = load(part);
        let (mut a, mut b) = (regen(), regen());
        let (x, y) = (rebuild(&mut a, &doc), rebuild(&mut b, &doc));
        for (s, t) in x.steps.iter().zip(&y.steps) {
            assert_eq!(s.key, t.key);
            if let (Some(p), Some(q)) = (&s.body, &t.body) {
                assert_eq!(a.kernel().topology(&p.body).unwrap(), b.kernel().topology(&q.body).unwrap(), "{part} {}", s.status.id);
            }
        }
    }
}

fn ref_sketch(edge_curve: &str) -> Arc<Feature> {
    let f: Feature = serde_json::from_value(json!({
        "id": "s9", "name": "Sketch9", "type": "sketch", "plane": { "kind": "std", "plane": "front" },
        "sketch": {
            "points": {
                "origin": { "id": "origin", "x": 0, "y": 0 },
                "a": { "id": "a", "x": 20, "y": 10, "ext": "x1" },
                "b": { "id": "b", "x": 40, "y": 12 },
                "c": { "id": "c", "x": 0, "y": 10, "ext": "x2" },
                "d": { "id": "d", "x": 20, "y": 10, "ext": "x2" }
            },
            "entities": {
                "l": { "id": "l", "type": "line", "p1": "a", "p2": "b" },
                "m": { "id": "m", "type": "line", "p1": "c", "p2": "d", "ext": "x2" }
            },
            "relations": { "h": { "id": "h", "type": "horizontal", "e": "l" } },
            "dimensions": {},
            "nextDim": 1,
            "refs": {
                "x1": { "kind": "vertex", "id": "x1", "p": [20, 10, 5],
                        "edges": [{ "curve": "LINE", "mid": [10, 10, 5], "a": [0, 10, 5], "b": [20, 10, 5], "length": 20 }] },
                "x2": { "kind": "edge", "id": "x2",
                        "sig": { "curve": edge_curve, "mid": [10, 10, 5], "a": [0, 10, 5], "b": [20, 10, 5], "length": 20 } }
            }
        }
    }))
    .unwrap();
    Arc::new(f)
}

#[test]
fn sketch_references_follow_the_model() {
    let doc = load("block-hole");
    let mut fs = features(&doc);
    fs.push(ref_sketch("LINE"));
    let mut r = regen();
    let b = r.regenerate(&fs, &CancelToken::new()).unwrap();
    assert_eq!(b.steps[4].status, oc_graph::Status { id: "s9".into(), ..Default::default() });
    let sk = &b.sketches["s9"];
    // Re-solved: the horizontal line's free end comes level with the referenced vertex.
    assert!((sk.points["b"].y - 10.0).abs() < 1e-9 && (sk.points["a"].x - 20.0).abs() < 1e-12);
    // Widen the block: the vertex and the edge follow.
    let wide = edit(&fs, "f1", |v| {
        for p in v["sketch"]["points"].as_object_mut().unwrap().values_mut() {
            if p["x"].as_f64() == Some(20.0) {
                p["x"] = json!(30);
            }
        }
    });
    let b = r.regenerate(&wide, &CancelToken::new()).unwrap();
    assert!(!b.steps[4].cached);
    let sk = &b.sketches["s9"];
    assert_eq!((sk.points["a"].x, sk.points["a"].y), (30.0, 10.0));
    let mut ends = [sk.points["c"].x, sk.points["d"].x];
    ends.sort_by(f64::total_cmp);
    assert_eq!(ends, [0.0, 30.0]);
    // A reference that cannot be found keeps its geometry and warns.
    fs.pop();
    fs.push(ref_sketch("ELLIPSE"));
    let b = r.regenerate(&fs, &CancelToken::new()).unwrap();
    assert_eq!(b.steps[4].status.warning.as_deref(), Some("A model reference of this sketch could not be found"));
}

/// Timing report (release: `cargo test --release -p oc-graph --test graph timings -- --ignored --nocapture`).
/// Cold rebuild, warm rebuild, and the share of cold time in nodes that need nothing from
/// the body chain (the only nodes another thread could take, since bodies are not `Send`).
#[test]
#[ignore]
fn timings() {
    let (mut cold_all, mut free_all) = (0.0, 0.0);
    for part in PARTS {
        let doc = load(part);
        let fs = features(&doc);
        let free = oc_graph::graph::body_free(&analyse(&fs));
        let mut r = regen();
        let a = rebuild(&mut r, &doc);
        let warm = std::time::Instant::now();
        rebuild(&mut r, &doc);
        let warm = warm.elapsed().as_secs_f64() * 1e3;
        let cold: f64 = a.steps.iter().map(|s| s.time.as_secs_f64() * 1e3).sum();
        let f: f64 = a.steps.iter().zip(&free).filter(|(_, f)| **f).map(|(s, _)| s.time.as_secs_f64() * 1e3).sum::<f64>() + 0.0;
        let slowest = a.steps.iter().zip(&fs).max_by(|x, y| x.0.time.cmp(&y.0.time)).map(|(s, f)| format!("{} {:.1} ms", f.name, s.time.as_secs_f64() * 1e3)).unwrap();
        eprintln!("{part:>18}: cold {cold:7.2} ms, warm {warm:6.3} ms, body-free nodes {f:6.3} ms; slowest {slowest}");
        cold_all += cold;
        free_all += f;
    }
    eprintln!("all: cold {cold_all:.1} ms, body-free {free_all:.3} ms ({:.3} %)", 100.0 * free_all / cold_all);
}
