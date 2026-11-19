/**
 * Shortcut (right-click) menus and context toolbars, structured like SOLIDWORKS:
 * an icon toolbar on top, then the menu with sub-menus, bold section headers
 * ("Relations", "Selected Entity (Line1)", "Feature (Boss-Extrude1)") and a ≫ row
 * that expands the less-used items. Contents were taken from SOLIDWORKS 2026.
 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { App, SelItem } from "../app/app";
import { CMD, FLYOUTS, runCommand, type Ctx } from "../commands/commands";
import { possibleRelations } from "../sketch/relations";
import { Icon } from "./icons";

export type MI =
  | { cmd: string; label?: string; more?: boolean }
  | { label: string; run: () => void; icon?: string; more?: boolean; disabled?: boolean }
  | { label: string; submenu: MI[]; more?: boolean }
  | { header: string }
  | { sep: true; more?: boolean }
  | { grid: string[][]; footer?: string };

export interface MenuSpec {
  toolbar: string[][];
  items: MI[];
}

const std: Record<string, string> = { front: "Front Plane", top: "Top Plane", right: "Right Plane" };

const SELECTION_TOOLS: MI = { label: "Selection Tools", submenu: [{ cmd: "select.box" }, { cmd: "select.lasso" }, { cmd: "select.invert" }] };
const ZOOM_PAN_ROTATE: MI[] = [{ cmd: "view.zoomFit" }, { cmd: "view.zoomArea" }, { cmd: "view.zoomInOut" }, { cmd: "view.rotateView" }, { cmd: "view.pan" }, { cmd: "view.rollView" }, { cmd: "view.rotateFloor" }, { cmd: "view.orientation", label: "View Orientation..." }];
const SET_VIEW_AS: MI = { label: "Set Current View As...", more: true, submenu: ["Front", "Back", "Top", "Bottom", "Left", "Right"].map((l) => ({ label: l, run: () => {}, disabled: true, icon: "viewOrient" })) };
const SKETCH_ENTITIES: MI = {
  label: "Sketch Entities",
  submenu: [
    {
      grid: [
        ["sketch.line", "sketch.rectangle", "sketch.centerRectangle", "sketch.3ptRectangle", "sketch.3ptCenterRectangle", "sketch.parallelogram"],
        ["sketch.slot", "sketch.centerSlot", "sketch.arcSlot3", "sketch.arcSlotCenter", "sketch.polygon"],
        ["sketch.circle", "sketch.perimeterCircle", "sketch.arc", "sketch.tangentArc", "sketch.threePointArc"],
        ["sketch.ellipse", "sketch.partialEllipse", "sketch.parabola", "sketch.conic"],
        ["sketch.spline", "sketch.styleSpline", "sketch.equationCurve", "sketch.point", "sketch.centerline", "sketch.midpointLine"],
        ["sketch.text", "sketch.plane"],
      ],
      footer: "view.sketchRelations",
    },
  ],
};
const MORE_DIMENSIONS: MI = { label: "More Dimensions", submenu: FLYOUTS.dimension.slice(1).map((cmd) => ({ cmd })) };
const SKETCH_TOOLS: MI = { label: "Sketch Tools", submenu: [{ cmd: "sketch.offset" }, { cmd: "sketch.convert" }, { cmd: "sketch.trim" }, { cmd: "sketch.fillet" }, { cmd: "sketch.mirror" }, { cmd: "sketch.makePath" }, { cmd: "feat.properties", label: "Properties..." }] };
const CUSTOMIZE: MI = { cmd: "tools.customizeMenu", more: true };
const SKETCH_ROW = ["sketch.selectOther", "sketch.toggleConstruction", "sketch.makeBlock", "view.zoomSelection", "sketch.replaceEntity", "sketch.numericInput", "sketch.smartDimension", "sketch.exit"];

function recent(app: App): MI {
  return { label: "Recent Commands", submenu: app.recentCommands.length ? app.recentCommands.map((cmd) => ({ cmd })) : [{ label: "(none)", run: () => {}, disabled: true }] };
}

const FEATURE_ITEMS = (name: string): MI[] => [
  { header: `Feature (${name})` },
  { label: "Comment", submenu: [{ cmd: "feat.comment", label: "Add Comment" }] },
  { cmd: "feat.parentChild" },
  { cmd: "feat.configure" },
  { cmd: "feat.delete" },
  { cmd: "feat.favorites" },
  { label: "Save Selection", submenu: [{ cmd: "feat.saveSelection", label: "New Selection Set" }] },
];
const TREE_TAIL: MI[] = [{ cmd: "feat.goTo" }, { cmd: "feat.createFolder" }, { cmd: "feat.rename" }, { cmd: "tree.hideShowItems" }, { cmd: "tree.collapse" }, CUSTOMIZE];

/** Menu + toolbar for a right-click (or the toolbar alone for a left-click selection). */
export function buildMenu(ctx: Ctx, item: SelItem | null, where: "graphics" | "tree"): MenuSpec {
  const { app } = ctx;
  // Right-clicking in the tree always acts on the tree item, also while sketching.
  if (item && (where === "tree" || !app.sketch) && !app.isSelected(item)) app.select(item);
  // ------------------------------------------------------------ in a sketch
  if (app.sketch && where !== "tree") {
    const sk = app.sketch.data;
    const tool = app.tool;
    if (tool && tool !== "smartDimension") {
      const lineTool = tool === "line" || tool === "centerline" || tool === "midpointLine";
      const drawing = (app.editor?.st.pts.length ?? 0) > 0;
      return {
        toolbar: [lineTool ? ["rel.horizontal", "rel.vertical", "sketch.autoDimension", "sketch.dimPreview"] : ["sketch.autoDimension", "sketch.dimPreview"], SKETCH_ROW],
        items: [
          { cmd: "sketch.select" },
          ...(lineTool && drawing ? ([{ cmd: "sketch.endChain" }, { cmd: "sketch.switchToArc" }] as MI[]) : []),
          { sep: true },
          SKETCH_ENTITIES,
          recent(app),
          MORE_DIMENSIONS,
          { header: "Selected Entity" },
          { label: "Quick Snaps", submenu: [{ cmd: "sketch.quickSnaps" }] },
          { cmd: "sketch.snapsOptions", more: true },
          CUSTOMIZE,
        ],
      };
    }
    const ids = app.selection.filter((s) => s.kind === "skEntity" || s.kind === "skPoint").map((s) => (s as { id: string }).id);
    const rels = possibleRelations(sk, ids).filter((t) => CMD[`rel.${t}`]).map((t) => `rel.${t}`);
    const ent = ids.length === 1 ? sk.entities[ids[0]] : undefined;
    const index = ent ? Object.values(sk.entities).filter((e) => e.type === ent.type).findIndex((e) => e.id === ent.id) + 1 : 0;
    const entName = ent ? `${ent.type === "line" ? "Line" : ent.type === "point" ? "Point" : "Arc"}${index}` : ids.length === 1 ? "Point" : "";
    const dim = app.selection.find((s) => s.kind === "skDim");
    const items: MI[] = [
      ...(ent?.type === "line" ? ([{ cmd: "sketch.selectMidpoint" }] as MI[]) : []),
      { cmd: "sketch.contourSelect", more: true },
      SELECTION_TOOLS,
      { label: "Zoom/Pan/Rotate", submenu: ZOOM_PAN_ROTATE },
      SET_VIEW_AS,
      { cmd: "sketch.displayGrid" },
      { sep: true },
      recent(app),
      SKETCH_ENTITIES,
      MORE_DIMENSIONS,
      { header: "Relations" },
      { cmd: "sketch.relations", label: "Display/Delete Relations..." },
      ...(ids.length ? ([{ cmd: "sketch.fullyDefine", label: "Fully Define Sketch..." }] as MI[]) : []),
      { cmd: "sketch.snapsOptions", more: true },
    ];
    if (ids.length || dim) {
      items.push({ header: ids.length === 1 ? `Selected Entity (${entName})` : dim ? "Dimension" : "Selected Entities" }, { cmd: "edit.delete", label: "Delete" });
      if (ent?.type === "line") items.push({ cmd: "sketch.detachSegment" });
      if (dim && dim.kind === "skDim") {
        const d = sk.dimensions[dim.id];
        if (d) items.push({ label: d.driven ? "Driving" : "Driven", run: () => (app.sketchCheckpoint(), (d.driven = !d.driven), app.resolveSketch()) });
      }
    }
    items.push(SKETCH_TOOLS, CUSTOMIZE);
    return {
      toolbar: [ids.length ? [...rels.slice(0, 7), "sketch.autoDimension", "sketch.dimPreview"] : ["sketch.autoDimension", "sketch.dimPreview"], ids.length ? [...SKETCH_ROW, "feat.extrude"] : SKETCH_ROW],
      items,
    };
  }
  // ------------------------------------------------------------ assembly (G1b)
  if (app.asm && item) {
    const s = app.asm;
    const instId = item.kind === "feature" ? item.id : "body" in item && item.body !== undefined ? s.instanceOfOccurrence(item.body) : null;
    const inst = s.doc.instances.find((i) => i.id === instId);
    const mate = item.kind === "feature" ? s.doc.mates.find((m) => m.id === item.id) : undefined;
    if (inst) {
      const kind = s.componentOf(inst)?.kind === "assembly" ? "Assembly" : "Part";
      return {
        toolbar: [[...(kind === "Part" ? ["asm.editPart"] : []), "asm.openPart", inst.hidden ? "asm.show" : "asm.hide", inst.fixed ? "asm.float" : "asm.fix", "asm.suppress", "asm.appearance", "asm.moveComponent", "asm.rotateComponent"]],
        items: [
          ...(where === "graphics" ? ([{ label: "Zoom/Pan/Rotate", submenu: ZOOM_PAN_ROTATE }] as MI[]) : []),
          { header: `Component (${inst.name})` },
          ...(kind === "Part" ? ([{ cmd: "asm.editPart" }] as MI[]) : []),
          { cmd: "asm.openPart", label: `Open ${kind}` },
          { cmd: inst.hidden ? "asm.show" : "asm.hide" },
          { cmd: inst.fixed ? "asm.float" : "asm.fix" },
          { cmd: "asm.suppress", label: inst.suppressed ? "Unsuppress" : "Suppress" },
          { cmd: "asm.appearance" },
          { cmd: "asm.removeAppearance" },
          { cmd: "asm.moveComponent" },
          { cmd: "asm.rotateComponent" },
          { cmd: "asm.isolate" },
          { sep: true },
          { cmd: "asm.rename" },
          { cmd: "asm.properties" },
          { cmd: "asm.delete", label: "Delete..." },
          CUSTOMIZE,
        ],
      };
    }
    if (mate) {
      return {
        toolbar: [["asm.editMate", "asm.suppress"]],
        items: [{ header: `Mate (${mate.name})` }, { cmd: "asm.editMate" }, { cmd: "asm.suppress", label: mate.suppressed ? "Unsuppress" : "Suppress" }, { cmd: "asm.rename" }, { cmd: "asm.delete", label: "Delete..." }, CUSTOMIZE],
      };
    }
    return { toolbar: [], items: [...ZOOM_PAN_ROTATE, recent(app)] };
  }
  // ------------------------------------------------------------ part
  const viewItems: MI[] = [SELECTION_TOOLS, { label: "Zoom/Pan/Rotate", submenu: ZOOM_PAN_ROTATE }, recent(app)];
  // Editing a part in an assembly (G1d): back to the assembly, as SOLIDWORKS' "Edit Assembly: <name>".
  const ctxTop = app.asmStack[app.asmStack.length - 1];
  const editAssembly: MI[] = ctxTop?.context && !app.asm ? [{ cmd: "asm.editComponent", label: `Edit Assembly: ${ctxTop.session.name}` }] : [];
  if (!item) {
    return {
      toolbar: [["select.box", "select.lasso", "select.filters", "select.other", "sketch.select"]],
      items: [...editAssembly, ...ZOOM_PAN_ROTATE, { sep: true }, { cmd: "view.editScene" }, { cmd: "view.openDrawing" }, { cmd: "view.generateDrawing" }, { sep: true }, recent(app), CUSTOMIZE],
    };
  }
  if (item.kind === "plane" && std[item.id]) {
    return {
      toolbar: [["sketch.sketch", "view.showHide", "view.zoomSelection", "view.normalTo"]],
      items: [
        ...(where === "graphics" ? viewItems : []),
        { header: `Feature (${std[item.id]})` },
        { cmd: "feat.sectionView" },
        { label: "Comment", submenu: [{ cmd: "feat.comment", label: "Add Comment" }] },
        { cmd: "feat.parentChild" },
        { cmd: "feat.favorites" },
        { label: "Save Selection", submenu: [{ cmd: "feat.saveSelection", label: "New Selection Set" }] },
        { cmd: "feat.properties", label: "Properties..." },
        { sep: true },
        { cmd: "feat.goTo" },
        { cmd: "feat.rename" },
        { cmd: "tree.hideShowItems" },
        { cmd: "tree.collapse" },
        CUSTOMIZE,
      ],
    };
  }
  if (item.kind === "origin") {
    return { toolbar: [["view.origins", "view.zoomSelection"]], items: [{ header: "Feature (Origin)" }, { cmd: "feat.parentChild" }, { cmd: "feat.favorites" }, { sep: true }, { cmd: "feat.goTo" }, { cmd: "tree.hideShowItems" }, { cmd: "tree.collapse" }, CUSTOMIZE] };
  }
  const fid = app.selectedFeatureId();
  const f = fid ? app.featureById(fid) : undefined;
  const name = f?.name ?? "";
  if (item.kind === "face" || item.kind === "edge" || item.kind === "vertex") {
    const kind = item.kind === "face" ? "Face" : item.kind === "edge" ? "Edge" : "Vertex";
    return {
      toolbar: [
        ["feat.edit", "feat.editSketch", "feat.openDrawing", "feat.suppress", "feat.rollback", "feat.appearance", "feat.copyAppearance"],
        ["select.other", "sketch.sketch", "feat.hideBody", "view.zoomSelection", "view.normalTo", "feat.fillet", "feat.chamfer"],
      ],
      items: [
        ...viewItems,
        { header: kind },
        ...(item.kind === "face" ? ([{ cmd: "feat.transparency" }, { cmd: "feat.planeParallel" }] as MI[]) : ([{ cmd: "select.other", label: "Select Tangency" }] as MI[])),
        ...(f ? FEATURE_ITEMS(name) : []),
        { header: "Body" },
        { cmd: "feat.isolate" },
        { cmd: "feat.exportDxf" },
        { label: "FeatureWorks...", submenu: [{ cmd: "feat.featureWorks", label: "Recognize Features..." }] },
        { cmd: "feat.partSupply" },
        CUSTOMIZE,
      ],
    };
  }
  if (f) {
    const isSketch = f.type === "sketch";
    const isPlane = f.type === "refplane";
    return {
      toolbar: isPlane
        ? [["feat.edit", "sketch.sketch", "feat.suppress", "feat.rollback"], ["view.showHide", "view.zoomSelection", "view.normalTo"]]
        : isSketch
          ? [["feat.editSketch", "feat.suppress", "feat.rollback"], ["view.showHide", "view.zoomSelection", "view.normalTo"]]
          : [["feat.edit", "feat.editSketch", "feat.suppress", "feat.rollback"], ["feat.hideBody", "view.zoomSelection", "view.normalTo", "feat.appearance"]],
      items: [
        ...(where === "graphics" ? viewItems : []),
        ...FEATURE_ITEMS(name),
        { cmd: "feat.newFolder" },
        { cmd: "feat.properties" },
        { cmd: "feat.transparency" },
        { sep: true },
        { label: "FeatureWorks...", submenu: [{ cmd: "feat.featureWorks", label: "Recognize Features..." }] },
        ...(app.doc.rollback !== null ? ([{ cmd: "feat.rollToEnd" }] as MI[]) : []),
        ...TREE_TAIL,
      ],
    };
  }
  return { toolbar: [], items: [...ZOOM_PAN_ROTATE, recent(app)] };
}

// ---------------------------------------------------------------- rendering

function TB({ id, ctx, close }: { id: string; ctx: Ctx; close: () => void }) {
  const c = CMD[id];
  if (!c) return null;
  const disabled = c.todo || (c.enabled ? !c.enabled(ctx) : false);
  return (
    <button
      className={`ctb ${disabled ? "disabled" : ""} ${c.checked?.(ctx) ? "on" : ""}`}
      data-tip={c.label}
      data-tip-desc={c.todo ? "Not yet implemented in OpenCAD." : c.tip !== c.label ? c.tip : undefined}
      onClick={() => {
        if (disabled) return;
        close();
        runCommand(id, ctx);
      }}
    >
      <Icon name={c.icon ?? "generic"} size={18} />
    </button>
  );
}

function Items({ items, ctx, close, expanded }: { items: MI[]; ctx: Ctx; close: () => void; expanded: boolean }) {
  return (
    <>
      {items.map((it, i) => {
        if ("more" in it && it.more && !expanded) return null;
        if ("sep" in it) return <div key={i} className="msep" />;
        if ("header" in it) return <div key={i} className="cm-header">{it.header}</div>;
        if ("grid" in it) {
          return (
            <div key={i} className="cm-grid">
              {it.grid.map((row, r) => (
                <div key={r} className="cm-grid-row">
                  {row.map((id) => (
                    <TB key={id} id={id} ctx={ctx} close={close} />
                  ))}
                </div>
              ))}
              {it.footer && <Items items={[{ cmd: it.footer, label: "View Sketch Relations" }]} ctx={ctx} close={close} expanded />}
            </div>
          );
        }
        if ("submenu" in it) {
          return (
            <div key={i} className="mi sub">
              <span className="mi-icon" />
              {it.label}
              <span className="arrow">▸</span>
              <div className="submenu">
                <Items items={it.submenu} ctx={ctx} close={close} expanded />
              </div>
            </div>
          );
        }
        if ("cmd" in it) {
          const c = CMD[it.cmd];
          if (!c) return null;
          const disabled = c.todo || (c.enabled ? !c.enabled(ctx) : false);
          const checked = c.checked?.(ctx);
          return (
            <div
              key={i}
              className={`mi ${disabled ? "disabled" : ""}`}
              onClick={() => {
                if (disabled) return;
                close();
                runCommand(it.cmd, ctx);
              }}
            >
              <span className={`mi-icon ${checked ? "checked" : ""}`}>{c.icon ? <Icon name={c.icon} size={15} /> : checked ? "✓" : null}</span>
              {it.label ?? c.label}
            </div>
          );
        }
        return (
          <div
            key={i}
            className={`mi ${it.disabled ? "disabled" : ""}`}
            onClick={() => {
              if (it.disabled) return;
              close();
              it.run();
            }}
          >
            <span className="mi-icon">{it.icon ? <Icon name={it.icon} size={15} /> : null}</span>
            {it.label}
          </div>
        );
      })}
    </>
  );
}

export function ContextMenu({ x, y, spec, ctx, onClose }: { x: number; y: number; spec: MenuSpec; ctx: Ctx; onClose: () => void }) {
  const [expanded, setExpanded] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const close = () => closeRef.current();
    const key = (e: KeyboardEvent) => e.key === "Escape" && (e.stopPropagation(), closeRef.current());
    const t = setTimeout(() => window.addEventListener("mousedown", close), 0);
    window.addEventListener("keydown", key, true);
    return () => {
      clearTimeout(t);
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", key, true);
    };
  }, []);
  // Keep the menu on screen (SOLIDWORKS flips it up/left near the window edges).
  useLayoutEffect(() => {
    const r = ref.current?.getBoundingClientRect();
    if (!r) return;
    setPos({ x: Math.max(4, Math.min(x, window.innerWidth - r.width - 6)), y: Math.max(4, Math.min(y, window.innerHeight - r.height - 6)) });
  }, [x, y, expanded]);
  const hasMore = spec.items.some((i) => "more" in i && i.more);
  return (
    <div className="ctx" ref={ref} style={{ left: pos.x, top: pos.y }} onMouseDown={(e) => e.stopPropagation()} onContextMenu={(e) => e.preventDefault()}>
      {spec.toolbar.length > 0 && (
        <div className="ctx-toolbar">
          {spec.toolbar.map((row, r) => (
            <div key={r} className="ctx-row">
              {row.map((id) => (
                <TB key={id} id={id} ctx={ctx} close={onClose} />
              ))}
            </div>
          ))}
        </div>
      )}
      <div className="ctx-menu">
        <Items items={spec.items} ctx={ctx} close={onClose} expanded={expanded} />
        {hasMore && !expanded && (
          <div className="cm-more" data-tip="More Commands" onClick={() => setExpanded(true)}>
            ≫
          </div>
        )}
      </div>
    </div>
  );
}

/**
 * Context toolbar shown next to the pointer after a left-click selection. It is
 * faint until the pointer moves onto it and goes away when the pointer leaves.
 */
export function ContextToolbar({ x, y, rows, ctx, onClose }: { x: number; y: number; rows: string[][]; ctx: Ctx; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(false);
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    const move = (e: MouseEvent) => {
      const r = ref.current?.getBoundingClientRect();
      if (!r) return;
      const dx = Math.max(r.left - e.clientX, 0, e.clientX - r.right), dy = Math.max(r.top - e.clientY, 0, e.clientY - r.bottom);
      const d = Math.hypot(dx, dy);
      setNear(d < 2);
      if (d > 130) closeRef.current();
    };
    const down = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) closeRef.current();
    };
    const key = () => closeRef.current();
    window.addEventListener("mousemove", move);
    window.addEventListener("mousedown", down, true);
    window.addEventListener("keydown", key, true);
    window.addEventListener("wheel", key, true);
    return () => {
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mousedown", down, true);
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("wheel", key, true);
    };
  }, []);
  if (!rows.some((r) => r.length)) return null;
  const left = Math.min(x + 14, window.innerWidth - 220), top = Math.max(4, y - 22 - rows.length * 24);
  return (
    <div ref={ref} className={`ctx-toolbar floating ${near ? "near" : ""}`} style={{ left, top }}>
      {rows.map((row, r) => (
        <div key={r} className="ctx-row">
          {row.map((id) => (
            <TB key={id} id={id} ctx={ctx} close={onClose} />
          ))}
        </div>
      ))}
    </div>
  );
}
