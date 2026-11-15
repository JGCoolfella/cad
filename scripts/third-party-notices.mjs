// Third-party notices for the native desktop app (OVERHAUL_PLAN F3, docs/THIRD_PARTY.md).
//
// Lists every Rust crate linked into `oc-app` (normal dependencies, resolved for the target
// platform, workspace crates excluded) and the JavaScript packages bundled into its UI, and
// appends each distinct licence text once with the components that use it. Texts come from
// the packages themselves (LICENSE*, LICENCE*, COPYING*, NOTICE*, UNLICENSE*); a package
// that ships none gets the standard text of its licence from crates/oc-app/licenses/.
//
// Usage: node scripts/third-party-notices.mjs [out.txt] [--target <triple>]
// As a module: thirdPartyNotices({ target, extra }) → { text, crates, missing }.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const LICENCE_FILE = /^(licen[cs]e|copying|notice|unlicense)/i;
/** JavaScript packages bundled into dist/ (the UI): the app's runtime imports and theirs. */
export const UI_PACKAGES = ["react", "react-dom", "scheduler", "three"];

const hostTarget = () => /host: (\S+)/.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))[1];

/** Licence-looking files at the top of a package directory. */
function licenceFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true })
    .filter((e) => e.isFile() && LICENCE_FILE.test(e.name))
    .map((e) => join(dir, e.name))
    .sort();
}

/** Standard texts for packages without their own: the first SPDX id of the expression we have. */
function standardText(expr) {
  const dir = join(root, "crates/oc-app/licenses");
  for (const id of String(expr ?? "").split(/\s+(?:OR|AND|WITH)\s+|[()/]/).map((s) => s.trim().replace(/\+$/, ""))) {
    const f = join(dir, `${id}.txt`);
    if (id && existsSync(f)) return f;
  }
  return null;
}

/** Normal-dependency closure of `oc-app` from `cargo metadata`, workspace crates excluded. */
export function rustCrates(target = hostTarget()) {
  const meta = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--format-version", "1", "--filter-platform", target, "--features", "oc-app/custom-protocol", "--manifest-path", join(root, "Cargo.toml")],
      { encoding: "utf8", maxBuffer: 256 << 20 },
    ),
  );
  const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const start = meta.packages.find((p) => p.name === "oc-app" && p.source == null);
  const seen = new Set([start.id]);
  const todo = [start.id];
  while (todo.length) {
    for (const d of nodes.get(todo.pop()).deps) {
      if (!d.dep_kinds.some((k) => k.kind == null) || seen.has(d.pkg)) continue;
      seen.add(d.pkg);
      todo.push(d.pkg);
    }
  }
  return [...seen]
    .map((id) => pkgs.get(id))
    .filter((p) => p.source != null)
    .map((p) => ({
      name: p.name,
      version: p.version,
      licence: p.license ?? (p.license_file ? `see ${p.license_file}` : "unknown"),
      url: `https://crates.io/crates/${p.name}/${p.version}`,
      authors: p.authors?.length ? p.authors.join(", ") : `the ${p.name} authors`,
      files: [...licenceFiles(dirname(p.manifest_path)), ...(p.license_file ? [join(dirname(p.manifest_path), p.license_file)] : [])],
    }))
    .sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
}

/** The UI's bundled npm packages, resolved from this checkout (or the one above a worktree). */
export function uiPackages() {
  const req = createRequire(join(root, "package.json"));
  return UI_PACKAGES.map((name) => {
    // The package root: up from its entry point (some packages don't export package.json).
    let dir = dirname(req.resolve(name));
    while (!(existsSync(join(dir, "package.json")) && JSON.parse(readFileSync(join(dir, "package.json"), "utf8")).name === name)) {
      if (dirname(dir) === dir) throw new Error(`no package root for ${name}`);
      dir = dirname(dir);
    }
    const pj = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    const authors = typeof pj.author === "string" ? pj.author : pj.author?.name ?? `the ${name} authors`;
    return { name, version: pj.version, licence: pj.license, authors, url: `https://www.npmjs.com/package/${name}/v/${pj.version}`, files: licenceFiles(dir) };
  });
}

/**
 * The notices text. `extra` are sections written by the caller (OCCT, the C++ runtime),
 * placed before the package lists.
 */
export function thirdPartyNotices({ target, extra = [], product = "OpenCAD", version = "" } = {}) {
  const crates = rustCrates(target);
  const ui = uiPackages();
  const texts = new Map(); // hash → { text, users }
  const missing = [];
  for (const p of [...crates, ...ui]) {
    let files = [...new Set(p.files)].filter((f) => existsSync(f));
    let standard = false;
    if (!files.length) {
      const std = standardText(p.licence);
      if (std) [files, standard] = [[std], true];
      else missing.push(`${p.name} ${p.version} (${p.licence})`);
    }
    for (const f of files) {
      let text = readFileSync(f, "utf8").replace(/\r\n/g, "\n").trim();
      // A standard text names the package's authors as the copyright holders.
      if (standard) text = text.replace("{copyright}", p.authors.replace(/\s*<[^>]*>/g, ""));
      const key = createHash("sha256").update(text.replace(/\s+/g, " ")).digest("hex");
      if (!texts.has(key)) texts.set(key, { text, users: [] });
      texts.get(key).users.push(`${p.name} ${p.version}`);
    }
  }
  const rule = "=".repeat(78);
  const list = (ps) => ps.map((p) => `- ${p.name} ${p.version}: ${p.licence} (${p.url})`).join("\n");
  const out = [
    `${product} ${version}: third-party notices`.trim(),
    rule,
    "",
    `${product} includes the third-party components below. Each is used under its own`,
    "licence; the licence texts follow the lists. Source code of every component is",
    "available from the address given with it.",
    "",
    ...extra.flatMap((s) => [s.trim(), ""]),
    rule,
    `Rust crates linked into the program (${crates.length})`,
    rule,
    "MPL-2.0 components (file-level copyleft) are used unmodified; their source is at the",
    "crates.io address listed.",
    "",
    list(crates),
    "",
    rule,
    `JavaScript packages in the user interface (${ui.length})`,
    rule,
    list(ui),
    "",
    rule,
    "Licence texts",
    rule,
    ...[...texts.values()].flatMap((t) => ["", `--- Used by: ${t.users.join(", ")}`, "", t.text, ""]),
  ];
  return { text: out.join("\n") + "\n", crates, ui, missing };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const t = args.indexOf("--target");
  const target = t >= 0 ? args.splice(t, 2)[1] : undefined;
  const { text, crates, ui, missing } = thirdPartyNotices({ target });
  if (args[0]) writeFileSync(args[0], text);
  else process.stdout.write(text);
  console.error(`${crates.length} crates, ${ui.length} UI packages, ${missing.length} without a licence text${missing.length ? ": " + missing.join(", ") : ""}`);
  if (missing.length) process.exitCode = 1;
}
