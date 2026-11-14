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
//! - [`ops`]: kernel operations with names. Every body a feature makes has a persistent
//!   name per face, derived by `oc-naming` from the operation's history
//!   (`docs/OVERHAUL_PLAN.md` section 3).
//! - [`resolve`]: finding stored face, edge and vertex references by name, with the
//!   position and legacy fallbacks of section 3.3, and writing the names found for legacy
//!   references back into the document ([`Rebuild::heals`], [`apply_heals`]).
//!
//! No kernel types appear here: geometry goes in and out through the kernel trait.

pub mod eval;
pub mod features;
pub mod graph;
pub mod math;
pub mod ops;
pub mod refgeom;
pub mod regen;
pub mod resolve;
pub mod sketch;

pub use eval::{AxisDef, Dressup, FeatureError, Instance, NodeOut, Shape, Tool, FAILED, INVALID};
pub use regen::{CancelToken, Cancelled, Rebuild, Regenerator, Status, Step};
pub use resolve::{apply_heals, Heal};
