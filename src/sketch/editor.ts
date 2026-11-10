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
import { P, arcGeom, circumcenter, closestOnEntity, deleteFromSketch, entitiesUsingPoint, pruneRefs, sampleEntity } from "./model";
import { ORIGIN_ID, SketchSystem, entityPoints, relationRefs, solveSketch } from "./solver";

export interface SketchViewApi {
  toSketch(sx: number, sy: number): Vec2 | null;
  toScreen(p: Vec2): [number, number];
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

type Hit = { kind: "point"; id: Id } | { kind: "entity"; id: Id } | { kind: "dim"; id: Id };

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
  "trim",
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
}

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

  constructor(private app: App, private view: SketchViewApi) {}

  get sk(): SketchData {
    return this.app.sketch!.data;
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
    this.app.tool = t;
    this.resetTool();
    if (t !== null) this.app.selection = [];
    this.app.pm = t ? { kind: "sketchTool", tool: t } : null;
    this.app.changed();
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
      this.app.requestRender();
      return;
    }
    if (tool === "smartDimension") {
      this.hover = this.hitTest(e.sx, e.sy, false);
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
    if (tool === "smartDimension") return this.dimClick(e, raw);
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
    if (tool && tool !== "smartDimension" && this.st.moved && this.st.pts.length >= 1) {
      this.st.moved = false;
      this.st.downAt = undefined;
      this.toolClick(true);
    }
    this.st.downAt = undefined;
  }

  onDblClick(e: PointerInfo) {
    const tool = this.tool;
    if (tool === "line" || tool === "centerline") {
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
    return false;
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
    const anchor = this.st.pts.length && (tool === "line" || tool === "centerline") ? this.st.pts[this.st.pts.length - 1].p : undefined;
    const exclude = new Set<Id>();
    return infer(this.sk, raw, {
      anchor: this.st.arcMode ? undefined : anchor,
      prevLine: this.st.arcMode ? undefined : this.st.prevLine,
      tol: this.tol,
      woken: this.woken,
      exclude,
      ctrl: this.ctrl,
      options: this.app.options.sketch,
      hvFromAnchor: tool === "line" || tool === "centerline",
    });
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
    switch (tool) {
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
        if (tool === "rectangle") {
          const p1 = this.pointFor(a.snap);
          const p3 = this.pointFor(s);
          const q1 = P(this.sk, p1), q3 = P(this.sk, p3);
          const p2 = this.newPoint([q3[0], q1[1]]);
          const p4 = this.newPoint([q1[0], q3[1]]);
          this.rectLines(p1, p2, p3, p4);
        } else {
          const c = a.p, k = s.point;
          const dx = k[0] - c[0], dy = k[1] - c[1];
          const pts = [
            this.newPoint([c[0] - dx, c[1] - dy]),
            this.newPoint([c[0] + dx, c[1] - dy]),
            this.pointFor(s),
            this.newPoint([c[0] - dx, c[1] + dy]),
          ];
          this.rectLines(pts[0], pts[1], pts[2], pts[3]);
          const d1 = this.addEntity({ id: uid("e"), type: "line", p1: pts[0], p2: pts[2], construction: true });
          this.addEntity({ id: uid("e"), type: "line", p1: pts[1], p2: pts[3], construction: true });
          const cp = this.pointFor(a.snap);
          this.addRel({ type: "midpoint", p: cp, e: d1 });
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

  private selectDown(e: PointerInfo, raw: Vec2) {
    const h = this.hitTest(e.sx, e.sy);
    const mode = e.ctrl ? "toggle" : e.shift ? "add" : "replace";
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
    const h = this.hitTest(e.sx, e.sy, true);
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
    this.app.sketchCheckpoint();
    this.sk.dimensions[dim.id] = dim;
    this.sk.nextDim++;
    const a = solveSketch(this.sk);
    if (a.bad.has(dim.id)) {
      dim.driven = true;
      this.app.flash("This dimension would over define the sketch; it was added as a driven (reference) dimension.", "warning");
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
    if (!horizontalLine && !verticalLine) {
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

    // Origin arrows (sketch origin is red).
    const o = V.toScreen([0, 0]);
    const ox = V.toScreen([1, 0]), oy = V.toScreen([0, 1]);
    const ux = v2.norm(v2.sub(ox as Vec2, o as Vec2)), uy = v2.norm(v2.sub(oy as Vec2, o as Vec2));
    ctx.strokeStyle = "#d40000";
    ctx.fillStyle = "#d40000";
    ctx.lineWidth = 1.5;
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
    // Points.
    for (const p of Object.values(sk.points)) {
      if (p.id === ORIGIN_ID) continue;
      const [x, y] = V.toScreen([p.x, p.y]);
      const col = colorOf(p.id, an.pointState[p.id]);
      ctx.fillStyle = col;
      const r = selected.has(p.id) || hoverId === p.id ? 4 : 2.6;
      ctx.beginPath();
      ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
    }

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
    let t = d.kind === "angle" ? formatAngle(d.value) : formatLength(d.value, u);
    if (d.kind === "radius") t = "R" + t;
    if (d.kind === "diameter") t = "Ø" + t;
    return d.driven ? `(${t})` : t;
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
      case "radius":
      case "diameter": {
        const e = sk.entities[d.refs[0]];
        if (!e || (e.type !== "circle" && e.type !== "arc")) return;
        const c = P(sk, e.c);
        const r = e.type === "circle" ? e.r : arcGeom(sk, e).r;
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
    if ((tool === "smartDimension" || tool === "trim") && this.hover) {
      this.highlight(ctx, this.hover, tool === "trim" ? "#ff4040" : C.hover);
    }
    if (tool === "smartDimension") {
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
    const construction = tool === "centerline";
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
    };
    drawCursorBadge(ctx, mx, my, st.arcMode ? "◠" : glyph[tool] ?? "", snap.badges, info);
  }

  private highlight(ctx: CanvasRenderingContext2D, h: Hit, color: string) {
    const V = this.view;
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
