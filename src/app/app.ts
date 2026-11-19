/**
 * Application controller: owns the document, selection, undo history, the active
 * sketch session and the PropertyManager state. UI components subscribe to it;
 * the viewport reads from it every frame it renders.
 */
import type { AssemblySession } from "../assembly/session";
import type { AsmMeasureReply } from "../assembly/types";
import type {
  RibFeature,
  DraftFeature,
  HoleFeature,
  SweepFeature,
  LoftFeature,
  CoordSysFeature,
  RefAxisFeature,
  RefPointFeature,
  CircularPatternFeature,
  GeomRef,
  LinearPatternFeature,
  MirrorFeature,
  ShellFeature,
  ChamferFeature,
  ExtrudeFeature,
  Feature,
  FilletFeature,
  Frame,
  Id,
  PartDocument,
  PlaneRef,
  RebuildResult,
  RefPlaneFeature,
  RevolveFeature,
  SketchData,
  SketchFeature,
} from "../core/types";
import { DOC_VERSION } from "../core/types";
import { migrateDocument } from "../core/migrate";
import { STD_FRAMES } from "../core/math";
import { deepClone, uid } from "../core/ids";
import { KernelClient, type KernelApi } from "../kernel/client";
import { NativeKernelClient } from "../native/engine";
import { invoke, isNative } from "../native/tauri";
import { circumcenter, emptySketch, isEmptySketch, pruneVirtualPoints } from "../sketch/model";
import { pruneUnusedRefs } from "../sketch/refs";
import { newSketchSystem, solveSketch, type Analysis } from "../sketch/solver";
import { loadOptions, saveOptions, type SystemOptions } from "./settings";
import { OP_TOOLS, type SketchEditor } from "../sketch/editor";
import { DOC_DEFAULTS, SYSTEM_PAGES, collectDefaults } from "./optionsSchema";
import { History, TextHistory } from "./history";
import { sketchesOf } from "../core/features";
import { featureParams, setFeatureParam } from "../core/featureParams";
import { applyEquations } from "../core/equations";
import { UNIT_INFO } from "../core/units";

const SYSTEM_DEFAULTS = collectDefaults(SYSTEM_PAGES);

/**
 * Features a pattern or mirror can repeat ("Features to Pattern"): everything that adds or
 * removes material, plus fillets and chamfers, which are re-applied to each instance of
 * their parent feature (as in SOLIDWORKS).
 */
const PATTERNABLE = new Set<string>(["extrude", "revolve", "sweep", "loft", "hole", "rib", "linearPattern", "circularPattern", "mirror", "fillet", "chamfer"]);

export type SelItem =
  // `body`: the occurrence index in an assembly (none for the part's own body).
  | { kind: "face"; id: number; body?: number }
  | { kind: "edge"; id: number; body?: number }
  | { kind: "vertex"; id: number; body?: number }
  | { kind: "plane"; id: string }
  | { kind: "origin" }
  | { kind: "feature"; id: Id }
  | { kind: "skEntity"; id: Id }
  | { kind: "skPoint"; id: Id }
  | { kind: "skDim"; id: Id };

export function selKey(s: SelItem): string {
  if (s.kind === "origin") return "origin";
  return "body" in s && s.body !== undefined ? `${s.kind}:${s.body}:${s.id}` : `${s.kind}:${s.id}`;
}

export type PMState =
  | { kind: "extrude"; feature: ExtrudeFeature; editing: boolean }
  | { kind: "revolve"; feature: RevolveFeature; editing: boolean }
  | { kind: "fillet"; feature: FilletFeature; editing: boolean }
  | { kind: "chamfer"; feature: ChamferFeature; editing: boolean }
  | { kind: "refplane"; feature: RefPlaneFeature; editing: boolean }
  | { kind: "linearPattern"; feature: LinearPatternFeature; editing: boolean }
  | { kind: "circularPattern"; feature: CircularPatternFeature; editing: boolean }
  | { kind: "mirror"; feature: MirrorFeature; editing: boolean }
  | { kind: "shell"; feature: ShellFeature; editing: boolean }
  | { kind: "sweep"; feature: SweepFeature; editing: boolean }
  | { kind: "hole"; feature: HoleFeature; editing: boolean }
  | { kind: "draft"; feature: DraftFeature; editing: boolean }
  | { kind: "rib"; feature: RibFeature; editing: boolean }
  | { kind: "loft"; feature: LoftFeature; editing: boolean }
  | { kind: "refaxis"; feature: RefAxisFeature; editing: boolean }
  | { kind: "refpoint"; feature: RefPointFeature; editing: boolean }
  | { kind: "coordsys"; feature: CoordSysFeature; editing: boolean }
  | { kind: "sketchTool"; tool: string }
  | { kind: "smartDimension" }
  | { kind: "section" }
  | { kind: "addRelations" };

/** What the Modify box edits outside an open sketch. */
export type ModifyTarget = { kind: "sketchDim"; sketchId: Id; dimId: Id } | { kind: "feature"; featureId: Id; key: string };

export interface SketchSession {
  featureId: Id;
  data: SketchData;
  frame: Frame;
  isNew: boolean;
  analysis: Analysis;
  /** Sketch-level undo while the sketch is open. */
  history: TextHistory;
  original: string;
  /** The document's unsaved-changes flag before the sketch opened (restored if it leaves no change). */
  wasDirty: boolean;
}

export interface Message {
  text: string;
  kind: "info" | "error" | "warning";
}

type Listener = () => void;

/** JSON with object keys sorted, for comparing documents across engines. */
function canon(v: unknown): string {
  return JSON.stringify(v, (_k, x) => (x && typeof x === "object" && !Array.isArray(x) ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, x[k]])) : x));
}

export class App {
  doc: PartDocument;
  options: SystemOptions = loadOptions();
  /** The OCCT WebAssembly worker, or the native engine inside the desktop app. */
  kernel: KernelApi = isNative() ? new NativeKernelClient() : new KernelClient();
  kernelReady = false;
  result: RebuildResult | null = null;
  previewResult: RebuildResult | null = null;
  rebuilding = false;
  dirty = false;
  fileHandle: FileSystemFileHandle | null = null;
  /** A part opened through the native file dialog: its path (Save writes there). */
  filePath: string | null = null;
  /** The open assembly (native app only, OVERHAUL_PLAN G1a); the part is unused meanwhile. */
  asm: AssemblySession | null = null;
  /** Assemblies a component was opened from (Open Part / Open Assembly, G1b), innermost
   * last: Return to Assembly goes back to the last one. `file` is the component opened. */
  asmStack: { session: AssemblySession; dirty: boolean; file: string }[] = [];

  selection: SelItem[] = [];
  hover: SelItem | null = null;
  sketch: SketchSession | null = null;
  /** Active sketch tool id (line, circle, ...) or null for the select tool. */
  tool: string | null = null;
  pm: PMState | null = null;
  message: Message | null = null;
  /** Inline "Modify" dimension editor. */
  modify: { dimId: Id; x: number; y: number; isNew: boolean; target?: ModifyTarget } | null = null;
  /** Features whose dimensions are shown in the graphics area (double-click a feature). */
  shownDims: Id[] = [];
  /** A part dimension changed and the model has not been rebuilt yet (traffic light). */
  needsRebuild = false;
  dirtyFeatures = new Set<Id>();
  cursorSketch: [number, number] | null = null;
  /** Interactive sketch editor (owned by the viewport while a sketch is open). */
  editor: SketchEditor | null = null;
  /** Options chosen in sketch tool PropertyManagers; they persist while the app is open. */
  toolOpts = {
    lineOrientation: "asSketched" as "asSketched" | "horizontal" | "vertical" | "angle",
    lineAngle: 0,
    construction: false,
    rectConstruction: false,
    rectFromMidpoints: false,
    polygonSides: 6,
    polygonInscribed: true,
    filletRadius: 10,
    filletKeepCorners: true,
    filletDimensionEach: false,
    chamferType: "distanceDistance" as "distanceDistance" | "angleDistance",
    chamferD1: 10,
    chamferD2: 10,
    chamferAngle: Math.PI / 4,
    chamferEqual: true,
    offsetDistance: 10,
    offsetReverse: false,
    offsetBidirectional: false,
    offsetCapEnds: false,
    offsetBaseConstruction: false,
    offsetConstruction: false,
    offsetAddDimensions: true,
    offsetSelectChain: true,
    mirrorCopy: true,
    moveMode: "fromTo" as "fromTo" | "xy",
    moveDX: 0,
    moveDY: 0,
    keepRelations: false,
    rotateAngle: Math.PI / 2,
    scaleFactor: 1,
    scaleCopy: false,
    lpSpacing1: 10,
    lpCount1: 2,
    lpAngle1: 0,
    lpSpacing2: 10,
    lpCount2: 1,
    lpAngle2: Math.PI / 2,
    cpAngle: 2 * Math.PI,
    cpCount: 4,
    cpEqual: true,
    cpReverse: false,
  };
  /** Most recently run commands, newest first (context menu > Recent Commands). */
  recentCommands: string[] = [];
  /** Sketch tool picked before a sketch plane was chosen. */
  pendingTool: string | null = null;
  /** For Enter = repeat last command. */
  lastCommand: string | null = null;

  /** Part-level undo history (sketch edits have their own while a sketch is open). */
  history = new History();
  private listeners = new Set<Listener>();
  private version = 0;
  /** Viewport hooks (set by the viewport). */
  requestRender: () => void = () => {};
  viewHooks: {
    normalTo?: (frame: Frame, animate?: boolean) => void;
    leaveNormal?: (frame: Frame) => void;
    fit?: () => void;
    orient?: (name: string) => void;
    /** Zoom by k about a world point (keeps a sketch that was scaled by k the same size on screen). */
    scaleAbout?: (p: Frame["origin"], k: number) => void;
  } = {};

  constructor() {
    this.doc = App.newDocument(1);
    this.kernel.init().then(() => {
      this.kernelReady = true;
      this.rebuild();
    });
  }

  static newDocument(n: number): PartDocument {
    return { format: "opencad-part", version: DOC_VERSION, name: `Part${n}`, units: "MMGS", features: [], rollback: null };
  }

  // ------------------------------------------------------------ subscription
  subscribe = (l: Listener) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };
  getVersion = () => this.version;

  /** Separate, cheap channel for the cursor coordinate readout (avoids re-rendering the UI). */
  private cursorListeners = new Set<Listener>();
  private cursorVersion = 0;
  subscribeCursor = (l: Listener) => {
    this.cursorListeners.add(l);
    return () => this.cursorListeners.delete(l);
  };
  getCursorVersion = () => this.cursorVersion;
  setCursor(p: [number, number] | null) {
    this.cursorSketch = p;
    this.cursorVersion++;
    this.cursorListeners.forEach((l) => l());
  }
  changed() {
    this.version++;
    this.listeners.forEach((l) => l());
    this.requestRender();
  }

  setOptions(mut: (o: SystemOptions) => void) {
    mut(this.options);
    saveOptions(this.options);
    this.changed();
  }

  /** Read a Tools > Options value by schema key (live path, stored extra, or default). */
  opt<T = unknown>(key: string): T {
    const path = getPath(this.options, key);
    if (path.found) return path.value as T;
    if (key in this.options.extra) return this.options.extra[key] as T;
    return SYSTEM_DEFAULTS[key] as T;
  }

  setOpt(key: string, value: unknown) {
    if (!setPath(this.options, key, value)) this.options.extra[key] = value;
    saveOptions(this.options);
    this.changed();
  }

  /** Document Properties value. */
  docProp<T = unknown>(key: string): T {
    const v = this.doc.props?.[key];
    return (v === undefined ? DOC_DEFAULTS[key] : v) as T;
  }

  setDocProp(key: string, value: unknown) {
    this.doc.props = { ...(this.doc.props ?? {}), [key]: value };
    this.dirty = true;
    this.changed();
  }

  /** Transition length in seconds from the View > Transitions slider (0 = off, 10 = fast). */
  transition(key = "view.transition") {
    const v = Number(this.opt(key) ?? 10);
    return v <= 0 ? 0 : 0.18 + (10 - v) * 0.07;
  }

  flash(text: string, kind: Message["kind"] = "info") {
    this.message = { text, kind };
    this.changed();
  }

  // ------------------------------------------------------------ document ops
  /** Record an undo point before mutating the document. */
  checkpoint(label = "Edit") {
    this.history.push(this.doc, label);
    this.dirty = true;
  }

  /** A feature PropertyManager or the Modify box is open: Undo waits until it closes. */
  private undoBlocked() {
    return !!this.modify || !!(this.pm && "feature" in this.pm);
  }

  canUndo() {
    if (this.asm) return this.asm.canUndo();
    if (this.undoBlocked()) return false;
    return this.sketch ? this.sketch.history.canUndo : this.history.canUndo;
  }

  canRedo() {
    if (this.asm) return this.asm.canRedo();
    if (this.undoBlocked()) return false;
    return this.sketch ? this.sketch.history.canRedo : this.history.canRedo;
  }

  undo() {
    if (this.asm) return this.asm.matePM || this.asm.placing || this.asm.isDragging ? undefined : this.asm.undo();
    if (this.undoBlocked()) return;
    if (this.sketch) return this.sketchUndo();
    const d = this.history.undo(this.doc);
    if (!d) return;
    this.restoreDoc(d);
  }

  redo() {
    if (this.asm) return this.asm.matePM || this.asm.placing || this.asm.isDragging ? undefined : this.asm.redo();
    if (this.undoBlocked()) return;
    if (this.sketch) return this.sketchRedo();
    const d = this.history.redo(this.doc);
    if (!d) return;
    this.restoreDoc(d);
  }

  private restoreDoc(d: PartDocument) {
    this.doc = d;
    this.dirty = true;
    // Keep selections that still exist (tree items); geometry ids change on rebuild.
    this.selection = this.selection.filter((s) => (s.kind === "feature" ? !!this.featureById(s.id) : s.kind === "plane" ? ["front", "top", "right"].includes(s.id) || !!this.featureById(s.id) : false));
    this.rebuild();
  }

  /**
   * Edit > Rebuild (Ctrl+B). In a sketch it exits the sketch and rebuilds, as SOLIDWORKS
   * does. Force Rebuild (Ctrl+Q) also discards every cached feature result first.
   */
  rebuildCommand(force: boolean) {
    if (this.modify) {
      this.modify = null;
      this.changed();
    }
    if (this.sketch) {
      if (force) this.kernel.clearCache();
      this.exitSketch(true);
      return;
    }
    if (this.pm && "feature" in this.pm) {
      this.schedulePreview();
      return;
    }
    if (force) this.kernel.clearCache();
    const t0 = performance.now();
    this.rebuild().then(() => {
      if (!this.result?.statuses.some((s) => s.error)) this.flash(`Rebuild completed (${((performance.now() - t0) / 1000).toFixed(2)} s)`, "info");
    });
  }

  /** Features that are currently active (respecting the rollback bar and sketch editing). */
  activeFeatures(): Feature[] {
    let feats = this.doc.features;
    if (this.doc.rollback !== null) feats = feats.slice(0, this.doc.rollback);
    if (this.sketch) {
      const idx = feats.findIndex((f) => f.id === this.sketch!.featureId);
      if (idx >= 0) feats = feats.slice(0, idx + 1);
    }
    return feats;
  }

  async rebuild() {
    if (!this.kernelReady) return;
    this.needsRebuild = false;
    this.dirtyFeatures.clear();
    this.runEquations();
    this.kernel.quality = Number(this.docProp("iq.shaded") ?? 3);
    this.rebuilding = true;
    this.changed();
    try {
      this.result = await this.kernel.rebuild(this.activeFeatures());
      this.applyHeals(this.result.heals);
      this.applyResolvedSketches(this.result.sketches);
      const errs = this.result.statuses.filter((s) => s.error);
      const warns = this.result.statuses.filter((s) => s.warning);
      if (errs.length) {
        const f = this.doc.features.find((x) => x.id === errs[0].id);
        this.flash(`${f?.name ?? "Feature"}: ${errs[0].error}`, "error");
      } else if (warns.length) {
        const f = this.doc.features.find((x) => x.id === warns[0].id);
        this.flash(`${f?.name ?? "Feature"}: ${warns[0].warning}`, "warning");
      }
    } catch (e) {
      this.flash(`Rebuild failed: ${(e as Error).message}`, "error");
    } finally {
      this.rebuilding = false;
      this.changed();
    }
  }

  /**
   * Legacy references (no persistent name) the native engine found by signature come back
   * with their names: write them into the features they were found for, if those are
   * unchanged, without an undo step and without making the document dirty (section 3.3,
   * step 4, of docs/OVERHAUL_PLAN.md). The engine's cache already holds the healed
   * features, so the next rebuild is a cache hit.
   */
  /**
   * The native engine could not find this reference (a stored signature, or `{ p }` of a
   * vertex) of feature `featureId` on the model: its selection box shows "Missing".
   */
  isMissingRef(featureId: Id, sig: unknown): boolean {
    const r = (this.pm && "feature" in this.pm && this.pm.feature.id === featureId && this.previewResult) || this.result;
    const list = r?.missing?.[featureId];
    if (!list?.length) return false;
    const key = canon(sig);
    return list.some((x) => canon(x) === key);
  }

  private applyHeals(heals?: RebuildResult["heals"]) {
    if (!heals?.length) return;
    for (const h of heals) {
      const i = this.doc.features.findIndex((f) => f.id === h.id);
      if (i >= 0 && canon(this.doc.features[i]) === canon(h.from)) this.doc.features[i] = h.feature;
    }
  }

  /**
   * Sketches with model references come back re-solved against the rebuilt model: store
   * the moved geometry, so sketches follow the model. In the open sketch only the reference
   * geometry moves (the user's own edits stay) and it is re-solved.
   */
  private applyResolvedSketches(res?: Record<Id, SketchData>) {
    if (!res) return;
    const moved = (a: { x: number; y: number }, b: { x: number; y: number }) => Math.abs(a.x - b.x) > 1e-9 || Math.abs(a.y - b.y) > 1e-9;
    for (const [id, data] of Object.entries(res)) {
      const open = this.sketch?.featureId === id ? this.sketch : null;
      if (open) {
        const sk = open.data;
        let changed = false;
        for (const p of Object.values(data.points)) {
          const q = sk.points[p.id];
          if (p.ext && q?.ext && moved(p, q)) [q.x, q.y, changed] = [p.x, p.y, true];
        }
        for (const e of Object.values(data.entities)) {
          const g = sk.entities[e.id];
          if (e.type === "circle" && e.ext && g?.type === "circle" && Math.abs(g.r - e.r) > 1e-9) [g.r, changed] = [e.r, true];
        }
        for (const r of Object.values(data.refs ?? {})) if (sk.refs?.[r.id]) sk.refs[r.id] = r;
        if (changed) open.analysis = solveSketch(sk);
        continue;
      }
      const f = this.featureById(id);
      if (!f || f.type !== "sketch") continue;
      const differs =
        Object.values(data.points).some((p) => !f.sketch.points[p.id] || moved(p, f.sketch.points[p.id])) ||
        Object.values(data.entities).some((e) => e.type === "circle" && !(Math.abs(((f.sketch.entities[e.id] as { r?: number } | undefined)?.r ?? Infinity) - e.r) <= 1e-9));
      if (differs) f.sketch = data;
    }
  }

  private previewTimer = 0;
  /** Debounced preview rebuild for the open PropertyManager. */
  schedulePreview() {
    clearTimeout(this.previewTimer);
    this.previewTimer = window.setTimeout(async () => {
      const pm = this.pm;
      if (!pm || !("feature" in pm)) {
        this.previewResult = null;
        this.changed();
        return;
      }
      let feats = this.activeFeatures();
      if (pm.editing) {
        // Replace the edited feature and roll back to it.
        const i = feats.findIndex((f) => f.id === pm.feature.id);
        feats = feats.slice(0, i);
      }
      try {
        this.previewResult = await this.kernel.rebuild(feats, pm.feature);
        const st = this.previewResult.statuses.find((s) => s.id === pm.feature.id);
        this.message = st?.error ? { text: st.error, kind: "error" } : null;
      } catch (e) {
        this.message = { text: (e as Error).message, kind: "error" };
      }
      this.changed();
    }, 60);
  }

  nextName(base: string) {
    let n = 1;
    const names = new Set(this.doc.features.map((f) => f.name));
    while (names.has(`${base}${n}`)) n++;
    return `${base}${n}`;
  }

  featureById(id: Id) {
    return this.doc.features.find((f) => f.id === id);
  }

  frameOfPlane(id: string): Frame | null {
    if (id === "front" || id === "top" || id === "right") return STD_FRAMES[id];
    return this.result?.planes[id]?.frame ?? null;
  }

  // ------------------------------------------------------------ selection
  select(item: SelItem | null, mode: "replace" | "toggle" | "add" = "replace") {
    if (this.asm?.matePM) {
      if (item) void this.asm.matePick(item);
      return;
    }
    if (this.asm?.movePM?.pickingEntity) {
      if (item) void this.asm.movePick(item);
      return;
    }
    if (this.asm?.interferencePM) {
      if (item) this.asm.interferencePick(item);
      return;
    }
    if (this.isFieldPM()) {
      if (item) this.pmPick(item);
      return;
    }
    if (this.pm?.kind === "section" && item) {
      const ref = this.refOf(item);
      if (ref && (ref.kind === "plane" || (ref.kind === "face" && ref.sig.surface === "PLANE")) && this.section) {
        this.section.ref = ref;
        this.section.offset = 0;
        this.changed();
      }
      return;
    }
    if (!item) {
      if (mode === "replace") {
        this.selection = [];
        this.shownDims = [];
      }
    } else {
      const k = selKey(item);
      const i = this.selection.findIndex((s) => selKey(s) === k);
      if (mode === "replace") this.selection = [item];
      else if (mode === "toggle") {
        if (i >= 0) this.selection.splice(i, 1);
        else this.selection.push(item);
      } else if (i < 0) this.selection.push(item);
    }
    this.onSelectionChanged();
    if (this.measureOpen) this.runMeasure();
    this.changed();
  }

  isSelected(item: SelItem) {
    const k = selKey(item);
    return this.selection.some((s) => selKey(s) === k);
  }

  clearSelection() {
    this.selection = [];
    this.shownDims = [];
    this.changed();
  }

  setHover(item: SelItem | null) {
    const a = this.hover ? selKey(this.hover) : "";
    const b = item ? selKey(item) : "";
    if (a === b) return;
    this.hover = item;
    this.changed();
  }

  /** Active selection box of a field PropertyManager (patterns, mirror, shell). */
  pmField: string = "seeds";

  /** PropertyManagers whose selection boxes receive graphics and tree picks. */
  isFieldPM() {
    const k = this.pm?.kind;
    return k === "linearPattern" || k === "circularPattern" || k === "mirror" || k === "shell" || k === "extrude" || k === "revolve" || k === "refplane" || k === "refaxis" || k === "refpoint" || k === "coordsys" || k === "sweep" || k === "loft" || k === "hole" || k === "draft";
  }

  /** Instances to Skip: skip or restore a pattern instance by its preview key ("i,j" or "i"). */
  toggleSkip(key: string) {
    const pm = this.pm;
    if (pm?.kind === "linearPattern") {
      const [i, j] = key.split(",").map(Number);
      const list = pm.feature.skip ?? [];
      const has = list.some(([a, b]) => a === i && b === j);
      const next = has ? list.filter(([a, b]) => a !== i || b !== j) : [...list, [i, j] as [number, number]];
      pm.feature.skip = next.length ? next.sort((a, b) => a[0] - b[0] || a[1] - b[1]) : undefined;
    } else if (pm?.kind === "circularPattern") {
      const n = Number(key);
      const list = pm.feature.skip ?? [];
      const next = list.includes(n) ? list.filter((x) => x !== n) : [...list, n];
      pm.feature.skip = next.length ? next.sort((a, b) => a - b) : undefined;
    } else return;
    this.schedulePreview();
    this.changed();
  }

  /** Selected Contours: toggle the region of the profile sketch containing `p`. */
  toggleContour(p: [number, number], regionOf: (q: [number, number]) => unknown) {
    const pm = this.pm;
    if (!pm || (pm.kind !== "extrude" && pm.kind !== "revolve")) return;
    const list = pm.feature.contours ?? [];
    const r = regionOf(p);
    if (!r) return;
    const i = list.findIndex((q) => regionOf(q) === r);
    pm.feature.contours = i >= 0 ? list.filter((_, k) => k !== i) : [...list, p];
    this.schedulePreview();
    this.changed();
  }

  /** Mesh the graphics area is showing (the preview's while a feature previews). */
  private shownMesh() {
    return (this.pm && "feature" in this.pm && this.previewResult?.mesh) || this.result?.mesh || null;
  }

  /** Feature whose body a pick refers to (tree feature, or the feature that made a face). */
  private seedOf(item: SelItem): Id | null {
    const ok = (id: Id | undefined) => {
      const f = id ? this.featureById(id) : null;
      return f && PATTERNABLE.has(f.type) ? f.id : null;
    };
    if (item.kind === "feature") return ok(item.id);
    if (item.kind === "face") return ok(this.shownMesh()?.faces[item.id]?.owner);
    return null;
  }

  /** Geometric reference of a pick (edge, face, plane). */
  private refOf(item: SelItem): GeomRef | null {
    const mesh = this.shownMesh();
    if (item.kind === "edge" && mesh?.edges[item.id]) return { kind: "edge", sig: mesh.edges[item.id].sig };
    if (item.kind === "face" && mesh?.faces[item.id]) return { kind: "face", sig: mesh.faces[item.id].sig };
    if (item.kind === "plane") {
      if (item.id === "front" || item.id === "top" || item.id === "right") return { kind: "plane", plane: { kind: "std", plane: item.id } };
      return { kind: "plane", plane: { kind: "refplane", featureId: item.id } };
    }
    if (item.kind === "feature") {
      const t = this.featureById(item.id)?.type;
      if (t === "refplane") return { kind: "plane", plane: { kind: "refplane", featureId: item.id } };
      if (t === "refaxis") return { kind: "axis", featureId: item.id };
      if (t === "refpoint") return { kind: "point", featureId: item.id };
    }
    if (item.kind === "vertex" && mesh?.vertices[item.id]) {
      const name = mesh.vertexNames?.[item.id];
      return { kind: "vertex", p: mesh.vertices[item.id], ...(name ? { name } : {}) };
    }
    if (item.kind === "origin") return { kind: "origin" };
    return null;
  }

  /** Route a pick into the active selection box of a field PropertyManager. */
  pmPick(item: SelItem) {
    const pm = this.pm;
    if (!pm) return;
    const field = this.pmField;
    const toggle = (list: Id[], id: Id) => (list.includes(id) ? list.filter((x) => x !== id) : [...list, id]);
    const mark = () => {
      const k = selKey(item);
      const i = this.selection.findIndex((s) => selKey(s) === k);
      if (i >= 0) this.selection.splice(i, 1);
      else this.selection.push(item);
    };
    if (pm.kind === "extrude" || pm.kind === "revolve") {
      const f = pm.feature;
      const fe = item.kind === "feature" ? this.featureById(item.id) : null;
      if (fe?.type === "sketch") {
        if (pm.editing) return;
        f.sketchId = fe.id;
        if (pm.kind === "revolve") pm.feature.axis = this.findAxis(fe.id) ?? "";
        else pm.feature.contours = undefined;
      } else if (pm.kind === "revolve" && field === "axis") {
        const ref = this.refOf(item);
        if (!ref || (ref.kind === "edge" && ref.sig.curve !== "LINE" && ref.sig.curve !== "CIRCLE") || ref.kind === "vertex" || ref.kind === "origin" || ref.kind === "point") return this.flash("Select a line, edge or axis.", "info");
        pm.feature.axisRef = ref;
      } else if (pm.kind === "extrude") {
        const ef = pm.feature;
        const target = field === "end1" ? ef.end1 : field === "dir2" ? ef.dir2 : null;
        if (field === "from") {
          const ref = this.refOf(item);
          if (!ref || (ref.kind === "face" && ref.sig.surface !== "PLANE") || ref.kind === "edge") return this.flash("Select a plane or planar face to start from.", "info");
          ef.from = { ...(ef.from ?? { type: "surface" }), type: "surface", ref };
        } else if (target) {
          if (target.type === "upToVertex") {
            const v = item.kind === "vertex" ? this.shownMesh()?.vertices[item.id] : null;
            if (!v) return this.flash("Select a vertex.", "info");
            target.vertex = v;
          } else {
            const ref = this.refOf(item);
            if (!ref || ref.kind === "edge" || (ref.kind === "face" && ref.sig.surface !== "PLANE")) return this.flash("Select a planar face or a plane.", "info");
            target.ref = ref;
          }
        } else return;
      } else return;
      this.schedulePreview();
      this.changed();
      return;
    }
    if (pm.kind === "refplane" || pm.kind === "refaxis" || pm.kind === "refpoint") {
      const ref = this.refOf(item);
      if (!ref) return;
      const f = pm.feature;
      const refs = [...(f.refs ?? [])];
      const i = Math.min(Number(field.replace("ref", "")) || 0, 2);
      refs[i] = ref;
      f.refs = refs.filter(Boolean);
      // Move on to the next reference box, as SOLIDWORKS does.
      this.pmField = `ref${Math.min(f.refs.length, 2)}`;
      this.schedulePreview();
      this.changed();
      return;
    }
    if (pm.kind === "draft") {
      const ref = this.refOf(item);
      if (field === "neutral") {
        if (!ref || ref.kind === "edge" || (ref.kind === "face" && ref.sig.surface !== "PLANE")) return this.flash("Select a plane or planar face for the neutral plane.", "info");
        pm.feature.neutral = ref;
        this.pmField = "faces";
      } else {
        if (item.kind !== "face" || ref?.kind !== "face") return;
        const i = pm.feature.faces.findIndex((x) => JSON.stringify(x) === JSON.stringify(ref.sig));
        if (i >= 0) pm.feature.faces.splice(i, 1);
        else pm.feature.faces.push(ref.sig);
        const k = selKey(item);
        const j = this.selection.findIndex((s) => selKey(s) === k);
        if (j >= 0) this.selection.splice(j, 1);
        else this.selection.push(item);
      }
      this.schedulePreview();
      this.changed();
      return;
    }
    if (pm.kind === "hole") {
      // Picking a planar face (re)starts the positions on it; points are clicked by the viewport.
      if (pm.editing || item.kind !== "face") return;
      const sig = this.shownMesh()?.faces[item.id]?.sig;
      if (!sig || sig.surface !== "PLANE") return this.flash("Select a planar face.", "info");
      if (!pm.feature.face || JSON.stringify(pm.feature.face) !== JSON.stringify(sig)) {
        pm.feature.face = sig;
        pm.feature.positions = [];
      }
      this.changed();
      return;
    }
    if (pm.kind === "sweep" || pm.kind === "loft") {
      const fe = item.kind === "feature" ? this.featureById(item.id) : null;
      if (fe?.type !== "sketch") return this.flash("Select a sketch in the graphics area or the FeatureManager tree.", "info");
      if (pm.kind === "loft") {
        const list = pm.feature.profiles;
        pm.feature.profiles = list.includes(fe.id) ? list.filter((x) => x !== fe.id) : [...list, fe.id];
      } else if (field === "path") pm.feature.path = fe.id;
      else {
        pm.feature.profile = fe.id;
        this.pmField = "path";
      }
      this.schedulePreview();
      this.changed();
      return;
    }
    if (pm.kind === "coordsys") {
      const ref = this.refOf(item);
      if (!ref) return;
      const f = pm.feature;
      if (field === "xAxis") (f.xAxis = ref), (this.pmField = "yAxis");
      else if (field === "yAxis") f.yAxis = ref;
      else (f.origin = ref), (this.pmField = "xAxis");
      this.schedulePreview();
      this.changed();
      return;
    }
    if (pm.kind === "shell") {
      const mesh = this.shownMesh();
      if (item.kind !== "face" || !mesh) return;
      const sig = mesh.faces[item.id].sig;
      const i = pm.feature.faces.findIndex((x) => JSON.stringify(x) === JSON.stringify(sig));
      if (i >= 0) pm.feature.faces.splice(i, 1);
      else pm.feature.faces.push(sig);
      mark();
    } else if (field === "skip" && (pm.kind === "linearPattern" || pm.kind === "circularPattern")) {
      // Instances to Skip takes clicks on the instance dots only (handled by the viewport).
      return;
    } else if (field === "seeds" && (pm.kind === "linearPattern" || pm.kind === "circularPattern" || pm.kind === "mirror")) {
      const id = this.seedOf(item);
      if (!id || id === pm.feature.id) return this.flash("Select features that add or remove material (extrudes, revolves, sweeps, lofts, holes, ribs, patterns, mirrors), or fillets and chamfers with their parent features, in the graphics area or the tree.", "info");
      pm.feature.seeds = toggle(pm.feature.seeds, id);
    } else {
      const ref = this.refOf(item);
      if (!ref) return;
      if (pm.kind === "linearPattern") {
        const d = field === "dir2" ? pm.feature.dir2 : pm.feature.dir1;
        if (!d) return;
        if (ref.kind === "face" && ref.sig.surface !== "PLANE") return this.flash("Select a linear edge, a planar face or a plane for the direction.", "info");
        d.ref = ref;
      } else if (pm.kind === "circularPattern") pm.feature.axis = ref;
      else if (pm.kind === "mirror") {
        if (ref.kind === "edge" || (ref.kind === "face" && ref.sig.surface !== "PLANE")) return this.flash("Select a plane or a planar face to mirror about.", "info");
        pm.feature.plane = ref;
      }
      // After the reference, picks go to the features box.
      this.pmField = "seeds";
      mark();
    }
    this.schedulePreview();
    this.changed();
  }

  /** Features pre-selected for a pattern/mirror. */
  private preselectedSeeds(): Id[] {
    return [...new Set(this.selection.map((s) => this.seedOf(s)).filter((x): x is Id => !!x))];
  }

  startLinearPattern() {
    if (this.sketch) return;
    const edge = this.selection.map((s) => this.refOf(s)).find((r) => r?.kind === "edge" && r.sig.curve === "LINE") ?? null;
    const feature: LinearPatternFeature = {
      id: uid("f"),
      type: "linearPattern",
      name: this.nextName("LPattern"),
      seeds: this.preselectedSeeds(),
      dir1: { ref: edge, reverse: false, spacing: 10, count: 2 },
    };
    this.pmField = feature.dir1.ref ? "seeds" : "dir1";
    this.selection = [];
    this.openPM({ kind: "linearPattern", feature, editing: false });
  }

  startCircularPattern() {
    if (this.sketch) return;
    // OCCT/replicad name the cylinder surface type "CYLINDRE".
    const axis = this.selection.map((s) => this.refOf(s)).find((r) => r && (r.kind === "edge" || (r.kind === "face" && r.sig.surface === "CYLINDRE"))) ?? null;
    const feature: CircularPatternFeature = {
      id: uid("f"),
      type: "circularPattern",
      name: this.nextName("CirPattern"),
      seeds: this.preselectedSeeds(),
      axis,
      reverse: false,
      angle: 2 * Math.PI,
      count: 4,
      equalSpacing: true,
    };
    this.pmField = axis ? "seeds" : "axis";
    this.selection = [];
    this.openPM({ kind: "circularPattern", feature, editing: false });
  }

  startMirror() {
    if (this.sketch) return;
    const plane = this.selection.map((s) => this.refOf(s)).find((r) => r && (r.kind === "plane" || (r.kind === "face" && r.sig.surface === "PLANE"))) ?? null;
    const feature: MirrorFeature = { id: uid("f"), type: "mirror", name: this.nextName("Mirror"), seeds: this.preselectedSeeds(), plane };
    this.pmField = plane ? "seeds" : "plane";
    this.selection = [];
    this.openPM({ kind: "mirror", feature, editing: false });
  }

  startShell() {
    if (this.sketch) return;
    const mesh = this.result?.mesh;
    const faces = mesh ? this.selection.filter((s) => s.kind === "face").map((s) => mesh.faces[s.id as number].sig) : [];
    const feature: ShellFeature = { id: uid("f"), type: "shell", name: this.nextName("Shell"), thickness: 1, faces, outward: false };
    this.pmField = "faces";
    this.openPM({ kind: "shell", feature, editing: false });
  }

  /** PropertyManagers that collect selections (fillet edges, revolve axis...). */
  private onSelectionChanged() {
    const pm = this.pm;
    if (!pm || !("feature" in pm)) return;
    const mesh = this.result?.mesh;
    if (pm.kind === "fillet" && mesh) {
      pm.feature.edges = this.selection.filter((s) => s.kind === "edge").map((s) => mesh.edges[s.id as number].sig);
      pm.feature.faces = this.selection.filter((s) => s.kind === "face").map((s) => mesh.faces[s.id as number].sig);
      this.schedulePreview();
    } else if (pm.kind === "chamfer" && mesh) {
      pm.feature.edges = this.selection.filter((s) => s.kind === "edge").map((s) => mesh.edges[s.id as number].sig);
      this.schedulePreview();
    } else if ((pm.kind === "extrude" || pm.kind === "revolve") && !pm.editing) {
      const sk = this.selection.find((s) => s.kind === "feature" && this.featureById(s.id)?.type === "sketch");
      if (sk && sk.kind === "feature" && sk.id !== pm.feature.sketchId) {
        pm.feature.sketchId = sk.id;
        if (pm.kind === "revolve") pm.feature.axis = this.findAxis(sk.id) ?? "";
        this.schedulePreview();
      }
    }
  }

  // ------------------------------------------------------------ sketches
  /** Insert > Sketch. Uses the selected plane or planar face. */
  startSketch(): boolean {
    if (this.sketch) return false;
    const plane = this.planeRefFromSelection();
    if (!plane) {
      this.flash("Select a plane on which to create a sketch for the entity.", "info");
      this.pendingSketch = true;
      this.changed();
      return false;
    }
    this.pendingSketch = false;
    const wasDirty = this.dirty;
    this.checkpoint("Sketch");
    const f: SketchFeature = { id: uid("s"), type: "sketch", name: this.nextName("Sketch"), plane: plane.ref, sketch: emptySketch() };
    // Insert at rollback position.
    const at = this.doc.rollback ?? this.doc.features.length;
    this.doc.features.splice(at, 0, f);
    if (this.doc.rollback !== null) this.doc.rollback++;
    this.enterSketch(f, plane.frame, true, wasDirty);
    return true;
  }
  /** "Select a plane" state after clicking Sketch with nothing selected. */
  pendingSketch = false;

  planeRefFromSelection(): { ref: PlaneRef; frame: Frame } | null {
    const s = this.selection[0];
    if (!s) return null;
    if (s.kind === "plane") {
      if (s.id === "front" || s.id === "top" || s.id === "right") return { ref: { kind: "std", plane: s.id }, frame: STD_FRAMES[s.id] };
      const fr = this.frameOfPlane(s.id);
      if (fr) return { ref: { kind: "refplane", featureId: s.id }, frame: fr };
    }
    if (s.kind === "face") {
      const info = this.result?.mesh?.faces[s.id];
      if (info?.planar) {
        const fr = frameFromFaceSig(info.sig);
        return { ref: { kind: "face", sig: info.sig }, frame: fr };
      }
    }
    return null;
  }

  editSketch(featureId: Id) {
    const f = this.featureById(featureId);
    if (!f || f.type !== "sketch" || this.sketch) return;
    const frame = this.result?.frames[f.id];
    if (!frame) {
      this.flash("The sketch plane could not be resolved.", "error");
      return;
    }
    const wasDirty = this.dirty;
    this.checkpoint("Edit Sketch");
    this.enterSketch(f, frame, false, wasDirty);
  }

  private enterSketch(f: SketchFeature, frame: Frame, isNew: boolean, wasDirty: boolean) {
    this.shownDims = [];
    const data = deepClone(f.sketch);
    const analysis = solveSketch(data);
    const text = JSON.stringify(data);
    this.sketch = { featureId: f.id, data, frame, isNew, analysis, history: new TextHistory(), original: text, wasDirty };
    // Like SOLIDWORKS, the sketch plane stays selected (and so displayed) on entry.
    const planeId = f.plane.kind === "std" ? f.plane.plane : f.plane.kind === "refplane" ? f.plane.featureId : null;
    this.selection = planeId && !this.pendingTool ? [{ kind: "plane", id: planeId }] : [];
    this.tool = this.pendingTool;
    this.pendingTool = null;
    this.pm = this.tool ? { kind: "sketchTool", tool: this.tool } : null;
    // System Options > Sketch: auto-rotate normal to the sketch plane on creation and edit.
    // The first sketch of a part always rotates.
    const first = isNew && this.doc.features.filter((x) => x.type === "sketch").length <= 1;
    if (this.options.sketch.autoRotateNormal || first) this.viewHooks.normalTo?.(frame, true);
    this.rebuild();
  }

  /**
   * Exit sketch keeping changes (confirmation corner ✓). A new sketch left empty is not
   * created, as in SOLIDWORKS. Returns whether the sketch feature still exists.
   */
  exitSketch(keep = true): boolean {
    const s = this.sketch;
    if (!s) return false;
    const f = this.featureById(s.featureId) as SketchFeature | undefined;
    pruneVirtualPoints(s.data);
    pruneUnusedRefs(s.data);
    if (keep && s.isNew && isEmptySketch(s.data)) keep = false;
    // Opening and closing an existing sketch without changes, or abandoning a new one,
    // leaves nothing to undo.
    if (!keep || (!s.isNew && JSON.stringify(s.data) === s.original)) {
      this.history.dropLast();
      this.dirty = s.wasDirty;
    }
    if (keep && f) f.sketch = s.data;
    if (!keep && s.isNew && f) {
      const i = this.doc.features.indexOf(f);
      this.doc.features.splice(i, 1);
      if (this.doc.rollback !== null && i < this.doc.rollback) this.doc.rollback--;
    }
    this.sketch = null;
    this.tool = null;
    this.modify = null;
    this.pm = null;
    this.selection = keep && f ? [{ kind: "feature", id: f.id }] : [];
    this.rebuild();
    return keep && !!f;
  }

  /** Snapshot sketch state for sketch-level undo. Call before mutating. */
  sketchCheckpoint() {
    const s = this.sketch;
    if (!s) return;
    s.history.push(JSON.stringify(s.data));
  }
  sketchUndo() {
    const s = this.sketch;
    if (!s) return;
    const prev = s.history.undo(JSON.stringify(s.data));
    if (prev === null) return;
    this.restoreSketch(JSON.parse(prev));
  }
  sketchRedo() {
    const s = this.sketch;
    if (!s) return;
    const next = s.history.redo(JSON.stringify(s.data));
    if (next === null) return;
    this.restoreSketch(JSON.parse(next));
  }
  private restoreSketch(data: SketchData) {
    const s = this.sketch!;
    s.data = data;
    this.editor?.resetTool();
    this.selection = this.selection.filter((x) => (x.kind === "skEntity" ? !!data.entities[x.id] : x.kind === "skPoint" ? !!data.points[x.id] : x.kind === "skDim" ? !!data.dimensions[x.id] : true));
    this.resolveSketch();
  }

  resolveSketch() {
    const s = this.sketch;
    if (!s) return;
    s.analysis = solveSketch(s.data);
    this.changed();
  }

  // ------------------------------------------------------------ features
  private sketchForCommand(): Id | null {
    if (this.sketch) {
      const id = this.sketch.featureId;
      const frame = this.sketch.frame;
      const kept = this.exitSketch(true);
      // Leaving a sketch for a feature swings the view off the sketch plane (as SOLIDWORKS does).
      this.viewHooks.leaveNormal?.(frame);
      if (kept) return id;
    }
    const sel = this.selection.find((s) => s.kind === "feature" && this.featureById(s.id)?.type === "sketch");
    if (sel && sel.kind === "feature") return sel.id;
    // Use the last unused sketch, as SOLIDWORKS does when one is "active".
    const consumed = new Set(this.doc.features.flatMap((f) => sketchesOf(f)));
    const last = [...this.activeFeatures()].reverse().find((f) => f.type === "sketch" && !consumed.has(f.id));
    return last?.id ?? null;
  }

  findAxis(sketchId: Id): Id | null {
    const f = this.featureById(sketchId);
    if (!f || f.type !== "sketch") return null;
    const ents = Object.values(f.sketch.entities);
    const cl = ents.find((e) => e.type === "line" && e.construction);
    return cl?.id ?? null;
  }

  startExtrude(cut: boolean) {
    this.pmField = "sketch";
    const sketchId = this.sketchForCommand() ?? "";
    const feature: ExtrudeFeature = {
      id: uid("f"),
      type: "extrude",
      name: this.nextName(cut ? "Cut-Extrude" : "Boss-Extrude"),
      cut,
      sketchId,
      reverse: false,
      end1: { type: cut ? "throughAll" : "blind", depth: 10 },
      merge: true,
    };
    if (!cut) feature.end1.type = "blind";
    this.openPM({ kind: "extrude", feature, editing: false });
  }

  startRevolve(cut: boolean) {
    this.pmField = "sketch";
    const sketchId = this.sketchForCommand() ?? "";
    const feature: RevolveFeature = {
      id: uid("f"),
      type: "revolve",
      name: this.nextName(cut ? "Cut-Revolve" : "Revolve"),
      cut,
      sketchId,
      axis: sketchId ? (this.findAxis(sketchId) ?? "") : "",
      angle: 2 * Math.PI,
      revType: "oneDirection",
      reverse: false,
      merge: true,
    };
    this.openPM({ kind: "revolve", feature, editing: false });
  }

  startFillet() {
    if (this.sketch) return;
    const mesh = this.result?.mesh;
    const feature: FilletFeature = {
      id: uid("f"),
      type: "fillet",
      name: this.nextName("Fillet"),
      radius: 10,
      edges: mesh ? this.selection.filter((s) => s.kind === "edge").map((s) => mesh.edges[s.id as number].sig) : [],
      faces: mesh ? this.selection.filter((s) => s.kind === "face").map((s) => mesh.faces[s.id as number].sig) : [],
    };
    this.openPM({ kind: "fillet", feature, editing: false });
  }

  startChamfer() {
    if (this.sketch) return;
    const mesh = this.result?.mesh;
    const feature: ChamferFeature = {
      id: uid("f"),
      type: "chamfer",
      name: this.nextName("Chamfer"),
      chamferType: "angleDistance",
      angle: Math.PI / 4,
      distance: 10,
      edges: mesh ? this.selection.filter((s) => s.kind === "edge").map((s) => mesh.edges[s.id as number].sig) : [],
    };
    this.openPM({ kind: "chamfer", feature, editing: false });
  }

  /** References picked before starting a reference geometry command. */
  private preselectedRefs(): GeomRef[] {
    return this.selection.map((s) => this.refOf(s)).filter((r): r is GeomRef => !!r).slice(0, 3);
  }

  startRefPlane() {
    if (this.sketch) return;
    const refs = this.preselectedRefs();
    const feature: RefPlaneFeature = {
      id: uid("f"),
      type: "refplane",
      name: this.nextName("Plane"),
      base: { kind: "std", plane: "front" },
      offset: 10,
      flip: false,
      refs,
      angle: Math.PI / 4,
    };
    this.pmField = `ref${Math.min(refs.length, 2)}`;
    this.selection = [];
    this.openPM({ kind: "refplane", feature, editing: false });
  }

  startRefAxis() {
    if (this.sketch) return;
    const refs = this.preselectedRefs();
    const feature: RefAxisFeature = { id: uid("f"), type: "refaxis", name: this.nextName("Axis"), refs };
    this.pmField = `ref${Math.min(refs.length, 2)}`;
    this.selection = [];
    this.openPM({ kind: "refaxis", feature, editing: false });
  }

  startRefPoint() {
    if (this.sketch) return;
    const refs = this.preselectedRefs().slice(0, 1);
    const feature: RefPointFeature = { id: uid("f"), type: "refpoint", name: this.nextName("Point"), refs, xyz: [0, 0, 0] };
    this.pmField = "ref0";
    this.selection = [];
    this.openPM({ kind: "refpoint", feature, editing: false });
  }

  /** Sketches selected before starting Sweep/Loft, in selection order. */
  private preselectedSketches(): Id[] {
    return this.selection.filter((s) => s.kind === "feature" && this.featureById(s.id)?.type === "sketch").map((s) => (s as { id: Id }).id);
  }

  startSweep(cut: boolean) {
    if (this.sketch) this.exitSketch(true);
    const sks = this.preselectedSketches();
    const feature: SweepFeature = { id: uid("f"), type: "sweep", name: this.nextName(cut ? "Cut-Sweep" : "Sweep"), cut, profileType: "sketch", profile: sks[0] ?? "", diameter: 10, path: sks[1] ?? "", orientation: "followPath", merge: true };
    this.pmField = !feature.profile ? "profile" : "path";
    this.selection = [];
    this.openPM({ kind: "sweep", feature, editing: false });
  }

  startLoft(cut: boolean) {
    if (this.sketch) this.exitSketch(true);
    const feature: LoftFeature = { id: uid("f"), type: "loft", name: this.nextName(cut ? "Cut-Loft" : "Loft"), cut, profiles: this.preselectedSketches(), merge: true };
    this.pmField = "profiles";
    this.selection = [];
    this.openPM({ kind: "loft", feature, editing: false });
  }

  startHole() {
    if (this.sketch) return;
    const mesh = this.result?.mesh;
    const face = this.selection.map((s) => (s.kind === "face" ? mesh?.faces[s.id] : null)).find((f) => f?.planar)?.sig;
    const feature: HoleFeature = {
      id: uid("f"),
      type: "hole",
      name: "",
      holeType: "cbore",
      standard: "ISO",
      size: "M6",
      fit: "normal",
      end: { type: "throughAll", depth: 20 },
      face,
      positions: [],
    };
    feature.name = this.holeName(feature);
    this.pmField = "positions";
    this.selection = [];
    if (!face) this.flash("Select a planar face, then click on it to place the holes.", "info");
    this.openPM({ kind: "hole", feature, editing: false });
  }

  /** SOLIDWORKS-style hole feature name, e.g. "CBORE for M6 Socket Head Cap Screw1". */
  holeName(f: HoleFeature) {
    const base = f.holeType === "cbore" ? `CBORE for ${f.size} Socket Head Cap Screw` : f.holeType === "csk" ? `CSK for ${f.size} Countersunk Head Screw` : f.holeType === "tap" ? `${f.size}x1.0 Tapped Hole` : `${f.size} Hole`;
    return this.nextName(base);
  }

  /** Hole Wizard: add a position (face sketch coordinates). */
  addHolePosition(p: [number, number]) {
    const pm = this.pm;
    if (pm?.kind !== "hole") return;
    pm.feature.positions = [...(pm.feature.positions ?? []), p];
    this.schedulePreview();
    this.changed();
  }

  startRib() {
    const sketchId = this.sketchForCommand() ?? "";
    const feature: RibFeature = { id: uid("f"), type: "rib", name: this.nextName("Rib"), sketchId, thickness: 5, side: "both", direction: "parallel", flip: false };
    this.selection = [];
    this.openPM({ kind: "rib", feature, editing: false });
  }

  startDraft() {
    if (this.sketch) return;
    const feature: DraftFeature = { id: uid("f"), type: "draft", name: this.nextName("Draft"), angle: Math.PI / 180, neutral: null, reverse: false, faces: [] };
    this.pmField = "neutral";
    this.selection = [];
    this.openPM({ kind: "draft", feature, editing: false });
  }

  startCoordSys() {
    if (this.sketch) return;
    const feature: CoordSysFeature = { id: uid("f"), type: "coordsys", name: this.nextName("Coordinate System"), origin: null, xAxis: null, yAxis: null };
    this.pmField = "origin";
    this.selection = [];
    this.openPM({ kind: "coordsys", feature, editing: false });
  }

  openPM(pm: PMState) {
    this.shownDims = [];
    this.pm = pm;
    this.previewResult = null;
    if ("feature" in pm) this.schedulePreview();
    this.changed();
  }

  editFeature(id: Id) {
    const f = this.featureById(id);
    if (!f) return;
    if (f.type === "sketch") return this.editSketch(id);
    const feature = deepClone(f);
    this.selection = [];
    switch (feature.type) {
      case "extrude": return (this.pmField = "sketch"), this.openPM({ kind: "extrude", feature, editing: true });
      case "revolve": return (this.pmField = "sketch"), this.openPM({ kind: "revolve", feature, editing: true });
      case "fillet": return this.openPM({ kind: "fillet", feature, editing: true });
      case "chamfer": return this.openPM({ kind: "chamfer", feature, editing: true });
      case "refplane": return (this.pmField = "ref0"), this.openPM({ kind: "refplane", feature, editing: true });
      case "refaxis": return (this.pmField = "ref0"), this.openPM({ kind: "refaxis", feature, editing: true });
      case "refpoint": return (this.pmField = "ref0"), this.openPM({ kind: "refpoint", feature, editing: true });
      case "coordsys": return (this.pmField = "origin"), this.openPM({ kind: "coordsys", feature, editing: true });
      case "sweep": return (this.pmField = "profile"), this.openPM({ kind: "sweep", feature, editing: true });
      case "hole": return (this.pmField = "positions"), this.openPM({ kind: "hole", feature, editing: true });
      case "draft": return (this.pmField = "faces"), this.openPM({ kind: "draft", feature, editing: true });
      case "rib": return this.openPM({ kind: "rib", feature, editing: true });
      case "loft": return (this.pmField = "profiles"), this.openPM({ kind: "loft", feature, editing: true });
      case "linearPattern": return (this.pmField = "seeds"), this.openPM({ kind: "linearPattern", feature, editing: true });
      case "circularPattern": return (this.pmField = "seeds"), this.openPM({ kind: "circularPattern", feature, editing: true });
      case "mirror": return (this.pmField = "seeds"), this.openPM({ kind: "mirror", feature, editing: true });
      case "shell": return (this.pmField = "faces"), this.openPM({ kind: "shell", feature, editing: true });
    }
  }

  /**
   * Does the open PropertyManager have OK/Cancel (features, Add Relations, Section View,
   * sketch operation tools)? Only those accept on Enter, as in SOLIDWORKS. Sketch drawing
   * tools and entity properties have just a close button, and Enter there only applies
   * the typed value.
   */
  pmHasOk() {
    const pm = this.pm;
    return !!pm && ("feature" in pm || pm.kind === "addRelations" || pm.kind === "section" || (pm.kind === "sketchTool" && OP_TOOLS.has(pm.tool)));
  }

  /** PropertyManager OK (green check). */
  acceptPM() {
    const pm = this.pm;
    if (!pm) return;
    if (pm.kind === "section") {
      this.pm = null;
      this.changed();
      return;
    }
    if (pm.kind === "sketchTool" && this.editor && this.editor.opPreview()) {
      this.editor.applyOp();
      return;
    }
    if ("feature" in pm) {
      const st = this.previewResult?.statuses.find((s) => s.id === pm.feature.id);
      if (st?.error) {
        this.flash(st.error, "error");
        return;
      }
      if ((pm.kind === "extrude" || pm.kind === "revolve") && !pm.feature.sketchId) {
        this.flash("Select a sketch to extrude.", "error");
        return;
      }
      this.checkpoint(pm.feature.name);
      if (pm.kind === "hole" && !pm.editing) {
        // Hole positions become a sketch on the face (absorbed under the hole), so they can be dimensioned.
        const f = pm.feature;
        if (!f.face || !f.positions?.length) return this.flash("Click on a face to place at least one hole.", "error");
        const sk = emptySketch();
        f.positions.forEach((p, i) => {
          const pid = `hp${i}`, eid = `he${i}`;
          sk.points[pid] = { id: pid, x: p[0], y: p[1], standalone: true };
          sk.entities[eid] = { id: eid, type: "point", p: pid };
        });
        const sketch: SketchFeature = { id: uid("s"), type: "sketch", name: this.nextName("Sketch"), plane: { kind: "face", sig: f.face }, sketch: sk };
        const at = this.doc.rollback ?? this.doc.features.length;
        this.doc.features.splice(at, 0, sketch);
        if (this.doc.rollback !== null) this.doc.rollback++;
        f.sketchId = sketch.id;
        f.positions = undefined;
      }
      if (pm.editing) {
        const i = this.doc.features.findIndex((f) => f.id === pm.feature.id);
        if (i >= 0) this.doc.features[i] = pm.feature;
      } else {
        const at = this.doc.rollback ?? this.doc.features.length;
        this.doc.features.splice(at, 0, pm.feature);
        if (this.doc.rollback !== null) this.doc.rollback++;
      }
      this.selection = [];
      this.pm = null;
      this.previewResult = null;
      // The first solid of a part is zoomed to fit, as in SOLIDWORKS.
      const hadBody = !!this.result?.mesh;
      this.rebuild().then(() => !hadBody && this.result?.mesh && this.viewHooks.fit?.());
      return;
    }
    this.pm = null;
    this.tool = null;
    this.changed();
  }

  cancelPM() {
    if (this.pm?.kind === "section") this.section = null;
    this.pm = null;
    this.previewResult = null;
    if (this.sketch) this.tool = null;
    this.message = null;
    this.changed();
  }

  /** Is a plane shown (standard planes default hidden, reference planes default shown)? */
  isPlaneVisible(id: string) {
    const d = this.doc.display ?? {};
    return ["front", "top", "right"].includes(id) ? !!d.shownPlanes?.includes(id) : !d.hiddenPlanes?.includes(id);
  }

  /** Show/Hide a plane (context toolbar eye). */
  setPlaneVisible(id: string, show: boolean) {
    this.checkpoint(show ? "Show" : "Hide");
    const d = (this.doc.display ??= {});
    if (["front", "top", "right"].includes(id)) {
      const s = new Set(d.shownPlanes ?? []);
      show ? s.add(id) : s.delete(id);
      d.shownPlanes = [...s];
    } else {
      const s = new Set(d.hiddenPlanes ?? []);
      show ? s.delete(id) : s.add(id);
      d.hiddenPlanes = [...s];
    }
    this.dirty = true;
    this.changed();
  }

  /** Features that depend on `id` (children): consumers of a sketch, sketches on a plane. */
  dependentsOf(id: Id): Feature[] {
    const dead = new Set([id]);
    let grew = true;
    while (grew) {
      grew = false;
      for (const x of this.doc.features) {
        if (dead.has(x.id)) continue;
        // Any reference to a removed feature's id (sketch, plane, axis, point, pattern seed...).
        const { id: _own, ...body } = x;
        void _own;
        const text = JSON.stringify(body);
        const dep = [...dead].some((d) => text.includes(`"${d}"`));
        if (dep) {
          dead.add(x.id);
          grew = true;
        }
      }
    }
    dead.delete(id);
    return this.doc.features.filter((x) => dead.has(x.id));
  }

  /** Sketches absorbed by a feature (shown under it in the tree), unless shared with others. */
  absorbedSketches(id: Id): Feature[] {
    const f = this.featureById(id);
    if (!f) return [];
    return sketchesOf(f)
      .filter((s) => !this.doc.features.some((x) => x.id !== id && sketchesOf(x).includes(s)))
      .map((s) => this.featureById(s))
      .filter((x): x is Feature => !!x);
  }

  /** First absorbed sketch (Delete dialog "Delete absorbed features"). */
  absorbedOf(id: Id): Feature | null {
    return this.absorbedSketches(id)[0] ?? null;
  }

  /** Delete a feature and its children; optionally also its absorbed sketch. */
  deleteFeature(id: Id, opts: { absorbed?: boolean } = {}) {
    const f = this.featureById(id);
    if (!f) return;
    this.checkpoint(`Delete ${f.name}`);
    const dead = new Set([id, ...this.dependentsOf(id).map((x) => x.id)]);
    if (opts.absorbed) for (const a of this.absorbedSketches(id)) dead.add(a.id);
    this.doc.features = this.doc.features.filter((x) => !dead.has(x.id));
    if (this.doc.rollback !== null && this.doc.rollback >= this.doc.features.length) this.doc.rollback = null;
    this.selection = [];
    this.rebuild();
  }

  /** Feature the selection refers to: a tree feature, or the feature that made the selected face. */
  selectedFeatureId(): Id | null {
    for (const s of this.selection) {
      if (s.kind === "feature") return s.id;
      if (s.kind === "plane" && this.featureById(s.id)) return s.id;
      if (s.kind === "face") {
        const owner = this.result?.mesh?.faces[s.id]?.owner;
        if (owner && this.featureById(owner)) return owner;
      }
    }
    return null;
  }

  selectedSketchId(): Id | null {
    const id = this.selectedFeatureId();
    const f = id ? this.featureById(id) : null;
    if (!f) return null;
    if (f.type === "sketch") return f.id;
    return sketchesOf(f)[0] ?? null;
  }

  isSketchVisible(id: Id) {
    return !this.doc.display?.hiddenSketches?.includes(id);
  }

  setSketchVisible(id: Id, show: boolean) {
    this.checkpoint(show ? "Show" : "Hide");
    const d = (this.doc.display ??= {});
    const s = new Set(d.hiddenSketches ?? []);
    show ? s.delete(id) : s.add(id);
    d.hiddenSketches = [...s];
    this.dirty = true;
    this.changed();
  }

  /**
   * Section View (view-only, not a feature): the plane, offset and side kept. `on` keeps
   * the section displayed after the PropertyManager closes.
   */
  section: { on: boolean; ref: GeomRef; offset: number; flip: boolean } | null = null;

  /** View > Display > Section View: open its PropertyManager, or turn an active section off. */
  toggleSection() {
    if (this.section?.on && this.pm?.kind !== "section") {
      this.section = null;
      this.changed();
      return;
    }
    if (!this.section) this.section = { on: true, ref: { kind: "plane", plane: { kind: "std", plane: "front" } }, offset: 0, flip: false };
    this.pmField = "sectionRef";
    this.pm = { kind: "section" };
    this.changed();
  }

  /** Plane (point + normal) of the section, world coordinates. */
  sectionPlane(): { o: [number, number, number]; n: [number, number, number] } | null {
    const s = this.section;
    if (!s) return null;
    let o: [number, number, number] = [0, 0, 0], n: [number, number, number] = [0, 0, 1];
    if (s.ref.kind === "plane") {
      const fr = s.ref.plane.kind === "std" ? STD_FRAMES[s.ref.plane.plane] : s.ref.plane.kind === "refplane" ? this.result?.planes[s.ref.plane.featureId]?.frame : null;
      if (!fr) return null;
      o = fr.origin;
      n = fr.normal;
    } else if (s.ref.kind === "face") {
      o = s.ref.sig.center;
      n = s.ref.sig.normal;
    }
    const k = s.offset;
    return { o: [o[0] + n[0] * k, o[1] + n[1] * k, o[2] + n[2] * k], n: s.flip ? [-n[0], -n[1], -n[2]] : n };
  }

  /**
   * Convert Entities: project the selected model edges/faces onto the open sketch and add
   * them as sketch entities with On Edge relations (shared endpoints are merged).
   */
  async applyConvert() {
    const s = this.sketch;
    const mesh = this.result?.mesh;
    if (!s || !mesh) return;
    const edges = this.selection.filter((x) => x.kind === "edge").map((x) => mesh.edges[x.id as number]?.sig).filter(Boolean);
    const faces = this.selection.filter((x) => x.kind === "face").map((x) => mesh.faces[x.id as number]?.sig).filter(Boolean);
    const curves = await this.kernel.convert(this.activeFeatures(), s.frame, edges, faces);
    if (!this.sketch || !curves.length) return this.flash("Nothing to convert: the edges are perpendicular to the sketch plane.", "info");
    this.sketchCheckpoint();
    const sk = this.sketch.data;
    const made: Id[] = [];
    const pointAt = (p: [number, number]): Id => {
      for (const q of Object.values(sk.points)) if (Math.hypot(q.x - p[0], q.y - p[1]) < 1e-6) return q.id;
      const id = uid("p");
      sk.points[id] = { id, x: p[0], y: p[1] };
      return id;
    };
    const line = (a: [number, number], b: [number, number]) => {
      const id = uid("e");
      sk.entities[id] = { id, type: "line", p1: pointAt(a), p2: pointAt(b) };
      made.push(id);
    };
    for (const c of curves) {
      if (c.kind === "line") line(c.a, c.b);
      else if (c.kind === "poly") {
        // Curves that are not lines or arcs become splines through a few of their points.
        const step = Math.max(1, Math.floor((c.pts.length - 1) / 8));
        const keep = c.pts.filter((_, i) => i % step === 0 || i === c.pts.length - 1);
        const id = uid("e");
        sk.entities[id] = { id, type: "spline", pts: keep.map((q) => pointAt(q)) };
        made.push(id);
      }
      else if (c.kind === "circle") {
        const id = uid("e");
        sk.entities[id] = { id, type: "circle", c: pointAt(c.c), r: c.r };
        made.push(id);
      } else {
        const cc = circumcenter(c.a, c.m, c.b);
        if (!cc) {
          line(c.a, c.b);
          continue;
        }
        // Stored counter-clockwise: swap the ends of a clockwise arc.
        const ccw = (c.m[0] - c.a[0]) * (c.b[1] - c.a[1]) - (c.m[1] - c.a[1]) * (c.b[0] - c.a[0]) < 0;
        const id = uid("e");
        sk.entities[id] = { id, type: "arc", c: pointAt(cc), p1: pointAt(ccw ? c.a : c.b), p2: pointAt(ccw ? c.b : c.a) };
        made.push(id);
      }
    }
    for (const id of made) {
      const rid = uid("r");
      sk.relations[rid] = { id: rid, type: "onEdge", target: id };
    }
    this.selection = [];
    this.resolveSketch();
  }

  /** Results of the last equation evaluation (Equations dialog "Evaluates to"). */
  equationResults: { values: Record<string, number>; errors: Record<string, string> } = { values: {}, errors: {} };

  /** Evaluate global variables and equations; re-solve sketches whose dimensions changed. */
  runEquations() {
    if (!this.doc.globals?.length && !this.doc.equations?.length) return;
    // While a sketch is open, its live data stands in for the stored copy.
    const open = this.sketch ? (this.featureById(this.sketch.featureId) as SketchFeature | undefined) : undefined;
    const stored = open?.sketch;
    if (open && this.sketch) open.sketch = this.sketch.data;
    let r: ReturnType<typeof applyEquations>;
    try {
      r = applyEquations(this.doc, UNIT_INFO[this.doc.units].perMm);
    } finally {
      if (open && stored) open.sketch = stored;
    }
    this.equationResults = { values: r.values, errors: r.errors };
    for (const f of r.changed) {
      if (f.type !== "sketch") continue;
      const data = this.sketch?.featureId === f.id ? this.sketch.data : f.sketch;
      const sys = newSketchSystem(data);
      if (sys.solve(200)) sys.commit();
      if (this.sketch?.featureId === f.id) this.sketch.analysis = solveSketch(data);
    }
    const errs = Object.entries(r.errors);
    if (errs.length) this.flash(`Equation error (${errs[0][0]}): ${errs[0][1]}`, "error");
  }

  /** Make a dimension driven by an equation (Modify box text starting with "="). */
  setEquation(target: string, expr: string) {
    const list = (this.doc.equations ??= []).filter((e) => e.target !== target);
    list.push({ target, expr: expr.replace(/^\s*=\s*/, "") });
    this.doc.equations = list;
    this.dirty = true;
  }

  /** Double-click a feature: show its dimensions (sketch and feature dimensions). */
  showFeatureDims(id: Id | null) {
    this.shownDims = id ? [id] : [];
    this.changed();
  }

  /** Mark a feature (and what depends on it) as needing a rebuild. */
  markDirty(id: Id) {
    this.needsRebuild = true;
    this.dirtyFeatures.add(id);
    for (const d of this.dependentsOf(id)) this.dirtyFeatures.add(d.id);
    this.dirty = true;
  }

  /** Open the Modify box for a shown sketch or feature dimension. */
  openModifyTarget(target: ModifyTarget, x: number, y: number) {
    this.modify = { dimId: target.kind === "sketchDim" ? target.dimId : target.key, x, y, isNew: false, target };
    this.changed();
  }

  /**
   * Value access for the Modify box: the open sketch's dimension, a dimension of a
   * closed sketch, or a feature parameter. Part-level changes wait for Rebuild.
   */
  modifyAccess(): { name: string; value: number; angle: boolean; set: (v: number) => boolean; record: () => void; partLevel: boolean; full?: string } | null {
    const m = this.modify;
    if (!m) return null;
    const t = m.target;
    if (!t) {
      const d = this.sketch?.data.dimensions[m.dimId];
      if (!d || !this.editor) return null;
      const sname = this.featureById(this.sketch!.featureId)?.name ?? "Sketch";
      return { name: d.name, full: `${d.name}@${sname}`, value: d.value, angle: d.kind === "angle", set: (v) => this.editor!.setDimensionValue(m.dimId, v, m.isNew), record: () => this.sketchCheckpoint(), partLevel: false };
    }
    if (t.kind === "sketchDim") {
      const f = this.featureById(t.sketchId);
      if (f?.type !== "sketch") return null;
      const d = f.sketch.dimensions[t.dimId];
      if (!d) return null;
      return {
        name: `${d.name}@${f.name}`,
        full: `${d.name}@${f.name}`,
        value: d.value,
        angle: d.kind === "angle",
        partLevel: true,
        record: () => this.checkpoint("Modify"),
        set: (v) => {
          const prev = d.value;
          d.value = v;
          const sys = newSketchSystem(f.sketch);
          if (!sys.solve(200)) {
            d.value = prev;
            this.flash("The sketch cannot be solved with this value.", "error");
            return false;
          }
          sys.commit();
          this.markDirty(f.id);
          this.changed();
          return true;
        },
      };
    }
    const f = this.featureById(t.featureId);
    const p = f ? featureParams(f).find((x) => x.key === t.key) : null;
    if (!f || !p) return null;
    return {
      name: `${p.label}@${f.name}`,
      full: `${p.label}@${f.name}`,
      value: p.value,
      angle: !!p.angle,
      partLevel: true,
      record: () => this.checkpoint("Modify"),
      set: (v) => {
        setFeatureParam(f, t.key, v);
        this.markDirty(f.id);
        this.changed();
        return true;
      },
    };
  }

  /**
   * Selection filters (F5 toolbar, F6 on/off, E/V/X): when any are set and filtering is
   * on, only those entity types can be picked in the graphics area.
   */
  selFilter = new Set<"face" | "edge" | "vertex" | "plane" | "sketch" | "ref">();
  filterOn = true;
  filterBar = false;
  toggleFilter(k: "face" | "edge" | "vertex" | "plane" | "sketch" | "ref") {
    if (this.selFilter.has(k)) this.selFilter.delete(k);
    else this.selFilter.add(k);
    this.filterOn = true;
    this.changed();
  }
  /** Entities under the cursor offered by Select Other (right-click > Select Other). */
  selectOther: { x: number; y: number; items: SelItem[] } | null = null;

  /** Measure dialog (Evaluate > Measure) is open: selections are measured. */
  measureOpen = false;
  measureResult: { distance: number; p1: [number, number, number]; p2: [number, number, number] } | null = null;
  /** Measure in an assembly (G1c): the selected entities at their occurrences' placements. */
  asmMeasure: AsmMeasureReply | null = null;
  private measureSeq = 0;

  /** Minimum distance between the first two measurable selections (kernel, exact). In an
   * assembly every selected entity is measured at its placement (G1c). */
  runMeasure() {
    if (this.asm) {
      const seq = ++this.measureSeq;
      this.asm
        .measure(this.selection)
        .then((r) => {
          if (seq !== this.measureSeq) return;
          this.asmMeasure = r;
          this.changed();
        })
        .catch((e) => {
          if (seq !== this.measureSeq) return;
          this.asmMeasure = null;
          this.flash(`Measure failed: ${(e as Error).message ?? e}`, "error");
        });
      return;
    }
    const mesh = this.result?.mesh;
    const refs = this.selection
      .map((s) =>
        s.kind === "face" && mesh?.faces[s.id] ? { kind: "face" as const, sig: mesh.faces[s.id].sig } : s.kind === "edge" && mesh?.edges[s.id] ? { kind: "edge" as const, sig: mesh.edges[s.id].sig } : s.kind === "vertex" && mesh?.vertices[s.id] ? { kind: "vertex" as const, p: mesh.vertices[s.id] } : s.kind === "origin" ? { kind: "vertex" as const, p: [0, 0, 0] as [number, number, number] } : null,
      )
      .filter((r): r is NonNullable<typeof r> => !!r);
    if (refs.length < 2) {
      this.measureResult = null;
      return;
    }
    const seq = ++this.measureSeq;
    this.kernel.measure(this.activeFeatures(), refs.slice(0, 2)).then((r) => {
      if (seq !== this.measureSeq) return;
      this.measureResult = r;
      this.changed();
    });
  }

  /** Edit Material: set the part material (undoable). */
  setMaterial(name: string | null) {
    this.checkpoint("Edit Material");
    if (name) this.doc.material = name;
    else delete this.doc.material;
    this.changed();
  }

  /** Tree item being renamed in place, and a counter that collapses all tree items. */
  renaming: Id | null = null;
  treeCollapse = 0;

  toggleSuppress(id: Id) {
    const f = this.featureById(id);
    if (!f) return;
    this.checkpoint(f.suppressed ? "Unsuppress" : "Suppress");
    f.suppressed = !f.suppressed;
    this.rebuild();
  }

  renameFeature(id: Id, name: string) {
    const f = this.featureById(id);
    if (!f || !name.trim()) return;
    this.checkpoint("Rename");
    f.name = name.trim();
    this.changed();
  }

  setRollback(index: number | null) {
    this.doc.rollback = index !== null && index >= this.doc.features.length ? null : index;
    this.rebuild();
  }

  // ------------------------------------------------------------ file
  /** Unsaved changes are the caller's to ask about (`UiHooks.whenSaved`). */
  newDocument() {
    this.asm = null;
    this.filePath = null;
    const n = (parseInt(this.doc.name.replace(/\D/g, "")) || 0) + 1;
    this.doc = App.newDocument(n);
    this.sketch = null;
    this.pm = null;
    this.selection = [];
    this.history.clear();
    this.dirty = false;
    this.fileHandle = null;
    this.rebuild();
  }

  /** File > Open of a STEP file: a new part with an Imported1 feature. */
  importStep(text: string, name: string) {
    this.asm = null;
    this.filePath = null;
    const doc = App.newDocument(1);
    doc.name = name.replace(/\.[^.]+$/, "");
    doc.features.push({ id: uid("f"), type: "imported", name: "Imported1", format: "step", data: text });
    this.doc = doc;
    this.sketch = null;
    this.pm = null;
    this.selection = [];
    this.history.clear();
    this.dirty = true;
    this.fileHandle = null;
    this.rebuild().then(() => this.viewHooks.fit?.());
  }

  /** Hide/Show Body, Transparency and Appearance colour of the part (view-only, undoable). */
  setBodyDisplay(patch: { bodyHidden?: boolean; transparent?: boolean; color?: string | undefined }) {
    this.checkpoint("Display");
    this.doc.display = { ...(this.doc.display ?? {}), ...patch };
    if ("color" in patch && !patch.color) delete this.doc.display.color;
    this.changed();
  }

  loadDocument(text: string, name?: string) {
    this.asm = null;
    this.filePath = null;
    this.doc = migrateDocument(JSON.parse(text));
    if (name) this.doc.name = name.replace(/\.[^.]+$/, "");
    this.sketch = null;
    this.pm = null;
    this.selection = [];
    this.history.clear();
    this.dirty = false;
    this.rebuild().then(() => this.viewHooks.fit?.());
  }

  serialize() {
    return JSON.stringify(this.doc, null, 1);
  }

  /** Makes `s` the open document (New Assembly, or an assembly opened). */
  openAssembly(s: AssemblySession) {
    if (this.asm && this.asm !== s) {
      this.asm.matePM = null;
      this.asm.movePM = null;
      this.asm.placing = null;
    }
    this.asm = s;
    this.doc = App.newDocument(1);
    this.result = null;
    this.previewResult = null;
    this.sketch = null;
    this.pm = null;
    this.selection = [];
    this.hover = null;
    this.history.clear();
    this.fileHandle = null;
    this.filePath = null;
    this.dirty = !s.path;
    this.changed();
    void s.build().then(() => this.viewHooks.fit?.());
  }

  /** The open document's name (assembly or part). */
  docName() {
    return this.asm?.name ?? this.doc.name;
  }

  /**
   * Open Part / Open Assembly on a component (G1b): its document becomes the open one and
   * the assembly waits on `asmStack` (SOLIDWORKS opens it in its own window; Return to
   * Assembly comes back). A part edited and not saved in the meantime is read from memory
   * by the assembly (`AssemblySession.overrides`).
   */
  async openComponent(instance: Id): Promise<boolean> {
    const s = this.asm;
    const f = s?.componentFile(instance);
    if (!s || !f) {
      this.flash("The component's file was not found.", "error");
      return false;
    }
    const text = s.overrides[f.path] ?? null;
    try {
      if (f.kind === "assembly") {
        const { openAssembly, AssemblySession } = await import("../assembly/session");
        const sub = text ? new AssemblySession(this, JSON.parse(text), f.path) : await openAssembly(this, f.path);
        sub.doc.name = f.path.split("/").pop()!.replace(/\.[^.]+$/, "");
        sub.overrides = { ...s.overrides };
        delete sub.overrides[f.path];
        this.asmStack.push({ session: s, dirty: this.dirty, file: f.path });
        this.openAssembly(sub);
        this.dirty = !!text;
      } else {
        const body = text ?? (await invoke<string>("app_read_text", { path: f.path }));
        this.asmStack.push({ session: s, dirty: this.dirty, file: f.path });
        this.loadDocument(body, f.path.split("/").pop());
        this.filePath = f.path;
        this.dirty = !!text;
      }
      this.changed();
      return true;
    } catch (e) {
      this.flash(String((e as Error).message ?? e), "error");
      return false;
    }
  }

  /**
   * Back to the assembly the open component was opened from. Unsaved edits go with it (the
   * assembly shows them and saves them with itself); mates follow the edited geometry by
   * persistent name.
   */
  returnToAssembly(): boolean {
    const top = this.asmStack.pop();
    if (!top) return false;
    const parent = top.session;
    // Unsaved edits are read from memory; a saved file is the truth again.
    if (this.dirty) parent.overrides[top.file] = this.asm ? JSON.stringify(this.asm.doc) : this.serialize();
    else delete parent.overrides[top.file];
    // Edits made further down (a part opened from the sub-assembly) come along.
    if (this.asm) Object.assign(parent.overrides, this.asm.overrides);
    const edited = this.dirty;
    this.openAssembly(parent);
    this.dirty = top.dirty || edited;
    this.changed();
    return true;
  }
}

export function frameFromFaceSig(sig: { center: [number, number, number]; normal: [number, number, number] }): Frame {
  // Imported lazily to avoid a cycle.
  return frameForPlaneLocal(sig.center, sig.normal);
}
import { frameForPlane as frameForPlaneLocal } from "../core/math";

function getPath(obj: unknown, key: string): { found: boolean; value?: unknown } {
  let cur: unknown = obj;
  for (const part of key.split(".")) {
    if (cur && typeof cur === "object" && part in (cur as Record<string, unknown>)) cur = (cur as Record<string, unknown>)[part];
    else return { found: false };
  }
  return { found: true, value: cur };
}

function setPath(obj: unknown, key: string, value: unknown): boolean {
  const parts = key.split(".");
  let cur = obj as Record<string, unknown>;
  for (let i = 0; i < parts.length - 1; i++) {
    const next = cur[parts[i]];
    if (!next || typeof next !== "object") return false;
    cur = next as Record<string, unknown>;
  }
  const last = parts[parts.length - 1];
  if (!(last in cur)) return false;
  cur[last] = value;
  return true;
}
