// Cargo's runner for wasm32-unknown-emscripten programs (tests, examples), set by
// scripts/wasm.mjs: `node scripts/wasm-runner.mjs <program.js> [args]`. A program linked against
// OCCT's side module loads `libocct.so` by name from its own folder at start, so the module
// (OC_WASM_SIDE, the install's copy) is put there first; then the program runs under Node.
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, statSync } from "node:fs";
import { dirname, join } from "node:path";

const [program, ...args] = process.argv.slice(2);
const side = process.env.OC_WASM_SIDE;
if (side) {
  const here = join(dirname(program), "libocct.so");
  const fresh = existsSync(here) && statSync(here).mtimeMs >= statSync(side).mtimeMs && statSync(here).size === statSync(side).size;
  if (!fresh) copyFileSync(side, here);
}
const r = spawnSync(process.execPath, [program, ...args], { stdio: "inherit" });
process.exit(r.status ?? 1);
