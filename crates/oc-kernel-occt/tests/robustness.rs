//! The phase-0 OCCT failure inventory (`corpus/hard/inventory.json`, `hard.ts`) on the
//! native kernel, plus argument errors. A hard case must give the outcome the WASM build
//! recorded (so any change, better or worse, is noticed), a valid body when it succeeds,
//! and the right volume: the exact value where geometry gives one, else the inventory's.
mod common;

use common::*;
use oc_kernel::*;
use oc_kernel_occt::{Body, OcctKernel};
use std::f64::consts::PI;

const Z: P3 = [0.0, 0.0, 1.0];

fn fillet_all(k: &OcctKernel, b: &Body, edges: Vec<EdgeId>, r: Radius) -> KResult<Op<Body>> {
    k.fillet(b, &edges.into_iter().map(|edge| FilletEdge { edge, radius: r }).collect::<Vec<_>>())
}

fn top_long_edges(k: &OcctKernel, b: &Body) -> Vec<EdgeId> {
    let t = k.topology(b).unwrap();
    edges_where(&t, true, |m| near(m[2], 10.0) && (near(m[1], 0.0) || near(m[1], 10.0)))
        .into_iter()
        .filter(|e| (t.edges[e.index()].end[0] - t.edges[e.index()].start[0]).abs() > 1.0)
        .collect()
}

fn vertical(k: &OcctKernel, b: &Body, x: f64, y: f64, h: f64) -> Vec<EdgeId> {
    edges_where(&k.topology(b).unwrap(), true, |m| near(m[0], x) && near(m[1], y) && m[2] > 0.0 && m[2] < h)
}

fn rounded_box(a: f64, b: f64, c: f64, r: f64) -> f64 {
    let (aa, bb, cc) = (a - 2.0 * r, b - 2.0 * r, c - 2.0 * r);
    aa * bb * cc + 2.0 * r * (aa * bb + bb * cc + aa * cc) + PI * r * r * (aa + bb + cc) + 4.0 / 3.0 * PI * r.powi(3)
}

fn fillet_ring(big: f64, r: f64) -> f64 {
    let area = r * r * (1.0 - PI / 4.0);
    let d = r * (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    2.0 * PI * (big + d) * area
}

fn shelled(k: &OcctKernel, b: &Body, open: FaceId, t: f64, outward: bool) -> KResult<Op<Body>> {
    k.shell(b, &ShellSpec { open: vec![open], thickness: t, outward })
}

fn top(k: &OcctKernel, b: &Body, z: f64) -> FaceId {
    face_where(&k.topology(b).unwrap(), planar(Z, 2, z))
}

fn cyl_x(k: &OcctKernel, r: f64) -> Body {
    let w = Wire::new(vec![Segment::Circle { center: [-20.0, 0.0, 0.0], normal: [1.0, 0.0, 0.0], radius: r }]);
    prism(k, Region::new(w), [40.0, 0.0, 0.0]).body
}

/// (name, inventory outcome ok?, exact volume, inventory volume, case)
type Case = (&'static str, bool, Option<f64>, Option<f64>, fn(&OcctKernel) -> KResult<Op<Body>>);

fn cases() -> Vec<Case> {
    vec![
        ("H01", true, Some(2000.0 - 2.0 * (1.0 - PI / 4.0) * 4.99f64.powi(2) * 20.0), None, |k| {
            let b = block(k, 20.0, 10.0, 10.0);
            fillet_all(k, &b, top_long_edges(k, &b), Radius::Constant(4.99))
        }),
        ("H02", false, None, None, |k| {
            let b = block(k, 20.0, 10.0, 10.0);
            fillet_all(k, &b, top_long_edges(k, &b), Radius::Constant(5.0))
        }),
        ("H03", false, None, None, |k| {
            let b = block(k, 20.0, 10.0, 10.0);
            fillet_all(k, &b, top_long_edges(k, &b), Radius::Constant(5.01))
        }),
        ("H04", true, Some(27000.0 - 3.0 * (1.0 - PI / 4.0) * 25.0 * 25.0 - 125.0 * (1.0 - PI / 6.0)), None, |k| {
            let b = block(k, 30.0, 30.0, 30.0);
            let t = k.topology(&b).unwrap();
            let e = edges_where(&t, true, |m| m.iter().filter(|x| near(**x, 30.0)).count() == 2);
            fillet_all(k, &b, e, Radius::Constant(5.0))
        }),
        ("H05", true, None, Some(9982.97), |k| {
            let p = |x: f64, y: f64| [x, y, 0.0];
            let l = Wire::polygon(&[p(0.0, 0.0), p(30.0, 0.0), p(30.0, 10.0), p(10.0, 10.0), p(10.0, 30.0), p(0.0, 30.0)]);
            let b = prism(k, Region::new(l), [0.0, 0.0, 20.0]).body;
            let t = k.topology(&b).unwrap();
            let at = |x: f64, y: f64, z: f64| edges_where(&t, true, |m| near(m[0], x) && near(m[1], y) && near(m[2], z));
            fillet_all(k, &b, [at(10.0, 10.0, 10.0), at(20.0, 10.0, 20.0), at(10.0, 20.0, 20.0)].concat(), Radius::Constant(2.0))
        }),
        ("H06", true, None, Some(26904.4), |k| {
            let b = block(k, 30.0, 30.0, 30.0);
            let e = edges_where(&k.topology(&b).unwrap(), true, |m| near(m[1], 0.0) && near(m[2], 30.0));
            fillet_all(k, &b, e, Radius::Linear(1.0, 6.0))
        }),
        ("H07", true, Some(rounded_box(30.0, 20.0, 10.0, 2.0)), None, |k| {
            let b = block(k, 30.0, 20.0, 10.0);
            fillet_all(k, &b, (0..12).map(EdgeId).collect(), Radius::Constant(2.0))
        }),
        ("H08", true, Some(16000.0 + PI * 640.0 + fillet_ring(8.0, 2.0)), None, |k| {
            let b = fuse(k, &block(k, 40.0, 40.0, 10.0), &prism(k, Region::new(circle(20.0, 20.0, 8.0, 10.0)), [0.0, 0.0, 10.0]).body);
            let e = edges_where(&k.topology(&b).unwrap(), false, |m| near(m[2], 10.0))
                .into_iter()
                .filter(|e| matches!(k.topology(&b).unwrap().edges[e.index()].curve, Curve::Circle { .. }))
                .collect();
            fillet_all(k, &b, e, Radius::Constant(2.0))
        }),
        ("H09", false, None, None, |k| {
            let p = |x: f64, y: f64| [x, y, 0.0];
            let w = Wire::polygon(&[p(0.0, 0.0), p(30.0, 0.0), p(30.0, 20.0), p(29.0, 20.0), p(29.0, 21.0), p(0.0, 21.0)]);
            let b = prism(k, Region::new(w), [0.0, 0.0, 10.0]).body;
            let t = k.topology(&b).unwrap();
            let v = |x: f64, y: f64| edges_where(&t, true, |m| near(m[0], x) && near(m[1], y) && near(m[2], 5.0));
            fillet_all(k, &b, [v(30.0, 20.0), v(29.0, 20.0), v(29.0, 21.0)].concat(), Radius::Constant(2.0))
        }),
        ("H23", false, None, None, |k| {
            let b = block(k, 40.0, 30.0, 20.0);
            let e = [vertical(k, &b, 0.0, 0.0, 20.0), vertical(k, &b, 0.0, 30.0, 20.0)].concat();
            let b = fillet_all(k, &b, e, Radius::Constant(3.0)).unwrap().body;
            let b = fillet_all(k, &b, vertical(k, &b, 40.0, 0.0, 20.0), Radius::Linear(2.0, 4.0)).unwrap().body;
            let t = k.topology(&b).unwrap();
            let e = edges_where(&t, true, |m| near(m[1], 0.0) && near(m[2], 0.0));
            let face = t.edges[e[0].index()].faces[0];
            k.chamfer(&b, &[ChamferEdge { edge: e[0], size: ChamferSize::Distances { d1: 2.0, d2: 3.0, face } }])
        }),
        ("H10", false, None, None, |k| {
            let b = block(k, 30.0, 20.0, 10.0);
            let b = fillet_all(k, &b, (0..12).map(EdgeId).collect(), Radius::Constant(2.0)).unwrap().body;
            shelled(k, &b, top(k, &b, 10.0), 1.0, false)
        }),
        ("H11", false, None, None, |k| {
            let b = block(k, 40.0, 30.0, 20.0);
            let e = edges_where(&k.topology(&b).unwrap(), true, |m| near(m[2], 10.0));
            let b = fillet_all(k, &b, e, Radius::Constant(2.0)).unwrap().body;
            shelled(k, &b, top(k, &b, 20.0), 3.0, false)
        }),
        ("H12", true, None, Some(3465.5), |k| {
            let b = fuse(k, &block(k, 40.0, 40.0, 10.0), &prism(k, Region::new(circle(20.0, 20.0, 8.0, 10.0)), [0.0, 0.0, 10.0]).body);
            let bottom = face_where(&k.topology(&b).unwrap(), planar([0.0, 0.0, -1.0], 2, 0.0));
            shelled(k, &b, bottom, 1.0, false)
        }),
        ("H13", true, None, Some(4060.1), |k| {
            let b = block(k, 40.0, 30.0, 20.0);
            let e = edges_where(&k.topology(&b).unwrap(), true, |m| near(m[2], 10.0));
            let b = fillet_all(k, &b, e, Radius::Constant(3.0)).unwrap().body;
            shelled(k, &b, top(k, &b, 20.0), 1.0, true)
        }),
        ("H14", false, None, None, |k| {
            let b = prism(k, Region::new(circle(0.0, 0.0, 10.0, 0.0)), [0.0, 0.0, 20.0]).body;
            let faces = faces_where(&k.topology(&b).unwrap(), |f| matches!(f.surface, Surface::Cylinder { .. }));
            k.draft(&b, &DraftSpec { faces, neutral: Plane { origin: [0.0; 3], normal: Z }, angle: deg(5.0) })
        }),
        ("H15", true, None, Some(22407.4), |k| {
            let b = block(k, 40.0, 30.0, 20.0);
            let e = edges_where(&k.topology(&b).unwrap(), true, |m| near(m[2], 10.0));
            let b = fillet_all(k, &b, e, Radius::Constant(3.0)).unwrap().body;
            let faces = faces_where(&k.topology(&b).unwrap(), |f| f.normal[2].abs() < 1e-6);
            k.draft(&b, &DraftSpec { faces, neutral: Plane { origin: [0.0; 3], normal: Z }, angle: deg(3.0) })
        }),
        ("H16", true, Some(4000.0), None, |k| {
            k.boolean(BooleanOp::Fuse, &block(k, 20.0, 10.0, 10.0), &[&prism(k, Region::new(rect(20.0, 0.0, 40.0, 10.0, 0.0)), [0.0, 0.0, 10.0]).body])
        }),
        ("H17", true, Some(10000.0), None, |k| {
            k.boolean(BooleanOp::Fuse, &block(k, 40.0, 20.0, 10.0), &[&prism(k, Region::new(rect(0.0, 0.0, 40.0, 10.0, 10.0)), [0.0, 0.0, 5.0]).body])
        }),
        ("H18", true, None, Some(8785.4), |k| {
            k.boolean(BooleanOp::Fuse, &block(k, 40.0, 20.0, 10.0), &[&prism(k, Region::new(circle(20.0, 25.0, 5.0, 0.0)), [0.0, 0.0, 10.0]).body])
        }),
        ("H19", true, Some(6000.0), None, |k| {
            k.boolean(BooleanOp::Cut, &block(k, 40.0, 20.0, 10.0), &[&prism(k, Region::new(rect(10.0, 5.0, 30.0, 15.0, 10.0)), [0.0, 0.0, -10.0]).body])
        }),
        ("H20", true, Some(2.0 * PI * 25.0 * 40.0 - 16.0 / 3.0 * 125.0), None, |k| {
            let a = prism(k, Region::new(circle(0.0, 0.0, 5.0, -20.0)), [0.0, 0.0, 40.0]).body;
            k.boolean(BooleanOp::Fuse, &a, &[&cyl_x(k, 5.0)])
        }),
        ("H21", true, None, Some(5620.44), |k| {
            let a = prism(k, Region::new(circle(0.0, 0.0, 5.0, -20.0)), [0.0, 0.0, 40.0]).body;
            k.boolean(BooleanOp::Fuse, &a, &[&cyl_x(k, 5.001)])
        }),
        ("H22", true, Some(2.0 / 3.0 * PI * 1000.0), None, |k| {
            let half_disc = Wire::new(vec![
                Segment::Arc { a: [0.0, -10.0, 0.0], mid: [10.0, 0.0, 0.0], b: [0.0, 10.0, 0.0] },
                Segment::Line { a: [0.0, 10.0, 0.0], b: [0.0, -10.0, 0.0] },
            ]);
            let axis = Axis { origin: [0.0; 3], dir: [0.0, 1.0, 0.0] };
            let s = k.revolve(&RevolveSpec { profile: Profile::single(half_disc), axis, angle: 2.0 * PI }).unwrap().body;
            let tool = prism(k, Region::new(rect(-20.0, -20.0, 0.0, 20.0, -50.0)), [0.0, 0.0, 100.0]).body;
            k.boolean(BooleanOp::Cut, &s, &[&tool])
        }),
    ]
}

#[test]
fn failure_inventory() {
    let k = kernel();
    let mut report = vec![];
    for (name, ok, exact, recorded, run) in cases() {
        let r = run(&k);
        let line = match &r {
            Ok(op) => {
                let v = k.mass_properties(&op.body).unwrap().volume;
                assert!(k.check(&op.body).unwrap().valid, "{name}: invalid body returned");
                if let Some(x) = exact {
                    assert!((v - x).abs() <= 1e-4 * x, "{name}: volume {v}, exact {x}");
                }
                if let Some(x) = recorded {
                    assert_eq!(sig6(v), x, "{name}: volume {v}, inventory {x}");
                }
                format!("{name}: ok, volume {v:.6}, {} solids{}", k.topology(&op.body).unwrap().solids, if op.healed { ", healed" } else { "" })
            }
            Err(e) => format!("{name}: error: {e}"),
        };
        println!("{line}");
        report.push((name, r.is_ok(), ok, line));
    }
    let changed: Vec<_> = report.iter().filter(|r| r.1 != r.2).map(|r| r.3.clone()).collect();
    assert!(changed.is_empty(), "outcome differs from the WASM inventory:\n{}", changed.join("\n"));
}

#[test]
fn invalid_shell_is_an_error_not_a_body() {
    // H10: the WASM build returned an invalid solid silently; healing it "worked" by
    // keeping the removed face as a lid. Natively it must be an error.
    let k = kernel();
    let b = block(&k, 30.0, 20.0, 10.0);
    let b = fillet_all(&k, &b, (0..12).map(EdgeId).collect(), Radius::Constant(2.0)).unwrap().body;
    let r = shelled(&k, &b, top(&k, &b, 10.0), 1.0, false);
    assert!(matches!(r, Err(KernelError::InvalidResult { .. } | KernelError::Failed { .. })), "{r:?}");
}

#[test]
fn impossible_fillet_fails_cleanly() {
    let k = kernel();
    let b = block(&k, 20.0, 10.0, 5.0);
    let r = fillet_all(&k, &b, vec![EdgeId(0)], Radius::Constant(50.0));
    assert!(matches!(r, Err(KernelError::Failed { op: OpKind::Fillet, .. })), "{r:?}");
}

#[test]
fn argument_errors() {
    let k = kernel();
    let b = block(&k, 20.0, 10.0, 5.0);
    let input = |r: KResult<Op<Body>>| matches!(r, Err(KernelError::InvalidInput { .. }));
    let open = Wire::new(rect(0.0, 0.0, 1.0, 1.0, 0.0).segments[..3].to_vec());
    assert!(input(k.prism(&PrismSpec { profile: Profile::single(open), vector: Z, taper: None })));
    let sq = Profile::single(rect(0.0, 0.0, 1.0, 1.0, 0.0));
    assert!(input(k.prism(&PrismSpec { profile: sq.clone(), vector: [0.0; 3], taper: None })));
    assert!(input(k.prism(&PrismSpec { profile: sq.clone(), vector: [1.0, 0.0, 0.0], taper: None })), "direction in the profile plane");
    let steep = Some(Taper { angle: deg(90.0), outward: false });
    assert!(input(k.prism(&PrismSpec { profile: sq, vector: Z, taper: steep })));
    assert!(input(fillet_all(&k, &b, vec![EdgeId(12)], Radius::Constant(1.0))));
    assert!(input(fillet_all(&k, &b, vec![EdgeId(1), EdgeId(1)], Radius::Constant(1.0))));
    assert!(input(fillet_all(&k, &b, vec![EdgeId(1)], Radius::Constant(-1.0))));
    assert!(input(k.fillet(&b, &[])));
    // A chamfer reference face must touch the edge.
    let t = k.topology(&b).unwrap();
    let e = EdgeId(0);
    let away = (0..6).map(FaceId).find(|f| !t.edges[0].faces.contains(f)).unwrap();
    assert!(input(k.chamfer(&b, &[ChamferEdge { edge: e, size: ChamferSize::Distances { d1: 1.0, d2: 1.0, face: away } }])));
    assert!(input(k.shell(&b, &ShellSpec { open: vec![FaceId(6)], thickness: 1.0, outward: false })));
    assert!(input(k.boolean(BooleanOp::Fuse, &b, &[])));
    let mut scale = Transform::IDENTITY;
    scale.m[0][0] = 2.0;
    assert!(input(k.transform(&b, &scale)));
    assert!(input(k.loft(&LoftSpec { sections: vec![rect(0.0, 0.0, 1.0, 1.0, 0.0)], ruled: false })));
    assert!(input(k.revolve(&RevolveSpec { profile: Profile::single(rect(1.0, 0.0, 2.0, 1.0, 0.0)), axis: Axis { origin: [0.0; 3], dir: [0.0; 3] }, angle: 1.0 })));
    assert!(matches!(k.tessellate(&b, &MeshParams { linear: 0.0, angular: 0.1 }), Err(KernelError::InvalidInput { .. })));
}

#[test]
fn empty_results_and_bad_files() {
    let k = kernel();
    let b = block(&k, 20.0, 10.0, 5.0);
    let big = k.primitive(&Primitive::Block { min: [-1.0; 3], size: [30.0; 3] }).unwrap().body;
    assert!(matches!(k.boolean(BooleanOp::Cut, &b, &[&big]), Err(KernelError::Empty { .. })));
    let far = k.primitive(&Primitive::Block { min: [100.0; 3], size: [1.0; 3] }).unwrap().body;
    assert!(matches!(k.boolean(BooleanOp::Common, &b, &[&far]), Err(KernelError::Empty { .. })));
    assert!(matches!(k.section(&b, &Plane { origin: [0.0, 0.0, 50.0], normal: Z }), Err(KernelError::Empty { .. })));
    assert!(matches!(k.read_step(b"not a step file"), Err(KernelError::Io { .. })));
    assert!(matches!(k.read_step(b""), Err(KernelError::Io { .. })));
}
