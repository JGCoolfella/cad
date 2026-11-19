/**
 * Configurations of a part (G1e; mirror of `crates/oc-doc/src/config.rs` and
 * `crates/oc-graph/src/config.rs`). SOLIDWORKS' ConfigurationManager: named variants that
 * differ in configured dimension values and feature suppression.
 *
 * The features always hold the active configuration's values; a configuration stores what
 * is configured. Every configured key has an entry in every configuration, so activating
 * one writes its entries and re-solves the sketches whose dimensions changed. Before the
 * active configuration is left or copied, its entries are refreshed from the features
 * (`capturePart`), so an edit made in it (in a PropertyManager, the sketch editor or the
 * Modify box) stays with it. A configuration's sketch solutions are written back with its
 * values, so activating it again gives the same document bit for bit and the engine's
 * cache for it hits.
 */
import type { Configuration, Configurations, Feature, Id, PartDocument, SketchData, SketchState } from "./types";

export const DEFAULT_CONFIGURATION = "Default";

/** `"<id>/<path>"` → [id, path segments]; null when malformed. */
export function splitKey(key: string): [string, string[]] | null {
  const i = key.indexOf("/");
  if (i <= 0) return null;
  const path = key.slice(i + 1).split(".");
  return path.every((p) => p.length > 0) ? [key.slice(0, i), path] : null;
}

export function getPath(o: unknown, path: string[]): number | undefined {
  let v: unknown = o;
  for (const k of path) {
    if (!v || typeof v !== "object") return undefined;
    v = (v as Record<string, unknown>)[k];
  }
  return typeof v === "number" ? v : undefined;
}

/** Sets an existing number at `path`; a configuration never invents a field. */
export function setPath(o: unknown, path: string[], x: number): boolean {
  let v: unknown = o;
  for (const k of path.slice(0, -1)) {
    if (!v || typeof v !== "object") return false;
    v = (v as Record<string, unknown>)[k];
  }
  const last = path[path.length - 1];
  if (!v || typeof v !== "object" || !(last in (v as object))) return false;
  (v as Record<string, unknown>)[last] = x;
  return true;
}

/** Key of a feature parameter (`featureParams` key) or a sketch dimension. */
export function paramKey(featureId: Id, key: string): string {
  return `${featureId}/${key}`;
}
export function sketchDimKey(sketchId: Id, dimId: Id): string {
  return `${sketchId}/sketch.dimensions.${dimId}.value`;
}

/** The configurations of a document; absent means one, Default (not stored until a second is added). */
export function configurationsOf(doc: { configurations?: Configurations }): Configurations {
  return doc.configurations ?? { active: "c1", list: [{ id: "c1", name: DEFAULT_CONFIGURATION }] };
}

export function activeConfiguration(doc: { configurations?: Configurations }): Configuration {
  const c = configurationsOf(doc);
  return c.list.find((x) => x.id === c.active) ?? c.list[0];
}

/** By id, else by name. */
export function findConfiguration(c: Configurations, idOrName: string): Configuration | undefined {
  return c.list.find((x) => x.id === idOrName) ?? c.list.find((x) => x.name === idOrName);
}

/** The keys configured anywhere, and the ids whose suppression is. */
export function configured(c: Configurations): { values: string[]; suppressed: Id[] } {
  const values = new Set<string>();
  const suppressed = new Set<Id>();
  for (const x of c.list) {
    Object.keys(x.values ?? {}).forEach((k) => values.add(k));
    Object.keys(x.suppressed ?? {}).forEach((k) => suppressed.add(k));
  }
  return { values: [...values].sort(), suppressed: [...suppressed].sort() };
}

export function isValueConfigured(doc: { configurations?: Configurations }, key: string): boolean {
  return !!doc.configurations?.list.some((c) => c.values && key in c.values);
}

export function isSuppressionConfigured(doc: { configurations?: Configurations }, id: Id): boolean {
  return !!doc.configurations?.list.some((c) => c.suppressed && id in c.suppressed);
}

const feature = (doc: PartDocument, id: Id) => doc.features.find((f) => f.id === id);

/** Sketch features with a configured dimension: their solutions travel with the configurations. */
function configuredSketches(doc: PartDocument, c: Configurations): Id[] {
  const out: Id[] = [];
  for (const k of configured(c).values) {
    const s = splitKey(k);
    if (s && feature(doc, s[0])?.type === "sketch" && !out.includes(s[0])) out.push(s[0]);
  }
  return out;
}

export function sketchState(s: SketchData): SketchState {
  const points: Record<Id, [number, number]> = {};
  for (const [k, p] of Object.entries(s.points)) points[k] = [p.x, p.y];
  const radii: Record<Id, number> = {};
  for (const [k, e] of Object.entries(s.entities)) if (e.type === "circle") radii[k] = e.r;
  return Object.keys(radii).length ? { points, radii } : { points };
}

/** Makes the document's configurations explicit (one, Default, when there were none). */
export function ensureConfigurations(doc: { configurations?: Configurations }): Configurations {
  return (doc.configurations ??= configurationsOf(doc));
}

/** Refreshes the active configuration's entries from the features (values, suppression, sketch solutions). */
export function capturePart(doc: PartDocument) {
  const c = doc.configurations;
  if (!c) return;
  const a = c.list.find((x) => x.id === c.active);
  if (!a) return;
  const { values, suppressed } = configured(c);
  for (const k of values) {
    const s = splitKey(k);
    const f = s && feature(doc, s[0]);
    const v = f ? getPath(f, s[1]) : undefined;
    if (v !== undefined) (a.values ??= {})[k] = v;
  }
  for (const id of suppressed) {
    const f = feature(doc, id);
    if (f) (a.suppressed ??= {})[id] = !!f.suppressed;
  }
  const sketches = configuredSketches(doc, c);
  delete a.sketches;
  for (const id of sketches) {
    const f = feature(doc, id);
    if (f?.type === "sketch") (a.sketches ??= {})[id] = sketchState(f.sketch);
  }
}

/**
 * Activates configuration `idOrName`: captures the active one, writes the target's values,
 * suppression and sketch solutions into the features, and re-solves the sketches whose
 * dimensions or geometry were written (`solve`). Returns the features that changed.
 */
export function activatePart(doc: PartDocument, idOrName: string, solve: (sk: SketchData) => void): Feature[] {
  const c = doc.configurations;
  const target = c && findConfiguration(c, idOrName);
  if (!c || !target) throw new Error(`No configuration ${idOrName}`);
  capturePart(doc);
  const changed = new Set<Feature>();
  const touched = new Set<Feature>();
  for (const [k, x] of Object.entries(target.values ?? {})) {
    const s = splitKey(k);
    const f = s && feature(doc, s[0]);
    if (!f || getPath(f, s[1]) === x) continue;
    if (setPath(f, s[1], x)) {
      changed.add(f);
      if (f.type === "sketch") touched.add(f);
    }
  }
  for (const [id, sup] of Object.entries(target.suppressed ?? {})) {
    const f = feature(doc, id);
    if (!f || !!f.suppressed === sup) continue;
    if (sup) f.suppressed = true;
    else delete f.suppressed;
    changed.add(f);
  }
  for (const [id, st] of Object.entries(target.sketches ?? {})) {
    const f = feature(doc, id);
    if (f?.type !== "sketch") continue;
    const sk = f.sketch;
    const same =
      Object.entries(st.points ?? {}).every(([k, p]) => !sk.points[k] || (sk.points[k].x === p[0] && sk.points[k].y === p[1])) &&
      Object.entries(st.radii ?? {}).every(([k, r]) => {
        const e = sk.entities[k];
        return e?.type !== "circle" || e.r === r;
      });
    if (same) continue;
    for (const [k, p] of Object.entries(st.points ?? {})) {
      const q = sk.points[k];
      if (q) [q.x, q.y] = p;
    }
    for (const [k, r] of Object.entries(st.radii ?? {})) {
      const e = sk.entities[k];
      if (e?.type === "circle") e.r = r;
    }
    touched.add(f);
    changed.add(f);
  }
  for (const f of touched) if (f.type === "sketch") settle(f.sketch, solve);
  c.active = target.id;
  return [...changed];
}

/** How far a solve may move a sketch that already satisfies its dimensions (solver noise). */
const SETTLED = 1e-9;

/**
 * Re-solves a sketch whose dimensions or stored solution were written. When the solution
 * moves nothing by more than solver noise (the configuration's own solution, still valid),
 * the written geometry is kept as it was, so the document is the same bit for bit.
 */
function settle(sk: SketchData, solve: (sk: SketchData) => void) {
  const trial: SketchData = JSON.parse(JSON.stringify(sk));
  solve(trial);
  let moved = 0;
  for (const [k, p] of Object.entries(trial.points)) {
    const q = sk.points[k];
    if (q) moved = Math.max(moved, Math.abs(p.x - q.x), Math.abs(p.y - q.y));
  }
  for (const [k, e] of Object.entries(trial.entities)) {
    const q = sk.entities[k];
    if (e.type === "circle" && q?.type === "circle") moved = Math.max(moved, Math.abs(e.r - q.r));
  }
  if (moved <= SETTLED) return;
  for (const [k, p] of Object.entries(trial.points)) {
    const q = sk.points[k];
    if (q) [q.x, q.y] = [p.x, p.y];
  }
  for (const [k, e] of Object.entries(trial.entities)) {
    const q = sk.entities[k];
    if (e.type === "circle" && q?.type === "circle") q.r = e.r;
  }
}

function uniqueId(c: Configurations): Id {
  let n = c.list.length + 1;
  while (c.list.some((x) => x.id === `c${n}`)) n++;
  return `c${n}`;
}

/** A configuration name not yet used: `base`, else `base2`, `base3`... */
export function freeName(c: Configurations, base: string): string {
  if (!c.list.some((x) => x.name === base)) return base;
  let n = 2;
  while (c.list.some((x) => x.name === `${base}${n}`)) n++;
  return `${base}${n}`;
}

/**
 * Add Configuration: a copy of the active configuration's state under a new name, made
 * active (SOLIDWORKS activates a configuration it adds). Returns it, or throws on a name in use.
 */
export function addConfiguration(doc: { configurations?: Configurations }, name: string, description?: string, capture?: () => void): Configuration {
  const c = ensureConfigurations(doc);
  const n = name.trim();
  if (!n) throw new Error("Enter a configuration name.");
  if (c.list.some((x) => x.name === n)) throw new Error(`A configuration named "${n}" already exists.`);
  capture?.();
  const from = c.list.find((x) => x.id === c.active) ?? c.list[0];
  const copy: Configuration = JSON.parse(JSON.stringify(from));
  copy.id = uniqueId(c);
  copy.name = n;
  if (description?.trim()) copy.description = description.trim();
  else delete copy.description;
  c.list.push(copy);
  c.active = copy.id;
  return copy;
}

export function renameConfiguration(doc: { configurations?: Configurations }, id: Id, name: string) {
  const c = ensureConfigurations(doc);
  const n = name.trim();
  const x = c.list.find((y) => y.id === id);
  if (!x || !n) return;
  if (c.list.some((y) => y.id !== id && y.name === n)) throw new Error(`A configuration named "${n}" already exists.`);
  x.name = n;
}

/** Deletes a configuration. The active one and the last one cannot be deleted (as in SOLIDWORKS). */
export function deleteConfiguration(doc: { configurations?: Configurations }, id: Id) {
  const c = doc.configurations;
  if (!c) throw new Error("The only configuration cannot be deleted.");
  if (c.active === id) throw new Error("The active configuration cannot be deleted. Activate another one first.");
  if (c.list.length <= 1) throw new Error("The only configuration cannot be deleted.");
  c.list = c.list.filter((x) => x.id !== id);
}

export type Scope = "this" | "all";

/**
 * Sets a dimension or feature parameter (`key` = `<id>/<path>`) to `value` through `set`
 * (which writes the feature), for this configuration or all of them, as the Modify box's
 * configurations option does. "This configuration" configures the key: every other
 * configuration keeps the value it had. "All configurations" sets it everywhere (the key
 * is no longer configured). For a sketch dimension, `sketch` is the sketch's solution
 * before the change: configurations that keep the old value keep that solution too.
 */
export function setConfiguredValue(doc: { configurations?: Configurations }, key: string, before: number, value: number, scope: Scope, sketch?: { id: Id; state: SketchState }) {
  const c = doc.configurations;
  if (!c) return;
  if (scope === "all") {
    for (const x of c.list) if (x.values) delete x.values[key];
    return;
  }
  for (const x of c.list) {
    const v = (x.values ??= {});
    if (x.id === c.active) v[key] = value;
    else if (!(key in v)) {
      v[key] = before;
      if (sketch && !x.sketches?.[sketch.id]) (x.sketches ??= {})[sketch.id] = JSON.parse(JSON.stringify(sketch.state));
    }
  }
}

/** Suppression of `id` in this configuration or all of them (the item itself is written by the caller). */
export function setConfiguredSuppression(doc: { configurations?: Configurations }, id: Id, before: boolean, suppressed: boolean, scope: Scope) {
  const c = doc.configurations;
  if (!c) return;
  if (scope === "all") {
    for (const x of c.list) if (x.suppressed) delete x.suppressed[id];
    return;
  }
  for (const x of c.list) {
    const s = (x.suppressed ??= {});
    if (x.id === c.active) s[id] = suppressed;
    else if (!(id in s)) s[id] = before;
  }
}

/** Drops entries of features (or instances and mates) that no longer exist. */
export function pruneConfigurations(doc: { configurations?: Configurations }, exists: (id: Id) => boolean) {
  for (const x of doc.configurations?.list ?? []) {
    for (const k of Object.keys(x.values ?? {})) if (!exists(splitKey(k)?.[0] ?? "")) delete x.values![k];
    for (const k of Object.keys(x.suppressed ?? {})) if (!exists(k)) delete x.suppressed![k];
    for (const k of Object.keys(x.sketches ?? {})) if (!exists(k)) delete x.sketches![k];
    for (const k of Object.keys(x.components ?? {})) if (!exists(k)) delete x.components![k];
  }
}

/** The tree header's text, as SOLIDWORKS shows it: `Part1 (Default)`. */
export function configurationTitle(doc: { name: string; configurations?: Configurations }, displayState?: string): string {
  const a = activeConfiguration(doc).name;
  return displayState ? `${doc.name} (${a}<${displayState}>)` : `${doc.name} (${a})`;
}
