//! The assembly corpus (`corpus/assemblies/`): each `.ocasm` is built here from the corpus
//! parts as the app would build it, and its solved state is recorded in `<name>.json`.
//!
//! Re-record after an intended change:   UPDATE_GOLDEN=1 cargo test -p oc-asm --test corpus
//! Never re-record to make a regression pass (plan section 5).
mod common;

use common::*;
use oc_asm::{Cache, Disk};
use oc_doc::container::Container;

/// What a B-rep entry holds: names, reference geometry, errors, and the body's topology.
fn brep_content(b: &[u8]) -> String {
    use oc_kernel::Kernel;
    let (blob, names, g) = oc_asm::cache::decode_brep(b).unwrap();
    let k = oc_kernel_occt::OcctKernel::default();
    let topo = k.topology(&k.import_native(blob).unwrap()).unwrap();
    let mut geom: Vec<String> = g.planes.iter().chain(&g.axes).chain(&g.points).map(|(id, x)| format!("{id} {x:?}")).collect();
    geom.sort();
    format!("{names:?} {geom:?} {:?} {topo:?}", g.errors)
}

/// WebAssembly (W1a): Emscripten's C library (musl) and the Windows runtime round `sin`, `cos`
/// and their kind differently in the last bit, in the mate solver and inside OCCT, so a file
/// recorded natively is rebuilt in WebAssembly with placements a few units in the last place
/// apart, and OCCT's mesher, from the same body, makes a triangulation with the same counts
/// per face and edge but other vertices (a circle's points start elsewhere). There, and only
/// there (section 8, W1a), the document's numbers are compared within `1e-12` (relative to
/// `max(1, |x|)`), and meshes by layout and geometry: the same model vertices (within `1e-5`
/// of the mesh's radius), flags, levels, vertex, index and segment counts, and face and edge
/// ranges, exactly; each face's triangle area and each edge's polyline length within `1e-6`
/// (relative to `max(1, value)`).
fn same_json(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value::*;
    match (a, b) {
        (Number(x), Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            (x - y).abs() <= 1e-12 * x.abs().max(1.0)
        }
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_json(x, y)),
        (Object(x), Object(y)) => x.len() == y.len() && x.iter().zip(y).all(|((kx, x), (ky, y))| kx == ky && same_json(x, y)),
        _ => a == b,
    }
}

fn same_mesh(x: &[u8], y: &[u8]) -> bool {
    use oc_mesh::{Lod, RenderMesh};
    let (Ok(a), Ok(b)) = (RenderMesh::from_bytes(x), RenderMesh::from_bytes(y)) else { return false };
    let tol = 1e-5 * a.radius.max(1.0);
    let near = |p: f64, q: f64| (p - q).abs() <= 1e-6 * p.abs().max(1.0);
    let sub = |p: [f32; 3], q: [f32; 3]| [0, 1, 2].map(|k| f64::from(p[k]) - f64::from(q[k]));
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let areas = |l: &Lod| -> Vec<f64> {
        l.faces
            .iter()
            .map(|f| {
                let t0 = f.first_index as usize / 3;
                (t0..t0 + f.index_count as usize / 3)
                    .map(|t| {
                        let [p, q, r] = l.triangle(t).map(|i| l.vertex(i as usize).position);
                        let (u, v) = (sub(q, p), sub(r, p));
                        len([u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]) / 2.0
                    })
                    .sum()
            })
            .collect()
    };
    let lengths = |l: &Lod| -> Vec<f64> {
        l.edges.iter().map(|e| (e.first_segment..e.first_segment + e.segment_count).map(|s| { let g = l.segment(s as usize); len(sub(g.b, g.a)) }).sum()).collect()
    };
    a.points.len() == b.points.len()
        && a.points.iter().zip(&b.points).all(|(p, q)| p.iter().zip(q).all(|(p, q)| (p - q).abs() <= tol))
        && a.point_flags == b.point_flags
        && a.edge_flags == b.edge_flags
        && a.lods.len() == b.lods.len()
        && a.lods.iter().zip(&b.lods).all(|(p, q)| {
            (p.vertex_count, p.index_count, p.segment_count) == (q.vertex_count, q.index_count, q.segment_count)
                && p.faces == q.faces
                && p.edges == q.edges
                && areas(p).iter().zip(areas(q)).all(|(x, y)| near(*x, y))
                && lengths(p).iter().zip(lengths(q)).all(|(x, y)| near(*x, y))
        })
}

fn update() -> bool {
    std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1")
}

#[test]
fn corpus_assemblies() {
    let dir = corpus_dir();
    for name in ASSEMBLIES {
        let (doc, a, built) = corpus_assembly(name);
        let file = dir.join(format!("{name}.ocasm"));
        let oracle = dir.join(format!("{name}.json"));
        let bytes = a.container(&doc, &built, 3.0).unwrap().to_bytes();
        if update() {
            std::fs::write(&file, &bytes).unwrap();
            std::fs::write(&oracle, serde_json::to_string_pretty(&summary(&built)).unwrap() + "\n").unwrap();
        }
        // The stored file is what building it now gives: the document and the meshes byte for
        // byte; the B-rep entries (G3c) by what they hold, since the kernel's native blob is not
        // canonical (owner decision 2026-11-20: saves may differ in those entries).
        let stored = std::fs::read(&file).unwrap_or_else(|_| panic!("{} is missing; record it with UPDATE_GOLDEN=1", file.display()));
        let c = Container::from_bytes(&stored).unwrap();
        let fresh = Container::from_bytes(&bytes).unwrap();
        // The stored file's caches are this engine build's (G3d): re-record after a version
        // bump of the app or the kernel.
        assert!(a.accepts(&Cache::read(&c)), "{name}: its caches were written by another engine build; re-record with UPDATE_GOLDEN=1");
        if cfg!(target_family = "wasm") {
            let (x, y): (serde_json::Value, serde_json::Value) = (serde_json::from_str(c.document().unwrap()).unwrap(), serde_json::from_str(fresh.document().unwrap()).unwrap());
            assert!(same_json(&x, &y), "{name}: document.json");
        } else {
            assert_eq!(c.document().unwrap(), fresh.document().unwrap(), "{name}: document.json");
        }
        assert_eq!(c.names().collect::<Vec<_>>(), fresh.names().collect::<Vec<_>>(), "{name}: entries");
        for entry in c.names() {
            let (x, y) = (c.get(entry).unwrap(), fresh.get(entry).unwrap());
            if entry.starts_with("cache/brep/") {
                assert_eq!(brep_content(x), brep_content(y), "{name}: {entry}");
            } else {
                assert!(x == y || (cfg!(target_family = "wasm") && (entry == "document.json" || (entry.starts_with("cache/mesh/") && same_mesh(x, y)))), "{name}: {entry}");
            }
        }
        assert_eq!(c.names().filter(|n| n.starts_with("cache/brep/")).count(), c.names().filter(|n| n.starts_with("cache/mesh/")).count(), "{name}: a B-rep per mesh");
        let want: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&oracle).unwrap()).unwrap();
        assert_eq!(summary(&built), want, "{name}");

        // Opening the stored file: resolved (its parts read from their B-rep caches, G3d), and
        // lightweight from its caches.
        let doc = oc_doc::load_assembly(c.document().unwrap()).unwrap().doc;
        for light in [false, true] {
            let mut a = assembler();
            a.set_cache(Cache::read(&c));
            let b = a.build(&doc, &dir, &Disk, &opts(light));
            assert_eq!(summary(&b), want, "{name}, lightweight {light}");
            assert!(b.heals.is_empty(), "{name}: nothing to heal");
            assert_eq!(a.regenerations, 0, "{name}, lightweight {light}");
            assert_eq!(a.stats.read, if light { 0 } else { want["parts"].as_u64().unwrap() as usize }, "{name}, lightweight {light}");
        }
    }
}

#[test]
fn corpus_expectations() {
    let dir = corpus_dir();
    let read = |n: &str| -> serde_json::Value { serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{n}.json"))).unwrap()).unwrap() };
    // Two pins, each turning in its hole.
    let b = read("bracket-pins");
    assert_eq!(b["dof"], 2);
    assert_eq!(b["instances"]["i1"]["state"], "Fixed");
    for (i, x) in [("i2", 15.0), ("i3", 45.0)] {
        assert_eq!(b["instances"][i]["state"], "Under");
        assert_eq!(b["instances"][i]["free"], 1);
        assert_eq!(b["instances"][i]["position"], serde_json::json!([x, 15.0, 8.0]), "{i} sits on the plate in its hole");
    }
    assert_eq!(b["parts"], 2, "the two pins share one part");
    assert!(b["mates"].as_object().unwrap().values().all(|v| v == "Ok"));
    // The hinge: the arm turns about the plate's hole, the pin turns in the arm.
    let h = read("hinge");
    assert_eq!(h["dof"], 2);
    assert_eq!(h["instances"]["i2"]["free"], 1);
    assert_eq!(h["instances"]["i3"]["free"], 1);
    assert_eq!(h["instances"]["i3"]["position"], serde_json::json!([15.0, 15.0, 14.0]), "pin on the arm, in line with the plate's hole");
    assert!(h["mates"].as_object().unwrap().values().all(|v| v == "Ok"));
}

