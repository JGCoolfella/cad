import type { Feature, RebuildResult } from "../core/types";
import type { KernelRequest, KernelResponse } from "./protocol";

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void };

type DistributiveOmit<T, K extends keyof never> = T extends unknown ? Omit<T, K> : never;

/**
 * Promise-based client for the kernel worker. Rebuild requests are coalesced:
 * if several arrive while one is running, only the latest is executed.
 */
export class KernelClient {
  private worker: Worker;
  private seq = 0;
  private pending = new Map<number, Pending>();
  private rebuildBusy = false;
  private queued: { features: Feature[]; preview?: Feature; waiters: Pending[] } | null = null;
  /** Image quality 0..10 (Document Properties > Image Quality). */
  quality = 3;

  constructor() {
    this.worker = new Worker(new URL("./kernel.worker.ts", import.meta.url), { type: "module" });
    this.worker.onmessage = (ev: MessageEvent<KernelResponse>) => {
      const r = ev.data;
      const p = this.pending.get(r.id);
      if (!p) return;
      this.pending.delete(r.id);
      if (r.ok) p.resolve(r.result);
      else p.reject(new Error(r.error));
    };
  }

  private call<T>(req: DistributiveOmit<KernelRequest, "id">): Promise<T> {
    const id = ++this.seq;
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: resolve as (v: unknown) => void, reject });
      this.worker.postMessage({ ...req, id });
    });
  }

  init() {
    return this.call<null>({ op: "init" });
  }

  /** Rebuild, dropping intermediate requests if the kernel is busy (latest wins). */
  rebuild(features: Feature[], preview?: Feature): Promise<RebuildResult> {
    return new Promise((resolve, reject) => {
      const waiter = { resolve: resolve as (v: unknown) => void, reject };
      if (this.rebuildBusy) {
        if (this.queued) {
          // Superseded requests resolve with the newer result.
          this.queued.features = features;
          this.queued.preview = preview;
          this.queued.waiters.push(waiter);
        } else this.queued = { features, preview, waiters: [waiter] };
        return;
      }
      this.run(features, preview, [waiter]);
    });
  }

  private async run(features: Feature[], preview: Feature | undefined, waiters: Pending[]) {
    this.rebuildBusy = true;
    try {
      const r = await this.call<RebuildResult>({ op: "rebuild", features, preview, quality: this.quality });
      waiters.forEach((w) => w.resolve(r));
    } catch (e) {
      waiters.forEach((w) => w.reject(e as Error));
    } finally {
      this.rebuildBusy = false;
      const q = this.queued;
      this.queued = null;
      if (q) this.run(q.features, q.preview, q.waiters);
    }
  }

  exportFile(features: Feature[], format: "step" | "stl") {
    return this.call<ArrayBuffer>({ op: "export", features, format });
  }

  /** Force Rebuild: drop every cached feature result in the worker. */
  clearCache() {
    return this.call<null>({ op: "clearCache" });
  }

  massProps(features: Feature[]) {
    return this.call<{ volume: number; area: number; centerOfMass: [number, number, number] } | null>({ op: "massProps", features });
  }
}
