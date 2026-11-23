// The browser's Web Workers under Node, for the checks of src/web/ (OVERHAUL_PLAN W1b): a
// `Worker` class over node:worker_threads that runs the bundled worker scripts with `self`,
// `postMessage`, `onmessage` and `close` as a dedicated worker has them (and `Worker` again,
// for the engine worker's host workers). Workers are named by the URL the source gives
// (`new URL("./host.worker.ts", import.meta.url)`); the bundle beside it has `.js` instead.
import { Worker as NodeWorker, parentPort, workerData } from "node:worker_threads";

const boot = new URL(import.meta.url);

/** Installs `globalThis.Worker`. */
export function installWorker() {
  globalThis.Worker = class Worker extends EventTarget {
    onmessage = null;
    onerror = null;
    constructor(url) {
      super();
      const entry = String(url).replace(/\.ts$/, ".js");
      this.w = new NodeWorker(boot, { workerData: { entry } });
      this.w.on("message", (data) => {
        const ev = new MessageEvent("message", { data });
        this.onmessage?.(ev);
        this.dispatchEvent(ev);
      });
      this.w.on("error", (e) => {
        const ev = new Event("error");
        ev.message = String(e?.message ?? e);
        this.onerror?.(ev);
        this.dispatchEvent(ev);
      });
      // A worker that ends by itself (close() after a trap) without having said so.
      this.w.on("exit", () => {
        if (this.terminated) return;
        const ev = new Event("error");
        ev.message = "the worker exited";
        this.dispatchEvent(ev);
      });
    }
    postMessage(m, transfer) {
      this.w.postMessage(m, transfer);
    }
    terminate() {
      this.terminated = true;
      void this.w.terminate();
    }
  };
}

// Inside a worker thread started above: a dedicated worker's globals, then its script.
if (parentPort && workerData?.entry) {
  installWorker();
  globalThis.self = globalThis;
  globalThis.postMessage = (m, transfer) => parentPort.postMessage(m, transfer);
  globalThis.close = () => process.exit(0);
  await import(workerData.entry);
  // Listening only now: messages sent before the script set `onmessage` wait in the port.
  parentPort.on("message", (data) => globalThis.onmessage?.(new MessageEvent("message", { data })));
}
