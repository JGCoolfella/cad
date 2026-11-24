/**
 * The render worker (OVERHAUL_PLAN W1c, "W1 design" point 4): `oc-render` on WebGPU through
 * the `oc-web-render` module, drawing into the `OffscreenCanvas` under the transparent UI.
 * It does what the desktop app's render thread does (`crates/oc-app/src/view.rs`): the UI's
 * frames, labels, picks and box selections arrive as messages (`src/web/render.ts`), meshes
 * from the engine worker on a port of their own, and the worker draws the newest frame.
 *
 * Nothing here waits for the GPU: after its messages the worker ticks the module, and while
 * picks or draw counts are on the GPU it ticks again from the event loop, which is when
 * WebGPU resolves their read-backs.
 */
import type { ToRender, FromRender, MeshMessage } from "./render-protocol";

interface Module {
  default(init?: { module_or_path?: string | URL }): Promise<unknown>;
  WebRender: { create(canvas: OffscreenCanvas): Promise<WebRender> };
}

interface WebRender {
  info(): string;
  adapter(): string;
  resize(width: number, height: number): void;
  frame(json: string): void;
  mesh(handle: number, bytes: Uint8Array): void;
  label(key: number, rgba: Uint8Array, width: number, height: number): void;
  pick(json: string, reply: (json: string) => void): void;
  box_select(json: string, reply: (json: string) => void): void;
  stats(): string;
  drawn(): string;
  tick(): boolean;
}

const post = (m: FromRender, transfer: Transferable[] = []) => (self as unknown as Worker).postMessage(m, transfer);

let view: WebRender | null = null;
/** The adapter as `app_info` gives it natively: "name (backend)". */
let adapterName = "";

/**
 * WebGPU tells wgpu no adapter name; `GPUAdapter.info` has the vendor and architecture
 * (asked of a second adapter request, which the browser answers from the same GPU).
 */
async function describeAdapter(): Promise<string> {
  type Info = { vendor?: string; architecture?: string; description?: string };
  const gpu = (navigator as unknown as { gpu?: { requestAdapter(o?: object): Promise<{ info?: Info } | null> } }).gpu;
  const i = (await gpu?.requestAdapter({ powerPreference: "high-performance" }).catch(() => null))?.info;
  const name = [i?.vendor, i?.architecture, i?.description].filter((s) => s && s.trim()).join(" ");
  return `${name || "GPU"} (WebGPU)`;
}
let scheduled = false;
let lastSeq = -1;
/** Ticks in a row that found the GPU still busy. */
let spins = 0;
/**
 * Ticks run from a message to ourselves: a task after the messages queued now, without the
 * 4 ms that browsers add to nested `setTimeout(0)`s (hover picks would wait for it).
 */
const wake = new MessageChannel();
wake.port1.onmessage = () => tick();
/**
 * While the GPU owes answers the worker ticks again at once (as the desktop app's render
 * thread spins while a hover pick is on the GPU), up to this many times; then every
 * millisecond, so a slow GPU does not keep a core busy.
 */
const SPINS = 200;

/** Runs the module's work after the messages queued now, and again while the GPU owes answers. */
function schedule(waiting = false) {
  if (scheduled || !view) return;
  scheduled = true;
  if (!waiting) spins = 0;
  if (spins++ < SPINS) wake.port2.postMessage(0);
  else setTimeout(tick, 1);
}

function tick() {
  scheduled = false;
  if (!view) return;
  const more = view.tick();
  const d = JSON.parse(view.drawn()) as { seq: number; frames: number; cpuMs: number; gpuMs: number | null };
  if (d.frames > 0 && d.seq !== lastSeq) {
    lastSeq = d.seq;
    post({ t: "drawn", seq: d.seq, cpuMs: d.cpuMs });
  }
  if (more) schedule(true);
}

function onMesh(m: MeshMessage) {
  if (!view) return;
  view.mesh(m.handle, new Uint8Array(m.bytes));
  schedule();
}

async function start(m: Extract<ToRender, { t: "init" }>) {
  try {
    const mod = (await import(/* @vite-ignore */ m.module)) as Module;
    await mod.default({ module_or_path: m.module.replace(/\.js$/, "_bg.wasm") });
    view = await mod.WebRender.create(m.canvas);
    adapterName = await describeAdapter();
    post({ t: "ready", info: JSON.parse(view.info()), adapter: adapterName });
  } catch (e) {
    post({ t: "failed", error: String((e as Error)?.message ?? e) });
  }
}

self.onmessage = (e: MessageEvent<ToRender>) => {
  const m = e.data;
  if (m.t === "init") {
    void start(m);
    return;
  }
  if (!view) {
    if ("id" in m) post({ t: "reply", id: m.id, error: "the renderer is not running" });
    return;
  }
  try {
    switch (m.t) {
      case "engine":
        m.port.onmessage = (ev: MessageEvent<MeshMessage>) => onMesh(ev.data);
        break;
      case "mesh":
        onMesh(m);
        break;
      case "resize":
        view.resize(m.width, m.height);
        break;
      case "frame":
        view.frame(m.json);
        break;
      case "label":
        view.label(m.key, new Uint8Array(m.rgba), m.width, m.height);
        break;
      case "pick":
        view.pick(m.json, (json) => post({ t: "reply", id: m.id, json }));
        break;
      case "box":
        view.box_select(m.json, (json) => post({ t: "reply", id: m.id, json }));
        break;
      case "stats":
        post({ t: "reply", id: m.id, json: view.stats() });
        break;
      case "drawn":
        post({ t: "reply", id: m.id, json: view.drawn() });
        break;
      case "info":
        post({ t: "reply", id: m.id, json: JSON.stringify(adapterName || view.adapter()) });
        break;
    }
  } catch (err) {
    if ("id" in m) post({ t: "reply", id: m.id, error: String((err as Error)?.message ?? err) });
    else console.error("render worker:", err);
  }
  schedule();
};
