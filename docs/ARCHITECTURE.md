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
| `crates/*` | Native engine (Rust, `docs/OVERHAUL_PLAN.md`), not used by the app yet: `oc-doc` document model, `oc-sketch` solver, `oc-kernel` geometry kernel trait, `oc-kernel-occt` its OCCT 8.0.1 implementation (C++ shim in `cpp/`; needs `OCCT_DIR`), `oc-mesh` display meshes in the renderer's buffer layout, `oc-naming` persistent names of faces, edges and vertices and reference resolution, `oc-graph` feature DAG, regeneration cache and every feature type on the kernel trait, `oc-render` the wgpu viewport renderer. |

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
- **Native port:** `crates/oc-sketch` (Rust; WebAssembly binding in `src/sketch/wasmSolver.ts`
  with the same API) runs the same algorithm with exact (dual-number) Jacobians, per
  independent component, on sparse factorisations. Not used by the app yet (OVERHAUL_PLAN
  E2, switched over in F2).

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
Sketches refer to model vertices and edges the same way (`sketch/refs.ts`): each
reference owns fixed sketch geometry (flagged `ext`) that ordinary relations and
dimensions attach to. During the rebuild the engine re-finds the reference (a vertex as
the common end of its edges), moves that geometry and re-solves the sketch before later
features use it; the re-solved sketch is sent back and stored in the document.
This app still works that way. The native engine replaces it with persistent names
(next section); the document schema (version 2) already has room for them: an optional
`name` beside each signature, which this engine ignores (`src/core/migrate.ts` reads
version 1 and 2 files).

### Persistent names (crates/oc-naming, not used by the app yet)
- A reference must survive edits to the features before it. Face indices change with
  every rebuild and positions change with the model; what stays is where a face *came
  from*. A face's **name** is its set of **tags**, `"<featureId>/<role>[/<key>]"`:
  `f2/side/l3` (swept from sketch entity `l3` by feature `f2`), `f2/end/<loopKey>` (cap of
  a profile loop), `f5/fillet/<edgeKey>` (made from an edge), `f7/i3/f4/side/l1`
  (instance 3 of pattern `f7`). Splitting a face keeps its tags on every piece; merging
  faces unions them. An **edge** is named by the two faces it lies between, a **vertex** by
  the faces around it. Keys are a fixed FNV-1a 64 hash of names or entity ids, never of
  indices, so names are the same on every run and with any thread count.
- **Propagation** (`oc_naming::track`): every kernel operation returns its history, and
  `oc-graph` calls the kernel only through `ops.rs`, which names the result at once: birth
  tags from the feature (profile segments, caps) or from the edge, vertex or face that
  generated the face; otherwise the tags of the input faces history maps to it; otherwise
  those of an input face on the same surface; otherwise `<fid>/new/<rank>`, a history gap
  (none in the corpus). An imported body has no history: `<fid>/import/<rank>`, ranked by
  geometry.
- **Resolution** (`oc_naming::resolve`, `oc-graph` `resolve.rs`): a stored reference is a
  name plus a geometric signature. Live entities sharing a tag with the name are the
  candidates; one is the answer, several (a split) are told apart by signature. With none,
  the one face of the same feature and role stands in if there is exactly one (the end cap
  of an extrusion whose profile was redrawn), and the reference is renamed. Failing that,
  an entity at the same position (same plane for a planar face, same line for a straight
  edge) is accepted with the warning "re-attached by position"; otherwise the feature
  fails with "Missing reference". Features use only what a reference resolves to on the
  live body, never the stored signature's geometry. The part origin is its own kind of
  reference (`{ kind: "origin" }`), so a vertex reference is always model topology.
- **Legacy documents** (version 1, no names): loading turns a picked origin, which
  version 1 stored as a vertex at (0, 0, 0), into the origin kind. A reference without a
  name is found by signature, as this app does, and the rebuild reports the feature with
  the names written in (`Rebuild::heals`). The caller applies them with `oc_graph::apply_heals` inside
  `oc_doc::History::heal`: no undo step, and the document does not become dirty. The
  regeneration cache already holds the healed features, so the next rebuild is a cache hit.

### Native geometry kernel (crates/oc-kernel, not used by the app yet)
- `oc_kernel::Kernel` is the interface features and naming will use (OVERHAUL_PLAN E3). It
  names no kernel types: profiles and paths go in as plain curves, bodies come back as an
  opaque handle, and faces/edges/vertices are addressed by index in a deterministic,
  geometric order (`FaceId`, …). A Parasolid implementation must be able to satisfy it.
- Every operation returns the body plus its **history**: for every input face whether it
  was kept, modified, deleted, and which faces each edge, vertex, profile segment and cap
  generated. Persistent naming (E6) is built from it.
- Every result is validated (OCCT `BRepCheck_Analyzer` plus a positive volume per solid).
  An invalid result is healed only if healing keeps the face count, volume and area;
  otherwise the operation fails with `InvalidResult`. One tolerance policy
  (`oc_kernel::Tolerances`) holds every tolerance.
- `crates/oc-kernel-occt` implements it on OCCT 8.0.1 through a `cxx` bridge
  (`src/ffi.rs`) and a C++ shim (`cpp/`); OCCT is linked from the install named by
  `OCCT_DIR` (`build-occt.cmd`/`.sh`), statically for tests, shared for shipping
  (`docs/THIRD_PARTY.md`).

### Display meshes (crates/oc-mesh, not used by the app yet)
- `oc_mesh::build` meshes a body through the kernel at the Image Quality (the current
  viewer's formula, `Tolerances::display_mesh`) and at up to three coarser levels of
  detail (4× the chordal deviation each; a level is kept only if it has at most 3/5 of
  the previous level's triangles). The size in that formula is the union of the faces'
  boxes (`display_box`), not the exact box, which is slow on many fillets. OCCT meshes
  edges, then faces, in parallel, planar faces with Delabella (Watson's time explodes on a
  face with hundreds of holes) and the rest with Watson; if Delabella leaves a face empty
  the whole body is meshed again with Watson. `oc-mesh` packs faces and edges in parallel
  (rayon) into the renderer's buffers.
- Per level: a 20-byte vertex (`f32` position relative to the body's `f64` origin,
  octahedral `snorm16` normal, `u32` face index), a `u16` or `u32` triangle index buffer,
  and 32-byte edge segments (start, edge index, end) for screen-space wide lines; plus
  per-face vertex/index ranges and per-edge segment ranges for picking and highlighting.
  Seam and degenerate edges have no segments; vertices on full circles are flagged
  hidden. `RenderMesh::select_lod` picks a level from the screen size of a millimetre.
- Faces sharing an edge use bit-identical boundary points, the edge's polyline, so the
  mesh is closed without welding tolerances. Normals are the exact surface normals.
  Meshes depend only on the body and the options (the kernel discards OCCT's stored
  triangulations before and after meshing), so the bytes are the same run after run and
  with any thread count. `RenderMesh::to_bytes` is the blob the geometry host hands over.

### Native renderer (crates/oc-render, not used by the app yet)
- wgpu on D3D12, Vulkan or Metal. It draws a `Scene`, a plain description the app rebuilds
  from its state (display style, bodies by uploaded `MeshId`, planes, axes, points,
  coordinate systems, inactive sketches as world segments, origin, section plane, hover and
  selection), through a `Camera` that ports `camera.ts` in `f64`. Document-dependent
  visibility (which planes are shown, which sketches are consumed) stays with the caller.
- `oc-mesh` buffers are uploaded as they are: the 20-byte vertex feeds the face pipeline
  directly, edge segments are read from a storage buffer and expanded to screen-space
  quads. Face and edge highlights are one `u32` state per face and per edge in small
  storage buffers, so hover changes rewrite a few bytes.
- Positions reach the GPU relative to the camera target (`f64` subtraction first); depth is
  reversed into `Depth32FloatStencil8` (or 24-bit with stencil where that is missing), with
  the near and far planes fitted to the scene. Faces carry a depth bias so their edges win.
- Picking renders a 13×13 logical-pixel `Rg32Uint` ID window (kind and body slot, entity
  index) through a cropped projection and applies the current priority rules
  (`oc_render::pick`); `pick` waits, `pick_async` is polled on a later frame for hover.
  Select Other and the rotate-about point use a CPU ray cast over level 0.
- The 2D canvas overlay (sketch editor, dimensions, measure, box, tooltips, view name)
  stays in the webview over the viewport (F2); the origin marker and the triad, drawn on
  that canvas today, are drawn by the renderer. Plane names arrive as images.

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
