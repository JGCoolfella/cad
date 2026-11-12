//! P1 pass test: box minus cylinder, then a fillet on the hole rim, with full history.
use p1_occt::*;
use std::f64::consts::PI;

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol * b.abs().max(1.0)
}

#[test]
fn box_cylinder_history() {
    let bx = make_box(20.0, 10.0, 5.0);
    let cyl = make_cylinder(10.0, 5.0, -1.0, 3.0, 7.0);
    let box_faces = faces(&bx);
    let cyl_faces = faces(&cyl);
    assert_eq!(box_faces.len(), 6);
    assert_eq!(cyl_faces.len(), 3);

    let op = cut(&bx, &cyl).expect("cut");
    let r = op_result(&op);
    let m = metrics(&r);
    assert!(m.valid);
    assert_eq!(m.faces, 7);
    assert!(close(m.volume, 1000.0 - 45.0 * PI, 1e-9), "volume {}", m.volume);

    let rf = faces(&r);
    let hist = op_history(&op);
    let hole = rf.iter().position(|f| f.surface == CYLINDER).unwrap() as u32;
    for h in &hist {
        let src = if h.input == 0 { box_faces[h.index as usize] } else { cyl_faces[h.index as usize] };
        match (h.input, src.surface) {
            // Box top and bottom: each becomes exactly one face (now with a hole).
            (0, PLANE) if src.cz == 0.0 || src.cz == 5.0 => {
                assert!(!h.deleted && h.kept < 0, "{h:?}");
                assert_eq!(h.modified.len(), 1, "{h:?}");
                let f = rf[h.modified[0] as usize];
                assert!(close(f.cz, src.cz, 1e-9) && close(f.area, 200.0 - 9.0 * PI, 1e-9), "{f:?}");
            }
            // Box sides: untouched, so they keep their identity.
            (0, _) => assert!(h.kept >= 0 && h.modified.is_empty() && !h.deleted, "{h:?}"),
            // Tool wall: becomes the hole wall.
            (1, CYLINDER) => assert_eq!(h.modified, vec![hole], "{h:?}"),
            // Tool caps: outside the box, gone.
            (1, _) => assert!(h.deleted, "{h:?}"),
            _ => unreachable!(),
        }
    }

    // Fillet the hole rim on the top face.
    let top = rf.iter().position(|f| f.surface == PLANE && close(f.cz, 5.0, 1e-9)).unwrap() as u32;
    let rim = edges_between(&r, top, hole);
    assert_eq!(rim.len(), 1, "rim edges {rim:?}");
    let fop = fillet(&r, &rim, 1.0).expect("fillet");
    let fr = op_result(&fop);
    let fm = metrics(&fr);
    assert!(fm.valid);
    assert_eq!(fm.faces, 8);
    // Removed ring: Pappus, spandrel area r²(1 − π/4) at centroid radius 3 + r(10 − 3π)/(3(4 − π)).
    let k = (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI));
    let removed = 2.0 * PI * (3.0 + k) * (1.0 - PI / 4.0);
    assert!(close(fm.volume, 1000.0 - 45.0 * PI - removed, 1e-7), "volume {}", fm.volume);

    let ff = faces(&fr);
    let fh = op_history(&fop);
    let rim_fate = fh.iter().find(|h| h.kind == 1 && h.index == rim[0]).unwrap();
    assert_eq!(rim_fate.generated.len(), 1, "{rim_fate:?}");
    assert_eq!(ff[rim_fate.generated[0] as usize].surface, TORUS);
    let face_fate = |i: u32| fh.iter().find(|h| h.kind == 0 && h.index == i).unwrap();
    assert_eq!(face_fate(top).modified.len(), 1);
    assert_eq!(face_fate(hole).modified.len(), 1);
}

#[test]
fn kernel_errors_become_rust_errors() {
    let bx = make_box(20.0, 10.0, 5.0);
    let all: Vec<u32> = (0..12).collect();
    // A radius larger than the part cannot be built; it must come back as Err (or at worst
    // an invalid solid), never a crash.
    match fillet(&bx, &all, 50.0) {
        Err(e) => println!("fillet r50 -> Err({e})"),
        Ok(op) => assert!(!metrics(&op_result(&op)).valid, "r50 fillet produced a valid solid"),
    }
}

#[test]
fn throughput() {
    let n = 200;
    let t = std::time::Instant::now();
    for i in 0..n {
        let bx = make_box(20.0, 10.0, 5.0);
        let cyl = make_cylinder(10.0, 5.0, -1.0, 2.0 + (i % 10) as f64 * 0.1, 7.0);
        let r = op_result(&cut(&bx, &cyl).unwrap());
        let rf = faces(&r);
        let top = rf.iter().position(|f| f.surface == PLANE && f.cz == 5.0).unwrap() as u32;
        let hole = rf.iter().position(|f| f.surface == CYLINDER).unwrap() as u32;
        let rim = edges_between(&r, top, hole);
        let f = op_result(&fillet(&r, &rim, 0.5).unwrap());
        assert!(metrics(&f).valid);
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("box - cylinder + rim fillet + validity check: {ms:.2} ms per part");
}
