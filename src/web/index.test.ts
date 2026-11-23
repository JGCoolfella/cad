import { afterEach, describe, expect, it, vi } from "vitest";
import type { EngineIn, EngineOut } from "./protocol";
import { WebEngine, isEngineCommand } from "./index";
import { invoke, isNative, useWebIpc } from "../native/tauri";

/** A stand-in engine worker: records what it is sent, answers what the test says. */
class FakeWorker extends EventTarget {
  static all: FakeWorker[] = [];
  sent: EngineIn[] = [];
  terminated = false;
  constructor() {
    super();
    FakeWorker.all.push(this);
  }
  postMessage(m: EngineIn) {
    this.sent.push(m);
  }
  terminate() {
    this.terminated = true;
  }
  emit(m: EngineOut) {
    this.dispatchEvent(new MessageEvent("message", { data: m }));
  }
  last<T extends EngineIn["type"]>(type: T) {
    return this.sent.filter((m) => m.type === type).at(-1) as Extract<EngineIn, { type: T }>;
  }
}

const bytes = (s: string) => new TextEncoder().encode(s).buffer as ArrayBuffer;

function engine(o: Partial<ConstructorParameters<typeof WebEngine>[0]> = {}) {
  vi.stubGlobal("Worker", FakeWorker);
  FakeWorker.all = [];
  const e = new WebEngine({ moduleUrl: "file:///oc-web.js", hosts: 2, ...o });
  return { e, w: FakeWorker.all[0] };
}

afterEach(() => {
  vi.unstubAllGlobals();
  useWebIpc(undefined);
});

describe("the browser's engine workers (W1b)", () => {
  it("routes the engine's commands as Tauri's invoke answers them", async () => {
    const { e, w } = engine();
    expect(w.sent[0]).toMatchObject({ type: "start", moduleUrl: "file:///oc-web.js", hosts: 2, budgetMb: 1024 });
    const json = e.invoke("engine_mass_props", { features: [] });
    const m = w.last("invoke");
    expect(m).toMatchObject({ cmd: "engine_mass_props", args: { features: [] } });
    w.emit({ type: "reply", id: m.id, ok: true, json: bytes('{"volume":2}') });
    await expect(json).resolves.toEqual({ volume: 2 });
    const bin = e.invoke("engine_rebuild", { features: [] });
    const b = bytes("OCR1");
    w.emit({ type: "reply", id: w.last("invoke").id, ok: true, bytes: b, stats: { deferrals: 1, hostParts: 3, engineMs: 1, hostsMs: 2 } });
    await expect(bin).resolves.toBe(b);
    expect(e.lastStats?.hostParts).toBe(3);
    const bad = e.invoke("asm_drag", {});
    w.emit({ type: "reply", id: w.last("invoke").id, ok: false, error: "no assembly is built" });
    await expect(bad).rejects.toBe("no assembly is built");
  });

  it("answers the window's commands itself and leaves view_* to the render worker", async () => {
    const render = vi.fn(async () => "rendered");
    const { e, w } = engine({ render });
    await expect(e.invoke("app_pick_file", { save: false, kinds: [] })).resolves.toBeNull();
    await expect(e.invoke("app_ready")).resolves.toBeNull();
    await expect(e.invoke("view_pick", { req: 1 })).resolves.toBe("rendered");
    expect(render).toHaveBeenCalledWith("view_pick", { req: 1 }, undefined);
    await expect(e.invoke("smoke_report")).rejects.toMatch(/not available/);
    expect(w.sent.filter((m) => m.type === "invoke")).toHaveLength(0);
    const { e: bare } = engine();
    await expect(bare.invoke("view_frame", {})).rejects.toMatch(/no viewport/);
  });

  it("starts another engine when one stops, failing what it held", async () => {
    const { e, w } = engine();
    const p = e.invoke("asm_build", {});
    w.emit({ type: "fatal", error: "RuntimeError: unreachable" });
    await expect(p).rejects.toMatch(/the engine stopped: RuntimeError/);
    expect(w.terminated).toBe(true);
    expect(e.starts).toBe(2);
    const w2 = FakeWorker.all[1];
    const q = e.invoke("engine_clear_cache");
    w2.emit({ type: "reply", id: w2.last("invoke").id, ok: true, json: bytes("null") });
    await expect(q).resolves.toBeNull();
  });

  it("is the seam: with it installed the page runs on the native engine", async () => {
    expect(isNative()).toBe(false);
    const { e, w } = engine();
    useWebIpc(e);
    expect(isNative()).toBe(true);
    const r = invoke<string>("app_read_text", { path: "/work/a.ocpart" });
    const m = w.last("invoke");
    expect(m.cmd).toBe("app_read_text");
    w.emit({ type: "reply", id: m.id, ok: true, json: bytes('"text"') });
    await expect(r).resolves.toBe("text");
    expect(["engine_export_to", "asm_save", "app_write_text"].every(isEngineCommand)).toBe(true);
    expect(["view_frame", "app_info", "app_doc"].some(isEngineCommand)).toBe(false);
  });
});
