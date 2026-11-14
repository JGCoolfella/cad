//! `oc-render`: the wgpu viewport renderer, phase 1 (`docs/OVERHAUL_PLAN.md` F1).
//!
//! It draws what the current three.js viewport (`src/render/viewport.ts`) draws, from
//! [`oc_mesh::RenderMesh`] buffers uploaded as they are:
//!
//! - display styles: Shaded With Edges, Shaded, Hidden Lines Removed, Hidden Lines Visible
//!   (dashed or solid grey hidden edges), Wireframe; transparent bodies; the feature
//!   preview body;
//! - highlights: hovered and selected faces (tinted), edges (3 px, coloured) and vertices
//!   (8 px markers), fillet/chamfer preview faces;
//! - reference planes (two-sided translucent fill, border by state, in-plane name),
//!   axes (dashed), points, coordinate systems, inactive sketches, the part origin;
//! - the reference triad; Section View with a stencil-capped cut; the viewport background;
//! - ID-buffer picking with the current priority rules ([`pick`]).
//!
//! The renderer draws a [`Scene`] (a plain description rebuilt from app state) with a
//! [`Camera`] (port of `camera.ts`, all `f64`). GPU positions are relative to the camera
//! target; depth is reversed into a float buffer. Rendering is on demand: nothing happens
//! between calls.
//!
//! What stays out (F2 and later): the 2D canvas overlay of the sketch editor, dimensions,
//! measure line, selection box and tooltips (the webview draws them over the viewport),
//! the 3D View Selector cube, text rasterisation (plane labels arrive as images).
#![doc = ""]

mod batch;
pub mod camera;
pub mod color;
mod frame;
mod gpu_pick;
mod mesh;
mod overlay;
pub mod pick;
mod pipelines;
pub mod planes;
mod raycast;
mod renderer;
pub mod scene;

pub use camera::{Aabb, Camera, Ray};
pub use color::Rgb;
pub use gpu_pick::PendingPick;
pub use pick::{PickFilter, PickKind, PICK};
pub use raycast::RayHit;
pub use renderer::{Image, RenderError, Renderer, Viewport};
pub use scene::*;
