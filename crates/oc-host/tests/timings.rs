//! Timing report for geometry hosts (G3b):
//! `cargo test --release -p oc-host --test timings -- --ignored --nocapture`.
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Instant;

use oc_asm::{scale, Assembler, Memory, Options};
use oc_host::{HostCommand, Pool};
use oc_kernel_occt::OcctKernel;
use serde_json::json;

fn opts() -> Options {
    Options { lightweight: false, ..Options::default() }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// A 400 × 300 × 10 plate with `n` × `m` holes of Ø8 (E4's large plate, without fillets).
fn plate(n: usize, m: usize, depth: f64) -> String {
    let mut points = serde_json::Map::new();
    let mut entities = serde_json::Map::new();
    points.insert("origin".into(), json!({ "id": "origin", "x": 0, "y": 0 }));
    for (i, (x, y)) in [(0.0, 0.0), (400.0, 0.0), (400.0, 300.0), (0.0, 300.0)].iter().enumerate() {
        points.insert(format!("p{i}"), json!({ "id": format!("p{i}"), "x": x, "y": y }));
        entities.insert(format!("l{i}"), json!({ "id": format!("l{i}"), "type": "line", "p1": format!("p{i}"), "p2": format!("p{}", (i + 1) % 4) }));
    }
    for i in 0..n {
        for j in 0..m {
            let (c, k) = (format!("c{i}_{j}"), format!("k{i}_{j}"));
            points.insert(c.clone(), json!({ "id": c, "x": 400.0 * (i as f64 + 0.5) / n as f64, "y": 300.0 * (j as f64 + 0.5) / m as f64 }));
            entities.insert(k.clone(), json!({ "id": k, "type": "circle", "c": c, "r": 4.0 }));
        }
    }
    serde_json::to_string(&json!({
        "format": "opencad-part", "version": 1, "name": "plate", "units": "MMGS",
        "features": [
            { "id": "f1", "name": "Sketch1", "type": "sketch", "plane": { "kind": "std", "plane": "front" },
              "sketch": { "points": points, "entities": entities, "relations": {}, "dimensions": {}, "nextDim": 1 } },
            { "id": "f2", "name": "Boss-Extrude1", "type": "extrude", "cut": false, "sketchId": "f1", "reverse": false, "merge": true, "end1": { "type": "blind", "depth": depth } },
        ],
        "rollback": null,
    }))
    .unwrap()
}

#[test]
#[ignore = "timing report"]
fn host_timings() {
    let host = HostCommand::new(env!("CARGO_BIN_EXE_oc-host"));
    println!("threads available: {:?}", std::thread::available_parallelism());
    // Start-up: spawn to Hello, through a batch of one part each on fresh pools.
    let (files, doc) = scale::generate(100, 10_000);
    let mut m = Memory::default();
    for (name, text) in &files {
        m.0.insert(PathBuf::from(format!("/w/scale/{name}")), text.clone().into_bytes());
    }
    let dir = Path::new("/w/scale");
    // In-process threads (G3a).
    let mut t8 = Assembler::new(OcctKernel::default());
    let t = Instant::now();
    let _ = t8.build(&doc, dir, &m, &opts());
    println!("cold open on threads: {:.0} ms ({:?})", ms(t), t8.stats);
    // Hosts: the first batch starts them.
    let pool = Pool::new(host.clone(), 0);
    let stats = pool.stats();
    let mut a = Assembler::new(OcctKernel::default());
    a.runner = Some(Box::new(pool));
    // `OC_HOST_PRIME=1`: one small part per host first (as warming could do).
    if std::env::var("OC_HOST_PRIME").as_deref() == Ok("1") {
        use oc_asm::parallel::PartJob;
        let job = PartJob { hash: "p".into(), doc: std::sync::Arc::new(oc_doc::load(&scale::part_json(0)).unwrap().doc), quality: 3.0 };
        let n = std::thread::available_parallelism().map_or(1, |n| n.get());
        let t = Instant::now();
        let _ = a.runner.as_mut().unwrap().run(oc_naming::Rules::default(), &vec![job; n]);
        println!("priming {n} hosts: {:.0} ms", ms(t));
    }
    let t = Instant::now();
    let _ = a.build(&doc, dir, &m, &opts());
    println!("cold open on hosts, starting them: {:.0} ms ({:?}, {} started)", ms(t), a.stats, stats.started.load(Ordering::Relaxed));
    a.parts.clear();
    let t = Instant::now();
    let _ = a.build(&doc, dir, &m, &opts());
    println!("cold open on hosts already running: {:.0} ms ({:?})", ms(t), a.stats);
    // A large part, regenerated in the app and on a host (two at once: a batch needs two).
    let big = [plate(24, 18, 10.0), plate(24, 18, 12.0)];
    let mut pm = Memory::default();
    let mut asm = oc_doc::AssemblyDocument::new("plates", oc_doc::UnitSystem::Mmgs);
    for (k, text) in big.iter().enumerate() {
        pm.0.insert(PathBuf::from(format!("/w/p/plate{k}.ocpart")), text.clone().into_bytes());
        asm.components.push_back(oc_doc::Component { id: oc_doc::Id::from(format!("c{k}")), kind: oc_doc::ComponentKind::Part, path: format!("plate{k}.ocpart"), abs_path: None, config: "Default".into() });
        asm.instances.push_back(oc_doc::Instance { id: oc_doc::Id::from(format!("i{k}")), component: oc_doc::Id::from(format!("c{k}")), name: format!("plate<{k}>"), fixed: true, transform: oc_doc::Transform::IDENTITY, suppressed: None, hidden: None, color: None, display: None, transparent: None, config: None });
    }
    let pdir = Path::new("/w/p");
    let mut threads = Assembler::new(OcctKernel::default());
    let t = Instant::now();
    let _ = threads.build(&asm, pdir, &pm, &opts());
    println!("two 432-hole plates on threads: {:.0} ms ({:?})", ms(t), threads.stats);
    let t = Instant::now();
    let _ = a.build(&asm, pdir, &pm, &opts());
    println!("two 432-hole plates on hosts: {:.0} ms ({:?})", ms(t), a.stats);
    let mesh_mb: f64 = a.parts.values().filter_map(|p| p.mesh.as_ref()).map(|m| m.to_bytes().len() as f64 / 1048576.0).sum();
    let blob_mb: f64 = a.parts.values().filter_map(|p| p.live.as_ref()?.shape.as_ref()).map(|s| oc_kernel::Kernel::export_native(a.regen.kernel(), &s.body).unwrap().len() as f64 / 1048576.0).sum();
    println!("  meshes {mesh_mb:.1} MB, bodies {blob_mb:.1} MB crossed the pipes");
}

/// Start-up of one host: spawn to the answer for one small part, on a fresh pool. The host is
/// `OC_HOST_PROGRAM` (with `--geometry-host`; the app: `opencad.exe`) or this crate's binary.
#[test]
#[ignore = "timing report"]
fn host_start_up() {
    use oc_asm::parallel::{PartJob, Runner};
    let cmd = match std::env::var("OC_HOST_PROGRAM") {
        Ok(p) => HostCommand::new(p).arg("--geometry-host"),
        Err(_) => HostCommand::new(env!("CARGO_BIN_EXE_oc-host")),
    };
    let job = PartJob { hash: "p".into(), doc: std::sync::Arc::new(oc_doc::load(&scale::part_json(0)).unwrap().doc), quality: 3.0 };
    for n in [1usize, 8] {
        let jobs = vec![job.clone(); n];
        let mut pool = Pool::new(cmd.clone(), n);
        let t = Instant::now();
        let r = pool.run(oc_naming::Rules::default(), &jobs);
        let cold = ms(t);
        let t = Instant::now();
        let _ = pool.run(oc_naming::Rules::default(), &jobs);
        println!("{} hosts of {}: first part {cold:.0} ms (started), then {:.1} ms; ok {}", n, cmd.program.display(), ms(t), r.iter().all(|x| x.is_ok()));
    }
}

/// Memory of live bodies against their native blobs (G3c's budget unit): 12 large plates
/// regenerated in this process, bodies kept, meshes dropped.
#[cfg(windows)]
#[test]
#[ignore = "timing report"]
fn body_memory_against_blob_size() {
    #[allow(unsafe_code)]
    fn ws() -> f64 {
        use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: plain Win32 call filling a struct of the size given.
        unsafe {
            let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
            c.WorkingSetSize as f64 / 1048576.0
        }
    }
    let mut pm = Memory::default();
    let mut asm = oc_doc::AssemblyDocument::new("plates", oc_doc::UnitSystem::Mmgs);
    for k in 0..12 {
        pm.0.insert(PathBuf::from(format!("/w/p/plate{k}.ocpart")), plate(12 + k, 9, 10.0 + k as f64).into_bytes());
        asm.components.push_back(oc_doc::Component { id: oc_doc::Id::from(format!("c{k}")), kind: oc_doc::ComponentKind::Part, path: format!("plate{k}.ocpart"), abs_path: None, config: "Default".into() });
        asm.instances.push_back(oc_doc::Instance { id: oc_doc::Id::from(format!("i{k}")), component: oc_doc::Id::from(format!("c{k}")), name: format!("plate<{k}>"), fixed: true, transform: oc_doc::Transform::IDENTITY, suppressed: None, hidden: None, color: None, display: None, transparent: None, config: None });
    }
    let before = ws();
    let mut a = Assembler::new(OcctKernel::default());
    a.workers = 1;
    let _ = a.build(&asm, Path::new("/w/p"), &pm, &opts());
    for p in a.parts.values_mut() {
        p.mesh = None;
    }
    a.regen.clear_cache();
    let after = ws();
    let blobs: f64 = a.parts.values().filter_map(|p| p.live.as_ref()?.shape.as_ref()).map(|s| oc_kernel::Kernel::export_native(a.regen.kernel(), &s.body).unwrap().len() as f64 / 1048576.0).sum();
    for p in a.parts.values_mut() {
        p.live = None;
    }
    let dropped = ws();
    println!("12 plates: working set {before:.0} -> {after:.0} MB with bodies (meshes dropped), {dropped:.0} MB without; blobs {blobs:.1} MB: bodies take {:.1}x their blobs", (after - dropped) / blobs);
}
