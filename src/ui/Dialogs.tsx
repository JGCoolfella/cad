import { referencedConfiguration } from "../assembly/configurations";
import { useEffect, useState, type ReactNode } from "react";
import type { App } from "../app/app";
import { MATERIALS, materialByName } from "../core/materials";
import { applyEquations, dimensionIndex } from "../core/equations";
import { UNIT_INFO } from "../core/units";
import { isNative } from "../native/tauri";
import { editionName, versionText } from "../app/version";
import { Icon } from "./icons";

export function Dialog({ title, children, onClose, footer, wide }: { title: string; children: ReactNode; onClose: () => void; footer?: ReactNode; wide?: boolean }) {
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

/**
 * Mass Properties of an assembly (G1b): every occurrence at its placement, each part with
 * its material's density (1000 kg/m³ without one, as SOLIDWORKS), suppressed components
 * left out.
 */
function AssemblyMassProps({ app, onClose }: { app: App; onClose: () => void }) {
  const [r, setR] = useState<{ mass: number; volume: number; area: number; com: [number, number, number]; count: number } | null | "loading" | string>("loading");
  // "Include hidden bodies/components" (🔍 SOLIDWORKS' default; Options > Assemblies).
  const [hidden, setHidden] = useState(app.opt("asm.massIncludeHidden") !== false);
  useEffect(() => {
    const s = app.asm!;
    const shown = new Set(s.occurrences().filter((o) => !o.hidden).map((o) => o.path.join("/")));
    s.massProps()
      .then((m) => {
        const list = m.occurrences.filter((o) => hidden || shown.has(o.path.join("/")));
        if (!list.length) return setR(null);
        let mass = 0, volume = 0, area = 0;
        const com: [number, number, number] = [0, 0, 0];
        for (const o of list) {
          const rho = materialByName(o.material ?? undefined)?.density ?? 1000;
          const g = o.volume * 1e-9 * rho * 1000;
          mass += g;
          volume += o.volume;
          area += o.area;
          o.center.forEach((c, k) => (com[k] += c * g));
        }
        setR({ mass, volume, area, com: com.map((c) => c / (mass || 1)) as [number, number, number], count: list.length });
      })
      .catch((e) => setR(String(e.message ?? e)));
  }, [app, hidden]);
  return (
    <Dialog title="Mass Properties" onClose={onClose}>
      {r === "loading" ? (
        <p>Calculating…</p>
      ) : typeof r === "string" ? (
        <p className="err">{r}</p>
      ) : !r ? (
        <p>No solid bodies.</p>
      ) : (
        <div className="massprops" data-mass-asm="">
          <div className="row muted">
            Mass properties of {app.asm!.name} ({r.count} components)
          </div>
          <label className="row">
            <input type="checkbox" data-mass-hidden="" checked={hidden} onChange={(e) => setHidden(e.target.checked)} /> Include hidden bodies/components
          </label>
          <pre>
            {`Mass = ${r.mass.toFixed(2)} grams
Volume = ${r.volume.toFixed(2)} cubic millimeters
Surface area = ${r.area.toFixed(2)} square millimeters

Center of mass: ( millimeters )
	X = ${r.com[0].toFixed(2)}
	Y = ${r.com[1].toFixed(2)}
	Z = ${r.com[2].toFixed(2)}`}
          </pre>
        </div>
      )}
    </Dialog>
  );
}

export function MassPropsDialog({ app, onClose }: { app: App; onClose: () => void }) {
  if (app.asm) return <AssemblyMassProps app={app} onClose={onClose} />;
  return <PartMassProps app={app} onClose={onClose} />;
}

function PartMassProps({ app, onClose }: { app: App; onClose: () => void }) {
  const [r, setR] = useState<{ volume: number; area: number; centerOfMass: [number, number, number] } | null | "loading" | string>("loading");
  // The part material's density, or 1000 kg/m³ when no material is set (as SOLIDWORKS).
  const mat = materialByName(app.doc.material);
  const [density, setDensity] = useState(mat?.density ?? Number(app.docProp("material.density") ?? 1000));
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
          <div className="row muted">Material: {mat ? mat.name : "<not specified>"}</div>
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

/** Tools > Equations: global variables and equations driving dimensions. */
export function EquationsDialog({ app, onClose }: { app: App; onClose: () => void }) {
  const [globals, setGlobals] = useState(() => (app.doc.globals ?? []).map((g) => ({ ...g })));
  const [eqs, setEqs] = useState(() => (app.doc.equations ?? []).map((e) => ({ ...e })));
  const names = [...dimensionIndex(app.doc).keys()].sort();
  // Live "Evaluates to" with the edited rows.
  const preview = (() => {
    const doc = structuredClone(app.doc);
    doc.globals = globals.filter((g) => g.name.trim());
    doc.equations = eqs.filter((e) => e.target);
    return applyEquations(doc, UNIT_INFO[app.doc.units].perMm);
  })();
  const shown = (k: string) => (preview.errors[k] ? <span className="err">{preview.errors[k]}</span> : k in preview.values ? +preview.values[k].toFixed(6) : "");
  const apply = () => {
    app.checkpoint("Equations");
    app.doc.globals = globals.filter((g) => g.name.trim());
    app.doc.equations = eqs.filter((e) => e.target && e.expr.trim());
    app.rebuild();
  };
  return (
    <Dialog
      title="Equations, Global Variables, and Dimensions"
      wide
      onClose={onClose}
      footer={
        <>
          <button className="btn" onClick={() => (apply(), onClose())}>
            OK
          </button>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn" onClick={apply}>
            Rebuild
          </button>
        </>
      }
    >
      <table className="eqtable">
        <thead>
          <tr>
            <th>Name</th>
            <th>Value / Equation</th>
            <th>Evaluates to</th>
            <th />
          </tr>
        </thead>
        <tbody>
          <tr className="eqsec">
            <td colSpan={4}>Global Variables</td>
          </tr>
          {globals.map((g, i) => (
            <tr key={"g" + i}>
              <td>
                <input value={g.name} onChange={(e) => setGlobals(globals.map((x, k) => (k === i ? { ...x, name: e.target.value } : x)))} onKeyDown={(e) => e.stopPropagation()} />
              </td>
              <td>
                <input value={g.expr} onChange={(e) => setGlobals(globals.map((x, k) => (k === i ? { ...x, expr: e.target.value } : x)))} onKeyDown={(e) => e.stopPropagation()} />
              </td>
              <td>{shown(g.name)}</td>
              <td>
                <button onClick={() => setGlobals(globals.filter((_, k) => k !== i))}>✕</button>
              </td>
            </tr>
          ))}
          <tr>
            <td colSpan={4}>
              <button className="btn small" onClick={() => setGlobals([...globals, { name: `Var${globals.length + 1}`, expr: "10" }])}>
                Add global variable
              </button>
            </td>
          </tr>
          <tr className="eqsec">
            <td colSpan={4}>Equations</td>
          </tr>
          {eqs.map((q, i) => (
            <tr key={"e" + i}>
              <td>
                <select value={q.target} onChange={(e) => setEqs(eqs.map((x, k) => (k === i ? { ...x, target: e.target.value } : x)))}>
                  <option value="">(dimension)</option>
                  {names.map((n) => (
                    <option key={n} value={n}>
                      "{n}"
                    </option>
                  ))}
                </select>
              </td>
              <td>
                <input value={q.expr} placeholder='e.g. "Width" / 2' onChange={(e) => setEqs(eqs.map((x, k) => (k === i ? { ...x, expr: e.target.value } : x)))} onKeyDown={(e) => e.stopPropagation()} />
              </td>
              <td>{q.target ? shown(q.target) : ""}</td>
              <td>
                <button onClick={() => setEqs(eqs.filter((_, k) => k !== i))}>✕</button>
              </td>
            </tr>
          ))}
          <tr>
            <td colSpan={4}>
              <button className="btn small" onClick={() => setEqs([...eqs, { target: "", expr: "" }])}>
                Add equation
              </button>
            </td>
          </tr>
        </tbody>
      </table>
      <p className="muted small">Use names in double quotes ("Width", "D1@Sketch1"); + - * / ^ ( ), sin cos tan (degrees), sqrt, abs, int, pi.</p>
    </Dialog>
  );
}

/** Edit Material: pick a material from the library (categories on the left, properties on the right). */
export function MaterialDialog({ app, onClose }: { app: App; onClose: () => void }) {
  const [sel, setSel] = useState<string | null>(app.doc.material ?? null);
  const [open, setOpen] = useState<Record<string, boolean>>(() => {
    const cur = materialByName(app.doc.material);
    return cur ? { [cur.category]: true } : {};
  });
  const m = materialByName(sel);
  const cats = [...new Set(MATERIALS.map((x) => x.category))];
  const fmt = (v: number, unit: string) => (v ? `${v >= 1e6 ? (v / 1e6).toFixed(2) : v.toFixed(2)} ${unit}` : "—");
  return (
    <Dialog
      title="Material"
      wide
      onClose={onClose}
      footer={
        <>
          <button
            className="btn"
            disabled={!m}
            onClick={() => {
              app.setMaterial(sel);
              onClose();
            }}
          >
            Apply
          </button>
          <button
            className="btn"
            onClick={() => {
              app.setMaterial(null);
              onClose();
            }}
          >
            Remove Material
          </button>
          <button className="btn" onClick={onClose}>
            Close
          </button>
        </>
      }
    >
      <div className="material">
        <div className="mat-tree">
          <div className="muted">Materials</div>
          {cats.map((c) => (
            <div key={c}>
              <div className="mat-cat" onClick={() => setOpen({ ...open, [c]: !open[c] })}>
                {open[c] ? "▾" : "▸"} {c}
              </div>
              {open[c] &&
                MATERIALS.filter((x) => x.category === c).map((x) => (
                  <div key={x.name} className={`mat-item ${sel === x.name ? "on" : ""}`} onClick={() => setSel(x.name)} onDoubleClick={() => (app.setMaterial(x.name), onClose())}>
                    {x.name}
                  </div>
                ))}
            </div>
          ))}
        </div>
        <div className="mat-props">
          {m ? (
            <table>
              <tbody>
                <tr><td>Name:</td><td>{m.name}</td></tr>
                <tr><td>Category:</td><td>{m.category}</td></tr>
                <tr><td>Mass Density</td><td>{m.density} kg/m^3</td></tr>
                <tr><td>Elastic Modulus</td><td>{fmt(m.modulus, "N/mm^2")}</td></tr>
                <tr><td>Poisson's Ratio</td><td>{m.poisson}</td></tr>
                <tr><td>Yield Strength</td><td>{fmt(m.yield, "N/mm^2")}</td></tr>
              </tbody>
            </table>
          ) : (
            <p className="muted">Select a material.</p>
          )}
        </div>
      </div>
    </Dialog>
  );
}

export function AboutDialog({ onClose }: { onClose: () => void }) {
  return (
    <Dialog title="About OpenCAD" onClose={onClose}>
      <p>
        <b>OpenCAD</b> — a modern, from-scratch parametric 3D CAD application with a familiar
        workflow.
      </p>
      <p className="about-version">
        {versionText()}
        <br />
        {editionName()}
      </p>
      <p className="muted">
        {isNative()
          ? "Geometry kernel: Open CASCADE Technology 8.0.1 (LGPL 2.1 with the Open CASCADE exception), as replaceable DLLs in the program's folder. Third-party licences: THIRD_PARTY_NOTICES.txt in the same folder."
          : "Geometry kernel: Open CASCADE Technology (LGPL) via replicad."}
      </p>
    </Dialog>
  );
}

/**
 * "Save changes to <part>?" before closing the program, File > New or File > Open discards
 * unsaved changes (SOLIDWORKS: Save / Don't Save / Cancel). Enter saves, Esc cancels.
 */
export function SaveChangesDialog({ name, onSave, onDontSave, onCancel }: { name: string; onSave: () => void; onDontSave: () => void; onCancel: () => void }) {
  return (
    <div className="modal-bg" onMouseDown={onCancel}>
      <div
        className="modal save-changes"
        role="alertdialog"
        aria-label="Save changes"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") onCancel();
          e.stopPropagation();
        }}
      >
        <div className="modal-head">
          OpenCAD
          <button onClick={onCancel}>✕</button>
        </div>
        <div className="sc-body">
          <div className="sc-q">Save changes to {name}?</div>
          <div className="muted">Your changes will be lost if you don't save them.</div>
        </div>
        <div className="sc-buttons">
          <button className="btn primary" autoFocus onClick={onSave}>
            Save
          </button>
          <button className="btn" onClick={onDontSave}>
            Don't Save
          </button>
          <button className="btn" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Component Properties (G1c), as SOLIDWORKS' dialog: Component Name (the `<n>` instance
 * number kept), Instance Id, Full Name, Model Document Path, Referenced configuration (G1e:
 * any configuration of the component's document), Suppression state (Suppressed /
 * Resolved), and with several assembly configurations whether these apply to This
 * configuration or All configurations. Solve as Flexible is not implemented yet (greyed).
 * OK applies, one undo step.
 */
export function ComponentPropsDialog({ app, id, onClose }: { app: App; id: string; onClose: () => void }) {
  const s = app.asm!;
  const inst = s.doc.instances.find((i) => i.id === id);
  const n = inst ? /<(\d+)>$/.exec(inst.name)?.[1] : undefined;
  const [name, setName] = useState(inst ? inst.name.replace(/<\d+>$/, "") : "");
  const [suppressed, setSuppressed] = useState(!!inst?.suppressed);
  const [config, setConfig] = useState(inst ? referencedConfiguration(s.doc, inst) : "Default");
  const [scope, setScope] = useState<"this" | "all">("this");
  if (!inst) return null;
  const comp = s.componentOf(inst);
  const configs = s.built?.components.find((c) => c.id === inst.component)?.configs ?? [comp?.config ?? "Default"];
  const many = (s.doc.configurations?.list.length ?? 1) > 1;
  const path = s.componentFile(id)?.path ?? comp?.absPath ?? comp?.path ?? "";
  return (
    <Dialog title="Component Properties" onClose={onClose}>
      <div className="massprops comp-props" data-comp-props={id}>
        <label className="row">
          Component Name <input data-comp-name="" value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        <div className="row">Instance Id: {n ?? "-"}</div>
        <div className="row">Full Name: {name.trim() ? `${name.trim()}${n ? `<${n}>` : ""}` : inst.name}</div>
        <div className="row muted">Model Document Path: {path}</div>
        <label className="row">
          Referenced configuration{" "}
          <select data-comp-config="" value={config} onChange={(e) => setConfig(e.target.value)}>
            {[...new Set([...configs, config])].map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </label>
        <div className="row">Suppression state</div>
        <label className="row">
          <input type="radio" name="comp-supp" data-comp-suppressed="" checked={suppressed} onChange={() => setSuppressed(true)} /> Suppressed
        </label>
        <label className="row">
          <input type="radio" name="comp-supp" data-comp-resolved="" checked={!suppressed} onChange={() => setSuppressed(false)} /> Resolved
        </label>
        <label className="row muted" title="Not yet implemented">
          <input type="checkbox" disabled /> Solve as Flexible
        </label>
        {many && (
          <label className="row">
            Apply to{" "}
            <select data-comp-scope="" value={scope} onChange={(e) => setScope(e.target.value === "all" ? "all" : "this")}>
              <option value="this">This configuration</option>
              <option value="all">All configurations</option>
            </select>
          </label>
        )}
        <div className="sc-buttons">
          <button
            className="btn primary"
            data-comp-ok=""
            onClick={() => {
              s.setProperties(id, { name, suppressed, config, scope });
              onClose();
            }}
          >
            OK
          </button>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
        </div>
      </div>
    </Dialog>
  );
}

/** A document listed by the Save Modified Documents dialog. */
export interface ModifiedDoc {
  key: string;
  /** File name as listed (`plate.ocpart`). */
  name: string;
  kind: "part" | "assembly";
  /** Always saved (the assembly being saved); its box cannot be cleared. */
  locked?: boolean;
}

/**
 * SOLIDWORKS' Save Modified Documents dialog (G1c, owner decision 2): the modified
 * documents with a box each. Save All saves every one; clearing a box turns it into Save
 * Selected. When closing (`mode` "close") Don't Save discards them all; when saving the
 * assembly (`mode` "save") the assembly itself is always saved. Wording 🔍.
 */
export function SaveModifiedDialog({ docs, mode, onSave, onDontSave, onCancel }: { docs: ModifiedDoc[]; mode: "save" | "close"; onSave: (keys: string[]) => void; onDontSave: () => void; onCancel: () => void }) {
  const [checked, setChecked] = useState<Set<string>>(() => new Set(docs.map((d) => d.key)));
  const all = docs.every((d) => checked.has(d.key));
  return (
    <div className="modal-bg" onMouseDown={onCancel}>
      <div
        className="modal save-changes save-modified"
        role="alertdialog"
        aria-label="Save Modified Documents"
        data-save-modified={mode}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") onCancel();
          e.stopPropagation();
        }}
      >
        <div className="modal-head">
          Save Modified Documents
          <button onClick={onCancel}>✕</button>
        </div>
        <div className="sc-body">
          <div className="sc-q">{mode === "close" ? "Save changes to the modified documents?" : "The following documents have been modified:"}</div>
          <div className="sm-list">
            {docs.map((d) => (
              <label key={d.key} className="pm-row" data-save-doc={d.name}>
                <input
                  type="checkbox"
                  checked={d.locked || checked.has(d.key)}
                  disabled={d.locked}
                  onChange={(e) => {
                    const next = new Set(checked);
                    if (e.target.checked) next.add(d.key);
                    else next.delete(d.key);
                    setChecked(next);
                  }}
                />
                <Icon name={d.kind === "assembly" ? "assembly" : "part"} size={14} /> {d.name}
              </label>
            ))}
          </div>
          {mode === "close" && <div className="muted">Your changes will be lost if you don't save them.</div>}
        </div>
        <div className="sc-buttons">
          <button className="btn primary" autoFocus data-save-modified-save="" onClick={() => onSave(docs.filter((d) => d.locked || checked.has(d.key)).map((d) => d.key))}>
            {all ? "Save All" : "Save Selected"}
          </button>
          {mode === "close" && (
            <button className="btn" data-save-modified-dontsave="" onClick={onDontSave}>
              Don't Save
            </button>
          )}
          <button className="btn" data-save-modified-cancel="" onClick={onCancel}>
            Cancel
          </button>
        </div>
      </div>
    </div>
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
