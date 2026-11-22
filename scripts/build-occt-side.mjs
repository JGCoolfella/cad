// OCCT as an Emscripten side module (OVERHAUL_PLAN W1a, LGPL): links the static archives
// of a WebAssembly OCCT install (crates/oc-kernel-occt/build-occt-wasm.cmd) into one dynamic
// library, `<install>/lib/libocct.so`, that the engine (an Emscripten main module) loads at
// start. This is the step a user follows to replace OCCT in the browser build: build any
// OCCT 8.0.1 with build-occt-wasm.cmd and the same Emscripten, run this, use the result.
//
//   node scripts/wasm.mjs side
//
// Every symbol of the archives is exported (SIDE_MODULE=1), as a DLL exports its interface,
// so the module does not depend on which of them this engine build happens to call.
import { spawnSync } from "node:child_process";
import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";

export function buildSide({ occt, exe, env }) {
  const lib = join(occt, "lib");
  const archives = readdirSync(lib).filter((n) => /^libTK.*\.a$/.test(n)).sort();
  if (!archives.length) throw new Error(`no OCCT archives in ${lib}`);
  const out = join(lib, "libocct.so");
  const args = [
    "-sSIDE_MODULE=1",
    "-fwasm-exceptions",
    "-O2",
    "-Wl,--whole-archive",
    ...archives.map((a) => join(lib, a)),
    "-Wl,--no-whole-archive",
    "-o",
    out,
  ];
  const t = performance.now();
  const r = spawnSync(exe("em++"), args, { stdio: "inherit", env });
  if (r.status !== 0) return r.status ?? 1;
  console.log(`${out}: ${archives.length} archives, ${(statSync(out).size / 1e6).toFixed(1)} MB, ${((performance.now() - t) / 1e3).toFixed(1)} s`);
  return 0;
}
