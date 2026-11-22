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
//! - `--log=<file>`: where the `SMOKE {json}` line is also written (default: the screenshot's
//!   path with the extension `.log`). The release build is a GUI program with no console of
//!   its own: it attaches to the console it was started from, if any (see [`attach_console`]),
//!   and the log file holds the same line in every case.
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default)]
pub struct Args {
    pub out: Option<PathBuf>,
    pub script: Option<PathBuf>,
    pub corpus: Option<PathBuf>,
    pub log: Option<PathBuf>,
}

impl Args {
    pub fn parse() -> Args {
        Args::parse_from(std::env::args().skip(1))
    }
    pub fn parse_from(args: impl IntoIterator<Item = String>) -> Args {
        let mut a = Args::default();
        for x in args {
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
            } else if let Some(v) = x.strip_prefix("--log=") {
                a.log = Some(v.into());
            }
        }
        if a.out.is_none() && (a.script.is_some() || a.corpus.is_some() || a.log.is_some()) {
            a.out = Some("smoke.png".into());
        }
        if a.log.is_none() {
            a.log = a.out.as_ref().map(|o| o.with_extension("log"));
        }
        a
    }
    pub fn active(&self) -> bool {
        self.out.is_some()
    }
}

/// `corpus/` in the working directory (an installed app run from a checkout), else the
/// checkout the binary was built from.
fn default_corpus() -> PathBuf {
    let here = PathBuf::from("corpus");
    if here.is_dir() {
        return here;
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Prints the `SMOKE {json}` line and writes it to the log file.
pub fn emit(args: &Args, line: &str) {
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
    if let Some(log) = &args.log {
        let _ = std::fs::write(log, format!("{line}
"));
    }
}

/// The release build is a GUI-subsystem program (no console window). Started from a
/// console with its output not redirected, it has no standard output: attach to the parent
/// console so the `SMOKE` line shows there. Redirected output (a pipe or a file, as from
/// `cargo run`, Git Bash or `> out.txt`) is inherited and used as it is.
#[cfg(windows)]
#[allow(unsafe_code)]
pub fn attach_console() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
    // SAFETY: plain Win32 calls without pointers.
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
pub fn attach_console() {}

/// `wait` and `settle` for the scenarios. `wait` also records where the scenario is (the
/// stack of its caller) in `window.__smoke`, for the stuck-state dump ([`DUMP`]); a timer
/// beats once a second there, to tell a webview whose timers stopped.
const WAIT: &str = "const S = window.__smoke = { t0: performance.now(), beat: performance.now(), waits: 0, at: null, atT: 0, done: false };
  setInterval(() => { S.beat = performance.now(); }, 1000);
  const here = () => (new Error().stack || '').split('\\n').slice(3, 6).map((l) => l.trim()).join(' | ');
  const wait = (ms) => { S.waits++; S.at = here(); S.atT = performance.now(); return new Promise((r) => setTimeout(r, ms)); };
  const settle = async () => { for (let i = 0; i < 400 && (a.rebuilding || !a.kernelReady); i++) await wait(50); await wait(100); };";

/// Run in the page before its own scripts on every load during a check run (`run` names the
/// run). On the run's first load it clears the stored options, so every run starts from the
/// defaults. It keeps the number of loads, the last console warnings and errors, uncaught
/// errors, unhandled rejections and visibility changes in `window.__smokeEarly` for the
/// stuck-state dump.
pub fn early(run: &str) -> String {
    format!("(() => {{ const run = {};\n{EARLY}", Value::String(run.to_string()))
}

const EARLY: &str = "
  try {
    if (localStorage.getItem('opencad.checkRun') !== run) { localStorage.clear(); localStorage.setItem('opencad.checkRun', run); }
  } catch (e) {}
  let loads = 1;
  try { loads = Number(sessionStorage.getItem('opencad.checkLoads') || 0) + 1; sessionStorage.setItem('opencad.checkLoads', String(loads)); } catch (e) {}
  const S = window.__smokeEarly = { loads, log: [] };
  const push = (s) => { if (S.log.length >= 80) S.log.shift(); S.log.push(Math.round(performance.now()) + ' ' + String(s).slice(0, 400)); };
  for (const k of ['warn', 'error']) {
    const f = console[k];
    console[k] = function (...x) { try { push(k + ': ' + x.map((v) => (v && v.stack) || String(v)).join(' ')); } catch (e) {} return f.apply(this, x); };
  }
  addEventListener('error', (e) => push('onerror: ' + e.message + ' ' + (e.filename || '') + ':' + (e.lineno || '')));
  addEventListener('unhandledrejection', (e) => push('rejection: ' + ((e.reason && e.reason.stack) || e.reason)));
  document.addEventListener('visibilitychange', () => push('visibility: ' + document.visibilityState));
})();";

/// Evaluated in the webview when no report has come: what the scenario and the app are
/// doing, sent back with `smoke_dump`. Line numbers in `script.at` are of the compiled
/// function: the scenario's own line is `script.lineOffset` less.
pub const DUMP: &str = "(() => { let d;
  try {
    const S = window.__smoke, a = window.opencad, v = window.opencadView, ipc = globalThis.__opencadIpc, now = performance.now();
    const T = window.__TAURI_INTERNALS__;
    d = {
      url: location.href, visibility: document.visibilityState, focus: document.hasFocus(), readyState: document.readyState,
      early: window.__smokeEarly || null,
      script: S ? { runningMs: Math.round(now - S.t0), sinceBeatMs: Math.round(now - S.beat), waits: S.waits, at: S.at, sinceWaitMs: Math.round(now - S.atT), lineOffset: S.lineOffset, done: S.done } : null,
      tauriCallbacks: T && T.callbacks ? T.callbacks.size : null,
      ipc: ipc ? { pending: [...ipc.pending.values()].map((p) => ({ cmd: p.cmd, ageMs: Math.round(now - p.t) })), sent: ipc.sent, done: ipc.done } : null,
      app: a ? { kernelReady: a.kernelReady, rebuilding: a.rebuilding, pm: a.pm ? a.pm.kind : null, sketch: !!a.sketch, asm: !!a.asm } : null,
      kernel: a && a.kernel ? { busy: a.kernel.busy, queued: !!a.kernel.queued } : null,
      viewport: v ? { draining: v.draining, events: v.events ? v.events.length : null, native: v.native ? { seq: v.native.seq, inFlight: v.native.inFlight, queued: !!v.native.queued } : null } : null,
    };
  } catch (e) { d = { dumpError: String(e && e.stack || e) }; }
  window.__TAURI_INTERNALS__.invoke('smoke_dump', { json: JSON.stringify(d) });
})();";

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
    // Lines before the scenario's own: the AsyncFunction's header (2), the kernel wait (1).
    let mut offset = 2;
    let body = if let Some(s) = &args.script {
        offset += 1;
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
    // The working folder, for scripts that open files of the checkout (`corpus/...`).
    let cwd = std::env::current_dir().map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    Ok(wrap(&body, &cwd, offset))
}

/// The scenario `body` (an async function body) wrapped so that it always reports: it is
/// passed as a string and compiled with the `AsyncFunction` constructor inside the `try`,
/// so a syntax error in a `--script` file is reported at once (`{ exception: "SyntaxError
/// ..." }`) instead of leaving the app waiting for its watchdog.
fn wrap(body: &str, cwd: &str, offset: usize) -> String {
    format!(
        "(async () => {{ const a = window.opencad; const cwd = {}; {WAIT}
  S.lineOffset = {offset};
  let report;
  try {{
    // The viewport answers once its renderer exists: its shaders take a second or two to
    // compile, many more on a loaded machine, and a first query before that would time out.
    for (let i = 0; i < 24; i++) {{ try {{ await window.__TAURI_INTERNALS__.invoke('app_info'); break; }} catch (e) {{ S.viewportWaits = i + 1; }} }}
    const AsyncFunction = Object.getPrototypeOf(async function () {{}}).constructor;
    const run = new AsyncFunction('a', 'cwd', 'wait', 'settle', {});
    report = await run(a, cwd, wait, settle);
  }} catch (e) {{ report = {{ exception: String(e && e.stack || e) }}; }}
  S.done = true;
  await window.__TAURI_INTERNALS__.invoke('smoke_report', {{ json: JSON.stringify(report ?? null) }});
}})();",
        Value::String(cwd.to_string()),
        Value::String(body.to_string())
    )
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

    /// The app's version is the same in package.json, tauri.conf.json and Cargo.toml.
    #[test]
    fn versions_agree() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let read = |p: &str| -> Value { serde_json::from_str(&std::fs::read_to_string(root.join(p)).unwrap()).unwrap() };
        let cargo = env!("CARGO_PKG_VERSION");
        assert_eq!(read("../../package.json")["version"], cargo, "package.json");
        assert_eq!(read("tauri.conf.json")["version"], cargo, "tauri.conf.json");
    }

    #[test]
    fn log_next_to_screenshot() {
        let parse = |v: &[&str]| Args::parse_from(v.iter().map(|s| s.to_string()));
        assert_eq!(parse(&["--smoke=dir/native-smoke.png"]).log, Some(PathBuf::from("dir/native-smoke.log")));
        assert_eq!(parse(&["--corpus"]).log, Some(PathBuf::from("smoke.log")));
        assert_eq!(parse(&["--smoke", "--log=x.txt"]).log, Some(PathBuf::from("x.txt")));
        assert!(!parse(&[]).active() && parse(&[]).log.is_none());
    }

    /// A script is compiled inside the `try` (as a string), so its syntax errors are
    /// reported instead of stopping the whole wrapper from parsing.
    #[test]
    fn script_compiled_inside_try() {
        let js = wrap("const x = 1;\nconst x = 2; // `bad` \"quote\"", "C:/w", 2);
        assert!(js.contains(r#"new AsyncFunction('a', 'cwd', 'wait', 'settle', "const x = 1;\nconst x = 2; // `bad` \"quote\"")"#), "{js}");
        assert!(!js.contains("\nconst x = 2"), "the body is not inlined as code");
        assert!(js.find("invoke('app_info')").unwrap() < js.find("await run(").unwrap(), "the viewport is waited for first");
        let t = js.find("try {").unwrap();
        assert!(t < js.find("new AsyncFunction").unwrap() && js.rfind("catch (e)").unwrap() > js.find("new AsyncFunction").unwrap());
    }

    /// Check runs start from the default options: the early script clears them once per run.
    #[test]
    fn early_script_clears_options_once_per_run() {
        let js = early("12-345");
        assert!(js.starts_with("(() => { const run = \"12-345\";"), "{js}");
        assert!(js.contains("localStorage.getItem('opencad.checkRun') !== run) { localStorage.clear();"));
        assert!(js.trim_end().ends_with("})();"));
    }

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
