//! `oc-view`: the viewport's frame logic, without a window or a thread of its own
//! (`docs/OVERHAUL_PLAN.md` W1c). Moved out of `oc-app` so the browser's render worker
//! (`oc-web-render`) draws and picks exactly as the desktop app's render thread does.
//!
//! - [`scene_dto`]: the scene and camera as the webview sends them (`src/native/view.ts`)
//!   and their conversion to `oc-render`'s [`oc_render::Scene`] and [`oc_render::Camera`];
//! - [`pick`]: picks and box selections against the scene of a frame;
//! - [`view`]: [`View`], the newest frame, its meshes uploaded, drawn into an off-screen
//!   target the size of the graphics area, and the queries waiting for their frame.
//!
//! The caller owns the surface (a Tauri window's swapchain, a browser `OffscreenCanvas`),
//! presents the target, and delivers the replies.
pub mod pick;
pub mod scene_dto;
pub mod view;

pub use view::{FrameStats, Hooks, Meshes, Rect, Reply, Rgba, View};
