# OpenCAD

A modern, from-scratch parametric 3D CAD application with a **familiar, industry-standard
user experience**: the layout, commands, mouse and keyboard behaviour that professional CAD
users already know. It is built
on a clean architecture with no legacy code. It runs as a Windows desktop app (Electron)
and in the browser.

![status](https://img.shields.io/badge/status-v0.1%20foundation-blue)

## What works today
- Familiar CAD interface: menus, CommandManager, FeatureManager tree, PropertyManagers,
  confirmation corner, heads-up view toolbar, status bar, shortcut bar (S), mouse gestures,
  spacebar orientation dialog.
- Standard view manipulation: middle-drag rotate, Ctrl pan, Shift zoom, wheel
  zoom-to-pointer, middle-click an entity to rotate about it, middle double-click to fit,
  Ctrl+1…8 views, arrow keys, animated transitions.
- Sketcher with inference lines, snaps (endpoint, midpoint, centre, quadrant,
  intersection, nearest, H/V, perpendicular/parallel), automatic relations, the full
  relation set, Smart Dimension with the Modify box, trim, constrained dragging, and
  under/fully/over defined colouring.
- Features: Extruded Boss/Cut (blind, through all, mid plane, 2 directions), Revolved
  Boss/Cut, Fillet, Chamfer, reference planes, sketch on faces, rollback, suppress, edit.
- Native file save/open, STEP/STL export, mass properties.

See [docs/FEATURE_SPEC.md](docs/FEATURE_SPEC.md) for the full parity checklist and
[docs/ROADMAP.md](docs/ROADMAP.md) for what's next.

## Run

```bash
npm install
npm run dev          # browser: http://localhost:5173
npm run desktop      # Windows desktop app (builds, then launches Electron)
npm test             # unit tests (solver, profiles)
```

## Architecture
The UI runs on the main thread. Geometry (OpenCascade B-rep, compiled to WebAssembly) runs
in a worker with incremental rebuild caching, so the interface never freezes. A custom 2D
constraint solver drives the sketcher. Details: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Legal
OpenCAD is an independent project. All icons and artwork are original. The geometry kernel is Open CASCADE Technology (LGPL-2.1), used
through replicad (MIT).
