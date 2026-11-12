//! OpenCAD document model (work package E1, `docs/OVERHAUL_PLAN.md`).
//!
//! A typed mirror of the document part of `src/core/types.ts` (`PartDocument` and
//! everything it contains; the kernel output types belong to the kernel and graph
//! crates). Field names, optionality and `null`-ability follow the TypeScript types, so
//! any document the TypeScript app saves loads here, and saves back to the same
//! canonical bytes.
//!
//! - [`load`] / [`save`]: strict, lossless JSON I/O with schema migrations ([`io`]).
//! - [`to_canonical_string`]: RFC 8785 canonical JSON, for comparing and hashing.
//! - [`History`]: transactions and undo/redo by structural sharing.
//!
//! Units: millimetres and radians.

mod canon;
mod document;
mod feature;
mod history;
mod id;
pub mod io;
mod migrate;
mod nullable;
mod refs;
mod sketch;

pub use canon::{to_canonical_string, to_pretty_string, CanonError};
pub use document::*;
pub use feature::*;
pub use history::{History, Transaction};
pub use id::Id;
pub use io::{canonical, canonicalize_json, load, save, LoadError, Loaded};
pub use migrate::{Migration, MigrationError, Migrations, CURRENT_VERSION, MIGRATIONS};
pub use refs::*;
pub use sketch::*;

/// A point in sketch coordinates (mm).
pub type Vec2 = [f64; 2];
/// A point or direction in model coordinates (mm).
pub type Vec3 = [f64; 3];

/// The persistent collections the model is built from.
pub use imbl::{OrdMap, Vector};
