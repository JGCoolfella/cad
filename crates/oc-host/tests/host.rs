//! Geometry hosts (G3b): parts regenerated in child processes give what regenerating them in
//! the app gives; a host that crashes, aborts, exits or hangs fails only the part it was
//! regenerating, with an error on that part, and is started again; the app (this test
//! process) survives. The host program is this crate's `oc-host` binary; the app runs itself
//! with `--geometry-host`.
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use oc_asm::{scale, Assembler, Disk, Memory, Options};
use oc_doc::container::Container;
use oc_host::{pool, HostCommand, Pool};
use oc_kernel::Topology;
use oc_kernel_occt::OcctKernel;
use oc_naming::BodyNames;

fn host() -> HostCommand {
    HostCommand::new(env!("CARGO_BIN_EXE_oc-host"))
}

fn opts() -> Options {
    Options { lightweight: false, ..Options::default() }
}

fn serial() -> Assembler<OcctKernel> {
    let mut a = Assembler::new(OcctKernel::default());
    a.workers = 1;
    a
}

fn on(pool: Pool) -> Assembler<OcctKernel> {
    let mut a = Assembler::new(OcctKernel::default());
    a.runner = Some(Box::new(pool));
    a
}

fn scale_memory(parts: usize, edit: impl Fn(usize, &mut serde_json::Value)) -> Memory {
    let mut m = Memory::default();
    for k in 0..parts {
        let mut v: serde_json::Value = serde_json::from_str(&scale::part_json(k)).unwrap();
        edit(k, &mut v);
        m.0.insert(PathBuf::from(format!("/w/scale/{}", scale::part_file(k))), serde_json::to_vec(&v).unwrap());
    }
    m
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
                let topo = l.shape.as_ref().map(|s| s.topo(k).unwrap().clone()).unwrap_or_default();
                let names = l.shape.as_ref().map(|s| s.names.clone()).unwrap_or_default();
                let mut geom: Vec<String> = l.planes.iter().chain(&l.axes).chain(&l.points).map(|(id, g)| format!("{id} {g:?}")).collect();
                geom.sort();
                (topo, names, geom, l.errors.clone())
            });
            (h.clone(), (p.name.clone(), p.mesh.as_ref().map(|m| m.to_bytes()), live))
        })
        .collect();
    out.sort_by(|x, y| x.0.cmp(&y.0));
    out
}

#[test]
fn hosts_build_what_the_app_builds() {
    let m = scale_memory(12, |_, _| {});
    let doc = scale::assembly(12, 48);
    let dir = Path::new("/w/scale");
    let mut s = serial();
    let bs = s.build(&doc, dir, &m, &opts());
    let pool = Pool::new(host(), 4);
    let stats = pool.stats();
    let mut a = on(pool);
    let b = a.build(&doc, dir, &m, &opts());
    assert_eq!((a.stats.regenerated, a.stats.workers), (12, 4));
    assert!(b == bs, "the same build");
    assert!(parts_of(&a) == parts_of(&s), "the same parts: bodies, names, geometry, meshes");
    // The hosts are kept: a second batch starts none.
    let m2 = scale_memory(12, |_, v| v["features"][1]["end1"]["depth"] = 20.into());
    let _ = a.build(&doc, dir, &m2, &opts());
    assert_eq!(a.stats.regenerated, 12);
    assert_eq!(stats.started.load(Ordering::Relaxed), 4);
}

#[test]
fn corpus_assemblies_with_mates_build_the_same_on_hosts() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/assemblies");
    for name in ["hinge", "bracket-pins"] {
        let c = Container::from_bytes(&std::fs::read(dir.join(format!("{name}.ocasm"))).unwrap()).unwrap();
        let doc = oc_doc::load_assembly(c.document().unwrap()).unwrap().doc;
        let mut s = serial();
        let bs = s.build(&doc, &dir, &Disk, &opts());
        let mut a = on(Pool::new(host(), 0));
        let b = a.build(&doc, &dir, &Disk, &opts());
        assert!(a.stats.workers > 1);
        assert!(b == bs, "{name}");
        assert!(parts_of(&a) == parts_of(&s), "{name}");
    }
}

/// Six parts on two hosts, part 2 named `fault:<kind>`: part 2 fails with `message`, the
/// others are as the app builds them, the host is started again, and the next build with the
/// part repaired regenerates it.
fn fault(kind: &str, timeout: Option<Duration>) -> (String, usize, usize) {
    let bad = scale_memory(6, |k, v| {
        if k == 2 {
            v["name"] = format!("fault:{kind}").into();
        }
    });
    let good = scale_memory(6, |_, _| {});
    let doc = scale::assembly(6, 12);
    let dir = Path::new("/w/scale");
    let mut pool = Pool::new(host().env(oc_host::serve::FAULTS, "1"), 2);
    if let Some(t) = timeout {
        pool.timeout = t;
    }
    let stats = pool.stats();
    let mut a = on(pool);
    let b = a.build(&doc, dir, &bad, &opts());
    assert_eq!(b.occurrences.len(), 12, "{kind}: the assembly is built");
    let mut s = serial();
    let _ = s.build(&doc, dir, &good, &opts());
    let mine = parts_of(&a);
    let theirs = parts_of(&s);
    let failed: Vec<_> = mine.iter().filter(|(h, _)| !theirs.iter().any(|(g, _)| g == h)).collect();
    assert_eq!(failed.len(), 1, "{kind}: only the faulty part differs");
    let (_, (name, mesh, live)) = failed[0];
    assert_eq!(name, &format!("fault:{kind}"));
    assert!(mesh.is_none(), "{kind}: no mesh");
    let (topo, _, _, errors) = live.as_ref().unwrap();
    assert!(topo.faces.is_empty(), "{kind}: no body");
    let message = errors[0].1.clone();
    for (h, p) in &mine {
        if let Some((_, q)) = theirs.iter().find(|(g, _)| g == h) {
            assert!(p == q, "{kind}: the other parts are as the app builds them");
        }
    }
    // The next batch runs on hosts again (the dead one started anew).
    let deeper = scale_memory(6, |_, v| v["features"][1]["end1"]["depth"] = 20.into());
    let b2 = a.build(&doc, dir, &deeper, &opts());
    assert_eq!((a.stats.regenerated, a.stats.workers), (6, 2), "{kind}");
    // Each failed host was started again once (when its slot took its next part, in this
    // batch or the next).
    let failures = stats.died.load(Ordering::Relaxed) + stats.timed_out.load(Ordering::Relaxed);
    assert_eq!(stats.started.load(Ordering::Relaxed), 2 + failures, "{kind}: the failed host started again");
    let mut s2 = serial();
    let bs2 = s2.build(&doc, dir, &deeper, &opts());
    assert!(b2 == bs2, "{kind}");
    let theirs2 = parts_of(&s2);
    assert!(parts_of(&a).iter().filter(|(h, _)| theirs2.iter().any(|(g, _)| g == h)).count() == 6);
    for (h, p) in parts_of(&a) {
        if let Some((_, q)) = theirs2.iter().find(|(g, _)| *g == h) {
            assert!(&p == q, "{kind}: after the restart");
        }
    }
    (message, stats.died.load(Ordering::Relaxed), stats.timed_out.load(Ordering::Relaxed))
}

#[test]
fn a_host_that_crashes_fails_only_its_part() {
    for kind in ["crash", "abort", "exit"] {
        let (message, died, timed_out) = fault(kind, None);
        assert_eq!(message, pool::DIED, "{kind}");
        assert_eq!((died, timed_out), (1, 0), "{kind}");
    }
}

#[test]
fn a_host_that_hangs_is_stopped() {
    let (message, died, timed_out) = fault("hang", Some(Duration::from_secs(3)));
    assert!(message.starts_with("The rebuild of this part did not finish within"), "{message}");
    assert_eq!((died, timed_out), (0, 1));
}

#[test]
fn without_a_host_program_the_app_regenerates_the_parts_itself() {
    let m = scale_memory(4, |_, _| {});
    let doc = scale::assembly(4, 8);
    let dir = Path::new("/w/scale");
    let pool = Pool::new(HostCommand::new("Z:/nowhere/no-such-host.exe"), 2);
    let stats = pool.stats();
    let mut a = on(pool);
    let b = a.build(&doc, dir, &m, &opts());
    let mut s = serial();
    let bs = s.build(&doc, dir, &m, &opts());
    assert!(b == bs);
    assert!(parts_of(&a) == parts_of(&s));
    assert!(stats.failed_to_start.load(Ordering::Relaxed) >= 1);
    assert_eq!(a.regenerations, 4);
}

/// Hosts unused for longer than the pool's keep-alive are stopped when the app is idle, and
/// started again by the next batch.
#[test]
fn idle_hosts_stop_and_start_again() {
    use oc_asm::parallel::{PartJob, Runner};
    let jobs: Vec<PartJob> = (0..4)
        .map(|k| {
            let doc = oc_doc::load(&scale::part_json(k)).unwrap().doc;
            PartJob { hash: format!("p{k}"), doc: std::sync::Arc::new(doc), quality: 3.0 }
        })
        .collect();
    let mut pool = Pool::new(host(), 2);
    let stats = pool.stats();
    let first = pool.run(oc_naming::Rules::default(), &jobs);
    assert!(first.iter().all(|r| r.as_ref().is_ok_and(|o| o.body.is_some() && o.geometry.errors.is_empty())));
    assert_eq!(pool.running(), 2);
    pool.idle();
    assert_eq!(pool.running(), 2, "used just now: kept");
    pool.keep = Duration::ZERO;
    pool.idle();
    assert_eq!(pool.running(), 0, "stopped");
    let again = pool.run(oc_naming::Rules::default(), &jobs);
    assert_eq!(pool.running(), 2);
    assert_eq!(stats.started.load(Ordering::Relaxed), 4);
    assert_eq!(stats.died.load(Ordering::Relaxed), 0, "stopping is not dying");
    for (a, b) in first.iter().zip(&again) {
        let (a, b) = (a.as_ref().unwrap(), b.as_ref().unwrap());
        assert_eq!(a.mesh.as_ref().map(|m| m.to_bytes()), b.mesh.as_ref().map(|m| m.to_bytes()));
        assert_eq!(a.body.as_ref().map(|x| &x.1), b.body.as_ref().map(|x| &x.1));
    }
}
