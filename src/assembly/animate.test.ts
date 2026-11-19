/** Mate animation (G1d): timing from the option, easing, lerp/slerp, and what animates. */
import { describe, expect, it } from "vitest";
import { animationMs, drawnAt, ease, interpolate, startAnimation } from "./animate";
import type { Transform } from "./types";

const at = (x: number, z = 0, turn = 0): Transform => ({ position: [x, 0, z], rotation: [Math.cos(turn / 2), 0, 0, Math.sin(turn / 2)] });

describe("mate animation", () => {
  it("takes 1 s at the slowest, 0.1 s at the fastest, nothing when Off", () => {
    expect([animationMs(0), animationMs(1), animationMs(5), animationMs(10), animationMs(-3), animationMs(12)]).toEqual([0, 1000, 600, 100, 0, 100]);
  });

  it("eases from rest to rest", () => {
    expect([ease(-1), ease(0), ease(0.5), ease(1), ease(2)]).toEqual([0, 0, 0.5, 1, 1]);
  });

  it("lerps the translation and slerps the rotation the short way", () => {
    const m = interpolate(at(0, 0, 0), at(10, 4, Math.PI / 2), 0.5);
    expect(m.position).toEqual([5, 0, 2]);
    expect(m.rotation[3] / m.rotation[0]).toBeCloseTo(Math.tan(Math.PI / 8), 12);
    // q and −q are the same rotation: the way through the smaller angle.
    const neg: Transform = { position: [0, 0, 0], rotation: [-1, 0, 0, 0] };
    const w = interpolate(at(0, 0, 0.2), neg, 0.5);
    expect(Math.abs(w.rotation[0])).toBeGreaterThan(0.99);
    expect(interpolate(at(1), at(3), 1)).toEqual(at(3));
  });

  it("animates only what moved, ends exactly at the target, and not at all when Off", () => {
    const from = new Map([["i1", at(0)], ["i2", at(5)]]);
    const to = new Map([["i1", at(0)], ["i2", at(5, 10)]]);
    const a = startAnimation(from, to, 1000, 0)!;
    expect([...a.from.keys()]).toEqual(["i2"]);
    expect(drawnAt(a, "i1", 500)).toBeNull();
    expect(drawnAt(a, "i2", 500)?.position).toEqual([5, 0, 5]);
    expect(drawnAt(a, "i2", 1000)).toBeNull();
    expect(startAnimation(from, to, 0, 0)).toBeNull();
    expect(startAnimation(from, from, 1000, 0)).toBeNull();
  });
});
