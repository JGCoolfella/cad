//! G2b (level of detail while the view moves): what the app relies on when it switches a
//! scene's bodies between `LodChoice::Finest` (the view at rest) and `LodChoice::Auto`
//! (the view moving).
//!
//! - The level drawn follows the threshold and the zoom, by the rule of `level_of` in
//!   `cull.wgsl`.
//! - Picks find what that level draws: where the coarse level leaves the background a pick
//!   finds nothing, where it covers the hole a pick finds the face; at full detail the
//!   other way round.
//! - The full-detail frame after the motion is the image of a view that never moved (no
//!   stale level kept by the renderer's write-on-change buffers).
//!
//! Without a GPU adapter the tests print SKIP and pass, unless `OC_RENDER_REQUIRE_GPU=1`.
use glam::DVec3;
use oc_kernel::{BooleanOp, Kernel, PrismSpec, Profile, Segment, Wire, P3};
use oc_kernel_occt::{Body, OcctKernel};
use oc_render::*;
use std::sync::Arc;

const W: u32 = 480;
const H: u32 = 400;

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
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("oc-render-lod");
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

fn circle(r: f64, z: f64) -> Wire {
    Wire::new(vec![Segment::Circle { center: [0.0, 0.0, z], normal: [0.0, 0.0, 1.0], radius: r }])
}

/// A Ø40 disc, 4 thick, with a Ø24 hole: the cylinders' levels differ a lot.
fn ring(r: &mut Renderer) -> (MeshId, Arc<oc_mesh::RenderMesh>) {
    let k = OcctKernel::default();
    let disc = prism(&k, circle(20.0, 0.0), [0.0, 0.0, 4.0]);
    let pin = prism(&k, circle(12.0, -1.0), [0.0, 0.0, 6.0]);
    let body = k.boolean(BooleanOp::Cut, &disc, &[&pin]).unwrap().body;
    let m = Arc::new(oc_mesh::build(&k, &body, &oc_mesh::MeshOptions::default()).unwrap());
    println!("levels (linear mm, triangles): {:?}", m.lods.iter().map(|l| (l.params.linear, l.triangle_count())).collect::<Vec<_>>());
    assert!(m.lods.len() >= 3, "the mesh has coarser levels");
    (r.add_mesh(m.clone()), m)
}

/// The ring seen from the front (along -Z), filling most of the view, on a white background.
fn setup(style: DisplayStyle, mesh: MeshId) -> (Scene, Camera) {
    let mut cam = Camera::default();
    cam.resize(W as f64, H as f64);
    cam.set_view("front", Some(&Aabb { min: DVec3::new(-20.0, -20.0, 0.0), max: DVec3::new(20.0, 20.0, 4.0) }));
    let options = Options { background: Background::Plain(Rgb::hex("#ffffff")), triad: false, ..Options::default() };
    let scene = Scene { style, options, bodies: vec![BodyItem::part(7, mesh, Rgb::hex("#3060c0"))], ..Default::default() };
    (scene, cam)
}

fn with_lod(s: &Scene, lod: LodChoice) -> Scene {
    let mut s = s.clone();
    for b in &mut s.bodies {
        b.lod = lod;
    }
    s
}

/// The threshold (px) that selects level `k` and no coarser one at this zoom.
fn threshold_for(m: &oc_mesh::RenderMesh, cam: &Camera, k: usize) -> f64 {
    m.lods[k].params.linear / cam.world_per_pixel() * 1.05
}

fn level_drawn(r: &Renderer) -> usize {
    let st = r.draw_stats();
    assert_eq!(st.opaque_levels.iter().sum::<u32>(), 1, "{st:?}");
    st.opaque_levels.iter().position(|&n| n == 1).unwrap()
}

/// Model (the blue ring) rather than the white background.
fn model(img: &Image, x: i32, y: i32) -> bool {
    img.pixel(x as u32, y as u32)[0] < 200
}

/// Whether every pixel within `r` of (x, y) is (`want`) model or background.
fn around(img: &Image, x: i32, y: i32, r: i32, want: bool) -> bool {
    (-r..=r).all(|dy| (-r..=r).all(|dx| model(img, x + dx, y + dy) == want))
}

/// Pixels on circles about the ring's centre where `a` shows `in_a` and `b` the opposite,
/// with 3 px of the same around them in both images (the face pick radius with a filter).
fn differing(a: &Image, b: &Image, cam: &Camera, radii: &[f64], in_a: bool) -> Vec<(i32, i32)> {
    let c = cam.project(DVec3::new(0.0, 0.0, 4.0));
    let mut out = vec![];
    for &rad in radii {
        let px = rad / cam.world_per_pixel();
        for i in 0..720 {
            let t = i as f64 * std::f64::consts::PI / 360.0;
            let (x, y) = ((c[0] + px * t.cos()).round() as i32, (c[1] + px * t.sin()).round() as i32);
            if x < 8 || y < 8 || x >= W as i32 - 8 || y >= H as i32 - 8 {
                continue;
            }
            if around(a, x, y, 3, in_a) && around(b, x, y, 3, !in_a) && !out.contains(&(x, y)) {
                out.push((x, y));
            }
        }
    }
    out
}

#[test]
fn picks_find_what_the_level_drawn_shows() {
    let Some(mut r) = renderer() else { return };
    let (id, m) = ring(&mut r);
    let (fine, mut cam) = setup(DisplayStyle::Shaded, id);
    // Zoomed onto the right of the ring (the hole's edge and the outside): the coarse
    // level's chords stand 10 px and more from the circles.
    cam.set_view("front", Some(&Aabb { min: DVec3::new(10.0, -4.0, 0.0), max: DVec3::new(21.0, 4.0, 4.0) }));
    let vp = Viewport::new(W, H, 1.0);
    let k = m.lods.len() - 1;
    let coarse = with_lod(&fine, LodChoice::Auto { max_error_px: threshold_for(&m, &cam, k) });
    // One level finer than the coarsest is not chosen with a threshold just under it.
    let mid = with_lod(&fine, LodChoice::Auto { max_error_px: threshold_for(&m, &cam, k - 1) });
    let _ = r.render_image(&mid, &cam, vp);
    assert_eq!(level_drawn(&r), k - 1, "the threshold selects the level");
    let img_fine = r.render_image(&fine, &cam, vp);
    assert_eq!(level_drawn(&r), 0);
    let img_coarse = r.render_image(&coarse, &cam, vp);
    assert_eq!(level_drawn(&r), k);
    save(&img_fine, "pick-fine");
    save(&img_coarse, "pick-coarse");
    // Between the outer circle and the coarse level's chords: model at full detail only.
    // Inside the hole, by the coarse level's chords: model in the coarse level only.
    let radii: Vec<f64> = (100..=200).map(|r| r as f64 / 10.0).collect();
    let outside = differing(&img_fine, &img_coarse, &cam, &radii[60..], true);
    let inside = differing(&img_coarse, &img_fine, &cam, &radii[..25], true);
    println!("{} pixels outside the coarse disc, {} inside the hole", outside.len(), inside.len());
    assert!(outside.len() >= 4 && inside.len() >= 4, "the levels differ by several pixels");
    let faces = PickFilter { only: Some(vec![PickKind::Face]), edges_only: false };
    for &(x, y) in outside.iter().chain(&inside) {
        let shown_coarse = model(&img_coarse, x, y);
        // The coarse scene is the one drawn last, then the fine one: each pick runs its own
        // scene's levels, whatever was drawn before.
        for (scene, shown) in [(&coarse, shown_coarse), (&fine, !shown_coarse), (&coarse, shown_coarse)] {
            let hit = r.pick(scene, &cam, vp, x as f64, y as f64, &faces);
            assert_eq!(matches!(hit, Some(Hit::Face { body: 7, .. })), shown, "pick at ({x}, {y}): {hit:?}");
        }
    }
}

#[test]
fn the_frame_after_the_motion_is_full_detail() {
    let Some(mut r) = renderer() else { return };
    let (id, m) = ring(&mut r);
    let vp = Viewport::new(W, H, 1.0);
    for style in [DisplayStyle::ShadedEdges, DisplayStyle::HiddenLinesVisible, DisplayStyle::Wireframe] {
        let (still, cam) = setup(style, id);
        let moving = with_lod(&still, LodChoice::Auto { max_error_px: threshold_for(&m, &cam, 2) });
        // Twice: the second frame's occlusion prepass uses the first's visibility.
        let _ = r.render_image(&still, &cam, vp);
        let before = r.render_image(&still, &cam, vp);
        // The view turns: coarse frames, the camera moving a little each time.
        let mut turned = cam.clone();
        for n in 0..4 {
            turned.rotate(3.0, 1.0, DVec3::new(0.0, 0.0, 2.0), 0.5);
            let img = r.render_image(&moving, &turned, vp);
            // Opaque faces are counted per level (line styles draw none).
            if style == DisplayStyle::ShadedEdges {
                assert_eq!(level_drawn(&r), 2, "{style:?}: coarse while moving");
            }
            if n == 0 {
                save(&img, &format!("moving-{style:?}"));
            }
        }
        // Back where it was, still coarse (the last moving frame), then at rest.
        let coarse = r.render_image(&moving, &cam, vp);
        assert!(coarse.rgba != before.rgba, "{style:?}: the coarse level looks different");
        let after = r.render_image(&still, &cam, vp);
        if style == DisplayStyle::ShadedEdges {
            assert_eq!(level_drawn(&r), 0, "{style:?}: full detail at rest");
        }
        assert!(after.rgba == before.rgba, "{style:?}: the image of a view that never moved");
    }
}

