/**
 * The browser's viewport on the UI thread (OVERHAUL_PLAN W1c, "W1 design" point 4): starts
 * the render worker (`render.worker.ts`, `oc-render` on WebGPU) on a canvas that lies under
 * the transparent UI, and answers the desktop app's `view_*` commands with it, so
 * `NativeView` (`src/native/view.ts`) runs unchanged: in the browser `invoke` hands these
 * commands to {@link WebRenderClient.command} (the routing is W1b's `src/web/` and W1f's
 * switch).
 *
 * The canvas covers the window, as the desktop app's swapchain covers its window: frames name
 * the graphics area (`rect`), the worker clears the rest to the UI background.
 *
 * Without WebGPU (no `navigator.gpu`, no adapter, no device) {@link startViewport} answers
 * the three.js viewport with the reason: it draws the engine's level-0 meshes (the
 * `BodyMesh` every rebuild reply carries), parts only, with assemblies greyed and
 * {@link FALLBACK_NOTE} (owner decision 2026-11-23; switched on in W1f).
 */
import type { FromRender, RenderInfo, ToRender } from "./render-protocol";

/** Where the wasm-bindgen module is served (`node scripts/web-render.mjs build`; W1f ships it). */
export const DEFAULT_MODULE = `${import.meta.env.BASE_URL}web-render/oc_web_render.js`;

/** Why the browser has no WebGPU viewport (for the fallback's note). */
export interface RenderUnavailable {
  reason: string;
}

/** Whether to try WebGPU at all: `navigator.gpu` exists (the adapter may still be missing). */
export function hasWebGpu(): boolean {
  return typeof navigator !== "undefined" && "gpu" in navigator && !!(navigator as Navigator & { gpu?: unknown }).gpu;
}

export class WebRenderClient {
  private next = 1;
  private waiting = new Map<number, { ok: (v: unknown) => void; fail: (e: Error) => void; parse: boolean }>();
  private drawnListeners: ((seq: number, cpuMs: number) => void)[] = [];
  /** The newest frame drawn (its `seq`). */
  drawnSeq = -1;
  private resizeObserver: (() => void) | null = null;

  private constructor(
    private worker: Worker,
    readonly info: RenderInfo,
    readonly adapter: string,
  ) {
    worker.addEventListener("message", (e: MessageEvent<FromRender>) => this.onMessage(e.data));
  }

  /** Starts the render worker on `canvas`; `null` and a reason when WebGPU cannot run. */
  static async start(canvas: HTMLCanvasElement, module = DEFAULT_MODULE): Promise<WebRenderClient | RenderUnavailable> {
    if (!hasWebGpu()) return { reason: "This browser has no WebGPU." };
    const worker = new Worker(new URL("./render.worker.ts", import.meta.url), { type: "module" });
    const size = deviceSize();
    const offscreen = canvas.transferControlToOffscreen();
    offscreen.width = size.width;
    offscreen.height = size.height;
    const first = new Promise<FromRender>((ok) => {
      const h = (e: MessageEvent<FromRender>) => {
        if (e.data.t === "ready" || e.data.t === "failed") {
          worker.removeEventListener("message", h);
          ok(e.data);
        }
      };
      worker.addEventListener("message", h);
      worker.addEventListener("error", (e) => ok({ t: "failed", error: e.message || "the render worker failed to start" }));
    });
    const init: ToRender = { t: "init", canvas: offscreen, module: new URL(module, location.href).href };
    worker.postMessage(init, [offscreen]);
    const r = await first;
    if (r.t !== "ready") {
      worker.terminate();
      return { reason: r.t === "failed" ? r.error : "the render worker did not start" };
    }
    const c = new WebRenderClient(worker, r.info, r.adapter);
    c.followWindow();
    return c;
  }

  private post(m: ToRender, transfer: Transferable[] = []) {
    this.worker.postMessage(m, transfer);
  }

  private onMessage(m: FromRender) {
    if (m.t === "reply") {
      const w = this.waiting.get(m.id);
      if (!w) return;
      this.waiting.delete(m.id);
      if (m.error !== undefined) w.fail(new Error(m.error));
      else w.ok(w.parse && m.json !== undefined ? JSON.parse(m.json) : m.json);
    } else if (m.t === "drawn") {
      this.drawnSeq = m.seq;
      for (const l of this.drawnListeners) l(m.seq, m.cpuMs);
    }
  }

  private ask<T>(make: (id: number) => ToRender, parse = true): Promise<T> {
    const id = this.next++;
    return new Promise<T>((ok, fail) => {
      this.waiting.set(id, { ok: ok as (v: unknown) => void, fail, parse });
      this.post(make(id));
    });
  }

  /** Called with each frame drawn (`seq`, CPU ms from its arrival to its commands submitted). */
  onDrawn(l: (seq: number, cpuMs: number) => void): () => void {
    this.drawnListeners.push(l);
    return () => {
      this.drawnListeners = this.drawnListeners.filter((x) => x !== l);
    };
  }

  /** The canvas follows the window's size and `devicePixelRatio`. */
  private followWindow() {
    const send = () => {
      const s = deviceSize();
      this.post({ t: "resize", width: s.width, height: s.height });
    };
    window.addEventListener("resize", send);
    this.resizeObserver = () => window.removeEventListener("resize", send);
    send();
  }

  /**
   * A desktop app command the renderer answers, with Tauri's `invoke` arguments and result
   * shapes; `undefined` when `cmd` is not one of them.
   */
  command(cmd: string, args?: unknown, options?: { headers?: Record<string, string> }): Promise<unknown> | undefined {
    switch (cmd) {
      case "view_frame":
        this.post({ t: "frame", json: JSON.stringify(args) });
        return Promise.resolve(undefined);
      case "view_add_label": {
        const px = args as Uint8Array;
        const h = options?.headers ?? {};
        const rgba = px.slice().buffer;
        this.post({ t: "label", key: Number(h["x-key"]), width: Number(h["x-width"]), height: Number(h["x-height"]), rgba }, [rgba]);
        return Promise.resolve(undefined);
      }
      case "view_pick":
        return this.ask((id) => ({ t: "pick", id, json: JSON.stringify((args as { req: unknown }).req) }));
      case "view_box":
        return this.ask((id) => ({ t: "box", id, json: JSON.stringify((args as { req: unknown }).req) }));
      case "view_stats":
        return this.ask((id) => ({ t: "stats", id }));
      case "app_info":
        return this.ask((id) => ({ t: "info", id }));
      default:
        return undefined;
    }
  }

  /** The last frame drawn and its times (scripts). */
  drawn(): Promise<{ seq: number; frames: number; cpuMs: number; gpuMs: number | null }> {
    return this.ask((id) => ({ t: "drawn", id }));
  }

  /**
   * A channel for the engine worker's meshes: the render worker keeps one end, the caller
   * hands the other to the engine worker (transferred), which posts {@link MeshMessage}s on it.
   */
  engineChannel(): MessagePort {
    const ch = new MessageChannel();
    this.post({ t: "engine", port: ch.port1 }, [ch.port1]);
    return ch.port2;
  }

  /** A mesh sent from this thread (tests and the harness; the engine worker uses its port). */
  mesh(handle: number, bytes: ArrayBuffer) {
    this.post({ t: "mesh", handle, bytes }, [bytes]);
  }

  stop() {
    this.resizeObserver?.();
    this.worker.terminate();
    for (const w of this.waiting.values()) w.fail(new Error("the renderer stopped"));
    this.waiting.clear();
  }
}

/** The window in device pixels. */
function deviceSize() {
  const dpr = window.devicePixelRatio || 1;
  return { width: Math.max(1, Math.round(window.innerWidth * dpr)), height: Math.max(1, Math.round(window.innerHeight * dpr)) };
}

/** Shown where assemblies are greyed in the three.js fallback. */
export const FALLBACK_NOTE = "Assemblies need WebGPU, which this browser does not offer. Parts work as usual.";

/**
 * The browser's viewport: `oc-render` on WebGPU in the render worker, or the three.js
 * viewport (the frozen TypeScript one) on the engine's level-0 meshes, parts only.
 */
export type ViewportMode = { kind: "webgpu"; client: WebRenderClient } | { kind: "three"; reason: string; assemblies: false };

/** Starts the WebGPU viewport on `canvas`, or says why the three.js one draws instead. */
export async function startViewport(canvas: HTMLCanvasElement | null, module = DEFAULT_MODULE): Promise<ViewportMode> {
  if (!canvas || !hasWebGpu()) return { kind: "three", reason: "This browser has no WebGPU.", assemblies: false };
  const r = await WebRenderClient.start(canvas, module).catch((e: unknown) => ({ reason: String((e as Error)?.message ?? e) }));
  return r instanceof WebRenderClient ? { kind: "webgpu", client: r } : { kind: "three", reason: r.reason, assemblies: false };
}

export type { MeshMessage, RenderInfo } from "./render-protocol";
