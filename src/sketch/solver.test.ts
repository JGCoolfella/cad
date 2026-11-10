import { describe, expect, it } from "vitest";
import { emptySketch, extractProfiles } from "./model";
import { SketchSystem, solveSketch } from "./solver";
import type { SketchData } from "../core/types";

function rect(sk: SketchData, w: number, h: number) {
  const pts = [
    ["a", 0, 0],
    ["b", w, 0],
    ["c", w, h],
    ["d", 0, h],
  ] as const;
  for (const [id, x, y] of pts) sk.points[id] = { id, x, y };
  const ls = [["l1", "a", "b"], ["l2", "b", "c"], ["l3", "c", "d"], ["l4", "d", "a"]] as const;
  for (const [id, p1, p2] of ls) sk.entities[id] = { id, type: "line", p1, p2 };
}

describe("solver", () => {
  it("solves horizontal / vertical and reports dof", () => {
    const sk = emptySketch();
    rect(sk, 10, 5);
    sk.points.c.y = 6; // perturb
    sk.relations.r1 = { id: "r1", type: "horizontal", e: "l1" };
    sk.relations.r2 = { id: "r2", type: "horizontal", e: "l3" };
    sk.relations.r3 = { id: "r3", type: "vertical", e: "l2" };
    sk.relations.r4 = { id: "r4", type: "vertical", e: "l4" };
    const a = solveSketch(sk);
    expect(a.status).toBe("under");
    expect(a.dof).toBe(4); // position x,y + width + height
    expect(sk.points.c.y).toBeCloseTo(sk.points.d.y, 8);
  });

  it("becomes fully defined with origin + two dimensions", () => {
    const sk = emptySketch();
    rect(sk, 10, 5);
    sk.relations.r1 = { id: "r1", type: "horizontal", e: "l1" };
    sk.relations.r2 = { id: "r2", type: "horizontal", e: "l3" };
    sk.relations.r3 = { id: "r3", type: "vertical", e: "l2" };
    sk.relations.r4 = { id: "r4", type: "vertical", e: "l4" };
    sk.relations.r5 = { id: "r5", type: "coincident", p: "a", e: "origin" };
    sk.dimensions.d1 = { id: "d1", name: "D1", kind: "distance", refs: ["a", "b"], value: 20, label: [0, 0] };
    sk.dimensions.d2 = { id: "d2", name: "D2", kind: "distance", refs: ["b", "c"], value: 8, label: [0, 0] };
    const a = solveSketch(sk);
    expect(a.status).toBe("full");
    expect(sk.points.c.x).toBeCloseTo(20, 8);
    expect(sk.points.c.y).toBeCloseTo(8, 8);
    expect(a.entityState.l3).toBe("full");
  });

  it("flags redundant relations as over defined", () => {
    const sk = emptySketch();
    rect(sk, 10, 5);
    sk.relations.r1 = { id: "r1", type: "horizontal", e: "l1" };
    sk.relations.r2 = { id: "r2", type: "horizontal", e: "l3" };
    sk.relations.r3 = { id: "r3", type: "vertical", e: "l2" };
    sk.relations.r4 = { id: "r4", type: "vertical", e: "l4" };
    sk.relations.r5 = { id: "r5", type: "parallel", e1: "l1", e2: "l3" };
    const a = solveSketch(sk);
    expect(a.status).toBe("over");
    expect(a.bad.has("r5")).toBe(true);
  });

  it("drags along constraints (fixed-length line swings)", () => {
    const sk = emptySketch();
    sk.points.a = { id: "a", x: 0, y: 0 };
    sk.points.b = { id: "b", x: 10, y: 0 };
    sk.entities.l = { id: "l", type: "line", p1: "a", p2: "b" };
    sk.relations.r = { id: "r", type: "coincident", p: "a", e: "origin" };
    sk.dimensions.d = { id: "d", name: "D1", kind: "distance", refs: ["a", "b"], value: 10, label: [0, 0] };
    const sys = new SketchSystem(sk);
    sys.solve();
    sys.drag([{ id: "b", to: [0, 20] }]);
    sys.commit();
    expect(Math.hypot(sk.points.b.x, sk.points.b.y)).toBeCloseTo(10, 6);
    expect(sk.points.b.y).toBeGreaterThan(9.9);
  });

  it("solves tangent arc + circle radius", () => {
    const sk = emptySketch();
    sk.points.c = { id: "c", x: 1, y: 1 };
    sk.entities.k = { id: "k", type: "circle", c: "c", r: 3 };
    sk.points.a = { id: "a", x: -10, y: 5 };
    sk.points.b = { id: "b", x: 10, y: 5 };
    sk.entities.l = { id: "l", type: "line", p1: "a", p2: "b" };
    sk.relations.t = { id: "t", type: "tangent", e1: "l", e2: "k" };
    sk.dimensions.d = { id: "d", name: "D1", kind: "diameter", refs: ["k"], value: 8, label: [0, 0] };
    const a = solveSketch(sk);
    expect(a.status).not.toBe("unsolved");
    const k = sk.entities.k as { r: number };
    expect(k.r).toBeCloseTo(4, 8);
  });
});

describe("profiles", () => {
  it("finds rectangle with circular hole", () => {
    const sk = emptySketch();
    rect(sk, 10, 10);
    sk.points.cc = { id: "cc", x: 5, y: 5 };
    sk.entities.circ = { id: "circ", type: "circle", c: "cc", r: 2 };
    const r = extractProfiles(sk);
    expect(r.regions.length).toBe(1);
    expect(r.regions[0].holes.length).toBe(1);
    expect(r.regions[0].outer.area).toBeCloseTo(100, 6);
  });

  it("handles a rectangle split by a line (uses outer boundary)", () => {
    const sk = emptySketch();
    rect(sk, 10, 10);
    sk.points.m1 = { id: "m1", x: 5, y: 0 };
    sk.points.m2 = { id: "m2", x: 5, y: 10 };
    // split l1 and l3 at m1/m2
    sk.entities.l1 = { id: "l1", type: "line", p1: "a", p2: "m1" };
    sk.entities.l1b = { id: "l1b", type: "line", p1: "m1", p2: "b" };
    sk.entities.l3 = { id: "l3", type: "line", p1: "c", p2: "m2" };
    sk.entities.l3b = { id: "l3b", type: "line", p1: "m2", p2: "d" };
    sk.entities.mid = { id: "mid", type: "line", p1: "m1", p2: "m2" };
    const r = extractProfiles(sk);
    expect(r.regions.length).toBe(1);
    expect(r.regions[0].outer.area).toBeCloseTo(100, 6);
  });
});
