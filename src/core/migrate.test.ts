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
