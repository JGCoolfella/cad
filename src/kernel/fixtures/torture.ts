/**
 * Torture cases for topological references: build a part, apply the kind of upstream
 * edit users make all the time, rebuild, and check that downstream features still
 * attach to the right faces and edges.
 *
 * Each case is stored as plain data in corpus/torture/<name>.json (initial document,
 * edits as JSON path operations, expectations), so any engine can replay it. Most
 * cases compare against a *reference*: the same part built directly in its final state,
 * with references picked on the final body. Feature ids match between the edited and the
 * reference document because the builder assigns ids in script order.
 */
/// <reference types="node" />
import { readFileSync } from "node:fs";
import type { ExtrudeFeature, FaceSig, Id, PartDocument, SketchData, SketchFeature } from "../../core/types";
import { emptySketch } from "../../sketch/model";
import { clearCache, rebuild } from "../engine";
import { PartBuilder, STD, SketchDraw, edgeAt, measure, planar, type PartMetrics, type StepMetrics } from "./build";
import { CORPUS_DIR, blind, extrude } from "./parts";

// ------------------------------------------------------------------ data model

export type Path = (string | number)[];
export type Edit =
  | { op: "set"; path: Path; value: unknown }
  | { op: "remove"; path: Path }
  | { op: "insert"; path: Path; value: unknown };

export interface TortureCase {
  name: string;
  description: string;
  /** Work package expected to make a failing case pass. */
  fixedBy?: string;
  /** Recorded outcome on the current TypeScript/WASM engine (checked by the test). */
  tsEngine: "passes" | "fails";
  doc: PartDocument;
  edits: Edit[];
  expect: {
    /** Per-step metrics the edited document must reproduce (steps matched by id). */
    reference?: PartMetrics;
    /**
     * Faces of the reference's final body (type and centre, sorted). Position-sensitive:
     * a fillet on the wrong corner of a box has the same volume and area as the right one.
     */
    referenceFaces?: FaceKey[];
    /** Features that must report an error matching the pattern (case-insensitive regex). */
    errors?: { id: Id; match: string }[];
    /** Rebuild cold, warm and cold again: identical metrics and face signatures. */
    deterministic?: boolean;
    /** Must reproduce the golden metrics of this corpus part. */
    corpus?: string;
  };
}

export interface FaceKey {
  surface: string;
  center: [number, number, number];
}

/** Final faces as sorted keys, centres rounded to 1e-3 mm. */
function faceKeys(features: PartDocument["features"]): FaceKey[] {
  const r = (x: number) => {
    const v = Math.round(x * 1000) / 1000;
    return v === 0 ? 0 : v;
  };
  const faces = rebuild(features).mesh?.faces ?? [];
  return faces
    .map((f) => ({ surface: f.sig.surface, center: f.sig.center.map(r) as [number, number, number] }))
    .sort((a, b) => (JSON.stringify(a) < JSON.stringify(b) ? -1 : 1));
}

export function applyEdits(doc: PartDocument, edits: Edit[]): PartDocument {
  const out = structuredClone(doc);
  for (const e of edits) {
    const parent = e.path.slice(0, -1).reduce<unknown>((o, k) => (o as Record<string | number, unknown>)[k], out) as Record<string | number, unknown> | unknown[];
    const key = e.path[e.path.length - 1];
    if (Array.isArray(parent)) {
      const i = key as number;
      if (e.op === "remove") parent.splice(i, 1);
      else if (e.op === "insert") parent.splice(i, 0, structuredClone(e.value));
      else parent[i] = structuredClone(e.value);
    } else if (e.op === "remove") delete parent[key];
    else parent[key] = structuredClone(e.value);
  }
  return out;
}

// ------------------------------------------------------------------ evaluation

const close = (a: number, b: number, rel = 1e-6, abs = 1e-6) => Math.abs(a - b) <= Math.max(abs, rel * Math.max(Math.abs(a), Math.abs(b)));

function closeDeep(a: unknown, b: unknown): boolean {
  if (typeof a === "number" && typeof b === "number") return close(a, b);
  if (Array.isArray(a) && Array.isArray(b)) return a.length === b.length && a.every((x, i) => closeDeep(x, b[i]));
  if (a && b && typeof a === "object" && typeof b === "object") {
    const ka = Object.keys(a).sort(), kb = Object.keys(b).sort();
    return ka.length === kb.length && ka.every((k, i) => k === kb[i] && closeDeep((a as never)[k], (b as never)[k]));
  }
  return a === b;
}

function stepDiff(got: StepMetrics | undefined, want: StepMetrics): string | null {
  if (!got) return `${want.name}: missing`;
  if ((got.error ?? null) !== (want.error ?? null)) return `${want.name}: error ${JSON.stringify(got.error ?? null)}, want ${JSON.stringify(want.error ?? null)}`;
  if (!closeDeep(got.body ?? null, want.body ?? null)) return `${want.name}: body ${JSON.stringify(got.body)}, want ${JSON.stringify(want.body)}`;
  if (!closeDeep(got.ref ?? null, want.ref ?? null)) return `${want.name}: reference geometry ${JSON.stringify(got.ref)}, want ${JSON.stringify(want.ref)}`;
  return null;
}

/** Replay a case. Returns the reasons it fails (empty = passes). */
export async function evaluate(c: TortureCase): Promise<string[]> {
  const why: string[] = [];
  // As in the app: the document is built first, then edited.
  clearCache();
  rebuild(c.doc.features);
  const doc = applyEdits(c.doc, c.edits);
  const m = await measure(doc);
  const byId = new Map(m.steps.map((s) => [s.id, s]));
  if (c.expect.reference) {
    for (const want of c.expect.reference.steps) {
      const d = stepDiff(byId.get(want.id), want);
      if (d) why.push(d);
    }
  }
  if (c.expect.referenceFaces) {
    const got = faceKeys(doc.features);
    const want = c.expect.referenceFaces;
    const key = (k: FaceKey) => JSON.stringify(k);
    const missing = want.filter((k) => !got.some((g) => key(g) === key(k)));
    const extra = got.filter((g) => !want.some((k) => key(g) === key(k)));
    if (missing.length || extra.length)
      why.push(`final faces differ from the reference: missing ${missing.map(key).join(" ") || "none"}; unexpected ${extra.map(key).join(" ") || "none"}`);
  }
  for (const e of c.expect.errors ?? []) {
    const s = byId.get(e.id);
    if (!s?.error || !new RegExp(e.match, "i").test(s.error)) why.push(`${s?.name ?? e.id}: error ${JSON.stringify(s?.error ?? null)}, want /${e.match}/`);
  }
  if (c.expect.deterministic) {
    const run = async (cold: boolean) => {
      if (cold) clearCache();
      const r = rebuild(doc.features);
      return { metrics: await measure(doc), faces: r.mesh?.faces.map((f) => f.sig), edges: r.mesh?.edges.map((e) => e.sig) };
    };
    const a = JSON.stringify(await run(true)), b = JSON.stringify(await run(false)), c2 = JSON.stringify(await run(true));
    if (a !== b) why.push("warm rebuild differs from cold rebuild");
    if (a !== c2) why.push("second cold rebuild differs from the first");
  }
  if (c.expect.corpus) {
    const want = JSON.parse(readFileSync(new URL(`${c.expect.corpus}.json`, CORPUS_DIR), "utf8")) as PartMetrics;
    for (const s of want.steps) {
      const d = stepDiff(byId.get(s.id), s);
      if (d) why.push(d);
    }
  }
  return why;
}

// ------------------------------------------------------------------ cases

interface Def {
  name: string;
  description: string;
  fixedBy?: string;
  /** Build the part; return the ids the edits refer to. */
  initial(b: PartBuilder): Promise<Record<string, Id>> | Record<string, Id>;
  edits(doc: PartDocument, ids: Record<string, Id>): Edit[];
  /** The same script built directly in the final state (same feature order, so same ids). */
  reference?(b: PartBuilder): void | Promise<void>;
  errors?(ids: Record<string, Id>): { id: Id; match: string }[];
  deterministic?: boolean;
  corpus?: string;
}

const near = (a: number, b: number) => Math.abs(a - b) < 1e-6;
const idx = (doc: PartDocument, id: Id) => doc.features.findIndex((f) => f.id === id);
const topFace = (b: PartBuilder, z: number): FaceSig => b.face(planar([0, 0, 1], 2, z));

/** A sketch drawn with the same id prefix the builder would give feature `id`. */
function sketchData(id: Id, draw: (d: SketchDraw) => void): SketchData {
  const sk = emptySketch();
  draw(new SketchDraw(sk, `${id}_`));
  return sk;
}

/** Ø6 hole at (10, 10) through a part, as two literal features (ids past any builder id). */
function throughHole(): [SketchFeature, ExtrudeFeature] {
  return [
    { id: "f90", name: "Sketch90", type: "sketch", plane: STD.front, sketch: sketchData("f90", (d) => d.circle(10, 10, 3)) },
    { id: "f91", name: "Cut-Extrude90", type: "extrude", cut: true, sketchId: "f90", reverse: false, merge: true, end1: { type: "throughAllBoth", depth: 0 } },
  ];
}

const NOTCHED: [number, number][] = [[0, 0], [60, 0], [60, 30], [50, 30], [50, 40], [0, 40]];

/** Pad, sketch on its top face, pocket from that sketch. */
function padPocket(b: PartBuilder, profile: (d: SketchDraw) => void, height: number) {
  const s1 = b.sketch(STD.front, profile);
  const e1 = extrude(b, s1, blind(height));
  const s2 = b.sketch({ kind: "face", sig: topFace(b, height) }, (d) => d.rect(15, 10, 45, 30));
  const c1 = extrude(b, s2, blind(4), { cut: true });
  return { s1, e1, s2, c1 };
}

/** Box with a fillet on the vertical edge at (x, y). */
function filletedBox(b: PartBuilder, w: number, x: number, y: number) {
  const s1 = b.sketch(STD.front, (d) => d.rect(0, 0, w, 30));
  const e1 = extrude(b, s1, blind(20));
  const edge = b.edges(edgeAt((m) => near(m[0], x) && near(m[1], y) && near(m[2], 10), "LINE"));
  const f1 = b.add("Fillet", { type: "fillet", radius: 3, edges: edge, faces: [] });
  return { s1, e1, f1 };
}

/**
 * Tapered box (5° draft) shelled with its top removed. The taper matters: on a plain box,
 * removing the bottom instead of the top gives identical volume, area and topology, so
 * the metrics could not tell a wrong re-attachment from a right one.
 */
function shelledBox(b: PartBuilder, h: number) {
  const s1 = b.sketch(STD.front, (d) => d.rect(0, 0, 40, 30));
  const e1 = extrude(b, s1, { type: "blind", depth: h, draft: (5 * Math.PI) / 180 });
  const sh = b.add("Shell", { type: "shell", thickness: 2, faces: [topFace(b, h)], outward: false });
  return { s1, e1, sh };
}

function pocketFillets(b: PartBuilder, x0: number) {
  const s1 = b.sketch(STD.front, (d) => d.rect(0, 0, 100, 40));
  const e1 = extrude(b, s1, blind(10));
  const s2 = b.sketch({ kind: "face", sig: topFace(b, 10) }, (d) => d.rect(x0, 10, x0 + 20, 30));
  const c1 = extrude(b, s2, blind(4), { cut: true });
  const f1 = b.add("Fillet", { type: "fillet", radius: 1.5, edges: b.edges(edgeAt((m) => near(m[2], 6), "LINE")), faces: [] });
  return { s1, e1, s2, c1, f1 };
}

export const DEFS: Def[] = [
  {
    name: "TT1a-notch-base",
    description: "Pad, sketch on its top face, pocket. Then notch a corner of the base profile (4 to 6 vertices). The pocket sketch must stay on the top face.",
    fixedBy: "E6",
    initial: (b) => padPocket(b, (d) => d.rect(0, 0, 60, 40), 10),
    edits: (doc, ids) => [{ op: "set", path: ["features", idx(doc, ids.s1), "sketch"], value: sketchData(ids.s1, (d) => d.poly(NOTCHED)) }],
    reference: (b) => void padPocket(b, (d) => d.poly(NOTCHED), 10),
  },
  {
    name: "TT1b-taller-pad",
    description: "Pad, sketch on its top face, pocket. Then change the pad height from 10 to 40. The pocket sketch must move up with the top face.",
    fixedBy: "E6",
    initial: (b) => padPocket(b, (d) => d.rect(0, 0, 60, 40), 10),
    edits: (doc, ids) => [{ op: "set", path: ["features", idx(doc, ids.e1), "end1", "depth"], value: 40 }],
    reference: (b) => void padPocket(b, (d) => d.rect(0, 0, 60, 40), 40),
  },
  {
    name: "TT2-fillet-follows-corner",
    description: "Box 40 wide with a fillet on the vertical edge at the (40, 30) corner. Then double the width. The fillet must stay on the same corner, now at (80, 30).",
    fixedBy: "E6",
    initial: (b) => filletedBox(b, 40, 40, 30),
    edits: (doc, ids) => {
      const i = idx(doc, ids.s1);
      // Rectangle points in order: (0,0), (w,0), (w,30), (0,30).
      return [2, 3].map((k) => ({ op: "set" as const, path: ["features", i, "sketch", "points", `${ids.s1}_p${k}`, "x"], value: 80 }));
    },
    reference: (b) => void filletedBox(b, 80, 80, 30),
  },
  {
    name: "TT3-shell-keeps-top-open",
    description: "Box shelled with its top face removed. Then change the height from 10 to 100. The top must stay open (not the bottom).",
    fixedBy: "E6",
    initial: (b) => shelledBox(b, 10),
    edits: (doc, ids) => [{ op: "set", path: ["features", idx(doc, ids.e1), "end1", "depth"], value: 100 }],
    reference: (b) => void shelledBox(b, 100),
  },
  {
    name: "TT4-pocket-fillets-follow",
    description: "Pocket with its floor edges filleted. Then move the pocket 30 mm along X in its sketch. The fillets must follow the pocket.",
    fixedBy: "E6",
    initial: (b) => pocketFillets(b, 10),
    edits: (doc, ids) => {
      const i = idx(doc, ids.s2);
      return [1, 2, 3, 4].map((k) => {
        const p = `${ids.s2}_p${k}`;
        const x = (doc.features[i] as { sketch: SketchData }).sketch.points[p].x;
        return { op: "set" as const, path: ["features", i, "sketch", "points", p, "x"], value: x + 30 };
      });
    },
    reference: (b) => void pocketFillets(b, 40),
  },
  {
    name: "TT5-deleted-boss",
    description: "Block, boss on its top face, a sketch on the boss's top face and a cut from it. Then delete the boss (and its sketch). The sketch must report a missing reference instead of moving to another face.",
    fixedBy: "E6",
    initial: (b) => {
      const s1 = b.sketch(STD.front, (d) => d.rect(0, 0, 60, 40));
      const e1 = extrude(b, s1, blind(10));
      const s2 = b.sketch({ kind: "face", sig: topFace(b, 10) }, (d) => d.rect(10, 10, 30, 30));
      const e2 = extrude(b, s2, blind(10));
      const s3 = b.sketch({ kind: "face", sig: topFace(b, 20) }, (d) => d.circle(20, 20, 4));
      const c1 = extrude(b, s3, blind(5), { cut: true });
      return { s1, e1, s2, e2, s3, c1 };
    },
    edits: (doc, ids) => [
      { op: "remove", path: ["features", idx(doc, ids.e2)] },
      { op: "remove", path: ["features", idx(doc, ids.s2)] },
    ],
    errors: (ids) => [
      { id: ids.s3, match: "missing reference" },
      { id: ids.c1, match: "." },
    ],
  },
  {
    name: "TT6-determinism",
    description: "Rebuild the patterns part cold, warm and cold again. Metrics and every face and edge signature must be identical.",
    initial: (b) => {
      const doc = JSON.parse(readFileSync(new URL("patterns.ocpart", CORPUS_DIR), "utf8")) as PartDocument;
      b.doc.features.push(...doc.features);
      return {};
    },
    edits: () => [],
    deterministic: true,
  },
  {
    name: "TT7-legacy-document",
    description: "A version 1 document without persistent names (pad-pocket from the corpus) must rebuild to its golden metrics.",
    initial: (b) => {
      const doc = JSON.parse(readFileSync(new URL("pad-pocket.ocpart", CORPUS_DIR), "utf8")) as PartDocument;
      b.doc.features.push(...doc.features);
      return {};
    },
    edits: () => [],
    corpus: "pad-pocket",
  },
  {
    name: "TT8-unrelated-upstream-feature",
    description: "Box with a filleted corner; then insert a hole elsewhere before the fillet. Face and edge order changes, but the fillet must stay on its corner. (Control case: must keep passing.)",
    initial: (b) => filletedBox(b, 40, 40, 30),
    edits: (doc, ids) => {
      const at = idx(doc, ids.f1);
      const [sk, cut] = throughHole();
      return [
        { op: "insert", path: ["features", at], value: sk },
        { op: "insert", path: ["features", at + 1], value: cut },
      ];
    },
    reference: (b) => {
      // Same ids as the edited document: f1 sketch, f2 extrude, f90/f91 hole, f3 fillet.
      const s1 = b.sketch(STD.front, (d) => d.rect(0, 0, 40, 30));
      extrude(b, s1, blind(20));
      b.doc.features.push(...throughHole());
      const edge = b.edges(edgeAt((m) => near(m[0], 40) && near(m[1], 30) && near(m[2], 10), "LINE"));
      b.add("Fillet", { type: "fillet", radius: 3, edges: edge, faces: [] });
    },
  },
];

/** Build a case's data from its definition. */
export async function buildCase(d: Def): Promise<Omit<TortureCase, "tsEngine">> {
  const b = new PartBuilder(d.name);
  const ids = await d.initial(b);
  const expect: TortureCase["expect"] = {};
  if (d.reference) {
    const r = new PartBuilder(d.name);
    await d.reference(r);
    expect.reference = await measure(r.doc);
    expect.referenceFaces = faceKeys(r.doc.features);
  }
  if (d.errors) expect.errors = d.errors(ids);
  if (d.deterministic) expect.deterministic = true;
  if (d.corpus) expect.corpus = d.corpus;
  return { name: d.name, description: d.description, ...(d.fixedBy ? { fixedBy: d.fixedBy } : {}), doc: b.doc, edits: d.edits(b.doc, ids), expect };
}
