// `npm run test:native:install [-- <installer.exe>]`: the native app's end-to-end check on the
// installed program (OVERHAUL_PLAN F3), not on `cargo run`.
//
// 1. Installs the NSIS installer silently into a new temporary folder (`/S /NS /D=…`: no
//    shortcuts), for the current user.
// 2. Checks the install: opencad.exe is a GUI program that imports OCCT dynamically, the
//    OCCT and C++ runtime DLLs and the licence texts are next to it.
// 3. Runs the installed opencad.exe with a minimal PATH (only Windows' folders, so nothing
//    is found outside the install folder), from this checkout (it reads corpus/ and the
//    picking script from here):
//    - `--smoke`: 7 faces, volume 20 858.407 (24 000 − 1000π), no errors;
//    - `--corpus`: `corpusDiffs: []` (17 parts against the oracle);
//    - `--script=crates/oc-app/scripts/picking.js`: every picking check passes.
//    Each run's `SMOKE {...}` line must be in its standard output and in its log file.
// 4. Uninstalls silently and checks the folder and the uninstall registry entry are gone.
// Screenshots and logs: target/dist/install-check/. Exit code 1 if anything fails.
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { peImports, peSubsystem } from "./native-pe.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
if (process.platform !== "win32") {
  console.error("test-native-install: Windows only");
  process.exit(1);
}
const version = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
const installer = resolve(process.argv[2] ?? join(root, "release", `OpenCAD-${version}-win-x64-setup.exe`));
if (!existsSync(installer)) {
  console.error(`test-native-install: ${installer} missing (npm run dist:native:win)`);
  process.exit(1);
}
const outDir = join(root, "target/dist/install-check");
mkdirSync(outDir, { recursive: true });
const results = [];
const check = (name, ok, detail) => {
  results.push({ name, ok: !!ok, detail });
  console.log(`${ok ? "PASS" : "FAIL"} ${name}${detail !== undefined ? `: ${typeof detail === "string" ? detail : JSON.stringify(detail)}` : ""}`);
};
const uninstallKey = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\OpenCAD";
const regHas = () => spawnSync("reg", ["query", uninstallKey], { stdio: "ignore" }).status === 0;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

if (regHas()) {
  console.error(`test-native-install: OpenCAD is already installed for this user (${uninstallKey}); uninstall it first.`);
  process.exit(1);
}

// 1. Install.
const dir = mkdtempSync(join(tmpdir(), "opencad-install-"));
if (/\s/.test(dir)) throw new Error(`temporary folder has a space: ${dir}`);
console.log(`installer ${installer} (${(statSync(installer).size / 1048576).toFixed(1)} MB)\ninstall folder ${dir}`);
let t = Date.now();
// NSIS: /D= must come last and unquoted.
const inst = spawnSync(`"${installer}" /S /NS /D=${dir}`, { shell: true, stdio: "inherit", timeout: 600_000 });
check("install", inst.status === 0, `exit ${inst.status}, ${((Date.now() - t) / 1000).toFixed(1)} s`);
const exe = join(dir, "opencad.exe");

// 2. Files.
const files = existsSync(dir) ? readdirSync(dir) : [];
check("opencad.exe installed", existsSync(exe), files.length + " files");
if (existsSync(exe)) {
  check("GUI subsystem", peSubsystem(exe) === 2, `subsystem ${peSubsystem(exe)}`);
  const tk = peImports(exe).filter((n) => /^TK.*\.dll$/i.test(n));
  check("imports OCCT DLLs", tk.length > 0, tk.join(" "));
  const tkFiles = files.filter((f) => /^TK.*\.dll$/i.test(f));
  check("OCCT DLLs next to the exe", tk.every((n) => files.some((f) => f.toLowerCase() === n.toLowerCase())), `${tkFiles.length} TK*.dll`);
  for (const f of ["LICENSE_LGPL_21.txt", "OCCT_LGPL_EXCEPTION.txt", "THIRD_PARTY_NOTICES.txt", "vcruntime140.dll", "msvcp140.dll", "uninstall.exe"]) {
    check(`${f} installed`, files.some((x) => x.toLowerCase() === f.toLowerCase()));
  }
  check("uninstall entry", regHas());
}

// 3. Runs.
const sys = process.env.SystemRoot ?? "C:\\Windows";
const env = { ...process.env, PATH: [join(sys, "System32"), sys, join(sys, "System32\\Wbem")].join(";") };
delete env.OCCT_DIR;
function run(name, args) {
  return new Promise((done) => {
    const png = join(outDir, `${name}.png`);
    const log = join(outDir, `${name}.log`);
    rmSync(log, { force: true });
    const t0 = Date.now();
    const p = spawn(exe, [...args, `--smoke=${png}`], { cwd: root, env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    p.stdout.on("data", (d) => (stdout += d));
    p.stderr.on("data", () => {});
    const timer = setTimeout(() => p.kill(), 600_000);
    p.on("close", (code) => {
      clearTimeout(timer);
      const line = stdout.split(/\r?\n/).find((l) => l.startsWith("SMOKE "));
      const logLine = existsSync(log) ? readFileSync(log, "utf8").trim() : null;
      let report = null;
      try {
        report = line ? JSON.parse(line.slice(6)) : null;
      } catch {
        /* reported below */
      }
      check(`${name}: SMOKE line on stdout and in ${name}.log`, line && logLine === line, { code, seconds: (Date.now() - t0) / 1000 });
      done({ code, report });
    });
  });
}

if (existsSync(exe)) {
  const smoke = await run("smoke", []);
  const r = smoke.report ?? {};
  check(
    "--smoke: 7 faces, volume 20858.407, no errors",
    smoke.code === 0 && r.faces === 7 && Math.abs(r.volume - (24000 - 1000 * Math.PI)) < 1e-3 && (r.errors ?? []).length === 0 && !r.exception,
    { faces: r.faces, volume: r.volume, errors: r.errors, rebuildMs: r.rebuildMs, occt: r.occt, app: r.app, gpu: r.gpu, exception: r.exception },
  );
  check("OCCT linked dynamically (report)", r.occt === "8.0.1 shared", r.occt);
  const corpus = await run("corpus", ["--corpus"]);
  const c = corpus.report ?? {};
  check("--corpus: corpusDiffs []", corpus.code === 0 && Array.isArray(c.corpusDiffs) && c.corpusDiffs.length === 0 && (c.parts ?? []).length === 17, {
    parts: c.parts?.length,
    corpusDiffs: c.corpusDiffs,
    exception: c.exception,
  });
  const picking = await run("picking", ["--script=crates/oc-app/scripts/picking.js"]);
  const k = picking.report ?? {};
  check("--script picking.js: every check passes", picking.code === 0 && k.ok === true, {
    checks: k.checks ? Object.keys(k.checks).filter((n) => n !== "hoverMs").length : undefined,
    failed: k.checks ? Object.entries(k.checks).filter(([n, v]) => v !== true && n !== "hoverMs").map(([n]) => n) : undefined,
    hoverMs: k.checks?.hoverMs, // move until settled, a timing rather than a check
    exception: k.exception,
  });
}

// 4. Uninstall. The NSIS uninstaller copies itself to %TEMP% and returns at once: wait for
// the folder to empty.
t = Date.now();
const un = join(dir, "uninstall.exe");
if (existsSync(un)) {
  execFileSync(un, ["/S"], { stdio: "inherit" });
  for (let i = 0; i < 120 && (existsSync(exe) || regHas()); i++) await sleep(500);
}
check("uninstall removes the program and its entry", !existsSync(exe) && !regHas(), {
  left: existsSync(dir) ? readdirSync(dir) : [],
  seconds: (Date.now() - t) / 1000,
});
rmSync(dir, { recursive: true, force: true });

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} checks passed${failed.length ? "; failed: " + failed.map((f) => f.name).join(", ") : ""}`);
process.exit(failed.length ? 1 : 0);
