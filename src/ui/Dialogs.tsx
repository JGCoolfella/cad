import { useEffect, useState, type ReactNode } from "react";
import type { App } from "../app/app";
import type { Viewport } from "../render/viewport";
import { DEFAULT_OPTIONS, type SystemOptions } from "../app/settings";
import type { UnitSystem } from "../core/types";
import { UNIT_INFO } from "../core/units";

function Dialog({ title, children, onClose, footer, wide }: { title: string; children: ReactNode; onClose: () => void; footer?: ReactNode; wide?: boolean }) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);
  return (
    <div className="modal-bg" onMouseDown={onClose}>
      <div className={`modal ${wide ? "wide" : ""}`} onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          {title}
          <button onClick={onClose}>✕</button>
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-foot">{footer}</div>}
      </div>
    </div>
  );
}

const PAGES = ["General", "View", "Sketch", "Relations/Snaps", "Colors", "Performance", "Document Properties: Units"] as const;

/** Tools > Options (System Options + Document Properties). */
export function OptionsDialog({ app, vp, onClose }: { app: App; vp: Viewport | null; onClose: () => void }) {
  const [page, setPage] = useState<(typeof PAGES)[number]>("View");
  const [o, setO] = useState<SystemOptions>(() => structuredClone(app.options));
  const [units, setUnits] = useState<UnitSystem>(app.doc.units);
  const upd = (f: (x: SystemOptions) => void) => {
    const n = structuredClone(o);
    f(n);
    setO(n);
  };
  const chk = (label: string, get: (x: SystemOptions) => boolean, set: (x: SystemOptions, v: boolean) => void) => (
    <label className="chk">
      <input type="checkbox" checked={get(o)} onChange={(e) => upd((x) => set(x, e.target.checked))} /> {label}
    </label>
  );
  const ok = () => {
    app.setOptions((x) => Object.assign(x, o));
    if (units !== app.doc.units) {
      app.checkpoint();
      app.doc.units = units;
    }
    if (vp) {
      vp.renderer.setPixelRatio(window.devicePixelRatio * o.performance.pixelRatio);
      vp.invalidate();
    }
    app.changed();
    onClose();
  };
  return (
    <Dialog
      title="System Options - View"
      onClose={onClose}
      wide
      footer={
        <>
          <button className="btn" onClick={() => setO(structuredClone(DEFAULT_OPTIONS))}>
            Reset...
          </button>
          <span className="grow" />
          <button className="btn primary" onClick={ok}>
            OK
          </button>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
        </>
      }
    >
      <div className="opt">
        <div className="opt-nav">
          {PAGES.map((p) => (
            <div key={p} className={p === page ? "on" : ""} onClick={() => setPage(p)}>
              {p}
            </div>
          ))}
        </div>
        <div className="opt-page">
          {page === "General" && (
            <>
              <label className="row">
                Theme{" "}
                <select value={o.theme} onChange={(e) => upd((x) => (x.theme = e.target.value as "light" | "dark"))}>
                  <option value="light">Light</option>
                  <option value="dark">Dark</option>
                </select>
              </label>
              {chk("Input dimension value", (x) => x.sketch.inputDimensionValue, (x, v) => (x.sketch.inputDimensionValue = v))}
            </>
          )}
          {page === "View" && (
            <>
              {chk("Reverse mouse wheel zoom direction", (x) => x.view.reverseWheelZoom, (x, v) => (x.view.reverseWheelZoom = v))}
              {chk("Zoom about screen center", (x) => x.view.zoomAboutScreenCenter, (x, v) => (x.view.zoomAboutScreenCenter = v))}
              {chk("Perspective (default for new views)", (x) => x.view.perspective, (x, v) => (x.view.perspective = v))}
              <label className="row">
                Arrow keys rotation increment (deg)
                <input type="number" value={o.view.arrowKeyIncrement} onChange={(e) => upd((x) => (x.view.arrowKeyIncrement = +e.target.value || 15))} />
              </label>
              <label className="row">
                View animation speed (s)
                <input type="range" min={0} max={1} step={0.05} value={o.view.animationSpeed} onChange={(e) => upd((x) => (x.view.animationSpeed = +e.target.value))} />
                <span>{o.view.animationSpeed.toFixed(2)}</span>
              </label>
            </>
          )}
          {page === "Sketch" && (
            <>
              {chk("Auto-rotate view normal to sketch plane on sketch creation", (x) => x.sketch.autoRotateNormal, (x, v) => (x.sketch.autoRotateNormal = v))}
              {chk("Display sketch relations", (x) => x.sketch.showRelations, (x, v) => (x.sketch.showRelations = v))}
              {chk("Display grid", (x) => x.sketch.displayGrid, (x, v) => (x.sketch.displayGrid = v))}
              <label className="row">
                Grid spacing (mm)
                <input type="number" value={o.sketch.gridSpacing} onChange={(e) => upd((x) => (x.sketch.gridSpacing = +e.target.value || 10))} />
              </label>
            </>
          )}
          {page === "Relations/Snaps" && (
            <>
              {chk("Enable snapping", (x) => x.sketch.enableSnapping, (x, v) => {
                x.sketch.enableSnapping = v;
                if (!v) x.sketch.automaticRelations = false;
              })}
              {chk("Automatic relations", (x) => x.sketch.automaticRelations, (x, v) => (x.sketch.automaticRelations = v))}
              <div className="sub">Sketch snaps</div>
              {(Object.keys(o.sketch.snaps) as (keyof SystemOptions["sketch"]["snaps"])[]).map((k) => (
                <span key={k}>{chk(SNAP_LABELS[k], (x) => x.sketch.snaps[k], (x, v) => (x.sketch.snaps[k] = v))}</span>
              ))}
              <label className="row">
                Snap angle (deg)
                <input type="number" value={o.sketch.snapAngle} onChange={(e) => upd((x) => (x.sketch.snapAngle = +e.target.value || 15))} />
              </label>
            </>
          )}
          {page === "Colors" && (
            <>
              {(Object.keys(o.colors) as (keyof SystemOptions["colors"])[]).map((k) =>
                k === "background" ? null : (
                  <label key={k} className="row">
                    {COLOR_LABELS[k] ?? k}
                    <input type="color" value={o.colors[k] as string} onChange={(e) => upd((x) => ((x.colors[k] as string) = e.target.value))} />
                  </label>
                ),
              )}
            </>
          )}
          {page === "Performance" && (
            <>
              <label className="row">
                Render resolution
                <select value={o.performance.pixelRatio} onChange={(e) => upd((x) => (x.performance.pixelRatio = +e.target.value))}>
                  <option value={1}>Full (best quality)</option>
                  <option value={0.75}>75%</option>
                  <option value={0.5}>50% (fastest)</option>
                </select>
              </label>
              {chk("Anti-aliasing (MSAA, takes effect on restart)", (x) => x.performance.msaa, (x, v) => (x.performance.msaa = v))}
              <p className="muted">Geometry rebuilds run in a background worker with incremental caching, so the interface stays responsive during regeneration.</p>
            </>
          )}
          {page === "Document Properties: Units" && (
            <>
              {(Object.keys(UNIT_INFO) as UnitSystem[]).map((u) => (
                <label key={u} className="chk">
                  <input type="radio" checked={units === u} onChange={() => setUnits(u)} /> {UNIT_INFO[u].label}
                </label>
              ))}
            </>
          )}
        </div>
      </div>
    </Dialog>
  );
}

const SNAP_LABELS: Record<string, string> = {
  endpoints: "End points and sketch points",
  centers: "Center Points",
  midpoints: "Midpoints",
  quadrants: "Quadrant Points",
  intersections: "Intersections",
  nearest: "Nearest",
  tangent: "Tangent",
  perpendicular: "Perpendicular",
  parallel: "Parallel",
  hvLines: "Horizontal/vertical lines",
  hvPoints: "Horizontal/vertical to points",
  length: "Length",
  grid: "Grid",
  angle: "Angle",
};

const COLOR_LABELS: Record<string, string> = {
  model: "Default part color",
  edges: "Edges",
  hover: "Dynamic highlight",
  selected: "Selected item",
  sketchUnder: "Sketch, under defined",
  sketchFull: "Sketch, fully defined",
  sketchOver: "Sketch, over defined",
  sketchInactive: "Inactive sketch",
  inferenceRelation: "Inference line (relation)",
  inferenceGuide: "Inference line (guide)",
  dimension: "Dimensions",
  dimensionDriven: "Dimensions, driven",
  plane: "Planes",
};

export function MassPropsDialog({ app, onClose }: { app: App; onClose: () => void }) {
  const [r, setR] = useState<{ volume: number; area: number; centerOfMass: [number, number, number] } | null | "loading" | string>("loading");
  const [density, setDensity] = useState(1000);
  useEffect(() => {
    app.kernel
      .massProps(app.activeFeatures())
      .then(setR)
      .catch((e) => setR(String(e.message ?? e)));
  }, [app]);
  return (
    <Dialog title="Mass Properties" onClose={onClose}>
      {r === "loading" ? (
        <p>Calculating…</p>
      ) : typeof r === "string" ? (
        <p className="err">{r}</p>
      ) : !r ? (
        <p>No solid bodies.</p>
      ) : (
        <div className="massprops">
          <label className="row">
            Density (kg/m³) <input type="number" value={density} onChange={(e) => setDensity(+e.target.value)} />
          </label>
          <pre>
            {`Mass = ${((r.volume * 1e-9 * density) * 1000).toFixed(2)} grams
Volume = ${r.volume.toFixed(2)} cubic millimeters
Surface area = ${r.area.toFixed(2)} square millimeters

Center of mass: ( millimeters )
	X = ${r.centerOfMass[0].toFixed(2)}
	Y = ${r.centerOfMass[1].toFixed(2)}
	Z = ${r.centerOfMass[2].toFixed(2)}`}
          </pre>
        </div>
      )}
    </Dialog>
  );
}

export function AboutDialog({ onClose }: { onClose: () => void }) {
  return (
    <Dialog title="About OpenCAD" onClose={onClose}>
      <p>
        <b>OpenCAD</b> — a modern, from-scratch parametric 3D CAD application with a familiar
        SOLIDWORKS-style workflow.
      </p>
      <p className="muted">
        Not affiliated with or endorsed by Dassault Systèmes. SOLIDWORKS is a trademark of Dassault Systèmes. Geometry kernel: Open CASCADE Technology (LGPL) via replicad.
      </p>
    </Dialog>
  );
}
