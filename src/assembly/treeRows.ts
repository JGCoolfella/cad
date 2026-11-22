/**
 * The assembly FeatureManager's rows (G1a–G1e layout) as plain data, and the window of rows
 * a virtualised list draws (G3d: a 10 000-component assembly draws only the rows in view).
 * `AssemblyTree.tsx` renders them. Lookups are by map, so laying out n components is O(n).
 */
import { activeConfiguration } from "../core/configurations";
import { activeDisplayState } from "./configurations";
import { MATE_LABEL, type AssemblyDocument, type BuildReply, type MateType } from "./types";

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

export interface TreeRow {
  id: string;
  icon: string;
  label: string;
  depth: number;
  muted?: boolean;
  error?: string | null;
  warning?: string | null;
  title?: string;
  /** A folder row: whether it is open. */
  folder?: boolean;
  renameable?: boolean;
  color?: string;
}

/** What the rows are made from (an `AssemblySession`, or a test's stand-in). */
export interface TreeSource {
  doc: AssemblyDocument;
  built: BuildReply | null;
  name: string;
}

/**
 * The rows of the tree: the assembly, History, its planes and origin, each component (with
 * its state prefix, `->` for in-context references, its configuration; expanded: its planes
 * and origin), then the Mates folder and, open, its mates. `open`: expanded components.
 */
export function assemblyRows(s: TreeSource, open: Record<string, boolean>, matesOpen: boolean): TreeRow[] {
  const { doc, built } = s;
  const rows: TreeRow[] = [];
  const refRows = (prefix: string, depth: number) => {
    for (const [p, l] of PLANES) rows.push({ id: `${prefix}$plane:${p}`, icon: "plane", label: l, depth });
    rows.push({ id: `${prefix}$origin`, icon: "origin", label: "Origin", depth });
  };
  rows.push({ id: "$asm", icon: "assembly", label: `${s.name} (${activeConfiguration(doc).name}<${activeDisplayState(doc).name}>)`, depth: 0, title: built ? `${built.dof} degrees of freedom` : undefined });
  rows.push({ id: "$history", icon: "history", label: "History", depth: 1, muted: true });
  refRows("", 1);
  const comp = new Map((built?.components ?? []).map((c) => [c.id, c] as const));
  const docComp = new Map(doc.components.map((c) => [c.id, c] as const));
  const states = new Map((built?.instances ?? []).map((i) => [i.id, i] as const));
  const names = new Map(doc.instances.map((i) => [i.id, i.name] as const));
  for (const i of doc.instances) {
    const st = states.get(i.id);
    const c = comp.get(i.component);
    const own = docComp.get(i.component);
    const state = i.suppressed ? "suppressed" : (st?.state ?? (i.fixed ? "fixed" : "under"));
    const part = c?.part ? built?.parts[c.part] : undefined;
    const warn = c?.warning ?? (part?.errors.length ? part.errors.map((e) => e.message).join("; ") : null);
    const isOpen = !!open[i.id];
    // `->`: the part has in-context references (G1d), as SOLIDWORKS marks it. The
    // configuration it references, as SOLIDWORKS shows it after the name (G1e).
    const config = i.config ?? own?.config ?? "Default";
    rows.push({
      id: i.id,
      icon: own?.kind === "assembly" ? "assembly" : "component",
      label: `${PREFIX[state] ?? ""}${i.name}${part?.external ? " ->" : ""} (${config})`,
      depth: 1,
      muted: !!i.suppressed || !!i.hidden,
      error: c?.error,
      warning: warn,
      renameable: true,
      color: i.color,
      folder: isOpen,
      title:
        c?.error ??
        warn ??
        `${state === "fixed" ? "Fixed" : state === "full" ? "Fully defined" : state === "over" ? "Over defined" : state === "suppressed" ? "Suppressed" : `Under defined: ${st?.free ?? 6} degrees of freedom`}${i.hidden ? " (hidden)" : ""}${part?.lightweight ? " (lightweight)" : ""}${c?.path ? ` — ${c.path}` : ""}`,
    });
    if (isOpen) refRows(`${i.id}/`, 2);
  }
  rows.push({ id: "$mates", icon: "mate", label: "Mates", depth: 1, folder: matesOpen });
  if (matesOpen) {
    const status = new Map((built?.mates ?? []).map((m) => [m.id, m] as const));
    for (const m of doc.mates) {
      const st = status.get(m.id);
      const bad = st && (st.status === "redundant" || st.status === "conflict" || st.status === "invalid");
      const missing = st?.status === "missing";
      const refs = m.refs.map((r, k) => (st?.missing.includes(k) ? "**Missing**" : (names.get(r.occurrence[0]) ?? s.name)));
      rows.push({
        id: m.id,
        icon: MATE_ICON[m.type] ?? "mate",
        label: `${m.name} (${refs.join(",")})`,
        depth: 2,
        muted: !!m.suppressed,
        renameable: true,
        error: bad ? (st!.message ?? st!.status) : missing ? "A reference of this mate is missing." : null,
        title: `${MATE_LABEL[m.type]}${st ? `: ${st.status}` : ""}`,
      });
    }
  }
  return rows;
}

/** Rows `[start, end)` a list scrolled to `scrollTop` and `height` high shows, with
 * `overscan` rows either side. Before the list has a height, a screenful is assumed. */
export function visibleWindow(scrollTop: number, height: number, rowHeight: number, count: number, overscan: number): { start: number; end: number } {
  const h = height > 0 ? height : 40 * rowHeight;
  const first = Math.floor(Math.max(0, scrollTop) / rowHeight);
  const last = Math.ceil((Math.max(0, scrollTop) + h) / rowHeight);
  return { start: Math.max(0, Math.min(count, first - overscan)), end: Math.min(count, last + overscan) };
}

/** The scroll position that brings row `index` into view with the least movement; null
 * when it is in view already. */
export function scrollToShow(index: number, scrollTop: number, height: number, rowHeight: number): number | null {
  const top = index * rowHeight;
  if (top < scrollTop) return top;
  if (top + rowHeight > scrollTop + height) return top + rowHeight - height;
  return null;
}
