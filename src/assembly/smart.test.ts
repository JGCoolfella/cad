/**
 * SmartMates (G1d): the mate proposed for two entities, as the Mate PropertyManager's
 * defaults; `asm.smartMates` is a command (greyed outside the native app).
 */
import { describe, expect, it } from "vitest";
import { CMD, type Ctx } from "../commands/commands";
import { proposeMate, type MatePick } from "./session";
import type { MateGeom } from "./types";

const pick = (geom: MateGeom, p: [number, number, number], d: [number, number, number] | null): MatePick => ({
  label: "",
  ref: { occurrence: ["i1"], entity: { kind: "face", sig: { surface: "", center: p, normal: [0, 0, 1], area: 1 }, geom } },
  p,
  d,
});

describe("SmartMates proposals", () => {
  it("proposes Concentric for two round faces, aligned as they are", () => {
    const a = pick({ type: "cylinder", p: [0, 0, 0], d: [0, 0, 1], r: 3 }, [0, 0, 0], [0, 0, 1]);
    const b = pick({ type: "cylinder", p: [15, 15, 4], d: [0, 0, -1], r: 3 }, [15, 15, 4], [0, 0, -1]);
    expect(proposeMate([a, b])).toMatchObject({ type: "concentric", align: "antiAligned" });
  });

  it("proposes Coincident for two planes, with the distance between them", () => {
    const a = pick({ type: "plane", p: [0, 0, 3], n: [0, 0, 1] }, [0, 0, 3], [0, 0, 1]);
    const b = pick({ type: "plane", p: [30, 5, 8], n: [0, 0, 1] }, [30, 5, 8], [0, 0, 1]);
    const p = proposeMate([a, b]);
    expect(p.type).toBe("coincident");
    expect(p.align).toBe("aligned");
    expect(p.value).toBeCloseTo(5, 12);
  });

  it("proposes Concentric for an axis and a cylinder", () => {
    const line = pick({ type: "line", p: [0, 0, 0], d: [0, 0, 1] }, [0, 0, 0], [0, 0, 1]);
    const cyl = pick({ type: "cylinder", p: [5, 0, 0], d: [0, 0, 1], r: 2 }, [5, 0, 0], [0, 0, 1]);
    expect(proposeMate([line, cyl]).type).toBe("concentric");
  });

  it("is a command, greyed outside the native app", () => {
    const ctx = { app: { asm: null, asmStack: [], selection: [] }, vp: null, ui: {} } as unknown as Ctx;
    expect(CMD["asm.smartMates"]?.todo).toBeFalsy();
    expect(CMD["asm.smartMates"].enabled?.(ctx)).toBe(false);
  });
});

describe("Interference Detection's Component view (G1d)", () => {
  it("lists each component with the interferences it takes part in, numbered as in the interference view", async () => {
    const { byComponent } = await import("./InterferencePM");
    const r = (a: string, b: string) => ({ a: 0, b: 1, pathA: [a], pathB: [b], volume: 1, coincident: false, center: [0, 0, 0] as [number, number, number], mesh: 0 });
    const shown = [
      { r: r("i1", "i2"), k: 0, ignored: false },
      { r: r("i1", "i3"), k: 1, ignored: false },
    ];
    const out = byComponent(shown, (p) => ({ i1: "plate<1>", i2: "pin<1>", i3: "pin<2>" })[p[0]] ?? p[0]);
    expect(out.map(([n, l]) => [n, l.map((x) => x.n)])).toEqual([
      ["plate<1>", [0, 1]],
      ["pin<1>", [0]],
      ["pin<2>", [1]],
    ]);
  });
});
