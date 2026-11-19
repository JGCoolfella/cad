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
| `src/native/*` | The native desktop app's side of the UI: Tauri IPC, kernel client, scene sender (inactive in the browser and Electron). |
| `desktop/main.cjs` | Electron shell for Windows (and macOS/Linux): GPU flags, `app://` protocol, smoke test. |
| `crates/*` | Native engine (Rust, `docs/OVERHAUL_PLAN.md`), used by the native desktop app (`oc-app`, F2a) only; the browser and Electron builds still use the TypeScript engine: `oc-doc` document model, `oc-sketch` solver, `oc-kernel` geometry kernel trait, `oc-kernel-occt` its OCCT 8.0.1 implementation (C++ shim in `cpp/`; needs `OCCT_DIR`), `oc-mesh` display meshes in the renderer's buffer layout, `oc-naming` persistent names of faces, edges and vertices and reference resolution, `oc-graph` feature DAG, regeneration cache and every feature type on the kernel trait, `oc-render` the wgpu viewport renderer, `oc-mate` the assembly mate solver, `oc-asm` assembly building, `oc-app` the native desktop app (Tauri) on all of them. |
| `src/assembly/*` | Assemblies in the native app (G1a–G1d): document types, the session (build, insert with a placement preview, drag, Move/Rotate Component, mates, undo, files, components edited from the assembly), SmartMates (`smart.ts`), Edit Part in context and in-context references (`context.ts`, `contextIds.ts`), the assembly FeatureManager (also the fly-out tree), the Mate PropertyManager and pop-up toolbar, the Move/Rotate Component and Interference Detection PropertyManagers. |

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

### Persistent names (crates/oc-naming, used by the native app)
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
  of an extrusion whose profile was redrawn), and the reference is renamed; it must be
  within 25 % of its size of where it was (G1c), except caps, which stand in wherever they
  are (G1d: SOLIDWORKS names caps by feature). Failing that,
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

### Native geometry kernel (crates/oc-kernel, used by the native app)
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

### Display meshes (crates/oc-mesh, used by the native app)
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

### Native renderer (crates/oc-render, used by the native app)
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
  Select Other, the rotate-about point and Hole Wizard positions use a CPU ray cast over
  level 0 (`raycast_faces`); box selection projects level-0 edge segments (`box_edges`).
- The 3D View Selector cube (`selector.rs`, a port of `viewSelector.ts`) is screen-space
  overlay geometry drawn over the model; its facets are picked on the CPU.
- Plain lines (edges, hidden edges, sketches, axes) are `Options::line_width` device px,
  1.5 by default (the weight of the Electron app's WebGL lines).
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

### Native desktop app (crates/oc-app, OVERHAUL_PLAN F2; the shipped Windows app since F3, 0.5.0)
- A Tauri 2 window whose client area is a wgpu swapchain (D3D12 on Windows; `OC_BACKEND`
  overrides) with the same React UI in a transparent WebView2 on top (P2). The UI detects
  the app (`src/native/tauri.ts`) and switches three things:
  - **Kernel:** `App.kernel` is `NativeKernelClient` (`src/native/engine.ts`) instead of the
    WebAssembly worker. The same requests (rebuild with preview, export, mass properties,
    measure, Convert Entities, clear cache) go over Tauri IPC to the engine thread, which
    owns OCCT 8 (`oc-kernel-occt`) and the `oc-graph` regenerator. A rebuild reply is binary
    (`pack.rs`: JSON header plus typed arrays): the `BodyMesh` the UI uses for picking,
    snapping and references is level 0 of the `oc-mesh` display mesh, with each face's and
    edge's signature and persistent name, vertex names, and the vertices SOLIDWORKS does not
    have (closed edges) marked hidden. The reply also carries heals (legacy references found
    by signature, written back by `App.applyHeals` without an undo step) and the references
    of failing features that no longer resolve (`missing`, shown as "Missing").
  - **Viewport:** `Viewport` keeps navigation and the 2D overlay in the webview, but makes
    no three.js scene and no WebGL context: the model and the View Selector are drawn, and
    picks answered, by `oc-render` (F2b). `NativeView`
    (`src/native/view.ts`) sends a scene description (bodies by mesh handle, planes with
    their name images, axes, points, coordinate systems, sketches, origin, section,
    hover/selection, display options), the camera (target, orientation, height,
    projection: the same fields as `oc_render::Camera`) and the graphics area's rectangle.
    The render thread (`view.rs`) draws the newest frame with `oc-render` into a texture the
    size of the graphics area and copies it into the swapchain at that position. Meshes go
    from the engine to the renderer through a shared store by handle; the renderer drops
    handles the UI no longer holds. Plane names are rasterised in the webview (today's
    font) and uploaded once as images.
  - **Picking:** frames are numbered; `view_pick` asks the render thread (`pick.rs`) what
    is under a point with the scene of the newest frame sent (it waits for that frame if
    the pick overtook it on the IPC) and answers with the frame it used, so plane and
    feature indices map back through that frame's lists. Hover uses `pick_async` and is
    answered when the GPU is done; clicks wait. A pick can add the faces along the ray
    (first: rotate-about point, Hole Wizard; all: Select Other) or try the View Selector's
    facets first; `view_box` is box selection. Since `pick()` is synchronous at a dozen
    call sites (the sketch editor's snaps among them), pointer events go through a queue
    (`Viewport.dispatch`): each waits for the pick at its position, then the unchanged
    handler runs and `pick()` returns that answer. Events keep their order; consecutive
    moves are coalesced; navigation drags make no picks.
  - **Sketch solver:** `setSketchSystemFactory` selects `WasmSketchSystem` (`oc-sketch`
    compiled to WebAssembly) for every solve in the app.
- `cargo run -p oc-app` loads the Vite dev server; `--features custom-protocol` embeds the
  built UI (`npm run native`). `opencad --smoke[=out.png]`, `--script=<file.js>` and
  `--corpus` run end-to-end checks through `window.opencad` (`smoke.rs`); `--corpus` opens
  every corpus part and compares the final body with the oracle.
- **Packaging (F3).** Release builds are GUI-subsystem programs (no console window); with
  `--smoke` they attach to the console they were started from and also write the
  `SMOKE {...}` line to `<screenshot>.log`. `npm run dist:native:win`
  (`scripts/dist-native.mjs`) builds the per-user NSIS installer with the Tauri CLI
  (`tauri build --no-bundle`, then `tauri bundle`) against a **shared** OCCT 8.0.1, in its
  own cargo target directory (`target/dist/cargo`). The embedded UI leaves out the
  TypeScript engine's OCCT WebAssembly. Next to `opencad.exe` it installs the OCCT DLLs the
  program imports (followed through their own imports, `scripts/native-pe.mjs`), the Visual
  C++ runtime DLLs (app-local), OCCT's licence texts and `THIRD_PARTY_NOTICES.txt`
  (`scripts/third-party-notices.mjs`, from the dependency graph).
  `npm run test:native:install` installs it into a temporary folder, runs `--smoke`,
  `--corpus` and the picking script on the installed program with a PATH holding only
  Windows' folders, and uninstalls.

### Assemblies (crates/oc-mate, crates/oc-asm; native app only, OVERHAUL_PLAN G1a–G1d)
- **Document** (`oc_doc::AssemblyDocument`, its own schema version line): components (a
  part or sub-assembly file, by path relative to the assembly plus the last absolute path),
  instances (an occurrence of a component with a placement, fixed or floating: SOLIDWORKS'
  (f)/(-) belong to occurrences) and mates. A mate reference is an occurrence path plus an
  entity: a face or edge by persistent name and signature (section "Persistent names"), a
  vertex, a standard or reference plane, axis or point, the origin, or the whole component
  (Lock). Each also stores its analytic geometry in part coordinates, which a lightweight
  build solves with.
- **File** (`.ocasm`, `oc_doc::container`): a zip with `document.json` (authoritative) and
  `cache/manifest.json` plus `cache/mesh/<hash>.ocm` (each distinct part's display mesh, by
  content hash of its canonical document, the image quality and a cache version). Written
  deterministically; any zip tool reads it. Caches can always be deleted.
- **Building** (`oc_asm::Assembler`, on the engine thread, sharing the part regenerator):
  every component file is found (relative path, then absolute, then the assembly's folder,
  then search folders) and read; each distinct part is regenerated once by `oc-graph` and
  meshed once by `oc-mesh`, or, when its hash matches the cache, shown from the cached mesh
  without regenerating (lightweight; picking its geometry resolves it). Mate references
  are found on regenerated parts by persistent name; one that no longer resolves is
  Missing; one found under a new name or with new geometry is written back without an undo
  step (as legacy feature references are). Instances are placements: `oc-render`'s
  `BodyItem::placement` draws one uploaded mesh at many transforms, and picking, ray casts
  and box selection apply it.
- **Mate solver** (`oc-mate`): each floating instance is a rigid body with six parameters,
  a translation and a rotation increment `(1, ω/2)/√(1+|ω|²/4)` on top of its pose, re-based
  after every step, so no angle convention and no gimbal lock exist. Mate equations are
  written once over `Real` (exact dual-number Jacobians); parallel directions are three
  rows `b − s·a` when solving (only the asked alignment is a solution) and the two
  components across `a` for rank analysis. The step, the analysis (RREF in mate order: DOF,
  fully defined bodies, redundant mates) and sparse factorisation are the sketch solver's.
  A mate that cannot be solved with those before it is set aside (mates are added one by
  one when the full solve fails), so the newest conflicting mate is the one reported.
- **Evaluate** (`oc_asm::evaluate`, G1c): occurrence bodies at their placements
  (`placed_body`; entities found on them by geometry, since the kernel may renumber a
  transformed body). Interference Detection pre-screens pairs by world bounding boxes and
  computes the exact B-rep common of each candidate pair with its volume; the common
  bodies are drawn with `BodyRole::Highlight`. Measure measures picked entities on the
  placed bodies.
  Dragging moves within the null space of the mate Jacobian (an explicit basis from the
  same RREF), rotation weighted as a lever length so a free body translates and a hinged
  one turns. Placing (`oc_asm::place`: Move/Rotate Component by values, rotation and
  constrained drags) puts a body at a pose and solves with it held; if that leaves a mate
  unsolved it solves again with the body free, so the mates take it to the nearest pose
  they allow.
- **UI** (`src/assembly/*`): `App.asm` holds the open assembly; the CommandManager shows
  the Assembly tab, the left pane the assembly FeatureManager, the Mate PropertyManager or
  the Move/Rotate Component PropertyManager (with the tree as the fly-out FeatureManager,
  whose planes and origins can be picked). Graphics-area selections carry the occurrence
  index as `body`. File > Open and Save in the native app use the native file dialog
  (`crates/oc-app/src/dialog.rs`), since assemblies refer to their parts by path.
- **Components edited from the assembly** (G1b): Open Part / Open Assembly makes the
  component's document the open one and keeps the assembly on `App.asmStack`; Return to
  Assembly (Window menu) goes back. Unsaved edits travel as `AssemblySession.overrides`
  (file → document text): every build and save sends them, and the engine reads them
  before the disk (`asm::Overlay`, a `Store`; a sub-assembly's document is put into its
  container so its caches still apply). Saving the assembly saves them first.
- **SmartMates** (G1d, `src/assembly/smart.ts`): an Alt-drag is an ordinary drag of the
  component (`asm_drag`) with the component drawn transparent and not pickable
  (`oc_render::BodyItem::pickable`), so picks during the drag see what is under it. Over a
  face, edge or vertex of another component the proposed mate (`proposeMate`, the Mate
  PropertyManager's defaults) is solved by a **probe build** (`asm_build` with
  `probe: true`: the engine's last build, which drags and placing use, is not replaced)
  from the drag's placements, and its placements are shown; off the target the drag's are
  shown again. Dropping commits the drag (one undo step) and opens the Mate
  PropertyManager with both references.
- **Edit Part in context** (G1d, `src/assembly/context.ts`): the part's document becomes the
  open one with the assembly on `App.asmStack` (the entry carries `context`: the instance,
  its occurrence path and placement), as for Open Part. The part is edited in its own
  coordinates; the camera is re-expressed in them on entry and back on return
  (`viewHooks.transformView`), so nothing moves on screen. `NativeView` adds the other
  occurrences as bodies from `CTX_BODY` (placed by `inverse(context) ∘ occurrence`), whose
  picks are selection items with that `body`; the viewport lets only plain selection
  (Sketch) and Convert Entities take them. **In-context references** are kept with the
  part (`PlaneRef` / `SketchRef` kind `external`): the referenced entity as a mate stores
  it (persistent name and signature in its own part's coordinates), where it came from
  (assembly file, its occurrence, this part's occurrence) and its geometry in the part's
  coordinates as last updated. `oc-graph` rebuilds from that stored geometry and reads no
  body for it, so the part rebuilds the same without the assembly. After each assembly
  build, parts flagged `external` in the reply are updated: each entity is re-found by
  name on its occurrence (`asm_ref`, `Assembler::occurrence_entity`), placed relative to
  the part, and the part becomes an unsaved component edit if anything moved, followed by
  another build (at most three rounds).
- **Display** (G1d): `oc_render::BodyItem::style` draws a body in its own display style
  (Component Display, `oc_doc::Instance::display`; a real Wireframe for Isolate and
  Interference Detection's Non-interfering Components); `Instance::transparent` is Change
  Transparency. Named display states belong with configurations (G1e).

### Electron app (frozen since F3)
The Electron shell and the TypeScript engine get bug fixes only; new modelling goes into the
Rust crates. They stay buildable (`npm run desktop`, `electron . --smoke`) and the browser
build keeps using the TypeScript engine until W1.

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
