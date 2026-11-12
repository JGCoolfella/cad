# Overhaul plan: native engine

Revised 2026-11-12. Supersedes the first version of this file (which kept the
TypeScript + OCCT-in-WebAssembly stack).

**Owner's requirements** (the yardstick for every decision below):
1. Robust and mathematically sound, as good as SOLIDWORKS: engine, structure, physics,
   calculations, rendering. Not a look-alike shell over a mess.
2. Scales: large parts, large assemblies, long feature trees.
3. Uses native hardware properly: NVIDIA, AMD, Intel, Apple GPUs and all CPU cores.
4. Multi-platform: Windows, macOS, Linux.
5. Browser version possible, very low priority.
6. Language does not matter; the result does.

Execution model: work packages sized for one Claude Opus 5.5 session at high effort.
The judgement calls are made in sections 1 to 4; packages are mechanical against those
contracts and their tests.

**How to use this file:** read sections 1 to 4 once, then do the next unchecked package
in section 6 following the session protocol in section 5.

---

## 1. Decisions

| Area | Decision | Why |
|---|---|---|
| Engine language | **Rust** (2021 edition, stable toolchain) | Native speed, safe threading (`rayon`), one build tool on 3 OSes, compiles to WASM for the low-priority browser build. C++ would also work; Rust wins on concurrency safety and on the browser path. |
| Geometry kernel | **OCCT, native build, behind a `Kernel` trait.** Parasolid evaluated in parallel. | No new kernel: every serious CAD product licenses a kernel that took decades. OCCT is the best open one. Parasolid (SOLIDWORKS' kernel, C API) is the only route to SOLIDWORKS-grade fillet/shell/boolean robustness, at a licence cost. The trait is designed around what both provide so the swap is a contained project. |
| Kernel isolation | **Kernel runs in a separate "geometry host" process pool**, not in the UI process | OCCT can crash or hang on bad geometry. A crashed or timed-out host is restarted and the feature reports an error; the app and document survive. Separate processes also let independent parts and bodies regenerate truly in parallel without shared kernel global state. |
| Renderer | **wgpu** (Rust) on D3D12 / Vulkan (Windows), Metal (macOS), Vulkan (Linux); WebGPU in the browser | One renderer on every GPU vendor through each platform's native API. Vendor-specific features (multi-draw-indirect-count, ray queries) used when the adapter reports them, with fallbacks. |
| Vendor compute | **Optional accelerators**, never required: CUDA / cuSPARSE / OptiX (NVIDIA), HIP / rocSPARSE (AMD), oneAPI / Embree (Intel, any x86) | macOS has none of CUDA/HIP, so every GPU path has a CPU path with identical results within tolerance. |
| App shell and UI | **Tauri 2 with the existing React UI** over a native wgpu viewport. Gated on spike P2; fallback below. | Keeps the large SOLIDWORKS-faithful UI investment. Fallback if P2 fails: Electron + React, renderer as Rust wgpu compiled to WASM on WebGPU (Dawn, which itself runs on D3D12/Vulkan/Metal), engine as a native addon. The fallback loses hardware ray tracing and some vendor extensions. |
| Sketch solver | **Rust crate, compiled native and to WASM** | The sketch editor stays in TypeScript for its UI logic but calls the solver in-process (WASM inside the webview) so dragging never waits on IPC. Same code, same results, both sides. |
| Numbers | f64 everywhere in the engine. f32 only on the GPU, camera-relative, with reverse-Z depth | No jitter at large coordinates; no precision loss in modelling. |
| Document | Typed Rust model, serde-serialisable; immutable, structurally shared (`Arc` trees); edits are transactions | Cheap undo of large documents (structural sharing instead of full JSON snapshots), safe concurrent reads by renderer and graph. |
| File format | Zip container: `document.json` (authoritative, versioned, migrated) plus content-hashed caches (B-rep, tessellation) that can always be regenerated | Large assemblies open from cache in seconds; caches are never the source of truth. Current `opencad-part` v1 JSON imports. |

### What the GPU will and will not do

- **Will:** all rendering, GPU-driven culling and indirect draws for large assemblies,
  ID-buffer picking, section capping, edge and silhouette rendering, transparency (OIT),
  hardware ray-traced rendering (module), FEA linear solves and other simulation (module),
  possibly interference/clearance screening on tessellations.
- **Will not:** B-rep modelling (booleans, fillets, shells). No production kernel does
  this on GPU. Modelling speed comes from multi-core: parallel booleans inside OCCT,
  parallel regeneration of independent features, bodies and parts, parallel meshing.

### Honest limits

- **OCCT is the robustness ceiling until a commercial kernel is licensed.** On hard
  geometry (variable fillets meeting at corners, shells of complex blends) Parasolid is
  better. Track 0's golden corpus measures where OCCT fails on this app's feature set;
  that list is the evidence for the Parasolid decision.
- This is a multi-year scope. The plan keeps the current app working and shipping while
  the engine is built, and switches over only when the new engine passes parity.

### Claims in the research not to treat as specification

- "Dogleg strictly requires a square system": false in general.
- "Arris" kernel, "byte-identical" cross-platform results, AP242 edition identifiers:
  unverified. Check primary sources before relying on them.
- Graph-isomorphism naming (Ullmann): not used. History-derived names are deterministic
  and cheaper (section 3).

---

## 2. Target architecture

```
┌──────────────── App process (Tauri) ─────────────────────────────────────────────┐
│ React UI (webview, transparent over viewport)                                     │
│   commands, FeatureManager, PropertyManagers, sketch editor (TS)                  │
│   └─ oc-sketch (Rust → WASM) for interactive solve/drag                           │
│                ▲ typed IPC (commands, events, mesh handles)                       │
│ Rust core                                                                          │
│   oc-doc      document model, transactions, undo, file I/O, migrations           │
│   oc-graph    feature DAG, incremental + parallel regeneration, cancellation     │
│   oc-naming   persistent topological names, reference resolution                 │
│   oc-render   wgpu renderer: scene, GPU picking, edges, sections, LOD            │
│                ▲ shared memory (meshes, B-rep blobs) + message channel            │
├─────────────── Geometry host processes (pool, restartable) ──────────────────────┤
│   oc-kernel        Kernel trait: ops with history, validation, tolerances        │
│   oc-kernel-occt   OCCT 8.0.x via a C++ shim (cxx crate)                         │
│   (oc-kernel-parasolid later, via the PK C API)                                   │
│   oc-mesh          parallel tessellation, LOD                                     │
└───────────────────────────────────────────────────────────────────────────────────┘
Modules (later): oc-sim (mass props, interference, motion, FEA), oc-raytrace, drawings (HLR)
Browser (later): same crates → WASM, wgpu → WebGPU, kernel host → Web Worker.
```

Rules that keep it from becoming a mess:
- **One-way dependencies:** `doc ← graph ← naming ← kernel trait`; renderer reads
  snapshots, never mutates the document; UI talks only through commands.
- **Every kernel result is validated** (`BRepCheck_Analyzer`) and healed (`ShapeFix`)
  inside the kernel crate; invalid results are feature errors, never silently passed on.
- **One tolerance policy**, in `oc-kernel`: modelling tolerance, fuzzy boolean value,
  angular tolerance, tessellation tolerances. No literals elsewhere.
- **Determinism:** same document, same names, same mesh, any thread count, cold or warm
  cache. Tested.
- **No module over ~600 lines**, no crate cycles (`cargo deny`/`cargo udeps` in CI).
- **CI on all three OSes** for every commit: build, tests, golden parity, clippy deny warnings.

---

## 3. Design: persistent topological names

Contract for `oc-naming`. Implement as written; if something proves impossible, record
why in section 8 and stop rather than improvising another scheme. Natively, OCCT's full
history API is available (`BRepTools_History`, `Modified`, `Generated`, `IsDeleted` on
every builder), which the WASM build lacked.

### 3.1 Model

Only **faces** carry history. Edges and vertices are named by the faces that meet at them.

```rust
/// One origin of a face: "<featureId>/<role>[/<key>]".
pub struct Tag(Arc<str>);
/// A face's name: sorted, de-duplicated tags. A merged face carries several.
pub struct FaceName(SmallVec<[Tag; 2]>);
/// Names aligned with the kernel's face order for one body.
pub struct BodyNames { pub faces: Vec<FaceName> }
```

| Tag | Produced by | Key |
|---|---|---|
| `<fid>/side/<entityId>` | extrude, revolve, sweep: face swept from a sketch entity | sketch entity id |
| `<fid>/start/<loopKey>`, `<fid>/end/<loopKey>` | caps | hash of sorted entity ids of the loop |
| `<fid>/fillet/<edgeKey>`, `<fid>/chamfer/<edgeKey>` | face generated from an edge | hash of the edge's two face names |
| `<fid>/corner/<vertexKey>` | fillet corner patch | hash of the vertex's face names |
| `<fid>/i<n>/<seedTag>` | pattern or mirror instance `n` | seed tag |
| `<fid>/new/<rank>` | no traceable origin | rank in a sort by (surface type, rounded centre, rounded area) |

Splits keep the parent's tags on every piece; merges union them. Edge name = unordered
pair of adjacent face names. Vertex name = set of face names around it.

### 3.2 Propagation

Every kernel operation returns `(shape, history)`. `oc-naming::track` assigns each result
face, in order: an explicit birth tag from the operation (prism sides and caps, fillet
faces) → union of tags of input faces that history says map to it → surface identity
with an input face (same underlying surface, overlapping bounds) → `<fid>/new/<rank>`.
The corpus target is zero `/new/` tags on every operation the kernel trait exposes.

### 3.3 References

Stored references carry the name and a geometric signature (surface/curve type, centre,
normal, endpoints, size). Resolution:
1. Candidates sharing a tag with the stored name. One: done.
2. Several (split): best signature score among them.
3. None: strict signature match (same type; planes coplanar with the same normal; others
   within 25% of √area). Accept with warning "re-attached by position", else error
   `Missing reference`, shown in the tree and as `**Missing**` in the feature's selection
   box for re-picking (mark 🔍 in FEATURE_SPEC until wording is confirmed).
4. Legacy (imported v1 document, no name): loose signature match, then the resolved name
   is written back as a non-undoable, non-dirtying heal.

The kernel never reads geometry from a stored signature; it always resolves to live
topology first.

### 3.4 Determinism

No tag depends on face index, pointer, hash-map iteration order or thread scheduling.
Hash keys: a fixed, documented 64-bit hash (e.g. FxHash or xxHash with fixed seed).

---

## 4. Other key designs

**Constraint solver (`oc-sketch`).** Port of the current solver's behaviour: minimum-norm
damped Gauss-Newton (LM in dual form), null-space drag, RREF rank analysis for DOF,
black/blue/red state and the newest conflicting relation. Additions: analytic Jacobians
via forward-mode dual numbers (no finite differences), connected-component decomposition,
sparse rows, adaptive damping (Nielsen). Assembly mates (later) reuse the same core in 3D
with rigid-body parameters.

**Regeneration (`oc-graph`).** Features form a DAG (sketch → extrude → fillet; reference
edges add dependencies). Each node's result is cached by hash of (node data, input
results' hashes). Independent nodes regenerate in parallel across geometry hosts.
Every job is cancellable; a newer edit cancels stale jobs. Per-node timings recorded.

**Rendering (`oc-render`).** GPU-resident scene: geometry deduplicated by content hash,
instances as transforms. Phase 1: indexed draws, ID-buffer picking (R32Uint, async
readback for hover, synchronous for click), exact edge rendering (screen-space wide
lines, depth bias, hidden-line modes), silhouettes, section planes with stencil capping,
MSAA, PBR with image-based lighting, SSAO, order-independent transparency, render on
demand. Phase 2 (assemblies): GPU frustum + Hi-Z occlusion culling into indirect draw
buffers, multi-draw-indirect-count where supported, LOD, streaming. Optional hardware ray
traced mode via ray queries where the adapter supports them.

**Simulation (`oc-sim`, modules).** Exact mass properties from the kernel (already
needed for parity). Interference detection (exact on B-rep, GPU screening on meshes for
large assemblies). Motion: multibody with the mate solver. Linear static FEA: tet meshing
(Netgen, LGPL; Gmsh is GPL, use only if the app's licence allows), sparse solve on CPU
(faer or MKL PARDISO) with optional cuDSS/AMGX (NVIDIA) and rocSPARSE/rocALUTION (AMD),
results validated against analytic benchmarks (cantilever, plate with hole, NAFEMS LE1).

**Licensing note.** OCCT is LGPL 2.1 with exception: link dynamically and ship its
licence. Check every new dependency's licence before adding it; record in
`docs/THIRD_PARTY.md`.

---

## 5. Session protocol (Opus 5.5, high effort)

One work package per session, from a clean tree, in dependency order.

1. Read `CLAUDE.md`, `docs/ARCHITECTURE.md`, sections 1 to 5 of this file and the
   package. Read only the files the package names, plus what they import.
2. Write the package's tests first; see them fail for the right reason.
3. Implement inside the package's file list. Needing a file outside it is a finding for
   section 8, not licence to widen the change.
4. Verify: `cargo test --workspace`, `cargo clippy -- -D warnings` for Rust; `npm run
   typecheck` and `npm test` for TS; packages marked **smoke** also run the app.
5. Tick the package, add a dated entry to section 8 (measurements, deviations, quirks),
   update `ARCHITECTURE.md` / `FEATURE_SPEC.md` if behaviour changed, commit with the
   package id in the subject.

Rules:
- **Never loosen a golden, parity or torture assertion to make it pass.** If an expected
  value is wrong, explain in section 8 and change it in its own commit.
- Pure moves and behaviour changes in separate commits.
- Evidence-gated packages: measure first; below the threshold, record and skip.
- **Spikes are throwaway.** Their deliverable is a written verdict in section 8 against
  stated pass criteria, not production code.
- Stop and write up when a contract here can't be met, a fact in section 7 is wrong, or a
  test fails after three distinct fix attempts.
- Don't touch deployment, signing or release workflows without asking the owner.

Suggested stronger-model checkpoints (diff review only): after P2 (shell verdict), after
E3 (kernel trait), after E6 (naming), after F3 (switch-over).

---

## 6. Work packages

Sizes: S < ~300 changed lines, M < ~800, L above. Phases run in order; tracks within a
phase can run in parallel sessions when they touch disjoint files.

### Phase 0: oracle in the current app (TypeScript; keeps shipping)

The current app becomes the reference the new engine must match.

- [x] **0.1 Node kernel harness** (M) — move all of `src/kernel/kernel.worker.ts` except
  OCCT init and `onmessage` into `src/kernel/engine.ts`; add `src/kernel/testing.ts`
  (OCCT under Node, see section 7) and a box-with-hole test (7 faces, exact volume).
  No behaviour change; smoke test passes.
- [x] **0.2 Golden corpus** (L) — `src/kernel/fixtures/build.ts` builder producing
  `PartDocument`s via real helpers (no hand-written JSON); ≥14 parts covering every
  `Feature` type and `EndConditionType`, thin features, selected contours,
  sketch-on-face, fillet on pocket edges, shell, draft, patterns, mirror. Export each
  part's per-step metrics (volume, area, face/edge counts, bbox, status) to
  `corpus/<part>.json` and the documents to `corpus/<part>.ocpart`. These files are the
  parity oracle for phase E/F, **except the known divergences listed in section 8**,
  which the native engine must fix rather than match.
- [x] **0.3 Torture scripts** (M) — edit scripts as data in `corpus/torture/*.json` with
  the correct expectation (TT1 sketch-on-top-face after notch added; TT2 fillet follows
  corner after resize; TT3 shell keeps top open after height change; TT4 pocket fillets
  follow moved pocket; TT5 deleted boss gives `Missing reference`; TT6 determinism;
  TT7 legacy document). Record which fail on the current app (expected: most).
- [x] **0.4 OCCT failure inventory** (S) — run the corpus plus 20 deliberately hard cases
  (fillet radius near face width, fillets meeting at a vertex, shell of filleted body,
  draft on curved faces, coplanar booleans). Record failures in section 8. This is the
  evidence for the Parasolid decision.

### Phase P: spikes (decision gates; throwaway code in `spikes/`)

- [ ] **P1 Native OCCT toolchain** (M) — **Windows passed 2026-11-13 with OCCT 8.0.1
  (section 8); CI on macOS and Linux still to do.** Cargo workspace; build a pinned OCCT from
  source with CMake (minimal modules: Foundation, Modeling, DataExchange) on Windows
  (MSVC), macOS (arm64 + x86_64), Linux in GitHub Actions with build caching; `cxx` shim
  exposing make-box, cut, fillet with history. Pass: box − cylinder test with history
  passes on all three OSes in CI; cold build time and cached build time recorded.
- [ ] **P2 Shell** (M) — **Windows passed 2026-11-13 (section 8); macOS, Linux and
  multi-monitor still to verify.** Tauri 2 window, wgpu viewport drawing a 1M-triangle mesh, the
  React UI in a transparent webview on top. Pass, on all three OSes: viewport mouse
  input reaches Rust with < 8 ms added latency at 120 Hz; React menus/tooltips/
  PropertyManagers render over the viewport; resize and HiDPI and multi-monitor correct;
  no flicker. Fail → adopt the Electron fallback (section 1) and record why.
- [x] **P3 Geometry host** (S) — child process running OCCT, shared-memory mesh transfer
  of 1M triangles, kill-and-restart on timeout. Pass: transfer < 20 ms; a forced crash
  is reported as an error and the next request succeeds.
- [x] **P4 Renderer baseline** (M) — wgpu: 10k instances of 100 distinct meshes, ID
  picking, wide-line edges, section plane with capping. Record frame times on whatever
  NVIDIA / AMD / Intel / Apple hardware is available.
- **Owner action, in parallel:** ask Siemens (Parasolid) about licensing for a small
  independent CAD product; record the answer here. Not blocking.

### Phase E: engine core (Rust) — after P1, P3

- [ ] **E1 `oc-doc`** (L) — Rust document model mirroring `src/core/types.ts`; serde
  round-trip of every corpus document byte-equivalent (after canonical formatting);
  transactions; undo/redo via structural sharing; schema version + migration hook;
  property tests (random edit sequences, undo all → original).
- [ ] **E2 `oc-sketch`** (L) — port solver with all existing `solver.test.ts` cases;
  dual-number Jacobians; component decomposition; benchmarks (N rectangles grid, N-bar
  linkage, polygon, tangent-arc chain at N = 10/50/200). Pass: matches TS solver results
  within 1e-9 on the suite; grid N=200 drag step < 2 ms native. WASM build plus a TS
  binding with the same API as `SketchSystem`.
- [ ] **E3 `oc-kernel` trait + `oc-kernel-occt`** (L) — operations the feature set needs
  (prism, revol, sweep, loft, boolean fuse/cut/common, fillet, chamfer, shell, draft,
  offset, section, measure, mass properties, STEP read/write, tessellate), each returning
  shape + history; validation and healing; tolerance policy; error types. Designed so a
  Parasolid implementation can satisfy it (no OCCT types in the trait).
  **Review checkpoint.**
- [ ] **E4 `oc-mesh`** (M) — parallel per-face tessellation, edge polylines, LOD levels,
  normals; output layout the renderer consumes directly.
- [ ] **E5 `oc-graph` + features** (L, split per feature family if large) — DAG, cache,
  parallel regeneration, cancellation, per-node timings; implement every feature type.
  Pass: every corpus part matches the phase-0 oracle (volume/area 1e-6 relative,
  topology counts exact, statuses equal) except cases recorded in section 8 with a reason.
- [ ] **E6 `oc-naming`** (L) — section 3 in full, wired into E5. Pass: all torture
  scripts TT1–TT7 pass; zero `/new/` tags across the corpus; determinism under 1, 4 and
  16 threads. **Review checkpoint.**

### Phase F: app on the new engine — after E6, P2, P4

- [ ] **F1 `oc-render`** (L, smoke) — phase-1 renderer from section 4; parity with the
  current viewport's display styles, highlights, planes, triad and picking priority.
- [ ] **F2 IPC and app wiring** (L, smoke) — Tauri commands replacing `kernel/client.ts`;
  React UI moved into the Tauri app; sketch editor on the WASM `oc-sketch`;
  `window.opencad` debug hook kept.
- [ ] **F3 Switch-over** (M, smoke) — new app passes the Electron smoke-equivalent
  end-to-end test and a manual checklist from FEATURE_SPEC; installers for the three
  OSes in the release workflow (ask the owner before changing release/signing).
  Old Electron app frozen. **Review checkpoint.**

### Phase G: scale

- [ ] **G1 Assemblies data model** — components, instances, mates (3D solver from E2's
  core), content-hash geometry sharing, lightweight loading from caches.
- [ ] **G2 GPU-driven rendering** — culling compute, indirect draws, Hi-Z, LOD selection.
  Pass: 10k-component assembly ≥ 60 fps on a mid-range GPU (record hardware).
- [ ] **G3 Parallel and out-of-core regeneration** across parts; memory budget tests.

### Phase H: modules

- [ ] **H1 Evaluate:** exact mass properties, measure, interference/clearance.
- [ ] **H2 Drawings:** exact HLR on the kernel, views, dimensions.
- [ ] **H3 FEA linear static** with CPU path, then optional CUDA/HIP solvers; validated
  against analytic and NAFEMS benchmarks.
- [ ] **H4 Ray-traced rendering** (hardware RT via ray queries; OptiX/HIP RT optional).
- [ ] **H5 Motion study** on the mate solver.
- [ ] **H6 STEP AP242 semantic PMI** (native OCCT has XCAF GD&T classes) on top of names.

### Phase W: browser (lowest priority)

- [ ] **W1** — crates to WASM, kernel host as Web Worker with OCCT's WASM build, wgpu on
  WebGPU, same React UI; features needing vendor compute disabled.

### Order

```
0.1 → 0.2 → 0.3 → 0.4
P1, P2, P3, P4 (parallel; P1 first if only one)
E1, E2 (parallel) → E3 → E4 → E5 → E6
F1 → F2 → F3 → G → H → W
```

---

## 7. Verified facts (2026-11-12)

- Current app: `replicad` 1.2.1 / `replicad-opencascadejs` 1.2.1 (OCCT in WASM).
- OCCT runs under plain Node in ~250 ms:
  ```js
  const init = require("replicad-opencascadejs");          // via createRequire
  const oc = await (init.default ?? init)({ locateFile: () => "<abs>/node_modules/replicad-opencascadejs/dist/replicad_single.wasm" });
  require("replicad").setOC(oc);
  ```
- In the WASM build, boolean builders expose `Modified` / `IsDeleted` (verified: box −
  cylinder gives 2 box faces and 1 cylinder face with one image each). `Generated` is in
  the typings, not run. `BRepTools_History`, `TNaming_*`, `XCAFDoc_DimTolTool` are not
  bound. Constructors are unsuffixed (`new oc.BRepAlgoAPI_Cut(a, b)`).
- (2026-11-13) Dev machine: Rust 1.99.1 stable MSVC (`%USERPROFILE%\.cargo\bin`, not on
  the PATH of existing shells), VS 2022 C++ tools, CMake 3.30 (bundled with VS), Ninja.
  OCCT 8.0.1 source in `C:\occt\OCCT-8.0.1`, static minimal install in
  `C:\occt\install-8.0.1` (`spikes/p1-occt/build-occt.cmd`). Crate versions used:
  wgpu 30.0.1, Tauri 2.12.5, cxx 1.x, glam 0.30.
- OCCT 8.0.1 (2026-07-30) is the current release; 7.9.3 the last 7.x. The P1 shim
  (`BRepAlgoAPI_Cut`, `BRepFilletAPI_MakeFillet`, `Modified`/`Generated`/`IsDeleted`,
  `BRepCheck_Analyzer`, `BRepMesh_IncrementalMesh`, `Poly_Triangulation` normals)
  compiled against 8.0.1 unchanged. `Standard_Failure` reaches Rust as `Err` through a
  custom `rust::behavior::trycatch`.
- wgpu 30 API differences from older examples: `Instance::new(InstanceDescriptor::
  new_without_display_handle())`, `request_adapter`/`request_device` return `Result`,
  `PipelineLayoutDescriptor { bind_group_layouts: &[Option<&_>], immediate_size }`,
  `get_current_texture()` returns the `CurrentSurfaceTexture` enum, `queue.present(frame)`,
  `get_mapped_range()` returns `Result`. glam's `look_to_rh` needs a **unit** direction.
- Tauri 2 transparent WebView2 over a wgpu swapchain works on Windows (P2). Still not
  verified: macOS and Linux transparency, wgpu
  support for multi-draw-indirect-count and ray queries per backend, state of existing
  Rust OCCT bindings (likely too partial; plan assumes our own `cxx` shim).

---

## 8. Session log

- 2026-11-12 plan revised for native-first architecture after the owner set the
  requirements at the top. Working tree had uncommitted changes in 8 files; commit or
  stash before 0.1.
- 2026-11-12 **0.1 done.** `src/kernel/engine.ts` holds all kernel logic and exports
  `rebuild`, `finalShape`, `massProps`, `faceSig`, `edgeSig`, `errorText`, `clearCache`
  and `handle(req) → { result, transfer }`; the worker is 36 lines of init and relay.
  `src/kernel/testing.ts` `loadKernel()` resolves the WASM with
  `createRequire(import.meta.url).resolve("replicad-opencascadejs/wasm")`.
  `engine.test.ts`: box 20×10×5 with Ø6 through hole (7 faces, volume within 1e-6,
  bbox), warm-cache equality, failing feature reported without aborting, `handle`
  parity. 4 tests run in ~350 ms, OCCT init included. typecheck, 32/32 tests, build and
  Electron smoke (`faces: 7, errors: []`, RTX 5070 Ti via ANGLE D3D11) all pass.
  Deviation: the owner's uncommitted STEP-import work (`ImportedFeature`,
  `ensureImports`) was in the worker and moved into engine.ts unchanged; nothing
  committed, pending the owner's go-ahead.
- 2026-11-12 **0.2 done.** 17 parts in `src/kernel/fixtures/parts.ts`, recorded in
  `corpus/` (240 KB: `.ocpart` documents in the app's save format, `.json` per-step
  metrics, `assets/block.step` for the imported part). `golden.test.ts` checks the
  builder still produces each stored document, each stored document rebuilds to its
  stored metrics, no step errors, and every `FeatureType` and `EndConditionType` is used
  (compile-time exhaustive). Re-record: `UPDATE_GOLDEN=1 npx vitest run golden`.
  Metrics: volume, area, solids, faces, edges, bbox after every step, plus resolved
  frames/planes/axes/points/csys; 6 significant figures, |x| < 1e-7 → 0. Stable over two
  fresh runs; a 1-in-the-6th-figure change fails the test. 50 tests, ~3 s total.
  Final volumes hand-checked for block-hole, up-to, through-mid-dir2, thin-contours,
  curved-profiles, revolve, fillet-chamfer, holes.
  - Engine change: `rebuild(…, onStep?)` observer; `ensureImports` exported. No
    behaviour change.
  - Deviations: ids are deterministic per-part counters, not `uid()` (random session
    prefix would change the files every run). Sketches are drawn at exact coordinates
    without relations; the solver has its own tests. No vertex count (replicad has no
    accessor). Line endings normalised on read (Windows checkouts).
  - **Known divergence (do not match in the native engine):** Up To Body behaves as
    Through All (`through-mid-dir2` Boss-Extrude2 runs to z = −124.288; SOLIDWORKS
    stops at the selected body, and `EndCondition` has no body field). FEATURE_SPEC
    row changed ✅ → 🟡.
  - **App bug found, fixed separately after 0.2:** `src/app/app.ts:716` tested
    `sig.surface === "CYLINDER"`, but replicad/OCCT reports `"CYLINDRE"`, so a
    pre-selected cylindrical face is never used as the circular pattern axis.
  - Observed, correct: OCCT chamfers propagate along tangent edges (fillet-chamfer
    Chamfer1 runs round the whole filleted top loop), as SOLIDWORKS does. Plain "Hole"
    type uses the drill (nominal) diameter by design.
- 2026-11-12 **0.3 done.** 9 cases in `src/kernel/fixtures/torture.ts`, stored as data
  in `corpus/torture/*.json` (initial document, edits as JSON path set/remove/insert
  operations, expectations), replayed by `torture.test.ts`. Most cases compare against
  a reference: the same script built directly in the final state (same feature ids),
  checked per step (metrics, errors, sketch frames) **and** by the final face list
  (surface type + centre to 1e-3). The face list is required: a fillet on the wrong
  corner of a box, or a shell open at the wrong end of a plain box, has identical
  volume, area and topology. Mutation-checked: moving TT8's fillet to the wrong corner
  makes it fail. Each file records `tsEngine: passes|fails`; the test fails if that
  outcome changes, so fixes and regressions are never silent. 59 tests, stable.
  Results on the current engine (5 of 9 fail, 4 of them **silently**, with no error):
  - TT1a notch base profile: passes (small change, nearest-match copes).
  - TT1b pad height 10 → 40: **fails silently.** The pocket sketch re-attaches to the
    *bottom* face (normal flips to −Z), and the pocket cuts into empty space; the part
    loses its pocket with no error.
  - TT2 widen box with filleted corner: fails with "Fillet edges could not be found".
  - TT3 taller shelled box: **fails silently.** The shell opens the bottom instead of
    the top. (Needed a 5° taper to be detectable; see above.)
  - TT4 move filleted pocket: **fails silently.** Fillets land on the wrong edges
    (12 faces instead of 15).
  - TT5 delete boss under a sketch: **fails silently.** The sketch moves to the block's
    top face instead of reporting a missing reference.
  - TT6 determinism, TT7 legacy document, TT8 unrelated upstream hole (control): pass.
  - Deviation: TT1 split into TT1a/TT1b; TT8 added as a control that must keep passing.
- 2026-11-12 **0.4 done.** `src/kernel/fixtures/hard.ts`: 22 hard cases in four
  groups (fillet, shell, draft, boolean), each stating what a robust kernel should do
  (`ok`, `error`, or `either` = a clean error is acceptable but a bad solid is not) and,
  where geometry allows, the exact volume (rounded box by Minkowski sum, spherical
  corner blend, Pappus for a fillet ring, Steinmetz solid, hemisphere). A case passes
  only with the wanted outcome, a valid solid (`BRepCheck_Analyzer`, available in the
  WASM build) and the right volume (1e-4). Results in `corpus/hard/inventory.json`;
  `hard.test.ts` fails if any result changes. It also checks every step of every corpus
  part for validity.
  **Result: 19 of 22 pass; 3 OCCT failures, plus 1 found in the golden corpus.**
  - **H10, shell of a fully rounded box (1 mm, top removed): invalid solid returned
    with no error.** Topologically invalid, not a tolerance issue.
  - **Corpus `fillet-chamfer` Chamfer2 (2 × 3 mm chamfer propagated round a filleted
    loop): invalid solid, no error.** Chamfer3 inherits it. The golden metrics for these
    two steps come from a broken body: **known divergence, do not match.**
  - H11, shell 3 mm inward over r = 2 fillets: error (OCCT cannot offset past the
    collapsing fillets; SOLIDWORKS can).
  - H14, draft on a cylindrical face: error in every variant tried (both directions,
    both signs, cylinder split into two half faces), via replicad's draft wrapper on
    `BRepOffsetAPI_DraftAngle`. Not isolated further. Drafting quarter-cylinder fillet
    faces between planar faces (H15) works.
  - Passed, including the hardest analytic cases: equal and nearly equal crossing
    cylinders (singular intersection), sphere halved along its seam and poles, spherical
    corner blend, all-edge rounding, mixed-convexity vertex blend, coplanar and touching
    booleans, pocket exactly as deep as the plate. Fillets at or over half a face width
    and r larger than neighbouring edges fail cleanly (acceptable).
  - Deviation: "three different radii at a corner" replaced (H05). Successive fillet
    features can't build it: tangent propagation carries the second fillet round the
    corner. The case is now a concave edge meeting two convex edges.
  - **Conclusions.** (1) OCCT's booleans and fillets are strong on this feature set;
    its weak area is offsets (shell, draft, some chamfers), which is where Parasolid's
    advantage is usually cited. (2) The most important finding is that OCCT can return
    **invalid solids silently**, and the current app never validates kernel results, so
    a broken body is shown as if fine. Validating and healing every result (section 2)
    is confirmed as a requirement, and a cheap check in the current app is worthwhile
    before the native engine exists. (3) Evidence for Parasolid is moderate, not
    decisive: worth the licensing enquiry, not worth blocking on.
- 2026-11-12 **Follow-up to 0.4: the current app now rejects invalid kernel results.**
  `engine.checkResult` runs `BRepCheck_Analyzer` on every body a committed feature
  produces (rebuild and export/mass-properties paths alike); an invalid body fails the
  feature with "The geometry kernel produced an invalid solid…", and the previous body
  passes through. Previews are not checked (drag speed); about 9 ms per check on the
  98-face `patterns` part. The tree now also shows warnings (yellow triangle, message as
  tooltip, flash after rebuild); nothing produces them yet, but section 3.3 will.
  - **Automatic repair was tried and rejected.** `ShapeFix_Solid` (the only general
    fixer in this WASM build; `ShapeFix_Shape` is not bound) made H10 pass the checker
    by keeping the removed top face as a zero-thickness double-sided lid: valid, but
    the wrong part, with the right volume. Lesson for the native engine: healing output
    must be verified against intent (removed faces really gone, volume, face count),
    never trusted because the checker passes.
  - H10 and H23 now fail cleanly. H23 is the unequal chamfer round a filleted loop,
    moved out of the corpus. The corpus part `fillet-chamfer` was restructured so both
    unequal chamfers sit on plain vertical edges and was re-recorded. The 0.4 note
    calling its Chamfer2/3 golden values a known divergence no longer applies: the
    corpus has no invalid steps (checked by `hard.test.ts`).
- 2026-11-13 **Phase P spikes.** Code in `spikes/` (Cargo workspace, throwaway); raw
  outputs and screenshots in `spikes/results/`. One machine: Windows 11, Ryzen 7 7800X3D
  (16 threads), RTX 5070 Ti (driver 621.01), one 2560×1440 monitor at 125 %, 180 Hz.
  The AMD iGPU is disabled in Device Manager (error 22), so no AMD/Intel/Apple numbers.
  Run from `spikes/`: `OCCT_DIR=C:\occt\install-8.0.1 cargo test --release -p p1-occt`,
  `cargo run --release -p p3-host`, `cargo run --release -p p4-render -- shot.png`,
  `cargo run --release -p p2-tauri` (`P2_BACKEND=dx12|vulkan`).
  - **P1, Windows: pass.** Deviation: **OCCT 8.0.1, not 7.x.** 8.0.1 is the current
    release and the shim needed no 8.0-specific changes, so there was no reason to start
    the engine on a superseded major. The 7.9.3 fallback was not needed. Pin 8.0.1 in
    E3. Static minimal build (`spikes/p1-occt/build-occt.cmd`: Foundation, ModelingData,
    ModelingAlgorithms, DataExchange; no Tcl/Tk, FreeType, OpenGL, TBB): **cold build
    11.7 min** wall (configure + 5 576 Ninja steps + install, 16 threads), build dir
    1.9 GB, static libraries 942 MB. DataExchange pulls in TKV3d/TKService/TKVCAF
    through TKXCAF even with Visualization off; harmless, they compile without
    FreeType. Rust side: `cxx` bridge (`cpp/shim.h|.cpp`, `src/lib.rs`; `build.rs` reads
    `OCCT_DIR` and links every `TK*.lib`, with `--start-group` for Linux). Shim compile
    + link 11 s; test binary 12 MB, statically linked. `tests/history.rs` passes:
    - box 20×10×5 − cylinder r3: 7 faces, valid, volume 1000 − 45π (1e-9). History:
      top and bottom faces *modified* into exactly one face each (area 200 − 9π), the
      four sides *kept* (same TShape), tool wall *modified* into the hole face, tool caps
      *deleted*;
    - fillet r1 on the hole rim (found as the edge shared by the top and hole faces):
      8 faces, valid, volume matches Pappus (spandrel r²(1 − π/4) at radius
      3 + r(10 − 3π)/(3(4 − π))) to 1e-7. The rim edge *generates* exactly one toroidal
      face; top and hole faces are each *modified* into one face. This is the history
      section 3 needs, natively; the WASM build cannot provide it;
    - an impossible fillet (r = 50 on a 5 mm plate) comes back as `Err("fillet
      failed")`, not a crash. OCCT's `Standard_Failure` does not derive from
      `std::exception`, so the shim defines `rust::behavior::trycatch`;
    - box − cylinder + rim fillet + validity check: **3.1–3.4 ms per part**.

    **Not done: macOS and Linux in GitHub Actions**, which the pass criterion needs.
    That means pushing a workflow to GitHub, which needs the owner's OK (session
    protocol), so the cached build time is not recorded either. P1 stays unticked until
    CI is green.
  - **P2, Windows: pass.** Tauri 2.12.5 with `WebviewWindowBuilder::transparent(true)`.
    wgpu creates its surface directly on the Tauri window (`create_surface(win.clone())`)
    and renders on its own thread; WebView2 is composited on top. The self-test
    (`p2-tauri/src/main.rs`) ran twice per backend:
    - **Overlay:** screen pixels under the HTML menu are the menu colour (250,250,250),
      the menu's alpha shadow blends over the viewport, and every other pixel is the wgpu
      clear colour (0,127,127). React menus and PropertyManagers can sit over the
      viewport. Screenshot: `spikes/results/p2-overlay.png`.
    - **Input latency:** WM_MOUSEMOVE was posted to WebView2's
      `Chrome_RenderWidgetHostHWND` every 8.33 ms, like a 120 Hz mouse, and timed through
      DOM `pointermove` → `invoke` → Rust. 400/400 moves were delivered in 3 of 4 runs
      (339/400 in the first, where Chromium coalesced some). **Median 0.6–1.0 ms, p95
      1.0–6.0 ms, max ≤ 7.3 ms** over four runs, with no consistent DX12/Vulkan
      difference. That is under the 8 ms limit, but p95 has less margin than the median
      suggests. Not covered: the OS → WebView2 hop for real hardware input. Posted
      messages skip it, and a native window has the same hop.
    - **Resize:** 30 programmatic resizes, with a screen grab after each: no frame had
      anything but viewport pixels in the viewport, and the overlay was still correct
      afterwards. Interactive drag-resize was not exercised.
    - **HiDPI:** devicePixelRatio 1.25 matches the monitor and the physical surface size
      is correct. **Multi-monitor not tested** (one monitor). The webview is ready
      20–120 ms after the window opens.
    - The P2 viewport only clears; drawing load is P4's measurement. The webview covers
      the viewport, so viewport mouse input goes through it, and F1's viewport mouse
      handling will arrive over IPC. At these latencies that is acceptable.

    **Verdict: keep Tauri; no Electron fallback on Windows.** Still required before F1:
    the same self-test on macOS (WKWebView transparency) and on Linux (WebKitGTK, the
    platform most likely to fail), plus a two-monitor mixed-DPI check.
  - **P3: pass.** `p3-host` runs as both parent and `--child`. The child links OCCT
    statically; the parent does not. Control messages are lines on stdin/stdout; the
    mesh goes over a named shared-memory segment. The `shared_memory` 0.12 crate has
    been unmaintained since 2022, so the engine should own a small wrapper. Child start
    to OCCT ready: **8–9 ms**.
    - **Transfer of 1 038 472 triangles (~25 MB of positions, normals and indices):
      median 2.5 ms, max 3.0 ms** over 10 requests, against a 20 ms target. Measured from
      "ready" until the parent holds its own checksum-verified copy. The child's write
      takes 2.1–2.7 ms.
    - Forced crash (wild pointer in the child): detected in 1 ms (the pipe closes),
      restart 10 ms, retry succeeds. `abort()`: detected in 84 ms, restart 8 ms. Hang:
      detected by the 2 s request timeout, killed, restart 8 ms. The parent survived all
      three.
    - **Finding: `BRepMesh_IncrementalMesh` slows down sharply on one large face.**
      Sphere r = 50: 10k triangles take 40 ms, 122k 1.8 s, 249k 5.7 s, 509k 18.8 s and
      1.04M 66 s; time grows about as n^1.6. Meshing a face to display quality is fast;
      refining one face to 1M triangles is not. For `oc-mesh` in phase E: tessellate
      faces in parallel, choose the deflection from screen size (LOD), cache
      tessellations by content hash, and never refine a single face to that density.
      Consider a dedicated tessellator for analytic surfaces (plane, cylinder, cone,
      sphere, torus) if profiling calls for it.
  - **P4: pass (measurement package).** A headless wgpu 30 benchmark:
    - 100 meshes (spheres and tori, 138 752 unique triangles);
    - 10 000 instances sorted by mesh, drawn with 100 `draw_indexed` calls;
    - feature edges as 1.5 px screen-space quads from a storage buffer, drawn with 100
      `draw` calls;
    - 2560×1440, 4× MSAA, Depth24PlusStencil8;
    - **13.9M triangles and 3.9M edge segments per frame.**

    Results on the RTX 5070 Ti:

    | Backend | Shaded + edges (GPU / sustained) | With section and cap | CPU per frame | Pick 16×16 |
    |---|---|---|---|---|
    | Vulkan | 3.12 / 3.17 ms | 4.67 / 4.64 ms | 0.29 ms | 3.3 ms median, 4.3 max |
    | D3D12 | 3.10 / 3.15 ms | 4.70 / 4.69 ms | 0.50 ms | 3.1 ms median, 3.9 max |

    - CPU time includes recomputing all 10k instance transforms camera-relative in f64.
    - Section capping is one pass: the clipped shaded draw, a stencil parity draw (all
      instances again, depth off, stencil invert), then the cap quad where stencil ≠ 0.
      The close-up (`results/p4-section-close.jpg`) is correct. The second geometry pass
      costs 1.5 ms. Parity capping needs closed meshes and the camera on the removed
      side. Phase F should restrict the parity draw to the sectioned bodies.
    - A pick re-uploads the instances, renders a 16×16 ID window (instance id plus face or
      edge id, Rg32Uint) through a cropped projection and reads it back, all
      synchronously. It takes less than one 120 Hz frame. 28 of 50 random picks hit an
      object; the rest were background.
    - **Precision:** the same view was rendered with the scene at the origin and at
      10 km (1e7 mm). Camera-relative f32 gives **0 of 230 400 ID pixels different**;
      naive absolute f32 (what three.js does today) gives **24.2 % different**. This
      confirms the section 1 rule: f64 in the engine, camera-relative f32 on the GPU.
    - Bug caught: the first run rendered nothing, because glam's `look_to_rh` requires
      a unit direction. The precision test still "passed", because two empty images are
      equal. It now reports coverage. Rule for phase E and F tests: every image
      comparison must also assert that something was drawn.
    - Deviations: standard Z with Depth24PlusStencil8 instead of section 1's reverse-Z,
      which this scene's depth range did not need; phase F uses reverse-Z with
      Depth32Float and stencil. Multi-draw-indirect-count and ray queries were not
      probed; section 7 still lists them as unverified.
  - Next: the owner decides whether to push a CI workflow for P1 on macOS and Linux.
    P2 on macOS and Linux needs real machines, because CI runners only have virtual
    framebuffers. E1 and E2 can start now. E3 needs P1's CI to pin OCCT on every OS.
