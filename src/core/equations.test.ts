import { describe, expect, it } from "vitest";
import { evaluate } from "./equations";

describe("evaluate", () => {
  const vars: Record<string, number> = { Width: 80, "D1@Sketch1": 25 };
  const look = (n: string) => {
    if (!(n in vars)) throw new Error("unknown " + n);
    return vars[n];
  };
  it("handles arithmetic and precedence", () => {
    expect(evaluate("1 + 2 * 3", look)).toBe(7);
    expect(evaluate("(1 + 2) * 3", look)).toBe(9);
    expect(evaluate("2 ^ 3 ^ 2", look)).toBe(512);
    expect(evaluate("-4 + 10", look)).toBe(6);
  });
  it("resolves quoted names and a leading =", () => {
    expect(evaluate('= "Width" / 2 + "D1@Sketch1"', look)).toBe(65);
  });
  it("uses degrees for trigonometry", () => {
    expect(evaluate("sin(30)", look)).toBeCloseTo(0.5);
    expect(evaluate("atan(1)", look)).toBeCloseTo(45);
    expect(evaluate("sqrt(16) + abs(-2) + int(2.7)", look)).toBe(8);
  });
  it("reports errors", () => {
    expect(() => evaluate('"Nope" + 1', look)).toThrow();
    expect(() => evaluate("1 +", look)).toThrow();
    expect(() => evaluate("foo(2)", look)).toThrow();
  });
});
