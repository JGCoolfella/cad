import { describe, expect, it } from "vitest";
import { assemblyRows, scrollToShow, visibleWindow } from "./treeRows";
import type { AssemblyDocument, BuildReply } from "./types";

describe("visibleWindow", () => {
  it("covers the rows in view plus the overscan, clamped to the list", () => {
    expect(visibleWindow(0, 210, 21, 10_000, 5)).toEqual({ start: 0, end: 15 });
    expect(visibleWindow(21 * 1000, 210, 21, 10_000, 5)).toEqual({ start: 995, end: 1015 });
    expect(visibleWindow(21 * 9_995, 210, 21, 10_000, 5)).toEqual({ start: 9_990, end: 10_000 });
    expect(visibleWindow(0, 210, 21, 3, 5)).toEqual({ start: 0, end: 3 });
    // A partly shown row counts.
    expect(visibleWindow(10, 21, 21, 100, 0)).toEqual({ start: 0, end: 2 });
  });
  it("draws something before the view has a height", () => {
    expect(visibleWindow(0, 0, 21, 100, 5).end).toBeGreaterThan(0);
  });
});

describe("scrollToShow", () => {
  it("scrolls as little as needed, and not at all for a row in view", () => {
    expect(scrollToShow(3, 0, 210, 21)).toBeNull();
    expect(scrollToShow(50, 0, 210, 21)).toBe(51 * 21 - 210);
    expect(scrollToShow(2, 21 * 40, 210, 21)).toBe(42);
  });
});

/** A 10 000-instance assembly as the generated scale assembly has it (100 components). */
function scale(n: number): { doc: AssemblyDocument; built: BuildReply } {
  const doc: AssemblyDocument = { format: "opencad-assembly", version: 2, name: "Big", units: "MMGS", components: [], instances: [], mates: [] };
  for (let c = 0; c < 100; c++) doc.components.push({ id: `c${c}`, kind: "part", path: `p${c}.ocpart` } as AssemblyDocument["components"][number]);
  for (let i = 0; i < n; i++) doc.instances.push({ id: `i${i}`, component: `c${i % 100}`, name: `p${i % 100}-${i}`, transform: { position: [i, 0, 0], rotation: [1, 0, 0, 0] }, fixed: i === 0 } as AssemblyDocument["instances"][number]);
  doc.mates.push({ id: "m1", name: "Coincident1", type: "coincident", refs: [{ occurrence: ["i1"], entity: {} }, { occurrence: ["i2"], entity: {} }] } as unknown as AssemblyDocument["mates"][number]);
  const built = {
    gen: 1,
    components: doc.components.map((c) => ({ id: c.id, path: c.path, found: null, part: `h${c.id}`, error: null, warning: null })),
    instances: doc.instances.map((i) => ({ id: i.id, transform: i.transform, state: i.fixed ? "fixed" : "under", free: i.fixed ? 0 : 6 })),
    occurrences: [],
    parts: Object.fromEntries(doc.components.map((c) => [`h${c.id}`, { name: c.id, handle: 1, lightweight: false, errors: [], bbox: null }])),
    mates: [{ id: "m1", status: "ok", message: null, missing: [] }],
    dof: 0,
    converged: true,
    newestError: null,
    heals: [],
    timeMs: 0,
  } as unknown as BuildReply;
  return { doc, built };
}

describe("assemblyRows", () => {
  it("lays the tree out as SOLIDWORKS does: assembly, History, planes, origin, components, Mates", () => {
    const { doc, built } = scale(3);
    const rows = assemblyRows({ doc, built, name: "Big" }, {}, true);
    expect(rows.map((r) => r.id)).toEqual(["$asm", "$history", "$plane:front", "$plane:top", "$plane:right", "$origin", "i0", "i1", "i2", "$mates", "m1"]);
    expect(rows.find((r) => r.id === "i0")!.label).toBe("(f) p0-0 (Default)");
    expect(rows.find((r) => r.id === "i1")!.label).toBe("(-) p1-1 (Default)");
    expect(rows.find((r) => r.id === "m1")!.label).toBe("Coincident1 (p1-1,p2-2)");
  });
  it("expands a component to its planes and origin, and collapses the Mates folder", () => {
    const { doc, built } = scale(3);
    const rows = assemblyRows({ doc, built, name: "Big" }, { i1: true }, false);
    expect(rows.map((r) => r.id)).toEqual(["$asm", "$history", "$plane:front", "$plane:top", "$plane:right", "$origin", "i0", "i1", "i1/$plane:front", "i1/$plane:top", "i1/$plane:right", "i1/$origin", "i2", "$mates"]);
    expect(rows.find((r) => r.id === "i1")!.folder).toBe(true);
    expect(rows.find((r) => r.id === "$mates")!.folder).toBe(false);
  });
  it("lays out 10 000 components in linear time", () => {
    const { doc, built } = scale(10_000);
    const t = performance.now();
    const rows = assemblyRows({ doc, built, name: "Big" }, {}, true);
    expect(rows.length).toBe(10_000 + 8);
    // Quadratic lookups (a find per row) take seconds here; linear ones a few ms.
    expect(performance.now() - t).toBeLessThan(250);
  });
});
