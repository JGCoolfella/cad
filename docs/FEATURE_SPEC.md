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
| 3D View Selector (bevelled cube: 6 faces, 12 edges, 8 corners + floating panels for hidden faces) | ✅ | Shows with the Orientation flyout/dialog, Ctrl+Space alone; hover tints facet blue; click orients (respects "Zoom to fit when changing to standard views"). Native desktop app (F2b): drawn and picked by `oc-render` (`selector.rs`, a port of `viewSelector.ts`), no WebGL in the webview. |
| Ctrl+1..8: Front, Back, Left, Right, Top, Bottom, Isometric, Normal To | ✅ | |
| Normal To twice flips to the other side | ✅ | |
| Animated view transitions | ✅ | Speed option. |
| Auto-rotate normal to sketch plane on sketch creation | ✅ | Option. |
| Orthographic default, Perspective toggle | ✅ | |
| Display styles: Wireframe, HLV, HLR, Shaded with Edges, Shaded | ✅ | Also in the native renderer (`crates/oc-render`, OVERHAUL_PLAN F1), which draws the native desktop app's viewport (`crates/oc-app`, F2a). Plain lines (edges, hidden edges, sketches, axes) are 1.5 device px there (`Options::line_width`, F2b), the weight of Electron's WebGL lines; 1 px lines looked stippled under 4× MSAA. |
| Face highlight in Hidden Lines Removed / Visible and Wireframe | 🟡 | 🔍 Today's viewer shows none (its white HLR material washes the tint out). Native renderer: option `face_highlight_in_line_styles`, off by default to match. Check what SOLIDWORKS does. |
| Tangent edges: visible / as phantom / removed | ⬜ | Currently visible (SW default). |
| Silhouette edges on curved faces | ⬜ | |
| Section view (Front/Top/Right or a face/plane, offset, flip; capped) | ✅ | Multiple section planes and saving the view pending. |
| Named views, multiple viewports (2/4 view) | ⬜ | |
| RealView / shadows / ambient occlusion | ⬜ | Out of phase-1 scope except basic shadows. |

## 3. Selection

| Behaviour | Status | Notes |
|---|---|---|
| Dynamic pre-selection highlight of faces/edges/vertices under the cursor | ✅ | GPU ID-buffer picking, pixel tolerance. Native renderer (`crates/oc-render`): same priority and radii (origin 5, vertex 5, edge 4, sketch 4, face 1.5, axis/point 4, plane 1.5 px; doubled with a selection filter), smoke-tested on D3D12, Vulkan and WARP. The native desktop app picks through it (F2b): hover asynchronously, clicks waiting for the GPU, pointer events queued so each handler sees the pick at its own position (`Viewport.dispatch`). |
| A vertex where a full circle starts (closed edges, seams) is selectable | 🟡 | 🔍 Today's viewer offers one; SOLIDWORKS (Parasolid) has no vertex on a full circle. Native renderer: hidden by default (`closed_edge_vertices` option), so F2 changes behaviour unless the option is turned on. Owner decision 2026-11-15: hidden by default, as SOLIDWORKS. The native app (F2a) neither draws nor picks them (`BodyMesh.hiddenVertices`); shipped in 0.5.0 (release note). The frozen Electron app and the browser version still offer them. |
| Click selects; Ctrl+click toggles; Shift+click adds | ✅ | |
| Esc clears selection | ✅ | |
| Click empty space clears selection | ✅ | |
| Box selection: left→right window (inside), right→left crossing | ✅ | Parts: edges. Sketch: entities + points. Native desktop app (F2b): `oc-render`'s `box_edges`, whose crossing box also takes an edge it crosses between mesh points (the browser and Electron test the points only, so they miss a long straight edge crossed in the middle). |
| Lasso selection | ⬜ | |
| Select Other (right-click) | ✅ | Lists the faces along the ray (hidden ones too) and the edge/vertex under the cursor; hover previews. Native desktop app (F2b): `oc-render`'s pick and `raycast_faces`, made when the right button comes up. |
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
| References follow upstream edits; a reference whose face, edge or vertex no longer exists shows **Missing reference** on the feature in the tree and `**Missing**` in the feature's selection box for re-picking; one found again only by position shows a warning | 🟡 | Native desktop app (F2a): persistent names (`crates/oc-naming`, E6) on every pick (faces, edges, vertices), legacy references healed by name after the first rebuild without an undo step, and unresolved references of a failing or warning feature shown as bold "Missing" in its selection box (fillet and chamfer items, and every box labelled through `refName`). Owner decision 2026-11-15: keep the plain bold "Missing". Sketch-plane and sketch-reference misses show only in the tree. The Electron app (until F3): references are re-found by signature, which can silently pick the wrong face (torture cases TT1b to TT5). 🔍 wording: "Missing reference: <what>", "A reference was re-attached by position", "Missing reference: 1 of 4 fillet edges". |
| Entering a sketch rotates the view normal to the plane (never to the back side); option covers creation and edit; first sketch always rotates | ✅ | View is named after the standard view ("*Front"). Zooms to the sketch plane as drawn plus the model (an empty part fills the view with the plane). |
| Exiting a new sketch with nothing in it removes it (no feature, no undo step) | ✅ | Also when leaving it for a feature command. An existing sketch emptied by editing is kept, as in SOLIDWORKS. 🔍 |
| Model edges, vertices and faces highlight and select inside a sketch (no tool active) | ✅ | Edges/vertices win over shaded contours; faces only outside contours. |
| Scale sketch on first dimension creation (System Options > Sketch) | ✅ | Only when it is the sketch's only driving length dimension and nothing is fixed (Fix, On Edge). Scales about the sketch origin; the view zooms by the same factor so the sketch keeps its on-screen size (🔍 SW view behaviour). |
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
| Line midpoints: hover shows the midpoint; pick it for dimensions/relations | ✅ | Shown after the pointer rests on it for 250 ms (until then the line is picked). Stored as a hidden point with a midpoint relation, pruned when unused. |
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
| Snaps to model geometry: vertices, edge midpoints, nearest on edge (highlighted while snapped), in the sketch plane or anywhere on the model (projected onto the plane) | ✅ | Adds Coincident / Midpoint relations to the model geometry (external references). Curves other than lines and circles parallel to the plane snap by position only. |
| Relations and dimensions to model edges and vertices | ✅ | Selecting a model edge/vertex in a sketch selects reference geometry that Add Relations and Smart Dimension use. References are re-found by signature on every rebuild (vertices via their edges), so the sketch follows model changes; a missing reference shows a warning on the sketch. Unused references are dropped when the sketch closes. Persistent naming exists in the native engine only (plan section 3, E6), not in this app yet. |

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
| Linear pattern (two directions), Circular pattern (equal/instance spacing), Mirror (features or body) | ✅ | Features to Pattern/Mirror: any feature that adds or removes material (extrude, revolve, sweep, loft, Hole Wizard, rib, pattern, mirror), picked in the tree or by a face in the graphics area. Fillets and chamfers pattern with their parent feature: they are re-applied to the matching edges of each instance; instances without those edges are skipped with a warning. Seeds apply in FeatureManager order, whatever the pick order. Instances to Skip (linear and circular): while its box is active, a pink dot marks each instance; clicking one skips it (hollow dot) or restores it; the box lists skipped instances as (i, j) / (i) with a ✕ to restore. With fillet/chamfer seeds the preview shows the whole result so the rounded instances are visible. Instances are geometric copies (as with Geometry pattern on); re-evaluating end conditions per instance, Faces to Pattern and Bodies to Pattern (patterns) pending. |
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

| Area | Status | Notes |
|---|---|---|
| Assembly document (`.ocasm`: zip of `document.json` plus content-hashed mesh caches); New Assembly, Open, Save / Save As | 🟡 | Native app only (OVERHAUL_PLAN G1a); greyed in the browser and Electron builds. Parts stay `.ocpart` JSON. Component paths are stored relative to the assembly with the last absolute path as a fallback; opening searches relative path → absolute path → the assembly's folder → search folders (🔍 SOLIDWORKS' exact order). Lightweight open: unchanged parts are shown from the cached meshes without being regenerated; picking geometry resolves a part. |
| Insert Components | 🟡 | Native file dialog; the first component is fixed at the origin. Later ones follow the pointer (their box centre in the plane through the view's centre facing the viewer) and drop where you click; Esc cancels the insertion (G1b). 🔍 SOLIDWORKS drops at the depth of the geometry under the pointer when there is some; here always the view plane. Sub-assemblies are rigid. |
| Assembly FeatureManager: components with (f) fixed, (-) under defined, (+) over defined, no prefix fully defined; the assembly's Front/Top/Right Planes and Origin; Mates folder | 🟡 | Mates in error show the error icon; a reference that no longer resolves reads **Missing** (as in feature boxes). A component expands to its own planes and origin. While an assembly PropertyManager is open the tree is the fly-out FeatureManager in the graphics area, and its planes and origins can be picked. Configurations are not implemented. |
| Component context menu: Open Part / Open Assembly, Hide / Show Components, Fix / Float, Suppress / Unsuppress, Appearance, Remove Appearance, Move / Rotate Component, Rename tree item, Delete | 🟡 | G1b. Appearance is a colour per component occurrence (`Instance.color`), over the part's. Hidden components stay mated. Rename keeps the `<n>` instance number. Not yet: Isolate, Make Virtual, Edit Part in context, Component Properties. |
| Open Part / Open Assembly from a component, Return to Assembly | 🟡 | G1b. The component's document opens in place of the assembly (the native app has one document window); Window lists the assemblies to return to, as SOLIDWORKS' Window menu does. Edits not saved show in the assembly at once (read from memory) and mates follow them by persistent name; saving the assembly saves the edited components first. 🔍 SOLIDWORKS asks in its Save Modified Documents dialog; here they are always saved with the assembly. A part inside a sub-assembly opens its sub-assembly (open the part from there). |
| Standard mates: Coincident, Parallel, Perpendicular, Tangent, Concentric, Lock, Distance, Angle; Aligned / Anti-Aligned; Flip Dimension | 🟡 | `oc-mate`: rigid bodies on the sketch solver's core, exact Jacobians, no gimbal lock. Two picks propose Concentric (two round faces) or Coincident (else the first the pair allows) and the alignment closest to the current placement; Distance and Angle start at the current value; components move as the mate is set up. Picks: faces, edges, vertices (also of parts inside sub-assemblies), and the assembly's or a component's planes and origin from the fly-out tree. Advanced and Mechanical mates are later. 🔍 Tangent plane–cylinder side follows the alignment (anti-aligned: on the face's outer side). |
| Mate pop-up toolbar | 🟡 | G1b: opens next to the pointer once two entities are picked: the proposed mate highlighted, the other standard mates the pair allows (the rest greyed, from `oc-mate`'s rule table), the distance or angle, Flip Mate Alignment, OK and Cancel. Cancel clears the selections and keeps the PropertyManager. |
| Edit Mate; re-picking Missing references | 🟡 | G1b: Edit Feature lists the mate's references; a Missing one reads **Missing**, the PropertyManager says so and the next pick replaces it (OK stays disabled until then). Clicking an entry makes it the one the next pick replaces; × removes it. |
| SmartMates | 🟡 | G1b: Alt-drag a face, edge or vertex of a component onto one of another component: the Mate PropertyManager opens with both picked, the proposed mate previewed and the pop-up toolbar to confirm. ≈ The dragged component does not follow the pointer during the drag, and the SmartMates button of the Move Component PropertyManager (double-click to start) is not implemented. |
| Degrees of freedom; over-defined and conflicting mates | 🟡 | DOF and per-component freedom by the sketch solver's RREF. In the Mate PropertyManager, as in SOLIDWORKS, a mate that would over-define the assembly is refused: "This mate would over define the assembly. Consider deleting some of the over defining mates." (🔍 exact wording) shows in the PropertyManager, OK is disabled and the mate is not added. A mate already in the document that becomes conflicting (a part edited, a file saved so) stays in the tree with the error icon, its component shows (+), the newest one is the one reported, and the others stay solved. A mate none of whose equations adds anything over-defines; partial repeats (three coincident faces sharing rotations, two pins in two exactly spaced holes) are accepted, as SOLIDWORKS accepts them. A mate between two fixed components over-defines (🔍). |
| Dragging an under-defined component moves it within its free degrees of freedom | 🟡 | Left-drag on a face: the grabbed point follows the pointer in the plane facing the viewer; a hinged component turns about its pin, a floating one translates. One undo step per drag. |
| Move Component and Rotate Component PropertyManagers | 🟡 | G1b. Move: Free Drag, Along Assembly XYZ (the axis nearest the pointer's motion), Along Entity (a linear edge, a cylinder's axis or a plane's normal, picked), By Delta XYZ and To XYZ Position (Apply, on the selected components). Rotate: Free Drag (about the component's box centre), About Entity, By Delta XYZ (about the assembly X, then Y, then Z axes through the component's origin, 🔍). Always within the freedom the mates leave: a placement the mates allow is kept and the components mated to it follow; one they do not allow is taken back to the nearest allowed (`oc_asm::place`). Collision Detection and Physical Dynamics are greyed. 🔍 SOLIDWORKS' rotation centre for Free Drag (here the component's box centre). |
| Mates follow part edits (persistent names, section 3.3 of the plan) | ✅ | A moved hole or a thicker plate moves the mated component; references found under a new name or with new geometry are written back without an undo step. Also for edits made by Open Part and not saved yet. |
| Assembly Mass Properties | 🟡 | G1b: every component occurrence at its placement, each part with its material's density (1000 kg/m³ without one); suppressed components left out; hidden ones counted unless "Include hidden bodies/components" is cleared (🔍 its default, Options > Assemblies). Mass, volume, area, centre of mass; no inertia tensor yet (the part dialog has none either). |
| Assembly Measure | ⬜ | Measure in an assembly still measures part geometry only (G1c). |
| Interference Detection | ⬜ | Pending the owner: G1c or H1 (exact B-rep common volumes, GPU mesh screening later). Greyed. |
| Mates (advanced/mechanical), component patterns, exploded views, assembly features, BOM, configurations | ⬜ | Visible as greyed commands in the Assembly tab. |
| Drawings: sheets/formats, standard 3 view, model views, projected/section/detail/broken/crop views, dimensions, annotations, BOM, tables | ⬜ | |

## 7. Files and interoperability

| Item | Status | Notes |
|---|---|---|
| Native part file (`.ocpart`, JSON, diff-friendly) | ✅ | |
| Save / Save As / Open (browser File System Access, download fallback) | ✅ | Native app (G1a): the Windows Open and Save As dialogs and real paths for parts, assemblies and STEP/STL exports, as SOLIDWORKS; File > Open lists parts, assemblies and STEP together. Assembly export to STEP is not yet available (greyed in an assembly). |
| "Save changes to <part>?" (Save / Don't Save / Cancel) before closing the program, New or Open discards unsaved changes | ✅ | Enter saves, Esc cancels; a cancelled Save keeps the dialog. Native app: on window close too (Tauri runs no `beforeunload`). Browser and Electron close: the browser's own leave-page prompt. 🔍 Exact SOLIDWORKS wording. |
| Export STEP, STL | ✅ | Native desktop app: STEP written by OCCT 8 (`Kernel::write_step`), binary STL from the kernel's tessellation at the Image Quality. |
| Desktop app on the native engine (Windows) | ✅ | The shipped Windows app since 0.5.0 (OVERHAUL_PLAN F3): per-user NSIS installer `OpenCAD-<version>-win-x64-setup.exe` (`npm run dist:native:win`), OCCT 8.0.1 as replaceable DLLs next to `opencad.exe`, licences and `THIRD_PARTY_NOTICES.txt` installed with it; the installed program passes smoke, corpus and picking (`npm run test:native:install`). macOS packaging pending (Electron dmg until then). `crates/oc-app` (OVERHAUL_PLAN F2a): Tauri 2 window, the same React UI in a transparent WebView2 over the `oc-render` viewport (D3D12), documents regenerated by `oc-graph` on OCCT 8.0.1, sketches solved by the Rust solver in WebAssembly. Opens and regenerates all 17 corpus parts with the oracle's results (`npm run native:corpus`). Picking, hover, Select Other, the rotate-about point, box selection, sketch snaps to model geometry and the View Selector go through `oc-render` (F2b): the webview makes no WebGL context. The Electron app is frozen (bug fixes only) and no longer released on Windows. |
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
| Enter with a PropertyManager open (graphics area or any value box) | OK (green check) in PropertyManagers with OK/Cancel (features, Add Relations, Section View, sketch Offset/Mirror/Move/Pattern…). In a value box the first Enter applies the typed value and stays in the box; the next Enter (no typing in between) is OK, so from a box it takes two Enters. In sketch tool and entity property panels Enter only applies the typed value; in the Modify dialog Enter = ✓ | ✅ |
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
