//! P2 spike: Tauri 2 window whose client area is a wgpu swapchain, with the (transparent)
//! WebView2 UI composited on top. Runs a self-test and exits, printing a verdict:
//!   1. overlay: screen pixels under the HTML menu show the menu, pixels elsewhere show the
//!      wgpu clear colour (teal) — i.e. menus can drop over the viewport;
//!   2. input latency: WM_MOUSEMOVE posted to the webview's input window → DOM pointermove →
//!      IPC → Rust. This is the latency a webview-hosted viewport adds over a native one;
//!   3. resize: 30 programmatic resizes, screen grabbed after each, counting frames where the
//!      viewport is not teal (black/white flashes);
//!   4. DPI and monitors.
//!
//! Run: cargo run --release (from spikes/p2-tauri)
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Manager, PhysicalSize, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

#[derive(Default)]
struct Shared {
    sent: Mutex<HashMap<(i32, i32), Instant>>,
    lat: Mutex<Vec<f64>>,
    ready: Mutex<Option<f64>>,
    resize: Mutex<Option<(u32, u32)>>,
    frames: Mutex<Vec<f64>>,
    quit: AtomicBool,
}

#[tauri::command]
fn pointer(x: i32, y: i32, s: State<Arc<Shared>>) {
    if let Some(t) = s.sent.lock().unwrap().remove(&(x, y)) {
        s.lat.lock().unwrap().push(t.elapsed().as_secs_f64() * 1e3);
    }
}

#[tauri::command]
fn ready(dpr: f64, s: State<Arc<Shared>>) {
    *s.ready.lock().unwrap() = Some(dpr);
}

fn render_loop(win: WebviewWindow, s: Arc<Shared>) {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: match std::env::var("P2_BACKEND").as_deref() {
            Ok("dx12") => wgpu::Backends::DX12,
            Ok("vulkan") => wgpu::Backends::VULKAN,
            _ => wgpu::Backends::PRIMARY,
        },
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let surface = instance.create_surface(win.clone()).expect("surface");
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: Some(&surface),
        ..Default::default()
    }))
    .expect("adapter");
    let info = adapter.get_info();
    println!("viewport adapter: {} ({:?})", info.name, info.backend);
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).expect("device");
    let size = win.inner_size().unwrap();
    let caps = surface.get_capabilities(&adapter);
    let mut config = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).unwrap();
    if let Some(f) = caps.formats.iter().find(|f| !f.is_srgb()) {
        config.format = *f;
    }
    config.present_mode = wgpu::PresentMode::Fifo;
    config.desired_maximum_frame_latency = 1;
    surface.configure(&device, &config);
    let mut last = Instant::now();
    while !s.quit.load(Ordering::Relaxed) {
        if let Some((w, h)) = s.resize.lock().unwrap().take() {
            config.width = w.max(1);
            config.height = h.max(1);
            surface.configure(&device, &config);
        }
        let frame = match surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.configure(&device, &config);
                continue;
            }
            _ => {
                std::thread::sleep(Duration::from_millis(5));
                continue;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let mut enc = device.create_command_encoder(&Default::default());
        enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.5, b: 0.5, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        queue.submit([enc.finish()]);
        queue.present(frame);
        s.frames.lock().unwrap().push(last.elapsed().as_secs_f64() * 1e3);
        last = Instant::now();
    }
}

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    /// BGRA pixels of a screen rectangle (the composited desktop, so it includes the swapchain).
    pub fn grab(x: i32, y: i32, w: i32, h: i32) -> Vec<u8> {
        unsafe {
            let screen = GetDC(std::ptr::null_mut());
            let mem = CreateCompatibleDC(screen);
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(mem, bmp as _);
            BitBlt(mem, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT);
            let mut bi: BITMAPINFO = std::mem::zeroed();
            bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bi.bmiHeader.biWidth = w;
            bi.bmiHeader.biHeight = -h;
            bi.bmiHeader.biPlanes = 1;
            bi.bmiHeader.biBitCount = 32;
            bi.bmiHeader.biCompression = BI_RGB as u32;
            let mut buf = vec![0u8; (w * h * 4) as usize];
            GetDIBits(mem, bmp, 0, h as u32, buf.as_mut_ptr() as _, &mut bi, DIB_RGB_COLORS);
            SelectObject(mem, old);
            DeleteObject(bmp as _);
            DeleteDC(mem);
            ReleaseDC(std::ptr::null_mut(), screen);
            buf
        }
    }

    unsafe extern "system" fn find(h: HWND, out: LPARAM) -> windows_sys::core::BOOL {
        let mut name = [0u16; 128];
        let n = GetClassNameW(h, name.as_mut_ptr(), 128);
        let s = String::from_utf16_lossy(&name[..n as usize]);
        if s == "Chrome_RenderWidgetHostHWND" {
            *(out as *mut HWND) = h;
            return 0;
        }
        1
    }

    /// The window inside WebView2 that receives mouse input.
    pub fn input_window(top: isize) -> Option<isize> {
        let mut out: HWND = std::ptr::null_mut();
        unsafe { EnumChildWindows(top as HWND, Some(find), &mut out as *mut HWND as LPARAM) };
        (!out.is_null()).then_some(out as isize)
    }

    pub fn post_move(h: isize, x: i32, y: i32) {
        let lp = ((y as u32 & 0xFFFF) << 16 | (x as u32 & 0xFFFF)) as LPARAM;
        unsafe { PostMessageW(h as HWND, WM_MOUSEMOVE, 0, lp) };
    }
}

/// BGRA pixels of the window's client area as composited on screen, or None if this
/// platform/session does not allow screen capture.
#[cfg(windows)]
fn grab_window(win: &WebviewWindow) -> Option<(Vec<u8>, i32, i32)> {
    let pos = win.inner_position().ok()?;
    let size = win.inner_size().ok()?;
    let (w, h) = (size.width as i32, size.height as i32);
    let buf = win::grab(pos.x, pos.y, w, h);
    (buf.len() >= (w * h * 4) as usize).then_some((buf, w, h))
}

#[cfg(not(windows))]
fn grab_window(win: &WebviewWindow) -> Option<(Vec<u8>, i32, i32)> {
    let pos = win.inner_position().ok()?;
    let size = win.inner_size().ok()?;
    let mon = win.current_monitor().ok()??;
    let monitors = xcap::Monitor::all().ok()?;
    let m = monitors.iter().find(|m| m.is_primary().unwrap_or(false)).or(monitors.first())?;
    let img = match m.capture_image() {
        Ok(i) => i,
        Err(e) => {
            println!("  screen capture failed: {e}");
            return None;
        }
    };
    // Map Tauri physical pixels onto the captured image.
    let sc = img.width() as f64 / mon.size().width as f64;
    let x0 = ((pos.x - mon.position().x) as f64 * sc) as i32;
    let y0 = ((pos.y - mon.position().y) as f64 * sc) as i32;
    let (w, h) = (size.width as i32, size.height as i32);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (ix, iy) = (x0 + (x as f64 * sc) as i32, y0 + (y as f64 * sc) as i32);
            if ix < 0 || iy < 0 || ix >= img.width() as i32 || iy >= img.height() as i32 {
                continue;
            }
            let p = img.get_pixel(ix as u32, iy as u32).0;
            let i = ((y * w + x) * 4) as usize;
            buf[i..i + 4].copy_from_slice(&[p[2], p[1], p[0], 255]);
        }
    }
    Some((buf, w, h))
}

fn pixel(buf: &[u8], w: i32, x: i32, y: i32) -> (u8, u8, u8) {
    let i = ((y * w + x) * 4) as usize;
    (buf[i + 2], buf[i + 1], buf[i])
}

fn teal((r, g, b): (u8, u8, u8)) -> bool {
    r < 20 && (110..150).contains(&g) && (110..150).contains(&b)
}

fn save_png(path: &str, bgra: &[u8], w: i32, h: i32) {
    let rgba: Vec<u8> = bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&rgba).unwrap();
}

fn median(v: &mut [f64]) -> (f64, f64, f64) {
    if v.is_empty() {
        return (f64::NAN, f64::NAN, f64::NAN);
    }
    v.sort_by(f64::total_cmp);
    (v[v.len() / 2], v[v.len() * 95 / 100], v[v.len() - 1])
}

fn self_test(win: WebviewWindow, s: Arc<Shared>) {
    let t = Instant::now();
    let dpr = loop {
        if let Some(d) = *s.ready.lock().unwrap() {
            break d;
        }
        assert!(t.elapsed() < Duration::from_secs(20), "webview never became ready");
        std::thread::sleep(Duration::from_millis(20));
    };
    println!("webview ready after {:.0} ms, devicePixelRatio {dpr}", t.elapsed().as_secs_f64() * 1e3);
    std::thread::sleep(Duration::from_millis(1500));

    // 1. Overlay.
    let check = |label: &str, save: Option<&str>| -> Option<bool> {
        let Some((buf, w, h)) = grab_window(&win) else {
            println!("  {label}: screen capture unavailable");
            return None;
        };
        let sc = dpr;
        let at = |x: f64, y: f64| pixel(&buf, w, (x * sc) as i32, (y * sc) as i32);
        let menu = at(120.0, 120.0);
        let bar = at(400.0, 16.0);
        let view = at(w as f64 / sc * 0.7, h as f64 / sc * 0.6);
        let ok = menu.0 > 240 && menu.1 > 240 && bar.0 > 220 && bar.0 < 240 && teal(view);
        println!("  {label}: menu {menu:?}, bar {bar:?}, viewport {view:?} -> {}", if ok { "ok" } else { "FAIL" });
        if let Some(p) = save {
            save_png(p, &buf, w, h);
        }
        Some(ok)
    };
    println!("overlay:");
    let overlay = check("initial", Some("p2-overlay.png"));

    // 2. Input latency: 400 moves at 120 Hz, each delivered by `send`.
    let measure = |label: &str, send: &dyn Fn(i32, i32)| -> (usize, (f64, f64, f64)) {
        s.lat.lock().unwrap().clear();
        s.sent.lock().unwrap().clear();
        let n = 400;
        for i in 0..n {
            let (x, y) = (300 + (i % 300) * 2, 300 + (i / 300) * 7 + (i % 3));
            s.sent.lock().unwrap().insert((x, y), Instant::now());
            send(x, y);
            std::thread::sleep(Duration::from_micros(8333));
        }
        std::thread::sleep(Duration::from_millis(300));
        let mut lat = s.lat.lock().unwrap().clone();
        let r = median(&mut lat);
        println!(
            "  {label}: {}/{n} moves reached Rust; latency median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
            lat.len(),
            r.0,
            r.1,
            r.2
        );
        (lat.len(), r)
    };
    println!("input:");
    // OS-level injection exists only on Windows; elsewhere the eval round trip (two IPC
    // hops instead of one) is the measured upper bound.
    #[cfg(windows)]
    let os = {
        let hwnd = win.hwnd().unwrap().0 as isize;
        let input = win::input_window(hwnd).expect("no WebView2 input window");
        measure("OS mouse message -> DOM -> IPC", &|x, y| win::post_move(input, x, y))
    };
    let eval = measure("Rust -> eval -> DOM -> IPC round trip", &|x, y| {
        let _ = win.eval(format!("__p2move({x},{y})"));
    });
    #[cfg(not(windows))]
    let os = eval;
    let (delivered, lat_summary) = os;
    let _ = eval;

    // 3. Resize flicker.
    println!("resize:");
    let base = win.inner_size().unwrap();
    let mut bad = 0;
    let mut captured = true;
    let steps = 30;
    for k in 0..steps {
        let size = PhysicalSize::new(base.width - 300 + (k % 10) * 60, base.height - 200 + (k % 7) * 50);
        win.set_size(size).unwrap();
        std::thread::sleep(Duration::from_millis(if k % 2 == 0 { 0 } else { 16 }));
        let Some((buf, w, h)) = grab_window(&win) else {
            captured = false;
            break;
        };
        if w < 100 || h < 100 {
            println!("  step {k}: window reports {w}x{h}, skipped");
            continue;
        }
        // Sample a grid over the viewport area (below the menu bar, right of the menu).
        let mut wrong = 0;
        for gy in 1..6 {
            for gx in 2..8 {
                let p = pixel(&buf, w, w * gx / 9, h * gy / 7 + (40.0 * dpr) as i32);
                if !teal(p) {
                    wrong += 1;
                }
            }
        }
        if wrong > 0 {
            bad += 1;
            if bad == 1 {
                save_png("p2-resize-bad.png", &buf, w, h);
            }
        }
    }
    win.set_size(base).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    if captured {
        println!("  {bad}/{steps} grabs during resizing showed non-viewport pixels in the viewport");
    } else {
        println!("  screen capture unavailable, resize not checked");
    }
    let after = check("after resize", None);

    // 4. DPI and monitors.
    let monitors = win.available_monitors().unwrap();
    println!("monitors: {}", monitors.len());
    for m in &monitors {
        println!("  {:?} {}x{} scale {}", m.name(), m.size().width, m.size().height, m.scale_factor());
    }
    let mut frames = s.frames.lock().unwrap().clone();
    println!("frames presented: {}", frames.len());
    if frames.len() > 20 {
        let f = median(&mut frames[10..]);
        println!("present interval: median {:.2} ms (refresh about {:.0} Hz)", f.0, 1000.0 / f.0);
    }

    let fmt = |o: Option<bool>| match o {
        Some(true) => "ok",
        Some(false) => "FAIL",
        None => "n/a",
    };
    let pass = overlay != Some(false)
        && after != Some(false)
        && (!captured || bad == 0)
        && frames.len() > 20
        && delivered > 0
        && lat_summary.1 < 8.0;
    println!(
        "\nVERDICT {} overlay={} input_median={:.2}ms input_p95={:.2}ms delivered={delivered} resize_bad={} after_resize={} frames={}",
        if pass { "PASS" } else { "FAIL" },
        fmt(overlay),
        lat_summary.0,
        lat_summary.1,
        if captured { format!("{bad}/{steps}") } else { "n/a".into() },
        fmt(after),
        frames.len()
    );
    s.quit.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(100));
    win.app_handle().exit(if pass { 0 } else { 2 });
}

fn main() {
    // Watchdog: a window that never shows (no display, no GPU) must fail the run, not hang CI.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(120));
        println!("\nVERDICT FAIL watchdog: self-test did not finish in 120 s");
        std::process::exit(3);
    });
    let shared = Arc::new(Shared::default());
    tauri::Builder::default()
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![pointer, ready])
        .setup(move |app| {
            let win = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("P2 spike")
                .inner_size(1280.0, 800.0)
                .transparent(true)
                .always_on_top(true)
                .build()?;
            let s = shared.clone();
            win.on_window_event(move |e| {
                if let WindowEvent::Resized(size) = e {
                    *s.resize.lock().unwrap() = Some((size.width, size.height));
                }
            });
            let (w1, s1) = (win.clone(), shared.clone());
            std::thread::spawn(move || render_loop(w1, s1));
            let (w2, s2) = (win.clone(), shared.clone());
            std::thread::spawn(move || {
                // A failed check must end the run, not leave the window open.
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self_test(w2, s2))).is_err() {
                    std::process::exit(1);
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("tauri");
}
