//! Persistent topological names (work package E6, `docs/OVERHAUL_PLAN.md` section 3).
//!
//! A reference in a document (a sketch's face, a fillet's edges) must survive edits to
//! the features before it. Indices cannot do that: the kernel renumbers faces whenever
//! the body changes. Positions cannot either: a taller pad moves its top face. What does
//! not change is where a face *came from*: "the face Boss-Extrude1 swept from sketch line
//! l3", "the cap of Cut-Extrude2's profile", "the fillet on the edge between those two".
//!
//! - [`name`]: [`Tag`] (one origin), [`FaceName`] (a face's tags; split pieces share
//!   them, merged faces union them), [`EdgeName`] and [`VertexName`] (the faces that meet
//!   there), [`BodyNames`] (a name per face of a body).
//! - [`track`](mod@track): names of an operation's result from the history every
//!   [`oc_kernel::Kernel`] operation returns.
//! - [`resolve`]: finding a stored reference (name + geometric signature) on the live
//!   body, with the position and legacy fallbacks of section 3.3.
//! - [`signature`]: the geometric signatures references carry.
//! - [`hash`]: the fixed 64-bit hash behind every key.
//!
//! Determinism (section 3.4): no tag depends on a face index, a pointer, hash-map order
//! or thread scheduling. The same document gives the same names on every run.

pub mod hash;
pub mod name;
pub mod resolve;
pub mod signature;
pub mod track;

pub use name::{vertex_faces, BodyNames, EdgeName, FaceName, Tag, VertexName};
pub use resolve::{Found, How, Live, Rules};
pub use track::{import_names, track, Births, Identity, Input, NoTopo, Topo, Tracked};
