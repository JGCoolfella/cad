/**
 * An open assembly in the native app (OVERHAUL_PLAN G1a). The document is plain JSON
 * (`types.ts`); the engine (`crates/oc-app/src/asm.rs`, `oc-asm`) finds the component
 * files, regenerates each distinct part once (or takes its cached mesh), solves the mates
 * and answers with placements and states. Undo is a stack of document snapshots, as for
 * parts.
 *
 * Selection in the graphics area uses the part selection items with `body` = the index of
 * the occurrence (`BuildReply.occurrences`); FeatureManager rows select
 * `{ kind: "feature", id }` with an instance or mate id.
 */
import type { App, SelItem } from "../app/app";
import type { Id, Vec3 } from "../core/types";
import { invoke } from "../native/tauri";
import {
  type Align,
  type AssemblyDocument,
  type BuildReply,
  type DragReply,
  type Instance,
  type Mate,
  type MateEntity,
  type MateRef,
  type MateType,
  type Occurrence,
  type Transform,
  IDENTITY,
  MATE_LABEL,
  apply as applyWorld,
  applyInverse,
  entityGeom,
  placedGeom,
} from "./types";

/** A selection in the Mate PropertyManager's Entities to Mate box. */
export interface MatePick {
  label: string;
  ref: MateRef;
  /** World anchor and direction when picked (for the default alignment and value). */
  p: Vec3;
  d: Vec3 | null;
}

export interface MatePM {
  type: MateType;
  picks: MatePick[];
  align: Align;
  value: number;
  flip: boolean;
  /** Editing this mate (Edit Feature); otherwise a new one. */
  editing: Id | null;
  /** A pick is being resolved by the engine. */
  busy: boolean;
  /** Why the mate as set up cannot be added (shown in the PropertyManager; OK refuses it). */
  problem: string | null;
}

/** SOLIDWORKS' wording when a mate would over-define the assembly (🔍 exact text). */
export const OVER_DEFINES = "This mate would over define the assembly. Consider deleting some of the over defining mates.";

const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const len = (a: Vec3) => Math.hypot(a[0], a[1], a[2]);

export function newAssembly(n: number): AssemblyDocument {
  return { format: "opencad-assembly", version: 1, name: `Assem${n}`, units: "MMGS", components: [], instances: [], mates: [] };
}

export class AssemblySession {
  doc: AssemblyDocument;
  /** The assembly file (none until saved). */
  path: string | null;
  built: BuildReply | null = null;
  matePM: MatePM | null = null;
  building = false;
  /** Lightweight builds take cached meshes of unchanged parts (the first build after opening). */
  lightweight = true;
  private undoStack: { doc: string; label: string }[] = [];
  private redoStack: { doc: string; label: string }[] = [];
  private queued: AssemblyDocument | null = null;
  private dragging: { instance: Id; grab: Vec3; busy: boolean; next: Vec3 | null; lever: number; before: string } | null = null;
  /** Settles when the drag requests sent so far are answered. */
  private dragChain: Promise<void> = Promise.resolve();

  constructor(
    readonly app: App,
    doc: AssemblyDocument,
    path: string | null,
  ) {
    this.doc = doc;
    this.path = path;
  }

  get name() {
    return this.doc.name;
  }

  private get quality() {
    return (this.app.kernel as { quality?: number }).quality ?? 3;
  }

  // ------------------------------------------------------------------ build

  /** Rebuilds (the newest request wins while one runs). */
  async build(doc: AssemblyDocument = this.doc): Promise<BuildReply | null> {
    if (this.building) {
      this.queued = doc;
      return null;
    }
    this.building = true;
    try {
      const r = await invoke<BuildReply>("asm_build", { doc, path: this.path, lightweight: this.lightweight, quality: this.quality });
      this.built = r;
      if (doc === this.doc) this.absorb(r);
      return r;
    } catch (e) {
      this.app.flash(`Assembly rebuild failed: ${(e as Error).message ?? e}`, "error");
      return null;
    } finally {
      this.building = false;
      this.app.changed();
      const q = this.queued;
      this.queued = null;
      if (q) void this.build(q);
    }
  }

  /** Heals and solved placements written into the document (no undo step). */
  private absorb(r: BuildReply) {
    for (const h of r.heals) {
      const m = this.doc.mates.find((x) => x.id === h.mate);
      if (m?.refs[h.side]) m.refs[h.side].entity = h.entity;
    }
    for (const out of r.instances) {
      if (out.state === "suppressed") continue;
      const i = this.doc.instances.find((x) => x.id === out.id);
      if (i) i.transform = out.transform;
    }
    const errs = r.components.filter((c) => c.error).map((c) => c.error);
    if (errs.length) this.app.flash(errs.join("; "), "error");
  }

  /** Occurrences to draw (placements follow a drag in progress). */
  occurrences(): Occurrence[] {
    return this.built?.occurrences ?? [];
  }

  /** World box of everything shown, or null. */
  box(): [Vec3, Vec3] | null {
    let lo: Vec3 = [Infinity, Infinity, Infinity];
    let hi: Vec3 = [-Infinity, -Infinity, -Infinity];
    for (const o of this.occurrences()) {
      const b = this.built?.parts[o.part]?.bbox;
      if (!b || o.hidden) continue;
      for (let k = 0; k < 8; k++) {
        const c: Vec3 = [b[k & 1][0], b[(k >> 1) & 1][1], b[(k >> 2) & 1][2]];
        const w = applyWorld(o.world, c);
        lo = [Math.min(lo[0], w[0]), Math.min(lo[1], w[1]), Math.min(lo[2], w[2])];
        hi = [Math.max(hi[0], w[0]), Math.max(hi[1], w[1]), Math.max(hi[2], w[2])];
      }
    }
    return lo[0] <= hi[0] ? [lo, hi] : null;
  }

  /** Mesh handles the renderer must keep. */
  handles(): number[] {
    return Object.values(this.built?.parts ?? {}).map((p) => p.handle).filter((h) => h > 0);
  }

  instanceState(id: Id) {
    return this.built?.instances.find((i) => i.id === id);
  }

  mateStatus(id: Id) {
    return this.built?.mates.find((m) => m.id === id);
  }

  componentOf(inst: Instance) {
    return this.doc.components.find((c) => c.id === inst.component);
  }

  // ------------------------------------------------------------------ undo

  checkpoint(label: string) {
    this.undoStack.push({ doc: JSON.stringify(this.doc), label });
    if (this.undoStack.length > 100) this.undoStack.shift();
    this.redoStack = [];
    this.app.dirty = true;
  }

  canUndo() {
    return this.undoStack.length > 0;
  }

  canRedo() {
    return this.redoStack.length > 0;
  }

  undo() {
    const s = this.undoStack.pop();
    if (!s) return;
    this.redoStack.push({ doc: JSON.stringify(this.doc), label: s.label });
    this.doc = JSON.parse(s.doc);
    this.app.dirty = true;
    void this.build();
  }

  redo() {
    const s = this.redoStack.pop();
    if (!s) return;
    this.undoStack.push({ doc: JSON.stringify(this.doc), label: s.label });
    this.doc = JSON.parse(s.doc);
    this.app.dirty = true;
    void this.build();
  }

  // ------------------------------------------------------------------ edits

  private nextId(prefix: string) {
    const used = new Set<string>([...this.doc.components, ...this.doc.instances, ...this.doc.mates].map((x) => x.id));
    let n = 1;
    while (used.has(`${prefix}${n}`)) n++;
    return `${prefix}${n}`;
  }

  /**
   * Insert Components: adds the file as a component (once per file) and an instance of it.
   * The first component is fixed at the assembly origin, as SOLIDWORKS places it; later ones
   * float and are set beside the others (🔍 SOLIDWORKS places them where you click; G1b).
   */
  async insertComponent(file: string) {
    const kind = /\.ocasm$/i.test(file) ? "assembly" : "part";
    const norm = file.replace(/\\/g, "/");
    this.checkpoint("Insert Component");
    let comp = this.doc.components.find((c) => c.absPath === norm || c.path === norm);
    if (!comp) {
      comp = { id: this.nextId("c"), kind, path: norm, absPath: norm, config: "Default" };
      this.doc.components.push(comp);
    }
    const stem = norm.split("/").pop()!.replace(/\.[^.]+$/, "");
    const n = this.doc.instances.filter((i) => i.component === comp!.id).length + 1;
    const first = this.doc.instances.length === 0;
    const transform: Transform = first ? IDENTITY : { position: [60 * this.doc.instances.length, 0, 0], rotation: [1, 0, 0, 0] };
    const inst: Instance = { id: this.nextId("i"), component: comp.id, name: `${stem}<${n}>`, fixed: first, transform };
    this.doc.instances.push(inst);
    this.lightweight = true;
    await this.build();
    return inst.id;
  }

  setFixed(ids: Id[], fixed: boolean) {
    const list = this.doc.instances.filter((i) => ids.includes(i.id) && i.fixed !== fixed);
    if (!list.length) return;
    this.checkpoint(fixed ? "Fix" : "Float");
    list.forEach((i) => (i.fixed = fixed));
    void this.build();
  }

  toggleSuppress(id: Id) {
    const x = this.doc.instances.find((i) => i.id === id) ?? this.doc.mates.find((m) => m.id === id);
    if (!x) return;
    this.checkpoint(x.suppressed ? "Unsuppress" : "Suppress");
    if (x.suppressed) delete x.suppressed;
    else x.suppressed = true;
    void this.build();
  }

  /** Deletes instances (with their mates) and mates. */
  delete(ids: Id[]) {
    const inst = new Set(this.doc.instances.filter((i) => ids.includes(i.id)).map((i) => i.id));
    const mates = this.doc.mates.filter((m) => ids.includes(m.id) || m.refs.some((r) => r.occurrence[0] && inst.has(r.occurrence[0])));
    if (!inst.size && !mates.length) return;
    this.checkpoint("Delete");
    const gone = new Set(mates.map((m) => m.id));
    this.doc.mates = this.doc.mates.filter((m) => !gone.has(m.id));
    this.doc.instances = this.doc.instances.filter((i) => !inst.has(i.id));
    const used = new Set(this.doc.instances.map((i) => i.component));
    this.doc.components = this.doc.components.filter((c) => used.has(c.id));
    this.app.selection = [];
    void this.build();
  }

  // ------------------------------------------------------------------ drag

  /** Starts dragging the occurrence under the pointer at world point `at`. */
  dragStart(occ: number, at: Vec3): boolean {
    const o = this.occurrences()[occ];
    const st = o && this.instanceState(o.path[0]);
    if (!o || !st || st.state === "fixed" || st.state === "suppressed" || st.free === 0) return false;
    const inst = this.built!.instances.find((i) => i.id === o.path[0])!;
    const size = Math.max(100, ...this.occurrences().map((x) => len(x.world.position)));
    this.dragging = { instance: o.path[0], grab: applyInverse(inst.transform, at), busy: false, next: null, lever: 10 * size, before: JSON.stringify(this.doc) };
    return true;
  }

  dragTo(target: Vec3) {
    const d = this.dragging;
    if (!d) return;
    if (d.busy) {
      d.next = target;
      return;
    }
    d.busy = true;
    this.dragChain = invoke<DragReply>("asm_drag", { instance: d.instance, grab: d.grab, target, lever: d.lever })
      .then((r) => {
        if (!this.built) return;
        r.instances.forEach((x) => {
          const b = this.built!.instances.find((i) => i.id === x.id);
          if (b) b.transform = x.transform;
        });
        r.occurrences.forEach((w, k) => this.built!.occurrences[k] && (this.built!.occurrences[k].world = w));
        this.app.changed();
      })
      .catch((e) => console.error("asm_drag", e))
      .finally(() => {
        d.busy = false;
        const n = d.next;
        d.next = null;
        if (n && this.dragging === d) this.dragTo(n);
      });
  }

  /** Ends a drag: the placements become the document's (one undo step). */
  async dragEnd() {
    const d = this.dragging;
    if (!d) return;
    // The newest target is answered first, so the document gets where the pointer let go.
    while (d.busy || d.next) await this.dragChain;
    this.dragging = null;
    if (!this.built) return;
    this.undoStack.push({ doc: d.before, label: "Move Component" });
    this.redoStack = [];
    for (const out of this.built.instances) {
      const i = this.doc.instances.find((x) => x.id === out.id);
      if (i && out.state !== "suppressed") i.transform = out.transform;
    }
    this.app.dirty = true;
    this.app.changed();
  }

  /** A press that did not move: nothing changes. */
  dragCancel() {
    this.dragging = null;
  }

  get isDragging() {
    return !!this.dragging;
  }

  // ------------------------------------------------------------------ mates

  openMate(editing: Id | null = null) {
    const m = editing ? this.doc.mates.find((x) => x.id === editing) : null;
    this.matePM = m
      ? { type: m.type, picks: m.refs.map((r) => ({ label: this.refLabel(r), ref: r, ...this.worldOf(r) })), align: m.align ?? "antiAligned", value: m.value ?? 0, flip: !!m.flip, editing, busy: false, problem: null }
      : { type: "coincident", picks: [], align: "antiAligned", value: 10, flip: false, editing: null, busy: false, problem: null };
    this.app.selection = [];
    this.app.changed();
  }

  /** World anchor and direction of a reference with the current placements. */
  private worldOf(r: MateRef): { p: Vec3; d: Vec3 | null } {
    const g = entityGeom(r.entity) ?? { type: "point" as const, p: [0, 0, 0] as Vec3 };
    const occ = this.occurrences().find((o) => o.path.join("/") === r.occurrence.join("/"));
    return placedGeom(g, occ?.world ?? IDENTITY);
  }

  refLabel(r: MateRef): string {
    const inst = this.doc.instances.find((i) => i.id === r.occurrence[0]);
    const what = r.entity.kind === "face" ? "Face" : r.entity.kind === "edge" ? "Edge" : r.entity.kind === "vertex" ? "Vertex" : r.entity.kind;
    return `${what}@${inst?.name ?? "Assembly"}`;
  }

  /** A graphics-area pick while the Mate PropertyManager is open. */
  async matePick(item: SelItem) {
    const pm = this.matePM;
    if (!pm || !("body" in item) || item.body === undefined || pm.busy) return;
    if (item.kind !== "face" && item.kind !== "edge" && item.kind !== "vertex") return;
    const occ = this.occurrences()[item.body];
    if (!occ) return;
    if (pm.picks.length >= 2) pm.picks = [];
    pm.busy = true;
    this.app.changed();
    try {
      const entity = await invoke<MateEntity>("asm_entity", { part: occ.part, kind: item.kind, index: item.id, quality: this.quality });
      const ref: MateRef = { occurrence: occ.path, entity };
      if (pm.picks.some((p) => p.ref.occurrence[0] === ref.occurrence[0])) {
        this.app.flash("Pick the second entity on another component.", "warning");
        return;
      }
      pm.picks.push({ label: this.refLabel(ref), ref, ...this.worldOf(ref) });
      if (pm.picks.length === 2 && !pm.editing) this.defaults(pm);
      void this.preview();
    } catch (e) {
      this.app.flash(String((e as Error).message ?? e), "warning");
    } finally {
      pm.busy = false;
      this.app.changed();
    }
  }

  /**
   * Defaults once two entities are picked, as SOLIDWORKS proposes them: Concentric for two
   * round faces, else Coincident; the alignment closest to the current placement; Distance
   * and Angle start at the current value.
   */
  private defaults(pm: MatePM) {
    const [a, b] = pm.picks;
    const round = (r: MateRef) => { const g = entityGeom(r.entity); return g?.type === "cylinder" || g?.type === "cone"; };
    pm.type = round(a.ref) && round(b.ref) ? "concentric" : "coincident";
    if (a.d && b.d) pm.align = dot(a.d, b.d) >= 0 ? "aligned" : "antiAligned";
    if (a.d) {
      const off = dot(sub(b.p, a.p), a.d) / (len(a.d) || 1);
      pm.value = Math.abs(off) > 1e-9 ? Math.abs(off) : len(sub(b.p, a.p));
      pm.flip = off < 0;
    } else pm.value = len(sub(b.p, a.p));
  }

  /** The mate the PropertyManager describes (null until two entities are picked). */
  pmMate(): Mate | null {
    const pm = this.matePM;
    if (!pm || pm.picks.length !== 2) return null;
    const id = pm.editing ?? this.nextId("m");
    const old = pm.editing ? this.doc.mates.find((m) => m.id === pm.editing) : null;
    const count = this.doc.mates.filter((m) => m.type === pm.type).length + 1;
    const m: Mate = { id, name: old && old.type === pm.type ? old.name : `${MATE_LABEL[pm.type]}${count}`, type: pm.type, refs: pm.picks.map((p) => p.ref) };
    if (pm.type !== "lock" && pm.type !== "perpendicular") m.align = pm.align;
    if (pm.type === "distance" || pm.type === "angle") {
      m.value = pm.value;
      if (pm.flip) m.flip = true;
    }
    if (pm.type === "lock") {
      // Lock keeps the second component where it is relative to the first.
      const ia = this.built?.instances.find((i) => i.id === m.refs[0].occurrence[0]);
      const ib = this.built?.instances.find((i) => i.id === m.refs[1].occurrence[0]);
      if (!ia || !ib) return null;
      m.relative = relative(ia.transform, ib.transform);
      m.refs = m.refs.map((r) => ({ occurrence: [r.occurrence[0]], entity: { kind: "component" } }));
    }
    return m;
  }

  private docWith(m: Mate | null): AssemblyDocument {
    const d: AssemblyDocument = JSON.parse(JSON.stringify(this.doc));
    if (!m) return d;
    const i = d.mates.findIndex((x) => x.id === m.id);
    if (i >= 0) d.mates[i] = m;
    else d.mates.push(m);
    return d;
  }

  /** Solves with the mate being defined, so the components move as it is set up. */
  async preview() {
    await this.check(this.pmMate());
  }

  /**
   * Solves with mate `m` added (or edited) and records in the PropertyManager why it cannot
   * be added: as SOLIDWORKS, a mate that would over-define the assembly (conflicting, or
   * adding nothing) or that cannot pair its entities is refused while the PropertyManager
   * is open. Returns the problem, or null.
   */
  private async check(m: Mate | null): Promise<string | null> {
    while (this.building) await new Promise((r) => setTimeout(r, 20));
    const r = await this.build(this.docWith(m));
    const pm = this.matePM;
    const st = m ? r?.mates.find((x) => x.id === m.id) : undefined;
    const problem = !st || st.status === "ok" ? null : st.status === "invalid" ? st.message ?? "These entities cannot be mated this way." : OVER_DEFINES;
    if (pm) pm.problem = problem;
    this.app.changed();
    return problem;
  }

  /** OK in the Mate PropertyManager. Returns the problem when the mate cannot be added
   * (the PropertyManager then stays open, as in SOLIDWORKS). */
  async acceptMate(): Promise<string | null> {
    const m = this.pmMate();
    if (!m) return "Select two entities to mate.";
    const problem = await this.check(m);
    if (problem) {
      this.app.flash(problem, "error");
      return problem;
    }
    this.checkpoint(this.matePM?.editing ? "Edit Mate" : "Add Mate");
    this.doc = this.docWith(m);
    this.matePM = null;
    await this.build();
    return null;
  }

  cancelMate() {
    this.matePM = null;
    void this.build();
  }

  // ------------------------------------------------------------------ files

  /** Saves to `path` (Save As) or the current file; the engine rewrites component paths. */
  async save(path: string): Promise<boolean> {
    try {
      const r = await invoke<{ doc: AssemblyDocument }>("asm_save", { doc: this.doc, path, quality: this.quality });
      this.doc = r.doc;
      this.path = path;
      this.doc.name = path.replace(/\\/g, "/").split("/").pop()!.replace(/\.[^.]+$/, "");
      this.app.dirty = false;
      this.app.flash(`Saved ${this.doc.name}`);
      return true;
    } catch (e) {
      this.app.flash(`Save failed: ${(e as Error).message ?? e}`, "error");
      return false;
    }
  }
}

/** `b` in `a`'s coordinates. */
export function relative(a: Transform, b: Transform): Transform {
  const [w, x, y, z] = a.rotation;
  const ai: Transform["rotation"] = [w, -x, -y, -z];
  const q = qmul(ai, b.rotation);
  return { position: applyInverse(a, b.position), rotation: q };
}

function qmul(a: Transform["rotation"], b: Transform["rotation"]): Transform["rotation"] {
  return [
    a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
    a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
    a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
    a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
  ];
}

/** Opens an `.ocasm` file through the engine. */
export async function openAssembly(app: App, path: string): Promise<AssemblySession> {
  const r = await invoke<{ doc: AssemblyDocument; sourceVersion: number }>("asm_open", { path });
  const s = new AssemblySession(app, r.doc, path);
  s.doc.name = path.replace(/\\/g, "/").split("/").pop()!.replace(/\.[^.]+$/, "");
  return s;
}
