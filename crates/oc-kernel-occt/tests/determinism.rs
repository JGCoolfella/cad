//! Determinism (plan section 2): the same operations give the same bodies, entity order
//! and history, run after run and whether booleans run serially or in parallel. Plus a
//! timing report (`cargo test --release -p oc-kernel-occt --test determinism -- --ignored
//! --nocapture`).
mod common;

use common::*;
use oc_kernel::*;
use oc_kernel_occt::{Body, OcctKernel};

/// A script touching every operation family; returns each step's topology and history.
fn script(k: &OcctKernel) -> Vec<(String, Topology, History)> {
    let mut out = vec![];
    let mut keep = |name: &str, op: &Op<Body>| {
        out.push((name.to_string(), k.topology(&op.body).unwrap(), op.history.clone()));
        op.body.clone()
    };
    let plate = keep("prism", &prism(k, Region::new(rect(0.0, 0.0, 100.0, 60.0, 0.0)), [0.0, 0.0, 5.0]));
    let boss = prism(k, Region::new(circle(10.0, 10.0, 4.0, 5.0)), [0.0, 0.0, 6.0]).body;
    let copies: Vec<Body> = (0..8)
        .map(|i| k.transform(&boss, &Transform::translation([12.0 * i as f64, 20.0 * (i % 3) as f64, 0.0])).unwrap().body)
        .collect();
    let tools: Vec<&Body> = copies.iter().collect();
    let b = keep("fuse many", &k.boolean(BooleanOp::Fuse, &plate, &tools).unwrap());
    let hole = prism(k, Region::new(circle(50.0, 45.0, 6.0, -1.0)), [0.0, 0.0, 20.0]).body;
    let b = keep("cut", &k.boolean(BooleanOp::Cut, &b, &[&hole]).unwrap());
    let t = k.topology(&b).unwrap();
    let rims: Vec<FilletEdge> = edges_where(&t, false, |m| near(m[2], 11.0))
        .into_iter()
        .map(|edge| FilletEdge { edge, radius: Radius::Constant(1.0) })
        .collect();
    let b = keep("fillet", &k.fillet(&b, &rims).unwrap());
    let t = k.topology(&b).unwrap();
    let outer = edges_where(&t, true, |m| near(m[2], 5.0) && (near(m[1], 0.0) || near(m[1], 60.0)));
    let b = keep("chamfer", &k.chamfer(&b, &outer.iter().map(|&edge| ChamferEdge { edge, size: ChamferSize::Equal(1.0) }).collect::<Vec<_>>()).unwrap());
    let bottom = face_where(&k.topology(&b).unwrap(), planar([0.0, 0.0, -1.0], 2, 0.0));
    let b = keep("shell", &k.shell(&b, &ShellSpec { open: vec![bottom], thickness: 0.5, outward: false }).unwrap());
    keep("mirror", &k.transform(&b, &Transform::mirror(Plane { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] })).unwrap());
    let m = k.tessellate(&b, &MeshParams { linear: 0.05, angular: 0.3 }).unwrap();
    out.push(("mesh".into(), Topology { vertices: m.positions, ..Default::default() }, History::default()));
    out
}

#[test]
fn repeatable() {
    let a = script(&kernel());
    let b = script(&kernel());
    for (x, y) in a.iter().zip(&b) {
        assert!(x == y, "{} differs between runs", x.0);
    }
}

#[test]
fn serial_and_parallel_booleans_agree() {
    let serial = OcctKernel::new(Tolerances { boolean_parallel: false, ..Tolerances::DEFAULT });
    let a = script(&serial);
    let b = script(&kernel());
    for (x, y) in a.iter().zip(&b) {
        assert!(x == y, "{} differs between serial and parallel booleans", x.0);
    }
}

/// Timings for the section 8 log (release build).
#[test]
#[ignore]
fn timings() {
    use std::time::Instant;
    let k = kernel();
    let time = |name: &str, n: usize, f: &dyn Fn()| {
        f();
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        println!("{name}: {:.3} ms", t.elapsed().as_secs_f64() * 1e3 / n as f64);
    };
    time("script (7 ops + topology + mesh)", 20, &|| {
        script(&k);
    });
    let b = block(&k, 40.0, 30.0, 20.0);
    let tool = prism(&k, Region::new(circle(20.0, 15.0, 5.0, -1.0)), [0.0, 0.0, 22.0]).body;
    time("prism (rectangle)", 200, &|| {
        block(&k, 40.0, 30.0, 20.0);
    });
    time("cut box - cylinder", 200, &|| {
        cut(&k, &b, &tool);
    });
    let holed = cut(&k, &b, &tool);
    let t = k.topology(&holed).unwrap();
    let e: Vec<_> = edges_where(&t, true, |m| near(m[2], 10.0)).into_iter().map(|edge| FilletEdge { edge, radius: Radius::Constant(3.0) }).collect();
    time("fillet 4 edges", 100, &|| {
        k.fillet(&holed, &e).unwrap();
    });
    let top = face_where(&t, planar([0.0, 0.0, 1.0], 2, 20.0));
    time("shell", 50, &|| {
        k.shell(&holed, &ShellSpec { open: vec![top], thickness: 2.0, outward: false }).unwrap();
    });
    time("topology (7 faces)", 500, &|| {
        k.topology(&holed).unwrap();
    });
    time("check (7 faces)", 500, &|| {
        k.check(&holed).unwrap();
    });
    time("tessellate (display, quality 3)", 100, &|| {
        k.tessellate(&holed, &k.tolerances().display_mesh(3.0, 54.0)).unwrap();
    });
    let step = k.write_step(&[&holed]).unwrap();
    time("STEP write", 50, &|| {
        k.write_step(&[&holed]).unwrap();
    });
    time("STEP read", 50, &|| {
        k.read_step(&step).unwrap();
    });
}
