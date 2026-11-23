import { createRoot } from "react-dom/client";
import { Shell } from "./ui/Shell";
import { App } from "./app/app";
import { isNative, invoke, useWebIpc } from "./native/tauri";
import { setSketchSystemFactory } from "./sketch/solver";
import "./ui/styles.css";

async function start() {
  // The browser on the native engine (OVERHAUL_PLAN W1b; opt-in until W1f switches the web
  // build): `?engine=native`, the engine program from `?module=` (default `engine/oc-web.js`).
  const q = new URLSearchParams(location.search);
  if (q.get("engine") === "native" && !isNative()) {
    const { WebEngine } = await import("./web");
    useWebIpc(new WebEngine({ moduleUrl: new URL(q.get("module") ?? "engine/oc-web.js", document.baseURI).href }));
  }
  const native = isNative();
  if (native) {
    // The native app: the viewport is drawn under this (transparent) page, and sketches
    // are solved by the Rust solver compiled to WebAssembly (OVERHAUL_PLAN F2).
    document.documentElement.classList.add("native");
    const { loadSketchWasm, WasmSketchSystem } = await import("./sketch/wasmSolver");
    await loadSketchWasm();
    setSketchSystemFactory((sk, fixed) => new WasmSketchSystem(sk, fixed));
  }
  const app = new App();
  (window as unknown as { opencad: App }).opencad = app;
  if (native) {
    // Debugging and the assembly smoke script (`crates/oc-app/scripts/assembly.js`).
    const asm = await import("./assembly/session");
    (window as unknown as { opencadAsm: typeof asm }).opencadAsm = asm;
  }
  createRoot(document.getElementById("root")!).render(<Shell app={app} />);
  if (native) invoke("app_ready").catch((e) => console.error("app_ready", e));
}

start();
