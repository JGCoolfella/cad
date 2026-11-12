import { beforeAll, describe, expect, it } from "vitest";
import type { EdgeSig, ExtrudeFeature, Feature, SketchData, SketchFeature } from "../core/types";
import { emptySketch } from "../sketch/model";
import { clearCache, finalShape, handle, massProps, rebuild } from "./engine";
import { loadKernel } from "./testing";

beforeAll(loadKernel, 60_000);

/** Closed rectangle [0,w]×[0,h] drawn with four lines. */
function rect(sk: SketchData, w: number, h: number) {
  const pts = [["a", 0, 0], ["b", w, 0], ["c", w, h], ["d", 0, h]] as const;
  for (const [id, x, y] of pts) sk.points[id] = { id, x, y };
  const ls = [["l1", "a", "b"], ["l2", "b", "c"], ["l3", "c", "d"], ["l4", "d", "a"]] as const;
  for (const [id, p1, p2] of ls) sk.entities[id] = { id, type: "line", p1, p2 };
}

function sketch(id: string, fill: (sk: SketchData) => void): SketchFeature {
  const sk = emptySketch();
  fill(sk);
  return { id, name: id, type: "sketch", plane: { kind: "std", plane: "front" }, sketch: sk };
}

function extrude(id: string, sketchId: string, o: Partial<ExtrudeFeature>): ExtrudeFeature {
  return { id, name: id, type: "extrude", cut: false, sketchId, reverse: false, merge: true, end1: { type: "blind", depth: 10 }, ...o };
}

/** 20×10×5 block with a Ø6 hole cut through all (both directions). */
function boxWithHole(): Feature[] {
  return [
    sketch("s1", (sk) => rect(sk, 20, 10)),
    extrude("e1", "s1", { end1: { type: "blind", depth: 5 } }),
    sketch("s2", (sk) => {
      sk.points.c = { id: "c", x: 10, y: 5 };
      sk.entities.k = { id: "k", type: "circle", c: "c", r: 3 };
    }),
    extrude("cut1", "s2", { cut: true, end1: { type: "throughAllBoth", depth: 0 } }),
  ];
}

const VOLUME = 20 * 10 * 5 - Math.PI * 3 * 3 * 5;

describe("kernel engine (Node)", () => {
  it("builds a box with a through hole", () => {
    clearCache();
    const r = rebuild(boxWithHole());
    expect(r.statuses.filter((s) => s.error)).toEqual([]);
    expect(r.mesh).not.toBeNull();
    expect(r.mesh!.faces.length).toBe(7);
    const [lo, hi] = r.mesh!.bbox!;
    expect(lo.map((x) => +x.toFixed(6))).toEqual([0, 0, 0]);
    expect(hi.map((x) => +x.toFixed(6))).toEqual([20, 10, 5]);
    const v = massProps(finalShape(boxWithHole())!).volume;
    expect(Math.abs(v - VOLUME) / VOLUME).toBeLessThan(1e-6);
  });

  it("gives the same result from a warm cache", () => {
    clearCache();
    const a = rebuild(boxWithHole());
    const b = rebuild(boxWithHole());
    expect(b.mesh!.faces.map((f) => f.sig)).toEqual(a.mesh!.faces.map((f) => f.sig));
    expect(b.statuses).toEqual(a.statuses);
  });

  it("reports a failing feature without aborting the rebuild", () => {
    clearCache();
    const fs = boxWithHole();
    (fs[1] as ExtrudeFeature).end1 = { type: "upToVertex", depth: 0 };
    const r = rebuild(fs);
    expect(r.statuses.find((s) => s.id === "e1")?.error).toMatch(/vertex/i);
    expect(r.statuses).toHaveLength(4);
  });

  it("re-solves sketches against moved model references", () => {
    clearCache();
    const top: EdgeSig = { curve: "LINE", a: [0, 10, 0], b: [20, 10, 0], mid: [10, 10, 0], length: 20 };
    const fs = (w: number): Feature[] => [
      sketch("s1", (sk) => rect(sk, w, 10)),
      extrude("e1", "s1", { end1: { type: "blind", depth: 5 } }),
      sketch("s2", (sk) => {
        // A circle centred on the model corner (20,10,0) and one at the midpoint of the top edge.
        sk.refs = { xv: { id: "xv", kind: "vertex", p: [20, 10, 0] }, xe: { id: "xe", kind: "edge", sig: top } };
        sk.points.v = { id: "v", x: 20, y: 10, ext: "xv" };
        sk.points.ea = { id: "ea", x: 0, y: 10, ext: "xe" };
        sk.points.eb = { id: "eb", x: 20, y: 10, ext: "xe" };
        sk.entities.el = { id: "el", type: "line", p1: "ea", p2: "eb", ext: "xe" };
        sk.entities.c1 = { id: "c1", type: "circle", c: "v", r: 2 };
        sk.points.m = { id: "m", x: 10, y: 10, standalone: true };
        sk.relations.r1 = { id: "r1", type: "midpoint", p: "m", e: "el" };
        sk.entities.c2 = { id: "c2", type: "circle", c: "m", r: 1 };
      }),
      extrude("cut", "s2", { cut: true, end1: { type: "throughAllBoth", depth: 0 } }),
    ];
    const r = rebuild(fs(30));
    expect(r.statuses.filter((s) => s.error || s.warning)).toEqual([]);
    const s2 = r.sketches!.s2;
    expect([s2.points.v.x, s2.points.v.y]).toEqual([30, 10]);
    expect([s2.points.m.x, s2.points.m.y].map((x) => +x.toFixed(9))).toEqual([15, 10]);
    // Both circles cut half/quarter discs out of the top edge of the widened block.
    const v = massProps(finalShape(fs(30))!).volume;
    const expected = 30 * 10 * 5 - (Math.PI * 4) / 4 * 5 - (Math.PI * 1) / 2 * 5;
    expect(Math.abs(v - expected) / expected).toBeLessThan(1e-6);
  });

  describe("patterning individual features", () => {
    const xEdge: EdgeSig = { curve: "LINE", a: [0, 0, 0], b: [60, 0, 0], mid: [30, 0, 0], length: 60 };
    // 60×20×5 plate; a 4×4 boss standing 5 above it; a R1 fillet on the boss's outer vertical edge.
    const parts = (seeds: string[]): Feature[] => [
      sketch("s1", (sk) => rect(sk, 60, 20)),
      extrude("e1", "s1", { end1: { type: "blind", depth: 5 } }),
      sketch("s2", (sk) => {
        rect(sk, 4, 4);
        for (const p of Object.values(sk.points)) if (p.id !== "o") (p.x += 2), (p.y += 8);
      }),
      extrude("e2", "s2", { end1: { type: "blind", depth: 10 } }),
      { id: "f1", name: "Fillet1", type: "fillet", radius: 1, faces: [], edges: [{ curve: "LINE", a: [6, 12, 5], b: [6, 12, 10], mid: [6, 12, 7.5], length: 5 }] },
      { id: "p1", name: "LPattern1", type: "linearPattern", seeds, dir1: { ref: { kind: "edge", sig: xEdge }, reverse: false, spacing: 15, count: 3 } },
    ];
    const plate = 60 * 20 * 5, boss = 4 * 4 * 5, round = (1 - Math.PI / 4) * 5;
    const vol = (fs: Feature[]) => massProps(finalShape(fs)!).volume;

    it("patterns a feature on its own, leaving out the fillet", () => {
      clearCache();
      const fs = parts(["e2"]);
      expect(rebuild(fs).statuses.filter((s) => s.error || s.warning)).toEqual([]);
      const expected = plate + 3 * boss - round;
      expect(Math.abs(vol(fs) - expected) / expected).toBeLessThan(1e-6);
    });

    it("re-applies a fillet to each instance of its parent, in tree order whatever the pick order", () => {
      clearCache();
      const fs = parts(["f1", "e2"]);
      const r = rebuild(fs);
      expect(r.statuses.filter((s) => s.error || s.warning)).toEqual([]);
      const expected = plate + 3 * (boss - round);
      expect(Math.abs(vol(fs) - expected) / expected).toBeLessThan(1e-6);
    });

    it("warns when a fillet is patterned without its parent", () => {
      clearCache();
      const r = rebuild(parts(["f1"]));
      const st = r.statuses.find((s) => s.id === "p1")!;
      expect(st.error).toBeUndefined();
      expect(st.warning).toMatch(/2 fillet\/chamfer instances/);
    });

    // Plane x = 30 and axis x = 30, y = 10 (along Z): the plate's middle.
    const mid: Feature[] = [
      { id: "px", name: "PlaneX", type: "refplane", base: { kind: "std", plane: "right" }, offset: 30, flip: false },
      { id: "py", name: "PlaneY", type: "refplane", base: { kind: "std", plane: "top" }, offset: 10, flip: false },
      { id: "ax", name: "Axis1", type: "refaxis", refs: [{ kind: "plane", plane: { kind: "refplane", featureId: "px" } }, { kind: "plane", plane: { kind: "refplane", featureId: "py" } }] },
    ];

    it("re-applies a fillet in a circular pattern", () => {
      clearCache();
      const fs = [...mid, ...parts([]).slice(0, 5), { id: "p1", name: "CirPattern1", type: "circularPattern", seeds: ["e2", "f1"], axis: { kind: "axis", featureId: "ax" }, reverse: false, angle: 2 * Math.PI, count: 2, equalSpacing: true } as Feature];
      expect(rebuild(fs).statuses.filter((s) => s.error || s.warning)).toEqual([]);
      const expected = plate + 2 * (boss - round);
      expect(Math.abs(vol(fs) - expected) / expected).toBeLessThan(1e-6);
    });

    it("re-applies a fillet in a mirror", () => {
      clearCache();
      const fs = [...mid, ...parts([]).slice(0, 5), { id: "p1", name: "Mirror1", type: "mirror", seeds: ["e2", "f1"], plane: { kind: "plane", plane: { kind: "refplane", featureId: "px" } } } as Feature];
      expect(rebuild(fs).statuses.filter((s) => s.error || s.warning)).toEqual([]);
      const expected = plate + 2 * (boss - round);
      expect(Math.abs(vol(fs) - expected) / expected).toBeLessThan(1e-6);
    });

    it("names a feature that cannot be patterned", () => {
      clearCache();
      const r = rebuild(parts(["s2"]));
      expect(r.statuses.find((s) => s.id === "p1")?.error).toMatch(/s2 cannot be patterned/);
    });
  });

  it("handles requests like the worker does", async () => {
    clearCache();
    const out = await handle({ id: 1, op: "massProps", features: boxWithHole() });
    expect(Math.abs((out.result as { volume: number }).volume - VOLUME) / VOLUME).toBeLessThan(1e-6);
    const rb = await handle({ id: 2, op: "rebuild", features: boxWithHole() });
    expect(rb.transfer).toHaveLength(4);
  });
});
