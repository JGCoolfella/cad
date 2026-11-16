//! F1 smoke test (plan "L, smoke"): a real part (OCCT block with a through hole, meshed by
//! oc-mesh) rendered headless in every display style and checked by pixel, plus picking
//! through the GPU ID buffer against the kernel's topology.
//!
//! Images are written to `<target>/tmp/oc-render-smoke/` for inspection.
//! Without a GPU adapter the test prints SKIP and passes, unless `OC_RENDER_REQUIRE_GPU=1`.
use glam::DVec3;
use oc_kernel::{BooleanOp, FaceInfo, Kernel, PrismSpec, Profile, Segment, Surface, Topology, Wire, P3};
use oc_kernel_occt::{Body, OcctKernel};
use oc_render::*;
use std::sync::Arc;

const W: u32 = 800;
const H: u32 = 600;

fn block_hole(k: &OcctKernel) -> Body {
    let prism = |w: Wire, v: P3| {
        k.prism(&PrismSpec { profile: Profile::single(w), vector: v, taper: None })
            .unwrap()
            .body
    };
    let block = prism(Wire::rect_xy(0.0, 0.0, 20.0, 10.0, 0.0), [0.0, 0.0, 5.0]);
    let pin = prism(
        Wire::new(vec![Segment::Circle { center: [10.0, 5.0, -1.0], normal: [0.0, 0.0, 1.0], radius: 3.0 }]),
        [0.0, 0.0, 7.0],
    );
    k.boolean(BooleanOp::Cut, &block, &[&pin]).unwrap().body
}

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
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("oc-render-smoke");
    std::fs::create_dir_all(&dir).unwrap();
    let f = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), img.width, img.height);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img.rgba).unwrap();
}

fn srgb(hex: &str) -> [u8; 3] {
    Rgb::hex(hex).to_srgb().map(|v| (v * 255.0).round() as u8)
}

fn near(p: [u8; 4], c: [u8; 3], tol: i32) -> bool {
    (0..3).all(|k| (p[k] as i32 - c[k] as i32).abs() <= tol)
}

/// Pixels within `r` of a point (physical px).
fn around(img: &Image, at: [f64; 2], r: i32) -> Vec<[u8; 4]> {
    let mut v = vec![];
    for dy in -r..=r {
        for dx in -r..=r {
            let (x, y) = (at[0].round() as i32 + dx, at[1].round() as i32 + dy);
            if x >= 0 && y >= 0 && (x as u32) < img.width && (y as u32) < img.height {
                v.push(img.pixel(x as u32, y as u32));
            }
        }
    }
    v
}

struct Part {
    top: u32,
    hole: u32,
    top_front_edge: u32,
    hidden_edge: u32,
    corner: u32,
}

fn find(topo: &Topology) -> Part {
    let face = |pred: &dyn Fn(&FaceInfo) -> bool| topo.faces.iter().position(pred).unwrap() as u32;
    let top = face(&|f| f.normal[2] > 0.99 && (f.centroid[2] - 5.0).abs() < 1e-6);
    let hole = face(&|f| !matches!(f.surface, Surface::Plane { .. }));
    let edge = |m: P3| {
        topo.edges
            .iter()
            .position(|e| (0..3).all(|k| (e.mid[k] - m[k]).abs() < 1e-6))
            .unwrap() as u32
    };
    let corner = topo
        .vertices
        .iter()
        .position(|v| (0..3).all(|k| (v[k] - [20.0, 10.0, 5.0][k]).abs() < 1e-9))
        .unwrap() as u32;
    Part {
        top,
        hole,
        // Isometric from (1, 1, 1): the top edge along x at y = 10 is visible; the vertical
        // edge at x = 0, y = 0 is behind the body.
        top_front_edge: edge([10.0, 10.0, 5.0]),
        hidden_edge: edge([0.0, 0.0, 2.5]),
        corner,
    }
}

#[test]
fn smoke_render_and_pick() {
    let Some(mut r) = renderer() else { return };
    println!("adapter: {:?}", r.adapter_info());
    let k = OcctKernel::default();
    let body = block_hole(&k);
    let part = find(&k.topology(&body).unwrap());
    let mesh = Arc::new(oc_mesh::build(&k, &body, &oc_mesh::MeshOptions::default()).unwrap());
    let mid = r.add_mesh(mesh);

    // A 4:1 label image (the caller rasterises text; here a solid bar is enough).
    let label = r.add_label(&[0x3f, 0x86, 0xec, 255].repeat(64 * 16), 64, 16);
    let mut scene = Scene {
        bodies: vec![BodyItem::part(7, mid, Rgb::hex("#c9ccd1"))],
        // The origin marker lies over the hidden edge at x = y = 0: shown later.
        origin: OriginItem { shown: false, pickable: true },
        ..Default::default()
    };
    let model = Aabb { min: DVec3::ZERO, max: DVec3::new(20.0, 10.0, 5.0) };
    // Right Plane (x = 0 is the YZ plane) shown off to the side: frame moved to x = -15 so
    // it does not cover the body in the isometric view.
    let mut frame = Frame::right();
    frame.origin = DVec3::new(-15.0, 0.0, 0.0);
    scene.planes.push(PlaneItem { id: 3, frame, extents: [-10.0, 10.0, -8.0, 8.0], label: Some(label) });
    scene.axes.push(AxisItem { id: 21, point: DVec3::new(30.0, 5.0, 2.5), dir: DVec3::Z, length: 30.0 });
    scene.points.push(PointItem { id: 22, point: DVec3::new(30.0, -8.0, 0.0) });
    scene.csys.push(CsysItem { id: 23, frame: Frame { origin: DVec3::new(30.0, 20.0, 0.0), ..Frame::front() } });
    scene.sketches.push(SketchItem { id: 24, segments: vec![[DVec3::new(-5.0, 15.0, 0.0), DVec3::new(25.0, 15.0, 0.0)]], profile: false });

    let mut cam = Camera::default();
    let vp = Viewport::new(W, H, 1.0);
    cam.resize(W as f64, H as f64);
    let mut fit = model;
    fit.add(DVec3::new(-15.0, -10.0, -8.0));
    fit.add(DVec3::new(32.0, 20.0, 15.0));
    cam.set_view("isometric", Some(&fit));
    let px = |p: [f64; 3]| cam.project(DVec3::from(p));

    // ---------------------------------------------------------------- display styles
    // A point of the top face away from the hole and the edges.
    let on_top = px([3.0, 2.5, 5.0]);
    let on_edge = px([10.0, 10.0, 5.0]);

    let shaded = r.render_image(&scene, &cam, vp);
    save(&shaded, "shaded_edges");
    let bg = shaded.pixel(5, 5);
    let drawn = shaded.rgba.chunks(4).filter(|p| !near([p[0], p[1], p[2], p[3]], [bg[0], bg[1], bg[2]], 6)).count();
    assert!(drawn > 20_000, "something was drawn: {drawn}");
    let top_px = shaded.pixel(on_top[0] as u32, on_top[1] as u32);
    assert!(top_px[3] == 255 && (top_px[0] as i32 - top_px[2] as i32).abs() < 25, "grey lit face {top_px:?}");
    assert!(top_px[0] > 90, "lit, not black: {top_px:?}");
    let edge_dark = around(&shaded, on_edge, 1).iter().any(|p| p[0] < 80 && p[1] < 80 && p[2] < 80);
    assert!(edge_dark, "edges drawn dark in Shaded With Edges");
    // Background: the scene gradient, white near the middle, darker in the corner.
    assert!(near(bg, srgb("#c8cdd4"), 30), "background corner {bg:?}");

    scene.style = DisplayStyle::Shaded;
    let plain = r.render_image(&scene, &cam, vp);
    save(&plain, "shaded");
    assert!(!around(&plain, on_edge, 1).iter().any(|p| p[0] < 60), "no edges in Shaded");

    scene.style = DisplayStyle::HiddenLinesRemoved;
    let hlr = r.render_image(&scene, &cam, vp);
    save(&hlr, "hlr");
    assert!(near(hlr.pixel(on_top[0] as u32, on_top[1] as u32), [255, 255, 255], 2), "white faces in HLR");
    assert!(around(&hlr, on_edge, 1).iter().any(|p| p[0] < 80), "visible edge in HLR");
    let hidden_mid = px([0.0, 0.0, 2.5]);
    assert!(near(hlr.pixel(hidden_mid[0] as u32, hidden_mid[1] as u32), [255, 255, 255], 2), "hidden edge removed");

    scene.style = DisplayStyle::HiddenLinesVisible;
    let hlv = r.render_image(&scene, &cam, vp);
    save(&hlv, "hlv");
    let grey = srgb("#8a8a8a");
    // Sample along the hidden edge (1 px, antialiased): dashed, so some samples have a
    // grey pixel next to them and some are white all round.
    let darkest: Vec<u8> = (0..=24)
        .map(|i| {
            let p = px([0.0, 0.0, 0.2 + 4.6 * i as f64 / 24.0]);
            around(&hlv, p, 1).iter().map(|q| q[0]).min().unwrap()
        })
        .collect();
    let greys = darkest.iter().filter(|&&v| v < grey[0] + 50).count();
    let whites = darkest.iter().filter(|&&v| v > 245).count();
    assert!(greys >= 5 && whites >= 2, "dashed hidden edge: {darkest:?}");
    scene.style = DisplayStyle::Wireframe;
    let wire = r.render_image(&scene, &cam, vp);
    save(&wire, "wireframe");
    assert!(around(&wire, hidden_mid, 1).iter().any(|p| p[0] < 80), "hidden edges drawn in Wireframe");
    let wt = wire.pixel(on_top[0] as u32, on_top[1] as u32);
    assert!(wt[0] > 200, "no faces in Wireframe: {wt:?}");

    // ---------------------------------------------------------------- highlights
    scene.style = DisplayStyle::ShadedEdges;
    scene.hover = Some(Hit::Face { body: 7, index: part.top });
    let hov = r.render_image(&scene, &cam, vp);
    save(&hov, "hover_face");
    let h = hov.pixel(on_top[0] as u32, on_top[1] as u32);
    assert!(h[0] as i32 - h[2] as i32 > 40, "hovered face tinted orange: {h:?} (plain {top_px:?})");
    scene.hover = Some(Hit::Edge { body: 7, index: part.top_front_edge });
    scene.selected = vec![Hit::Face { body: 7, index: part.top }];
    let sel = r.render_image(&scene, &cam, vp);
    save(&sel, "selected_face_hover_edge");
    let s = sel.pixel(on_top[0] as u32, on_top[1] as u32);
    assert!(s[2] as i32 - s[0] as i32 > 40, "selected face tinted blue: {s:?}");
    let orange = srgb("#ff9a1f");
    let wide: Vec<_> = around(&sel, on_edge, 2);
    assert!(wide.iter().filter(|p| near(**p, orange, 40)).count() >= 6, "hovered edge 3 px orange");
    scene.options.dynamic_highlight = false;
    let no_dyn = r.render_image(&scene, &cam, vp);
    assert!(around(&no_dyn, on_edge, 2).iter().all(|p| !near(*p, orange, 40)), "no hover with dynamic highlight off");
    scene.options.dynamic_highlight = true;
    scene.hover = Some(Hit::Vertex { body: 7, index: part.corner });
    let vtx = r.render_image(&scene, &cam, vp);
    let c = px([20.0, 10.0, 5.0]);
    assert!(around(&vtx, c, 3).iter().filter(|p| near(**p, orange, 30)).count() >= 30, "vertex marker 8 px");
    scene.hover = Some(Hit::Plane(3));
    scene.selected.clear();
    scene.origin.shown = true;

    // ---------------------------------------------------------------- reference geometry
    let planes_img = r.render_image(&scene, &cam, vp);
    save(&planes_img, "planes_refs");
    let corner = px(frame.to_world(-10.0, -8.0).to_array());
    assert!(around(&planes_img, corner, 2).iter().any(|p| near(*p, orange, 40)), "hovered plane border orange");
    let axis_mid = px([30.0, 5.0, 2.5]);
    let blue = srgb("#2a62c9");
    // 1 px antialiased and dashed: count samples with a bluish pixel next to them.
    let bluish = |q: &[u8; 4]| q[2] as i32 - q[0] as i32 > 50;
    let axis_hits = (0..40)
        .filter(|i| {
            let p = px([30.0, 5.0, -12.0 + 29.0 * *i as f64 / 40.0]);
            around(&planes_img, p, 1).iter().any(bluish)
        })
        .count();
    assert!((15..40).contains(&axis_hits), "dashed blue axis: {axis_hits} of 40");
    let refp = px([30.0, -8.0, 0.0]);
    assert!(around(&planes_img, refp, 2).iter().all(|p| near(*p, blue, 40)), "reference point square");
    let red = srgb("#e01010");
    assert!(
        (0..W / 6).flat_map(|x| (H - 110..H).map(move |y| (x, y))).filter(|&(x, y)| near(planes_img.pixel(x, y), red, 30)).count() > 20,
        "triad X axis"
    );
    let o = px([0.0, 0.0, 0.0]);
    assert!(around(&planes_img, o, 1).iter().any(|p| near(*p, srgb("#1c3fd6"), 40)), "origin marker");

    // ---------------------------------------------------------------- section
    // Looking straight down (Top view) at a cut through the hole's axis, keeping y < 5:
    // the cap (model colour × 0.85) is the rectangle x 0..20, z 0..5 less the hole's slot.
    scene.hover = None;
    scene.planes.clear();
    let mut top_cam = cam.clone();
    top_cam.set_view("top", Some(&model));
    let tp = |p: [f64; 3]| top_cam.project(DVec3::from(p));
    let uncut = r.render_image(&scene, &top_cam, vp);
    scene.section = Some(Section { origin: DVec3::new(10.0, 5.0, 0.0), normal: DVec3::Y });
    let sec = r.render_image(&scene, &top_cam, vp);
    save(&sec, "section");
    let at = |img: &Image, p: [f64; 3]| {
        let q = tp(p);
        img.pixel(q[0].round() as u32, q[1].round() as u32)
    };
    let (c1, c2, c3) = (at(&sec, [3.0, 5.0, 2.5]), at(&sec, [17.0, 5.0, 1.0]), at(&sec, [10.0, 5.0, 2.5]));
    let top_face = at(&uncut, [3.0, 10.0, 2.5]);
    assert_eq!(c1, c2, "flat cap");
    assert!(c1[0] + 8 < top_face[0], "cap darker than the uncut top face: {c1:?} vs {top_face:?}");
    assert_ne!(c3, c1, "no cap across the hole");
    // Iso view: the half beyond the plane is gone.
    let iso = r.render_image(&scene, &cam, vp);
    save(&iso, "section_iso");
    // On the removed half; its view ray misses the kept half (y < 5 needs z < 0 there).
    let gone = px([19.0, 9.0, 2.5]);
    let gp = iso.pixel(gone[0] as u32, gone[1] as u32);
    let empty = Scene { bodies: vec![], ..scene.clone() };
    let bgp = r.render_image(&empty, &cam, vp).pixel(gone[0] as u32, gone[1] as u32);
    assert!(near(gp, [bgp[0], bgp[1], bgp[2]], 2), "corner removed by the section: {gp:?} vs background {bgp:?}");
    scene.planes.push(PlaneItem { id: 3, frame, extents: [-10.0, 10.0, -8.0, 8.0], label: Some(label) });

    // ---------------------------------------------------------------- picking
    scene.section = None;
    let none = PickFilter::default();
    let mut pick = |scene: &Scene, p: [f64; 2], f: &PickFilter| r.pick(scene, &cam, vp, p[0], p[1], f);
    assert_eq!(pick(&scene, on_top, &none), Some(Hit::Face { body: 7, index: part.top }));
    assert_eq!(pick(&scene, on_edge, &none), Some(Hit::Edge { body: 7, index: part.top_front_edge }));
    // 3 px off the edge onto the top face: the edge still wins (radius 4).
    let toward = |a: [f64; 2], b: [f64; 2], d: f64| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = dx.hypot(dy);
        [a[0] + dx / l * d, a[1] + dy / l * d]
    };
    let off = toward(on_edge, px([10.0, 5.0, 5.0]), 3.0);
    assert_eq!(pick(&scene, off, &none), Some(Hit::Edge { body: 7, index: part.top_front_edge }));
    // Selection filter on faces: the face, not the edge.
    let faces = PickFilter { only: Some(vec![PickKind::Face]), edges_only: false };
    assert_eq!(pick(&scene, off, &faces), Some(Hit::Face { body: 7, index: part.top }));
    // Vertex beats edge near a corner; Chamfer (edges only) takes the edge.
    let near_corner = toward(c, on_edge, 2.0);
    assert_eq!(pick(&scene, near_corner, &none), Some(Hit::Vertex { body: 7, index: part.corner }));
    let chamfer = PickFilter { only: None, edges_only: true };
    assert!(matches!(pick(&scene, near_corner, &chamfer), Some(Hit::Edge { .. })));
    // The hole's cylinder.
    let in_hole = px([10.0, 2.0, 3.5]); // the wall side facing the eye, seen through the opening
    assert_eq!(pick(&scene, in_hole, &none), Some(Hit::Face { body: 7, index: part.hole }), "hole wall");
    // The hidden edge is occluded; in Wireframe it is pickable.
    let hidden = Hit::Edge { body: 7, index: part.hidden_edge };
    assert_ne!(pick(&scene, hidden_mid, &none), Some(hidden));
    scene.style = DisplayStyle::Wireframe;
    assert_eq!(pick(&scene, hidden_mid, &none), Some(hidden));
    assert!(!matches!(pick(&scene, on_top, &none), Some(Hit::Face { .. })), "no faces in Wireframe");
    scene.style = DisplayStyle::ShadedEdges;
    // The origin is the box's back corner, hidden in this view: the origin is still picked
    // while it is pickable (screen distance, before the ID pass); otherwise the occluded
    // vertex cannot be, and the face in front of it is.
    assert_eq!(pick(&scene, o, &none), Some(Hit::Origin));
    scene.origin.pickable = false;
    assert_eq!(pick(&scene, o, &none), Some(Hit::Face { body: 7, index: part.top }));
    // Plane, axis, point, coordinate system, sketch.
    let plane_in = px(frame.to_world(0.0, -4.0).to_array());
    assert_eq!(pick(&scene, plane_in, &none), Some(Hit::Plane(3)));
    assert_eq!(pick(&scene, axis_mid, &none), Some(Hit::Feature(21)));
    assert_eq!(pick(&scene, refp, &none), Some(Hit::Feature(22)));
    let csys_x = px([30.0 + cam.world_per_pixel() * 30.0, 20.0, 0.0]);
    assert_eq!(pick(&scene, csys_x, &none), Some(Hit::Feature(23)));
    assert_eq!(pick(&scene, px([-2.0, 15.0, 0.0]), &none), Some(Hit::Feature(24)));
    assert_eq!(pick(&scene, [3.0, 3.0], &none), None, "background");
    // Async (hover) gives the same answer.
    let mut pending = r.pick_async(&scene, &cam, vp, on_top[0], on_top[1], &none);
    let got = loop {
        if let Some(h) = pending.poll(&r) {
            break h;
        }
        std::thread::yield_now();
    };
    assert_eq!(got, Some(Hit::Face { body: 7, index: part.top }));

    // Select Other: every face along the ray, nearest first.
    let ray = cam.ray(on_top[0], on_top[1]);
    let hits = r.raycast_faces(&scene, &ray);
    assert_eq!(hits.first().map(|h| h.hit), Some(Hit::Face { body: 7, index: part.top }));
    assert!(hits.len() >= 2, "top and bottom faces along the ray: {hits:?}");
}

#[test]
fn hidpi_scales_logical_sizes() {
    let Some(mut r) = renderer() else { return };
    let scene = Scene::default();
    let cam = Camera::default();
    // Same logical viewport at scale 2: the triad lands at twice the physical position.
    let img = r.render_image(&scene, &cam, Viewport::new(400, 300, 2.0));
    assert_eq!((img.width, img.height), (400, 300));
    let red = srgb("#e01010");
    let reds: Vec<(u32, u32)> = (0..400).flat_map(|x| (0..300).map(move |y| (x, y))).filter(|&(x, y)| near(img.pixel(x, y), red, 30)).collect();
    assert!(!reds.is_empty());
    // Triad centre (46, 150 − 52) logical → (92, 196) physical; X arm ends ~30 logical px away.
    let max_dx = reds.iter().map(|&(x, _)| x as i32 - 92).max().unwrap();
    assert!((50..=110).contains(&max_dx), "{max_dx}");
}

/// Frame and pick times (run with `--release -- --ignored --nocapture`): a plate with
/// 400 holes at 1920×1080, 4× MSAA.
#[test]
#[ignore]
fn timings() {
    let Some(mut r) = renderer() else { return };
    let k = OcctKernel::default();
    let prism = |w: Wire, v: P3| k.prism(&PrismSpec { profile: Profile::single(w), vector: v, taper: None }).unwrap().body;
    let mut plate = prism(Wire::rect_xy(0.0, 0.0, 400.0, 300.0, 0.0), [0.0, 0.0, 10.0]);
    let pins: Vec<Body> = (0..20)
        .flat_map(|i| (0..20).map(move |j| (i, j)))
        .map(|(i, j)| {
            let c = [12.0 + 19.0 * i as f64, 10.0 + 14.5 * j as f64, -1.0];
            prism(Wire::new(vec![Segment::Circle { center: c, normal: [0.0, 0.0, 1.0], radius: 4.0 }]), [0.0, 0.0, 12.0])
        })
        .collect();
    let refs: Vec<&Body> = pins.iter().collect();
    plate = k.boolean(BooleanOp::Cut, &plate, &refs).unwrap().body;
    let t = std::time::Instant::now();
    let mesh = Arc::new(oc_mesh::build(&k, &plate, &oc_mesh::MeshOptions::default()).unwrap());
    let tris = mesh.lods[0].triangle_count();
    println!("mesh: {tris} triangles, {} segments, {:.0} ms", mesh.lods[0].segment_count, t.elapsed().as_secs_f64() * 1e3);
    let id = r.add_mesh(mesh);
    let mut scene = Scene { bodies: vec![BodyItem::part(1, id, Rgb::hex("#c9ccd1"))], ..Default::default() };
    scene.hover = Some(Hit::Face { body: 1, index: 0 });
    let vp = Viewport::new(1920, 1080, 1.0);
    let mut cam = Camera::default();
    cam.resize(1920.0, 1080.0);
    cam.set_view("isometric", Some(&Aabb { min: DVec3::ZERO, max: DVec3::new(400.0, 300.0, 10.0) }));
    let target = r.device().create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: 1920, height: 1080, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&Default::default());
    for style in [DisplayStyle::ShadedEdges, DisplayStyle::HiddenLinesVisible] {
        scene.style = style;
        r.render(&scene, &cam, vp, &view);
        r.device().poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let n = 100;
        let t = std::time::Instant::now();
        for i in 0..n {
            cam.rotate(1.0, 0.0, DVec3::new(200.0, 150.0, 5.0), 0.4);
            scene.hover = Some(Hit::Face { body: 1, index: i % 50 });
            r.render(&scene, &cam, vp, &view);
        }
        r.device().poll(wgpu::PollType::wait_indefinitely()).unwrap();
        println!("{style:?}: {:.2} ms per frame (submit to idle, {n} frames)", t.elapsed().as_secs_f64() * 1e3 / n as f64);
    }
    let mut times: Vec<f64> = (0..50)
        .map(|i| {
            let t = std::time::Instant::now();
            let _ = r.pick(&scene, &cam, vp, 400.0 + 20.0 * i as f64, 500.0, &PickFilter::default());
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    times.sort_by(f64::total_cmp);
    println!("pick (sync): median {:.2} ms, max {:.2} ms", times[25], times[49]);
}

/// F2b: box selection of model edges, the View Selector cube, and the weight of plain
/// lines (`Options::line_width`).
#[test]
fn smoke_box_selector_and_line_weight() {
    let Some(mut r) = renderer() else { return };
    let k = OcctKernel::default();
    let body = block_hole(&k);
    let topo = k.topology(&body).unwrap();
    let part = find(&topo);
    let mesh = Arc::new(oc_mesh::build(&k, &body, &oc_mesh::MeshOptions::default()).unwrap());
    let seams = mesh.edge_flags.iter().filter(|f| *f & (oc_mesh::EDGE_SEAM | oc_mesh::EDGE_DEGENERATE) != 0).count();
    let mid = r.add_mesh(mesh);
    let mut scene = Scene { bodies: vec![BodyItem::part(7, mid, Rgb::hex("#c9ccd1"))], ..Default::default() };
    let mut cam = Camera::default();
    let vp = Viewport::new(W, H, 1.0);
    cam.resize(W as f64, H as f64);
    cam.set_view("isometric", Some(&Aabb { min: DVec3::ZERO, max: DVec3::new(20.0, 10.0, 5.0) }));
    let px = |p: [f64; 3]| cam.project(DVec3::from(p));

    // ---------------------------------------------------------------- box selection
    // A window around everything takes every edge but the seams; dragged right to left
    // (crossing) the same.
    let all = r.box_edges(&scene, &cam, [0.0, 0.0], [W as f64, H as f64], false);
    assert_eq!(all.len(), topo.edges.len() - seams, "{all:?}");
    assert_eq!(r.box_edges(&scene, &cam, [W as f64, H as f64], [0.0, 0.0], true).len(), all.len());
    // A small box on the middle of the top front edge: crossing takes it, window does not.
    let e = px([10.0, 10.0, 5.0]);
    let edge = Hit::Edge { body: 7, index: part.top_front_edge };
    let crossing = r.box_edges(&scene, &cam, [e[0] + 6.0, e[1] + 6.0], [e[0] - 6.0, e[1] - 6.0], true);
    assert_eq!(crossing, vec![edge]);
    assert!(r.box_edges(&scene, &cam, [e[0] - 6.0, e[1] - 6.0], [e[0] + 6.0, e[1] + 6.0], false).is_empty());
    // A window around just that edge's end points takes it.
    let (a, b) = (px([0.0, 10.0, 5.0]), px([20.0, 10.0, 5.0]));
    let w = r.box_edges(&scene, &cam, [a[0].min(b[0]) - 3.0, a[1].min(b[1]) - 3.0], [a[0].max(b[0]) + 3.0, a[1].max(b[1]) + 3.0], false);
    assert!(w.contains(&edge), "{w:?}");

    // ---------------------------------------------------------------- View Selector
    let mut front = cam.clone();
    front.set_view("front", None);
    let f = selector::pick(&front, W as f64 / 2.0, H as f64 / 2.0).expect("a facet at the centre");
    assert_eq!(selector::facets()[f].name, Some("Front"));
    scene.view_selector = Some(SelectorItem { hover: None });
    let plain = r.render_image(&scene, &front, vp);
    scene.view_selector = Some(SelectorItem { hover: Some(f as u32) });
    let hov = r.render_image(&scene, &front, vp);
    save(&hov, "view_selector_hover");
    scene.view_selector = None;
    let none = r.render_image(&scene, &front, vp);
    let c = |img: &Image| img.pixel(W / 2, H / 2);
    let (p0, p1, p2) = (c(&none), c(&plain), c(&hov));
    assert_ne!(p0, p1, "the cube is drawn over the model");
    assert!(p2[2] as i32 - p2[0] as i32 > 30, "hovered facet light blue: {p2:?}");
    assert!((p1[2] as i32 - p1[0] as i32).abs() < 20, "unhovered facet grey: {p1:?}");
    // Its outline: a dark ring around the Front face, 0.66 cube units from the centre.
    let s = H as f64 * 0.17;
    let ring = around(&plain, [W as f64 / 2.0 + 0.66 * s, H as f64 / 2.0], 2);
    assert!(ring.iter().any(|q| q[0] < 110), "facet outline");

    // ---------------------------------------------------------------- line weight
    // A black line at a shallow slope (1 in 8) on white: for each pixel column, the
    // strongest coverage (linear light). With 4× MSAA a 1-pixel line drops to half
    // coverage where it straddles two rows (the stippled look F1 and F2a noted); at the
    // default 1.5 device px every column keeps at least three of four samples.
    let line_scene = |width: f32| {
        let mut s = Scene { options: Options { background: Background::Plain(Rgb::WHITE), sketch_inactive: Rgb::BLACK, triad: false, line_width: width, ..Options::default() }, ..Default::default() };
        let (p, q) = (front.screen_to_plane_point(100.0, 300.0), front.screen_to_plane_point(700.0, 375.0));
        s.sketches.push(SketchItem { id: 1, segments: vec![[p, q]], profile: false });
        s
    };
    let lin = |v: u8| color::srgb_to_linear(v as f32 / 255.0);
    for scale in [1.0, 1.25] {
        let vp = Viewport::new((W as f64 * scale) as u32, (H as f64 * scale) as u32, scale);
        let mut cam = front.clone();
        cam.resize(W as f64, H as f64);
        let mut min_peak = |width: f32| {
            let img = r.render_image(&line_scene(width), &cam, vp);
            save(&img, &format!("line_{width}_{scale}"));
            let (x0, x1) = ((120.0 * scale) as u32, (680.0 * scale) as u32);
            (x0..x1)
                .map(|x| (0..img.height).map(|y| 1.0 - lin(img.pixel(x, y)[0])).fold(0.0f32, f32::max))
                .fold(1.0f32, f32::min)
        };
        let (one, wide) = (min_peak(1.0), min_peak(Options::default().line_width));
        println!("scale {scale}: weakest column {one:.2} at 1 px, {wide:.2} at 1.5 px");
        assert!(wide >= 0.7, "1.5 px line solid at scale {scale}: {wide}");
        assert!(wide > one, "wider is heavier: {wide} vs {one}");
    }
}

/// G1: two instances of one uploaded mesh, the second turned 90° about Z and moved. Both are
/// drawn, picked, ray-cast and box-selected at their placements; the mesh is uploaded once.
#[test]
fn smoke_instances_share_one_mesh() {
    let Some(mut r) = renderer() else { return };
    let k = OcctKernel::default();
    let body = block_hole(&k);
    let part = find(&k.topology(&body).unwrap());
    let mesh = Arc::new(oc_mesh::build(&k, &body, &oc_mesh::MeshOptions::default()).unwrap());
    let mid = r.add_mesh(mesh);
    let turn = Placement { rotation: glam::DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2), translation: DVec3::new(60.0, 0.0, 0.0) };
    let mut second = BodyItem::part(2, mid, Rgb::hex("#c9ccd1"));
    second.placement = turn;
    let scene = Scene { bodies: vec![BodyItem::part(1, mid, Rgb::hex("#c9ccd1")), second], ..Default::default() };
    let mut cam = Camera::default();
    let vp = Viewport::new(W, H, 1.0);
    cam.resize(W as f64, H as f64);
    let fit = Aabb { min: DVec3::new(-5.0, -5.0, 0.0), max: DVec3::new(65.0, 25.0, 5.0) };
    cam.set_view("isometric", Some(&fit));
    let img = r.render_image(&scene, &cam, vp);
    save(&img, "instances");
    // A point on the top face of each instance (block local (3, 2.5, 5)).
    let local = DVec3::new(3.0, 2.5, 5.0);
    let filter = PickFilter::default();
    for (id, at) in [(1u32, local), (2u32, turn.apply(local))] {
        let s = cam.project(at);
        let p = img.pixel(s[0] as u32, s[1] as u32);
        assert!(p[0] > 90 && p[3] == 255, "instance {id} drawn at {s:?}: {p:?}");
        assert_eq!(r.pick(&scene, &cam, vp, s[0], s[1], &filter), Some(Hit::Face { body: id, index: part.top }), "pick instance {id}");
        let hits = r.raycast_faces(&scene, &cam.ray(s[0], s[1]));
        let first = hits.first().expect("ray hits");
        assert_eq!(first.hit, Hit::Face { body: id, index: part.top });
        assert!((first.point - at).length() < 1e-6, "ray point on instance {id}: {:?}", first.point);
    }
    // The hole of the turned instance is at local (10, 5) → world (55, 10).
    let hole = cam.project(DVec3::new(55.0, 10.0, 5.0));
    let near_hole = r.raycast_faces(&scene, &cam.ray(hole[0], hole[1]));
    assert!(near_hole.iter().all(|h| h.hit != Hit::Face { body: 2, index: part.top }), "the turned instance has its hole there");
    // A window box around the second instance takes only its edges.
    let (a, b) = (cam.project(DVec3::new(48.0, -2.0, 0.0)), cam.project(DVec3::new(62.0, 22.0, 5.0)));
    let (from, to) = ([a[0].min(b[0]) - 30.0, a[1].min(b[1]) - 30.0], [a[0].max(b[0]) + 30.0, a[1].max(b[1]) + 30.0]);
    let edges = r.box_edges(&scene, &cam, from, to, false);
    assert!(!edges.is_empty() && edges.iter().all(|h| matches!(h, Hit::Edge { body: 2, .. })), "{edges:?}");
}
