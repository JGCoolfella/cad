/**
 * The assembly FeatureManager design tree (G1a), laid out as SOLIDWORKS lays it out: the
 * assembly, its origin, then each component with its state prefix, (f) fixed, (-) under
 * defined, (+) over defined, none when fully defined, then the Mates folder. Mates in
 * error show the error icon; a mate whose reference no longer resolves says Missing.
 */
import { useState } from "react";
import type { App, SelItem } from "../app/app";
import { Icon } from "../ui/icons";
import { MATE_LABEL, type MateType } from "./types";

const MATE_ICON: Record<MateType, string> = {
  coincident: "rel:coincident",
  parallel: "rel:parallel",
  perpendicular: "rel:perpendicular",
  tangent: "rel:tangent",
  concentric: "rel:concentric",
  lock: "rel:fix",
  distance: "dimH",
  angle: "smartDimension",
};

const PREFIX: Record<string, string> = { fixed: "(f) ", under: "(-) ", over: "(+) ", full: "", suppressed: "" };

export function AssemblyTree({ app, onContext }: { app: App; onContext: (x: number, y: number, item: SelItem) => void }) {
  const s = app.asm!;
  const [matesOpen, setMatesOpen] = useState(true);
  const built = s.built;
  const row = (id: string, icon: string, label: string, o: { depth?: number; muted?: boolean; error?: string | null; warning?: string | null; title?: string; folder?: [boolean, () => void] } = {}) => {
    const item: SelItem = { kind: "feature", id };
    const selected = app.isSelected(item);
    return (
      <div
        key={id}
        className={`tree-row ${selected ? "sel" : ""} ${o.muted ? "muted" : ""}`}
        style={{ paddingLeft: 6 + (o.depth ?? 0) * 16 }}
        title={o.title ?? o.error ?? o.warning ?? undefined}
        data-asm-row={id}
        onClick={(e) => (id.startsWith("$") ? o.folder?.[1]() : app.select(item, e.ctrlKey ? "toggle" : e.shiftKey ? "add" : "replace"))}
        onContextMenu={(e) => {
          e.preventDefault();
          if (!id.startsWith("$")) onContext(e.clientX, e.clientY, item);
        }}
      >
        <span className="twisty" onClick={(e) => (e.stopPropagation(), o.folder?.[1]())}>
          {o.folder ? (o.folder[0] ? "▾" : "▸") : ""}
        </span>
        <Icon name={icon} size={16} />
        {o.error ? <Icon name="error" size={12} /> : o.warning ? <Icon name="warning" size={12} /> : null}
        <span className="tree-label">{label}</span>
      </div>
    );
  };
  const comp = new Map((built?.components ?? []).map((c) => [c.id, c]));
  return (
    <div className="tree">
      {row("$asm", "assembly", `${s.name}${built ? ` (Default)` : ""}`, { title: built ? `${built.dof} degrees of freedom` : undefined })}
      {row("$history", "history", "History", { depth: 1, muted: true })}
      {row("$origin", "origin", "Origin", { depth: 1 })}
      {s.doc.instances.map((i) => {
        const st = s.instanceState(i.id);
        const c = comp.get(i.component);
        const state = i.suppressed ? "suppressed" : st?.state ?? (i.fixed ? "fixed" : "under");
        const part = c?.part ? built?.parts[c.part] : undefined;
        const warn = c?.warning ?? (part?.errors.length ? part.errors.map((e) => e.message).join("; ") : null);
        return row(i.id, "component", `${PREFIX[state] ?? ""}${i.name}`, {
          depth: 1,
          muted: !!i.suppressed || !!i.hidden,
          error: c?.error,
          warning: warn,
          title: c?.error ?? warn ?? `${state === "fixed" ? "Fixed" : state === "full" ? "Fully defined" : state === "over" ? "Over defined" : state === "suppressed" ? "Suppressed" : `Under defined: ${st?.free ?? 6} degrees of freedom`}${part?.lightweight ? " (lightweight)" : ""}${c?.path ? ` — ${c.path}` : ""}`,
        });
      })}
      {row("$mates", "mate", "Mates", { depth: 1, folder: [matesOpen, () => setMatesOpen(!matesOpen)] })}
      {matesOpen &&
        s.doc.mates.map((m) => {
          const st = s.mateStatus(m.id);
          const bad = st && (st.status === "redundant" || st.status === "conflict" || st.status === "invalid");
          const missing = st?.status === "missing";
          const names = m.refs.map((r, k) => (st?.missing.includes(k) ? "**Missing**" : s.doc.instances.find((i) => i.id === r.occurrence[0])?.name ?? "Assembly"));
          const label = `${m.name} (${names.join(",")})`;
          return row(m.id, MATE_ICON[m.type] ?? "mate", label, {
            depth: 2,
            muted: !!m.suppressed,
            error: bad ? st!.message ?? st!.status : missing ? "A reference of this mate is missing." : null,
            title: `${MATE_LABEL[m.type]}${st ? `: ${st.status}` : ""}`,
          });
        })}
    </div>
  );
}
