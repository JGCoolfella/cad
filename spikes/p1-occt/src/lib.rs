//! P1 spike: native OCCT behind a `cxx` shim, with the operation history the persistent
//! naming design (plan section 3) needs: for every input face, whether it was kept,
//! modified (into which result faces) or deleted, and which faces each edge generated.

#[cxx::bridge(namespace = "oc")]
pub mod ffi {
    /// What happened to one face (kind 0) or edge (kind 1) of an operation input.
    #[derive(Debug, Clone)]
    struct Fate {
        /// 0 = first argument (the body), 1 = tool.
        input: u8,
        kind: u8,
        /// 0-based index in the input's face or edge map.
        index: u32,
        deleted: bool,
        /// Result face index if the face passed through unchanged, else -1.
        kept: i32,
        modified: Vec<u32>,
        generated: Vec<u32>,
    }

    #[derive(Debug, Clone, Copy)]
    struct Metrics {
        volume: f64,
        area: f64,
        faces: u32,
        edges: u32,
        valid: bool,
    }

    /// `surface` is GeomAbs_SurfaceType: 0 plane, 1 cylinder, 2 cone, 3 sphere, 4 torus,
    /// 5 Bezier, 6 B-spline, 7 revolution, 8 extrusion, 9 offset, 10 other.
    #[derive(Debug, Clone, Copy)]
    struct FaceInfo {
        surface: i32,
        area: f64,
        cx: f64,
        cy: f64,
        cz: f64,
    }

    /// One indexed triangle mesh for a whole shape; `face_ends[i]` is the end offset of
    /// face i in `indices` (the ID-picking face ranges).
    #[derive(Debug, Clone, Default)]
    struct MeshOut {
        positions: Vec<f32>,
        normals: Vec<f32>,
        indices: Vec<u32>,
        face_ends: Vec<u32>,
    }

    unsafe extern "C++" {
        include!("p1-occt/cpp/shim.h");

        type Shape;
        type Op;

        fn make_box(dx: f64, dy: f64, dz: f64) -> UniquePtr<Shape>;
        fn make_cylinder(x: f64, y: f64, z: f64, r: f64, h: f64) -> UniquePtr<Shape>;
        fn make_sphere(r: f64) -> UniquePtr<Shape>;
        fn tessellate(s: &Shape, deflection: f64) -> Result<MeshOut>;
        fn cut(a: &Shape, b: &Shape) -> Result<UniquePtr<Op>>;
        fn fillet(a: &Shape, edges: &[u32], r: f64) -> Result<UniquePtr<Op>>;
        fn op_result(op: &Op) -> UniquePtr<Shape>;
        fn op_history(op: &Op) -> Vec<Fate>;
        fn metrics(s: &Shape) -> Metrics;
        fn faces(s: &Shape) -> Vec<FaceInfo>;
        fn edges_between(s: &Shape, fa: u32, fb: u32) -> Vec<u32>;
    }
}

// The engine runs every kernel call on one geometry-host thread; OCCT shapes are
// reference-counted without atomics, so they must not be shared between threads.
pub use ffi::*;

pub const PLANE: i32 = 0;
pub const CYLINDER: i32 = 1;
pub const TORUS: i32 = 4;
