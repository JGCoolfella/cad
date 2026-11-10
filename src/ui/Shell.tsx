import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { App, SelItem } from "../app/app";
import { Viewport } from "../render/viewport";
import { handleKey } from "../commands/keyboard";
import { runCommand, type Ctx, type UiHooks } from "../commands/commands";
import { useApp } from "./hooks";
import { TopBar } from "./TopBar";
import { CommandManager, HeadsUpToolbar } from "./CommandManager";
import { FeatureTree } from "./FeatureTree";
import { PropertyManager, SketchSelectionPM } from "./PropertyManager";
import { ConfirmationCorner, ContextMenu, GestureWheel, ModifyBox, ShortcutBar, type MenuItem } from "./Overlays";
import { OptionsDialog } from "./OptionsDialog";
import { OrientationDialog } from "./ViewOrientation";
import { TooltipLayer } from "./Tooltip";
import { AboutDialog, MassPropsDialog } from "./Dialogs";
import { Icon } from "./icons";
import { UNIT_INFO, formatLength } from "../core/units";
import { saveBlob, openTextFile, saveText } from "./files";

export function Shell({ app }: { app: App }) {
  useApp(app);
  const host = useRef<HTMLDivElement>(null);
  const [vp, setVp] = useState<Viewport | null>(null);
  const [dialog, setDialog] = useState<null | "options" | "mass" | "about">(null);
  const [orient, setOrient] = useState(false);
  const [shortcutBar, setShortcutBar] = useState<{ x: number; y: number } | null>(null);
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; items: MenuItem[] } | null>(null);
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
      showOptions: () => setDialog("options"),
      showOrientation: () => setOrient((o) => !o),
      showMassProps: () => setDialog("mass"),
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
    v.onGesture = (g) => setGesture(g && g.active ? { x: g.x, y: g.y, dir: g.dir } : null);
    v.onContextMenu = (x, y, item) => setCtxMenu({ x, y, items: contextItems(ctxRef.current, item) });
    setVp(v);
    return () => v.dispose();
  }, [app]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (dialog) return;
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
  }, [app, dialog, orient]);

  useEffect(() => {
    document.title = `${app.doc.name}${app.dirty ? " *" : ""} - OpenCAD`;
  });

  const sketchSel = app.sketch && !app.pm && app.selection.some((s) => s.kind === "skEntity" || s.kind === "skPoint");
  const leftContent = app.pm ? <PropertyManager app={app} ctx={ctx} /> : sketchSel ? <SketchSelectionPM app={app} /> : <FeatureTree app={app} ctx={ctx} onContext={(x, y, item) => setCtxMenu({ x, y, items: contextItems(ctx, item) })} />;
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
          {app.modify && app.editor && <ModifyBox app={app} />}
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
      {ctxMenu && <ContextMenu x={ctxMenu.x} y={ctxMenu.y} items={ctxMenu.items} onClose={() => setCtxMenu(null)} />}
      {gesture && vp && <GestureWheel x={gesture.x} y={gesture.y} dir={gesture.dir} labels={vp.gestureCommands().map((c) => c.label)} />}
      {shortcutBar && <ShortcutBar x={shortcutBar.x} y={shortcutBar.y} ctx={ctx} onClose={() => setShortcutBar(null)} />}
      {dialog === "options" && <OptionsDialog app={app} vp={vp} onClose={() => setDialog(null)} />}
      <TooltipLayer />
      {dialog === "mass" && <MassPropsDialog app={app} onClose={() => setDialog(null)} />}
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

function contextItems(ctx: Ctx, item: SelItem | null): MenuItem[] {
  const { app } = ctx;
  const run = (id: string) => () => runCommand(id, ctx);
  if (app.sketch) {
    const items: MenuItem[] = [
      { label: "Select", run: () => app.editor?.setTool(null) },
      { label: "Exit Sketch", run: () => app.exitSketch(true) },
      { label: "Discard Changes and Exit", run: () => app.exitSketch(false) },
      { sep: true },
      { label: "Line", run: run("sketch.line") },
      { label: "Smart Dimension", run: run("sketch.smartDimension") },
      { label: "Display/Delete Relations", run: run("sketch.relations") },
      { label: "Construction Geometry", run: run("sketch.toggleConstruction") },
      { sep: true },
      { label: "Delete", run: run("edit.delete") },
      { label: "Zoom to Fit", run: run("view.zoomFit") },
    ];
    if (app.tool === "line" || app.tool === "centerline") items.unshift({ label: "End Chain (double-click)", run: () => app.editor && (app.editor.resetTool(), app.changed()) });
    return items;
  }
  if (item) {
    if (!app.isSelected(item)) app.select(item);
    if (item.kind === "feature") {
      const f = app.featureById(item.id);
      if (!f) return [];
      const items: MenuItem[] = [];
      if (f.type === "sketch") items.push({ label: "Edit Sketch", run: () => app.editSketch(f.id) });
      else items.push({ label: "Edit Feature", run: () => app.editFeature(f.id) });
      const consumer = app.doc.features.find((x) => "sketchId" in x && x.sketchId === f.id);
      if (!consumer && f.type !== "sketch") {
        const sk = "sketchId" in f ? f.sketchId : null;
        if (sk) items.push({ label: "Edit Sketch", run: () => app.editSketch(sk) });
      }
      items.push(
        { label: f.suppressed ? "Unsuppress" : "Suppress", run: () => app.toggleSuppress(f.id) },
        { label: "Roll to Previous", run: () => app.setRollback(app.doc.features.findIndex((x) => x.id === f.id)) },
        { label: "Roll to End", run: () => app.setRollback(null) },
        { sep: true },
        {
          label: "Rename (F2)",
          run: () => {
            const n = prompt("Rename feature", f.name);
            if (n) app.renameFeature(f.id, n);
          },
        },
        { label: "Delete...", run: () => confirm(`Delete ${f.name}?`) && app.deleteFeature(f.id) },
      );
      return items;
    }
    if (item.kind === "plane" || item.kind === "face") {
      const planeVis = item.kind === "plane" ? [{ label: app.isPlaneVisible(item.id) ? "Hide" : "Show", run: () => app.setPlaneVisible(item.id, !app.isPlaneVisible(item.id)) }] : [];
      return [
        ...planeVis,
        { label: "Sketch", run: () => app.startSketch() },
        { label: "Normal To", run: run("view.normalTo") },
        ...(item.kind === "plane" && !["front", "top", "right"].includes(item.id) ? [{ label: "Edit Feature", run: () => app.editFeature(item.id) }] : []),
        { sep: true } as MenuItem,
        { label: "Zoom to Fit", run: run("view.zoomFit") },
      ];
    }
    if (item.kind === "edge") return [{ label: "Fillet", run: run("feat.fillet") }, { label: "Chamfer", run: run("feat.chamfer") }, { label: "Zoom to Fit", run: run("view.zoomFit") }];
  }
  return [
    { label: "Zoom to Fit", run: run("view.zoomFit") },
    { label: "View Orientation (Spacebar)", run: run("view.orientation") },
    { label: "Previous View", run: run("view.previous") },
    { sep: true },
    { label: "Sketch", run: run("sketch.sketch") },
    { label: "Rebuild", run: run("edit.rebuild") },
    { label: "Options…", run: run("tools.options") },
  ];
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
