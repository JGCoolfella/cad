/**
 * Parity of the Rust solver (WebAssembly build, ./wasmSolver.ts) with the TypeScript solver
 * (./solver.ts): parameters within 1e-9 after solve and drag, identical analysis.
 *
 * The same cases with the TypeScript results are stored in
 * crates/oc-sketch/tests/fixtures/ts-parity.json for the native Rust test (`cargo test`).
 * Re-record after a deliberate solver change: `UPDATE_GOLDEN=1 npx vitest run wasmSolver`.
 */
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import type { Dimension, Id, SketchData, SketchEntity, Vec2 } from "../core/types";
import { STD_FRAMES } from "../core/math";
import { emptySketch } from "./model";
import { addRef } from "./refs";
import { SHAPES } from "./shapes";
import { SketchSystem, type Analysis } from "./solver";
import { benchSketch, initSketchWasm, WasmSketchSystem, type SketchSystemApi } from "./wasmSolver";

const FIXTURE = new URL("../../crates/oc-sketch/tests/fixtures/ts-parity.json", import.meta.url);
const TOL = 1e-9;

interface Case {
  name: string;
  sketch: SketchData;
  extraFixed?: Id[];
  /** Point dragged after the first solve, and where to. */
  drag?: { id: Id; to: Vec2 };
}

interface Result {
  solved: boolean;
  x1: number[];
  dragged: boolean | null;
  x2: number[];
  analysis: { dof: number; status: string; pointState: Record<Id, string>; entityState: Record<Id, string>; bad: Id[] };
  measures: (number | null)[];
  committed: SketchData;
}

// ------------------------------------------------------------------ case builders

const pt = (sk: SketchData, id: Id, x: number, y: number) => (sk.points[id] = { id, x, y });
const line = (sk: SketchData, id: Id, p1: Id, p2: Id) => (sk.entities[id] = { id, type: "line", p1, p2 });
const circle = (sk: SketchData, id: Id, c: Id, r: number) => (sk.entities[id] = { id, type: "circle", c, r });
const arc = (sk: SketchData, id: Id, c: Id, p1: Id, p2: Id) => (sk.entities[id] = { id, type: "arc", c, p1, p2 });
let relN = 0;
const rel = (sk: SketchData, r: object) => {
  const id = `r${relN++}`;
  sk.relations[id] = { ...r, id } as never;
};
const dim = (sk: SketchData, kind: Dimension["kind"], refs: Id[], value: number, extra: Partial<Dimension> = {}) => {
  const id = `d${sk.nextDim++}`;
  sk.dimensions[id] = { id, name: id.toUpperCase(), kind, refs, value, label: [0, 0], ...extra };
};

function rect(sk: SketchData, w: number, h: number) {
  for (const [id, x, y] of [["a", 0, 0], ["b", w, 0], ["c", w, h], ["d", 0, h]] as const) pt(sk, id, x, y);
  for (const [id, p1, p2] of [["l1", "a", "b"], ["l2", "b", "c"], ["l3", "c", "d"], ["l4", "d", "a"]] as const) line(sk, id, p1, p2);
}
function hv(sk: SketchData) {
  rel(sk, { type: "horizontal", e: "l1" });
  rel(sk, { type: "horizontal", e: "l3" });
  rel(sk, { type: "vertical", e: "l2" });
  rel(sk, { type: "vertical", e: "l4" });
}

function shape(name: string, opts: object, clicks: Vec2[]): SketchData {
  const sk = emptySketch();
  let n = 0;
  const b = {
    newPoint: (p: Vec2) => {
      const id = `p${n++}`;
      sk.points[id] = { id, x: p[0], y: p[1] };
      return id;
    },
    pointFor: (s: { point: Vec2 }) => b.newPoint(s.point),
    addEntity: (e: SketchEntity) => ((sk.entities[e.id] = e), e.id),
    addRel: (r: object) => {
      const id = `rel${n++}`;
      sk.relations[id] = { ...r, id } as never;
      return id;
    },
    autoRel: true,
  };
  const snaps = clicks.map((p) => ({ point: p, kind: "free" as const, guides: [], badges: [] }));
  SHAPES[name].create(b as never, snaps, opts);
  return renumber(sk, new Set(Object.keys(sk.points)));
}

/** Renames ids made by uid() (session-random prefix) to `u0`, `u1`, ... so cases are deterministic. */
function renumber(sk: SketchData, keep: Set<Id>): SketchData {
  const ids = [...Object.keys(sk.points), ...Object.keys(sk.entities), ...Object.keys(sk.refs ?? {})];
  const ren = new Map(ids.filter((id) => !keep.has(id)).map((id, i) => [id, `u${i}`]));
  const json = JSON.stringify(sk).replace(/"([^"]+)"/g, (m, s) => (ren.has(s) ? `"${ren.get(s)}"` : m));
  return JSON.parse(json);
}

function cases(): Case[] {
  relN = 0;
  const out: Case[] = [];
  const add = (name: string, sketch: SketchData, extra: Partial<Case> = {}) => out.push({ name, sketch, ...extra });

  // solver.test.ts
  {
    const sk = emptySketch();
    rect(sk, 10, 5);
    sk.points.c.y = 6;
    hv(sk);
    add("hv-rect", sk);
  }
  {
    const sk = emptySketch();
    rect(sk, 10, 5);
    hv(sk);
    rel(sk, { type: "coincident", p: "a", e: "origin" });
    dim(sk, "distance", ["a", "b"], 20);
    dim(sk, "distance", ["b", "c"], 8);
    add("full-rect", sk);
  }
  {
    const sk = emptySketch();
    rect(sk, 10, 5);
    hv(sk);
    rel(sk, { type: "parallel", e1: "l1", e2: "l3" });
    add("redundant-parallel", sk);
  }
  {
    const sk = emptySketch();
    pt(sk, "a", 0, 0);
    pt(sk, "b", 10, 0);
    line(sk, "l", "a", "b");
    rel(sk, { type: "coincident", p: "a", e: "origin" });
    dim(sk, "distance", ["a", "b"], 10);
    add("drag-swing", sk, { drag: { id: "b", to: [0, 20] } });
  }
  {
    const sk = emptySketch();
    pt(sk, "c", 1, 1);
    circle(sk, "k", "c", 3);
    pt(sk, "a", -10, 5);
    pt(sk, "b", 10, 5);
    line(sk, "l", "a", "b");
    rel(sk, { type: "tangent", e1: "l", e2: "k" });
    dim(sk, "diameter", ["k"], 8);
    add("tangent-circle", sk);
  }
  add("straight-slot", shape("straightSlot", {}, [[10, 10], [40, 10], [40, 16]]));
  add("polygon-inscribed", shape("polygon", { sides: 6, inscribed: true }, [[5, 5], [25, 5]]));
  add("polygon-circumscribed", shape("polygon", { sides: 6, inscribed: false }, [[5, 5], [25, 5]]));

  // refs.test.ts
  {
    const sk = emptySketch();
    const v = addRef(sk, { kind: "vertex", p: [20, 10, 0] }, STD_FRAMES.front)!;
    const e = addRef(sk, { kind: "edge", sig: { curve: "LINE", a: [0, 10, 0], b: [20, 10, 0], mid: [10, 10, 0], length: 20 } }, STD_FRAMES.front)!;
    sk.points.m = { id: "m", x: 3, y: 3, standalone: true };
    rel(sk, { type: "midpoint", p: "m", e: e.entity! });
    sk.points.k = { id: "k", x: 30, y: 30 };
    line(sk, "l", v.point!, "k");
    const c = addRef(sk, { kind: "edge", sig: { curve: "CIRCLE", a: [45, 0, 0], b: [45, 0, 0], mid: [35, 0, 0], length: 10 * Math.PI } }, STD_FRAMES.front)!;
    pt(sk, "c2", 60, 3);
    circle(sk, "k2", "c2", 2);
    rel(sk, { type: "coradial", e1: "k2", e2: c.entity! });
    add("model-references", renumber(sk, new Set(["origin", "m", "k", "l", "c2", "k2"])));
  }

  // Every relation and dimension kind, from perturbed starting positions.
  {
    const sk = emptySketch();
    rect(sk, 10, 5);
    sk.points.c.x = 10.4;
    rel(sk, { type: "perpendicular", e1: "l1", e2: "l2" });
    rel(sk, { type: "parallel", e1: "l1", e2: "l3" });
    rel(sk, { type: "horizontal", e: "l1" });
    rel(sk, { type: "fix", target: "a" });
    pt(sk, "e", 14, 0.3);
    pt(sk, "f", 20, 0.6);
    line(sk, "l5", "e", "f");
    rel(sk, { type: "collinear", e1: "l1", e2: "l5" });
    pt(sk, "g", 3, 9);
    pt(sk, "h", 7, 9.5);
    line(sk, "l6", "g", "h");
    rel(sk, { type: "horizontalPoints", p1: "g", p2: "h" });
    rel(sk, { type: "verticalPoints", p1: "g", p2: "d" });
    dim(sk, "horizontal", ["g", "h"], 4, { sign: 1 });
    dim(sk, "vertical", ["c", "g"], 3.5, { sign: 1 });
    dim(sk, "pointLine", ["h", "l1"], 9);
    pt(sk, "m", 5.2, 2);
    rel(sk, { type: "midpoint", p: "m", e: "l6" });
    pt(sk, "s1", -2, 2);
    pt(sk, "s2", -2, 8);
    line(sk, "axis", "s1", "s2");
    rel(sk, { type: "vertical", e: "axis" });
    pt(sk, "q1", -5, 3);
    pt(sk, "q2", 1.5, 3.2);
    rel(sk, { type: "symmetric", p1: "q1", p2: "q2", axis: "axis" });
    pt(sk, "x", -1.5, 8.6);
    rel(sk, { type: "intersection", p: "x", e1: "l6", e2: "axis" });
    pt(sk, "u1", 30, 0);
    pt(sk, "u2", 40, 12);
    line(sk, "l7", "u1", "u2");
    dim(sk, "angle", ["l1", "l7"], 1.0, { sign: 1, quadrant: 1 });
    dim(sk, "lineLine", ["l1", "l3"], 5.5);
    add("lines", sk, { drag: { id: "u2", to: [41, 15] } });
  }
  {
    const sk = emptySketch();
    pt(sk, "c1", 0, 0.2);
    circle(sk, "k1", "c1", 5);
    pt(sk, "c2", 12, 1);
    circle(sk, "k2", "c2", 3);
    rel(sk, { type: "tangent", e1: "k1", e2: "k2" });
    pt(sk, "c3", 1, 1);
    circle(sk, "k3", "c3", 2);
    rel(sk, { type: "tangent", e1: "k1", e2: "k3", inner: true });
    pt(sk, "c4", 0.3, -0.2);
    circle(sk, "k4", "c4", 7);
    rel(sk, { type: "concentric", e1: "k1", e2: "k4" });
    dim(sk, "diameter", ["k4"], 15);
    // Fillet-like: line ending on an arc, tangent there; a second arc tangent at a shared end.
    pt(sk, "ac", 20, 10);
    pt(sk, "a1", 25, 10.3);
    pt(sk, "a2", 20, 15);
    arc(sk, "arc1", "ac", "a1", "a2");
    pt(sk, "lb", 25.2, 0);
    line(sk, "lf", "lb", "a1");
    rel(sk, { type: "tangent", e1: "lf", e2: "arc1" });
    pt(sk, "bc", 20, 20.5);
    pt(sk, "b2", 15, 20);
    arc(sk, "arc2", "bc", "b2", "a2");
    rel(sk, { type: "tangent", e1: "arc1", e2: "arc2" });
    rel(sk, { type: "equal", e1: "arc1", e2: "arc2" });
    rel(sk, { type: "equal", e1: "k2", e2: "arc1" });
    dim(sk, "radius", ["arc1"], 4.5);
    dim(sk, "arcLength", ["arc2"], 6);
    pt(sk, "mid", 23, 14);
    rel(sk, { type: "midpoint", p: "mid", e: "arc1" });
    pt(sk, "on", 4, 4);
    rel(sk, { type: "coincident", p: "on", e: "k1" });
    pt(sk, "on2", 18, 12);
    rel(sk, { type: "coincident", p: "on2", e: "arc2" });
    pt(sk, "ec", 40, 0);
    pt(sk, "e1", 45, 0);
    pt(sk, "e2", 40, 6);
    pt(sk, "e3", 40.5, -5.5);
    rel(sk, { type: "equidistant", c: "ec", p1: "e1", p2: "e2" });
    rel(sk, { type: "equidistant", c: "ec", p1: "e1", p2: "e3" });
    pt(sk, "c5", 50, 20);
    circle(sk, "k5", "c5", 2);
    rel(sk, { type: "fix", target: "k5" });
    pt(sk, "c6", 58, 21);
    circle(sk, "k6", "c6", 3);
    rel(sk, { type: "coradial", e1: "k6", e2: "k5" });
    add("arcs", sk, { drag: { id: "c2", to: [14, 4] } });
  }
  {
    const sk = emptySketch();
    pt(sk, "c", 0, 0);
    pt(sk, "a", 10, 0.5);
    pt(sk, "q", 0, 5);
    sk.entities.el = { id: "el", type: "ellipse", c: "c", a: "a", q: "q" };
    rel(sk, { type: "coincident", p: "c", e: "origin" });
    pt(sk, "p", 6, 3.5);
    rel(sk, { type: "coincident", p: "p", e: "el" });
    for (const [id, x, y] of [["s0", 20, 0], ["s1", 25, 5], ["s2", 30, 2], ["s3", 36, 6]] as const) pt(sk, id, x, y);
    sk.entities.sp = { id: "sp", type: "spline", pts: ["s0", "s1", "s2", "s3"] };
    pt(sk, "on", 28, 6);
    rel(sk, { type: "coincident", p: "on", e: "sp" });
    pt(sk, "pe", 50, 50);
    sk.entities.pt = { id: "pt", type: "point", p: "pe" };
    pt(sk, "z", 49, 52);
    rel(sk, { type: "coincident", p: "z", e: "pt" });
    rel(sk, { type: "fix", target: "s0" });
    add("ellipse-spline", sk, { drag: { id: "q", to: [1, 7] } });
  }
  {
    const sk = emptySketch();
    pt(sk, "a1", 0, 0);
    pt(sk, "a2", 10, 0);
    line(sk, "la", "a1", "a2");
    pt(sk, "b1", 0, 3);
    pt(sk, "b2", 10, 3.2);
    line(sk, "lb", "b1", "b2");
    pt(sk, "m1", 0, 10);
    pt(sk, "m2", 10, 10);
    line(sk, "ma", "m1", "m2");
    pt(sk, "n1", 0, 13);
    pt(sk, "n2", 10, 13);
    line(sk, "mb", "n1", "n2");
    rel(sk, { type: "offset", e1: "ma", e2: "mb", m1: "la", m2: "lb" });
    rel(sk, { type: "parallel", e1: "la", e2: "lb" });
    dim(sk, "lineLine", ["la", "lb"], 4);
    pt(sk, "c", 30, 0);
    circle(sk, "k1", "c", 5);
    circle(sk, "k2", "c", 7);
    dim(sk, "offset", ["k1", "k2"], 2.5);
    pt(sk, "c3", 50, 0);
    circle(sk, "k3", "c3", 4);
    pt(sk, "c4", 50, 0.5);
    circle(sk, "k4", "c4", 6);
    rel(sk, { type: "offset", e1: "k3", e2: "k4", m1: "k1", m2: "k2" });
    add("offsets", sk, { drag: { id: "b2", to: [11, 5] } });
  }
  {
    const sk = emptySketch();
    rect(sk, 10, 5);
    hv(sk);
    dim(sk, "distance", ["a", "c"], 0, { driven: true });
    pt(sk, "u", 3, 9);
    line(sk, "l7", "d", "u");
    dim(sk, "angle", ["l4", "l7"], 0, { driven: true, sign: -1 });
    add("driven", sk, { extraFixed: ["a"] });
  }
  {
    const sk = emptySketch();
    pt(sk, "a", 0, 0);
    pt(sk, "b", 10, 0);
    line(sk, "l", "a", "b");
    rel(sk, { type: "fix", target: "a" });
    dim(sk, "distance", ["a", "b"], 10);
    dim(sk, "distance", ["a", "b"], 20);
    add("conflict", sk);
  }
  {
    const sk = emptySketch();
    pt(sk, "p", 3, 4);
    rel(sk, { type: "fix", target: "p" });
    rel(sk, { type: "coincident", p: "p", e: "origin" });
    pt(sk, "q", 1, 1);
    add("constant-equation", sk);
  }

  // Sketches of the golden corpus (drawn by the app's own helpers).
  const corpus = new URL("../../corpus/", import.meta.url);
  for (const f of readdirSync(corpus).filter((f) => f.endsWith(".ocpart")).sort()) {
    const doc = JSON.parse(readFileSync(new URL(f, corpus), "utf8"));
    for (const feat of doc.features) if (feat.type === "sketch") add(`corpus/${f}/${feat.id}`, feat.sketch);
  }

  // Benchmark sketches (crates/oc-sketch/src/generators.rs) at N = 10.
  for (const kind of ["grid", "grid-linked", "linkage", "polygon", "tangent-chain"]) {
    const b = benchSketch(kind, 10);
    add(`bench/${kind}`, b.sketch, { drag: b.drag });
  }
  return out;
}

// ------------------------------------------------------------------ running

function run(make: (sk: SketchData, fixed: Set<Id>) => SketchSystemApi, c: Case): Result {
  const sk: SketchData = structuredClone(c.sketch);
  const sys = make(sk, new Set(c.extraFixed ?? []));
  const solved = sys.solve();
  const x1 = sys.x.slice();
  let dragged: boolean | null = null;
  if (c.drag) dragged = sys.drag([{ id: c.drag.id, to: c.drag.to }]);
  const x2 = sys.x.slice();
  sys.commit();
  const a: Analysis = sys.analyse(solved);
  const measures = Object.values(sk.dimensions).map((d) => sys.measureFn(d)?.() ?? null);
  return {
    solved,
    x1,
    dragged,
    x2,
    analysis: { ...a, bad: [...a.bad] },
    measures,
    committed: sk,
  };
}

/** The TypeScript solver as shipped (Jacobian by central differences). */
const ts = (sk: SketchData, f: Set<Id>) => new SketchSystem(sk, f);
/**
 * The TypeScript solver's iteration (step, damping, line search, drag, RREF) with the exact
 * Jacobian in place of central differences. Isolates the algorithm from the ~1e-9 relative
 * truncation error of the differences, which alone moves results by up to ~1e-7 (see
 * OVERHAUL_PLAN section 8, E2).
 */
const tsExact = (sk: SketchData, f: Set<Id>) => {
  const t = new SketchSystem(sk, f);
  const exact = new WasmSketchSystem(structuredClone(sk), f);
  t.jacobian = () => {
    exact.x = t.x;
    return exact.jacobian();
  };
  return t;
};
const rs = (sk: SketchData, f: Set<Id>) => new WasmSketchSystem(sk, f);

/** Parameters and measured values: the algorithm (`tsExact`) to 1e-9; the shipped solver to 1e-6. */
const TOL_FD = 1e-6;

function maxDiff(a: number[], b: number[]) {
  expect(a.length).toBe(b.length);
  return a.reduce((m, v, i) => Math.max(m, Math.abs(v - b[i])), 0);
}

type Record_ = Omit<Result, "committed">;

/** Same return values and analysis exactly; parameters and measures within `tol`. */
function expectParity(name: string, got: Record_, want: Record_, tol: number) {
  expect(got.solved, `${name}: solve result`).toBe(want.solved);
  expect(got.dragged, `${name}: drag result`).toBe(want.dragged);
  expect(maxDiff(got.x1, want.x1), `${name}: parameters after solve`).toBeLessThanOrEqual(tol);
  expect(maxDiff(got.x2, want.x2), `${name}: parameters after drag`).toBeLessThanOrEqual(tol);
  expect(got.analysis, `${name}: analysis`).toEqual(want.analysis);
  expect(got.measures.length).toBe(want.measures.length);
  got.measures.forEach((m, i) => {
    if (m === null || want.measures[i] === null) expect(m, `${name}: measure ${i}`).toBe(want.measures[i]);
    else expect(Math.abs(m - want.measures[i]!), `${name}: measure ${i}`).toBeLessThanOrEqual(tol);
  });
}

const strip = ({ committed: _, ...r }: Result): Record_ => r;

beforeAll(() => {
  initSketchWasm(readFileSync(new URL("./wasm/oc_sketch_bg.wasm", import.meta.url)));
});

describe("oc-sketch (WebAssembly) against the TypeScript solver", () => {
  it("matches the TypeScript algorithm within 1e-9 and the shipped solver within 1e-6", () => {
    const all = cases();
    const records = all.map((c) => {
      const got = run(rs, c);
      const exact = run(tsExact, c);
      const fd = run(ts, c);
      expectParity(c.name, got, exact, TOL);
      expectParity(`${c.name} (shipped)`, got, fd, TOL_FD);
      return { ...c, tsExact: strip(exact), ts: strip(fd) };
    });
    expect(all.length).toBeGreaterThan(30);
    if (process.env.UPDATE_GOLDEN) {
      writeFileSync(FIXTURE, JSON.stringify(records, null, 1) + "\n");
      return;
    }
    // The stored fixture (used by the native Rust test) must still be what TypeScript does.
    const stored = JSON.parse(readFileSync(FIXTURE, "utf8")) as typeof records;
    expect(stored.map((r) => r.name)).toEqual(records.map((r) => r.name));
    stored.forEach((s, i) => {
      expect(s.sketch).toEqual(records[i].sketch);
      expectParity(`fixture ${s.name}`, s.tsExact, records[i].tsExact, TOL);
      expectParity(`fixture ${s.name} (shipped)`, s.ts, records[i].ts, TOL);
    });
  });

  it("commits like the TypeScript solver", () => {
    for (const c of cases()) {
      const a = run(tsExact, c).committed, b = run(rs, c).committed;
      expect(Object.keys(b.points)).toEqual(Object.keys(a.points));
      for (const id of Object.keys(a.points)) {
        expect(Math.max(Math.abs(b.points[id].x - a.points[id].x), Math.abs(b.points[id].y - a.points[id].y)), `${c.name} ${id}`).toBeLessThanOrEqual(TOL);
      }
      for (const [id, e] of Object.entries(a.entities)) {
        const f = b.entities[id];
        if (e.type === "circle" && f.type === "circle") expect(Math.abs(f.r - e.r), `${c.name} ${id}`).toBeLessThanOrEqual(TOL);
      }
      for (const d of Object.keys(a.dimensions)) {
        expect(Math.abs(b.dimensions[d].value - a.dimensions[d].value), `${c.name} ${d}`).toBeLessThanOrEqual(TOL);
      }
    }
  });

  it("exposes the same parameter layout", () => {
    const sk = benchSketch("polygon", 6).sketch;
    const a = new SketchSystem(structuredClone(sk)), b = new WasmSketchSystem(structuredClone(sk));
    expect([...b.pts.entries()]).toEqual([...a.pts.entries()]);
    expect([...b.radii.entries()]).toEqual([...a.radii.entries()]);
    expect(b.x).toEqual(a.x);
    // Exact (dual-number) Jacobian vs central differences.
    const ja = a.jacobian(), jb = b.jacobian();
    expect(jb.length).toBe(ja.length);
    ja.forEach((row, i) => row.forEach((v, j) => expect(Math.abs(jb[i][j] - v)).toBeLessThan(1e-6)));
    expect(maxDiff(b.residuals(), a.residuals())).toBeLessThan(1e-12);
  });
});
