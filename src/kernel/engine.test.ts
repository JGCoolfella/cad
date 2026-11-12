import { beforeAll, describe, expect, it } from "vitest";
import type { ExtrudeFeature, Feature, SketchData, SketchFeature } from "../core/types";
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

  it("handles requests like the worker does", async () => {
    clearCache();
    const out = await handle({ id: 1, op: "massProps", features: boxWithHole() });
    expect(Math.abs((out.result as { volume: number }).volume - VOLUME) / VOLUME).toBeLessThan(1e-6);
    const rb = await handle({ id: 2, op: "rebuild", features: boxWithHole() });
    expect(rb.transfer).toHaveLength(4);
  });
});
