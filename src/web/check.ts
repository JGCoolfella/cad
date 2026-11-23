/**
 * The checks of the browser's engine workers (OVERHAUL_PLAN W1b), run under Node by
 * `scripts/web-engine.mjs` (W1d runs the browser's): requests round trip through the engine
 * worker, the corpus regenerates there to the oracle through the unchanged
 * `NativeKernelClient`, the corpus assemblies solve as recorded, several parts regenerate
 * on geometry host workers, and a host worker that traps, aborts, exits or hangs fails only
 * its part. Reports in the shape of the desktop app's checks; `failures` lists what is wrong.
 */
import { NativeKernelClient } from "../native/engine";
import { useWebIpc } from "../native/tauri";
import { decodeBuild } from "../assembly/buildReply";
import type { BuildReply } from "../assembly/types";
import type { Feature } from "../core/types";
import { WebEngine, type WebEngineOptions } from "./index";

export interface CorpusPart {
  name: string;
  /** The `.ocpart` file's text. */
  doc: string;
  /** `corpus/<name>.json`. */
  oracle: { steps: { body?: OracleBody }[] };
}

export interface CheckInput {
  moduleUrl: string;
  parts: CorpusPart[];
  /** The corpus assemblies (`corpus/assemblies/<name>.ocasm`), with their recorded state, and the files of that folder. */
  assemblies: { name: string; record: AssemblyRecord }[];
  assemblyFiles: { name: string; bytes: Uint8Array }[];
  /** Geometry host workers for the parallel checks. */
  hosts: number;
  /** Rounds of the round-trip measurements. */
  rounds?: number;
}

interface AssemblyRecord {
  dof: number;
  instances: Record<string, { state: string; free: number; position: number[] }>;
}

interface OracleBody {
  volume: number;
  area: number;
  faces: number;
  edges: number;
}

const DIED = "The geometry kernel stopped while rebuilding this part.";

function stats(xs: number[]) {
  const s = [...xs].sort((a, b) => a - b);
  const q = (p: number) => s[Math.min(s.length - 1, Math.floor(p * s.length))];
  return { n: s.length, median: +q(0.5).toFixed(3), p95: +q(0.95).toFixed(3), min: +s[0].toFixed(3), mean: +(s.reduce((a, b) => a + b, 0) / s.length).toFixed(3) };
}

/** The final body the oracle expects (as `check_corpus` in `crates/oc-app/src/smoke.rs`,
 * with the correction recorded in section 8, E5) and the slack its rounding allows. */
function expected(name: string, oracle: CorpusPart["oracle"]) {
  const last = [...oracle.steps].reverse().find((s) => s.body)!.body!;
  const round = (x: number) => (x === 0 ? 1e-9 : 0.5 * 10 ** (Math.floor(Math.log10(Math.abs(x))) - 5) * (1 + 1e-9));
  const w: OracleBody & { slack: { volume: number; area: number } } = { ...last, slack: { volume: round(last.volume), area: round(last.area) } };
  if (name === "through-mid-dir2") {
    w.volume -= 100 * 114.288;
    w.area -= 4 * 10 * 114.288;
    w.faces -= 5;
    w.edges -= 12;
    w.slack.volume += 100 * 0.0005;
    w.slack.area += 40 * 0.0005;
  }
  return w;
}

const enc = new TextEncoder();

/** Corpus files into the engine's `/work/corpus` (W1e mirrors the user's workspace there). */
async function files(e: WebEngine, input: CheckInput) {
  for (const p of input.parts) await e.writeFile(`/work/corpus/${p.name}.ocpart`, enc.encode(p.doc));
  for (const f of input.assemblyFiles) await e.writeFile(`/work/corpus/assemblies/${f.name}`, f.bytes);
}

/** An assembly of `names` (parts in `/work/<dir>`), side by side, fixed, no mates. */
function sideBySide(name: string, names: string[]) {
  return {
    format: "opencad-assembly",
    version: 2,
    name,
    units: "MMGS",
    components: names.map((n, i) => ({ id: `c${i}`, kind: "part", path: `${n}.ocpart`, config: "Default" })),
    instances: names.map((n, i) => ({ id: `i${i}`, component: `c${i}`, name: `${n}<1>`, fixed: true, transform: { position: [i * 150, 0, 0], rotation: [1, 0, 0, 0] } })),
    mates: [],
  };
}

async function build(e: WebEngine, doc: unknown, path: string) {
  const t0 = performance.now();
  const buf = (await e.request("asm_build", { doc, path, lightweight: false, quality: 3, overrides: {}, probe: false, base: null })) as ArrayBuffer;
  const ms = performance.now() - t0;
  const head = decodeBuild(buf, null) as BuildReply & { regenerated: number; workers: number; regenMs: number; hosts?: Record<string, number> };
  return { head, ms, stats: e.lastStats };
}

export async function run(input: CheckInput) {
  const failures: string[] = [];
  const report: Record<string, unknown> = { failures };
  const engine = (o: Partial<WebEngineOptions> = {}) => new WebEngine({ moduleUrl: input.moduleUrl, hosts: input.hosts, ...o });

  // ---- start-up and request round trips (no host workers involved)
  const t0 = performance.now();
  const e = engine();
  const ready = await e.ready();
  report.identity = ready.identity;
  report.start = { moduleMs: +ready.startMs.toFixed(1), firstReplyMs: 0 };
  await e.request("engine_mass_props", { features: [] });
  (report.start as { firstReplyMs: number }).firstReplyMs = +(performance.now() - t0).toFixed(1);
  await files(e, input);
  const rounds = input.rounds ?? 200;
  const empty: number[] = [];
  for (let i = 0; i < rounds; i++) {
    const t = performance.now();
    await e.request("engine_mass_props", { features: [] });
    empty.push(performance.now() - t);
  }

  // ---- the corpus through the unchanged client, as the app's `--corpus` check
  useWebIpc(e);
  const client = new NativeKernelClient();
  const parts: Record<string, unknown>[] = [];
  let corpusMs = 0;
  for (const p of input.parts) {
    const features = (JSON.parse(p.doc) as { features: Feature[] }).features;
    const t = performance.now();
    const r = await client.rebuild(features);
    const ms = performance.now() - t;
    corpusMs += ms;
    const mp = await client.massProps(features);
    const got = { name: p.name, faces: r.mesh?.faces.length ?? 0, edges: r.mesh?.edges.length ?? 0, volume: mp?.volume ?? null, area: mp?.area ?? null, statuses: r.statuses.filter((s) => s.error || s.warning), ms: +ms.toFixed(1), engineMs: +r.timeMs.toFixed(1) };
    parts.push(got);
    const w = expected(p.name, p.oracle);
    for (const k of ["faces", "edges"] as const) if (got[k] !== w[k]) failures.push(`${p.name}: ${k} ${got[k]} vs oracle ${w[k]}`);
    for (const k of ["volume", "area"] as const) if (!(Math.abs((got[k] ?? NaN) - w[k]) <= w.slack[k])) failures.push(`${p.name}: ${k} ${got[k]} vs oracle ${w[k]}`);
    if (got.statuses.length) failures.push(`${p.name}: statuses ${JSON.stringify(got.statuses)}`);
  }
  // A rebuild served from the regeneration cache: the round trip with a mesh reply.
  const cached: number[] = [];
  const block = (JSON.parse(input.parts.find((p) => p.name === "block-hole")!.doc) as { features: Feature[] }).features;
  let replyBytes = 0;
  for (let i = 0; i < Math.min(rounds, 100); i++) {
    const t = performance.now();
    const buf = (await e.request("engine_rebuild", { features: block, preview: null, quality: 3, config: null })) as ArrayBuffer;
    cached.push(performance.now() - t);
    replyBytes = buf.byteLength;
  }
  report.roundTrip = { empty: stats(empty), cachedRebuild: { ...stats(cached), replyBytes } };
  report.corpus = { parts, totalMs: +corpusMs.toFixed(1) };
  useWebIpc(undefined);

  // ---- the corpus assemblies (their caches: no regeneration), against their records
  const asm: Record<string, unknown> = {};
  for (const a of input.assemblies) {
    const path = `/work/corpus/assemblies/${a.name}.ocasm`;
    const open = (await e.request("asm_open", { path })) as { doc: unknown };
    const { head, ms } = await build(e, open.doc, path);
    asm[a.name] = { dof: head.dof, regenerated: head.regenerated, ms: +ms.toFixed(1) };
    if (head.dof !== a.record.dof) failures.push(`${a.name}: dof ${head.dof} vs ${a.record.dof}`);
    for (const i of head.instances) {
      const want = a.record.instances[i.id];
      const st = i.state.charAt(0).toUpperCase() + i.state.slice(1);
      if (!want || want.state !== st || want.free !== i.free || i.transform.position.some((x, k) => Math.abs(x - want.position[k]) > 1e-5)) {
        failures.push(`${a.name}/${i.id}: ${st} ${i.free} ${i.transform.position} vs ${JSON.stringify(want)}`);
      }
    }
  }
  report.assemblies = asm;
  e.stop();

  // ---- every corpus part at once: on host workers, and in the engine alone
  const names = input.parts.map((p) => p.name);
  const all = sideBySide("all", names);
  const runAll = async (hosts: number) => {
    const x = engine({ hosts });
    await x.ready();
    await files(x, input);
    const r = await build(x, all, "/work/corpus/all.ocasm");
    const errors = Object.values(r.head.parts).filter((p) => p.errors.length || !p.handle);
    if (r.head.regenerated !== names.length) failures.push(`all (${hosts} hosts): ${r.head.regenerated} parts regenerated of ${names.length}`);
    if (errors.length) failures.push(`all (${hosts} hosts): ${errors.length} parts with errors or no mesh: ${JSON.stringify(errors.map((p) => [p.name, p.errors]))}`);
    if (hosts > 0 && r.head.workers !== Math.min(hosts, names.length)) failures.push(`all: ${r.head.workers} workers, ${hosts} hosts`);
    const bboxes = Object.fromEntries(Object.values(r.head.parts).map((p) => [p.name, p.bbox]));
    // Again with the workers (and the engine's compiled code) warm: the cache cleared, every
    // part regenerated as after an edit of all of them.
    const warm: number[] = [];
    for (let i = 0; i < 3; i++) {
      await x.request("engine_clear_cache");
      const w = await build(x, all, "/work/corpus/all.ocasm");
      if (w.head.regenerated !== names.length) failures.push(`all again (${hosts} hosts): ${w.head.regenerated} regenerated`);
      warm.push(w.ms);
    }
    x.stop();
    return { firstMs: +r.ms.toFixed(1), warmMs: warm.map((m) => +m.toFixed(1)), regenMs: +r.head.regenMs.toFixed(1), workers: r.head.workers, hosts: r.head.hosts ?? null, request: r.stats, bboxes };
  };
  const alone = await runAll(0);
  const hosted = await runAll(input.hosts);
  if (JSON.stringify(alone.bboxes) !== JSON.stringify(hosted.bboxes)) failures.push("all: parts differ between host workers and the engine alone");
  report.parallel = { parts: names.length, engineAlone: { ...alone, bboxes: undefined }, hostWorkers: { ...hosted, bboxes: undefined } };

  // ---- host workers that trap, abort, exit or hang: only their parts fail
  const f = engine({ hosts: 4, faults: true, hostTimeoutMs: 5000 });
  await f.ready();
  const base = JSON.parse(input.parts.find((p) => p.name === "block-hole")!.doc);
  const kinds = ["crash", "abort", "exit", "hang"];
  for (const k of [...kinds, "ok"]) await f.writeFile(`/work/faults/${k}.ocpart`, enc.encode(JSON.stringify({ ...base, name: k === "ok" ? "ok" : `fault:${k}` })));
  const fr = await build(f, sideBySide("faults", [...kinds, "ok"]), "/work/faults/faults.ocasm");
  const byName = Object.fromEntries(Object.values(fr.head.parts).map((p) => [p.name, p]));
  const msg = (n: string) => byName[n]?.errors.map((x) => x.message).join("; ") ?? "missing";
  for (const k of ["crash", "abort", "exit"]) if (!msg(`fault:${k}`).startsWith(DIED)) failures.push(`fault:${k}: ${msg(`fault:${k}`)}`);
  if (!/did not finish within \d+ s/.test(msg("fault:hang"))) failures.push(`fault:hang: ${msg("fault:hang")}`);
  if (msg("ok") !== "" || !byName.ok?.handle) failures.push(`ok beside the faults: ${msg("ok")}`);
  const h = fr.head.hosts ?? {};
  if (h.died !== 3 || h.timedOut !== 1) failures.push(`fault counts ${JSON.stringify(h)}`);
  // The workers start again: two more parts regenerate on them.
  await files(f, input);
  const again = await build(f, sideBySide("again", ["block-hole", "holes"]), "/work/corpus/again.ocasm");
  if (again.head.regenerated !== 2 || Object.values(again.head.parts).some((p) => p.errors.length || !p.handle)) failures.push(`after the faults: ${JSON.stringify(again.head.parts)}`);
  if ((again.head.hosts?.started ?? 0) <= (h.started ?? 0)) failures.push(`no host worker started again: ${JSON.stringify(again.head.hosts)}`);
  report.faults = { ms: +fr.ms.toFixed(1), hosts: h, errors: Object.fromEntries(Object.keys(byName).map((n) => [n, msg(n)])), after: { regenerated: again.head.regenerated, hosts: again.head.hosts } };
  f.stop();
  return report;
}
