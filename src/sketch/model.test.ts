import { describe, expect, it } from "vitest";
import { emptySketch, isEmptySketch } from "./model";

describe("isEmptySketch", () => {
  it("is true for a new sketch with only the origin", () => {
    expect(isEmptySketch(emptySketch())).toBe(true);
  });
  it("is false once anything is drawn, related or dimensioned", () => {
    const pt = emptySketch();
    pt.points.p1 = { id: "p1", x: 1, y: 2, standalone: true };
    expect(isEmptySketch(pt)).toBe(false);

    const ln = emptySketch();
    ln.points.p1 = { id: "p1", x: 0, y: 0 };
    ln.points.p2 = { id: "p2", x: 5, y: 0 };
    ln.entities.l1 = { id: "l1", type: "line", p1: "p1", p2: "p2" };
    expect(isEmptySketch(ln)).toBe(false);
  });
});
