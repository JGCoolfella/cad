/**
 * Assembly commands (G1a) are native-only: outside the native app (browser, Electron, here)
 * they are greyed out; unimplemented SOLIDWORKS assembly commands stay visible as todo.
 */
import { describe, expect, it } from "vitest";
import { CMD, type Ctx } from "../commands/commands";
import { selKey } from "../app/app";
import { relative } from "./session";
import { apply, applyInverse, type Transform } from "./types";

describe("assembly commands", () => {
  const ctx = { app: { asm: { matePM: null, doc: { instances: [{ id: "i1" }, { id: "i2" }], mates: [] } }, selection: [] }, vp: null, ui: {} } as unknown as Ctx;

  it("are greyed out outside the native app", () => {
    for (const id of ["file.newAssembly", "asm.insertComponent", "asm.mate", "asm.fix", "asm.float", "asm.suppress", "asm.delete"]) {
      expect(CMD[id], id).toBeDefined();
      expect(CMD[id].enabled?.(ctx), id).toBe(false);
    }
  });

  it("keeps unimplemented SOLIDWORKS assembly commands visible as todo", () => {
    for (const id of ["asm.componentPattern", "asm.smartFasteners", "asm.moveComponent", "asm.rotateComponent", "asm.explodedView", "asm.interference", "asm.openPart", "asm.bom"]) {
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
