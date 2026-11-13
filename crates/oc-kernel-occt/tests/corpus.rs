//! Corpus parts (`corpus/*.json`, `src/kernel/fixtures/parts.ts`) replayed step by step
//! with kernel calls, the way the TypeScript engine builds each feature, and compared with
//! the recorded metrics: volume, area and bounding box to 6 significant figures, solid,
//! face and edge counts exactly.
mod common;

use common::*;
use oc_kernel::*;
use std::f64::consts::PI;

const Z: P3 = [0.0, 0.0, 1.0];

#[test]
fn block_hole() {
    let k = kernel();
    let b = block(&k, 20.0, 10.0, 5.0);
    assert_step(&k, &b, "block-hole", "Boss-Extrude1");
    // Through All Both: a long cylinder through the plate.
    let tool = prism(&k, Region::new(circle(10.0, 5.0, 3.0, -50.0)), [0.0, 0.0, 100.0]).body;
    assert_step(&k, &cut(&k, &b, &tool), "block-hole", "Cut-Extrude1");
}

#[test]
fn pad_pocket() {
    let k = kernel();
    let b = block(&k, 60.0, 40.0, 10.0);
    let pocket = prism(&k, Region::new(rect(15.0, 10.0, 45.0, 30.0, 10.0)), [0.0, 0.0, -4.0]).body;
    let b = cut(&k, &b, &pocket);
    assert_step(&k, &b, "pad-pocket", "Cut-Extrude1");
    let t = k.topology(&b).unwrap();
    let floor = edges_where(&t, true, |m| near(m[2], 6.0));
    assert_eq!(floor.len(), 4);
    let edges: Vec<_> = floor.iter().map(|&edge| FilletEdge { edge, radius: Radius::Constant(1.5) }).collect();
    let f = k.fillet(&b, &edges).unwrap();
    assert_tracked(&f);
    assert_step(&k, &f.body, "pad-pocket", "Fillet1");
}

#[test]
fn fillet_chamfer() {
    let k = kernel();
    let part = "fillet-chamfer";
    let b = block(&k, 40.0, 30.0, 20.0);
    let vertical = |b: &oc_kernel_occt::Body, x: f64, y: f64| {
        let t = k.topology(b).unwrap();
        let e = edges_where(&t, true, |m| near(m[0], x) && near(m[1], y) && m[2] > 0.0 && m[2] < 20.0);
        assert_eq!(e.len(), 1);
        e[0]
    };
    let e = vertical(&b, 0.0, 0.0);
    let b = k.fillet(&b, &[FilletEdge { edge: e, radius: Radius::Constant(3.0) }]).unwrap().body;
    assert_step(&k, &b, part, "Fillet1");
    let e = vertical(&b, 0.0, 30.0);
    let b = k.fillet(&b, &[FilletEdge { edge: e, radius: Radius::Linear(2.0, 4.0) }]).unwrap().body;
    assert_step(&k, &b, part, "Fillet2");
    // Equal chamfer on the top back edge; it propagates round the tangent fillet chain.
    let t = k.topology(&b).unwrap();
    let top = edges_where(&t, true, |m| near(m[1], 30.0) && near(m[2], 20.0));
    assert_eq!(top.len(), 1);
    let c = k.chamfer(&b, &[ChamferEdge { edge: top[0], size: ChamferSize::Equal(2.0) }]).unwrap();
    assert_tracked(&c);
    let b = c.body;
    assert_step(&k, &b, part, "Chamfer1");
    // Unequal chamfers: the TS engine measures the first distance on the edge's first face
    // in OCCT's traversal order, an enumeration artefact. Here the face is named by
    // geometry: the y = 0 face for Chamfer2 and the x = 40 face for Chamfer3 (the only
    // choices that reproduce the recorded metrics).
    let face_with = |b: &oc_kernel_occt::Body, e: EdgeId, n: P3| {
        let t = k.topology(b).unwrap();
        *t.edges[e.index()].faces.iter().find(|f| v3::dist(t.faces[f.index()].normal, n) < 1e-6).unwrap()
    };
    let e = vertical(&b, 40.0, 0.0);
    let size = ChamferSize::Distances { d1: 2.0, d2: 3.0, face: face_with(&b, e, [0.0, -1.0, 0.0]) };
    let b = k.chamfer(&b, &[ChamferEdge { edge: e, size }]).unwrap().body;
    assert_step(&k, &b, part, "Chamfer2");
    let e = vertical(&b, 40.0, 30.0);
    let size = ChamferSize::DistanceAngle { distance: 2.0, angle: deg(30.0), face: face_with(&b, e, [1.0, 0.0, 0.0]) };
    let b = k.chamfer(&b, &[ChamferEdge { edge: e, size }]).unwrap().body;
    assert_step(&k, &b, part, "Chamfer3");
}

fn filleted_box(k: &oc_kernel_occt::OcctKernel, r: f64) -> oc_kernel_occt::Body {
    let b = block(k, 40.0, 30.0, 20.0);
    let t = k.topology(&b).unwrap();
    let edges: Vec<_> =
        edges_where(&t, true, |m| near(m[2], 10.0)).into_iter().map(|edge| FilletEdge { edge, radius: Radius::Constant(r) }).collect();
    assert_eq!(edges.len(), 4);
    k.fillet(&b, &edges).unwrap().body
}

#[test]
fn shell_inward() {
    let k = kernel();
    let b = filleted_box(&k, 4.0);
    assert_step(&k, &b, "shell", "Fillet1");
    let top = face_where(&k.topology(&b).unwrap(), planar(Z, 2, 20.0));
    let s = k.shell(&b, &ShellSpec { open: vec![top], thickness: 2.0, outward: false }).unwrap();
    assert_tracked(&s);
    assert_step(&k, &s.body, "shell", "Shell1");
}

#[test]
fn shell_outward() {
    let k = kernel();
    let b = prism(&k, Region::new(circle(0.0, 0.0, 10.0, 0.0)), [0.0, 0.0, 20.0]).body;
    assert_step(&k, &b, "shell-outward", "Boss-Extrude1");
    let top = face_where(&k.topology(&b).unwrap(), planar(Z, 2, 20.0));
    let s = k.shell(&b, &ShellSpec { open: vec![top], thickness: 1.5, outward: true }).unwrap();
    assert_tracked(&s);
    assert_step(&k, &s.body, "shell-outward", "Shell1");
}

#[test]
fn draft() {
    let k = kernel();
    let b = block(&k, 40.0, 30.0, 20.0);
    let sides = faces_where(&k.topology(&b).unwrap(), |f| matches!(f.surface, Surface::Plane { .. }) && f.normal[2].abs() < 1e-6);
    assert_eq!(sides.len(), 4);
    let spec = DraftSpec { faces: sides, neutral: Plane { origin: [0.0; 3], normal: Z }, angle: deg(3.0) };
    let d = k.draft(&b, &spec).unwrap();
    assert_tracked(&d);
    assert_step(&k, &d.body, "draft", "Draft1");
}

/// Revolve sketch on Front about the Y axis (the corpus' construction line x = 0).
fn rev(k: &oc_kernel_occt::OcctKernel, w: Wire, angle: f64, dir: f64) -> oc_kernel_occt::Body {
    let axis = Axis { origin: [0.0; 3], dir: [0.0, dir, 0.0] };
    k.revolve(&RevolveSpec { profile: Profile::single(w), axis, angle }).unwrap().body
}

#[test]
fn revolve() {
    let k = kernel();
    let part = "revolve";
    let b = rev(&k, rect(10.0, 0.0, 20.0, 50.0, 0.0), 2.0 * PI, 1.0);
    assert_step(&k, &b, part, "Revolve1");
    // Mid-plane cut revolve of 90°: the profile is first turned back by 45°.
    let back = Transform::rotation(Axis { origin: [0.0; 3], dir: [0.0, 1.0, 0.0] }, -PI / 4.0);
    let notch: Vec<P3> = [[15.0, 40.0, 0.0], [25.0, 40.0, 0.0], [25.0, 50.0, 0.0], [15.0, 50.0, 0.0]].map(|p| back.apply(p)).to_vec();
    let tool = rev(&k, Wire::polygon(&notch), PI / 2.0, 1.0);
    let b = cut(&k, &b, &tool);
    assert_step(&k, &b, part, "Cut-Revolve1");
    // 60° one way plus 30° the other, fused.
    let w = rect(20.0, 0.0, 30.0, 10.0, 0.0);
    let tool = fuse(&k, &rev(&k, w.clone(), deg(60.0), 1.0), &rev(&k, w, deg(30.0), -1.0));
    let b = fuse(&k, &b, &tool);
    assert_step(&k, &b, part, "Revolve2");
}

#[test]
fn sweep_loft() {
    let k = kernel();
    let part = "sweep-loft";
    let c = 10.0 - 10.0 * (PI / 4.0).cos();
    let path = Wire::new(vec![
        Segment::Line { a: [0.0, 0.0, 0.0], b: [0.0, 30.0, 0.0] },
        Segment::Arc { a: [0.0, 30.0, 0.0], mid: [c, 30.0 + 10.0 * (PI / 4.0).sin(), 0.0], b: [10.0, 40.0, 0.0] },
        Segment::Line { a: [10.0, 40.0, 0.0], b: [40.0, 40.0, 0.0] },
    ]);
    let profile = Wire::new(vec![Segment::Circle { center: [0.0; 3], normal: [0.0, 1.0, 0.0], radius: 3.0 }]);
    let s1 = k.sweep(&SweepSpec { profile: Profile::single(profile), path, orientation: SweepOrientation::FollowPath }).unwrap();
    assert_tracked(&s1);
    assert_step(&k, &s1.body, part, "Sweep1");
    // Square on Top (XZ; sketch y = −Z) swept up the Y axis.
    let sq = Wire::polygon(&[[57.0, 0.0, 3.0], [63.0, 0.0, 3.0], [63.0, 0.0, -3.0], [57.0, 0.0, -3.0]]);
    let path2 = Wire::new(vec![Segment::Line { a: [60.0, 0.0, 0.0], b: [60.0, 40.0, 0.0] }]);
    let s2 = k.sweep(&SweepSpec { profile: Profile::single(sq), path: path2, orientation: SweepOrientation::FollowPath }).unwrap();
    assert_tracked(&s2);
    let b = fuse(&k, &s1.body, &s2.body);
    assert_step(&k, &b, part, "Sweep2");
    let l = k.loft(&LoftSpec { sections: vec![rect(100.0, 0.0, 120.0, 20.0, 0.0), circle(110.0, 10.0, 6.0, 30.0)], ruled: false }).unwrap();
    assert_tracked(&l);
    let b = fuse(&k, &b, &l.body);
    assert_step(&k, &b, part, "Loft1");
}

#[test]
fn curved_profiles() {
    let k = kernel();
    let part = "curved-profiles";
    let p = |x: f64, y: f64| [x, y, 0.0];
    let slot = Wire::new(vec![
        Segment::Line { a: p(10.0, 5.0), b: p(40.0, 5.0) },
        Segment::Arc { a: p(40.0, 5.0), mid: p(45.0, 10.0), b: p(40.0, 15.0) },
        Segment::Line { a: p(40.0, 15.0), b: p(10.0, 15.0) },
        Segment::Arc { a: p(10.0, 15.0), mid: p(5.0, 10.0), b: p(10.0, 5.0) },
    ]);
    let b = prism(&k, Region::new(slot), [0.0, 0.0, 6.0]).body;
    assert_step(&k, &b, part, "Boss-Extrude1");
    let ellipse = Segment::Ellipse { center: p(70.0, 10.0), normal: Z, major_dir: [1.0, 0.0, 0.0], major: 12.0, minor: 6.0 };
    let e = prism(&k, Region::new(Wire::new(vec![ellipse])), [0.0, 0.0, 4.0]).body;
    assert_step(&k, &fuse(&k, &b, &e), part, "Boss-Extrude2");
}

#[test]
fn through_mid_dir2() {
    let k = kernel();
    let part = "through-mid-dir2";
    let b = prism(&k, Region::new(rect(0.0, 0.0, 50.0, 30.0, -10.0)), [0.0, 0.0, 20.0]).body;
    assert_step(&k, &b, part, "Boss-Extrude1");
    // Through All from the sketch plane z = 0: downwards for the cut, upwards reversed.
    let down = prism(&k, Region::new(circle(10.0, 15.0, 4.0, 0.0)), [0.0, 0.0, -200.0]).body;
    let b = cut(&k, &b, &down);
    assert_step(&k, &b, part, "Cut-Extrude1");
    let up = prism(&k, Region::new(circle(25.0, 15.0, 4.0, 0.0)), [0.0, 0.0, 200.0]).body;
    assert_step(&k, &cut(&k, &b, &up), part, "Cut-Extrude2");
}

#[test]
fn thin_contours_region_with_hole() {
    // Boss-Extrude4: a rectangle with a circular hole (Selected Contours), a separate solid.
    let k = kernel();
    let r = Region::new(rect(0.0, 50.0, 40.0, 80.0, 0.0)).with_hole(circle(20.0, 65.0, 5.0, 0.0));
    let op = prism(&k, r, [0.0, 0.0, 5.0]);
    assert_tracked(&op);
    let m = metrics(&k, &op.body);
    let (before, after) = (oracle("thin-contours", "Boss-Extrude3"), oracle("thin-contours", "Boss-Extrude4"));
    assert!((m.volume - (1200.0 - 25.0 * PI) * 5.0).abs() < 1e-6);
    // Differences of values rounded to 6 significant figures: within half a unit of the
    // sixth figure of each (0.05 + 0.005).
    assert!((m.volume - (after.volume - before.volume)).abs() < 0.056);
    assert!((m.area - (after.area - before.area)).abs() < 0.056);
    assert_eq!((m.faces, m.edges), (after.faces - before.faces, after.edges - before.edges));
}

#[test]
fn patterns() {
    let k = kernel();
    let part = "patterns";
    let b = block(&k, 100.0, 60.0, 5.0);
    let boss = prism(&k, Region::new(circle(10.0, 10.0, 4.0, 5.0)), [0.0, 0.0, 6.0]).body;
    let b = fuse(&k, &b, &boss);
    assert_step(&k, &b, part, "Boss-Extrude2");
    let pocket = prism(&k, Region::new(rect(20.0, 45.0, 24.0, 55.0, 5.0)), [0.0, 0.0, -3.0]).body;
    let b = cut(&k, &b, &pocket);
    assert_step(&k, &b, part, "Cut-Extrude1");
    // Linear pattern: copies of the boss tool, fused together, then into the part.
    let repeat = |tool: &oc_kernel_occt::Body, xfs: Vec<Transform>| {
        let copies: Vec<_> = xfs.iter().map(|t| k.transform(tool, t).unwrap().body).collect();
        let mut acc = copies[0].clone();
        for c in &copies[1..] {
            acc = fuse(&k, &acc, c);
        }
        acc
    };
    let mut xfs = vec![];
    for i in 0..4 {
        for j in 0..2 {
            if i + j > 0 {
                xfs.push(Transform::translation([15.0 * i as f64, 20.0 * j as f64, 0.0]));
            }
        }
    }
    let b = fuse(&k, &b, &repeat(&boss, xfs));
    assert_step(&k, &b, part, "LPattern1");
    let xfs = (1..3).map(|i| Transform::translation([10.0 * i as f64, 0.0, 0.0])).collect();
    let b = cut(&k, &b, &repeat(&pocket, xfs));
    assert_step(&k, &b, part, "LPattern2");
    let pin = prism(&k, Region::new(circle(70.0, 30.0, 3.0, 5.0)), [0.0, 0.0, 4.0]).body;
    let b = fuse(&k, &b, &pin);
    assert_step(&k, &b, part, "Boss-Extrude3");
    let axis = Axis { origin: [50.0, 30.0, 0.0], dir: Z };
    let xfs = (1..6).map(|i| Transform::rotation(axis, deg(60.0) * i as f64)).collect();
    let b = fuse(&k, &b, &repeat(&pin, xfs));
    assert_step(&k, &b, part, "CirPattern1");
    let m = Transform::mirror(Plane { origin: [50.0, 0.0, 0.0], normal: [1.0, 0.0, 0.0] });
    let b = fuse(&k, &b, &repeat(&boss, vec![m]));
    assert_step(&k, &b, part, "Mirror1");
    let whole = k.transform(&b, &Transform::mirror(Plane { origin: [0.0; 3], normal: Z })).unwrap().body;
    let b = fuse(&k, &b, &whole);
    assert_step(&k, &b, part, "Mirror2");
}

#[test]
fn imported_step() {
    let k = kernel();
    let op = k.read_step(&corpus_file("assets/block.step")).unwrap();
    assert_step(&k, &op.body, "imported", "Imported1");
    let boss = prism(&k, Region::new(circle(15.0, 8.0, 4.0, 10.0)), [0.0, 0.0, 5.0]).body;
    assert_step(&k, &fuse(&k, &op.body, &boss), "imported", "Boss-Extrude1");
}
