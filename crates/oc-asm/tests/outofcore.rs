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

/// G3d (owner decision 3 on G3c): the caches name the engine build that wrote them, the app's
/// version and the kernel's name and version (not how it is linked: the installed app's
/// shared OCCT and a development build's static one make the same geometry).
#[test]
fn caches_name_the_engine_build_that_wrote_them() {
    let (_, _, a, c) = saved(2, 2);
    let e = Cache::read(&c).manifest.engine.expect("the engine is recorded");
    assert_eq!(e, a.engine());
    assert_eq!(e.app, oc_asm::cache::APP_VERSION);
    assert!(!e.app.is_empty());
    assert!(e.kernel.starts_with("OCCT ") && !e.kernel.contains('('), "{}", e.kernel);
}

/// G3d: a cache written by another engine build (another app version, another kernel, or
/// before caches named their engine) is ignored: its parts are regenerated, as for a file
/// without caches, and saving writes caches of this build.
#[test]
fn a_cache_written_by_another_engine_build_is_ignored_and_regenerated() {
    let (m, doc, mut serial, c) = saved(4, 8);
    let dir = Path::new("/w/scale");
    let want = serial.build(&doc, dir, &m, &opts(false));
    let edits: [(&str, &dyn Fn(&mut serde_json::Value)); 3] = [
        ("another app version", &|v| v["engine"]["app"] = "0.0.1".into()),
        ("another kernel version", &|v| v["engine"]["kernel"] = "OCCT 7.9.1".into()),
        ("no engine (saved before G3d)", &|v| {
            v.as_object_mut().unwrap().remove("engine");
        }),
    ];
    for (what, edit) in edits {
        let mut other = Container::from_bytes(&c.to_bytes()).unwrap();
        let mut manifest: serde_json::Value = serde_json::from_slice(other.get(oc_asm::cache::MANIFEST).unwrap()).unwrap();
        edit(&mut manifest);
        other.insert(oc_asm::cache::MANIFEST, serde_json::to_vec(&manifest).unwrap());
        for light in [true, false] {
            let mut a = assembler();
            a.workers = 1;
            a.set_cache(Cache::read(&other));
            let b = a.build(&doc, dir, &m, &opts(light));
            assert_eq!((a.regenerations, a.stats.read), (4, 0), "{what}, lightweight {light}: regenerated, nothing read");
            assert!(b == want, "{what}");
            assert!(live_parts(&a) == live_parts(&serial), "{what}");
            let saved = a.container(&doc, &b, 3.0).unwrap();
            assert_eq!(Cache::read(&saved).manifest.engine, Some(a.engine()), "{what}: saved with this build's caches");
        }
    }
}

/// G3d (owner decision 3): a resolved build of a freshly opened file reads its parts from
/// their B-rep caches instead of regenerating them, and gives what regenerating gives; a part
/// edited since the save (its hash differs) is regenerated.
#[test]
fn resolved_builds_of_a_freshly_opened_file_read_the_breps() {
    let (m, doc, mut serial, c) = saved(12, 24);
    let dir = Path::new("/w/scale");
    let want = serial.build(&doc, dir, &m, &opts(false));
    let c = Container::from_bytes(&c.to_bytes()).unwrap();
    let mut fresh = assembler();
    fresh.set_cache(Cache::read(&c));
    let b = fresh.build(&doc, dir, &m, &opts(false));
    assert_eq!((fresh.regenerations, fresh.stats.read, fresh.stats.regenerated), (0, 12, 0), "read, not regenerated");
    assert!(fresh.parts.values().all(|p| p.live.is_some()), "resolved");
    assert!(b == want);
    assert!(live_parts(&fresh) == live_parts(&serial), "the same bodies, names, geometry, errors and meshes");
    // One part edited since the save.
    let mut edited = scale_memory(12);
    let p3 = PathBuf::from(format!("/w/scale/{}", scale::part_file(3)));
    let mut v: serde_json::Value = serde_json::from_slice(&edited.0[&p3]).unwrap();
    v["features"][1]["end1"]["depth"] = 21.into();
    edited.0.insert(p3, serde_json::to_vec(&v).unwrap());
    let mut fresh = assembler();
    fresh.set_cache(Cache::read(&c));
    fresh.build(&doc, dir, &edited, &opts(false));
    assert_eq!((fresh.regenerations, fresh.stats.read), (1, 11));
    // The corpus assemblies, with mates.
    for name in ASSEMBLIES {
        let (doc, mut resolved, built) = corpus_assembly(name);
        let c = Container::from_bytes(&resolved.container(&doc, &built, 3.0).unwrap().to_bytes()).unwrap();
        let mut fresh = assembler();
        fresh.set_cache(Cache::read(&c));
        let b = fresh.build(&doc, &corpus_dir(), &Disk, &opts(false));
        assert_eq!(fresh.regenerations, 0, "{name}");
        assert_eq!(summary(&b), summary(&built), "{name}");
        for h in b.occurrences.iter().map(|o| o.part.clone()).collect::<std::collections::BTreeSet<_>>() {
            assert_eq!(fresh.entity(&h, "face", 0, 3.0), resolved.entity(&h, "face", 0, 3.0), "{name}");
        }
    }
}
