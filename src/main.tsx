import { createRoot } from "react-dom/client";
import { Shell } from "./ui/Shell";
import { App } from "./app/app";
import { isNative, invoke } from "./native/tauri";
import { setSketchSystemFactory } from "./sketch/solver";
import "./ui/styles.css";

async function start() {
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
  createRoot(document.getElementById("root")!).render(<Shell app={app} />);
  if (native) invoke("app_ready").catch((e) => console.error("app_ready", e));
}

start();
