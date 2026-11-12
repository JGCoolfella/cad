/* tslint:disable */
/* eslint-disable */

/**
 * A sketch system (`SketchSystem` in TypeScript).
 */
export class WasmSketchSystem {
    free(): void;
    [Symbol.dispose](): void;
    /**
     * The analysis as JSON (`bad` as an array).
     */
    analyse(converged: boolean): string;
    /**
     * `ids[k]` is dragged to `(to[2k], to[2k + 1])`.
     */
    drag(ids: string[], to: Float64Array | number[], steps: number): boolean;
    /**
     * Dense Jacobian, row-major (`equations × parameters`).
     */
    jacobian(): Float64Array;
    /**
     * Current value a dimension (`JSON.stringify(dim)`) measures, `undefined` if none.
     */
    measure(dim: string): number | undefined;
    /**
     * `sketch`: `JSON.stringify(sketchData)`; `extra_fixed`: ids of points held fixed.
     */
    constructor(sketch: string, extra_fixed: string[]);
    /**
     * `{ [pointId]: { ix, iy, cx, cy } }` as JSON, in point order.
     */
    points(): string;
    /**
     * `{ [circleId]: { i, c } }` as JSON.
     */
    radii(): string;
    residuals(): Float64Array;
    setX(x: Float64Array | number[]): void;
    solve(max_iter: number, tol: number): boolean;
    /**
     * Parameter values.
     */
    x(): Float64Array;
}

/**
 * A benchmark sketch (`grid`, `grid-linked`, `linkage`, `polygon`, `tangent-chain`) as
 * JSON `{ sketch, drag: { id, to } }`, or `undefined` for an unknown kind.
 */
export function benchSketch(kind: string, n: number): string | undefined;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_wasmsketchsystem_free: (a: number, b: number) => void;
    readonly benchSketch: (a: number, b: number, c: number, d: number) => void;
    readonly wasmsketchsystem_analyse: (a: number, b: number, c: number) => void;
    readonly wasmsketchsystem_drag: (a: number, b: number, c: number, d: number, e: number, f: number) => number;
    readonly wasmsketchsystem_jacobian: (a: number, b: number) => void;
    readonly wasmsketchsystem_measure: (a: number, b: number, c: number, d: number) => void;
    readonly wasmsketchsystem_new: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly wasmsketchsystem_points: (a: number, b: number) => void;
    readonly wasmsketchsystem_radii: (a: number, b: number) => void;
    readonly wasmsketchsystem_residuals: (a: number, b: number) => void;
    readonly wasmsketchsystem_setX: (a: number, b: number, c: number, d: number) => void;
    readonly wasmsketchsystem_solve: (a: number, b: number, c: number) => number;
    readonly wasmsketchsystem_x: (a: number, b: number) => void;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number, b: number, c: number) => void;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export4: () => number;
    readonly __wbindgen_export5: (a: number, b: number) => void;
    readonly __wbindgen_export6: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
