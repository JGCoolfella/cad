/**
 * The assembly FeatureManager design tree (G1a, G1b), laid out as SOLIDWORKS lays it out:
 * the assembly, History, its Front, Top and Right Planes and Origin, then each component
 * with its state prefix, (f) fixed, (-) under defined, (+) over defined, none when fully
 * defined, then the Mates folder. Mates in error show the error icon; a mate whose
 * reference no longer resolves says Missing. A component expands to its own planes and
 * origin, which (like the assembly's) can be picked for mates. Hidden components are
 * greyed; renaming works as for features (`app.renaming`). The same tree is the fly-out
 * FeatureManager in the graphics area while an assembly PropertyManager is open.
 */
import { activeConfiguration } from "../core/configurations";
import { activeDisplayState, referencedConfiguration } from "./configurations";
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
const PLANES: [string, string][] = [
  ["front", "Front Plane"],
  ["top", "Top Plane"],
  ["right", "Right Plane"],
];

/** Rows that can be selected (and picked for mates): `$plane:*`, `$origin`, `<inst>/$…`. */
const pickable = (id: string) => !id.startsWith("$") || id.startsWith("$plane:") || id === "$origin";

export function AssemblyTree({ app, onContext }: { app: App; onContext: (x: number, y: number, item: SelItem) => void }) {
  const s = app.asm!;
  const [matesOpen, setMatesOpen] = useState(true);
  const [open, setOpen] = useState<Record<string, boolean>>({});
  const built = s.built;
  const row = (
    id: string,
    icon: string,
    label: string,
    o: { depth?: number; muted?: boolean; error?: string | null; warning?: string | null; title?: string; folder?: [boolean, () => void]; renameable?: boolean; color?: string } = {},
  ) => {
    const item: SelItem = { kind: "feature", id };
    const selected = app.isSelected(item);
    const renaming = o.renameable && app.renaming === id;
    return (
      <div
        key={id}
        className={`tree-row ${selected ? "sel" : ""} ${o.muted ? "muted" : ""}`}
        style={{ paddingLeft: 6 + (o.depth ?? 0) * 16 }}
        title={o.title ?? o.error ?? o.warning ?? undefined}
        data-asm-row={id}
        onClick={(e) => (pickable(id) ? app.select(item, e.ctrlKey ? "toggle" : e.shiftKey ? "add" : "replace") : o.folder?.[1]())}
        onContextMenu={(e) => {
          e.preventDefault();
          if (pickable(id)) onContext(e.clientX, e.clientY, item);
        }}
      >
        <span className="twisty" onClick={(e) => (e.stopPropagation(), o.folder?.[1]())}>
          {o.folder ? (o.folder[0] ? "▾" : "▸") : ""}
        </span>
        <Icon name={icon} size={16} />
        {o.color && <span className="asm-swatch" style={{ background: o.color }} />}
        {o.error ? <Icon name="error" size={12} /> : o.warning ? <Icon name="warning" size={12} /> : null}
        {renaming ? (
          <input
            autoFocus
            defaultValue={(s.doc.instances.find((x) => x.id === id) ?? s.doc.mates.find((x) => x.id === id))?.name ?? label}
            className="rename"
            data-asm-rename={id}
            onClick={(e) => e.stopPropagation()}
            onBlur={(e) => {
              s.rename(id, e.target.value);
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
          <span className="tree-label">{label}</span>
        )}
      </div>
    );
  };
  const refRows = (prefix: string, depth: number) => [
    ...PLANES.map(([p, l]) => row(`${prefix}$plane:${p}`, "plane", l, { depth })),
    row(`${prefix}$origin`, "origin", "Origin", { depth }),
  ];
  const comp = new Map((built?.components ?? []).map((c) => [c.id, c]));
  return (
    <div className="tree">
      {row("$asm", "assembly", `${s.name} (${activeConfiguration(s.doc).name}<${activeDisplayState(s.doc).name}>)`, { title: built ? `${built.dof} degrees of freedom` : undefined })}
      {row("$history", "history", "History", { depth: 1, muted: true })}
      {refRows("", 1)}
      {s.doc.instances.flatMap((i) => {
        const st = s.instanceState(i.id);
        const c = comp.get(i.component);
        const state = i.suppressed ? "suppressed" : (st?.state ?? (i.fixed ? "fixed" : "under"));
        const part = c?.part ? built?.parts[c.part] : undefined;
        const warn = c?.warning ?? (part?.errors.length ? part.errors.map((e) => e.message).join("; ") : null);
        const isOpen = !!open[i.id];
        const kind = s.componentOf(i)?.kind;
        return [
          // `->`: the part has in-context references (G1d), as SOLIDWORKS marks it.
          // The configuration it references, as SOLIDWORKS shows it after the name (G1e).
          row(i.id, kind === "assembly" ? "assembly" : "component", `${PREFIX[state] ?? ""}${i.name}${part?.external ? " ->" : ""} (${referencedConfiguration(s.doc, i)})`, {
            depth: 1,
            muted: !!i.suppressed || !!i.hidden,
            error: c?.error,
            warning: warn,
            renameable: true,
            color: i.color,
            folder: [isOpen, () => setOpen({ ...open, [i.id]: !isOpen })],
            title:
              c?.error ??
              warn ??
              `${state === "fixed" ? "Fixed" : state === "full" ? "Fully defined" : state === "over" ? "Over defined" : state === "suppressed" ? "Suppressed" : `Under defined: ${st?.free ?? 6} degrees of freedom`}${i.hidden ? " (hidden)" : ""}${part?.lightweight ? " (lightweight)" : ""}${c?.path ? ` — ${c.path}` : ""}`,
          }),
          ...(isOpen ? refRows(`${i.id}/`, 2) : []),
        ];
      })}
      {row("$mates", "mate", "Mates", { depth: 1, folder: [matesOpen, () => setMatesOpen(!matesOpen)] })}
      {matesOpen &&
        s.doc.mates.map((m) => {
          const st = s.mateStatus(m.id);
          const bad = st && (st.status === "redundant" || st.status === "conflict" || st.status === "invalid");
          const missing = st?.status === "missing";
          const names = m.refs.map((r, k) => (st?.missing.includes(k) ? "**Missing**" : (s.doc.instances.find((i) => i.id === r.occurrence[0])?.name ?? s.name)));
          const label = `${m.name} (${names.join(",")})`;
          return row(m.id, MATE_ICON[m.type] ?? "mate", label, {
            depth: 2,
            muted: !!m.suppressed,
            renameable: true,
            error: bad ? (st!.message ?? st!.status) : missing ? "A reference of this mate is missing." : null,
            title: `${MATE_LABEL[m.type]}${st ? `: ${st.status}` : ""}`,
          });
        })}
    </div>
  );
}
