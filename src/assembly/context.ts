/**
 * Edit Part in context (G1d), as SOLIDWORKS' Edit Part: a part component of the open
 * assembly is edited inside it. Its document becomes the open one (its features, sketches
 * and PropertyManagers work as usual) while the assembly waits on `App.asmStack`, as for
 * Open Part; the other components stay in the graphics area at their places, transparent
 * (Options > Display > Assembly transparency for in context edit), and their faces and
 * edges can be picked. The view does not jump: the part is edited in its own coordinates
 * and the camera is re-expressed in them.
 *
 * In-context (external) references, kept with the part as SOLIDWORKS keeps them (`->` in
 * the FeatureManager, `->?` when the part is open without that assembly): a sketch on a
 * planar face of another component (`PlaneRef` kind "external") and Convert Entities of
 * another component's edges (`SketchRef` kind "external"). Each stores the referenced
 * entity as a mate reference does (persistent name and signature, in its own part's
 * coordinates) and its geometry in this part's coordinates as last updated; the rebuild
 * uses the stored geometry. The assembly updates it (`updateExternals`) when it is built:
 * the entity is re-found by name on the referenced occurrence, placed relative to this
 * part's occurrence, and the part is edited in memory if it moved (an unsaved component
 * edit, saved with the assembly).
 */
import type { App, SelItem } from "../app/app";
import type { EdgeSig, ExternalRef, FaceSig, Feature, Frame, Id, PartDocument, PlaneRef, Vec3 } from "../core/types";
import { invoke } from "../native/tauri";
import { sketchesOf } from "../core/features";
import { convertExternal } from "../sketch/refs";
import type { AssemblySession } from "./session";
import { compose, inverse } from "./session";
import { CTX_BODY, isContextItem } from "./contextIds";
import { apply, qrot, type MateEntity, type Transform } from "./types";

export { CTX_BODY, isContextItem };


/** The part edited in context: its instance, occurrence path and placement in the assembly. */
export interface ContextEntry {
  instance: Id;
  path: Id[];
  world: Transform;
}

/** The assembly the open part is being edited in (the top of `asmStack`), or null. */
export function inContext(app: App): { session: AssemblySession; context: ContextEntry; file: string } | null {
  const top = app.asmStack[app.asmStack.length - 1];
  return top?.context && !app.asm ? { session: top.session, context: top.context, file: top.file } : null;
}


const pathKey = (p: Id[]) => p.join("/");

/** A context pick resolved: the entity in its part's coordinates, its occurrence. */
export interface ContextPick {
  occurrence: Id[];
  entity: MateEntity;
  world: Transform;
  label: string;
}

/** Context picks resolved so far, by selection key (`face:2000003:5`). */
const resolved = new Map<string, Promise<ContextPick | null>>();

/** Resolves a context pick (asks the engine for the entity, as the Mate PropertyManager does). */
export function resolveContextPick(app: App, item: SelItem): Promise<ContextPick | null> {
  const c = inContext(app);
  if (!c || !isContextItem(item)) return Promise.resolve(null);
  const key = `${item.kind}:${item.body}:${item.id}`;
  let p = resolved.get(key);
  if (!p) {
    const occ = c.session.occurrences()[item.body - CTX_BODY];
    p = !occ
      ? Promise.resolve(null)
      : invoke<MateEntity>("asm_entity", { part: occ.part, kind: item.kind, index: item.id, quality: c.session.quality })
          .then((entity) => ({ occurrence: occ.path, entity, world: occ.world, label: c.session.refLabel({ occurrence: occ.path, entity }) }))
          .catch(() => null);
    resolved.set(key, p);
  }
  return p;
}

/** Forget resolved picks (the assembly was rebuilt, or the edit ended). */
export function clearContextPicks() {
  resolved.clear();
}

// ------------------------------------------------------------------ geometry

/** `world` (another occurrence's placement) re-expressed in the edited part's coordinates. */
function toPart(context: ContextEntry, world: Transform): Transform {
  return compose(inverse(context.world), world);
}

const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];

/** A face signature placed by `t` (in this part's coordinates; no name: it is another part's). */
export function placedFaceSig(sig: FaceSig, t: Transform): FaceSig {
  return { surface: sig.surface, center: apply(t, sig.center), normal: qrot(t.rotation, sig.normal), area: sig.area };
}

export function placedEdgeSig(sig: EdgeSig, t: Transform): EdgeSig {
  return { curve: sig.curve, a: apply(t, sig.a), b: apply(t, sig.b), mid: apply(t, sig.mid), length: sig.length };
}

function externalOf(session: AssemblySession, context: ContextEntry, pick: ContextPick): ExternalRef {
  return { assembly: (session.path ?? "").replace(/\\/g, "/"), component: pick.occurrence, context: context.path, entity: pick.entity, label: pick.label };
}

/**
 * Sketch on a planar face of another component (Insert > Sketch with it selected): the
 * plane, as an in-context reference, and its frame. Null if the pick is not a planar face,
 * not resolved yet, or Options > External References forbids references external to the
 * model.
 */
export async function externalPlane(app: App, item: SelItem): Promise<{ ref: PlaneRef; sig: FaceSig } | null> {
  const c = inContext(app);
  if (!c || item.kind !== "face") return null;
  if (app.opt("xref.allowExternal") === false) {
    app.flash("References external to the model are turned off (Options > External References).", "warning");
    return null;
  }
  const pick = await resolveContextPick(app, item);
  if (!pick || pick.entity.kind !== "face" || pick.entity.geom.type !== "plane") return null;
  const sig = placedFaceSig(pick.entity.sig, toPart(c.context, pick.world));
  return { ref: { kind: "external", sig, ext: externalOf(c.session, c.context, pick) }, sig };
}

/**
 * Convert Entities of other components' edges into the open sketch (in context): each a
 * sketch entity following the edge (`convertExternal`). Returns how many were converted.
 */
export async function convertExternalEdges(app: App, items: SelItem[], frame: Frame): Promise<number> {
  const c = inContext(app);
  const s = app.sketch;
  if (!c || !s) return 0;
  if (app.opt("xref.allowExternal") === false) {
    app.flash("References external to the model are turned off (Options > External References).", "warning");
    return 0;
  }
  const picks = await Promise.all(items.filter((x) => isContextItem(x) && x.kind === "edge").map((x) => resolveContextPick(app, x)));
  if (!app.sketch) return 0;
  app.sketchCheckpoint();
  let n = 0;
  for (const p of picks) {
    if (!p || p.entity.kind !== "edge") continue;
    const sig = placedEdgeSig(p.entity.sig, toPart(c.context, p.world));
    if (convertExternal(app.sketch.data, { kind: "external", sig, ext: externalOf(c.session, c.context, p) }, frame)) n++;
  }
  return n;
}

// ------------------------------------------------------------------ the FeatureManager

/** The in-context references of a feature (a sketch's plane and converted edges). */
export function externalsOf(f: Feature): ExternalRef[] {
  if (f.type !== "sketch") return [];
  const out: ExternalRef[] = [];
  if (f.plane.kind === "external") out.push(f.plane.ext);
  for (const r of Object.values(f.sketch.refs ?? {})) if (r.kind === "external") out.push(r.ext);
  return out;
}

/**
 * SOLIDWORKS' suffix for a feature with external references: `->` in context (the part is
 * edited in, or opened from, the assembly the references come from), `->?` out of context.
 */
export function externalSuffix(app: App, f: Feature): string {
  // A feature made from such a sketch is marked too (🔍 SOLIDWORKS marks both).
  const refs = [f, ...sketchesOf(f).map((id) => app.featureById(id))].flatMap((x) => (x ? externalsOf(x) : []));
  if (!refs.length) return "";
  const open = app.asmStack.map((x) => (x.session.path ?? "").replace(/\\/g, "/").toLowerCase());
  return refs.every((r) => open.includes(r.assembly.toLowerCase())) ? "->" : "->?";
}

// ------------------------------------------------------------------ updating in context

/**
 * Updates the in-context references of `doc` (a part of `session` at occurrence `path`)
 * from the assembly as last built: each referenced entity re-found by name on its
 * occurrence and placed relative to the part. Returns whether anything moved (the document
 * is changed in place). References to other assemblies are left alone; a missing one keeps
 * its last geometry (SOLIDWORKS: the reference is out of date until it can be updated).
 */
export async function updateExternals(session: AssemblySession, doc: PartDocument, path: Id[]): Promise<{ changed: boolean; missing: number }> {
  const self = session.occurrences().find((o) => pathKey(o.path) === pathKey(path));
  const here = (session.path ?? "").replace(/\\/g, "/").toLowerCase();
  let changed = false;
  let missing = 0;
  if (!self) return { changed, missing };
  const now = async (ext: ExternalRef): Promise<{ entity: MateEntity; t: Transform } | null> => {
    if (ext.assembly.toLowerCase() !== here || pathKey(ext.context) !== pathKey(path)) return null;
    try {
      const r = await invoke<{ entity: MateEntity; world: Transform }>("asm_ref", { path: ext.component, entity: ext.entity, quality: session.quality });
      return { entity: r.entity, t: toPart({ instance: path[0], path, world: self.world }, r.world) };
    } catch {
      missing++;
      return null;
    }
  };
  const near = (a: Vec3, b: Vec3) => Math.hypot(...sub(a, b)) <= 1e-9 * Math.max(1, Math.hypot(...a));
  for (const f of doc.features) {
    if (f.type !== "sketch") continue;
    if (f.plane.kind === "external") {
      const r = await now(f.plane.ext);
      if (r && r.entity.kind === "face") {
        const sig = placedFaceSig(r.entity.sig, r.t);
        if (!near(sig.center, f.plane.sig.center) || !near(sig.normal, f.plane.sig.normal)) {
          f.plane = { ...f.plane, sig, ext: { ...f.plane.ext, entity: r.entity } };
          changed = true;
        }
      }
    }
    for (const [k, ref] of Object.entries(f.sketch.refs ?? {})) {
      if (ref.kind !== "external") continue;
      const r = await now(ref.ext);
      if (r && r.entity.kind === "edge") {
        const sig = placedEdgeSig(r.entity.sig, r.t);
        if (!near(sig.a, ref.sig.a) || !near(sig.b, ref.sig.b) || !near(sig.mid, ref.sig.mid)) {
          f.sketch.refs![k] = { ...ref, sig, ext: { ...ref.ext, entity: r.entity } };
          changed = true;
        }
      }
    }
  }
  return { changed, missing };
}

/**
 * After a build of the assembly: the parts that have in-context references to it are
 * updated (each part read from memory if edited, else from its file) and, if any moved,
 * kept as edited in memory and the assembly rebuilt. Returns the files updated.
 */
export async function updateAssemblyExternals(session: AssemblySession): Promise<string[]> {
  const b = session.built;
  if (!b || !session.path) return [];
  const updated: string[] = [];
  for (const inst of session.doc.instances) {
    const comp = session.componentOf(inst);
    const found = comp && b.components.find((c) => c.id === comp.id);
    if (!comp || comp.kind !== "part" || !found?.path || !found.part || !b.parts[found.part]?.external) continue;
    const file = found.path.replace(/\\/g, "/");
    const text = session.overrides[file] ?? (await invoke<string>("app_read_text", { path: file }).catch(() => null));
    if (!text) continue;
    const doc = JSON.parse(text) as PartDocument;
    const r = await updateExternals(session, doc, [inst.id]);
    if (r.changed) {
      session.overrides[file] = JSON.stringify(doc, null, 1);
      updated.push(file);
    }
  }
  return updated;
}
