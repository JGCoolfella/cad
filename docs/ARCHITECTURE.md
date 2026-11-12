# Architecture

OpenCAD is a clean-room, from-scratch parametric CAD application. It keeps the
SOLIDWORKS user experience (layout, commands, mouse and keyboard behaviour) but not
its internals. The goals that drive the design:

1. **Never block the UI.** All B-rep work happens in a worker; the UI thread only draws
   and handles input. A slow rebuild never freezes the window.
2. **Use the hardware.** GPU picking, render-on-demand, incremental rebuild caching,
   and (next) multi-threaded kernel ops.
3. **Data first.** The document is plain JSON. Geometry is always derived from it.
   Undo is a snapshot of that JSON, so it can't drift from the model.
4. **One definition per behaviour.** Commands, shortcuts, menus and toolbars all point
   at one command registry.
5. **Same code on Windows, other desktops and the web.**

## Runtime layout

```
┌──────────────────────── UI thread (browser / Electron renderer) ─────────────────────────┐
│ React UI (ui/*)          App controller (app/app.ts)        Viewport (render/*)          │
│  menus, CommandManager,  document, selection, undo,         three.js, GPU picking,       │
│  FeatureManager, PMs,    PropertyManager state, sketch      SOLIDWORKS navigation,       │
│  dialogs                 session                            2D overlay (sketch, triad)   │
│                                │                                  │                      │
│                       Sketch editor (sketch/editor.ts) ◄──────────┘                      │
│                       tools, inference, Smart Dimension, trim, drag                      │
│                                │                                                         │
│                       2D constraint solver (sketch/solver.ts)                            │
└────────────────────────────────┼─────────────────────────────────────────────────────────┘
                                 │ postMessage (feature list → meshes + topology)
┌────────────────────────────────▼──────────────── Kernel worker (kernel/*) ───────────────┐
│ OpenCascade (OCCT, WebAssembly) via replicad. Regenerates features in order, memoised   │
│ per (feature JSON + upstream key). Meshes faces/edges, emits topology signatures.       │
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

## Source map

| Path | Responsibility |
|---|---|
| `src/core/types.ts` | Document model (features, sketches, relations, dimensions) and kernel output types. |
| `src/core/math.ts` | 2D/3D vector helpers, standard plane frames (Front = XY, Top = XZ, Right = YZ, Y up). |
| `src/core/units.ts` | Unit systems, formatting, expression parsing for numeric boxes. |
| `src/app/app.ts` | Application controller: document, selection, undo/redo, rebuild orchestration, sketch session, PropertyManager state. |
| `src/app/settings.ts` | System Options with SOLIDWORKS defaults, persisted in localStorage. |
| `src/commands/commands.ts` | Command registry. Every menu item, button, shortcut and gesture runs a command from here. |
| `src/commands/keyboard.ts` | Default keymap and key dispatch (arrows, Esc, Enter = repeat). |
| `src/sketch/solver.ts` | Geometric constraint solver + DOF / defined-state analysis. |
| `src/sketch/inference.ts` | Snaps and inference lines. |
| `src/sketch/editor.ts` | Sketch tools, selection, constrained drag, Smart Dimension, trim, overlay drawing. |
| `src/sketch/model.ts` | Sketch helpers and closed-profile (region) extraction for features. |
| `src/kernel/engine.ts` | Feature regeneration with OCCT, memo cache, meshing, export, mass properties; `handle(req)` serves every kernel request. No worker or bundler imports, so it also runs under Node. |
| `src/kernel/kernel.worker.ts` | Initialises OCCT with the bundled WASM and relays messages to `engine.handle`. |
| `src/kernel/testing.ts` | Test-only: `loadKernel()` initialises OCCT under Node so vitest can run the engine (`engine.test.ts`). |
| `src/kernel/fixtures/*` | Test-only: corpus part builder and parts; `golden.test.ts` checks them against `corpus/` (documents + per-step metrics). Re-record with `UPDATE_GOLDEN=1`. |
| `src/kernel/client.ts` | Promise RPC to the worker, coalesces rebuild requests (latest wins). |
| `src/render/camera.ts` | Orthographic/perspective camera, trackball rotate, pan, zoom-to-pointer, roll, animated orientations. |
| `src/render/viewport.ts` | Scene sync, GPU ID-buffer picking, mouse handling, highlights, planes, triad. |
| `src/ui/*` | React components that mirror the SOLIDWORKS interface. |
| `desktop/main.cjs` | Electron shell for Windows (and macOS/Linux): GPU flags, `app://` protocol, smoke test. |

## Key designs

### Constraint solver (sketch/solver.ts)
- Unknowns: free point coordinates and circle radii. Fixed points (origin, Fix relation)
  are constants, not variables.
- Each relation and dimension adds one or more scalar residual equations. Residuals are
  chosen to be well conditioned (normalised cross/dot products, signed distances).
- **Solve:** damped, minimum-norm Gauss–Newton (`Δ = -Jᵀ (J Jᵀ + μI)⁻¹ r`) with a line
  search. Minimum norm means under-defined geometry moves as little as possible, so
  adding a dimension never sends unrelated geometry flying.
- **Drag:** the drag vector is projected onto the null space of the Jacobian and then
  re-solved, in several sub-steps. Constrained geometry slides along whatever freedom
  it has left (a fixed-length line swings around its anchor).
- **Analysis:** the row-normalised Jacobian is reduced to RREF, processing equations in
  creation order. Rows that vanish are redundant, so the newest conflicting relation is
  the one flagged. A parameter is *determined* when it is a pivot column whose RREF row
  has no free-column components. This gives per-point/per-entity black vs blue
  colouring, the DOF count and over-defined detection.

### Profiles → faces
`extractProfiles` builds a half-edge graph of non-construction lines/arcs (circles are
loops on their own), prunes dangling chains, walks faces with angular ordering, keeps
the outer boundary of each connected component and nests boundaries even-odd
(outer → hole → island …). The worker turns each region into an OCCT face
(`makeFace` + `addHolesInFace` with ShapeFix orientation), orients it along the
extrusion direction, and builds prisms/revolutions.

### Topological references
Features that reference model topology (sketch on face, fillet edges) store a
geometric **signature** (surface/curve type, centre, normal, endpoints, size). After a
rebuild the worker re-finds the best match. This is robust for typical parametric edits.
The planned upgrade is history-based naming via OCCT's `BRepTools_History`
(generated/modified maps), with signatures kept as the fallback.

### Rebuild pipeline
- The worker keeps a cache keyed by a running hash of every feature's JSON. Editing
  feature *n* recomputes only *n…end*. Previews reuse the cache and are never stored.
- The client coalesces requests: while one rebuild runs, only the newest pending one
  is kept.
- Feature failures don't abort the rebuild. The failing feature reports an error, its
  input passes through unchanged, and later features continue.
- Every body a committed feature produces is checked with OCCT's `BRepCheck_Analyzer`
  (`checkResult` in `engine.ts`). OCCT occasionally returns an invalid solid without
  raising an error; such a feature fails instead, so a broken body never reaches later
  features, the screen or an export. There is no automatic repair: `ShapeFix_Solid`
  turned one such result into a valid-looking but wrong part. Previews are not checked
  (they must stay fast while dragging); the feature is checked when committed.

### Rendering and picking
- Rendering is on demand (dirty flag). An idle window does no GPU work.
- Body: one indexed mesh with per-vertex colour (face highlight = colour write to that
  face's vertex range, no material swaps). Edges: one `LineSegments`; highlighted
  edges are drawn on top as fat `LineSegments2`.
- Picking: a separate pick scene encodes `type<<20 | index` as colour. On hover only a
  13×13 px window around the cursor is rendered (`setViewOffset`) and read back.
  Priority vertex → edge → sketch → face → plane gives SOLIDWORKS-like pick tolerance
  and respects occlusion.
- Seam edges of periodic faces (cylinder seams) are hidden, as in Parasolid-based SOLIDWORKS.
- Lights are attached to the camera (SOLIDWORKS default lighting behaviour).

### Desktop
Electron 33 loads the built app from a privileged `app://` scheme with COOP/COEP headers,
so module workers, WebAssembly and SharedArrayBuffer behave as on the web. GPU switches
force the high-performance adapter and ignore the GPU blocklist. `npm run desktop`
builds and launches. `electron . --smoke=out.png` runs an end-to-end smoke test.

## Modules (extensibility)
The command registry is the seam for modules. A module contributes commands (id,
label, icon, run), CommandManager tabs and PropertyManagers, and (for kernel features) a
feature type plus its worker implementation. Simulation, rendering, CAM and the like
will be separate modules that plug in through the same registry and feature pipeline.

## Performance roadmap
- Multi-threaded OCCT build (`replicad-opencascadejs/multi`) for parallel booleans and
  meshing. The Electron shell already sends the cross-origin-isolation headers it needs.
- Several kernel workers: one for interactive previews, one for full rebuilds and
  exports.
- Mesh LOD and frustum culling for large assemblies; instanced rendering of repeated
  components; WebGPU renderer once three.js WebGPU is mature.
- Sparse Jacobians and graph decomposition in the solver for very large sketches.
