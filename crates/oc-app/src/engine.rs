//! The engine thread: in `oc-engine` since W1b (shared with the browser's engine worker).
//! The app starts it with its stuck-state diagnostics (`crate::diag`).
pub use oc_engine::engine::*;

/// Starts the engine thread (see [`oc_engine::engine::spawn`]).
pub fn spawn(store: std::sync::Arc<MeshStore>) -> std::sync::mpsc::Sender<Job> {
    oc_engine::engine::spawn(store, crate::diag::engine)
}
