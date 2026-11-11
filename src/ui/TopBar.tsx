import { useEffect, useState } from "react";
import type { App } from "../app/app";
import { CMD, runCommand, type Ctx } from "../commands/commands";
import { Icon } from "./icons";

type Entry = string | "-" | { label: string; items: Entry[] };

const MENUS: { label: string; items: Entry[] }[] = [
  { label: "File", items: ["file.new", "file.open", "-", "file.save", "file.saveAs", "-", "file.exportStep", "file.exportStl", "-", "file.print"] },
  { label: "Edit", items: ["edit.undo", "edit.redo", "-", "edit.delete", "edit.suppress", "-", "edit.rebuild", "edit.forceRebuild"] },
  {
    label: "View",
    items: [
      { label: "Display", items: ["view.wireframe", "view.hlv", "view.hlr", "view.shadedEdges", "view.shaded", "-", "view.perspective"] },
      { label: "Hide/Show", items: ["view.sketchRelations"] },
      { label: "Modify", items: ["view.orientation", "view.zoomFit", "view.zoomIn", "view.zoomOut", "view.previous", "-", "view.front", "view.back", "view.left", "view.right", "view.top", "view.bottom", "view.isometric", "view.trimetric", "view.dimetric", "view.normalTo"] },
      "-",
      { label: "User Interface", items: ["view.taskPane", "view.treePane"] },
      "view.redraw",
    ],
  },
  {
    label: "Insert",
    items: [
      { label: "Boss/Base", items: ["feat.extrude", "feat.revolve", "feat.swept", "feat.lofted", "feat.boundary"] },
      { label: "Cut", items: ["feat.cutExtrude", "feat.cutRevolve", "feat.sweptCut", "feat.loftedCut"] },
      { label: "Features", items: ["feat.fillet", "feat.chamfer", "feat.hole", "feat.draft", "feat.shell", "feat.rib", "feat.wrap", "feat.intersect"] },
      { label: "Pattern/Mirror", items: ["feat.mirror", "feat.linearPattern", "feat.circularPattern", "feat.curvePattern", "feat.sketchPattern", "feat.tablePattern", "feat.fillPattern", "feat.variablePattern"] },
      { label: "Reference Geometry", items: ["feat.plane", "feat.axis", "feat.coordSys", "feat.refPoint"] },
      "-",
      "sketch.sketch",
    ],
  },
  {
    label: "Tools",
    items: [
      {
        label: "Sketch Entities",
        items: ["sketch.line", "sketch.centerline", "sketch.rectangle", "sketch.centerRectangle", "sketch.circle", "sketch.arc", "sketch.tangentArc", "sketch.threePointArc", "sketch.point", "sketch.slot", "sketch.polygon", "sketch.spline", "sketch.ellipse", "sketch.text"],
      },
      { label: "Sketch Tools", items: ["sketch.fillet", "sketch.chamfer", "sketch.offset", "sketch.convert", "sketch.trim", "sketch.extend", "sketch.mirror", "sketch.linearPattern", "sketch.circularPattern", "sketch.move", "sketch.copy", "sketch.rotate", "sketch.scale", "-", "sketch.toggleConstruction"] },
      { label: "Relations", items: ["sketch.relations"] },
      { label: "Dimensions", items: ["sketch.smartDimension", "sketch.dimHorizontal", "sketch.dimVertical"] },
      { label: "Sketch Settings", items: ["sketch.autoRelations", "sketch.grid"] },
      "-",
      { label: "Evaluate", items: ["eval.measure", "eval.massProps"] },
      "-",
      "tools.equations",
      "-",
      "tools.options",
    ],
  },
  { label: "Window", items: [] },
  { label: "Help", items: ["help.about"] },
];

export function TopBar({ app, ctx }: { app: App; ctx: Ctx }) {
  const [open, setOpen] = useState<number | null>(null);
  useEffect(() => {
    if (open === null) return;
    const close = (e: MouseEvent) => {
      if (!(e.target as HTMLElement).closest(".menubar")) setOpen(null);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  return (
    <div className="topbar">
      <div className="brand">
        <span className="logo">◆</span>
        <b>OpenCAD</b>&nbsp;<i>Design</i>
      </div>
      <div className="menubar">
        {MENUS.map((m, i) => (
          <div key={m.label} className={`menu ${open === i ? "open" : ""}`} onMouseDown={() => setOpen(open === i ? null : i)} onMouseEnter={() => open !== null && setOpen(i)}>
            {m.label}
            {open === i && (
              <div className="dropdown" onMouseDown={(e) => e.stopPropagation()}>
                {m.label === "Window" ? (
                  <div className="mi checked">✓ {app.doc.name}</div>
                ) : (
                  <MenuItems items={m.items} ctx={ctx} close={() => setOpen(null)} />
                )}
              </div>
            )}
          </div>
        ))}
        <span className="pin" title="Pin menu">
          <Icon name="pin" size={14} />
        </span>
      </div>
      <div className="doc-title">
        {app.doc.name}
        {app.dirty ? " *" : ""}
      </div>
      <div className="tb-right">
        <span className="searchbox">
          <Icon name="search" size={14} />
          <input placeholder="Search Commands" onKeyDown={(e) => e.key === "Enter" && searchCommand((e.target as HTMLInputElement).value, ctx)} />
        </span>
        <button className="icon-btn" title="Help" onClick={() => runCommand("help.about", ctx)}>
          <Icon name="help" size={18} />
        </button>
      </div>
    </div>
  );
}

function searchCommand(q: string, ctx: Ctx) {
  const s = q.trim().toLowerCase();
  if (!s) return;
  const c = Object.values(CMD).find((c) => c.label.toLowerCase().includes(s) && !c.todo);
  if (c) runCommand(c.id, ctx);
  else ctx.app.flash(`No command matching "${q}"`, "info");
}

function MenuItems({ items, ctx, close }: { items: Entry[]; ctx: Ctx; close: () => void }) {
  return (
    <>
      {items.map((it, i) => {
        if (it === "-") return <div key={i} className="msep" />;
        if (typeof it === "object") {
          return (
            <div key={i} className="mi sub">
              {it.label}
              <span className="arrow">▸</span>
              <div className="submenu">
                <MenuItems items={it.items} ctx={ctx} close={close} />
              </div>
            </div>
          );
        }
        const c = CMD[it];
        if (!c) return null;
        const disabled = c.todo || (c.enabled && !c.enabled(ctx));
        const checked = c.checked?.(ctx);
        return (
          <div
            key={i}
            className={`mi ${disabled ? "disabled" : ""}`}
            title={c.todo ? "Not yet implemented" : c.tip}
            onClick={() => {
              if (disabled) return;
              close();
              runCommand(c.id, ctx);
            }}
          >
            <span className="mi-icon">{checked ? "✓" : c.icon ? <Icon name={c.icon} size={14} /> : null}</span>
            {c.label}
            {c.shortcut && <span className="sc">{c.shortcut}</span>}
          </div>
        );
      })}
    </>
  );
}
