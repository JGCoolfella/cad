//! The `cxx` bridge to the C++ shim (`cpp/`). Plain data crosses as shared structs; OCCT
//! shapes stay behind the opaque `Body`. Every fallible call returns `Result`: C++
//! exceptions, including OCCT's `Standard_Failure`, arrive as `Err` whose message starts
//! with a category (`input|`, `failed|`, `invalid|`, `empty|`, `io|`) that
//! `convert::error` maps onto `KernelError`.

#[cxx::bridge(namespace = "occ")]
pub mod bridge {
    /// `oc_kernel::Tolerances`, field for field.
    #[derive(Clone, Copy, Debug)]
    struct Tol {
        confusion: f64,
        angular: f64,
        fuzzy: f64,
        parallel: bool,
        unify_angular: f64,
        offset: f64,
        loft: f64,
        spline_fit: f64,
        heal: f64,
        heal_acceptance: f64,
    }

    /// A profile segment. `kind`: 0 line (a, b), 1 arc (a, mid, b), 2 circle (centre,
    /// normal, radius), 3 ellipse (centre, normal, major dir, major, minor), 4 spline
    /// (points). `v` holds the numbers in that order.
    struct SegIn {
        kind: u8,
        v: Vec<f64>,
    }

    struct WireIn {
        segs: Vec<SegIn>,
    }

    struct RegionIn {
        outer: WireIn,
        holes: Vec<WireIn>,
    }

    /// One history record. `src`: 0 face, 1 edge, 2 vertex (of input `arg`, index `a`);
    /// 3 profile segment (region `a`, wire `b`, segment `c`); 4 cap (region `a`, `b` = 0
    /// start / 1 end). Image lists hold 0-based indices into the result's faces (or
    /// edges, for a section).
    struct FateOut {
        src: u8,
        arg: u32,
        a: u32,
        b: u32,
        c: u32,
        deleted: bool,
        same: Vec<u32>,
        modified: Vec<u32>,
        generated: Vec<u32>,
    }

    struct OpOut {
        body: UniquePtr<Body>,
        fates: Vec<FateOut>,
        /// 0 faces, 1 edges.
        image_kind: u8,
        healed: bool,
    }

    /// `surface`: 0 plane, 1 cylinder, 2 cone, 3 sphere, 4 torus, 5 B-spline, 6 Bezier,
    /// 7 revolution, 8 extrusion, 9 offset, 10 other. `params` (analytic only):
    /// plane (origin, normal); cylinder (origin, dir, r); cone (origin, dir, r, half
    /// angle); sphere (centre, r); torus (origin, dir, R, r).
    struct FaceOut {
        surface: u8,
        params: Vec<f64>,
        area: f64,
        centroid: Vec<f64>,
        normal: Vec<f64>,
        bbox: Vec<f64>,
        edges: Vec<u32>,
    }

    /// `curve`: 0 line, 1 circle (centre, normal, r), 2 ellipse (centre, normal, major
    /// dir, major, minor), 3 B-spline, 4 Bezier, 5 other. `pts` = start, end, mid.
    struct EdgeOut {
        curve: u8,
        params: Vec<f64>,
        pts: Vec<f64>,
        length: f64,
        faces: Vec<u32>,
        seam: bool,
        vertices: Vec<u32>,
    }

    struct TopoOut {
        solids: u32,
        shells: u32,
        faces: Vec<FaceOut>,
        edges: Vec<EdgeOut>,
        vertices: Vec<f64>,
    }

    struct MassOut {
        volume: f64,
        area: f64,
        center: Vec<f64>,
        inertia: Vec<f64>,
        moments: Vec<f64>,
        axes: Vec<f64>,
    }

    struct DistOut {
        distance: f64,
        points: Vec<f64>,
    }

    /// `kind`: 0 face, 1 edge, 2 vertex, 3 none (whole body).
    struct ProblemOut {
        kind: u8,
        index: u32,
        code: String,
    }

    struct CheckOut {
        valid: bool,
        problems: Vec<ProblemOut>,
    }

    /// `face_ends[i]`: end of face i in `triangles` (in triangles); `edge_ends[i]`: end
    /// of edge i's polyline in `edge_points` (in points).
    struct MeshOut {
        positions: Vec<f64>,
        normals: Vec<f32>,
        triangles: Vec<u32>,
        face_ends: Vec<u32>,
        edge_points: Vec<f64>,
        edge_ends: Vec<u32>,
    }

    unsafe extern "C++" {
        include!("oc-kernel-occt/cpp/shim.h");

        type Body;
        type BodyList;

        fn body_list() -> UniquePtr<BodyList>;
        fn body_list_push(list: Pin<&mut BodyList>, body: &Body);
        fn clone_body(body: &Body) -> UniquePtr<Body>;
        fn make_point(x: f64, y: f64, z: f64) -> UniquePtr<Body>;
        /// Straight edge from `ab[0..3]` to `ab[3..6]` (a distance target).
        fn make_segment(ab: &[f64]) -> Result<UniquePtr<Body>>;
        /// Faces, edges, vertices.
        fn body_counts(body: &Body) -> [u32; 3];
        fn solid_count(body: &Body) -> u32;
        /// Solid `i` of the body (solids ordered by their lowest face index), with the
        /// body's faces as history: `same` if in that solid, deleted otherwise.
        fn solid(tol: &Tol, body: &Body, i: u32) -> Result<OpOut>;

        fn primitive(tol: &Tol, kind: u8, v: &[f64]) -> Result<OpOut>;
        fn prism(tol: &Tol, regions: &[RegionIn], vector: &[f64], taper: f64, outward: bool) -> Result<OpOut>;
        fn revolve(tol: &Tol, regions: &[RegionIn], axis: &[f64], angle: f64) -> Result<OpOut>;
        fn sweep(tol: &Tol, regions: &[RegionIn], path: &WireIn, frenet: bool) -> Result<OpOut>;
        fn loft(tol: &Tol, sections: &[WireIn], ruled: bool) -> Result<OpOut>;

        fn boolean(tol: &Tol, op: u8, target: &Body, tools: &BodyList) -> Result<OpOut>;
        fn fillet(tol: &Tol, body: &Body, edges: &[u32], r1: &[f64], r2: &[f64]) -> Result<OpOut>;
        fn chamfer(tol: &Tol, body: &Body, edges: &[u32], kinds: &[u8], d1: &[f64], d2: &[f64], faces: &[u32]) -> Result<OpOut>;
        fn shell(tol: &Tol, body: &Body, faces: &[u32], offset: f64) -> Result<OpOut>;
        fn draft(tol: &Tol, body: &Body, faces: &[u32], plane: &[f64], angle: f64) -> Result<OpOut>;
        fn offset(tol: &Tol, body: &Body, distance: f64) -> Result<OpOut>;
        fn transform(tol: &Tol, body: &Body, m: &[f64]) -> Result<OpOut>;
        fn compound(tol: &Tol, bodies: &BodyList) -> Result<OpOut>;
        fn section(tol: &Tol, body: &Body, plane: &[f64]) -> Result<OpOut>;
        fn heal(tol: &Tol, body: &Body) -> Result<OpOut>;

        fn topology(body: &Body) -> Result<TopoOut>;
        fn bbox(body: &Body) -> Result<[f64; 6]>;
        fn mass(body: &Body) -> Result<MassOut>;
        /// Kinds as in `ProblemOut`.
        fn distance(a: &Body, ka: u8, ia: u32, b: &Body, kb: u8, ib: u32) -> Result<DistOut>;
        fn check(tol: &Tol, body: &Body) -> Result<CheckOut>;
        fn tessellate(body: &Body, linear: f64, angular: f64) -> Result<MeshOut>;

        fn read_step(tol: &Tol, data: &[u8]) -> Result<OpOut>;
        fn write_step(bodies: &BodyList) -> Result<Vec<u8>>;
    }
}

pub use bridge::*;
