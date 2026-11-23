//! Display meshes for the render worker's harness (`node scripts/web-render.mjs smoke`):
//! `RenderMesh::to_bytes` blobs, as the engine worker will send them (W1b). Until then the
//! harness feeds these.
//!
//! `cargo run -p oc-web-render --example mesh_blobs -- <dir>` writes `block.ocmesh` (the F1
//! test block: 20 × 10 × 5 with a Ø6 through hole) and `disc.ocmesh` (Ø40 × 4 with a Ø24
//! hole: levels of detail that differ).
#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use oc_kernel::{BooleanOp, Kernel, PrismSpec, Profile, Segment, Wire, P3};
    use oc_kernel_occt::{Body, OcctKernel};
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("usage: mesh_blobs <dir>"));
    std::fs::create_dir_all(&dir).expect("output folder");
    let k = OcctKernel::default();
    let prism = |w: Wire, v: P3| -> Body { k.prism(&PrismSpec { profile: Profile::single(w), vector: v, taper: None }).unwrap().body };
    let circle = |c: P3, r: f64| Wire::new(vec![Segment::Circle { center: c, normal: [0.0, 0.0, 1.0], radius: r }]);
    let block = prism(Wire::rect_xy(0.0, 0.0, 20.0, 10.0, 0.0), [0.0, 0.0, 5.0]);
    let pin = prism(circle([10.0, 5.0, -1.0], 3.0), [0.0, 0.0, 7.0]);
    let block = k.boolean(BooleanOp::Cut, &block, &[&pin]).unwrap().body;
    let disc = prism(circle([0.0, 0.0, 0.0], 20.0), [0.0, 0.0, 4.0]);
    let hole = prism(circle([0.0, 0.0, -1.0], 12.0), [0.0, 0.0, 6.0]);
    let disc = k.boolean(BooleanOp::Cut, &disc, &[&hole]).unwrap().body;
    for (name, body) in [("block", &block), ("disc", &disc)] {
        let m = oc_mesh::build(&k, body, &oc_mesh::MeshOptions::default()).unwrap();
        let bytes = m.to_bytes();
        std::fs::write(dir.join(format!("{name}.ocmesh")), &bytes).expect("write");
        println!("{name}: {} bytes, {} levels, {} triangles at level 0", bytes.len(), m.lods.len(), m.lods[0].triangle_count());
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}
