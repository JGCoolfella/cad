/// <reference lib="webworker" />
/**
 * Geometry kernel worker. Owns the OpenCascade (B-rep) instance so the UI thread never
 * blocks on a rebuild. All modelling logic lives in ./engine; this file only initialises
 * OCCT (with the bundler-resolved WASM URL) and relays messages.
 */
import opencascade from "replicad-opencascadejs";
import wasmUrl from "replicad-opencascadejs/wasm?url";
import { setOC } from "replicad";
import { errorText, handle } from "./engine";
import type { KernelRequest, KernelResponse } from "./protocol";

let ready: Promise<void> | null = null;
function init() {
  if (!ready) {
    ready = (async () => {
      const oc = await (opencascade as unknown as (o: object) => Promise<unknown>)({
        locateFile: () => wasmUrl,
      });
      setOC(oc as never);
    })();
  }
  return ready;
}

self.onmessage = async (ev: MessageEvent<KernelRequest>) => {
  const req = ev.data;
  const reply = (r: KernelResponse, transfer: Transferable[] = []) => (self as DedicatedWorkerGlobalScope).postMessage(r, transfer);
  try {
    await init();
    const { result, transfer } = await handle(req);
    reply({ id: req.id, ok: true, result }, transfer);
  } catch (e) {
    reply({ id: req.id, ok: false, error: errorText(e) });
  }
};
