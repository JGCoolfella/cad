//! OpenCAD's engine without a shell (OVERHAUL_PLAN W1b): what was `oc-app`'s engine thread.
//!
//! - [`engine`]: the jobs the webview's commands become (`engine_*`, `asm_*`), served one at a
//!   time on one thread that owns the OCCT kernel and the regenerator; display meshes go to
//!   the renderer through the [`engine::MeshStore`].
//! - [`asm`]: assemblies (open, build, drag, evaluate, save).
//! - [`pack`]: the binary reply layout (`OCR1`); [`bodymesh`]: a body as the webview's `BodyMesh`.
//!
//! The desktop app (`oc-app`) runs it on a thread behind Tauri's commands.
pub mod asm;
pub mod bodymesh;
pub mod engine;
pub mod pack;
