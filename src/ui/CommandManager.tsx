import { useCallback, useEffect, useRef, useState } from "react";
import { ViewOrientationFlyout } from "./ViewOrientation";
import type { App } from "../app/app";
import { CMD, FLYOUTS, flyoutCurrent, runCommand, type Ctx } from "../commands/commands";
import { Icon } from "./icons";

/** A flyout cell: `{ fly: "line" }` shows the last-used tool of that family with a drop-down arrow. */
type Cell = string | { fly: string };
type Item = { big: Cell } | { stack: Cell[] } | { grid: Cell[][] } | { menu: string[]; icon: string; label: string } | "|";

/** CommandManager tabs, laid out as in SOLIDWORKS. */
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
    { big: { fly: "fillet" } },
    { big: "feat.linearPattern" },
    { stack: ["feat.rib", "feat.draft", "feat.shell"] },
    { stack: ["feat.wrap", "feat.intersect", "feat.mirror"] },
    "|",
    { menu: ["feat.plane", "feat.axis", "feat.coordSys", "feat.refPoint", "feat.centerOfMass", "feat.boundingBox", "feat.mateRef"], icon: "refGeom", label: "Reference Geometry" },
    { big: "feat.curves" },
    { big: "feat.instant3d" },
  ],
  Sketch: [
    { big: { fly: "sketch" } },
    { big: { fly: "dimension" } },
    "|",
    {
      grid: [
        [{ fly: "line" }, { fly: "circle" }, { fly: "spline" }, "sketch.plane"],
        [{ fly: "rectangle" }, { fly: "arc" }, { fly: "ellipse" }, "sketch.text"],
        [{ fly: "slot" }, "sketch.polygon", { fly: "sketchFillet" }, "sketch.point"],
      ],
    },
    "|",
    { big: { fly: "trim" } },
    { big: { fly: "convert" } },
    { big: "sketch.offset" },
    { big: "sketch.offsetSurface" },
    { stack: ["sketch.mirror", { fly: "sketchPattern" }, { fly: "move" }] },
    "|",
    { big: { fly: "relations" } },
    { big: "sketch.repair" },
    "|",
    { big: "sketch.quickSnaps" },
    "|",
    { big: "sketch.rapidSketch" },
    { big: "sketch.instant2d" },
    { big: "sketch.shadedContours" },
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
  const cell = (c: Cell, mode: Mode, key: string | number) => (typeof c === "string" ? <CmdBtn key={key} id={c} ctx={ctx} mode={mode} /> : <FlyBtn key={key} k={c.fly} ctx={ctx} mode={mode} />);
  return (
    <div className="cmdmgr">
      <div className="cm-row">
        <div className="quick">
          {QUICK.map((col, i) => (
            <div key={i} className="qcol">
              {col.map((id) =>
                id === "home" ? (
                  <button key={id} className="qbtn" data-tip="Welcome to OpenCAD">
                    <Icon name="home" size={16} />
                  </button>
                ) : (
                  <CmdBtn key={id} id={id} ctx={ctx} mode="icon" />
                ),
              )}
            </div>
          ))}
        </div>
        <div className="cm-items">
          {TABS[tab].map((it, i) => {
            if (it === "|") return <div key={i} className="cm-sep" />;
            if ("big" in it) return cell(it.big, "big", i);
            if ("stack" in it)
              return (
                <div key={i} className="cm-stack">
                  {it.stack.map((c, j) => cell(c, "row", j))}
                </div>
              );
            if ("grid" in it)
              return (
                <div key={i} className="cm-grid3">
                  {it.grid.map((row, r) => (
                    <div key={r} className="cm-grid3-row">
                      {row.map((c, j) => cell(c, "icon", j))}
                    </div>
                  ))}
                </div>
              );
            return <MenuBtn key={i} ids={it.menu} icon={it.icon} label={it.label} ctx={ctx} />;
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

type Mode = "big" | "row" | "icon";

function tipProps(id: string) {
  const c = CMD[id];
  return { "data-tip": c.label, "data-tip-desc": c.todo ? "Not yet implemented in OpenCAD." : c.tip && c.tip !== c.label ? c.tip : undefined, "data-tip-key": c.shortcut };
}

export function CmdBtn({ id, ctx, mode = "icon", label }: { id: string; ctx: Ctx; mode?: Mode; label?: string }) {
  const c = CMD[id];
  if (!c) return null;
  const disabled = c.todo || (c.enabled ? !c.enabled(ctx) : false);
  const checked = c.checked?.(ctx);
  const cls = `cbtn ${mode === "icon" ? "small" : mode} ${disabled ? "disabled" : ""} ${checked ? "checked" : ""}`;
  return (
    <div className="cbtn-wrap">
      <button className={cls} {...tipProps(id)} onClick={() => !disabled && runCommand(id, ctx)}>
        <Icon name={c.icon ?? "generic"} size={mode === "big" ? 24 : 16} />
        {mode !== "icon" && <span className="lbl">{id === "sketch.sketch" && ctx.app.sketch ? "Exit Sketch" : label ?? c.label}</span>}
      </button>
    </div>
  );
}

/** Close a drop-down on any click outside it or on Escape. */
function useOutsideClose(open: boolean, close: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key);
    };
  }, [open, close]);
  return ref;
}

function DropList({ ids, ctx, close }: { ids: string[]; ctx: Ctx; close: () => void }) {
  return (
    <div className="flymenu">
      {ids.map((f) => {
        const c = CMD[f];
        if (!c) return null;
        const disabled = c.todo || (c.enabled ? !c.enabled(ctx) : false);
        return (
          <div key={f} className={`mi ${disabled ? "disabled" : ""}`} {...tipProps(f)} onClick={() => !disabled && (close(), runCommand(f, ctx))}>
            <span className="mi-icon">
              <Icon name={c.icon ?? "generic"} size={16} />
            </span>
            {c.label}
          </div>
        );
      })}
    </div>
  );
}

/**
 * Flyout tool button. The button runs (and shows) the tool used last from its
 * family; the arrow lists the whole family. The choice is remembered.
 */
export function FlyBtn({ k, ctx, mode }: { k: string; ctx: Ctx; mode: Mode }) {
  const [open, setOpen] = useState(false);
  const close = useCallback(() => setOpen(false), []);
  const ref = useOutsideClose(open, close);
  const ids = FLYOUTS[k] ?? [];
  const id = flyoutCurrent(ctx.app, k);
  const c = CMD[id];
  if (!c) return null;
  const disabled = c.todo || (c.enabled ? !c.enabled(ctx) : false);
  // The button looks pressed while any tool of its family is active.
  const checked = ids.some((x) => CMD[x]?.checked?.(ctx) && (x !== "sketch.sketch" || mode === "big"));
  const cls = `cbtn ${mode === "icon" ? "small" : mode} ${disabled ? "disabled" : ""} ${checked ? "checked" : ""}`;
  return (
    <div className={`cbtn-wrap fly ${mode}`} ref={ref}>
      <button className={cls} {...tipProps(id)} onClick={() => !disabled && runCommand(id, ctx)}>
        <Icon name={c.icon ?? "generic"} size={mode === "big" ? 24 : 16} />
        {mode !== "icon" && <span className="lbl">{id === "sketch.sketch" && ctx.app.sketch ? "Exit Sketch" : c.label}</span>}
      </button>
      <button className={`flyarrow ${open ? "on" : ""}`} onClick={() => setOpen(!open)}>
        ▾
      </button>
      {open && <DropList ids={ids} ctx={ctx} close={close} />}
    </div>
  );
}

/** Menu-only button (Reference Geometry): clicking it opens the list. */
function MenuBtn({ ids, icon, label, ctx }: { ids: string[]; icon: string; label: string; ctx: Ctx }) {
  const [open, setOpen] = useState(false);
  const close = useCallback(() => setOpen(false), []);
  const ref = useOutsideClose(open, close);
  return (
    <div className="cbtn-wrap fly big" ref={ref}>
      <button className={`cbtn big ${open ? "checked" : ""}`} data-tip={label} onClick={() => setOpen(!open)}>
        <Icon name={icon} size={24} />
        <span className="lbl">{label}</span>
      </button>
      <button className="flyarrow" onClick={() => setOpen(!open)}>
        ▾
      </button>
      {open && <DropList ids={ids} ctx={ctx} close={close} />}
    </div>
  );
}

const DISPLAY_STYLES = ["view.shadedEdges", "view.shaded", "view.hlr", "view.hlv", "view.wireframe"];

/** Heads-up View toolbar, top-centre of the graphics area. */
export function HeadsUpToolbar({ app, ctx }: { app: App; ctx: Ctx }) {
  const [open, setOpen] = useState<null | "orient" | "display" | "settings" | "hide">(null);
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
      <div className="hu-wrap">
        <button className={`hu ${open === "hide" ? "on" : ""}`} data-tip="Hide/Show Items" data-tip-desc="Changes the visibility of items in the graphics area." onClick={() => setOpen(open === "hide" ? null : "hide")}>
          <Icon name="hideShow" size={18} />
          <span className="tiny">▾</span>
        </button>
        {open === "hide" && (
          <div className="flymenu hu-menu icons" onMouseLeave={close}>
            {["view.planes", "view.origins", "view.sketchRelations", "sketch.displayGrid"].map((id) => (
              <button key={id} className={`hu ${CMD[id].checked?.(ctx) ? "on" : ""}`} data-tip={`View ${CMD[id].label}`} data-tip-desc={CMD[id].tip} onClick={() => runCommand(id, ctx)}>
                <Icon name={CMD[id].icon ?? "generic"} size={20} />
              </button>
            ))}
          </div>
        )}
      </div>
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
