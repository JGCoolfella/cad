/** Relation helpers shared by the PropertyManager, context toolbars and commands. */
import type { Id, NewRelation, RelationType, SketchData } from "../core/types";
import { uid } from "../core/ids";

export function describeEntity(sk: SketchData, id: Id) {
  if (sk.points[id]) return id === "origin" ? "Point1@Origin" : `Point`;
  const e = sk.entities[id];
  if (!e) return id;
  return e.type === "line" ? ((e as { construction?: boolean }).construction ? "Centerline" : "Line") : e.type === "circle" ? "Arc (circle)" : e.type === "arc" ? "Arc" : "Point";
}

export const REL_LABEL: Record<RelationType, string> = {
  horizontal: "Horizontal",
  vertical: "Vertical",
  horizontalPoints: "Horizontal",
  verticalPoints: "Vertical",
  coincident: "Coincident",
  midpoint: "Midpoint",
  parallel: "Parallel",
  perpendicular: "Perpendicular",
  collinear: "Collinear",
  equal: "Equal",
  tangent: "Tangent",
  concentric: "Concentric",
  coradial: "Coradial",
  symmetric: "Symmetric",
  intersection: "Intersection",
  fix: "Fix",
  equidistant: "Pattern",
  offset: "Offset",
  onEdge: "On Edge",
};
export function relationLabel(t: RelationType) {
  return REL_LABEL[t];
}

/** Which relations can be added for the selection (as in Add Relations). */
export function possibleRelations(sk: SketchData, sel: Id[]): RelationType[] {
  const pts = sel.filter((id) => sk.points[id]);
  const ents = sel.map((id) => sk.entities[id]).filter(Boolean);
  const lines = ents.filter((e) => e.type === "line");
  const curves = ents.filter((e) => e.type === "circle" || e.type === "arc");
  const out: RelationType[] = [];
  if (sel.length === 1) {
    if (lines.length === 1) out.push("horizontal", "vertical");
    out.push("fix");
    return out;
  }
  if (pts.length === sel.length && pts.length >= 2) out.push("horizontal", "vertical");
  if (pts.length === 2 && sel.length === 2) out.push("coincident");
  if (pts.length === 1 && ents.length === 1) {
    out.push("coincident");
    if (lines.length === 1 || curves.some((c) => c.type === "arc")) out.push("midpoint");
  }
  if (pts.length === 2 && lines.length === 1 && sel.length === 3) out.push("symmetric");
  if (lines.length === sel.length && lines.length >= 2) {
    out.push("horizontal", "vertical", "collinear", "parallel", "equal");
    if (lines.length === 2) out.push("perpendicular");
  }
  if (lines.length === 1 && curves.length === 1 && sel.length === 2) out.push("tangent");
  if (curves.length === sel.length && curves.length >= 2) out.push("tangent", "concentric", "coradial", "equal");
  if (pts.length === 1 && lines.length === 2 && sel.length === 3) out.push("intersection");
  out.push("fix");
  return [...new Set(out)];
}

export function buildRelation(sk: SketchData, type: RelationType, sel: Id[]): NewRelation | null {
  const pts = sel.filter((id) => sk.points[id]);
  const ents = sel.filter((id) => sk.entities[id]);
  switch (type) {
    case "horizontal":
    case "vertical":
      if (ents.length >= 1 && pts.length === 0) {
        if (ents.length === 1) return { type, e: ents[0] };
        // multiple lines: chain as parallel to the first + H/V on first
        for (let i = 1; i < ents.length; i++) sk.relations[uid("r")] = { id: "", type: type, e: ents[i] } as never;
        fixIds(sk);
        return { type, e: ents[0] };
      }
      if (pts.length >= 2) {
        for (let i = 2; i < pts.length; i++) sk.relations[uid("r")] = { id: "", type: type === "horizontal" ? "horizontalPoints" : "verticalPoints", p1: pts[0], p2: pts[i] } as never;
        fixIds(sk);
        return { type: type === "horizontal" ? "horizontalPoints" : "verticalPoints", p1: pts[0], p2: pts[1] };
      }
      return null;
    case "coincident":
      if (pts.length === 2) return { type, p: pts[1], e: pts[0] };
      return { type, p: pts[0], e: ents[0] };
    case "midpoint":
      return { type, p: pts[0], e: ents[0] };
    case "symmetric":
      return { type, p1: pts[0], p2: pts[1], axis: ents[0] };
    case "intersection":
      return { type, p: pts[0], e1: ents[0], e2: ents[1] };
    case "fix":
      return { type, target: sel[0] };
    case "tangent": {
      const a = sk.entities[ents[0]], b = sk.entities[ents[1]];
      let inner = false;
      if ((a.type === "circle" || a.type === "arc") && (b.type === "circle" || b.type === "arc")) {
        const ca = sk.points[a.c], cb = sk.points[b.c];
        const d = Math.hypot(ca.x - cb.x, ca.y - cb.y);
        const ra = a.type === "circle" ? a.r : Math.hypot(sk.points[a.p1].x - ca.x, sk.points[a.p1].y - ca.y);
        const rb = b.type === "circle" ? b.r : Math.hypot(sk.points[b.p1].x - cb.x, sk.points[b.p1].y - cb.y);
        inner = Math.abs(d - Math.abs(ra - rb)) < Math.abs(d - (ra + rb));
      }
      return { type, e1: ents[0], e2: ents[1], inner };
    }
    default:
      return { type, e1: ents[0], e2: ents[1] } as NewRelation;
  }
}

function fixIds(sk: SketchData) {
  for (const [k, r] of Object.entries(sk.relations)) if (!r.id) r.id = k;
}
