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
