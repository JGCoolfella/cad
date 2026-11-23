/**
 * The native engine in the browser (OVERHAUL_PLAN W1b): the page's side. `invoke` takes the
 * commands the desktop app's Tauri commands take, with the same arguments and replies, and
 * routes them: `engine_*`, `asm_*` and the text file commands to the **engine worker**
 * (`engine.worker.ts`, with its geometry host workers), `view_*` to the **render worker**
 * (W1c), the window's own commands to small stand-ins here. `src/native/tauri.ts` is the one
 * seam: once installed there (`useWebIpc`), `NativeKernelClient`, `NativeView` and the
 * assembly session run unchanged.
 *
 * An engine worker that traps or aborts is started again; what it held (the open assembly,
 * the regeneration cache, the files in its `/work`) is gone, and the request that stopped
 * it is rejected. The next rebuild sends the whole document again, as every request does.
 */
import type { EngineIn, EngineOut, Identity, RequestStats } from "./protocol";

export interface WebEngineOptions {
  /** The engine program, `oc-web.js` (with `oc_web.wasm` and, as a side module, OCCT's `libocct.so` beside it). */
  moduleUrl: string;
  /** Geometry host workers (default: half the logical processors, as the desktop app). 0: none. */
  hosts?: number;
  /** Estimated memory for the parts' bodies in the engine worker, MB (default 1024; 0: no limit). */
  budgetMb?: number;
  /** Test faults in host workers (`oc_host::serve::FAULTS`). */
  faults?: boolean;
  /** How long a host worker may take for one part (default 600 s, as the desktop app). */
  hostTimeoutMs?: number;
  /** Where the engine worker sends the display meshes it makes (`MeshOut`, the render
   * worker's port, W1c). Without it they are dropped once the request is answered. Given to
   * the first engine worker only (a port moves once). */
  meshes?: MessagePort;
  /** The render worker's `invoke` for `view_*` commands (W1c). */
  render?: (cmd: string, args?: unknown, options?: { headers?: Record<string, string> }) => Promise<unknown>;
}

/** What the engine worker serves (`oc_engine::request::COMMANDS`). */
export function isEngineCommand(cmd: string) {
  return cmd.startsWith("engine_") || cmd.startsWith("asm_") || cmd === "app_read_text" || cmd === "app_write_text";
}

type Pending = { resolve: (v: unknown) => void; reject: (e: unknown) => void };

export class WebEngine {
  private worker!: Worker;
  private next = 1;
  private pending = new Map<number, Pending>();
  private started!: Promise<{ identity: Identity; startMs: number }>;
  /** Engine workers started (restarts after a trap included). */
  starts = 0;
  /** The last request's statistics (deferrals, host parts, times). */
  lastStats: RequestStats | null = null;

  constructor(readonly o: WebEngineOptions) {
    this.start();
  }

  private start() {
    const w = new Worker(new URL("./engine.worker.ts", import.meta.url), { type: "module" });
    this.worker = w;
    this.starts++;
    this.started = new Promise((resolve, reject) => {
      const first = (ev: MessageEvent<EngineOut>) => {
        if (ev.data.type === "ready") resolve(ev.data);
        else if (ev.data.type === "fatal") reject(new Error(ev.data.error));
      };
      w.addEventListener("message", first, { once: true });
    });
    this.started.catch(() => {});
    w.addEventListener("message", (ev: MessageEvent<EngineOut>) => this.onMessage(w, ev.data));
    w.addEventListener("error", (ev) => this.lost(w, (ev as ErrorEvent).message ?? "the engine worker failed"));
    const hosts = this.o.hosts ?? Math.max(1, Math.floor((globalThis.navigator?.hardwareConcurrency ?? 2) / 2));
    const meshes = this.starts === 1 ? this.o.meshes : undefined;
    w.postMessage({ type: "start", moduleUrl: this.o.moduleUrl, hosts, budgetMb: this.o.budgetMb ?? 1024, faults: this.o.faults, hostTimeoutMs: this.o.hostTimeoutMs, meshes } satisfies EngineIn, meshes ? [meshes] : []);
  }

  /** The engine program's identity and start time, once started. */
  ready() {
    return this.started;
  }

  private onMessage(w: Worker, m: EngineOut) {
    if (m.type === "reply") {
      const p = this.pending.get(m.id);
      if (!p) return;
      this.pending.delete(m.id);
      if (!m.ok) return p.reject(m.error);
      if (m.stats) this.lastStats = m.stats;
      if (m.bytes) p.resolve(m.bytes);
      else p.resolve(JSON.parse(new TextDecoder().decode(m.json!)));
    } else if (m.type === "fatal") this.lost(w, m.error);
  }

  /** The engine worker stopped: its pending requests fail, and another engine starts. */
  private lost(w: Worker, why: string) {
    if (w !== this.worker) return;
    w.terminate();
    for (const p of this.pending.values()) p.reject(`the engine stopped: ${why}`);
    this.pending.clear();
    this.start();
  }

  private send(m: EngineIn) {
    return new Promise<unknown>((resolve, reject) => {
      this.pending.set((m as { id: number }).id, { resolve, reject });
      this.worker.postMessage(m);
    });
  }

  /** A command of the engine (`isEngineCommand`), as Tauri's `invoke` answers it. */
  request(cmd: string, args?: unknown) {
    return this.send({ type: "invoke", id: this.next++, cmd, args: args ?? null });
  }

  /** The engine's file system (`/work`, W1e mirrors the workspace there): write a file. */
  writeFile(path: string, data: Uint8Array) {
    const copy = data.slice().buffer;
    return new Promise<unknown>((resolve, reject) => {
      const id = this.next++;
      this.pending.set(id, { resolve, reject });
      this.worker.postMessage({ type: "fs", id, op: "write", path, data: copy } satisfies EngineIn, [copy]);
    });
  }

  readFile(path: string) {
    return this.send({ type: "fs", id: this.next++, op: "read", path }) as Promise<ArrayBuffer>;
  }

  list(path: string) {
    return this.send({ type: "fs", id: this.next++, op: "list", path }) as Promise<string[]>;
  }

  stop() {
    this.worker.terminate();
  }

  /**
   * Tauri's `invoke` in the browser: engine commands to the engine worker, `view_*` to the
   * render worker, the window's commands answered here. Rejections are the error text, as
   * Tauri's are.
   */
  invoke = <T>(cmd: string, args?: unknown, options?: { headers?: Record<string, string> }): Promise<T> => {
    if (isEngineCommand(cmd)) return this.request(cmd, args) as Promise<T>;
    if (cmd.startsWith("view_") || cmd === "app_info") {
      if (this.o.render) return this.o.render(cmd, args, options) as Promise<T>;
      return Promise.reject(`${cmd}: no viewport in the browser yet (W1c)`);
    }
    switch (cmd) {
      case "app_ready":
      case "app_close":
        return Promise.resolve(null as T);
      case "app_doc":
        if (typeof document !== "undefined") document.title = (args as { title: string }).title;
        return Promise.resolve(null as T);
      // No native file dialogs in the browser: the workspace replaces them (W1e).
      case "app_pick_file":
        return Promise.resolve(null as T);
      case "app_process_stats": {
        const mem = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory;
        return Promise.resolve({ workingSetMb: mem ? mem.usedJSHeapSize / 2 ** 20 : 0, peakMb: 0, threads: 0, cpus: globalThis.navigator?.hardwareConcurrency ?? 0 } as T);
      }
      default:
        return Promise.reject(`${cmd}: not available in the browser`);
    }
  };
}
