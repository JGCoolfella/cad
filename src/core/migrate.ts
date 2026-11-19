/**
 * Document schema versions and migrations (mirror of `crates/oc-doc/src/migrate.rs`).
 *
 * A saved document carries `version`. Loading runs every migration from the file's
 * version up to `DOC_VERSION`, one step per version, before the app uses it.
 *
 * - 1: the original format. References are geometric signatures.
 * - 2: references may carry persistent names (`name` on face and edge signatures and on
 *   vertex references, `vertexName` on end conditions; `docs/OVERHAUL_PLAN.md` section
 *   3.3), and the part origin is its own kind of reference (`{ kind: "origin" }`) instead
 *   of a vertex at (0, 0, 0).
 * - 3: configurations (`configurations`, G1e); a cap tag in a stored name may carry its
 *   feature's region count (`f2/end/<key>#1`).
 */
import { DOC_VERSION, type PartDocument } from "./types";

type Raw = Record<string, unknown>;

/**
 * 1 → 2. Names need no rewriting: they are optional, and a version 1 reference is a
 * version 2 reference without one (a legacy reference, found by signature).
 *
 * The origin does: version 1 wrote a picked origin as `{ kind: "vertex", p: [0, 0, 0] }`,
 * the same as a model vertex there. Every picked reference that is exactly that object
 * becomes `{ kind: "origin" }`. Sketch references to vertices carry an `id` and Up To
 * Vertex stores a bare position; both are always model vertices and are not touched.
 */
function originGetsItsOwnKind(doc: Raw) {
  const walk = (v: unknown) => {
    if (Array.isArray(v)) v.forEach(walk);
    else if (v && typeof v === "object") {
      const o = v as Raw;
      const p = o.p;
      if (Object.keys(o).length === 2 && o.kind === "vertex" && Array.isArray(p) && p.length === 3 && p.every((x) => x === 0)) {
        delete o.p;
        o.kind = "origin";
      } else Object.values(o).forEach(walk);
    }
  };
  walk(doc.features);
}

/**
 * 2 → 3. Nothing to rewrite: a document without `configurations` has one, Default, and a
 * cap tag without a region count keeps the G1d rule.
 */
function configurationsAreOptional() {}

/** `MIGRATIONS[i]` rewrites a version `i + 1` document into a version `i + 2` one. */
const MIGRATIONS: ((doc: Raw) => void)[] = [originGetsItsOwnKind, configurationsAreOptional];

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
