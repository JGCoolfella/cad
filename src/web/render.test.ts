import { describe, expect, it } from "vitest";
import { hasWebGpu, startViewport } from "./render";

describe("the browser's viewport", () => {
  it("falls back to three.js without WebGPU", async () => {
    expect(hasWebGpu()).toBe(false); // Node has no navigator.gpu
    const m = await startViewport(null);
    expect(m.kind).toBe("three");
    if (m.kind === "three") {
      expect(m.assemblies).toBe(false);
      expect(m.reason).toMatch(/WebGPU/);
    }
  });
});
