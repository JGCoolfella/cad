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
│   oc-kernel-occt   OCCT 7.x via a C++ shim (cxx crate)                           │
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
- [ ] **0.3 Torture scripts** (M) — edit scripts as data in `corpus/torture/*.json` with
  the correct expectation (TT1 sketch-on-top-face after notch added; TT2 fillet follows
  corner after resize; TT3 shell keeps top open after height change; TT4 pocket fillets
  follow moved pocket; TT5 deleted boss gives `Missing reference`; TT6 determinism;
  TT7 legacy document). Record which fail on the current app (expected: most).
- [ ] **0.4 OCCT failure inventory** (S) — run the corpus plus 20 deliberately hard cases
  (fillet radius near face width, fillets meeting at a vertex, shell of filleted body,
  draft on curved faces, coplanar booleans). Record failures in section 8. This is the
  evidence for the Parasolid decision.

### Phase P: spikes (decision gates; throwaway code in `spikes/`)

- [ ] **P1 Native OCCT toolchain** (M) — Cargo workspace; build a pinned OCCT 7.x from
  source with CMake (minimal modules: Foundation, Modeling, DataExchange) on Windows
  (MSVC), macOS (arm64 + x86_64), Linux in GitHub Actions with build caching; `cxx` shim
  exposing make-box, cut, fillet with history. Pass: box − cylinder test with history
  passes on all three OSes in CI; cold build time and cached build time recorded.
- [ ] **P2 Shell** (M) — Tauri 2 window, wgpu viewport drawing a 1M-triangle mesh, the
  React UI in a transparent webview on top. Pass, on all three OSes: viewport mouse
  input reaches Rust with < 8 ms added latency at 120 Hz; React menus/tooltips/
  PropertyManagers render over the viewport; resize and HiDPI and multi-monitor correct;
  no flicker. Fail → adopt the Electron fallback (section 1) and record why.
- [ ] **P3 Geometry host** (S) — child process running OCCT, shared-memory mesh transfer
  of 1M triangles, kill-and-restart on timeout. Pass: transfer < 20 ms; a forced crash
  is reported as an error and the next request succeeds.
- [ ] **P4 Renderer baseline** (M) — wgpu: 10k instances of 100 distinct meshes, ID
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
- Rust (`cargo`) is **not installed** on the dev machine; P1 installs it.
- Not verified, check in the spikes: Tauri transparent-webview behaviour per OS, wgpu
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
