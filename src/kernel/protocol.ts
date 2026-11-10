import type { Feature } from "../core/types";

export type KernelRequest =
  | { id: number; op: "init" }
  | { id: number; op: "rebuild"; features: Feature[]; preview?: Feature }
  | { id: number; op: "export"; features: Feature[]; format: "step" | "stl" }
  | { id: number; op: "massProps"; features: Feature[] };

export type KernelResponse = { id: number; ok: true; result: unknown } | { id: number; ok: false; error: string };
