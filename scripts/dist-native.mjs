// `npm run dist:native:win`: the Windows installer of the native desktop app (crates/oc-app,
// OVERHAUL_PLAN F3) in release/OpenCAD-<version>-win-x64-setup.exe.
//
// 1. Build the web UI (vite) and copy it to target/dist/web without the TypeScript
//    engine's OCCT WebAssembly and its worker: the native app never loads them, and an
//    embedded copy of OCCT could not be replaced by the user (LGPL, docs/THIRD_PARTY.md).
// 2. `tauri build --no-bundle` with OCCT_DIR set to a **shared** OCCT 8.0.1 install, into
//    its own cargo target directory (target/dist/cargo), so development builds against the
//    static install are left alone.
// 3. Stage what goes next to opencad.exe: the OCCT DLLs it needs (followed through their
//    imports; it must import OCCT dynamically or the build stops), the Visual C++ runtime
//    DLLs they need (app-local), OCCT's two licence texts and THIRD_PARTY_NOTICES.txt.
// 4. `tauri bundle --bundles nsis` with those files as resources in the install directory.
// 5. Copy the installer to release/ and print its size and SHA-256.
//
// Environment:
//   OCCT_SHARED_DIR  shared OCCT 8.0.1 install (default: OCCT_DIR if it is shared, else
//                    C:\occt\install-8.0.1-shared). Build one with
//                    crates\oc-kernel-occt\build-occt.cmd <src> <build> <install> Shared
//   VC_REDIST_DIR    folder holding vcruntime140.dll etc. (default: the newest
//                    Microsoft.VC14x.CRT of Visual Studio 2022 or later)
//   TAURI_CLI        path to the Tauri CLI's tauri.js (default: @tauri-apps/cli from node_modules)
// Flags: --skip-web (reuse dist/), --skip-build (reuse the built exe; stage and bundle only).
import { execFileSync, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { basename, dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { dllClosure, peImports } from "./native-pe.mjs";
import { thirdPartyNotices } from "./third-party-notices.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const app = join(root, "crates/oc-app");
const out = join(root, "target/dist");
const flags = new Set(process.argv.slice(2));
const fail = (msg) => {
  console.error(`dist-native: ${msg}`);
  process.exit(1);
};
const step = (msg) => console.log(`\n== ${msg}`);
if (process.platform !== "win32") fail("Windows only for now (OVERHAUL_PLAN F3: Windows first; macOS packaging is a follow-up).");

// Versions must agree (also checked by `cargo test -p oc-app`).
const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
const confVersion = JSON.parse(readFileSync(join(app, "tauri.conf.json"), "utf8")).version;
const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(readFileSync(join(app, "Cargo.toml"), "utf8"))?.[1];
if (confVersion !== version || cargoVersion !== version) {
  fail(`version mismatch: package.json ${version}, tauri.conf.json ${confVersion}, crates/oc-app/Cargo.toml ${cargoVersion}`);
}

// Shared OCCT.
const occtBinOf = (dir) => (dir ? [join(dir, "win64/vc14/bin"), join(dir, "bin")].find((b) => existsSync(join(b, "TKernel.dll"))) : undefined);
const occt =
  process.env.OCCT_SHARED_DIR ?? (occtBinOf(process.env.OCCT_DIR) ? process.env.OCCT_DIR : "C:\\occt\\install-8.0.1-shared");
const occtBin = occtBinOf(occt);
if (!occtBin) {
  fail(
    `no shared OCCT install at ${occt} (TKernel.dll not found). Build one with\n` +
      `  crates\\oc-kernel-occt\\build-occt.cmd C:\\occt\\OCCT-8.0.1 C:\\occt\\build-8.0.1-shared C:\\occt\\install-8.0.1-shared Shared\n` +
      `or set OCCT_SHARED_DIR.`,
  );
}
for (const f of ["LICENSE_LGPL_21.txt", "OCCT_LGPL_EXCEPTION.txt"]) if (!existsSync(join(occt, f))) fail(`${f} missing from ${occt}`);

// Visual C++ runtime (app-local copy of Microsoft's redistributable DLLs).
function vcRedist() {
  if (process.env.VC_REDIST_DIR) return process.env.VC_REDIST_DIR;
  const fromEnv = process.env.VCToolsRedistDir && join(process.env.VCToolsRedistDir, "x64");
  const dirs = [];
  if (fromEnv && existsSync(fromEnv)) dirs.push(fromEnv);
  for (const pf of [process.env.ProgramFiles, process.env["ProgramFiles(x86)"]].filter(Boolean)) {
    const vs = join(pf, "Microsoft Visual Studio");
    if (!existsSync(vs)) continue;
    for (const year of readdirSync(vs)) {
      for (const ed of existsSync(join(vs, year)) ? readdirSync(join(vs, year)) : []) {
        const msvc = join(vs, year, ed, "VC/Redist/MSVC");
        if (!existsSync(msvc)) continue;
        for (const v of readdirSync(msvc).filter((v) => /^\d/.test(v)).sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))) {
          dirs.push(join(msvc, v, "x64"));
        }
      }
    }
  }
  for (const d of dirs) {
    const crt = existsSync(d) ? readdirSync(d).find((n) => /^Microsoft\.VC\d+\.CRT$/.test(n)) : undefined;
    if (crt) return join(d, crt);
  }
  fail("Visual C++ redistributable DLLs not found; set VC_REDIST_DIR to a Microsoft.VC14x.CRT folder.");
}
const crtDir = vcRedist();

const tauriCli = process.env.TAURI_CLI ?? createRequire(join(root, "package.json")).resolve("@tauri-apps/cli/tauri.js");
const cargoTarget = join(out, "cargo");
const env = { ...process.env, OCCT_DIR: occt, CARGO_TARGET_DIR: cargoTarget };
const tauri = (args) => {
  console.log(`> tauri ${args.join(" ")}`);
  const r = spawnSync(process.execPath, [tauriCli, ...args], { cwd: app, env, stdio: "inherit" });
  if (r.status !== 0) fail(`tauri ${args[0]} failed (${r.status ?? r.error})`);
};

console.log(`OpenCAD ${version}: OCCT ${occtBin}, C++ runtime ${crtDir}`);

// 1. Web UI.
const web = join(out, "web");
if (!flags.has("--skip-web") && !flags.has("--skip-build")) {
  step("web UI (npm run build)");
  const r = spawnSync("npm", ["run", "build"], { cwd: root, stdio: "inherit", shell: true });
  if (r.status !== 0) fail("npm run build failed");
}
if (!flags.has("--skip-build")) {
  rmSync(web, { recursive: true, force: true });
  const dropped = [];
  cpSync(join(root, "dist"), web, {
    recursive: true,
    filter: (src) => {
      const drop = /^(replicad_single-.*\.wasm|kernel\.worker-.*\.js)$/.test(basename(src));
      if (drop) dropped.push(basename(src));
      return !drop;
    },
  });
  if (!existsSync(join(web, "index.html"))) fail("dist/index.html missing: run npm run build");
  console.log(`web UI staged in ${web} (left out: ${dropped.join(", ") || "nothing"})`);
}
// frontendDist relative to crates/oc-app: an absolute Windows path ("C:\…") would be read
// as a URL with the scheme "c", and the program would open that folder instead of
// embedding the UI.
const overlay = (extra = {}) => {
  const file = join(out, "tauri.dist.json");
  const frontendDist = relative(app, web).replaceAll("\\", "/");
  writeFileSync(file, JSON.stringify({ build: { frontendDist }, ...extra }, null, 2));
  return file;
};

// 2. Program.
const exe = join(cargoTarget, "release/opencad.exe");
if (!flags.has("--skip-build")) {
  step("program (tauri build --no-bundle, shared OCCT)");
  mkdirSync(out, { recursive: true });
  tauri(["build", "--no-bundle", "--ci", "--config", overlay()]);
}
if (!existsSync(exe)) fail(`${exe} missing`);

// 3. Files next to the program.
step("DLLs, licences and notices");
if (!peImports(exe).some((n) => /^TK.*\.dll$/i.test(n))) {
  fail(`${exe} imports no OCCT DLL: it was linked against a static OCCT, which must not be shipped (docs/THIRD_PARTY.md).`);
}
const { files: dlls } = dllClosure(exe, [occtBin, crtDir]);
const stage = join(out, "stage");
rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });
const resources = {};
const put = (src, name = basename(src)) => {
  copyFileSync(src, join(stage, name));
  resources[join(stage, name)] = name;
};
const occtDlls = [...dlls.values()].filter((p) => dirname(p) === occtBin).sort();
const crtDlls = [...dlls.values()].filter((p) => dirname(p) === crtDir).sort();
occtDlls.forEach((p) => put(p));
crtDlls.forEach((p) => put(p));
put(join(occt, "LICENSE_LGPL_21.txt"));
put(join(occt, "OCCT_LGPL_EXCEPTION.txt"));
const extra = [
  `Open CASCADE Technology (OCCT) 8.0.1
--------------------------------------
Licence: GNU Lesser General Public License 2.1 with the Open CASCADE Exception 1.0
(LICENSE_LGPL_21.txt and OCCT_LGPL_EXCEPTION.txt in this folder).
OpenCAD uses OCCT unmodified, dynamically linked. Its libraries are the TK*.dll files in
this folder (${occtDlls.length} files) and can be replaced by compatible builds of OCCT 8.0.1.
Source: https://github.com/Open-Cascade-SAS/OCCT/archive/refs/tags/V8.0.1.tar.gz
Build configuration: crates/oc-kernel-occt/build-occt.cmd in OpenCAD's source
(FoundationClasses, ModelingData, ModelingAlgorithms and DataExchange; Shared).`,
  `Microsoft Visual C++ runtime
----------------------------
${crtDlls.map((p) => basename(p)).join(", ")}: redistributable files of Microsoft Visual
Studio (${basename(dirname(dirname(crtDir)))}), deployed next to the program under the Visual Studio licence
terms for distributable code.`,
  `Microsoft Edge WebView2 Runtime
-------------------------------
The user interface runs in the WebView2 Runtime that is part of Windows 10 and 11. It is not
included; the installer downloads Microsoft's bootstrapper only where it is missing.`,
  `NSIS (Nullsoft Scriptable Install System)
-----------------------------------------
The installer and uninstaller are built with NSIS (zlib/libpng licence,
https://nsis.sourceforge.io/License) and the Tauri bundler (MIT or Apache-2.0).`,
];
const notices = thirdPartyNotices({ product: "OpenCAD", version, extra });
if (notices.missing.length) fail(`no licence text for: ${notices.missing.join(", ")}`);
writeFileSync(join(stage, "THIRD_PARTY_NOTICES.txt"), notices.text.replace(/\n/g, "\r\n"));
resources[join(stage, "THIRD_PARTY_NOTICES.txt")] = "THIRD_PARTY_NOTICES.txt";
console.log(`${occtDlls.length} OCCT DLLs, ${crtDlls.length} C++ runtime DLLs, notices for ${notices.crates.length} crates and ${notices.ui.length} UI packages`);

// 4. Installer.
step("installer (tauri bundle --bundles nsis)");
const nsisDir = join(cargoTarget, "release/bundle/nsis");
rmSync(nsisDir, { recursive: true, force: true });
tauri(["bundle", "--bundles", "nsis", "--ci", "--config", overlay({ bundle: { resources } })]);
const built = existsSync(nsisDir) ? readdirSync(nsisDir).find((n) => n.endsWith("-setup.exe") && n.includes(version)) : undefined;
if (!built) fail(`no installer in ${nsisDir}`);

// 5. release/.
const target = join(root, "release", `OpenCAD-${version}-win-x64-setup.exe`);
mkdirSync(dirname(target), { recursive: true });
copyFileSync(join(nsisDir, built), target);
const bytes = statSync(target).size;
const sha = createHash("sha256").update(readFileSync(target)).digest("hex");
console.log(`\n${target}\n${(bytes / 1048576).toFixed(1)} MB (${bytes} bytes), SHA-256 ${sha}`);
console.log("Check it: npm run test:native:install");
// Keep the git commit in the log, for the release notes.
try {
  console.log(`commit ${execFileSync("git", ["-C", root, "rev-parse", "--short", "HEAD"], { encoding: "utf8" }).trim()}`);
} catch {
  /* not a checkout */
}
