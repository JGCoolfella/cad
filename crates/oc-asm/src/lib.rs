//! OpenCAD assemblies (work package G1, `docs/OVERHAUL_PLAN.md`).
//!
//! - [`locate`]: finding component files (relative path, then a fallback search).
//! - [`Assembler`]: building an assembly on one kernel. Every component is read; each
//!   distinct part (by content hash, [`cache::part_hash`]) is regenerated once by
//!   `oc-graph` and meshed once by `oc-mesh`, however many instances use it and whichever
//!   file it came from; instances are placements of that one mesh (`oc-render` draws
//!   instances as transforms). Mate references are found on their parts by persistent name
//!   (`oc-naming`, section 3.3), so mates follow part edits; one that no longer resolves
//!   is Missing. The mates are solved by `oc-mate`.
//! - [`cache`]: the `.ocasm` container's caches. A part whose hash matches its cached mesh
//!   is shown from the cache without being regenerated (lightweight), and its mates use
//!   the geometry stored with each reference.
//! - [`evaluate`]: Interference Detection, and occurrence bodies at their placements.
//! - [`scale`]: a generated large assembly (10 000 components of 100 parts) for scale tests.

pub mod build;
pub mod cache;
pub mod evaluate;
pub mod geom;
pub mod locate;
pub mod outofcore;
pub mod parallel;
pub mod scale;

pub use build::{apply_heals, apply_placements, drag, place, Assembler, BuildStats, Built, ComponentOut, InstanceOut, InstanceState, MateHeal, MateOut, MateStatus, Occurrence, Options, Part};
pub use cache::{part_hash, Cache};
pub use evaluate::{Interference, InterferenceOptions, InterferenceReport};
pub use locate::{Disk, Found, Memory, Store};
