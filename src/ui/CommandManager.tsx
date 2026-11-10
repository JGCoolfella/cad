import { useCallback, useState } from "react";
import { ViewOrientationFlyout } from "./ViewOrientation";
import type { App } from "../app/app";
import { CMD, runCommand, type Ctx } from "../commands/commands";
import { Icon } from "./icons";

type Item = { big: string } | { stack: string[] } | { flyout: string[]; big?: boolean; label?: string } | "|";

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
    { flyout: ["feat.plane", "feat.axis", "feat.coordSys", "feat.refPoint", "feat.centerOfMass", "feat.boundingBox", "feat.mateRef"], big: true, label: "Reference Geometry" },
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
    { big: "sketch.offsetSurface" },
    { stack: ["sketch.mirror", "sketch.linearPattern", "sketch.move"] },
    "|",
    { big: "sketch.relations" },
    { big: "sketch.repair" },
    "|",
    { big: "sketch.quickSnaps" },
    "|",
    { big: "sketch.rapidSketch" },
    { big: "sketch.instant2d" },
    { big: "sketch.shadedContours" },
    "|",
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
            if ("flyout" in it) return <CmdBtn key={i} id={it.flyout[0]} ctx={ctx} big flyout={it.flyout} label={it.label} />;
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

export function CmdBtn({ id, ctx, big, row, small, flyout, label }: { id: string; ctx: Ctx; big?: boolean; row?: boolean; small?: boolean; flyout?: string[]; label?: string }) {
  const [open, setOpen] = useState(false);
  const c = CMD[id];
  if (!c) return null;
  const disabled = c.todo || (c.enabled ? !c.enabled(ctx) : false);
  const checked = c.checked?.(ctx);
  const desc = c.todo ? "Not yet implemented in OpenCAD." : c.tip && c.tip !== c.label ? c.tip : undefined;
  const cls = `cbtn ${big ? "big" : row ? "row" : small ? "small" : ""} ${disabled ? "disabled" : ""} ${checked ? "checked" : ""}`;
  return (
    <div className="cbtn-wrap">
      <button className={cls} data-tip={c.label} data-tip-desc={desc} data-tip-key={c.shortcut} onClick={() => !disabled && runCommand(id, ctx)}>
        <Icon name={c.icon ?? "generic"} size={big ? 24 : 16} />
        {!small && <span className="lbl">{c.id === "sketch.sketch" && ctx.app.sketch ? "Exit Sketch" : label ?? c.label}</span>}
      </button>
      {flyout && flyout.length > 1 && (
        <button className="flyarrow" onClick={() => setOpen(!open)}>
          ▾
        </button>
      )}
      {open && flyout && (
        <div className="flymenu" onMouseLeave={() => setOpen(false)}>
          {flyout.map((f) => (
            <div key={f} className={`mi ${CMD[f]?.todo ? "disabled" : ""}`} data-tip={CMD[f]?.label} data-tip-desc={CMD[f]?.todo ? "Not yet implemented in OpenCAD." : CMD[f]?.tip} onClick={() => !CMD[f]?.todo && (setOpen(false), runCommand(f, ctx))}>
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

const DISPLAY_STYLES = ["view.shadedEdges", "view.shaded", "view.hlr", "view.hlv", "view.wireframe"];

/** Heads-up View toolbar, top-centre of the graphics area. */
export function HeadsUpToolbar({ app, ctx }: { app: App; ctx: Ctx }) {
  const [open, setOpen] = useState<null | "orient" | "display" | "settings">(null);
  const close = useCallback(() => setOpen(null), []);
  const btn = (id: string) => {
    const c = CMD[id];
    return (
      <button key={id} className={`hu ${c.todo ? "disabled" : ""}`} data-tip={c.label} data-tip-desc={c.todo ? "Not yet implemented in OpenCAD." : c.tip !== c.label ? c.tip : undefined} data-tip-key={c.shortcut} onClick={() => runCommand(id, ctx)}>
        <Icon name={c.icon ?? "generic"} size={18} />
      </button>
    );
  };
  const style = DISPLAY_STYLES.find((id) => CMD[id].checked?.(ctx)) ?? "view.shadedEdges";
  return (
    <div className="headsup">
      {btn("view.zoomFit")}
      {btn("view.zoomArea")}
      {btn("view.previous")}
      {btn("view.section")}
      <div className="hu-wrap">
        <button className={`hu ${open === "orient" ? "on" : ""}`} data-tip="View Orientation" data-tip-desc="Changes the view orientation or the number of viewports." data-tip-key="Space" onClick={() => setOpen(open === "orient" ? null : "orient")}>
          <Icon name="viewOrient" size={18} />
          <span className="tiny">▾</span>
        </button>
        {open === "orient" && <ViewOrientationFlyout app={app} ctx={ctx} onClose={close} />}
      </div>
      <div className="hu-wrap">
        <button className={`hu ${open === "display" ? "on" : ""}`} data-tip="Display Style" data-tip-desc="Changes the display style for the active view." onClick={() => setOpen(open === "display" ? null : "display")}>
          <Icon name={styleIcon(style)} size={18} />
          <span className="tiny">▾</span>
        </button>
        {open === "display" && (
          <div className="flymenu hu-menu icons" onMouseLeave={close}>
            {DISPLAY_STYLES.map((id) => (
              <button key={id} className={`hu ${id === style ? "on" : ""}`} data-tip={CMD[id].label} onClick={() => (close(), runCommand(id, ctx))}>
                <Icon name={styleIcon(id)} size={20} />
              </button>
            ))}
          </div>
        )}
      </div>
      {btn("view.hideShow")}
      {btn("view.appearance")}
      {btn("view.scene")}
      <div className="hu-wrap">
        <button className={`hu ${open === "settings" ? "on" : ""}`} data-tip="View Settings" data-tip-desc="Toggles view settings such as perspective and sketch relations." onClick={() => setOpen(open === "settings" ? null : "settings")}>
          <Icon name="viewSettings" size={18} />
          <span className="tiny">▾</span>
        </button>
        {open === "settings" && (
          <div className="flymenu hu-menu" onMouseLeave={close}>
            {["view.perspective", "view.sketchRelations"].map((id) => (
              <div key={id} className="mi" onClick={() => (close(), runCommand(id, ctx))}>
                <span className="mi-icon">{CMD[id].checked?.(ctx) ? "✓" : ""}</span>
                {CMD[id].label}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function styleIcon(id: string) {
  return ({ "view.shadedEdges": "dsShadedEdges", "view.shaded": "dsShaded", "view.hlr": "dsHlr", "view.hlv": "dsHlv", "view.wireframe": "dsWireframe" } as Record<string, string>)[id] ?? "displayStyle";
}
