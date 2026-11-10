/**
 * Command registry. Every toolbar button, menu item, shortcut and gesture maps to a
 * command id here, so behaviour is defined once.
 */
import type { App } from "../app/app";
import type { Viewport } from "../render/viewport";
import type { SketchTool } from "../sketch/editor";

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
  openFile: () => void;
  saveFile: (as?: boolean) => void;
  exportFile: (fmt: "step" | "stl") => void;
  showOptions: () => void;
  showOrientation: () => void;
  showMassProps: () => void;
  showShortcutBar: () => void;
  toggleTaskPane: () => void;
  toggleTreePane: () => void;
  showAbout: () => void;
}

const inSketch = (c: Ctx) => !!c.app.sketch;
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

export const COMMANDS: Command[] = [
  // ------------------------------------------------------------ file
  { id: "file.new", label: "New", icon: "new", shortcut: "Ctrl+N", run: (c) => c.app.newDocument() },
  { id: "file.open", label: "Open...", icon: "open", shortcut: "Ctrl+O", run: (c) => c.ui.openFile() },
  { id: "file.save", label: "Save", icon: "save", shortcut: "Ctrl+S", run: (c) => c.ui.saveFile(false) },
  { id: "file.saveAs", label: "Save As...", run: (c) => c.ui.saveFile(true) },
  { id: "file.exportStep", label: "Export STEP (*.step)", run: (c) => c.ui.exportFile("step") },
  { id: "file.exportStl", label: "Export STL (*.stl)", run: (c) => c.ui.exportFile("stl") },
  todo("file.print", "Print...", "print"),
  // ------------------------------------------------------------ edit
  { id: "edit.undo", label: "Undo", icon: "undo", shortcut: "Ctrl+Z", run: (c) => c.app.undo() },
  { id: "edit.redo", label: "Redo", icon: "redo", shortcut: "Ctrl+Y", run: (c) => c.app.redo() },
  { id: "edit.rebuild", label: "Rebuild", icon: "rebuild", shortcut: "Ctrl+B", run: (c) => c.app.rebuild() },
  { id: "edit.forceRebuild", label: "Force Rebuild", shortcut: "Ctrl+Q", run: (c) => c.app.rebuild() },
  {
    id: "edit.delete",
    label: "Delete",
    shortcut: "Delete",
    run: (c) => {
      if (c.app.sketch) return c.app.editor?.deleteSelection();
      const f = c.app.selection.find((s) => s.kind === "feature");
      if (f && f.kind === "feature") c.app.deleteFeature(f.id);
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
  todo("view.zoomArea", "Zoom to Area", "zoomArea"),
  { id: "view.zoomIn", label: "Zoom In", shortcut: "Shift+Z", run: (c) => c.vp?.zoomStep(true) },
  { id: "view.zoomOut", label: "Zoom Out", shortcut: "Z", run: (c) => c.vp?.zoomStep(false) },
  { id: "view.previous", label: "Previous View", icon: "prevView", shortcut: "Ctrl+Shift+Z", run: (c) => { c.vp?.cam.previousView(c.app.options.view.animationSpeed); c.vp?.invalidate(); } },
  todo("view.section", "Section View", "section"),
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
  { id: "view.normalTo", label: "Normal To", shortcut: "Ctrl+8", run: (c) => c.vp?.normalToSelection() },
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
  todo("view.appearance", "Edit Appearance", "appearance"),
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
  todo("sketch.3ptRectangle", "3 Point Corner Rectangle", "rectangle"),
  todo("sketch.parallelogram", "Parallelogram", "rectangle"),
  { id: "sketch.circle", label: "Circle", icon: "circle", tip: "Sketches a circle by center and radius.", run: sketchTool("circle"), checked: (c) => c.app.tool === "circle" },
  todo("sketch.perimeterCircle", "Perimeter Circle", "circle"),
  { id: "sketch.arc", label: "Centerpoint Arc", icon: "arc", tip: "Sketches an arc from center, start and end points.", run: sketchTool("arc"), checked: (c) => c.app.tool === "arc" },
  { id: "sketch.tangentArc", label: "Tangent Arc", icon: "tangentArc", tip: "Sketches an arc tangent to a sketch entity.", run: sketchTool("tangentArc"), checked: (c) => c.app.tool === "tangentArc" },
  { id: "sketch.threePointArc", label: "3 Point Arc", icon: "threePointArc", tip: "Sketches an arc through start, end and middle points.", run: sketchTool("threePointArc"), checked: (c) => c.app.tool === "threePointArc" },
  { id: "sketch.point", label: "Point", icon: "point", tip: "Sketches a point.", run: sketchTool("point"), checked: (c) => c.app.tool === "point" },
  { id: "sketch.trim", label: "Trim Entities", icon: "trim", tip: "Trims a sketch entity (Power trim: drag across entities).", run: sketchTool("trim"), checked: (c) => c.app.tool === "trim" },
  todo("sketch.slot", "Straight Slot", "slot"),
  todo("sketch.polygon", "Polygon", "polygon"),
  todo("sketch.spline", "Spline", "spline"),
  todo("sketch.ellipse", "Ellipse", "ellipse"),
  todo("sketch.fillet", "Sketch Fillet", "sketchFillet"),
  todo("sketch.text", "Text", "text"),
  todo("sketch.convert", "Convert Entities", "convert"),
  todo("sketch.offset", "Offset Entities", "offset"),
  todo("sketch.mirror", "Mirror Entities", "sketchMirror"),
  todo("sketch.linearPattern", "Linear Sketch Pattern", "sketchPattern"),
  todo("sketch.move", "Move Entities", "move"),
  todo("sketch.offsetSurface", "Offset On Surface", "offset"),
  todo("sketch.repair", "Repair Sketch", "repair"),
  todo("sketch.quickSnaps", "Quick Snaps", "quickSnaps"),
  todo("sketch.rapidSketch", "Rapid Sketch", "rapidSketch"),
  todo("sketch.instant2d", "Instant2D", "instant2d"),
  todo("sketch.shadedContours", "Shaded Sketch Contours", "shadedContours"),
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
  todo("feat.axis", "Axis", "refGeom"),
  todo("feat.coordSys", "Coordinate System", "origin"),
  todo("feat.refPoint", "Point", "point"),
  todo("feat.centerOfMass", "Center of Mass", "massProps"),
  todo("feat.boundingBox", "Bounding Box", "shell"),
  todo("feat.mateRef", "Mate Reference", "refGeom"),
  todo("feat.swept", "Swept Boss/Base", "swept"),
  todo("feat.lofted", "Lofted Boss/Base", "lofted"),
  todo("feat.boundary", "Boundary Boss/Base", "boundary"),
  todo("feat.hole", "Hole Wizard", "hole"),
  todo("feat.sweptCut", "Swept Cut", "sweptCut"),
  todo("feat.loftedCut", "Lofted Cut", "loftedCut"),
  todo("feat.boundaryCut", "Boundary Cut", "boundaryCut"),
  todo("feat.linearPattern", "Linear Pattern", "linearPattern"),
  todo("feat.rib", "Rib", "rib"),
  todo("feat.draft", "Draft", "draft"),
  todo("feat.shell", "Shell", "shell"),
  todo("feat.wrap", "Wrap", "wrap"),
  todo("feat.intersect", "Intersect", "intersect"),
  todo("feat.mirror", "Mirror", "mirror"),
  todo("feat.curves", "Curves", "curves"),
  todo("feat.instant3d", "Instant3D", "instant3d"),
  // ------------------------------------------------------------ evaluate / tools
  todo("eval.measure", "Measure", "measure"),
  { id: "eval.massProps", label: "Mass Properties", icon: "massProps", run: (c) => c.ui.showMassProps() },
  { id: "tools.options", label: "Options...", icon: "options", run: (c) => c.ui.showOptions() },
  { id: "tools.shortcutBar", label: "Shortcut Bar", shortcut: "S", run: (c) => c.ui.showShortcutBar() },
  { id: "help.about", label: "About OpenCAD", run: (c) => c.ui.showAbout() },
];

export const CMD: Record<string, Command> = Object.fromEntries(COMMANDS.map((c) => [c.id, c]));

export function runCommand(id: string, ctx: Ctx) {
  const c = CMD[id];
  if (!c || c.todo) return;
  if (c.enabled && !c.enabled(ctx)) return;
  if (id.startsWith("feat.") || id.startsWith("sketch.")) ctx.app.lastCommand = id;
  c.run(ctx);
}
