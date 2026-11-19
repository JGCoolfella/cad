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
