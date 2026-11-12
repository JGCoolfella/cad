OpenCAD is a from-scratch parametric 3D CAD application with a SOLIDWORKS-style workflow.

**Try it in the browser:** https://cad.greendealer.co.nz

## Downloads
- **Windows:** `OpenCAD-0.4.2-win-x64.exe` (installer)
- **macOS (Apple silicon and Intel):** `OpenCAD-0.4.2-mac-universal.dmg`

These builds are not code-signed yet:
- **Windows:** SmartScreen may say "Windows protected your PC". Click **More info → Run anyway**.
- **macOS:** after dragging OpenCAD to Applications, right-click it and choose **Open** the first time. On macOS 15 and later, if it is still blocked, go to **System Settings → Privacy & Security** and click **Open Anyway**.

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
