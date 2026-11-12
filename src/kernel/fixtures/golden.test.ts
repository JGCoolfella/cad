/// <reference types="node" />
/**
 * Golden corpus: every part in parts.ts is built, saved to corpus/<part>.ocpart (opens
 * in the app) and measured after every feature into corpus/<part>.json. These files are
 * the parity oracle for the native engine (docs/OVERHAUL_PLAN.md).
 *
 * Re-record after an intended change:   UPDATE_GOLDEN=1 npx vitest run golden
 * Never re-record to make a regression pass; see the plan's session protocol.
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import type { EndConditionType, Feature, FeatureType, PartDocument } from "../../core/types";
import { loadKernel } from "../testing";
import { PartBuilder, measure } from "./build";
import { CORPUS_DIR, PARTS } from "./parts";

const UPDATE = process.env.UPDATE_GOLDEN === "1";

const FEATURE_TYPES = {
  sketch: 1, extrude: 1, revolve: 1, fillet: 1, chamfer: 1, refplane: 1, linearPattern: 1, circularPattern: 1, mirror: 1, shell: 1,
  imported: 1, rib: 1, draft: 1, hole: 1, sweep: 1, loft: 1, refaxis: 1, refpoint: 1, coordsys: 1,
} satisfies Record<FeatureType, 1>;
const END_CONDITIONS = {
  blind: 1, throughAll: 1, throughAllBoth: 1, upToNext: 1, upToVertex: 1, upToSurface: 1, offsetFromSurface: 1, upToBody: 1, midPlane: 1,
} satisfies Record<EndConditionType, 1>;

/** Git may check text out with CRLF on Windows; compare content, not line endings. */
const read = (u: URL) => readFileSync(u, "utf8").replace(/\r\n/g, "\n");
const docText = (d: PartDocument) => JSON.stringify(d, null, 1) + "\n";

const docs = new Map<string, PartDocument>();

beforeAll(async () => {
  await loadKernel();
  mkdirSync(CORPUS_DIR, { recursive: true });
}, 60_000);

describe("golden corpus", () => {
  for (const part of PARTS) {
    it(part.name, async () => {
      const b = new PartBuilder(part.name);
      await part.build(b);
      docs.set(part.name, b.doc);
      const docFile = new URL(`${part.name}.ocpart`, CORPUS_DIR);
      const metricsFile = new URL(`${part.name}.json`, CORPUS_DIR);
      if (UPDATE || !existsSync(docFile)) {
        if (!UPDATE) throw new Error(`corpus/${part.name}.ocpart is missing. Record it with UPDATE_GOLDEN=1.`);
        writeFileSync(docFile, docText(b.doc));
        writeFileSync(metricsFile, JSON.stringify(await measure(b.doc), null, 1) + "\n");
      }
      // The builder still produces the stored document (catches unintended fixture drift).
      expect(docText(b.doc)).toBe(read(docFile));
      // The stored document still rebuilds to the stored metrics.
      const metrics = await measure(JSON.parse(read(docFile)) as PartDocument);
      expect(metrics).toEqual(JSON.parse(read(metricsFile)));
      expect(metrics.steps.filter((s) => s.error).map((s) => `${s.name}: ${s.error}`)).toEqual([]);
    }, 120_000);
  }

  it("covers every feature type and end condition", () => {
    expect(docs.size).toBe(PARTS.length);
    const types = new Set<string>();
    const ends = new Set<string>();
    for (const d of docs.values()) {
      for (const f of d.features as Feature[]) {
        types.add(f.type);
        if (f.type === "extrude") {
          ends.add(f.end1.type);
          if (f.dir2) ends.add(f.dir2.type);
        }
      }
    }
    expect([...types].sort()).toEqual(Object.keys(FEATURE_TYPES).sort());
    expect([...ends].sort()).toEqual(Object.keys(END_CONDITIONS).sort());
  });
});
