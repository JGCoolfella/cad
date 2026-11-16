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
// 4. Files by path: a part opened and exported to STEP.
// 5. G1b, on copies of the corpus parts in target/g1b/: Insert Components following the
//    pointer; SmartMates (Alt-drag) and the Mate pop-up toolbar; Open Part, edit, Return
//    to Assembly (mates follow); a Missing reference re-picked in Edit Mate; Move and
//    Rotate Component (every mode); Hide/Show, rename, appearance, Fix/Float, Suppress
//    from the context menu; mates to assembly and component planes and origins from the
//    fly-out tree; a sub-assembly: mates on its geometry, open, edit, return, save;
//    assembly Mass Properties.
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
  v.overlay.dispatchEvent(new PointerEvent(type, { clientX: r.left + sx, clientY: r.top + sy, bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", isPrimary: true, button: o.button ?? 0, buttons: o.buttons ?? 0, altKey: !!o.altKey }));
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
await s.insertComponent(`${cwd}/corpus/assemblies/pin.ocpart`, [75, 15, 0]);
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
// A third mate that contradicts the second. As in SOLIDWORKS, the Mate PropertyManager
// refuses a mate that would over-define the assembly: it says so, OK stays disabled, and
// the mate is not added.
s.openMate();
await s.matePick({ kind: "face", id: top, body: 0 });
await s.matePick({ kind: "face", id: under, body: 1 });
s.matePM.type = "distance";
s.matePM.value = 5;
await s.preview();
await wait(200);
check("over-defining: the PM says so", !!s.matePM?.problem && !!document.querySelector("[data-mate-problem]"), s.matePM?.problem);
const refused = await s.acceptMate();
check("over-defining: OK refuses it, the PM stays open", !!refused && !!s.matePM && s.doc.mates.length === 2, refused);
s.cancelMate();
b = await built();
check("over-defining: cancel leaves the two mates solved", b.mates.length === 2 && b.mates.every((m) => m.status === "ok") && near(inst("i2").transform.position, [15, 15, 8]));
// The same mate in the document (a file saved so, or parts edited since): the newest
// conflicting mate is flagged and the others stay solved.
s.checkpoint("Add Mate");
s.doc.mates.push({ ...JSON.parse(JSON.stringify(s.doc.mates[1])), id: "m9", name: "Distance1", type: "distance", value: 5 });
b = await s.build();
check("conflict: the newest mate is flagged", b.mates.find((m) => m.id === "m9")?.status === "conflict" && b.newestError === "m9", b.mates);
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

// ---------------------------------------------------------------- 4. files by path
// The native app opens, saves and exports by path (the Windows dialogs give the paths;
// here they are given directly).
const pinPath = `${cwd}/corpus/assemblies/pin.ocpart`;
a.loadDocument(await inv("app_read_text", { path: pinPath }), "pin.ocpart");
a.filePath = pinPath;
await settle();
check("part by path: opened, assembly closed", !a.asm && a.result?.mesh?.faces.length === 5);
const pinVol = (await a.kernel.massProps(a.activeFeatures())).volume;
const stepPath = `${cwd}/target/native-assembly-smoke.step`;
const n = await inv("engine_export_to", { features: a.activeFeatures(), format: "step", quality: 3, path: stepPath });
const stepHead = (await inv("app_read_text", { path: stepPath })).slice(0, 13);
check("export by path: STEP written", n > 1000 && stepHead === "ISO-10303-21;", { n, stepHead });

// ---------------------------------------------------------------- 5. G1b
const G = `${cwd}/target/g1b`;
const corpusText = {};
for (const n of ["plate", "pin", "arm"]) {
  corpusText[n] = await inv("app_read_text", { path: `${cwd}/corpus/assemblies/${n}.ocpart` });
  await inv("app_write_text", { path: `${G}/${n}.ocpart`, text: corpusText[n] });
}
const el = (sel) => document.querySelector(sel);
const rowSel = (id, root = "") => `${root}[data-asm-row="${id}"]`;
const rowCtx = async (id) => {
  const r = el(rowSel(id));
  const q = r.getBoundingClientRect();
  r.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: q.left + 30, clientY: q.top + 5 }));
  await wait(150);
};
const menu = async (label) => {
  const it = [...document.querySelectorAll(".mi")].find((m) => m.textContent.trim() === label && !m.classList.contains("disabled"));
  if (!it) return false;
  it.click();
  await wait(150);
  return true;
};
const cmdBtn = (label) => [...document.querySelectorAll("button.cbtn")].find((x) => x.dataset.tip === label && !x.classList.contains("disabled"));
const setValue = (input, value) => {
  const proto = input instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value").set.call(input, String(value));
  input.dispatchEvent(new Event(input instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
};
const key = (k, target = document) => target.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
const until = async (f, n = 200) => {
  for (let i = 0; i < n && !f(); i++) await wait(50);
  return !!f();
};
const occOf = (id) => a.asm.built.occurrences.findIndex((o) => o.path.join("/") === id);
const facesOf = async (occ) => {
  const list = [];
  for (let k = 0; k < 40; k++) {
    try {
      list.push({ k, e: await inv("asm_entity", { part: a.asm.built.occurrences[occ].part, kind: "face", index: k, quality: 3 }) });
    } catch (e) {
      if (String(e).includes("cannot")) continue;
      break;
    }
  }
  return list;
};
const findFace = (list, pred) => list.find((x) => pred(x.e.geom))?.k;
const zOf = (t) => qrot(t.rotation, [0, 0, 1]);
const viewPlanePoint = (sp) => {
  const vd = v.cam.viewDir();
  const r = v.cam.ray(sp[0], sp[1]);
  const t = vd.dot(v.cam.target.clone().sub(r.origin)) / vd.dot(r.direction);
  const p = r.origin.clone().add(r.direction.clone().multiplyScalar(t));
  return [p.x, p.y, p.z];
};
const W = v.overlay.getBoundingClientRect();
const pressAt = async (sp, o = {}) => {
  ev("pointerdown", sp, { buttons: 1, ...o });
  await v.settled();
  ev("pointerup", sp, o);
  await v.settled();
};
const dragPath = async (from, to, o = {}) => {
  ev("pointerdown", from, { buttons: 1, ...o });
  await v.settled();
  for (let k = 1; k <= 8; k++) {
    ev("pointermove", [from[0] + ((to[0] - from[0]) * k) / 8, from[1] + ((to[1] - from[1]) * k) / 8], { buttons: 1, ...o });
    await v.settled();
    await wait(30);
  }
  await wait(250);
  ev("pointerup", to, o);
  await v.settled();
  await wait(250);
};
const hoverAt = async (sp) => {
  ev("pointermove", sp);
  await v.settled();
  await wait(80);
  ev("pointermove", sp);
  await v.settled();
  return a.hover;
};

// 5a. Insert Components: the first is fixed at the origin; the next follows the pointer
// and drops where you click; Esc cancels an insertion.
a.openAssembly(new A.AssemblySession(a, A.newAssembly(8), null));
await built();
const S5 = a.asm;
await S5.insertComponent(`${G}/plate.ocpart`);
check("G1b insert: the first component is fixed at the origin, not placed", !S5.placing && inst("i1").state === "fixed" && near(inst("i1").transform.position, [0, 0, 0]));
a.viewHooks.orient("isometric");
await wait(300);
a.viewHooks.fit?.();
await wait(800);
await S5.insertComponent(`${G}/pin.ocpart`);
check("G1b insert: the next one follows the pointer", !!S5.placing && S5.placing.instance === "i2");
const pinCenter = S5.placing.center;
const center = () => place(inst("i2").transform, pinCenter);
const sp1 = [W.width * 0.3, W.height * 0.3];
ev("pointermove", sp1);
await v.settled();
await wait(100);
check("G1b insert: its centre is under the pointer", near(center(), viewPlanePoint(sp1), 1e-6), [center(), viewPlanePoint(sp1)]);
// Beside the plate and in front of it, so nothing hides the pin's head.
const sp2 = S([95, 15, 30]);
ev("pointermove", sp2);
await v.settled();
await wait(100);
check("G1b insert: it follows the pointer", near(center(), viewPlanePoint(sp2), 1e-6));
await pressAt(sp2);
await built();
check("G1b insert: a click drops it there", !S5.placing && near(place(S5.doc.instances[1].transform, pinCenter), viewPlanePoint(sp2), 1e-6) && inst("i2").free === 6, S5.doc.instances[1].transform);
const pinAt = S5.doc.instances[1].transform;
a.undo();
await built();
check("G1b insert: one undo step", S5.doc.instances.length === 1);
a.redo();
await built();
check("G1b insert: redo", S5.doc.instances.length === 2 && near(S5.doc.instances[1].transform.position, pinAt.position));
await S5.insertComponent(`${G}/arm.ocpart`);
key("Escape");
await built();
check("G1b insert: Esc cancels it", !S5.placing && S5.doc.instances.length === 2 && !S5.canRedo());

// 5b. SmartMates: Alt-drag the pin's head onto the plate's top face; the Mate pop-up
// toolbar proposes Coincident (the pair allows no Concentric), flips the alignment, adds it.
const pinOcc5 = occOf("i2");
const headTop = place(inst("i2").transform, [0, 0, 3]);
const plateTop = [30, 5, 8];
const hHead = await hoverAt(S(headTop));
check("G1b smartmates: the pin's head is under the pointer", hHead?.kind === "face" && hHead.body === pinOcc5, hHead);
const hPlate = await hoverAt(S(plateTop));
check("G1b smartmates: the plate's top is under the pointer", hPlate?.kind === "face" && hPlate.body === occOf("i1"), hPlate);
await dragPath(S(headTop), S(plateTop), { altKey: true });
await until(() => S5.matePM?.picks.length === 2 && !S5.matePM.busy && !S5.building);
await wait(300);
check("G1b smartmates: the Mate PropertyManager has both picks", S5.matePM?.picks.length === 2 && S5.matePM.picks[0].ref.occurrence[0] === "i2" && S5.matePM.picks[1].ref.occurrence[0] === "i1", S5.matePM?.picks);
check("G1b popup: shown with Coincident proposed", !!el("[data-mate-popup]") && !!el('[data-popup-type="coincident"].on'), el("[data-mate-popup]")?.outerHTML.slice(0, 200));
check("G1b popup: mates the pair cannot take are greyed", el('[data-popup-type="concentric"]')?.disabled === true && el('[data-popup-type="distance"]')?.disabled === false);
check("G1b popup: aligned (closest to the placement), previewed", S5.matePM?.align === "aligned" && Math.abs(place(inst("i2").transform, [0, 0, 3])[2] - 8) < 1e-6 && zOf(inst("i2").transform)[2] > 0.999);
el("[data-popup-flip]")?.click();
await wait(100);
await until(() => !S5.building);
await wait(200);
check("G1b popup: Flip Mate Alignment turns the pin over", S5.matePM?.align === "antiAligned" && zOf(inst("i2").transform)[2] < -0.999, zOf(inst("i2").transform));
el("[data-popup-flip]")?.click();
await wait(100);
await until(() => !S5.building);
await wait(200);
el("[data-popup-ok]")?.click();
await until(() => !S5.matePM && !S5.building);
b = await built();
check("G1b smartmates: OK adds the mate", S5.doc.mates.length === 1 && S5.doc.mates[0].type === "coincident" && S5.doc.mates[0].align === "aligned" && b.mates[0]?.status === "ok" && !el("[data-mate-popup]"), S5.doc.mates);
check("G1b smartmates: the head is flush with the plate's top", Math.abs(place(inst("i2").transform, [0, 0, 3])[2] - 8) < 1e-6 && inst("i2").free === 3);
if (S5.matePM) S5.cancelMate();
await built();
// Concentric with the first hole, picked by face.
let plateF5 = await facesOf(occOf("i1"));
const pinF5 = await facesOf(occOf("i2"));
const hole1 = findFace(plateF5, (g) => g.type === "cylinder" && Math.abs(g.p[0] - 15) < 1e-6);
const shank5 = findFace(pinF5, (g) => g.type === "cylinder" && Math.abs(g.r - 3) < 1e-6);
S5.openMate();
await S5.matePick({ kind: "face", id: hole1, body: occOf("i1") });
await S5.matePick({ kind: "face", id: shank5, body: occOf("i2") });
await wait(100);
check("G1b popup: two cylinders propose Concentric", S5.matePM?.type === "concentric" && !!el('[data-popup-type="concentric"].on'));
await until(() => !S5.building);
el("[data-popup-ok]")?.click();
await until(() => !S5.matePM && !S5.building);
b = await built();
check("G1b mates: the pin in the first hole", near(inst("i2").transform.position, [15, 15, 5]) && b.dof === 1, inst("i2").transform);

// 5c. Open Part from the assembly, edit it, Return to Assembly: the assembly shows the
// edit before it is saved and the mates follow it.
await rowCtx("i1");
check("G1b open part: in the component's context menu", await menu("Open Part"));
await until(() => !a.asm && !!a.result);
await settle();
check("G1b open part: the part is open, the assembly waits", !a.asm && a.doc.name === "plate" && a.asmStack.length === 1 && a.filePath === `${G}/plate.ocpart` && a.result?.mesh?.faces.length === 8, { name: a.doc.name, stack: a.asmStack.length, path: a.filePath, faces: a.result?.mesh?.faces.length });
const holeSketch = () => a.doc.features.find((f) => f.id === "f3").sketch;
a.checkpoint("Edit");
holeSketch().points.f3_p1.x = 20;
holeSketch().points.f3_p1.y = 12;
a.rebuild();
await settle();
check("G1b open part: edited", a.dirty && a.result?.mesh?.faces.length === 8);
const windowMenu = [...document.querySelectorAll(".menu")].find((m) => m.firstChild?.textContent === "Window");
windowMenu?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
await wait(150);
check("G1b return: the Window menu lists the assembly", el('[data-window="0"]')?.textContent === S5.name);
el('[data-window="0"]')?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
const viaMenu = a.asmStack.length === 0;
if (a.asmStack.length) a.returnToAssembly();
b = await built();
check("G1b return: back in the assembly from the Window menu", viaMenu && a.asm === S5 && a.asmStack.length === 0);
check("G1b return: the pin follows the moved hole (mates by persistent name)", near(inst("i2").transform.position, [20, 12, 5], 1e-6) && b.mates.every((m) => m.status === "ok"), inst("i2").transform.position);
check("G1b return: unsaved, the assembly reads the edit from memory", !!S5.overrides[`${G}/plate.ocpart`] && (await inv("app_read_text", { path: `${G}/plate.ocpart` })) === corpusText.plate && a.dirty);
// Delete that hole: the concentric mate's plate side is Missing.
await rowCtx("i1");
await menu("Open Part");
await until(() => !a.asm && !!a.result);
await settle();
check("G1b open part: the unsaved edit is what opens", holeSketch().points.f3_p1.x === 20 && a.dirty);
// The holes' cut deleted. (Deleting one of the two holes heals the reference onto the
// other hole by persistent name: a naming finding, section 8.)
a.checkpoint("Edit");
a.doc.features = a.doc.features.filter((f) => f.id !== "f4");
a.rebuild();
await settle();
check("G1b open part: holes deleted", a.result?.mesh?.faces.length === 6);
a.returnToAssembly();
b = await built();
const conc = S5.doc.mates.find((m) => m.type === "concentric");
check("G1b missing: the concentric mate is Missing", b.mates.find((m) => m.id === conc.id)?.status === "missing" && !!el(rowSel(conc.id))?.textContent.includes("**Missing**"), b.mates);
// Edit Mate: the Missing reference is listed and is the one the next pick replaces.
await rowCtx(conc.id);
check("G1b edit mate: from the context menu", await menu("Edit Feature"));
const missingSide = S5.matePM?.picks.findIndex((p) => p.missing) ?? -1;
check("G1b edit mate: the Missing reference is listed, ready to re-pick", missingSide >= 0 && S5.matePM.active === missingSide && !!el("[data-mate-missing]") && !!el(`[data-mate-pick="${missingSide}"]`)?.textContent.includes("**Missing**") && el("[data-mate-ok]")?.disabled === true);
// The plate's edge along Z at its corner (0, 0) instead: the pin's axis on it.
let corner;
for (let k = 0; k < 40 && corner === undefined; k++) {
  try {
    const e = await inv("asm_entity", { part: a.asm.built.occurrences[occOf("i1")].part, kind: "edge", index: k, quality: 3 });
    if (e.geom?.type === "line" && Math.abs(Math.abs(e.geom.d[2]) - 1) < 1e-9 && Math.hypot(e.geom.p[0], e.geom.p[1]) < 1e-9) corner = k;
  } catch (e) {
    if (!String(e).includes("cannot")) break;
  }
}
await S5.matePick({ kind: "edge", id: corner, body: occOf("i1") });
await until(() => !S5.building);
await wait(200);
check("G1b edit mate: re-picked", !!S5.matePM && !S5.matePM.picks.some((p) => p.missing) && S5.matePM.active === null && !S5.matePM.problem);
el("[data-mate-ok]")?.click();
await until(() => !S5.matePM && !S5.building);
b = await built();
check("G1b edit mate: OK, the mate is solved on the edge", b.mates.every((m) => m.status === "ok") && near(inst("i2").transform.position, [0, 0, 5], 1e-6) && S5.doc.mates.length === 2, { corner, mates: b.mates, t: inst("i2").transform.position });
// Re-picking an entry by clicking it (one that is not Missing).
S5.openMate(conc.id);
await wait(50);
el('[data-mate-pick="0"]')?.click();
await wait(50);
check("G1b edit mate: clicking an entry makes it the one to replace", S5.matePM?.active === 0 && !!el('[data-mate-pick="0"]')?.classList.contains("sel"));
key("Escape");
await built();

// 5d. Move and Rotate Component.
await S5.insertComponent(`${G}/arm.ocpart`, [0, 80, 20]);
b = await built();
check("G1b arm inserted floating at a point", inst("i3").free === 6 && !S5.placing);
a.viewHooks.fit?.();
await wait(800);
cmdBtn("Move Component")?.click();
await wait(100);
check("G1b move: the PropertyManager opens", !!S5.movePM && !!el("[data-move-pm]") && !!el("[data-flyout]"));
const armTop = () => place(inst("i3").transform, [40, 5, 6]);
// Along Assembly XYZ: only one coordinate changes.
setValue(el("[data-move-mode]"), "xyz");
await wait(50);
let before = inst("i3").transform;
let g0 = armTop();
await dragPath(S(g0), [S(g0)[0] + 80, S(g0)[1] + 10]);
let after = inst("i3").transform;
let dp = after.position.map((x, k) => x - before.position[k]);
check("G1b move: Along Assembly XYZ moves along one axis", dp.filter((x) => Math.abs(x) > 1e-6).length === 1 && near(after.rotation.slice(1), before.rotation.slice(1), 1e-9) && Math.max(...dp.map(Math.abs)) > 5, dp);
// Along Entity: a plate edge (or face) picked, the arm moves along it.
setValue(el("[data-move-mode]"), "entity");
await wait(50);
check("G1b move: Along Entity asks for the entity", !!S5.movePM?.pickingEntity && !!el("[data-move-entity]"));
await hoverAt(S([30, 0, 8]));
await pressAt(S([30, 0, 8]));
await until(() => !!S5.movePM?.entity);
const ent = S5.movePM?.entity;
check("G1b move: the entity is picked", !!ent && !S5.movePM.pickingEntity, ent);
before = inst("i3").transform;
g0 = armTop();
await dragPath(S(g0), [S(g0)[0] - 60, S(g0)[1] + 40]);
after = inst("i3").transform;
dp = after.position.map((x, k) => x - before.position[k]);
const dl = Math.hypot(...dp);
const du = ent ? Math.abs(dp[0] * ent.d[0] + dp[1] * ent.d[1] + dp[2] * ent.d[2]) / Math.hypot(...ent.d) : 0;
check("G1b move: Along Entity moves parallel to it", dl > 1 && Math.abs(du - dl) < 1e-6 * Math.max(1, dl), { dp, d: ent?.d });
// By Delta XYZ and To XYZ Position on the selected component; one undo step each.
setValue(el("[data-move-mode]"), "delta");
await wait(50);
a.select({ kind: "feature", id: "i3" });
await wait(50);
[10, -5, 2].forEach((x, k) => setValue(el(`[data-move-value="${k}"]`), x));
await wait(50);
before = inst("i3").transform;
el("[data-move-apply]")?.click();
await until(() => !near(inst("i3").transform.position, before.position, 1e-9));
await wait(100);
check("G1b move: By Delta XYZ", near(inst("i3").transform.position, [before.position[0] + 10, before.position[1] - 5, before.position[2] + 2], 1e-6) && near(S5.doc.instances[2].transform.position, inst("i3").transform.position), inst("i3").transform.position);
a.undo();
b = await built();
check("G1b move: undo", near(inst("i3").transform.position, before.position, 1e-6));
// The pin, mated (it only turns about its axis), stays where its mates hold it.
a.select({ kind: "feature", id: "i2" });
await wait(50);
const pinBefore = inst("i2").transform;
el("[data-move-apply]")?.click();
await wait(500);
check("G1b move: a mated component moves only as its mates allow", near(inst("i2").transform.position, pinBefore.position, 1e-6));
setValue(el("[data-move-mode]"), "toXYZ");
await wait(50);
a.select({ kind: "feature", id: "i3" });
[120, 40, 10].forEach((x, k) => setValue(el(`[data-move-value="${k}"]`), x));
await wait(50);
el("[data-move-apply]")?.click();
await until(() => near(inst("i3").transform.position, [120, 40, 10], 1e-6));
check("G1b move: To XYZ Position", near(inst("i3").transform.position, [120, 40, 10], 1e-6), inst("i3").transform.position);
// Rotate: By Delta XYZ. The pin may turn about its axis; a turn its mates forbid is undone by them.
setValue(el("[data-rotate-mode]"), "delta");
await wait(50);
check("G1b rotate: the Rotate group is active", !!S5.movePM?.rotate && S5.movePM.rot === "delta");
a.select({ kind: "feature", id: "i2" });
[0, 0, 90].forEach((x, k) => setValue(el(`[data-move-value="${k}"]`), x));
await wait(50);
const pinR0 = inst("i2").transform;
el("[data-move-apply]")?.click();
await wait(600);
const xAxis = (t) => qrot(t.rotation, [1, 0, 0]);
const turn = (t0, t1) => (Math.atan2(xAxis(t1)[1], xAxis(t1)[0]) - Math.atan2(xAxis(t0)[1], xAxis(t0)[0])) * (180 / Math.PI);
check("G1b rotate: By Delta XYZ turns the pin 90° about its axis", Math.abs(((turn(pinR0, inst("i2").transform) + 360) % 360) - 90) < 1e-6 && near(inst("i2").transform.position, pinR0.position, 1e-6), turn(pinR0, inst("i2").transform));
[90, 0, 0].forEach((x, k) => setValue(el(`[data-move-value="${k}"]`), x));
await wait(50);
el("[data-move-apply]")?.click();
await wait(600);
check("G1b rotate: a turn its mates forbid is taken back", zOf(inst("i2").transform)[2] > 0.999999 && near(inst("i2").transform.position, pinR0.position, 1e-6), zOf(inst("i2").transform));
// Rotate: Free Drag turns the arm about its centre.
setValue(el("[data-rotate-mode]"), "free");
await wait(50);
before = inst("i3").transform;
const armC = place(before, [25, 5, 3]);
g0 = armTop();
await dragPath(S(g0), [S(g0)[0] + 40, S(g0)[1] + 30]);
after = inst("i3").transform;
check("G1b rotate: Free Drag turns it about its centre", !near(after.rotation.slice(0, 3), before.rotation.slice(0, 3), 1e-3) && near(place(after, [25, 5, 3]), armC, 1e-6), { after, before });
// Rotate: About Entity, the plate edge: every point keeps its distance to the line.
setValue(el("[data-rotate-mode]"), "entity");
await wait(50);
check("G1b rotate: About Entity uses the picked entity", !S5.movePM?.pickingEntity && !!S5.movePM?.entity);
const e5 = S5.movePM?.entity ?? { p: [0, 0, 0], d: [1, 0, 0] };
const distLine = (p) => {
  const u = e5.d.map((x) => x / Math.hypot(...e5.d));
  const w = p.map((x, k) => x - e5.p[k]);
  const t = w[0] * u[0] + w[1] * u[1] + w[2] * u[2];
  return Math.hypot(w[0] - t * u[0], w[1] - t * u[1], w[2] - t * u[2]);
};
before = inst("i3").transform;
const far0 = place(before, [50, 10, 6]);
g0 = armTop();
await dragPath(S(g0), [S(g0)[0] + 30, S(g0)[1] - 50]);
after = inst("i3").transform;
check("G1b rotate: About Entity turns about the line", !near(after.rotation, before.rotation, 1e-3) && Math.abs(distLine(place(after, [50, 10, 6])) - distLine(far0)) < 1e-6, { a: distLine(place(after, [50, 10, 6])), b: distLine(far0) });
check("G1b rotate: undoable", S5.canUndo());
key("Escape");
await wait(50);
if (S5.movePM) key("Escape");
await wait(100);
check("G1b move: Esc closes the PropertyManager", !S5.movePM);

// 5e. Component commands from the context menu.
await rowCtx("i3");
await menu("Hide Components");
b = await built();
check("G1b hide: hidden, still mated, greyed in the tree", b.occurrences.find((o) => o.path[0] === "i3")?.hidden === true && !!el(rowSel("i3"))?.classList.contains("muted"));
await rowCtx("i3");
await menu("Show Components");
b = await built();
check("G1b show", b.occurrences.find((o) => o.path[0] === "i3")?.hidden === false && !S5.doc.instances[2].hidden);
await rowCtx("i3");
await menu("Rename tree item");
const ren = el('[data-asm-rename="i3"]');
check("G1b rename: the name is editable in the tree", !!ren);
if (ren) {
  setValue(ren, "lever");
  key("Enter", ren);
}
await wait(100);
check("G1b rename: the instance number stays", S5.doc.instances[2].name === "lever<1>" && !!el(rowSel("i3"))?.textContent.includes("lever<1>"), S5.doc.instances[2].name);
S5.setColor(["i3"], "#ff2000");
await wait(100);
check("G1b appearance: the component's colour", S5.doc.instances[2].color === "#ff2000" && !!el(`${rowSel("i3")} .asm-swatch`));
await rowCtx("i3");
await menu("Remove Appearance");
check("G1b appearance: removed", !S5.doc.instances[2].color);
await rowCtx("i3");
await menu("Fix");
b = await built();
check("G1b fix: (f)", inst("i3").state === "fixed" && !!el(rowSel("i3"))?.textContent.includes("(f) lever<1>"));
await rowCtx("i3");
await menu("Float");
b = await built();
check("G1b float: (-)", inst("i3").state === "under" && !!el(rowSel("i3"))?.textContent.includes("(-) lever<1>"));
await rowCtx("i3");
await menu("Suppress");
b = await built();
check("G1b suppress: not drawn", inst("i3").state === "suppressed" && b.occurrences.length === 2);
await rowCtx("i3");
await menu("Unsuppress");
b = await built();
check("G1b unsuppress", inst("i3").state === "under" && b.occurrences.length === 3);

// 5f. Mates to the assembly's planes and origin and to a component's, from the fly-out tree.
cmdBtn("Mate")?.click();
await wait(100);
const fly = "[data-flyout] ";
check("G1b planes: the fly-out tree lists the assembly's planes", !!el(rowSel("$plane:top", fly)) && !!el(rowSel("$origin", fly)));
el(rowSel("$plane:top", fly))?.click();
el(`${rowSel("i3", fly)} .twisty`)?.click();
await wait(100);
el(rowSel("i3/$plane:front", fly))?.click();
await until(() => S5.matePM?.picks.length === 2 && !S5.building);
await wait(200);
check("G1b planes: Top Plane of the assembly and Front Plane of the arm picked", S5.matePM?.picks[0]?.label === `Top Plane@${S5.name}` && S5.matePM?.picks[1]?.label === "Front Plane@lever<1>" && S5.matePM.type === "coincident", S5.matePM?.picks.map((p) => p.label));
el("[data-popup-ok]")?.click();
await until(() => !S5.matePM && !S5.building);
b = await built();
check("G1b planes: the arm's Front Plane lies on the assembly's Top Plane", Math.abs(Math.abs(zOf(inst("i3").transform)[1]) - 1) < 1e-9 && Math.abs(inst("i3").transform.position[1]) < 1e-9 && b.mates.every((m) => m.status === "ok"), inst("i3").transform);
cmdBtn("Mate")?.click();
await wait(100);
el(rowSel("$origin", fly))?.click();
if (!el(rowSel("i3/$origin", fly))) el(`${rowSel("i3", fly)} .twisty`)?.click();
await wait(100);
el(rowSel("i3/$origin", fly))?.click();
await until(() => S5.matePM?.picks.length === 2 && !S5.building);
await wait(200);
el("[data-popup-ok]")?.click();
await until(() => !S5.matePM && !S5.building);
b = await built();
check("G1b origin: the arm's origin on the assembly's", near(inst("i3").transform.position, [0, 0, 0], 1e-9) && b.mates.every((m) => m.status === "ok") && S5.doc.mates.length === 4, { t: inst("i3").transform.position, mates: b.mates });
if (S5.matePM) S5.cancelMate();

// 5g. A sub-assembly: this one saved (with the edited plate), inserted into another.
check("G1b save: with the edited part", await S5.save(`${G}/sub.ocasm`));
const savedPlate = JSON.parse(await inv("app_read_text", { path: `${G}/plate.ocpart` }));
check("G1b save: the edited part was saved with it", !savedPlate.features.some((f) => f.id === "f4") && savedPlate.features.find((f) => f.id === "f3").sketch.points.f3_p1.x === 20 && Object.keys(S5.overrides).length === 0);
check("G1b save: component paths relative to the assembly", S5.doc.components.map((c) => c.path).join() === "plate.ocpart,pin.ocpart,arm.ocpart", S5.doc.components);
a.openAssembly(new A.AssemblySession(a, A.newAssembly(9), null));
await built();
const T = a.asm;
await T.insertComponent(`${G}/arm.ocpart`);
await T.insertComponent(`${G}/sub.ocasm`, [0, 0, 60]);
b = await built();
check("G1b sub: inserted, drawn with its parts", b.occurrences.length === 4 && b.occurrences.filter((o) => o.path[0] === "i2").length === 3 && b.occurrences.every((o) => o.mesh > 0 && b.parts[o.part]) && T.componentOf(T.doc.instances[1]).kind === "assembly" && inst("i2").free === 6, b.occurrences.map((o) => [o.path, o.mesh, o.part]));
a.viewHooks.fit?.();
await wait(600);
// A mate on the sub-assembly's plate (occurrence i2/i1), picked as in the graphics area.
const subPlate = occOf("i2/i1");
const subTop = findFace(await facesOf(subPlate), (g) => g.type === "plane" && g.n[2] > 0.999 && Math.abs(g.p[2] - 8) < 1e-6);
const armUnder = findFace(await facesOf(occOf("i1")), (g) => g.type === "plane" && g.n[2] < -0.999);
T.openMate();
await T.matePick({ kind: "face", id: subTop, body: subPlate });
await T.matePick({ kind: "face", id: armUnder, body: occOf("i1") });
check("G1b sub: a face of a part in the sub-assembly picked", T.matePM?.picks[0]?.ref.occurrence.join("/") === "i2/i1" && T.matePM.picks[0].label === `Face@plate@${T.doc.instances[1].name}`, T.matePM?.picks[0]);
await until(() => !T.building);
el("[data-popup-ok]")?.click();
await until(() => !T.matePM && !T.building);
b = await built();
check("G1b sub: the mate on its geometry is solved", b.mates[0]?.status === "ok" && Math.abs(place(b.occurrences[subPlate].world, [30, 5, 8])[2]) < 1e-6 && inst("i2").free === 3, { m: b.mates, w: b.occurrences[subPlate]?.world });
// Open the sub-assembly, edit it (delete its arm), return: the top assembly shows it.
await rowCtx("i2");
check("G1b sub: Open Assembly in its context menu", await menu("Open Assembly"));
await until(() => a.asm && a.asm !== T && !!a.asm.built && !a.asm.building);
const SUB = a.asm;
check("G1b sub: open on its own", SUB !== T && SUB.name === "sub" && a.asmStack.length === 1 && SUB.doc.instances.length === 3, { name: SUB.name, n: SUB.doc.instances.length });
SUB.delete(["i3"]);
await built();
check("G1b sub: edited", a.dirty && SUB.doc.instances.length === 2);
a.returnToAssembly();
b = await built();
check("G1b sub: back, the top assembly shows the edit", a.asm === T && b.occurrences.length === 3 && !!T.overrides[`${G}/sub.ocasm`] && a.dirty && b.mates[0]?.status === "ok", { n: b.occurrences.length, o: Object.keys(T.overrides) });
check("G1b sub: save writes the sub-assembly too", await T.save(`${G}/top.ocasm`));
const subSaved = await inv("asm_open", { path: `${G}/sub.ocasm` });
check("G1b sub: saved", subSaved.doc.instances.length === 2);
b = await T.build();

// 5h. Mass Properties of the assembly: each part at its placement.
[...document.querySelectorAll(".cm-tab")].find((t) => t.textContent === "Evaluate")?.click();
await wait(100);
cmdBtn("Mass Properties")?.click();
await until(() => !!el("[data-mass-asm]"));
check("G1b mass: the dialog shows the assembly's", !!el("[data-mass-asm]")?.textContent.includes("3 components") && !!el("[data-mass-asm]")?.textContent.includes("Mass ="), el("[data-mass-asm]")?.textContent);
const m1 = await T.massProps();
const pinM = m1.occurrences.find((o) => o.path.join("/") === "i2/i2");
check("G1b mass: a part's volume as in the part", !!pinM && Math.abs(pinM.volume - pinVol) < 1e-6 * pinVol, { pinM, pinVol, occ: a.asm.built.occurrences.map((o) => [o.path, o.part]) });
// Moving the sub-assembly moves its parts' centres with it.
const subT0 = inst("i2").transform;
await inv("asm_place", { instance: "i2", to: { position: [subT0.position[0] + 7, subT0.position[1], subT0.position[2]], rotation: subT0.rotation } });
const m2 = await T.massProps();
const moved = m2.occurrences.filter((o) => o.path[0] === "i2").every((o) => {
  const o1 = m1.occurrences.find((x) => x.path.join("/") === o.path.join("/"));
  return near(o.center, [o1.center[0] + 7, o1.center[1], o1.center[2]], 1e-6) && Math.abs(o.volume - o1.volume) < 1e-9;
});
check("G1b mass: centres follow the placements", moved && m2.occurrences.length === 3, { m1, m2 });
b = await T.build();
a.dirty = false;

if (fails.length) throw new Error("assembly: " + JSON.stringify(fails));
return { ok: true, checks: out, gpu: await a.kernel.adapter?.() };
