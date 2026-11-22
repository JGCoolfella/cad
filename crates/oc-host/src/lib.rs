//! Geometry hosts (OVERHAUL_PLAN G3b; section 1 "Kernel isolation"): parts regenerated in
//! child processes, so a crash or a hang in the kernel fails that part with an error and
//! the app survives.
//!
//! - [`proto`]: the frames exchanged over the host's standard input and its reply pipe. A job is a
//!   part document; a reply is G3a's `PartOut`: the body as the kernel's native blob with its
//!   persistent names, reference geometry, errors and display mesh.
//! - [`serve`]: the host's loop (`opencad --geometry-host`, or the `oc-host` binary in tests).
//! - [`Pool`]: the app's side, an `oc_asm::parallel::Runner`: hosts started on the first
//!   batch and kept, one part per host at a time; a host that dies, times out or breaks the
//!   protocol is killed and started again, and its part gets the error ([`pool::DIED`]).
pub mod pool;
pub mod proto;
pub mod serve;
#[cfg(windows)]
pub mod win;

pub use pool::{HostCommand, HostStats, Pool};
pub use serve::serve;
