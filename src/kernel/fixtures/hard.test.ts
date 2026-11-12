/// <reference types="node" />
/**
 * Kernel failure inventory (see hard.ts). Each case's result on this engine is recorded
 * in corpus/hard/inventory.json; the test fails if any result changes, so a kernel
 * upgrade or engine change that fixes or breaks a hard case is never silent.
 *
 * Re-record:   UPDATE_GOLDEN=1 npx vitest run hard
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { beforeAll, expect, it } from "vitest";
import type { PartDocument } from "../../core/types";
import { ensureImports } from "../engine";
import { loadKernel } from "../testing";
import { HARD, invalidSteps, runHard, type HardResult } from "./hard";
import { CORPUS_DIR, PARTS } from "./parts";

const UPDATE = process.env.UPDATE_GOLDEN === "1";
const DIR = new URL("hard/", CORPUS_DIR);
const FILE = new URL("inventory.json", DIR);

beforeAll(loadKernel, 60_000);

it("kernel failure inventory matches the recording", () => {
  const results: HardResult[] = HARD.map(runHard);
  if (UPDATE || !existsSync(FILE)) {
    if (!UPDATE) throw new Error("corpus/hard/inventory.json is missing. Record it with UPDATE_GOLDEN=1.");
    mkdirSync(DIR, { recursive: true });
    writeFileSync(FILE, JSON.stringify(results, null, 1) + "\n");
    for (const r of results) console.log(`${r.pass ? "PASS" : "FAIL"} ${r.outcome.padEnd(14)} ${r.name}${r.detail ? ` — ${r.detail}` : ""}`);
  }
  const recorded = JSON.parse(readFileSync(FILE, "utf8")) as HardResult[];
  expect(results).toEqual(recorded);
}, 300_000);

/**
 * The engine rejects invalid kernel results (engine.checkResult), so no
 * successful step may ever leave an invalid solid. Cases where OCCT produces one live in
 * hard.ts (H10 and H23 are rejected).
 */
it("corpus parts are valid solids at every step", async () => {
  const bad: string[] = [];
  for (const p of PARTS) {
    const doc = JSON.parse(readFileSync(new URL(`${p.name}.ocpart`, CORPUS_DIR), "utf8")) as PartDocument;
    await ensureImports(doc.features);
    bad.push(...invalidSteps(doc));
  }
  expect(bad).toEqual([]);
}, 300_000);
