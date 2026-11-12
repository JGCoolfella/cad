import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { App } from "../app/app";
import { Viewport } from "../render/viewport";
import { handleKey } from "../commands/keyboard";
import type { Ctx, UiHooks } from "../commands/commands";
import { useApp } from "./hooks";
import { TopBar } from "./TopBar";
import { CommandManager, HeadsUpToolbar } from "./CommandManager";
import { FeatureTree } from "./FeatureTree";
import { PropertyManager } from "./PropertyManager";
import { SketchSelectionPM } from "./SketchPM";
import { ConfirmationCorner, FilterBar, GestureWheel, ModifyBox, SelectOtherList, ShortcutBar } from "./Overlays";
import { ContextMenu, ContextToolbar, buildMenu, type MenuSpec } from "./ContextMenu";
import { OptionsDialog } from "./OptionsDialog";
import { MeasureDialog } from "./MeasureDialog";
import { OrientationDialog } from "./ViewOrientation";
import { TooltipLayer } from "./Tooltip";
import { AboutDialog, ConfirmDeleteDialog, EquationsDialog, MassPropsDialog, MaterialDialog } from "./Dialogs";
import { Icon } from "./icons";
import { UNIT_INFO, formatLength } from "../core/units";
import { saveBlob, openTextFile, saveText } from "./files";

export function Shell({ app }: { app: App }) {
  useApp(app);
  const host = useRef<HTMLDivElement>(null);
  const [vp, setVp] = useState<Viewport | null>(null);
  const [dialog, setDialog] = useState<null | "options" | "mass" | "about" | "material" | "equations">(null);
  const [optionsPage, setOptionsPage] = useState<string | undefined>(undefined);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [ctxBar, setCtxBar] = useState<{ x: number; y: number; rows: string[][] } | null>(null);
  const [orient, setOrient] = useState(false);
  const [shortcutBar, setShortcutBar] = useState<{ x: number; y: number } | null>(null);
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; spec: MenuSpec } | null>(null);
  const [gesture, setGesture] = useState<{ x: number; y: number; dir: number | null } | null>(null);
  const [taskPane, setTaskPane] = useState(false);
  const [treePane, setTreePane] = useState(true);
  const mouse = useRef({ x: 0, y: 0 });
  const closeOrient = useCallback(() => setOrient(false), []);
  // Auto-dismiss messages (System Options > Messages/Errors/Warnings).
  useEffect(() => {
    if (!app.message || app.opt("messages.dismissTimes") === false) return;
    const msg = app.message;
    const t = setTimeout(() => {
      if (app.message === msg) {
        app.message = null;
        app.changed();
      }
    }, Number(app.opt("messages.seconds") ?? 5) * 1000 * (msg.kind === "error" ? 2 : 1));
    return () => clearTimeout(t);
  }, [app.message]);
  // Viewport background (System Options > Colors).
  const bgMode = app.opt<string>("ui.bgMode") ?? "scene";
  const [bgTop, bgBottom] = app.options.colors.background;
  const gfxStyle = bgMode === "plain" ? { background: bgBottom } : bgMode === "gradient" ? { background: `linear-gradient(${bgTop}, ${bgBottom})` } : undefined;

  const ui: UiHooks = useMemo(
    () => ({
      openFile: async () => {
        const f = await openTextFile();
        if (!f) return;
        try {
          if (/\.(step|stp)$/i.test(f.name)) return app.importStep(f.text, f.name);
          app.loadDocument(f.text, f.name);
          app.fileHandle = f.handle ?? null;
        } catch (e) {
          app.flash((e as Error).message, "error");
        }
      },
      saveFile: async (as) => {
        const h = await saveText(app.serialize(), `${app.doc.name}.ocpart`, as ? null : app.fileHandle);
        if (h !== false) {
          app.dirty = false;
          if (h) app.fileHandle = h;
          app.flash(`Saved ${app.doc.name}`);
        }
      },
      exportFile: async (fmt) => {
        try {
          const buf = await app.kernel.exportFile(app.activeFeatures(), fmt);
          await saveBlob(new Blob([buf]), `${app.doc.name}.${fmt}`);
        } catch (e) {
          app.flash(`Export failed: ${(e as Error).message}`, "error");
        }
      },
      showOptions: (page) => {
        setOptionsPage(page);
        setDialog("options");
      },
      confirmDelete: (id) => setDeleteId(id),
      showOrientation: () => setOrient((o) => !o),
      showMassProps: () => setDialog("mass"),
      showMaterial: () => setDialog("material"),
      showEquations: () => setDialog("equations"),
      showShortcutBar: () => setShortcutBar({ ...mouse.current }),
      toggleTaskPane: () => setTaskPane((t) => !t),
      toggleTreePane: () => setTreePane((t) => !t),
      showAbout: () => setDialog("about"),
    }),
    [app],
  );
  const ctx: Ctx = { app, vp, ui };
  const ctxRef = useRef(ctx);
  ctxRef.current = ctx;

  useEffect(() => {
    const v = new Viewport(host.current!, app);
    // Debugging/tests: window.opencadView is the graphics area (window.opencad is the app).
    (window as unknown as { opencadView: Viewport }).opencadView = v;
    v.onGesture = (g) => setGesture(g && g.active ? { x: g.x, y: g.y, dir: g.dir } : null);
    v.onContextMenu = (x, y, item) => {
      setCtxBar(null);
      setCtxMenu({ x, y, spec: buildMenu(ctxRef.current, item, "graphics") });
    };
    v.onSelectionClick = (x, y) => {
      const sel = app.selection[app.selection.length - 1] ?? null;
      const spec = buildMenu(ctxRef.current, sel, "graphics");
      setCtxBar(spec.toolbar.length ? { x, y, rows: spec.toolbar } : null);
    };
    setVp(v);
    return () => v.dispose();
  }, [app]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (dialog || deleteId) return;
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement;
      if (e.key === " " && !typing) {
        e.preventDefault();
        const v = ctxRef.current.vp;
        if (e.ctrlKey && v) {
          // Ctrl+Space: View Selector on its own.
          v.onSelectorPick = () => v.showSelector(false);
          v.showSelector(!v.selector.visible);
          return;
        }
        setOrient((o) => !o);
        return;
      }
      if (e.key === "Escape" && ctxRef.current.vp?.selector.visible && !orient) ctxRef.current.vp.showSelector(false);
      if (handleKey(e, ctxRef.current)) e.preventDefault();
    };
    const onMove = (e: MouseEvent) => (mouse.current = { x: e.clientX, y: e.clientY });
    const onBeforeUnload = (e: BeforeUnloadEvent) => {
      if (app.dirty) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousemove", onMove);
    window.addEventListener("beforeunload", onBeforeUnload);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("beforeunload", onBeforeUnload);
    };
  }, [app, dialog, orient, deleteId]);

  useEffect(() => {
    document.title = `${app.doc.name}${app.dirty ? " *" : ""} - OpenCAD`;
  });

  const sketchSel = app.sketch && !app.pm && app.selection.some((s) => s.kind === "skEntity" || s.kind === "skPoint");
  const leftContent = app.pm ? <PropertyManager app={app} ctx={ctx} /> : sketchSel ? (
    <SketchSelectionPM app={app} ctx={ctx} />
  ) : (
    <FeatureTree
      app={app}
      ctx={ctx}
      onContext={(x, y, item) => {
        setCtxBar(null);
        setCtxMenu({ x, y, spec: buildMenu(ctx, item, "tree") });
      }}
    />
  );
  const an = app.sketch?.analysis;
  const status = an ? (an.status === "full" ? "Fully Defined" : an.status === "over" ? "Over Defined" : an.status === "unsolved" ? "Unsolvable" : "Under Defined") : null;

  return (
    <div className="shell" onContextMenu={(e) => e.preventDefault()}>
      <TopBar app={app} ctx={ctx} />
      <CommandManager app={app} ctx={ctx} />
      <div className="main">
        {treePane && (
          <div className="leftpane">
            <div className="lp-tabs">
              <button className={!app.pm ? "on" : ""} title="FeatureManager Design Tree">
                <Icon name="part" size={16} />
              </button>
              <button className={app.pm ? "on" : ""} title="PropertyManager">
                <Icon name="options" size={16} />
              </button>
              <button title="ConfigurationManager (not yet implemented)" disabled>
                <Icon name="generic" size={16} />
              </button>
              <button title="DisplayManager (not yet implemented)" disabled>
                <Icon name="appearance" size={16} />
              </button>
            </div>
            {leftContent}
          </div>
        )}
        <div className="graphics" style={gfxStyle}>
          <div className="gl-host" ref={host} />
          <HeadsUpToolbar app={app} ctx={ctx} />
          {app.opt("general.confirmationCorner") !== false && <ConfirmationCorner app={app} ctx={ctx} />}
          {app.pm ? <FlyoutTree app={app} ctx={ctx} /> : <Breadcrumbs app={app} />}
          {app.modify && (app.editor || app.modify.target) && <ModifyBox key={app.modify.dimId} app={app} />}
          {app.measureOpen && <MeasureDialog app={app} />}
          {app.filterBar && <FilterBar app={app} />}
          {app.selectOther && <SelectOtherList app={app} />}
          {orient && <OrientationDialog app={app} ctx={ctx} onClose={closeOrient} />}
          {app.message && (
            <div className={`toast ${app.message.kind}`} onClick={() => ((app.message = null), app.changed())}>
              {app.message.text}
            </div>
          )}
          {app.rebuilding && <div className="rebuilding">Rebuilding…</div>}
          {!app.kernelReady && <div className="loading">Loading geometry kernel…</div>}
        </div>
        <div className={`taskpane ${taskPane ? "open" : ""}`}>
          <div className="tp-strip">
            {["home", "open", "part", "appearance", "options"].map((n) => (
              <button key={n} onClick={() => setTaskPane((t) => !t)} title="Task Pane">
                <Icon name={n} size={16} />
              </button>
            ))}
          </div>
          {taskPane && (
            <div className="tp-body">
              <div className="tp-title">Task Pane</div>
              <p className="muted">Design Library, File Explorer, View Palette and Appearances are planned (see docs/ROADMAP.md).</p>
              <button className="btn" onClick={() => ui.openFile()}>Open a part…</button>
            </div>
          )}
        </div>
      </div>
      <div className="bottomtabs">
        <span className="on">Model</span>
        <span className="muted">3D Views</span>
      </div>
      <div className="statusbar">
        <span>OpenCAD — parametric 3D CAD</span>
        <span className="grow" />
        {app.sketch && <CursorCoord app={app} />}
        {status && <span className={`sk-status ${an!.status}`}>{status}</span>}
        <span>{app.sketch ? "Editing Sketch" : "Editing Part"}</span>
        <span>{app.doc.units}</span>
        {app.result && <span className="muted" title="Last rebuild time">{app.result.timeMs.toFixed(0)} ms</span>}
      </div>
      {ctxMenu && <ContextMenu x={ctxMenu.x} y={ctxMenu.y} spec={ctxMenu.spec} ctx={ctx} onClose={() => setCtxMenu(null)} />}
      {ctxBar && !ctxMenu && <ContextToolbar x={ctxBar.x} y={ctxBar.y} rows={ctxBar.rows} ctx={ctx} onClose={() => setCtxBar(null)} />}
      {deleteId && <ConfirmDeleteDialog app={app} id={deleteId} onClose={() => setDeleteId(null)} />}
      {gesture && vp && <GestureWheel x={gesture.x} y={gesture.y} dir={gesture.dir} labels={vp.gestureCommands().map((c) => c.label)} />}
      {shortcutBar && <ShortcutBar x={shortcutBar.x} y={shortcutBar.y} ctx={ctx} onClose={() => setShortcutBar(null)} />}
      {dialog === "options" && <OptionsDialog app={app} vp={vp} initialPage={optionsPage} onClose={() => setDialog(null)} />}
      <TooltipLayer />
      {dialog === "mass" && <MassPropsDialog app={app} onClose={() => setDialog(null)} />}
      {dialog === "material" && <MaterialDialog app={app} onClose={() => setDialog(null)} />}
      {dialog === "equations" && <EquationsDialog app={app} onClose={() => setDialog(null)} />}
      {dialog === "about" && <AboutDialog onClose={() => setDialog(null)} />}
    </div>
  );
}

/** The FeatureManager fly-out shown in the graphics area while a PropertyManager is open. */
function FlyoutTree({ app, ctx }: { app: App; ctx: Ctx }) {
  const [open, setOpen] = useState(false);
  return (
    <div className={`flyout ${open ? "open" : ""}`}>
      <div className="flyout-head" onClick={() => setOpen(!open)}>
        <span>{open ? "▾" : "▸"}</span> <Icon name="part" size={14} /> {app.doc.name}
      </div>
      {open && <FeatureTree app={app} ctx={ctx} compact onContext={() => {}} />}
    </div>
  );
}

function CursorCoord({ app }: { app: App }) {
  useSyncExternalStore(app.subscribeCursor, app.getCursorVersion);
  const cs = app.cursorSketch;
  if (!cs) return null;
  const u = UNIT_INFO[app.doc.units].length;
  return <span className="coord">{`${formatLength(cs[0], app.doc.units)}${u}  ${formatLength(cs[1], app.doc.units)}${u}  0${u}`}</span>;
}

/** Breadcrumbs: the selected item's path, top-left of the graphics area. */
function Breadcrumbs({ app }: { app: App }) {
  if (app.opt("display.breadcrumbs") === false) return null;
  const s = app.selection[app.selection.length - 1];
  if (!s) return null;
  const std: Record<string, string> = { front: "Front Plane", top: "Top Plane", right: "Right Plane" };
  let label = "";
  let icon = "generic";
  if (s.kind === "plane") (label = std[s.id] ?? app.featureById(s.id)?.name ?? ""), (icon = "plane");
  else if (s.kind === "feature") {
    const f = app.featureById(s.id);
    label = f?.name ?? "";
    icon = f?.type === "sketch" ? "sketch" : f?.type ?? "generic";
  } else if (s.kind === "face" || s.kind === "edge" || s.kind === "vertex") (label = `${s.kind[0].toUpperCase()}${s.kind.slice(1)}<${s.id + 1}>`), (icon = "part");
  else if (s.kind === "origin") (label = "Origin"), (icon = "origin");
  else return null;
  return (
    <div className="breadcrumbs">
      <span className="crumb">
        <Icon name="part" size={14} />
      </span>
      <span className="crumb">
        <Icon name={icon} size={14} /> {label}
      </span>
    </div>
  );
}
