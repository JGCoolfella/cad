import { describe, expect, it } from "vitest";
import { DISARMED, enterStep, type EnterState } from "./enterKey";

/** Replays keys ("type" = an edit in the box) and records which Enters reached OK. */
function run(keys: string[]) {
  let s: EnterState = DISARMED;
  const out: string[] = [];
  for (const k of keys) {
    if (k === "type") {
      s = DISARMED;
      continue;
    }
    const step = enterStep(s, k);
    s = step.next;
    if (k === "Enter") out.push(step.propagate ? "ok" : "apply");
  }
  return out;
}

describe("Enter in a PropertyManager value box", () => {
  it("first Enter applies the value, the second one is OK", () => {
    expect(run(["type", "Enter", "Enter"])).toEqual(["apply", "ok"]);
  });

  it("typing again after applying needs two Enters again", () => {
    expect(run(["type", "Enter", "type", "Enter", "Enter"])).toEqual(["apply", "apply", "ok"]);
  });

  it("other keys stay in the box and do not reset", () => {
    const s = enterStep({ armed: true }, "ArrowLeft");
    expect(s).toEqual({ apply: false, propagate: false, next: { armed: true } });
  });

  it("every Enter applies the value", () => {
    expect(enterStep(DISARMED, "Enter").apply).toBe(true);
    expect(enterStep({ armed: true }, "Enter").apply).toBe(true);
  });
});
