# Roadmap

Phases are ordered by what makes the tool usable for real parts soonest. Each item maps
to rows in [FEATURE_SPEC.md](FEATURE_SPEC.md).

## Phase 0: foundation (this release, v0.1)
- App shell mirroring the SOLIDWORKS layout; command registry; default shortcuts.
- Viewport with SOLIDWORKS navigation, GPU picking, display styles, orientations.
- Sketcher: line/centerline/rectangles/circle/arcs/point, inference and snaps,
  automatic relations, full relation set, Smart Dimension, trim, constraint solver with
  defined-state colouring.
- Features: extrude/cut (blind, through all, mid plane, two directions), revolve/cut,
  fillet, chamfer, offset reference plane. Sketch on faces. Rollback, suppress, edit.
- Files: native JSON part, STEP/STL export. Electron Windows shell.

## Phase 1: everyday part modelling
1. Sketch: polygon, slots, ellipse, spline, sketch fillet/chamfer, offset, convert
   entities, mirror, sketch patterns, move/copy/rotate/scale, extend, text.
2. Line PropertyManager parameters, length snaps (Shift), angle snaps, Quick Snaps menu.
3. "Make dimension driven?" dialog, SketchXpert, equations and global variables.
4. Extrude: Up To Next/Vertex/Surface/Body, offset from surface, draft, thin features,
   contour selection.
5. Hole Wizard (with standards tables), shell, draft, rib, linear/circular patterns,
   mirror, swept, lofted, boundary.
6. Measure tool, selection filters, Select Other, lasso.
7. Instant3D drag handles, dimension display on double-clicking a feature.
8. Persistent naming upgrade (OCCT history + signature fallback).
9. Materials and appearances; configurations.
10. STEP/IGES import; packaged Windows installer (electron-builder, code-signed).

## Phase 2: assemblies
Component insertion, standard and advanced mates with an assembly solver, SmartMates,
patterns, exploded views, interference detection, large-assembly performance work
(instancing, LOD, lightweight components).

## Phase 3: drawings
Sheets and formats, standard 3 view, projected/section/detail views (exact HLR on the
kernel), dimensions/annotations, BOM, DXF/PDF export.

## Phase 4: modules
Sheet metal, weldments, surfaces, mold tools; then optional simulation/render/CAM modules
through the module API.

## Verification backlog (🔍 items)
Items in FEATURE_SPEC.md marked 🔍 need checking against a running SOLIDWORKS install:
wheel default direction, rotation speed, Shift+middle zoom direction, default gesture
assignments, exact highlight RGBs, inference-line colour semantics. Each check should
end in a test or a constant in `app/settings.ts`.
