/**
 * Command registry. Every toolbar button, menu item, shortcut and gesture maps to a
 * command id here, so behaviour is defined once.
 */
import type { App } from "../app/app";
import type { Viewport } from "../render/viewport";
import type { SketchTool } from "../sketch/editor";
import type { RelationType } from "../core/types";
import { isNative } from "../native/tauri";

export interface Command {
  id: string;
  label: string;
  icon?: string;
  /** Tooltip text (SOLIDWORKS-style: what it does). */
  tip?: string;
  shortcut?: string;
  run: (ctx: Ctx) => void;
  enabled?: (ctx: Ctx) => boolean;
  checked?: (ctx: Ctx) => boolean;
  /** Not implemented yet: shown greyed out. */
  todo?: boolean;
}

export interface Ctx {
  app: App;
  vp: Viewport | null;
  ui: UiHooks;
}

export interface UiHooks {
  /** Runs `then` at once, or after "Save changes to <part>?" if the part has unsaved changes. */
  whenSaved: (then: () => void) => void;
  newFile: () => void;
  /** File > New > Assembly (native app). */
  newAssembly: () => void;
  openFile: () => void;
  /** Insert Components: pick a part or assembly file and add it (native app). */
  insertComponent: () => void;
  /** Resolves false when the user cancelled the save. */
  saveFile: (as?: boolean) => Promise<boolean>;
  exportFile: (fmt: "step" | "stl") => void;
  showOptions: (page?: string) => void;
  confirmDelete: (featureId: string) => void;
  showOrientation: () => void;
  showMassProps: () => void;
  showMaterial: () => void;
  showEquations: () => void;
  showShortcutBar: () => void;
  toggleTaskPane: () => void;
  toggleTreePane: () => void;
  showAbout: () => void;
}

const inSketch = (c: Ctx) => !!c.app.sketch;
/** Assemblies exist in the native app only (the browser and Electron builds run the frozen
 * TypeScript engine until W1), so their commands are greyed out elsewhere. */
const inAsm = (c: Ctx) => isNative() && !!c.app.asm && !c.app.asm.matePM;
/** Instances and mates selected in the assembly FeatureManager. */
const selAsmItems = (c: Ctx) => (c.app.asm ? c.app.selection.flatMap((s) => (s.kind === "feature" ? [s.id] : [])) : []);
const selInstances = (c: Ctx) => selAsmItems(c).filter((id) => c.app.asm!.doc.instances.some((i) => i.id === id));
const selMate = (c: Ctx) => selAsmItems(c).find((id) => c.app.asm!.doc.mates.some((m) => m.id === id)) ?? null;
const instanceFixed = (c: Ctx, id: string) => !!c.app.asm!.doc.instances.find((i) => i.id === id)?.fixed;
const sketchTool = (t: SketchTool) => (c: Ctx) => {
  if (!c.app.sketch) {
    // Picking a sketch tool outside a sketch starts a sketch first (SOLIDWORKS behaviour).
    if (!c.app.startSketch()) {
      c.app.pendingTool = t;
      return;
    }
  }
  setTool(c, t);
};

function setTool(c: Ctx, t: SketchTool | null) {
  if (c.app.editor) c.app.editor.setTool(t);
  else {
    c.app.tool = t;
    c.app.changed();
  }
}

const todo = (id: string, label: string, icon?: string, tip?: string): Command => ({ id, label, icon, tip: tip ?? label, run: () => {}, todo: true });

const RELATION_TIPS: [RelationType, string, string][] = [
  ["horizontal", "Make Horizontal", "Adds a horizontal relation to the selection(s)."],
  ["vertical", "Make Vertical", "Adds a vertical relation to the selection(s)."],
  ["collinear", "Make Collinear", "Adds a collinear relation to the selection(s)."],
  ["perpendicular", "Make Perpendicular", "Adds a perpendicular relation to the selection(s)."],
  ["parallel", "Make Parallel", "Adds a parallel relation to the selection(s)."],
  ["equal", "Make Equal", "Adds an equal relation to the selection(s)."],
  ["tangent", "Make Tangent", "Adds a tangent relation to the selection(s)."],
  ["concentric", "Make Concentric", "Adds a concentric relation to the selection(s)."],
  ["coradial", "Make Coradial", "Adds a coradial relation to the selection(s)."],
  ["midpoint", "Make Midpoint", "Adds a midpoint relation to the selection(s)."],
  ["coincident", "Make Coincident", "Adds a coincident relation to the selection(s)."],
  ["symmetric", "Make Symmetric", "Adds a symmetric relation to the selection(s)."],
  ["intersection", "Make Intersection", "Adds an intersection relation to the selection(s)."],
  ["fix", "Make Fixed", "Adds a fixed relation to the selection(s)."],
];
const RELATION_COMMANDS: Command[] = RELATION_TIPS.map(([type, label, tip]) => ({
  id: `rel.${type}`,
  label,
  icon: `rel:${type}`,
  tip,
  run: (c: Ctx) => c.app.editor?.addRelation(type),
  enabled: inSketch,
}));

/**
 * Flyout tool buttons. Like SOLIDWORKS, each flyout shows the tool that was used
 * last and keeps it until another one from the same flyout is chosen.
 */
export const FLYOUTS: Record<string, string[]> = {
  sketch: ["sketch.sketch", "sketch.sketch3d", "sketch.sketch3dPlane"],
  dimension: ["sketch.smartDimension", "sketch.dimHorizontal", "sketch.dimVertical", "sketch.dimBaseline", "sketch.dimChain", "sketch.dimOrdinate", "sketch.dimHOrdinate", "sketch.dimVOrdinate", "sketch.dimPath"],
  line: ["sketch.line", "sketch.centerline", "sketch.midpointLine"],
  circle: ["sketch.circle", "sketch.perimeterCircle"],
  spline: ["sketch.spline", "sketch.styleSpline", "sketch.splineSurface", "sketch.equationCurve"],
  rectangle: ["sketch.rectangle", "sketch.centerRectangle", "sketch.3ptRectangle", "sketch.3ptCenterRectangle", "sketch.parallelogram"],
  arc: ["sketch.arc", "sketch.tangentArc", "sketch.threePointArc"],
  ellipse: ["sketch.ellipse", "sketch.partialEllipse", "sketch.parabola", "sketch.conic"],
  slot: ["sketch.slot", "sketch.centerSlot", "sketch.arcSlot3", "sketch.arcSlotCenter"],
  sketchFillet: ["sketch.fillet", "sketch.chamfer"],
  trim: ["sketch.trim", "sketch.extend"],
  convert: ["sketch.convert", "sketch.silhouette", "sketch.intersectionCurve"],
  sketchPattern: ["sketch.linearPattern", "sketch.circularPattern"],
  move: ["sketch.move", "sketch.copy", "sketch.rotate", "sketch.scale", "sketch.stretch"],
  relations: ["sketch.relations", "sketch.addRelation", "sketch.fullyDefine"],
  fillet: ["feat.fillet", "feat.chamfer"],
  pattern: ["feat.linearPattern", "feat.circularPattern", "feat.mirror", "feat.curvePattern", "feat.sketchPattern", "feat.tablePattern", "feat.fillPattern", "feat.variablePattern"],
};
const FLYOUT_OF: Record<string, string> = {};
for (const [k, ids] of Object.entries(FLYOUTS)) for (const id of ids) FLYOUT_OF[id] = k;

/** The command a flyout button currently shows (last used, else its first). */
export function flyoutCurrent(app: App, key: string): string {
  const ids = FLYOUTS[key] ?? [];
  const last = app.opt<string>(`flyout.${key}`);
  return last && ids.includes(last) ? last : ids[0];
}

export const COMMANDS: Command[] = [
  // ------------------------------------------------------------ file
  { id: "file.new", label: "New", icon: "new", shortcut: "Ctrl+N", run: (c) => c.ui.newFile() },
  { id: "file.newAssembly", label: "New Assembly", icon: "assembly", tip: "Creates a new assembly: parts and sub-assemblies placed and mated together.", run: (c) => c.ui.newAssembly(), enabled: () => isNative() },
  { id: "file.open", label: "Open...", icon: "open", shortcut: "Ctrl+O", run: (c) => c.ui.openFile() },
  { id: "file.save", label: "Save", icon: "save", shortcut: "Ctrl+S", run: (c) => c.ui.saveFile(false) },
  { id: "file.saveAs", label: "Save As...", run: (c) => c.ui.saveFile(true) },
  { id: "file.exportStep", label: "Export STEP (*.step)", run: (c) => c.ui.exportFile("step"), enabled: (c) => !c.app.asm },
  { id: "file.exportStl", label: "Export STL (*.stl)", run: (c) => c.ui.exportFile("stl"), enabled: (c) => !c.app.asm },
  todo("file.print", "Print...", "print"),
  // ------------------------------------------------------------ edit
  { id: "edit.undo", label: "Undo", icon: "undo", shortcut: "Ctrl+Z", tip: "Reverses the last action.", run: (c) => c.app.undo(), enabled: (c) => c.app.canUndo() },
  { id: "edit.redo", label: "Redo", icon: "redo", shortcut: "Ctrl+Y", tip: "Reverses the last undo action.", run: (c) => c.app.redo(), enabled: (c) => c.app.canRedo() },
  { id: "edit.rebuild", label: "Rebuild", icon: "rebuild", shortcut: "Ctrl+B", tip: "Rebuilds the part. In a sketch, exits the sketch and rebuilds.", run: (c) => c.app.rebuildCommand(false), checked: (c) => c.app.needsRebuild },
  { id: "edit.forceRebuild", label: "Force Rebuild", shortcut: "Ctrl+Q", tip: "Rebuilds every feature of the part from scratch.", run: (c) => c.app.rebuildCommand(true) },
  {
    id: "edit.delete",
    label: "Delete",
    icon: "delete",
    shortcut: "Delete",
    run: (c) => {
      if (c.app.sketch) return c.app.editor?.deleteSelection();
      const id = c.app.selectedFeatureId();
      if (id) c.ui.confirmDelete(id);
    },
  },
  {
    id: "edit.suppress",
    label: "Suppress",
    run: (c) => {
      const f = c.app.selection.find((s) => s.kind === "feature");
      if (f && f.kind === "feature") c.app.toggleSuppress(f.id);
    },
  },
  // ------------------------------------------------------------ view
  { id: "view.redraw", label: "Redraw", shortcut: "Ctrl+R", run: (c) => c.vp?.invalidate() },
  { id: "view.zoomFit", label: "Zoom to Fit", icon: "zoomFit", shortcut: "F", run: (c) => c.vp?.zoomToFit() },
  { id: "view.zoomArea", label: "Zoom to Area", icon: "zoomArea", tip: "Zooms to an area you select with a bounding box.", run: (c) => c.vp?.setViewMode("zoomArea"), checked: (c) => c.vp?.viewMode === "zoomArea" },
  { id: "view.zoomIn", label: "Zoom In", shortcut: "Shift+Z", run: (c) => c.vp?.zoomStep(true) },
  { id: "view.zoomSelection", label: "Zoom to Selection", icon: "zoomSelection", tip: "Zooms to selected entities.", run: (c) => c.vp?.zoomToSelection() },
  { id: "view.zoomInOut", label: "Zoom In/Out", icon: "zoomInOut", tip: "Zooms in or out as you drag the pointer up or down.", run: (c) => c.vp?.setViewMode("zoom"), checked: (c) => c.vp?.viewMode === "zoom" },
  { id: "view.rotateView", label: "Rotate View", icon: "rotateView", tip: "Rotates the model view as you drag.", run: (c) => c.vp?.setViewMode("rotate"), checked: (c) => c.vp?.viewMode === "rotate" },
  { id: "view.pan", label: "Pan", icon: "pan", tip: "Scrolls the view as you drag.", run: (c) => c.vp?.setViewMode("pan"), checked: (c) => c.vp?.viewMode === "pan" },
  { id: "view.rollView", label: "Roll View", icon: "rollView", tip: "Rolls the model view as you drag.", run: (c) => c.vp?.setViewMode("roll"), checked: (c) => c.vp?.viewMode === "roll" },
  todo("view.rotateFloor", "Rotate About Scene Floor"),
  todo("view.editScene", "Edit Scene", "scene"),
  todo("view.openDrawing", "Open Drawing", "drawing"),
  todo("view.generateDrawing", "Generate Drawing", "drawing"),
  todo("select.lasso", "Lasso Selection", "lasso"),
  todo("select.invert", "Invert Selection", "invert"),
  {
    id: "select.other",
    label: "Select Other",
    icon: "selectOther",
    tip: "Selects entities hidden behind others, or nearby, at the cursor.",
    run: (c) => {
      const items = c.vp?.selectOtherCandidates() ?? [];
      const at = c.vp?.lastContext;
      if (!items.length || !at) return c.app.flash("Nothing else to select here.", "info");
      c.app.selectOther = { x: at.cx, y: at.cy, items };
      c.app.changed();
    },
  },
  { id: "select.filters", label: "Selection Filters", icon: "filter", shortcut: "F5", tip: "Shows the Selection Filter toolbar.", run: (c) => ((c.app.filterBar = !c.app.filterBar), c.app.changed()), checked: (c) => c.app.filterBar },
  { id: "select.filterToggle", label: "Toggle Selection Filters", shortcut: "F6", tip: "Turns the selection filters on or off.", run: (c) => ((c.app.filterOn = !c.app.filterOn), c.app.flash(c.app.filterOn ? "Selection filters on" : "Selection filters off", "info")) },
  { id: "select.filterEdges", label: "Filter Edges", icon: "filter", shortcut: "E", tip: "Only edges can be selected.", run: (c) => c.app.toggleFilter("edge"), checked: (c) => c.app.selFilter.has("edge") },
  { id: "select.filterVertices", label: "Filter Vertices", icon: "filter", shortcut: "V", tip: "Only vertices can be selected.", run: (c) => c.app.toggleFilter("vertex"), checked: (c) => c.app.selFilter.has("vertex") },
  { id: "select.filterFaces", label: "Filter Faces", icon: "filter", shortcut: "X", tip: "Only faces can be selected.", run: (c) => c.app.toggleFilter("face"), checked: (c) => c.app.selFilter.has("face") },
  { id: "select.box", label: "Box Selection", icon: "boxSelect", tip: "Selects items with a box.", run: () => {}, checked: () => true },
  { id: "view.origins", label: "Origins", icon: "origin", tip: "Controls the visibility of origins.", run: (c) => c.app.setOpt("view.origins", !c.app.opt("view.origins")), checked: (c) => !!c.app.opt("view.origins") },
  { id: "view.planes", label: "Planes", icon: "plane", tip: "Controls the visibility of planes.", run: (c) => c.app.setOpt("view.planes", c.app.opt("view.planes") === false), checked: (c) => c.app.opt("view.planes") !== false },
  {
    id: "view.showHide",
    label: "Show/Hide",
    icon: "hideShow",
    tip: "Shows or hides the selected item.",
    run: (c) => {
      for (const s of c.app.selection) {
        if (s.kind === "plane") c.app.setPlaneVisible(s.id, !c.app.isPlaneVisible(s.id));
        else if (s.kind === "feature" && c.app.featureById(s.id)?.type === "refplane") c.app.setPlaneVisible(s.id, !c.app.isPlaneVisible(s.id));
        else if (s.kind === "feature" && c.app.featureById(s.id)?.type === "sketch") c.app.setSketchVisible(s.id, !c.app.isSketchVisible(s.id));
      }
    },
  },
  { id: "view.zoomOut", label: "Zoom Out", shortcut: "Z", run: (c) => c.vp?.zoomStep(false) },
  { id: "view.previous", label: "Previous View", icon: "prevView", shortcut: "Ctrl+Shift+Z", run: (c) => { c.vp?.cam.previousView(c.app.options.view.animationSpeed); c.vp?.invalidate(); } },
  { id: "view.section", label: "Section View", icon: "section", tip: "Displays a cutaway of a part using one or more cross section planes.", run: (c) => c.app.toggleSection(), checked: (c) => !!c.app.section },
  { id: "view.orientation", label: "View Orientation", icon: "viewOrient", shortcut: "Space", run: (c) => c.ui.showOrientation() },
  { id: "view.front", label: "Front", shortcut: "Ctrl+1", run: (c) => c.vp?.orient("front") },
  { id: "view.back", label: "Back", shortcut: "Ctrl+2", run: (c) => c.vp?.orient("back") },
  { id: "view.left", label: "Left", shortcut: "Ctrl+3", run: (c) => c.vp?.orient("left") },
  { id: "view.right", label: "Right", shortcut: "Ctrl+4", run: (c) => c.vp?.orient("right") },
  { id: "view.top", label: "Top", shortcut: "Ctrl+5", run: (c) => c.vp?.orient("top") },
  { id: "view.bottom", label: "Bottom", shortcut: "Ctrl+6", run: (c) => c.vp?.orient("bottom") },
  { id: "view.isometric", label: "Isometric", shortcut: "Ctrl+7", run: (c) => c.vp?.orient("isometric") },
  { id: "view.trimetric", label: "Trimetric", run: (c) => c.vp?.orient("trimetric") },
  { id: "view.dimetric", label: "Dimetric", run: (c) => c.vp?.orient("dimetric") },
  { id: "view.normalTo", label: "Normal To", icon: "normalTo", tip: "Rotates and zooms the model to the view orientation normal to the selected plane, planar face, or feature.", shortcut: "Ctrl+8", run: (c) => c.vp?.normalToSelection() },
  { id: "view.shadedEdges", label: "Shaded With Edges", run: (c) => { c.vp?.setDisplayStyle("shadedEdges"); c.app.changed(); }, checked: (c) => c.vp?.displayStyle === "shadedEdges" },
  { id: "view.shaded", label: "Shaded", run: (c) => { c.vp?.setDisplayStyle("shaded"); c.app.changed(); }, checked: (c) => c.vp?.displayStyle === "shaded" },
  { id: "view.hlr", label: "Hidden Lines Removed", run: (c) => { c.vp?.setDisplayStyle("hlr"); c.app.changed(); }, checked: (c) => c.vp?.displayStyle === "hlr" },
  { id: "view.hlv", label: "Hidden Lines Visible", run: (c) => { c.vp?.setDisplayStyle("hlv"); c.app.changed(); }, checked: (c) => c.vp?.displayStyle === "hlv" },
  { id: "view.wireframe", label: "Wireframe", run: (c) => { c.vp?.setDisplayStyle("wireframe"); c.app.changed(); }, checked: (c) => c.vp?.displayStyle === "wireframe" },
  {
    id: "view.sketchRelations",
    label: "Sketch Relations",
    icon: "relations",
    run: (c) => c.app.setOptions((o) => (o.sketch.showRelations = !o.sketch.showRelations)),
    checked: (c) => c.app.options.sketch.showRelations,
  },
  { id: "view.taskPane", label: "Task Pane", shortcut: "Ctrl+F1", run: (c) => c.ui.toggleTaskPane() },
  { id: "view.treePane", label: "FeatureManager Tree Area", shortcut: "F9", run: (c) => c.ui.toggleTreePane() },
  todo("view.hideShow", "Hide/Show Items", "hideShow"),
  {
    id: "view.appearance",
    label: "Edit Appearance",
    icon: "appearance",
    tip: "Edits the colour of the part.",
    run: (c) => {
      const input = document.createElement("input");
      input.type = "color";
      input.value = c.app.doc.display?.color ?? "#c9ccd1";
      input.onchange = () => c.app.setBodyDisplay({ color: input.value });
      input.click();
    },
  },
  todo("view.scene", "Apply Scene", "scene"),
  {
    id: "view.perspective",
    label: "Perspective",
    run: (c) => {
      if (!c.vp) return;
      c.vp.cam.perspective = !c.vp.cam.perspective;
      c.vp.cam.update();
      c.app.changed();
    },
    checked: (c) => !!c.vp?.cam.perspective,
  },
  // ------------------------------------------------------------ sketch
  {
    id: "sketch.sketch",
    label: "Sketch",
    icon: "sketch",
    tip: "Sketch: creates a new sketch, or edits an existing sketch.",
    run: (c) => {
      if (c.app.sketch) c.app.exitSketch(true);
      else {
        const f = c.app.selection.find((s) => s.kind === "feature");
        if (f && f.kind === "feature" && c.app.featureById(f.id)?.type === "sketch") c.app.editSketch(f.id);
        else c.app.startSketch();
      }
    },
    checked: inSketch,
  },
  { id: "sketch.exit", label: "Exit Sketch", icon: "exitSketch", run: (c) => c.app.exitSketch(true), enabled: inSketch },
  { id: "sketch.discard", label: "Discard Changes and Exit", run: (c) => c.app.exitSketch(false), enabled: inSketch },
  { id: "sketch.smartDimension", label: "Smart Dimension", icon: "smartDimension", tip: "Creates a dimension for one or more selected entities.", run: sketchTool("smartDimension"), checked: (c) => c.app.tool === "smartDimension" },
  { id: "sketch.line", label: "Line", icon: "line", shortcut: "L", tip: "Sketches a line.", run: sketchTool("line"), checked: (c) => c.app.tool === "line" },
  { id: "sketch.centerline", label: "Centerline", icon: "centerline", tip: "Sketches a centerline (construction).", run: sketchTool("centerline"), checked: (c) => c.app.tool === "centerline" },
  { id: "sketch.rectangle", label: "Corner Rectangle", icon: "rectangle", tip: "Sketches a corner rectangle.", run: sketchTool("rectangle"), checked: (c) => c.app.tool === "rectangle" },
  { id: "sketch.centerRectangle", label: "Center Rectangle", icon: "centerRectangle", tip: "Sketches a rectangle from its center.", run: sketchTool("centerRectangle"), checked: (c) => c.app.tool === "centerRectangle" },
  { id: "sketch.midpointLine", label: "Midpoint Line", icon: "midpointLine", tip: "Sketches a line that is symmetrical from the midpoint of the line.", run: sketchTool("midpointLine"), checked: (c) => c.app.tool === "midpointLine" },
  { id: "sketch.3ptRectangle", label: "3 Point Corner Rectangle", icon: "rect3", tip: "Sketches a rectangle at an angle.", run: sketchTool("threePointRectangle"), checked: (c) => c.app.tool === "threePointRectangle" },
  { id: "sketch.3ptCenterRectangle", label: "3 Point Center Rectangle", icon: "rectCenter3", tip: "Sketches a rectangle with a center point at an angle.", run: sketchTool("threePointCenterRectangle"), checked: (c) => c.app.tool === "threePointCenterRectangle" },
  { id: "sketch.parallelogram", label: "Parallelogram", icon: "parallelogram", tip: "Sketches a parallelogram.", run: sketchTool("parallelogram"), checked: (c) => c.app.tool === "parallelogram" },
  { id: "sketch.circle", label: "Circle", icon: "circle", tip: "Sketches a circle by center and radius.", run: sketchTool("circle"), checked: (c) => c.app.tool === "circle" },
  { id: "sketch.perimeterCircle", label: "Perimeter Circle", icon: "perimeterCircle", tip: "Sketches a circle through three points on its perimeter.", run: sketchTool("perimeterCircle"), checked: (c) => c.app.tool === "perimeterCircle" },
  { id: "sketch.arc", label: "Centerpoint Arc", icon: "arc", tip: "Sketches an arc from center, start and end points.", run: sketchTool("arc"), checked: (c) => c.app.tool === "arc" },
  { id: "sketch.tangentArc", label: "Tangent Arc", icon: "tangentArc", tip: "Sketches an arc tangent to a sketch entity.", run: sketchTool("tangentArc"), checked: (c) => c.app.tool === "tangentArc" },
  { id: "sketch.threePointArc", label: "3 Point Arc", icon: "threePointArc", tip: "Sketches an arc through start, end and middle points.", run: sketchTool("threePointArc"), checked: (c) => c.app.tool === "threePointArc" },
  { id: "sketch.point", label: "Point", icon: "point", tip: "Sketches a point.", run: sketchTool("point"), checked: (c) => c.app.tool === "point" },
  { id: "sketch.trim", label: "Trim Entities", icon: "trim", tip: "Trims a sketch entity (Power trim: drag across entities).", run: sketchTool("trim"), checked: (c) => c.app.tool === "trim" },
  { id: "sketch.slot", label: "Straight Slot", icon: "slot", tip: "Sketches a straight slot from its two arc centers.", run: sketchTool("straightSlot"), checked: (c) => c.app.tool === "straightSlot" },
  { id: "sketch.centerSlot", label: "Centerpoint Straight Slot", icon: "slotCenter", tip: "Sketches a straight slot from its center.", run: sketchTool("centerSlot"), checked: (c) => c.app.tool === "centerSlot" },
  todo("sketch.arcSlot3", "3 Point Arc Slot", "slotArc3"),
  todo("sketch.arcSlotCenter", "Centerpoint Arc Slot", "slotArcCenter"),
  { id: "sketch.polygon", label: "Polygon", icon: "polygon", tip: "Sketches a polygon. Change the number of sides in the PropertyManager.", run: sketchTool("polygon"), checked: (c) => c.app.tool === "polygon" },
  { id: "sketch.spline", label: "Spline", icon: "spline", tip: "Sketches a spline. Click to add points; double-click to end.", run: sketchTool("spline"), checked: (c) => c.app.tool === "spline" },
  todo("sketch.styleSpline", "Style Spline", "styleSpline"),
  todo("sketch.splineSurface", "Spline on Surface", "splineSurface"),
  todo("sketch.equationCurve", "Equation Driven Curve", "equationCurve"),
  { id: "sketch.ellipse", label: "Ellipse", icon: "ellipse", tip: "Sketches a complete ellipse: centre, then the end of one axis, then the other radius.", run: sketchTool("ellipse"), checked: (c) => c.app.tool === "ellipse" },
  todo("sketch.partialEllipse", "Partial Ellipse", "partialEllipse"),
  todo("sketch.parabola", "Parabola", "parabola"),
  todo("sketch.conic", "Conic", "conic"),
  { id: "sketch.fillet", label: "Sketch Fillet", icon: "sketchFillet", tip: "Rounds the corner where two sketch entities intersect to create a tangent arc.", run: sketchTool("sketchFillet"), checked: (c) => c.app.tool === "sketchFillet" },
  { id: "sketch.chamfer", label: "Sketch Chamfer", icon: "sketchChamfer", tip: "Applies a chamfer to adjacent sketch entities.", run: sketchTool("sketchChamfer"), checked: (c) => c.app.tool === "sketchChamfer" },
  todo("sketch.plane", "Plane", "sketchPlane"),
  { id: "sketch.extend", label: "Extend Entities", icon: "extend", tip: "Extends a sketch entity to meet another sketch entity.", run: sketchTool("extend"), checked: (c) => c.app.tool === "extend" },
  todo("sketch.silhouette", "Silhouette Entities", "silhouette"),
  todo("sketch.intersectionCurve", "Intersection Curve", "intersectionCurve"),
  { id: "sketch.circularPattern", label: "Circular Sketch Pattern", icon: "circularSketchPattern", tip: "Adds a circular pattern of sketch entities.", run: sketchTool("circularPattern"), checked: (c) => c.app.tool === "circularPattern" },
  { id: "sketch.copy", label: "Copy Entities", icon: "copyEntities", tip: "Copies sketch entities.", run: sketchTool("copy"), checked: (c) => c.app.tool === "copy" },
  { id: "sketch.rotate", label: "Rotate Entities", icon: "rotateEntities", tip: "Rotates sketch entities.", run: sketchTool("rotate"), checked: (c) => c.app.tool === "rotate" },
  { id: "sketch.scale", label: "Scale Entities", icon: "scaleEntities", tip: "Scales sketch entities.", run: sketchTool("scale"), checked: (c) => c.app.tool === "scale" },
  todo("sketch.stretch", "Stretch Entities", "stretchEntities"),
  todo("sketch.fullyDefine", "Fully Define Sketch", "fullyDefine"),
  todo("sketch.sketch3d", "3D Sketch", "sketch3d"),
  todo("sketch.sketch3dPlane", "3D Sketch On Plane", "sketch3d"),
  { id: "sketch.dimHorizontal", label: "Horizontal Dimension", icon: "dimH", tip: "Creates a horizontal dimension between two selected entities.", run: sketchTool("dimHorizontal"), checked: (c) => c.app.tool === "dimHorizontal" },
  { id: "sketch.dimVertical", label: "Vertical Dimension", icon: "dimV", tip: "Creates a vertical dimension between two selected entities.", run: sketchTool("dimVertical"), checked: (c) => c.app.tool === "dimVertical" },
  todo("sketch.dimBaseline", "Baseline Dimension", "dimBaseline"),
  todo("sketch.dimChain", "Chain Dimension", "dimChain"),
  todo("sketch.dimOrdinate", "Ordinate Dimension", "dimOrdinate"),
  todo("sketch.dimHOrdinate", "Horizontal Ordinate Dimension", "dimOrdinate"),
  todo("sketch.dimVOrdinate", "Vertical Ordinate Dimension", "dimOrdinate"),
  todo("sketch.dimPath", "Path Length Dimension", "dimPath"),
  todo("sketch.selectOther", "Select Other", "selectOther"),
  todo("sketch.makeBlock", "Make Block", "block"),
  todo("sketch.replaceEntity", "Replace Entity", "replace"),
  todo("sketch.numericInput", "Sketch Numeric Input", "numeric"),
  todo("sketch.dimPreview", "Preview Sketch Dimension", "dimPreview"),
  todo("sketch.selectMidpoint", "Select Midpoint", "midpointLine"),
  todo("sketch.contourSelect", "Contour Select Tool", "select"),
  todo("sketch.makePath", "Make Path", "spline"),
  todo("sketch.detachSegment", "Detach Segment On Drag", "extend"),
  { id: "sketch.addRelation", label: "Add Relation", icon: "addRelation", tip: "Controls the size or position of entities with constraints such as concentric or vertical.", run: (c) => c.app.sketch && c.app.openPM({ kind: "addRelations" }), enabled: inSketch },
  { id: "sketch.autoDimension", label: "Smart Dimension", icon: "smartDimension", tip: "Applies the correct dimension based on selected entities.", run: (c) => c.app.editor?.autoDimension(), enabled: inSketch },
  { id: "sketch.select", label: "Select", icon: "select", tip: "Selects sketch entities, edges, vertices, components and so on.", run: (c) => c.app.editor?.setTool(null), checked: (c) => !c.app.tool },
  { id: "sketch.endChain", label: "End chain  (double-click)", run: (c) => { c.app.editor?.resetTool(); c.app.changed(); }, enabled: inSketch },
  { id: "sketch.switchToArc", label: "Switch to arc  ( A )", run: (c) => c.app.editor?.onKey("a"), enabled: inSketch },
  { id: "sketch.displayGrid", label: "Display Grid", icon: "grid", run: (c) => c.app.setDocProp("grid.display", !c.app.docProp("grid.display")), checked: (c) => !!c.app.docProp("grid.display") },
  { id: "sketch.snapsOptions", label: "Relations/Snaps Options...", run: (c) => c.ui.showOptions("relationsSnaps") },
  ...RELATION_COMMANDS,
  todo("sketch.text", "Text", "text"),
  { id: "sketch.convert", label: "Convert Entities", icon: "convert", tip: "Converts selected model edges or faces (or sketch entities) into sketch segments.", run: sketchTool("convert"), checked: (c) => c.app.tool === "convert" },
  { id: "sketch.offset", label: "Offset Entities", icon: "offset", tip: "Adds sketch entities by offsetting faces, edges, curves or sketch entities a specified distance.", run: sketchTool("offset"), checked: (c) => c.app.tool === "offset" },
  { id: "sketch.mirror", label: "Mirror Entities", icon: "sketchMirror", tip: "Mirrors selected entities about a centerline.", run: sketchTool("mirror"), checked: (c) => c.app.tool === "mirror" },
  { id: "sketch.linearPattern", label: "Linear Sketch Pattern", icon: "sketchPattern", tip: "Adds a linear pattern of sketch entities.", run: sketchTool("linearPattern"), checked: (c) => c.app.tool === "linearPattern" },
  { id: "sketch.move", label: "Move Entities", icon: "move", tip: "Moves sketch entities.", run: sketchTool("move"), checked: (c) => c.app.tool === "move" },
  todo("sketch.offsetSurface", "Offset On Surface", "offset"),
  todo("sketch.repair", "Repair Sketch", "repair"),
  todo("sketch.quickSnaps", "Quick Snaps", "quickSnaps"),
  todo("sketch.rapidSketch", "Rapid Sketch", "rapidSketch"),
  todo("sketch.instant2d", "Instant2D", "instant2d"),
  {
    id: "sketch.shadedContours",
    label: "Shaded Sketch Contours",
    icon: "shadedContours",
    tip: "Shades closed sketch contours. Click inside a shaded area to select the entities around it; drag it to move them.",
    run: (c) => c.app.setOpt("sketch.shadeContours", c.app.opt("sketch.shadeContours") === false),
    checked: (c) => c.app.opt("sketch.shadeContours") !== false,
  },
  {
    id: "sketch.relations",
    label: "Display/Delete Relations",
    icon: "relations",
    run: (c) => {
      if (!c.app.sketch) return;
      c.app.openPM({ kind: "addRelations" });
    },
    enabled: inSketch,
  },
  {
    id: "sketch.toggleConstruction",
    label: "Construction Geometry",
    icon: "construction",
    run: (c) => {
      const sk = c.app.sketch?.data;
      if (!sk) return;
      c.app.sketchCheckpoint();
      for (const s of c.app.selection) {
        if (s.kind !== "skEntity") continue;
        const e = sk.entities[s.id];
        if (e && e.type !== "point") (e as { construction?: boolean }).construction = !(e as { construction?: boolean }).construction;
      }
      c.app.resolveSketch();
    },
    enabled: inSketch,
  },
  {
    id: "sketch.autoRelations",
    label: "Automatic Relations",
    icon: "autoRelations",
    run: (c) => c.app.setOptions((o) => (o.sketch.automaticRelations = !o.sketch.automaticRelations)),
    checked: (c) => c.app.options.sketch.automaticRelations,
  },
  {
    id: "sketch.grid",
    label: "Grid Snap",
    icon: "grid",
    run: (c) => c.app.setOptions((o) => (o.sketch.snaps.grid = !o.sketch.snaps.grid)),
    checked: (c) => c.app.options.sketch.snaps.grid,
  },
  // ------------------------------------------------------------ features
  { id: "feat.extrude", label: "Extruded Boss/Base", icon: "extrude", tip: "Extrudes a sketch or selected sketch contours in one or two directions to create a solid feature.", run: (c) => c.app.startExtrude(false) },
  { id: "feat.revolve", label: "Revolved Boss/Base", icon: "revolve", tip: "Revolves a sketch or selected sketch contours around an axis to create a solid feature.", run: (c) => c.app.startRevolve(false) },
  { id: "feat.cutExtrude", label: "Extruded Cut", icon: "cutExtrude", tip: "Cuts a solid model by extruding a sketched profile in one or two directions.", run: (c) => c.app.startExtrude(true) },
  { id: "feat.cutRevolve", label: "Revolved Cut", icon: "cutRevolve", tip: "Cuts a solid model by revolving a sketched profile around an axis.", run: (c) => c.app.startRevolve(true) },
  { id: "feat.fillet", label: "Fillet", icon: "fillet", tip: "Creates a rounded internal or external face along one or more edges.", run: (c) => c.app.startFillet() },
  { id: "feat.chamfer", label: "Chamfer", icon: "chamfer", tip: "Creates a bevel feature on selected edges.", run: (c) => c.app.startChamfer() },
  { id: "feat.plane", label: "Plane", icon: "refGeom", tip: "Adds a reference plane.", run: (c) => c.app.startRefPlane() },
  { id: "feat.axis", label: "Axis", icon: "refAxis", tip: "Adds a reference axis.", run: (c) => c.app.startRefAxis(), enabled: (c) => !c.app.sketch },
  { id: "feat.coordSys", label: "Coordinate System", icon: "coordSys", tip: "Defines a coordinate system for a part or assembly.", run: (c) => c.app.startCoordSys(), enabled: (c) => !c.app.sketch },
  { id: "feat.refPoint", label: "Point", icon: "refPoint", tip: "Adds a reference point.", run: (c) => c.app.startRefPoint(), enabled: (c) => !c.app.sketch },
  todo("feat.centerOfMass", "Center of Mass", "massProps"),
  todo("feat.boundingBox", "Bounding Box", "shell"),
  todo("feat.mateRef", "Mate Reference", "refGeom"),
  { id: "feat.swept", label: "Swept Boss/Base", icon: "swept", tip: "Sweeps a closed profile along an open or closed path.", run: (c) => c.app.startSweep(false) },
  { id: "feat.lofted", label: "Lofted Boss/Base", icon: "lofted", tip: "Adds a loft between two or more profiles.", run: (c) => c.app.startLoft(false) },
  todo("feat.boundary", "Boundary Boss/Base", "boundary"),
  { id: "feat.hole", label: "Hole Wizard", icon: "hole", tip: "Inserts holes of standard sizes (counterbore, countersink, hole, tapped) at positions on a face.", run: (c) => c.app.startHole(), enabled: (c) => !c.app.sketch && !!c.app.result?.mesh },
  { id: "feat.sweptCut", label: "Swept Cut", icon: "sweptCut", tip: "Cuts a solid model by sweeping a closed profile along an open or closed path.", run: (c) => c.app.startSweep(true), enabled: (c) => !!c.app.result?.mesh },
  { id: "feat.loftedCut", label: "Lofted Cut", icon: "loftedCut", tip: "Cuts a solid model by removing material between two or more profiles.", run: (c) => c.app.startLoft(true), enabled: (c) => !!c.app.result?.mesh },
  todo("feat.boundaryCut", "Boundary Cut", "boundaryCut"),
  { id: "feat.linearPattern", label: "Linear Pattern", icon: "linearPattern", tip: "Patterns features, faces and bodies in one or two linear directions.", run: (c) => c.app.startLinearPattern(), enabled: (c) => !c.app.sketch && !!c.app.result?.mesh },
  { id: "feat.circularPattern", label: "Circular Pattern", icon: "circularPattern", tip: "Patterns features, faces and bodies around an axis.", run: (c) => c.app.startCircularPattern(), enabled: (c) => !c.app.sketch && !!c.app.result?.mesh },
  todo("feat.curvePattern", "Curve Driven Pattern", "linearPattern"),
  todo("feat.sketchPattern", "Sketch Driven Pattern", "linearPattern"),
  todo("feat.tablePattern", "Table Driven Pattern", "linearPattern"),
  todo("feat.fillPattern", "Fill Pattern", "linearPattern"),
  todo("feat.variablePattern", "Variable Pattern", "linearPattern"),
  { id: "feat.rib", label: "Rib", icon: "rib", tip: "Adds a thin-walled support from an open sketch to the part.", run: (c) => c.app.startRib(), enabled: (c) => !!c.app.result?.mesh },
  { id: "feat.draft", label: "Draft", icon: "draft", tip: "Tapers selected faces of a model by a specified angle, using a neutral plane.", run: (c) => c.app.startDraft(), enabled: (c) => !c.app.sketch && !!c.app.result?.mesh },
  { id: "feat.shell", label: "Shell", icon: "shell", tip: "Hollows out a part, leaves selected faces open and creates thin-walled features on the remaining faces.", run: (c) => c.app.startShell(), enabled: (c) => !c.app.sketch && !!c.app.result?.mesh },
  todo("feat.wrap", "Wrap", "wrap"),
  todo("feat.intersect", "Intersect", "intersect"),
  { id: "feat.mirror", label: "Mirror", icon: "mirror", tip: "Mirrors features, faces and bodies about a face or a plane.", run: (c) => c.app.startMirror(), enabled: (c) => !c.app.sketch && !!c.app.result?.mesh },
  todo("feat.curves", "Curves", "curves"),
  todo("feat.instant3d", "Instant3D", "instant3d"),
  { id: "feat.edit", label: "Edit Feature", icon: "editFeature", tip: "Edits the definition of the selected feature.", run: (c) => { if (c.app.asm) { const m = selMate(c); if (m) c.app.asm.openMate(m); return; } const id = c.app.selectedFeatureId(); if (id) c.app.editFeature(id); }, enabled: (c) => !!c.app.selectedFeatureId() && !c.app.sketch },
  { id: "feat.editSketch", label: "Edit Sketch", icon: "editSketch", tip: "Edits the sketch of the selected feature.", run: (c) => { const id = c.app.selectedSketchId(); if (id) c.app.editSketch(id); }, enabled: (c) => !!c.app.selectedSketchId() && !c.app.sketch },
  { id: "feat.suppress", label: "Suppress", icon: "suppress", tip: "Removes one or more features or components from the model. Suppress does not delete the feature.", run: (c) => { if (c.app.asm) return selAsmItems(c).forEach((id) => c.app.asm!.toggleSuppress(id)); const id = c.app.selectedFeatureId(); if (id) c.app.toggleSuppress(id); }, enabled: (c) => !!c.app.selectedFeatureId() },
  { id: "feat.rollback", label: "Rollback", icon: "rollback", tip: "Reverts the model to the state it was in before the selected feature was built.", run: (c) => { const id = c.app.selectedFeatureId(); if (id) c.app.setRollback(c.app.doc.features.findIndex((f) => f.id === id)); }, enabled: (c) => !!c.app.selectedFeatureId() },
  { id: "feat.rollToEnd", label: "Roll to End", icon: "rollback", run: (c) => c.app.setRollback(null), enabled: (c) => c.app.doc.rollback !== null },
  { id: "feat.delete", label: "Delete...", icon: "delete", shortcut: undefined, run: (c) => { if (c.app.asm) return c.app.asm.delete(selAsmItems(c)); const id = c.app.selectedFeatureId(); if (id) c.ui.confirmDelete(id); }, enabled: (c) => !!c.app.selectedFeatureId() },
  { id: "feat.rename", label: "Rename tree item", run: (c) => { const id = c.app.selectedFeatureId(); if (id) c.app.renaming = id, c.app.changed(); }, enabled: (c) => !!c.app.selectedFeatureId() },
  { id: "tree.hideShowItems", label: "Hide/Show Tree Items...", run: (c) => c.ui.showOptions("featureManager") },
  { id: "tree.collapse", label: "Collapse Items", shortcut: "Shift+C", run: (c) => { c.app.treeCollapse++; c.app.changed(); } },
  { id: "feat.hideBody", label: "Hide/Show Body", icon: "hide", tip: "Hides or shows the solid body.", run: (c) => c.app.setBodyDisplay({ bodyHidden: !c.app.doc.display?.bodyHidden }), checked: (c) => !!c.app.doc.display?.bodyHidden },
  todo("feat.appearance", "Appearances", "appearance"),
  todo("feat.copyAppearance", "Copy Appearance", "appearance"),
  todo("feat.openDrawing", "Open Drawing", "drawing"),
  todo("feat.comment", "Comment", "comment"),
  todo("feat.parentChild", "Parent/Child...", "parentChild"),
  todo("feat.configure", "Configure Feature", "configure"),
  todo("feat.favorites", "Add to Favorites", "favorites"),
  todo("feat.saveSelection", "Save Selection"),
  todo("feat.newFolder", "Add to New Folder", "folder"),
  todo("feat.properties", "Feature Properties...", "properties"),
  { id: "feat.transparency", label: "Change Transparency", icon: "transparency", tip: "Toggles transparency of the body.", run: (c) => c.app.setBodyDisplay({ transparent: !c.app.doc.display?.transparent }), checked: (c) => !!c.app.doc.display?.transparent },
  todo("feat.featureWorks", "FeatureWorks..."),
  todo("feat.goTo", "Go To..."),
  todo("feat.createFolder", "Create New Folder"),
  todo("feat.sectionView", "Section View", "section"),
  todo("feat.planeParallel", "Create Plane Parallel to Screen", "plane"),
  todo("feat.isolate", "Isolate"),
  todo("feat.exportDxf", "Export to DXF / DWG"),
  todo("feat.partSupply", "Find Similar in PartSupply"),
  todo("tools.customizeMenu", "Customize Menu"),
  // ------------------------------------------------------------ assembly (G1a; native app only)
  { id: "asm.insertComponent", label: "Insert Components", icon: "insertComponent", tip: "Adds an existing part or sub-assembly to the assembly.", run: (c) => c.ui.insertComponent(), enabled: inAsm },
  { id: "asm.mate", label: "Mate", icon: "mate", tip: "Positions two components relative to one another.", run: (c) => c.app.asm?.openMate(), enabled: (c) => inAsm(c) && (c.app.asm?.doc.instances.length ?? 0) > 1 },
  { id: "asm.editMate", label: "Edit Feature", icon: "editFeature", tip: "Edits the selected mate.", run: (c) => { const m = selMate(c); if (m) c.app.asm?.openMate(m); }, enabled: (c) => inAsm(c) && !!selMate(c) },
  { id: "asm.fix", label: "Fix", icon: "pin", tip: "Fixes the selected components in place.", run: (c) => c.app.asm?.setFixed(selInstances(c), true), enabled: (c) => inAsm(c) && selInstances(c).some((id) => !instanceFixed(c, id)) },
  { id: "asm.float", label: "Float", icon: "move", tip: "Lets the selected components move (placed by mates and dragging).", run: (c) => c.app.asm?.setFixed(selInstances(c), false), enabled: (c) => inAsm(c) && selInstances(c).some((id) => instanceFixed(c, id)) },
  { id: "asm.suppress", label: "Suppress", icon: "suppress", tip: "Suppresses or unsuppresses the selected component or mate.", run: (c) => selAsmItems(c).forEach((id) => c.app.asm?.toggleSuppress(id)), enabled: (c) => inAsm(c) && selAsmItems(c).length > 0 },
  { id: "asm.delete", label: "Delete", icon: "delete", tip: "Deletes the selected components (with their mates) and mates.", run: (c) => c.app.asm?.delete(selAsmItems(c)), enabled: (c) => inAsm(c) && selAsmItems(c).length > 0 },
  todo("asm.componentPattern", "Linear Component Pattern", "linearPattern"),
  todo("asm.smartFasteners", "Smart Fasteners", "hole"),
  todo("asm.moveComponent", "Move Component", "move"),
  todo("asm.rotateComponent", "Rotate Component", "rotateEntities"),
  todo("asm.showHidden", "Show Hidden Components", "hide"),
  todo("asm.features", "Assembly Features", "cutExtrude"),
  todo("asm.newMotionStudy", "New Motion Study", "sensor"),
  todo("asm.bom", "Bill of Materials", "annotations"),
  todo("asm.explodedView", "Exploded View", "move"),
  todo("asm.interference", "Interference Detection", "intersect"),
  todo("asm.openPart", "Open Part", "open"),
  todo("asm.newPart", "New Part", "part"),
  todo("asm.smartMates", "SmartMates", "mate"),
  // ------------------------------------------------------------ evaluate / tools
  { id: "eval.measure", label: "Measure", icon: "measure", tip: "Calculates distance, length, surface area, angle, circles and XYZ location of selected entities.", run: (c) => ((c.app.measureOpen = !c.app.measureOpen), c.app.changed()), checked: (c) => c.app.measureOpen },
  { id: "eval.massProps", label: "Mass Properties", icon: "massProps", run: (c) => c.ui.showMassProps() },
  { id: "tools.equations", label: "Equations...", icon: "equations", tip: "Manages global variables and equations that drive dimensions.", run: (c) => c.ui.showEquations() },
  { id: "feat.editMaterial", label: "Edit Material", icon: "material", tip: "Applies a material to the part (used by Mass Properties).", run: (c) => c.ui.showMaterial() },
  { id: "tools.options", label: "Options...", icon: "options", run: (c) => c.ui.showOptions() },
  { id: "tools.shortcutBar", label: "Shortcut Bar", shortcut: "S", run: (c) => c.ui.showShortcutBar() },
  { id: "help.about", label: "About OpenCAD", run: (c) => c.ui.showAbout() },
];

export const CMD: Record<string, Command> = Object.fromEntries(COMMANDS.map((c) => [c.id, c]));

const NO_REPEAT = new Set(["sketch.exit", "sketch.select", "sketch.autoDimension", "feat.edit", "feat.editSketch", "feat.suppress", "feat.rollback", "feat.rollToEnd", "feat.delete", "sketch.displayGrid"]);

export function runCommand(id: string, ctx: Ctx) {
  const c = CMD[id];
  if (!c || c.todo) return;
  if (c.enabled && !c.enabled(ctx)) return;
  const app = ctx.app;
  if ((id.startsWith("feat.") || id.startsWith("sketch.")) && c.icon && !NO_REPEAT.has(id)) {
    app.lastCommand = id;
    app.recentCommands = [id, ...app.recentCommands.filter((x) => x !== id)].slice(0, 8);
  }
  const fly = FLYOUT_OF[id];
  if (fly && app.opt(`flyout.${fly}`) !== id) app.setOpt(`flyout.${fly}`, id);
  c.run(ctx);
}
