//! Solving per connected group of bodies (G3c): bodies joined by no chain of mates are
//! independent, so each group is solved, analysed and dragged on its own. The results are
//! those of solving everything together ([`oc_mate::solve_together`]), and the time grows
//! with the number of groups, not with its square.
use oc_mate::*;

fn fixed(pose: Pose) -> Body {
    Body { pose, fixed: true }
}
fn floating(pose: Pose) -> Body {
    Body { pose, fixed: false }
}
fn at(t: V3) -> Pose {
    Pose::new([1.0, 0.0, 0.0, 0.0], t)
}
fn rot(axis: V3, deg: f64, t: V3) -> Pose {
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let (s, c) = (deg.to_radians() / 2.0).sin_cos();
    Pose::new([c, axis[0] / n * s, axis[1] / n * s, axis[2] / n * s], t)
}
fn b(i: usize, geom: Geom) -> Side {
    Side { owner: Owner::Body(i), geom }
}
fn ground(geom: Geom) -> Side {
    Side { owner: Owner::Ground, geom }
}
fn plane(p: V3, n: V3) -> Geom {
    Geom::Plane { p, n }
}
const BOTTOM: Geom = Geom::Plane { p: [5.0, 5.0, 0.0], n: [0.0, 0.0, -1.0] };
const LEFT: Geom = Geom::Plane { p: [0.0, 5.0, 5.0], n: [-1.0, 0.0, 0.0] };

/// Several independent sub-assemblies in one problem, with their mates interleaved:
/// - bodies 0 (fixed) and 1: a hinge, 1 DOF;
/// - body 2: a cube on the floor with a contradicting Distance (a conflict);
/// - bodies 3 (fixed) and 4: a pin locked to a base;
/// - bodies 5 and 6, both fixed, with a satisfied mate (redundant);
/// - body 7: no mates (6 DOF);
/// - body 8: a suppressed mate and an invalid pair;
/// - bodies 9, 10, 11: a chain hanging off fixed body 0 (shared with the hinge).
fn mixed() -> Problem {
    let hole = Geom::Cylinder { p: [20.0, 0.0, 0.0], d: [0.0, 0.0, 1.0], r: 3.0 };
    let pin = Geom::Cylinder { p: [0.0; 3], d: [0.0, 0.0, 1.0], r: 3.0 };
    let floor = ground(plane([0.0; 3], [0.0, 0.0, 1.0]));
    let mut dist = Mate::new(MateKind::Distance, floor, b(2, BOTTOM), Align::AntiAligned);
    dist.value = 10.0;
    let mut lock = Mate::new(MateKind::Lock, b(3, Geom::Point { p: [0.0; 3] }), b(4, Geom::Point { p: [0.0; 3] }), Align::Aligned);
    lock.relative = rot([0.0, 1.0, 1.0], 40.0, [10.0, 0.0, -5.0]);
    let mut off = Mate::new(MateKind::Coincident, floor, b(8, BOTTOM), Align::AntiAligned);
    off.enabled = false;
    let face = |z: f64, nz: f64| plane([0.0, 0.0, z], [0.0, 0.0, nz]);
    let chain_lock = |a: usize, c: usize, dx: f64| {
        let mut m = Mate::new(MateKind::Lock, b(a, Geom::Point { p: [0.0; 3] }), b(c, Geom::Point { p: [0.0; 3] }), Align::Aligned);
        m.relative = at([dx, 0.0, 0.0]);
        m
    };
    Problem {
        bodies: vec![
            fixed(Pose::IDENTITY),
            floating(rot([1.0, 0.5, 0.0], 20.0, [14.0, 3.0, 9.0])),
            floating(at([0.0, 0.0, 4.0])),
            fixed(rot([1.0, 0.0, 0.0], 70.0, [3.0, 3.0, 3.0])),
            floating(at([0.0; 3])),
            fixed(Pose::IDENTITY),
            fixed(at([0.0, 0.0, 5.0])),
            floating(at([50.0, 0.0, 0.0])),
            floating(at([0.0, 0.0, 4.0])),
            floating(at([1.0, 2.0, 3.0])),
            floating(rot([0.0, 0.0, 1.0], 30.0, [0.0, 0.0, 0.0])),
            floating(at([-4.0, 0.0, 1.0])),
        ],
        mates: vec![
            Mate::new(MateKind::Concentric, b(0, hole), b(1, pin), Align::Aligned),
            Mate::new(MateKind::Coincident, floor, b(2, BOTTOM), Align::AntiAligned),
            lock,
            chain_lock(0, 9, 100.0),
            Mate::new(MateKind::Coincident, b(5, face(5.0, 1.0)), b(6, face(0.0, -1.0)), Align::AntiAligned),
            Mate::new(MateKind::Coincident, b(0, plane([0.0, 0.0, 5.0], [0.0, 0.0, 1.0])), b(1, plane([0.0; 3], [0.0, 0.0, -1.0])), Align::AntiAligned),
            off,
            dist,
            chain_lock(9, 10, 10.0),
            Mate::new(MateKind::Concentric, floor, b(8, LEFT), Align::AntiAligned),
            chain_lock(10, 11, 10.0),
            Mate::new(MateKind::Coincident, b(1, plane([0.0, 0.0, 9.0], [0.0, 0.0, 1.0])), b(1, plane([0.0; 3], [0.0, 0.0, 1.0])), Align::Aligned),
        ],
    }
}

fn same(a: &Solved, w: &Solved, what: &str) {
    assert_eq!(a.mates, w.mates, "{what}: mate states");
    assert_eq!(a.bodies, w.bodies, "{what}: body states");
    assert_eq!((a.dof, a.converged, a.newest_error), (w.dof, w.converged, w.newest_error), "{what}");
    for (i, (p, q)) in a.poses.iter().zip(&w.poses).enumerate() {
        let dt = (0..3).map(|k| (p.t[k] - q.t[k]).abs()).fold(0.0, f64::max);
        let dq = (0..4).map(|k| (p.q[k] - q.q[k]).abs()).fold(0.0, f64::max);
        // Within the solvers' convergence (the whole problem stops when its total residual is
        // small enough, a group when its own is: 1e-9 mm apart was seen); mates.rs' tolerance.
        assert!(dt < 1e-7 && dq < 1e-7, "{what}: body {i}: {p:?} against {q:?}");
    }
}

#[test]
fn groups_solve_like_the_whole() {
    let p = mixed();
    let g = solve(&p);
    let w = solve_together(&p);
    same(&g, &w, "mixed");
    // What the cases are, so the comparison means something.
    assert_eq!(g.mates[7], MateState::Conflict);
    assert_eq!(g.mates[4], MateState::Redundant);
    assert_eq!(g.mates[6], MateState::Off);
    assert!(matches!(g.mates[9], MateState::Invalid(_)));
    assert!(matches!(g.mates[11], MateState::Invalid(_)), "both sides on one body");
    assert_eq!(g.bodies[7], BodyInfo { state: BodyState::Under, free: 6 });
    assert_eq!(g.bodies[1], BodyInfo { state: BodyState::Under, free: 1 });
    assert_eq!(g.bodies[11].state, BodyState::Full);
    assert_eq!(g.newest_error, Some(7), "the newest conflict (invalid pairs are not counted)");
    // Every order of the mates gives the same as the whole.
    for k in 0..p.mates.len() {
        let mut q = p.clone();
        q.mates.rotate_left(k);
        same(&solve(&q), &solve_together(&q), &format!("mates rotated by {k}"));
    }
}

#[test]
fn drags_move_only_the_dragged_group_as_the_whole_does() {
    let p = mixed();
    for (body, grab, target) in [(1usize, [0.0, 10.0, 0.0], [20.0, 10.0, 5.0]), (7, [0.0; 3], [60.0, 5.0, 0.0]), (11, [0.0; 3], [0.0, 50.0, 0.0]), (0, [0.0; 3], [1.0; 3])] {
        let g = drag(&p, body, grab, target, 20.0);
        let w = drag_together(&p, body, grab, target, 20.0);
        same(&g, &w, &format!("drag of {body}"));
    }
}

/// `n` floating bodies, each locked to one fixed base: `n` groups of one body.
fn locked(n: usize) -> Problem {
    let mut p = Problem { bodies: vec![fixed(Pose::IDENTITY)], mates: vec![] };
    for i in 0..n {
        p.bodies.push(floating(at([i as f64, 0.0, 0.0])));
        let mut m = Mate::new(MateKind::Lock, b(0, Geom::Point { p: [0.0; 3] }), b(i + 1, Geom::Point { p: [0.0; 3] }), Align::Aligned);
        m.relative = at([i as f64, 1.0, 0.0]);
        p.mates.push(m);
    }
    p
}

#[test]
fn many_groups_take_linear_time() {
    let time = |n: usize| {
        let p = locked(n);
        let t = std::time::Instant::now();
        let s = solve(&p);
        let d = drag(&p, n, [0.0; 3], [5.0, 5.0, 5.0], 10.0);
        assert!(s.converged && s.dof == 0 && s.mates.iter().all(|m| *m == MateState::Ok));
        assert_eq!(d.dof, 0);
        t.elapsed().as_secs_f64()
    };
    let _ = time(100);
    let (small, large) = (time(500), time(2000));
    // Linear: four times the bodies, about four times the time; quadratic would be sixteen.
    assert!(large < small * 8.0 + 0.05, "500 bodies {small:.3} s, 2000 bodies {large:.3} s");
}
