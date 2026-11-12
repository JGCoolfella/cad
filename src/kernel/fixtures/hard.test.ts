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
 * Corpus steps that OCCT currently turns into an invalid solid without reporting an
 * error (BRepCheck_Analyzer fails even on topology). The golden metrics recorded for
 * these steps come from a broken body; the native engine must not reproduce them.
 */
const KNOWN_INVALID = [
  // Distance-distance (2 × 3) chamfer propagated round the filleted bottom loop.
  "fillet-chamfer: Chamfer2",
  // Inherits Chamfer2's invalid body.
  "fillet-chamfer: Chamfer3",
];

it("corpus parts are valid solids at every step (except the known OCCT failures)", async () => {
  const bad: string[] = [];
  for (const p of PARTS) {
    const doc = JSON.parse(readFileSync(new URL(`${p.name}.ocpart`, CORPUS_DIR), "utf8")) as PartDocument;
    await ensureImports(doc.features);
    bad.push(...invalidSteps(doc));
  }
  expect(bad).toEqual(KNOWN_INVALID);
}, 300_000);
