/**
 * Interactive sketch editing: tools, selection, dragging, Smart Dimension, trim,
 * and the 2D overlay drawing of the active sketch.
 */
import type { Dimension, DimensionKind, Id, NewRelation, Relation, SketchData, SketchEntity, Vec2 } from "../core/types";
import { v2, wrap2Pi, angleInArc } from "../core/math";
import { uid } from "../core/ids";
import { formatAngle, formatLength } from "../core/units";
import type { App } from "../app/app";
import { infer, intersectEntities, type SnapResult } from "./inference";
import { P, arcGeom, circumcenter, closestOnEntity, deleteFromSketch, entitiesUsingPoint, extractProfiles, faceHoles, pointInPoly, pruneRefs, splineThrough, sampleEntity, type Loop } from "./model";
import { ORIGIN_ID, SketchSystem, entityPoints, relationRefs, solveSketch } from "./solver";
import { SHAPES, filletGeom, rectConstruction, type BuildCtx, type ShapeOpts } from "./shapes";
import { buildRelation } from "./relations";
import { chamferCorner, circularPattern, detachMoved, extendEntity, linearPattern, mirrorEntities, offsetEntities, transformEntities, xf } from "./ops";
import type { RelationType } from "../core/types";

export interface SketchViewApi {
  toSketch(sx: number, sy: number): Vec2 | null;
  toScreen(p: Vec2): [number, number];
  /** Sketch coordinates under a screen pixel (for grid clipping). */
  toSketchPoint?(sx: number, sy: number): Vec2 | null;
  /** Sketch units per screen pixel. */
  pxSize(): number;
}

export interface PointerInfo {
  sx: number;
  sy: number;
  button: number;
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
}

/** `mid`: the midpoint of line `id` (becomes a real point when picked, see resolveHit). */
type Hit = { kind: "point"; id: Id } | { kind: "entity"; id: Id } | { kind: "dim"; id: Id } | { kind: "mid"; id: Id };

const PICK_PX = 7;
const SNAP_PX = 9;

export const SKETCH_TOOLS = [
  "line",
  "centerline",
  "rectangle",
  "centerRectangle",
  "circle",
  "arc",
  "threePointArc",
  "tangentArc",
  "point",
  "smartDimension",
  "dimHorizontal",
  "dimVertical",
  "trim",
  "midpointLine",
  "threePointRectangle",
  "threePointCenterRectangle",
  "parallelogram",
  "perimeterCircle",
  "straightSlot",
  "centerSlot",
  "polygon",
  "sketchFillet",
  "sketchChamfer",
  "extend",
  "offset",
  "mirror",
  "move",
  "copy",
  "rotate",
  "scale",
  "linearPattern",
  "circularPattern",
  "convert",
  "spline",
] as const;
export type SketchTool = (typeof SKETCH_TOOLS)[number];

interface ToolState {
  pts: { p: Vec2; snap: SnapResult; pid?: Id }[];
  downAt?: [number, number];
  moved?: boolean;
  prevLine?: Id;
  prevEnt?: Id;
  arcMode?: boolean;
  nearStart?: boolean;
  leftStart?: boolean;
  /** Accumulated signed sweep for centre-point arcs. */
  sweep?: number;
  lastAngle?: number;
  dimRefs?: Hit[];
  trimmedOnDrag?: Set<Id>;
  /** Sketch fillet: first picked line when filleting by picking two lines. */
  filletFirst?: Id;
  /** Operation tools: picked entities, mirror axis, picked points, active selection box. */
  opSel?: Id[];
  opAxis?: Id;
  opPts?: Vec2[];
  opField?: OpField;
}

const LINE_TOOLS = new Set(["line", "centerline", "midpointLine"]);
/** Smart Dimension and the Horizontal / Vertical Dimension tools. */
const DIM_TOOLS = new Set<string>(["smartDimension", "dimHorizontal", "dimVertical"]);

/** Tools that act on selected entities through a PropertyManager (✓ applies). */
export const OP_TOOLS = new Set(["offset", "mirror", "move", "copy", "rotate", "scale", "linearPattern", "circularPattern", "convert"]);
/** Which PropertyManager selection box receives graphics picks. */
export type OpField = "entities" | "axis" | "point1" | "point2";
/** Tools whose "From/To" points are picked in the graphics area. */
const POINT_TOOLS = new Set(["move", "copy", "rotate", "scale", "circularPattern"]);

export class SketchEditor {
  raw: Vec2 | null = null;
  rawScreen: [number, number] = [0, 0];
  snap: SnapResult | null = null;
  woken: Id[] = [];
  st: ToolState = { pts: [] };
  hover: Hit | null = null;
  ctrl = false;
  private drag:
    | { kind: "points"; ids: Id[]; start: Vec2; orig: Vec2[]; begun: boolean }
    | { kind: "radius"; id: Id; begun: boolean }
    | { kind: "dimLabel"; id: Id; begun: boolean }
    | { kind: "box"; x0: number; y0: number; x1: number; y1: number }
    | null = null;
  private downScreen: [number, number] | null = null;

  /** Closed area under the cursor (select tool, Shaded Sketch Contours on). */
  hoverRegion: Loop | null = null;
  private facesCache: { v: number; sk: SketchData; faces: Loop[] } | null = null;

  /**
   * `source` makes a read-only renderer for a sketch that is not being edited (feature
   * previews draw the profile sketch and its dimensions with it).
   */
  constructor(private app: App, private view: SketchViewApi, private source?: () => SketchData, private sourceId?: Id) {}

  /** Full dimension name ("D1@Sketch1") for equations. */
  private fullName(d: Dimension) {
    const id = this.sourceId ?? this.app.sketch?.featureId;
    const f = id ? this.app.featureById(id) : null;
    return `${d.name}@${f?.name ?? ""}`;
  }

  get sk(): SketchData {
    return this.source ? this.source() : this.app.sketch!.data;
  }
  get tool(): SketchTool | null {
    return this.app.tool as SketchTool | null;
  }
  private get tol() {
    return SNAP_PX * this.view.pxSize();
  }
  private get autoRel() {
    return this.app.options.sketch.automaticRelations && !this.ctrl;
  }

  resetTool() {
    this.st = { pts: [] };
    this.snap = null;
  }

  setTool(t: SketchTool | null) {
    // Operation tools start from the pre-selection, as in SOLIDWORKS.
    const pre = this.app.selection.filter((s) => s.kind === "skEntity").map((s) => (s as { id: Id }).id);
    this.app.tool = t;
    this.resetTool();
    if (t && OP_TOOLS.has(t)) {
      let sel = pre.filter((id) => this.sk.entities[id]);
      if (t === "mirror") {
        const axis = [...sel].reverse().find((id) => {
          const e = this.sk.entities[id];
          return e?.type === "line" && e.construction;
        });
        if (axis && sel.length > 1) {
          this.st.opAxis = axis;
          sel = sel.filter((id) => id !== axis);
        }
      }
      this.st.opSel = sel;
      this.st.opPts = [];
      this.st.opField = !sel.length ? "entities" : t === "mirror" ? (this.st.opAxis ? "entities" : "axis") : POINT_TOOLS.has(t) ? "point1" : "entities";
      if (t === "circularPattern") this.st.opField = "entities";
    }
    // Convert Entities keeps pre-selected model edges/faces; other tools start clean.
    if (t === "convert") this.app.selection = this.app.selection.filter((s) => s.kind === "edge" || s.kind === "face");
    else if (t !== null) this.app.selection = [];
    this.app.pm = t ? { kind: "sketchTool", tool: t } : null;
    this.app.changed();
  }

  // ================================================================ regions

  /** Shaded Sketch Contours (Sketch toolbar toggle, on by default). */
  get shading() {
    return this.app.opt("sketch.shadeContours") !== false;
  }

  /** Smallest closed areas of the sketch (cached per app change). */
  faces(): Loop[] {
    const v = this.app.getVersion(), sk = this.sk;
    const c = this.facesCache;
    if (c && c.v === v && c.sk === sk) return c.faces;
    let faces: Loop[] = [];
    try {
      faces = extractProfiles(sk).faces;
    } catch {
      faces = [];
    }
    this.facesCache = { v, sk, faces };
    return faces;
  }

  /** The closed area under a screen point (smallest one containing it). */
  regionAt(sx: number, sy: number): Loop | null {
    if (!this.shading) return null;
    const p = this.view.toSketch(sx, sy);
    if (!p) return null;
    let best: Loop | null = null;
    for (const f of this.faces()) if ((!best || f.area < best.area) && pointInPoly(p, f.poly)) best = f;
    return best;
  }

  /** Loops directly inside a region (its holes). */
  regionHoles(f: Loop): Loop[] {
    return faceHoles(this.faces(), f);
  }

  /** Entities bounding a region: its own loop and the loops of its holes. */
  regionEnts(f: Loop): Id[] {
    return [...new Set([...f.ents, ...this.regionHoles(f).flatMap((g) => g.ents)])];
  }

  // ================================================================ events

  onMove(e: PointerInfo) {
    this.ctrl = e.ctrl;
    this.rawScreen = [e.sx, e.sy];
    const raw = this.view.toSketch(e.sx, e.sy);
    if (!raw) return;
    this.raw = raw;
    this.app.setCursor(raw);
    this.wake(raw);
    if (this.drag) return this.dragMove(e, raw);
    const tool = this.tool;
    if (!tool) {
      this.app.setHover(null);
      this.hover = this.hitTest(e.sx, e.sy);
      this.hoverRegion = this.hover ? null : this.regionAt(e.sx, e.sy);
      this.app.requestRender();
      return;
    }
    this.hoverRegion = null;
    if (DIM_TOOLS.has(tool)) {
      this.hover = this.hitTest(e.sx, e.sy, false);
      this.app.requestRender();
      return;
    }
    if (tool === "sketchFillet" || tool === "sketchChamfer" || tool === "extend") {
      this.hover = this.hitTest(e.sx, e.sy, false, tool === "extend");
      this.app.requestRender();
      return;
    }
    if (OP_TOOLS.has(tool)) {
      const f = this.st.opField;
      this.hover = f === "entities" || f === "axis" ? this.hitTest(e.sx, e.sy, false, true) : null;
      if (f === "point1" || f === "point2") this.snap = this.computeSnap(raw);
      this.app.requestRender();
      return;
    }
    if (tool === "trim") {
      this.hover = this.hitTest(e.sx, e.sy, false, true);
      if (this.st.trimmedOnDrag && this.hover?.kind === "entity" && !this.st.trimmedOnDrag.has(this.hover.id)) {
        this.st.trimmedOnDrag.add(this.hover.id);
        this.trimAt(this.hover.id, raw, false);
      }
      this.app.requestRender();
      return;
    }
    this.snap = this.computeSnap(raw);
    if (this.st.downAt && !this.st.moved && Math.hypot(e.sx - this.st.downAt[0], e.sy - this.st.downAt[1]) > 5) this.st.moved = true;
    // Line tool: return to the last endpoint and leave again => tangent arc (SOLIDWORKS gesture).
    if ((tool === "line" || tool === "centerline") && this.st.pts.length === 1 && this.st.prevEnt) {
      const d = v2.dist(raw, this.st.pts[0].p) / this.view.pxSize();
      if (d < 6) this.st.nearStart = true;
      else if (this.st.nearStart && d > 12) {
        this.st.nearStart = false;
        this.st.arcMode = !this.st.arcMode;
      }
    }
    if (tool === "arc" && this.st.pts.length === 2) this.trackSweep();
    this.app.requestRender();
  }

  onDown(e: PointerInfo) {
    if (e.button !== 0) return;
    this.ctrl = e.ctrl;
    const raw = this.view.toSketch(e.sx, e.sy);
    if (!raw) return;
    this.raw = raw;
    this.downScreen = [e.sx, e.sy];
    const tool = this.tool;
    if (!tool) return this.selectDown(e, raw);
    if (DIM_TOOLS.has(tool)) return this.dimClick(e, raw);
    if (tool === "sketchFillet" || tool === "sketchChamfer") return this.filletClick(e);
    if (tool === "convert") return; // model picks are handled by the viewport
    if (OP_TOOLS.has(tool)) return this.opClick(e, raw);
    if (tool === "extend") {
      const h = this.hitTest(e.sx, e.sy, false, true);
      if (h?.kind !== "entity") return;
      const probe = structuredClone(this.sk);
      if (!extendEntity(probe, h.id, raw)) return this.app.flash("Nothing to extend to in that direction.", "info");
      this.app.sketchCheckpoint();
      extendEntity(this.sk, h.id, raw);
      this.commit();
      return;
    }
    if (tool === "trim") {
      const h = this.hitTest(e.sx, e.sy, false, true);
      this.st.trimmedOnDrag = new Set();
      if (h?.kind === "entity") {
        this.st.trimmedOnDrag.add(h.id);
        this.trimAt(h.id, raw, true);
      }
      return;
    }
    this.snap = this.computeSnap(raw);
    this.st.downAt = [e.sx, e.sy];
    this.st.moved = false;
    this.toolClick(false);
  }

  onUp(e: PointerInfo) {
    if (e.button !== 0) return;
    const tool = this.tool;
    if (this.drag) return this.dragEnd(e);
    if (tool === "trim") {
      this.st.trimmedOnDrag = undefined;
      return;
    }
    // Click-drag creation: releasing after dragging commits the entity.
    if (tool && !DIM_TOOLS.has(tool) && tool !== "sketchFillet" && this.st.moved && this.st.pts.length >= 1) {
      this.st.moved = false;
      this.st.downAt = undefined;
      this.toolClick(true);
    }
    this.st.downAt = undefined;
  }

  onDblClick(e: PointerInfo) {
    const tool = this.tool;
    if (tool === "spline") return this.finishSpline();
    if (tool && LINE_TOOLS.has(tool)) {
      // Double-click ends the chain.
      this.st = { pts: [] };
      this.app.changed();
      return;
    }
    if (!tool) {
      const h = this.hitTest(e.sx, e.sy);
      if (h?.kind === "dim") this.openModify(h.id, false);
    }
  }

  /** Returns true if the key was consumed. */
  onKey(key: string): boolean {
    const tool = this.tool;
    if (key === "Escape") {
      if (tool && this.st.pts.length) {
        this.st = { pts: [] };
        this.st.dimRefs = undefined;
        this.app.changed();
        return true;
      }
      if (tool && this.st.dimRefs?.length) {
        this.st.dimRefs = undefined;
        this.app.changed();
        return true;
      }
      if (tool) {
        this.setTool(null);
        return true;
      }
      if (this.app.selection.length) {
        this.app.clearSelection();
        return true;
      }
      return false;
    }
    if ((key === "a" || key === "A") && (tool === "line" || tool === "centerline") && this.st.pts.length === 1) {
      this.st.arcMode = !this.st.arcMode;
      this.app.requestRender();
      return true;
    }
    if (key === "Delete" || key === "Backspace") {
      this.deleteSelection();
      return true;
    }
    if (key === "Enter" && tool === "spline") {
      this.finishSpline();
      return true;
    }
    if (key === "Enter" && tool && OP_TOOLS.has(tool)) {
      this.applyOp();
      return true;
    }
    return false;
  }

  // ================================================================ operation tools

  /** Trim a spline: remove the part between the intersections around `at` (new splines keep the rest). */
  private trimSpline(e: Extract<SketchEntity, { type: "spline" }>, at: Vec2) {
    const sk = this.sk;
    const pts = sampleEntity(sk, e, 256);
    const cum = [0];
    for (let i = 1; i < pts.length; i++) cum.push(cum[i - 1] + v2.dist(pts[i - 1], pts[i]));
    const paramOf = (q: Vec2) => {
      let best = 0, bd = Infinity;
      for (let i = 0; i + 1 < pts.length; i++) {
        const a = pts[i], b = pts[i + 1];
        const ab = v2.sub(b, a), L2 = v2.dot(ab, ab) || 1e-18;
        const t = Math.max(0, Math.min(1, v2.dot(v2.sub(q, a), ab) / L2));
        const d = v2.dist(q, v2.add(a, v2.scale(ab, t)));
        if (d < bd) (bd = d), (best = cum[i] + t * (cum[i + 1] - cum[i]));
      }
      return best;
    };
    const xs: number[] = [];
    for (const o of Object.values(sk.entities)) if (o.id !== e.id && o.type !== "point") for (const q of intersectEntities(sk, e.id, o.id)) xs.push(paramOf(q));
    const t = paramOf(at), total = cum[cum.length - 1];
    const lo = Math.max(0, ...xs.filter((x) => x < t - 1e-6)), hi = Math.min(total, ...xs.filter((x) => x > t + 1e-6));
    const piece = (a: number, b: number): Vec2[] => {
      const out: Vec2[] = [];
      const n = Math.max(2, Math.min(12, Math.round(((b - a) / total) * 12) + 1));
      for (let k = 0; k <= n; k++) {
        const s = a + ((b - a) * k) / n;
        let i = 0;
        while (i + 1 < cum.length - 1 && cum[i + 1] < s) i++;
        const f = (s - cum[i]) / (cum[i + 1] - cum[i] || 1);
        out.push(v2.lerp(pts[i], pts[i + 1], Math.max(0, Math.min(1, f))));
      }
      return out;
    };
    const keep: Vec2[][] = [];
    if (lo > 1e-6) keep.push(piece(0, lo));
    if (hi < total - 1e-6) keep.push(piece(hi, total));
    const first = e.pts[0], last = e.pts[e.pts.length - 1];
    deleteFromSketch(sk, new Set([e.id]));
    for (const k of keep) {
      const ids = k.map((q, i) => {
        if (i === 0 && lo > 1e-6 && keep[0] === k && sk.points[first]) return first;
        if (i === k.length - 1 && hi < total - 1e-6 && k === keep[keep.length - 1] && sk.points[last]) return last;
        const pid = uid("p");
        sk.points[pid] = { id: pid, x: q[0], y: q[1] };
        return pid;
      });
      this.addEntity({ id: uid("e"), type: "spline", pts: ids });
    }
    this.commit();
  }

  /** Spline tool: create the spline through the clicked points. */
  finishSpline() {
    const st = this.st;
    if (st.pts.length >= 2) {
      this.app.sketchCheckpoint();
      const pts = st.pts.map((x) => x.pid ?? this.pointFor(x.snap));
      this.addEntity({ id: uid("e"), type: "spline", pts: pts.filter((p, i) => i === 0 || p !== pts[i - 1]) });
      this.commit();
    }
    this.st = { pts: [] };
    this.app.changed();
  }

  /** Graphics pick while an operation PropertyManager is open. */
  private opClick(e: PointerInfo, raw: Vec2): void {
    const st = this.st;
    const tool = this.tool!;
    const f = st.opField ?? "entities";
    if (f === "point1" || f === "point2") {
      const p = this.computeSnap(raw).point;
      st.opPts = f === "point1" ? [p] : [...(st.opPts ?? []).slice(0, 1), p];
      if (f === "point1" && (tool === "move" || tool === "copy")) st.opField = "point2";
      else if (f === "point2") {
        // From/To finishes the move as soon as the destination is picked.
        this.applyOp();
        return;
      }
      this.app.changed();
      return;
    }
    const h = this.hitTest(e.sx, e.sy, false, true);
    if (f === "axis") {
      const ent = h?.kind === "entity" ? this.sk.entities[h.id] : null;
      if (ent?.type !== "line") return this.app.flash("Select a line or centerline to mirror about.", "info");
      st.opAxis = ent.id;
      st.opField = "entities";
      this.app.changed();
      return;
    }
    if (h?.kind !== "entity") {
      // Clicking empty space after picking entities moves on to the point box.
      if (st.opSel?.length && POINT_TOOLS.has(tool) && tool !== "circularPattern") {
        st.opField = "point1";
        return this.opClick(e, raw);
      }
      return;
    }
    const ent = this.sk.entities[h.id];
    // Mirror: a centerline picked after the entities is the mirror line.
    if (tool === "mirror" && !st.opAxis && st.opSel?.length && ent?.type === "line" && ent.construction) {
      st.opAxis = h.id;
      this.app.changed();
      return;
    }
    const sel = new Set(st.opSel ?? []);
    if (sel.has(h.id)) sel.delete(h.id);
    else sel.add(h.id);
    st.opSel = [...sel];
    this.app.changed();
  }

  /** Point picked for rotate/scale/circular pattern, or the sketch origin. */
  private opCenter(): Vec2 {
    return this.st.opPts?.[0] ?? [0, 0];
  }

  /** Run the active operation on `sk`; returns the created/affected entity ids. */
  private runOp(sk: SketchData, preview: boolean): { created: Id[]; error?: string } {
    const st = this.st;
    const o = this.app.toolOpts;
    const sel = (st.opSel ?? []).filter((id) => sk.entities[id]);
    const tool = this.tool!;
    if (tool === "convert") return this.app.selection.some((s) => s.kind === "edge" || s.kind === "face") ? { created: [] } : { created: [], error: "Select model edges or faces to convert." };
    if (!sel.length) return { created: [], error: "Select the sketch entities first." };
    switch (tool) {
      case "offset":
        return offsetEntities(sk, sel, {
          distance: o.offsetDistance,
          reverse: o.offsetReverse,
          bidirectional: o.offsetBidirectional,
          capEnds: o.offsetCapEnds,
          baseConstruction: o.offsetBaseConstruction,
          offsetConstruction: o.offsetConstruction,
          addDimensions: o.offsetAddDimensions,
          selectChain: o.offsetSelectChain,
        });
      case "mirror":
        if (!st.opAxis) return { created: [], error: "Select a line or centerline to mirror about." };
        return mirrorEntities(sk, sel, st.opAxis, o.mirrorCopy);
      case "move":
      case "copy": {
        let d: Vec2;
        if (o.moveMode === "xy") d = [o.moveDX, o.moveDY];
        else {
          const [a, b] = st.opPts ?? [];
          const to = b ?? (preview && a && this.snap ? this.snap.point : null);
          if (!a || !to) return { created: [], error: "Pick the base point and then the destination." };
          d = v2.sub(to, a);
        }
        if (tool === "copy") return transformEntities(sk, sel, xf.translate(d), true);
        if (!o.keepRelations) detachMoved(sk, sel);
        return transformEntities(sk, sel, xf.translate(d), false);
      }
      case "rotate":
        if (!o.keepRelations) detachMoved(sk, sel);
        return transformEntities(sk, sel, xf.rotate(this.opCenter(), o.rotateAngle), false);
      case "scale": {
        const f = Math.max(1e-6, o.scaleFactor);
        if (o.scaleCopy) return transformEntities(sk, sel, xf.scale(this.opCenter(), f), true, { scale: f });
        detachMoved(sk, sel);
        return transformEntities(sk, sel, xf.scale(this.opCenter(), f), false, { scale: f });
      }
      case "linearPattern":
        return linearPattern(sk, sel, { angle1: o.lpAngle1, spacing1: o.lpSpacing1, count1: o.lpCount1, angle2: o.lpAngle2, spacing2: o.lpSpacing2, count2: o.lpCount2 });
      case "circularPattern":
        return circularPattern(sk, sel, { center: this.opCenter(), angle: o.cpAngle, count: o.cpCount, equalSpacing: o.cpEqual, reverse: o.cpReverse });
    }
    return { created: [] };
  }

  private previewCache: { key: string; sk: SketchData; created: Id[]; error?: string } | null = null;
  /** Result of the operation on a copy of the sketch (drawn as the yellow preview). */
  opPreview() {
    if (!this.tool || !OP_TOOLS.has(this.tool)) return null;
    const key = JSON.stringify([this.tool, this.st.opSel, this.st.opAxis, this.st.opPts, this.app.toolOpts, this.app.getVersion(), this.st.opField === "point2" ? this.snap?.point : null]);
    if (this.previewCache?.key === key) return this.previewCache;
    const sk = structuredClone(this.sk);
    let r: { created: Id[]; error?: string };
    try {
      r = this.runOp(sk, true);
    } catch (e) {
      r = { created: [], error: (e as Error).message };
    }
    this.previewCache = { key, sk, created: r.created, error: r.error };
    return this.previewCache;
  }

  /** PropertyManager ✓ / Enter: apply the operation and end the tool. */
  applyOp(): boolean {
    const tool = this.tool;
    if (!tool || !OP_TOOLS.has(tool)) return false;
    const probe = this.runOp(structuredClone(this.sk), false);
    if (probe.error) {
      this.app.flash(probe.error, "error");
      return false;
    }
    if (tool === "convert") {
      this.app.applyConvert().then(() => this.setTool(null));
      return true;
    }
    this.app.sketchCheckpoint();
    this.runOp(this.sk, false);
    this.setTool(null);
    this.commit();
    return true;
  }

  /** Focus a PropertyManager selection box (clicking it makes graphics picks go there). */
  setOpField(f: OpField) {
    this.st.opField = f;
    this.app.changed();
  }

  // ================================================================ helpers

  private wake(raw: Vec2) {
    const tol = this.tol;
    for (const p of Object.values(this.sk.points)) {
      if (Math.hypot(p.x - raw[0], p.y - raw[1]) < tol) {
        this.woken = [p.id, ...this.woken.filter((w) => w !== p.id)].slice(0, 6);
        break;
      }
    }
  }

  private computeSnap(raw: Vec2): SnapResult {
    const tool = this.tool;
    const lineTool = !!tool && LINE_TOOLS.has(tool);
    const anchor = this.st.pts.length && lineTool ? this.st.pts[this.st.pts.length - 1].p : undefined;
    const exclude = new Set<Id>();
    const r = infer(this.sk, raw, {
      anchor: this.st.arcMode ? undefined : anchor,
      prevLine: this.st.arcMode ? undefined : this.st.prevLine,
      tol: this.tol,
      woken: this.woken,
      exclude,
      ctrl: this.ctrl,
      options: { ...this.app.options.sketch, gridSpacing: this.gridSnap() },
      hvFromAnchor: lineTool,
    });
    return anchor && !this.st.arcMode ? this.applyLineOrientation(r, anchor) : r;
  }

  /** Insert Line PropertyManager > Orientation: Horizontal / Vertical / Angle force the direction. */
  private applyLineOrientation(r: SnapResult, anchor: Vec2): SnapResult {
    const o = this.app.toolOpts;
    if (o.lineOrientation === "asSketched") return r;
    let pt: Vec2;
    const out: SnapResult = { ...r, kind: "free", pointId: undefined, entityId: undefined, perpendicularTo: undefined, parallelTo: undefined, hv: undefined, badges: [] };
    if (o.lineOrientation === "horizontal") {
      pt = [r.point[0], anchor[1]];
      out.hv = "horizontal";
      out.badges = ["horizontal"];
    } else if (o.lineOrientation === "vertical") {
      pt = [anchor[0], r.point[1]];
      out.hv = "vertical";
      out.badges = ["vertical"];
    } else {
      const d = v2.fromAngle((o.lineAngle * Math.PI) / 180);
      pt = v2.add(anchor, v2.scale(d, v2.dot(v2.sub(r.point, anchor), d)));
    }
    out.point = pt;
    out.guides = [{ from: anchor, to: pt, relation: !!out.hv }];
    return out;
  }

  shapeOpts(): ShapeOpts {
    const t = this.app.toolOpts;
    return { construction: t.construction, sides: t.polygonSides, inscribed: t.polygonInscribed, constructionLines: t.rectConstruction, fromMidpoints: t.rectFromMidpoints };
  }

  private buildCtx(): BuildCtx {
    return {
      newPoint: (p) => this.newPoint(p),
      pointFor: (s) => this.pointFor(s),
      addEntity: (e) => this.addEntity(e),
      addRel: (r) => this.addRel(r),
      autoRel: this.autoRel,
    };
  }

  /** Grid snap spacing from Document Properties > Grid/Snap. */
  private gridSnap() {
    const major = Number(this.app.docProp("grid.major") ?? 100);
    const minorN = Math.max(1, Number(this.app.docProp("grid.minorPerMajor") ?? 4));
    const snapN = Math.max(1, Number(this.app.docProp("grid.snapPerMinor") ?? 1));
    return major / minorN / snapN;
  }

  private newPoint(p: Vec2, standalone = false): Id {
    const id = uid("p");
    this.sk.points[id] = { id, x: p[0], y: p[1], ...(standalone ? { standalone: true } : {}) };
    return id;
  }

  private addRel(r: NewRelation) {
    const id = uid("r");
    this.sk.relations[id] = { ...(r as object), id } as Relation;
    return id;
  }

  /** Point for a snap: merges with an existing point or creates one plus relations. */
  private pointFor(s: SnapResult): Id {
    if (s.kind === "point" && s.pointId && this.autoRel) return s.pointId;
    const id = this.newPoint(s.point);
    if (!this.autoRel) return id;
    if ((s.kind === "onEntity" || s.kind === "quadrant") && s.entityId) this.addRel({ type: "coincident", p: id, e: s.entityId });
    else if (s.kind === "midpoint" && s.entityId) this.addRel({ type: "midpoint", p: id, e: s.entityId });
    else if (s.kind === "intersection" && s.entityId && s.entity2) this.addRel({ type: "intersection", p: id, e1: s.entityId, e2: s.entity2 });
    return id;
  }

  private addEntity(e: SketchEntity) {
    this.sk.entities[e.id] = e;
    return e.id;
  }

  /** Solve + refresh after an edit. */
  private commit() {
    this.app.resolveSketch();
  }

  // ================================================================ tools

  private toolClick(fromRelease: boolean) {
    const tool = this.tool!;
    const s = this.snap!;
    const st = this.st;
    const shape = SHAPES[tool];
    if (shape) {
      if (st.pts.length && v2.dist(st.pts[st.pts.length - 1].p, s.point) < 1e-9) return;
      st.pts.push({ p: s.point, snap: s });
      if (st.pts.length < shape.clicks) return this.app.changed();
      this.app.sketchCheckpoint();
      shape.create(this.buildCtx(), st.pts.map((x) => x.snap), this.shapeOpts());
      this.st = { pts: [] };
      this.commit();
      return;
    }
    switch (tool) {
      case "spline": {
        // Each click adds a through point; double-click (or Enter) finishes the spline.
        if (st.pts.length && v2.dist(st.pts[st.pts.length - 1].p, s.point) < 1e-9) return;
        st.pts.push({ p: s.point, snap: s });
        return this.app.changed();
      }
      case "line":
      case "centerline": {
        if (!st.pts.length) {
          st.pts.push({ p: s.point, snap: s });
          return this.app.requestRender();
        }
        const start = st.pts[0];
        if (v2.dist(start.p, s.point) < 1e-9) return;
        this.app.sketchCheckpoint();
        const p1 = start.pid ?? this.pointFor(start.snap);
        const mergedExisting = s.kind === "point" && !!s.pointId && entitiesUsingPoint(this.sk, s.pointId).length > 0 && this.autoRel;
        const p2 = this.pointFor(s);
        let newId: Id;
        if (st.arcMode && st.prevEnt) {
          newId = this.tangentArcFrom(st.prevEnt, p1, p2, tool === "centerline");
          st.arcMode = false;
        } else {
          newId = this.addEntity({ id: uid("e"), type: "line", p1, p2, ...(tool === "centerline" ? { construction: true } : {}) });
          if (this.autoRel) {
            if (s.hv) this.addRel({ type: s.hv, e: newId });
            if (s.perpendicularTo) this.addRel({ type: "perpendicular", e1: s.perpendicularTo, e2: newId });
            if (s.parallelTo) this.addRel({ type: "parallel", e1: s.parallelTo, e2: newId });
          }
        }
        this.commit();
        // Click-drag creates a single line; closing onto existing geometry ends the chain.
        if (fromRelease || mergedExisting) {
          this.st = { pts: [] };
        } else {
          const np = this.sk.points[p2];
          const ent = this.sk.entities[newId];
          this.st = { pts: [{ p: [np.x, np.y], snap: s, pid: p2 }], prevLine: ent.type === "line" ? newId : undefined, prevEnt: newId };
        }
        this.app.changed();
        return;
      }
      case "rectangle":
      case "centerRectangle": {
        if (!st.pts.length) {
          st.pts.push({ p: s.point, snap: s });
          return this.app.requestRender();
        }
        const a = st.pts[0];
        if (Math.abs(a.p[0] - s.point[0]) < 1e-9 || Math.abs(a.p[1] - s.point[1]) < 1e-9) return;
        this.app.sketchCheckpoint();
        const ro = this.shapeOpts();
        if (tool === "rectangle") {
          const p1 = this.pointFor(a.snap);
          const p3 = this.pointFor(s);
          const q1 = P(this.sk, p1), q3 = P(this.sk, p3);
          const p2 = this.newPoint([q3[0], q1[1]]);
          const p4 = this.newPoint([q1[0], q3[1]]);
          const ls = this.rectLines(p1, p2, p3, p4);
          if (ro.constructionLines) rectConstruction(this.buildCtx(), [p1, p2, p3, p4], ls, [q1, [q3[0], q1[1]], q3, [q1[0], q3[1]]], ro);
        } else {
          const c = a.p, k = s.point;
          const dx = k[0] - c[0], dy = k[1] - c[1];
          const pts = [
            this.newPoint([c[0] - dx, c[1] - dy]),
            this.newPoint([c[0] + dx, c[1] - dy]),
            this.pointFor(s),
            this.newPoint([c[0] - dx, c[1] + dy]),
          ];
          const ls = this.rectLines(pts[0], pts[1], pts[2], pts[3]);
          const pos: Vec2[] = [[c[0] - dx, c[1] - dy], [c[0] + dx, c[1] - dy], [c[0] + dx, c[1] + dy], [c[0] - dx, c[1] + dy]];
          rectConstruction(this.buildCtx(), pts, ls, pos, ro, a.snap);
        }
        this.st = { pts: [] };
        this.commit();
        return;
      }
      case "circle": {
        if (!st.pts.length) {
          st.pts.push({ p: s.point, snap: s });
          return this.app.requestRender();
        }
        const c = st.pts[0];
        const r = v2.dist(c.p, s.point);
        if (r < 1e-9) return;
        this.app.sketchCheckpoint();
        const cid = this.pointFor(c.snap);
        const id = this.addEntity({ id: uid("e"), type: "circle", c: cid, r });
        if (this.autoRel && s.kind === "point" && s.pointId) this.addRel({ type: "coincident", p: s.pointId, e: id });
        this.st = { pts: [] };
        this.commit();
        return;
      }
      case "arc": {
        if (st.pts.length < 2) {
          st.pts.push({ p: s.point, snap: s });
          if (st.pts.length === 2) {
            st.sweep = 0;
            st.lastAngle = v2.angle(v2.sub(s.point, st.pts[0].p));
          }
          return this.app.requestRender();
        }
        const g = this.centerArcPreview();
        if (!g || Math.abs(g.sweep) < 1e-6) return;
        this.app.sketchCheckpoint();
        const cid = this.pointFor(st.pts[0].snap);
        const sid = this.pointFor(st.pts[1].snap);
        const eid = s.kind === "point" && s.pointId && this.autoRel ? s.pointId : this.newPoint(g.end);
        const [p1, p2] = g.sweep > 0 ? [sid, eid] : [eid, sid];
        this.addEntity({ id: uid("e"), type: "arc", c: cid, p1, p2 });
        this.st = { pts: [] };
        this.commit();
        return;
      }
      case "threePointArc": {
        if (st.pts.length < 2) {
          st.pts.push({ p: s.point, snap: s });
          return this.app.requestRender();
        }
        const [S, E] = st.pts;
        const c = circumcenter(S.p, s.point, E.p);
        if (!c) return;
        this.app.sketchCheckpoint();
        const ps = this.pointFor(S.snap);
        const pe = this.pointFor(E.snap);
        const cid = this.newPoint(c);
        const ccw = v2.cross(v2.sub(E.p, S.p), v2.sub(s.point, S.p)) < 0;
        this.addEntity({ id: uid("e"), type: "arc", c: cid, p1: ccw ? ps : pe, p2: ccw ? pe : ps });
        this.st = { pts: [] };
        this.commit();
        return;
      }
      case "tangentArc": {
        if (!st.pts.length) {
          // Must start on an endpoint of a line or arc.
          if (s.kind !== "point" || !s.pointId) {
            this.app.flash("Tangent arc: start on the endpoint of a line or arc.", "info");
            return;
          }
          const ent = entitiesUsingPoint(this.sk, s.pointId).find((x) => x.type === "line" || x.type === "arc");
          if (!ent) return;
          st.pts.push({ p: s.point, snap: s, pid: s.pointId });
          st.prevEnt = ent.id;
          return this.app.requestRender();
        }
        this.app.sketchCheckpoint();
        const p2 = this.pointFor(s);
        this.tangentArcFrom(st.prevEnt!, st.pts[0].pid!, p2, false);
        this.st = { pts: [] };
        this.commit();
        return;
      }
      case "point": {
        this.app.sketchCheckpoint();
        const pid = this.newPoint(s.point, true);
        this.addEntity({ id: uid("e"), type: "point", p: pid });
        if (this.autoRel) {
          if ((s.kind === "onEntity" || s.kind === "quadrant") && s.entityId) this.addRel({ type: "coincident", p: pid, e: s.entityId });
          if (s.kind === "midpoint" && s.entityId) this.addRel({ type: "midpoint", p: pid, e: s.entityId });
          if (s.kind === "point" && s.pointId) this.addRel({ type: "coincident", p: pid, e: s.pointId });
        }
        this.commit();
        return;
      }
    }
  }

  private rectLines(p1: Id, p2: Id, p3: Id, p4: Id) {
    const l1 = this.addEntity({ id: uid("e"), type: "line", p1, p2 });
    const l2 = this.addEntity({ id: uid("e"), type: "line", p1: p2, p2: p3 });
    const l3 = this.addEntity({ id: uid("e"), type: "line", p1: p3, p2: p4 });
    const l4 = this.addEntity({ id: uid("e"), type: "line", p1: p4, p2: p1 });
    this.addRel({ type: "horizontal", e: l1 });
    this.addRel({ type: "horizontal", e: l3 });
    this.addRel({ type: "vertical", e: l2 });
    this.addRel({ type: "vertical", e: l4 });
    return [l1, l2, l3, l4];
  }

  // ================================================================ sketch fillet

  private filletClick(e: PointerInfo) {
    const sk = this.sk;
    const h = this.hitTest(e.sx, e.sy, false);
    if (!h) return;
    const linesAt = (pid: Id) => entitiesUsingPoint(sk, pid).filter((x) => x.type === "line" && !x.construction);
    if (h.kind === "point") {
      const ls = linesAt(h.id);
      if (ls.length !== 2) return this.app.flash("Select a corner where two lines meet, or select two lines.", "info");
      this.cornerOp(ls[0].id, ls[1].id, h.id);
      return;
    }
    if (h.kind !== "entity" || sk.entities[h.id]?.type !== "line") return;
    if (!this.st.filletFirst || this.st.filletFirst === h.id) {
      this.st.filletFirst = h.id;
      this.app.requestRender();
      return;
    }
    const a = sk.entities[this.st.filletFirst], b = sk.entities[h.id];
    this.st.filletFirst = undefined;
    if (!a || !b || a.type !== "line" || b.type !== "line") return;
    const shared = [a.p1, a.p2].find((p) => p === b.p1 || p === b.p2);
    if (!shared) return this.app.flash("Select two lines that share a corner.", "info");
    this.cornerOp(a.id, b.id, shared);
  }

  /** Sketch Fillet or Sketch Chamfer at a corner. */
  private cornerOp(l1: Id, l2: Id, v: Id) {
    if (this.tool !== "sketchChamfer") return this.filletCorner(l1, l2, v);
    const o = this.app.toolOpts;
    const opts = { type: o.chamferType, d1: o.chamferD1, d2: o.chamferType === "angleDistance" ? o.chamferAngle : o.chamferD2, equal: o.chamferEqual };
    const probe = chamferCorner(structuredClone(this.sk), l1, l2, v, opts);
    if (probe.error) return this.app.flash(probe.error, "error");
    this.app.sketchCheckpoint();
    chamferCorner(this.sk, l1, l2, v, opts);
    this.commit();
  }

  private filletCorner(l1: Id, l2: Id, v: Id) {
    const sk = this.sk;
    const a = sk.entities[l1], b = sk.entities[l2];
    if (!a || !b || a.type !== "line" || b.type !== "line") return;
    const o = this.app.toolOpts;
    const A = a.p1 === v ? a.p2 : a.p1, B = b.p1 === v ? b.p2 : b.p1;
    const V = P(sk, v);
    const g = filletGeom(V, P(sk, A), P(sk, B), o.filletRadius);
    if (!g) return this.app.flash("The fillet radius is too large for the selected entities.", "error");
    this.app.sketchCheckpoint();
    const t1 = this.newPoint(g.T1), t2 = this.newPoint(g.T2), c = this.newPoint(g.C);
    if (a.p1 === v) a.p1 = t1;
    else a.p2 = t1;
    if (b.p1 === v) b.p1 = t2;
    else b.p2 = t2;
    const arc = this.addEntity({ id: uid("e"), type: "arc", c, p1: g.ccw ? t1 : t2, p2: g.ccw ? t2 : t1 });
    this.addRel({ type: "tangent", e1: l1, e2: arc });
    this.addRel({ type: "tangent", e1: l2, e2: arc });
    // Keep constrained corners: the old corner survives as a virtual sharp on both lines.
    const referenced =
      v === ORIGIN_ID ||
      entitiesUsingPoint(sk, v).length > 0 ||
      Object.values(sk.relations).some((r) => relationRefs(r).includes(v)) ||
      Object.values(sk.dimensions).some((d) => d.refs.includes(v));
    if (referenced && o.filletKeepCorners) {
      this.addRel({ type: "coincident", p: v, e: l1 });
      this.addRel({ type: "coincident", p: v, e: l2 });
    } else if (v !== ORIGIN_ID && !entitiesUsingPoint(sk, v).length) {
      delete sk.points[v];
      pruneRefs(sk);
    }
    const first = !Object.values(sk.dimensions).some((d) => d.kind === "radius" && Math.abs(d.value - o.filletRadius) < 1e-9);
    if (o.filletDimensionEach || first) {
      const out = v2.norm(v2.sub(g.C, V));
      const id = uid("d");
      sk.dimensions[id] = { id, name: `D${sk.nextDim++}`, kind: "radius", refs: [arc], value: o.filletRadius, label: v2.add(g.C, v2.scale(out, o.filletRadius * 1.2)) };
    } else {
      const other = Object.values(sk.dimensions).find((d) => d.kind === "radius" && Math.abs(d.value - o.filletRadius) < 1e-9);
      if (other) this.addRel({ type: "equal", e1: other.refs[0], e2: arc });
    }
    this.commit();
  }

  // ================================================================ selection helpers

  private selectedIds(): Id[] {
    return this.app.selection.filter((s) => s.kind === "skEntity" || s.kind === "skPoint").map((s) => (s as { id: Id }).id);
  }

  /** Add a relation of the given type to the current selection (Add Relations / context toolbar). */
  addRelation(type: RelationType) {
    const sk = this.sk;
    const r = buildRelation(sk, type, this.selectedIds());
    if (!r) return;
    this.app.sketchCheckpoint();
    const id = this.addRel(r);
    this.commit();
    if (this.app.sketch!.analysis.bad.has(id)) this.app.flash("Adding this relation over defines the sketch.", "warning");
  }

  /** Dimension the current selection without picking a placement (context toolbar). */
  autoDimension() {
    const refs: Hit[] = this.app.selection.flatMap((s): Hit[] => (s.kind === "skEntity" ? [{ kind: "entity", id: s.id }] : s.kind === "skPoint" ? [{ kind: "point", id: s.id }] : [])).slice(0, 2);
    if (!refs.length) return this.setTool("smartDimension");
    const anchors = refs.map((r) => this.anchorOf(r.id)).filter(Boolean) as Vec2[];
    const mid: Vec2 = [anchors.reduce((a, p) => a + p[0], 0) / anchors.length, anchors.reduce((a, p) => a + p[1], 0) / anchors.length];
    const off = 28 * this.view.pxSize();
    const dim = this.buildDimension(refs, [mid[0] + off * 0.6, mid[1] + off]);
    if (dim) this.addDimension(dim);
  }

  /** Sketch-space points bounding the selection (Zoom to Selection). */
  selectionPoints(): Vec2[] {
    const out: Vec2[] = [];
    for (const id of this.selectedIds()) {
      const e = this.sk.entities[id];
      if (e) out.push(...sampleEntity(this.sk, e, 24));
      else if (this.sk.points[id]) out.push(P(this.sk, id));
    }
    return out;
  }

  /** Edit a value typed in the entity PropertyManager (Parameters). Moves geometry as far as relations allow. */
  setEntityParam(id: Id, key: string, value: number) {
    const sk = this.sk;
    const e = sk.entities[id];
    const pt = sk.points[id];
    const targets: { id: Id; to: Vec2 }[] = [];
    if (pt) {
      targets.push({ id, to: key === "x" ? [value, pt.y] : [pt.x, value] });
    } else if (e?.type === "line") {
      const a = P(sk, e.p1), b = P(sk, e.p2);
      const len = v2.dist(a, b), ang = v2.angle(v2.sub(b, a));
      if (key === "length") targets.push({ id: e.p2, to: v2.add(a, v2.fromAngle(ang, Math.max(1e-6, value))) });
      else if (key === "angle") targets.push({ id: e.p2, to: v2.add(a, v2.fromAngle(value, len)) });
      else if (key === "x1") targets.push({ id: e.p1, to: [value, a[1]] });
      else if (key === "y1") targets.push({ id: e.p1, to: [a[0], value] });
      else if (key === "x2") targets.push({ id: e.p2, to: [value, b[1]] });
      else if (key === "y2") targets.push({ id: e.p2, to: [b[0], value] });
      else if (key === "dx") targets.push({ id: e.p2, to: [a[0] + value, b[1]] });
      else if (key === "dy") targets.push({ id: e.p2, to: [b[0], a[1] + value] });
    } else if (e?.type === "circle" || e?.type === "arc") {
      const c = P(sk, e.c);
      if (key === "cx") targets.push({ id: e.c, to: [value, c[1]] });
      else if (key === "cy") targets.push({ id: e.c, to: [c[0], value] });
      else if (key === "r" && value > 0) {
        if (e.type === "circle") {
          this.app.sketchCheckpoint();
          const prev = e.r;
          e.r = value;
          const sys = new SketchSystem(sk);
          if (sys.solve()) sys.commit();
          else e.r = prev;
          this.commit();
          return;
        }
        for (const p of [e.p1, e.p2]) targets.push({ id: p, to: v2.add(c, v2.scale(v2.norm(v2.sub(P(sk, p), c)), value)) });
      }
    } else if (e?.type === "point") {
      const q = P(sk, e.p);
      targets.push({ id: e.p, to: key === "x" ? [value, q[1]] : [q[0], value] });
    }
    if (!targets.length) return;
    this.app.sketchCheckpoint();
    const sys = new SketchSystem(sk);
    sys.solve();
    sys.drag(targets, 24);
    sys.commit();
    this.commit();
  }

  /** Tangent direction of an entity leaving point pid (pointing away from the entity). */
  private tangentAt(entId: Id, pid: Id): Vec2 | null {
    const e = this.sk.entities[entId];
    if (!e) return null;
    if (e.type === "line") {
      const other = e.p1 === pid ? e.p2 : e.p1;
      return v2.norm(v2.sub(P(this.sk, pid), P(this.sk, other)));
    }
    if (e.type === "arc") {
      const c = P(this.sk, e.c);
      const r = v2.sub(P(this.sk, pid), c);
      // CCW tangent at p2 continues forward; at p1 the arc arrives from the opposite way.
      const t = v2.norm(v2.perp(r));
      return e.p2 === pid ? t : v2.scale(t, -1);
    }
    return null;
  }

  private tangentArcGeom(entId: Id, start: Vec2, startPid: Id | undefined, end: Vec2) {
    const t = startPid ? this.tangentAt(entId, startPid) : null;
    if (!t) return null;
    const n = v2.perp(t);
    const d = v2.sub(end, start);
    const den = 2 * v2.dot(d, n);
    if (Math.abs(den) < 1e-9) return null;
    const rs = v2.dot(d, d) / den;
    const c = v2.add(start, v2.scale(n, rs));
    return { c, ccw: rs > 0 };
  }

  private tangentArcFrom(entId: Id, p1: Id, p2: Id, construction: boolean): Id {
    const g = this.tangentArcGeom(entId, P(this.sk, p1), p1, P(this.sk, p2));
    if (!g) return this.addEntity({ id: uid("e"), type: "line", p1, p2 });
    const cid = this.newPoint(g.c);
    const id = this.addEntity({ id: uid("e"), type: "arc", c: cid, p1: g.ccw ? p1 : p2, p2: g.ccw ? p2 : p1, ...(construction ? { construction: true } : {}) });
    this.addRel({ type: "tangent", e1: entId, e2: id });
    return id;
  }

  private trackSweep() {
    const st = this.st;
    if (!this.snap || st.lastAngle === undefined) return;
    const a = v2.angle(v2.sub(this.snap.point, st.pts[0].p));
    let d = a - st.lastAngle;
    if (d > Math.PI) d -= 2 * Math.PI;
    if (d < -Math.PI) d += 2 * Math.PI;
    st.sweep = (st.sweep ?? 0) + d;
    st.lastAngle = a;
  }

  private centerArcPreview() {
    const st = this.st;
    if (st.pts.length < 2 || !this.snap) return null;
    const c = st.pts[0].p, s = st.pts[1].p;
    const r = v2.dist(c, s);
    const a0 = v2.angle(v2.sub(s, c));
    let sw = st.sweep ?? 0;
    const a1 = v2.angle(v2.sub(this.snap.point, c));
    // Exact end angle, keeping direction from the accumulated sweep.
    if (sw >= 0) sw = wrap2Pi(a1 - a0) || 2 * Math.PI;
    else sw = -(wrap2Pi(a0 - a1) || 2 * Math.PI);
    return { c, r, a0, sweep: sw, end: v2.add(c, v2.fromAngle(a0 + sw, r)) };
  }

  // ================================================================ selection & drag

  hitTest(sx: number, sy: number, includeDims = true, entitiesOnly = false): Hit | null {
    const sk = this.sk;
    const V = this.view;
    if (!entitiesOnly) {
      let bp: { id: Id; d: number } | null = null;
      for (const p of Object.values(sk.points)) {
        const [x, y] = V.toScreen([p.x, p.y]);
        const d = Math.hypot(x - sx, y - sy);
        if (d < PICK_PX && (!bp || d < bp.d)) bp = { id: p.id, d };
      }
      if (bp) return { kind: "point", id: bp.id };
      // Line midpoints can be picked directly, as in SOLIDWORKS.
      let bm: { id: Id; d: number } | null = null;
      for (const e of Object.values(sk.entities)) {
        if (e.type !== "line") continue;
        const [x, y] = V.toScreen(v2.lerp(P(sk, e.p1), P(sk, e.p2), 0.5));
        const d = Math.hypot(x - sx, y - sy);
        if (d < PICK_PX - 1 && (!bm || d < bm.d)) bm = { id: e.id, d };
      }
      if (bm) {
        const existing = this.virtualMid(bm.id);
        return existing ? { kind: "point", id: existing } : { kind: "mid", id: bm.id };
      }
      if (includeDims) {
        for (const d of Object.values(sk.dimensions)) {
          const b = this.dimLabelBox(d);
          if (b && sx >= b[0] - 3 && sx <= b[2] + 3 && sy >= b[1] - 3 && sy <= b[3] + 3) return { kind: "dim", id: d.id };
        }
      }
    }
    const raw = V.toSketch(sx, sy);
    if (!raw) return null;
    let be: { id: Id; d: number } | null = null;
    for (const e of Object.values(sk.entities)) {
      if (e.type === "point") continue;
      const c = closestOnEntity(sk, e, raw);
      if (!c) continue;
      const d = c.dist / V.pxSize();
      if (d < PICK_PX && (!be || d < be.d)) be = { id: e.id, d };
    }
    return be ? { kind: "entity", id: be.id } : null;
  }

  /** Existing midpoint helper point of a line. */
  private virtualMid(lineId: Id): Id | null {
    const r = Object.values(this.sk.relations).find((x) => x.type === "midpoint" && x.e === lineId && this.sk.points[x.p]?.virtual);
    return r && r.type === "midpoint" ? r.p : null;
  }

  /** Turn a picked line midpoint into a point that relations and dimensions can use. */
  resolveHit(h: Hit | null): Hit | null {
    if (!h || h.kind !== "mid") return h;
    const existing = this.virtualMid(h.id);
    if (existing) return { kind: "point", id: existing };
    const line = this.sk.entities[h.id];
    if (line?.type !== "line") return null;
    const id = uid("p");
    const m = v2.lerp(P(this.sk, line.p1), P(this.sk, line.p2), 0.5);
    this.sk.points[id] = { id, x: m[0], y: m[1], virtual: true };
    const rid = uid("r");
    this.sk.relations[rid] = { id: rid, type: "midpoint", p: id, e: h.id };
    this.app.sketch!.analysis = solveSketch(this.sk);
    return { kind: "point", id };
  }

  private selectDown(e: PointerInfo, raw: Vec2) {
    const h = this.resolveHit(this.hitTest(e.sx, e.sy));
    const mode = e.ctrl ? "toggle" : e.shift ? "add" : "replace";
    const region = h ? null : this.regionAt(e.sx, e.sy);
    if (region) {
      // Clicking inside a shaded contour selects the entities that bound it; dragging moves them.
      const ents = this.regionEnts(region);
      const already = ents.every((id) => this.app.isSelected({ kind: "skEntity", id }));
      if (mode === "replace" && !already) this.app.selection = [];
      for (const id of ents) this.app.select({ kind: "skEntity", id }, mode === "toggle" ? "toggle" : "add");
      const ids = [
        ...new Set(
          ents.flatMap((id) => {
            const en = this.sk.entities[id];
            return en.type === "arc" ? [en.c, en.p1, en.p2] : entityPoints(en);
          }),
        ),
      ];
      this.drag = { kind: "points", ids, start: raw, orig: ids.map((i) => P(this.sk, i)), begun: false };
      return;
    }
    if (!h) {
      if (mode === "replace") this.app.select(null);
      this.drag = { kind: "box", x0: e.sx, y0: e.sy, x1: e.sx, y1: e.sy };
      return;
    }
    const item = h.kind === "point" ? ({ kind: "skPoint", id: h.id } as const) : h.kind === "dim" ? ({ kind: "skDim", id: h.id } as const) : ({ kind: "skEntity", id: h.id } as const);
    if (!(mode === "replace" && this.app.isSelected(item))) this.app.select(item, mode);
    if (h.kind === "dim") {
      this.drag = { kind: "dimLabel", id: h.id, begun: false };
      return;
    }
    if (h.kind === "point") {
      this.drag = { kind: "points", ids: [h.id], start: raw, orig: [P(this.sk, h.id)], begun: false };
      return;
    }
    const ent = this.sk.entities[h.id];
    if (ent.type === "circle") {
      this.drag = { kind: "radius", id: h.id, begun: false };
      return;
    }
    const ids = ent.type === "arc" ? [ent.c, ent.p1, ent.p2] : entityPoints(ent);
    this.drag = { kind: "points", ids, start: raw, orig: ids.map((i) => P(this.sk, i)), begun: false };
  }

  private dragMove(e: PointerInfo, raw: Vec2) {
    const d = this.drag!;
    if (d.kind === "box") {
      d.x1 = e.sx;
      d.y1 = e.sy;
      this.app.requestRender();
      return;
    }
    if (!d.begun) {
      if (!this.downScreen || Math.hypot(e.sx - this.downScreen[0], e.sy - this.downScreen[1]) < 3) return;
      d.begun = true;
      this.app.sketchCheckpoint();
    }
    if (d.kind === "dimLabel") {
      this.sk.dimensions[d.id].label = raw;
      this.app.changed();
      return;
    }
    if (d.kind === "radius") {
      const ent = this.sk.entities[d.id];
      if (ent.type !== "circle") return;
      const sys = new SketchSystem(this.sk);
      const prev = ent.r;
      ent.r = v2.dist(P(this.sk, ent.c), raw);
      const sys2 = new SketchSystem(this.sk);
      if (sys2.solve()) {
        sys2.commit();
      } else {
        ent.r = prev;
        sys.solve();
      }
      this.app.sketch!.analysis = solveSketch(this.sk);
      this.app.changed();
      return;
    }
    const delta = v2.sub(raw, d.start);
    const sys = new SketchSystem(this.sk);
    sys.solve();
    sys.drag(d.ids.map((id, i) => ({ id, to: v2.add(d.orig[i], delta) })));
    sys.commit();
    this.app.sketch!.analysis = sys.analyse(true);
    this.app.changed();
  }

  private dragEnd(e: PointerInfo) {
    const d = this.drag!;
    this.drag = null;
    if (d.kind === "box") {
      if (Math.hypot(d.x1 - d.x0, d.y1 - d.y0) > 3) this.boxSelect(d, e);
      this.app.requestRender();
      return;
    }
    if ("begun" in d && d.begun) this.commit();
  }

  /** Left→right: window (fully inside). Right→left: crossing (touching). */
  private boxSelect(b: { x0: number; y0: number; x1: number; y1: number }, e: PointerInfo) {
    const xa = Math.min(b.x0, b.x1), xb = Math.max(b.x0, b.x1), ya = Math.min(b.y0, b.y1), yb = Math.max(b.y0, b.y1);
    const crossing = b.x1 < b.x0;
    const inside = ([x, y]: [number, number]) => x >= xa && x <= xb && y >= ya && y <= yb;
    const mode = e.ctrl ? "toggle" : "add";
    for (const ent of Object.values(this.sk.entities)) {
      if (ent.type === "point") continue;
      const pts = sampleEntity(this.sk, ent, 32).map((p) => this.view.toScreen(p));
      const ok = crossing ? pts.some(inside) || segmentsCrossRect(pts, xa, ya, xb, yb) : pts.every(inside);
      if (ok) this.app.select({ kind: "skEntity", id: ent.id }, mode);
    }
    for (const p of Object.values(this.sk.points)) {
      if (p.id === ORIGIN_ID) continue;
      if (inside(this.view.toScreen([p.x, p.y]))) this.app.select({ kind: "skPoint", id: p.id }, mode);
    }
  }

  deleteSelection() {
    const ids = new Set<Id>();
    for (const s of this.app.selection) {
      if (s.kind === "skEntity" || s.kind === "skPoint" || s.kind === "skDim") ids.add(s.id);
    }
    // Relations are deleted from the PropertyManager list; also accept direct ids.
    if (!ids.size) return;
    this.app.sketchCheckpoint();
    // Deleting a free point that is shared by entities deletes those entities.
    for (const id of [...ids]) {
      if (this.sk.points[id] && !this.sk.points[id].standalone) entitiesUsingPoint(this.sk, id).forEach((e) => ids.add(e.id));
      const pe = Object.values(this.sk.entities).find((e) => e.type === "point" && e.p === id);
      if (pe) ids.add(pe.id);
    }
    deleteFromSketch(this.sk, ids);
    this.app.selection = [];
    this.commit();
  }

  // ================================================================ smart dimension

  private dimClick(e: PointerInfo, raw: Vec2) {
    const h = this.resolveHit(this.hitTest(e.sx, e.sy, true));
    const st = this.st;
    if (h?.kind === "dim") {
      this.openModify(h.id, false);
      return;
    }
    if (h && (!st.dimRefs || st.dimRefs.length < 2)) {
      const refs = st.dimRefs ?? [];
      if (!refs.some((r) => r.id === h.id)) {
        st.dimRefs = [...refs, h];
        // A single point cannot be dimensioned on its own: wait for a second pick.
        this.app.requestRender();
        return;
      }
    }
    if (!st.dimRefs?.length) return;
    const dim = this.buildDimension(st.dimRefs, raw);
    st.dimRefs = undefined;
    if (!dim) return;
    this.addDimension(dim);
  }

  /** Add a built dimension; over-defining ones become driven (or warn), then open Modify. */
  addDimension(dim: Dimension) {
    this.app.sketchCheckpoint();
    this.sk.dimensions[dim.id] = dim;
    this.sk.nextDim++;
    const a = solveSketch(this.sk);
    if (a.bad.has(dim.id)) {
      if (this.app.opt("sketch.drivenDefault") !== false) {
        dim.driven = true;
        this.app.flash("This dimension would over define the sketch; it was added as a driven (reference) dimension.", "warning");
      } else this.app.flash("Adding this dimension over defines the sketch.", "warning");
    }
    this.commit();
    if (!dim.driven && this.app.options.sketch.inputDimensionValue) this.openModify(dim.id, true);
  }

  openModify(dimId: Id, isNew: boolean) {
    const d = this.sk.dimensions[dimId];
    if (!d) return;
    const b = this.dimLabelBox(d);
    const [x, y] = b ? [b[0], b[3] + 4] : this.view.toScreen(d.label);
    this.app.modify = { dimId, x, y, isNew };
    this.app.changed();
  }

  /** Apply a value typed into the Modify box. */
  setDimensionValue(dimId: Id, value: number) {
    const d = this.sk.dimensions[dimId];
    if (!d) return false;
    if (d.driven) return false;
    const prev = d.value;
    d.value = value;
    const sys = new SketchSystem(this.sk);
    const ok = sys.solve(200);
    if (!ok) {
      d.value = prev;
      this.app.flash("The sketch cannot be solved with this value.", "error");
      return false;
    }
    sys.commit();
    this.commit();
    return true;
  }

  /** Decide dimension type from the picked references and the placement point. */
  buildDimension(refs: Hit[], at: Vec2): Dimension | null {
    const sk = this.sk;
    const name = `D${sk.nextDim}`;
    const base = { id: uid("d"), name, label: at } as const;
    const ent = (h: Hit) => (h.kind === "entity" ? sk.entities[h.id] : null);
    const ptOf = (h: Hit): Id | null => {
      if (h.kind === "point") return h.id;
      const e = ent(h);
      if (e && (e.type === "circle" || e.type === "arc")) return e.c;
      return null;
    };
    if (refs.length === 1) {
      const e = ent(refs[0]);
      if (!e) return null;
      if (e.type === "line") return this.pointPairDim(base, e.p1, e.p2, at);
      if (e.type === "circle") return { ...base, kind: "diameter", refs: [e.id], value: 2 * e.r };
      if (e.type === "arc") return { ...base, kind: "radius", refs: [e.id], value: arcGeom(sk, e).r };
      return null;
    }
    const [a, b] = refs;
    const ea = ent(a), eb = ent(b);
    if (ea?.type === "line" && eb?.type === "line") {
      const u = v2.norm(v2.sub(P(sk, ea.p2), P(sk, ea.p1)));
      const w = v2.norm(v2.sub(P(sk, eb.p2), P(sk, eb.p1)));
      if (Math.abs(v2.cross(u, w)) < 1e-6) {
        const d: Dimension = { ...base, kind: "lineLine", refs: [ea.id, eb.id], value: 0 };
        d.value = this.measure(d);
        return d;
      }
      return this.angleDim(base, ea.id, eb.id, at);
    }
    // point/centre to line
    const lineH = ea?.type === "line" ? a : eb?.type === "line" ? b : null;
    const other = lineH === a ? b : a;
    if (lineH) {
      const p = ptOf(other);
      if (!p) return null;
      const d: Dimension = { ...base, kind: "pointLine", refs: [p, lineH.id], value: 0 };
      d.value = this.measure(d);
      return d;
    }
    const pa = ptOf(a), pb = ptOf(b);
    if (pa && pb) return this.pointPairDim(base, pa, pb, at);
    return null;
  }

  private pointPairDim(base: { id: Id; name: string; label: Vec2 }, p1: Id, p2: Id, at: Vec2): Dimension {
    const a = P(this.sk, p1), b = P(this.sk, p2);
    const minx = Math.min(a[0], b[0]), maxx = Math.max(a[0], b[0]);
    const miny = Math.min(a[1], b[1]), maxy = Math.max(a[1], b[1]);
    let kind: DimensionKind = "distance";
    const horizontalLine = Math.abs(a[1] - b[1]) < 1e-9, verticalLine = Math.abs(a[0] - b[0]) < 1e-9;
    const forced = this.tool === "dimHorizontal" ? "horizontal" : this.tool === "dimVertical" ? "vertical" : null;
    if (forced) kind = forced;
    else if (!horizontalLine && !verticalLine) {
      if ((at[1] > maxy || at[1] < miny) && at[0] >= minx && at[0] <= maxx) kind = "horizontal";
      else if ((at[0] > maxx || at[0] < minx) && at[1] >= miny && at[1] <= maxy) kind = "vertical";
    } else if (horizontalLine && Math.abs(at[1] - a[1]) < 1e-12) kind = "distance";
    const d: Dimension = { ...base, kind, refs: [p1, p2], value: 0 };
    if (kind === "horizontal") d.sign = b[0] >= a[0] ? 1 : -1;
    if (kind === "vertical") d.sign = b[1] >= a[1] ? 1 : -1;
    d.value = this.measure(d);
    return d;
  }

  private angleDim(base: { id: Id; name: string; label: Vec2 }, l1: Id, l2: Id, at: Vec2): Dimension | null {
    const sk = this.sk;
    const e1 = sk.entities[l1], e2 = sk.entities[l2];
    if (e1.type !== "line" || e2.type !== "line") return null;
    const x = lineIntersection(P(sk, e1.p1), P(sk, e1.p2), P(sk, e2.p1), P(sk, e2.p2));
    if (!x) return null;
    const dirL = v2.norm(v2.sub(at, x));
    const u0 = v2.norm(v2.sub(P(sk, e1.p2), P(sk, e1.p1)));
    const w0 = v2.norm(v2.sub(P(sk, e2.p2), P(sk, e2.p1)));
    // Pick the quadrant whose bisector is closest to the placement direction.
    let best = 0, bestDot = -2;
    for (let q = 0; q < 4; q++) {
      const u = q & 1 ? v2.scale(u0, -1) : u0;
      const w = q & 2 ? v2.scale(w0, -1) : w0;
      const bis = v2.norm(v2.add(u, w));
      const dd = v2.dot(bis, dirL);
      if (dd > bestDot) {
        bestDot = dd;
        best = q;
      }
    }
    const u = best & 1 ? v2.scale(u0, -1) : u0;
    const w = best & 2 ? v2.scale(w0, -1) : w0;
    const ang = Math.atan2(v2.cross(u, w), v2.dot(u, w));
    return { ...base, kind: "angle", refs: [l1, l2], quadrant: best, sign: ang >= 0 ? 1 : -1, value: Math.abs(ang) };
  }

  measure(d: Dimension): number {
    const sys = new SketchSystem(this.sk);
    const f = sys.measureFn(d);
    return f ? Math.abs(f()) : 0;
  }

  // ================================================================ trim

  /** Trim the portion of an entity between its nearest intersections around `at`. */
  trimAt(id: Id, at: Vec2, checkpoint: boolean) {
    const sk = this.sk;
    const e = sk.entities[id];
    if (!e || e.type === "point") return;
    if (checkpoint) this.app.sketchCheckpoint();
    if (e.type === "spline") return this.trimSpline(e, at);
    const xs: { p: Vec2; other: Id }[] = [];
    for (const o of Object.values(sk.entities)) {
      if (o.id === id || o.type === "point") continue;
      for (const p of intersectEntities(sk, id, o.id)) xs.push({ p, other: o.id });
    }
    // Endpoints of other entities lying on this entity also bound trims (T-junctions).
    for (const pt of Object.values(sk.points)) {
      if (entityPoints(e).includes(pt.id)) continue;
      const c = closestOnEntity(sk, e, [pt.x, pt.y]);
      if (c && c.dist < 1e-7) {
        const owner = entitiesUsingPoint(sk, pt.id)[0];
        if (owner) xs.push({ p: [pt.x, pt.y], other: owner.id });
      }
    }
    if (e.type === "line") {
      const a = P(sk, e.p1), b = P(sk, e.p2);
      const L = v2.dist(a, b);
      const tOf = (p: Vec2) => v2.dot(v2.sub(p, a), v2.sub(b, a)) / (L * L);
      const tc = tOf(at);
      const ts = xs.map((x) => ({ t: tOf(x.p), ...x })).filter((x) => x.t > 1e-7 && x.t < 1 - 1e-7);
      const lo = ts.filter((x) => x.t < tc).sort((m, n) => n.t - m.t)[0];
      const hi = ts.filter((x) => x.t > tc).sort((m, n) => m.t - n.t)[0];
      if (!lo && !hi) {
        deleteFromSketch(sk, new Set([id]));
      } else if (lo && hi) {
        const pLo = this.newPoint(lo.p), pHi = this.newPoint(hi.p);
        const oldP2 = e.p2;
        e.p2 = pLo;
        const nid = this.addEntity({ id: uid("e"), type: "line", p1: pHi, p2: oldP2, ...(e.construction ? { construction: true } : {}) });
        this.addRel({ type: "coincident", p: pLo, e: lo.other });
        this.addRel({ type: "coincident", p: pHi, e: hi.other });
        this.addRel({ type: "collinear", e1: id, e2: nid });
        this.moveDimsFromPoint(oldP2, oldP2);
      } else if (lo) {
        const np = this.newPoint(lo.p);
        this.detachPoint(e.p2, id);
        e.p2 = np;
        this.addRel({ type: "coincident", p: np, e: lo.other });
      } else if (hi) {
        const np = this.newPoint(hi.p);
        this.detachPoint(e.p1, id);
        e.p1 = np;
        this.addRel({ type: "coincident", p: np, e: hi.other });
      }
    } else if (e.type === "circle" || e.type === "arc") {
      const c = P(sk, e.c);
      const a0 = e.type === "arc" ? arcGeom(sk, e).a0 : 0;
      const sweep = e.type === "arc" ? arcGeom(sk, e).sweep : 2 * Math.PI;
      const rel = (p: Vec2) => wrap2Pi(v2.angle(v2.sub(p, c)) - a0);
      const tc = rel(at);
      const ts = xs.map((x) => ({ t: rel(x.p), ...x })).filter((x) => x.t > 1e-7 && x.t < sweep - 1e-7);
      const r = e.type === "circle" ? e.r : arcGeom(sk, e).r;
      if (e.type === "circle") {
        if (ts.length < 2) {
          deleteFromSketch(sk, new Set([id]));
        } else {
          const sorted = ts.sort((m, n) => m.t - n.t);
          let hiI = sorted.findIndex((x) => x.t > tc);
          if (hiI < 0) hiI = 0;
          const hi = sorted[hiI], lo = sorted[(hiI - 1 + sorted.length) % sorted.length];
          // Keep the arc from hi to lo (CCW).
          const p1 = this.newPoint(hi.p), p2 = this.newPoint(lo.p);
          delete sk.entities[id];
          const nid = this.addEntity({ id, type: "arc", c: e.c, p1, p2, ...(e.construction ? { construction: true } : {}) });
          for (const d of Object.values(sk.dimensions)) if (d.refs.includes(id) && d.kind === "diameter") d.kind = "radius", d.value = r;
          this.addRel({ type: "coincident", p: p1, e: hi.other });
          this.addRel({ type: "coincident", p: p2, e: lo.other });
          void nid;
        }
      } else {
        const lo = ts.filter((x) => x.t < tc).sort((m, n) => n.t - m.t)[0];
        const hi = ts.filter((x) => x.t > tc).sort((m, n) => m.t - n.t)[0];
        if (!lo && !hi) deleteFromSketch(sk, new Set([id]));
        else if (lo && hi) {
          const pLo = this.newPoint(lo.p), pHi = this.newPoint(hi.p);
          const oldP2 = e.p2;
          e.p2 = pLo;
          const nid = this.addEntity({ id: uid("e"), type: "arc", c: e.c, p1: pHi, p2: oldP2, ...(e.construction ? { construction: true } : {}) });
          this.addRel({ type: "coincident", p: pLo, e: lo.other });
          this.addRel({ type: "coincident", p: pHi, e: hi.other });
          void nid;
        } else if (lo) {
          const np = this.newPoint(lo.p);
          this.detachPoint(e.p2, id);
          e.p2 = np;
          this.addRel({ type: "coincident", p: np, e: lo.other });
        } else if (hi) {
          const np = this.newPoint(hi.p);
          this.detachPoint(e.p1, id);
          e.p1 = np;
          this.addRel({ type: "coincident", p: np, e: hi.other });
        }
      }
    }
    pruneRefs(sk);
    // Remove orphan points.
    const used = new Set<Id>([ORIGIN_ID]);
    for (const en of Object.values(sk.entities)) entityPoints(en).forEach((p) => used.add(p));
    for (const p of Object.keys(sk.points)) if (!used.has(p)) delete sk.points[p];
    pruneRefs(sk);
    this.commit();
  }

  private detachPoint(_pid: Id, _entId: Id) {
    /* the old endpoint is dropped automatically if unused */
  }
  private moveDimsFromPoint(_a: Id, _b: Id) {
    /* dimensions on the original endpoint stay with the second segment */
  }

  // ================================================================ drawing

  draw(ctx: CanvasRenderingContext2D) {
    const s = this.app.sketch;
    if (!s) return;
    const sk = s.data;
    const C = this.app.options.colors;
    const V = this.view;
    const an = s.analysis;
    const selected = new Set(this.app.selection.map((x) => ("id" in x ? String(x.id) : "")));
    const hoverId = this.hover?.id;
    const colorOf = (id: Id, state: string | undefined) =>
      selected.has(id) ? C.selected : hoverId === id ? C.hover : state === "over" ? C.sketchOver : state === "full" ? C.sketchFull : C.sketchUnder;

    // Document grid (Document Properties > Grid/Snap).
    if (this.app.docProp("grid.display")) this.drawGrid(ctx);
    if (this.shading) this.drawRegions(ctx, selected);

    // Origin arrows (sketch origin is red).
    const o = V.toScreen([0, 0]);
    const ox = V.toScreen([1, 0]), oy = V.toScreen([0, 1]);
    const ux = v2.norm(v2.sub(ox as Vec2, o as Vec2)), uy = v2.norm(v2.sub(oy as Vec2, o as Vec2));
    // The sketch origin highlights and selects like any sketch point.
    const originCol = selected.has(ORIGIN_ID) ? C.selected : hoverId === ORIGIN_ID ? C.hover : "#d40000";
    ctx.strokeStyle = originCol;
    ctx.fillStyle = originCol;
    ctx.lineWidth = originCol === "#d40000" ? 1.5 : 2.5;
    if (originCol !== "#d40000") {
      ctx.beginPath();
      ctx.arc(o[0], o[1], 4.5, 0, Math.PI * 2);
      ctx.fill();
    }
    for (const u of [ux, uy]) {
      const tip: Vec2 = [o[0] + u[0] * 26, o[1] + u[1] * 26];
      ctx.beginPath();
      ctx.moveTo(o[0], o[1]);
      ctx.lineTo(tip[0], tip[1]);
      ctx.stroke();
      arrowHead(ctx, tip, u, 7);
    }

    // Entities.
    for (const e of Object.values(sk.entities)) {
      if (e.type === "point") continue;
      const pts = sampleEntity(sk, e, 96).map((p) => V.toScreen(p));
      ctx.strokeStyle = colorOf(e.id, an.entityState[e.id]);
      ctx.lineWidth = selected.has(e.id) || hoverId === e.id ? 2.5 : 1.5;
      ctx.setLineDash((e as { construction?: boolean }).construction ? [10, 3, 2, 3] : []);
      ctx.beginPath();
      pts.forEach((p, i) => (i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1])));
      ctx.stroke();
    }
    ctx.setLineDash([]);
    // Points (System Options > Sketch: display entity points / arc centerpoints).
    const showEntityPts = this.app.opt("sketch.entityPoints") !== false;
    const showCenters = this.app.opt("sketch.arcCenters") !== false;
    const centers = new Set<Id>();
    for (const e of Object.values(sk.entities)) if (e.type === "arc" || e.type === "circle") centers.add(e.c);
    for (const p of Object.values(sk.points)) {
      if (p.id === ORIGIN_ID) continue;
      const isSel = selected.has(p.id) || hoverId === p.id;
      if (p.virtual && !isSel) continue;
      if (!isSel && !p.standalone) {
        if (centers.has(p.id) ? !showCenters : !showEntityPts) continue;
      }
      const [x, y] = V.toScreen([p.x, p.y]);
      const col = colorOf(p.id, an.pointState[p.id]);
      ctx.fillStyle = col;
      const r = selected.has(p.id) || hoverId === p.id ? 4 : 2.6;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }

    if (this.hover?.kind === "mid") this.highlight(ctx, this.hover, C.hover);
    // Relations glyphs.
    if (this.app.options.sketch.showRelations) this.drawRelations(ctx, sk);

    // Dimensions.
    for (const d of Object.values(sk.dimensions)) this.drawDimension(ctx, d, selected.has(d.id) ? C.selected : hoverId === d.id ? C.hover : d.driven ? C.dimensionDriven : an.bad.has(d.id) ? C.sketchOver : C.dimension);

    // Tool preview, snaps, inference.
    this.drawToolPreview(ctx);

    // Box selection.
    const d = this.drag;
    if (d && d.kind === "box") drawBox(ctx, d);
  }

  /** Shaded Sketch Contours: every closed area is filled; nested areas read darker. */
  private drawRegions(ctx: CanvasRenderingContext2D, selected: Set<string>) {
    const faces = this.faces();
    if (!faces.length) return;
    const C = this.app.options.colors;
    const S = (p: Vec2) => this.view.toScreen(p);
    const path = (loops: Loop[]) => {
      ctx.beginPath();
      for (const l of loops) {
        l.poly.forEach((p, i) => {
          const [x, y] = S(p);
          if (i) ctx.lineTo(x, y);
          else ctx.moveTo(x, y);
        });
        ctx.closePath();
      }
    };
    ctx.save();
    ctx.globalAlpha = 0.5;
    ctx.fillStyle = this.app.opt<string>("color.sketchShaded") || "#b4c8ff";
    for (const f of faces) {
      path([f]);
      ctx.fill();
    }
    ctx.globalAlpha = 0.35;
    const hi = (f: Loop, color: string) => {
      ctx.fillStyle = color;
      path([f, ...this.regionHoles(f)]);
      ctx.fill("evenodd");
    };
    for (const f of faces) if (f.ents.every((id) => selected.has(id))) hi(f, C.selected);
    if (this.hoverRegion && !this.tool && faces.includes(this.hoverRegion)) hi(this.hoverRegion, C.hover);
    ctx.restore();
  }

  /** Read-only drawing of the sketch's dimensions (feature previews). */
  drawDimensions(ctx: CanvasRenderingContext2D) {
    const C = this.app.options.colors;
    for (const d of Object.values(this.sk.dimensions)) this.drawDimension(ctx, d, d.driven ? C.dimensionDriven : C.dimension);
  }

  /** Major/minor grid with automatic scaling, clipped to the visible area. */
  private drawGrid(ctx: CanvasRenderingContext2D) {
    const V = this.view;
    const px = V.pxSize();
    let major = Number(this.app.docProp("grid.major") ?? 100);
    const minorN = Math.max(1, Number(this.app.docProp("grid.minorPerMajor") ?? 4));
    if (this.app.docProp("grid.autoScale") !== false) {
      while (major / px > 400) major /= 2;
      while (major / px < 60) major *= 2;
    }
    const minor = major / minorN;
    const w = ctx.canvas.width / devicePixelRatio, h = ctx.canvas.height / devicePixelRatio;
    const corners = [[0, 0], [w, 0], [0, h], [w, h]].map(([x, y]) => V.toSketchPoint?.(x, y)).filter(Boolean) as Vec2[];
    if (corners.length < 4) return;
    const xs = corners.map((c) => c[0]), ys = corners.map((c) => c[1]);
    const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
    const C = this.app.options.colors;
    void C;
    ctx.lineWidth = 1;
    const dash = this.app.docProp("grid.dash") !== false;
    for (const [step, color] of [[minor, "#d6d9de"], [major, "#a9aeb6"]] as const) {
      if (step / px < 6) continue;
      ctx.strokeStyle = color;
      ctx.setLineDash(dash && step === minor ? [2, 3] : []);
      ctx.beginPath();
      for (let x = Math.ceil(x0 / step) * step; x <= x1; x += step) {
        const a = V.toScreen([x, y0]), b = V.toScreen([x, y1]);
        ctx.moveTo(a[0], a[1]);
        ctx.lineTo(b[0], b[1]);
      }
      for (let y = Math.ceil(y0 / step) * step; y <= y1; y += step) {
        const a = V.toScreen([x0, y]), b = V.toScreen([x1, y]);
        ctx.moveTo(a[0], a[1]);
        ctx.lineTo(b[0], b[1]);
      }
      ctx.stroke();
    }
    ctx.setLineDash([]);
  }

  private drawRelations(ctx: CanvasRenderingContext2D, sk: SketchData) {
    const V = this.view;
    const counts = new Map<Id, number>();
    const glyphs: Record<string, string> = {
      horizontal: "—",
      vertical: "|",
      parallel: "∥",
      perpendicular: "⊥",
      collinear: "⋯",
      equal: "=",
      tangent: "◠",
      concentric: "◎",
      coradial: "◉",
      midpoint: "⊿",
      symmetric: "⇹",
      offset: "⫽",
      onEdge: "⊡",
      fix: "⚓",
      intersection: "✕",
      horizontalPoints: "—",
      verticalPoints: "|",
    };
    ctx.font = "11px Segoe UI, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const r of Object.values(sk.relations)) {
      if (r.type === "coincident") continue;
      // A picked line midpoint is not a relation the user added.
      if (r.type === "midpoint" && sk.points[r.p]?.virtual) continue;
      const g = glyphs[r.type];
      if (!g) continue;
      for (const ref of relationRefs(r)) {
        const anchor = this.anchorOf(ref);
        if (!anchor) continue;
        const n = counts.get(ref) ?? 0;
        counts.set(ref, n + 1);
        const [x, y] = V.toScreen(anchor);
        const bx = x + 8 + n * 16, by = y + 10;
        ctx.fillStyle = this.app.sketch!.analysis.bad.has(r.id) ? "#ffb3b3" : "#c8f0c0";
        ctx.strokeStyle = "#3a7a30";
        ctx.lineWidth = 1;
        ctx.fillRect(bx - 7, by - 7, 14, 14);
        ctx.strokeRect(bx - 7, by - 7, 14, 14);
        ctx.fillStyle = "#1a3a10";
        ctx.fillText(g, bx, by + 0.5);
      }
    }
  }

  private anchorOf(id: Id): Vec2 | null {
    const sk = this.sk;
    if (sk.points[id]) return P(sk, id);
    const e = sk.entities[id];
    if (!e) return null;
    if (e.type === "line") return v2.lerp(P(sk, e.p1), P(sk, e.p2), 0.5);
    if (e.type === "arc") return arcGeom(sk, e).mid;
    if (e.type === "circle") return v2.add(P(sk, e.c), [e.r * 0.707, e.r * 0.707]);
    if (e.type === "spline") {
      const s = sampleEntity(sk, e, 32);
      return s[Math.floor(s.length / 2)];
    }
    return P(sk, e.p);
  }

  /** Screen bbox of a dimension's text: [x0, y0, x1, y1]. */
  dimLabelBox(d: Dimension): [number, number, number, number] | null {
    const [x, y] = this.view.toScreen(d.label);
    const t = this.dimText(d);
    const w = t.length * 7 + 6;
    return [x - w / 2, y - 9, x + w / 2, y + 9];
  }

  dimText(d: Dimension) {
    const u = this.app.doc.units;
    // Document Properties > Dimensions: precision, trailing zeros, parentheses on driven dims.
    const keep = this.app.docProp<string>("dim.zeros") === "show";
    const lin = Number(this.app.docProp("dim.precision") ?? 2);
    const ang = Number(this.app.docProp("dim.anglePrecision") ?? 2);
    let t = d.kind === "angle" ? `${keep ? ((d.value * 180) / Math.PI).toFixed(ang) : +((d.value * 180) / Math.PI).toFixed(ang)}°` : formatLength(d.value, u, false, lin, keep);
    if (d.kind === "radius") t = "R" + t;
    if (d.kind === "diameter") t = "Ø" + t;
    // Dimensions driven by equations carry a Σ, as in SOLIDWORKS.
    if (this.app.doc.equations?.some((e) => e.target === this.fullName(d))) t = "Σ" + t;
    return d.driven && this.app.docProp("dim.addParens") !== false ? `(${t})` : t;
  }

  private drawDimension(ctx: CanvasRenderingContext2D, d: Dimension, color: string) {
    const sk = this.sk;
    const V = this.view;
    const L = d.label;
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.lineWidth = 1;
    const S = (p: Vec2) => V.toScreen(p) as Vec2;
    const line = (a: Vec2, b: Vec2) => {
      ctx.beginPath();
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
      ctx.stroke();
    };
    const linear = (a: Vec2, b: Vec2, dir: Vec2) => {
      // dimension line parallel to dir through L, extension lines from a and b.
      const n = v2.perp(dir);
      const off = v2.dot(v2.sub(L, a), n);
      const da = v2.add(a, v2.scale(n, off));
      const db = v2.add(b, v2.scale(n, v2.dot(v2.sub(L, b), n)));
      const ext = 4 * V.pxSize();
      const sgn = (x: number) => (x >= 0 ? 1 : -1);
      line(S(a), S(v2.add(da, v2.scale(n, sgn(off) * ext))));
      line(S(b), S(v2.add(db, v2.scale(n, sgn(v2.dot(v2.sub(L, b), n)) * ext))));
      const A = S(da), B = S(db);
      line(A, B);
      // extend to text if outside
      const t = S(L);
      const u = v2.norm(v2.sub(B, A));
      const along = v2.dot(v2.sub(t, A), u);
      const len = v2.dist(A, B);
      if (along < 0) line(A, t);
      if (along > len) line(B, t);
      arrowHead(ctx, A, v2.scale(u, -1), 8);
      arrowHead(ctx, B, u, 8);
    };
    switch (d.kind) {
      case "distance": {
        const a = P(sk, d.refs[0]), b = P(sk, d.refs[1]);
        linear(a, b, v2.norm(v2.sub(b, a)));
        break;
      }
      case "horizontal":
      case "vertical": {
        const a = P(sk, d.refs[0]), b = P(sk, d.refs[1]);
        linear(a, b, d.kind === "horizontal" ? [1, 0] : [0, 1]);
        break;
      }
      case "pointLine":
      case "lineLine": {
        let a: Vec2, l: SketchEntity | undefined;
        if (d.kind === "pointLine") {
          a = P(sk, d.refs[0]);
          l = sk.entities[d.refs[1]];
        } else {
          const l2 = sk.entities[d.refs[1]];
          if (!l2 || l2.type !== "line") return;
          a = v2.lerp(P(sk, l2.p1), P(sk, l2.p2), 0.5);
          l = sk.entities[d.refs[0]];
        }
        if (!l || l.type !== "line") return;
        const p = P(sk, l.p1), q = P(sk, l.p2);
        const u = v2.norm(v2.sub(q, p));
        const foot = v2.add(p, v2.scale(u, v2.dot(v2.sub(a, p), u)));
        if (v2.dist(foot, a) < 1e-9) return;
        linear(a, foot, v2.norm(v2.sub(foot, a)));
        break;
      }
      case "angle": {
        const e1 = sk.entities[d.refs[0]], e2 = sk.entities[d.refs[1]];
        if (!e1 || !e2 || e1.type !== "line" || e2.type !== "line") return;
        const x = lineIntersection(P(sk, e1.p1), P(sk, e1.p2), P(sk, e2.p1), P(sk, e2.p2));
        if (!x) return;
        const q = d.quadrant ?? 0;
        let u = v2.norm(v2.sub(P(sk, e1.p2), P(sk, e1.p1)));
        let w = v2.norm(v2.sub(P(sk, e2.p2), P(sk, e2.p1)));
        if (q & 1) u = v2.scale(u, -1);
        if (q & 2) w = v2.scale(w, -1);
        const X = S(x);
        const r = v2.dist(S(L), X);
        // screen-space angles (y down)
        const su = v2.norm(v2.sub(S(v2.add(x, u)), X)), sw = v2.norm(v2.sub(S(v2.add(x, w)), X));
        let a0 = Math.atan2(su[1], su[0]), a1 = Math.atan2(sw[1], sw[0]);
        let delta = a1 - a0;
        while (delta > Math.PI) delta -= 2 * Math.PI;
        while (delta < -Math.PI) delta += 2 * Math.PI;
        ctx.beginPath();
        ctx.arc(X[0], X[1], r, a0, a0 + delta, delta < 0);
        ctx.stroke();
        a1 = a0 + delta;
        const tipA: Vec2 = [X[0] + Math.cos(a0) * r, X[1] + Math.sin(a0) * r];
        const tipB: Vec2 = [X[0] + Math.cos(a1) * r, X[1] + Math.sin(a1) * r];
        const s = Math.sign(delta) || 1;
        arrowHead(ctx, tipA, [Math.sin(a0) * s, -Math.cos(a0) * s], 8);
        arrowHead(ctx, tipB, [-Math.sin(a1) * s, Math.cos(a1) * s], 8);
        // extension along lines if needed
        line(X, tipA);
        line(X, tipB);
        void a0;
        break;
      }
      case "offset": {
        // Arc/circle offset: a short radial dimension between the two curves, towards the text.
        const ea = sk.entities[d.refs[0]], eb = sk.entities[d.refs[1]];
        if (!ea || !eb || (ea.type !== "circle" && ea.type !== "arc") || (eb.type !== "circle" && eb.type !== "arc")) return;
        const c = P(sk, ea.c);
        const ra = ea.type === "circle" ? ea.r : arcGeom(sk, ea).r, rb = eb.type === "circle" ? eb.r : arcGeom(sk, eb).r;
        const u = v2.norm(v2.sub(L, c));
        const A = S(v2.add(c, v2.scale(u, ra))), B = S(v2.add(c, v2.scale(u, rb))), T = S(L);
        line(A, B);
        const far = v2.dist(T, S(c)) > Math.max(v2.dist(A, S(c)), v2.dist(B, S(c))) ? (rb > ra ? B : A) : null;
        if (far) line(far, T);
        const k = v2.norm(v2.sub(B, A));
        arrowHead(ctx, A, v2.scale(k, -1), 8);
        arrowHead(ctx, B, k, 8);
        break;
      }
      case "radius":
      case "diameter": {
        const e = sk.entities[d.refs[0]];
        if (!e || (e.type !== "circle" && e.type !== "arc")) return;
        const c = P(sk, e.c);
        const r = e.type === "circle" ? e.r : arcGeom(sk, e).r;
        if (d.kind === "diameter" && e.type === "circle") {
          // Text dragged beside or above/below the circle: linear diameter with extension
          // lines from the tangent points (the tidy outside style). Elsewhere: radial.
          const dx = Math.abs(L[0] - c[0]), dy = Math.abs(L[1] - c[1]);
          if (dx > r && dy <= r) {
            linear([c[0], c[1] + r], [c[0], c[1] - r], [0, 1]);
            break;
          }
          if (dy > r && dx <= r) {
            linear([c[0] - r, c[1]], [c[0] + r, c[1]], [1, 0]);
            break;
          }
        }
        const u = v2.norm(v2.sub(L, c));
        const edge = v2.add(c, v2.scale(u, r));
        const C2 = S(c), E = S(edge), T = S(L);
        if (d.kind === "diameter") {
          const opp = S(v2.sub(c, v2.scale(u, r)));
          line(opp, v2.dist(T, C2) > v2.dist(E, C2) ? T : E);
          arrowHead(ctx, E, v2.norm(v2.sub(E, C2)), 8);
          arrowHead(ctx, opp, v2.norm(v2.sub(opp, C2)), 8);
        } else {
          line(C2, v2.dist(T, C2) > v2.dist(E, C2) ? T : E);
          arrowHead(ctx, E, v2.norm(v2.sub(E, C2)), 8);
        }
        break;
      }
    }
    // Text box.
    const b = this.dimLabelBox(d)!;
    ctx.fillStyle = "rgba(255,255,255,0.85)";
    ctx.fillRect(b[0], b[1], b[2] - b[0], b[3] - b[1]);
    ctx.fillStyle = color;
    ctx.font = "13px 'Century Gothic', 'Segoe UI', sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(this.dimText(d), (b[0] + b[2]) / 2, (b[1] + b[3]) / 2 + 1);
  }

  private drawToolPreview(ctx: CanvasRenderingContext2D) {
    const tool = this.tool;
    const V = this.view;
    const C = this.app.options.colors;
    const S = (p: Vec2) => V.toScreen(p) as Vec2;
    const st = this.st;
    const snap = this.snap;
    const [mx, my] = this.rawScreen;
    if (!tool) return;
    // Hover highlight for dimension/trim tools.
    if ((DIM_TOOLS.has(tool) || tool === "trim") && this.hover) {
      this.highlight(ctx, this.hover, tool === "trim" ? "#ff4040" : C.hover);
    }
    if (DIM_TOOLS.has(tool)) {
      for (const r of st.dimRefs ?? []) this.highlight(ctx, r, C.selected);
      if (st.dimRefs?.length && this.raw) {
        const prev = this.buildDimension(st.dimRefs, this.raw);
        if (prev) this.drawDimension(ctx, prev, C.selected);
      }
      drawCursorBadge(ctx, mx, my, "◇", [], "");
      return;
    }
    if (tool === "trim") {
      drawCursorBadge(ctx, mx, my, "✂", [], "");
      return;
    }
    if (tool === "sketchFillet" || tool === "sketchChamfer") {
      if (this.hover) this.highlight(ctx, this.hover, C.hover);
      if (st.filletFirst) this.highlight(ctx, { kind: "entity", id: st.filletFirst }, C.selected);
      drawCursorBadge(ctx, mx, my, tool === "sketchChamfer" ? "◸" : "◜", [], "");
      return;
    }
    if (tool === "extend") {
      if (this.hover) this.highlight(ctx, this.hover, C.hover);
      drawCursorBadge(ctx, mx, my, "⇥", [], "");
      return;
    }
    if (OP_TOOLS.has(tool)) {
      if (this.hover) this.highlight(ctx, this.hover, C.hover);
      for (const id of st.opSel ?? []) this.highlight(ctx, { kind: "entity", id }, C.selected);
      if (st.opAxis) this.highlight(ctx, { kind: "entity", id: st.opAxis }, "#d000d0");
      for (const p of st.opPts ?? []) {
        const [x, y] = S(p);
        ctx.fillStyle = "#d000d0";
        ctx.beginPath();
        ctx.arc(x, y, 4, 0, Math.PI * 2);
        ctx.fill();
      }
      const pv = this.opPreview();
      if (pv && !pv.error) {
        ctx.strokeStyle = "#d4a800";
        ctx.lineWidth = 1.6;
        for (const id of pv.created) {
          const e = pv.sk.entities[id];
          if (!e || e.type === "point") continue;
          polyline(ctx, sampleEntity(pv.sk, e, 64).map((p) => S(p)));
        }
      }
      if ((st.opField === "point1" || st.opField === "point2") && snap) {
        const sp = S(snap.point);
        ctx.strokeStyle = C.hover;
        ctx.lineWidth = 1.5;
        ctx.strokeRect(sp[0] - 4, sp[1] - 4, 8, 8);
      }
      drawCursorBadge(ctx, mx, my, "", [], "");
      return;
    }
    if (!snap) return;
    const p = snap.point;
    // Inference lines.
    for (const g of snap.guides) {
      ctx.strokeStyle = g.relation ? C.inferenceRelation : C.inferenceGuide;
      ctx.setLineDash([3, 3]);
      ctx.lineWidth = 1;
      const a = S(g.from), b = S(g.to);
      ctx.beginPath();
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(b[0], b[1]);
      ctx.stroke();
    }
    ctx.setLineDash([]);
    // Snap target highlight.
    if (snap.kind !== "free" && snap.kind !== "grid") {
      const sp = S(p);
      ctx.strokeStyle = C.hover;
      ctx.fillStyle = C.hover;
      ctx.lineWidth = 2;
      if (snap.entityId && snap.kind === "onEntity") this.highlight(ctx, { kind: "entity", id: snap.entityId }, C.hover);
      ctx.beginPath();
      ctx.arc(sp[0], sp[1], snap.kind === "point" ? 5 : 4, 0, Math.PI * 2);
      if (snap.kind === "midpoint") ctx.fill();
      else ctx.stroke();
    }
    // Rubber-band preview.
    ctx.strokeStyle = C.sketchUnder;
    ctx.lineWidth = 1.5;
    const construction = tool === "centerline" || (this.app.toolOpts.construction && !!SHAPES[tool]);
    ctx.setLineDash(construction ? [10, 3, 2, 3] : []);
    let info = "";
    const u = this.app.doc.units;
    if ((tool === "line" || tool === "centerline") && st.pts.length) {
      const a = st.pts[0].p;
      if (st.arcMode && st.prevEnt) {
        const g = this.tangentArcGeom(st.prevEnt, a, st.pts[0].pid, p);
        if (g) {
          const pts = arcPoints(g.c, a, p, g.ccw);
          polyline(ctx, pts.map(S));
          info = `R=${formatLength(v2.dist(g.c, a), u)}`;
        }
      } else {
        polyline(ctx, [S(a), S(p)]);
        const ang = (Math.atan2(p[1] - a[1], p[0] - a[0]) * 180) / Math.PI;
        info = `L=${formatLength(v2.dist(a, p), u)}  A=${ang.toFixed(1)}°`;
      }
    } else if ((tool === "rectangle" || tool === "centerRectangle") && st.pts.length) {
      const a = st.pts[0].p;
      let q1: Vec2, q3: Vec2;
      if (tool === "rectangle") {
        q1 = a;
        q3 = p;
      } else {
        q1 = [2 * a[0] - p[0], 2 * a[1] - p[1]];
        q3 = p;
      }
      const corners: Vec2[] = [q1, [q3[0], q1[1]], q3, [q1[0], q3[1]], q1];
      polyline(ctx, corners.map(S));
      info = `${formatLength(Math.abs(q3[0] - q1[0]), u)} x ${formatLength(Math.abs(q3[1] - q1[1]), u)}`;
    } else if (tool === "circle" && st.pts.length) {
      const c = st.pts[0].p;
      const r = v2.dist(c, p);
      const pts: Vec2[] = [];
      for (let i = 0; i <= 96; i++) pts.push(v2.add(c, v2.fromAngle((i / 96) * Math.PI * 2, r)));
      polyline(ctx, pts.map(S));
      info = `R=${formatLength(r, u)}`;
    } else if (tool === "arc" && st.pts.length) {
      const c = st.pts[0].p;
      if (st.pts.length === 1) {
        ctx.setLineDash([4, 4]);
        polyline(ctx, [S(c), S(p)]);
        info = `R=${formatLength(v2.dist(c, p), u)}`;
      } else {
        const g = this.centerArcPreview();
        if (g) {
          const n = 64, pts: Vec2[] = [];
          for (let i = 0; i <= n; i++) pts.push(v2.add(g.c, v2.fromAngle(g.a0 + (g.sweep * i) / n, g.r)));
          polyline(ctx, pts.map(S));
          info = `A=${((Math.abs(g.sweep) * 180) / Math.PI).toFixed(1)}°  R=${formatLength(g.r, u)}`;
        }
      }
    } else if (tool === "threePointArc" && st.pts.length) {
      if (st.pts.length === 1) polyline(ctx, [S(st.pts[0].p), S(p)]);
      else {
        const [A, B] = st.pts;
        const c = circumcenter(A.p, p, B.p);
        if (c) {
          const ccw = v2.cross(v2.sub(B.p, A.p), v2.sub(p, A.p)) < 0;
          polyline(ctx, arcPoints(c, A.p, B.p, ccw).map(S));
          info = `R=${formatLength(v2.dist(c, A.p), u)}`;
        }
      }
    } else if (tool === "tangentArc" && st.pts.length && st.prevEnt) {
      const g = this.tangentArcGeom(st.prevEnt, st.pts[0].p, st.pts[0].pid, p);
      if (g) polyline(ctx, arcPoints(g.c, st.pts[0].p, p, g.ccw).map(S));
    } else if (tool === "spline" && st.pts.length) {
      polyline(ctx, splineThrough([...st.pts.map((x) => x.p), p], 16).map((q) => S(q)));
      ctx.fillStyle = C.sketchUnder;
      for (const x of st.pts) {
        const [sx2, sy2] = S(x.p);
        ctx.fillRect(sx2 - 2.5, sy2 - 2.5, 5, 5);
      }
    } else if (SHAPES[tool] && st.pts.length) {
      const pv = SHAPES[tool].preview([...st.pts.map((x) => x.p), p], this.shapeOpts());
      for (const pl of pv.solid) polyline(ctx, pl.map(S));
      ctx.setLineDash([10, 3, 2, 3]);
      ctx.lineWidth = 1;
      for (const pl of pv.construction) polyline(ctx, pl.map(S));
      info = pv.info ?? "";
    }
    ctx.setLineDash([]);
    const glyph: Record<string, string> = {
      line: "╱",
      centerline: "┄",
      rectangle: "▭",
      centerRectangle: "▣",
      circle: "◯",
      arc: "◜",
      threePointArc: "⌒",
      tangentArc: "◠",
      point: "•",
      midpointLine: "╱",
      threePointRectangle: "◇",
      threePointCenterRectangle: "◈",
      parallelogram: "▱",
      perimeterCircle: "◯",
      straightSlot: "⬭",
      centerSlot: "⬭",
      polygon: "⬡",
    };
    drawCursorBadge(ctx, mx, my, st.arcMode ? "◠" : glyph[tool] ?? "", snap.badges, info);
  }

  private highlight(ctx: CanvasRenderingContext2D, h: Hit, color: string) {
    const V = this.view;
    if (h.kind === "mid") {
      const l = this.sk.entities[h.id];
      if (l?.type !== "line") return;
      ctx.strokeStyle = color;
      ctx.lineWidth = 2.5;
      polyline(ctx, [V.toScreen(P(this.sk, l.p1)) as Vec2, V.toScreen(P(this.sk, l.p2)) as Vec2]);
      const [x, y] = V.toScreen(v2.lerp(P(this.sk, l.p1), P(this.sk, l.p2), 0.5));
      ctx.fillStyle = color;
      ctx.fillRect(x - 4, y - 4, 8, 8);
      return;
    }
    if (h.kind === "point") {
      const p = this.sk.points[h.id];
      if (!p) return;
      const [x, y] = V.toScreen([p.x, p.y]);
      ctx.fillStyle = color;
      ctx.beginPath();
      ctx.arc(x, y, 4.5, 0, Math.PI * 2);
      ctx.fill();
      return;
    }
    if (h.kind === "entity") {
      const e = this.sk.entities[h.id];
      if (!e) return;
      ctx.strokeStyle = color;
      ctx.lineWidth = 3;
      polyline(ctx, sampleEntity(this.sk, e, 96).map((p) => V.toScreen(p) as Vec2));
    }
  }
}

// ---------------------------------------------------------------- drawing utils

function centroid(poly: Vec2[]): Vec2 {
  let x = 0, y = 0;
  for (const p of poly) {
    x += p[0];
    y += p[1];
  }
  return [x / poly.length, y / poly.length];
}

function polyline(ctx: CanvasRenderingContext2D, pts: Vec2[]) {
  ctx.beginPath();
  pts.forEach((p, i) => (i ? ctx.lineTo(p[0], p[1]) : ctx.moveTo(p[0], p[1])));
  ctx.stroke();
}

function arcPoints(c: Vec2, a: Vec2, b: Vec2, ccw: boolean): Vec2[] {
  const r = v2.dist(c, a);
  const a0 = v2.angle(v2.sub(a, c));
  const a1 = v2.angle(v2.sub(b, c));
  const sw = ccw ? wrap2Pi(a1 - a0) : -wrap2Pi(a0 - a1);
  const out: Vec2[] = [];
  for (let i = 0; i <= 64; i++) out.push(v2.add(c, v2.fromAngle(a0 + (sw * i) / 64, r)));
  return out;
}

export function arrowHead(ctx: CanvasRenderingContext2D, tip: Vec2 | [number, number], dir: Vec2, size: number) {
  const n = v2.perp(dir);
  const b = v2.sub(tip as Vec2, v2.scale(dir, size));
  ctx.beginPath();
  ctx.moveTo(tip[0], tip[1]);
  ctx.lineTo(b[0] + n[0] * size * 0.3, b[1] + n[1] * size * 0.3);
  ctx.lineTo(b[0] - n[0] * size * 0.3, b[1] - n[1] * size * 0.3);
  ctx.closePath();
  ctx.fill();
}

const BADGE_GLYPH: Record<string, string> = {
  coincident: "◉",
  midpoint: "⊿",
  intersection: "✕",
  horizontal: "—",
  vertical: "|",
  perpendicular: "⊥",
  parallel: "∥",
  tangent: "◠",
};

function drawCursorBadge(ctx: CanvasRenderingContext2D, x: number, y: number, toolGlyph: string, badges: string[], info: string) {
  ctx.font = "12px Segoe UI, sans-serif";
  ctx.textBaseline = "middle";
  ctx.textAlign = "center";
  // Pencil + tool glyph.
  ctx.fillStyle = "#202020";
  ctx.fillText("✎", x + 12, y + 12);
  if (toolGlyph) ctx.fillText(toolGlyph, x + 24, y + 20);
  // Relation badges (yellow boxes).
  badges.forEach((b, i) => {
    const bx = x + 16 + i * 17, by = y + 34;
    ctx.fillStyle = "#ffe46b";
    ctx.strokeStyle = "#a08000";
    ctx.lineWidth = 1;
    ctx.fillRect(bx - 7, by - 7, 15, 15);
    ctx.strokeRect(bx - 7, by - 7, 15, 15);
    ctx.fillStyle = "#3a2e00";
    ctx.fillText(BADGE_GLYPH[b] ?? "?", bx + 0.5, by + 1);
  });
  if (info) {
    ctx.textAlign = "left";
    ctx.font = "12px 'Century Gothic', 'Segoe UI', sans-serif";
    const w = ctx.measureText(info).width;
    ctx.fillStyle = "rgba(255,255,255,0.8)";
    ctx.fillRect(x + 14, y - 22, w + 8, 16);
    ctx.fillStyle = "#003c9e";
    ctx.fillText(info, x + 18, y - 13);
  }
}

export function drawBox(ctx: CanvasRenderingContext2D, d: { x0: number; y0: number; x1: number; y1: number }) {
  const crossing = d.x1 < d.x0;
  ctx.strokeStyle = crossing ? "#1a7a1a" : "#1a3fbf";
  ctx.fillStyle = crossing ? "rgba(60,180,60,0.08)" : "rgba(60,100,230,0.08)";
  ctx.setLineDash(crossing ? [5, 3] : []);
  ctx.lineWidth = 1;
  const x = Math.min(d.x0, d.x1), y = Math.min(d.y0, d.y1);
  ctx.fillRect(x, y, Math.abs(d.x1 - d.x0), Math.abs(d.y1 - d.y0));
  ctx.strokeRect(x, y, Math.abs(d.x1 - d.x0), Math.abs(d.y1 - d.y0));
  ctx.setLineDash([]);
}

function segmentsCrossRect(pts: [number, number][], xa: number, ya: number, xb: number, yb: number) {
  const edges: [Vec2, Vec2][] = [
    [[xa, ya], [xb, ya]],
    [[xb, ya], [xb, yb]],
    [[xb, yb], [xa, yb]],
    [[xa, yb], [xa, ya]],
  ];
  for (let i = 1; i < pts.length; i++)
    for (const [c, d] of edges) if (segIntersect(pts[i - 1], pts[i], c, d)) return true;
  return false;
}

function segIntersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) {
  const d1 = v2.cross(v2.sub(b, a), v2.sub(c, a)), d2 = v2.cross(v2.sub(b, a), v2.sub(d, a));
  const d3 = v2.cross(v2.sub(d, c), v2.sub(a, c)), d4 = v2.cross(v2.sub(d, c), v2.sub(b, c));
  return d1 * d2 < 0 && d3 * d4 < 0;
}

function lineIntersection(a: Vec2, b: Vec2, c: Vec2, d: Vec2): Vec2 | null {
  const r = v2.sub(b, a), s = v2.sub(d, c);
  const den = v2.cross(r, s);
  if (Math.abs(den) < 1e-12) return null;
  const t = v2.cross(v2.sub(c, a), s) / den;
  return v2.add(a, v2.scale(r, t));
}

export { angleInArc };
