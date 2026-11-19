/**
 * Edit Part in context (G1d): in-context references in the sketch (Convert Entities of
 * another component's edges follows them and still makes profiles), placement into the
 * part's coordinates, and SOLIDWORKS' `->` / `->?` in the FeatureManager.
 */
import { describe, expect, it } from "vitest";
import type { App } from "../app/app";
import { STD_FRAMES } from "../core/math";
import type { EdgeSig, ExternalRef, Feature } from "../core/types";
import { emptySketch } from "../sketch/model";
import { convertExternal, pruneUnusedRefs, updateRefGeometry } from "../sketch/refs";
import { externalSuffix, placedEdgeSig, placedFaceSig } from "./context";
import { isContextItem, CTX_BODY } from "./contextIds";

const ext: ExternalRef = { assembly: "C:/w/a.ocasm", component: ["i1"], context: ["i2"], entity: {}, label: "Edge@plate<1>" };
const circle: EdgeSig = { curve: "CIRCLE", a: [8, 0, 3], b: [8, 0, 3], mid: [2, 0, 3], length: 6 * Math.PI };

describe("in-context Convert Entities", () => {
  it("makes a profile circle that follows the other component's edge", () => {
    const sk = emptySketch();
    const id = convertExternal(sk, { kind: "external", sig: circle, ext }, STD_FRAMES.front)!;
    const e = sk.entities[id];
    expect(e.type === "circle" && !("ext" in e && e.ext) && e.r).toBeCloseTo(3, 12);
    const r = Object.values(sk.refs ?? {})[0];
    expect(r.kind).toBe("external");
    // Kept when the sketch closes (the circle uses it), and moved with it.
    pruneUnusedRefs(sk);
    expect(Object.keys(sk.refs ?? {})).toHaveLength(1);
    const moved = { ...r, sig: { ...circle, a: [13, 4, 3], b: [13, 4, 3], mid: [7, 4, 3] } } as typeof r;
    sk.refs![r.id] = moved;
    updateRefGeometry(sk, STD_FRAMES.front);
    if (e.type !== "circle") throw new Error();
    expect([sk.points[e.c].x, sk.points[e.c].y]).toEqual([10, 4]);
    expect(Object.values(sk.relations).some((x) => x.type === "equal")).toBe(true);
  });

  it("shares a line's end points with the reference geometry", () => {
    const sk = emptySketch();
    const sig: EdgeSig = { curve: "LINE", a: [0, 0, 0], b: [10, 0, 0], mid: [5, 0, 0], length: 10 };
    const id = convertExternal(sk, { kind: "external", sig, ext }, STD_FRAMES.front)!;
    const e = sk.entities[id];
    expect(e.type).toBe("line");
    if (e.type === "line") expect(sk.points[e.p1].ext).toBeTruthy();
    // An edge seen end-on converts to nothing.
    expect(convertExternal(sk, { kind: "external", sig: { ...sig, b: [0, 0, 9], mid: [0, 0, 4.5] }, ext }, STD_FRAMES.front)).toBeNull();
  });
});

describe("in-context references", () => {
  it("are placed into the part's coordinates", () => {
    const t = { position: [10, 0, 5] as [number, number, number], rotation: [Math.SQRT1_2, 0, 0, Math.SQRT1_2] as [number, number, number, number] };
    const f = placedFaceSig({ surface: "PLANE", center: [1, 0, 0], normal: [1, 0, 0], area: 4, name: ["f1/end/k"] }, t);
    expect(f.center.map((x) => +x.toFixed(9) + 0)).toEqual([10, 1, 5]);
    expect(f.normal.map((x) => +x.toFixed(9) + 0)).toEqual([0, 1, 0]);
    expect(f.name).toBeUndefined();
    expect(placedEdgeSig(circle, t).mid.map((x) => +x.toFixed(9) + 0)).toEqual([10, 2, 8]);
  });

  it("read -> in context and ->? out of it, on the sketch and the feature made from it", () => {
    const sketch = { id: "s1", type: "sketch", name: "Sketch1", plane: { kind: "external", sig: { surface: "PLANE", center: [0, 0, 3], normal: [0, 0, 1], area: 1 }, ext }, sketch: emptySketch() } as Feature;
    const boss = { id: "f1", type: "extrude", name: "Boss-Extrude1", sketchId: "s1" } as unknown as Feature;
    const app = (paths: string[]) => ({ asmStack: paths.map((p) => ({ session: { path: p } })), featureById: (id: string) => (id === "s1" ? sketch : undefined) }) as unknown as App;
    expect(externalSuffix(app(["C:/w/a.ocasm"]), sketch)).toBe("->");
    expect(externalSuffix(app(["c:/W/A.ocasm"]), boss)).toBe("->");
    expect(externalSuffix(app([]), sketch)).toBe("->?");
    expect(externalSuffix(app(["C:/w/other.ocasm"]), boss)).toBe("->?");
    expect(externalSuffix(app([]), { ...boss, sketchId: "none" } as Feature)).toBe("");
  });

  it("tells picks on other components apart", () => {
    expect(isContextItem({ kind: "face", id: 3, body: CTX_BODY + 1 })).toBe(true);
    expect(isContextItem({ kind: "face", id: 3 })).toBe(false);
    expect(isContextItem({ kind: "face", id: 3, body: 2 })).toBe(false);
  });
});
