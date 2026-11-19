/**
 * ConfigurationManager (G1e), the third tab of the left pane as in SOLIDWORKS: the
 * document's configurations (the active one checked) and, in an assembly, its display
 * states. Double-click shows (activates) a configuration or display state; right-click
 * gives Show Configuration, Add Configuration..., Properties... and Delete. Configurations
 * need the native engine; elsewhere the tab is greyed.
 */
import { useState } from "react";
import type { App } from "../app/app";
import type { Ctx } from "../commands/commands";
import { configurationsOf } from "../core/configurations";
import { displayStatesOf } from "../assembly/configurations";
import type { MenuSpec } from "./ContextMenu";
import { Dialog } from "./Dialogs";
import { Icon } from "./icons";

export function ConfigurationManager({ app, ctx, onMenu }: { app: App; ctx: Ctx; onMenu: (x: number, y: number, spec: MenuSpec) => void }) {
  const [sel, setSel] = useState<string | null>(null);
  void ctx;
  const doc = app.configDoc();
  const cfgs = configurationsOf(doc);
  const asm = app.asm;
  const ds = asm ? displayStatesOf(asm.doc) : null;
  const enabled = app.configurationsAvailable();
  const cfgMenu = (id: string): MenuSpec => ({
    toolbar: [],
    items: [
      { header: `Configuration (${cfgs.list.find((c) => c.id === id)?.name ?? ""})` },
      { label: "Show Configuration", icon: "configuration", run: () => app.activateConfiguration(id), disabled: !enabled || id === cfgs.active },
      { cmd: "config.add", label: "Add Configuration..." },
      { label: "Properties...", icon: "properties", run: () => ((app.configDialog = { mode: "props", id }), app.changed()), disabled: !enabled },
      { label: "Delete", icon: "delete", run: () => app.deleteConfiguration(id), disabled: !enabled || id === cfgs.active || cfgs.list.length <= 1 },
    ],
  });
  const rootMenu = (): MenuSpec => ({ toolbar: [], items: [{ cmd: "config.add", label: "Add Configuration..." }] });
  const dsMenu = (id: string): MenuSpec => ({
    toolbar: [],
    items: [
      { header: `Display State (${ds?.list.find((d) => d.id === id)?.name ?? ""})` },
      { label: "Activate Display State", icon: "displayState", run: () => asm?.activateDisplayState(id), disabled: !enabled || id === ds?.active },
      { cmd: "config.addDisplayState", label: "Add Display State" },
      { label: "Properties...", icon: "properties", run: () => ((app.configDialog = { mode: "displayProps", id }), app.changed()), disabled: !enabled },
      {
        label: "Delete",
        icon: "delete",
        run: () => {
          try {
            asm?.deleteDisplayState(id);
          } catch (e) {
            app.flash((e as Error).message, "error");
          }
        },
        disabled: !enabled || id === ds?.active || (ds?.list.length ?? 1) <= 1,
      },
    ],
  });
  return (
    <div className="tree config-manager" data-config-manager="">
      <div
        className="tree-row root"
        data-config-root=""
        onContextMenu={(e) => {
          e.preventDefault();
          onMenu(e.clientX, e.clientY, rootMenu());
        }}
      >
        <span className="twisty" />
        <Icon name={asm ? "assembly" : "part"} size={16} />
        <span className="tlabel">{doc.name} Configuration(s)</span>
      </div>
      {cfgs.list.map((c) => {
        const active = c.id === cfgs.active;
        return (
          <div
            key={c.id}
            className={`tree-row ${sel === c.id ? "sel" : ""} ${active ? "active-config" : ""}`}
            style={{ paddingLeft: 22 }}
            data-config={c.name}
            data-config-active={active ? "" : undefined}
            title={c.description ?? ""}
            onClick={() => setSel(c.id)}
            onDoubleClick={() => enabled && app.activateConfiguration(c.id)}
            onContextMenu={(e) => {
              e.preventDefault();
              setSel(c.id);
              onMenu(e.clientX, e.clientY, cfgMenu(c.id));
            }}
            tabIndex={0}
          >
            <span className="twisty">{active ? "✓" : ""}</span>
            <Icon name="configuration" size={16} />
            <span className="tlabel">
              {c.name}
              {c.description ? ` [ ${c.description} ]` : ""}
            </span>
          </div>
        );
      })}
      {ds && (
        <>
          <div className="tree-row root" style={{ marginTop: 6 }} data-display-root="">
            <span className="twisty" />
            <Icon name="displayState" size={16} />
            <span className="tlabel">Display States (linked)</span>
          </div>
          {ds.list.map((d) => {
            const active = d.id === ds.active;
            return (
              <div
                key={d.id}
                className={`tree-row ${sel === d.id ? "sel" : ""} ${active ? "active-config" : ""}`}
                style={{ paddingLeft: 22 }}
                data-display-state={d.name}
                data-display-active={active ? "" : undefined}
                onClick={() => setSel(d.id)}
                onDoubleClick={() => enabled && asm?.activateDisplayState(d.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setSel(d.id);
                  onMenu(e.clientX, e.clientY, dsMenu(d.id));
                }}
                tabIndex={0}
              >
                <span className="twisty">{active ? "✓" : ""}</span>
                <Icon name="displayState" size={16} />
                <span className="tlabel">{d.name}</span>
              </div>
            );
          })}
        </>
      )}
      {!enabled && !app.sketch && !app.pm && <div className="muted config-note">Configurations need the OpenCAD desktop app.</div>}
    </div>
  );
}

/** Add Configuration / Configuration Properties / Add Display State / Display State Properties (🔍 SOLIDWORKS uses a PropertyManager for Add Configuration). */
export function ConfigDialog({ app }: { app: App }) {
  const d = app.configDialog!;
  const doc = app.configDoc();
  const cfgs = configurationsOf(doc);
  const ds = app.asm ? displayStatesOf(app.asm.doc) : null;
  const cfg = d.id ? cfgs.list.find((c) => c.id === d.id) : undefined;
  const state = d.id && ds ? ds.list.find((s) => s.id === d.id) : undefined;
  const display = d.mode === "addDisplay" || d.mode === "displayProps";
  const [name, setName] = useState(d.mode === "props" ? (cfg?.name ?? "") : d.mode === "displayProps" ? (state?.name ?? "") : "");
  const [description, setDescription] = useState(d.mode === "props" ? (cfg?.description ?? "") : "");
  const close = () => {
    app.configDialog = null;
    app.changed();
  };
  const ok = () => {
    let done = true;
    try {
      if (d.mode === "add") done = app.addConfiguration(name, description);
      else if (d.mode === "props" && d.id) done = app.setConfigurationProps(d.id, name, description);
      else if (d.mode === "addDisplay") app.asm?.addDisplayState(name);
      else if (d.mode === "displayProps" && d.id) app.asm?.renameDisplayState(d.id, name);
    } catch (e) {
      app.flash((e as Error).message, "error");
      done = false;
    }
    if (done) close();
  };
  const title = d.mode === "add" ? "Add Configuration" : d.mode === "props" ? "Configuration Properties" : d.mode === "addDisplay" ? "Add Display State" : "Display State Properties";
  return (
    <Dialog title={title} onClose={close}>
      <div className="massprops config-dialog" data-config-dialog={d.mode} onKeyDown={(e) => (e.stopPropagation(), e.key === "Enter" && ok())}>
        <label className="row">
          {display ? "Display state name" : "Configuration name"}{" "}
          <input data-config-name="" autoFocus value={name} onChange={(e) => setName(e.target.value)} />
        </label>
        {!display && (
          <label className="row">
            Description <input data-config-description="" value={description} onChange={(e) => setDescription(e.target.value)} />
          </label>
        )}
        <div className="sc-buttons">
          <button className="btn primary" data-config-ok="" onClick={ok}>
            OK
          </button>
          <button className="btn" data-config-cancel="" onClick={close}>
            Cancel
          </button>
        </div>
      </div>
    </Dialog>
  );
}
