/**
 * Document schema versions and migrations (mirror of `crates/oc-doc/src/migrate.rs`).
 *
 * A saved document carries `version`. Loading runs every migration from the file's
 * version up to `DOC_VERSION`, one step per version, before the app uses it.
 *
 * - 1: the original format. References are geometric signatures.
 * - 2: references may carry persistent names (`name` on face and edge signatures and on
 *   vertex references, `vertexName` on end conditions). See `docs/OVERHAUL_PLAN.md`
 *   section 3.3.
 */
import { DOC_VERSION, type PartDocument } from "./types";

type Raw = Record<string, unknown>;

/** `MIGRATIONS[i]` rewrites a version `i + 1` document into a version `i + 2` one. */
const MIGRATIONS: ((doc: Raw) => void)[] = [
  // 1 → 2: nothing to rewrite. Version 2 only adds optional names; a version 1 reference
  // is a version 2 reference without one (a legacy reference, found by signature).
  () => {},
];

/** Brings a parsed part file to the current schema version, or throws if it cannot. */
export function migrateDocument(parsed: unknown): PartDocument {
  if (!parsed || typeof parsed !== "object" || (parsed as Raw).format !== "opencad-part") throw new Error("Not an OpenCAD part file");
  const doc = parsed as Raw;
  const found = doc.version;
  if (typeof found !== "number" || !Number.isInteger(found) || found < 1) throw new Error("Not an OpenCAD part file (no schema version)");
  if (found > DOC_VERSION) throw new Error(`This file was saved by a newer OpenCAD (schema version ${found}; this version reads up to ${DOC_VERSION})`);
  for (let v = found; v < DOC_VERSION; v++) {
    MIGRATIONS[v - 1](doc);
    doc.version = v + 1;
  }
  return doc as unknown as PartDocument;
}
