//! OpenCAD 2D sketch constraint solver (work package E2, `docs/OVERHAUL_PLAN.md`).
//!
//! A port of `src/sketch/solver.ts` that keeps its behaviour:
//! - minimum-norm damped Gauss-Newton, so under-defined geometry moves as little as possible;
//! - constrained dragging by null-space projection, so a dragged point follows the cursor as
//!   far as the relations allow;
//! - rank analysis of the row-reduced Jacobian for degrees of freedom, black/blue state and
//!   the newest redundant or conflicting relation.
//!
//! Additions: exact Jacobians by forward-mode dual numbers ([`real`]), decomposition into
//! independent components ([`system`]) and sparse factorisation of `J Jᵀ` ([`sparse`]).
//!
//! ```
//! use oc_sketch::{solve_sketch, SketchData, Relation, Status};
//! let mut sk = SketchData::empty();
//! sk.add_point("a", 0.0, 0.0);
//! sk.add_point("b", 10.0, 1.0);
//! sk.add_line("l", "a", "b");
//! sk.add_relation(Relation::Horizontal { id: "h".into(), e: "l".into() });
//! let a = solve_sketch(&mut sk);
//! assert_eq!(a.status, Status::Under);
//! assert!((sk.points["a"].y - sk.points["b"].y).abs() < 1e-9);
//! ```

pub mod analysis;
pub mod build;
pub mod equations;
pub mod generators;
pub mod geom;
pub mod model;
pub mod real;
mod solve;
pub mod sparse;
pub mod system;

pub use analysis::{Analysis, DefState, Status};
pub use build::ParamOwner;
pub use equations::{Pt, Rad};
pub use model::{
    Dimension, DimensionKind, Id, Record, Relation, SketchData, SketchEntity, SketchPoint, Vec2,
    ORIGIN_ID,
};
pub use solve::{DRAG_STEPS, MAX_ITER, TOL};
pub use system::SketchSystem;

/// Solves a sketch in place and analyses it (`solveSketch`).
pub fn solve_sketch(sk: &mut SketchData) -> Analysis {
    let mut sys = SketchSystem::new(sk);
    let ok = sys.solve_default();
    sys.commit(sk);
    sys.analyse(ok)
}
