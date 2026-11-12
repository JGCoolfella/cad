//! The cases of `src/sketch/solver.test.ts` and the solver behaviour of
//! `src/sketch/refs.test.ts`, ported. TypeScript-generated sketches (the Slot and Polygon
//! tools) are covered by `parity.rs`.

use oc_sketch::generators::{self, bench};
use oc_sketch::{
    solve_sketch, DefState, Dimension, DimensionKind, Pt, Rad, Relation, SketchData, SketchEntity,
    SketchSystem, Status,
};

fn rect(sk: &mut SketchData, w: f64, h: f64) {
    for (id, x, y) in [("a", 0.0, 0.0), ("b", w, 0.0), ("c", w, h), ("d", 0.0, h)] {
        sk.add_point(id, x, y);
    }
    for (id, p1, p2) in [
        ("l1", "a", "b"),
        ("l2", "b", "c"),
        ("l3", "c", "d"),
        ("l4", "d", "a"),
    ] {
        sk.add_line(id, p1, p2);
    }
}

fn hv(sk: &mut SketchData) {
    sk.add_relation(Relation::Horizontal {
        id: "r1".into(),
        e: "l1".into(),
    });
    sk.add_relation(Relation::Horizontal {
        id: "r2".into(),
        e: "l3".into(),
    });
    sk.add_relation(Relation::Vertical {
        id: "r3".into(),
        e: "l2".into(),
    });
    sk.add_relation(Relation::Vertical {
        id: "r4".into(),
        e: "l4".into(),
    });
}

fn close(a: f64, b: f64, digits: i32) -> bool {
    (a - b).abs() < 0.5 * 10f64.powi(-digits)
}

#[test]
fn solves_horizontal_vertical_and_reports_dof() {
    let mut sk = SketchData::empty();
    rect(&mut sk, 10.0, 5.0);
    sk.points["c"].y = 6.0; // perturb
    hv(&mut sk);
    let a = solve_sketch(&mut sk);
    assert_eq!(a.status, Status::Under);
    assert_eq!(a.dof, 4); // position x, y + width + height
    assert!(close(sk.points["c"].y, sk.points["d"].y, 8));
}

#[test]
fn becomes_fully_defined_with_origin_and_two_dimensions() {
    let mut sk = SketchData::empty();
    rect(&mut sk, 10.0, 5.0);
    hv(&mut sk);
    sk.add_relation(Relation::Coincident {
        id: "r5".into(),
        p: "a".into(),
        e: "origin".into(),
    });
    sk.add_dimension(Dimension::new(
        "d1",
        DimensionKind::Distance,
        &["a", "b"],
        20.0,
    ));
    sk.add_dimension(Dimension::new(
        "d2",
        DimensionKind::Distance,
        &["b", "c"],
        8.0,
    ));
    let a = solve_sketch(&mut sk);
    assert_eq!(a.status, Status::Full);
    assert!(close(sk.points["c"].x, 20.0, 8));
    assert!(close(sk.points["c"].y, 8.0, 8));
    assert_eq!(a.entity_state["l3"], DefState::Full);
}

#[test]
fn flags_redundant_relations_as_over_defined() {
    let mut sk = SketchData::empty();
    rect(&mut sk, 10.0, 5.0);
    hv(&mut sk);
    sk.add_relation(Relation::Parallel {
        id: "r5".into(),
        e1: "l1".into(),
        e2: "l3".into(),
    });
    let a = solve_sketch(&mut sk);
    assert_eq!(a.status, Status::Over);
    assert!(a.bad.contains("r5"));
    // The newest relation is the one flagged, and what it touches is marked over defined.
    assert_eq!(a.bad.len(), 1);
    assert_eq!(a.entity_state["l1"], DefState::Over);
}

#[test]
fn drags_along_constraints_fixed_length_line_swings() {
    let mut sk = SketchData::empty();
    sk.add_point("a", 0.0, 0.0);
    sk.add_point("b", 10.0, 0.0);
    sk.add_line("l", "a", "b");
    sk.add_relation(Relation::Coincident {
        id: "r".into(),
        p: "a".into(),
        e: "origin".into(),
    });
    sk.add_dimension(Dimension::new(
        "d",
        DimensionKind::Distance,
        &["a", "b"],
        10.0,
    ));
    let mut sys = SketchSystem::new(&sk);
    sys.solve_default();
    assert!(sys.drag(&[("b", [0.0, 20.0])], oc_sketch::DRAG_STEPS));
    sys.commit(&mut sk);
    let b = &sk.points["b"];
    assert!(close(b.x.hypot(b.y), 10.0, 6));
    assert!(b.y > 9.9);
}

#[test]
fn solves_tangent_line_and_circle_radius() {
    let mut sk = SketchData::empty();
    sk.add_point("c", 1.0, 1.0);
    sk.add_circle("k", "c", 3.0);
    sk.add_point("a", -10.0, 5.0);
    sk.add_point("b", 10.0, 5.0);
    sk.add_line("l", "a", "b");
    sk.add_relation(Relation::Tangent {
        id: "t".into(),
        e1: "l".into(),
        e2: "k".into(),
        inner: None,
    });
    sk.add_dimension(Dimension::new("d", DimensionKind::Diameter, &["k"], 8.0));
    let a = solve_sketch(&mut sk);
    assert_ne!(a.status, Status::Unsolved);
    let SketchEntity::Circle { r, .. } = sk.entities["k"] else {
        panic!()
    };
    assert!(close(r, 4.0, 8));
}

#[test]
fn polygon_tool_relations_leave_position_size_and_rotation() {
    for n in [3, 6, 40] {
        let mut b = generators::polygon(n);
        let a = solve_sketch(&mut b.sketch);
        assert!(a.bad.is_empty(), "n = {n}: {:?}", a.bad);
        assert_eq!(a.dof, 4, "n = {n}");
    }
}

// ------------------------------------------------------------------ refs.test.ts

fn ext(sk: &mut SketchData, id: &str, x: f64, y: f64, owner: &str) {
    sk.add_point(id, x, y);
    sk.points[id].ext = Some(owner.into());
}

/// Model references: points with `ext` are constants, the rest solves against them.
#[test]
fn keeps_reference_geometry_fixed_while_the_rest_solves_against_it() {
    let mut sk = SketchData::empty();
    ext(&mut sk, "rv", 20.0, 10.0, "ref1");
    ext(&mut sk, "e1", 0.0, 10.0, "ref2");
    ext(&mut sk, "e2", 20.0, 10.0, "ref2");
    sk.entities.insert(
        "re".into(),
        SketchEntity::Line {
            id: "re".into(),
            p1: "e1".into(),
            p2: "e2".into(),
            construction: Some(true),
            ext: Some("ref2".into()),
        },
    );
    sk.add_point("m", 3.0, 3.0);
    sk.add_relation(Relation::Midpoint {
        id: "r1".into(),
        p: "m".into(),
        e: "re".into(),
    });
    sk.add_point("q", 1.0, 1.0);
    sk.add_point("k", 30.0, 30.0);
    sk.add_line("l", "rv", "k");
    solve_sketch(&mut sk);
    assert!(close(sk.points["m"].x, 10.0, 9) && close(sk.points["m"].y, 10.0, 9));
    assert_eq!((sk.points["rv"].x, sk.points["rv"].y), (20.0, 10.0));
    let sys = SketchSystem::new(&sk);
    assert_eq!(sys.points()["rv"], Pt::Fixed([20.0, 10.0]));
}

/// A circle standing for a model edge keeps its radius: a dimension cannot change it.
#[test]
fn reference_circles_have_a_fixed_radius() {
    let mut sk = SketchData::empty();
    ext(&mut sk, "rc", 0.0, 0.0, "ref1");
    sk.entities.insert(
        "k".into(),
        SketchEntity::Circle {
            id: "k".into(),
            c: "rc".into(),
            r: 5.0,
            construction: Some(true),
            ext: Some("ref1".into()),
        },
    );
    sk.add_point("c2", 20.0, 0.0);
    sk.add_circle("k2", "c2", 2.0);
    sk.add_relation(Relation::Coradial {
        id: "co".into(),
        e1: "k2".into(),
        e2: "k".into(),
    });
    let sys = SketchSystem::new(&sk);
    assert_eq!(sys.radii()["k"], Rad::Fixed(5.0));
    let a = solve_sketch(&mut sk);
    assert_eq!(a.status, Status::Full);
    let SketchEntity::Circle { r, .. } = sk.entities["k2"] else {
        panic!()
    };
    assert!(close(r, 5.0, 9));
    assert!(close(sk.points["c2"].x, 0.0, 9));
    let SketchEntity::Circle { r, .. } = sk.entities["k"] else {
        panic!()
    };
    assert_eq!(r, 5.0);
}

#[test]
fn moved_reference_geometry_is_followed() {
    let mut sk = SketchData::empty();
    ext(&mut sk, "e1", 0.0, 10.0, "ref");
    ext(&mut sk, "e2", 30.0, 10.0, "ref");
    sk.entities.insert(
        "re".into(),
        SketchEntity::Line {
            id: "re".into(),
            p1: "e1".into(),
            p2: "e2".into(),
            construction: None,
            ext: Some("ref".into()),
        },
    );
    sk.add_point("m", 10.0, 10.0);
    sk.add_relation(Relation::Midpoint {
        id: "r1".into(),
        p: "m".into(),
        e: "re".into(),
    });
    solve_sketch(&mut sk);
    assert!(close(sk.points["m"].x, 15.0, 9) && close(sk.points["m"].y, 10.0, 9));
}

// ------------------------------------------------------------------ decomposition

#[test]
fn independent_rectangles_are_separate_components() {
    let b = bench("grid", 12).unwrap();
    let sys = SketchSystem::new(&b.sketch);
    assert_eq!(sys.component_sizes(), vec![(8, 6); 12]);
    let linked = SketchSystem::new(&bench("grid-linked", 12).unwrap().sketch);
    assert_eq!(linked.component_sizes(), vec![(96, 83)]);
}

#[test]
fn benchmarks_have_their_designed_freedom() {
    for n in [10, 50] {
        for (kind, dof) in [
            ("grid", 2 * n),
            ("grid-linked", n + 1),
            ("linkage", n - 3),
            ("polygon", 4),
            ("tangent-chain", n + 1),
        ] {
            let mut b = bench(kind, n).unwrap();
            let a = solve_sketch(&mut b.sketch);
            assert!(a.bad.is_empty(), "{kind} {n}: {:?}", a.bad);
            assert_eq!(a.status, Status::Under, "{kind} {n}");
            assert_eq!(a.dof, dof, "{kind} {n}");
        }
    }
}

#[test]
fn drag_moves_only_the_dragged_component() {
    let b = bench("grid", 9).unwrap();
    let mut sys = SketchSystem::new(&b.sketch);
    assert!(sys.solve_default());
    let before = sys.x().to_vec();
    let (id, to) = &b.drag;
    assert!(sys.drag(&[(id, *to)], oc_sketch::DRAG_STEPS));
    // The rectangle can only translate. The minimum-norm projection of a drag of one corner
    // onto a translation of all four moves it by a quarter of the remaining distance, so
    // after 12 sub-steps it has covered 1 - (3/4)^12 of the way (as in TypeScript).
    let p = sys.point_value(id).unwrap();
    let start = b.sketch.points[id.as_str()].clone();
    let frac = 1.0 - 0.75f64.powi(12);
    assert!(
        (p[0] - (start.x + frac * 0.5)).abs() < 1e-9
            && (p[1] - (start.y + frac * 0.3)).abs() < 1e-9,
        "{p:?} vs {to:?}"
    );
    // Rectangle 8 owns parameters 2 + 64.. (after the origin, which is fixed).
    let changed: Vec<usize> = (0..before.len())
        .filter(|&i| (sys.x()[i] - before[i]).abs() > 1e-12)
        .collect();
    assert!(changed.iter().all(|&i| i >= 64), "{changed:?}");
    assert!(sys.residuals().iter().all(|r| r.abs() < 1e-9));
}

#[test]
fn constant_equations_count_as_redundant() {
    let mut sk = SketchData::empty();
    sk.add_point("p", 3.0, 4.0);
    sk.add_relation(Relation::Fix {
        id: "f".into(),
        target: "p".into(),
    });
    sk.add_relation(Relation::Coincident {
        id: "c".into(),
        p: "p".into(),
        e: "origin".into(),
    });
    let mut sys = SketchSystem::new(&sk);
    assert!(!sys.solve_default());
    let a = sys.analyse(false);
    assert_eq!(a.status, Status::Over);
    assert!(a.bad.contains("c"));
}

#[test]
fn json_round_trip_keeps_order() {
    let b = bench("polygon", 5).unwrap();
    let text = serde_json::to_string(&b.sketch).unwrap();
    let back = SketchData::from_json(&text).unwrap();
    assert_eq!(back, b.sketch);
    assert!(back.points.keys().eq(b.sketch.points.keys()));
}
