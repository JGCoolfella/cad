//! Out-of-core parts (G3c): B-rep caches in the `.ocasm` resolve lightweight parts without
//! regenerating them (also when the part's file is missing), a cache entry that does not read
//! is regenerated instead, and a body budget evicts the least recently used bodies, which are
//! read back from their B-rep entries when needed.
mod common;

use std::path::{Path, PathBuf};

use common::*;
use oc_asm::{scale, Assembler, Cache, Disk, Memory};
use oc_doc::container::Container;
use oc_kernel::Topology;
use oc_kernel_occt::OcctKernel;
use oc_naming::BodyNames;

fn scale_memory(parts: usize) -> Memory {
    let mut m = Memory::default();
    for k in 0..parts {
        m.0.insert(PathBuf::from(format!("/w/scale/{}", scale::part_file(k))), scale::part_json(k).into_bytes());
    }
    m
}

type PartState = (String, Option<Vec<u8>>, Option<(Topology, BodyNames, Vec<String>, Vec<(String, String)>)>);

/// Everything kept per part with a live body, by hash.
fn live_parts(a: &Assembler<OcctKernel>) -> Vec<(String, PartState)> {
    let k = a.regen.kernel();
    let mut out: Vec<_> = a
        .parts
        .iter()
        .filter(|(_, p)| p.live.is_some())
        .map(|(h, p)| {
            let l = p.live.as_ref().unwrap();
            let shape = l.shape.as_ref().unwrap();
            let mut geom: Vec<String> = l.planes.iter().chain(&l.axes).chain(&l.points).map(|(id, g)| format!("{id} {g:?}")).collect();
            geom.sort();
            (h.clone(), (p.name.clone(), p.mesh.as_ref().map(|m| m.to_bytes()), Some((shape.topo(k).unwrap().clone(), shape.names.clone(), geom, l.errors.clone()))))
        })
        .collect();
    out.sort_by(|x, y| x.0.cmp(&y.0));
    out
}

/// The scale assembly built from its parts, and its saved container.
fn saved(parts: usize, instances: usize) -> (Memory, oc_doc::AssemblyDocument, Assembler<OcctKernel>, Container) {
    let m = scale_memory(parts);
    let doc = scale::assembly(parts, instances);
    let mut a = assembler();
    a.workers = 1;
    let b = a.build(&doc, Path::new("/w/scale"), &m, &opts(false));
    let c = a.container(&doc, &b, 3.0).unwrap();
    (m, doc, a, c)
}

#[test]
fn brep_caches_resolve_without_regenerating() {
    let (m, doc, serial, c) = saved(12, 24);
    let c = Container::from_bytes(&c.to_bytes()).unwrap();
    assert_eq!(c.names().filter(|n| n.starts_with("cache/brep/")).count(), 12, "a B-rep per part");
    let mut light = assembler();
    light.set_cache(Cache::read(&c));
    let b = light.build(&doc, Path::new("/w/scale"), &m, &opts(true));
    assert_eq!(light.regenerations, 0);
    let hashes: Vec<String> = b.components.iter().filter_map(|c| c.part.clone()).collect();
    assert!(light.resolve_many(&hashes, 3.0));
    assert_eq!((light.stats.read, light.stats.regenerated, light.regenerations), (12, 0, 0), "read, not regenerated");
    assert!(live_parts(&light) == live_parts(&serial), "the same bodies, names, geometry, errors and meshes");
}

#[test]
fn picks_on_a_lightweight_part_read_its_brep() {
    for name in ["hinge", "bracket-pins"] {
        let (doc, mut resolved, built) = corpus_assembly(name);
        let c = Container::from_bytes(&resolved.container(&doc, &built, 3.0).unwrap().to_bytes()).unwrap();
        let mut light = assembler();
        light.set_cache(Cache::read(&c));
        let b = light.build(&doc, &corpus_dir(), &Disk, &opts(true));
        assert_eq!(light.regenerations, 0);
        for h in b.occurrences.iter().map(|o| o.part.clone()).collect::<std::collections::BTreeSet<_>>() {
            for f in 0..6 {
                assert_eq!(light.entity(&h, "face", f, 3.0), resolved.entity(&h, "face", f, 3.0), "{name}: face {f}");
            }
        }
        assert_eq!(light.regenerations, 0, "{name}: read from the B-reps");
        // Interference Detection on the bodies read back.
        let x = light.interferences(&b, &oc_asm::InterferenceOptions::default(), 3.0).unwrap();
        let y = resolved.interferences(&built, &oc_asm::InterferenceOptions::default(), 3.0).unwrap();
        assert_eq!(x.interferences.len(), y.interferences.len(), "{name}");
        assert_eq!(light.regenerations, 0, "{name}");
    }
}

#[test]
fn a_missing_part_file_still_resolves_from_its_brep() {
    let (mut m, doc, mut serial, c) = saved(4, 4);
    let gone = format!("/w/scale/{}", scale::part_file(2));
    m.0.remove(&PathBuf::from(&gone));
    let mut light = assembler();
    light.set_cache(Cache::read(&c));
    let b = light.build(&doc, Path::new("/w/scale"), &m, &opts(true));
    let comp = &b.components[2];
    assert!(comp.warning.as_deref().is_some_and(|w| w.contains("shown from the assembly's cache")), "{comp:?}");
    let h = comp.part.clone().unwrap();
    assert!(light.resolve(&h, 3.0), "resolved from the B-rep without the file");
    assert_eq!(light.entity(&h, "face", 0, 3.0), serial.entity(&h, "face", 0, 3.0));
    assert_eq!(light.regenerations, 0);
}

#[test]
fn a_brep_that_does_not_read_is_regenerated() {
    let (m, doc, serial, c) = saved(3, 3);
    let mut cache = Cache::read(&c);
    for b in cache.breps.values_mut() {
        let mut v = (**b).clone();
        let n = v.len();
        v.truncate(n - 10);
        *b = std::sync::Arc::new(v);
    }
    let mut light = assembler();
    light.workers = 1;
    light.set_cache(cache);
    let b = light.build(&doc, Path::new("/w/scale"), &m, &opts(true));
    let hashes: Vec<String> = b.components.iter().filter_map(|c| c.part.clone()).collect();
    assert!(light.resolve_many(&hashes, 3.0));
    assert_eq!((light.stats.read, light.regenerations), (0, 3), "regenerated instead");
    assert!(light.parts.values().all(|p| p.brep.is_none()), "the damaged entries are dropped");
    assert!(live_parts(&light) == live_parts(&serial));
}

fn live(a: &Assembler<OcctKernel>) -> usize {
    a.parts.values().filter(|p| p.live.is_some()).count()
}

#[test]
fn bodies_beyond_the_budget_are_evicted_and_read_back() {
    let m = scale_memory(12);
    let doc = scale::assembly(12, 24);
    let dir = Path::new("/w/scale");
    let mut serial = assembler();
    serial.workers = 1;
    let bs = serial.build(&doc, dir, &m, &opts(false));
    // A budget of about three bodies.
    let one = serial.brep_of(&bs.components[0].part.clone().unwrap()).unwrap().len() * oc_asm::outofcore::BODY_PER_BLOB_BYTE;
    let mut a = assembler();
    a.body_budget = Some(one * 3);
    let b = a.build(&doc, dir, &m, &opts(false));
    assert!(b == bs);
    assert!(a.stats.evicted >= 8, "{:?}", a.stats);
    assert!((1..=4).contains(&live(&a)), "{} live", live(&a));
    assert!(a.body_memory() <= one * 4, "{} > {}", a.body_memory(), one * 3);
    assert_eq!(a.regenerations, 12);
    // A pick on an evicted part reads it back; nothing is regenerated.
    let evicted: Vec<String> = a.parts.iter().filter(|(_, p)| p.live.is_none()).map(|(h, _)| h.clone()).collect();
    for h in &evicted {
        assert_eq!(a.entity(h, "face", 1, 3.0), serial.entity(h, "face", 1, 3.0));
    }
    assert_eq!(a.regenerations, 12);
    assert!((1..=4).contains(&live(&a)), "still within the budget: {} live", live(&a));
    // Rebuilds: lightweight keeps evicted parts as they are; a resolved build reads them back.
    let again = a.build(&doc, dir, &m, &opts(true));
    assert!(again == bs);
    assert_eq!((a.stats.regenerated, a.stats.read), (0, 0));
    let again = a.build(&doc, dir, &m, &opts(false));
    assert!(again == bs);
    assert_eq!(a.stats.regenerated, 0);
    assert!(a.stats.read >= 8, "{:?}", a.stats);
    assert_eq!(a.regenerations, 12);
    // Every part, read back, is what was regenerated.
    let all: Vec<String> = a.parts.keys().cloned().collect();
    a.body_budget = None;
    assert!(a.resolve_many(&all, 3.0));
    assert!(live_parts(&a) == live_parts(&serial));
}
