/// <reference types="node" />
/**
 * Torture cases (see torture.ts): replay each stored case in corpus/torture/ and check
 * its outcome on this engine matches the recorded `tsEngine` value. A case that starts
 * passing (or failing) fails this test, so a fix or regression is never silent; update
 * the recording in the same commit as the change that caused it.
 *
 * Re-record:   UPDATE_GOLDEN=1 npx vitest run torture
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { loadKernel } from "../testing";
import { CORPUS_DIR } from "./parts";
import { DEFS, buildCase, evaluate, type TortureCase } from "./torture";

const UPDATE = process.env.UPDATE_GOLDEN === "1";
const DIR = new URL("torture/", CORPUS_DIR);
const read = (u: URL) => readFileSync(u, "utf8").replace(/\r\n/g, "\n");

beforeAll(async () => {
  await loadKernel();
  mkdirSync(DIR, { recursive: true });
}, 60_000);

describe("torture cases", () => {
  for (const def of DEFS) {
    it(def.name, async () => {
      const file = new URL(`${def.name}.json`, DIR);
      const built = await buildCase(def);
      if (UPDATE || !existsSync(file)) {
        if (!UPDATE) throw new Error(`corpus/torture/${def.name}.json is missing. Record it with UPDATE_GOLDEN=1.`);
        const why = await evaluate({ ...built, tsEngine: "passes" });
        const rec: TortureCase = { ...built, tsEngine: why.length ? "fails" : "passes" };
        writeFileSync(file, JSON.stringify(rec, null, 1) + "\n");
        if (why.length) console.log(`${def.name} fails on this engine:\n  ${why.join("\n  ")}`);
      }
      const stored = JSON.parse(read(file)) as TortureCase;
      // The definition still produces the stored case (catches fixture drift).
      const { tsEngine, ...data } = stored;
      expect(JSON.parse(JSON.stringify(built))).toEqual(data);
      const why = await evaluate(stored);
      expect({ outcome: why.length ? "fails" : "passes", why }).toEqual({ outcome: tsEngine, why: tsEngine === "fails" ? why : [] });
    }, 120_000);
  }
});
