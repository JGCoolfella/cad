import type { Feature, Id } from "./types";

/** Sketches a feature consumes (absorbed under it in the FeatureManager tree). */
export function sketchesOf(f: Feature): Id[] {
  switch (f.type) {
    case "extrude":
    case "revolve":
      return f.sketchId ? [f.sketchId] : [];
    case "sweep":
      return [...(f.profileType === "sketch" && f.profile ? [f.profile] : []), ...(f.path ? [f.path] : [])];
    case "loft":
      return [...f.profiles];
    default:
      return [];
  }
}
