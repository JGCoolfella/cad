//! Mate solver: every mate type, degrees of freedom, alignment, redundant and conflicting
//! mates, fixed components, dragging, and large rotations.
use oc_mate::geom::{dot, norm, qrot, sub, unit};
use oc_mate::*;

const EPS: f64 = 1e-7;

fn fixed(pose: Pose) -> Body {
    Body { pose, fixed: true }
}
fn floating(pose: Pose) -> Body {
    Body { pose, fixed: false }
}
fn at(t: V3) -> Pose {
    Pose::new([1.0, 0.0, 0.0, 0.0], t)
}
/// Rotation by `deg` degrees about `axis`, then translation `t`.
fn rot(axis: V3, deg: f64, t: V3) -> Pose {
    let a = unit(axis);
    let (s, c) = (deg.to_radians() / 2.0).sin_cos();
    Pose::new([c, a[0] * s, a[1] * s, a[2] * s], t)
}
fn side(owner: Owner, geom: Geom) -> Side {
    Side { owner, geom }
}
fn b(i: usize, geom: Geom) -> Side {
    side(Owner::Body(i), geom)
}
fn ground(geom: Geom) -> Side {
    side(Owner::Ground, geom)
}
fn plane(p: V3, n: V3) -> Geom {
    Geom::Plane { p, n }
}
fn mate(kind: MateKind, a: Side, bb: Side, align: Align) -> Mate {
    Mate::new(kind, a, bb, align)
}
fn world(s: &Solved, side: &Side) -> Geom {
    match side.owner {
        Owner::Ground => side.geom,
        Owner::Body(i) => side.geom.placed(&s.poses[i]),
    }
}
fn close(a: V3, b: V3) -> bool {
    norm(sub(a, b)) < EPS
}
fn assert_ok(s: &Solved) {
    assert!(s.converged, "not converged: {:?}", s.mates);
    assert!(s.mates.iter().all(|m| *m == MateState::Ok), "{:?}", s.mates);
}

/// The top face of a 10 mm cube whose origin is at a corner: z = 10, normal +z.
const TOP: Geom = Geom::Plane { p: [5.0, 5.0, 10.0], n: [0.0, 0.0, 1.0] };
const BOTTOM: Geom = Geom::Plane { p: [5.0, 5.0, 0.0], n: [0.0, 0.0, -1.0] };
const LEFT: Geom = Geom::Plane { p: [0.0, 5.0, 5.0], n: [-1.0, 0.0, 0.0] };
const FRONT: Geom = Geom::Plane { p: [5.0, 0.0, 5.0], n: [0.0, -1.0, 0.0] };

fn cube_on_floor(extra: Vec<Mate>, start: Pose) -> (Problem, Solved) {
    let mut mates = vec![mate(MateKind::Coincident, ground(plane([0.0; 3], [0.0, 0.0, 1.0])), b(0, BOTTOM), Align::AntiAligned)];
    mates.extend(extra);
    let p = Problem { bodies: vec![floating(start)], mates };
    let s = solve(&p);
    (p, s)
}

#[test]
fn coincident_planes_leave_three_dof() {
    let (p, s) = cube_on_floor(vec![], rot([1.0, 2.0, 0.3], 25.0, [3.0, -4.0, 7.0]));
    assert_ok(&s);
    assert_eq!(s.dof, 3);
    assert_eq!(s.bodies[0], BodyInfo { state: BodyState::Under, free: 3 });
    let g = world(&s, &p.mates[0].b);
    let Geom::Plane { p: q, n } = g else { unreachable!() };
    assert!(close(n, [0.0, 0.0, -1.0]), "anti-aligned: {n:?}");
    assert!(q[2].abs() < EPS);
}

#[test]
fn three_coincident_planes_fully_define() {
    let ws = |n: V3| ground(plane([0.0; 3], n));
    let extra = vec![
        mate(MateKind::Coincident, ws([-1.0, 0.0, 0.0]), b(0, LEFT), Align::Aligned),
        mate(MateKind::Coincident, ws([0.0, -1.0, 0.0]), b(0, FRONT), Align::Aligned),
    ];
    let (_, s) = cube_on_floor(extra, rot([0.2, 0.1, 1.0], 30.0, [4.0, 6.0, 2.0]));
    assert_ok(&s);
    assert_eq!(s.dof, 0);
    assert_eq!(s.bodies[0].state, BodyState::Full);
    assert!(close(s.poses[0].t, [0.0; 3]), "{:?}", s.poses[0]);
    assert!((s.poses[0].q[0].abs() - 1.0).abs() < EPS, "identity rotation {:?}", s.poses[0].q);
}

#[test]
fn aligned_and_anti_aligned_pick_the_asked_side() {
    for (align, want) in [(Align::Aligned, [0.0, 0.0, 1.0]), (Align::AntiAligned, [0.0, 0.0, -1.0])] {
        // Start upside down relative to the asked alignment (170° away).
        let start = rot([1.0, 0.0, 0.0], if align == Align::Aligned { 170.0 } else { 10.0 }, [0.0, 0.0, 5.0]);
        let p = Problem { bodies: vec![floating(start)], mates: vec![mate(MateKind::Coincident, ground(plane([0.0; 3], [0.0, 0.0, 1.0])), b(0, TOP), align)] };
        let s = solve(&p);
        assert_ok(&s);
        let Geom::Plane { n, .. } = world(&s, &p.mates[0].b) else { unreachable!() };
        assert!(close(n, want), "{align:?}: {n:?}");
    }
}

#[test]
fn coincident_pairs_and_their_dof() {
    let line = |p: V3, d: V3| Geom::Line { p, d };
    let pt = |p: V3| Geom::Point { p };
    let circ = |c: V3, n: V3| Geom::Circle { c, n, r: 3.0 };
    let base = plane([0.0; 3], [0.0, 0.0, 1.0]);
    let cases: Vec<(&str, Side, Geom, usize)> = vec![
        ("plane-line", ground(base), line([1.0, 2.0, 3.0], [1.0, 0.0, 0.2]), 4),
        ("plane-point", ground(base), pt([1.0, 2.0, 3.0]), 5),
        ("plane-circle", ground(base), circ([1.0, 1.0, 2.0], [0.1, 0.0, 1.0]), 3),
        ("line-line", ground(line([0.0; 3], [0.0, 1.0, 0.0])), line([2.0, 0.0, 1.0], [0.1, 1.0, 0.0]), 2),
        ("line-point", ground(line([0.0; 3], [0.0, 1.0, 0.0])), pt([2.0, 0.0, 1.0]), 4),
        ("point-point", ground(pt([1.0, 1.0, 1.0])), pt([0.0, 0.0, 0.0]), 3),
        ("circle-circle", ground(circ([0.0; 3], [0.0, 0.0, 1.0])), circ([1.0, 1.0, 1.0], [0.0, 0.1, 1.0]), 1),
    ];
    for (name, a, g, dof) in cases {
        let p = Problem { bodies: vec![floating(rot([0.3, 0.2, 1.0], 12.0, [0.5, -0.5, 0.25]))], mates: vec![mate(MateKind::Coincident, a, b(0, g), Align::Aligned)] };
        let s = solve(&p);
        assert_ok(&s);
        assert_eq!(s.dof, dof, "{name}");
        assert_eq!(s.bodies[0].free, dof, "{name}");
        let (wa, wb) = (world(&s, &p.mates[0].a), world(&s, &p.mates[0].b));
        match (wa, wb) {
            (Geom::Plane { p: q, n }, other) => assert!(dot(sub(other.anchor(), q), n).abs() < EPS, "{name}"),
            (Geom::Line { p: q, d }, other) => {
                let v = sub(other.anchor(), q);
                assert!(norm(sub(v, oc_mate::geom::scale(unit(d), dot(v, unit(d))))) < EPS, "{name}");
            }
            (x, y) => assert!(close(x.anchor(), y.anchor()), "{name}"),
        }
    }
}

#[test]
fn parallel_perpendicular_angle() {
    let pl = |n: V3| plane([0.0; 3], n);
    let ax = |d: V3| Geom::Line { p: [0.0; 3], d };
    type Check = Box<dyn Fn(V3, V3) -> bool>;
    let cases: Vec<(MateKind, Geom, Geom, f64, usize, Check)> = vec![
        (MateKind::Parallel, pl([0.0, 0.0, 1.0]), pl([0.0, 1.0, 0.0]), 0.0, 4, Box::new(|a, b| (dot(a, b) - 1.0).abs() < EPS)),
        (MateKind::Parallel, pl([0.0, 0.0, 1.0]), ax([1.0, 1.0, 1.0]), 0.0, 5, Box::new(|a, b| dot(a, b).abs() < EPS)),
        (MateKind::Parallel, ax([1.0, 0.0, 0.0]), ax([1.0, 1.0, 0.0]), 0.0, 4, Box::new(|a, b| (dot(a, b) - 1.0).abs() < EPS)),
        (MateKind::Perpendicular, pl([0.0, 0.0, 1.0]), pl([0.0, 0.3, 1.0]), 0.0, 5, Box::new(|a, b| dot(a, b).abs() < EPS)),
        (MateKind::Perpendicular, pl([0.0, 0.0, 1.0]), ax([0.2, 0.0, 1.0]), 0.0, 4, Box::new(|a, b| (dot(a, b).abs() - 1.0).abs() < EPS)),
        (MateKind::Perpendicular, ax([1.0, 0.0, 0.0]), ax([1.0, 1.0, 0.0]), 0.0, 5, Box::new(|a, b| dot(a, b).abs() < EPS)),
        (MateKind::Angle, pl([0.0, 0.0, 1.0]), pl([0.0, 1.0, 1.0]), 30f64.to_radians(), 5, Box::new(|a, b| (dot(a, b) - 30f64.to_radians().cos()).abs() < EPS)),
    ];
    for (kind, ga, gb, value, dof, check) in cases {
        let mut m = mate(kind, ground(ga), b(0, gb), Align::Aligned);
        m.value = value;
        let p = Problem { bodies: vec![floating(at([1.0, 2.0, 3.0]))], mates: vec![m] };
        let s = solve(&p);
        assert_ok(&s);
        assert_eq!(s.dof, dof, "{kind:?} {gb:?}");
        let (wa, wb) = (world(&s, &p.mates[0].a), world(&s, &p.mates[0].b));
        assert!(check(unit(wa.direction().unwrap()), unit(wb.direction().unwrap())), "{kind:?}: {wa:?} {wb:?}");
    }
}

/// A pin (Ø6 cylinder along its z axis, shoulder face at z = 0 facing -z) in a hole of a
/// fixed plate (axis along world z through (20, 0), top face z = 5).
fn hinge(start: Pose) -> Problem {
    let hole = Geom::Cylinder { p: [20.0, 0.0, 0.0], d: [0.0, 0.0, 1.0], r: 3.0 };
    let pin = Geom::Cylinder { p: [0.0; 3], d: [0.0, 0.0, 1.0], r: 3.0 };
    Problem {
        bodies: vec![fixed(Pose::IDENTITY), floating(start)],
        mates: vec![
            mate(MateKind::Concentric, b(0, hole), b(1, pin), Align::Aligned),
            mate(MateKind::Coincident, b(0, plane([0.0, 0.0, 5.0], [0.0, 0.0, 1.0])), b(1, plane([0.0; 3], [0.0, 0.0, -1.0])), Align::AntiAligned),
        ],
    }
}

#[test]
fn concentric_and_coincident_make_a_hinge() {
    let p = hinge(rot([1.0, 0.5, 0.0], 20.0, [14.0, 3.0, 9.0]));
    let s = solve(&p);
    assert_ok(&s);
    assert_eq!(s.dof, 1, "rotation about the pin's axis");
    assert_eq!(s.bodies[0].state, BodyState::Fixed);
    assert_eq!(s.bodies[1], BodyInfo { state: BodyState::Under, free: 1 });
    assert!(close(s.poses[1].t, [20.0, 0.0, 5.0]), "{:?}", s.poses[1].t);
    // Concentric alone: translation along and rotation about the axis.
    let mut q = p.clone();
    q.mates.truncate(1);
    assert_eq!(solve(&q).dof, 2);
}

#[test]
fn hinge_drag_turns_about_the_axis() {
    let p = hinge(Pose::IDENTITY);
    let s0 = solve(&p);
    let mut p1 = p.clone();
    p1.bodies[1].pose = s0.poses[1];
    // Grab the pin 10 mm off its axis (a lever arm) and pull it sideways.
    let grab = [10.0, 0.0, 0.0];
    let target = [20.0, 10.0, 5.0];
    let s = drag(&p1, 1, grab, target, 1000.0);
    assert_ok(&s);
    let pose = s.poses[1];
    assert!(close(pose.t, [20.0, 0.0, 5.0]), "the axis stays put: {:?}", pose.t);
    let got = pose.point(grab);
    assert!((norm(sub(got, [20.0, 0.0, 5.0])) - 10.0).abs() < 1e-6, "radius kept");
    assert!(norm(sub(got, target)) < 1e-6, "reaches the target on its circle: {got:?}");
    let axis = qrot(pose.q, [0.0, 0.0, 1.0]);
    assert!(close(axis, [0.0, 0.0, 1.0]));
}

#[test]
fn free_drag_translates_without_turning() {
    let p = Problem { bodies: vec![floating(rot([0.0, 0.0, 1.0], 30.0, [1.0, 2.0, 3.0]))], mates: vec![] };
    let s = drag(&p, 0, [5.0, 0.0, 0.0], [40.0, -10.0, 3.0], 1000.0);
    assert!(close(s.poses[0].point([5.0, 0.0, 0.0]), [40.0, -10.0, 3.0]));
    let q0 = p.bodies[0].pose.q;
    assert!(s.poses[0].q.iter().zip(q0).all(|(a, b)| (a - b).abs() < 1e-3), "barely turned: {:?}", s.poses[0].q);
    assert_eq!(s.dof, 6);
}

#[test]
fn drag_on_a_plane_slides_in_the_plane() {
    let (p, s) = cube_on_floor(vec![], at([0.0, 0.0, 3.0]));
    let mut p1 = p.clone();
    p1.bodies[0].pose = s.poses[0];
    let s = drag(&p1, 0, [5.0, 5.0, 10.0], [30.0, 30.0, 50.0], 1000.0);
    assert_ok(&s);
    let got = s.poses[0].point([5.0, 5.0, 10.0]);
    assert!(close(got, [30.0, 30.0, 10.0]), "stays on the floor: {got:?}");
}

#[test]
fn concentric_alignment_and_spheres() {
    let cyl = Geom::Cylinder { p: [0.0; 3], d: [0.0, 0.0, 1.0], r: 2.0 };
    for align in [Align::Aligned, Align::AntiAligned] {
        let p = Problem { bodies: vec![floating(rot([1.0, 0.0, 0.0], 100.0, [3.0, 4.0, 0.0]))], mates: vec![mate(MateKind::Concentric, ground(cyl), b(0, cyl), align)] };
        let s = solve(&p);
        assert_ok(&s);
        let d = qrot(s.poses[0].q, [0.0, 0.0, 1.0]);
        let want = if align == Align::Aligned { 1.0 } else { -1.0 };
        assert!((d[2] - want).abs() < EPS, "{align:?} {d:?}");
    }
    let sph = Geom::Sphere { c: [1.0, 1.0, 1.0], r: 4.0 };
    let p = Problem { bodies: vec![floating(at([5.0, 0.0, 0.0]))], mates: vec![mate(MateKind::Concentric, ground(sph), b(0, Geom::Sphere { c: [0.0; 3], r: 1.0 }), Align::Aligned)] };
    let s = solve(&p);
    assert_ok(&s);
    assert_eq!(s.dof, 3, "ball joint");
    assert!(close(s.poses[0].t, [1.0, 1.0, 1.0]));
}

#[test]
fn tangent_mates() {
    let floor = ground(plane([0.0; 3], [0.0, 0.0, 1.0]));
    // A cylinder lying on the floor (its axis horizontal at height r).
    let cyl = Geom::Cylinder { p: [0.0; 3], d: [1.0, 0.0, 0.0], r: 4.0 };
    let p = Problem { bodies: vec![floating(rot([1.0, 1.0, 0.0], 15.0, [0.0, 0.0, 9.0]))], mates: vec![mate(MateKind::Tangent, floor, b(0, cyl), Align::AntiAligned)] };
    let s = solve(&p);
    assert_ok(&s);
    assert_eq!(s.dof, 4);
    let Geom::Cylinder { p: c, d, .. } = world(&s, &p.mates[0].b) else { unreachable!() };
    assert!((c[2] - 4.0).abs() < EPS && d[2].abs() < EPS, "{c:?} {d:?}");
    // A ball on the floor; two cylinders side by side.
    let ball = Geom::Sphere { c: [0.0; 3], r: 2.5 };
    let s = solve(&Problem { bodies: vec![floating(at([0.0, 0.0, 9.0]))], mates: vec![mate(MateKind::Tangent, floor, b(0, ball), Align::AntiAligned)] });
    assert_ok(&s);
    assert!((s.poses[0].t[2] - 2.5).abs() < EPS);
    let shaft = ground(Geom::Cylinder { p: [0.0; 3], d: [0.0, 0.0, 1.0], r: 5.0 });
    let roller = Geom::Cylinder { p: [0.0; 3], d: [0.0, 0.0, 1.0], r: 2.0 };
    let p = Problem { bodies: vec![floating(at([10.0, 3.0, 0.0]))], mates: vec![mate(MateKind::Tangent, shaft, b(0, roller), Align::AntiAligned)] };
    let s = solve(&p);
    assert_ok(&s);
    assert_eq!(s.dof, 3);
    let t = s.poses[0].t;
    assert!((t[0].hypot(t[1]) - 7.0).abs() < EPS, "outside, touching: {t:?}");
}

#[test]
fn distance_mates() {
    let floor = ground(plane([0.0; 3], [0.0, 0.0, 1.0]));
    for (flip, z) in [(false, 12.0), (true, -12.0)] {
        let mut m = mate(MateKind::Distance, floor, b(0, BOTTOM), Align::AntiAligned);
        m.value = 12.0;
        m.flip = flip;
        let s = solve(&Problem { bodies: vec![floating(at([0.0, 0.0, 1.0]))], mates: vec![m] });
        assert_ok(&s);
        assert_eq!(s.dof, 3);
        assert!((s.poses[0].t[2] - z).abs() < EPS, "flip {flip}: {:?}", s.poses[0].t);
    }
    let mut m = mate(MateKind::Distance, ground(Geom::Point { p: [0.0; 3] }), b(0, Geom::Point { p: [0.0; 3] }), Align::Aligned);
    m.value = 7.0;
    let s = solve(&Problem { bodies: vec![floating(at([1.0, 1.0, 1.0]))], mates: vec![m] });
    assert_ok(&s);
    assert_eq!(s.dof, 5);
    assert!((norm(s.poses[0].t) - 7.0).abs() < EPS);
}

#[test]
fn lock_fixes_the_relative_placement() {
    let rel = rot([0.0, 1.0, 1.0], 40.0, [10.0, 0.0, -5.0]);
    let mut m = mate(MateKind::Lock, b(0, Geom::Point { p: [0.0; 3] }), b(1, Geom::Point { p: [0.0; 3] }), Align::Aligned);
    m.relative = rel;
    let base = rot([1.0, 0.0, 0.0], 70.0, [3.0, 3.0, 3.0]);
    let p = Problem { bodies: vec![fixed(base), floating(at([0.0; 3]))], mates: vec![m] };
    let s = solve(&p);
    assert_ok(&s);
    assert_eq!(s.dof, 0);
    assert_eq!(s.bodies[1].state, BodyState::Full);
    let want = base.compose(&rel);
    assert!(close(s.poses[1].t, want.t));
    let d: f64 = s.poses[1].q.iter().zip(want.q).map(|(a, b)| a * b).sum();
    assert!((d.abs() - 1.0).abs() < 1e-9);
}

#[test]
fn redundant_mate_is_the_newest_and_over_defines() {
    let floor = ground(plane([0.0; 3], [0.0, 0.0, 1.0]));
    let extra = vec![mate(MateKind::Parallel, floor, b(0, BOTTOM), Align::AntiAligned)];
    let (_, s) = cube_on_floor(extra, at([0.0, 0.0, 4.0]));
    assert!(s.converged);
    assert_eq!(s.mates, vec![MateState::Ok, MateState::Redundant]);
    assert_eq!(s.newest_error, Some(1));
    assert_eq!(s.bodies[0].state, BodyState::Over);
    assert_eq!(s.dof, 3);
}

#[test]
fn conflicting_mate_is_set_aside_and_the_rest_stay_solved() {
    let floor = ground(plane([0.0; 3], [0.0, 0.0, 1.0]));
    let mut d = mate(MateKind::Distance, floor, b(0, BOTTOM), Align::AntiAligned);
    d.value = 10.0;
    let (p, s) = cube_on_floor(vec![d], at([0.0, 0.0, 4.0]));
    assert_eq!(s.mates, vec![MateState::Ok, MateState::Conflict]);
    assert_eq!(s.newest_error, Some(1));
    assert!(s.converged, "the remaining mates are satisfied");
    assert!(s.poses[0].t[2].abs() < EPS, "coincident holds: {:?}", s.poses[0].t);
    assert_eq!(s.bodies[0].state, BodyState::Over);
    // Reversed order: the coincident mate is now the newest and the one set aside.
    let mut q = p.clone();
    q.mates.swap(0, 1);
    let s = solve(&q);
    assert_eq!(s.mates, vec![MateState::Ok, MateState::Conflict]);
    assert!((s.poses[0].t[2] - 10.0).abs() < EPS);
}

#[test]
fn mates_between_fixed_components() {
    let face = |z: f64, nz: f64| plane([0.0, 0.0, z], [0.0, 0.0, nz]);
    let ok = mate(MateKind::Coincident, b(0, face(5.0, 1.0)), b(1, face(0.0, -1.0)), Align::AntiAligned);
    let bodies = vec![fixed(Pose::IDENTITY), fixed(at([0.0, 0.0, 5.0]))];
    let s = solve(&Problem { bodies: bodies.clone(), mates: vec![ok] });
    assert_eq!(s.mates, vec![MateState::Redundant], "satisfied, but adds nothing: over defined (🔍)");
    assert_eq!(s.dof, 0);
    let bad = mate(MateKind::Coincident, b(0, face(5.0, 1.0)), b(1, face(3.0, -1.0)), Align::AntiAligned);
    let s = solve(&Problem { bodies, mates: vec![bad] });
    assert_eq!(s.mates, vec![MateState::Conflict]);
    assert!(s.bodies.iter().all(|b| b.state == BodyState::Fixed));
}

#[test]
fn invalid_pairs_and_suppressed_mates() {
    let mut off = mate(MateKind::Coincident, ground(plane([0.0; 3], [0.0, 0.0, 1.0])), b(0, BOTTOM), Align::AntiAligned);
    off.enabled = false;
    let bad = mate(MateKind::Concentric, ground(plane([0.0; 3], [0.0, 0.0, 1.0])), b(0, BOTTOM), Align::AntiAligned);
    let s = solve(&Problem { bodies: vec![floating(at([0.0, 0.0, 4.0]))], mates: vec![off, bad] });
    assert_eq!(s.mates[0], MateState::Off);
    assert!(matches!(s.mates[1], MateState::Invalid(_)));
    assert_eq!(s.dof, 6);
    assert!(close(s.poses[0].t, [0.0, 0.0, 4.0]), "nothing moved");
}

#[test]
fn chain_of_links_counts_dof_across_bodies() {
    // Two links pinned in series to a fixed base, all on one plane: a planar 2R arm.
    let z = [0.0, 0.0, 1.0];
    let pin = |p: V3| Geom::Cylinder { p, d: z, r: 1.0 };
    let face = |nz: f64| plane([0.0; 3], [0.0, 0.0, nz]);
    let bodies = vec![fixed(Pose::IDENTITY), floating(at([1.0, 2.0, 0.5])), floating(at([20.0, 5.0, -1.0]))];
    let mates = vec![
        mate(MateKind::Concentric, b(0, pin([0.0; 3])), b(1, pin([0.0; 3])), Align::Aligned),
        mate(MateKind::Coincident, b(0, face(1.0)), b(1, face(-1.0)), Align::AntiAligned),
        mate(MateKind::Concentric, b(1, pin([30.0, 0.0, 0.0])), b(2, pin([0.0; 3])), Align::Aligned),
        mate(MateKind::Coincident, b(1, face(1.0)), b(2, face(-1.0)), Align::AntiAligned),
    ];
    let s = solve(&Problem { bodies, mates });
    assert_ok(&s);
    assert_eq!(s.dof, 2);
    // With the other link held: the first link cannot turn (the second pins its far end),
    // the second still turns about its pin.
    assert_eq!(s.bodies[1].free, 0);
    assert_eq!(s.bodies[2].free, 1);
    assert!(s.bodies[1..].iter().all(|b| b.state == BodyState::Under));
}

#[test]
fn rotations_through_every_orientation_converge() {
    // No gimbal lock: from many start orientations (including 90° pitches) the cube
    // lands on the same fully defined placement.
    let ws = |n: V3| ground(plane([0.0; 3], n));
    for (axis, deg) in [([0.0, 1.0, 0.0], 90.0), ([1.0, 0.0, 0.0], -90.0), ([1.0, 1.0, 1.0], 120.0), ([0.0, 1.0, 0.0], 179.0), ([0.3, -1.0, 0.2], 150.0)] {
        let extra = vec![
            mate(MateKind::Coincident, ws([-1.0, 0.0, 0.0]), b(0, LEFT), Align::Aligned),
            mate(MateKind::Coincident, ws([0.0, -1.0, 0.0]), b(0, FRONT), Align::Aligned),
        ];
        let (_, s) = cube_on_floor(extra, rot(axis, deg, [2.0, 1.0, 3.0]));
        assert_ok(&s);
        assert!(close(s.poses[0].t, [0.0; 3]), "{axis:?} {deg}: {:?}", s.poses[0]);
        assert!((s.poses[0].q[0].abs() - 1.0).abs() < 1e-9, "{axis:?} {deg}: {:?}", s.poses[0].q);
    }
}
