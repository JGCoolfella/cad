/// <reference types="node" />
/**
 * Parts of the assembly corpus (corpus/assemblies/, OVERHAUL_PLAN G1): built with the
 * corpus builder like the golden parts, stored as .ocpart files the assemblies refer to.
 * The assemblies themselves (.ocasm, with mates picked by persistent name) are written by
 * the native engine: `UPDATE_GOLDEN=1 cargo test -p oc-asm --test corpus`.
 *
 * Re-record the parts after an intended change:   UPDATE_GOLDEN=1 npx vitest run assemblyParts
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import type { PartDocument } from "../../core/types";
import { loadKernel } from "../testing";
import { PartBuilder, STD } from "./build";
import { CORPUS_DIR, blind, extrude } from "./parts";

const UPDATE = process.env.UPDATE_GOLDEN === "1";
const DIR = new URL("assemblies/", CORPUS_DIR);
const read = (u: URL) => readFileSync(u, "utf8").replace(/\r\n/g, "\n");
const docText = (d: PartDocument) => JSON.stringify(d, null, 1) + "\n";
const through = { type: "throughAllBoth" as const, depth: 0 };

/** Units: mm. Every part is extruded from the Front plane (XY) along +Z. */
const PARTS: { name: string; build(b: PartBuilder): void }[] = [
  {
    // A 60 × 30 × 8 plate with two Ø6 holes, 30 apart.
    name: "plate",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 60, 30)), blind(8));
      extrude(b, b.sketch(STD.front, (d) => { d.circle(15, 15, 3); d.circle(45, 15, 3); }), through, { cut: true });
    },
  },
  {
    // A pin along Z: Ø10 head from z = 0 to 3, Ø6 shank from z = 0 down to -20.
    name: "pin",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.circle(0, 0, 5)), blind(3));
      extrude(b, b.sketch(STD.front, (d) => d.circle(0, 0, 3)), blind(20), { reverse: true });
    },
  },
  {
    // A 50 × 10 × 6 arm with a Ø6 hole near one end.
    name: "arm",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 50, 10)), blind(6));
      extrude(b, b.sketch(STD.front, (d) => d.circle(5, 5, 3)), through, { cut: true });
    },
  },
];

/** Faces of each part (catches a cut that misses the body, which is not an error). */
const FACES: Record<string, number> = { plate: 8, pin: 5, arm: 7 };

beforeAll(async () => {
  await loadKernel();
  mkdirSync(DIR, { recursive: true });
}, 60_000);

describe("assembly corpus parts", () => {
  for (const part of PARTS) {
    it(part.name, () => {
      const b = new PartBuilder(part.name);
      part.build(b);
      expect(b.mesh().faces.length).toBe(FACES[part.name]);
      const file = new URL(`${part.name}.ocpart`, DIR);
      if (UPDATE || !existsSync(file)) {
        if (!UPDATE) throw new Error(`corpus/assemblies/${part.name}.ocpart is missing. Record it with UPDATE_GOLDEN=1.`);
        writeFileSync(file, docText(b.doc));
      }
      expect(docText(b.doc)).toBe(read(file));
    }, 60_000);
  }
});
