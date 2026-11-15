import type { App } from "../app/app";
import type { GeomRef } from "../core/types";

/**
 * Label of a picked reference as SOLIDWORKS lists it in selection boxes. `featureId`: the
 * feature the box belongs to; a reference the native engine could not find on its model
 * is labelled "Missing" (🔍 wording, FEATURE_SPEC). Defaults to the feature whose
 * PropertyManager is open.
 */
export function refName(app: App, r: GeomRef | null | undefined, featureId?: string): string | null {
  if (!r) return null;
  featureId ??= app.pm && "feature" in app.pm ? app.pm.feature.id : undefined;
  if (featureId) {
    const sig = r.kind === "face" || r.kind === "edge" ? r.sig : r.kind === "vertex" ? { p: r.p } : r.kind === "plane" && r.plane.kind === "face" ? r.plane.sig : null;
    if (sig && app.isMissingRef(featureId, sig)) return "Missing";
  }
  switch (r.kind) {
    case "face":
      return "Face<1>";
    case "edge":
      return "Edge<1>";
    case "sketchLine":
      return "Line";
    case "vertex":
      return "Vertex<1>";
    case "origin":
      return "Origin";
    case "axis":
    case "point":
      return app.featureById(r.featureId)?.name ?? r.kind;
    case "plane":
      if (r.plane.kind === "std") return `${r.plane.plane[0].toUpperCase()}${r.plane.plane.slice(1)} Plane`;
      if (r.plane.kind === "refplane") return app.featureById(r.plane.featureId)?.name ?? "Plane";
      return "Face<1>";
  }
}
