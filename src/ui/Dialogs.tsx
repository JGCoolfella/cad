import { useEffect, useState, type ReactNode } from "react";
import type { App } from "../app/app";

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

export function MassPropsDialog({ app, onClose }: { app: App; onClose: () => void }) {
  const [r, setR] = useState<{ volume: number; area: number; centerOfMass: [number, number, number] } | null | "loading" | string>("loading");
  const [density, setDensity] = useState(Number(app.docProp("material.density") ?? 1000));
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

/** Confirm Delete dialog (same layout as SOLIDWORKS). */
export function ConfirmDeleteDialog({ app, id, onClose }: { app: App; id: string; onClose: () => void }) {
  const f = app.featureById(id);
  const [absorbed, setAbsorbed] = useState(false);
  if (!f) return null;
  const deps = app.dependentsOf(id);
  const abs = app.absorbedOf(id);
  const yes = () => {
    app.deleteFeature(id, { absorbed });
    onClose();
  };
  const kind = f.type === "sketch" ? "Sketch" : "Feature";
  return (
    <div className="modal-bg" onMouseDown={onClose}>
      <div className="modal confirm-delete" onMouseDown={(e) => e.stopPropagation()} onKeyDown={(e) => e.key === "Enter" && yes()}>
        <div className="modal-head">
          Confirm Delete
          <button onClick={onClose}>✕</button>
        </div>
        <div className="cd-body">
          <div className="cd-left">
            <div>Delete the following item?</div>
            <div className="cd-item">
              {f.name} ( {kind} )
            </div>
            <div>And all dependent items:</div>
            <div className="cd-list">
              {deps.map((d) => (
                <div key={d.id}>
                  {d.name} ( {d.type === "sketch" ? "Sketch" : "Feature"} )
                </div>
              ))}
              {absorbed && abs && (
                <div>
                  {abs.name} ( Sketch )
                </div>
              )}
            </div>
            <label className={`chk ${abs ? "" : "disabled"}`}>
              <input type="checkbox" disabled={!abs} checked={absorbed} onChange={(e) => setAbsorbed(e.target.checked)} /> Delete absorbed features
            </label>
            <label className="chk disabled">
              <input type="checkbox" disabled checked readOnly /> Delete child features
            </label>
            <label className="chk disabled">
              <input type="checkbox" disabled /> Delete equations
            </label>
          </div>
          <div className="cd-buttons">
            <button className="btn primary" autoFocus onClick={yes}>
              Yes
            </button>
            <button className="btn" disabled>
              Yes to All
            </button>
            <button className="btn" onClick={onClose}>
              No
            </button>
            <button className="btn" onClick={onClose}>
              Cancel
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
