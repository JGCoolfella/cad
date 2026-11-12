/**
 * Golden corpus parts. Between them they use every feature type and every extrude end
 * condition (checked by golden.test.ts). Each part is a short script against
 * PartBuilder; the built documents and their per-step metrics are stored in corpus/.
 *
 * Coordinates: Front = XY (normal +Z), Top = XZ (normal +Y, sketch y = -Z),
 * Right = YZ (normal +X, sketch x = -Z). A sketch on a +Z face uses world X/Y.
 */
/// <reference types="node" />
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import type { EndCondition, ExtrudeFeature, Feature, Id, SketchEntity, Vec2 } from "../../core/types";
import { ensureImports, handle } from "../engine";
import { PartBuilder, STD, edgeAt, planar, planeRef } from "./build";

export const CORPUS_DIR = new URL("../../../corpus/", import.meta.url);

/**
 * STEP file used by the "imported" part: a 30×20×10 block with one chamfered edge.
 * Generated once and committed (STEP headers carry a timestamp, so regenerating it
 * would change the corpus).
 */
async function stepAsset(): Promise<string> {
  const dir = new URL("assets/", CORPUS_DIR);
  const file = new URL("block.step", dir);
  if (existsSync(file)) return readFileSync(file, "utf8").replace(/\r\n/g, "\n");
  const b = new PartBuilder("step-source");
  extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 30, 20)), blind(10));
  b.add("Chamfer", { type: "chamfer", chamferType: "equal", distance: 2, edges: b.edges(edgeAt((m) => near(m[1], 20) && near(m[2], 10), "LINE")) });
  const out = await handle({ id: 0, op: "export", features: b.doc.features, format: "step" });
  const text = new TextDecoder().decode(out.result as ArrayBuffer).replace(/\r\n/g, "\n");
  mkdirSync(dir, { recursive: true });
  writeFileSync(file, text);
  return text;
}

export interface CorpusPart {
  name: string;
  build(b: PartBuilder): void | Promise<void>;
}

const deg = (d: number) => (d * Math.PI) / 180;
const near = (a: number, b: number) => Math.abs(a - b) < 1e-6;
export const blind = (depth: number): EndCondition => ({ type: "blind", depth });

export function extrude(b: PartBuilder, sketchId: Id, end1: EndCondition, o: Partial<Omit<ExtrudeFeature, "id" | "name" | "type">> = {}): Id {
  const cut = !!o.cut;
  return b.add(cut ? "Cut-Extrude" : "Boss-Extrude", { type: "extrude", cut, sketchId, reverse: false, merge: true, end1, ...o });
}

/** Revolve about a construction line in the same sketch. */
function revolve(b: PartBuilder, sketchId: Id, axis: Id, angle: number, o: Record<string, unknown> = {}): Id {
  return b.add(o.cut ? "Cut-Revolve" : "Revolve", { type: "revolve", cut: false, sketchId, axis, angle, revType: "oneDirection", reverse: false, merge: true, ...o } as never);
}

export const PARTS: CorpusPart[] = [
  {
    name: "block-hole",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 20, 10)), blind(5));
      extrude(b, b.sketch(STD.front, (d) => d.circle(10, 5, 3)), { type: "throughAllBoth", depth: 0 }, { cut: true });
    },
  },
  {
    name: "pad-pocket",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 60, 40)), blind(10));
      const top = b.face(planar([0, 0, 1], 2, 10));
      extrude(b, b.sketch({ kind: "face", sig: top }, (d) => d.rect(15, 10, 45, 30)), blind(4), { cut: true });
      const floor = b.edges(edgeAt((m) => near(m[2], 6), "LINE"));
      b.add("Fillet", { type: "fillet", radius: 1.5, edges: floor, faces: [] });
    },
  },
  {
    name: "up-to",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 80, 20)), blind(10));
      const top = b.face(planar([0, 0, 1], 2, 10));
      const pl = b.add("Plane", { type: "refplane", base: STD.front, offset: 30, flip: false });
      const on = { kind: "refplane" as const, featureId: pl };
      const box = (x: number) => b.sketch(on, (d) => d.rect(x, 5, x + 16, 15));
      // All four grow downwards from z = 30 towards the block's top face (z = 10).
      extrude(b, box(2), { type: "upToNext", depth: 0 }, { reverse: true });
      extrude(b, box(22), { type: "upToVertex", depth: 0, vertex: [80, 20, 10] }, { reverse: true });
      extrude(b, box(42), { type: "upToSurface", depth: 0, ref: { kind: "face", sig: top } }, { reverse: true });
      extrude(b, box(62), { type: "offsetFromSurface", depth: 3, ref: { kind: "face", sig: top }, reverseOffset: true }, { reverse: true });
    },
  },
  {
    name: "through-mid-dir2",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 50, 30)), { type: "midPlane", depth: 20 });
      extrude(b, b.sketch(STD.front, (d) => d.circle(10, 15, 4)), { type: "throughAll", depth: 0 }, { cut: true });
      extrude(b, b.sketch(STD.front, (d) => d.circle(25, 15, 4)), { type: "throughAll", depth: 0 }, { cut: true, reverse: true });
      const pl = b.add("Plane", { type: "refplane", base: STD.front, offset: 40, flip: false });
      extrude(b, b.sketch({ kind: "refplane", featureId: pl }, (d) => d.rect(35, 10, 45, 20)), { type: "upToBody", depth: 0 }, { reverse: true });
      extrude(b, b.sketch(STD.front, (d) => d.rect(-10, 0, 0, 10)), blind(5), { dir2: blind(8) });
      const top = b.face(planar([0, 0, 1], 2, 10));
      extrude(b, b.sketch({ kind: "face", sig: top }, (d) => d.rect(5, 20, 15, 28)), { type: "blind", depth: 8, draft: deg(5) });
      extrude(b, b.sketch(STD.front, (d) => d.rect(26, 2, 32, 8)), blind(5), { from: { type: "offset", offset: 10 } });
      extrude(b, b.sketch(STD.front, (d) => d.rect(2, 2, 8, 8)), blind(4), { from: { type: "surface", ref: { kind: "face", sig: top } } });
    },
  },
  {
    name: "thin-contours",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.poly([[0, 0], [40, 0], [40, 30]], false)), blind(10), { thin: { type: "oneDirection", thickness: 2 } });
      extrude(b, b.sketch(STD.front, (d) => d.rect(60, 0, 90, 30)), blind(10), { thin: { type: "twoDirection", thickness: 2, thickness2: 1 } });
      extrude(b, b.sketch(STD.front, (d) => d.circle(120, 15, 10)), blind(10), { thin: { type: "midPlane", thickness: 2 } });
      const s = b.sketch(STD.front, (d) => {
        d.rect(0, 50, 40, 80);
        d.circle(20, 65, 5);
        d.rect(60, 50, 90, 80);
      });
      extrude(b, s, blind(5), { contours: [[5, 55]] });
      extrude(b, s, blind(8), { contours: [[20, 65], [75, 65]] });
    },
  },
  {
    name: "revolve",
    build(b) {
      const axisSketch = (draw: (d: Parameters<Parameters<PartBuilder["sketch"]>[1]>[0]) => void) => {
        let axis = "";
        const id = b.sketch(STD.front, (d) => {
          axis = d.line(d.point(0, -10), d.point(0, 60), true);
          draw(d);
        });
        return { id, axis };
      };
      const tube = axisSketch((d) => d.rect(10, 0, 20, 50));
      revolve(b, tube.id, tube.axis, deg(360));
      const notch = axisSketch((d) => d.rect(15, 40, 25, 50));
      revolve(b, notch.id, notch.axis, deg(90), { cut: true, revType: "midPlane" });
      const two = axisSketch((d) => d.rect(20, 0, 30, 10));
      revolve(b, two.id, two.axis, deg(60), { angle2: deg(30) });
      const thin = axisSketch((d) => d.poly([[20, 20], [30, 30]], false));
      revolve(b, thin.id, thin.axis, deg(360), { thin: { type: "oneDirection", thickness: 1 } });
      const ax = b.add("Axis", { type: "refaxis", refs: [planeRef(STD.front), planeRef(STD.right)] });
      const s = b.sketch(STD.front, (d) => d.rect(40, 0, 45, 5));
      b.add("Revolve", { type: "revolve", cut: false, sketchId: s, axis: "", axisRef: { kind: "axis", featureId: ax }, angle: deg(180), revType: "oneDirection", reverse: false, merge: true });
    },
  },
  {
    name: "curved-profiles",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => {
        // Slot: two lines and two semicircular arcs (arcs run counter-clockwise p1 → p2).
        const c1 = d.point(10, 10), c2 = d.point(40, 10);
        const a1 = d.point(10, 5), b1 = d.point(40, 5), b2 = d.point(40, 15), a2 = d.point(10, 15);
        d.line(a1, b1);
        d.arc(c2, b1, b2);
        d.line(b2, a2);
        d.arc(c1, a2, a1);
      }), blind(6));
      extrude(b, b.sketch(STD.front, (d) => d.ellipse(70, 10, 12, 6)), blind(4));
      extrude(b, b.sketch(STD.front, (d) => {
        const s = d.spline([[100, 0], [105, 12], [115, 15], [125, 5]]);
        const pts = (d.sk.entities[s] as Extract<SketchEntity, { type: "spline" }>).pts;
        d.line(pts[pts.length - 1], pts[0]);
      }), blind(3));
      const top = b.face(planar([0, 0, 1], 2, 6));
      extrude(b, b.sketch({ kind: "face", sig: top }, (d) => d.circle(25, 10, 3)), blind(2), { cut: true });
    },
  },
  {
    name: "fillet-chamfer",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 40, 30)), blind(20));
      // Vertical box edge at (x, y); chamfers shorten some, so the midpoint height varies.
      const vertical = (x: number, y: number) => b.edges(edgeAt((m) => near(m[0], x) && near(m[1], y) && m[2] > 0 && m[2] < 20, "LINE"));
      b.add("Fillet", { type: "fillet", radius: 3, edges: vertical(0, 0), faces: [] });
      b.add("Fillet", { type: "fillet", radius: 2, radius2: 4, edges: vertical(0, 30), faces: [] });
      // The two fillets join the three top edges at x ≤ 40 into one tangent chain, so this
      // chamfer propagates along it (as in SOLIDWORKS) and stops at the sharp x = 40 corners.
      b.add("Chamfer", { type: "chamfer", chamferType: "equal", distance: 2, edges: b.edges(edgeAt((m) => near(m[1], 30) && near(m[2], 20), "LINE")) });
      // Unequal chamfers stay on plain edges: round a filleted tangent loop OCCT returns an
      // invalid solid or fails (hard.ts H23).
      b.add("Chamfer", { type: "chamfer", chamferType: "distanceDistance", distance: 2, distance2: 3, edges: vertical(40, 0) });
      b.add("Chamfer", { type: "chamfer", chamferType: "angleDistance", distance: 2, angle: deg(30), edges: vertical(40, 30) });
    },
  },
  {
    name: "shell",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 40, 30)), blind(20));
      b.add("Fillet", { type: "fillet", radius: 4, edges: b.edges(edgeAt((m) => near(m[2], 10), "LINE")), faces: [] });
      b.add("Shell", { type: "shell", thickness: 2, faces: [b.face(planar([0, 0, 1], 2, 20))], outward: false });
    },
  },
  {
    name: "shell-outward",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.circle(0, 0, 10)), blind(20));
      b.add("Shell", { type: "shell", thickness: 1.5, faces: [b.face(planar([0, 0, 1], 2, 20))], outward: true });
    },
  },
  {
    name: "draft",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 40, 30)), blind(20));
      const sides = b.faces((s) => s.surface === "PLANE" && Math.abs(s.normal[2]) < 1e-6);
      b.add("Draft", { type: "draft", angle: deg(3), neutral: planeRef(STD.front), reverse: false, faces: sides });
    },
  },
  {
    name: "holes",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 80, 40)), blind(10));
      const face = b.face(planar([0, 0, 1], 2, 10));
      const hole = (holeType: "cbore" | "csk" | "hole" | "tap", size: string, end: { type: "blind" | "throughAll" | "upToNext"; depth: number }, positions: Vec2[]) =>
        b.add("Hole", { type: "hole", holeType, standard: "ISO", size, fit: "normal", end, face, positions });
      hole("hole", "M6", { type: "throughAll", depth: 0 }, [[10, 10], [10, 30]]);
      hole("cbore", "M5", { type: "blind", depth: 8 }, [[30, 20]]);
      hole("csk", "M6", { type: "throughAll", depth: 0 }, [[50, 20]]);
      hole("tap", "M8", { type: "blind", depth: 8 }, [[70, 20]]);
      hole("hole", "M3", { type: "upToNext", depth: 0 }, [[40, 35]]);
      const pts = b.sketch({ kind: "face", sig: face }, (d) => {
        d.sketchPoint(20, 5);
        d.sketchPoint(60, 5);
      });
      b.add("Hole", { type: "hole", holeType: "hole", standard: "ISO", size: "M4", fit: "close", end: { type: "blind", depth: 6 }, sketchId: pts, custom: { dia: 4.2 } });
    },
  },
  {
    name: "patterns",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 100, 60)), blind(5));
      const top = { kind: "face" as const, sig: b.face(planar([0, 0, 1], 2, 5)) };
      const boss = extrude(b, b.sketch(top, (d) => d.circle(10, 10, 4)), blind(6));
      const cut = extrude(b, b.sketch(top, (d) => d.rect(20, 45, 24, 55)), blind(3), { cut: true });
      b.add("LPattern", {
        type: "linearPattern",
        seeds: [boss],
        dir1: { ref: planeRef(STD.right), reverse: false, spacing: 15, count: 4 },
        dir2: { ref: planeRef(STD.top), reverse: false, spacing: 20, count: 2 },
      });
      const edge = b.edges(edgeAt((m) => near(m[1], 0) && near(m[2], 0), "LINE"))[0];
      b.add("LPattern", { type: "linearPattern", seeds: [cut], dir1: { ref: { kind: "edge", sig: edge }, reverse: false, spacing: 10, count: 3 } });
      const ax = b.add("Axis", { type: "refaxis", refs: [{ kind: "vertex", p: [50, 30, 0] }, { kind: "vertex", p: [50, 30, 10] }] });
      const pin = extrude(b, b.sketch(top, (d) => d.circle(70, 30, 3)), blind(4));
      b.add("CirPattern", { type: "circularPattern", seeds: [pin], axis: { kind: "axis", featureId: ax }, reverse: false, angle: deg(360), count: 6, equalSpacing: true });
      const mid = b.add("Plane", { type: "refplane", base: STD.right, offset: 50, flip: false });
      b.add("Mirror", { type: "mirror", seeds: [boss], plane: planeRef({ kind: "refplane", featureId: mid }) });
      b.add("Mirror", { type: "mirror", seeds: [], plane: planeRef(STD.front), body: true });
    },
  },
  {
    name: "rib",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.poly([[0, 0], [60, 0], [60, 5], [5, 5], [5, 50], [0, 50]])), { type: "midPlane", depth: 30 });
      const s = b.sketch(STD.front, (d) => d.line(d.point(20, 30), d.point(30, 20)));
      b.add("Rib", { type: "rib", sketchId: s, thickness: 3, side: "both", direction: "parallel", flip: false });
    },
  },
  {
    name: "sweep-loft",
    build(b) {
      const path = b.sketch(STD.front, (d) => {
        const p0 = d.point(0, 0), p1 = d.point(0, 30), p2 = d.point(10, 40), p3 = d.point(40, 40);
        d.line(p0, p1);
        d.arc(d.point(10, 30), p2, p1);
        d.line(p2, p3);
      });
      b.add("Sweep", { type: "sweep", cut: false, profileType: "circular", profile: "", diameter: 6, path, orientation: "followPath", merge: true });
      const path2 = b.sketch(STD.front, (d) => d.line(d.point(60, 0), d.point(60, 40)));
      const prof = b.sketch(STD.top, (d) => d.rect(57, -3, 63, 3));
      b.add("Sweep", { type: "sweep", cut: false, profileType: "sketch", profile: prof, diameter: 0, path: path2, orientation: "followPath", merge: true });
      const a = b.sketch(STD.front, (d) => d.rect(100, 0, 120, 20));
      const pl = b.add("Plane", { type: "refplane", base: STD.front, offset: 30, flip: false });
      const c = b.sketch({ kind: "refplane", featureId: pl }, (d) => d.circle(110, 10, 6));
      b.add("Loft", { type: "loft", cut: false, profiles: [a, c], merge: true });
    },
  },
  {
    name: "refgeom",
    build(b) {
      extrude(b, b.sketch(STD.front, (d) => d.rect(0, 0, 40, 30)), blind(20));
      extrude(b, b.sketch(STD.front, (d) => d.circle(40, 15, 8)), blind(20));
      const v = (p: [number, number, number]) => ({ kind: "vertex" as const, p });
      const face = (n: [number, number, number], z: number) => ({ kind: "face" as const, sig: b.face(planar(n, 2, z)) });
      const xEdge = { kind: "edge" as const, sig: b.edges(edgeAt((m) => near(m[1], 0) && near(m[2], 0), "LINE"))[0] };
      const arc = { kind: "edge" as const, sig: b.edges(edgeAt((m) => near(m[2], 20), "CIRCLE"))[0] };
      // replicad (OCCT) names the cylinder surface type "CYLINDRE".
      const cyl = { kind: "face" as const, sig: b.face((s) => s.surface === "CYLINDRE") };
      const plane = (refs: unknown[], o: Record<string, unknown> = {}) => b.add("Plane", { type: "refplane", base: STD.front, offset: 0, flip: false, refs, ...o } as never);
      plane([v([0, 0, 20]), v([40, 0, 20]), v([0, 30, 0])]);
      plane([face([0, 0, -1], 0), face([0, 0, 1], 20)]);
      plane([planeRef(STD.top), xEdge], { angle: deg(30) });
      plane([planeRef(STD.right), v([40, 30, 20])]);
      plane([arc, v([48, 15, 20])]);
      plane([face([0, 0, 1], 20)], { offset: 5 });
      b.add("Axis", { type: "refaxis", refs: [planeRef(STD.front), planeRef(STD.right)] });
      b.add("Axis", { type: "refaxis", refs: [v([0, 0, 0]), v([40, 30, 20])] });
      b.add("Axis", { type: "refaxis", refs: [cyl] });
      b.add("Axis", { type: "refaxis", refs: [planeRef(STD.top), v([10, 10, 10])] });
      b.add("Point", { type: "refpoint", refs: [arc] });
      b.add("Point", { type: "refpoint", refs: [], xyz: [5, 5, 5] });
      b.add("Point", { type: "refpoint", refs: [face([0, 0, 1], 20)] });
      b.add("Coordinate System", { type: "coordsys", origin: v([40, 30, 20]), xAxis: xEdge, yAxis: planeRef(STD.front) });
    },
  },
  {
    name: "imported",
    async build(b) {
      const data = await stepAsset();
      await ensureImports([{ type: "imported", data } as Feature]);
      b.add("Imported", { type: "imported", format: "step", data });
      const top = b.face(planar([0, 0, 1], 2, 10));
      extrude(b, b.sketch({ kind: "face", sig: top }, (d) => d.circle(15, 8, 4)), blind(5));
    },
  },
];
