import { describe, expect, it } from "vitest";
import type { EdgeSig, Frame } from "../core/types";
import { STD_FRAMES } from "../core/math";
import { emptySketch } from "./model";
import { addRef, pruneUnusedRefs, refShape, updateRefGeometry } from "./refs";
import { SketchSystem, solveSketch } from "./solver";

const FRONT: Frame = STD_FRAMES.front;
const line = (a: [number, number, number], b: [number, number, number]): EdgeSig => ({
  curve: "LINE",
  a,
  b,
  mid: [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2],
  length: Math.hypot(a[0] - b[0], a[1] - b[1], a[2] - b[2]),
});

describe("sketch model references", () => {
  it("projects vertices, lines, circles and arcs onto the sketch plane", () => {
    expect(refShape({ kind: "vertex", p: [3, 4, 7] }, FRONT)).toEqual({ kind: "point", p: [3, 4] });
    expect(refShape({ kind: "edge", sig: line([0, 0, 5], [10, 0, 5]) }, FRONT)).toEqual({ kind: "line", a: [0, 0], b: [10, 0] });
    // An edge seen end-on has no sketch counterpart.
    expect(refShape({ kind: "edge", sig: line([1, 1, 0], [1, 1, 9]) }, FRONT)).toBeNull();
    const circle: EdgeSig = { curve: "CIRCLE", a: [5, 0, 2], b: [5, 0, 2], mid: [-5, 0, 2], length: 10 * Math.PI };
    expect(refShape({ kind: "edge", sig: circle }, FRONT)).toEqual({ kind: "circle", c: [0, 0], r: 5 });
    const arc: EdgeSig = { curve: "CIRCLE", a: [5, 0, 0], b: [-5, 0, 0], mid: [0, 5, 0], length: 5 * Math.PI };
    const s = refShape({ kind: "edge", sig: arc }, FRONT);
    expect(s?.kind).toBe("arc");
    if (s?.kind === "arc") expect(s.c.map((x) => +x.toFixed(9))).toEqual([0, 0]);
    // A circle tilted out of the sketch plane would project to an ellipse.
    const tilted: EdgeSig = { curve: "CIRCLE", a: [5, 0, 0], b: [-5, 0, 3], mid: [0, 5, 1.5], length: 5 * Math.PI };
    expect(refShape({ kind: "edge", sig: tilted }, FRONT)).toBeNull();
  });

  it("reuses the reference for the same model geometry", () => {
    const sk = emptySketch();
    const a = addRef(sk, { kind: "edge", sig: line([0, 0, 0], [10, 0, 0]) }, FRONT);
    const b = addRef(sk, { kind: "edge", sig: line([0, 0, 0], [10, 0, 0]) }, FRONT);
    expect(a?.entity).toBeTruthy();
    expect(b).toEqual(a);
    expect(Object.keys(sk.refs!)).toHaveLength(1);
  });

  it("keeps reference geometry fixed while the rest solves against it", () => {
    const sk = emptySketch();
    const v = addRef(sk, { kind: "vertex", p: [20, 10, 0] }, FRONT)!;
    const e = addRef(sk, { kind: "edge", sig: line([0, 10, 0], [20, 10, 0]) }, FRONT)!;
    sk.points.m = { id: "m", x: 3, y: 3, standalone: true };
    sk.relations.r1 = { id: "r1", type: "midpoint", p: "m", e: e.entity! };
    sk.points.q = { id: "q", x: 1, y: 1, standalone: true };
    sk.points.k = { id: "k", x: 30, y: 30 };
    sk.entities.l = { id: "l", type: "line", p1: v.point!, p2: "k" };
    solveSketch(sk);
    expect([sk.points.m.x, sk.points.m.y].map((x) => +x.toFixed(9))).toEqual([10, 10]);
    expect([sk.points[v.point!].x, sk.points[v.point!].y]).toEqual([20, 10]);
    // Reference points are constants, not unknowns.
    const sys = new SketchSystem(sk);
    expect(sys.pts.get(v.point!)!.ix).toBe(-1);
  });

  it("moves reference geometry when the model changes, then the sketch follows", () => {
    const sk = emptySketch();
    const e = addRef(sk, { kind: "edge", sig: line([0, 10, 0], [20, 10, 0]) }, FRONT)!;
    sk.points.m = { id: "m", x: 10, y: 10, standalone: true };
    sk.relations.r1 = { id: "r1", type: "midpoint", p: "m", e: e.entity! };
    const id = Object.keys(sk.refs!)[0];
    sk.refs![id] = { id, kind: "edge", sig: line([0, 10, 0], [30, 10, 0]) };
    expect(updateRefGeometry(sk, FRONT)).toEqual([]);
    solveSketch(sk);
    expect([sk.points.m.x, sk.points.m.y].map((x) => +x.toFixed(9))).toEqual([15, 10]);
  });

  it("drops references nothing uses", () => {
    const sk = emptySketch();
    addRef(sk, { kind: "vertex", p: [1, 2, 0] }, FRONT);
    const used = addRef(sk, { kind: "edge", sig: line([0, 0, 0], [5, 0, 0]) }, FRONT)!;
    sk.points.p = { id: "p", x: 2, y: 0, standalone: true };
    sk.relations.r = { id: "r", type: "coincident", p: "p", e: used.entity! };
    pruneUnusedRefs(sk);
    expect(Object.values(sk.refs!).map((r) => r.kind)).toEqual(["edge"]);
    expect(Object.values(sk.points).filter((p) => p.ext)).toHaveLength(2);
    delete sk.relations.r;
    pruneUnusedRefs(sk);
    expect(sk.refs).toBeUndefined();
    expect(Object.values(sk.points).filter((p) => p.ext)).toHaveLength(0);
  });
});
