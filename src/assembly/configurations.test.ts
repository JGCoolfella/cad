/**
 * Assembly configurations and display states (G1e): the TypeScript mirror of
 * `oc_doc::config` (capture, apply, display states linked to configurations), and the
 * configuration commands greyed outside the native app.
 */
import { describe, expect, it } from "vitest";
import { App } from "../app/app";
import { CMD, type Ctx } from "../commands/commands";
import { addConfiguration, setConfiguredSuppression, setConfiguredValue } from "../core/configurations";
import { activateDisplayState, addDisplayState, applyAssembly, captureAssembly, deleteDisplayState, displayStatesOf, ensureDisplayStates, referencedConfiguration, renameDisplayState } from "./configurations";
import type { AssemblyDocument } from "./types";

const I = { position: [0, 0, 0] as [number, number, number], rotation: [1, 0, 0, 0] as [number, number, number, number] };

function doc(): AssemblyDocument {
  return {
    format: "opencad-assembly",
    version: 2,
    name: "asm",
    units: "MMGS",
    components: [
      { id: "c1", kind: "part", path: "plate.ocpart", config: "Default" },
      { id: "c2", kind: "part", path: "pin.ocpart", config: "Default" },
    ],
    instances: [
      { id: "i1", component: "c1", name: "plate<1>", fixed: true, transform: I },
      { id: "i2", component: "c2", name: "pin<1>", fixed: false, transform: I },
      { id: "i3", component: "c2", name: "pin<2>", fixed: false, transform: I, color: "#00ff00" },
    ],
    mates: [{ id: "m1", name: "Distance1", type: "distance", refs: [], align: "aligned", value: 2 }],
  };
}

const clone = <T,>(x: T): T => JSON.parse(JSON.stringify(x));

describe("assembly configurations", () => {
  it("suppress, mate values and referenced configurations per configuration; looks per display state", () => {
    const d = doc();
    const original = clone(d);
    // Add "Short" with its own display state (as AssemblySession.addConfiguration does).
    ensureDisplayStates(d);
    addConfiguration(d, "Short", undefined, () => captureAssembly(d));
    addDisplayState(d);
    expect(d.configurations!.active).toBe("c2");
    expect(displayStatesOf(d).list.map((s) => s.name)).toEqual(["Display State-1", "Display State-2"]);
    // In Short: pin<2> suppressed, the distance 5, pin<1> uses the pin's "Long", pin<1> hidden.
    setConfiguredSuppression(d, "i3", false, true, "this");
    d.instances[2].suppressed = true;
    setConfiguredValue(d, "m1/value", 2, 5, "this");
    d.mates[0].value = 5;
    for (const c of d.configurations!.list) (c.components ??= {}).i2 = c.id === "c2" ? "Long" : "Default";
    d.instances[1].config = "Long";
    d.instances[1].hidden = true;
    expect(referencedConfiguration(d, d.instances[1])).toBe("Long");
    const short = clone(d);
    // Default: as it was (its display state keeps pin<2> green, nothing hidden).
    captureAssembly(d);
    applyAssembly(d, "Default");
    expect(d.instances).toEqual(original.instances);
    expect(d.mates).toEqual(original.mates);
    expect(displayStatesOf(d).active).toBe("d1");
    // Short again: as it was left.
    captureAssembly(d);
    applyAssembly(d, "c2");
    expect(d.instances).toEqual(short.instances);
    expect(d.mates[0].value).toBe(5);
    expect(() => applyAssembly(d, "Nope")).toThrow(/No configuration/);
  });

  it("display states: activate, rename, delete rules", () => {
    const d = doc();
    addDisplayState(d, "Red");
    d.instances[0].color = "#ff0000";
    activateDisplayState(d, "d1");
    expect(d.instances[0].color).toBeUndefined();
    expect(d.instances[2].color).toBe("#00ff00");
    activateDisplayState(d, "d2");
    expect(d.instances[0].color).toBe("#ff0000");
    expect(() => addDisplayState(d, "Red")).toThrow(/already exists/);
    renameDisplayState(d, "d2", "Crimson");
    expect(displayStatesOf(d).list[1].name).toBe("Crimson");
    expect(() => deleteDisplayState(d, "d2")).toThrow(/active/);
    activateDisplayState(d, "d1");
    // d2 is linked to the active configuration (Default) only while active; once d1 is
    // active again the configuration links d1 and d2 can go.
    deleteDisplayState(d, "d2");
    expect(displayStatesOf(d).list.map((s) => s.id)).toEqual(["d1"]);
    expect(() => deleteDisplayState(d, "d1")).toThrow(/only display state/);
  });

  it("configuration commands are greyed out outside the native app", () => {
    const app = { sketch: null, pm: null, asm: null, leftTab: "tree", configDialog: null, changed() {} } as unknown as App;
    app.configurationsAvailable = App.prototype.configurationsAvailable.bind(app);
    const ctx = { app, vp: null, ui: {} } as unknown as Ctx;
    for (const id of ["config.add", "config.addDisplayState", "config.manager"]) {
      expect(CMD[id], id).toBeDefined();
      expect(CMD[id].enabled?.(ctx), id).toBe(false);
    }
    for (const id of ["config.designTable", "edit.suppressSpecified", "edit.unsuppressSpecified"]) expect(CMD[id]?.todo, id).toBe(true);
    for (const id of ["edit.suppressThis", "edit.suppressAll", "edit.unsuppressThis", "edit.unsuppressAll"]) expect(CMD[id], id).toBeDefined();
  });
});
