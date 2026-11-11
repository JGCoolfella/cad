import { describe, expect, it } from "vitest";
import type { SketchData } from "../core/types";
import { emptySketch, P, arcGeom } from "./model";
import { solveSketch, SketchSystem } from "./solver";
import { chamferCorner, circularPattern, extendEntity, linearPattern, mirrorEntities, offsetEntities, transformEntities, xf } from "./ops";

function rect(): SketchData {
  const sk = emptySketch();
  const pts: [string, number, number][] = [["a", 0, 0], ["b", 40, 0], ["c", 40, 20], ["d", 0, 20]];
  for (const [id, x, y] of pts) sk.points[id] = { id, x, y };
  [["l1", "a", "b"], ["l2", "b", "c"], ["l3", "c", "d"], ["l4", "d", "a"]].forEach(([id, p1, p2]) => (sk.entities[id] = { id, type: "line", p1, p2 }));
  return sk;
}

describe("offsetEntities", () => {
  it("offsets a closed rectangle outward by the distance, fully tied to the original", () => {
    const sk = rect();
    // Fix the original so only the offset's DOF remain.
    sk.relations.fx = { id: "fx", type: "fix", target: "l1" };
    sk.relations.fx2 = { id: "fx2", type: "fix", target: "l3" };
    const r = offsetEntities(sk, ["l1"], { distance: 5 });
    expect(r.error).toBeUndefined();
    expect(r.created.length).toBe(4);
    const xs = Object.values(sk.points).map((p) => p.x);
    expect(Math.min(...xs)).toBeCloseTo(-5);
    expect(Math.max(...xs)).toBeCloseTo(45);
    const a = solveSketch(sk);
    for (const id of r.created) expect(a.entityState[id]).toBe("full");
  });

  it("offset follows when the dimension changes", () => {
    const sk = rect();
    for (const id of ["l1", "l2", "l3", "l4"]) sk.relations["f" + id] = { id: "f" + id, type: "fix", target: id };
    offsetEntities(sk, ["l1"], { distance: 5 });
    const dim = Object.values(sk.dimensions).find((d) => d.kind === "lineLine")!;
    dim.value = 8;
    const sys = new SketchSystem(sk);
    expect(sys.solve(200)).toBe(true);
    sys.commit();
    const xs = Object.values(sk.points).map((p) => p.x);
    expect(Math.min(...xs)).toBeCloseTo(-8, 5);
    expect(Math.max(...xs)).toBeCloseTo(48, 5);
  });

  it("offsets a circle outward with a concentric relation", () => {
    const sk = emptySketch();
    sk.points.c = { id: "c", x: 0, y: 0 };
    sk.entities.k = { id: "k", type: "circle", c: "c", r: 10 };
    const r = offsetEntities(sk, ["k"], { distance: 3 });
    const k2 = sk.entities[r.created[0]];
    expect(k2.type === "circle" && k2.r).toBeCloseTo(13);
    expect(Object.values(sk.relations).some((x) => x.type === "concentric")).toBe(true);
  });

  it("reports when an arc would vanish", () => {
    const sk = emptySketch();
    sk.points.c = { id: "c", x: 0, y: 0 };
    sk.points.p = { id: "p", x: 2, y: 0 };
    sk.points.q = { id: "q", x: 0, y: 2 };
    sk.entities.ar = { id: "ar", type: "arc", c: "c", p1: "p", p2: "q" };
    // Arc alone (open): left of CCW travel is towards the centre, so +5 shrinks below zero.
    expect(offsetEntities(sk, ["ar"], { distance: 5 }).error).toBeTruthy();
  });
});

describe("mirror / transform / patterns", () => {
  it("mirrors with symmetric relations and keeps the sketch solvable", () => {
    const sk = rect();
    sk.points.m1 = { id: "m1", x: -10, y: -10 };
    sk.points.m2 = { id: "m2", x: -10, y: 30 };
    sk.entities.ax = { id: "ax", type: "line", p1: "m1", p2: "m2", construction: true };
    const r = mirrorEntities(sk, ["l1", "l2", "l3", "l4"], "ax");
    expect(r.created.length).toBe(4);
    const xs = r.created.flatMap((id) => {
      const e = sk.entities[id];
      return e.type === "line" ? [P(sk, e.p1)[0], P(sk, e.p2)[0]] : [];
    });
    expect(Math.min(...xs)).toBeCloseTo(-60);
    expect(Math.max(...xs)).toBeCloseTo(-20);
    expect(Object.values(sk.relations).filter((x) => x.type === "symmetric").length).toBe(4);
    expect(new SketchSystem(sk).solve()).toBe(true);
  });

  it("rotates a copy about a point", () => {
    const sk = rect();
    const r = transformEntities(sk, ["l1"], xf.rotate([0, 0], Math.PI / 2), true);
    const l = sk.entities[r.created[0]];
    expect(l.type === "line" && P(sk, l.p2)[1]).toBeCloseTo(40);
  });

  it("linear pattern makes count-1 copies per direction", () => {
    const sk = emptySketch();
    sk.points.c = { id: "c", x: 0, y: 0 };
    sk.entities.k = { id: "k", type: "circle", c: "c", r: 2 };
    const r = linearPattern(sk, ["k"], { angle1: 0, spacing1: 10, count1: 4, angle2: Math.PI / 2, spacing2: 5, count2: 2 });
    expect(r.created.length).toBe(7);
  });

  it("circular pattern spreads instances over 360 degrees", () => {
    const sk = emptySketch();
    sk.points.c = { id: "c", x: 10, y: 0 };
    sk.entities.k = { id: "k", type: "circle", c: "c", r: 1 };
    const r = circularPattern(sk, ["k"], { center: [0, 0], angle: 2 * Math.PI, count: 4 });
    expect(r.created.length).toBe(3);
    const c3 = sk.entities[r.created[1]];
    expect(c3.type === "circle" && P(sk, c3.c)[0]).toBeCloseTo(-10);
  });
});

describe("extend / chamfer", () => {
  it("extends a line to the next entity", () => {
    const sk = emptySketch();
    for (const [id, x, y] of [["a", 0, 0], ["b", 5, 0], ["c", 10, -5], ["d", 10, 5]] as const) sk.points[id] = { id, x, y };
    sk.entities.l = { id: "l", type: "line", p1: "a", p2: "b" };
    sk.entities.w = { id: "w", type: "line", p1: "c", p2: "d" };
    expect(extendEntity(sk, "l", [5, 0])).toBe(true);
    expect(sk.points.b.x).toBeCloseTo(10);
  });

  it("chamfers a corner with equal distances", () => {
    const sk = rect();
    const r = chamferCorner(sk, "l1", "l2", "b", { type: "distanceDistance", d1: 4, d2: 4, equal: true });
    expect(r.error).toBeUndefined();
    const l = sk.entities[r.created!];
    expect(l.type === "line" && Math.hypot(P(sk, l.p1)[0] - P(sk, l.p2)[0], P(sk, l.p1)[1] - P(sk, l.p2)[1])).toBeCloseTo(Math.hypot(4, 4));
    void arcGeom;
  });
});
