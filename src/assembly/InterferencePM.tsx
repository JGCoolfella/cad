/**
 * The Interference Detection PropertyManager (G1c), laid out as SOLIDWORKS':
 *
 * - Selected Components: the top-level assembly unless components are selected (picks in
 *   the graphics area or the fly-out tree add or remove them), and Calculate. One component
 *   is checked against everything; two or more against one another only.
 * - Results: `Interference1 - 226.19mm^3` with its two components under it, Ignore, and the
 *   volume drawn in red in the graphics area (selecting a result highlights its
 *   components). "No Interferences" when there are none.
 * - Options: Treat coincidence as interference, Show ignored interferences, Treat
 *   subassemblies as components, Make interfering parts transparent, Ignore hidden
 *   bodies/components; the multibody, fasteners and cosmetic-thread options are not
 *   implemented yet (greyed).
 * - Non-interfering Components: Wireframe, Hidden, Transparent, Use current.
 *
 * The exact common of each candidate pair is computed on the engine thread
 * (`oc_asm::evaluate`).
 */
import type { App } from "../app/app";
import { UNIT_INFO } from "../core/units";
import { Group } from "../ui/pmWidgets";
import { Icon } from "../ui/icons";
import type { InterferencePM as State } from "./session";

const OTHERS: [State["others"], string][] = [
  ["wireframe", "Wireframe"],
  ["hidden", "Hidden"],
  ["transparent", "Transparent"],
  ["current", "Use current"],
];

/** A volume as SOLIDWORKS lists it: `226.19mm^3` in the document's length unit. */
export function volumeLabel(app: App, mm3: number): string {
  const u = UNIT_INFO[app.doc.units];
  const v = mm3 * u.perMm ** 3;
  return `${v.toFixed(u.decimals)}${u.length}^3`;
}

export function InterferencePM({ app }: { app: App }) {
  const s = app.asm!;
  const pm = s.interferencePM!;
  const set = (f: () => void) => (f(), app.changed());
  const shown = s.shownInterferences();
  const option = (key: "coincidence" | "showIgnored" | "subassemblies" | "transparent", label: string) => (
    <label className="pm-row">
      <input type="checkbox" data-intf-option={key} checked={pm[key]} onChange={(e) => set(() => (pm[key] = e.target.checked))} /> {label}
    </label>
  );
  const todo = (label: string) => (
    <label className="pm-row muted" title="Not yet implemented">
      <input type="checkbox" disabled /> {label}
    </label>
  );
  return (
    <div className="pm" data-intf-pm="">
      <div className="pm-head">
        <Icon name="intersect" size={18} />
        <span className="pm-title">Interference Detection</span>
      </div>
      <div className="pm-buttons">
        <button title="OK" data-intf-ok="" onClick={() => s.closeInterference()}>
          <Icon name="check" size={18} />
        </button>
        <button title="Cancel (Esc)" onClick={() => s.closeInterference()}>
          <Icon name="cancel" size={18} />
        </button>
      </div>
      <Group title="Selected Components">
        <div className="pm-field" data-intf-selected="" title="Select components in the graphics area or the FeatureManager design tree">
          {pm.selected.length ? (
            pm.selected.map((p, k) => (
              <div key={p.join("/")} className="pm-item">
                {s.occurrenceName(p)}
                <span className="pm-item-x" title="Remove" onClick={() => set(() => (pm.selected.splice(k, 1), (pm.results = null)))}>
                  ×
                </span>
              </div>
            ))
          ) : (
            <div className="pm-item">{s.name}</div>
          )}
        </div>
        <button className="btn" data-intf-calculate="" disabled={pm.busy} onClick={() => void s.calculateInterference()}>
          {pm.busy ? "Calculating…" : "Calculate"}
        </button>
      </Group>
      <Group title="Results">
        <div className="pm-field intf-results" data-intf-results="">
          {pm.results === null ? (
            <div className="muted">Click Calculate.</div>
          ) : !shown.length ? (
            <div className="pm-item" data-intf-none="">
              No Interferences
            </div>
          ) : (
            shown.map(({ r, k, ignored }, n) => (
              <div key={k} className={`pm-item intf-result ${pm.active === k ? "sel" : ""} ${ignored ? "muted" : ""}`} data-intf-result={n} onClick={() => set(() => (pm.active = pm.active === k ? null : k))}>
                <div>
                  Interference{n + 1} - {r.coincident ? "Coincident" : volumeLabel(app, r.volume)}
                </div>
                <div className="intf-comp">{s.occurrenceName(r.pathA)}</div>
                <div className="intf-comp">{s.occurrenceName(r.pathB)}</div>
              </div>
            ))
          )}
        </div>
        <div className="pm-row">
          <button
            className="btn"
            data-intf-ignore=""
            disabled={pm.active === null}
            onClick={() => {
              const k = pm.active;
              if (k !== null) s.ignoreInterference(k, !s.shownInterferences().find((x) => x.k === k)?.ignored);
            }}
          >
            {pm.active !== null && s.shownInterferences().find((x) => x.k === pm.active)?.ignored ? "Unignore" : "Ignore"}
          </button>
          <label className="pm-row muted" title="Not yet implemented">
            <input type="checkbox" disabled /> Component view
          </label>
        </div>
      </Group>
      <Group title="Options">
        {option("coincidence", "Treat coincidence as interference")}
        {option("showIgnored", "Show ignored interferences")}
        {option("subassemblies", "Treat subassemblies as components")}
        {todo("Include multibody part interferences")}
        {option("transparent", "Make interfering parts transparent")}
        {todo("Create fasteners folder")}
        {todo("Create matching cosmetic threads folder")}
        <label className="pm-row">
          <input type="checkbox" data-intf-option="ignoreHidden" checked={!pm.includeHidden} onChange={(e) => set(() => ((pm.includeHidden = !e.target.checked), (pm.results = null)))} /> Ignore hidden bodies/components
        </label>
      </Group>
      <Group title="Non-interfering Components">
        {OTHERS.map(([k, label]) => (
          <label key={k} className="pm-row">
            <input type="radio" name="intf-others" data-intf-others={k} checked={pm.others === k} onChange={() => set(() => (pm.others = k))} /> {label}
          </label>
        ))}
      </Group>
    </div>
  );
}
