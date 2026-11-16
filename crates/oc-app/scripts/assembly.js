// `opencad --script=crates/oc-app/scripts/assembly.js` (npm run native:assembly): assemblies
// in the native app (OVERHAUL_PLAN G1a). The body of an async function: `a` is
// window.opencad, `cwd` the working folder (a checkout), `wait` and `settle` come from
// smoke.rs. Throws (exit code 1) when a check fails.
//
// 1. Open corpus/assemblies/hinge.ocasm (lightweight, from its cached meshes): instances,
//    states, mates, degrees of freedom; pick a face on an instance; drag the arm about its
//    pin with pointer events; undo.
// 2. Open bracket-pins.ocasm: two pins share one mesh.
// 3. Build an assembly from the corpus parts: Insert Components, two mates through the Mate
//    PropertyManager (picked by face), a conflicting third one, undo; Save As; reopen.
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
const near = (p, q, tol = 1e-6) => !!p && !!q && Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]) < tol;
const built = async () => {
  for (let i = 0; i < 400 && (!a.asm || a.asm.building || !a.asm.built); i++) await wait(50);
  await wait(200);
  return a.asm.built;
};
const qrot = (q, p) => {
  const [w, x, y, z] = q;
  const c1 = [y * p[2] - z * p[1], z * p[0] - x * p[2], x * p[1] - y * p[0]];
  const c2 = [y * c1[2] - z * c1[1], z * c1[0] - x * c1[2], x * c1[1] - y * c1[0]];
  return [p[0] + 2 * (w * c1[0] + c2[0]), p[1] + 2 * (w * c1[1] + c2[1]), p[2] + 2 * (w * c1[2] + c2[2])];
};
const place = (t, p) => qrot(t.rotation, p).map((c, k) => c + t.position[k]);
const S = (p) => v.cam.project(new V3(...p));
const ev = (type, [sx, sy], o = {}) => {
  const r = v.overlay.getBoundingClientRect();
  v.overlay.dispatchEvent(new PointerEvent(type, { clientX: r.left + sx, clientY: r.top + sy, bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", isPrimary: true, button: o.button ?? 0, buttons: o.buttons ?? 0 }));
};
const t0 = performance.now();

// ---------------------------------------------------------------- 1. the hinge
a.openAssembly(await A.openAssembly(a, `${cwd}/corpus/assemblies/hinge.ocasm`));
let b = await built();
const inst = (id) => a.asm.built.instances.find((i) => i.id === id);
out.openMs = Math.round(performance.now() - t0);
check("hinge: three instances drawn", b.instances.length === 3 && b.occurrences.length === 3 && b.occurrences.every((o) => o.mesh > 0), b.occurrences);
check("hinge: lightweight from the file's caches", Object.values(b.parts).every((p) => p.lightweight), b.parts);
check("hinge: mates solved", b.mates.length === 4 && b.mates.every((m) => m.status === "ok"), b.mates);
check("hinge: 2 degrees of freedom", b.dof === 2, b.dof);
check("hinge: plate fixed (f)", inst("i1").state === "fixed");
check("hinge: arm (-), turns about its pin", inst("i2").state === "under" && inst("i2").free === 1, inst("i2"));
check("hinge: pin on the arm over the hole", near(inst("i3").transform.position, [15, 15, 14]), inst("i3").transform);
check("hinge: tree shows (f) and (-)", !!document.querySelector('[data-asm-row="i1"]')?.textContent.includes("(f) plate<1>") && !!document.querySelector('[data-asm-row="i2"]')?.textContent.includes("(-) arm<1>"));
check("hinge: not dirty after opening", !a.dirty);
a.viewHooks.orient("isometric");
await wait(900);

// Hover the pin's head (its top at z = 17 over the hole) and click it: a face of that occurrence.
const pinOcc = b.occurrences.findIndex((o) => o.path[0] === "i3");
const head = S([15, 15, 17]);
ev("pointermove", head);
await v.settled();
await wait(100);
ev("pointermove", head);
await v.settled();
check("pick: hover a face of the pin instance", a.hover?.kind === "face" && a.hover.body === pinOcc, a.hover);
ev("pointerdown", head, { buttons: 1 });
ev("pointerup", head);
await v.settled();
check("pick: click selects it", a.selection.length === 1 && a.selection[0].kind === "face" && a.selection[0].body === pinOcc, a.selection);
a.clearSelection();

// Drag the arm by its far end (local (45, 5, 6), 40 mm from its hole) a quarter turn round.
const axis = [15, 15];
const arm0 = inst("i2").transform;
const grab = place(arm0, [45, 5, 6]);
const ang0 = Math.atan2(grab[1] - axis[1], grab[0] - axis[0]);
const steps = 12;
ev("pointerdown", S(grab), { buttons: 1 });
await v.settled();
for (let k = 1; k <= steps; k++) {
  const ang = ang0 + (Math.PI / 2) * (k / steps);
  ev("pointermove", S([axis[0] + 40 * Math.cos(ang), axis[1] + 40 * Math.sin(ang), 14]), { buttons: 1 });
  await v.settled();
  await wait(40);
}
await wait(300);
ev("pointerup", S([axis[0] + 40 * Math.cos(ang0 + Math.PI / 2), axis[1] + 40 * Math.sin(ang0 + Math.PI / 2), 14]));
await v.settled();
await wait(200);
const arm1 = inst("i2").transform;
const end = place(arm1, [45, 5, 6]);
const turned = Math.atan2(end[1] - axis[1], end[0] - axis[0]) - ang0;
out.dragTurnDeg = Math.round(((((turned * 180) / Math.PI) % 360) + 360) % 360 * 10) / 10;
// The pointer moves the grabbed point in the plane through it facing the viewer; the arm
// can only turn on the plate, so it ends at the angle of that target seen from the axis.
const last = S([axis[0] + 40 * Math.cos(ang0 + Math.PI / 2), axis[1] + 40 * Math.sin(ang0 + Math.PI / 2), 14]);
const vd = v.cam.viewDir();
const ray = v.cam.ray(last[0], last[1]);
const g = new V3(...grab);
const tHit = vd.dot(g.clone().sub(ray.origin)) / vd.dot(ray.direction);
const target = ray.origin.clone().add(ray.direction.clone().multiplyScalar(tHit));
const want = Math.atan2(target.y - axis[1], target.x - axis[0]) - ang0;
out.dragWantDeg = Math.round(((((want * 180) / Math.PI) % 360) + 360) % 360 * 10) / 10;
check("drag: the arm turned about the pin towards the pointer", Math.abs(out.dragTurnDeg - out.dragWantDeg) < 0.5 && out.dragTurnDeg > 45, [out.dragTurnDeg, out.dragWantDeg]);
check("drag: its end stays 40 mm from the axis, on the plate", Math.abs(Math.hypot(end[0] - axis[0], end[1] - axis[1]) - 40) < 1e-4 && Math.abs(end[2] - 14) < 1e-6, end);
check("drag: the pin went along", near(inst("i3").transform.position, [15, 15, 14]));
check("drag: written to the document, one undo step", a.dirty && a.asm.canUndo() && near(a.asm.doc.instances[1].transform.position, arm1.position));
a.undo();
b = await built();
check("drag: undo puts the arm back", near(inst("i2").transform.position, arm0.position, 1e-6), inst("i2").transform);

// ---------------------------------------------------------------- 2. shared meshes
a.openAssembly(await A.openAssembly(a, `${cwd}/corpus/assemblies/bracket-pins.ocasm`));
b = await built();
const handles = new Set(b.occurrences.map((o) => o.mesh));
check("bracket: three occurrences, two meshes", b.occurrences.length === 3 && handles.size === 2, [...handles]);
check("bracket: pins in their holes", near(inst("i2").transform.position, [15, 15, 8]) && near(inst("i3").transform.position, [45, 15, 8]));
await wait(500);

// ---------------------------------------------------------------- 3. build one
a.openAssembly(new A.AssemblySession(a, A.newAssembly(7), null));
await built();
const s = a.asm;
await s.insertComponent(`${cwd}/corpus/assemblies/plate.ocpart`);
await s.insertComponent(`${cwd}/corpus/assemblies/pin.ocpart`);
b = await built();
check("insert: plate fixed, pin floating", inst("i1").state === "fixed" && inst("i2").state === "under" && inst("i2").free === 6, b.instances);
check("insert: tree names", s.doc.instances.map((i) => i.name).join() === "plate<1>,pin<1>");
// Faces by geometry, as the Mate PropertyManager gets them from picks.
const faces = async (occ) => {
  const list = [];
  for (let k = 0; k < 40; k++) {
    try {
      list.push({ k, e: await inv("asm_entity", { part: b.occurrences[occ].part, kind: "face", index: k, quality: 3 }) });
    } catch (e) {
      if (String(e).includes("cannot")) continue;
      break;
    }
  }
  return list;
};
const plateF = await faces(0);
const pinF = await faces(1);
const find = (list, pred) => list.find((x) => pred(x.e.geom))?.k;
const hole = find(plateF, (g) => g.type === "cylinder" && Math.abs(g.p[0] - 15) < 1e-6 && Math.abs(g.p[1] - 15) < 1e-6);
const top = find(plateF, (g) => g.type === "plane" && g.n[2] > 0.999 && Math.abs(g.p[2] - 8) < 1e-6);
const shank = find(pinF, (g) => g.type === "cylinder" && Math.abs(g.r - 3) < 1e-6);
const under = find(pinF, (g) => g.type === "plane" && g.n[2] < -0.999 && Math.abs(g.p[2]) < 1e-6);
check("faces found", [hole, top, shank, under].every((k) => k !== undefined), { hole, top, shank, under });
const mate = async (p, q, set) => {
  s.openMate();
  await s.matePick({ kind: "face", id: p[1], body: p[0] });
  await s.matePick({ kind: "face", id: q[1], body: q[0] });
  const proposed = s.matePM.type;
  set?.(s.matePM);
  await s.acceptMate();
  await built();
  return proposed;
};
const proposed1 = await mate([0, hole], [1, shank]);
check("mate PM: two cylinders propose Concentric", proposed1 === "concentric", proposed1);
const proposed2 = await mate([0, top], [1, under]);
check("mate PM: two planes propose Coincident", proposed2 === "coincident", proposed2);
b = a.asm.built;
check("mates: solved, pin in the hole on the plate", b.mates.every((m) => m.status === "ok") && near(inst("i2").transform.position, [15, 15, 8]), { mates: b.mates, t: inst("i2").transform });
check("mates: pin (-) with one free rotation, 1 DOF", inst("i2").state === "under" && inst("i2").free === 1 && b.dof === 1, { i: inst("i2"), dof: b.dof });
check("mates: tree lists them", s.doc.mates.map((m) => m.name).join() === "Concentric1,Coincident1");
// A third mate that contradicts the second: the newest is the one in conflict.
await mate([0, top], [1, under], (pm) => {
  pm.type = "distance";
  pm.value = 5;
});
b = a.asm.built;
const m3 = s.doc.mates[2];
check("conflict: the newest mate is flagged", b.mates.find((m) => m.id === m3.id)?.status === "conflict" && b.newestError === m3.id, b.mates);
check("conflict: the component shows (+)", inst("i2").state === "over" && !!document.querySelector(`[data-asm-row="i2"]`)?.textContent.includes("(+)"));
check("conflict: the others stay solved", near(inst("i2").transform.position, [15, 15, 8]));
a.undo();
b = await built();
check("conflict: undo removes it", s.doc.mates.length === 2 && b.mates.every((m) => m.status === "ok"));

// Save As next to the build output, and reopen (lightweight from what was just written).
const file = `${cwd}/target/native-assembly-smoke.ocasm`;
check("save", await s.save(file), file);
check("save: paths relative to the assembly", s.doc.components.every((c) => c.path.startsWith("../corpus/assemblies/")), s.doc.components);
a.openAssembly(await A.openAssembly(a, file));
b = await built();
check("reopen: same instances and mates", a.asm.doc.instances.length === 2 && a.asm.doc.mates.length === 2 && b.mates.every((m) => m.status === "ok"));
// (Its parts are already resolved in this session, so they are not lightweight here; the
// hinge above shows a lightweight open.)
check("reopen: same placement and freedom", near(inst("i2").transform.position, [15, 15, 8]) && b.dof === 1, { t: inst("i2").transform, dof: b.dof });
a.viewHooks.fit?.();
await wait(800);

if (fails.length) throw new Error("assembly: " + JSON.stringify(fails));
return { ok: true, checks: out, gpu: await a.kernel.adapter?.() };
