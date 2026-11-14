/// <reference types="node" />
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { migrateDocument } from "./migrate";
import { DOC_VERSION, type PartDocument } from "./types";

const v1 = () => ({
  format: "opencad-part",
  version: 1,
  name: "p",
  units: "MMGS",
  rollback: null,
  features: [
    {
      id: "f3",
      name: "Sketch2",
      type: "sketch",
      plane: { kind: "face", sig: { surface: "PLANE", center: [30, 20, 10], normal: [0, 0, 1], area: 2400 } },
      sketch: { points: {}, entities: {}, relations: {}, dimensions: {}, nextDim: 1 },
    },
  ],
});

describe("migrateDocument", () => {
  it("brings a version 1 document to the current version without touching its references", () => {
    const before = v1();
    const doc = migrateDocument(v1());
    expect(doc.version).toBe(DOC_VERSION);
    expect(doc.features).toEqual(before.features);
  });

  it("turns a version 1 origin vertex into the origin kind and leaves other vertices alone", () => {
    const doc = v1() as unknown as { features: unknown[] };
    doc.features.push(
      { id: "f5", name: "Axis1", type: "refaxis", refs: [{ kind: "vertex", p: [0, 0, 0] }, { kind: "vertex", p: [0, -0, 1e-9] }, { kind: "vertex", p: [40, 30, 20] }] },
      { id: "f6", name: "Coordinate System1", type: "coordsys", origin: { kind: "vertex", p: [0, 0, 0] }, xAxis: null, yAxis: null },
      { id: "f7", name: "Boss-Extrude1", type: "extrude", cut: false, sketchId: "f3", reverse: false, merge: true, end1: { type: "upToVertex", depth: 0, vertex: [0, 0, 0] } },
    );
    (doc.features[0] as { sketch: { refs?: unknown } }).sketch.refs = { r1: { id: "r1", kind: "vertex", p: [0, 0, 0] } };
    const out = migrateDocument(doc);
    const [sketch, axis, csys, boss] = out.features;
    expect(axis.type === "refaxis" && axis.refs).toEqual([{ kind: "origin" }, { kind: "vertex", p: [0, -0, 1e-9] }, { kind: "vertex", p: [40, 30, 20] }]);
    expect(csys.type === "coordsys" && csys.origin).toEqual({ kind: "origin" });
    expect(boss.type === "extrude" && boss.end1.vertex).toEqual([0, 0, 0]);
    expect(sketch.type === "sketch" && sketch.sketch.refs).toEqual({ r1: { id: "r1", kind: "vertex", p: [0, 0, 0] } });
    // A version 2 vertex at (0, 0, 0) is a model vertex: not converted.
    const v2 = { ...v1(), version: 2, features: [{ id: "f5", name: "Axis1", type: "refaxis", refs: [{ kind: "vertex", p: [0, 0, 0] }] }] };
    expect(migrateDocument(v2).features[0]).toEqual(v2.features[0]);
  });

  it("keeps the names of a version 2 document", () => {
    const named = v1() as unknown as PartDocument;
    named.version = 2;
    const sketch = named.features[0];
    if (sketch.type !== "sketch" || sketch.plane.kind !== "face") throw new Error("fixture");
    sketch.plane.sig.name = ["f2/end/9f6f5fd734d5dfc0"];
    const doc = migrateDocument(JSON.parse(JSON.stringify(named)));
    expect(doc).toEqual(named);
  });

  it("refuses files from a newer version and files that are not parts", () => {
    expect(() => migrateDocument({ ...v1(), version: DOC_VERSION + 1 })).toThrow(/newer OpenCAD/);
    expect(() => migrateDocument({ ...v1(), format: "something-else" })).toThrow(/Not an OpenCAD part/);
    expect(() => migrateDocument({ ...v1(), version: "1" })).toThrow(/schema version/);
    expect(() => migrateDocument(null)).toThrow(/Not an OpenCAD part/);
  });

  it("reads the every-field fixture of the native document model, names included", () => {
    // The same file `crates/oc-doc` round-trips: both sides agree on the version 2 schema.
    const text = readFileSync(new URL("../../crates/oc-doc/tests/fixtures/all-fields.ocpart", import.meta.url), "utf8");
    const doc = migrateDocument(JSON.parse(text));
    expect(doc.version).toBe(2);
    const sketch = doc.features.find((f) => f.id === "f2");
    expect(sketch?.type === "sketch" && sketch.plane.kind === "face" && sketch.plane.sig.name).toEqual(["f4/end/00f1"]);
    const extrude = doc.features.find((f) => f.id === "f4");
    expect(extrude?.type === "extrude" && extrude.dir2?.vertexName).toHaveLength(3);
  });
});
