/// <reference lib="webworker" />
/**
 * A geometry host worker (OVERHAUL_PLAN W1b): the browser's geometry host process. It loads
 * the engine program and regenerates one part job at a time (`oc_host::serve::answer`, the
 * host process' code). A trap or OCCT abort (a WebAssembly `RuntimeError`, Emscripten's
 * abort or exit) leaves the module unusable: the worker reports it and closes, and the engine
 * worker's pool fails that part and starts another worker.
 */
import { load, own, type OcWeb } from "./module";
import type { HostIn, HostOut } from "./protocol";

const scope = self as unknown as DedicatedWorkerGlobalScope;
let mod: OcWeb | null = null;
const post = (m: HostOut, transfer: Transferable[] = []) => scope.postMessage(m, transfer);

function die(e: unknown) {
  post({ type: "died", error: String((e as Error)?.message ?? e) });
  scope.close();
}

scope.onmessage = async (ev: MessageEvent<HostIn>) => {
  const m = ev.data;
  try {
    if (m.type === "start") {
      const t0 = performance.now();
      mod = await load(m.moduleUrl);
      mod.startHost(!!m.faults);
      post({ type: "ready", startMs: performance.now() - t0 });
    } else if (m.type === "job") {
      if (!mod) throw new Error("the host is not started");
      const r = mod.hostJob(new Uint8Array(m.job));
      const payload = own(r.payload);
      post({ type: "done", kind: r.kind, payload }, [payload]);
    }
  } catch (e) {
    die(e);
  }
};
