import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { App } from "../app/app";
import type { Viewport } from "../render/viewport";
import { COLOR_ITEMS, DOCUMENT_PAGES, FEATURE_COLOR_ITEMS, SYSTEM_PAGES, type Ctl, type Page } from "../app/optionsSchema";
import { DEFAULT_OPTIONS } from "../app/settings";
import type { UnitSystem } from "../core/types";
import { UNIT_INFO } from "../core/units";

type Scope = "system" | "document";

/**
 * Tools > Options. Edits are buffered and applied on OK (Cancel discards), like
 * SOLIDWORKS. Controls without an effect yet are labelled "not yet active".
 */
export function OptionsDialog({ app, vp, onClose, initialPage }: { app: App; vp: Viewport | null; onClose: () => void; initialPage?: string }) {
  const [scope, setScope] = useState<Scope>(initialPage?.startsWith("doc.") ? "document" : "system");
  const [pageId, setPageId] = useState(initialPage ?? "general");
  const [sys, setSys] = useState<Record<string, unknown>>({});
  const [doc, setDoc] = useState<Record<string, unknown>>({});
  const [units, setUnits] = useState<UnitSystem>(app.doc.units);
  const [search, setSearch] = useState("");
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const pages = scope === "system" ? SYSTEM_PAGES : DOCUMENT_PAGES;
  const all = useMemo(() => flatten(pages), [pages]);
  const page = all.find((p) => p.id === pageId) ?? all[0];

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose]);

  const get = (s: Scope, key: string): unknown => {
    if (s === "system") return key in sys ? sys[key] : app.opt(key);
    return key in doc ? doc[key] : app.docProp(key);
  };
  const set = (s: Scope, key: string, v: unknown) => (s === "system" ? setSys((x) => ({ ...x, [key]: v })) : setDoc((x) => ({ ...x, [key]: v })));

  const ok = () => {
    for (const [k, v] of Object.entries(sys)) applySystem(app, k, v);
    const docChanged = Object.keys(doc).length > 0 || units !== app.doc.units;
    if (docChanged) app.checkpoint();
    for (const [k, v] of Object.entries(doc)) app.setDocProp(k, v);
    if (units !== app.doc.units) app.doc.units = units;
    if (vp) {
      vp.renderer?.setPixelRatio(window.devicePixelRatio * Number(app.opt("performance.pixelRatio") ?? 1));
      vp.invalidate();
    }
    if ("iq.shaded" in doc) app.rebuild();
    app.changed();
    onClose();
  };

  const reset = () => {
    if (scope === "system") {
      if (!confirm("Reset all system options to their default values?")) return;
      app.setOptions((o) => Object.assign(o, structuredClone(DEFAULT_OPTIONS)));
      setSys({});
    } else {
      setDoc({});
    }
  };

  // Search: show every control whose label matches, across all pages of both tabs.
  const searchHits = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (q.length < 2) return null;
    const hits: { scope: Scope; page: Page; ctl: Ctl }[] = [];
    for (const [s, ps] of [["system", SYSTEM_PAGES], ["document", DOCUMENT_PAGES]] as const)
      for (const p of flatten(ps)) walk(p.items, (c) => "label" in c && typeof c.label === "string" && c.label.toLowerCase().includes(q) && c.t !== "group" && hits.push({ scope: s, page: p, ctl: c }));
    return hits;
  }, [search]);

  const title = `${scope === "system" ? "System Options" : "Document Properties"} - ${page.label}`;
  return (
    <div className="modal-bg" onMouseDown={onClose}>
      <div className="modal options" onMouseDown={(e) => e.stopPropagation()}>
        <div className="modal-head">
          {title}
          <button onClick={onClose} title="Close">
            ✕
          </button>
        </div>
        <div className="opt-tabs">
          <span className={scope === "system" ? "on" : ""} onClick={() => (setScope("system"), setPageId("general"))}>
            System Options
          </span>
          <span className={scope === "document" ? "on" : ""} onClick={() => (setScope("document"), setPageId("doc.drafting"))}>
            Document Properties
          </span>
          <span className="grow" />
          <span className="opt-search">
            ⚙ <input placeholder="Search Options" value={search} onChange={(e) => setSearch(e.target.value)} />
          </span>
        </div>
        <div className="opt">
          <div className="opt-left">
            <div className="opt-nav">
              <Tree pages={pages} depth={0} current={page.id} collapsed={collapsed} setCollapsed={setCollapsed} onPick={(id) => (setSearch(""), setPageId(id))} />
            </div>
            <button className="btn" onClick={reset}>
              Reset...
            </button>
          </div>
          <div className="opt-page">
            {searchHits ? (
              <>
                <div className="sub">Search results</div>
                {searchHits.map((h, i) => (
                  <div key={i} className="search-hit">
                    <a
                      onClick={() => {
                        setScope(h.scope);
                        setPageId(h.page.id);
                        setSearch("");
                      }}
                    >
                      {h.scope === "system" ? "System Options" : "Document Properties"} › {h.page.label}
                    </a>
                    <Control c={h.ctl} scope={h.scope} get={get} set={set} />
                  </div>
                ))}
                {!searchHits.length && <div className="muted">No matching options.</div>}
              </>
            ) : (
              <PageBody page={page} scope={scope} get={get} set={set} app={app} units={units} setUnits={setUnits} goto={(id) => (setScope(id.startsWith("doc.") ? "document" : "system"), setPageId(id))} />
            )}
          </div>
        </div>
        <div className="modal-foot">
          <span className="grow" />
          <button className="btn primary" onClick={ok}>
            OK
          </button>
          <button className="btn" onClick={onClose}>
            Cancel
          </button>
          <button className="btn" onClick={() => window.open("https://github.com/JGCoolfella/cad/blob/main/docs/FEATURE_SPEC.md", "_blank")}>
            Help
          </button>
        </div>
      </div>
    </div>
  );
}

function applySystem(app: App, key: string, v: unknown) {
  if (key.startsWith("color.")) {
    const item = COLOR_ITEMS.find((c) => `color.${c.key}` === key);
    if (item?.live) {
      const [k, idx] = item.live.split(".");
      app.setOptions((o) => {
        const colors = o.colors as unknown as Record<string, unknown>;
        if (idx !== undefined) (colors[k] as string[])[Number(idx)] = v as string;
        else colors[k] = v;
      });
    }
    app.setOpt(key, v);
    return;
  }
  if (key === "view.transition") app.setOptions((o) => (o.view.animationSpeed = app.transition()));
  if (key === "ui.background") app.setOptions((o) => (o.theme = v === "dark" || v === "mediumDark" ? "dark" : "light"));
  app.setOpt(key, v);
}

function flatten(ps: Page[]): Page[] {
  return ps.flatMap((p) => [p, ...(p.children ? flatten(p.children) : [])]);
}

function walk(items: Ctl[], f: (c: Ctl) => unknown) {
  for (const c of items) {
    f(c);
    if (c.t === "group") walk(c.items, f);
  }
}

function Tree({ pages, depth, current, onPick, collapsed, setCollapsed }: { pages: Page[]; depth: number; current: string; onPick: (id: string) => void; collapsed: Record<string, boolean>; setCollapsed: (c: Record<string, boolean>) => void }) {
  return (
    <>
      {pages.map((p) => (
        <div key={p.id}>
          <div className={`opt-node ${p.id === current ? "on" : ""}`} style={{ paddingLeft: 4 + depth * 14 }} onClick={() => onPick(p.id)}>
            {p.children ? (
              <span
                className="tw"
                onClick={(e) => {
                  e.stopPropagation();
                  setCollapsed({ ...collapsed, [p.id]: !collapsed[p.id] });
                }}
              >
                {collapsed[p.id] ? "⊞" : "⊟"}
              </span>
            ) : depth ? (
              <span className="tw dim">┄</span>
            ) : null}
            {p.label}
          </div>
          {p.children && !collapsed[p.id] && <Tree pages={p.children} depth={depth + 1} current={current} onPick={onPick} collapsed={collapsed} setCollapsed={setCollapsed} />}
        </div>
      ))}
    </>
  );
}

type Get = (s: Scope, key: string) => unknown;
type Set = (s: Scope, key: string, v: unknown) => void;

function PageBody({ page, scope, get, set, app, units, setUnits, goto }: { page: Page; scope: Scope; get: Get; set: Set; app: App; units: UnitSystem; setUnits: (u: UnitSystem) => void; goto: (id: string) => void }) {
  return (
    <div className="opt-controls">
      {page.items.map((c, i) => {
        if (c.t === "colorScheme") return <ColorScheme key={i} get={get} set={set} />;
        if (c.t === "unitsTable") return <UnitsPage key={i} units={units} setUnits={setUnits} get={get} set={set} />;
        if (c.t === "featureColors") return <FeatureColors key={i} get={get} set={set} />;
        return <Control key={i} c={c} scope={scope} get={get} set={set} goto={goto} />;
      })}
      {void app}
    </div>
  );
}

function Inactive({ live }: { live?: boolean }) {
  return live ? null : (
    <span className="inactive" title="Stored with your settings; not yet active in OpenCAD">
      •
    </span>
  );
}

function Control({ c, scope, get, set, goto }: { c: Ctl; scope: Scope; get: Get; set: Set; goto?: (id: string) => void }) {
  switch (c.t) {
    case "group":
      return (
        <fieldset className="opt-group">
          <legend>{c.label}</legend>
          {c.items.map((x, i) => (
            <Control key={i} c={x} scope={scope} get={get} set={set} goto={goto} />
          ))}
        </fieldset>
      );
    case "check":
      return (
        <label className={`chk ${c.disabled ? "disabled" : ""}`} style={{ paddingLeft: (c.indent ?? 0) * 18 }}>
          <input type="checkbox" disabled={c.disabled} checked={!!get(scope, c.key)} onChange={(e) => set(scope, c.key, e.target.checked)} /> {c.label}
          <Inactive live={c.live} />
        </label>
      );
    case "radio":
      return (
        <div className="opt-radio">
          {c.label && <div className="lbl">{c.label}</div>}
          {c.options.map(([v, l]) => (
            <label key={v} className="chk">
              <input type="radio" checked={get(scope, c.key) === v} onChange={() => set(scope, c.key, v)} /> {l}
            </label>
          ))}
          <Inactive live={c.live} />
        </div>
      );
    case "select":
      return (
        <label className="row">
          {c.label && <span>{c.label}</span>}
          <select value={String(get(scope, c.key))} onChange={(e) => set(scope, c.key, e.target.value)}>
            {c.options.map(([v, l]) => (
              <option key={v} value={v}>
                {l}
              </option>
            ))}
          </select>
          <Inactive live={c.live} />
        </label>
      );
    case "number":
      return (
        <label className="row">
          {c.label && <span>{c.label}</span>}
          <span className="spinbox">
            <input type="number" value={Number(get(scope, c.key))} step={c.step ?? 1} min={c.min} max={c.max} onChange={(e) => set(scope, c.key, Number(e.target.value))} />
            {c.unit && <span className="unit">{c.unit}</span>}
          </span>
          <Inactive live={c.live} />
        </label>
      );
    case "slider":
      return (
        <div className="row slider">
          {c.label && <span className="lbl">{c.label}</span>}
          <span className="slider-box">
            <span className="ends">
              <span>{c.left}</span>
              <span>{c.right}</span>
            </span>
            <input type="range" min={c.min} max={c.max} step={c.step} value={Number(get(scope, c.key))} onChange={(e) => set(scope, c.key, Number(e.target.value))} />
          </span>
          <Inactive live={c.live} />
        </div>
      );
    case "text":
      return (
        <label className="row">
          {c.label && <span>{c.label}</span>}
          <input className="wide" value={String(get(scope, c.key) ?? "")} onChange={(e) => set(scope, c.key, e.target.value)} />
        </label>
      );
    case "color":
      return (
        <label className="row">
          <input type="color" value={String(get(scope, c.key))} onChange={(e) => set(scope, c.key, e.target.value)} />
          <span>{c.label}</span>
          <Inactive live={c.live} />
        </label>
      );
    case "button":
      return (
        <button className="btn opt-btn" disabled={!c.action} onClick={() => c.action?.startsWith("page:") && goto?.(c.action.slice(5))}>
          {c.label}
        </button>
      );
    case "note":
      return <p className="muted">{c.text}</p>;
    default:
      return null;
  }
}

function ColorScheme({ get, set }: { get: Get; set: Set }) {
  const [sel, setSel] = useState(COLOR_ITEMS[0].key);
  const item = COLOR_ITEMS.find((c) => c.key === sel)!;
  const colorOf = (k: string) => String(get("system", `color.${k}`) ?? COLOR_ITEMS.find((c) => c.key === k)?.def);
  const input = useRef<HTMLInputElement>(null);
  return (
    <div className="opt-controls">
      <label className="row">
        <span>Background:</span>
        <select value={String(get("system", "ui.background") ?? "light")} onChange={(e) => set("system", "ui.background", e.target.value)}>
          <option value="light">Light</option>
          <option value="mediumLight">Medium light</option>
          <option value="mediumDark">Medium dark</option>
          <option value="dark">Dark</option>
        </select>
      </label>
      <div className="lbl">Current color scheme:</div>
      <div className="row">
        <select value="default" onChange={() => {}}>
          <option value="default">Default</option>
        </select>
        <button className="btn" disabled>
          Delete
        </button>
      </div>
      <fieldset className="opt-group">
        <legend>Color scheme settings</legend>
        <div className="scheme">
          <div className="scheme-list">
            {COLOR_ITEMS.map((c) => (
              <div key={c.key} className={`scheme-row ${c.key === sel ? "on" : ""}`} onClick={() => setSel(c.key)} onDoubleClick={() => input.current?.click()}>
                <span>{c.label}</span>
                {!c.live && <Inactive />}
                <span className="sw" style={{ background: colorOf(c.key) }} />
              </div>
            ))}
          </div>
          <div className="scheme-side">
            <div className="swatch" style={{ background: colorOf(item.key) }} />
            <button className="btn" onClick={() => input.current?.click()}>
              Edit...
            </button>
            <input ref={input} type="color" className="hidden-color" value={colorOf(item.key)} onChange={(e) => set("system", `color.${item.key}`, e.target.value)} />
          </div>
        </div>
        <div className="opt-radio">
          <div className="lbl">Background appearance:</div>
          {(
            [
              ["scene", "Use document scene background (recommended)"],
              ["plain", "Plain (Viewport Background color above)"],
              ["gradient", "Gradient (Top/Bottom Gradient colors above)"],
            ] as const
          ).map(([v, l]) => (
            <label key={v} className="chk">
              <input type="radio" checked={(get("system", "ui.bgMode") ?? "scene") === v} onChange={() => set("system", "ui.bgMode", v)} /> {l}
            </label>
          ))}
        </div>
        <div className="row">
          <button
            className="btn"
            onClick={() => {
              for (const c of COLOR_ITEMS) set("system", `color.${c.key}`, c.def);
            }}
          >
            Reset Colors To Defaults
          </button>
        </div>
      </fieldset>
    </div>
  );
}

function FeatureColors({ get, set }: { get: Get; set: Set }) {
  const [sel, setSel] = useState(FEATURE_COLOR_ITEMS[1].key);
  const item = FEATURE_COLOR_ITEMS.find((c) => c.key === sel)!;
  const colorOf = (k: string) => String(get("document", `fc.${k}`) ?? FEATURE_COLOR_ITEMS.find((c) => c.key === k)?.def);
  const input = useRef<HTMLInputElement>(null);
  return (
    <fieldset className="opt-group">
      <legend>Model/feature colors</legend>
      <div className="scheme">
        <div className="scheme-list short">
          {FEATURE_COLOR_ITEMS.map((c) => (
            <div key={c.key} className={`scheme-row ${c.key === sel ? "on" : ""}`} onClick={() => setSel(c.key)}>
              <span>{c.label}</span>
              {!c.live && <Inactive />}
              <span className="sw" style={{ background: colorOf(c.key) }} />
            </div>
          ))}
        </div>
        <div className="scheme-side">
          <div className="swatch" style={{ background: colorOf(item.key) }} />
          <button className="btn" onClick={() => input.current?.click()}>
            Edit...
          </button>
          <input ref={input} type="color" className="hidden-color" value={colorOf(item.key)} onChange={(e) => set("document", `fc.${item.key}`, e.target.value)} />
        </div>
      </div>
      <button className="btn" onClick={() => FEATURE_COLOR_ITEMS.forEach((c) => set("document", `fc.${c.key}`, c.def))}>
        Reset Colors To Defaults
      </button>
    </fieldset>
  );
}

function UnitsPage({ units, setUnits, get, set }: { units: UnitSystem; setUnits: (u: UnitSystem) => void; get: Get; set: Set }) {
  const order: UnitSystem[] = ["MKS", "CGS", "MMGS", "IPS"];
  const len = UNIT_INFO[units];
  const unitName: Record<string, string> = { mm: "millimeters", cm: "centimeters", m: "meters", in: "inches" };
  const rows: [string, string, string][] = [
    ["Length", unitName[len.length], "units.lengthDecimals"],
    ["Dual Dimension Length", "inches", "units.dualDecimals"],
    ["Angle", "degrees", "units.angleDecimals"],
  ];
  return (
    <>
      <fieldset className="opt-group">
        <legend>Unit system</legend>
        {order.map((u) => (
          <label key={u} className="chk">
            <input type="radio" checked={units === u} onChange={() => setUnits(u)} /> {UNIT_INFO[u].label}
          </label>
        ))}
        <label className="chk disabled">
          <input type="radio" disabled /> Custom
        </label>
      </fieldset>
      <table className="units">
        <thead>
          <tr>
            <th>Type</th>
            <th>Unit</th>
            <th>Decimals</th>
          </tr>
        </thead>
        <tbody>
          <tr className="sec">
            <td colSpan={3}>Basic Units</td>
          </tr>
          {rows.map(([t, u, k]) => (
            <tr key={t}>
              <td>{t}</td>
              <td className="muted">{u}</td>
              <td>
                <select value={String(get("document", k) ?? 2)} onChange={(e) => set("document", k, Number(e.target.value))}>
                  {[0, 1, 2, 3, 4, 5, 6, 7, 8].map((n) => (
                    <option key={n} value={n}>
                      {n === 0 ? "None" : "." + "123456789".slice(0, n)}
                    </option>
                  ))}
                </select>
              </td>
            </tr>
          ))}
          <tr className="sec">
            <td colSpan={3}>Mass/Section Properties</td>
          </tr>
          <tr>
            <td>Length</td>
            <td className="muted">{unitName[len.length]}</td>
            <td>.12</td>
          </tr>
          <tr>
            <td>Mass</td>
            <td className="muted">{units === "MKS" ? "kilograms" : units === "IPS" ? "pounds" : "grams"}</td>
            <td />
          </tr>
          <tr>
            <td>Per Unit Volume</td>
            <td className="muted">{unitName[len.length]}^3</td>
            <td />
          </tr>
          <tr className="sec">
            <td colSpan={3}>Motion Units</td>
          </tr>
          <tr>
            <td>Time</td>
            <td className="muted">second</td>
            <td>.12</td>
          </tr>
        </tbody>
      </table>
      <fieldset className="opt-group">
        <legend>Decimal rounding</legend>
        {(
          [
            ["halfAway", "Round half away from zero"],
            ["halfToward", "Round half towards zero"],
            ["halfEven", "Round half to even"],
            ["truncate", "Truncate without rounding"],
          ] as const
        ).map(([v, l]) => (
          <label key={v} className="chk">
            <input type="radio" checked={(get("document", "units.rounding") ?? "halfAway") === v} onChange={() => set("document", "units.rounding", v)} /> {l}
          </label>
        ))}
      </fieldset>
    </>
  );
}

export type { ReactNode };
