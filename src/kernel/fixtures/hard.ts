/**
 * Kernel failure inventory: deliberately hard modelling cases (fillets near their
 * limits, blends meeting at vertices, shells of filleted bodies, drafts on curved faces,
 * coplanar/tangent/singular booleans). Each case states what a robust kernel should do
 * and, where geometry allows, the exact volume. A case "passes" only if the result has
 * the wanted outcome, the solid is valid (BRepCheck_Analyzer) and the volume is right.
 *
 * Results are recorded in corpus/hard/inventory.json. This is the evidence for the
 * kernel decision (OCCT vs a commercial kernel) and the baseline for the native engine.
 */
import { getOC, measureShapeVolumeProperties, type Shape3D } from "replicad";
import type { FeatureStatus, PartDocument } from "../../core/types";
import { rebuild } from "../engine";
import { PartBuilder, STD, edgeAt, planar } from "./build";
import { blind, extrude } from "./parts";

export type Want = "ok" | "error" | "either";
export type Outcome = "ok" | "error" | "invalid" | "wrong-volume" | "wrong-topology";

export interface HardCase {
  name: string;
  group: "fillet" | "shell" | "draft" | "boolean";
  description: string;
  /** What a robust kernel should do. "either": failing cleanly is acceptable, a bad solid is not. */
  want: Want;
  /** Exact volume(s); the result must match one of them (relative 1e-4). */
  volume?: number[];
  solids?: number;
  build(b: PartBuilder): void;
}

export interface HardResult {
  name: string;
  group: HardCase["group"];
  want: Want;
  outcome: Outcome;
  pass: boolean;
  detail?: string;
  volume?: number;
}

const PI = Math.PI;
const near = (a: number, b: number) => Math.abs(a - b) < 1e-6;
const deg = (d: number) => (d * PI) / 180;

function box(b: PartBuilder, x: number, y: number, z: number) {
  extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, x, y)), blind(z));
}

/** Volume of an a×b×c box with every edge rounded (radius r): Minkowski sum of box and ball. */
function roundedBox(a: number, b: number, c: number, r: number) {
  const A = a - 2 * r, B = b - 2 * r, C = c - 2 * r;
  return A * B * C + 2 * r * (A * B + B * C + A * C) + PI * r * r * (A + B + C) + (4 / 3) * PI * r ** 3;
}

/** Concave fillet ring of radius r around the base of a cylinder of radius R (Pappus). */
function filletRing(R: number, r: number) {
  const area = r * r * (1 - PI / 4);
  const d = (r * (10 - 3 * PI)) / (3 * (4 - PI)); // centroid offset from the corner
  return 2 * PI * (R + d) * area;
}

export const HARD: HardCase[] = [
  // ------------------------------------------------------------- fillets
  {
    name: "H01 fillets just under half a face width",
    group: "fillet",
    description: "20×10×10 box; both long top edges filleted r=4.99 (face is 10 wide).",
    want: "ok",
    volume: [2000 - 2 * (1 - PI / 4) * 4.99 ** 2 * 20],
    build(b) {
      box(b, 20, 10, 10);
      b.add("Fillet", { type: "fillet", radius: 4.99, edges: b.edges(edgeAt((m) => near(m[2], 10) && (near(m[1], 0) || near(m[1], 10)), "LINE")).filter((e) => Math.abs(e.b[0] - e.a[0]) > 1), faces: [] });
    },
  },
  {
    name: "H02 fillets exactly half a face width",
    group: "fillet",
    description: "As H01 with r=5: the two fillets meet tangentially and the top face vanishes.",
    want: "either",
    volume: [2000 - 2 * (1 - PI / 4) * 25 * 20],
    build(b) {
      box(b, 20, 10, 10);
      b.add("Fillet", { type: "fillet", radius: 5, edges: b.edges(edgeAt((m) => near(m[2], 10) && (near(m[1], 0) || near(m[1], 10)), "LINE")).filter((e) => Math.abs(e.b[0] - e.a[0]) > 1), faces: [] });
    },
  },
  {
    name: "H03 fillets over half a face width",
    group: "fillet",
    description: "As H01 with r=5.01: the fillets would overlap. Failing cleanly is fine; a bad solid is not.",
    want: "either",
    build(b) {
      box(b, 20, 10, 10);
      b.add("Fillet", { type: "fillet", radius: 5.01, edges: b.edges(edgeAt((m) => near(m[2], 10) && (near(m[1], 0) || near(m[1], 10)), "LINE")).filter((e) => Math.abs(e.b[0] - e.a[0]) > 1), faces: [] });
    },
  },
  {
    name: "H04 three equal fillets at a corner",
    group: "fillet",
    description: "30 mm cube; the three edges at one corner filleted r=5 (spherical corner blend).",
    want: "ok",
    volume: [30 ** 3 - 3 * (1 - PI / 4) * 25 * (30 - 5) - 125 * (1 - PI / 6)],
    build(b) {
      box(b, 30, 30, 30);
      const e = b.edges(edgeAt((m) => [m[0], m[1], m[2]].filter((x) => near(x, 30)).length === 2, "LINE"));
      b.add("Fillet", { type: "fillet", radius: 5, edges: e, faces: [] });
    },
  },
  {
    // (A corner with three *different* radii can't be built from separate fillet features:
    // after the first, tangent propagation carries the next fillet round the corner.)
    name: "H05 concave edge meeting two convex edges",
    group: "fillet",
    description: "L-shaped block; the inner (concave) vertical edge and the two convex top edges meeting it filleted r=2 in one feature (mixed-convexity vertex blend).",
    want: "ok",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.poly([[0, 0], [30, 0], [30, 10], [10, 10], [10, 30], [0, 30]])), blind(20));
      const at = (x: number, y: number, z: number) => b.edges(edgeAt((m) => near(m[0], x) && near(m[1], y) && near(m[2], z), "LINE"));
      b.add("Fillet", { type: "fillet", radius: 2, edges: [...at(10, 10, 10), ...at(20, 10, 20), ...at(10, 20, 20)], faces: [] });
    },
  },
  {
    name: "H06 variable radius fillet 1 to 6",
    group: "fillet",
    description: "30 mm cube; one top edge with a fillet growing from r=1 to r=6.",
    want: "ok",
    build(b) {
      box(b, 30, 30, 30);
      b.add("Fillet", { type: "fillet", radius: 1, radius2: 6, edges: b.edges(edgeAt((m) => near(m[1], 0) && near(m[2], 30), "LINE")), faces: [] });
    },
  },
  {
    name: "H07 round every edge of a box",
    group: "fillet",
    description: "30×20×10 box with all 12 edges filleted r=2 in one feature.",
    want: "ok",
    volume: [roundedBox(30, 20, 10, 2)],
    build(b) {
      box(b, 30, 20, 10);
      b.add("Fillet", { type: "fillet", radius: 2, edges: b.edges(() => true), faces: [] });
    },
  },
  {
    name: "H08 concave fillet round a boss",
    group: "fillet",
    description: "40×40×10 plate with a Ø16 boss 10 high; the circular edge where they meet filleted r=2.",
    want: "ok",
    volume: [16000 + PI * 64 * 10 + filletRing(8, 2)],
    build(b) {
      box(b, 40, 40, 10);
      extrude(b, b.sketch({ kind: "face", sig: b.face(planar([0, 0, 1], 2, 10)) }, (d) => d.circle(20, 20, 8)), blind(10));
      b.add("Fillet", { type: "fillet", radius: 2, edges: b.edges(edgeAt((m) => near(m[2], 10), "CIRCLE")), faces: [] });
    },
  },
  {
    name: "H09 fillet larger than neighbouring edges",
    group: "fillet",
    description: "Plate with a 1 mm step; the step's three vertical edges filleted r=2 (longer than the 1 mm edges between them).",
    want: "either",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.poly([[0, 0], [30, 0], [30, 20], [29, 20], [29, 21], [0, 21]])), blind(10));
      const v = (x: number, y: number) => b.edges(edgeAt((m) => near(m[0], x) && near(m[1], y) && near(m[2], 5), "LINE"));
      b.add("Fillet", { type: "fillet", radius: 2, edges: [...v(30, 20), ...v(29, 20), ...v(29, 21)], faces: [] });
    },
  },
  // ------------------------------------------------------------- shells
  {
    name: "H10 shell a fully rounded box",
    group: "shell",
    description: "H07's rounded box, shelled 1 mm inward with the top face removed.",
    want: "ok",
    build(b) {
      box(b, 30, 20, 10);
      b.add("Fillet", { type: "fillet", radius: 2, edges: b.edges(() => true), faces: [] });
      b.add("Shell", { type: "shell", thickness: 1, faces: [b.face(planar([0, 0, 1], 2, 10))], outward: false });
    },
  },
  {
    name: "H11 shell thicker than its fillets",
    group: "shell",
    description: "Box with r=2 vertical fillets shelled 3 mm inward (the inner offset of each fillet collapses to a sharp edge).",
    want: "ok",
    build(b) {
      box(b, 40, 30, 20);
      b.add("Fillet", { type: "fillet", radius: 2, edges: b.edges(edgeAt((m) => near(m[2], 10), "LINE")), faces: [] });
      b.add("Shell", { type: "shell", thickness: 3, faces: [b.face(planar([0, 0, 1], 2, 20))], outward: false });
    },
  },
  {
    name: "H12 shell a plate with a boss",
    group: "shell",
    description: "H08's plate and boss (no fillet), shelled 1 mm inward with the bottom face removed.",
    want: "ok",
    build(b) {
      box(b, 40, 40, 10);
      extrude(b, b.sketch({ kind: "face", sig: b.face(planar([0, 0, 1], 2, 10)) }, (d) => d.circle(20, 20, 8)), blind(10));
      b.add("Shell", { type: "shell", thickness: 1, faces: [b.face(planar([0, 0, -1], 2, 0))], outward: false });
    },
  },
  {
    name: "H13 outward shell of a filleted box",
    group: "shell",
    description: "Box with r=3 vertical fillets, shelled 1 mm outward with the top removed.",
    want: "ok",
    build(b) {
      box(b, 40, 30, 20);
      b.add("Fillet", { type: "fillet", radius: 3, edges: b.edges(edgeAt((m) => near(m[2], 10), "LINE")), faces: [] });
      b.add("Shell", { type: "shell", thickness: 1, faces: [b.face(planar([0, 0, 1], 2, 20))], outward: true });
    },
  },
  // ------------------------------------------------------------- drafts
  {
    name: "H14 draft a cylindrical face",
    group: "draft",
    description: "Ø20 cylinder 20 high; its side drafted 5° about the bottom plane (becomes a cone frustum).",
    want: "ok",
    volume: [-1, 1].map((s) => {
      const R = 10, r = 10 + s * 20 * Math.tan(deg(5));
      return (PI / 3) * 20 * (R * R + R * r + r * r);
    }),
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.circle(0, 0, 10)), blind(20));
      b.add("Draft", { type: "draft", angle: deg(5), neutral: { kind: "plane", plane: STD.front }, reverse: false, faces: b.faces((s) => s.surface === "CYLINDRE") });
    },
  },
  {
    name: "H15 draft faces with fillets between them",
    group: "draft",
    description: "Box with r=3 vertical fillets; all four sides and the four fillet faces drafted 3°.",
    want: "ok",
    build(b) {
      box(b, 40, 30, 20);
      b.add("Fillet", { type: "fillet", radius: 3, edges: b.edges(edgeAt((m) => near(m[2], 10), "LINE")), faces: [] });
      b.add("Draft", { type: "draft", angle: deg(3), neutral: { kind: "plane", plane: STD.front }, reverse: false, faces: b.faces((s) => Math.abs(s.normal[2]) < 1e-6) });
    },
  },
  // ------------------------------------------------------------- booleans
  {
    name: "H16 boxes touching face to face",
    group: "boolean",
    description: "Two 20×10×10 boxes sharing the whole x=20 face. Must fuse into one solid.",
    want: "ok",
    volume: [4000],
    solids: 1,
    build(b) {
      box(b, 20, 10, 10);
      extrude(b, b.sketch(STD.front, (d) => d.rect(20, 0, 40, 10)), blind(10));
    },
  },
  {
    name: "H17 boss flush with three sides",
    group: "boolean",
    description: "40×20×10 block; a boss on its top face flush with three of its sides (coplanar side faces).",
    want: "ok",
    volume: [8000 + 40 * 10 * 5],
    solids: 1,
    build(b) {
      box(b, 40, 20, 10);
      extrude(b, b.sketch({ kind: "face", sig: b.face(planar([0, 0, 1], 2, 10)) }, (d) => d.rect(0, 0, 40, 10)), blind(5));
    },
  },
  {
    name: "H18 cylinder tangent to a box",
    group: "boolean",
    description: "Cylinder touching a box along one line only (tangent contact). A clean error or two separate solids are both acceptable; a corrupt solid is not.",
    want: "either",
    build(b) {
      box(b, 40, 20, 10);
      extrude(b, b.sketch(STD.front, (d) => d.circle(20, 25, 5)), blind(10));
    },
  },
  {
    name: "H19 pocket exactly as deep as the plate",
    group: "boolean",
    description: "40×20×10 block; a 20×10 blind cut exactly 10 deep (coplanar with the bottom face) must become a through hole.",
    want: "ok",
    volume: [6000],
    solids: 1,
    build(b) {
      box(b, 40, 20, 10);
      extrude(b, b.sketch({ kind: "face", sig: b.face(planar([0, 0, 1], 2, 10)) }, (d) => d.rect(10, 5, 30, 15)), blind(10), { cut: true });
    },
  },
  {
    name: "H20 equal cylinders crossing (Steinmetz)",
    group: "boolean",
    description: "Two Ø10 cylinders 40 long crossing at right angles. The intersection curves cross each other (singular surface intersection).",
    want: "ok",
    volume: [2 * PI * 25 * 40 - (16 / 3) * 125],
    solids: 1,
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.circle(0, 0, 5)), { type: "midPlane", depth: 40 });
      extrude(b, b.sketch(STD.right, (d) => d.circle(0, 0, 5)), { type: "midPlane", depth: 40 });
    },
  },
  {
    name: "H21 nearly equal cylinders crossing",
    group: "boolean",
    description: "As H20 with radii 5 and 5.001: the intersection is near-singular (two curves pass within microns).",
    want: "ok",
    solids: 1,
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.circle(0, 0, 5)), { type: "midPlane", depth: 40 });
      extrude(b, b.sketch(STD.right, (d) => d.circle(0, 0, 5.001)), { type: "midPlane", depth: 40 });
    },
  },
  {
    name: "H22 halve a sphere through its poles",
    group: "boolean",
    description: "Revolved sphere r=10 cut in half by a plane through its axis (the cut runs along the seam and both poles).",
    want: "ok",
    volume: [(2 / 3) * PI * 1000],
    solids: 1,
    build(b) {
      let axis = "";
      const s = b.sketch(STD.front, (d) => {
        // Right half-disc (arc runs counter-clockwise from the bottom pole to the top one).
        const lo = d.point(0, -10), hi = d.point(0, 10);
        d.arc(d.point(0, 0), lo, hi);
        d.line(hi, lo);
        axis = d.line(d.point(0, -20), d.point(0, 20), true);
      });
      b.add("Revolve", { type: "revolve", cut: false, sketchId: s, axis, angle: 2 * PI, revType: "oneDirection", reverse: false, merge: true });
      extrude(b, b.sketch(STD.front, (d) => d.rect(-20, -20, 0, 20)), { type: "throughAllBoth", depth: 0 }, { cut: true });
    },
  },
];

/** Features (by part and name) whose resulting body is not a valid solid. */
export function invalidSteps(doc: PartDocument): string[] {
  const bad: string[] = [];
  rebuild(doc.features, undefined, 3, (f, shape, st) => {
    if (shape && !st.error && !isValid(shape)) bad.push(`${doc.name}: ${f.name}`);
  });
  return bad;
}

export function isValid(shape: Shape3D): boolean {
  const oc = getOC() as unknown as { BRepCheck_Analyzer: new (s: unknown, geom: boolean, par: boolean, exact: boolean) => { IsValid(): boolean; delete(): void } };
  const a = new oc.BRepCheck_Analyzer(shape.wrapped, true, false, false);
  try {
    return a.IsValid();
  } finally {
    a.delete();
  }
}

/** Build and judge one case. */
export function runHard(c: HardCase): HardResult {
  const base = { name: c.name, group: c.group, want: c.want };
  const b = new PartBuilder(c.name);
  try {
    c.build(b);
  } catch (e) {
    // A query before the hard feature failed: the setup itself did not build.
    return { ...base, outcome: "error", pass: c.want !== "ok", detail: `setup: ${e instanceof Error ? e.message : String(e)}` };
  }
  let last: Shape3D | null = null;
  const statuses: FeatureStatus[] = [];
  rebuild(b.doc.features, undefined, 3, (_f, shape, st) => {
    last = shape;
    statuses.push(st);
  });
  const judge = (outcome: Outcome, detail?: string, volume?: number): HardResult => ({
    ...base,
    outcome,
    pass: c.want === "either" ? outcome === "ok" || outcome === "error" : outcome === c.want,
    ...(detail ? { detail } : {}),
    ...(volume !== undefined ? { volume: Number(volume.toPrecision(6)) } : {}),
  });
  const err = statuses.find((s) => s.error);
  if (err) return judge("error", `${b.doc.features.find((f) => f.id === err.id)?.name}: ${err.error}`);
  const shape = last as Shape3D | null;
  if (!shape) return judge("error", "no body");
  let v = NaN;
  try {
    v = measureShapeVolumeProperties(shape).volume;
  } catch {
    /* reported through the volume check */
  }
  if (!isValid(shape)) return judge("invalid", "BRepCheck_Analyzer reports the solid is not valid", v);
  const n = shape.solids.length;
  if (c.solids !== undefined && n !== c.solids) return judge("wrong-topology", `${n} solids, want ${c.solids}`, v);
  if (c.volume && !c.volume.some((w) => Math.abs(v - w) <= 1e-4 * Math.abs(w))) {
    return judge("wrong-volume", `volume ${v.toPrecision(8)}, want ${c.volume.map((w) => w.toPrecision(8)).join(" or ")}`, v);
  }
  return judge("ok", n !== 1 ? `${n} solids` : undefined, v);
}
