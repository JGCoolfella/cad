import type { EdgeSig, FaceSig, Feature, Frame, Vec3 } from "../core/types";

export type KernelRequest =
  | { id: number; op: "init" }
  | { id: number; op: "rebuild"; features: Feature[]; preview?: Feature; quality?: number }
  | { id: number; op: "export"; features: Feature[]; format: "step" | "stl" }
  | { id: number; op: "massProps"; features: Feature[] }
  | { id: number; op: "clearCache" }
  | { id: number; op: "measure"; features: Feature[]; refs: MeasureRef[] }
  | { id: number; op: "convert"; features: Feature[]; frame: Frame; edges: EdgeSig[]; faces: FaceSig[] };

/** Model edges projected onto a sketch plane (Convert Entities), in sketch coordinates. */
export type ConvertedCurve =
  | { kind: "line"; a: [number, number]; b: [number, number] }
  | { kind: "arc"; a: [number, number]; m: [number, number]; b: [number, number] }
  | { kind: "circle"; c: [number, number]; r: number }
  | { kind: "poly"; pts: [number, number][] };

/** Entities measured by the Measure tool. */
export type MeasureRef = { kind: "face"; sig: FaceSig } | { kind: "edge"; sig: EdgeSig } | { kind: "vertex"; p: Vec3 };

export interface MeasureResult {
  /** Minimum distance between the two entities and the closest points. */
  distance: number;
  p1: Vec3;
  p2: Vec3;
}

export type KernelResponse = { id: number; ok: true; result: unknown } | { id: number; ok: false; error: string };
