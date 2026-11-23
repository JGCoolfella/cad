// The browser's renderer (OVERHAUL_PLAN W1c): `oc-web-render` built for
// `wasm32-unknown-unknown` (wgpu's WebGPU backend) and bound by wasm-bindgen, and a smoke run
// of the render worker in headless Edge or Chrome on the real GPU.
//
//   node scripts/web-render.mjs build    the module: target/web-render/oc_web_render{.js,_bg.wasm}
//   node scripts/web-render.mjs smoke    build, meshes, Vite, the harness page in headless
//                                         Chromium over the DevTools protocol: `SMOKE {...}`
//
// Needs `rustup target add wasm32-unknown-unknown` and wasm-bindgen-cli of the version in
// Cargo.lock (`cargo install wasm-bindgen-cli --version 0.2.130`). The smoke also builds the
// harness meshes natively (OCCT_DIR, as `cargo test`). CARGO_TARGET_DIR as usual; the module
// and meshes go to <repo>/target/web-render either way (Vite serves them from there).
// OC_BROWSER names the browser (default: Edge, then Chrome). The full browser checks (the app's
// scenarios, the corpus) are W1d's.
import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "target", "web-render");
const targetDir = process.env.CARGO_TARGET_DIR ?? join(root, "target");
const BINDGEN = "0.2.130";
const cargoBin = join(process.env.USERPROFILE ?? process.env.HOME ?? "", ".cargo", "bin");
const env = { ...process.env, PATH: `${cargoBin}${process.platform === "win32" ? ";" : ":"}${process.env.PATH}` };

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: "inherit", env, cwd: root, ...opts });
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(" ")} failed (${r.status ?? r.error})`);
}

function sizes(file) {
  const b = readFileSync(file);
  const kb = (n) => Math.round(n / 1024);
  return { raw: kb(b.length), gzip: kb(gzipSync(b, { level: 9 }).length), brotli: kb(brotliCompressSync(b, { params: { [constants.BROTLI_PARAM_QUALITY]: 11 } }).length) };
}

export function build() {
  const v = spawnSync("wasm-bindgen", ["--version"], { env, encoding: "utf8" }).stdout ?? "";
  if (!v.includes(BINDGEN)) throw new Error(`wasm-bindgen ${BINDGEN} needed (found: ${v.trim() || "none"}): cargo install wasm-bindgen-cli --version ${BINDGEN}`);
  const t0 = Date.now();
  run("cargo", ["build", "--release", "-p", "oc-web-render", "--target", "wasm32-unknown-unknown"]);
  mkdirSync(out, { recursive: true });
  const wasm = join(targetDir, "wasm32-unknown-unknown", "release", "oc_web_render.wasm");
  run("wasm-bindgen", ["--target", "web", "--no-typescript", "--out-dir", out, wasm]);
  const report = { seconds: Math.round((Date.now() - t0) / 100) / 10, wasm: sizes(join(out, "oc_web_render_bg.wasm")), js: sizes(join(out, "oc_web_render.js")) };
  console.log(`web-render ${JSON.stringify(report)}`);
  return report;
}

function browser() {
  const c = [
    process.env.OC_BROWSER,
    "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  ].filter(Boolean);
  const b = c.find((p) => existsSync(p));
  if (!b) throw new Error("no Edge or Chrome found (OC_BROWSER)");
  return b;
}

/** A DevTools protocol session over Node's own WebSocket. */
async function devtools(url) {
  const ws = new WebSocket(url);
  await new Promise((ok, fail) => {
    ws.onopen = ok;
    ws.onerror = () => fail(new Error(`DevTools: cannot connect to ${url}`));
  });
  let next = 1;
  const waiting = new Map();
  const listeners = [];
  ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    if (m.id && waiting.has(m.id)) {
      const { ok, fail } = waiting.get(m.id);
      waiting.delete(m.id);
      if (m.error) fail(new Error(JSON.stringify(m.error)));
      else ok(m.result);
    } else if (m.method) listeners.forEach((l) => l(m));
  };
  return {
    send: (method, params = {}, sessionId) =>
      new Promise((ok, fail) => {
        const id = next++;
        waiting.set(id, { ok, fail });
        ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
      }),
    on: (l) => listeners.push(l),
    close: () => ws.close(),
  };
}

/**
 * Opens `url` in a fresh headless browser with `flags`, waits for the harness' result and
 * runs `after(send)` (screenshots); returns the result with the console's last lines. `init`:
 * a script run before the page's own.
 */
async function session(url, flags, after, init) {
  const profile = mkdtempSync(join(tmpdir(), "oc-web-render-"));
  const proc = spawn(browser(), ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "--no-first-run", "--no-default-browser-check", "--window-size=1280,800", ...flags, "about:blank"], { stdio: "ignore" });
  const timeout = (ms, what) => new Promise((_, fail) => setTimeout(() => fail(new Error(`timeout: ${what}`)), ms));
  const result = {};
  try {
    const portFile = join(profile, "DevToolsActivePort");
    for (let i = 0; i < 200 && !existsSync(portFile); i++) await new Promise((r) => setTimeout(r, 100));
    const [dport, path] = readFileSync(portFile, "utf8").split(/\r?\n/);
    const dt = await devtools(`ws://127.0.0.1:${dport}${path}`);
    const { targetInfos } = await dt.send("Target.getTargets");
    const tab = targetInfos.find((t) => t.type === "page");
    const { sessionId } = await dt.send("Target.attachToTarget", { targetId: tab.targetId, flatten: true });
    const s = (m, p) => dt.send(m, p, sessionId);
    const logs = [];
    dt.on((m) => {
      if (m.sessionId !== sessionId) return;
      if (m.method === "Runtime.consoleAPICalled") logs.push(`${m.params.type}: ${m.params.args.map((a) => a.value ?? a.description).join(" ")}`);
      if (m.method === "Runtime.exceptionThrown") logs.push(`exception: ${m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text}`);
    });
    await s("Runtime.enable");
    await s("Page.enable");
    if (init) await s("Page.addScriptToEvaluateOnNewDocument", { source: init });
    await s("Page.navigate", { url });
    const r = await Promise.race([
      s("Runtime.evaluate", { expression: "new Promise((ok) => { const w = () => window.__webRenderResult ? ok(JSON.stringify(window.__webRenderResult)) : setTimeout(w, 100); w(); })", awaitPromise: true, returnByValue: true }),
      timeout(120_000, "the harness gave no result"),
    ]);
    Object.assign(result, JSON.parse(r.result.value));
    if (after) await after(s, result);
    const shown = logs.filter((l) => !l.includes("[vite]"));
    if (shown.length) result.console = shown.slice(-20);
    dt.close();
  } catch (e) {
    result.ok = false;
    result.exception = String(e?.stack ?? e);
  } finally {
    proc.kill();
    await new Promise((r) => setTimeout(r, 500));
    try {
      rmSync(profile, { recursive: true, force: true });
    } catch {
      // the browser may still hold its profile for a moment
    }
  }
  return result;
}

export async function smoke() {
  const report = { build: build() };
  run("cargo", ["run", "-q", "--release", "-p", "oc-web-render", "--example", "mesh_blobs", "--", out]);
  const { createServer } = await import("vite");
  const server = await createServer({ root, logLevel: "error", server: { port: 0, strictPort: false, hmr: false } });
  await server.listen();
  const page = `http://localhost:${server.httpServer.address().port}/src/web/harness/render.html`;
  try {
    const shoot = async (s, name) => {
      const shot = await s("Page.captureScreenshot", { format: "png" });
      const png = join(root, name);
      writeFileSync(png, Buffer.from(shot.data, "base64"));
      return png;
    };
    // WebGPU on the real GPU.
    Object.assign(
      report,
      await session(page, ["--enable-unsafe-webgpu"], async (s, r) => {
        r.screenshots = [await shoot(s, "web-render-smoke.png")];
        if (r.ok) {
          await s("Runtime.evaluate", { expression: "window.__webRenderShowPart()", awaitPromise: true });
          r.screenshots.push(await shoot(s, "web-render-smoke-part.png"));
        }
      }),
    );
    // A browser without WebGPU (the page sees no navigator.gpu; Chromium has no switch that
    // removes it): the viewport must fall back (W1f draws with three.js then).
    const noGpu = "Object.defineProperty(Navigator.prototype, 'gpu', { get: () => undefined, configurable: true });";
    report.withoutWebGpu = await session(`${page}?fallback`, [], null, noGpu);
  } finally {
    await server.close();
  }
  report.ok = !!report.ok && !!report.withoutWebGpu.ok;
  console.log(`SMOKE ${JSON.stringify(report)}`);
  return report.ok ? 0 : 1;
}

const cmd = process.argv[2];
if (cmd === "build") build();
else if (cmd === "smoke") process.exitCode = await smoke();
else {
  console.error("usage: node scripts/web-render.mjs build|smoke");
  process.exitCode = 2;
}
