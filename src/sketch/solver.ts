/**
 * Geometric constraint solver for 2D sketches.
 *
 * - Minimum-norm damped Gauss-Newton: under-defined geometry moves as little as
 *   possible when constraints change (the "nothing jumps" behaviour users expect).
 * - Constrained dragging: the drag vector is projected onto the constraint manifold
 *   each step, so a dragged point follows the cursor as far as relations allow
 *   (e.g. an endpoint of a fixed-length line swings around).
 * - Rank analysis (row-reduced Jacobian) gives the degrees of freedom, which params
 *   are fully determined (black vs blue colouring) and which constraints are
 *   redundant or conflicting (over defined).
 */
import type { Dimension, Id, Relation, SketchData, SketchEntity, Vec2 } from "../core/types";
import { wrapAngle } from "../core/math";

export type DefState = "under" | "full" | "over";

export interface Analysis {
  dof: number;
  status: "under" | "full" | "over" | "unsolved";
  pointState: Record<Id, DefState>;
  entityState: Record<Id, DefState>;
  /** Relations/dimensions that are redundant or conflicting. */
  bad: Set<Id>;
}

interface PRef {
  ix: number; // -1 if fixed
  iy: number;
  cx: number;
  cy: number;
}

interface Eq {
  owner: Id; // relation/dimension id, or "internal"
  deps: number[];
  f: () => number;
}

export const ORIGIN_ID = "origin";

export class SketchSystem {
  x: number[] = [];
  pts = new Map<Id, PRef>();
  radii = new Map<Id, { i: number; c: number }>();
  eqs: Eq[] = [];
  private paramOwner: { kind: "pt" | "r"; id: Id; axis?: 0 | 1 }[] = [];

  constructor(public sk: SketchData, extraFixed: Set<Id> = new Set()) {
    const fixed = new Set<Id>(extraFixed);
    fixed.add(ORIGIN_ID);
    const fixedRadius = new Set<Id>();
    for (const r of Object.values(sk.relations)) {
      if (r.type !== "fix") continue;
      const e = sk.entities[r.target];
      if (sk.points[r.target]) fixed.add(r.target);
      else if (e) {
        for (const p of entityPoints(e)) fixed.add(p);
        if (e.type === "circle") fixedRadius.add(e.id);
      }
    }
    for (const p of Object.values(sk.points)) {
      if (fixed.has(p.id)) {
        this.pts.set(p.id, { ix: -1, iy: -1, cx: p.x, cy: p.y });
      } else {
        const ix = this.x.push(p.x) - 1;
        const iy = this.x.push(p.y) - 1;
        this.paramOwner.push({ kind: "pt", id: p.id, axis: 0 }, { kind: "pt", id: p.id, axis: 1 });
        this.pts.set(p.id, { ix, iy, cx: 0, cy: 0 });
      }
    }
    for (const e of Object.values(sk.entities)) {
      if (e.type !== "circle") continue;
      if (fixedRadius.has(e.id)) this.radii.set(e.id, { i: -1, c: e.r });
      else {
        const i = this.x.push(e.r) - 1;
        this.paramOwner.push({ kind: "r", id: e.id });
        this.radii.set(e.id, { i, c: 0 });
      }
    }
    this.buildEquations();
  }

  // ------------------------------------------------------------ accessors
  private px = (id: Id) => {
    const r = this.pts.get(id)!;
    return r.ix >= 0 ? this.x[r.ix] : r.cx;
  };
  private py = (id: Id) => {
    const r = this.pts.get(id)!;
    return r.iy >= 0 ? this.x[r.iy] : r.cy;
  };
  private P = (id: Id): Vec2 => [this.px(id), this.py(id)];
  private pdeps(id: Id) {
    const r = this.pts.get(id);
    if (!r) return [];
    return r.ix >= 0 ? [r.ix, r.iy] : [];
  }
  private rad(e: SketchEntity): number {
    if (e.type === "circle") {
      const r = this.radii.get(e.id)!;
      return r.i >= 0 ? this.x[r.i] : r.c;
    }
    if (e.type === "arc") {
      const c = this.P(e.c), p = this.P(e.p1);
      return Math.hypot(p[0] - c[0], p[1] - c[1]);
    }
    return 0;
  }
  private edeps(e: SketchEntity): number[] {
    const d = entityPoints(e).flatMap((p) => this.pdeps(p));
    if (e.type === "circle") {
      const r = this.radii.get(e.id)!;
      if (r.i >= 0) d.push(r.i);
    }
    return d;
  }
  private center(e: SketchEntity): Id | null {
    return e.type === "circle" || e.type === "arc" ? e.c : null;
  }

  private add(owner: Id, deps: number[], f: () => number) {
    this.eqs.push({ owner, deps: [...new Set(deps)], f });
  }

  // ------------------------------------------------------------ equations
  private pointOnEntity(owner: Id, p: Id, e: SketchEntity) {
    const deps = [...this.pdeps(p), ...this.edeps(e)];
    if (e.type === "line") {
      this.add(owner, deps, () => distToLine(this.P(p), this.P(e.p1), this.P(e.p2)));
    } else if (e.type === "circle" || e.type === "arc") {
      this.add(owner, deps, () => {
        const c = this.P(e.c), q = this.P(p);
        return Math.hypot(q[0] - c[0], q[1] - c[1]) - this.rad(e);
      });
    } else if (e.type === "point") {
      this.add(owner, deps, () => this.px(p) - this.px(e.p));
      this.add(owner, deps, () => this.py(p) - this.py(e.p));
    }
  }

  private buildEquations() {
    const sk = this.sk;
    // Internal: arc endpoints equidistant from the centre.
    for (const e of Object.values(sk.entities)) {
      if (e.type === "arc") {
        this.add("internal", this.edeps(e), () => {
          const c = this.P(e.c), a = this.P(e.p1), b = this.P(e.p2);
          return Math.hypot(a[0] - c[0], a[1] - c[1]) - Math.hypot(b[0] - c[0], b[1] - c[1]);
        });
      }
    }
    for (const r of Object.values(sk.relations)) this.relation(r);
    for (const d of Object.values(sk.dimensions)) if (!d.driven) this.dimension(d);
  }

  private relation(r: Relation) {
    const sk = this.sk;
    const E = (id: Id) => sk.entities[id];
    const id = r.id;
    switch (r.type) {
      case "horizontal":
      case "vertical": {
        const e = E(r.e);
        if (!e || e.type !== "line") return;
        const k = r.type === "horizontal" ? this.py : this.px;
        this.add(id, this.edeps(e), () => k(e.p1) - k(e.p2));
        return;
      }
      case "horizontalPoints":
        this.add(id, [...this.pdeps(r.p1), ...this.pdeps(r.p2)], () => this.py(r.p1) - this.py(r.p2));
        return;
      case "verticalPoints":
        this.add(id, [...this.pdeps(r.p1), ...this.pdeps(r.p2)], () => this.px(r.p1) - this.px(r.p2));
        return;
      case "coincident": {
        const e = E(r.e);
        if (e) this.pointOnEntity(id, r.p, e);
        else if (sk.points[r.e]) {
          const deps = [...this.pdeps(r.p), ...this.pdeps(r.e)];
          this.add(id, deps, () => this.px(r.p) - this.px(r.e));
          this.add(id, deps, () => this.py(r.p) - this.py(r.e));
        }
        return;
      }
      case "midpoint": {
        const e = E(r.e);
        if (!e) return;
        const deps = [...this.pdeps(r.p), ...this.edeps(e)];
        if (e.type === "line") {
          this.add(id, deps, () => this.px(r.p) - (this.px(e.p1) + this.px(e.p2)) / 2);
          this.add(id, deps, () => this.py(r.p) - (this.py(e.p1) + this.py(e.p2)) / 2);
        } else if (e.type === "arc") {
          this.pointOnEntity(id, r.p, e);
          this.add(id, deps, () => {
            const p = this.P(r.p), a = this.P(e.p1), b = this.P(e.p2);
            return Math.hypot(p[0] - a[0], p[1] - a[1]) - Math.hypot(p[0] - b[0], p[1] - b[1]);
          });
        }
        return;
      }
      case "parallel":
      case "perpendicular":
      case "collinear": {
        const a = E(r.e1), b = E(r.e2);
        if (!a || !b || a.type !== "line" || b.type !== "line") return;
        const deps = [...this.edeps(a), ...this.edeps(b)];
        if (r.type === "collinear") {
          this.add(id, deps, () => distToLine(this.P(b.p1), this.P(a.p1), this.P(a.p2)));
          this.add(id, deps, () => distToLine(this.P(b.p2), this.P(a.p1), this.P(a.p2)));
          return;
        }
        this.add(id, deps, () => {
          const u = unit(this.P(a.p1), this.P(a.p2)), v = unit(this.P(b.p1), this.P(b.p2));
          return r.type === "parallel" ? u[0] * v[1] - u[1] * v[0] : u[0] * v[0] + u[1] * v[1];
        });
        return;
      }
      case "equal": {
        const a = E(r.e1), b = E(r.e2);
        if (!a || !b) return;
        const deps = [...this.edeps(a), ...this.edeps(b)];
        const size = (e: SketchEntity) =>
          e.type === "line" ? dist(this.P(e.p1), this.P(e.p2)) : this.rad(e);
        this.add(id, deps, () => size(a) - size(b));
        return;
      }
      case "tangent": {
        let a = E(r.e1), b = E(r.e2);
        if (!a || !b) return;
        if (b.type === "line") [a, b] = [b, a];
        const deps = [...this.edeps(a), ...this.edeps(b)];
        if (a.type === "line" && (b.type === "arc" || b.type === "circle")) {
          const bb = b;
          // When the arc ends on the line (fillets, slots, tangent arcs) the distance form is
          // stationary at the solution, so use "radius at the shared point ⟂ line" instead.
          const shared = bb.type === "arc" ? [bb.p1, bb.p2].find((p) => p === a.p1 || p === a.p2) : undefined;
          if (shared) {
            this.add(id, deps, () => {
              const u = unit(this.P(a.p1), this.P(a.p2)), w = unit(this.P(bb.c), this.P(shared));
              return u[0] * w[0] + u[1] * w[1];
            });
          } else this.add(id, deps, () => Math.abs(distToLine(this.P(bb.c), this.P(a.p1), this.P(a.p2))) - this.rad(bb));
        } else if ((a.type === "arc" || a.type === "circle") && (b.type === "arc" || b.type === "circle")) {
          const aa = a, bb = b;
          // Two arcs meeting at a shared endpoint are tangent when both centres line up with it.
          const ends = (e: SketchEntity) => (e.type === "arc" ? [e.p1, e.p2] : []);
          const sharedPt = ends(aa).find((p) => ends(bb).includes(p));
          if (sharedPt) {
            this.add(id, deps, () => {
              const u = unit(this.P(sharedPt), this.P(aa.c)), w = unit(this.P(sharedPt), this.P(bb.c));
              return u[0] * w[1] - u[1] * w[0];
            });
            return;
          }
          this.add(id, deps, () => {
            const d = dist(this.P(aa.c), this.P(bb.c));
            return r.inner ? d - Math.abs(this.rad(aa) - this.rad(bb)) : d - (this.rad(aa) + this.rad(bb));
          });
        }
        return;
      }
      case "concentric":
      case "coradial": {
        const a = E(r.e1), b = E(r.e2);
        const ca = a && this.center(a), cb = b && this.center(b);
        if (!ca || !cb) return;
        const deps = [...this.edeps(a), ...this.edeps(b)];
        this.add(id, deps, () => this.px(ca) - this.px(cb));
        this.add(id, deps, () => this.py(ca) - this.py(cb));
        if (r.type === "coradial") this.add(id, deps, () => this.rad(a) - this.rad(b));
        return;
      }
      case "symmetric": {
        const ax = E(r.axis);
        if (!ax || ax.type !== "line") return;
        const deps = [...this.pdeps(r.p1), ...this.pdeps(r.p2), ...this.edeps(ax)];
        this.add(id, deps, () => {
          const p = this.P(r.p1), q = this.P(r.p2);
          return distToLine([(p[0] + q[0]) / 2, (p[1] + q[1]) / 2], this.P(ax.p1), this.P(ax.p2));
        });
        this.add(id, deps, () => {
          const p = this.P(r.p1), q = this.P(r.p2);
          const u = unit(this.P(ax.p1), this.P(ax.p2));
          return (q[0] - p[0]) * u[0] + (q[1] - p[1]) * u[1];
        });
        return;
      }
      case "intersection": {
        const a = E(r.e1), b = E(r.e2);
        if (a) this.pointOnEntity(id, r.p, a);
        if (b) this.pointOnEntity(id, r.p, b);
        return;
      }
      case "equidistant": {
        const deps = [...this.pdeps(r.c), ...this.pdeps(r.p1), ...this.pdeps(r.p2)];
        this.add(id, deps, () => dist(this.P(r.c), this.P(r.p1)) - dist(this.P(r.c), this.P(r.p2)));
        return;
      }
      case "offset": {
        const a = E(r.e1), b = E(r.e2), ma = E(r.m1), mb = E(r.m2);
        if (!a || !b || !ma || !mb) return;
        const sd = (x: SketchEntity, y: SketchEntity) => this.signedOffset(x, y);
        // Keep the sign seen at creation so the copy never flips sides.
        const s0 = Math.sign(sd(a, b)) || 1, sm = Math.sign(sd(ma, mb)) || 1;
        this.add(id, [...this.edeps(a), ...this.edeps(b), ...this.edeps(ma), ...this.edeps(mb)], () => s0 * sd(a, b) - sm * sd(ma, mb));
        return;
      }
      case "fix":
        return; // handled by removing params
    }
  }

  /** Signed offset of y from x: lines (perpendicular distance of y's midpoint), arcs (radius change). */
  private signedOffset(x: SketchEntity, y: SketchEntity): number {
    if (x.type === "line" && y.type === "line") {
      const a = this.P(x.p1), b = this.P(x.p2);
      const m: Vec2 = [(this.px(y.p1) + this.px(y.p2)) / 2, (this.py(y.p1) + this.py(y.p2)) / 2];
      const u = unit(a, b);
      return u[0] * (m[1] - a[1]) - u[1] * (m[0] - a[0]);
    }
    return this.rad(y) - this.rad(x);
  }

  private dimension(d: Dimension) {
    const id = d.id;
    const g = this.measureFn(d);
    if (!g) return;
    const deps = this.dimDeps(d);
    if (d.kind === "angle") this.add(id, deps, () => wrapAngle(g() - d.value));
    else this.add(id, deps, () => g() - d.value);
  }

  private dimDeps(d: Dimension) {
    const out: number[] = [];
    for (const r of d.refs) {
      if (this.sk.points[r]) out.push(...this.pdeps(r));
      const e = this.sk.entities[r];
      if (e) out.push(...this.edeps(e));
    }
    return out;
  }

  /** Function computing the current measured value of a dimension. */
  measureFn(d: Dimension): (() => number) | null {
    const sk = this.sk;
    const [r0, r1] = d.refs;
    const sign = d.sign ?? 1;
    switch (d.kind) {
      case "distance":
        return () => dist(this.P(r0), this.P(r1));
      case "horizontal":
        return () => sign * (this.px(r1) - this.px(r0));
      case "vertical":
        return () => sign * (this.py(r1) - this.py(r0));
      case "pointLine": {
        const l = sk.entities[r1];
        if (!l || l.type !== "line") return null;
        return () => Math.abs(distToLine(this.P(r0), this.P(l.p1), this.P(l.p2)));
      }
      case "lineLine": {
        const a = sk.entities[r0], b = sk.entities[r1];
        if (!a || !b || a.type !== "line" || b.type !== "line") return null;
        return () => {
          const m: Vec2 = [(this.px(b.p1) + this.px(b.p2)) / 2, (this.py(b.p1) + this.py(b.p2)) / 2];
          return Math.abs(distToLine(m, this.P(a.p1), this.P(a.p2)));
        };
      }
      case "angle": {
        const a = sk.entities[r0], b = sk.entities[r1];
        if (!a || !b || a.type !== "line" || b.type !== "line") return null;
        const q = d.quadrant ?? 0;
        return () => {
          let u = unit(this.P(a.p1), this.P(a.p2)), v = unit(this.P(b.p1), this.P(b.p2));
          if (q & 1) u = [-u[0], -u[1]];
          if (q & 2) v = [-v[0], -v[1]];
          return sign * Math.atan2(u[0] * v[1] - u[1] * v[0], u[0] * v[0] + u[1] * v[1]);
        };
      }
      case "radius":
      case "diameter": {
        const e = sk.entities[r0];
        if (!e) return null;
        const k = d.kind === "diameter" ? 2 : 1;
        return () => k * this.rad(e);
      }
      case "offset": {
        const a = sk.entities[r0], b = sk.entities[r1];
        if (!a || !b) return null;
        return () => Math.abs(this.signedOffset(a, b));
      }
      case "arcLength": {
        const e = sk.entities[r0];
        if (!e || e.type !== "arc") return null;
        return () => {
          const c = this.P(e.c), a = this.P(e.p1), b = this.P(e.p2);
          let sw = Math.atan2(b[1] - c[1], b[0] - c[0]) - Math.atan2(a[1] - c[1], a[0] - c[0]);
          while (sw <= 0) sw += 2 * Math.PI;
          return sw * this.rad(e);
        };
      }
    }
  }

  // ------------------------------------------------------------ numerics
  residuals(): number[] {
    return this.eqs.map((e) => e.f());
  }

  /** Dense Jacobian (rows = equations) by central differences on each equation's deps. */
  jacobian(): Float64Array[] {
    const n = this.x.length;
    return this.eqs.map((eq) => {
      const row = new Float64Array(n);
      for (const j of eq.deps) {
        const x0 = this.x[j];
        const h = 1e-7 * Math.max(1, Math.abs(x0));
        this.x[j] = x0 + h;
        const fp = eq.f();
        this.x[j] = x0 - h;
        const fm = eq.f();
        this.x[j] = x0;
        row[j] = (fp - fm) / (2 * h);
      }
      return row;
    });
  }

  /** Solve all hard constraints. Returns true when converged. */
  solve(maxIter = 60, tol = 1e-11): boolean {
    if (!this.eqs.length) return true;
    let r = this.residuals();
    let err = norm2(r);
    for (let it = 0; it < maxIter; it++) {
      if (Math.sqrt(err) < tol) return true;
      const J = this.jacobian();
      const step = minNormStep(J, r, this.x.length);
      if (!step) return false;
      const x0 = this.x.slice();
      let alpha = 1;
      let improved = false;
      for (let ls = 0; ls < 10; ls++) {
        for (let i = 0; i < x0.length; i++) this.x[i] = x0[i] - alpha * step[i];
        const r2 = this.residuals();
        const e2 = norm2(r2);
        if (e2 < err) {
          r = r2;
          err = e2;
          improved = true;
          break;
        }
        alpha *= 0.5;
      }
      if (!improved) {
        this.x = x0;
        return Math.sqrt(err) < 1e-6;
      }
    }
    return Math.sqrt(err) < 1e-6;
  }

  /**
   * Drag points towards targets while keeping constraints satisfied.
   * Returns true if anything moved.
   */
  drag(targets: { id: Id; to: Vec2 }[], steps = 12): boolean {
    const idx: [number, number, Vec2][] = [];
    for (const t of targets) {
      const r = this.pts.get(t.id);
      if (r && r.ix >= 0) idx.push([r.ix, r.iy, t.to]);
    }
    if (!idx.length) return false;
    const start = this.x.slice();
    for (let s = 0; s < steps; s++) {
      const d = new Float64Array(this.x.length);
      let remaining = 0;
      for (const [ix, iy, to] of idx) {
        d[ix] = to[0] - this.x[ix];
        d[iy] = to[1] - this.x[iy];
        remaining += d[ix] ** 2 + d[iy] ** 2;
      }
      if (remaining < 1e-18) break;
      const proj = this.eqs.length ? projectToNullSpace(this.jacobian(), d) : d;
      let moved = 0;
      for (let i = 0; i < this.x.length; i++) {
        this.x[i] += proj[i];
        moved += proj[i] ** 2;
      }
      if (!this.solve(30)) {
        this.x = start;
        this.solve(30);
        return false;
      }
      if (moved < 1e-14) break;
    }
    return this.x.some((v, i) => Math.abs(v - start[i]) > 1e-12);
  }

  /** Write solved values back into the sketch data (mutates). */
  commit() {
    for (const [id, r] of this.pts) {
      if (r.ix < 0) continue;
      const p = this.sk.points[id];
      p.x = this.x[r.ix];
      p.y = this.x[r.iy];
    }
    for (const [id, r] of this.radii) {
      if (r.i < 0) continue;
      const e = this.sk.entities[id];
      if (e.type === "circle") e.r = this.x[r.i];
    }
    // Driven dimensions display the measured value.
    for (const d of Object.values(this.sk.dimensions)) {
      if (!d.driven) continue;
      const f = this.measureFn(d);
      if (f) d.value = Math.abs(f());
    }
  }

  /** Degrees of freedom / definition state. Call after solve(). */
  analyse(converged: boolean): Analysis {
    const n = this.x.length;
    const J = this.jacobian();
    const m = J.length;
    const bad = new Set<Id>();
    // Row-normalise so tolerance is scale independent.
    const R = J.map((row) => {
      let s = 0;
      for (let j = 0; j < n; j++) s += row[j] * row[j];
      s = Math.sqrt(s);
      if (s > 0) for (let j = 0; j < n; j++) row[j] /= s;
      return row;
    });
    // RREF, processing equations in creation order so later ones are flagged redundant.
    const pivotCol: number[] = [];
    const rows: Float64Array[] = [];
    const tol = 1e-8;
    for (let i = 0; i < m; i++) {
      const row = R[i].slice();
      for (let k = 0; k < rows.length; k++) {
        const c = pivotCol[k];
        const f = row[c];
        if (f !== 0) for (let j = 0; j < n; j++) row[j] -= f * rows[k][j];
      }
      let best = -1, bv = tol;
      for (let j = 0; j < n; j++) if (Math.abs(row[j]) > bv) { bv = Math.abs(row[j]); best = j; }
      if (best < 0) {
        bad.add(this.eqs[i].owner);
        continue;
      }
      const pv = row[best];
      for (let j = 0; j < n; j++) row[j] /= pv;
      for (const other of rows) {
        const f = other[best];
        if (f !== 0) for (let j = 0; j < n; j++) other[j] -= f * row[j];
      }
      rows.push(row);
      pivotCol.push(best);
    }
    bad.delete("internal");
    const rank = rows.length;
    const dof = n - rank;
    const isPivot = new Map<number, number>();
    pivotCol.forEach((c, k) => isPivot.set(c, k));
    const free = [];
    for (let j = 0; j < n; j++) if (!isPivot.has(j)) free.push(j);
    const determined = new Uint8Array(n);
    for (let j = 0; j < n; j++) {
      const k = isPivot.get(j);
      if (k === undefined) continue;
      let ok = true;
      for (const f of free) if (Math.abs(rows[k][f]) > 1e-7) { ok = false; break; }
      if (ok) determined[j] = 1;
    }
    if (!converged) {
      // Every equation still violated is conflicting.
      this.eqs.forEach((e) => {
        if (Math.abs(e.f()) > 1e-6 && e.owner !== "internal") bad.add(e.owner);
      });
    }
    const pointState: Record<Id, DefState> = {};
    for (const [id, r] of this.pts) {
      pointState[id] = r.ix < 0 || (determined[r.ix] && determined[r.iy]) ? "full" : "under";
    }
    const entityState: Record<Id, DefState> = {};
    for (const e of Object.values(this.sk.entities)) {
      let full = entityPoints(e).every((p) => pointState[p] === "full");
      if (e.type === "circle") {
        const r = this.radii.get(e.id)!;
        full = full && (r.i < 0 || !!determined[r.i]);
      }
      entityState[e.id] = full ? "full" : "under";
    }
    // Mark entities touched by bad constraints as over defined.
    if (bad.size) {
      const mark = (id: Id) => {
        if (entityState[id]) entityState[id] = "over";
        if (pointState[id]) pointState[id] = "over";
      };
      for (const r of Object.values(this.sk.relations)) if (bad.has(r.id)) relationRefs(r).forEach(mark);
      for (const d of Object.values(this.sk.dimensions)) if (bad.has(d.id)) d.refs.forEach(mark);
    }
    const status = !converged ? "unsolved" : bad.size ? "over" : dof === 0 ? "full" : "under";
    return { dof, status: !converged && bad.size ? "over" : status, pointState, entityState, bad };
  }
}

// ---------------------------------------------------------------- helpers

export function entityPoints(e: SketchEntity): Id[] {
  switch (e.type) {
    case "line": return [e.p1, e.p2];
    case "circle": return [e.c];
    case "arc": return [e.c, e.p1, e.p2];
    case "point": return [e.p];
  }
}

export function relationRefs(r: Relation): Id[] {
  switch (r.type) {
    case "horizontal":
    case "vertical": return [r.e];
    case "horizontalPoints":
    case "verticalPoints": return [r.p1, r.p2];
    case "coincident":
    case "midpoint": return [r.p, r.e];
    case "symmetric": return [r.p1, r.p2, r.axis];
    case "intersection": return [r.p, r.e1, r.e2];
    case "fix": return [r.target];
    case "equidistant": return [r.c, r.p1, r.p2];
    case "offset": return [r.e1, r.e2];
    default: return [r.e1, r.e2];
  }
}

function dist(a: Vec2, b: Vec2) {
  return Math.hypot(a[0] - b[0], a[1] - b[1]);
}
function unit(a: Vec2, b: Vec2): Vec2 {
  const dx = b[0] - a[0], dy = b[1] - a[1];
  const l = Math.hypot(dx, dy) || 1e-12;
  return [dx / l, dy / l];
}
/** Signed distance from p to the infinite line through a, b. */
function distToLine(p: Vec2, a: Vec2, b: Vec2) {
  const dx = b[0] - a[0], dy = b[1] - a[1];
  const l = Math.hypot(dx, dy) || 1e-12;
  return ((p[0] - a[0]) * dy - (p[1] - a[1]) * dx) / l;
}
function norm2(r: number[]) {
  let s = 0;
  for (const v of r) s += v * v;
  return s;
}

/** Solve (J Jᵀ + μI) y = r and return Jᵀ y : the minimum-norm Gauss-Newton step. */
function minNormStep(J: Float64Array[], r: number[], n: number): Float64Array | null {
  const m = J.length;
  const A = new Float64Array(m * m);
  let trace = 0;
  for (let i = 0; i < m; i++) {
    for (let k = i; k < m; k++) {
      let s = 0;
      const a = J[i], b = J[k];
      for (let j = 0; j < n; j++) s += a[j] * b[j];
      A[i * m + k] = s;
      A[k * m + i] = s;
    }
    trace += A[i * m + i];
  }
  const mu = Math.max(1e-12, (trace / Math.max(1, m)) * 1e-9);
  for (let i = 0; i < m; i++) A[i * m + i] += mu;
  const y = solveSPD(A, Float64Array.from(r), m);
  if (!y) return null;
  const step = new Float64Array(n);
  for (let i = 0; i < m; i++) {
    const yi = y[i];
    if (yi === 0) continue;
    const row = J[i];
    for (let j = 0; j < n; j++) step[j] += row[j] * yi;
  }
  return step;
}

/** d - Jᵀ (J Jᵀ)⁻¹ J d : component of d that keeps constraints (to first order). */
function projectToNullSpace(J: Float64Array[], d: Float64Array): Float64Array {
  const n = d.length;
  const Jd = J.map((row) => {
    let s = 0;
    for (let j = 0; j < n; j++) s += row[j] * d[j];
    return s;
  });
  const corr = minNormStep(J, Jd, n);
  if (!corr) return new Float64Array(n);
  const out = new Float64Array(n);
  for (let j = 0; j < n; j++) out[j] = d[j] - corr[j];
  return out;
}

/** Cholesky solve with fallback to Gaussian elimination for semi-definite systems. */
function solveSPD(A: Float64Array, b: Float64Array, m: number): Float64Array | null {
  const L = new Float64Array(m * m);
  for (let i = 0; i < m; i++) {
    for (let j = 0; j <= i; j++) {
      let s = A[i * m + j];
      for (let k = 0; k < j; k++) s -= L[i * m + k] * L[j * m + k];
      if (i === j) {
        if (s <= 1e-300) return gauss(A, b, m);
        L[i * m + i] = Math.sqrt(s);
      } else L[i * m + j] = s / L[j * m + j];
    }
  }
  const y = new Float64Array(m);
  for (let i = 0; i < m; i++) {
    let s = b[i];
    for (let k = 0; k < i; k++) s -= L[i * m + k] * y[k];
    y[i] = s / L[i * m + i];
  }
  const x = new Float64Array(m);
  for (let i = m - 1; i >= 0; i--) {
    let s = y[i];
    for (let k = i + 1; k < m; k++) s -= L[k * m + i] * x[k];
    x[i] = s / L[i * m + i];
  }
  return x;
}

function gauss(A0: Float64Array, b0: Float64Array, m: number): Float64Array | null {
  const A = A0.slice(), b = b0.slice();
  for (let c = 0; c < m; c++) {
    let p = c;
    for (let r = c + 1; r < m; r++) if (Math.abs(A[r * m + c]) > Math.abs(A[p * m + c])) p = r;
    if (Math.abs(A[p * m + c]) < 1e-300) continue;
    if (p !== c) {
      for (let k = 0; k < m; k++) [A[c * m + k], A[p * m + k]] = [A[p * m + k], A[c * m + k]];
      [b[c], b[p]] = [b[p], b[c]];
    }
    for (let r = c + 1; r < m; r++) {
      const f = A[r * m + c] / A[c * m + c];
      if (!f) continue;
      for (let k = c; k < m; k++) A[r * m + k] -= f * A[c * m + k];
      b[r] -= f * b[c];
    }
  }
  const x = new Float64Array(m);
  for (let i = m - 1; i >= 0; i--) {
    let s = b[i];
    for (let k = i + 1; k < m; k++) s -= A[i * m + k] * x[k];
    x[i] = Math.abs(A[i * m + i]) < 1e-300 ? 0 : s / A[i * m + i];
  }
  return x;
}

/** Convenience: solve a sketch in place and analyse it. */
export function solveSketch(sk: SketchData): Analysis {
  const sys = new SketchSystem(sk);
  const ok = sys.solve();
  sys.commit();
  return sys.analyse(ok);
}
