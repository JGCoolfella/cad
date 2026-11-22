import { describe, expect, it } from "vitest";
import { decodeBuild, StaleBase } from "./buildReply";
import type { Transform } from "./types";

/** Packs a reply as `crates/oc-app/src/pack.rs` does (`OCR1`, header, aligned arrays). */
class Packer {
  private parts: { at: number; bytes: Uint8Array }[] = [];
  private len = 0;
  private push(bytes: Uint8Array, align: number) {
    while (this.len % align) this.len++;
    const at = this.len;
    this.parts.push({ at, bytes });
    this.len += bytes.length;
    return at;
  }
  f64(v: number[]) {
    return { $b: this.push(new Uint8Array(new Float64Array(v).buffer), 8), n: v.length, t: "f64" };
  }
  u32(v: number[]) {
    return { $b: this.push(new Uint8Array(new Uint32Array(v).buffer), 4), n: v.length, t: "u32" };
  }
  finish(head: unknown): ArrayBuffer {
    let text = new TextEncoder().encode(JSON.stringify(head));
    const pad = (8 - (text.length % 8)) % 8;
    text = new Uint8Array([...text, ...new Array(pad).fill(32)]);
    const out = new Uint8Array(8 + text.length + this.len);
    out.set(new TextEncoder().encode("OCR1"), 0);
    new DataView(out.buffer).setUint32(4, text.length, true);
    out.set(text, 8);
    for (const p of this.parts) out.set(p.bytes, 8 + text.length + p.at);
    return out.buffer;
  }
}

const tr = (x: number): Transform => ({ position: [x, 0, 0], rotation: [1, 0, 0, 0] });
const flat = (t: Transform) => [...t.position, ...t.rotation];

const common = (gen: number, base: number) => ({
  gen,
  base,
  components: [{ id: "c1", path: "a.ocpart", found: "Relative", part: "h0", error: null, warning: null, configs: ["Default"] }],
  parts: { h0: { name: "a", handle: 40 + gen, lightweight: false, errors: [], bbox: null, external: false } },
  dof: 6,
  converged: true,
  newestError: null,
  heals: [],
  timeMs: 1,
});

/** A whole reply: `n` instances, one occurrence each, one mate. */
function whole(n: number, gen = 1): ArrayBuffer {
  const p = new Packer();
  const ids = Array.from({ length: n }, (_, k) => `i${k}`);
  return p.finish({
    ...common(gen, 0),
    instances: { ids, tr: p.f64(ids.flatMap((_, k) => flat(tr(k)))), st: p.u32(ids.map((_, k) => (k === 0 ? 0 : 1 | (6 << 8)))) },
    occurrences: { paths: ids.map((id) => [id]), part: p.u32(ids.map(() => 0)), w: p.f64(ids.flatMap((_, k) => flat(tr(k)))), hidden: p.u32(ids.map(() => 0)) },
    hashes: ["h0"],
    mates: [{ id: "m1", status: "ok", message: null, missing: [] }],
  });
}

describe("decodeBuild", () => {
  it("decodes a whole reply", () => {
    const r = decodeBuild(whole(3), null);
    expect(r.gen).toBe(1);
    expect(r.instances).toEqual([
      { id: "i0", transform: tr(0), state: "fixed", free: 0 },
      { id: "i1", transform: tr(1), state: "under", free: 6 },
      { id: "i2", transform: tr(2), state: "under", free: 6 },
    ]);
    expect(r.occurrences[2]).toEqual({ path: ["i2"], part: "h0", mesh: 41, world: tr(2), hidden: false });
    expect(r.mates).toEqual([{ id: "m1", status: "ok", message: null, missing: [] }]);
    expect(r.dof).toBe(6);
    expect(r.components[0].id).toBe("c1");
  });

  it("applies a difference to the build held, sharing what did not change", () => {
    const base = decodeBuild(whole(4), null);
    const p = new Packer();
    const buf = p.finish({
      ...common(2, 1),
      instances: { at: p.u32([2]), tr: p.f64(flat(tr(20))), st: p.u32([2]) },
      occurrences: { at: p.u32([3]), part: p.u32([0]), w: p.f64(flat(tr(30))), hidden: p.u32([1]) },
      hashes: ["h0"],
      mateChanges: [[0, { id: "m1", status: "conflict", message: "no", missing: [] }]],
    });
    const r = decodeBuild(buf, base);
    expect(r.gen).toBe(2);
    expect(r.instances[2]).toEqual({ id: "i2", transform: tr(20), state: "full", free: 0 });
    expect(r.instances[1]).toBe(base.instances[1]);
    expect(r.occurrences[3]).toEqual({ path: ["i3"], part: "h0", mesh: 42, world: tr(30), hidden: true });
    // Every occurrence draws its part's new mesh handle.
    expect(r.occurrences.every((o) => o.mesh === 42)).toBe(true);
    expect(r.occurrences[1].world).toBe(base.occurrences[1].world);
    expect(r.mates[0].status).toBe("conflict");
    // The build held is left as it was (the mate animation eases from it).
    expect(base.instances[2].transform).toEqual(tr(2));
    expect(base.occurrences[3].hidden).toBe(false);
    expect(base.mates[0].status).toBe("ok");
  });

  it("refuses a difference against another build", () => {
    const base = decodeBuild(whole(2, 5), null);
    const p = new Packer();
    const buf = p.finish({ ...common(7, 6), instances: { at: p.u32([]), tr: p.f64([]), st: p.u32([]) }, occurrences: { at: p.u32([]), part: p.u32([]), w: p.f64([]), hidden: p.u32([]) }, hashes: [], mateChanges: [] });
    expect(() => decodeBuild(buf, base)).toThrow(StaleBase);
    expect(() => decodeBuild(buf, null)).toThrow(StaleBase);
  });

  it("decodes 10 000 instances quickly", () => {
    const buf = whole(10_000);
    const t = performance.now();
    const r = decodeBuild(buf, null);
    expect(r.occurrences.length).toBe(10_000);
    expect(performance.now() - t).toBeLessThan(200);
  });
});
