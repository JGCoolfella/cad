//! G2 (GPU-driven rendering): a 10k-component assembly benchmark.
//!
//! `scale_10k_benchmark` is ignored by default (it meshes 100 OCCT parts and draws 10 000
//! instances of them); run it in release:
//!
//! ```text
//! cargo test -p oc-render --release --test scale -- --ignored --nocapture
//! ```
//!
//! It prints the adapter, the scene size and, per layout, size, display style and culling
//! mode, the frame time (submit to idle over a rotating camera, CPU encoding included, so
//! 1000 / ms is the frame rate), the CPU time of `render` alone, the synchronous pick time
//! and how many instances the last frame drew with opaque faces.
//!
//! `OC_BENCH_QUICK=1` measures only the default culling at 1920 × 1080.
//! `OC_BENCH_ADAPTER=<part of a name>` runs on that adapter (e.g. `Radeon` for an
//! integrated GPU) instead of the high-performance one.
use glam::DVec3;
use oc_kernel::{BooleanOp, Kernel, PrismSpec, Profile, Segment, Wire, P3};
use oc_kernel_occt::{Body, OcctKernel};
use oc_render::*;
use std::sync::Arc;
use std::time::Instant;

fn renderer() -> Option<Renderer> {
    if let Ok(name) = std::env::var("OC_BENCH_ADAPTER") {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapters = pollster::block_on(instance.enumerate_adapters(wgpu::Backends::all()));
        let names: Vec<String> = adapters.iter().map(|a| format!("{} ({:?})", a.get_info().name, a.get_info().backend)).collect();
        let adapter = adapters.into_iter().find(|a| a.get_info().name.contains(&name)).unwrap_or_else(|| panic!("no adapter named {name}: {names:?}"));
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("bench"),
            required_features: Renderer::features(adapter.features()),
            required_limits: Renderer::limits(&adapter.limits()),
            ..Default::default()
        }))
        .unwrap();
        return Some(Renderer::new(device, queue, adapter.get_info(), wgpu::TextureFormat::Rgba8UnormSrgb));
    }
    match Renderer::headless() {
        Ok(r) => Some(r),
        Err(e) if std::env::var("OC_RENDER_REQUIRE_GPU").is_err() => {
            println!("SKIP: {e}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// Part `i` of the benchmark set: a block of varying size with 1 to 4 through holes of
/// varying radius (a typical machined component: planes, cylinders, many edges).
fn part(k: &OcctKernel, i: usize) -> Body {
    let prism = |w: Wire, v: P3| k.prism(&PrismSpec { profile: Profile::single(w), vector: v, taper: None }).unwrap().body;
    let (w, d, h) = (20.0 + (i % 7) as f64 * 3.0, 15.0 + (i % 5) as f64 * 2.0, 5.0 + (i % 3) as f64 * 2.0);
    let block = prism(Wire::rect_xy(0.0, 0.0, w, d, 0.0), [0.0, 0.0, h]);
    let holes = 1 + i % 4;
    let r = 1.5 + (i % 3) as f64 * 0.5;
    let pins: Vec<Body> = (0..holes)
        .map(|j| {
            let c = [w * (j as f64 + 0.5) / holes as f64, d / 2.0, -1.0];
            prism(Wire::new(vec![Segment::Circle { center: c, normal: [0.0, 0.0, 1.0], radius: r }]), [0.0, 0.0, h + 2.0])
        })
        .collect();
    let refs: Vec<&Body> = pins.iter().collect();
    k.boolean(BooleanOp::Cut, &block, &refs).unwrap().body
}

/// 100 distinct meshes uploaded once.
fn meshes(r: &mut Renderer) -> (Vec<MeshId>, usize, usize) {
    let k = OcctKernel::default();
    let t = Instant::now();
    let mut ids = vec![];
    let (mut tris, mut segs) = (0, 0);
    for i in 0..100 {
        let m = Arc::new(oc_mesh::build(&k, &part(&k, i), &oc_mesh::MeshOptions::default()).unwrap());
        tris += m.lods[0].triangle_count();
        segs += m.lods[0].segment_count as usize;
        ids.push(r.add_mesh(m));
    }
    println!("100 meshes: {tris} triangles, {segs} edge segments at level 0, {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    (ids, tris, segs)
}

/// 10 000 instances: a 25 × 20 × 20 block of components 45 mm apart ("cube": many seen
/// through the gaps), the same block packed 24 mm apart ("dense": the components touch and
/// interpenetrate, most are hidden), or a 100 × 100 sheet ("sheet", all visible from above
/// at an angle). Each turned about Z by its index.
fn scene(ids: &[MeshId], layout: &str, lod: LodChoice) -> (Scene, Aabb) {
    let mut bodies = vec![];
    let mut fit = Aabb::EMPTY;
    for n in 0..10_000u32 {
        let p = match layout {
            "cube" => DVec3::new((n % 25) as f64, ((n / 25) % 20) as f64, (n / 500) as f64) * 45.0,
            "dense" => DVec3::new((n % 25) as f64, ((n / 25) % 20) as f64, (n / 500) as f64) * 24.0,
            _ => DVec3::new((n % 100) as f64, (n / 100) as f64, 0.0) * 45.0,
        };
        let mut b = BodyItem::part(n, ids[(n as usize * 37) % ids.len()], Rgb::hex("#c9ccd1"));
        b.placement = Placement { rotation: glam::DQuat::from_rotation_z(n as f64 * 0.7), translation: p };
        b.lod = lod;
        fit.add(p - DVec3::splat(25.0));
        fit.add(p + DVec3::splat(25.0));
        bodies.push(b);
    }
    (Scene { bodies, ..Default::default() }, fit)
}

fn target(r: &Renderer, w: u32, h: u32) -> wgpu::TextureView {
    r.device()
        .create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

struct Timing {
    frame_ms: f64,
    cpu_ms: f64,
    pick_ms: f64,
    box_ms: f64,
    boxed: usize,
}

fn measure(r: &mut Renderer, scene: &mut Scene, fit: &Aabb, (w, h): (u32, u32)) -> Timing {
    let vp = Viewport::new(w, h, 1.0);
    let mut cam = Camera::default();
    cam.resize(w as f64, h as f64);
    cam.set_view("isometric", Some(fit));
    let view = target(r, w, h);
    let center = fit.center();
    // Warm up (pipelines, buffers, the first frame's visibility).
    for _ in 0..3 {
        r.render(scene, &cam, vp, &view);
    }
    r.device().poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let n = 60;
    let mut cpu = 0.0;
    let t = Instant::now();
    for i in 0..n {
        cam.rotate(1.5, 0.0, center, 0.4);
        scene.hover = Some(Hit::Face { body: (i * 97) % 10_000, index: 0 });
        let c = Instant::now();
        r.render(scene, &cam, vp, &view);
        cpu += c.elapsed().as_secs_f64();
    }
    r.device().poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let frame_ms = t.elapsed().as_secs_f64() * 1e3 / n as f64;
    let mut picks: Vec<f64> = (0..21)
        .map(|i| {
            let t = Instant::now();
            let _ = r.pick(scene, &cam, vp, w as f64 * (0.2 + 0.03 * i as f64), h as f64 * 0.5, &PickFilter::default());
            t.elapsed().as_secs_f64() * 1e3
        })
        .collect();
    picks.sort_by(f64::total_cmp);
    // A crossing box selection over a tenth of the view (CPU, level 0).
    let t = Instant::now();
    let (cx, cy) = (w as f64 * 0.5, h as f64 * 0.5);
    let boxed = r.box_edges(scene, &cam, [cx + 0.15 * w as f64, cy + 0.15 * h as f64], [cx - 0.15 * w as f64, cy - 0.15 * h as f64], true).len();
    let box_ms = t.elapsed().as_secs_f64() * 1e3;
    Timing { frame_ms, cpu_ms: cpu * 1e3 / n as f64, pick_ms: picks[10], box_ms, boxed }
}

#[test]
#[ignore]
fn scale_10k_benchmark() {
    let Some(mut r) = renderer() else { return };
    let info = r.adapter_info().clone();
    println!("adapter: {} ({:?}, {:?}, driver {} {})", info.name, info.backend, info.device_type, info.driver, info.driver_info);
    let (ids, tris, segs) = meshes(&mut r);
    // Every 37th mesh: the instance mix covers all 100 meshes, 100 instances each.
    println!("10 000 instances ≈ {:.1} M triangles and {:.1} M edge segments without culling", tris as f64 * 100.0 / 1e6, segs as f64 * 100.0 / 1e6);
    let quick = std::env::var("OC_BENCH_QUICK").is_ok_and(|v| v == "1");
    let modes: &[(&str, Culling, LodChoice)] = if quick {
        &[("culled", Culling { gpu: true, frustum: true, occlusion: true }, LodChoice::Finest)]
    } else {
        &[
            ("one draw per body", Culling { gpu: false, frustum: false, occlusion: false }, LodChoice::Finest),
            ("gpu lists, no culling", Culling { gpu: true, frustum: false, occlusion: false }, LodChoice::Finest),
            ("frustum", Culling { gpu: true, frustum: true, occlusion: false }, LodChoice::Finest),
            ("frustum + occlusion", Culling { gpu: true, frustum: true, occlusion: true }, LodChoice::Finest),
            ("frustum + occlusion + auto LOD 0.5 px", Culling { gpu: true, frustum: true, occlusion: true }, LodChoice::Auto { max_error_px: 0.5 }),
        ]
    };
    let sizes: &[(u32, u32)] = if quick { &[(1920, 1080)] } else { &[(1920, 1080), (2560, 1440)] };
    for layout in ["cube", "dense", "sheet"] {
        for &size in sizes {
            for style in [DisplayStyle::ShadedEdges, DisplayStyle::HiddenLinesVisible] {
                for (name, culling, lod) in modes {
                    let (mut scene, fit) = scene(&ids, layout, *lod);
                    scene.style = style;
                    r.set_culling(*culling);
                    let t = measure(&mut r, &mut scene, &fit, size);
                    let st = r.draw_stats();
                    let drawn = match (culling.gpu, culling.occlusion) {
                        (false, _) => "every body drawn".to_string(),
                        (true, false) => format!("opaque drawn {} of 10000, levels {:?}", st.kinds[1], st.opaque_levels),
                        (true, true) => format!("opaque drawn {} of 10000 ({} in the prepass), levels {:?}", st.kinds[1], st.kinds[0], st.opaque_levels),
                    };
                    println!(
                        "{layout} {}x{} {style:?} [{name}]: {:.2} ms per frame ({:.0} fps), render() CPU {:.2} ms, pick {:.2} ms, box selection {:.0} ms ({} edges); {drawn}",
                        size.0,
                        size.1,
                        t.frame_ms,
                        1e3 / t.frame_ms,
                        t.cpu_ms,
                        t.pick_ms,
                        t.box_ms,
                        t.boxed,
                    );
                }
            }
        }
    }
}
