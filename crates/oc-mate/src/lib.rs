//! OpenCAD assembly mate solver (work package G1, `docs/OVERHAUL_PLAN.md` section 4:
//! "Assembly mates reuse the same core in 3D with rigid-body parameters").
//!
//! Each floating component is a rigid body with six parameters: a translation and a
//! rotation increment on top of its pose, re-based after every step so no angle
//! convention (and no gimbal lock) exists ([`geom::increment`]). Mates are residual
//! equations between geometry of two bodies (or a body and the fixed assembly), written
//! once over `oc_sketch::real::Real`, so Jacobians are exact (dual numbers), as in the
//! sketch solver. The core is the sketch solver's:
//! - **solve**: minimum-norm damped Gauss-Newton (`oc_sketch::sparse::min_norm_step`), so
//!   under-defined components move as little as possible;
//! - **drag**: the drag projected onto the null space of the mate Jacobian, so a component
//!   moves only within its free degrees of freedom ([`drag`]);
//! - **analysis**: rank of the row-normalised Jacobian by the sketch solver's RREF
//!   (`oc_sketch::analysis::rref`) in mate order: degrees of freedom, fully defined bodies,
//!   redundant mates (the newest one flagged).
//!
//! A mate that cannot be satisfied together with the mates before it is a conflict: the
//! mates are then added one at a time in order and each one that breaks the solve is set
//! aside, so the newest conflicting mate is the one reported and the rest stay solved.
//!
//! Units: millimetres and radians.

pub mod geom;
mod analysis;
mod drag;
mod rows;
mod system;

pub use geom::{Geom, Pose, V3};
pub use rows::Mode;
pub use system::{ACCEPT, MAX_ITER, TOL};

use rows::{build, Spec};
use system::{Block, Sys};

/// Who owns one side of a mate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// The assembly itself (its planes and origin): never moves.
    Ground,
    /// Body `i` of [`Problem::bodies`].
    Body(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MateKind {
    Coincident,
    Parallel,
    Perpendicular,
    Tangent,
    Concentric,
    Lock,
    Distance,
    Angle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Aligned,
    AntiAligned,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub pose: Pose,
    pub fixed: bool,
}

/// One side: owner and geometry in the owner's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Side {
    pub owner: Owner,
    pub geom: Geom,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mate {
    pub kind: MateKind,
    pub a: Side,
    pub b: Side,
    pub align: Align,
    /// Distance (mm) or angle (radians).
    pub value: f64,
    pub flip: bool,
    /// Lock: `b`'s pose in `a`'s coordinates.
    pub relative: Pose,
    /// Suppressed or with missing references: not solved, not analysed.
    pub enabled: bool,
}

impl Mate {
    pub fn new(kind: MateKind, a: Side, b: Side, align: Align) -> Mate {
        Mate { kind, a, b, align, value: 0.0, flip: false, relative: Pose::IDENTITY, enabled: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Problem {
    pub bodies: Vec<Body>,
    pub mates: Vec<Mate>,
}

/// State of a mate after solving.
#[derive(Clone, Debug, PartialEq)]
pub enum MateState {
    Ok,
    /// Not enabled (suppressed, or a reference is missing).
    Off,
    /// Satisfied, but adds nothing the mates before it did not already fix: over-defining
    /// (SOLIDWORKS reports these as over defined; 🔍 whether it tolerates some).
    Redundant,
    /// Cannot be satisfied together with the mates before it; set aside.
    Conflict,
    /// The geometry pair cannot be mated this way.
    Invalid(String),
}

impl MateState {
    pub fn is_error(&self) -> bool {
        matches!(self, MateState::Redundant | MateState::Conflict | MateState::Invalid(_))
    }
}

/// Definition state of a body: SOLIDWORKS' (f), (-), no prefix, (+).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyState {
    Fixed,
    Under,
    Full,
    Over,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BodyInfo {
    pub state: BodyState,
    /// Degrees of freedom left to this body with every other body held (0 to 6).
    pub free: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Solved {
    pub poses: Vec<Pose>,
    /// All enabled, valid, non-conflicting mates are satisfied.
    pub converged: bool,
    /// Degrees of freedom of the whole assembly (floating bodies × 6 − rank).
    pub dof: usize,
    pub bodies: Vec<BodyInfo>,
    pub mates: Vec<MateState>,
    /// The newest mate in conflict or redundant, as the sketch solver reports the newest
    /// conflicting relation.
    pub newest_error: Option<usize>,
}

/// Builds the system; mates that cannot be built are marked invalid.
fn system(p: &Problem) -> (Sys, Vec<MateState>) {
    let mut states = vec![MateState::Ok; p.mates.len()];
    let mut blocks = Vec::new();
    for (i, m) in p.mates.iter().enumerate() {
        if !m.enabled {
            states[i] = MateState::Off;
            continue;
        }
        let bad_owner = |o: Owner| matches!(o, Owner::Body(b) if b >= p.bodies.len());
        if bad_owner(m.a.owner) || bad_owner(m.b.owner) {
            states[i] = MateState::Invalid("refers to a missing component".into());
            continue;
        }
        if m.a.owner == m.b.owner {
            states[i] = MateState::Invalid("both references are on the same component".into());
            continue;
        }
        let spec = Spec { kind: m.kind, ga: &m.a.geom, gb: &m.b.geom, align: m.align, value: m.value, flip: m.flip, relative: m.relative };
        match build(&spec) {
            Ok((rule, swap)) => {
                let (a, b) = if swap { (m.b, m.a) } else { (m.a, m.b) };
                blocks.push(Block { mate: i, rule, a: a.owner, b: b.owner, ga: a.geom, gb: b.geom });
            }
            Err(e) => states[i] = MateState::Invalid(e),
        }
    }
    let fixed: Vec<bool> = p.bodies.iter().map(|b| b.fixed).collect();
    let poses = p.bodies.iter().map(|b| Pose::new(b.pose.q, b.pose.t)).collect();
    (Sys::new(poses, &fixed, blocks), states)
}

/// Solves the mates from the bodies' current poses and analyses the result.
pub fn solve(p: &Problem) -> Solved {
    let (mut sys, states) = system(p);
    let active = solve_active(&mut sys, &states);
    finish(p, sys, states, active)
}

/// Solves; on failure adds mates one at a time in order and sets aside each that breaks
/// the solve. Returns the active set.
fn solve_active(sys: &mut Sys, states: &[MateState]) -> Vec<bool> {
    let wanted: Vec<bool> = states.iter().map(|s| *s == MateState::Ok).collect();
    let start = sys.poses.clone();
    if sys.solve(&wanted, MAX_ITER) {
        return wanted;
    }
    sys.poses = start;
    let mut active = vec![false; states.len()];
    for m in 0..states.len() {
        if !wanted[m] {
            continue;
        }
        active[m] = true;
        let keep = sys.poses.clone();
        if !sys.solve(&active, MAX_ITER) {
            active[m] = false;
            sys.poses = keep;
        }
    }
    active
}

fn finish(p: &Problem, sys: Sys, mut states: Vec<MateState>, active: Vec<bool>) -> Solved {
    let ranks = analysis::analyse(&sys, &active);
    for (m, s) in states.iter_mut().enumerate() {
        if *s == MateState::Ok && !active[m] {
            *s = MateState::Conflict;
        }
    }
    for &m in &ranks.redundant {
        if states[m] == MateState::Ok {
            states[m] = MateState::Redundant;
        }
    }
    let mut bodies: Vec<BodyInfo> = p
        .bodies
        .iter()
        .enumerate()
        .map(|(i, b)| BodyInfo {
            state: if b.fixed {
                BodyState::Fixed
            } else if ranks.determined[i] {
                BodyState::Full
            } else {
                BodyState::Under
            },
            free: ranks.free[i],
        })
        .collect();
    for (m, s) in states.iter().enumerate() {
        if matches!(s, MateState::Redundant | MateState::Conflict) {
            for o in [p.mates[m].a.owner, p.mates[m].b.owner] {
                if let Owner::Body(b) = o {
                    if bodies[b].state != BodyState::Fixed {
                        bodies[b].state = BodyState::Over;
                    }
                }
            }
        }
    }
    let newest_error = (0..states.len()).rev().find(|&m| matches!(states[m], MateState::Redundant | MateState::Conflict));
    let zero = vec![0.0; sys.n];
    let residual = system::norm2(&sys.residuals(&zero, Mode::Solve, &active)).sqrt();
    Solved { poses: sys.poses, converged: residual < ACCEPT, dof: ranks.dof, bodies, mates: states, newest_error }
}

/// Drags body `body` so that its point `grab` (body coordinates) moves towards `target`
/// (world), within the freedom the mates leave; connected under-defined bodies follow.
/// Rotation is weighted as `lever` millimetres per radian, so a body free to translate
/// translates rather than turns (SOLIDWORKS' drag), and a hinged one turns.
pub fn drag(p: &Problem, body: usize, grab: V3, target: V3, lever: f64) -> Solved {
    let (mut sys, states) = system(p);
    let active = solve_active(&mut sys, &states);
    if body < p.bodies.len() && !p.bodies[body].fixed {
        drag::drag(&mut sys, &active, body, grab, target, lever.max(1.0));
    }
    finish(p, sys, states, active)
}
