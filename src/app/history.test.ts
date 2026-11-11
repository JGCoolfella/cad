import { describe, expect, it } from "vitest";
import { History, TextHistory } from "./history";
import type { PartDocument } from "../core/types";

const doc = (n: number): PartDocument => ({
  format: "opencad-part",
  version: 1,
  name: "Part1",
  units: "MMGS",
  rollback: null,
  features: Array.from({ length: n }, (_, i) => ({ id: `f${i}`, type: "refplane", name: `Plane${i}`, base: { kind: "std", plane: "front" }, offset: i, flip: false })) as PartDocument["features"],
});

describe("History", () => {
  it("undoes and redoes whole documents", () => {
    const h = new History();
    const d1 = doc(1);
    h.push(d1, "A");
    const d2 = doc(2);
    expect(h.undo(d2)?.features.length).toBe(1);
    expect(h.canRedo).toBe(true);
    expect(h.redo(d1)?.features.length).toBe(2);
  });

  it("charges only for features that changed", () => {
    const h = new History();
    const big = doc(50);
    h.push(big, "A");
    const first = h.size;
    const next = doc(50);
    next.features[0].name = "Changed";
    h.push(next, "B");
    expect(h.size - first).toBeLessThan(first / 10);
  });

  it("trims the oldest steps to the budget", () => {
    const h = new History(5);
    for (let i = 0; i < 20; i++) h.push(doc(i + 1), String(i));
    expect(h.steps).toBe(5);
  });

  it("a new action clears redo", () => {
    const h = new History();
    h.push(doc(1), "A");
    h.undo(doc(2));
    h.push(doc(3), "B");
    expect(h.canRedo).toBe(false);
  });
});

describe("TextHistory", () => {
  it("merges identical consecutive states and clears redo", () => {
    const h = new TextHistory();
    h.push("a");
    h.push("a");
    expect(h.undo("b")).toBe("a");
    expect(h.canUndo).toBe(false);
    h.push("c");
    expect(h.canRedo).toBe(false);
  });
});
