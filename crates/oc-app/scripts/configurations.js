// `opencad --script=crates/oc-app/scripts/configurations.js` (npm run native:configurations):
// part configurations in the native app (OVERHAUL_PLAN G1e), through the ConfigurationManager
// tab, its context menus and dialog, the Modify box's configurations option, the tree's
// context toolbar and the Edit menu. The body of an async function: `a` is window.opencad,
// `cwd` the checkout, `wait` and `settle` come from smoke.rs. Throws when a check fails.
//
// Part: a 60 × 40 × 10 block with a Ø20 hole (D1 on its sketch), and a 10 × 10 post 40 high
// at a corner (Boss-Extrude2).
const inv = window.__TAURI_INTERNALS__.invoke;
const fails = [];
const out = {};
const check = (name, ok, detail) => {
  out[name] = ok ? true : detail ?? false;
  if (!ok) fails.push({ name, detail });
};
const el = (sel) => document.querySelector(sel);
const until = async (f, n = 200) => {
  for (let i = 0; i < n && !f(); i++) await wait(50);
  return !!f();
};
const setValue = (input, value) => {
  const proto = input instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, "value").set.call(input, String(value));
  input.dispatchEvent(new Event(input instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
};
const ctxAt = async (node) => {
  const q = node.getBoundingClientRect();
  node.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: q.left + 30, clientY: q.top + 5 }));
  await wait(150);
};
const menu = async (label) => {
  const find = () => [...document.querySelectorAll(".mi")].find((m) => m.textContent.trim() === label && !m.classList.contains("disabled"));
  for (let i = 0; i < 40 && !find(); i++) await wait(50);
  const it = find();
  if (!it) return false;
  it.click();
  await wait(150);
  return true;
};
const menuDisabled = (label) => [...document.querySelectorAll(".mi")].some((m) => m.textContent.trim() === label && m.classList.contains("disabled"));
const closeMenu = async () => {
  window.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
  await wait(100);
};
const dbl = async (node) => {
  node.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  node.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  await wait(100);
};
const rebuilt = async () => {
  await wait(80);
  await until(() => !a.rebuilding && !a.needsRebuild);
  await settle();
};
const volume = async () => (await a.kernel.massProps(a.activeFeatures()))?.volume ?? NaN;
const close = (x, y, tol = 1e-3) => Math.abs(x - y) <= tol;
const PI = Math.PI;
const cfgRow = (name) => el(`[data-config="${name}"]`);
const activeName = () => a.doc.configurations?.list.find((c) => c.id === a.doc.configurations.active)?.name ?? "Default";
const feat = (name) => a.doc.features.find((f) => f.name === name);
const tabConfig = async () => {
  el("[data-tab-config]").click();
  await wait(100);
};
const tabTree = async () => {
  el("[data-tab-tree]").click();
  await wait(100);
};
/** Opens the Modify box on a dimension (as a double-click on it does), types `v` with `scope`, rebuilds and closes it. */
const modify = async (target, v, scope) => {
  a.openModifyTarget(target, 300, 200);
  await until(() => !!el("[data-modify-scope]") || !!el(".modify input"));
  const sc = el("[data-modify-scope]");
  const scopeShown = !!sc;
  const defaultScope = sc?.value;
  if (sc && scope) setValue(sc, scope);
  await wait(50);
  const input = el(".modify-input input");
  setValue(input, v);
  await wait(30);
  el('.modify-tools button[title^="Regenerate"]').click();
  await rebuilt();
  el('.modify-tools button[title^="Save the current value"]').click();
  await wait(100);
  return { scopeShown, defaultScope };
};

// ---------------------------------------------------------------- the part
a.select({ kind: "plane", id: "front" });
a.startSketch();
await wait(300);
{
  const sk = a.sketch.data;
  const P = (id, x, y) => (sk.points[id] = { id, x, y });
  P("b", 60, 0); P("c", 60, 40); P("d", 0, 40); P("cc", 30, 20);
  [["l1", "origin", "b"], ["l2", "b", "c"], ["l3", "c", "d"], ["l4", "d", "origin"]].forEach(([id, p1, p2]) => (sk.entities[id] = { id, type: "line", p1, p2 }));
  sk.entities.k = { id: "k", type: "circle", c: "cc", r: 10 };
  sk.dimensions.dk = { id: "dk", name: "D1", kind: "diameter", refs: ["k"], value: 20, label: [42, 32] };
  sk.nextDim = 2;
}
a.resolveSketch();
a.startExtrude(false);
await wait(1200);
a.acceptPM();
await rebuilt();
a.select({ kind: "plane", id: "front" });
a.startSketch();
await wait(300);
{
  const sk = a.sketch.data;
  const P = (id, x, y) => (sk.points[id] = { id, x, y });
  P("q1", 0, 0); P("q2", 10, 0); P("q3", 10, 10); P("q4", 0, 10);
  [["m1", "q1", "q2"], ["m2", "q2", "q3"], ["m3", "q3", "q4"], ["m4", "q4", "q1"]].forEach(([id, p1, p2]) => (sk.entities[id] = { id, type: "line", p1, p2 }));
}
a.resolveSketch();
a.startExtrude(false);
await wait(1200);
a.pm.feature.end1.depth = 40;
a.schedulePreview?.();
await wait(600);
a.acceptPM();
await rebuilt();
const boss1 = feat("Boss-Extrude1");
const boss2 = feat("Boss-Extrude2");
const sketch1 = a.doc.features.find((f) => f.type === "sketch");
const V = (depth, holeR, post) => 60 * 40 * depth - PI * holeR * holeR * depth + (post ? 100 * Math.max(0, post - depth) : 0);
let vol = await volume();
check("part: block, hole and post", close(vol, V(10, 10, 40)), vol);
check("part: one configuration, Default, in the tree header", !a.doc.configurations && !!el('[data-tree-row="part-root"], .tree-row.root')?.textContent.includes("(Default)"), el(".tree-row.root")?.textContent);

// ---------------------------------------------------------------- ConfigurationManager
await tabConfig();
check("CM: the tab shows Default, active", !!el("[data-config-manager]") && !!cfgRow("Default") && cfgRow("Default").hasAttribute("data-config-active"));
await ctxAt(el("[data-config-root]"));
check("CM: Add Configuration... from the root's menu", await menu("Add Configuration..."));
await until(() => !!el('[data-config-dialog="add"]'));
setValue(el("[data-config-name]"), "Long");
setValue(el("[data-config-description]"), "deeper");
el("[data-config-ok]").click();
await wait(150);
check("CM: Long added and active (a copy of Default)", a.doc.configurations?.list.length === 2 && activeName() === "Long" && !!cfgRow("Long")?.hasAttribute("data-config-active") && !!cfgRow("Long")?.textContent.includes("[ deeper ]"), a.doc.configurations);
// A second "Long" is refused with a message.
await ctxAt(el("[data-config-root]"));
await menu("Add Configuration...");
await until(() => !!el('[data-config-dialog="add"]'));
setValue(el("[data-config-name]"), "Long");
el("[data-config-ok]").click();
await wait(150);
check("CM: a name in use is refused", a.doc.configurations.list.length === 2 && !!el('[data-config-dialog="add"]') && /already exists/.test(a.message?.text ?? ""), a.message);
el("[data-config-cancel]").click();
await wait(100);

// ---------------------------------------------------------------- dimensions in Long
await tabTree();
check("tree: the header names the active configuration", !!el(".tree-row.root")?.textContent.includes("(Long)"), el(".tree-row.root")?.textContent);
let m = await modify({ kind: "feature", featureId: boss1.id, key: "end1.depth" }, 25, null);
check("Modify: the configurations option, This Configuration by default", m.scopeShown && m.defaultScope === "this", m);
vol = await volume();
check("Long: 25 deep in this configuration", boss1.end1.depth === 25 && close(vol, V(25, 10, 40)), { depth: boss1.end1.depth, vol });
m = await modify({ kind: "sketchDim", sketchId: sketch1.id, dimId: "dk" }, 10, "this");
vol = await volume();
const circle = () => a.doc.features.find((f) => f.id === sketch1.id).sketch.entities.k;
check("Long: the hole Ø10 (sketch dimension, re-solved)", close(circle().r, 5, 1e-9) && close(vol, V(25, 5, 40)), { r: circle().r, vol });
const cfgs = a.doc.configurations.list;
check("Long: both values configured, Default keeps its own", cfgs[0].values?.[`${boss1.id}/end1.depth`] === 10 && cfgs[1].values?.[`${boss1.id}/end1.depth`] === 25 && cfgs[0].values?.[`${sketch1.id}/sketch.dimensions.dk.value`] === 20, cfgs);

// ---------------------------------------------------------------- suppression in Long
a.select({ kind: "feature", id: boss2.id });
await ctxAt(el(`[data-tree-row="${boss2.id}"]`));
const supBtn = [...document.querySelectorAll(".ctb")].find((b) => b.dataset.tip === "Suppress" && !b.classList.contains("disabled"));
supBtn?.click();
await rebuilt();
vol = await volume();
check("Long: the post suppressed from the tree's context toolbar (this configuration)", !!supBtn && boss2.suppressed === true && a.doc.configurations.list[1].suppressed?.[boss2.id] === true && a.doc.configurations.list[0].suppressed?.[boss2.id] === false && close(vol, V(25, 5, 0)), { vol, cfgs: a.doc.configurations.list });

// ---------------------------------------------------------------- activation
const longFeatures = JSON.stringify(a.doc.features);
await tabConfig();
await dbl(cfgRow("Default"));
await rebuilt();
vol = await volume();
const def = a.doc.features;
check("Default: double-click activates it; 10 deep, Ø20, post back", activeName() === "Default" && def.find((f) => f.id === boss1.id).end1.depth === 10 && close(circle().r, 10, 1e-12) && !def.find((f) => f.id === boss2.id).suppressed && close(vol, V(10, 10, 40)), { vol, active: activeName() });
check("Default: the checkmark moved", cfgRow("Default").hasAttribute("data-config-active") && !cfgRow("Long").hasAttribute("data-config-active"));
await dbl(cfgRow("Long"));
await rebuilt();
check("Long again: the same document, bit for bit", JSON.stringify(a.doc.features) === longFeatures);
// (A suppressed feature is no step of the graph: Long has one feature fewer to take.)
check("Long again: every feature from the regeneration cache", a.result.evaluatedSteps === 0 && a.result.cachedSteps === a.activeFeatures().filter((f) => !f.suppressed).length, { cached: a.result.cachedSteps, evaluated: a.result.evaluatedSteps, n: a.activeFeatures().length });
await dbl(cfgRow("Default"));
await rebuilt();
check("Default again: from the cache too", a.result.evaluatedSteps === 0 && a.result.cachedSteps === a.activeFeatures().filter((f) => !f.suppressed).length, { cached: a.result.cachedSteps, evaluated: a.result.evaluatedSteps });
// Activation is one undo step.
a.undo();
await rebuilt();
check("undo: back in Long", activeName() === "Long" && a.doc.features.find((f) => f.id === boss1.id).end1.depth === 25);
a.redo();
await rebuilt();
check("redo: Default", activeName() === "Default");

// ---------------------------------------------------------------- All Configurations
await tabTree();
m = await modify({ kind: "feature", featureId: boss2.id, key: "end1.depth" }, 30, "all");
vol = await volume();
check("All Configurations: the post 30 high here", a.doc.features.find((f) => f.id === boss2.id).end1.depth === 30 && close(vol, V(10, 10, 30)), vol);
await tabConfig();
await dbl(cfgRow("Long"));
await rebuilt();
check("All Configurations: and in Long (still suppressed there)", a.doc.features.find((f) => f.id === boss2.id).end1.depth === 30 && a.doc.features.find((f) => f.id === boss2.id).suppressed === true);
// Edit > Unsuppress > All Configurations.
await tabTree();
a.select({ kind: "feature", id: boss2.id });
const editMenu = [...document.querySelectorAll(".menu")].find((x) => x.firstChild?.textContent === "Edit");
editMenu.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
await wait(150);
const sub = [...document.querySelectorAll(".mi.sub")].find((x) => x.firstChild?.textContent?.trim() === "Unsuppress" || x.textContent.trim().startsWith("Unsuppress"));
const allItem = sub && [...sub.querySelectorAll(".mi")].find((x) => x.textContent.trim() === "All Configurations" && !x.classList.contains("disabled"));
allItem?.click();
await rebuilt();
vol = await volume();
check("Edit > Unsuppress > All Configurations", !!allItem && !a.doc.features.find((f) => f.id === boss2.id).suppressed && !a.doc.configurations.list.some((c) => c.suppressed && boss2.id in c.suppressed) && close(vol, V(25, 5, 30)), { vol, cfgs: a.doc.configurations.list });
await closeMenu();

// ---------------------------------------------------------------- properties, delete
await tabConfig();
await ctxAt(cfgRow("Long"));
check("CM: the active configuration cannot be deleted", menuDisabled("Delete"));
await menu("Properties...");
await until(() => !!el('[data-config-dialog="props"]'));
setValue(el("[data-config-name]"), "Deep");
el("[data-config-ok]").click();
await wait(150);
check("CM: renamed through Properties", !!cfgRow("Deep") && activeName() === "Deep");
await dbl(cfgRow("Default"));
await rebuilt();
await ctxAt(cfgRow("Deep"));
check("CM: Delete", await menu("Delete"));
check("CM: Deep deleted", a.doc.configurations.list.length === 1 && !cfgRow("Deep") && activeName() === "Default");
a.undo();
await wait(150);
check("CM: undo brings it back", a.doc.configurations.list.length === 2 && !!cfgRow("Deep"));

// ---------------------------------------------------------------- save and reopen
const file = `${cwd}/target/g1e/configured.ocpart`;
await inv("app_write_text", { path: file, text: a.serialize() });
const text = await inv("app_read_text", { path: file });
const saved = JSON.parse(text);
check("save: schema 3 with both configurations", saved.version === 3 && saved.configurations.list.map((c) => c.name).join() === "Default,Deep", saved.configurations);
a.loadDocument(text, "configured");
await rebuilt();
await tabConfig();
check("reopen: Default active, Deep listed", activeName() === "Default" && !!cfgRow("Deep"));
await dbl(cfgRow("Deep"));
await rebuilt();
vol = await volume();
check("reopen: Deep activates with its values", close(vol, V(25, 5, 30)), vol);

if (fails.length) throw new Error("configurations: " + JSON.stringify(fails));
return { ok: true, checks: out };
