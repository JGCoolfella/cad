// `opencad --script=crates/oc-app/scripts/lod.js` (npm run native:lod): level of detail while
// the view moves (OVERHAUL_PLAN G2b), driven by DOM pointer and wheel events on the graphics
// area. The body of an async function: `a` is window.opencad, `cwd` the checkout, `wait` and
// `settle` come from smoke.rs. Throws (exit code 1) when a check fails.
//
// `view_stats` reports what the render thread drew last: the frame's number, whether it said
// the view was moving, its Level of detail slider and the part instances per level.
//
// 1. corpus/assemblies/hinge.ocasm: at rest every instance at level 0; a middle-button turn
//    draws coarser levels in every frame, also while the button is held still; released, the
//    next frame is full detail. A pick during the turn finds the component under the pointer.
// 2. A wheel step: coarse, then full detail once the zoom settles. A view transition: coarse
//    frames on the way, full detail at the end.
// 3. Options > Performance > Level of detail: more of the slider coarsens more; Off draws the
//    finest level while moving.
// 4. Parts: a multibody part coarsens while turning; a single-body part does not (SOLIDWORKS
//    applies the option to assemblies and multibody parts), with the slider at 20.
const A = window.opencadAsm;
const inv = window.__TAURI_INTERNALS__.invoke;
const v = window.opencadView;
const V3 = v.cam.target.constructor;
const fails = [];
const out = {};
const check = (name, ok, detail) => {
  out[name] = ok ? true : detail ?? false;
  if (!ok) fails.push({ name, detail });
};
const ev = (type, [sx, sy], o = {}) => {
  const r = v.overlay.getBoundingClientRect();
  v.overlay.dispatchEvent(new PointerEvent(type, { clientX: r.left + sx, clientY: r.top + sy, bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", isPrimary: true, button: o.button ?? 0, buttons: o.buttons ?? 0 }));
};
const S = (p) => v.cam.project(new V3(...p));
const W = v.overlay.clientWidth;
const H = v.overlay.clientHeight;
const C = [W / 2, H / 2];

// What the render thread drew for the newest frame sent (or a newer one).
await inv("view_stats"); // turns the counting on
const drawn = async () => {
  await wait(40);
  const want = v.native.seq;
  for (let i = 0; i < 150; i++) {
    const s = await inv("view_stats");
    if (s && s.seq >= want) return s;
    await wait(20);
  }
  return null;
};
const total = (s) => (s?.levels ?? []).reduce((t, n) => t + n, 0);
const coarse = (s) => total(s) - (s?.levels?.[0] ?? 0);
const mean = (s) => (s?.levels ?? []).reduce((t, n, l) => t + n * l, 0) / Math.max(1, total(s));
const full = (s) => !!s && !s.moving && total(s) > 0 && coarse(s) === 0;
const brief = (s) => s && { seq: s.seq, moving: s.moving, lod: s.lod, levels: s.levels };
const atRest = async () => {
  v.invalidate();
  return drawn();
};
// A middle-button turn of the view: the frames drawn after each move, then (held) the pointer
// rests, then the release and the frame after it.
const turn = async (steps = 8, hold = 0, during = null) => {
  ev("pointerdown", C, { button: 1, buttons: 4 });
  await v.settled();
  const frames = [];
  let p = C;
  for (let k = 1; k <= steps; k++) {
    p = [C[0] + 7 * k, C[1] + 3 * k];
    ev("pointermove", p, { buttons: 4 });
    await v.settled();
    frames.push(await drawn());
  }
  const extra = during ? await during() : null;
  if (hold) await wait(hold);
  const held = await inv("view_stats");
  const t = performance.now();
  ev("pointerup", p, { button: 1 });
  await v.settled();
  const after = await drawn();
  return { frames, held, after, extra, releaseMs: Math.round(performance.now() - t) };
};
const setLod = (n) => a.setOpt("performance.lod", n);

try {
  // ------------------------------------------------------------ 1. an assembly, turned
  setLod(10);
  a.openAssembly(await A.openAssembly(a, `${cwd}/corpus/assemblies/hinge.ocasm`));
  for (let i = 0; i < 400 && (!a.asm || a.asm.building || !a.asm.built); i++) await wait(50);
  a.viewHooks.orient("isometric");
  await wait(900);
  const rest = await atRest();
  check("asm at rest: every instance at level 0", full(rest) && total(rest) === 3, brief(rest));
  const b = a.asm.built;
  const pinOcc = b.occurrences.findIndex((o) => o.path[0] === "i3");
  // During the turn, the pin's head (top at z = 17 over the hole) is picked where it is drawn.
  const pickPin = async () => {
    const p = S([15, 15, 17]);
    const h = await v.native.pick(v.cam, v.host, { x: p[0], y: p[1], filter: { only: null, edgesOnly: false }, wait: true, ray: "none", selector: false });
    return { item: h.item, stats: await inv("view_stats") };
  };
  const t10 = await turn(8, 600, pickPin);
  out.asmTurn = t10.frames.map(brief);
  check("asm turn: every frame moving at the slider's level", t10.frames.every((s) => s?.moving && s.lod === 10 && total(s) === 3), out.asmTurn);
  check("asm turn: coarser levels drawn", t10.frames.every((s) => coarse(s) > 0), out.asmTurn);
  check("asm turn: held still, the view stays coarse", !!t10.held?.moving && coarse(t10.held) > 0, brief(t10.held));
  check("asm turn: a pick finds the pin as drawn", t10.extra?.item?.kind === "face" && t10.extra.item.body === pinOcc && !!t10.extra.stats?.moving, t10.extra);
  check("asm released: the next frame is full detail", full(t10.after), brief(t10.after));
  out.releaseMs = t10.releaseMs;

  // ------------------------------------------------------------ 2. wheel and transitions
  const r = v.overlay.getBoundingClientRect();
  v.overlay.dispatchEvent(new WheelEvent("wheel", { clientX: r.left + C[0], clientY: r.top + C[1], deltaY: 100, bubbles: true, cancelable: true }));
  const zoomed = await drawn();
  check("wheel: coarse while zooming", !!zoomed?.moving && coarse(zoomed) > 0, brief(zoomed));
  await wait(400);
  const settled = await inv("view_stats");
  check("wheel: full detail once the zoom settles", full(settled) && settled.seq > zoomed.seq, brief(settled));
  a.viewHooks.orient("front");
  const way = [];
  for (let i = 0; i < 40; i++) {
    await wait(25);
    way.push(await inv("view_stats"));
  }
  await wait(300);
  const end = await atRest();
  check("transition: coarse on the way", way.some((s) => s?.moving && coarse(s) > 0), way.map(brief).slice(0, 6));
  check("transition: full detail at the end", full(end), brief(end));

  // ------------------------------------------------------------ 3. the slider
  // The hinge's parts have three levels; at a whole-assembly view slider 10 already draws
  // the coarsest, so the slider is compared zoomed in too. More of the slider is never finer,
  // and at some zoom strictly coarser.
  const means = {};
  for (const z of [1, 4, 16]) {
    a.viewHooks.orient("isometric");
    await wait(900);
    if (z > 1) {
      v.cam.zoomAt(z, C[0], C[1]);
      v.invalidate();
      await wait(400);
    }
    for (const n of [1, 10, 20]) {
      setLod(n);
      const t = await turn();
      means[`${z}x`] = { ...means[`${z}x`], [n]: +mean(t.frames.at(-1)).toFixed(3) };
      if (!full(t.after)) check(`slider ${n} at ${z}x: full detail after the turn`, false, brief(t.after));
    }
  }
  out.meanLevel = means;
  const m = Object.values(means);
  check("slider: more is never finer", m.every((x) => x[1] <= x[10] && x[10] <= x[20]), means);
  check("slider: 10 coarser than 1, 20 coarser than 10 (some zoom)", m.some((x) => x[1] < x[10]) && m.some((x) => x[10] < x[20]), means);
  a.viewHooks.orient("isometric");
  await wait(900);
  setLod(0);
  const t0 = await turn();
  check("slider Off: the finest level while moving", t0.frames.every((s) => s?.moving && s.lod === 0 && coarse(s) === 0 && total(s) === 3), t0.frames.map(brief));
  check("slider Off: full detail after", full(t0.after), brief(t0.after));
  setLod(10);

  // ------------------------------------------------------------ 4. parts
  const disc = (id, x, r) => ({
    id,
    name: `Sketch-${id}`,
    type: "sketch",
    plane: { kind: "std", plane: "front" },
    sketch: { points: { origin: { id: "origin", x: 0, y: 0 }, [`${id}_c`]: { id: `${id}_c`, x, y: 0 } }, entities: { [`${id}_k`]: { id: `${id}_k`, type: "circle", c: `${id}_c`, r } }, relations: {}, dimensions: {}, nextDim: 1 },
  });
  const boss = (id, sketchId, merge) => ({ type: "extrude", cut: false, sketchId, reverse: false, merge, end1: { type: "blind", depth: 10 }, id, name: `Boss-${id}` });
  const part = (name, features) => JSON.stringify({ format: "opencad-part", version: 1, name, units: "MMGS", features, rollback: null });
  const loaded = async (text) => {
    a.loadDocument(text);
    await wait(100);
    for (let i = 0; i < 200 && (a.rebuilding || !a.result?.mesh); i++) await wait(50);
    a.viewHooks.orient("isometric");
    await wait(900);
  };
  await loaded(part("lod-multibody", [disc("f1", 0, 20), boss("f2", "f1", true), disc("f3", 60, 15), boss("f4", "f3", false)]));
  check("multibody part: two solids", a.result?.mesh?.solids === 2, a.result?.mesh?.solids);
  // Filling the view, its level 1 is about as far off as slider 10 allows; 20 coarsens it.
  const tm10 = await turn();
  check("multibody part: the option applies", tm10.frames.every((s) => s?.moving && s.lod === 10 && total(s) === 1), tm10.frames.map(brief));
  setLod(20);
  const tm = await turn();
  check("multibody part: coarser while turning (slider 20)", tm.frames.every((s) => s?.moving && s.lod === 20 && coarse(s) === 1), tm.frames.map(brief));
  check("multibody part: full detail after", full(tm.after) && full(tm10.after), [brief(tm10.after), brief(tm.after)]);
  await loaded(part("lod-single", [disc("f1", 0, 20), boss("f2", "f1", true)]));
  check("single-body part: one solid", a.result?.mesh?.solids === 1, a.result?.mesh?.solids);
  const ts = await turn();
  check("single-body part: full detail while turning (slider 20, not applicable)", ts.frames.every((s) => s?.moving && s.lod === 0 && coarse(s) === 0 && total(s) === 1), ts.frames.map(brief));
} catch (e) {
  check("no exception", false, String(e.stack));
}

if (fails.length) throw new Error("lod: " + JSON.stringify(fails));
return { ok: true, checks: out, gpu: await a.kernel.adapter?.() };
