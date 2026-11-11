import { useState } from "react";
import type { App, SelItem } from "../app/app";
import type { Ctx } from "../commands/commands";
import type { Feature } from "../core/types";
import { sketchesOf } from "../core/features";
import { Icon } from "./icons";

const FEATURE_ICON: Record<Feature["type"], string> = {
  sketch: "sketch",
  extrude: "extrude",
  revolve: "revolve",
  fillet: "fillet",
  chamfer: "chamfer",
  refplane: "plane",
  linearPattern: "linearPattern",
  circularPattern: "circularPattern",
  mirror: "mirror",
  shell: "shell",
  sweep: "swept",
  hole: "hole",
  draft: "draft",
  loft: "lofted",
  refaxis: "refAxis",
  refpoint: "refPoint",
  coordsys: "coordSys",
};

/** FeatureManager design tree. */
export function FeatureTree({ app, ctx, compact, onContext }: { app: App; ctx: Ctx; compact?: boolean; onContext: (x: number, y: number, item: SelItem) => void }) {
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  // Rename state lives on the app so the context menu's "Rename tree item" can start it.
  const renaming = app.renaming;
  const setRenaming = (id: string | null) => {
    app.renaming = id;
    app.changed();
  };
  // "Collapse Items" bumps a counter; collapse everything when it changes.
  const [collapseSeen, setCollapseSeen] = useState(app.treeCollapse);
  if (collapseSeen !== app.treeCollapse) {
    setCollapseSeen(app.treeCollapse);
    setExpanded({});
  }
  const [filter, setFilter] = useState("");
  void ctx;
  const statuses = new Map((app.result?.statuses ?? []).map((s) => [s.id, s]));
  const consumedBy = new Map<string, string>();
  for (const f of app.doc.features) for (const s of sketchesOf(f)) consumedBy.set(s, f.id);
  const rb = app.doc.rollback;
  /** FeatureManager > Hide/show tree items. */
  const show = (k: string) => app.opt(`fm.tree.${k}`) !== "hide";

  const row = (item: SelItem, icon: string, label: string, opts: { depth?: number; key: string; muted?: boolean; error?: string; expandable?: boolean; isOpen?: boolean; onToggle?: () => void; featureId?: string }) => {
    const selected = app.isSelected(item);
    const hovered = app.hover && JSON.stringify(app.hover) === JSON.stringify(item);
    if (filter && !label.toLowerCase().includes(filter.toLowerCase())) return null;
    return (
      <div
        key={opts.key}
        className={`tree-row ${selected ? "sel" : ""} ${hovered ? "hov" : ""} ${opts.muted ? "muted" : ""}`}
        style={{ paddingLeft: 6 + (opts.depth ?? 0) * 16 }}
        onMouseEnter={() => app.setHover(item)}
        onMouseLeave={() => app.setHover(null)}
        onClick={(e) => app.select(item, e.ctrlKey ? "toggle" : e.shiftKey ? "add" : "replace")}
        onDoubleClick={() => opts.featureId && app.editFeature(opts.featureId)}
        onContextMenu={(e) => {
          e.preventDefault();
          onContext(e.clientX, e.clientY, item);
        }}
        onKeyDown={(e) => {
          if (e.key === "F2" && opts.featureId) setRenaming(opts.featureId);
        }}
        tabIndex={0}
      >
        <span className="twisty" onClick={(e) => (e.stopPropagation(), opts.onToggle?.())}>
          {opts.expandable ? (opts.isOpen ? "▾" : "▸") : ""}
        </span>
        <Icon name={icon} size={16} />
        {opts.error && (
          <span title={opts.error} className="err-ic">
            <Icon name="error" size={12} />
          </span>
        )}
        {renaming && renaming === opts.featureId ? (
          <input
            autoFocus
            defaultValue={label}
            className="rename"
            onClick={(e) => e.stopPropagation()}
            onBlur={(e) => (app.renameFeature(opts.featureId!, e.target.value), setRenaming(null))}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
              if (e.key === "Escape") setRenaming(null);
            }}
          />
        ) : (
          <span className="tlabel">{label}</span>
        )}
      </div>
    );
  };

  const rows: (JSX.Element | null)[] = [];
  const feats = app.doc.features;
  feats.forEach((f, idx) => {
    if (rb !== null && idx === rb) rows.push(<RollbackBar key="rb" app={app} index={idx} />);
    if (f.type === "sketch" && consumedBy.has(f.id)) return; // shown under its feature
    const st = statuses.get(f.id);
    const isOpen = expanded[f.id];
    const children = sketchesOf(f).map((id) => feats.find((s) => s.id === id)).filter((x): x is (typeof feats)[number] => !!x);
    const rolled = rb !== null && idx >= rb;
    rows.push(
      row({ kind: "feature", id: f.id }, f.type === "refplane" && app.isPlaneVisible(f.id) ? "planeShown" : FEATURE_ICON[f.type], f.name + (app.sketch?.featureId === f.id ? " (editing)" : ""), {
        key: f.id,
        muted: f.suppressed || rolled,
        error: st?.error,
        expandable: children.length > 0,
        isOpen,
        onToggle: () => setExpanded({ ...expanded, [f.id]: !isOpen }),
        featureId: f.id,
      }),
    );
    if (isOpen) for (const c of children) rows.push(row({ kind: "feature", id: c.id }, "sketch", c.name, { key: c.id, depth: 1, featureId: c.id, muted: rolled }));
  });
  if (rb === null) rows.push(<RollbackBar key="rb" app={app} index={null} />);

  return (
    <div className={`tree ${compact ? "compact" : ""}`}>
      {!compact && (
        <div className="tree-filter">
          <span>⏷</span>
          <input placeholder="" value={filter} onChange={(e) => setFilter(e.target.value)} title="Filter the FeatureManager design tree" />
        </div>
      )}
      <div className="tree-body">
        <div className="tree-row root">
          <span className="twisty" />
          <Icon name="part" size={16} />
          <span className="tlabel">
            {app.doc.name} (Default) &lt;&lt;Default&gt;_Display State 1&gt;
          </span>
        </div>
        {show("history") && (
          <div className="tree-row" style={{ paddingLeft: 6 }}>
            <span className="twisty" />
            <Icon name="history" size={16} />
            <span className="tlabel">History</span>
          </div>
        )}
        {show("sensors") && (
          <div className="tree-row" style={{ paddingLeft: 6 }}>
            <span className="twisty" />
            <Icon name="sensor" size={16} />
            <span className="tlabel">Sensors</span>
          </div>
        )}
        {show("annotations") && (
          <div className="tree-row" style={{ paddingLeft: 6 }}>
            <span className="twisty">▸</span>
            <Icon name="annotations" size={16} />
            <span className="tlabel">Annotations</span>
          </div>
        )}
        {show("material") && (
          <div className="tree-row" style={{ paddingLeft: 6 }}>
            <span className="twisty" />
            <Icon name="material" size={16} />
            <span className="tlabel">Material &lt;not specified&gt;</span>
          </div>
        )}
        {show("defaultPlanes") && (["front", "top", "right"] as const).map((id) => row({ kind: "plane", id }, app.isPlaneVisible(id) ? "planeShown" : "plane", `${id[0].toUpperCase()}${id.slice(1)} Plane`, { key: id }))}
        {show("origin") && row({ kind: "origin" }, "origin", "Origin", { key: "origin" })}
        {rows}
      </div>
    </div>
  );
}

function RollbackBar({ app, index }: { app: App; index: number | null }) {
  const [drag, setDrag] = useState(false);
  return (
    <div
      className={`rollback ${drag ? "drag" : ""}`}
      title="Rollback bar: drag up/down to roll back the model"
      onMouseDown={(e) => {
        e.preventDefault();
        setDrag(true);
        const rowsEls = Array.from(document.querySelectorAll(".tree:not(.compact) .tree-row[data-fidx]"));
        void rowsEls;
        const startY = e.clientY;
        const start = index ?? app.doc.features.length;
        const move = (ev: MouseEvent) => {
          const d = Math.round((ev.clientY - startY) / 22);
          const n = Math.max(0, Math.min(app.doc.features.length, start + d));
          const want = n >= app.doc.features.length ? null : n;
          if (want !== app.doc.rollback) app.setRollback(want);
        };
        const up = () => {
          setDrag(false);
          window.removeEventListener("mousemove", move);
          window.removeEventListener("mouseup", up);
        };
        window.addEventListener("mousemove", move);
        window.addEventListener("mouseup", up);
      }}
    />
  );
}
