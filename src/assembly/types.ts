/**
 * Assembly documents (OVERHAUL_PLAN G1): the TypeScript mirror of `oc_doc::AssemblyDocument`
 * (`crates/oc-doc/src/assembly.rs`), and the native engine's build replies
 * (`crates/oc-app/src/asm.rs`). Assemblies exist in the native app only.
 */
import type { EdgeSig, FaceSig, Id, UnitSystem, Vec3 } from "../core/types";

export type StdPlane = "front" | "top" | "right";

/** `world = rotate(rotation, local) + position`; rotation is a unit quaternion [w, x, y, z]. */
export interface Transform {
  position: Vec3;
  rotation: [number, number, number, number];
}

export interface Component {
  id: Id;
  kind: "part" | "assembly";
  /** Relative to the assembly's folder, `/`-separated. */
  path: string;
  absPath?: string;
  config: string;
}

export interface Instance {
  id: Id;
  component: Id;
  /** `pin<1>` */
  name: string;
  fixed: boolean;
  transform: Transform;
  suppressed?: boolean;
  hidden?: boolean;
}

export type MateType = "coincident" | "parallel" | "perpendicular" | "tangent" | "concentric" | "lock" | "distance" | "angle";
export type Align = "aligned" | "antiAligned";

export type MateGeom =
  | { type: "plane"; p: Vec3; n: Vec3 }
  | { type: "line"; p: Vec3; d: Vec3 }
  | { type: "point"; p: Vec3 }
  | { type: "cylinder"; p: Vec3; d: Vec3; r: number }
  | { type: "cone"; p: Vec3; d: Vec3; r: number; halfAngle: number }
  | { type: "sphere"; c: Vec3; r: number }
  | { type: "circle"; c: Vec3; n: Vec3; r: number };

export type MateEntity =
  | { kind: "face"; sig: FaceSig; geom: MateGeom }
  | { kind: "edge"; sig: EdgeSig; geom: MateGeom }
  | { kind: "vertex"; p: Vec3; name?: string[][] }
  | { kind: "plane"; plane: StdPlane }
  | { kind: "refPlane"; featureId: Id; geom: MateGeom }
  | { kind: "refAxis"; featureId: Id; geom: MateGeom }
  | { kind: "refPoint"; featureId: Id; geom: MateGeom }
  | { kind: "origin" }
  | { kind: "component" };

export interface MateRef {
  /** Instance ids from this assembly down; empty = the assembly itself. */
  occurrence: Id[];
  entity: MateEntity;
}

export interface Mate {
  id: Id;
  name: string;
  type: MateType;
  refs: MateRef[];
  align?: Align;
  value?: number;
  flip?: boolean;
  relative?: Transform;
  suppressed?: boolean;
}

export interface AssemblyDocument {
  format: "opencad-assembly";
  version: 1;
  name: string;
  units: UnitSystem;
  components: Component[];
  instances: Instance[];
  mates: Mate[];
}

export type InstanceState = "fixed" | "under" | "full" | "over" | "suppressed";
export type MateStatus = "ok" | "suppressed" | "missing" | "redundant" | "conflict" | "invalid";

export interface Occurrence {
  path: Id[];
  part: string;
  /** Engine mesh handle (one per distinct part). */
  mesh: number;
  world: Transform;
  hidden: boolean;
}

export interface BuildReply {
  components: { id: Id; path: string | null; found: string | null; part: string | null; error: string | null; warning: string | null }[];
  instances: { id: Id; transform: Transform; state: InstanceState; free: number }[];
  occurrences: Occurrence[];
  parts: Record<string, { name: string; handle: number; lightweight: boolean; errors: { id: string; message: string }[]; bbox: [Vec3, Vec3] | null }>;
  mates: { id: Id; status: MateStatus; message: string | null; missing: number[] }[];
  dof: number;
  converged: boolean;
  newestError: Id | null;
  heals: { mate: Id; side: number; entity: MateEntity }[];
  timeMs: number;
}

export interface DragReply {
  moved: boolean;
  instances: { id: Id; transform: Transform }[];
  occurrences: Transform[];
  dof: number;
}

// ------------------------------------------------------------------ transforms

export const IDENTITY: Transform = { position: [0, 0, 0], rotation: [1, 0, 0, 0] };

export function qrot(q: Transform["rotation"], v: Vec3): Vec3 {
  const [w, x, y, z] = q;
  const c1: Vec3 = [y * v[2] - z * v[1], z * v[0] - x * v[2], x * v[1] - y * v[0]];
  const c2: Vec3 = [y * c1[2] - z * c1[1], z * c1[0] - x * c1[2], x * c1[1] - y * c1[0]];
  return [v[0] + 2 * (w * c1[0] + c2[0]), v[1] + 2 * (w * c1[1] + c2[1]), v[2] + 2 * (w * c1[2] + c2[2])];
}

export function apply(t: Transform, p: Vec3): Vec3 {
  const r = qrot(t.rotation, p);
  return [r[0] + t.position[0], r[1] + t.position[1], r[2] + t.position[2]];
}

/** World point to the transform's local coordinates. */
export function applyInverse(t: Transform, p: Vec3): Vec3 {
  const [w, x, y, z] = t.rotation;
  return qrot([w, -x, -y, -z], [p[0] - t.position[0], p[1] - t.position[1], p[2] - t.position[2]]);
}

/** The geometry's anchor and direction placed by `t`. */
export function placedGeom(g: MateGeom, t: Transform): { p: Vec3; d: Vec3 | null } {
  switch (g.type) {
    case "plane":
      return { p: apply(t, g.p), d: qrot(t.rotation, g.n) };
    case "line":
    case "cylinder":
    case "cone":
      return { p: apply(t, g.p), d: qrot(t.rotation, g.d) };
    case "circle":
      return { p: apply(t, g.c), d: qrot(t.rotation, g.n) };
    case "sphere":
      return { p: apply(t, g.c), d: null };
    case "point":
      return { p: apply(t, g.p), d: null };
  }
}

export function entityGeom(e: MateEntity): MateGeom | null {
  switch (e.kind) {
    case "face":
    case "edge":
    case "refPlane":
    case "refAxis":
    case "refPoint":
      return e.geom;
    case "vertex":
      return { type: "point", p: e.p };
    case "origin":
    case "component":
      return { type: "point", p: [0, 0, 0] };
    case "plane":
      return { type: "plane", p: [0, 0, 0], n: e.plane === "front" ? [0, 0, 1] : e.plane === "top" ? [0, 1, 0] : [1, 0, 0] };
  }
}

export const MATE_LABEL: Record<MateType, string> = {
  coincident: "Coincident",
  parallel: "Parallel",
  perpendicular: "Perpendicular",
  tangent: "Tangent",
  concentric: "Concentric",
  lock: "Lock",
  distance: "Distance",
  angle: "Angle",
};
