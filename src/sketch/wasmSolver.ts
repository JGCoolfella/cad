/**
 * The Rust sketch solver (`crates/oc-sketch`, compiled to WebAssembly) behind the same API
 * as the TypeScript `SketchSystem` (./solver.ts). Not used by the app yet (work package F2
 * switches the editor over); `wasmSolver.test.ts` checks it against the TypeScript solver.
 *
 * Initialise once before constructing: `await loadSketchWasm()` in the browser, or
 * `initSketchWasm(bytes)` with the module's bytes (Node, tests).
 * Rebuild the module with `npm run build:sketch-wasm` after changing the Rust crates.
 */
import type { Dimension, Id, SketchData, Vec2 } from "../core/types";
import type { Analysis, SketchSystem } from "./solver";
import init, { benchSketch as rawBench, initSync, WasmSketchSystem as Raw } from "./wasm/oc_sketch.js";

/** The public surface of `SketchSystem` both implementations share. */
export type SketchSystemApi = Pick<
  SketchSystem,
  "sk" | "x" | "pts" | "radii" | "residuals" | "jacobian" | "solve" | "drag" | "commit" | "analyse" | "measureFn"
>;

type PRef = SketchSystem["pts"] extends Map<Id, infer P> ? P : never;
type RRef = SketchSystem["radii"] extends Map<Id, infer R> ? R : never;

/** Synchronous initialisation from the module's bytes (or a compiled module). */
export function initSketchWasm(module: BufferSource | WebAssembly.Module) {
  initSync({ module });
}

/** Browser initialisation: fetches the module next to this file. */
export async function loadSketchWasm() {
  await init();
}

export class WasmSketchSystem implements SketchSystemApi {
  readonly pts: Map<Id, PRef>;
  readonly radii: Map<Id, RRef>;
  private raw: Raw;

  constructor(public sk: SketchData, extraFixed: Set<Id> = new Set()) {
    this.raw = new Raw(JSON.stringify(sk), [...extraFixed]);
    this.pts = new Map(Object.entries(JSON.parse(this.raw.points()) as Record<Id, PRef>));
    this.radii = new Map(Object.entries(JSON.parse(this.raw.radii()) as Record<Id, RRef>));
  }

  /** Parameter values (a copy; assign to replace them). */
  get x(): number[] {
    return Array.from(this.raw.x());
  }
  set x(v: number[]) {
    this.raw.setX(Float64Array.from(v));
  }

  residuals(): number[] {
    return Array.from(this.raw.residuals());
  }

  /** Dense Jacobian (rows = equations), exact rather than by finite differences. */
  jacobian(): Float64Array[] {
    const flat = this.raw.jacobian();
    const n = this.raw.x().length;
    const m = this.raw.residuals().length;
    return Array.from({ length: m }, (_, i) => flat.slice(i * n, (i + 1) * n));
  }

  solve(maxIter = 60, tol = 1e-11): boolean {
    return this.raw.solve(maxIter, tol);
  }

  drag(targets: { id: Id; to: Vec2 }[], steps = 12): boolean {
    const to = new Float64Array(targets.length * 2);
    targets.forEach((t, k) => {
      to[2 * k] = t.to[0];
      to[2 * k + 1] = t.to[1];
    });
    return this.raw.drag(
      targets.map((t) => t.id),
      to,
      steps,
    );
  }

  /** Write solved values back into the sketch data (mutates). */
  commit() {
    const x = this.raw.x();
    for (const [id, r] of this.pts) {
      if (r.ix < 0) continue;
      const p = this.sk.points[id];
      if (!p) continue;
      p.x = x[r.ix];
      p.y = x[r.iy];
    }
    for (const [id, r] of this.radii) {
      if (r.i < 0) continue;
      const e = this.sk.entities[id];
      if (e?.type === "circle") e.r = x[r.i];
    }
    for (const d of Object.values(this.sk.dimensions)) {
      if (!d.driven) continue;
      const f = this.measureFn(d);
      if (f) d.value = Math.abs(f());
    }
  }

  analyse(converged: boolean): Analysis {
    const a = JSON.parse(this.raw.analyse(converged)) as Omit<Analysis, "bad"> & { bad: Id[] };
    return { ...a, bad: new Set(a.bad) };
  }

  /** Function computing the current measured value of a dimension, or null. */
  measureFn(d: Dimension): (() => number) | null {
    const text = JSON.stringify(d);
    if (this.raw.measure(text) === undefined) return null;
    return () => this.raw.measure(text) ?? NaN;
  }

  /** Releases the WebAssembly memory now (otherwise freed when garbage collected). */
  free() {
    this.raw.free();
  }
}

/** Benchmark sketch from `crates/oc-sketch/src/generators.rs`, with the point to drag. */
export function benchSketch(kind: string, n: number): { sketch: SketchData; drag: { id: Id; to: Vec2 } } {
  const text = rawBench(kind, n);
  if (text === undefined) throw new Error(`unknown benchmark sketch ${kind}`);
  return JSON.parse(text);
}
