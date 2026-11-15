// `opencad --script=crates/oc-app/scripts/picking.js` (npm run native:picking): picking in
// the native app through oc-render (F2b), driven by DOM pointer events on the graphics
// area, the way a mouse drives it. The body of an async function: `a` is window.opencad,
// `wait` and `settle` come from smoke.rs. Throws (exit code 1) when a check fails.
// It also runs in the browser build (three.js picking) for comparison; the two checks
// that only the native path passes are marked.
//
// Part: the smoke test's 60 × 40 × 10 block with a Ø20 hole at (30, 20), extruded from the
// Front Plane towards +Z, seen isometric.
a.select({ kind: "plane", id: "front" }); a.startSketch();
await wait(300);
{
  const sk = a.sketch.data;
  const P = (id, x, y) => (sk.points[id] = { id, x, y });
  P("b", 60, 0); P("c", 60, 40); P("d", 0, 40); P("cc", 30, 20);
  [["l1", "origin", "b"], ["l2", "b", "c"], ["l3", "c", "d"], ["l4", "d", "origin"]].forEach(([id, p1, p2]) => (sk.entities[id] = { id, type: "line", p1, p2 }));
  sk.entities.k = { id: "k", type: "circle", c: "cc", r: 10 };
}
a.resolveSketch(); a.startExtrude(false);
await wait(1200); a.acceptPM();
await wait(800); await settle();
a.viewHooks.orient("isometric"); await wait(900);

const v = window.opencadView;
const mesh = () => a.result.mesh;
const V3 = v.cam.target.constructor;
const fails = [];
const out = {};
const check = (name, ok, detail) => {
  out[name] = ok ? true : detail ?? false;
  if (!ok) fails.push({ name, detail });
};
const near = (p, q, tol = 1e-3) => !!p && !!q && Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]) < tol;
const S = (p) => v.cam.project(new V3(...p));
const toward = (a, b, d) => {
  const l = Math.hypot(b[0] - a[0], b[1] - a[1]);
  return [a[0] + ((b[0] - a[0]) / l) * d, a[1] + ((b[1] - a[1]) / l) * d];
};
const ev = (type, [sx, sy], o = {}) => {
  const r = v.overlay.getBoundingClientRect();
  v.overlay.dispatchEvent(
    new PointerEvent(type, { clientX: r.left + sx, clientY: r.top + sy, bubbles: true, cancelable: true, pointerId: 1, pointerType: "mouse", isPrimary: true, button: o.button ?? 0, buttons: o.buttons ?? 0, ctrlKey: !!o.ctrl }),
  );
};
const move = async (p, buttons = 0) => {
  ev("pointermove", p, { buttons });
  await v.settled();
};
const BTN = [1, 4, 2]; // buttons bit of left, middle, right
const click = async (p, button = 0) => {
  ev("pointerdown", p, { button, buttons: BTN[button] });
  ev("pointerup", p, { button });
  await v.settled();
};

// ---------------------------------------------------------------- hover and click
const onFront = S([10, 30, 10]); // front face (z = 10), away from the hole and the edges
const times = [];
for (let i = 0; i < 20; i++) {
  const t = performance.now();
  await move([onFront[0] + (i % 2), onFront[1]]);
  times.push(performance.now() - t);
}
times.sort((x, y) => x - y);
out.hoverMs = { median: +times[10].toFixed(2), max: +times[19].toFixed(2) };
await move(onFront);
const hov = a.hover;
check("hoverFace", hov?.kind === "face", hov);
// Which face: the renderer's ray hit lies on z = 10.
if (v.native) {
  const ray = await v.native.pick(v.cam, v.host, { x: onFront[0], y: onFront[1], filter: { only: null, edgesOnly: false }, wait: true, ray: "first", selector: false });
  check("rayOnFront", ray.faces[0]?.id === hov?.id && Math.abs(ray.faces[0].point[2] - 10) < 1e-3, ray);
}
const frontId = hov?.id;
await click(onFront);
check("clickSelectsFace", a.selection.length === 1 && a.selection[0].kind === "face" && a.selection[0].id === frontId, a.selection);

// Top front edge (y = 40, z = 10) at its middle, and 3 px off it onto the front face.
const onEdge = S([30, 40, 10]);
await move(onEdge);
const eh = a.hover;
const esig = eh?.kind === "edge" ? mesh().edges[eh.id].sig : null;
check("hoverEdge", !!esig && Math.abs(esig.a[1] - 40) < 1e-6 && Math.abs(esig.b[1] - 40) < 1e-6 && Math.abs(esig.a[2] - 10) < 1e-6 && Math.abs(esig.b[2] - 10) < 1e-6, eh);
await move(toward(onEdge, S([30, 30, 10]), 3));
check("edgeWithin4px", a.hover?.kind === "edge" && a.hover.id === eh?.id, a.hover);
// Selection filter on faces: the face, with the edge 3 px away.
a.filterOn = true; a.selFilter = new Set(["face"]);
await move(toward(onEdge, S([30, 30, 10]), 3));
check("filterFaces", a.hover?.kind === "face", a.hover);
a.filterOn = false; a.selFilter = new Set();

// Corner vertex (60, 40, 10), 2 px off it along the edge.
const corner = S([60, 40, 10]);
await move(toward(corner, onEdge, 2));
const vh = a.hover;
check("hoverVertex", vh?.kind === "vertex" && near(mesh().vertices[vh.id], [60, 40, 10]), vh);
// Background.
await move([5, 5]);
check("hoverNothing", a.hover === null, a.hover);
// The origin (0, 0, 0) is the back bottom corner, hidden in this view: picked first while shown.
await move(S([0, 0, 0]));
check("hoverOrigin", a.hover?.kind === "origin", a.hover);

// ---------------------------------------------------------------- Select Other
// From (15, 25, 10) the view ray leaves through the back face at (5, 15, 0).
await click(S([15, 25, 10]), 2);
const others = v.selectOtherCandidates();
check("selectOther", others.length >= 2 && others[0].kind === "face" && others[0].id === frontId && others.every((x) => x.kind === "face"), others);
document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
await wait(100);

// ---------------------------------------------------------------- rotate about (middle click)
await wait(400); // not a double middle click (Zoom to Fit)
await click(onFront, 1);
const piv = v.rotateAbout?.pivot;
check("rotateAboutFace", !!piv && Math.abs(piv.z - 10) < 1e-3 && piv.x > 0 && piv.x < 60, piv && piv.toArray());
await wait(400);
await click(onEdge, 1);
const ax = v.rotateAbout;
check("rotateAboutEdge", !!ax?.axis && Math.abs(Math.abs(ax.axis.x) - 1) < 1e-6 && Math.abs(ax.pivot.y - 40) < 1e-6, ax && { pivot: ax.pivot.toArray(), axis: ax.axis?.toArray() });
await wait(400);
await click([5, 5], 1);
check("rotateAboutCleared", v.rotateAbout === null, v.rotateAbout);

// ---------------------------------------------------------------- double-click: feature dimensions
await click([5, 5]);
await wait(400);
ev("dblclick", onFront, {});
await v.settled();
const extrude = a.doc.features.find((f) => f.type === "extrude");
check("dblClickShowsDims", a.shownDims.includes(extrude?.id), a.shownDims);
await click([5, 5]);

// ---------------------------------------------------------------- box selection
const pts = [];
for (const x of [0, 60]) for (const y of [0, 40]) for (const z of [0, 10]) pts.push(S([x, y, z]));
const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
const [x0, x1, y0, y1] = [Math.min(...xs) - 15, Math.max(...xs) + 15, Math.min(...ys) - 15, Math.max(...ys) + 15];
const drag = async (from, to) => {
  ev("pointerdown", from, { buttons: 1 });
  await move([(from[0] + to[0]) / 2, (from[1] + to[1]) / 2], 1);
  await move(to, 1);
  ev("pointerup", to, {});
  await v.settled();
};
await drag([x0, y0], [x1, y1]);
const edges = mesh().edges.filter((e) => !e.seam).length;
check("boxWindowAll", a.selection.length === edges && a.selection.every((s) => s.kind === "edge"), { got: a.selection.length, edges });
// Crossing (right to left) over the middle of the top front edge: that edge alone. (Native
// only: the browser's box tests mesh points, and this straight edge has none in the box.)
a.select(null);
if (v.native) {
  await drag([onEdge[0] + 8, onEdge[1] - 8], [onEdge[0] - 8, onEdge[1] + 4]);
  check("boxCrossingEdge", a.selection.length === 1 && a.selection[0].kind === "edge" && a.selection[0].id === eh?.id, a.selection);
  a.select(null);
}

// ---------------------------------------------------------------- View Selector
v.showSelector(true);
await wait(200);
const w = v.overlay.clientWidth, h = v.overlay.clientHeight;
const s = Math.min(w, h) * 0.17;
const q = v.cam.quat.clone().invert();
const cube = (p) => {
  const c = new V3(...p).applyQuaternion(q);
  return [w / 2 + c.x * s, h / 2 - c.y * s];
};
await move(cube([0, 0, 1]));
check("selectorHover", v.selector.hover !== null && a.hover === null, v.selector.hover);
await wait(100);
await click(cube([0, 0, 1]));
await wait(1200);
check("selectorFront", v.cam.viewName === "Front" && !v.selector.visible, { view: v.cam.viewName, visible: v.selector.visible });
a.viewHooks.orient("isometric"); await wait(900);

// ---------------------------------------------------------------- sketch snap to a model vertex
// A sketch on the Front Plane (z = 0), seen isometric: the front face's corner (60, 40, 10)
// is off the sketch plane, so only the pick under the pointer can snap to it.
a.select({ kind: "plane", id: "front" }); a.startSketch(); await wait(400);
a.viewHooks.orient("isometric"); await wait(900);
a.editor.setTool("line"); await wait(100);
const c2 = S([60, 40, 10]);
await move(toward(c2, S([30, 40, 10]), 2));
const snap = a.editor.snap;
check("sketchSnapVertex", snap?.kind === "model" && Math.abs(snap.point[0] - 60) < 1e-6 && Math.abs(snap.point[1] - 40) < 1e-6, snap && { kind: snap.kind, point: snap.point });
await move(S([30, 40, 10]));
const snap2 = a.editor.snap;
check("sketchSnapEdgeMid", snap2?.kind === "model" && snap2.badges?.includes("midpoint") && Math.abs(snap2.point[0] - 30) < 1e-6, snap2 && { kind: snap2.kind, point: snap2.point, badges: snap2.badges });
a.editor.setTool(null);
a.exitSketch?.(false);

if (fails.length) throw new Error("picking: " + JSON.stringify(fails));
return { ok: true, checks: out, gpu: await a.kernel.adapter?.() };
