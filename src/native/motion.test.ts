import { describe, expect, it } from "vitest";
import { SYSTEM_PAGES, type Ctl } from "../app/optionsSchema";
import { LOD_DEFAULT, LOD_MAX, SETTLE_MS, ViewMotion, lodApplies, lodSlider } from "./motion";

const cam = (h: number, q = 0) => ({ target: [0, 0, 0], quat: [q, 0, 0, 1], height: h, perspective: false, width: 800, heightPx: 600 });

describe("ViewMotion (G2b)", () => {
  it("the first frame and frames with an unchanged camera are at rest", () => {
    const m = new ViewMotion();
    expect(m.update(0, cam(100), false)).toBe(false);
    expect(m.update(500, cam(100), false)).toBe(false);
    expect(m.settleIn(500)).toBeNull();
  });

  it("a drag or a transition moves the view until it ends; the next frame is at rest", () => {
    const m = new ViewMotion();
    m.update(0, cam(100), false);
    // Middle-drag rotate: every frame of the drag is moving, also while the pointer rests.
    expect(m.update(10, cam(100, 0.1), true)).toBe(true);
    expect(m.update(20, cam(100, 0.2), true)).toBe(true);
    expect(m.update(900, cam(100, 0.2), true)).toBe(true);
    expect(m.settleIn(900)).toBeNull();
    // Released: full detail at once, even though the camera moved moments ago.
    expect(m.update(905, cam(100, 0.2), false)).toBe(false);
    // A view transition's last frame (the animation has just ended) is at rest too.
    expect(m.update(1000, cam(90, 0.3), true)).toBe(true);
    expect(m.update(1016, cam(80, 0.4), false)).toBe(false);
  });

  it("a camera change without an end (wheel zoom, arrow keys) moves the view for SETTLE_MS", () => {
    const m = new ViewMotion();
    m.update(0, cam(100), false);
    expect(m.update(1000, cam(80), false)).toBe(true);
    expect(m.settleIn(1000)).toBe(SETTLE_MS);
    // Another wheel step restarts the wait.
    expect(m.update(1100, cam(64), false)).toBe(true);
    expect(m.settleIn(1150)).toBe(SETTLE_MS - 50);
    // A frame for something else (hover) during the wait is still moving.
    expect(m.update(1200, cam(64), false)).toBe(true);
    // Settled.
    expect(m.update(1100 + SETTLE_MS, cam(64), false)).toBe(false);
    expect(m.settleIn(1100 + SETTLE_MS)).toBeNull();
  });

  it("a resize of the graphics area is not a view operation", () => {
    const m = new ViewMotion();
    m.update(0, cam(100), false);
    const resized = { ...cam(100), width: 1000, heightPx: 700 };
    expect(m.update(10, resized, false)).toBe(false);
  });
});

describe("Level of detail option (G2b)", () => {
  it("applies to assemblies, parts edited in context and multibody parts", () => {
    expect(lodApplies({ assembly: true, context: false, solids: 1 })).toBe(true);
    expect(lodApplies({ assembly: false, context: true, solids: 1 })).toBe(true);
    expect(lodApplies({ assembly: false, context: false, solids: 2 })).toBe(true);
    expect(lodApplies({ assembly: false, context: false, solids: 1 })).toBe(false);
    expect(lodApplies({ assembly: false, context: false, solids: undefined })).toBe(false);
  });

  it("Tools > Options has the slider: 0 (Off) to 20, live", () => {
    const ctl = SYSTEM_PAGES.flatMap((p) => p.items as unknown[]).flatMap(function all(c: unknown): Ctl[] {
      const x = c as Ctl & { items?: unknown[] };
      return [x, ...(x.items ?? []).flatMap(all)];
    }).find((c) => "key" in c && c.key === "performance.lod");
    expect(ctl).toMatchObject({ t: "slider", min: 0, max: LOD_MAX, def: LOD_DEFAULT, live: true });
  });

  it("the slider is 0 (Off) to 20, the default when unset", () => {
    expect(lodSlider(undefined)).toBe(10);
    expect(lodSlider(0)).toBe(0);
    expect(lodSlider(7)).toBe(7);
    expect(lodSlider(99)).toBe(20);
    expect(lodSlider(-3)).toBe(0);
    expect(lodSlider("x")).toBe(10);
  });
});
