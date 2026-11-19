/**
 * Configurations and display states of an assembly (G1e; mirror of `capture_assembly`,
 * `apply_assembly` and `apply_display_state` in `crates/oc-doc/src/config.rs`).
 *
 * As in parts, the document holds the active configuration's state: instance and mate
 * suppression, mate values, the configuration each instance references (`Instance.config`)
 * and, from the active display state, each instance's hide/show, colour, display mode and
 * transparency. A configuration stores what is configured; a display state stores every
 * instance's look; each configuration is linked to a display state (SOLIDWORKS' "Link
 * Display States to Configurations", on by default).
 */
import type { Configuration, Id } from "../core/types";
import { configured, ensureConfigurations, findConfiguration, splitKey, getPath, setPath } from "../core/configurations";
import type { AssemblyDocument, DisplayState, Instance, Look } from "./types";

export const DEFAULT_DISPLAY_STATE = "Display State-1";

const lookOf = (i: Instance): Look => {
  const l: Look = {};
  if (i.hidden !== undefined) l.hidden = i.hidden;
  if (i.color !== undefined) l.color = i.color;
  if (i.display !== undefined) l.display = i.display;
  if (i.transparent !== undefined) l.transparent = i.transparent;
  return l;
};

export function looksOf(doc: AssemblyDocument): Record<Id, Look> {
  const out: Record<Id, Look> = {};
  for (const i of doc.instances) {
    const l = lookOf(i);
    if (Object.keys(l).length) out[i.id] = l;
  }
  return out;
}

/** The display states of an assembly; absent means one, holding the instances' looks. */
export function displayStatesOf(doc: AssemblyDocument): { active: Id; list: DisplayState[] } {
  return doc.displayStates ?? { active: "d1", list: [{ id: "d1", name: DEFAULT_DISPLAY_STATE }] };
}

export function ensureDisplayStates(doc: AssemblyDocument) {
  return (doc.displayStates ??= displayStatesOf(doc));
}

export function activeDisplayState(doc: AssemblyDocument): DisplayState {
  const d = displayStatesOf(doc);
  return d.list.find((x) => x.id === d.active) ?? d.list[0];
}

/** The configuration an instance references: its own, else its component's. */
export function referencedConfiguration(doc: AssemblyDocument, i: Instance): string {
  return i.config ?? doc.components.find((c) => c.id === i.component)?.config ?? "Default";
}

/** Refreshes the active configuration's and display state's entries from the document. */
export function captureAssembly(doc: AssemblyDocument) {
  const ds = doc.displayStates;
  if (ds) {
    const s = ds.list.find((x) => x.id === ds.active);
    if (s) {
      const looks = looksOf(doc);
      if (Object.keys(looks).length) s.instances = looks;
      else delete s.instances;
    }
  }
  const c = doc.configurations;
  const a = c?.list.find((x) => x.id === c.active);
  if (!c || !a) return;
  const { values, suppressed } = configured(c);
  for (const k of values) {
    const s = splitKey(k);
    const m = s && doc.mates.find((x) => x.id === s[0]);
    const v = m ? getPath(m, s[1]) : undefined;
    if (v !== undefined) (a.values ??= {})[k] = v;
  }
  for (const id of suppressed) {
    const x = doc.instances.find((i) => i.id === id) ?? doc.mates.find((m) => m.id === id);
    if (x) (a.suppressed ??= {})[id] = !!x.suppressed;
  }
  const comps = new Set(c.list.flatMap((x) => Object.keys(x.components ?? {})));
  for (const id of comps) {
    const i = doc.instances.find((x) => x.id === id);
    if (i) (a.components ??= {})[id] = referencedConfiguration(doc, i);
  }
  if (ds) a.displayState = ds.active;
}

/** Writes display state `id`'s looks into the instances and makes it active. */
export function applyDisplayState(doc: AssemblyDocument, id: Id) {
  const ds = ensureDisplayStates(doc);
  const s = ds.list.find((x) => x.id === id);
  if (!s) throw new Error(`No display state ${id}`);
  ds.active = s.id;
  for (const i of doc.instances) {
    const l = s.instances?.[i.id] ?? {};
    for (const k of ["hidden", "color", "display", "transparent"] as const) {
      if (l[k] === undefined) delete i[k];
      else (i as unknown as Record<string, unknown>)[k] = l[k];
    }
  }
}

/** Writes assembly configuration `idOrName` into the document and makes it active (capture first). */
export function applyAssembly(doc: AssemblyDocument, idOrName: string) {
  const c = doc.configurations;
  const t = c && findConfiguration(c, idOrName);
  if (!c || !t) throw new Error(`No configuration ${idOrName}`);
  for (const i of doc.instances) {
    const s = t.suppressed?.[i.id];
    if (s !== undefined) {
      if (s) i.suppressed = true;
      else delete i.suppressed;
    }
    const cfg = t.components?.[i.id];
    if (cfg !== undefined) {
      const own = doc.components.find((x) => x.id === i.component)?.config;
      if (cfg === own) delete i.config;
      else i.config = cfg;
    }
  }
  for (const m of doc.mates) {
    const s = t.suppressed?.[m.id];
    if (s !== undefined) {
      if (s) m.suppressed = true;
      else delete m.suppressed;
    }
    for (const [k, v] of Object.entries(t.values ?? {})) {
      const p = splitKey(k);
      if (p && p[0] === m.id) setPath(m, p[1], v);
    }
  }
  if (t.displayState && doc.displayStates?.list.some((x) => x.id === t.displayState)) applyDisplayState(doc, t.displayState);
  c.active = t.id;
}

function freeDisplayName(doc: AssemblyDocument): string {
  const ds = displayStatesOf(doc);
  let n = ds.list.length + 1;
  while (ds.list.some((x) => x.name === `Display State-${n}`)) n++;
  return `Display State-${n}`;
}

function freeDisplayId(doc: AssemblyDocument): Id {
  const ds = displayStatesOf(doc);
  let n = ds.list.length + 1;
  while (ds.list.some((x) => x.id === `d${n}`)) n++;
  return `d${n}`;
}

/** Add Display State: a copy of the current looks, made active and linked to the active configuration. */
export function addDisplayState(doc: AssemblyDocument, name?: string): DisplayState {
  // The first display state takes the current looks before the new one is added.
  const ds = ensureDisplayStates(doc);
  captureAssembly(doc);
  const n = (name ?? "").trim() || freeDisplayName(doc);
  if (ds.list.some((x) => x.name === n)) throw new Error(`A display state named "${n}" already exists.`);
  const looks = looksOf(doc);
  const s: DisplayState = { id: freeDisplayId(doc), name: n, ...(Object.keys(looks).length ? { instances: looks } : {}) };
  ds.list.push(s);
  ds.active = s.id;
  const c = ensureConfigurations(doc);
  const a = c.list.find((x) => x.id === c.active);
  if (a) a.displayState = s.id;
  return s;
}

/** Activates a display state; the active configuration is linked to it. */
export function activateDisplayState(doc: AssemblyDocument, id: Id) {
  captureAssembly(doc);
  applyDisplayState(doc, id);
  const c = doc.configurations;
  const a = c?.list.find((x) => x.id === c.active);
  if (a) a.displayState = id;
}

export function renameDisplayState(doc: AssemblyDocument, id: Id, name: string) {
  const ds = ensureDisplayStates(doc);
  const n = name.trim();
  const s = ds.list.find((x) => x.id === id);
  if (!s || !n) return;
  if (ds.list.some((x) => x.id !== id && x.name === n)) throw new Error(`A display state named "${n}" already exists.`);
  s.name = n;
}

/** Deletes a display state; not the active one, nor one another configuration is linked to. */
export function deleteDisplayState(doc: AssemblyDocument, id: Id) {
  const ds = doc.displayStates;
  if (!ds || ds.list.length <= 1) throw new Error("The only display state cannot be deleted.");
  if (ds.active === id) throw new Error("The active display state cannot be deleted.");
  const user = doc.configurations?.list.find((c: Configuration) => c.displayState === id);
  if (user) throw new Error(`Configuration "${user.name}" uses this display state.`);
  ds.list = ds.list.filter((x) => x.id !== id);
}
