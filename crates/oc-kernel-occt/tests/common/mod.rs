//! Shared test helpers: the kernel, profile shorthands, corpus oracle access and
//! topology lookups by geometry (the way the corpus builder picks faces and edges).
#![allow(dead_code)]

use oc_kernel::*;
use oc_kernel_occt::{Body, OcctKernel};
use std::f64::consts::PI;

pub fn kernel() -> OcctKernel {
    OcctKernel::default()
}

pub fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Wire {
    Wire::rect_xy(x0, y0, x1, y1, z)
}

pub fn circle(cx: f64, cy: f64, r: f64, z: f64) -> Wire {
    Wire::new(vec![Segment::Circle { center: [cx, cy, z], normal: [0.0, 0.0, 1.0], radius: r }])
}

/// Prism of one region along `v`.
pub fn prism(k: &OcctKernel, region: Region, v: P3) -> Op<Body> {
    k.prism(&PrismSpec { profile: Profile::new(vec![region]), vector: v, taper: None }).expect("prism")
}

pub fn block(k: &OcctKernel, x: f64, y: f64, z: f64) -> Body {
    prism(k, Region::new(rect(0.0, 0.0, x, y, 0.0)), [0.0, 0.0, z]).body
}

pub fn fuse(k: &OcctKernel, a: &Body, b: &Body) -> Body {
    k.boolean(BooleanOp::Fuse, a, &[b]).expect("fuse").body
}

pub fn cut(k: &OcctKernel, a: &Body, b: &Body) -> Body {
    k.boolean(BooleanOp::Cut, a, &[b]).expect("cut").body
}

/// Metrics the corpus records after each step (`corpus/<part>.json`).
#[derive(Debug, Clone, PartialEq)]
pub struct Metrics {
    pub volume: f64,
    pub area: f64,
    pub solids: u32,
    pub faces: usize,
    pub edges: usize,
    pub bbox: [P3; 2],
}

/// JavaScript's `Number(x.toPrecision(6))`, with |x| < 1e-7 → 0, as the corpus records.
pub fn sig6(x: f64) -> f64 {
    if x.abs() < 1e-7 {
        return 0.0;
    }
    format!("{x:.5e}").parse().unwrap()
}

pub fn metrics(k: &OcctKernel, b: &Body) -> Metrics {
    let m = k.mass_properties(b).unwrap();
    let t = k.topology(b).unwrap();
    let bb = k.bounding_box(b).unwrap();
    Metrics {
        volume: m.volume,
        area: m.area,
        solids: t.solids,
        faces: t.faces.len(),
        edges: t.edges.len(),
        bbox: [bb.min, bb.max],
    }
}

fn corpus_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

pub fn corpus_file(rel: &str) -> Vec<u8> {
    std::fs::read(corpus_dir().join(rel)).unwrap()
}

/// The recorded metrics of step `name` of corpus part `part`.
pub fn oracle(part: &str, name: &str) -> Metrics {
    let text = std::fs::read_to_string(corpus_dir().join(format!("{part}.json"))).unwrap();
    let j: serde_json::Value = serde_json::from_str(&text).unwrap();
    let step = j["steps"].as_array().unwrap().iter().find(|s| s["name"] == name).unwrap_or_else(|| panic!("{part}/{name}"));
    let b = &step["body"];
    let p = |v: &serde_json::Value| -> P3 { [0, 1, 2].map(|i| v[i].as_f64().unwrap()) };
    Metrics {
        volume: b["volume"].as_f64().unwrap(),
        area: b["area"].as_f64().unwrap(),
        solids: b["solids"].as_u64().unwrap() as u32,
        faces: b["faces"].as_u64().unwrap() as usize,
        edges: b["edges"].as_u64().unwrap() as usize,
        bbox: [p(&b["bbox"][0]), p(&b["bbox"][1])],
    }
}

/// The body matches a corpus step: volume, area and bbox to the corpus's 6 significant
/// figures, topology counts exactly. Also valid.
pub fn assert_step(k: &OcctKernel, b: &Body, part: &str, name: &str) {
    let want = oracle(part, name);
    let got = metrics(k, b);
    let rounded = Metrics {
        volume: sig6(got.volume),
        area: sig6(got.area),
        bbox: got.bbox.map(|p| p.map(sig6)),
        ..got.clone()
    };
    assert!(k.check(b).unwrap().valid, "{part}/{name}: invalid body");
    assert_eq!(rounded, want, "{part}/{name}: got {got:?}");
}

/// The one face (index) satisfying `pred`.
pub fn face_where(t: &Topology, pred: impl Fn(&FaceInfo) -> bool) -> FaceId {
    let hits: Vec<usize> = t.faces.iter().enumerate().filter(|(_, f)| pred(f)).map(|(i, _)| i).collect();
    assert_eq!(hits.len(), 1, "expected one face, found {hits:?}");
    FaceId(hits[0] as u32)
}

pub fn faces_where(t: &Topology, pred: impl Fn(&FaceInfo) -> bool) -> Vec<FaceId> {
    t.faces.iter().enumerate().filter(|(_, f)| pred(f)).map(|(i, _)| FaceId(i as u32)).collect()
}

/// Planar face with outward normal `n` whose centroid has coordinate `axis` = `at`
/// (the corpus builder's `planar(n, axis, at)`).
pub fn planar(n: P3, axis: usize, at: f64) -> impl Fn(&FaceInfo) -> bool {
    move |f| matches!(f.surface, Surface::Plane { .. }) && v3::dist(f.normal, n) < 1e-6 && near(f.centroid[axis], at)
}

/// Edges whose midpoint satisfies `pred`, optionally only straight ones.
pub fn edges_where(t: &Topology, line_only: bool, pred: impl Fn(P3) -> bool) -> Vec<EdgeId> {
    t.edges
        .iter()
        .enumerate()
        .filter(|(_, e)| !line_only || e.curve == Curve::Line)
        .filter(|(_, e)| pred(e.mid))
        .map(|(i, _)| EdgeId(i as u32))
        .collect()
}

pub fn deg(d: f64) -> f64 {
    d * PI / 180.0
}

/// Every face of the result has a source, and the history is well formed.
pub fn assert_tracked(op: &Op<Body>) {
    let n = op.body.face_count();
    op.history.check(n).unwrap();
    assert_eq!(op.history.untracked(n), Vec::<u32>::new(), "faces without history");
}
