/**
 * Assembly commands (G1a) are native-only: outside the native app (browser, Electron, here)
 * they are greyed out; unimplemented SOLIDWORKS assembly commands stay visible as todo.
 */
import { describe, expect, it } from "vitest";
import { CMD, type Ctx } from "../commands/commands";
import { selKey } from "../app/app";
import { compose, inverse, relative, rotated, treeRef } from "./session";
import { apply, applyInverse, validMates, type MateGeom, type Transform } from "./types";

describe("assembly commands", () => {
  const ctx = { app: { asm: { matePM: null, movePM: null, inPM: false, placing: null, doc: { instances: [{ id: "i1" }, { id: "i2" }], mates: [] } }, asmStack: [{}], selection: [{ kind: "feature", id: "i1" }] }, vp: null, ui: {} } as unknown as Ctx;

  it("are greyed out outside the native app", () => {
    for (const id of [
      "file.newAssembly",
      "asm.insertComponent",
      "asm.mate",
      "asm.fix",
      "asm.float",
      "asm.suppress",
      "asm.delete",
      "asm.openPart",
      "asm.returnToAssembly",
      "asm.moveComponent",
      "asm.rotateComponent",
      "asm.hide",
      "asm.appearance",
      "asm.rename",
      "asm.interference",
    ]) {
      expect(CMD[id], id).toBeDefined();
      expect(CMD[id].enabled?.(ctx), id).toBe(false);
    }
  });

  it("keeps unimplemented SOLIDWORKS assembly commands visible as todo", () => {
    for (const id of ["asm.componentPattern", "asm.smartFasteners", "asm.showHidden", "asm.explodedView", "asm.newPart", "asm.bom"]) {
      expect(CMD[id]?.todo, id).toBe(true);
    }
  });
});

describe("assembly selection and placements", () => {
  it("keys faces of different occurrences apart", () => {
    expect(selKey({ kind: "face", id: 3, body: 1 })).not.toBe(selKey({ kind: "face", id: 3, body: 2 }));
    expect(selKey({ kind: "face", id: 3 })).toBe("face:3");
  });

  it("relative placement round-trips", () => {
    const s = Math.SQRT1_2;
    const a: Transform = { position: [10, 0, 5], rotation: [s, 0, 0, s] };
    const b: Transform = { position: [3, 4, 5], rotation: [s, s, 0, 0] };
    const r = relative(a, b);
    // b = a ∘ r: a point of b's frame lands in the same place either way.
    const p: [number, number, number] = [1, 2, 3];
    const viaA = apply(a, apply(r, p));
    const direct = apply(b, p);
    viaA.forEach((x, k) => expect(x).toBeCloseTo(direct[k], 12));
    applyInverse(a, apply(a, p)).forEach((x, k) => expect(x).toBeCloseTo(p[k], 12));
  });
});

describe("Mate pop-up toolbar: the mates a pair allows", () => {
  const plane: MateGeom = { type: "plane", p: [0, 0, 0], n: [0, 0, 1] };
  const cyl: MateGeom = { type: "cylinder", p: [0, 0, 0], d: [0, 0, 1], r: 3 };
  const point: MateGeom = { type: "point", p: [1, 2, 3] };
  const sphere: MateGeom = { type: "sphere", c: [0, 0, 0], r: 2 };
  it("follows the solver's rule table", () => {
    expect(validMates(plane, plane)).toEqual(["coincident", "parallel", "perpendicular", "lock", "distance", "angle"]);
    expect(validMates(cyl, cyl)).toEqual(["parallel", "perpendicular", "tangent", "concentric", "lock", "distance", "angle"]);
    expect(validMates(plane, cyl)).toEqual(["parallel", "perpendicular", "tangent", "lock", "distance", "angle"]);
    expect(validMates(cyl, plane)).toEqual(validMates(plane, cyl));
    expect(validMates(point, point)).toEqual(["coincident", "lock", "distance"]);
    expect(validMates(sphere, plane)).toEqual(["tangent", "lock", "distance"]);
  });
});

describe("fly-out FeatureManager picks", () => {
  it("names the assembly's and the components' planes and origins", () => {
    expect(treeRef("$plane:front")).toEqual({ occurrence: [], entity: { kind: "plane", plane: "front" } });
    expect(treeRef("i2/$plane:right")).toEqual({ occurrence: ["i2"], entity: { kind: "plane", plane: "right" } });
    expect(treeRef("$origin")).toEqual({ occurrence: [], entity: { kind: "origin" } });
    expect(treeRef("i3/$origin")).toEqual({ occurrence: ["i3"], entity: { kind: "origin" } });
    expect(treeRef("$mates")).toBeNull();
    expect(treeRef("i1")).toBeNull();
  });
});

describe("Move and Rotate Component placements", () => {
  it("turns about a line through a point", () => {
    const t: Transform = { position: [10, 0, 0], rotation: [1, 0, 0, 0] };
    const r = rotated(t, [0, 0, 0], [0, 0, 1], Math.PI / 2);
    r.position.forEach((x, k) => expect(x).toBeCloseTo([0, 10, 0][k], 12));
    apply(r, [1, 0, 0]).forEach((x, k) => expect(x).toBeCloseTo([0, 11, 0][k], 12));
    // About a line through (5, 0, 0): the point on the line stays.
    const q = rotated(t, [5, 0, 0], [0, 0, 1], 1.234);
    apply(q, [-5, 0, 0]).forEach((x, k) => expect(x).toBeCloseTo([5, 0, 0][k], 12));
  });

  it("composes and inverts", () => {
    const s = Math.SQRT1_2;
    const a: Transform = { position: [1, 2, 3], rotation: [s, 0, s, 0] };
    const b: Transform = { position: [-4, 0, 2], rotation: [s, 0, 0, s] };
    const p: [number, number, number] = [0.5, -2, 7];
    apply(compose(a, b), p).forEach((x, k) => expect(x).toBeCloseTo(apply(a, apply(b, p))[k], 12));
    apply(compose(inverse(a), a), p).forEach((x, k) => expect(x).toBeCloseTo(p[k], 12));
  });
});
