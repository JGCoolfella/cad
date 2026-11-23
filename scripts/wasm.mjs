// The engine crates on WebAssembly (OVERHAUL_PLAN W1a): runs cargo for the
// `wasm32-unknown-emscripten` target with the pinned Emscripten, and builds OCCT's side module.
//
//   node scripts/wasm.mjs test [cargo test args]   engine crates' tests in WebAssembly, on Node
//   node scripts/wasm.mjs side                     OCCT's side module (lib/libocct.so)
//   node scripts/wasm.mjs cargo <args>             any cargo command with the toolchain set up
//   node scripts/wasm.mjs web [--debug]            the browser's engine program (oc-web, W1b) into
//                                                  target/web/<side|static>/ (OC_WEB_OUT overrides)
//
// Environment: EMSDK_ROOT (default C:\occt\emsdk, Emscripten 7.0.1 activated), OCCT_WASM_DIR
// (default C:\occt\install-8.0.1-wasm, built by crates/oc-kernel-occt/build-occt-wasm.cmd),
// CARGO_TARGET_DIR as usual. OC_WASM_LINK=static links OCCT's archives into each program
// even when the side module exists (to compare the two).
import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { join, delimiter } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const TARGET = "wasm32-unknown-emscripten";
export const EMSCRIPTEN = "7.0.1";
/** The engine crates (W1a): everything the engine thread and the geometry hosts use, no shell. */
export const CRATES = ["oc-doc", "oc-sketch", "oc-kernel", "oc-kernel-occt", "oc-naming", "oc-mesh", "oc-graph", "oc-mate", "oc-asm", "oc-engine"];

const win = process.platform === "win32";
const emsdk = process.env.EMSDK_ROOT ?? (win ? "C:\\occt\\emsdk" : `${process.env.HOME}/emsdk`);
const occt = process.env.OCCT_WASM_DIR ?? (win ? "C:\\occt\\install-8.0.1-wasm" : `${process.env.HOME}/occt/install-8.0.1-wasm`);
const em = join(emsdk, "upstream", "emscripten");
const exe = (n) => join(em, win ? `${n}.exe` : n);

/** Link flags of every engine program (tests, the engine and host workers). */
export function linkFlags({ side, node }) {
  const f = [
    "-fwasm-exceptions", // OCCT and the shim throw; Rust's target uses WebAssembly exceptions
    "-sDEFAULT_TO_CXX", // libc++ for the shim and OCCT
    "-sALLOW_MEMORY_GROWTH=1",
    "-sMAXIMUM_MEMORY=4GB",
    "-sSTACK_SIZE=8MB", // OCCT recurses deeply (booleans, healing); Emscripten's default is 64 KB
  ];
  if (side) f.push("-sMAIN_MODULE=2"); // loads OCCT's side module at start
  if (node) f.push("-sNODERAWFS=1"); // tests read the corpus from the real file system
  return f;
}

export function env({ node = true } = {}) {
  if (!existsSync(exe("emcc"))) throw new Error(`no Emscripten under ${emsdk} (install emsdk ${EMSCRIPTEN}: see crates/oc-kernel-occt/build-occt-wasm.cmd)`);
  const side = process.env.OC_WASM_LINK !== "static" && existsSync(join(occt, "lib", "libocct.so"));
  const t = TARGET.toUpperCase().replaceAll("-", "_");
  const u = TARGET.replaceAll("-", "_");
  const paths = [em, join(process.env.USERPROFILE ?? process.env.HOME ?? "", ".cargo", "bin")].filter(Boolean);
  const e = {
    ...process.env,
    PATH: [...paths, process.env.PATH].join(delimiter),
    EMSDK: emsdk.replaceAll("\\", "/"),
    OCCT_DIR: occt,
    [`CC_${u}`]: exe("emcc"),
    [`CXX_${u}`]: exe("em++"),
    [`AR_${u}`]: exe("emar"),
    [`CXXFLAGS_${u}`]: "-fwasm-exceptions",
    [`CARGO_TARGET_${t}_LINKER`]: exe("em++"),
    [`CARGO_TARGET_${t}_RUSTFLAGS`]: linkFlags({ side, node }).map((a) => `-Clink-arg=${a}`).join(" "),
    // Puts OCCT's side module next to each program before running it under Node.
    [`CARGO_TARGET_${t}_RUNNER`]: `node ${fileURLToPath(new URL("wasm-runner.mjs", import.meta.url))}`,
  };
  if (side) e.OC_WASM_SIDE = join(occt, "lib", "libocct.so");
  return { env: e, side };
}

function run(cmd, args, e) {
  const r = spawnSync(cmd, args, { stdio: "inherit", env: e, shell: false });
  if (r.error) throw r.error;
  return r.status ?? 1;
}

/**
 * Emscripten writes CommonJS programs (`.js`); the repository's `package.json` says
 * `"type": "module"`, which Node would apply to a target folder inside it.
 */
function commonJsTargetDir() {
  const root = fileURLToPath(new URL("..", import.meta.url));
  const dir = join(process.env.CARGO_TARGET_DIR ?? join(root, "target"), TARGET);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "package.json"), '{ "type": "commonjs" }\n');
}

function cargo(args, opts) {
  commonJsTargetDir();
  const { env: e, side } = env(opts);
  console.error(`wasm: ${TARGET}, OCCT ${side ? "side module" : "static archives"} from ${occt}`);
  return run("cargo", args, e);
}

const root = fileURLToPath(new URL("..", import.meta.url));

/** Where `web` puts the browser's engine program for one OCCT form. */
export function webOut(side) {
  return process.env.OC_WEB_OUT ?? join(root, "target", "web", side ? "side" : "static");
}

/**
 * Builds `oc-web` (release unless `debug`) and copies the program (`oc-web.js`, an ES module,
 * and `oc_web.wasm`) and, for the side-module form, OCCT's `libocct.so` into `webOut`. Returns
 * the folder, or null when the build failed.
 */
export function buildWeb({ debug = false } = {}) {
  commonJsTargetDir();
  const { env: e, side } = env({ node: false });
  console.error(`wasm: oc-web, OCCT ${side ? "side module" : "static archives"} from ${occt}`);
  const args = ["build", "--target", TARGET, "-p", "oc-web", ...(debug ? [] : ["--release"])];
  if (run("cargo", args, e) !== 0) return null;
  const from = join(process.env.CARGO_TARGET_DIR ?? join(root, "target"), TARGET, debug ? "debug" : "release");
  const out = webOut(side);
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  for (const f of ["oc-web.js", "oc_web.wasm"]) copyFileSync(join(from, f), join(out, f));
  if (side) copyFileSync(e.OC_WASM_SIDE, join(out, "libocct.so"));
  const mb = (f) => (statSync(join(out, f)).size / 1e6).toFixed(1);
  console.error(`wasm: ${out}: oc-web.js ${mb("oc-web.js")} MB, oc_web.wasm ${mb("oc_web.wasm")} MB${side ? `, libocct.so ${mb("libocct.so")} MB` : ""}`);
  return out;
}

const main = process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url;
const [cmd, ...rest] = main ? process.argv.slice(2) : [];
if (cmd === "test") {
  const packages = rest.some((a) => a === "-p" || a.startsWith("--package")) ? [] : CRATES.flatMap((c) => ["-p", c]);
  process.exit(cargo(["test", "--target", TARGET, ...packages, ...rest]));
} else if (cmd === "cargo") {
  process.exit(cargo(rest));
} else if (cmd === "web") {
  process.exit(buildWeb({ debug: rest.includes("--debug") }) ? 0 : 1);
} else if (cmd === "side") {
  const { buildSide } = await import("./build-occt-side.mjs");
  process.exit(buildSide({ emsdk, occt, exe, env: env().env }));
} else if (cmd) {
  console.error(`unknown command ${cmd}`);
  process.exit(2);
}
