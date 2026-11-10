import { useState } from "react";
import type { App } from "../app/app";
import { CMD, runCommand, type Ctx } from "../commands/commands";
import { Icon } from "./icons";

type Item = { big: string } | { stack: string[] } | { flyout: string[]; big?: boolean } | "|";

const TABS: Record<string, Item[]> = {
  Features: [
    { big: "feat.extrude" },
    { big: "feat.revolve" },
    { stack: ["feat.swept", "feat.lofted", "feat.boundary"] },
    "|",
    { big: "feat.cutExtrude" },
    { big: "feat.hole" },
    { big: "feat.cutRevolve" },
    { stack: ["feat.sweptCut", "feat.loftedCut", "feat.boundaryCut"] },
    "|",
    { flyout: ["feat.fillet", "feat.chamfer"], big: true },
    { big: "feat.linearPattern" },
    { stack: ["feat.rib", "feat.draft", "feat.shell"] },
    { stack: ["feat.wrap", "feat.intersect", "feat.mirror"] },
    "|",
    { flyout: ["feat.plane"], big: true },
    { big: "feat.curves" },
    { big: "feat.instant3d" },
  ],
  Sketch: [
    { flyout: ["sketch.sketch"], big: true },
    { big: "sketch.smartDimension" },
    "|",
    { stack: ["sketch.line", "sketch.rectangle", "sketch.slot"] },
    { stack: ["sketch.circle", "sketch.arc", "sketch.polygon"] },
    { stack: ["sketch.spline", "sketch.ellipse", "sketch.fillet"] },
    { stack: ["sketch.centerline", "sketch.point", "sketch.text"] },
    "|",
    { big: "sketch.trim" },
    { big: "sketch.convert" },
    { big: "sketch.offset" },
    { stack: ["sketch.mirror", "sketch.linearPattern", "sketch.move"] },
    "|",
    { big: "sketch.relations" },
    { stack: ["sketch.centerRectangle", "sketch.threePointArc", "sketch.tangentArc"] },
    { stack: ["sketch.toggleConstruction", "sketch.autoRelations", "sketch.grid"] },
  ],
  Evaluate: [{ big: "eval.measure" }, { big: "eval.massProps" }, "|", { big: "view.section" }],
};

const QUICK = [["home", "file.new"], ["file.save", "edit.undo"], ["file.open", "edit.redo"], ["edit.rebuild", "tools.options"]];

export function CommandManager({ app, ctx }: { app: App; ctx: Ctx }) {
  const [tab, setTab] = useState("Features");
  const [lastSketchAuto, setLastSketchAuto] = useState(false);
  // Switch to the Sketch tab automatically when a sketch is opened (SOLIDWORKS behaviour).
  if (!!app.sketch !== lastSketchAuto) {
    setLastSketchAuto(!!app.sketch);
    setTab(app.sketch ? "Sketch" : "Features");
  }
  return (
    <div className="cmdmgr">
      <div className="cm-row">
        <div className="quick">
          {QUICK.map((col, i) => (
            <div key={i} className="qcol">
              {col.map((id) => (id === "home" ? <button key={id} className="qbtn" title="Home"><Icon name="home" size={16} /></button> : <CmdBtn key={id} id={id} ctx={ctx} small />))}
            </div>
          ))}
        </div>
        <div className="cm-items">
          {TABS[tab].map((it, i) => {
            if (it === "|") return <div key={i} className="cm-sep" />;
            if ("big" in it && typeof it.big === "string") return <CmdBtn key={i} id={it.big} ctx={ctx} big />;
            if ("stack" in it)
              return (
                <div key={i} className="cm-stack">
                  {it.stack.map((id) => (
                    <CmdBtn key={id} id={id} ctx={ctx} row />
                  ))}
                </div>
              );
            if ("flyout" in it) return <CmdBtn key={i} id={it.flyout[0]} ctx={ctx} big flyout={it.flyout} />;
            return null;
          })}
        </div>
      </div>
      <div className="cm-tabs">
        {[...Object.keys(TABS), "Markup", "MBD Dimensions", "Add-Ins"].map((t) => (
          <span key={t} className={`cm-tab ${t === tab ? "on" : ""} ${TABS[t] ? "" : "muted"}`} onClick={() => TABS[t] && setTab(t)}>
            {t}
          </span>
        ))}
      </div>
    </div>
  );
}

export function CmdBtn({ id, ctx, big, row, small, flyout }: { id: string; ctx: Ctx; big?: boolean; row?: boolean; small?: boolean; flyout?: string[] }) {
  const [open, setOpen] = useState(false);
  const c = CMD[id];
  if (!c) return null;
  const disabled = c.todo || (c.enabled ? !c.enabled(ctx) : false);
  const checked = c.checked?.(ctx);
  const tip = `${c.label}${c.shortcut ? ` (${c.shortcut})` : ""}\n${c.todo ? "Not yet implemented" : c.tip ?? ""}`;
  const cls = `cbtn ${big ? "big" : row ? "row" : small ? "small" : ""} ${disabled ? "disabled" : ""} ${checked ? "checked" : ""}`;
  return (
    <div className="cbtn-wrap">
      <button className={cls} title={tip} onClick={() => !disabled && runCommand(id, ctx)}>
        <Icon name={c.icon ?? "generic"} size={big ? 24 : 16} />
        {!small && <span className="lbl">{c.id === "sketch.sketch" && ctx.app.sketch ? "Exit Sketch" : c.label}</span>}
      </button>
      {flyout && flyout.length > 1 && (
        <button className="flyarrow" onClick={() => setOpen(!open)}>
          ▾
        </button>
      )}
      {open && flyout && (
        <div className="flymenu" onMouseLeave={() => setOpen(false)}>
          {flyout.map((f) => (
            <div key={f} className="mi" onClick={() => (setOpen(false), runCommand(f, ctx))}>
              <span className="mi-icon">
                <Icon name={CMD[f]?.icon ?? "generic"} size={14} />
              </span>
              {CMD[f]?.label}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

const HEADS_UP: (string | { menu: string[]; icon: string; label: string })[] = [
  "view.zoomFit",
  "view.zoomArea",
  "view.previous",
  "view.section",
  { icon: "viewOrient", label: "View Orientation", menu: ["view.front", "view.back", "view.left", "view.right", "view.top", "view.bottom", "view.isometric", "view.trimetric", "view.dimetric", "view.normalTo"] },
  { icon: "displayStyle", label: "Display Style", menu: ["view.shadedEdges", "view.shaded", "view.hlr", "view.hlv", "view.wireframe"] },
  "view.hideShow",
  "view.appearance",
  "view.scene",
  { icon: "viewSettings", label: "View Settings", menu: ["view.perspective", "view.sketchRelations"] },
];

/** Heads-up View toolbar, top-centre of the graphics area. */
export function HeadsUpToolbar({ app, ctx }: { app: App; ctx: Ctx }) {
  const [open, setOpen] = useState<number | null>(null);
  void app;
  return (
    <div className="headsup">
      {HEADS_UP.map((h, i) => {
        if (typeof h === "string") {
          const c = CMD[h];
          return (
            <button key={i} className={`hu ${c.todo ? "disabled" : ""}`} title={c.todo ? `${c.label} (not yet implemented)` : c.label} onClick={() => runCommand(h, ctx)}>
              <Icon name={c.icon ?? "generic"} size={18} />
            </button>
          );
        }
        return (
          <div key={i} className="hu-wrap">
            <button className="hu" title={h.label} onClick={() => setOpen(open === i ? null : i)}>
              <Icon name={h.icon} size={18} />
              <span className="tiny">▾</span>
            </button>
            {open === i && (
              <div className="flymenu" onMouseLeave={() => setOpen(null)}>
                {h.menu.map((id) => {
                  const c = CMD[id];
                  return (
                    <div key={id} className="mi" onClick={() => (setOpen(null), runCommand(id, ctx))}>
                      <span className="mi-icon">{c.checked?.(ctx) ? "✓" : ""}</span>
                      {c.label}
                      {c.shortcut && <span className="sc">{c.shortcut}</span>}
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
