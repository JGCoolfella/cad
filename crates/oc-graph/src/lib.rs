//! OpenCAD feature graph (work package E5, `docs/OVERHAUL_PLAN.md`).
//!
//! Regenerates the feature tree of an `oc-doc` [`oc_doc::PartDocument`] on any
//! [`oc_kernel::Kernel`]:
//! - [`graph`]: the feature DAG. Edges come from stored references plus the body chain;
//!   tree order is a topological order.
//! - [`Regenerator`]: evaluation with a cache keyed by each node's inputs (so editing a
//!   feature recomputes only what reads it), cancellation between nodes
//!   ([`CancelToken`]) and per-node timings ([`Step`]).
//! - [`features`]: every feature type the app has, built the way `src/kernel/engine.ts`
//!   builds it, with the same error and warning texts.
//! - [`resolve`]: re-finding stored face and edge references (by signature until
//!   persistent naming, E6, replaces the [`resolve::Resolver`]).
//!
//! No kernel types appear here: geometry goes in and out through the kernel trait.

pub mod eval;
pub mod features;
pub mod math;
pub mod refgeom;
pub mod resolve;
pub mod sketch;

pub use eval::{AxisDef, Dressup, FeatureError, Instance, NodeOut, Shape, Tool, FAILED, INVALID};
