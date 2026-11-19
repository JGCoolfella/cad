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
import { ConfigDialog, ConfigurationManager } from "./ConfigurationManager";
import { AboutDialog, ComponentPropsDialog, ConfirmDeleteDialog, EquationsDialog, MassPropsDialog, MaterialDialog, SaveChangesDialog, SaveModifiedDialog, type ModifiedDoc } from "./Dialogs";
import { Icon } from "./icons";
import { UNIT_INFO, formatLength } from "../core/units";
import { saveBlob, openTextFile, saveText } from "./files";
import { invoke, isNative } from "../native/tauri";
import { AssemblySession, newAssembly, openAssembly, saveComponent } from "../assembly/session";
import { AssemblyTree } from "../assembly/AssemblyTree";
import { MatePM, MatePopup } from "../assembly/MatePM";
import { MovePM } from "../assembly/MovePM";
import { InterferencePM } from "../assembly/InterferencePM";

/** The native Open / Save As dialog (G1a: assemblies need real paths). Null when cancelled. */
function pickFile(save: boolean, kinds: string[], name?: string) {
  return invoke<string | null>("app_pick_file", { save, kinds, name: name ?? null });
}

const fileName = (p: string) => p.replace(/\\/g, "/").split("/").pop()!;

/**
 * The modified documents of an assembly session (G1c, Save Modified Documents): the open
 * document, the components opened from the assembly and edited (`overrides`), and the
 * assemblies waiting on `asmStack` with their own edits. Keys: `$current`, `$stack<n>`, or
 * the component's file.
 */
function modifiedDocs(app: App): ModifiedDoc[] {
  const docs: ModifiedDoc[] = [];
  const seen = new Set<string>();
  const kind = (f: string): ModifiedDoc["kind"] => (/\.ocasm$/i.test(f) ? "assembly" : "part");
  if (app.dirty || app.asm) {
    const name = app.asm ? (app.asm.path ? fileName(app.asm.path) : `${app.asm.name}.ocasm`) : app.filePath ? fileName(app.filePath) : `${app.doc.name}.ocpart`;
    if (app.dirty) docs.push({ key: "$current", name, kind: app.asm ? "assembly" : "part" });
  }
  const files = (o: Record<string, string>) =>
    Object.keys(o).forEach((f) => {
      if (seen.has(f.toLowerCase())) return;
      seen.add(f.toLowerCase());
      docs.push({ key: f, name: fileName(f), kind: kind(f) });
    });
  if (app.asm) files(app.asm.overrides);
  for (let k = app.asmStack.length - 1; k >= 0; k--) {
    const e = app.asmStack[k];
    // An edited component file that is the open document is listed once, as `$current`.
    seen.add(e.file.toLowerCase());
    files(e.session.overrides);
    if (e.dirty || Object.keys(e.session.overrides).length) docs.push({ key: `$stack${k}`, name: e.session.path ? fileName(e.session.path) : `${e.session.name}.ocasm`, kind: "assembly" });
  }
  return docs;
}

/** Whether leaving the open document must ask about more than itself. */
function needsSaveModified(app: App) {
  return (!!app.asm && Object.keys(app.asm.overrides).length > 0) || app.asmStack.some((e) => e.dirty || Object.keys(e.session.overrides).length > 0);
}

export function Shell({ app }: { app: App }) {
  useApp(app);
  const host = useRef<HTMLDivElement>(null);
  /** The last press in the graphics area (the Mate pop-up toolbar opens there). */
  const lastPress = useRef<[number, number]>([200, 200]);
  const [vp, setVp] = useState<Viewport | null>(null);
  const [dialog, setDialog] = useState<null | "options" | "mass" | "about" | "material" | "equations">(null);
  const [optionsPage, setOptionsPage] = useState<string | undefined>(undefined);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [compProps, setCompProps] = useState<string | null>(null);
  // "Save changes to <part>?" before an action that would discard unsaved changes.
  const [saveAsk, setSaveAsk] = useState<{ then: () => void } | null>(null);
  // Save Modified Documents (G1c): saving or leaving an assembly with edited components.
  const [saveModified, setSaveModified] = useState<{ mode: "save" | "close"; docs: ModifiedDoc[]; done: (keys: string[] | null | "discard") => void } | null>(null);
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

  /**
   * Saves the documents chosen in Save Modified Documents when leaving: the open part, the
   * edited components, the assemblies waiting on `asmStack`, the open assembly last. What
   * was not chosen is discarded with the document. False when a save failed or was
   * cancelled.
   */
  const saveDocs = async (keys: string[]): Promise<boolean> => {
    const quality = (app.kernel as { quality?: number }).quality ?? 3;
    const want = new Set(keys);
    if (want.has("$current") && !app.asm && !(await ui.saveFile(false))) return false;
    const sessions = [app.asm, ...app.asmStack.map((e) => e.session)].filter((s): s is AssemblySession => !!s);
    const all: Record<string, string> = {};
    for (const s of [...sessions].reverse()) Object.assign(all, s.overrides);
    try {
      for (const [file, text] of Object.entries(all)) {
        if (!want.has(file)) continue;
        await saveComponent(file, text, all, quality);
        sessions.forEach((s) => delete s.overrides[file]);
      }
    } catch (e) {
      app.flash(`Save failed: ${(e as Error).message ?? e}`, "error");
      return false;
    }
    for (let k = app.asmStack.length - 1; k >= 0; k--) {
      const e = app.asmStack[k];
      if (!want.has(`$stack${k}`)) continue;
      const path = e.session.path ?? (await pickFile(true, ["assembly"], `${e.session.name}.ocasm`).catch(() => null));
      if (!path || !(await e.session.save(path, []))) return false;
      e.dirty = false;
    }
    if (want.has("$current") && app.asm) {
      const a = app.asm;
      const path = a.path ?? (await pickFile(true, ["assembly"], `${a.name}.ocasm`).catch(() => null));
      if (!path || !(await a.save(path, []))) return false;
    }
    return true;
  };

  const ui: UiHooks = useMemo(
    () => ({
      whenSaved: (then) => {
        // Leaving the document: the assemblies a component was opened from go too.
        const leave = () => {
          app.asmStack = [];
          then();
        };
        if (needsSaveModified(app)) {
          const docs = modifiedDocs(app);
          setSaveModified({
            mode: "close",
            docs,
            done: (keys) => {
              setSaveModified(null);
              if (keys === null) return;
              if (keys === "discard") return leave();
              void saveDocs(keys).then((ok) => ok && leave());
            },
          });
          return;
        }
        if (app.dirty) setSaveAsk({ then: leave });
        else leave();
      },
      newFile: () => ui.whenSaved(() => app.newDocument()),
      newAssembly: () =>
        ui.whenSaved(() => {
          const n = (parseInt((app.asm?.name ?? "").replace(/\D/g, "")) || 0) + 1;
          app.openAssembly(new AssemblySession(app, newAssembly(n), null));
        }),
      insertComponent: async () => {
        const asm = app.asm;
        if (!asm) return;
        try {
          const path = await pickFile(false, ["part", "assembly"]);
          if (path) await asm.insertComponent(path);
        } catch (e) {
          app.flash(String((e as Error).message ?? e), "error");
        }
      },
      openFile: () =>
        ui.whenSaved(async () => {
          if (isNative()) {
            // The native app opens by path: assemblies refer to their parts by path.
            try {
              const path = await pickFile(false, ["open", "part", "assembly", "step"]);
              if (!path) return;
              if (/\.ocasm$/i.test(path)) return app.openAssembly(await openAssembly(app, path));
              const text = await invoke<string>("app_read_text", { path });
              const name = path.replace(/\\/g, "/").split("/").pop()!;
              if (/\.(step|stp)$/i.test(path)) return app.importStep(text, name);
              app.loadDocument(text, name);
              app.filePath = path;
            } catch (e) {
              app.flash(String((e as Error).message ?? e), "error");
            }
            return;
          }
          const f = await openTextFile();
          if (!f) return;
          try {
            if (/\.(step|stp)$/i.test(f.name)) return app.importStep(f.text, f.name);
            app.loadDocument(f.text, f.name);
            app.fileHandle = f.handle ?? null;
          } catch (e) {
            app.flash((e as Error).message, "error");
          }
        }),
      saveFile: async (as) => {
        if (app.asm) {
          const a = app.asm;
          const path = !as && a.path ? a.path : await pickFile(true, ["assembly"], `${a.name}.ocasm`).catch(() => null);
          if (!path) return false;
          const edited = Object.keys(a.overrides);
          if (!edited.length) return a.save(path);
          // Components opened from the assembly and edited: Save Modified Documents.
          const keys = await new Promise<string[] | null>((resolve) => {
            const docs: ModifiedDoc[] = [{ key: "$current", name: fileName(path), kind: "assembly", locked: true }, ...edited.map((f) => ({ key: f, name: fileName(f), kind: (/\.ocasm$/i.test(f) ? "assembly" : "part") as ModifiedDoc["kind"] }))];
            setSaveModified({ mode: "save", docs, done: (k) => (setSaveModified(null), resolve(k === "discard" ? null : k)) });
          });
          if (!keys) return false;
          return a.save(path, edited.filter((f) => keys.includes(f)));
        }
        if (isNative()) {
          // The native app saves parts to a path through the Save As dialog (as SOLIDWORKS).
          const path = !as && app.filePath ? app.filePath : await pickFile(true, ["part"], `${app.doc.name}.ocpart`).catch(() => null);
          if (!path) return false;
          try {
            await invoke("app_write_text", { path, text: app.serialize() });
          } catch (e) {
            app.flash(`Save failed: ${(e as Error).message ?? e}`, "error");
            return false;
          }
          app.filePath = path;
          app.dirty = false;
          app.flash(`Saved ${app.doc.name}`);
          return true;
        }
        const h = await saveText(app.serialize(), `${app.doc.name}.ocpart`, as ? null : app.fileHandle);
        if (h === false) return false;
        app.dirty = false;
        if (h) app.fileHandle = h;
        app.flash(`Saved ${app.doc.name}`);
        return true;
      },
      exportFile: async (fmt) => {
        if (isNative()) {
          // The native app exports through the Save As dialog, to a path (as SOLIDWORKS).
          try {
            const path = await pickFile(true, [fmt], `${app.docName()}.${fmt}`);
            if (!path) return;
            await invoke("engine_export_to", { features: app.activeFeatures(), format: fmt, quality: (app.kernel as { quality?: number }).quality ?? 3, path });
            app.flash(`Exported ${path.replace(/\\/g, "/").split("/").pop()}`);
          } catch (e) {
            app.flash(`Export failed: ${(e as Error).message ?? e}`, "error");
          }
          return;
        }
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
      showComponentProps: (id) => setCompProps(id),
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
      if (dialog || deleteId || saveAsk) return;
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
  }, [app, dialog, orient, deleteId, saveAsk]);

  const nativeDoc = useRef("");
  useEffect(() => {
    document.title = `${app.docName()}${app.dirty ? " *" : ""} - OpenCAD`;
    // Native app: the window title and the unsaved state go to the shell, which asks before
    // closing a modified document (Tauri does not run `beforeunload` on close).
    // Edits waiting in assemblies a component was opened from count too (G1c).
    const dirty = app.dirty || needsSaveModified(app);
    const key = `${dirty}${document.title}`;
    if (isNative() && key !== nativeDoc.current) {
      nativeDoc.current = key;
      invoke("app_doc", { title: document.title, dirty }).catch(() => {});
    }
  });
  useEffect(() => {
    if (!isNative()) return;
    const w = window as unknown as { __opencadConfirmClose?: () => void };
    w.__opencadConfirmClose = () => ui.whenSaved(() => void invoke("app_close").catch(() => {}));
    return () => {
      delete w.__opencadConfirmClose;
    };
  }, [app, ui]);

  const sketchSel = app.sketch && !app.pm && app.selection.some((s) => s.kind === "skEntity" || s.kind === "skPoint");
  // The ConfigurationManager tab (G1e) while no PropertyManager is open.
  const configTab = app.leftTab === "config" && !app.pm && !app.asm?.inPM && !sketchSel;
  const leftContent = configTab ? (
    <ConfigurationManager
      app={app}
      ctx={ctx}
      onMenu={(x, y, spec) => {
        setCtxBar(null);
        setCtxMenu({ x, y, spec });
      }}
    />
  ) : app.asm ? (
    app.asm.matePM ? (
      <MatePM app={app} />
    ) : app.asm.movePM ? (
      <MovePM app={app} />
    ) : app.asm.interferencePM ? (
      <InterferencePM app={app} />
    ) : (
      <AssemblyTree
        app={app}
        onContext={(x, y, item) => {
          setCtxBar(null);
          setCtxMenu({ x, y, spec: buildMenu(ctx, item, "tree") });
        }}
      />
    )
  ) : app.pm ? <PropertyManager app={app} ctx={ctx} /> : sketchSel ? (
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
              <button
                className={!app.pm && !configTab ? "on" : ""}
                title="FeatureManager Design Tree"
                data-tab-tree=""
                onClick={() => {
                  app.leftTab = "tree";
                  app.changed();
                }}
              >
                <Icon name="part" size={16} />
              </button>
              <button className={app.pm ? "on" : ""} title="PropertyManager">
                <Icon name="options" size={16} />
              </button>
              <button
                className={configTab ? "on" : ""}
                title={isNative() ? "ConfigurationManager" : "ConfigurationManager (needs the OpenCAD desktop app)"}
                data-tab-config=""
                disabled={!isNative()}
                onClick={() => {
                  app.leftTab = "config";
                  app.changed();
                }}
              >
                <Icon name="configuration" size={16} />
              </button>
              <button title="DisplayManager (not yet implemented)" disabled>
                <Icon name="appearance" size={16} />
              </button>
            </div>
            {leftContent}
          </div>
        )}
        <div
          className="graphics"
          style={gfxStyle}
          onPointerDownCapture={(e) => {
            if ((e.target as HTMLElement).closest?.(".mate-popup, .flyout")) return;
            const r = e.currentTarget.getBoundingClientRect();
            lastPress.current = [e.clientX - r.left, e.clientY - r.top];
          }}
        >
          <div className="gl-host" ref={host} />
          <HeadsUpToolbar app={app} ctx={ctx} />
          {app.opt("general.confirmationCorner") !== false && <ConfirmationCorner app={app} ctx={ctx} />}
          {app.pm || app.asm?.inPM ? <FlyoutTree app={app} ctx={ctx} /> : <Breadcrumbs app={app} />}
          {app.asm?.matePM && app.asm.matePM.picks.length === 2 && !app.asm.matePM.popupClosed && !app.asm.matePM.picks.some((p) => p.missing) && (
            <MatePopup app={app} x={lastPress.current[0]} y={lastPress.current[1]} />
          )}
          {app.asm?.smart && (
            // SmartMates (G1d): the proposed mate's type at the pointer, as SOLIDWORKS shows its icon.
            <div className="smart-pointer" data-smart-pointer={app.asm.smart.proposal?.type ?? ""} style={{ left: app.asm.smart.at[0] + 14, top: app.asm.smart.at[1] + 14 }}>
              <Icon name="mate" size={14} />
              {app.asm.smart.proposal?.label ?? ""}
            </div>
          )}
          {app.modify && (app.editor || app.modify.target) && <ModifyBox key={app.modify.dimId} app={app} />}
          {app.measureOpen && <MeasureDialog app={app} />}
          {app.asm?.isolated && (
            <div className="isolate-bar" data-isolate-bar="">
              <span>Isolate</span>
              <label>
                Visibility of removed components{" "}
                <select
                  data-isolate-others=""
                  value={app.asm.isolated.others}
                  onChange={(e) => {
                    app.asm!.isolated!.others = e.target.value as "hidden" | "transparent" | "wireframe";
                    app.changed();
                  }}
                >
                  <option value="wireframe">Wireframe</option>
                  <option value="transparent">Transparent</option>
                  <option value="hidden">Hidden</option>
                </select>
              </label>
              <button className="btn" data-exit-isolate="" onClick={() => app.asm?.exitIsolate()}>
                Exit Isolate
              </button>
            </div>
          )}
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
        <span>{app.sketch ? "Editing Sketch" : app.asm ? "Editing Assembly" : "Editing Part"}</span>
        {app.asm?.built && <span title="Degrees of freedom of the assembly">{app.asm.built.dof} DOF</span>}
        <span>{app.doc.units}</span>
        {app.result && <span className="muted" title="Last rebuild time">{app.result.timeMs.toFixed(0)} ms</span>}
      </div>
      {ctxMenu && <ContextMenu x={ctxMenu.x} y={ctxMenu.y} spec={ctxMenu.spec} ctx={ctx} onClose={() => setCtxMenu(null)} />}
      {ctxBar && !ctxMenu && <ContextToolbar x={ctxBar.x} y={ctxBar.y} rows={ctxBar.rows} ctx={ctx} onClose={() => setCtxBar(null)} />}
      {compProps && app.asm && <ComponentPropsDialog app={app} id={compProps} onClose={() => setCompProps(null)} />}
      {app.configDialog && <ConfigDialog app={app} />}
      {deleteId && <ConfirmDeleteDialog app={app} id={deleteId} onClose={() => setDeleteId(null)} />}
      {saveModified && (
        <SaveModifiedDialog
          mode={saveModified.mode}
          docs={saveModified.docs}
          onSave={(keys) => saveModified.done(keys)}
          onDontSave={() => saveModified.done("discard")}
          onCancel={() => saveModified.done(null)}
        />
      )}
      {saveAsk && (
        <SaveChangesDialog
          name={app.docName()}
          onSave={async () => {
            if (!(await ui.saveFile(false))) return; // save picker cancelled: stay in the dialog
            setSaveAsk(null);
            saveAsk.then();
          }}
          onDontSave={() => {
            setSaveAsk(null);
            saveAsk.then();
          }}
          onCancel={() => setSaveAsk(null)}
        />
      )}
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
  // An assembly's PropertyManagers pick its planes and origins from the fly-out tree.
  const shown = open || !!app.asm?.inPM;
  return (
    <div className={`flyout ${shown ? "open" : ""}`} data-flyout="">
      <div className="flyout-head" onClick={() => setOpen(!open)}>
        <span>{shown ? "▾" : "▸"}</span> <Icon name={app.asm ? "assembly" : "part"} size={14} /> {app.docName()}
      </div>
      {shown && (app.asm ? <AssemblyTree app={app} onContext={() => {}} /> : <FeatureTree app={app} ctx={ctx} compact onContext={() => {}} />)}
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
