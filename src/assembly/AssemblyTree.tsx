/**
 * The assembly FeatureManager design tree (G1a, G1b), laid out as SOLIDWORKS lays it out:
 * the assembly, History, its Front, Top and Right Planes and Origin, then each component
 * with its state prefix, (f) fixed, (-) under defined, (+) over defined, none when fully
 * defined, then the Mates folder. Mates in error show the error icon; a mate whose
 * reference no longer resolves says Missing. A component expands to its own planes and
 * origin, which (like the assembly's) can be picked for mates. Hidden components are
 * greyed; renaming works as for features (`app.renaming`). The same tree is the fly-out
 * FeatureManager in the graphics area while an assembly PropertyManager is open.
 *
 * Only the rows in view are drawn (G3d): the rows are laid out as data (`treeRows.ts`) and
 * a window of them, with spacers for the rest, is rendered as the list scrolls. A row
 * selected elsewhere (the graphics area) or being renamed is scrolled into view, as
 * SOLIDWORKS' "Scroll selected item into view" (Options > FeatureManager, on by default).
 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { App, SelItem } from "../app/app";
import { Icon } from "../ui/icons";
import { assemblyRows, scrollToShow, visibleWindow, type TreeRow } from "./treeRows";

/** Rows that can be selected (and picked for mates): `$plane:*`, `$origin`, `<inst>/$…`. */
const pickable = (id: string) => !id.startsWith("$") || id.startsWith("$plane:") || id === "$origin";

/** Rows drawn beyond each edge of the view (smooth scrolling and keyboard focus). */
const OVERSCAN = 12;
/** `.tree-row`'s height (styles.css) until a row is measured. */
const ROW_HEIGHT = 21;

export function AssemblyTree({ app, onContext }: { app: App; onContext: (x: number, y: number, item: SelItem) => void }) {
  const s = app.asm!;
  const [matesOpen, setMatesOpen] = useState(true);
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const body = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ top: 0, height: 0 });
  const [rowHeight, setRowHeight] = useState(ROW_HEIGHT);
  const rows = assemblyRows(s, open, matesOpen);

  // The list's height follows the pane (and the fly-out).
  useLayoutEffect(() => {
    const el = body.current;
    if (!el) return;
    const measure = () => setView((v) => (v.height === el.clientHeight && v.top === el.scrollTop ? v : { top: el.scrollTop, height: el.clientHeight }));
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  // The real row height (the fly-out's rows are as tall as the pane's; a theme may differ).
  useLayoutEffect(() => {
    const r = body.current?.querySelector<HTMLElement>(".tree-row");
    if (r && r.offsetHeight > 0 && r.offsetHeight !== rowHeight) setRowHeight(r.offsetHeight);
  });

  // Selection made elsewhere, or a rename started from a menu: bring the row into view.
  const target = app.renaming ?? [...app.selection].reverse().find((x): x is Extract<SelItem, { kind: "feature" }> => x.kind === "feature")?.id ?? null;
  const shown = useRef<string | null>(null);
  useEffect(() => {
    const el = body.current;
    if (!el || target === shown.current) return;
    shown.current = target;
    if (target == null || app.opt<boolean>("fm.scrollIntoView") === false) return;
    const k = rows.findIndex((r) => r.id === target);
    const to = k < 0 ? null : scrollToShow(k, el.scrollTop, el.clientHeight, rowHeight);
    if (to != null) el.scrollTop = to;
  });

  const { start, end } = visibleWindow(view.top, view.height, rowHeight, rows.length, OVERSCAN);
  const toggle = (r: TreeRow) => {
    if (r.id === "$mates") setMatesOpen(!matesOpen);
    else setOpen({ ...open, [r.id]: !open[r.id] });
  };
  const row = (r: TreeRow) => {
    const item: SelItem = { kind: "feature", id: r.id };
    const selected = app.isSelected(item);
    const renaming = r.renameable && app.renaming === r.id;
    const folder = r.folder !== undefined;
    return (
      <div
        key={r.id}
        className={`tree-row ${selected ? "sel" : ""} ${r.muted ? "muted" : ""}`}
        style={{ paddingLeft: 6 + r.depth * 16 }}
        title={r.title ?? r.error ?? r.warning ?? undefined}
        data-asm-row={r.id}
        onClick={(e) => (pickable(r.id) ? app.select(item, e.ctrlKey ? "toggle" : e.shiftKey ? "add" : "replace") : folder && toggle(r))}
        onContextMenu={(e) => {
          e.preventDefault();
          if (pickable(r.id)) onContext(e.clientX, e.clientY, item);
        }}
      >
        <span className="twisty" onClick={(e) => (e.stopPropagation(), folder && toggle(r))}>
          {folder ? (r.folder ? "▾" : "▸") : ""}
        </span>
        <Icon name={r.icon} size={16} />
        {r.color && <span className="asm-swatch" style={{ background: r.color }} />}
        {r.error ? <Icon name="error" size={12} /> : r.warning ? <Icon name="warning" size={12} /> : null}
        {renaming ? (
          <input
            autoFocus
            defaultValue={(s.doc.instances.find((x) => x.id === r.id) ?? s.doc.mates.find((x) => x.id === r.id))?.name ?? r.label}
            className="rename"
            data-asm-rename={r.id}
            onClick={(e) => e.stopPropagation()}
            onBlur={(e) => {
              s.rename(r.id, e.target.value);
              app.renaming = null;
              app.changed();
            }}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              if (e.key === "Escape") {
                app.renaming = null;
                app.changed();
              }
            }}
          />
        ) : (
          <span className="tree-label">{r.label}</span>
        )}
      </div>
    );
  };
  return (
    <div className="tree asm-tree">
      <div className="tree-body" ref={body} data-asm-rows={rows.length} onScroll={(e) => setView({ top: e.currentTarget.scrollTop, height: e.currentTarget.clientHeight })}>
        <div style={{ height: start * rowHeight }} />
        {rows.slice(start, end).map(row)}
        <div style={{ height: (rows.length - end) * rowHeight }} />
      </div>
    </div>
  );
}
