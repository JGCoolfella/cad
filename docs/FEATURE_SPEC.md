# Feature specification — SOLIDWORKS-parity inventory

This is the master checklist of what "the same as SOLIDWORKS from the user's point of
view" means for OpenCAD. Every user-visible behaviour we intend to match is listed here
with its status. Keep it current: when you implement or change something, update the row.

Legend: ✅ done · 🟡 partial (see note) · ⬜ planned · 🔍 behaviour needs verification
against a real SOLIDWORKS install before we lock it in.

Scope (phase 1): the core part/assembly/drawing modeller. Simulation, rendering
(Visualize/PhotoView), PDM, CAM, Costing, Flow, Routing, Electrical, Toolbox and other
add-ins are out of scope for now, but the command system is designed so they can plug in
as modules (see ARCHITECTURE.md → Modules).

Sources: SOLIDWORKS online help (Middle Mouse Button Functions; Inferencing; Sketch Snaps;
Keyboard Shortcuts pages, 2024 edition), vendor shortcut lists (Javelin, Hawk Ridge, CATI),
and hands-on knowledge. Items marked 🔍 were not confirmed in the official docs.

---

## 1. User interface

| Item | Status | Notes |
|---|---|---|
| Menu bar: File, Edit, View, Insert, Tools, Window, Help + pin | ✅ | Nested sub-menus mirror SW structure. "Pin" is cosmetic. |
| Document title centred in the title bar, `*` when modified | ✅ | |
| Search Commands box (top right) | 🟡 | Runs the first matching command. SW also searches files/knowledge base. |
| Quick-access toolbar (New, Open, Save, Print, Undo, Redo, Select, Rebuild, Options) | 🟡 | Print and Select pending. |
| CommandManager with tabs: Features, Sketch, Evaluate, Markup, MBD Dimensions, Add-Ins, … | 🟡 | Features/Sketch/Evaluate populated; others shown greyed. |
| CommandManager big buttons + stacked small buttons + flyouts | ✅ | Sketch tab uses the SOLIDWORKS layout: Sketch▾, Smart Dimension▾, the 3×4 tool grid, Trim▾, Convert▾, Offset, Mirror/Pattern▾/Move▾, Display/Delete Relations▾, Repair, Quick Snaps, Rapid Sketch, Instant2D, Shaded Sketch Contours. |
| Flyout buttons show and run the last-used tool of their family (Line/Centerline/Midpoint Line, the 5 rectangles, arcs, slots, …), remembered between sessions | ✅ | Families captured from SOLIDWORKS 2026. |
| Auto-switch to Sketch tab when a sketch is opened | ✅ | |
| Unimplemented commands visible but greyed with a "Not yet implemented" tooltip | ✅ | Deliberate: the UI layout matches SW from day one. |
| Tooltips with name, shortcut and description | ✅ | Rich delayed tooltips (bold name, shortcut, description). |
| FeatureManager design tree: part, History, Sensors, Annotations, Material, Front/Top/Right Plane, Origin, features | ✅ | |
| Tree: absorbed sketches under their feature (expand arrow) | ✅ | |
| Tree: hover highlights geometry, click selects, Ctrl/Shift multi-select | ✅ | |
| Tree: double-click to edit, right-click context menu (Edit Feature/Sketch, Suppress, Rollback, Rename, Delete) | ✅ | |
| Tree: F2 / slow double-click rename | 🟡 | F2 works when the row has focus; slow double-click pending. |
| Tree: rollback bar (drag) | 🟡 | Drag works row-by-row; SW-exact drop targets pending. |
| Tree: error/warning icons on failed features | 🟡 | Error icon done; yellow warning and "What's wrong" dialog pending. |
| Tree filter box | ✅ | |
| Tree: folders, reorder by drag, dynamic reference visualisation | ⬜ | |
| PropertyManager replaces the tree in the left pane while a command runs | ✅ | |
| PropertyManager: ✓ ✗ buttons, collapsible group boxes, checkable groups (Direction 2) | ✅ | |
| FeatureManager fly-out tree in the graphics area while a PM is open | ✅ | |
| ConfigurationManager, DimXpertManager, DisplayManager tabs | ⬜ | Tabs present, disabled. |
| Confirmation corner (top-right): Exit Sketch / Discard; OK / Cancel for features | ✅ | |
| Heads-up View toolbar (Zoom to Fit, Zoom to Area, Previous View, Section View, View Orientation, Display Style, Hide/Show, Appearance, Scene, View Settings) | 🟡 | View Orientation flyout (cube-icon grid, Isometric/Dimetric/Trimetric, Normal To, viewport buttons, » named views) and Display Style icon flyout, Zoom to Area and Section View done; Hide/Show, Appearance, Scene pending. |
| Task Pane (right) with icon strip | 🟡 | Strip + placeholder panel. Design Library, File Explorer, View Palette, Appearances pending. |
| Status bar: sketch coordinates, sketch status (Under/Fully/Over Defined), "Editing Part/Sketch", units | ✅ | Coordinates update live without re-rendering the UI. |
| Bottom tabs (Model / Motion Study / 3D Views) | 🟡 | Cosmetic. |
| Reference triad (bottom-left) | ✅ | Click-to-orient on triad arrows pending. |
| Shortcut bar (S key), context-sensitive | ✅ | Customisation pending. |
| Shortcut (right-click) menus: icon toolbar, sub-menus, bold section headers, ≫ expander | ✅ | Captured from SOLIDWORKS 2026 for: sketch background, sketch entity, running sketch tool, face/edge/vertex, feature and plane in the tree, empty graphics area. Items without an implementation are greyed. |
| Context toolbar next to the pointer after a left-click selection (faint until hovered) | ✅ | Relation buttons depend on the selection (Make Horizontal/Vertical/Fixed, …). |
| Confirm Delete dialog (dependent items, Delete absorbed features) | ✅ | |
| Left-drag view modes from the menu: Rotate View, Pan, Zoom In/Out, Roll View; Zoom to Selection | ✅ | Esc leaves the mode. |
| Hide/Show Items flyout: planes, origins, sketch relations, grid | 🟡 | Other item types pending. |
| Part origin (blue), selectable, View Origins toggle | ✅ | 🔍 shown by default here; hidden by default in some SOLIDWORKS templates. |
| Mouse gestures (right-drag radial menu) | 🟡 | 4-way; defaults are 🔍 guesses; 8/12-way and customisation pending. |
| Breadcrumbs on selection (top-left chip) | 🟡 | Shows selected item; D key / hierarchy pending. |
| Hover name tag at the pointer for planes and sketches | ✅ | |
| Current view name ("*Isometric") bottom-left | ✅ | Cleared on free rotation. |
| Tools > Customize (toolbars, shortcuts, mouse gestures) | ⬜ | Keymap table already central (commands/keyboard.ts). |
| Light / dark UI themes | 🟡 | Option exists; dark palette pending. |
| Multiple documents / Window menu tiling | ⬜ | |

## 2. View manipulation (verified against SW help unless 🔍)

| Behaviour | Status | Notes |
|---|---|---|
| Middle-drag rotates (free/trackball, not turntable) | ✅ | Rotation centre = model bounding-box centre. 🔍 speed (we use 0.4°/px). |
| Middle-click a vertex/edge/face, then middle-drag rotates about it | ✅ | Linear edge = rotate about the edge axis; face = about clicked point. |
| Ctrl + middle-drag pans | ✅ | |
| Shift + middle-drag zooms | ✅ | Drag up = zoom in 🔍. |
| Alt + middle-drag rolls about the screen normal | ✅ | 🔍 |
| Mouse wheel zooms toward the pointer | ✅ | SW help: "keep the pointer on the area where you want to zoom". |
| Wheel direction: SW default (wheel forward = zoom out 🔍) + "Reverse mouse wheel zoom direction" option | ✅ | Option in Tools > Options > View. |
| Zoom about screen centre option | ✅ | |
| Middle double-click = Zoom to Fit | ✅ | |
| Wheel zoom works under a PropertyManager without focusing | ✅ | |
| Arrow keys rotate by increment (default 15°), Shift+arrows 90°, Alt+←/→ roll, Ctrl+arrows pan | ✅ | Increment configurable. |
| F zoom to fit, Z zoom out, Shift+Z zoom in | ✅ | |
| G magnifying glass | ⬜ | |
| Ctrl+Shift+Z Previous View | ✅ | |
| Spacebar Orientation dialog | ✅ | View list, pin, View Selector toggle; New View (saved views) pending. |
| 3D View Selector (bevelled cube: 6 faces, 12 edges, 8 corners + floating panels for hidden faces) | ✅ | Shows with the Orientation flyout/dialog, Ctrl+Space alone; hover tints facet blue; click orients (respects "Zoom to fit when changing to standard views"). |
| Ctrl+1..8: Front, Back, Left, Right, Top, Bottom, Isometric, Normal To | ✅ | |
| Normal To twice flips to the other side | ✅ | |
| Animated view transitions | ✅ | Speed option. |
| Auto-rotate normal to sketch plane on sketch creation | ✅ | Option. |
| Orthographic default, Perspective toggle | ✅ | |
| Display styles: Wireframe, HLV, HLR, Shaded with Edges, Shaded | ✅ | |
| Tangent edges: visible / as phantom / removed | ⬜ | Currently visible (SW default). |
| Silhouette edges on curved faces | ⬜ | |
| Section view (Front/Top/Right or a face/plane, offset, flip; capped) | ✅ | Multiple section planes and saving the view pending. |
| Named views, multiple viewports (2/4 view) | ⬜ | |
| RealView / shadows / ambient occlusion | ⬜ | Out of phase-1 scope except basic shadows. |

## 3. Selection

| Behaviour | Status | Notes |
|---|---|---|
| Dynamic pre-selection highlight of faces/edges/vertices under the cursor | ✅ | GPU ID-buffer picking, pixel tolerance. |
| Click selects; Ctrl+click toggles; Shift+click adds | ✅ | |
| Esc clears selection | ✅ | |
| Click empty space clears selection | ✅ | |
| Box selection: left→right window (inside), right→left crossing | ✅ | Parts: edges. Sketch: entities + points. |
| Lasso selection | ⬜ | |
| Select Other (right-click) | ✅ | Lists the faces along the ray (hidden ones too) and the edge/vertex under the cursor; hover previews. |
| Selection filters (F5 toolbar, F6 toggle, E/V/X filters) | ✅ | Faces, edges, vertices, planes, sketch segments, axes/points. |
| Select loop / tangency / chain | ⬜ | |
| Planes hidden by default, shown when hovered/selected or when picking a sketch plane | ✅ | |
| Plane display: faint front/back-coloured fill, thin blue border, name rendered in the plane, orange 2px border + name tag on hover, 8 resize handles when selected | ✅ | Document Properties > Plane Display (colours, transparency) live; intersections pending. |
| Plane Show/Hide (context menu), auto-size to model, drag handles to resize | ✅ | Saved with the document. |

## 4. Sketching

### 4.1 General
| Behaviour | Status | Notes |
|---|---|---|
| Sketch on standard planes, reference planes and planar faces | ✅ | Face reference resolved geometrically after rebuilds. |
| Entering a sketch rotates the view normal to the plane (never to the back side); option covers creation and edit; first sketch always rotates | ✅ | View is named after the standard view ("*Front"). |
| Sketch plane stays displayed (and selected on entry) while sketching | ✅ | |
| Starting a feature from a sketch swings the view to Trimetric; first solid zooms to fit | ✅ | |
| Sketch command with nothing selected → "select a plane" prompt with planes shown | ✅ | |
| Choosing a sketch tool outside a sketch starts a sketch first | ✅ | |
| Edit Sketch rolls the model back to the sketch | ✅ | |
| Exit sketch / Discard changes | ✅ | |
| Sketch-level undo/redo (Ctrl+Z/Y) | ✅ | Part-level history shares unchanged features between steps (200 steps / ~64 MB cap). Opening and closing a sketch without changes adds no step. Hide/Show and plane resizing are undoable. Undo waits while a feature PropertyManager or the Modify box is open. |
| Colour coding: under defined blue, fully defined black, over defined red | ✅ | Per entity and per point (null-space analysis). |
| Status bar shows Under/Fully/Over Defined | ✅ | DOF shown in Relations PM. |
| Sketch origin shown as red arrows | ✅ | |
| Construction geometry (dash-dot) toggle | ✅ | |
| Dragging under-defined geometry follows the cursor within constraints | ✅ | Projected-gradient constrained drag. |
| Delete entities (Delete key) cleans up orphaned relations/dimensions | ✅ | |
| Grid display and grid snap | 🟡 | Snap done; grid drawing pending. |
| Shaded Sketch Contours | ✅ | Closed areas shaded (nested areas darker); click inside to select the bounding entities, drag to move them. On by default. |
| Rapid Sketch, Instant2D | ⬜ | |
| Horizontal / Vertical Dimension tools | ✅ | |
| Contour selection (Selected Contours) | ⬜ | Group box present. |
| 3D sketch | ⬜ | |
| Block, sketch picture, derived sketch | ⬜ | |

### 4.2 Sketch entities
| Tool | Status | Notes |
|---|---|---|
| Line: click-click chaining, click-drag single line, double-click/Esc ends chain | ✅ | |
| Line: closing onto existing geometry ends the chain | ✅ | |
| Line → tangent arc by moving back to the endpoint and away, or pressing A | ✅ | |
| Insert Line PM: Orientation (As sketched/Horizontal/Vertical/Angle), For construction, Midpoint line | ✅ | Infinite length pending. |
| Centerline | ✅ | |
| Midpoint line | ✅ | |
| Corner rectangle | ✅ | Adds H/V relations. |
| Center rectangle | ✅ | Construction diagonals + midpoint relation. |
| 3 Point corner rectangle, 3 Point center rectangle, Parallelogram | ✅ | Rectangle PM: 5 type buttons, Add construction lines (From Corners / From Midpoints). |
| Circle (centre + radius), Perimeter circle | ✅ | |
| Centerpoint arc (direction follows pointer movement), Tangent arc, 3 Point arc | ✅ | |
| Polygon (sides, inscribed/circumscribed circle) | ✅ | |
| Straight slot, Centerpoint straight slot | ✅ | Tangent arcs, equal radii, construction centerline. |
| 3 Point arc slot, Centerpoint arc slot, Ellipse, Partial ellipse, Parabola, Conic | ⬜ | |
| Spline (through points, double-click to end) | ✅ | Tangency handles, closed splines, style spline, equation curve pending. |
| Point | ✅ | |
| Text | ⬜ | |
| Sketch fillet (corner or two lines; radius, Keep constrained corners, Dimension each fillet) | ✅ | Lines sharing a corner. |
| Sketch chamfer (angle-distance, distance-distance, equal distance) | ✅ | Corner or two lines. |
| Line midpoints: hover shows the midpoint; pick it for dimensions/relations | ✅ | Stored as a hidden point with a midpoint relation, pruned when unused. |
| Sketch origin hover/selection | ✅ | |

### 4.3 Sketch tools
| Tool | Status |
|---|---|
| Trim: power trim (drag across), trim to closest (click) | ✅ |
| Trim: corner, trim away inside/outside | ⬜ |
| Extend | ✅ | Lines and arcs to the next entity. |
| Offset entities | ✅ | Chain, reverse, bi-directional, cap ends (arcs), base/offset construction, one driving dimension plus Offset relations. 🔍 side follows Reverse, not the cursor. |
| Convert entities (model edges/faces, On Edge relations; non-circular edges become splines) | ✅ | Intersection curve pending. |
| Mirror entities | ✅ | Copy option, symmetric relations, pre-selected centerline used as the mirror line. Dynamic mirror pending. |
| Linear / circular sketch pattern | ✅ | Two directions, angle, equal spacing; equal relations. Instances to skip and editable patterns pending. |
| Move / Copy / Rotate / Scale entities | ✅ | From/To or X/Y, Keep relations. Stretch pending. |
| Split entities, Jog line, Segment | ⬜ |
| Repair sketch, Check sketch for feature | ⬜ |

### 4.4 Inferencing and snaps (SW help "Sketch Snaps")
| Snap | Status | Notes |
|---|---|---|
| End points and sketch points | ✅ | Merges points (shared vertex). |
| Center points | ✅ | |
| Midpoints | ✅ | Adds Midpoint relation. |
| Quadrant points | ✅ | |
| Intersections | ✅ | Adds Intersection relation. |
| Nearest (on curve) | ✅ | Adds Coincident relation. |
| Tangent | 🟡 | Tangent arc tools only. |
| Perpendicular / Parallel (to previous chained line) | ✅ | Adds relation. Arbitrary woken lines pending. |
| Horizontal / vertical lines | ✅ | Blue inference line, adds relation. |
| Horizontal / vertical to points ("woken" points) | ✅ | Orange inference line, no relation (🔍 colour semantics). |
| Length snaps (hold Shift) | ⬜ | |
| Grid | ✅ | Off by default (as SW). |
| Angle snap | ⬜ | Option present. |
| Ctrl suppresses snaps and automatic relations | ✅ | |
| Clearing Enable snapping also clears Automatic Relations | ✅ | |
| Pointer feedback: pencil + tool glyph + yellow relation badges + live L/A/R values | ✅ | |
| Quick Snaps menu (right-click) | ⬜ | |

### 4.5 Relations
| Relation | Status |
|---|---|
| Horizontal, Vertical (lines and points) | ✅ |
| Coincident, Midpoint, Intersection | ✅ |
| Parallel, Perpendicular, Collinear | ✅ |
| Equal (lines, arcs/circles) | ✅ |
| Tangent (line–arc, arc–arc inner/outer) | ✅ |
| Concentric, Coradial | ✅ |
| Symmetric (about a line) | ✅ |
| Fix | ✅ |
| Merge points | ✅ (implicit when snapping to a point) |
| Pierce, Along X/Y/Z, Parallel YZ/ZX, Fix length/angle, Equal curve length | ⬜ |
| Relation glyphs on screen (View > Hide/Show > Sketch Relations) | ✅ |
| Entity PropertyManager (Line Properties, Circle, Arc, Point): Existing Relations, Add Relations, Options, Parameters, Additional Parameters | ✅ |
| Add Relations PM driven by selection | ✅ |
| Display/Delete Relations PM | ✅ |
| Over-defining relation flagged (red), redundant relations detected | ✅ |
| SketchXpert (resolve conflicts) | ⬜ |

### 4.6 Dimensions
| Behaviour | Status | Notes |
|---|---|---|
| Smart Dimension: type inferred from selection | ✅ | Line length, point–point, point–line, parallel lines, angle, radius, diameter. Circle diameter placed beside or above/below the circle shows as a linear diameter with extension lines; elsewhere radial. 🔍 exact zone rule. |
| Placement decides aligned / horizontal / vertical | ✅ | |
| Angle quadrant chosen by placement | ✅ | |
| Modify box: value, ✓, ✗ (restore), Rebuild, wheel/arrow spin | ✅ | Value is focused and selected on placement (type immediately). Editing an existing dimension is one undo step. Reverse-sense button pending. |
| Expressions and units in value boxes (`25.4*2`, `1in`) | ✅ | |
| Input dimension value option | ✅ | |
| Over-defining dimension → driven (reference) dimension | 🟡 | Auto-converts with a message; SW's "Make Driven / Leave Driving" dialog pending. |
| Driven ↔ driving toggle | ✅ | In the Relations PM. |
| Drag dimension text | ✅ | |
| Double-click dimension to edit | ✅ | |
| Arc min/max conditions (Shift-click), arc length, ordinate/baseline/chain dims | ⬜ | |
| Equations / global variables / linking values | ⬜ | |
| Dimension names (D1@Sketch1) | 🟡 | Names stored, not yet referenceable. |

## 5. Features (parts)

| Feature | Status | Notes |
|---|---|---|
| Extruded Boss/Base: Blind, Through All, Mid Plane, Direction 2, Reverse, Merge result | ✅ | SOLIDWORKS-style preview: added (or, for cuts, removed) body in transparent yellow with yellow edges, profile sketch with its dimensions, sketch plane shown, draggable depth arrow. Fillet/chamfer previews tint new faces yellow. |
| Extrude: Up To Next/Vertex/Surface/Body, Offset From Surface, Through All - Both, From (surface/offset), Draft (outward), Thin (one/mid/two-direction), Selected contours | 🟡 | 🔍 Up To Next uses faces parallel to the sketch. Up To Body has no body selection and behaves as Through All (it passes through and out of the body; SOLIDWORKS stops at the selected body). |
| Extruded Cut (defaults into material; Through All) | ✅ | Flip side to cut pending. |
| Revolved Boss/Cut: axis from sketch line, edge or reference axis; Blind, Mid Plane, Direction 2; Thin Feature; Selected Contours | ✅ | Up To Vertex/Surface pending. |
| Fillet: constant radius on edges / faces; variable size (start/end radius) | ✅ | Face, full round, setback pending. |
| Chamfer: Angle Distance (default), Distance Distance, Equal distance, Flip direction | ✅ | Vertex, Offset Face, Face Face pending. |
| Reference plane: offset, at angle (plane + line/axis), mid plane, three points, parallel at point, normal to curve | ✅ | Type follows the references. Tangent planes and Number of planes pending. |
| Reference axis (line/edge, two planes, two points, cylindrical face, point + plane), point (arc/face centre, vertex, coordinates), coordinate system | ✅ | Drawn and pickable; usable by patterns and planes. |
| Swept (sketch or circular profile, follow path / keep normal) and Lofted (boss + cut) | ✅ | Guide curves, start/end constraints, twist pending. Boundary pending. |
| Hole Wizard: counterbore, countersink, hole, straight tap (ISO sizes and fits), custom sizing, blind/through/up to next; positions sketch | ✅ | ANSI sizes, tapered tap, cosmetic thread display pending. |
| Advanced Hole, Thread, Cosmetic thread | ⬜ | |
| Shell (faces to remove, thickness, outward) | ✅ | Multi-thickness pending. |
| Draft (neutral plane) | ✅ | Parting line / step draft pending. |
| Rib (both/one side thickness, parallel or normal to sketch, auto material side, flip) | ✅ | Lines only; draft on ribs pending. |
| Wrap, Dome, Intersect | ⬜ | |
| Linear pattern (two directions), Circular pattern (equal/instance spacing), Mirror (features or body) | ✅ | Seeds: extrudes, revolves, patterns, mirrors. Faces/bodies to pattern, instances to skip pending. |
| Curve / Sketch / Table / Fill / Variable pattern | ⬜ | Listed in the Pattern flyout. |
| Combine, Split, Move/Copy body, Delete body | ⬜ | |
| Sheet metal, Weldments, Surfaces, Mold tools | ⬜ | Phase 2. |
| Suppress / Unsuppress | ✅ | |
| Rollback | ✅ | |
| Edit feature (re-open PM with values) | ✅ | |
| Rebuild (Ctrl+B), Force rebuild (Ctrl+Q) | ✅ | Incremental; Force Rebuild clears the kernel cache. In a sketch, Rebuild exits the sketch and rebuilds (SOLIDWORKS behaviour). |
| Double-click a feature (tree or face) to show its dimensions; double-click a dimension to Modify it; the model waits for Rebuild (⟳ mark in the tree, Rebuild button lit); Modify's Rebuild button regenerates at once | ✅ | Sketch dimensions and feature dimensions (extrude depth, revolve angle, fillet/chamfer sizes, shell thickness, plane offset, pattern spacing, draft angle). |
| Instant3D drag handles | ⬜ | |
| Multi-body parts | 🟡 | "Merge result" off creates a compound. |
| Configurations, design tables | ⬜ | |
| Materials, appearances | ⬜ | |

## 6. Assemblies, drawings

| Area | Status |
|---|---|
| Assemblies: insert components, mates (standard/advanced/mechanical), SmartMates, patterns, exploded views, interference detection | ⬜ |
| Drawings: sheets/formats, standard 3 view, model views, projected/section/detail/broken/crop views, dimensions, annotations, BOM, tables | ⬜ |

## 7. Files and interoperability

| Item | Status | Notes |
|---|---|---|
| Native part file (`.ocpart`, JSON, diff-friendly) | ✅ | |
| Save / Save As / Open (browser File System Access, download fallback) | ✅ | |
| Export STEP, STL | ✅ | |
| Import STEP / IGES / Parasolid / native SLDPRT | ⬜ | STEP import supported by the kernel. Reading SLDPRT would mean reverse engineering a proprietary format: not planned. |
| Export IGES, 3MF, OBJ, PDF/DXF/DWG from drawings | ⬜ | |
| Auto-recover / backup | ⬜ | |

## 8. Evaluate

| Tool | Status |
|---|---|
| Mass Properties (volume, area, centre of mass, density) | ✅ |
| Measure (distance with closest points, length, area, diameter, centre, angle, XYZ) | ✅ |
| Section properties, interference, clearance, curvature, zebra, draft analysis, undercut, thickness | ⬜ |

## 9. Options (Tools > Options)

The full SOLIDWORKS 2026 tree is reproduced (captured from a running install):
System Options: General, Drawings (Display Style, Area Hatch/Fill, Performance), Colors,
Sketch (Relations/Snaps), Display, Selection, Performance, Assemblies, External References,
Default Templates, File Locations, FeatureManager, Spin Box Increments, View,
Backup/Recover/Version, Touch, Hole Wizard/Toolbox, File Explorer,
Messages/Errors/Warnings (Dismissed Messages), Synchronize Settings, Import, Export.
Document Properties: Drafting Standard (Annotations, Dimensions, Virtual Sharps, Tables,
DimXpert), Detailing, Grid/Snap, Units, Model Display, Material Properties, Image Quality,
Sheet Metal MBD, Sheet Metal, Weldments, Plane Display, Configurations.

The dialog is schema-driven (`src/app/optionsSchema.ts`). It buffers edits until OK, has
an options search box, and gives every page the same controls. Settings with a live effect
today include:
- **View:** wheel direction, zoom to fit on standard views, arrow increment, mouse speed,
  transitions (view, view selector).
- **Sketch:** auto-rotate, entity points / arc centres, driven by default, every sketch snap,
  snap angle (default 45°).
- **Display:** hidden edges solid/dashed, shaded face highlighting, dynamic highlight,
  shaded planes, triad, breadcrumbs.
- **General:** Confirmation Corner.
- **FeatureManager:** hide/show tree items.
- **Spin box increments**, and **Messages:** auto-dismiss time.
- **Colors:** selected, dynamic highlight, sketch states, edges, dimensions, background
  mode.
- **Document Properties:** units and decimals, dimension precision / trailing zeros /
  parentheses, Grid/Snap (display + snap spacing), Image Quality (tessellation),
  Plane Display, Model Display shading colour, material density.

Every other control is stored and marked with an amber • "not yet active".

## 10. Keyboard shortcuts (defaults)

| Key | Command | Status |
|---|---|---|
| Ctrl+N / Ctrl+O / Ctrl+S | New / Open / Save | ✅ |
| Ctrl+Z / Ctrl+Y | Undo / Redo | ✅ |
| Ctrl+B / Ctrl+Q | Rebuild / Force rebuild | ✅ |
| Ctrl+R | Redraw | ✅ |
| Ctrl+1..8 | Front, Back, Left, Right, Top, Bottom, Isometric, Normal To | ✅ |
| Space | Orientation | ✅ |
| F / Z / Shift+Z | Zoom to fit / out / in | ✅ |
| Ctrl+Shift+Z | Previous view | ✅ |
| Arrows / Shift / Alt / Ctrl+arrows | Rotate / 90° / roll / pan | ✅ |
| Enter | Repeat last command | ✅ |
| S | Shortcut bar | ✅ |
| L | Line | ✅ |
| A (while sketching a line) | Toggle line/tangent arc | ✅ |
| Esc | Cancel / end chain / exit tool / clear selection | ✅ |
| Delete | Delete | ✅ |
| F9 | Toggle FeatureManager area | ✅ |
| Ctrl+F1 | Task pane | ✅ |
| F2 | Rename | 🟡 |
| G, D, C, W, R, F3, F5, F6, F10, F11, Tab, Shift+Tab | Magnifier, breadcrumbs, collapse tree, search, recent docs, quick snaps, filters, toolbars, full screen, hide/show | ⬜ |
| Ctrl+Tab, Ctrl+F6, Ctrl+W | Document switching / close | ⬜ |

## 11. Colours (System Options > Colors defaults)

| Element | Value | Status |
|---|---|---|
| Viewport background | White-to-grey gradient | ✅ |
| Dynamic highlight | Orange | ✅ 🔍 exact RGB |
| Selected item | Blue | ✅ 🔍 exact RGB |
| Sketch under / fully / over defined | Blue / Black / Red | ✅ |
| Inference lines | Blue (relation) / Orange-yellow (guide) | ✅ 🔍 |
| Driven dimension | Grey, in parentheses | ✅ |
| Relation glyphs | Small green-bordered icons | ✅ (own glyph art) |
