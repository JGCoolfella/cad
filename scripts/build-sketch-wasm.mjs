// Builds the oc-sketch WebAssembly binding into src/sketch/wasm/ (committed, so `npm test`
// runs the parity test without a Rust toolchain). Needs Rust with the wasm32-unknown-unknown
// target and wasm-bindgen-cli 0.2.130 (`cargo install wasm-bindgen-cli --version 0.2.130`).
// Writes src/sketch/wasm/SOURCE_HASH, which `cargo test` (crates/oc-sketch-wasm/tests/fresh.rs)
// checks against the current sources.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { homedir } from "node:os";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "src/sketch/wasm");
const exe = process.platform === "win32" ? ".exe" : "";
const tool = (name) => {
  const p = join(homedir(), ".cargo", "bin", name + exe);
  return existsSync(p) ? p : name;
};
// No debug info in the shipped module (the workspace release profile keeps line tables).
const env = { ...process.env, CARGO_PROFILE_RELEASE_DEBUG: "false", CARGO_PROFILE_RELEASE_STRIP: "true",
  CARGO_PROFILE_RELEASE_LTO: "true", CARGO_PROFILE_RELEASE_CODEGEN_UNITS: "1" };
const run = (cmd, args) => execFileSync(tool(cmd), args, { cwd: root, stdio: "inherit", env });

run("cargo", ["build", "-p", "oc-sketch-wasm", "--release", "--target", "wasm32-unknown-unknown"]);
run("wasm-bindgen", [
  "target/wasm32-unknown-unknown/release/oc_sketch_wasm.wasm",
  "--target", "web",
  "--out-dir", out,
  "--out-name", "oc_sketch",
  "--remove-name-section",
  "--remove-producers-section",
]);

// FNV-1a 64 over `path \0 content(LF) \0` of every source, sorted by path (as fresh.rs).
function sources() {
  const files = [];
  const walk = (dir) => {
    for (const e of readdirSync(dir)) {
      const p = join(dir, e);
      if (statSync(p).isDirectory()) walk(p);
      else if (p.endsWith(".rs")) files.push(p);
    }
  };
  for (const c of ["crates/oc-sketch", "crates/oc-sketch-wasm"]) {
    files.push(join(root, c, "Cargo.toml"));
    walk(join(root, c, "src"));
  }
  return files.map((f) => relative(root, f).split("\\").join("/")).sort();
}
let h = 0xcbf29ce484222325n;
const feed = (bytes) => {
  for (const b of bytes) {
    h ^= BigInt(b);
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
};
for (const rel of sources()) {
  const text = readFileSync(join(root, rel), "utf8").replaceAll("\r\n", "\n");
  feed(Buffer.from(rel, "utf8"));
  feed([0]);
  feed(Buffer.from(text, "utf8"));
  feed([0]);
}
writeFileSync(join(out, "SOURCE_HASH"), h.toString(16).padStart(16, "0") + "\n");
console.log(`oc-sketch wasm: ${statSync(join(out, "oc_sketch_bg.wasm")).size} bytes, source hash ${h.toString(16)}`);
