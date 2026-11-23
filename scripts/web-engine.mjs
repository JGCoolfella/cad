// Checks of the browser's engine workers under Node (OVERHAUL_PLAN W1b):
//
//   node scripts/web-engine.mjs [--no-build] [--hosts=N] [--rounds=N]    (npm run test:web-engine)
//
// Builds the engine program (`node scripts/wasm.mjs web`: OCCT as its side module, or linked
// in with OC_WASM_LINK=static), bundles src/web/ (the workers and the check scenario,
// src/web/check.ts) with Vite's esbuild, runs the workers on worker_threads
// (scripts/web-worker-shim.mjs) and prints `WEB {...}`: start-up, request round trips, the
// corpus through the engine worker against the oracle, the corpus assemblies, every corpus
// part at once on host workers and in the engine alone, and host workers that fail. Exit 1
// if `failures` is not empty.
import { readFileSync, readdirSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { build } from "esbuild";
import { buildWeb, env, webOut } from "./wasm.mjs";
import { installWorker } from "./web-worker-shim.mjs";

const root = fileURLToPath(new URL("..", import.meta.url));
const flag = (n, d) => process.argv.find((a) => a.startsWith(`--${n}=`))?.split("=")[1] ?? d;

const out = process.argv.includes("--no-build") ? webOut(env({ node: false }).side) : buildWeb();
if (!out) process.exit(1);

// The workers and the scenario, bundled side by side (a worker's URL names its bundle).
const bundles = join(root, "target", "web", "check");
mkdirSync(bundles, { recursive: true });
await build({
  entryPoints: ["src/web/check.ts", "src/web/engine.worker.ts", "src/web/host.worker.ts"].map((f) => join(root, f)),
  outdir: bundles,
  outbase: join(root, "src", "web"),
  bundle: true,
  format: "esm",
  platform: "browser",
  target: "es2022",
  logLevel: "warning",
});
writeFileSync(join(bundles, "package.json"), '{ "type": "module" }\n');
installWorker();

const corpus = join(root, "corpus");
const parts = readdirSync(corpus)
  .filter((f) => f.endsWith(".ocpart"))
  .sort()
  .map((f) => {
    const name = f.replace(/\.ocpart$/, "");
    return { name, doc: readFileSync(join(corpus, f), "utf8"), oracle: JSON.parse(readFileSync(join(corpus, `${name}.json`), "utf8")) };
  });
const adir = join(corpus, "assemblies");
const assemblyFiles = readdirSync(adir)
  .filter((f) => /\.(ocpart|ocasm)$/.test(f))
  .map((name) => ({ name, bytes: new Uint8Array(readFileSync(join(adir, name))) }));
const assemblies = readdirSync(adir)
  .filter((f) => f.endsWith(".ocasm"))
  .map((f) => f.replace(/\.ocasm$/, ""))
  .map((name) => ({ name, record: JSON.parse(readFileSync(join(adir, `${name}.json`), "utf8")) }));

const { run } = await import(pathToFileURL(join(bundles, "check.js")).href);
const t0 = performance.now();
const report = await run({
  moduleUrl: pathToFileURL(join(out, "oc-web.js")).href,
  parts,
  assemblies,
  assemblyFiles,
  hosts: Number(flag("hosts", Math.max(1, Math.floor(navigator.hardwareConcurrency / 2)))),
  rounds: Number(flag("rounds", 200)),
}).catch((e) => ({ failures: [`exception: ${e?.stack ?? e}`] }));
report.totalS = +((performance.now() - t0) / 1000).toFixed(1);
report.node = process.version;
console.log(`WEB ${JSON.stringify(report)}`);
process.exit(report.failures.length ? 1 : 0);
