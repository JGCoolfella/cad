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
        assert_eq!(c.document().unwrap(), fresh.document().unwrap(), "{name}: document.json");
        assert_eq!(c.names().collect::<Vec<_>>(), fresh.names().collect::<Vec<_>>(), "{name}: entries");
        for entry in c.names() {
            let (x, y) = (c.get(entry).unwrap(), fresh.get(entry).unwrap());
            if entry.starts_with("cache/brep/") {
                assert_eq!(brep_content(x), brep_content(y), "{name}: {entry}");
            } else {
                assert!(x == y, "{name}: {entry}");
            }
        }
        assert_eq!(c.names().filter(|n| n.starts_with("cache/brep/")).count(), c.names().filter(|n| n.starts_with("cache/mesh/")).count(), "{name}: a B-rep per mesh");
        let want: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&oracle).unwrap()).unwrap();
        assert_eq!(summary(&built), want, "{name}");

        // Opening the stored file: resolved, and lightweight from its caches.
        let doc = oc_doc::load_assembly(c.document().unwrap()).unwrap().doc;
        for light in [false, true] {
            let mut a = assembler();
            a.set_cache(Cache::read(&c));
            let b = a.build(&doc, &dir, &Disk, &opts(light));
            assert_eq!(summary(&b), want, "{name}, lightweight {light}");
            assert!(b.heals.is_empty(), "{name}: nothing to heal");
            assert_eq!(a.regenerations, if light { 0 } else { want["parts"].as_u64().unwrap() as usize }, "{name}, lightweight {light}");
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

