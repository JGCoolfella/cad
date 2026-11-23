/**
 * Messages between the UI thread (`render.ts`), the engine worker and the render worker
 * (`render.worker.ts`), OVERHAUL_PLAN W1c. JSON payloads are the desktop app's IPC
 * arguments and replies unchanged (`view_frame`, `view_pick`, `view_box`, `view_stats`).
 */

/** What the renderer runs on (`oc-web-render`'s `info`). */
export interface RenderInfo {
  adapter: string;
  backend: string;
  /** `depth32float-stencil8`: 32-bit depth, else 24-bit. */
  depth32: boolean;
  /** `indirect-first-instance`: GPU lists and culling, else bodies one by one. */
  indirectFirstInstance: boolean;
  surface: string;
}

/**
 * A display mesh from the engine worker: the engine's handle and `RenderMesh::to_bytes`
 * (the buffer is transferred). Sent on the port the render worker got with `engine`.
 */
export interface MeshMessage {
  t: "mesh";
  handle: number;
  bytes: ArrayBuffer;
}

export type ToRender =
  /** `module`: URL of the wasm-bindgen module (`oc_web_render.js`, its `_bg.wasm` beside it). */
  | { t: "init"; canvas: OffscreenCanvas; module: string }
  /** The engine worker's end of a channel for meshes. */
  | { t: "engine"; port: MessagePort }
  | MeshMessage
  /** The canvas' size in device pixels. */
  | { t: "resize"; width: number; height: number }
  /** `view_frame`'s arguments as JSON. */
  | { t: "frame"; json: string }
  | { t: "label"; key: number; rgba: ArrayBuffer; width: number; height: number }
  /** `view_pick`'s `req` as JSON. */
  | { t: "pick"; id: number; json: string }
  /** `view_box`'s `req` as JSON. */
  | { t: "box"; id: number; json: string }
  | { t: "stats"; id: number }
  | { t: "drawn"; id: number }
  | { t: "info"; id: number };

export type FromRender =
  | { t: "ready"; info: RenderInfo; adapter: string }
  | { t: "failed"; error: string }
  | { t: "reply"; id: number; json?: string; error?: string }
  /** A frame was drawn (its commands submitted): its `seq` and CPU time since it arrived. */
  | { t: "drawn"; seq: number; cpuMs: number };
