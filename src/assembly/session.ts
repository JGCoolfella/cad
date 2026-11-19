/**
 * An open assembly in the native app (OVERHAUL_PLAN G1a, G1b). The document is plain JSON
 * (`types.ts`); the engine (`crates/oc-app/src/asm.rs`, `oc-asm`) finds the component
 * files, regenerates each distinct part once (or takes its cached mesh), solves the mates
 * and answers with placements and states. Undo is a stack of document snapshots, as for
 * parts.
 *
 * Selection in the graphics area uses the part selection items with `body` = the index of
 * the occurrence (`BuildReply.occurrences`); FeatureManager rows select
 * `{ kind: "feature", id }` with an instance or mate id, or a plane or origin row
 * (`$plane:front`, `i2/$origin`, see `treeRef`).
 */
import type { App, SelItem } from "../app/app";
import type { Id, Vec3 } from "../core/types";
import { invoke } from "../native/tauri";
import { SmartMates } from "./smart";
import {
  type Align,
  type AssemblyDocument,
  type AsmMeasureReply,
  type BuildReply,
  type DragReply,
  type InterferenceReply,
  type InterferenceResult,
  type Instance,
  type MassReply,
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
  qrot,
  validMates,
} from "./types";

/** A selection in the Mate PropertyManager's Entities to Mate box. */
export interface MatePick {
  label: string;
  ref: MateRef;
  /** World anchor and direction when picked (for the default alignment and value). */
  p: Vec3;
  d: Vec3 | null;
  /** A reference of the mate being edited that no longer resolves (shown **Missing**). */
  missing?: boolean;
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
  /** The Mate Selections entry the next pick replaces (Edit Mate: a Missing reference, or
   * an entry clicked to re-pick it); null adds picks. */
  active: number | null;
  /** The mate pop-up toolbar was closed (it opens once two entities are picked). */
  popupClosed?: boolean;
}

/** The Move Component and Rotate Component PropertyManager (one PropertyManager with a
 * Move and a Rotate group, as in SOLIDWORKS). */
export interface MovePM {
  /** Which group pointer drags use: Move or Rotate. */
  rotate: boolean;
  move: "free" | "xyz" | "entity" | "delta" | "toXYZ";
  rot: "free" | "entity" | "delta";
  /** Delta X/Y/Z (mm), the To XYZ position (mm) or rotation angles about X/Y/Z (degrees). */
  values: Vec3;
  /** Along Entity / About Entity: a line in assembly coordinates. */
  entity: { label: string; p: Vec3; d: Vec3 } | null;
  /** The next graphics-area pick is the entity. */
  pickingEntity: boolean;
  /** SmartMates mode (G1d): double-click an entity of the component to move, then click
   * one of another component. */
  smartMates: boolean;
}

/**
 * The Interference Detection PropertyManager (G1c), as SOLIDWORKS': Selected Components
 * (the top-level assembly when empty), Calculate, Results with Ignore, Options and
 * Non-interfering Components.
 */
export interface InterferencePM {
  /** Selected Components as instance paths; empty: the whole assembly. */
  selected: Id[][];
  /** Treat coincidence as interference. */
  coincidence: boolean;
  /** Show ignored interferences (greyed in Results). */
  showIgnored: boolean;
  /** Treat subassemblies as components. */
  subassemblies: boolean;
  /** Include hidden components ("Ignore hidden bodies/components" cleared). */
  includeHidden: boolean;
  /** Make interfering parts transparent. */
  transparent: boolean;
  /** How the components that do not interfere are shown. */
  others: "wireframe" | "hidden" | "transparent" | "current";
  /** The last Calculate (null before it). */
  results: InterferenceResult[] | null;
  /** Ignored interferences, by their pair of occurrence paths (kept across Calculate). */
  ignored: string[];
  /** The result selected in Results (its components are highlighted). */
  active: number | null;
  busy: boolean;
}

/** An interference's key: its two occurrence paths. */
export const pairKey = (r: { pathA: Id[]; pathB: Id[] }) => `${r.pathA.join("/")}|${r.pathB.join("/")}`;

/** Insert Components: the new component follows the pointer until a click drops it. */
export interface Placing {
  instance: Id;
  /** The part's box centre in its own coordinates: what follows the pointer. */
  center: Vec3;
}

/** SOLIDWORKS' wording when a mate would over-define the assembly (🔍 exact text). */
export const OVER_DEFINES = "This mate would over define the assembly. Consider deleting some of the over defining mates.";

const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const len = (a: Vec3) => Math.hypot(a[0], a[1], a[2]);

export function newAssembly(n: number): AssemblyDocument {
  return { format: "opencad-assembly", version: 1, name: `Assem${n}`, units: "MMGS", components: [], instances: [], mates: [] };
}

type DragHow = "free" | "xyz" | "entity" | "rotFree" | "rotEntity";

interface Dragging {
  instance: Id;
  /** The grabbed point in instance coordinates. */
  grab: Vec3;
  /** Where the drag started (assembly coordinates) and the instance's placement then. */
  start: Vec3;
  from: Transform;
  busy: boolean;
  next: Vec3 | null;
  lever: number;
  before: string;
  how: DragHow;
  viewDir: Vec3;
  /** Free rotation turns about the component's box centre, a radian per `radius` of motion. */
  pivot: Vec3;
  radius: number;
}

export class AssemblySession {
  doc: AssemblyDocument;
  /** The assembly file (none until saved). */
  path: string | null;
  built: BuildReply | null = null;
  matePM: MatePM | null = null;
  movePM: MovePM | null = null;
  interferencePM: InterferencePM | null = null;
  /** Isolate (G1c): only these top-level components are shown; the others hidden or
   * transparent ("Visibility of removed components"). View only: not saved, no undo. */
  isolated: { ids: Id[]; others: "hidden" | "transparent" | "wireframe" } | null = null;
  placing: Placing | null = null;
  /** SmartMates in progress (G1d): an Alt-drag, or Move Component's SmartMates mode. */
  smart: SmartMates | null = null;
  /** Components opened from this assembly and edited, not saved yet: file → document text.
   * Builds read them instead of the files (SOLIDWORKS shows a part's unsaved edits in the
   * assemblies that use it); saving the assembly saves them first. */
  overrides: Record<string, string> = {};
  building = false;
  /** Lightweight builds take cached meshes of unchanged parts (the first build after opening). */
  lightweight = true;
  private undoStack: { doc: string; label: string }[] = [];
  private redoStack: { doc: string; label: string }[] = [];
  private queued: AssemblyDocument | null = null;
  private dragging: Dragging | null = null;
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

  get quality() {
    return (this.app.kernel as { quality?: number }).quality ?? 3;
  }

  /** A PropertyManager of the assembly is open (Mate, Move/Rotate Component, Interference
   * Detection). */
  get inPM() {
    return !!(this.matePM || this.movePM || this.interferencePM);
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
      const r = await invoke<BuildReply>("asm_build", { doc, path: this.path, lightweight: this.lightweight, quality: this.quality, overrides: this.overrides });
      this.built = r;
      if (doc === this.doc) {
        this.absorb(r);
        void this.followExternals(r);
      }
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

  private externalRounds = 0;

  /**
   * Parts with in-context references to this assembly (G1d) follow it: their references
   * are updated from this build and, if any moved, the parts are kept as edited in memory
   * and the assembly is built again (at most three rounds in a row, as the parts may move
   * what they refer to).
   */
  private async followExternals(r: BuildReply) {
    if (!this.path || !Object.values(r.parts).some((p) => p.external)) {
      this.externalRounds = 0;
      return;
    }
    const { updateAssemblyExternals } = await import("./context");
    const files = await updateAssemblyExternals(this);
    if (files.length && this.externalRounds < 3 && this.app.asm === this) {
      this.externalRounds++;
      this.app.dirty = true;
      await this.build();
    } else this.externalRounds = 0;
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

  /** Mesh handles the renderer must keep (parts, and the volumes Interference Detection
   * found while its PropertyManager is open). */
  handles(): number[] {
    const parts = Object.values(this.built?.parts ?? {}).map((p) => p.handle);
    const found = (this.interferencePM?.results ?? []).map((r) => r.mesh);
    return [...parts, ...found].filter((h) => h > 0);
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

  /** The instance an occurrence belongs to (top level). */
  instanceOfOccurrence(occ: number): Id | null {
    return this.occurrences()[occ]?.path[0] ?? null;
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

  // ------------------------------------------------------------------ components

  private nextId(prefix: string) {
    const used = new Set<string>([...this.doc.components, ...this.doc.instances, ...this.doc.mates].map((x) => x.id));
    let n = 1;
    while (used.has(`${prefix}${n}`)) n++;
    return `${prefix}${n}`;
  }

  /**
   * Insert Components: adds the file as a component (once per file) and an instance of it.
   * The first component is fixed at the assembly origin, as SOLIDWORKS places it. Later
   * ones float and follow the pointer (their box centre under it) until a click drops them
   * (`placeAt`, `dropAt`; Esc cancels the insertion), or are dropped at `at` at once.
   */
  async insertComponent(file: string, at?: Vec3) {
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
    const inst: Instance = { id: this.nextId("i"), component: comp.id, name: `${stem}<${n}>`, fixed: first, transform: IDENTITY };
    this.doc.instances.push(inst);
    this.lightweight = true;
    await this.build();
    if (!first) {
      this.placing = { instance: inst.id, center: this.localCenter(inst.id) };
      if (at) await this.dropAt(at);
      else this.app.flash("Click in the graphics area to place the component.");
    }
    return inst.id;
  }

  /** Box centre of an instance in its own coordinates (all its occurrences). */
  private localCenter(id: Id): Vec3 {
    let lo: Vec3 = [Infinity, Infinity, Infinity];
    let hi: Vec3 = [-Infinity, -Infinity, -Infinity];
    const inst = this.built?.instances.find((i) => i.id === id);
    for (const o of this.occurrences().filter((x) => x.path[0] === id)) {
      const b = this.built?.parts[o.part]?.bbox;
      if (!b || !inst) continue;
      for (let k = 0; k < 8; k++) {
        const w = applyInverse(inst.transform, applyWorld(o.world, [b[k & 1][0], b[(k >> 1) & 1][1], b[(k >> 2) & 1][2]]));
        lo = [Math.min(lo[0], w[0]), Math.min(lo[1], w[1]), Math.min(lo[2], w[2])];
        hi = [Math.max(hi[0], w[0]), Math.max(hi[1], w[1]), Math.max(hi[2], w[2])];
      }
    }
    return lo[0] <= hi[0] ? [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2] : [0, 0, 0];
  }

  /** The placement that puts the placing component's box centre at `world`. */
  private placingTransform(world: Vec3): Transform | null {
    const p = this.placing;
    const inst = p && this.doc.instances.find((i) => i.id === p.instance);
    if (!p || !inst) return null;
    const c = qrot(inst.transform.rotation, p.center);
    return { position: [world[0] - c[0], world[1] - c[1], world[2] - c[2]], rotation: inst.transform.rotation };
  }

  /** Insert Components preview: the component follows the pointer (nothing is solved). */
  placeAt(world: Vec3) {
    const t = this.placingTransform(world);
    if (!t || !this.built) return;
    const id = this.placing!.instance;
    const b = this.built.instances.find((i) => i.id === id);
    const old = b?.transform;
    if (b) b.transform = t;
    for (const o of this.built.occurrences) {
      if (o.path[0] !== id || !old) continue;
      // The occurrence keeps its placement in the instance (sub-assemblies).
      o.world = compose(t, compose(inverse(old), o.world));
    }
    this.app.changed();
  }

  /** Insert Components: the click that drops the component. */
  async dropAt(world: Vec3) {
    const t = this.placingTransform(world);
    const p = this.placing;
    this.placing = null;
    if (!t || !p) return;
    const inst = this.doc.instances.find((i) => i.id === p.instance)!;
    inst.transform = t;
    await this.build();
  }

  /** Esc while placing: the insertion is undone. */
  cancelPlacing() {
    if (!this.placing) return;
    this.placing = null;
    const s = this.undoStack.pop();
    if (s) this.doc = JSON.parse(s.doc);
    void this.build();
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

  /** Hide Components / Show Components (a hidden component stays mated). */
  setHidden(ids: Id[], hidden: boolean) {
    const list = this.doc.instances.filter((i) => ids.includes(i.id) && !!i.hidden !== hidden);
    if (!list.length) return;
    this.checkpoint(hidden ? "Hide Components" : "Show Components");
    list.forEach((i) => (hidden ? (i.hidden = true) : delete i.hidden));
    void this.build();
  }

  /** Isolate: shows only these components until Exit Isolate. */
  isolate(ids: Id[]) {
    if (!ids.length) return;
    this.isolated = { ids: [...ids], others: "hidden" };
    this.app.changed();
  }

  exitIsolate() {
    this.isolated = null;
    this.app.changed();
  }

  /**
   * Component Properties (G1c): the component name (its `<n>` kept) and the suppression
   * state, as one undo step.
   */
  setProperties(id: Id, p: { name: string; suppressed: boolean }) {
    const inst = this.doc.instances.find((i) => i.id === id);
    if (!inst) return;
    const n = /<(\d+)>$/.exec(inst.name)?.[1];
    const base = p.name.trim().replace(/<\d+>$/, "");
    const name = base ? (n ? `${base}<${n}>` : base) : inst.name;
    if (name === inst.name && !!inst.suppressed === p.suppressed) return;
    this.checkpoint("Component Properties");
    inst.name = name;
    if (p.suppressed) inst.suppressed = true;
    else delete inst.suppressed;
    void this.build();
  }

  /** Renames a component instance or a mate in the FeatureManager. An instance keeps its
   * `<n>` number, as SOLIDWORKS keeps it. */
  rename(id: Id, name: string) {
    const x = this.doc.instances.find((i) => i.id === id) ?? this.doc.mates.find((m) => m.id === id);
    const text = name.trim();
    if (!x || !text) return;
    const n = "component" in x ? /<(\d+)>$/.exec(x.name)?.[1] : undefined;
    const next = n && !/<\d+>$/.test(text) ? `${text}<${n}>` : text;
    if (next === x.name) return;
    this.checkpoint("Rename");
    x.name = next;
    this.app.changed();
  }

  /** Component appearance colour (`undefined`: back to the default). */
  setColor(ids: Id[], color: string | undefined) {
    const list = this.doc.instances.filter((i) => ids.includes(i.id));
    if (!list.length) return;
    this.checkpoint("Appearance");
    list.forEach((i) => (color ? (i.color = color) : delete i.color));
    this.app.changed();
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

  /**
   * Starts dragging the occurrence under the pointer at world point `at` (`viewDir`: the
   * camera's viewing direction). Without the Move/Rotate Component PropertyManager the
   * component moves freely within the freedom its mates leave; with it, as its Move or
   * Rotate group says.
   */
  dragStart(occ: number, at: Vec3, viewDir: Vec3 = [0, 0, -1]): boolean {
    const o = this.occurrences()[occ];
    const st = o && this.instanceState(o.path[0]);
    if (!o || !st || st.state === "fixed" || st.state === "suppressed" || st.free === 0) return false;
    const pm = this.movePM;
    if (pm?.pickingEntity) return false;
    if (pm && (pm.rotate ? pm.rot === "delta" : pm.move === "delta" || pm.move === "toXYZ")) return false;
    const how: DragHow = !pm ? "free" : pm.rotate ? (pm.rot === "entity" ? "rotEntity" : "rotFree") : pm.move === "xyz" ? "xyz" : pm.move === "entity" ? "entity" : "free";
    if ((how === "entity" || how === "rotEntity") && !pm?.entity) return false;
    const inst = this.built!.instances.find((i) => i.id === o.path[0])!;
    const size = Math.max(100, ...this.occurrences().map((x) => len(x.world.position)));
    const pivot = applyWorld(inst.transform, this.localCenter(inst.id));
    this.dragging = {
      instance: o.path[0],
      grab: applyInverse(inst.transform, at),
      start: at,
      from: inst.transform,
      busy: false,
      next: null,
      lever: 10 * size,
      before: JSON.stringify(this.doc),
      how,
      viewDir,
      pivot,
      radius: Math.max(10, len(sub(at, pivot))),
    };
    return true;
  }

  /** `target`: the pointer's point in the plane through the grabbed point facing the viewer. */
  dragTo(target: Vec3) {
    const d = this.dragging;
    if (!d) return;
    if (d.busy) {
      d.next = target;
      return;
    }
    d.busy = true;
    this.dragChain = this.dragRequest(d, target)
      .then((r) => {
        if (r) this.takePlacements(r);
      })
      .catch((e) => console.error("asm_drag", e))
      .finally(() => {
        d.busy = false;
        const n = d.next;
        d.next = null;
        if (n && this.dragging === d) this.dragTo(n);
      });
  }

  private dragRequest(d: Dragging, target: Vec3): Promise<DragReply | null> {
    const move = sub(target, d.start);
    const drag = (to: Vec3) => invoke<DragReply>("asm_drag", { instance: d.instance, grab: d.grab, target: to, lever: d.lever });
    const place = (t: Transform) => invoke<DragReply>("asm_place", { instance: d.instance, to: t });
    const e = this.movePM?.entity;
    switch (d.how) {
      case "free":
        return drag(target);
      case "xyz": {
        // Along Assembly XYZ: the assembly axis nearest the pointer's motion. A pure
        // translation, kept where the mates allow it.
        const k = [0, 1, 2].reduce((a, b) => (Math.abs(move[b]) > Math.abs(move[a]) ? b : a), 0);
        const by: Vec3 = [0, 0, 0];
        by[k] = move[k];
        return place(translated(d.from, by));
      }
      case "entity": {
        const u = unit(e!.d);
        const t = dot(move, u);
        return place(translated(d.from, [u[0] * t, u[1] * t, u[2] * t]));
      }
      case "rotFree": {
        // Free Drag: about the axis across the pointer's motion in the view plane, through
        // the component's centre; a radian per radius of motion.
        const axis = cross(d.viewDir, move);
        if (len(axis) < 1e-9) return Promise.resolve(null);
        return place(rotated(d.from, d.pivot, axis, len(move) / d.radius));
      }
      case "rotEntity": {
        // About Entity: the angle the pointer turned about the line.
        const u = unit(e!.d);
        const perp = (v: Vec3): Vec3 => {
          const t = dot(v, u);
          return [v[0] - u[0] * t, v[1] - u[1] * t, v[2] - u[2] * t];
        };
        const a = perp(sub(d.start, e!.p));
        const b = perp(sub(target, e!.p));
        if (len(a) < 1e-6 || len(b) < 1e-6) {
          // The pointer is on the line: its sideways motion turns the component.
          return place(rotated(d.from, e!.p, u, dot(move, unit(cross(u, d.viewDir))) / d.radius));
        }
        return place(rotated(d.from, e!.p, u, Math.atan2(dot(cross(a, b), u), dot(a, b))));
      }
    }
  }

  private takePlacements(r: DragReply) {
    if (!this.built) return;
    this.showPlacements(r);
    this.built.dof = r.dof;
  }

  /** Shows placements (of a drag, or of a trial solve) without changing the document. */
  showPlacements(r: { instances: { id: Id; transform: Transform }[]; occurrences: Transform[] }) {
    if (!this.built) return;
    r.instances.forEach((x) => {
      const b = this.built!.instances.find((i) => i.id === x.id);
      if (b) b.transform = x.transform;
    });
    r.occurrences.forEach((w, k) => this.built!.occurrences[k] && (this.built!.occurrences[k].world = w));
    this.app.changed();
  }

  /** The placements shown now. */
  snapshot(): { instances: { id: Id; transform: Transform }[]; occurrences: Transform[] } {
    return { instances: (this.built?.instances ?? []).map((i) => ({ id: i.id, transform: i.transform })), occurrences: this.occurrences().map((o) => o.world) };
  }

  /** Settles when the drag requests sent so far are answered. */
  async settled() {
    const d = this.dragging;
    while (d && (d.busy || d.next)) await this.dragChain;
  }

  /** Esc during a drag: the components go back to where the document has them. */
  async dragAbort() {
    const d = this.dragging;
    if (!d) return;
    await this.settled();
    this.dragging = null;
    await this.build();
  }

  /**
   * Starts SmartMates from a face, edge or vertex of a component (G1d). `at`: the grabbed
   * point when the component is dragged (Alt-drag; it follows the pointer if it is free to
   * move); null when Move Component's SmartMates mode picked it by double-click.
   */
  smartStart(from: SelItem, at: Vec3 | null, viewDir: Vec3): boolean {
    if (!("body" in from) || from.body === undefined || (from.kind !== "face" && from.kind !== "edge" && from.kind !== "vertex")) return false;
    const moving = !!at && this.dragStart(from.body, at, viewDir);
    this.smart = new SmartMates(this, from as SelItem & { body: number }, moving);
    this.app.changed();
    return true;
  }

  /** Ends a drag: the placements become the document's (one undo step). */
  async dragEnd() {
    const d = this.dragging;
    if (!d) return;
    // The newest target is answered first, so the document gets where the pointer let go.
    while (d.busy || d.next) await this.dragChain;
    this.dragging = null;
    if (!this.built) return;
    this.pushPlacements(d.before, d.how.startsWith("rot") ? "Rotate Component" : "Move Component");
  }

  /** The built placements become the document's, undone to `before`. */
  private pushPlacements(before: string, label: string) {
    if (!this.built) return;
    this.undoStack.push({ doc: before, label });
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

  // ------------------------------------------------------------------ move / rotate

  /** Opens the Move Component (or Rotate Component) PropertyManager. */
  openMove(rotate: boolean) {
    this.matePM = null;
    this.interferencePM = null;
    this.movePM = { rotate, move: "free", rot: "free", values: [0, 0, 0], entity: null, pickingEntity: false, smartMates: false };
    this.app.changed();
  }

  closeMove() {
    this.smart?.cancel();
    this.movePM = null;
    this.app.changed();
  }

  /** A pick while the Move/Rotate PropertyManager waits for its entity: a linear edge, an
   * axis (a cylinder's) or a plane's normal. Returns whether the pick was taken. */
  async movePick(item: SelItem): Promise<boolean> {
    const pm = this.movePM;
    if (!pm?.pickingEntity || !("body" in item) || item.body === undefined || (item.kind !== "edge" && item.kind !== "face")) return false;
    const occ = this.occurrences()[item.body];
    if (!occ) return false;
    try {
      const e = await invoke<MateEntity>("asm_entity", { part: occ.part, kind: item.kind, index: item.id, quality: this.quality });
      const g = entityGeom(e);
      const w = g && placedGeom(g, occ.world);
      if (!w?.d || g!.type === "sphere" || g!.type === "point") throw new Error("Select a linear edge, a cylindrical face or a planar face.");
      pm.entity = { label: this.refLabel({ occurrence: occ.path, entity: e }), p: w.p, d: w.d };
      pm.pickingEntity = false;
    } catch (err) {
      this.app.flash(String((err as Error).message ?? err), "warning");
    }
    this.app.changed();
    return true;
  }

  /** Apply in By Delta XYZ or To XYZ Position: moves or turns the components, respecting
   * their mates (as far as they allow). One undo step. */
  async applyMove(ids: Id[]) {
    const pm = this.movePM;
    if (!pm || !this.built || !ids.length) return;
    const before = JSON.stringify(this.doc);
    let moved = false;
    for (const id of ids) {
      const inst = this.built.instances.find((i) => i.id === id);
      const st = this.instanceState(id);
      if (!inst || !st || st.state === "fixed" || st.state === "suppressed") continue;
      const v = pm.values;
      let r: DragReply;
      if (pm.rotate) {
        // By Delta XYZ: about the assembly X, then Y, then Z axes through the component's
        // origin (🔍 SOLIDWORKS' order and centre).
        let t = inst.transform;
        ([[1, 0, 0], [0, 1, 0], [0, 0, 1]] as Vec3[]).forEach((ax, k) => {
          if (v[k]) t = rotated(t, t.position, ax, (v[k] * Math.PI) / 180);
        });
        r = await invoke<DragReply>("asm_place", { instance: id, to: t });
      } else {
        // The component's origin goes to the position, or by the delta: a pure
        // translation, or the nearest placement the mates allow.
        const p = inst.transform.position;
        const by: Vec3 = pm.move === "toXYZ" ? sub(v, p) : [...v];
        r = await invoke<DragReply>("asm_place", { instance: id, to: translated(inst.transform, by) });
      }
      moved = moved || r.moved;
      this.takePlacements(r);
    }
    if (moved) this.pushPlacements(before, pm.rotate ? "Rotate Component" : "Move Component");
  }

  // ------------------------------------------------------------------ evaluate

  /** Mass properties of the occurrences at their placements (unit density). */
  massProps(): Promise<MassReply> {
    return invoke<MassReply>("asm_mass", { quality: this.quality });
  }

  /**
   * Measure (Evaluate > Measure) on faces, edges and vertices picked in the graphics area:
   * each at its occurrence's placement, and the minimum distance between the first two.
   * Null when nothing measurable is picked.
   */
  async measure(items: SelItem[]): Promise<AsmMeasureReply | null> {
    const picks = items.flatMap((s) => ((s.kind === "face" || s.kind === "edge" || s.kind === "vertex") && "body" in s && s.body !== undefined ? [{ occ: s.body, kind: s.kind, index: s.id }] : []));
    if (!picks.length) return null;
    return invoke<AsmMeasureReply>("asm_measure", { picks, quality: this.quality });
  }

  /**
   * Opens Interference Detection. Components selected beforehand become its Selected
   * Components; otherwise the whole assembly is checked (SOLIDWORKS lists the top-level
   * assembly).
   */
  openInterference(preselected: Id[] = []) {
    this.matePM = null;
    this.movePM = null;
    this.interferencePM = {
      selected: preselected.map((id) => [id]),
      coincidence: false,
      showIgnored: false,
      subassemblies: false,
      includeHidden: false,
      transparent: true,
      others: "current",
      results: null,
      ignored: [],
      active: null,
      busy: false,
    };
    this.app.selection = [];
    this.app.changed();
  }

  closeInterference() {
    this.interferencePM = null;
    this.app.changed();
  }

  /** A pick while Interference Detection is open: the component under it (a top-level
   * instance, from the graphics area or the fly-out tree) is added to Selected
   * Components, or removed if it is there. The results are cleared until Calculate. */
  interferencePick(item: SelItem) {
    const pm = this.interferencePM;
    if (!pm) return;
    const id = item.kind === "feature" ? (this.doc.instances.some((i) => i.id === item.id) ? item.id : null) : "body" in item && item.body !== undefined ? this.instanceOfOccurrence(item.body) : null;
    if (!id) return;
    const k = pm.selected.findIndex((p) => p.length === 1 && p[0] === id);
    if (k >= 0) pm.selected.splice(k, 1);
    else pm.selected.push([id]);
    pm.results = null;
    pm.active = null;
    this.app.changed();
  }

  /** Calculate: interferences among the Selected Components (or the whole assembly). */
  async calculateInterference(): Promise<InterferenceResult[] | null> {
    const pm = this.interferencePM;
    if (!pm || pm.busy) return null;
    pm.busy = true;
    this.app.changed();
    try {
      const r = await invoke<InterferenceReply>("asm_interference", {
        selected: pm.selected,
        coincidence: pm.coincidence,
        subassemblies: pm.subassemblies,
        includeHidden: pm.includeHidden,
        quality: this.quality,
      });
      pm.results = r.interferences;
      pm.active = null;
      if (r.skipped.length) this.app.flash(`${r.skipped.length} component(s) could not be checked (no geometry).`, "warning");
      return r.interferences;
    } catch (e) {
      this.app.flash(`Interference Detection failed: ${(e as Error).message ?? e}`, "error");
      return null;
    } finally {
      pm.busy = false;
      this.app.changed();
    }
  }

  /** Ignore (or stop ignoring) an interference of Results. */
  ignoreInterference(k: number, ignore: boolean) {
    const pm = this.interferencePM;
    const r = pm?.results?.[k];
    if (!pm || !r) return;
    const key = pairKey(r);
    pm.ignored = pm.ignored.filter((x) => x !== key);
    if (ignore) pm.ignored.push(key);
    if (pm.active === k && ignore && !pm.showIgnored) pm.active = null;
    this.app.changed();
  }

  /** The interferences Results shows: ignored ones only with Show ignored interferences. */
  shownInterferences(): { r: InterferenceResult; k: number; ignored: boolean }[] {
    const pm = this.interferencePM;
    if (!pm?.results) return [];
    return pm.results.map((r, k) => ({ r, k, ignored: pm.ignored.includes(pairKey(r)) })).filter((x) => pm.showIgnored || !x.ignored);
  }

  /** The name of an occurrence path as Results lists it (`pin<2>`, `plate<1>@sub<1>`). */
  occurrenceName(path: Id[]): string {
    const top = this.doc.instances.find((i) => i.id === path[0])?.name ?? path[0];
    if (path.length === 1) return top;
    const o = this.occurrences().find((x) => x.path.join("/") === path.join("/"));
    return `${(o && this.built?.parts[o.part]?.name) ?? path[path.length - 1]}@${top}`;
  }

  // ------------------------------------------------------------------ mates

  /**
   * Opens the Mate PropertyManager, for a new mate or to edit `editing`. A reference of the
   * edited mate that no longer resolves is listed as **Missing** and is the one the next
   * pick replaces, as SOLIDWORKS asks to select it again.
   */
  openMate(editing: Id | null = null) {
    this.movePM = null;
    this.interferencePM = null;
    const m = editing ? this.doc.mates.find((x) => x.id === editing) : null;
    const missing = (editing && this.mateStatus(editing)?.missing) || [];
    this.matePM = m
      ? {
          type: m.type,
          picks: m.refs.map((r, k) => ({ label: missing.includes(k) ? "**Missing**" : this.refLabel(r), ref: r, ...this.worldOf(r), ...(missing.includes(k) ? { missing: true } : {}) })),
          align: m.align ?? "antiAligned",
          value: m.value ?? 0,
          flip: !!m.flip,
          editing,
          busy: false,
          problem: null,
          active: missing.length ? missing[0] : null,
          popupClosed: true,
        }
      : { type: "coincident", picks: [], align: "antiAligned", value: 10, flip: false, editing: null, busy: false, problem: null, active: null };
    this.app.selection = [];
    this.app.changed();
  }

  /** Clicking an entry of Mate Selections: the next pick replaces it. */
  setActivePick(k: number | null) {
    if (!this.matePM) return;
    this.matePM.active = k;
    this.app.changed();
  }

  /** Removes an entry of Mate Selections. */
  removePick(k: number) {
    const pm = this.matePM;
    if (!pm || !pm.picks[k]) return;
    pm.picks.splice(k, 1);
    pm.active = null;
    pm.problem = null;
    void this.preview();
  }

  /** The mate types the pop-up toolbar offers for the two picks (the others are greyed). */
  validTypes(): MateType[] {
    const pm = this.matePM;
    if (!pm || pm.picks.length !== 2) return [];
    const [a, b] = pm.picks.map((p) => entityGeom(p.ref.entity));
    return a && b ? validMates(a, b) : [];
  }

  /** A Mate Selections entry for a reference, at the current placements. */
  pickOf(ref: MateRef): MatePick {
    return { label: this.refLabel(ref), ref, ...this.worldOf(ref) };
  }

  /** World anchor and direction of a reference with the current placements. */
  private worldOf(r: MateRef): { p: Vec3; d: Vec3 | null } {
    const g = entityGeom(r.entity) ?? { type: "point" as const, p: [0, 0, 0] as Vec3 };
    const occ = this.occurrences().find((o) => o.path.join("/") === r.occurrence.join("/"));
    // A top-level instance of a sub-assembly has no occurrence of its own: its placement.
    const inst = !occ && r.occurrence.length === 1 ? this.built?.instances.find((i) => i.id === r.occurrence[0]) : undefined;
    return placedGeom(g, occ?.world ?? inst?.transform ?? IDENTITY);
  }

  refLabel(r: MateRef): string {
    const e = r.entity;
    const what =
      e.kind === "face"
        ? "Face"
        : e.kind === "edge"
          ? "Edge"
          : e.kind === "vertex"
            ? "Vertex"
            : e.kind === "plane"
              ? `${e.plane[0].toUpperCase()}${e.plane.slice(1)} Plane`
              : e.kind === "origin"
                ? "Origin"
                : e.kind;
    // SOLIDWORKS names the occurrence from the inside out: `Face@pin<1>@sub<1>`.
    const names: string[] = [];
    r.occurrence.forEach((id, k) => {
      if (k === 0) names.push(this.doc.instances.find((i) => i.id === id)?.name ?? id);
      else {
        const o = this.occurrences().find((x) => x.path.length === k + 1 && x.path.every((p, j) => p === r.occurrence[j]));
        names.push(o ? `${this.built?.parts[o.part]?.name ?? id}` : id);
      }
    });
    return `${what}@${names.length ? names.reverse().join("@") : this.doc.name}`;
  }

  /**
   * A pick while the Mate PropertyManager is open: a face, edge or vertex in the graphics
   * area (of a part, or of a part inside a sub-assembly), or in the fly-out FeatureManager
   * the assembly's or a component's Front, Top or Right Plane or Origin (`treeRef`).
   */
  async matePick(item: SelItem) {
    const pm = this.matePM;
    if (!pm || pm.busy) return;
    if (item.kind === "feature") {
      const ref = treeRef(item.id);
      if (ref) this.addPick(ref);
      return;
    }
    if (!("body" in item) || item.body === undefined) return;
    if (item.kind !== "face" && item.kind !== "edge" && item.kind !== "vertex") return;
    const occ = this.occurrences()[item.body];
    if (!occ) return;
    pm.busy = true;
    this.app.changed();
    try {
      const entity = await invoke<MateEntity>("asm_entity", { part: occ.part, kind: item.kind, index: item.id, quality: this.quality });
      this.addPick({ occurrence: occ.path, entity });
    } catch (e) {
      this.app.flash(String((e as Error).message ?? e), "warning");
    } finally {
      pm.busy = false;
      this.app.changed();
    }
  }

  /**
   * SmartMates: a face, edge or vertex Alt-dragged onto one of another component. The Mate
   * PropertyManager opens with the two picked, the proposed mate previewed and its pop-up
   * toolbar to confirm or change it (as SOLIDWORKS' SmartMates pop-up).
   */
  async smartMate(from: SelItem, to: SelItem) {
    const a = "body" in from ? from.body : undefined;
    const b = "body" in to ? to.body : undefined;
    if (a === undefined || b === undefined || this.instanceOfOccurrence(a) === this.instanceOfOccurrence(b)) return;
    this.openMate();
    await this.matePick(from);
    await this.matePick(to);
  }

  /** Adds a reference to Mate Selections, or replaces the active entry. */
  addPick(ref: MateRef) {
    const pm = this.matePM;
    if (!pm) return;
    let at: number;
    if (pm.active !== null && pm.active < pm.picks.length) at = pm.active;
    else {
      if (pm.picks.length >= 2) pm.picks = [];
      at = pm.picks.length;
    }
    // Both entities on one rigid component (a sub-assembly moves as one) cannot be mated.
    const top = (r: MateRef) => r.occurrence[0] ?? "";
    if (pm.picks.some((p, k) => k !== at && !p.missing && top(p.ref) === top(ref))) {
      this.app.flash(top(ref) ? "Pick the second entity on another component." : "Pick the second entity on a component.", "warning");
      return;
    }
    pm.picks[at] = { label: this.refLabel(ref), ref, ...this.worldOf(ref) };
    const next = pm.picks.findIndex((p) => p.missing);
    pm.active = next >= 0 ? next : null;
    if (pm.picks.length === 2 && next < 0) {
      if (!pm.editing) this.defaults(pm);
      pm.popupClosed = false;
      void this.preview();
    }
    this.app.changed();
  }

  /** Defaults once two entities are picked (`proposeMate`). */
  private defaults(pm: MatePM) {
    const p = proposeMate(pm.picks);
    pm.type = p.type;
    if (p.align) pm.align = p.align;
    pm.value = p.value;
    pm.flip = p.flip;
  }

  /** The mate SmartMates proposes for two entities, or null when the pair allows none. */
  propose(picks: MatePick[]): { type: MateType; align: Align; value: number; flip: boolean } | null {
    const [a, b] = picks.map((p) => entityGeom(p.ref.entity));
    if (!a || !b || !validMates(a, b).length) return null;
    const p = proposeMate(picks);
    return { ...p, align: p.align ?? "antiAligned" };
  }

  /** The mate the PropertyManager describes (null until two entities are picked). */
  pmMate(): Mate | null {
    const pm = this.matePM;
    if (!pm || pm.picks.length !== 2 || pm.picks.some((p) => p.missing)) return null;
    const id = pm.editing ?? this.nextId("m");
    const old = pm.editing ? this.doc.mates.find((m) => m.id === pm.editing) : null;
    const count = this.doc.mates.filter((m) => m.type === pm.type).length + 1;
    const m: Mate = { id, name: old && old.type === pm.type ? old.name : `${MATE_LABEL[pm.type]}${count}`, type: pm.type, refs: pm.picks.map((p) => p.ref) };
    if (old?.suppressed) m.suppressed = true;
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

  docWith(m: Mate | null): AssemblyDocument {
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
    const problem = !st || st.status === "ok" || st.status === "suppressed" ? null : st.status === "invalid" ? (st.message ?? "These entities cannot be mated this way.") : st.status === "missing" ? "A reference of this mate is missing." : OVER_DEFINES;
    if (pm) pm.problem = problem;
    this.app.changed();
    return problem;
  }

  /** OK in the Mate PropertyManager (or its pop-up toolbar). Returns the problem when the
   * mate cannot be added (the PropertyManager then stays open, as in SOLIDWORKS). */
  async acceptMate(): Promise<string | null> {
    const m = this.pmMate();
    if (!m) return this.matePM?.picks.some((p) => p.missing) ? "Select an entity to replace the missing reference." : "Select two entities to mate.";
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

  /**
   * Saves to `path` (Save As) or the current file; the engine rewrites component paths.
   * `components`: which of the components opened from the assembly and edited are saved
   * first (the Save Modified Documents dialog's choice, G1c); all of them by default. The
   * others stay edited in memory: the assembly still shows them, and the document stays
   * modified until they are saved or discarded.
   */
  async save(path: string, components: string[] = Object.keys(this.overrides)): Promise<boolean> {
    try {
      for (const file of components) {
        if (!(file in this.overrides)) continue;
        await saveComponent(file, this.overrides[file], this.overrides, this.quality);
      }
      const r = await invoke<{ doc: AssemblyDocument }>("asm_save", { doc: this.doc, path, from: this.path, quality: this.quality, overrides: this.overrides });
      for (const file of components) delete this.overrides[file];
      this.doc = r.doc;
      this.path = path;
      this.doc.name = path.replace(/\\/g, "/").split("/").pop()!.replace(/\.[^.]+$/, "");
      this.app.dirty = Object.keys(this.overrides).length > 0;
      this.app.flash(`Saved ${this.doc.name}`);
      return true;
    } catch (e) {
      this.app.flash(`Save failed: ${(e as Error).message ?? e}`, "error");
      return false;
    }
  }

  /** The file of an instance's component as last found (absolute, `/`-separated). */
  componentFile(id: Id): { path: string; kind: "part" | "assembly" } | null {
    const inst = this.doc.instances.find((i) => i.id === id);
    const comp = inst && this.componentOf(inst);
    const found = comp && this.built?.components.find((c) => c.id === comp.id)?.path;
    if (!comp || !found) return null;
    return { path: found.replace(/\\/g, "/"), kind: comp.kind };
  }
}

/**
 * Defaults once two entities are picked, as SOLIDWORKS proposes them: Concentric for two
 * round faces, else Coincident when the pair allows it; the alignment closest to the
 * current placement (when both have a direction); Distance and Angle start at the current
 * value.
 */
export function proposeMate(picks: MatePick[]): { type: MateType; align: Align | null; value: number; flip: boolean } {
  const [a, b] = picks;
  const round = (r: MateRef) => {
    const g = entityGeom(r.entity);
    return g?.type === "cylinder" || g?.type === "cone";
  };
  const [ga, gb] = picks.map((p) => entityGeom(p.ref.entity));
  const valid = ga && gb ? validMates(ga, gb) : [];
  const type: MateType = round(a.ref) && round(b.ref) ? "concentric" : valid.includes("coincident") ? "coincident" : valid.includes("concentric") ? "concentric" : (valid[0] ?? "coincident");
  const align: Align | null = a.d && b.d ? (dot(a.d, b.d) >= 0 ? "aligned" : "antiAligned") : null;
  if (a.d) {
    const off = dot(sub(b.p, a.p), a.d) / (len(a.d) || 1);
    return { type, align, value: Math.abs(off) > 1e-9 ? Math.abs(off) : len(sub(b.p, a.p)), flip: off < 0 };
  }
  return { type, align, value: len(sub(b.p, a.p)), flip: false };
}

/** The Mate Selections reference of a fly-out FeatureManager row: the assembly's or an
 * instance's Front/Top/Right Plane or Origin (`$plane:front`, `i2/$plane:top`, `$origin`,
 * `i2/$origin`). */
export function treeRef(id: string): MateRef | null {
  const m = /^(?:([^/$]+)\/)?\$(?:plane:(front|top|right)|origin)$/.exec(id);
  if (!m) return null;
  const entity: MateEntity = m[2] ? { kind: "plane", plane: m[2] as "front" | "top" | "right" } : { kind: "origin" };
  return { occurrence: m[1] ? [m[1]] : [], entity };
}

/** `b` in `a`'s coordinates. */
export function relative(a: Transform, b: Transform): Transform {
  const [w, x, y, z] = a.rotation;
  const ai: Transform["rotation"] = [w, -x, -y, -z];
  const q = qmul(ai, b.rotation);
  return { position: applyInverse(a, b.position), rotation: q };
}

/** `a ∘ b`. */
export function compose(a: Transform, b: Transform): Transform {
  return { position: applyWorld(a, b.position), rotation: qmul(a.rotation, b.rotation) };
}

export function inverse(t: Transform): Transform {
  const [w, x, y, z] = t.rotation;
  const qi: Transform["rotation"] = [w, -x, -y, -z];
  const p = qrot(qi, t.position);
  return { position: [-p[0], -p[1], -p[2]], rotation: qi };
}

export function translated(t: Transform, by: Vec3): Transform {
  return { position: [t.position[0] + by[0], t.position[1] + by[1], t.position[2] + by[2]], rotation: t.rotation };
}

/** `t` turned by `angle` about the line through `at` with direction `axis`. */
export function rotated(t: Transform, at: Vec3, axis: Vec3, angle: number): Transform {
  const u = unit(axis);
  const s = Math.sin(angle / 2);
  const r: Transform["rotation"] = [Math.cos(angle / 2), u[0] * s, u[1] * s, u[2] * s];
  const p = qrot(r, sub(t.position, at));
  return { position: [p[0] + at[0], p[1] + at[1], p[2] + at[2]], rotation: qmul(r, t.rotation) };
}

function unit(a: Vec3): Vec3 {
  const l = len(a) || 1;
  return [a[0] / l, a[1] / l, a[2] / l];
}

function cross(a: Vec3, b: Vec3): Vec3 {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}

function qmul(a: Transform["rotation"], b: Transform["rotation"]): Transform["rotation"] {
  return [
    a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
    a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
    a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
    a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
  ];
}

/** Writes a component opened from an assembly and edited (`file` → its document text): a
 * part's JSON, or a sub-assembly through the engine (its caches refreshed). */
export async function saveComponent(file: string, text: string, overrides: Record<string, string>, quality: number) {
  if (/\.ocasm$/i.test(file)) await invoke("asm_save", { doc: JSON.parse(text), path: file, from: file, quality, overrides });
  else await invoke("app_write_text", { path: file, text });
}

/** Opens an `.ocasm` file through the engine. */
export async function openAssembly(app: App, path: string): Promise<AssemblySession> {
  const r = await invoke<{ doc: AssemblyDocument; sourceVersion: number }>("asm_open", { path });
  const s = new AssemblySession(app, r.doc, path);
  s.doc.name = path.replace(/\\/g, "/").split("/").pop()!.replace(/\.[^.]+$/, "");
  return s;
}
