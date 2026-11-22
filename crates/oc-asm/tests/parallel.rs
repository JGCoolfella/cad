//! Parts regenerated at once on worker threads (G3a) give exactly what regenerating them one
//! by one on the assembler's thread gives: the same build (placements, states, mates, heals),
//! the same meshes byte for byte, the same bodies (entities, order, geometry), names and
//! reference geometry. Sub-assemblies' parts join the same batch; lightweight parts resolve in
//! parallel too.
mod common;

use std::path::{Path, PathBuf};

use common::*;
use oc_asm::{scale, Assembler, Cache, Disk, Memory};
use oc_doc::container::Container;
use oc_doc::*;
use oc_kernel::Topology;
use oc_kernel_occt::OcctKernel;
use oc_naming::BodyNames;

fn scale_memory(parts: usize, edit: impl Fn(usize, &mut serde_json::Value)) -> Memory {
    let mut m = Memory::default();
    for k in 0..parts {
        let mut v: serde_json::Value = serde_json::from_str(&scale::part_json(k)).unwrap();
        edit(k, &mut v);
        m.0.insert(PathBuf::from(format!("/w/scale/{}", scale::part_file(k))), serde_json::to_vec(&v).unwrap());
    }
    m
}

fn with_workers(n: usize) -> Assembler<OcctKernel> {
    let mut a = assembler();
    a.workers = n;
    a
}

type PartState = (String, Option<Vec<u8>>, Option<(Topology, BodyNames, Vec<String>, Vec<(String, String)>)>);

/// Everything the assembler keeps per part, by hash.
fn parts_of(a: &Assembler<OcctKernel>) -> Vec<(String, PartState)> {
    let k = a.regen.kernel();
    let mut out: Vec<_> = a
        .parts
        .iter()
        .map(|(h, p)| {
            let live = p.live.as_ref().map(|l| {
                let shape = l.shape.as_ref().expect("a body");
                let mut geom: Vec<String> = l.planes.iter().chain(&l.axes).chain(&l.points).map(|(id, g)| format!("{id} {g:?}")).collect();
                geom.sort();
                (shape.topo(k).unwrap().clone(), shape.names.clone(), geom, l.errors.clone())
            });
            (h.clone(), (p.name.clone(), p.mesh.as_ref().map(|m| m.to_bytes()), live))
        })
        .collect();
    out.sort_by(|x, y| x.0.cmp(&y.0));
    out
}

#[test]
#[cfg_attr(target_family = "wasm", ignore = "worker threads: WebAssembly has none (W1 design point 1); the browser runs parts on host workers (W1b)")]
fn parallel_build_equals_serial_build() {
    let m = scale_memory(12, |_, _| {});
    let doc = scale::assembly(12, 48);
    let dir = Path::new("/w/scale");
    let mut serial = with_workers(1);
    let bs = serial.build(&doc, dir, &m, &opts(false));
    assert_eq!((serial.stats.regenerated, serial.stats.workers), (12, 1));
    for round in 0..3 {
        let mut par = with_workers(4);
        let bp = par.build(&doc, dir, &m, &opts(false));
        assert_eq!((par.stats.regenerated, par.stats.workers), (12, 4), "round {round}: twelve parts on four workers");
        assert_eq!(par.regenerations, serial.regenerations);
        assert!(bp == bs, "round {round}: the same build");
        assert!(parts_of(&par) == parts_of(&serial), "round {round}: the same parts");
        // Nothing changed: nothing regenerated, nothing sent to workers.
        let again = par.build(&doc, dir, &m, &opts(false));
        assert_eq!((par.stats.regenerated, par.stats.workers), (0, 0));
        assert!(again == bs);
    }
}

#[test]
#[cfg_attr(target_family = "wasm", ignore = "worker threads: WebAssembly has none (W1 design point 1); the browser runs parts on host workers (W1b)")]
fn corpus_assemblies_with_mates_build_the_same_in_parallel() {
    for name in ["hinge", "bracket-pins"] {
        let c = Container::from_bytes(&std::fs::read(corpus_dir().join(format!("{name}.ocasm"))).unwrap()).unwrap();
        let doc = load_assembly(c.document().unwrap()).unwrap().doc;
        let mut serial = with_workers(1);
        let bs = serial.build(&doc, &corpus_dir(), &Disk, &opts(false));
        let mut par = with_workers(0);
        let bp = par.build(&doc, &corpus_dir(), &Disk, &opts(false));
        assert!(par.stats.workers > 1, "{name}: {:?}", par.stats);
        assert!(bp == bs, "{name}: the same build, mates solved on transferred bodies");
        assert!(parts_of(&par) == parts_of(&serial), "{name}");
        // Picking geometry of a part regenerated on a worker.
        for h in par.parts.keys().cloned().collect::<Vec<_>>() {
            let faces = parts_of(&par).into_iter().find(|p| p.0 == h).unwrap().1 .2.unwrap().0.faces.len() as u32;
            for f in 0..faces {
                assert_eq!(par.entity(&h, "face", f, 3.0), serial.entity(&h, "face", f, 3.0), "{name}: face {f}");
            }
        }
    }
}

#[test]
#[cfg_attr(target_family = "wasm", ignore = "worker threads: WebAssembly has none (W1 design point 1); the browser runs parts on host workers (W1b)")]
fn sub_assembly_parts_join_the_same_batch() {
    // Parts 0-3 in a sub-assembly, parts 4-7 beside it in the top assembly.
    let mut m = scale_memory(8, |_, _| {});
    let sub = scale::assembly(4, 8);
    let mut c = Container::new();
    c.insert(oc_doc::container::DOCUMENT_ENTRY, save_assembly(&sub).unwrap().into_bytes());
    m.0.insert(PathBuf::from("/w/scale/sub.ocasm"), c.to_bytes());
    let mut top = AssemblyDocument::new("top", UnitSystem::Mmgs);
    top.components.push_back(Component { id: Id::new("s"), kind: ComponentKind::Assembly, path: "sub.ocasm".into(), abs_path: None, config: "Default".into() });
    top.instances.push_back(instance("is", "s", "sub<1>", true, Transform::IDENTITY));
    for k in 4..8 {
        top.components.push_back(component(&format!("c{k}"), &scale::part_file(k)));
        top.instances.push_back(instance(&format!("i{k}"), &format!("c{k}"), &format!("p{k}<1>"), true, place([0.0, 0.0, 1.0], 0.0, [0.0, 100.0 * k as f64, 0.0])));
    }
    let dir = Path::new("/w/scale");
    let mut serial = with_workers(1);
    let bs = serial.build(&top, dir, &m, &opts(false));
    let mut par = with_workers(8);
    let bp = par.build(&top, dir, &m, &opts(false));
    assert_eq!((par.stats.regenerated, par.stats.workers), (8, 8), "one batch of eight parts");
    assert_eq!(bp.occurrences.len(), 8 + 4);
    assert!(bp == bs);
    assert!(parts_of(&par) == parts_of(&serial));
}

#[test]
#[cfg_attr(target_family = "wasm", ignore = "worker threads: WebAssembly has none (W1 design point 1); the browser runs parts on host workers (W1b)")]
fn lightweight_parts_resolve_in_parallel() {
    let m = scale_memory(12, |_, _| {});
    let doc = scale::assembly(12, 24);
    let dir = Path::new("/w/scale");
    let mut serial = with_workers(1);
    let b = serial.build(&doc, dir, &m, &opts(false));
    let c = serial.container(&doc, &b, 3.0).unwrap();
    let mut light = with_workers(4);
    // Without the B-rep caches (G3c reads those instead: `outofcore.rs`).
    let mut cache = Cache::read(&c);
    cache.breps.clear();
    light.set_cache(cache);
    let lb = light.build(&doc, dir, &m, &opts(true));
    assert_eq!(light.stats.regenerated, 0);
    assert!(light.parts.values().all(|p| p.live.is_none()));
    let hashes: Vec<String> = lb.components.iter().filter_map(|c| c.part.clone()).collect();
    assert!(light.resolve_many(&hashes, 3.0));
    assert_eq!((light.stats.regenerated, light.stats.workers), (12, 4));
    assert!(parts_of(&light) == parts_of(&serial), "resolved parts equal regenerated ones");
}

/// Name, feature errors, has a body, has a mesh.
type PartErrors = (String, Vec<(String, String)>, bool, bool);

#[test]
#[cfg_attr(target_family = "wasm", ignore = "worker threads: WebAssembly has none (W1 design point 1); the browser runs parts on host workers (W1b)")]
fn failing_parts_report_the_same_errors() {
    // Every third part extrudes a sketch that does not exist.
    let m = scale_memory(9, |k, v| {
        if k % 3 == 0 {
            v["features"][1]["sketchId"] = "gone".into();
        }
    });
    let doc = scale::assembly(9, 9);
    let dir = Path::new("/w/scale");
    let mut serial = with_workers(1);
    let bs = serial.build(&doc, dir, &m, &opts(false));
    let mut par = with_workers(3);
    let bp = par.build(&doc, dir, &m, &opts(false));
    assert!(bp == bs);
    let errors = |a: &Assembler<OcctKernel>| {
        let mut e: Vec<PartErrors> = a.parts.values().map(|p| (p.name.clone(), p.live.as_ref().unwrap().errors.clone(), p.live.as_ref().unwrap().shape.is_some(), p.mesh.is_some())).collect();
        e.sort();
        e
    };
    let es = errors(&serial);
    assert_eq!(es.iter().filter(|e| !e.1.is_empty()).count(), 3, "{es:?}");
    assert!(es.iter().filter(|e| !e.1.is_empty()).all(|e| !e.2 && !e.3), "no body, no mesh");
    assert_eq!(errors(&par), es);
}

/// G3d (owner, G3a question 4): by default parts regenerate on half the logical processors,
/// at least one, leaving the rest to the UI and the renderer; an explicit count is kept.
#[test]
fn default_workers_are_half_the_logical_processors() {
    use oc_asm::parallel::{default_workers, worker_count};
    assert_eq!([1, 2, 3, 4, 8, 16, 17].map(default_workers), [1, 1, 1, 2, 4, 8, 8]);
    let logical = std::thread::available_parallelism().map_or(1, |n| n.get());
    assert_eq!(worker_count(0, usize::MAX), (logical / 2).max(1));
    assert_eq!(worker_count(0, 1), 1, "at most one per part");
    assert_eq!(worker_count(6, usize::MAX), 6, "an explicit count (OC_PART_WORKERS) is kept");
}
