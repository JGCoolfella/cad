/**
 * Feature dimensions shown when a feature is double-clicked (SOLIDWORKS shows the
 * sketch dimensions and the feature's own, e.g. the extrude depth "D1@Boss-Extrude1")
 * and edited through the Modify box.
 */
import type { Feature } from "./types";

export interface FeatureParam {
  key: string;
  label: string;
  value: number;
  angle?: boolean;
}

export function featureParams(f: Feature): FeatureParam[] {
  switch (f.type) {
    case "extrude": {
      const out: FeatureParam[] = [];
      if (f.end1.type === "blind" || f.end1.type === "midPlane" || f.end1.type === "offsetFromSurface") out.push({ key: "end1.depth", label: "D1", value: f.end1.depth });
      if (f.dir2 && (f.dir2.type === "blind" || f.dir2.type === "offsetFromSurface")) out.push({ key: "dir2.depth", label: "D2", value: f.dir2.depth });
      if (f.end1.draft) out.push({ key: "end1.draft", label: "D3", value: f.end1.draft, angle: true });
      return out;
    }
    case "revolve":
      return [{ key: "angle", label: "D1", value: f.angle, angle: true }];
    case "fillet":
      return [{ key: "radius", label: "D1", value: f.radius }, ...(f.radius2 !== undefined ? [{ key: "radius2", label: "D2", value: f.radius2 }] : [])];
    case "chamfer":
      return [{ key: "distance", label: "D1", value: f.distance }, ...(f.chamferType === "angleDistance" ? [{ key: "angle", label: "D2", value: f.angle ?? Math.PI / 4, angle: true }] : f.chamferType === "distanceDistance" ? [{ key: "distance2", label: "D2", value: f.distance2 ?? f.distance }] : [])];
    case "shell":
      return [{ key: "thickness", label: "D1", value: f.thickness }];
    case "refplane":
      return !f.refs?.length || (f.refs.length === 1 && f.refs[0].kind !== "vertex" && f.refs[0].kind !== "origin") ? [{ key: "offset", label: "D1", value: f.offset }] : f.angle !== undefined ? [{ key: "angle", label: "D1", value: f.angle, angle: true }] : [];
    case "linearPattern":
      return [{ key: "dir1.spacing", label: "D1", value: f.dir1.spacing }, ...(f.dir2 ? [{ key: "dir2.spacing", label: "D2", value: f.dir2.spacing }] : [])];
    case "circularPattern":
      return [{ key: "angle", label: "D1", value: f.angle, angle: true }];
    case "draft":
      return [{ key: "angle", label: "D1", value: f.angle, angle: true }];
    case "sweep":
      return f.profileType === "circular" ? [{ key: "diameter", label: "D1", value: f.diameter }] : [];
    default:
      return [];
  }
}

/** Set a feature parameter by key path ("end1.depth", "radius"...). */
export function setFeatureParam(f: Feature, key: string, value: number) {
  const parts = key.split(".");
  let o = f as unknown as Record<string, unknown>;
  for (let i = 0; i < parts.length - 1; i++) {
    o = o[parts[i]] as Record<string, unknown>;
    if (!o) return;
  }
  o[parts[parts.length - 1]] = value;
}
