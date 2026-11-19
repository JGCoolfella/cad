//! G2 (GPU-driven rendering): culling, indirect draws, levels of detail and shared mesh
//! uploads, checked against the renderer drawing every body one by one.
//!
//! The rule under test: culling changes speed, never the image. A wall stands in front of
//! a grid of blocks; more blocks stand beside it, a few far outside the view. Every display
//! style is rendered with one draw per body (`Culling::gpu` off), with GPU lists and no
//! culling, with frustum culling, and with frustum and occlusion culling, and the images
//! must be identical, while the draw counts show what was culled.
//!
//! Without a GPU adapter the tests print SKIP and pass, unless `OC_RENDER_REQUIRE_GPU=1`.
use glam::{DQuat, DVec3};
use oc_kernel::{BooleanOp, Kernel, PrismSpec, Profile, Segment, Wire, P3};
use oc_kernel_occt::{Body, OcctKernel};
use oc_render::*;
use std::sync::Arc;

const W: u32 = 640;
const H: u32 = 480;
const GRID: usize = 72; // behind the wall
const SIDE: usize = 5; // beside it
const FAR: usize = 5; // outside the view

fn renderer() -> Option<Renderer> {
    match Renderer::headless() {
        Ok(r) => Some(r),
        Err(e) if std::env::var("OC_RENDER_REQUIRE_GPU").is_err() => {
            println!("SKIP: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

fn save(img: &Image, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("oc-render-culling");
    std::fs::create_dir_all(&dir).unwrap();
    let f = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), img.width, img.height);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img.rgba).unwrap();
}

fn prism(k: &OcctKernel, w: Wire, v: P3) -> Body {
    k.prism(&PrismSpec { profile: Profile::single(w), vector: v, taper: None }).unwrap().body
}

/// The F1 test block: 20 × 10 × 5 with a Ø6 through hole.
fn block_hole(k: &OcctKernel) -> Body {
    let block = prism(k, Wire::rect_xy(0.0, 0.0, 20.0, 10.0, 0.0), [0.0, 0.0, 5.0]);
    let pin = prism(k, Wire::new(vec![Segment::Circle { center: [10.0, 5.0, -1.0], normal: [0.0, 0.0, 1.0], radius: 3.0 }]), [0.0, 0.0, 7.0]);
    k.boolean(BooleanOp::Cut, &block, &[&pin]).unwrap().body
}

fn mesh(k: &OcctKernel, b: &Body) -> Arc<oc_mesh::RenderMesh> {
    Arc::new(oc_mesh::build(k, b, &oc_mesh::MeshOptions::default()).unwrap())
}

fn at(id: u32, mesh: MeshId, p: DVec3) -> BodyItem {
    let mut b = BodyItem::part(id, mesh, Rgb::hex("#c9ccd1"));
    b.placement = Placement { rotation: DQuat::IDENTITY, translation: p };
    b
}

struct World {
    scene: Scene,
    fit: Aabb,
}

/// A wall (2 mm thick at z = 50) over a 9 × 8 grid of blocks at z = 0, five more blocks
/// (the third transparent) and five far away. `big`: the wall is 800 × 600 and covers the
/// whole view, the five blocks stand in front of it (z = 60); else it is 300 × 200 and they
/// stand beside it (z = 0).
fn world(r: &mut Renderer, big: bool) -> World {
    let k = OcctKernel::default();
    let block = r.add_mesh(mesh(&k, &block_hole(&k)));
    let (wx, wy) = if big { (400.0, 300.0) } else { (150.0, 100.0) };
    let wall = r.add_mesh(mesh(&k, &prism(&k, Wire::rect_xy(-wx, -wy, wx, wy, 0.0), [0.0, 0.0, 2.0])));
    let z = if big { 60.0 } else { 0.0 };
    let mut wall = at(1000, wall, DVec3::new(0.0, 0.0, 50.0));
    wall.color = Rgb::hex("#7d93ad");
    let mut bodies = vec![wall];
    for n in 0..GRID {
        bodies.push(at(n as u32, block, DVec3::new(-140.0 + 30.0 * (n % 9) as f64, -95.0 + 22.0 * (n / 9) as f64, 0.0)));
    }
    for j in 0..SIDE {
        let mut b = at(100 + j as u32, block, DVec3::new(200.0, -80.0 + 35.0 * j as f64, z));
        b.transparent = j == 2;
        bodies.push(b);
    }
    for j in 0..FAR {
        bodies.push(at(200 + j as u32, block, DVec3::new(5000.0, 30.0 * j as f64, 0.0)));
    }
    let fit = Aabb { min: DVec3::new(-160.0, -110.0, 0.0), max: DVec3::new(230.0, 110.0, 52.0) };
    World { scene: Scene { bodies, ..Default::default() }, fit }
}

const MODES: [(&str, Culling); 4] = [
    ("one-by-one", Culling { gpu: false, frustum: false, occlusion: false }),
    ("gpu-all", Culling { gpu: true, frustum: false, occlusion: false }),
    ("frustum", Culling { gpu: true, frustum: true, occlusion: false }),
    ("occlusion", Culling { gpu: true, frustum: true, occlusion: true }),
];

/// Pixels that differ between two images.
fn diff(a: &Image, b: &Image) -> usize {
    a.rgba.chunks(4).zip(b.rgba.chunks(4)).filter(|(p, q)| p != q).count()
}

/// Renders twice (the second frame's prepass uses the first frame's visibility).
fn draw(r: &mut Renderer, s: &Scene, cam: &Camera, c: Culling) -> Image {
    r.set_culling(c);
    let vp = Viewport::new(W, H, 1.0);
    let _ = r.render_image(s, cam, vp);
    r.render_image(s, cam, vp)
}

#[test]
fn culling_never_changes_the_image() {
    let Some(mut r) = renderer() else { return };
    println!("adapter: {:?}", r.adapter_info());
    assert!(r.culling().gpu, "the adapter has INDIRECT_FIRST_INSTANCE");
    let mut w = world(&mut r, true);
    // A hovered face, a selected edge (highlight states through the GPU lists).
    w.scene.hover = Some(Hit::Face { body: 101, index: 0 });
    w.scene.selected = vec![Hit::Edge { body: 103, index: 0 }];
    let mut cam = Camera::default();
    cam.resize(W as f64, H as f64);
    cam.set_view("front", Some(&w.fit));
    let total = 1 + GRID + SIDE + FAR;
    for style in [DisplayStyle::ShadedEdges, DisplayStyle::Shaded, DisplayStyle::HiddenLinesRemoved, DisplayStyle::HiddenLinesVisible, DisplayStyle::Wireframe] {
        w.scene.style = style;
        let images: Vec<(&str, Image)> = MODES.iter().map(|(n, c)| (*n, draw(&mut r, &w.scene, &cam, *c))).collect();
        let stats = r.draw_stats();
        let base = &images[0].1;
        save(base, &format!("{style:?}"));
        // Something is drawn over the wall (the side blocks' faces or edges).
        let corner = base.pixel(2, 2);
        let drawn = base.rgba.chunks(4).filter(|p| *p != corner).count();
        assert!(drawn > 500, "{style:?}: blocks drawn over the wall ({drawn} px)");
        for (name, img) in &images[1..] {
            if diff(base, img) != 0 {
                save(img, &format!("{style:?}-{name}"));
            }
            assert_eq!(diff(base, img), 0, "{style:?}: {name} differs from one-by-one drawing");
        }
        println!("{style:?}: {stats:?}");
        assert_eq!(stats.instances as usize, total);
        // With occlusion culling (the last mode): the grid behind the wall is not drawn
        // (the wall covers the view); in Wireframe nothing hides anything.
        let opaque = stats.kinds[1] as usize;
        let edges = stats.kinds[3] as usize;
        let seen = 1 + SIDE; // wall and the blocks in front (one transparent: not opaque)
        match style {
            DisplayStyle::Wireframe => {
                assert_eq!(opaque, 0);
                assert_eq!(edges, total - FAR, "{style:?}: every edge in view is drawn");
            }
            DisplayStyle::Shaded => {
                assert_eq!(opaque, seen - 1, "{style:?}: opaque faces of the wall and side blocks only");
                // Only bodies with highlighted edges draw edges in Shaded.
                assert_eq!(edges, 1, "{style:?}: highlighted edges only");
            }
            _ => {
                assert_eq!(opaque, seen - 1, "{style:?}: opaque faces of the wall and side blocks only");
                assert_eq!(edges, seen, "{style:?}: edges of the visible bodies");
            }
        }
        assert_eq!(stats.kinds[2], u32::from(style != DisplayStyle::Wireframe), "{style:?}: the transparent block");
        if style == DisplayStyle::HiddenLinesVisible {
            assert_eq!(stats.kinds[4] as usize, total - FAR, "hidden edges are culled by the frustum only");
        }
    }
}

#[test]
fn culling_follows_the_section_and_the_view() {
    let Some(mut r) = renderer() else { return };
    let mut w = world(&mut r, false);
    let vp = Viewport::new(W, H, 1.0);
    let mut cam = Camera::default();
    cam.resize(W as f64, H as f64);
    cam.set_view("front", Some(&w.fit));
    // From a cold start: the first frame knows no visibility (nothing culled), the second
    // draws its prepass from all of it and learns what is hidden, the third culls.
    r.set_culling(Culling::default());
    let mut drawn = vec![];
    for _ in 0..3 {
        let _ = r.render_image(&w.scene, &cam, vp);
        drawn.push(r.draw_stats().kinds[1] as usize);
    }
    println!("opaque instances drawn, frames 1-3: {drawn:?}");
    assert_eq!(drawn[0], GRID + SIDE, "first frame: wall, grid and opaque side blocks");
    assert!(drawn[2] < GRID / 2, "third frame: most of the grid behind the wall culled: {drawn:?}");
    // A section above the grid cuts the wall away: the grid shows at once (the prepass
    // draws only what is left of the wall) and matches the image drawn one by one.
    w.scene.section = Some(Section { origin: DVec3::new(0.0, 0.0, 40.0), normal: DVec3::Z });
    let culled = r.render_image(&w.scene, &cam, vp);
    let st = r.draw_stats();
    assert_eq!(st.kinds[1] as usize, GRID + SIDE - 1, "grid and opaque side blocks; the wall is cut away");
    assert_eq!(st.kinds[5] as usize, GRID + SIDE, "section parity of every body in view and not cut away");
    r.set_culling(Culling { gpu: false, frustum: false, occlusion: false });
    let plain = r.render_image(&w.scene, &cam, vp);
    save(&culled, "section");
    assert_eq!(diff(&plain, &culled), 0, "section: culled image equals the plain one");
    // Turning the view: the grid seen past the wall's edge, frame after frame.
    w.scene.section = None;
    for i in 0..6 {
        cam.rotate(0.0, 9.0, w.fit.center(), 1.0);
        r.set_culling(Culling::default());
        let culled = r.render_image(&w.scene, &cam, vp);
        r.set_culling(Culling { gpu: false, frustum: false, occlusion: false });
        let plain = r.render_image(&w.scene, &cam, vp);
        if diff(&plain, &culled) != 0 {
            save(&culled, &format!("turn-{i}-culled"));
            save(&plain, &format!("turn-{i}-plain"));
        }
        assert_eq!(diff(&plain, &culled), 0, "turn {i}: culled image equals the plain one");
    }
}

#[test]
fn picks_go_through_the_gpu_lists() {
    let Some(mut r) = renderer() else { return };
    let w = world(&mut r, false);
    let vp = Viewport::new(W, H, 1.0);
    let mut cam = Camera::default();
    cam.resize(W as f64, H as f64);
    cam.set_view("isometric", Some(&w.fit));
    let filter = PickFilter::default();
    for c in [Culling::default(), Culling { gpu: false, frustum: false, occlusion: false }] {
        r.set_culling(c);
        let _ = r.render_image(&w.scene, &cam, vp);
        // The top of every side block (block local (3, 2.5, 5): beside the hole) finds
        // that block (its face, or an edge or vertex near it).
        for j in [0usize, 1, 3, 4] {
            let p = DVec3::new(203.0, -77.5 + 35.0 * j as f64, 5.0);
            let s = cam.project(p);
            match r.pick(&w.scene, &cam, vp, s[0], s[1], &filter) {
                Some(Hit::Face { body, .. } | Hit::Edge { body, .. } | Hit::Vertex { body, .. }) => assert_eq!(body, 100 + j as u32, "{c:?}: side block {j}"),
                h => panic!("{c:?}: side block {j}: {h:?}"),
            }
        }
        // The wall's top face over the grid's middle.
        let s = cam.project(DVec3::new(0.0, 0.0, 52.0));
        assert!(matches!(r.pick(&w.scene, &cam, vp, s[0], s[1], &filter), Some(Hit::Face { body: 1000, .. })), "{c:?}: wall");
        if c.gpu {
            let st = r.draw_stats();
            assert!(st.kinds[6] >= 1 && (st.kinds[6] as usize) < 10, "the pick window culls: {st:?}");
        }
    }
}

#[test]
fn automatic_detail_coarsens_far_instances() {
    let Some(mut r) = renderer() else { return };
    let k = OcctKernel::default();
    // A disc with a large hole: the cylinders' levels differ.
    let disc = prism(&k, Wire::new(vec![Segment::Circle { center: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0], radius: 20.0 }]), [0.0, 0.0, 4.0]);
    let pin = prism(&k, Wire::new(vec![Segment::Circle { center: [0.0, 0.0, -1.0], normal: [0.0, 0.0, 1.0], radius: 12.0 }]), [0.0, 0.0, 6.0]);
    let body = k.boolean(BooleanOp::Cut, &disc, &[&pin]).unwrap().body;
    let m = mesh(&k, &body);
    let levels = m.lods.len();
    println!("levels: {:?}", m.lods.iter().map(|l| (l.params.linear, l.triangle_count())).collect::<Vec<_>>());
    assert!(levels >= 3, "the mesh has coarser levels");
    let id = r.add_mesh(m);
    // A row of discs going away from a perspective camera.
    let bodies: Vec<BodyItem> = (0..40)
        .map(|n| {
            let mut b = at(n, id, DVec3::new(0.0, 0.0, -150.0 * n as f64));
            b.lod = LodChoice::Auto { max_error_px: 0.5 };
            b
        })
        .collect();
    let scene = Scene { bodies, ..Default::default() };
    let mut cam = Camera::default();
    cam.resize(W as f64, H as f64);
    cam.perspective = true;
    cam.set_view("front", Some(&Aabb { min: DVec3::new(-25.0, -25.0, -10.0), max: DVec3::new(25.0, 25.0, 5.0) }));
    let vp = Viewport::new(W, H, 1.0);
    r.set_culling(Culling { gpu: true, frustum: true, occlusion: false });
    let img = r.render_image(&scene, &cam, vp);
    save(&img, "lod");
    let st = r.draw_stats();
    println!("{st:?}");
    assert_eq!(st.opaque_levels.iter().sum::<u32>(), 40);
    assert!(st.opaque_levels[0] >= 1, "the nearest disc at the finest level");
    assert!(st.opaque_levels[1..].iter().sum::<u32>() >= 1, "far discs coarser");
    // The nearest disc is picked on its face at the finest level.
    let s = cam.project(DVec3::new(16.0, 0.0, 4.0));
    assert!(matches!(r.pick(&scene, &cam, vp, s[0], s[1], &PickFilter::default()), Some(Hit::Face { body: 0, .. })));
}

#[test]
fn the_same_mesh_is_uploaded_once() {
    let Some(mut r) = renderer() else { return };
    let k = OcctKernel::default();
    let m = mesh(&k, &block_hole(&k));
    let a = r.add_mesh(m.clone());
    let bytes = r.pool_bytes();
    // An assembly rebuilt with the part unchanged: a new handle, the same Arc.
    let b = r.add_mesh(m.clone());
    assert_eq!(a, b);
    assert_eq!((r.mesh_count(), r.pool_bytes()), (1, bytes), "not uploaded again");
    // The first holder lets go: still drawn for the second.
    r.remove_mesh(a);
    assert_eq!(r.mesh_count(), 1);
    let scene = Scene { bodies: vec![BodyItem::part(1, b, Rgb::hex("#c9ccd1"))], ..Default::default() };
    let mut cam = Camera::default();
    cam.resize(W as f64, H as f64);
    cam.set_view("isometric", Some(&Aabb { min: DVec3::ZERO, max: DVec3::new(20.0, 10.0, 5.0) }));
    let s = cam.project(DVec3::new(3.0, 2.5, 5.0));
    let img = r.render_image(&scene, &cam, Viewport::new(W, H, 1.0));
    assert!(img.pixel(s[0] as u32, s[1] as u32)[0] > 90, "drawn after the first remove");
    r.remove_mesh(b);
    assert_eq!(r.mesh_count(), 0);
    // A different Arc with the same contents is a different mesh.
    let c = r.add_mesh(Arc::new((*m).clone()));
    assert_ne!(c, a);
}
