/**
 * Evaluate > Measure: a floating dialog that measures the selected entities
 * (distance, length, area, angle, diameter, XYZ), as SOLIDWORKS does.
 */
import type { App, SelItem } from "../app/app";
import type { EdgeSig, Vec3 } from "../core/types";
import { v3 } from "../core/math";
import { formatLength, UNIT_INFO } from "../core/units";
import { Icon } from "./icons";

/** Circle through a (partial or full) circular edge: centre and radius. */
export function edgeCircle(sig: EdgeSig): { c: Vec3; r: number } | null {
  if (sig.curve !== "CIRCLE") return null;
  const full = v3.dist(sig.a, sig.b) < 1e-9;
  if (full) {
    const c = v3.scale(v3.add(sig.a, sig.mid), 0.5);
    return { c, r: v3.dist(sig.a, sig.mid) / 2 };
  }
  const a = sig.a, b = sig.mid, c3 = sig.b;
  const ab = v3.sub(b, a), ac = v3.sub(c3, a);
  const n = v3.cross(ab, ac);
  const nn = v3.dot(n, n);
  if (nn < 1e-18) return null;
  const t1 = v3.scale(v3.cross(n, ab), v3.dot(ac, ac));
  const t2 = v3.scale(v3.cross(ac, n), v3.dot(ab, ab));
  const c = v3.add(a, v3.scale(v3.add(t1, t2), 1 / (2 * nn)));
  return { c, r: v3.dist(c, a) };
}

/**
 * Measure in an assembly (G1c): the same lines as for a part, from the engine's answer for
 * the selected entities at their occurrences' placements (`AssemblySession.measure`).
 */
function assemblyLines(app: App, L: (v: number) => string, A: (v: number) => string, P: (p: Vec3) => string): string[] {
  const m = app.asmMeasure;
  const lines: string[] = [];
  if (!m?.items.length) return lines;
  if (m.items.length === 1) {
    const it = m.items[0];
    if (it.kind === "face") lines.push(`Area: ${A(it.area)}`);
    if (it.kind === "edge") {
      if (it.circle) lines.push(`Diameter: ${L(it.circle.diameter)}`, `Center: ${P(it.circle.center)}`);
      lines.push(`Length: ${L(it.length)}`);
    }
    if (it.kind === "vertex") lines.push(`X: ${L(it.point[0])}`, `Y: ${L(it.point[1])}`, `Z: ${L(it.point[2])}`);
    return lines;
  }
  const d = m.distance;
  if (d) lines.push(`Distance: ${L(d.distance)}`, `Delta X: ${L(Math.abs(d.p2[0] - d.p1[0]))}`, `Delta Y: ${L(Math.abs(d.p2[1] - d.p1[1]))}`, `Delta Z: ${L(Math.abs(d.p2[2] - d.p1[2]))}`);
  const dir = (k: number) => {
    const it = m.items[k];
    return it.kind === "vertex" ? null : it.dir;
  };
  const d1 = dir(0), d2 = dir(1);
  if (d1 && d2) {
    const ang = (Math.acos(Math.min(1, Math.abs(v3.dot(v3.norm(d1), v3.norm(d2))))) * 180) / Math.PI;
    lines.push(`Angle: ${ang.toFixed(2)}°`);
  }
  if (m.items.length > 2) {
    const total = m.items.reduce((t, it) => t + (it.kind === "edge" ? it.length : 0), 0);
    if (total) lines.push(`Total Length: ${L(total)}`);
  }
  return lines;
}

export function MeasureDialog({ app }: { app: App }) {
  const mesh = app.result?.mesh;
  const u = app.doc.units;
  const L = (v: number) => `${formatLength(v, u, false, 3, true)}${UNIT_INFO[u].length}`;
  const A = (v: number) => `${formatLength(v / UNIT_INFO[u].perMm, u, false, 3, true)}${UNIT_INFO[u].length}^2`;
  const P = (p: Vec3) => p.map((x) => formatLength(x, u, false, 3, true)).join(", ");
  const items = app.selection.filter((s) => s.kind === "face" || s.kind === "edge" || s.kind === "vertex" || s.kind === "origin");
  const lines: string[] = app.asm ? assemblyLines(app, L, A, P) : [];
  const dirOf = (s: SelItem): Vec3 | null => {
    if (s.kind === "face" && mesh?.faces[s.id]?.planar) return mesh.faces[s.id].sig.normal;
    if (s.kind === "edge" && mesh?.edges[s.id]?.linear) return v3.norm(v3.sub(mesh.edges[s.id].sig.b, mesh.edges[s.id].sig.a));
    return null;
  };
  if (app.asm) {
    // Measured by the engine (above).
  } else if (items.length === 1) {
    const s = items[0];
    if (s.kind === "face" && mesh?.faces[s.id]) lines.push(`Area: ${A(mesh.faces[s.id].sig.area)}`);
    if (s.kind === "edge" && mesh?.edges[s.id]) {
      const sig = mesh.edges[s.id].sig;
      const circ = edgeCircle(sig);
      if (circ) lines.push(`Diameter: ${L(circ.r * 2)}`, `Center: ${P(circ.c)}`);
      lines.push(`Length: ${L(sig.length)}`);
    }
    if (s.kind === "vertex" && mesh?.vertices[s.id]) {
      const p = mesh.vertices[s.id];
      lines.push(`X: ${L(p[0])}`, `Y: ${L(p[1])}`, `Z: ${L(p[2])}`);
    }
    if (s.kind === "origin") lines.push("X: 0", "Y: 0", "Z: 0");
  } else if (items.length >= 2) {
    const m = app.measureResult;
    if (m) lines.push(`Distance: ${L(m.distance)}`, `Delta X: ${L(Math.abs(m.p2[0] - m.p1[0]))}`, `Delta Y: ${L(Math.abs(m.p2[1] - m.p1[1]))}`, `Delta Z: ${L(Math.abs(m.p2[2] - m.p1[2]))}`);
    const d1 = dirOf(items[0]), d2 = dirOf(items[1]);
    if (d1 && d2) {
      const ang = (Math.acos(Math.min(1, Math.abs(v3.dot(v3.norm(d1), v3.norm(d2))))) * 180) / Math.PI;
      lines.push(`Angle: ${ang.toFixed(2)}°`);
    }
    if (items.length > 2) {
      const total = items.reduce((t, s) => t + (s.kind === "edge" && mesh?.edges[s.id] ? mesh.edges[s.id].sig.length : 0), 0);
      if (total) lines.push(`Total Length: ${L(total)}`);
    }
  }
  return (
    <div className="measure" onMouseDown={(e) => e.stopPropagation()}>
      <div className="measure-head">
        <Icon name="measure" size={16} /> Measure - {app.docName()}
        <button
          title="Close"
          onClick={() => {
            app.measureOpen = false;
            app.measureResult = null;
            app.asmMeasure = null;
            app.changed();
          }}
        >
          ✕
        </button>
      </div>
      <div className="measure-body" data-measure-lines="">
        {lines.length ? lines.map((l) => <div key={l}>{l}</div>) : <div className="muted">Make a selection, or right-click and choose Clear Selections.</div>}
        {items.length > 0 && <div className="muted small">{items.length} item{items.length > 1 ? "s" : ""} selected</div>}
      </div>
    </div>
  );
}
