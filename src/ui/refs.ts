import type { App } from "../app/app";
import type { GeomRef } from "../core/types";

/** Label of a picked reference as SOLIDWORKS lists it in selection boxes. */
export function refName(app: App, r: GeomRef | null | undefined): string | null {
  if (!r) return null;
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
