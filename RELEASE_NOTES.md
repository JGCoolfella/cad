OpenCAD is a from-scratch parametric 3D CAD application with a SOLIDWORKS-style workflow.

**Try it in the browser:** https://cad.greendealer.co.nz

## Downloads
- **Windows:** `OpenCAD-0.6.0-win-x64-setup.exe` (installer). New in 0.6.0: assemblies (below).
- **macOS:** no installer for this release. Use the browser version, or the 0.4.2 `.dmg`.
- `occt-8.0.1-source.tar.gz`: the source of Open CASCADE 8.0.1, the geometry library the
  installer ships as replaceable DLLs (LGPL 2.1 with the Open CASCADE exception).

The installer is not code-signed yet:
- **Windows:** SmartScreen may say "Windows protected your PC". Click **More info → Run anyway**.

**Upgrading:** 0.6.0 installs over 0.5.0. From 0.4.x, see "Upgrading from 0.4.x" under 0.5.0.

## What's new in 0.6.0
**Assemblies (Windows app).** Parts can now be put together in an assembly (`.ocasm`):
- **New Assembly** and **Insert Components**: the component follows the pointer and drops
  where you click. The first component is fixed at the origin.
- **Assembly FeatureManager** with SOLIDWORKS' prefixes: (f) fixed, (-) under defined,
  (+) over defined, and a **Mates** folder.
- **Mates:** Coincident, Parallel, Perpendicular, Tangent, Concentric, Distance, Angle and
  Lock, with aligned/anti-aligned and Flip.
  - Pick two faces, edges or vertices: the **Mate pop-up toolbar** proposes the mate and
    offers only the ones that fit.
  - **SmartMates:** Alt-drag a component onto a face or edge of another to mate it.
  - Mates to the assembly's planes and origin, and to the planes and origin of components
    and sub-assemblies (from the fly-out tree).
  - A mate that would over-define the assembly is refused in the Mate PropertyManager,
    as in SOLIDWORKS; a mate that becomes conflicting after an edit is flagged in the tree,
    the newest one first.
  - **Edit Mate**, including re-picking a reference that has gone **Missing**.
- **Moving components:** drag an under-defined component and it moves within the freedom
  its mates leave. **Move Component** and **Rotate Component** (free drag, along an entity,
  by delta XYZ, about an axis) respect the mates.
- **Components:** Hide/Show, Fix/Float, Suppress, rename and appearance from the context menu.
- **Open Part** (or a sub-assembly) from the assembly, edit it, and return with the Window
  menu: mates follow the edit by persistent name. Saving the assembly saves the edited
  components too.
- **Sub-assemblies** can be inserted, mated, opened and edited.
- **Mass Properties** of the whole assembly, with each part's material.
- Identical parts share one copy of their geometry, and an assembly opens from the meshes
  saved in its file without rebuilding its parts.

Assemblies are in the Windows app only; the browser version doesn't have them yet.
Not in this release: Interference Detection, Measure in assemblies, configurations, a
"Save Modified Documents" list (edited components are saved with the assembly), and
separate windows for parts opened from an assembly (the part replaces the assembly in the
window; use the Window menu to go back).

**Other changes:**
- **Unsaved changes:** closing the program, File > New and File > Open ask
  "Save changes to <part>?" with **Save**, **Don't Save** and **Cancel**, as SOLIDWORKS does
  (0.5.0 asked OK/Cancel when closing).
- **File dialogs (Windows app):** Open, Save, Save As and STEP/STL export use the Windows
  file dialogs and remember where a file lives, which assemblies need to find their parts.

## What's new in 0.5.0
**A native engine and renderer for the Windows desktop app.** The user interface is the
same; underneath, the desktop app no longer runs the geometry kernel in WebAssembly or draws
with WebGL:
- **Geometry:** a new engine written in Rust on Open CASCADE 8.0.1 (the latest release,
  native code instead of the 7.x WebAssembly build), with the sketch solver in Rust too.
- **Graphics:** a new Direct3D 12 renderer (wgpu): display styles, highlights, planes,
  section view, the View Selector cube and picking are all drawn and picked there.
- **Persistent names for faces, edges and vertices.** References now follow the geometry
  through edits, where 0.4 sometimes re-attached them to the wrong face or edge without
  saying so:
  - a sketch on a face stays on that face when the part grows (0.4 could move it to the
    opposite face and the cut went into empty space);
  - a fillet on a corner follows the corner when the part is resized (0.4: "Fillet edges
    could not be found");
  - a shell keeps the face you picked open when the part's height changes (0.4 could
    open the other side);
  - fillets on a pocket follow the pocket when it moves;
  - when the referenced geometry is deleted, the feature says so: see "Missing" below.
- **Performance** (Windows 11, RTX 5070 Ti):
  - the 17 test parts open and rebuild in 0.8 s in total, meshing included;
  - a typical sketch + extrude rebuilds in about 5 ms (0.4.4: about 18 ms on the same machine);
  - drawing a 136 000-triangle part with edges takes under 0.5 ms a frame;
  - picking under the pointer answers in about 1.2 ms.

**Behaviour changes you will notice:**
- **Full circles have no selectable start vertex**, as in SOLIDWORKS (0.4 offered a vertex
  where a circular edge starts).
- **"Missing" references:** when an edge, face or vertex a feature uses no longer exists,
  the feature shows the error *Missing reference* (or a warning such as *Missing reference:
  1 of 4 fillet edges*, and uses the rest), and the selection box lists the item as
  **Missing**. Before, the reference silently moved to other geometry.
- **Files are saved as version 2** (`.ocpart`), which stores the persistent names. 0.5.0
  opens all 0.4 files. 0.4.x opens version 2 files but ignores the names, so its old
  behaviour applies there; a reference to the part origin is stored in a new form that
  0.4.x does not recognise.
- **Edges are drawn 1.5 device pixels wide**, a little heavier than 0.4 at 100 % display
  scaling, and continuous rather than dotted on shallow slopes.
- **Crossing box selection** (right to left) also takes an edge the box crosses between its
  end points, as SOLIDWORKS does; 0.4 missed a long straight edge crossed in the middle.
- **Edit Feature shows the yellow preview at once**, before any value changes (as
  SOLIDWORKS); 0.4 showed none until a value changed.
- **Closing a modified part asks first** ("Close OpenCAD without saving them?"), and the
  window title and taskbar show the part's name with `*` when it has unsaved changes.
- Small display differences from the new renderer: translucent View Selector facets blend
  slightly lighter over the model; the X/Y/Z letters of the reference triad are drawn as
  lines; the pre-selection highlight can trail a very fast pointer by one frame.

**Other changes:**
- The installer ships Open CASCADE's libraries as separate, replaceable DLLs with their
  licence (LGPL 2.1 with the Open CASCADE exception) and `THIRD_PARTY_NOTICES.txt` with the
  licences of every other component.
- The browser version is unchanged: it still runs the 0.4 engine in WebAssembly.

## What's new in 0.4.4
- **Instances to Skip** for Linear and Circular Pattern: open the group, click its box, and click the pink dot of any instance to leave it out (click again to bring it back). Skipped instances are listed as (i, j) with a ✕ to restore them.
- **Enter in a value box:** the first Enter applies the typed value and keeps the PropertyManager open; a second Enter is OK. Enter in the graphics area is OK straight away.
- Patterns that include fillets or chamfers now preview with the rounded instances.
- Includes everything from 0.4.3, which had no Windows installer.

## What's new in 0.4.3
- **Pattern and mirror individual features:** Linear Pattern, Circular Pattern and Mirror accept any feature that adds or removes material (extrudes, revolves, sweeps, lofts, Hole Wizard holes, ribs, patterns, mirrors), picked in the tree or by one of its faces.
  - Fillets and chamfers can be patterned together with their parent feature: they are applied again to every copy. Patterning one without its parent gives a warning.
  - The features are applied in FeatureManager order, whatever order they were picked in.

## What's new in 0.4.2
- **Enter accepts:** in a feature PropertyManager (and Add Relations, Section View, sketch Offset, Mirror, Move, Pattern…), pressing Enter in a value box or the graphics area does the same as the green check. In panels with only a close button, Enter just applies the typed value.
- **Empty sketches:** exiting a new sketch with nothing drawn removes it instead of leaving an empty sketch in the tree.

## What's new in 0.4.1
- **Sketch relations to the model:** sketch geometry can now have relations and dimensions to model edges and vertices, and follows when the model changes.
  - Snapping to a model vertex, edge midpoint or edge adds a Coincident or Midpoint relation. This also works for edges and vertices off the sketch plane, which are projected onto it.
  - Selecting a model edge or vertex inside a sketch lets Add Relations (Parallel, Collinear, …) and Smart Dimension use it.
  - If a referenced edge or vertex disappears, the sketch shows a warning.

## What's new in 0.4.0
- **Features:**
  - Extrude: all end conditions, From, Draft, Thin Feature and Selected Contours.
  - Revolve: axis from edges or axes, Direction 2, Thin Feature, Selected Contours.
  - Swept and Lofted Boss/Base and Cut, Rib, Draft, Shell.
  - Linear Pattern, Circular Pattern (a cylindrical face can be the axis) and Mirror.
  - Hole Wizard: counterbore, countersink, hole and straight tap, with ISO sizes.
  - Chamfer types (Angle Distance, Distance Distance, equal distance) and Variable Size Fillet.
  - Reference geometry: planes (offset, angle, mid, 3 points, parallel at point, normal to curve), axes, points, coordinate systems.
  - STEP import as an Imported feature.
- **Sketching:**
  - New tools: spline, ellipse, Horizontal/Vertical Dimension.
  - Sketch tools: Offset, Mirror, Move/Copy/Rotate/Scale, Linear/Circular Pattern, Extend, Chamfer, Convert Entities.
  - Shaded sketch contours.
  - The view focuses on the sketch plane when a sketch starts.
  - Scale sketch on first dimension: undimensioned geometry keeps its proportions.
  - Model edges and vertices can be selected inside a sketch, and drawing snaps to them (vertices, edge midpoints, points on edges).
  - Line midpoints highlight after a short hover instead of immediately.
- **Model and tools:**
  - Equations and global variables.
  - Materials, used by Mass Properties.
  - Measure, Section View, Zoom to Area.
  - Selection filters and Select Other.
  - Hide/Show Body, Transparency, Edit Appearance.
  - Double-click a feature to show and edit its dimensions.
- **Reliability:**
  - Invalid solids from the geometry kernel are rejected with a clear error instead of being passed on silently.
  - Feature warnings show in the FeatureManager tree.
  - A regression corpus of 17 parts, torture edit scripts and 23 known hard kernel cases run on every build.

## Included since 0.3.0
SOLIDWORKS-style interface (menus, CommandManager, FeatureManager, PropertyManagers, shortcut bar, mouse gestures), SOLIDWORKS navigation and view tools, the core sketch tools with snaps, relations and Smart Dimension, extrude/revolve/fillet/chamfer, the full Tools > Options trees, save/open, STEP and STL export, and mass properties.
