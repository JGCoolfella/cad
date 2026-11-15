//! `opencad --smoke[=out.png]`: the end-to-end check of the Electron app's `--smoke`, on the
//! native app. It runs a script in the webview through the real UI path (window.opencad),
//! waits for its report, saves a screenshot of the window as composited on screen
//! (`out.png`) and the viewport as `oc-render` drew it (`out-viewport.png`), prints
//! `SMOKE {json}` and exits.
//!
//! - default: sketch a rectangle with a hole on the Front plane, extrude it, view isometric;
//! - `--script=<file.js>`: that file instead (an async function body, `a` = window.opencad);
//! - `--corpus[=<dir>]`: open every corpus part (`corpus/*.ocpart`), regenerate it and
//!   compare the final body with the recorded oracle (`corpus/<part>.json`; the values
//!   section 8 of the plan corrects are corrected here too). Exit code 1 on a mismatch.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Args {
    pub out: Option<PathBuf>,
    pub script: Option<PathBuf>,
    pub corpus: Option<PathBuf>,
}

impl Args {
    pub fn parse() -> Args {
        let mut a = Args::default();
        for x in std::env::args().skip(1) {
            if x == "--smoke" {
                a.out = Some("smoke.png".into());
            } else if let Some(v) = x.strip_prefix("--smoke=") {
                a.out = Some(v.into());
            } else if let Some(v) = x.strip_prefix("--script=") {
                a.script = Some(v.into());
            } else if x == "--corpus" {
                a.corpus = Some(default_corpus());
            } else if let Some(v) = x.strip_prefix("--corpus=") {
                a.corpus = Some(v.into());
            }
        }
        if a.out.is_none() && (a.script.is_some() || a.corpus.is_some()) {
            a.out = Some("smoke.png".into());
        }
        a
    }
    pub fn active(&self) -> bool {
        self.out.is_some()
    }
}

fn default_corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

const WAIT: &str = "const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const settle = async () => { for (let i = 0; i < 400 && (a.rebuilding || !a.kernelReady); i++) await wait(50); await wait(100); };";

/// The default scenario: the Electron smoke test's, plus the native adapter.
const DEFAULT: &str = r#"
    for (let i = 0; i < 60 && !a.kernelReady; i++) await wait(250);
    a.select({ kind: "plane", id: "front" }); a.startSketch();
    await wait(300);
    const sk = a.sketch.data;
    const P = (id, x, y) => sk.points[id] = { id, x, y };
    P("b", 60, 0); P("c", 60, 40); P("d", 0, 40); P("cc", 30, 20);
    [["l1","origin","b"],["l2","b","c"],["l3","c","d"],["l4","d","origin"]].forEach(([id,p1,p2]) => sk.entities[id] = { id, type: "line", p1, p2 });
    sk.entities.k = { id: "k", type: "circle", c: "cc", r: 10 };
    a.resolveSketch(); a.startExtrude(false);
    await wait(1500); a.acceptPM();
    await wait(1500); await settle(); a.viewHooks.orient("isometric");
    await wait(800);
    const mp = await a.kernel.massProps(a.activeFeatures());
    return { kernel: a.kernelReady, faces: a.result?.mesh?.faces.length, volume: mp?.volume, rebuildMs: a.result?.timeMs, errors: a.result?.statuses.filter(s => s.error), gpu: await a.kernel.adapter?.() };
"#;

/// The corpus scenario: `parts` is injected as `[{ name, doc }]`.
const CORPUS: &str = r#"
    for (let i = 0; i < 60 && !a.kernelReady; i++) await wait(250);
    const out = [];
    for (const p of parts) {
      a.loadDocument(p.doc, p.name + ".ocpart");
      await wait(50); await settle();
      const r = a.result;
      const mp = await a.kernel.massProps(a.activeFeatures());
      out.push({ name: p.name, faces: r?.mesh?.faces.length ?? 0, edges: r?.mesh?.edges.length ?? 0, volume: mp?.volume ?? null, area: mp?.area ?? null,
        statuses: (r?.statuses ?? []).filter(s => s.error || s.warning), timeMs: r?.timeMs, dirty: a.dirty });
    }
    a.viewHooks.orient("isometric"); await wait(600);
    return { parts: out, gpu: await a.kernel.adapter?.() };
"#;

/// The JavaScript to evaluate once the app is ready. It reports through `smoke_report`.
pub fn script(args: &Args) -> std::io::Result<String> {
    let body = if let Some(s) = &args.script {
        format!("for (let i = 0; i < 80 && !a.kernelReady; i++) await wait(250);\n{}", std::fs::read_to_string(s)?)
    } else if let Some(dir) = &args.corpus {
        let parts: Vec<Value> = corpus_parts(dir)?
            .into_iter()
            .map(|(name, doc, _)| json!({ "name": name, "doc": doc }))
            .collect();
        format!("const parts = {};\n{CORPUS}", Value::Array(parts))
    } else {
        DEFAULT.to_string()
    };
    Ok(format!(
        "(async () => {{ const a = window.opencad; {WAIT}
  let report;
  try {{ report = await (async () => {{ {body} }})(); }} catch (e) {{ report = {{ exception: String(e && e.stack || e) }}; }}
  await window.__TAURI_INTERNALS__.invoke('smoke_report', {{ json: JSON.stringify(report ?? null) }});
}})();"
    ))
}

/// `(name, document text, oracle)` per corpus part, sorted by name.
pub fn corpus_parts(dir: &Path) -> std::io::Result<Vec<(String, String, Value)>> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "ocpart") {
            let name = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let doc = std::fs::read_to_string(&p)?;
            let oracle = std::fs::read_to_string(p.with_extension("json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            out.push((name, doc, oracle));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// The final body the oracle expects, with the corrections recorded in section 8 (E5):
/// the TypeScript engine's mid-plane extrusion in `through-mid-dir2` added material that
/// is not there (`corrected_oracle` in `crates/oc-graph/tests/common/mod.rs`).
/// Returns the expected body and the body as recorded (for the rounding of its values).
/// A corrected value also carries the rounding of its correction (114.288 is itself
/// recorded to 6 figures), in `b["slack"]`.
fn expected(name: &str, oracle: &Value) -> Option<(Value, Value)> {
    let last = oracle["steps"].as_array()?.iter().rev().find(|s| s.get("body").is_some())?;
    let recorded = last["body"].clone();
    let mut b = recorded.clone();
    if name == "through-mid-dir2" {
        let sub = |b: &mut Value, k: &str, x: f64| b[k] = json!(b[k].as_f64().unwrap_or(0.0) - x);
        sub(&mut b, "volume", 100.0 * 114.288);
        sub(&mut b, "area", 4.0 * 10.0 * 114.288);
        b["faces"] = json!(b["faces"].as_u64().unwrap_or(0) - 5);
        b["edges"] = json!(b["edges"].as_u64().unwrap_or(0) - 12);
        b["slack"] = json!({ "volume": 100.0 * 0.0005, "area": 40.0 * 0.0005 });
    }
    Some((b, recorded))
}

/// Half a unit in the 6th significant figure of a recorded value (the oracle's rounding).
fn rounding(recorded: f64) -> f64 {
    if recorded == 0.0 || !recorded.is_finite() {
        return 1e-9;
    }
    0.5 * 10f64.powi(recorded.abs().log10().floor() as i32 - 5) * (1.0 + 1e-9)
}

/// Compares the corpus report with the oracle; returns the differences.
pub fn check_corpus(dir: &Path, report: &Value) -> Vec<String> {
    let Ok(parts) = corpus_parts(dir) else { return vec!["corpus not readable".into()] };
    let got = report["parts"].as_array().cloned().unwrap_or_default();
    let mut diffs = vec![];
    for (name, _, oracle) in &parts {
        let Some(g) = got.iter().find(|g| g["name"] == name.as_str()) else {
            diffs.push(format!("{name}: no result"));
            continue;
        };
        let Some((w, recorded)) = expected(name, oracle) else { continue };
        for k in ["faces", "edges"] {
            if g[k].as_u64() != w[k].as_u64() {
                diffs.push(format!("{name}: {k} {} vs oracle {}", g[k], w[k]));
            }
        }
        for k in ["volume", "area"] {
            let (a, b) = (g[k].as_f64().unwrap_or(f64::NAN), w[k].as_f64().unwrap_or(f64::NAN));
            // The oracle is recorded at 6 significant figures; a corrected value carries the
            // rounding of the value it was corrected from.
            let slack = w["slack"][k].as_f64().unwrap_or(0.0);
            let close = (a - b).abs() <= rounding(recorded[k].as_f64().unwrap_or(b)) + slack;
            if !close {
                diffs.push(format!("{name}: {k} {a} vs oracle {b}"));
            }
        }
        if g["statuses"].as_array().is_some_and(|s| !s.is_empty()) {
            diffs.push(format!("{name}: statuses {}", g["statuses"]));
        }
    }
    diffs
}

pub fn save_png(path: &Path, rgba: &[u8], w: u32, h: u32) -> std::io::Result<()> {
    let f = std::fs::File::create(path)?;
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    let mut wr = e.write_header().map_err(std::io::Error::other)?;
    wr.write_image_data(rgba).map_err(std::io::Error::other)
}

/// RGBA pixels of a screen rectangle as composited (swapchain and webview together).
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn grab(x: i32, y: i32, w: i32, h: i32) -> Vec<u8> {
    use windows_sys::Win32::Graphics::Gdi::*;
    // SAFETY: plain GDI calls on handles created and released here.
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
        bi.bmiHeader.biCompression = BI_RGB;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        GetDIBits(mem, bmp, 0, h as u32, buf.as_mut_ptr() as _, &mut bi, DIB_RGB_COLORS);
        SelectObject(mem, old);
        DeleteObject(bmp as _);
        DeleteDC(mem);
        ReleaseDC(std::ptr::null_mut(), screen);
        for p in buf.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        buf
    }
}

#[cfg(not(windows))]
pub fn grab(_x: i32, _y: i32, _w: i32, _h: i32) -> Vec<u8> {
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_rounding() {
        assert!((rounding(858.628) - 0.0005).abs() < 1e-12);
        assert!((rounding(13393.6) - 0.05).abs() < 1e-9);
    }

    /// The corpus as the app would report it from the oracle itself has no differences.
    #[test]
    fn corpus_against_itself() {
        let dir = default_corpus();
        let parts: Vec<Value> = corpus_parts(&dir)
            .unwrap()
            .iter()
            .map(|(name, _, o)| {
                let (b, _) = expected(name, o).unwrap();
                json!({ "name": name, "faces": b["faces"], "edges": b["edges"], "volume": b["volume"], "area": b["area"], "statuses": [] })
            })
            .collect();
        assert_eq!(parts.len(), 17);
        assert_eq!(check_corpus(&dir, &json!({ "parts": parts })), Vec::<String>::new());
        let mut bad = parts.clone();
        bad[0]["faces"] = json!(1);
        assert_eq!(check_corpus(&dir, &json!({ "parts": bad })).len(), 1);
    }
}
