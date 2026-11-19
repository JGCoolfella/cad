import { describe, expect, it } from "vitest";
import {
  activatePart,
  addConfiguration,
  capturePart,
  configurationTitle,
  configurationsOf,
  deleteConfiguration,
  isSuppressionConfigured,
  isValueConfigured,
  paramKey,
  pruneConfigurations,
  renameConfiguration,
  setConfiguredSuppression,
  setConfiguredValue,
  sketchDimKey,
  sketchState,
  splitKey,
} from "./configurations";
import { newSketchSystem } from "../sketch/solver";
import type { ExtrudeFeature, PartDocument, SketchData, SketchFeature } from "./types";

const solve = (sk: SketchData) => {
  const sys = newSketchSystem(sk);
  if (sys.solve(200)) sys.commit();
};

/** A 20 × 10 block 5 deep with a Ø6 hole at (10, 5) dimensioned from the origin (as the Rust test's). */
function block(): PartDocument {
  const dist = Math.hypot(10, 5);
  return {
    format: "opencad-part",
    version: 3,
    name: "block",
    units: "MMGS",
    rollback: null,
    features: [
      {
        id: "f1",
        name: "Sketch1",
        type: "sketch",
        plane: { kind: "std", plane: "front" },
        sketch: {
          points: { origin: { id: "origin", x: 0, y: 0 }, p1: { id: "p1", x: 10, y: 5 } },
          entities: { c1: { id: "c1", type: "circle", c: "p1", r: 3 } },
          relations: {},
          dimensions: {
            d1: { id: "d1", name: "D1", kind: "diameter", refs: ["c1"], value: 6, label: [14, 9] },
            d2: { id: "d2", name: "D2", kind: "distance", refs: ["origin", "p1"], value: dist, label: [4, 4] },
          },
          nextDim: 3,
        },
      },
      { id: "f2", name: "Boss-Extrude1", type: "extrude", cut: false, sketchId: "f1", reverse: false, merge: true, end1: { type: "blind", depth: 5 } },
      { id: "f3", name: "Fillet1", type: "fillet", radius: 1, edges: [] },
    ],
  } as PartDocument;
}

const sketch = (d: PartDocument) => (d.features[0] as SketchFeature).sketch;
const depth = (d: PartDocument) => (d.features[1] as ExtrudeFeature).end1.depth;
const clone = <T>(x: T): T => JSON.parse(JSON.stringify(x));

describe("configurations", () => {
  it("keys address feature parameters and sketch dimensions", () => {
    expect(paramKey("f2", "end1.depth")).toBe("f2/end1.depth");
    expect(sketchDimKey("f1", "d2")).toBe("f1/sketch.dimensions.d2.value");
    expect(splitKey("f1/sketch.dimensions.d2.value")).toEqual(["f1", ["sketch", "dimensions", "d2", "value"]]);
    expect(splitKey("nodelimiter")).toBeNull();
    expect(splitKey("f1/a..b")).toBeNull();
  });

  it("a document without configurations has one, Default", () => {
    const d = block();
    expect(configurationsOf(d).list.map((c) => c.name)).toEqual(["Default"]);
    expect(configurationTitle(d)).toBe("block (Default)");
    expect(() => deleteConfiguration(d, "c1")).toThrow(/only configuration/);
  });

  it("This configuration configures a value; activating writes it and re-solves the sketch; back is bit for bit", () => {
    const d = block();
    const original = clone(d);
    // Add Long (active), make it 12 deep and the hole Ø4 and 6 from the origin, in Long only.
    addConfiguration(d, "Long", "deeper", () => capturePart(d));
    expect(d.configurations!.active).toBe("c2");
    expect(configurationTitle(d)).toBe("block (Long)");
    const edit = (key: string, before: number, v: number, set: () => void) => {
      setConfiguredValue(d, key, before, v, "this", key.startsWith("f1/") ? { id: "f1", state: sketchState(sketch(d)) } : undefined);
      set();
    };
    edit(paramKey("f2", "end1.depth"), 5, 12, () => ((d.features[1] as ExtrudeFeature).end1.depth = 12));
    edit(sketchDimKey("f1", "d1"), 6, 4, () => (sketch(d).dimensions.d1.value = 4));
    edit(sketchDimKey("f1", "d2"), Math.hypot(10, 5), 6, () => (sketch(d).dimensions.d2.value = 6));
    solve(sketch(d));
    expect(isValueConfigured(d, "f2/end1.depth")).toBe(true);
    expect(d.configurations!.list[0].values!["f2/end1.depth"]).toBe(5);
    const long = clone(d);
    // Default: the original features, bit for bit (its solution written back, no solver step).
    const changed = activatePart(d, "Default", solve);
    expect(changed.map((f) => f.id).sort()).toEqual(["f1", "f2"]);
    expect(d.features).toEqual(original.features);
    expect(depth(d)).toBe(5);
    // Long again: the same Long.
    activatePart(d, "c2", solve);
    expect(d.features).toEqual(long.features);
    const p = sketch(d).points.p1;
    expect(Math.hypot(p.x, p.y)).toBeCloseTo(6, 9);
    const c = sketch(d).entities.c1;
    expect(c.type === "circle" && c.r).toBeCloseTo(2, 9);
    expect(() => activatePart(d, "Nope", solve)).toThrow(/No configuration/);
  });

  it("an edit made in a configuration (PropertyManager, sketch) stays with it", () => {
    const d = block();
    addConfiguration(d, "Long", undefined, () => capturePart(d));
    setConfiguredValue(d, "f2/end1.depth", 5, 12, "this");
    (d.features[1] as ExtrudeFeature).end1.depth = 12;
    // Later the PropertyManager sets 20 (no scope): Long keeps it when left.
    (d.features[1] as ExtrudeFeature).end1.depth = 20;
    activatePart(d, "Default", solve);
    expect(depth(d)).toBe(5);
    activatePart(d, "Long", solve);
    expect(depth(d)).toBe(20);
    // A parameter nobody configured is shared: the fillet radius set in Long is Default's too.
    (d.features[2] as { radius: number }).radius = 2;
    activatePart(d, "Default", solve);
    expect((d.features[2] as { radius: number }).radius).toBe(2);
  });

  it("All configurations sets a value everywhere", () => {
    const d = block();
    addConfiguration(d, "Long", undefined, () => capturePart(d));
    setConfiguredValue(d, "f2/end1.depth", 5, 12, "this");
    (d.features[1] as ExtrudeFeature).end1.depth = 12;
    setConfiguredValue(d, "f2/end1.depth", 12, 8, "all");
    (d.features[1] as ExtrudeFeature).end1.depth = 8;
    expect(isValueConfigured(d, "f2/end1.depth")).toBe(false);
    activatePart(d, "Default", solve);
    expect(depth(d)).toBe(8);
  });

  it("suppression per configuration", () => {
    const d = block();
    addConfiguration(d, "NoFillet", undefined, () => capturePart(d));
    setConfiguredSuppression(d, "f3", false, true, "this");
    d.features[2].suppressed = true;
    expect(isSuppressionConfigured(d, "f3")).toBe(true);
    activatePart(d, "Default", solve);
    expect(d.features[2].suppressed).toBeUndefined();
    activatePart(d, "NoFillet", solve);
    expect(d.features[2].suppressed).toBe(true);
    // All configurations: unsuppressed everywhere.
    setConfiguredSuppression(d, "f3", true, false, "all");
    delete d.features[2].suppressed;
    activatePart(d, "Default", solve);
    activatePart(d, "NoFillet", solve);
    expect(d.features[2].suppressed).toBeUndefined();
  });

  it("add, rename and delete follow SOLIDWORKS' rules", () => {
    const d = block();
    expect(() => addConfiguration(d, "Default")).toThrow(/already exists/);
    expect(() => addConfiguration(d, "  ")).toThrow(/name/);
    addConfiguration(d, "A");
    addConfiguration(d, "B");
    expect(d.configurations!.list.map((c) => c.id)).toEqual(["c1", "c2", "c3"]);
    expect(() => renameConfiguration(d, "c2", "B")).toThrow(/already exists/);
    renameConfiguration(d, "c2", "Short");
    expect(() => deleteConfiguration(d, "c3")).toThrow(/active/);
    deleteConfiguration(d, "c2");
    expect(d.configurations!.list.map((c) => c.name)).toEqual(["Default", "B"]);
    // Entries of deleted features are dropped.
    setConfiguredSuppression(d, "f3", false, true, "this");
    pruneConfigurations(d, (id) => id !== "f3");
    expect(isSuppressionConfigured(d, "f3")).toBe(false);
  });
});
