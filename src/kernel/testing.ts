/// <reference types="node" />
/**
 * Test-only helpers: run the kernel engine under Node (vitest) without a worker or a
 * bundler. Never imported by the app.
 */
import { createRequire } from "node:module";
import { setOC } from "replicad";

let ready: Promise<void> | null = null;

/** Initialise OpenCascade (single-threaded WASM build) once per test process. */
export function loadKernel(): Promise<void> {
  if (!ready) {
    ready = (async () => {
      const require = createRequire(import.meta.url);
      const wasm = require.resolve("replicad-opencascadejs/wasm");
      const mod = require("replicad-opencascadejs") as { default?: unknown };
      const init = (mod.default ?? mod) as (o: object) => Promise<unknown>;
      const oc = await init({ locateFile: () => wasm });
      setOC(oc as never);
    })();
  }
  return ready;
}
