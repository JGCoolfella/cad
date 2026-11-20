// `opencad --script=crates/oc-app/scripts/scale.js` (npm run native:scale): the generated
// 10 000-component assembly (OVERHAUL_PLAN G3a; G2's benchmark as a file, 100 distinct parts),
// opened, rebuilt and turned in the app, end to end. The body of an async function: `a` is
// window.opencad, `cwd` the checkout, `wait` comes from smoke.rs. `npm run native:scale` first
// writes the assembly with `cargo run -p oc-asm --example scale -- target/scale` (no caches, so
// the first open regenerates every part). Throws (exit code 1) when a check fails.
//
// Reports (all times in ms, from the webview unless named):
// - open: `asm_open` until the build reply, the engine's build time and what it regenerated,
//   and until the first frame showing every instance;
// - rebuilds: nothing changed; one part edited; ten parts edited (component edits held as
//   unsaved overrides, as Open Part does);
// - frames: a middle-button turn, per frame from the pointer event to the render thread having
//   drawn it, and the render thread's own time per frame; at slider 10 (coarser while moving)
//   and Off (the finest level);
// - save with caches, then a lightweight open of the saved file (the engine's parts forgotten
//   first) and Mass Properties, which reads every part from its B-rep cache (G3c);
// - the process' memory and thread count, and the geometry hosts' (G3b; `OC_GEOMETRY_HOSTS=0`
//   runs the parts on threads in the app instead).
const A = window.opencadAsm;
const inv = window.__TAURI_INTERNALS__.invoke;
const v = window.opencadView;
const fails = [];
const out = {};
const check = (name, ok, detail) => {
  if (!ok) fails.push({ name, detail });
};
const ev = (type, [sx, sy], o = {}) => {
  const r = v.overlay.getBoundingClientRect();
  v.overlay.dispatchEvent(new PointerEvent(type, { clientX: r.left + sx, clientY: r.top + sy, bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", isPrimary: true, button: o.button ?? 0, buttons: o.buttons ?? 0 }));
};
const W = v.overlay.clientWidth;
const H = v.overlay.clientHeight;
const C = [W / 2, H / 2];
const ms = (t) => Math.round((performance.now() - t) * 10) / 10;
const stat = (xs) => {
  const s = [...xs].sort((p, q) => p - q);
  const q = (f) => s[Math.min(s.length - 1, Math.floor(f * s.length))];
  return { n: s.length, mean: +(s.reduce((t, x) => t + x, 0) / Math.max(1, s.length)).toFixed(2), median: +q(0.5).toFixed(2), p95: +q(0.95).toFixed(2), max: +s.at(-1).toFixed(2) };
};
const built = async () => {
  for (let i = 0; i < 2400 && (!a.asm || a.asm.building || !a.asm.built); i++) await wait(25);
  return a.asm?.built;
};
// The render thread has drawn the newest frame sent: its stats.
const drawn = async () => {
  const want = v.native.seq;
  for (let i = 0; i < 4000; i++) {
    const s = await inv("view_stats");
    if (s && s.seq >= want) return s;
    await wait(2);
  }
  return null;
};
const total = (s) => (s?.levels ?? []).reduce((t, n) => t + n, 0);
const memory = () => inv("app_process_stats").catch(() => null);
const dir = `${cwd}/target/scale`;
const file = `${dir}/scale-10000.ocasm`;
const saved = `${dir}/scale-10000-saved.ocasm`;

// Opens `path` and waits for the build and for the first frame drawing every instance.
const open = async (path) => {
  const t = performance.now();
  const s = await A.openAssembly(a, path);
  const readMs = ms(t); // asm_open: the file read and its document sent to the webview
  a.openAssembly(s);
  const b = await built();
  const replied = ms(t);
  let first = null;
  for (let i = 0; i < 2000; i++) {
    const s = await drawn();
    if (total(s) === 10000 || s?.instances === 10000) {
      first = s;
      break;
    }
    await wait(5);
  }
  return { readMs, replyMs: replied, engineMs: Math.round(b?.timeMs ?? -1), regenerated: b?.regenerated, workers: b?.workers, regenMs: b?.regenMs && Math.round(b.regenMs), firstFrameMs: first ? ms(t) : null, occurrences: b?.occurrences.length, hosts: b?.hosts };
};

// A middle-button turn of `steps` moves: per frame, the pointer event until it is drawn.
const turn = async (steps) => {
  ev("pointerdown", C, { button: 1, buttons: 4 });
  await v.settled();
  const latency = [];
  const render = [];
  const t = performance.now();
  for (let k = 1; k <= steps; k++) {
    const t1 = performance.now();
    ev("pointermove", [C[0] + 4 * k, C[1] + 1.5 * k], { buttons: 4 });
    await v.settled();
    const s = await drawn();
    latency.push(performance.now() - t1);
    if (s?.renderMs != null) render.push(s.renderMs);
  }
  const wall = performance.now() - t;
  ev("pointerup", [C[0] + 4 * steps, C[1] + 1.5 * steps], { button: 1 });
  await v.settled();
  const after = await drawn();
  return { latency: stat(latency), renderThread: render.length ? stat(render) : null, fps: +((1000 * steps) / wall).toFixed(1), after: after && { levels: after.levels } };
};

const edited = async (k) => {
  const p = `${dir}/scale-p${String(k).padStart(3, "0")}.ocpart`;
  const doc = JSON.parse(await inv("app_read_text", { path: p }));
  doc.features[1].end1.depth += 0.5;
  return [p, JSON.stringify(doc)];
};

try {
  await inv("view_stats"); // turns the counting on
  for (let i = 0; i < 60 && !a.kernelReady; i++) await wait(250);
  out.cpus = navigator.hardwareConcurrency;
  out.memoryBefore = await memory();
  a.setOpt("performance.lod", 10);

  // ---------------------------------------------------------------- 1. cold open
  out.open = await open(file);
  check("open: 10 000 occurrences", out.open.occurrences === 10000, out.open);
  check("open: a frame shows every instance", out.open.firstFrameMs != null, out.open);
  const b = a.asm.built;
  check("open: no component errors", b.components.every((c) => !c.error), b.components.filter((c) => c.error).slice(0, 3));
  check("open: 100 distinct meshes", new Set(b.occurrences.map((o) => o.mesh)).size === 100, new Set(b.occurrences.map((o) => o.mesh)).size);
  out.memoryOpen = await memory();

  // ---------------------------------------------------------------- 2. rebuilds
  const S = a.asm;
  // `replyMs`: the session's rebuild as the UI runs it (request, engine, reply handled and the
  // tree updated); `ipcMs`: the same request alone, as a probe (the document sent and the
  // reply parsed, nothing shown), so the rest is the webview's handling.
  const rebuild = async () => {
    const t = performance.now();
    const r = await S.build();
    const replyMs = ms(t);
    const t2 = performance.now();
    const p = await inv("asm_build", { doc: S.doc, path: S.path, lightweight: S.lightweight, quality: S.quality, overrides: S.overrides, probe: true });
    return { replyMs, ipcMs: ms(t2), engineMs: Math.round(r?.timeMs ?? -1), probeEngineMs: Math.round(p?.timeMs ?? -1), regenerated: r?.regenerated, workers: r?.workers, regenMs: r?.regenMs && Math.round(r.regenMs), hosts: r?.hosts };
  };
  out.rebuild = await rebuild();
  const [p7, t7] = await edited(7);
  S.overrides[p7] = t7;
  out.rebuildOnePart = await rebuild();
  for (let k = 10; k < 20; k++) {
    const [p, t] = await edited(k);
    S.overrides[p] = t;
  }
  out.rebuildTenParts = await rebuild();
  for (const p of Object.keys(S.overrides)) delete S.overrides[p];
  out.rebuildBack = await rebuild();
  check("rebuilds: 10 000 occurrences", a.asm.built.occurrences.length === 10000);

  // ---------------------------------------------------------------- 3. frames
  a.viewHooks.orient("isometric");
  await wait(1500);
  await drawn();
  const rest = [];
  for (let i = 0; i < 30; i++) {
    const t = performance.now();
    v.invalidate();
    const s = await drawn();
    rest.push({ ms: performance.now() - t, render: s?.renderMs });
  }
  out.framesAtRest = { latency: stat(rest.map((x) => x.ms)), renderThread: rest[0].render != null ? stat(rest.map((x) => x.render)) : null };
  out.turnSlider10 = await turn(60);
  a.setOpt("performance.lod", 0);
  out.turnOff = await turn(60);
  a.setOpt("performance.lod", 10);
  check("frames: drawn", out.turnSlider10.latency.n === 60 && out.turnOff.latency.n === 60, out.turnSlider10);

  // ---------------------------------------------------------------- 4. save, lightweight open
  let t = performance.now();
  check("save", await S.save(saved));
  out.saveMs = ms(t);
  // As a fresh start would: the engine forgets the parts it built (they are kept by content).
  await inv("engine_clear_cache");
  out.light = await open(saved);
  check("lightweight open: nothing regenerated", out.light.regenerated === undefined || out.light.regenerated === 0, out.light);
  // Mass Properties resolves every part: read from the B-rep caches the save wrote (G3c).
  t = performance.now();
  const mass = await inv("asm_mass", { quality: a.asm.quality });
  out.massLightweight = { ms: ms(t), read: mass?.read, regenerated: mass?.regenerated };
  check("lightweight: parts read from their B-reps, none regenerated", mass?.read === 100 && mass?.regenerated === 0, out.massLightweight);
  check("lightweight open: 10 000 occurrences", out.light.occurrences === 10000, out.light);
  out.memoryEnd = await memory();
} catch (e) {
  check("no exception", false, String(e.stack));
}

if (fails.length) throw new Error("scale: " + JSON.stringify(fails) + " " + JSON.stringify(out));
return { ok: true, ...out, gpu: await a.kernel.adapter?.() };
