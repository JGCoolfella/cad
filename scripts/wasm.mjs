// The engine crates on WebAssembly (OVERHAUL_PLAN W1a): runs cargo for the
// `wasm32-unknown-emscripten` target with the pinned Emscripten, and builds OCCT's side module.
//
//   node scripts/wasm.mjs test [cargo test args]   engine crates' tests in WebAssembly, on Node
//   node scripts/wasm.mjs side                     OCCT's side module (lib/libocct.so)
//   node scripts/wasm.mjs cargo <args>             any cargo command with the toolchain set up
//
// Environment: EMSDK_ROOT (default C:\occt\emsdk, Emscripten 7.0.1 activated), OCCT_WASM_DIR
// (default C:\occt\install-8.0.1-wasm, built by crates/oc-kernel-occt/build-occt-wasm.cmd),
// CARGO_TARGET_DIR as usual. OC_WASM_LINK=static links OCCT's archives into each program
// even when the side module exists (to compare the two).
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
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

const main = process.argv[1] && pathToFileURL(process.argv[1]).href === import.meta.url;
const [cmd, ...rest] = main ? process.argv.slice(2) : [];
if (cmd === "test") {
  const packages = rest.some((a) => a === "-p" || a.startsWith("--package")) ? [] : CRATES.flatMap((c) => ["-p", c]);
  process.exit(cargo(["test", "--target", TARGET, ...packages, ...rest]));
} else if (cmd === "cargo") {
  process.exit(cargo(rest));
} else if (cmd === "side") {
  const { buildSide } = await import("./build-occt-side.mjs");
  process.exit(buildSide({ emsdk, occt, exe, env: env().env }));
} else if (cmd) {
  console.error(`unknown command ${cmd}`);
  process.exit(2);
}
