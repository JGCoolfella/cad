//! Native body transfer (G3a, `Kernel::export_native` / `import_native`): a body built by one
//! kernel instance (another thread, later another geometry host) is the same body in
//! another: the same entities in the same order, the same geometry, the same meshes, and
//! operations on it give the same results. Kernels on several threads at once give the same
//! bodies as one thread alone.
mod common;

use common::*;
use oc_kernel::*;
use oc_kernel_occt::{Body, OcctKernel};

/// Bodies from every operation family: (name, body).
fn bodies(k: &OcctKernel) -> Vec<(&'static str, Body)> {
    let mut out = vec![];
    let plate = prism(k, Region::new(rect(0.0, 0.0, 100.0, 60.0, 0.0)), [0.0, 0.0, 5.0]).body;
    out.push(("prism", plate.clone()));
    let boss = prism(k, Region::new(circle(10.0, 10.0, 4.0, 5.0)), [0.0, 0.0, 6.0]).body;
    let copies: Vec<Body> = (0..6).map(|i| k.transform(&boss, &Transform::translation([14.0 * i as f64, 20.0 * (i % 3) as f64, 0.0])).unwrap().body).collect();
    let tools: Vec<&Body> = copies.iter().collect();
    let b = k.boolean(BooleanOp::Fuse, &plate, &tools).unwrap().body;
    let hole = prism(k, Region::new(circle(50.0, 45.0, 6.0, -1.0)), [0.0, 0.0, 20.0]).body;
    let b = k.boolean(BooleanOp::Cut, &b, &[&hole]).unwrap().body;
    out.push(("cut", b.clone()));
    let t = k.topology(&b).unwrap();
    let rims: Vec<FilletEdge> = edges_where(&t, false, |m| near(m[2], 11.0)).into_iter().map(|edge| FilletEdge { edge, radius: Radius::Constant(1.0) }).collect();
    let b = k.fillet(&b, &rims).unwrap().body;
    out.push(("fillet", b.clone()));
    let bottom = face_where(&k.topology(&b).unwrap(), planar([0.0, 0.0, -1.0], 2, 0.0));
    let b = k.shell(&b, &ShellSpec { open: vec![bottom], thickness: 0.5, outward: false }).unwrap().body;
    out.push(("shell", b.clone()));
    out.push(("mirror", k.transform(&b, &Transform::mirror(Plane { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] })).unwrap().body));
    out.push(("sphere", k.primitive(&Primitive::Sphere { center: [1.0, 2.0, 3.0], radius: 7.0 }).unwrap().body));
    let far = block(k, 200.0, 10.0, 10.0);
    out.push(("two solids", k.compound(&[&plate, &far]).unwrap().body));
    out
}

/// Positions, triangles, face ranges, edge polylines.
type MeshParts = (Vec<P3>, Vec<[u32; 3]>, Vec<std::ops::Range<u32>>, Vec<Vec<P3>>);

fn mesh_of(k: &OcctKernel, b: &Body) -> MeshParts {
    let m = k.tessellate(b, &MeshParams { linear: 0.05, angular: 0.3 }).unwrap();
    (m.positions, m.triangles, m.faces, m.edges)
}

#[test]
fn a_transferred_body_is_the_same_body() {
    let k = kernel();
    for (name, b) in bodies(&k) {
        let blob = k.export_native(&b).unwrap();
        // Another kernel instance on another thread reads it.
        let (topo, mass, mesh, check, again) = std::thread::scope(|s| {
            s.spawn(|| {
                let k2 = kernel();
                let c = k2.import_native(&blob).unwrap();
                (k2.topology(&c).unwrap(), k2.mass_properties(&c).unwrap(), mesh_of(&k2, &c), k2.check(&c).unwrap(), k2.export_native(&c).unwrap())
            })
            .join()
            .unwrap()
        });
        assert_eq!(topo, k.topology(&b).unwrap(), "{name}: entities, order and geometry");
        assert_eq!(mass, k.mass_properties(&b).unwrap(), "{name}: mass properties");
        assert_eq!(mesh, mesh_of(&k, &b), "{name}: mesh");
        assert!(check.valid, "{name}: {check:?}");
        // The bytes are not canonical (a negative zero reads back as zero; shared locations
        // are written in memory order), but a second round trip is the same body again.
        assert_eq!(k.topology(&k.import_native(&again).unwrap()).unwrap(), topo, "{name}: a second round trip");
    }
}

#[test]
fn operations_on_a_transferred_body_give_the_same_results() {
    let k = kernel();
    let (_, b) = bodies(&k).into_iter().find(|(n, _)| *n == "cut").unwrap();
    let c = k.import_native(&k.export_native(&b).unwrap()).unwrap();
    let t = k.topology(&b).unwrap();
    let rims: Vec<FilletEdge> = edges_where(&t, false, |m| near(m[2], 11.0)).into_iter().map(|edge| FilletEdge { edge, radius: Radius::Constant(1.0) }).collect();
    let x = k.fillet(&b, &rims).unwrap();
    let y = k.fillet(&c, &rims).unwrap();
    assert_eq!(x.history, y.history);
    assert_eq!(k.topology(&x.body).unwrap(), k.topology(&y.body).unwrap());
    let tool = block(&k, 30.0, 30.0, 30.0);
    let x = k.boolean(BooleanOp::Cut, &b, &[&tool]).unwrap();
    let y = k.boolean(BooleanOp::Cut, &c, &[&tool]).unwrap();
    assert_eq!(x.history, y.history);
    assert_eq!(k.topology(&x.body).unwrap(), k.topology(&y.body).unwrap());
}

#[test]
fn bad_data_is_an_error_not_a_crash() {
    let k = kernel();
    let blob = k.export_native(&block(&k, 10.0, 10.0, 10.0)).unwrap();
    assert!(k.import_native(&[]).is_err());
    assert!(k.import_native(b"not a body at all").is_err());
    let mut wrong = blob.clone();
    wrong[0] ^= 0xff;
    assert!(k.import_native(&wrong).is_err(), "wrong magic");
    for cut in [8, blob.len() / 2, blob.len() - 1] {
        assert!(k.import_native(&blob[..cut]).is_err(), "truncated at {cut} of {}", blob.len());
    }
    let mut longer = blob.clone();
    longer.push(0);
    assert!(k.import_native(&longer).is_err(), "trailing bytes");
}

/// Kernels on eight threads at once (as G3a's part workers run them) build the same bodies as
/// one kernel alone, and each thread's bodies transfer to this one. Compared by topology
/// (entities, order, geometry) and mesh: the export bytes of equal bodies built separately can
/// differ (OCCT writes shared locations in an order that follows memory addresses).
#[test]
fn kernels_on_several_threads_agree() {
    let k = kernel();
    let serial: Vec<_> = bodies(&k).iter().map(|(_, b)| (k.topology(b).unwrap(), mesh_of(&k, b))).collect();
    for _round in 0..3 {
        let all: Vec<Vec<_>> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..8)
                .map(|_| {
                    s.spawn(|| {
                        let k = kernel();
                        bodies(&k)
                            .iter()
                            // Meshing runs on every thread too (OCCT's shared thread pool).
                            .map(|(_, b)| (mesh_of(&k, b), k.export_native(b).unwrap()))
                            .collect()
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for thread in &all {
            for ((mesh, blob), (topo, mesh0)) in thread.iter().zip(&serial) {
                assert!(mesh == mesh0, "a thread meshed differently");
                let b = k.import_native(blob).unwrap();
                assert!(&k.topology(&b).unwrap() == topo, "a thread built a different body");
            }
        }
    }
}


/// G3b's measurement for E5 option (a), building a feature's tool body on another host ahead
/// of its boolean: how long tools take to build against their booleans and their transfer.
/// `cargo test --release -p oc-kernel-occt --test transfer -- --ignored --nocapture`
#[test]
#[ignore = "timing report"]
fn tools_against_booleans() {
    let k = kernel();
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1e3;
    let plate = block(&k, 400.0, 300.0, 10.0);
    // Tools: 432 hole cylinders (one prism of 432 regions), a small block, a sweep along an arc.
    let holes: Vec<Region> = (0..24).flat_map(|i| (0..18).map(move |j| Region::new(circle(400.0 * (i as f64 + 0.5) / 24.0, 300.0 * (j as f64 + 0.5) / 18.0, 4.0, -1.0)))).collect();
    type Make<'a> = Box<dyn Fn() -> Body + 'a>;
    let cases: Vec<(&str, Make)> = vec![
        ("prism, 432 holes", Box::new(|| k.prism(&PrismSpec { profile: Profile::new(holes.clone()), vector: [0.0, 0.0, 12.0], taper: None }).unwrap().body)),
        ("prism, a small block on the top face", Box::new(|| {
            let r = Region::new(Wire::rect_xy(150.0, 150.0, 160.0, 160.0, 0.0));
            k.transform(&k.prism(&PrismSpec { profile: Profile::new(vec![r]), vector: [0.0, 0.0, 1.0], taper: None }).unwrap().body, &Transform::translation([0.0, 0.0, 9.5])).unwrap().body
        })),
        ("sweep, circle along an arc", Box::new(|| {
            let path = Wire::new(vec![Segment::Arc { a: [50.0, 150.0, 10.0], mid: [200.0, 250.0, 10.0], b: [350.0, 150.0, 10.0] }]);
            let prof = Wire::new(vec![Segment::Circle { center: [50.0, 150.0, 10.0], normal: [0.6, 0.8, 0.0], radius: 3.0 }]);
            k.sweep(&SweepSpec { profile: Profile::new(vec![Region::new(prof)]), path, orientation: SweepOrientation::FollowPath }).unwrap().body
        })),
    ];
    for (name, make) in &cases {
        let t = std::time::Instant::now();
        let tool = make();
        let build = ms(t);
        let t = std::time::Instant::now();
        let _ = k.boolean(BooleanOp::Cut, &plate, &[&tool]).unwrap();
        let cut = ms(t);
        let t = std::time::Instant::now();
        let blob = k.export_native(&tool).unwrap();
        let _ = k.import_native(&blob).unwrap();
        let transfer = ms(t);
        println!("{name}: tool {build:.1} ms, cut {cut:.1} ms, transfer {transfer:.1} ms ({} KB)", blob.len() / 1024);
    }
}
