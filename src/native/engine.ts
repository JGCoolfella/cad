/**
 * Kernel client of the native app: the same requests `KernelClient` sends to the
 * WebAssembly worker, served by `crates/oc-app` (oc-graph on OCCT 8, meshed by oc-mesh).
 * Rebuild replies are binary (`crates/oc-app/src/pack.rs`): a JSON header plus typed
 * arrays, decoded here without copying.
 */
import type { EdgeSig, FaceSig, Feature, Frame, RebuildResult } from "../core/types";
import type { ConvertedCurve, MeasureRef, MeasureResult } from "../kernel/protocol";
import type { KernelApi } from "../kernel/client";
import { invoke } from "./tauri";

type Pending = { resolve: (v: RebuildResult) => void; reject: (e: Error) => void };

/** Replaces `{ $b, n, t }` array references in the header by views on the reply. */
function hydrate(v: unknown, buf: ArrayBuffer, base: number): unknown {
  if (Array.isArray(v)) return v.map((x) => hydrate(x, buf, base));
  if (v && typeof v === "object") {
    const o = v as Record<string, unknown>;
    if (typeof o.$b === "number" && typeof o.n === "number") {
      const at = base + o.$b;
      return o.t === "u32" ? new Uint32Array(buf, at, o.n) : new Float32Array(buf, at, o.n);
    }
    for (const k of Object.keys(o)) o[k] = hydrate(o[k], buf, base);
  }
  return v;
}

export function decodeRebuild(buf: ArrayBuffer): RebuildResult {
  const head = new Uint8Array(buf, 0, 8);
  if (String.fromCharCode(head[0], head[1], head[2], head[3]) !== "OCR1") throw new Error("bad rebuild reply");
  const n = new DataView(buf).getUint32(4, true);
  const json = JSON.parse(new TextDecoder().decode(new Uint8Array(buf, 8, n)));
  return hydrate(json, buf, 8 + n) as RebuildResult;
}

export class NativeKernelClient implements KernelApi {
  quality = 3;
  /** The part's active configuration (G1e): the engine keeps its results cached for when it is activated again. */
  config: string | null = null;
  /** Newest mesh handle received (the viewport frees older ones the app no longer holds). */
  seen = 0;
  private busy = false;
  private queued: { features: Feature[]; preview?: Feature; waiters: Pending[] } | null = null;

  init() {
    return Promise.resolve(null);
  }

  /** Rebuild; while one runs only the newest request is kept (as `KernelClient`). */
  rebuild(features: Feature[], preview?: Feature): Promise<RebuildResult> {
    return new Promise((resolve, reject) => {
      const w = { resolve, reject };
      if (this.busy) {
        if (this.queued) Object.assign(this.queued, { features, preview }).waiters.push(w);
        else this.queued = { features, preview, waiters: [w] };
        return;
      }
      this.run(features, preview, [w]);
    });
  }

  private async run(features: Feature[], preview: Feature | undefined, waiters: Pending[]) {
    this.busy = true;
    try {
      const buf = await invoke<ArrayBuffer>("engine_rebuild", { features, preview: preview ?? null, quality: this.quality, config: this.config });
      const r = decodeRebuild(buf);
      for (const m of [r.mesh, r.toolMesh]) if (m?.handle) this.seen = Math.max(this.seen, m.handle);
      waiters.forEach((w) => w.resolve(r));
    } catch (e) {
      waiters.forEach((w) => w.reject(e instanceof Error ? e : new Error(String(e))));
    } finally {
      this.busy = false;
      const q = this.queued;
      this.queued = null;
      if (q) this.run(q.features, q.preview, q.waiters);
    }
  }

  exportFile(features: Feature[], format: "step" | "stl") {
    return invoke<ArrayBuffer>("engine_export", { features, format, quality: this.quality });
  }

  convert(features: Feature[], frame: Frame, edges: EdgeSig[], faces: FaceSig[]) {
    return invoke<ConvertedCurve[]>("engine_convert", { features, frame, edges, faces });
  }

  measure(features: Feature[], refs: MeasureRef[]) {
    return invoke<MeasureResult | null>("engine_measure", { features, refs });
  }

  clearCache() {
    return invoke<null>("engine_clear_cache");
  }

  massProps(features: Feature[]) {
    return invoke<{ volume: number; area: number; centerOfMass: [number, number, number] } | null>("engine_mass_props", { features });
  }

  /** Viewport GPU adapter and backend. */
  adapter() {
    return invoke<string>("app_info");
  }
}
