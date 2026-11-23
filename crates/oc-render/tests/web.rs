//! W1c: what the browser's render worker relies on, checked natively (the browser runs the
//! same code on WebGPU):
//!
//! - a renderer made on a device asynchronously ([`Renderer::on_adapter`]);
//! - a device without the optional features (`DEPTH32FLOAT_STENCIL8`,
//!   `INDIRECT_FIRST_INSTANCE`; WebGPU adapters need not have them): 24-bit depth, bodies
//!   drawn one by one, and the same image and picks as with them;
//! - picks and draw counts collected without waiting for the GPU ([`Renderer::pick_async`],
//!   [`Renderer::draw_stats_async`]) equal the blocking ones.
//!
//! Without a GPU adapter the tests print SKIP and pass, unless `OC_RENDER_REQUIRE_GPU=1`.
use glam::{DQuat, DVec3};
use oc_kernel::{BooleanOp, Kernel, PrismSpec, Profile, Segment, Wire, P3};
use oc_kernel_occt::{Body, OcctKernel};
use oc_render::*;
use std::sync::Arc;

const W: u32 = 480;
const H: u32 = 360;

fn adapter() -> Option<wgpu::Adapter> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let opts = wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() };
    match pollster::block_on(instance.request_adapter(&opts)) {
        Ok(a) => Some(a),
        Err(e) if std::env::var("OC_RENDER_REQUIRE_GPU").is_err() => {
            println!("SKIP: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// A renderer on a device with only the features in `features` (of those it uses).
fn renderer_with(a: &wgpu::Adapter, features: wgpu::Features) -> Renderer {
    let (device, queue) = pollster::block_on(a.request_device(&wgpu::DeviceDescriptor {
        label: Some("web test"),
        required_features: Renderer::features(a.features()) & features,
        required_limits: Renderer::limits(&a.limits()),
        ..Default::default()
    }))
    .unwrap();
    Renderer::new(device, queue, a.get_info(), wgpu::TextureFormat::Rgba8UnormSrgb)
}

fn prism(k: &OcctKernel, w: Wire, v: P3) -> Body {
    k.prism(&PrismSpec { profile: Profile::single(w), vector: v, taper: None }).unwrap().body
}

/// The F1 test block: 20 × 10 × 5 with a Ø6 through hole.
fn block_hole(k: &OcctKernel) -> Arc<oc_mesh::RenderMesh> {
    let block = prism(k, Wire::rect_xy(0.0, 0.0, 20.0, 10.0, 0.0), [0.0, 0.0, 5.0]);
    let pin = prism(k, Wire::new(vec![Segment::Circle { center: [10.0, 5.0, -1.0], normal: [0.0, 0.0, 1.0], radius: 3.0 }]), [0.0, 0.0, 7.0]);
    let b = k.boolean(BooleanOp::Cut, &block, &[&pin]).unwrap().body;
    Arc::new(oc_mesh::build(k, &b, &oc_mesh::MeshOptions::default()).unwrap())
}

/// Six blocks in a row, the mesh through its bytes (as the engine worker sends it).
fn scene(r: &mut Renderer, mesh: &Arc<oc_mesh::RenderMesh>) -> (Scene, Camera) {
    let m = Arc::new(oc_mesh::RenderMesh::from_bytes(&mesh.to_bytes()).unwrap());
    let id = r.add_mesh(m);
    let bodies = (0..6)
        .map(|n| {
            let mut b = BodyItem::part(n, id, Rgb::hex("#c9ccd1"));
            b.placement = Placement { rotation: DQuat::IDENTITY, translation: DVec3::new(30.0 * n as f64, 0.0, 0.0) };
            b
        })
        .collect();
    let mut s = Scene { bodies, ..Default::default() };
    s.hover = Some(Hit::Face { body: 2, index: 0 });
    let mut cam = Camera::default();
    cam.resize(W as f64, H as f64);
    cam.set_view("isometric", Some(&Aabb { min: DVec3::new(0.0, 0.0, 0.0), max: DVec3::new(170.0, 10.0, 5.0) }));
    (s, cam)
}

/// Pixels that differ by more than 2 in some channel.
fn diff(a: &Image, b: &Image) -> usize {
    a.rgba.chunks(4).zip(b.rgba.chunks(4)).filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 2)).count()
}

/// A pick collected the way the browser does: submitted, then polled until the GPU is done.
fn pick_polled(r: &mut Renderer, s: &Scene, cam: &Camera, x: f64, y: f64) -> Option<Hit> {
    let vp = Viewport::new(W, H, 1.0);
    let mut p = r.pick_async(s, cam, vp, x, y, &PickFilter::default());
    for _ in 0..100_000 {
        if let Some(h) = p.poll(r) {
            return h;
        }
        std::thread::yield_now();
    }
    panic!("the pick never came back");
}

#[test]
fn a_device_made_asynchronously_draws() {
    let Some(a) = adapter() else { return };
    let mut r = pollster::block_on(Renderer::on_adapter(&a, wgpu::TextureFormat::Rgba8UnormSrgb)).unwrap();
    let k = OcctKernel::default();
    let (s, cam) = scene(&mut r, &block_hole(&k));
    let img = r.render_image(&s, &cam, Viewport::new(W, H, 1.0));
    let bg = img.pixel(2, 2);
    assert!(img.rgba.chunks(4).filter(|p| *p != bg).count() > 1000, "the blocks are drawn");
}

#[test]
fn without_the_optional_features_the_image_and_picks_are_the_same() {
    let Some(a) = adapter() else { return };
    let mut full = renderer_with(&a, wgpu::Features::all());
    let mut bare = renderer_with(&a, wgpu::Features::empty());
    assert_eq!(full.depth_format(), wgpu::TextureFormat::Depth32FloatStencil8, "this adapter has 32-bit depth");
    assert!(full.culling().gpu, "this adapter has indirect first instance");
    assert_eq!(bare.depth_format(), wgpu::TextureFormat::Depth24PlusStencil8);
    assert!(!bare.culling().gpu, "no GPU lists without INDIRECT_FIRST_INSTANCE");
    let k = OcctKernel::default();
    let m = block_hole(&k);
    let (s, cam) = scene(&mut full, &m);
    let (s2, _) = scene(&mut bare, &m);
    let vp = Viewport::new(W, H, 1.0);
    for style in [DisplayStyle::ShadedEdges, DisplayStyle::HiddenLinesVisible] {
        let (mut s, mut s2) = (s.clone(), s2.clone());
        s.style = style;
        s2.style = style;
        let _ = full.render_image(&s, &cam, vp);
        let (i1, i2) = (full.render_image(&s, &cam, vp), bare.render_image(&s2, &cam, vp));
        let d = diff(&i1, &i2);
        println!("{style:?}: {d} pixels differ");
        // Depth precision only: an edge pixel here and there.
        assert!(d * 1000 < (W * H) as usize, "{style:?}: {d} pixels differ");
    }
    for n in 0..6 {
        let at = cam.project(DVec3::new(30.0 * n as f64 + 3.0, 2.5, 5.0));
        let h1 = full.pick(&s, &cam, vp, at[0], at[1], &PickFilter::default());
        let h2 = pick_polled(&mut bare, &s2, &cam, at[0], at[1]);
        assert_eq!(h1, h2, "block {n}");
        // The top of the block, beside the hole: that block's face, or an edge or vertex near it.
        assert!(matches!(h1, Some(Hit::Face { body, .. } | Hit::Edge { body, .. } | Hit::Vertex { body, .. }) if body == n), "block {n}: {h1:?}");
    }
}

#[test]
fn picks_and_draw_counts_without_waiting_equal_the_blocking_ones() {
    let Some(a) = adapter() else { return };
    let mut r = renderer_with(&a, wgpu::Features::all());
    let k = OcctKernel::default();
    let (s, cam) = scene(&mut r, &block_hole(&k));
    let vp = Viewport::new(W, H, 1.0);
    let _ = r.render_image(&s, &cam, vp);
    let mut p = r.draw_stats_async();
    let mut later = None;
    for _ in 0..100_000 {
        if let Some(st) = p.poll(r.device()) {
            later = Some(st);
            break;
        }
        std::thread::yield_now();
    }
    let now = r.draw_stats();
    assert_eq!(later.as_ref(), Some(&now));
    assert_eq!(now.opaque_levels.iter().sum::<u32>(), 6, "{now:?}");
    for n in 0..6 {
        let at = cam.project(DVec3::new(30.0 * n as f64 + 3.0, 2.5, 5.0));
        let blocking = r.pick(&s, &cam, vp, at[0], at[1], &PickFilter::default());
        assert_eq!(pick_polled(&mut r, &s, &cam, at[0], at[1]), blocking, "block {n}");
    }
}
