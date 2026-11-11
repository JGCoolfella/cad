// OpenCAD desktop shell (Windows first; also runs on macOS/Linux).
// Serves the built web app from a privileged app:// scheme so ES-module workers,
// WebAssembly and (later) SharedArrayBuffer work exactly as on the web.
const { app, BrowserWindow, protocol, net, Menu, shell } = require("electron");
const path = require("node:path");
const { pathToFileURL } = require("node:url");

const DEV = process.argv.includes("--dev");
const DIST = path.join(__dirname, "..", "dist");

// Make full use of the GPU: prefer the discrete adapter and never fall back to software
// rendering because of a driver blocklist entry.
app.commandLine.appendSwitch("force_high_performance_gpu");
app.commandLine.appendSwitch("ignore-gpu-blocklist");
app.commandLine.appendSwitch("enable-gpu-rasterization");
app.commandLine.appendSwitch("enable-zero-copy");
app.commandLine.appendSwitch("enable-features", "SharedArrayBuffer");

protocol.registerSchemesAsPrivileged([
  { scheme: "app", privileges: { standard: true, secure: true, supportFetchAPI: true, corsEnabled: true, stream: true } },
]);

function createWindow() {
  const win = new BrowserWindow({
    width: 1600,
    height: 1000,
    backgroundColor: "#f0f0f0",
    title: "OpenCAD",
    autoHideMenuBar: true,
    webPreferences: {
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
      backgroundThrottling: false,
    },
  });
  // The app draws its own SOLIDWORKS-style menu bar.
  Menu.setApplicationMenu(null);
  win.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url);
    return { action: "deny" };
  });
  if (!SMOKE) win.maximize();
  win.loadURL(DEV ? "http://localhost:5173" : "app://opencad/index.html");
  if (DEV && !SMOKE) win.webContents.openDevTools({ mode: "detach" });
  if (SMOKE) smokeTest(win);
}

// `electron . --smoke=<out.png>`: build a part, screenshot it, print a JSON report, exit.
// `--script=<file.js>` runs that file (an async function body using `a` = window.opencad)
// instead of the built-in scenario; its return value is the report. Add `--dev` to test
// against the Vite dev server without building.
const SMOKE = (process.argv.find((a) => a.startsWith("--smoke")) || "").split("=")[1] || (process.argv.includes("--smoke") ? "smoke.png" : "");
const SCRIPT = (process.argv.find((a) => a.startsWith("--script=")) || "").slice(9);
async function smokeTest(win) {
  const fs = require("node:fs");
  win.webContents.on("console-message", (_e, level, msg) => level >= 2 && console.log("[renderer]", msg));
  await new Promise((r) => win.webContents.once("did-finish-load", r));
  const custom = SCRIPT && `(async () => { const a = window.opencad; const wait = (ms) => new Promise(r => setTimeout(r, ms));
    for (let i = 0; i < 80 && !a.kernelReady; i++) await wait(250);
    ${fs.readFileSync(SCRIPT, "utf8")}
  })()`;
  const report = await win.webContents.executeJavaScript(custom || `(async () => {
    const a = window.opencad;
    for (let i = 0; i < 60 && !a.kernelReady; i++) await new Promise(r => setTimeout(r, 250));
    a.select({ kind: "plane", id: "front" }); a.startSketch();
    await new Promise(r => setTimeout(r, 300));
    const sk = a.sketch.data;
    const P = (id, x, y) => sk.points[id] = { id, x, y };
    P("b", 60, 0); P("c", 60, 40); P("d", 0, 40); P("cc", 30, 20);
    [["l1","origin","b"],["l2","b","c"],["l3","c","d"],["l4","d","origin"]].forEach(([id,p1,p2]) => sk.entities[id] = { id, type: "line", p1, p2 });
    sk.entities.k = { id: "k", type: "circle", c: "cc", r: 10 };
    a.resolveSketch(); a.startExtrude(false);
    await new Promise(r => setTimeout(r, 1500)); a.acceptPM();
    await new Promise(r => setTimeout(r, 1500)); a.viewHooks.orient("isometric");
    await new Promise(r => setTimeout(r, 800));
    const gl = document.createElement("canvas").getContext("webgl2");
    const dbg = gl && gl.getExtension("WEBGL_debug_renderer_info");
    return { kernel: a.kernelReady, faces: a.result?.mesh?.faces.length, rebuildMs: a.result?.timeMs, errors: a.result?.statuses.filter(s => s.error), gpu: dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : null };
  })()`);
  // A script may ask for real input: `return { input: [["click", x, y] | ["key", "a"] | ["type", "12"]], after: "js expression" }`.
  if (report && Array.isArray(report.input)) {
    const wc = win.webContents;
    for (const [kind, a1, a2] of report.input) {
      if (kind === "click" || kind === "dblclick") {
        const n = kind === "dblclick" ? 2 : 1;
        for (let c = 1; c <= n; c++) {
          wc.sendInputEvent({ type: "mouseMove", x: a1, y: a2 });
          wc.sendInputEvent({ type: "mouseDown", x: a1, y: a2, button: "left", clickCount: c });
          wc.sendInputEvent({ type: "mouseUp", x: a1, y: a2, button: "left", clickCount: c });
        }
      } else if (kind === "move") wc.sendInputEvent({ type: "mouseMove", x: a1, y: a2 });
      else if (kind === "key") {
        wc.sendInputEvent({ type: "keyDown", keyCode: a1 });
        wc.sendInputEvent({ type: "keyUp", keyCode: a1 });
      } else if (kind === "type") for (const ch of String(a1)) wc.sendInputEvent({ type: "char", keyCode: ch });
      else if (kind === "wait") await new Promise((r) => setTimeout(r, a1));
      await new Promise((r) => setTimeout(r, 60));
    }
    if (report.after) report.afterResult = await wc.executeJavaScript(`(async () => { const a = window.opencad; ${report.after} })()`);
  }
  const img = await win.webContents.capturePage();
  fs.writeFileSync(SMOKE, img.toPNG());
  console.log("SMOKE " + JSON.stringify(report));
  app.quit();
}

app.whenReady().then(() => {
  protocol.handle("app", async (req) => {
    const { pathname } = new URL(req.url);
    const file = path.normalize(path.join(DIST, decodeURIComponent(pathname)));
    if (!file.startsWith(DIST)) return new Response("Forbidden", { status: 403 });
    const res = await net.fetch(pathToFileURL(file).toString());
    const headers = new Headers(res.headers);
    // Cross-origin isolation enables SharedArrayBuffer for multi-threaded kernels.
    headers.set("Cross-Origin-Opener-Policy", "same-origin");
    headers.set("Cross-Origin-Embedder-Policy", "require-corp");
    if (file.endsWith(".wasm")) headers.set("Content-Type", "application/wasm");
    return new Response(res.body, { status: res.status, headers });
  });
  createWindow();
  app.on("activate", () => BrowserWindow.getAllWindows().length === 0 && createWindow());
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") app.quit();
});
