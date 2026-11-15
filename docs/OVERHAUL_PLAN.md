# Overhaul plan: native engine

Revised 2026-11-12. Supersedes the first version of this file (which kept the
TypeScript + OCCT-in-WebAssembly stack).

**Owner's requirements** (the yardstick for every decision below):
1. Robust and mathematically sound, as good as SOLIDWORKS: engine, structure, physics,
   calculations, rendering. Not a look-alike shell over a mess.
2. Scales: large parts, large assemblies, long feature trees.
3. Uses native hardware properly: NVIDIA, AMD, Intel, Apple GPUs and all CPU cores.
4. Multi-platform: **Windows desktop first**; macOS desktop is a nice-to-have, expected to
   work but not a gate (decision 2026-11-13, section 1 "Desktop platforms"); Linux through
   the browser version until a Linux desktop shell is proven (section 1 "Linux desktop").
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
| App shell and UI | **Tauri 2 with the existing React UI** over a native wgpu viewport. Gated on spike P2 on Windows (passed); fallback below. | Keeps the large SOLIDWORKS-faithful UI investment. Fallback if P2 fails: Electron + React, renderer as Rust wgpu compiled to WASM on WebGPU (Dawn, which itself runs on D3D12/Vulkan/Metal), engine as a native addon. The fallback loses hardware ray tracing and some vendor extensions. |
| Desktop platforms | **Windows first (owner decision 2026-11-13).** Windows is the target every package is built and verified on. macOS is a nice-to-have: assumed to work (P2's overlay passed in macOS CI, OCCT and the engine build there) and kept building, but a real-Mac check gates nothing. | P2 passed on real Windows hardware. If a Mac problem turns up later, it is handled for macOS alone (for example the Electron fallback on macOS only); it no longer moves Windows off Tauri. |
| Linux desktop | **Deferred (owner decision 2026-11-13).** Desktop ships on Windows and macOS; Linux users use the browser version. The engine and renderer stay portable and keep building and testing on Linux in CI. | P2 showed that on WebKitGTK the wgpu swapchain draws over the transparent webview (section 8). The alternatives cost more than Linux's share of SOLIDWORKS-style users justifies: an Electron shell on Linux only means a second shell plus a WASM renderer build, and a native child window under the webview needs a compositor and ARGB visuals on X11 (Wayland subsurfaces would likely work). Revisit with a time-boxed Wayland child-surface spike if Linux demand appears. A macOS problem no longer changes this (see "Desktop platforms"). |
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

As built (E6, section 8), where the table has no row:

| Tag | Produced by | Key |
|---|---|---|
| `<fid>/mid/<loopKey>` | caps on the sketch plane of a two-direction extrude or revolve (they meet inside the body); direction 2 then ends in `start` | as for caps |
| `<fid>/offset/<faceKey>` | shell: the wall a face gains on the other side of the material | hash of the face's name |
| `<fid>/edge/<edgeKey>` | outward shell: rounded edge (the corner is `corner`) | hash of the edge's name |
| `<fid>/<wall\|cbore\|cboreFloor\|csk\|tip\|top>/<position>` | Hole Wizard faces | id of the position's sketch point |
| `<fid>/side/<line>[.a\|.b]`, `<fid>/far/<line>` | rib: face along the sketch line, at its ends, and the far face | sketch line id |
| `<fid>/side/<entity>~1\|~2\|~cap0\|~cap1` | Thin Feature walls: the two offset copies of an entity and the lines closing open ends | source entity id |
| `<fid>/side/<sketch>.<entity>` | loft: a side face carries one tag per profile | sketch feature id and entity id |
| `<fid>/clip/…` | helper body of an inclined Up To Surface (never survives) | |
| `<fid>/import/<rank>` | faces of an imported body (no history exists) | rank by (surface type, rounded centre, rounded area) |

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
   - 2a (owner decision 2026-11-14, E6 review). None, but **exactly one** live face has a
     tag of the same feature and role as the stored name (the same tag up to its key, e.g.
     `f2/end/…`): that face, by name, with no warning, as SOLIDWORKS keeps a sketch on
     the end face of an extrusion whose profile was redrawn. The reference's stored name
     is replaced by the face's current one (a heal, like step 4). For an edge or vertex, a
     stored face name with no tag-sharing face is stood in for by such a face. Two or more
     faces of that feature and role: no stand-in, go to step 3.
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

- [x] **P1 Native OCCT toolchain** (M) — **passed 2026-11-13 with OCCT 8.0.1: Windows
  locally, and CI on Windows, macOS arm64, macOS x64 and Linux (section 8).** Cargo workspace; build a pinned OCCT from
  source with CMake (minimal modules: Foundation, Modeling, DataExchange) on Windows
  (MSVC), macOS (arm64 + x86_64), Linux in GitHub Actions with build caching; `cxx` shim
  exposing make-box, cut, fillet with history. Pass: box − cylinder test with history
  passes on all three OSes in CI; cold build time and cached build time recorded.
- [ ] **P2 Shell** (M) — **Windows passed 2026-11-13 on real hardware. Linux fails by
  design (the swapchain overdraws the webview), so Linux desktop is deferred (section 1).
  macOS overlay works in CI; a real-Mac run and a two-monitor mixed-DPI check are
  nice-to-haves, not gates (owner decision 2026-11-13, section 1 "Desktop platforms").** Tauri 2 window, wgpu viewport drawing a 1M-triangle mesh, the
  React UI in a transparent webview on top. Pass, on Windows and macOS: viewport mouse
  input reaches Rust with < 8 ms added latency at 120 Hz; React menus/tooltips/
  PropertyManagers render over the viewport; resize and HiDPI and multi-monitor correct;
  no flicker. Fail on Windows → adopt the Electron fallback (section 1) and record why; a
  macOS-only failure is handled for macOS alone.
- [x] **P3 Geometry host** (S) — child process running OCCT, shared-memory mesh transfer
  of 1M triangles, kill-and-restart on timeout. Pass: transfer < 20 ms; a forced crash
  is reported as an error and the next request succeeds.
- [x] **P4 Renderer baseline** (M) — wgpu: 10k instances of 100 distinct meshes, ID
  picking, wide-line edges, section plane with capping. Record frame times on whatever
  NVIDIA / AMD / Intel / Apple hardware is available.
- **Owner action, in parallel:** ask Siemens (Parasolid) about licensing for a small
  independent CAD product; record the answer here. Not blocking.

### Phase E: engine core (Rust) — after P1, P3

- [x] **E1 `oc-doc`** (L) — Rust document model mirroring `src/core/types.ts`; serde
  round-trip of every corpus document byte-equivalent (after canonical formatting);
  transactions; undo/redo via structural sharing; schema version + migration hook;
  property tests (random edit sequences, undo all → original).
- [x] **E2 `oc-sketch`** (L) — port solver with all existing `solver.test.ts` cases;
  dual-number Jacobians; component decomposition; benchmarks (N rectangles grid, N-bar
  linkage, polygon, tangent-arc chain at N = 10/50/200). Pass: matches TS solver results
  within 1e-9 on the suite; grid N=200 drag step < 2 ms native. WASM build plus a TS
  binding with the same API as `SketchSystem`.
- [x] **E3 `oc-kernel` trait + `oc-kernel-occt`** (L) — **done and reviewed 2026-11-13
  (section 8); macOS and Linux CI (`engine.yml`) pending.** — operations the feature set needs
  (prism, revol, sweep, loft, boolean fuse/cut/common, fillet, chamfer, shell, draft,
  offset, section, measure, mass properties, STEP read/write, tessellate), each returning
  shape + history; validation and healing; tolerance policy; error types. Designed so a
  Parasolid implementation can satisfy it (no OCCT types in the trait).
  **Review checkpoint.**
- [x] **E4 `oc-mesh`** (M) — **done 2026-11-13 (section 8).** — parallel
  per-face tessellation, edge polylines, LOD levels, normals; output layout the renderer
  consumes directly.
- [x] **E5 `oc-graph` + features** (L, split per feature family if large) — **done
  2026-11-13 (section 8). Owner decision: single-part regeneration stays serial; parallel
  regeneration moved to G3, native body transfer to the geometry-host work.** DAG, cache,
  ~~parallel regeneration~~ (moved to G3), cancellation, per-node timings; implement every feature type.
  Pass: every corpus part matches the phase-0 oracle (volume/area 1e-6 relative,
  topology counts exact, statuses equal) except cases recorded in section 8 with a reason.
- [x] **E6 `oc-naming`** (L) — **done 2026-11-14 (section 8); review checkpoint.** —
  section 3 in full, wired into E5. Pass: all torture
  scripts TT1–TT7 pass; zero `/new/` tags across the corpus; determinism under 1, 4 and
  16 threads. **Review checkpoint.**

### Phase F: app on the new engine — after E6, P2 (Windows), P4

- [x] **F1 `oc-render`** (L, smoke) — **done 2026-11-15 (section 8), reviewed and
  merged; parity checklist and owner decisions there. Phase-1 items not built (IBL, SSAO, OIT, silhouettes) are
  listed as follow-ups.** — phase-1 renderer from section 4; parity with the
  current viewport's display styles, highlights, planes, triad and picking priority.
- [ ] **F2 IPC and app wiring** (L, smoke) — Tauri commands replacing `kernel/client.ts`;
  React UI moved into the Tauri app; sketch editor on the WASM `oc-sketch`;
  `window.opencad` debug hook kept. Split in two (section 8, 2026-11-15):
  - [x] **F2a native app slice** — **done 2026-11-15 (section 8).** `crates/oc-app` (Tauri 2,
    D3D12 swapchain under the transparent WebView2); kernel requests served by `oc-graph` on
    OCCT with heals and per-reference Missing; viewport drawn by `oc-render` from a scene the
    webview sends; plane labels rasterised in the webview; WASM solver; `--smoke`,
    `--script`, `--corpus` (17/17 corpus parts equal the oracle through the UI).
  - [x] **F2b picking and polish** — **done 2026-11-16 (section 8), not merged.** Picking,
    hover, Select Other, rotate-about, box selection, sketch snaps and the View Selector go
    through `oc-render`; no WebGL context in the native app; plain lines 1.5 device px.
    Open: the real-mouse and interactive drag-resize check (the owner's; computer-use
    access was not granted) and the decisions listed there.
- [ ] **F3 Switch-over** (M, smoke) — new app passes the Electron smoke-equivalent
  end-to-end test and a manual checklist from FEATURE_SPEC; installers for the three
  OSes in the release workflow (ask the owner before changing release/signing).
  Old Electron app frozen. **Review checkpoint.**

### Phase G: scale

- [ ] **G1 Assemblies data model** — components, instances, mates (3D solver from E2's
  core), content-hash geometry sharing, lightweight loading from caches.
- [ ] **G2 GPU-driven rendering** — culling compute, indirect draws, Hi-Z, LOD selection.
  Pass: 10k-component assembly ≥ 60 fps on a mid-range GPU (record hardware).
- [ ] **G3 Parallel and out-of-core regeneration** across parts and bodies (moved here from
  E5, owner decision 2026-11-13: a single part's body chain stays serial); memory budget
  tests. With the geometry-host work: a native body transfer in the kernel trait
  (`export_native` / `import_native`: OCCT `BinTools`, Parasolid XT), so tool bodies can be
  built on worker hosts ahead of their booleans (E5 section 8, option (a)).

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
  `C:\occt\install-8.0.1` (`crates/oc-kernel-occt/build-occt.cmd`). Crate versions used:
  wgpu 30.0.1, Tauri 2.12.5, cxx 1.x, glam 0.30.
- OCCT 8.0.1 (2026-07-30) is the current release; 7.9.3 the last 7.x. The P1 shim
  (`BRepAlgoAPI_Cut`, `BRepFilletAPI_MakeFillet`, `Modified`/`Generated`/`IsDeleted`,
  `BRepCheck_Analyzer`, `BRepMesh_IncrementalMesh`, `Poly_Triangulation` normals)
  compiled against 8.0.1 unchanged. `Standard_Failure` reaches Rust as `Err` through a
  custom `rust::behavior::trycatch`.
- (2026-11-13, E3) Correction: in OCCT 8.0.1 `Standard_Failure` **does** derive from
  `std::exception` (`what()`, `ExceptionType()`; `DynamicType()` is gone). The
  `TopTools_*`/`TColgp_*` collection typedefs and their headers are deprecated in favour of
  `NCollection_*`; `Handle(T)` still works. `BRepOffsetAPI_DraftAngle` crashes the process
  (access violation) on a face that closes on itself (full cylinder), where the WASM
  build threw an error.
- wgpu 30 API differences from older examples: `Instance::new(InstanceDescriptor::
  new_without_display_handle())`, `request_adapter`/`request_device` return `Result`,
  `PipelineLayoutDescriptor { bind_group_layouts: &[Option<&_>], immediate_size }`,
  `get_current_texture()` returns the `CurrentSurfaceTexture` enum, `queue.present(frame)`,
  `get_mapped_range()` returns `Result`. glam's `look_to_rh` needs a **unit** direction.
- Tauri 2 transparent WebView2 over a wgpu swapchain works on Windows (P2). On macOS a
  transparent WKWebView composites over a Metal swapchain on the same window (CI only);
  the wgpu surface must be created on the main thread there, or raw-window-metal panics.
  On Linux (WebKitGTK, X11) a swapchain on the Tauri window draws **over** the webview,
  so this layering does not work there.
- OCCT 8.0.1 static minimal builds and the P1 shim pass on GitHub's windows-latest,
  macos-latest (arm64), macos-15-intel and ubuntu-latest runners. On Unix the static
  archives need dependents-first order, read from OCCT's installed `*Targets.cmake`; on
  Apple, OCCT names AppKit and IOKit bare and they must be linked as frameworks. In a
  Git Bash step on Windows, `/usr/bin/link` shadows MSVC's `link.exe`.
- Still not verified: wgpu
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
    12.5 min** wall (configure + 5 576 Ninja steps + install, 16 threads; 11.7 min of it after configure), build dir
    1.9 GB, static libraries 942 MB. DataExchange pulls in TKV3d/TKService/TKVCAF
    through TKXCAF even with Visualization off; harmless, they compile without
    FreeType. Rust side: `cxx` bridge (`cpp/shim.h|.cpp`, `src/lib.rs`; `build.rs` reads
    `OCCT_DIR` and links every `TK*` archive in dependency order). Shim compile
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

    macOS and Linux were run in GitHub Actions afterwards; see the CI entry below.
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
- 2026-11-13 **Phase P in CI (P1 ticked; P2 not).** The owner approved a workflow,
  `.github/workflows/spikes.yml`. It runs by hand or on pushes to main that touch
  `spikes/`, never on tags, so it stays out of Release's way. Results are written to each
  job's summary. Two runs were needed: 43283584863 and 43301694758.
  - **P1: pass on every OS.** Run 2, `cargo test --release -p p1-occt`, 3/3 tests:

    | Runner | Cold OCCT build (configure + build + install) | Box − cyl + fillet + check |
    |---|---|---|
    | windows-latest (x64, MSVC) | 89.6 min (run 1: 103 min) | 5.33 ms per part |
    | macos-latest (arm64) | 11.7 min (run 1: 32.3 min) | 2.67 ms |
    | macos-15-intel (x64) | 46.1 min (run 1: 49.5 min) | 7.21 ms |
    | ubuntu-latest (x64) | 69.0 min (run 1) | 4.00–4.05 ms |

    - **Cached build:** the OCCT install (47 MB compressed on Linux) restores in 2 s. A
      whole cached P1 job takes 1 min 19 s on Linux; shim compile, link and test take
      55 s of that. The history assertions (modified, kept, generated, deleted) are
      identical on all four.
    - Cold times vary 2–3× between runs on the same image, so treat them as a range.
      The cache key is OCCT version + runner + hash of `build-occt.*`.
    - Run 1 failed on three runners, all in the toolchain, not in OCCT:
      - Windows: Git Bash's `/usr/bin/link` shadowed MSVC's `link.exe`; the job deletes it.
      - macOS: OCCT's targets name `AppKit` and `IOKit` bare, and they were passed as
        `-l`; `build.rs` now links them as frameworks.
      - `actions/cache` saves only when the whole job succeeds, so those builds were
        lost. The workflow now uses separate restore and save steps, saving right after
        the OCCT build.
    - E3 can pin OCCT 8.0.1 with this recipe (`build-occt.cmd|.sh`, `build.rs` link
      order from `*Targets.cmake`).
  - **P2: not passed.** Self-test per runner (run 2, after moving surface creation to the
    main thread, which macOS requires):

    | | Windows VM (WARP, D3D12) | macOS arm64 VM (Paravirtual Metal) | Linux Xvfb (llvmpipe Vulkan) |
    |---|---|---|---|
    | Overlay | ok | menu (250) over viewport (teal) ok; bar sample white, see below | **FAIL: whole window is viewport colour** |
    | OS input → Rust | 239/400, median 7.64, p95 12.71 ms | n/a | n/a |
    | eval round trip | median 1.83, p95 3.95 ms | median 1.81, p95 6.40, max 71.9 ms | median 1.56, p95 3.27 ms |
    | Resize grabs bad | 15/30 | 15/30 | 16/30 |
    | Frames | 519 at 59 Hz | 963 at 59 Hz | 3746 (no vsync) |

    - **Linux is a real failure, not a runner artefact.** With wry on GTK the webview
      and the wgpu surface share one X window, and the swapchain covers the webview
      completely (frames presented, every pixel teal, both runs). Layering a
      transparent webview over a swapchain on the same window does not work on
      WebKitGTK. Options for the owner:
      - (a) give the viewport its own native child window under a webview that is
        itself a child window (GTK overlay); needs a spike;
      - (b) the Electron fallback on Linux only;
      - (c) drop first-class Linux desktop support.

      Until decided, P2 stays unticked.
    - **macOS: transparency works.** The menu's pixels and the teal viewport are both
      where expected. The one failing sample is the menu bar at y = 16 px, which reads
      white (255) rather than #e8e8e8. That is most likely the capture origin being off
      by the title bar (Tauri's `inner_position` vs xcap's screen coordinates) rather
      than a rendering fault, but no screenshot was kept, so it is unproven.
    - **Resize:** on every runner exactly the 15 grabs taken with no wait after
      `set_size` fail, and the 15 taken after 16 ms pass. Locally (180 Hz, RTX) all 30
      pass. On 59 Hz software renderers the newly exposed area is unpainted for up to one
      frame. Whether users would see that as flicker needs real hardware.
    - **Windows VM latency** exceeds 8 ms (median 7.64 ms with a software renderer
      and 2 vCPUs, and Chromium coalesced 161 of 400 moves). The same test on real
      hardware gives median 0.6–1.0 ms, so the Windows verdict stands.
    - Still needed for P2 on macOS: a run on a real Mac with a saved screenshot, plus
      the two-monitor mixed-DPI check on any OS.
  - **Decision (owner, 2026-11-13): defer Linux desktop**, a variant of (c). Desktop
    ships on Windows and macOS; Linux uses the browser version; the engine keeps
    building and testing on Linux. Recorded in section 1 and goal 4. (b) was rejected
    because it doubles the shell and needs a WASM renderer build for one platform. (a)
    stays as a possible later Wayland spike.
  - Next:
    - E3 can start; OCCT 8.0.1 builds on every target OS.
    - Real-Mac P2 run (screenshot kept) and a two-monitor mixed-DPI check. If macOS
      fails, the Electron fallback applies on every OS and the Linux decision is moot.
    - F1 waits for that run. The P2 Linux CI job now only documents the overdraw; drop
      it from `spikes.yml` when the workflow is next touched.
- 2026-11-13 **E1 done.** Crate `crates/oc-doc` (serde, serde_json, imbl, ryu,
  serde_path_to_error; licences in the new `docs/THIRD_PARTY.md`). It mirrors the document
  part of `types.ts`: `PartDocument` and everything in it, including `SketchData.refs`,
  `SketchRef`, and `ext` on points, lines, circles and arcs. The kernel output types
  (`BodyMesh`, `RebuildResult`, ...) belong to E3 to E5 and are not mirrored.
  - **Round trip:** 39 corpus documents. These are the 17 parts, plus each of the 9 torture
    scripts' initial document and the document after every edit step (13 steps,
    replayed like `applyEdits`). There is also a hand-written fixture,
    `tests/fixtures/all-fields.ocpart`, with every field, every enum value and all three
    states of each `T | null` field. The corpus has no relations, dimensions, sketch
    refs, display, props or equations, so it alone does not cover the schema. For each
    document: canonical(file) = canonical(model) = canonical(saved text), the reloaded
    model is equal, saving is idempotent, and `validate()` reports nothing. The corpus
    uses all 19 feature types (asserted).
  - **Canonical form = RFC 8785 (JCS):** keys sorted by UTF-16 code units, numbers
    exactly as JavaScript's `String(x)` prints them (from ryu's shortest digits), and
    strings escaped as `JSON.stringify` escapes them. Cross-checked against Node for all
    40 documents: **0 of 40 differ byte-wise** (`tests/measure.rs export_canonical` plus
    a JS JCS one-liner). Writing refuses NaN, infinities and integers beyond 2^53
    instead of emitting `null`. `save` writes the canonical member order with a
    one-space indent, the app's `JSON.stringify(doc, null, 1)` layout. Exception, since
    the order fix below: sketch records keep insertion order.
  - **Decision: unknown fields are rejected, not carried along.** Rule: after typing, a
    document is written back to canonical JSON and compared with the canonical form of
    the (migrated) file. Any difference is `LoadError::Lossy` with the exact JSON path:
    an unknown field, `null` in an optional non-null field, or anything else the model
    would drop or change. Repeated keys in one object are a syntax error. Why not round
    trip unknown fields: opaque data cannot be kept consistent by edits (it may name a
    feature an edit deletes), and a newer schema is caught earlier by `version`.
    `serde(deny_unknown_fields)` was tried and dropped. serde buffers internally
    tagged enums, so its errors point at the enclosing feature, not at the field. The
    lossless check gives exact paths and also catches null/absent confusion. The one
    free-form field is `props` (`Record<string, unknown>` → `serde_json::Value`).
    `T | null` fields are `Option<Option<T>>` (absent / null / value). Required
    nullable fields (`rollback`, `origin`, `PatternDirection.ref`, ...) must be present.
    Integer fields (`nextDim`, `count`, `quadrant`, `rollback`) are `u32`, so `1.5`
    is a schema error.
  - **Schema version + migrations:** `CURRENT_VERSION = 1`. Migrations run on the untyped
    JSON root, one `Migration { from, apply }` per version, before typing. The typed
    model only ever describes the current schema. The registry is empty today, and
    `check()` asserts it is a contiguous chain. A newer file is refused ("saved by a
    newer OpenCAD"), an older one below the oldest step too. Tested with a made-up
    version 0 through `io::load_with`. `load` returns `source_version` so the UI can
    say a file was converted.
  - **Transactions and undo:** `History` holds `Arc<PartDocument>` snapshots.
    `features` is an `imbl::Vector<Arc<Feature>>` and sketch records are persistent
    maps (`oc_doc::Record` since the order fix below, built on `imbl::OrdMap`), so a
    transaction copies only what it writes (`Arc::make_mut`, persistent-map path copy). An unchanged feature stays pointer-equal across steps,
    for E5's cache. Undo and redo swap pointers. Undo all returns the *same* snapshot
    (`Arc::ptr_eq`), not a copy. A transaction is `begin`/`commit`
    (drop = cancel, for PropertyManagers) or `transact(label, |doc| Result)`. `Err`,
    panic or no net change leave no undo step. Also: undo/redo labels, an undo limit,
    a saved marker (`is_dirty`; undoing back to the saved snapshot is clean), and
    `heal()`, the non-undoable, non-dirtying write-back section 3.3 step 4 needs.
  - **Property test:** 256 cases of 1 to 40 random ops on random corpus parts. Ops:
    rename, suppress, rollback, delete, duplicate, reorder, move or add a sketch point,
    delete an entity, scale a feature's main parameter, material, props, multi-edit
    batches, failing transactions, undo, redo. The oracle keeps full canonical-JSON
    snapshots, like the TS app. It applies each edit to its own deep copy re-read from
    JSON, so it shares nothing with the history. Checked after every op: same bytes,
    `can_undo`/`can_redo` agree, `validate()` is clean. At the end: save/load is equal,
    undo all gives back the original snapshot pointer, redo all gives the newest state.
  - **Mutation-checked:** each of these breaks at least one test: undo not restoring;
    commit not clearing redo; change detection always "unchanged" (the first oracle
    missed this, now fixed); lossless check off; `null` read as absent.
  - **Measurements** (release build, Ryzen 7 7800X3D, `tests/measure.rs`):
    - Corpus, 39 documents, 136 KB: load 6.6 ms total, save 1.2 ms total.
    - Large part, 3 001 features (the `patterns` features ×200) plus one sketch of 10 000
      lines, 3.9 MB JSON: load 166 ms. One full canonical snapshot, which is the TS undo
      approach, takes 27 ms and 2.2 MB per step.
    - Structural sharing on the large part: renaming a feature commits in 13 µs and
      keeps **2.9 KB** per undo step. Moving one point of the 10k-line sketch commits
      in 23 µs and keeps **8.3 KB**. Undo/redo takes < 0.1 µs.
  - **Deviations and quirks:**
    - Tests were written alongside the code, not strictly first. To compensate, every
      key test was verified to fail for the right reason by the mutations above.
    - `docs/THIRD_PARTY.md` is outside the package's files but required by section 4.
    - `#[allow(clippy::large_enum_variant)]` on `FeatureKind` (Extrude is 544 bytes):
      every feature is its own `Arc` allocation.
    - ~~Sketch records are sorted maps~~. Fixed the same day: sketch records keep
      insertion order in memory and in saved files, as the TS app does, because E2's
      solver depends on it (entry "Sketch records keep insertion order" below).
      Canonical form (RFC 8785) still sorts, so hashes are unaffected. Section 3.4
      forbids depending on *nondeterministic* order (hash maps, threads); insertion
      order is part of the document.
    - Loading the 3.9 MB part spends most of its 166 ms building and comparing two
      canonical trees. This is fine for now; profile in G1 if assemblies need it.
    - `crates/oc-doc/tests/history.proptest-regressions` holds a seed that shrank from
      a mutation run. It is kept as a regression case, as proptest recommends.
- 2026-11-13 **E2 done.** `crates/oc-sketch` (native solver), `crates/oc-sketch-wasm`
  (wasm-bindgen binding), `src/sketch/wasmSolver.ts` (TS wrapper with the `SketchSystem`
  API, type-checked as `Pick<SketchSystem, ...>`), `scripts/build-sketch-wasm.mjs`
  (`npm run build:sketch-wasm`, output committed in `src/sketch/wasm/`). The app still uses
  the TS solver; F2 switches it.
  - **Port.** Same parameters in the same order, same equations (every relation and
    dimension kind, written once over a `Real` trait), same minimum-norm damped
    Gauss-Newton with halving line search, null-space drag, row-normalised RREF in creation
    order. Additions: exact Jacobians by forward-mode dual numbers (`Dual<8>`, chunked);
    connected-component decomposition (union-find over parameters: solve, drag and
    analysis run per component; RREF of independent rows is exactly the global RREF);
    sparse `J Jᵀ + μI` Cholesky with minimum-degree ordering, symbolic factorisation once
    per component, dense Gaussian fallback as in TS. Modules: `model`, `real`, `geom`,
    `equations`, `build`, `system`, `solve`, `sparse`, `analysis`, `generators`; none over
    600 lines.
  - **Tests.** `tests/solver.rs` ports every `solver.test.ts` case and the solver
    behaviour of `refs.test.ts` (ext points are constants, ext circles have a fixed
    radius, reference geometry is followed), plus decomposition, drag and constant-equation
    tests. `src/sketch/wasmSolver.test.ts` runs 71 cases through the TS solver and the WASM
    build:
    - the `solver.test.ts` and `refs.test.ts` cases;
    - the Slot and Polygon tools;
    - cases covering every relation and dimension kind (including ellipse, spline, offset,
      driven and angle dimensions, conflicts and constant equations);
    - every sketch in the golden corpus (54);
    - the benchmark sketches at N = 10.

    It records the TS results in `crates/oc-sketch/tests/fixtures/ts-parity.json`
    (re-record: `UPDATE_GOLDEN=1 npx vitest run wasmSolver`) and checks the stored file
    still equals live TS. The native `tests/parity.rs` replays the fixture.
    `crates/oc-sketch-wasm/tests/fresh.rs` fails `cargo test` if the committed WASM was
    built from other sources (FNV-1a hash in `src/sketch/wasm/SOURCE_HASH`).
  - **Parity result.** On all 71 cases these are identical: solve and drag return values,
    DOF, status, black/blue/red state, and the redundant-relation list (in order).
    Parameters are within **7.0e-10** of the TS iteration with an exact Jacobian (native
    and WASM), and within **7.9e-7** of the shipped TS solver.
  - **Deviation: the 1e-9 criterion holds against the TS algorithm, not against the
    shipped TS numbers.** The shipped solver differentiates by central differences
    (`h = 1e-7·max(1, |x|)`, about 1e-9 relative error), and that error alone moves
    results. `drag-swing` (the `solver.test.ts` drag, 20 mm) differs by 5.3e-9; the
    under-defined coverage cases differ by up to 2.6e-7.
    - Evidence: with the exact Jacobian swapped into the *unchanged* TS solver,
      single-component cases agree with Rust to about 1e-15. The gap comes from the
      differences in TS, not from the port.
    - The parity test therefore asserts 1e-9 against "TS iteration + exact Jacobian" and
      1e-6 against the shipped TS.
    - All original suite cases (`solver.test.ts`, shapes, `refs.test.ts`, corpus) are
      within 1e-9 of the shipped TS as well, except `drag-swing` at 5.3e-9.

    **For the owner/reviewer:** if the criterion must mean the shipped TS numbers, TS would
    need exact derivatives too. Copying its finite-difference noise into Rust would be
    wrong.
  - **Deviation: per-component iteration.** The damping `μ` and the line search are chosen
    per component, and a converged component stops iterating; TS runs one global
    iteration. Single-component sketches iterate exactly as in TS. On multi-component
    cases the difference is at most 7e-10 here. A component that is unchanged and already
    converged is not re-evaluated, so a drag costs only what its own component costs.
  - **Deviation, fail-soft:**
    - A relation or dimension naming a point that does not exist is skipped (TS throws
      when evaluating it).
    - Unknown relation, entity and dimension kinds parse as `Other` and are skipped.
    - Equations whose references are all fixed are "constant": they count towards
      convergence and are flagged redundant, as TS's zero Jacobian rows are.
  - **Benchmarks** (`cargo bench -p oc-sketch`, release build, Ryzen 7 7800X3D, medians).
    `drag` is one `drag()` call (12 sub-steps) on a solved system, i.e. one mouse move.
    `cycle` is what the editor does per mouse move today: build, solve, drag, commit,
    analyse. The sketches come from `src/generators.rs`:
    - `grid`: N independent dimensioned rectangles (2 DOF each);
    - `grid-linked`: the same, each rectangle aligned to its neighbour (one component,
      N + 1 DOF);
    - `linkage`: a closed N-bar linkage with fixed bar lengths (N − 3 DOF);
    - `polygon`: the Polygon tool's relations on an N-gon (4 DOF);
    - `tangent-chain`: N semicircles tangent at shared ends, radii equal to arc 0's
      (N + 1 DOF).

    | sketch | N | params × eqs (largest component) | drag native | drag WASM | drag TS | cycle native | cycle TS |
    |---|---|---|---|---|---|---|---|
    | grid | 10 | 8 × 6 | 18 µs | 44 µs | 5.8 ms | 79 µs | 5.6 ms |
    | grid | 50 | 8 × 6 | 20 µs | 39 µs | 473 ms | 0.30 ms | 489 ms |
    | **grid** | **200** | 8 × 6 | **25 µs** | 45 µs | 28.7 s | 1.1 ms | 28.0 s |
    | grid-linked | 10 | 80 × 69 | 0.14 ms | 0.24 ms | 6.9 ms | 0.26 ms | 7.1 ms |
    | grid-linked | 50 | 400 × 349 | 0.72 ms | 1.1 ms | 644 ms | 1.8 ms | 645 ms |
    | grid-linked | 200 | 1600 × 1399 | 3.1 ms | 4.6 ms | 39.6 s | 15 ms | 39.5 s |
    | linkage | 10 | 20 × 13 | 38 µs | 74 µs | 0.41 ms | 58 µs | 0.46 ms |
    | linkage | 50 | 100 × 53 | 0.19 ms | 0.43 ms | 7.6 ms | 0.27 ms | 7.9 ms |
    | linkage | 200 | 400 × 203 | 0.93 ms | 2.1 ms | 415 ms | 1.5 ms | 422 ms |
    | polygon | 10 | 23 × 19 | 0.11 ms | 0.25 ms | 1.1 ms | 0.16 ms | 1.2 ms |
    | polygon | 50 | 103 × 99 | 3.2 ms | 6.3 ms | 20 ms | 5.1 ms | 21 ms |
    | polygon | 200 | 403 × 399 | 141 ms | 231 ms | 897 ms | 213 ms | 943 ms |
    | tangent-chain | 10 | 42 × 31 | 0.16 ms | 0.34 ms | 2.9 ms | 0.24 ms | 3.1 ms |
    | tangent-chain | 50 | 202 × 151 | 2.0 ms | 3.8 ms | 107 ms | 3.4 ms | 107 ms |
    | tangent-chain | 200 | 802 × 601 | 77 ms | 108 ms | 11.3 s | 110 ms | 11.4 s |

    **Pass: one drag step on the N = 200 grid takes 25 µs native (target < 2 ms).** The
    WASM and TS columns were timed in Node through vitest. On the grid, N = 200 costs the
    same as N = 10, because only the dragged component is touched.
  - **Finding: "equal to one" relations cost cubic time in the dual form.** The Polygon
    tool relates every side to side 0 and every vertex to vertex 0; the chain benchmark
    relates every radius to arc 0. Each such row shares a parameter with every other, so
    `J Jᵀ` has a dense block. At N = 200 the polygon's factor is a full 399 × 399 Cholesky
    (6 ms, 24 per drag), and its minimum-degree ordering takes 66 ms per system build.
    - In practice this is bounded: the Polygon tool caps at 40 sides, and N = 50 drags in
      3 ms. It is still the scaling limit.
    - Remedy when needed: treat dense columns of `J` separately (sparse Cholesky of the
      rest, plus a Woodbury/Schur update), with iterative refinement against the full
      system. Keep the current factorisation as fallback, because rows that touch only
      dense columns make the reduced system ill-conditioned.
    - Not done here: it goes beyond the pass criteria and needs its own stability tests.
  - **Other limits.**
    - Analysis is a dense RREF per component (11 ms for the 1600-parameter linked grid),
      and the editor calls it on every mouse move. Sparse rank analysis would help large
      connected sketches.
    - The WASM module is 616 KB (188 KB gzipped) with LTO. Size was not optimised.
  - **For E1 (switching to `oc-doc` types).** The data types live in `oc-sketch::model`,
    named exactly as in TS and as `oc-doc` will name them: `SketchData`, `SketchPoint`,
    `SketchEntity`, `Relation`, `Dimension`, `DimensionKind`, and `Record<T>` =
    `IndexMap<Id, T>`, with the same serde field names. Switching means replacing the
    `crate::model` imports with `oc_doc` ones. **Requirement on `oc-doc`:** sketch maps
    must keep insertion order. Parameter order, equation order, and therefore which
    relation is flagged redundant all depend on it; a sorted map would change solver
    behaviour.
  - Toolchain added (owner-approved): the `wasm32-unknown-unknown` target and
    `wasm-bindgen-cli` 0.2.130 (`cargo install wasm-bindgen-cli --version 0.2.130`; it
    must equal the crate's pinned `=0.2.130`). Dependency licences are recorded in the new
    `docs/THIRD_PARTY.md`.
- 2026-11-13 **E1 and E2 merged into main.** The owner accepted E2's parity deviation
  (1e-9 against the TS algorithm with exact derivatives, 7.9e-7 against the shipped TS).
  `docs/THIRD_PARTY.md` now lists both crates' dependencies in one table.
  - **Mismatch (resolved the same day, see the next entry):** E1 stored sketch records
    in `imbl::OrdMap` (sorted by id), but E2 requires insertion order, because
    parameter order, equation order and which relation is flagged redundant depend on
    it. Ids such as `p10` also sort before `p2`.
- 2026-11-13 **Sketch records keep insertion order (`oc-doc`).** Resolves the mismatch
  above; `oc-sketch` can now switch to `oc-doc` types.
  - **Type:** `oc_doc::Record<T>`, used for sketch `points`, `entities`, `relations`,
    `dimensions` and `refs`. It is a persistent map from `Id` that iterates, compares and
    serializes in insertion order: `get`, `get_mut`, `insert`, `remove`, `iter`, `keys`,
    `values`, `ptr_eq`, and `Index` by `&str` or `&Id`. All operations are O(log n).
    Inserting an existing id replaces it in place. Removing keeps the others' order,
    like JavaScript's `delete`.
  - **Layout:** not "a `Vector` of ids plus an index", because removing from that is
    O(n). Instead there are two `OrdMap`s: entries keyed by a sequence number that only
    grows (`u64 → (Id, T)`, which gives the order), and `Id → u64`.
  - **Equality** includes order. When both versions have the same id → sequence slots,
    which is the normal case inside an undo step, `==` compares the entry maps with
    imbl's diff. That skips shared subtrees, so detecting a one-point edit stays
    O(log n).
  - **I/O:**
    - serde_json gets the `preserve_order` feature, so the loader's intermediate
      `Value` keeps file order. indexmap is now also an `oc-doc` dependency;
      `THIRD_PARTY.md` is updated.
    - `save` keeps `Record` member order. `Record` serializes as a newtype named
      `$oc_doc::Record` that only `canon`'s save mode recognises. Other serializers see
      a plain map.
    - The canonical form and the lossless check still sort everything, per RFC 8785.
      Struct fields and other maps are still sorted on save.
  - **Tests:**
    - `roundtrip.rs` `load_then_save_keeps_sketch_member_order` covers every corpus
      document plus `all-fields.ocpart` (40 documents, 378 sketch records, 104 of them
      not in id order, e.g. `origin` first, `f7_p2` before `f7_p11`). For each one, the saved text and the
      in-memory iteration give exactly the file's member order.
    - Mutation-checked: it fails when save sorts records, and when the loader loses
      file order (no `preserve_order`).
    - `record.rs` unit tests cover order, replace-in-place, remove, re-add, order-aware
      equality and duplicate ids in JSON.
  - **Test changes:**
    - The history property test's oracle now snapshots `save()` text instead of
      canonical text. Canonical snapshots lose order, so after a reload "the first
      point" was a different point. The TS app's `JSON.stringify` snapshots keep
      order too. The comparison is now stricter (order must match), not looser.
    - The torture replay's `remove` uses `shift_remove`, which matches JS `delete`.
  - **Measurements** (release build, same machine, `tests/measure.rs`, main measured
    alongside):
    - Undo step, rename: 2.9 KB, unchanged.
    - Undo step, point move: 8.0 KB, was 8.3 KB. Commit takes 23 to 29 µs, was 23 µs.
    - New case, adding a point: 13.5 KB per undo step, 24 µs commit. An insert writes
      both inner maps.
    - Corpus load 5.6 to 6.7 ms (main 5.5 to 5.8 ms), save 1.0 to 1.1 ms (main 1.1 ms).
    - Large-part load 163 to 184 ms (main 162 to 185 ms).
    - A first version compared records by walking them: a point move took 75 µs because
      `same_as` walked all 20 000 points. Fixed by the slot fast path above.
- 2026-11-13 **P1 done.** Spikes run 43301694758 passed on all four P1 jobs. Cold OCCT
  builds on GitHub runners: Windows x64 89.6 min, macOS arm64 11.7 min, macOS x64 (Intel)
  46.1 min. Linux restored its cache from an earlier run: the whole job took 1 min 19 s,
  of which shim build and history test 55 s. P1 ticked.
- 2026-11-13 **E3 done; review checkpoint.** Crates `crates/oc-kernel` (the trait, no
  dependencies) and `crates/oc-kernel-occt` (OCCT 8.0.1 through a `cxx` bridge and a C++
  shim). Run: `OCCT_DIR=C:\occt\install-8.0.1 cargo test -p oc-kernel-occt`.
  - **The trait (`oc_kernel::Kernel`).**
    - `type Body: Clone` is an opaque, immutable handle; cloning shares geometry.
      Operations never modify inputs (booleans run non-destructive). Bodies need not be
      `Send`: one kernel per geometry-host thread, and OCCT parallelises inside its calls.
    - Inputs are plain data. A `Profile` is planar `Region`s (an outer `Wire` plus holes)
      of `Segment`s: line, three-point arc, circle, ellipse, spline through sample points.
      Specs state intent: `PrismSpec` (vector and optional `Taper`), `RevolveSpec`,
      `SweepSpec` (path, Follow Path or Keep Normal), `LoftSpec`, `FilletEdge` (constant
      or linear radius), `ChamferEdge` (equal, two distances or distance-angle, each with
      its reference face), `ShellSpec`, `DraftSpec` (faces, neutral plane, angle),
      `Transform` (rigid or mirror), `MeshParams`.
    - Operations: `primitive`, `prism`, `revolve`, `sweep`, `loft`, `boolean` (fuse, cut,
      common; one target, several tools), `fillet`, `chamfer`, `shell`, `draft`, `offset`,
      `transform`, `compound`, `section`, `heal`. Each returns
      `Op { body, history, healed }`.
    - Queries: `topology` (faces with surface type and parameters, area, centroid,
      signature normal, box and edges; edges with curve, ends, mid point, length, faces,
      seam flag; vertices), `bounding_box`, `mass_properties` (volume, area, centre,
      inertia tensor about the centre, principal moments and axes), `distance` between
      `Target`s (point, body, face, edge, vertex), `check`, `tessellate` (triangles per
      face, normals, a polyline per edge). Exchange: `read_step` (healed, multi-body) and
      `write_step` (AP214, mm).
    - Entities are addressed by index (`FaceId`, `EdgeId`, `VertexId`) in a
      **deterministic geometric order**, not OCCT's traversal order (finding 3 below).
      Indices are not persistent across edits; names (E6) come from history.
    - **History:** one `Fate` per source. A source is a face, edge or vertex of input body
      `arg` (0 = target, 1.. = tools), a profile `Segment { region, wire, segment }`, or a
      `Cap { region, Start|End }`. Each fate has `deleted` and `images` (output index plus
      `Same`, `Modified` or `Generated`). Every input face has a fate; edges, vertices,
      segments and caps appear where they produce faces. Sections report edges.
      `History::untracked` lists output faces with no source: what naming would call
      `/new/`.
    - **Errors:** `KernelError::{InvalidInput, Failed, InvalidResult, Empty, Io}`, each
      tagged with an `OpKind`. Argument checks live in `oc_kernel::validate`, so every
      kernel rejects the same inputs the same way.
    - **Tolerance policy** (`oc_kernel::Tolerances`, every literal in one place):
      confusion 1e-7 mm, angular 1e-12, boolean fuzzy value 0 (exact, as the oracle),
      parallel booleans on, face and edge unification after booleans at 1e-3 rad
      (replicad's `SimplifyResult`), offset 1e-3, loft 1e-6, spline fit 1e-5, heal 1e-7,
      heal acceptance 1e-6 relative, and the current viewer's display-mesh formula
      (`display_mesh(quality, size)`). A Parasolid kernel reports its own fixed precision
      through `tolerances()`.
    - **Validation:** every returned body passed `BRepCheck_Analyzer` and has a positive
      volume per solid (an inside-out solid passes the analyser). An invalid result is
      healed (`ShapeFix_Shape`) only if the healed body is valid and keeps its face count,
      volume and area within 1e-6; otherwise the call fails with `InvalidResult`. History
      is composed through the healing.
  - **Implementation.** `cpp/body.cpp` (canonical enumeration, primitives),
    `profile.cpp` (segments to wires and faces, prism, tapered prism, revolve, sweep,
    loft), `ops.cpp`, `finish.cpp` (history recording and composition, validation,
    guarded healing), `query.cpp`, `exchange.cpp` (STEP, tessellation). When one
    operation chains several OCCT builders (sweep with holes, tapered prism, healing),
    history is kept as OCCT shapes and turned into indices once, against the final body.
    `build.rs` follows P1 (static link order from OCCT's CMake target files), detects
    static or shared installs, and refuses any OCCT but 8.0.1. `build-occt.cmd|.sh` moved
    from the spike into the crate with a `Static|Shared` argument. All files are under
    600 lines.
  - **Tests** (38 in `oc-kernel-occt` plus an ignored timing report, 6 in `oc-kernel`).
    - `corpus.rs` replays corpus parts step by step with kernel calls, the way
      `engine.ts` builds each feature, and compares with `corpus/*.json`: volume, area and
      bounding box to 6 significant figures, solid, face and edge counts exactly. **All 35
      compared steps match.** Parts: block-hole, pad-pocket, fillet-chamfer (fillet,
      variable fillet, three chamfer types), shell, shell-outward, draft, revolve (full,
      mid-plane cut revolve, two directions), sweep-loft (circle swept along a line-arc-line
      path, square sweep, loft), curved-profiles (arcs, ellipse), through-mid-dir2
      (mid-plane, Through All both ways), patterns (the whole part: linear patterns of a
      boss and a pocket, circular pattern, mirror of a feature and of the body, ending at
      98 faces and 200 edges) and imported (STEP read of `corpus/assets/block.step`, then
      a boss). A region with a hole (thin-contours) is checked against the difference of
      two steps.
    - `history.rs`: prism sides and caps per segment, hole wires, box − cylinder (P1's
      case), a coplanar fuse merging both inputs' faces, fillet faces from edges and the
      corner patch from the vertex (H04), shell, partial versus full revolve, transform,
      compound, section, loft, tapered prisms (exact frustums, sharp outward corners, a
      hole), which end of a variable fillet gets the first radius.
    - `robustness.rs`: the 22 hard cases of the failure inventory. **Every outcome equals
      the WASM inventory's**, with the exact or the recorded volume where the case
      succeeds. Also argument errors, empty results and unreadable STEP.
    - `queries.rs` (topology parameters, box inertia, distances, tessellation, STEP round
      trip, heal) and `determinism.rs` (a 7-operation script plus a mesh gives identical bodies,
      order and history run after run, and with serial or parallel booleans).
    - **No operation tested left a face without history** (`assert_tracked` on prism,
      tapered prism, revolve, sweep, loft, boolean, fillet, chamfer, shell, draft,
      transform, compound and heal results), so the E6 target of zero `/new/` tags looks
      reachable.
    - Mutation-checked: without face unification after booleans, `revolve` and `patterns`
      miss the oracle; accepting any healed result turns H10 into a "valid" body (the
      zero-thickness lid of 0.4) and fails two tests; without the canonical order,
      `determinism.rs` fails.
    - The full suite also passes against a **shared** OCCT build (DLLs).
  - **Measurements** (release, Ryzen 7 7800X3D, `determinism.rs` `timings`): rectangle
    prism 0.9 ms, box − cylinder 3.0 ms, fillet of 4 edges 8.3 ms, shell 5.1 ms, topology
    of 7 faces 0.3 ms, check 0.9 ms, display tessellation 0.5 ms, STEP write 1.4 ms and
    read 6.0 ms. The 7-operation script, with a topology query after each step and a mesh,
    takes 112 ms. Every operation time includes validation. The debug test suite runs in
    about 3 s. The shared OCCT build took 12 min, like the static one.
  - **Findings and deviations.**
    1. **`DraftAngle` crashes OCCT 8.0.1 on a full cylindrical face** (H14): an access
       violation, not an exception. `draft` refuses faces that close on themselves with
       `Failed`, the outcome the WASM build gave. Fillet faces between planes (H15) draft
       fine. This is the first in-process crash found; the geometry host (P3) is the
       containment.
    2. **Tapered extrusion.** `LocOpe_DPrism` was tried first. It is exact, but it rounds
       every corner that grows (an outward square gets cone corner patches), unlike
       SOLIDWORKS and the oracle. Polygon profiles are now a prism whose sides are drafted
       (exact planes, sharp corners). Profiles with curves use the oracle's construction,
       a ruled loft to the sharp-cornered offset wire: exact cones for arcs, but the
       straight segments of such a profile become B-spline surfaces. For E5: a drafted
       slot has B-spline side faces.
    3. **Entity order.** The shell's face order changed between runs in one process
       (OCCT's offset iterates address-keyed maps); the geometry was identical. The shim
       therefore numbers faces, edges and vertices by a geometric key, and stores edges in
       their own orientation (an edge's first occurrence flips with traversal order, which
       swapped a circle's start and end in the shared build). **Consequence for E5:**
       indices no longer follow replicad's order. The TS engine measures an unequal
       chamfer's first distance on "the edge's first face", an artefact of that order. The
       corpus is reproduced by naming the face by geometry (Chamfer2: the y = 0 face,
       Chamfer3: the x = 40 face).
    4. OCCT history gaps, worked around in the shim: a full `MakeRevol` does not report the
       annuli swept by segments perpendicular to the axis (they are taken from the sweep
       itself); `DraftAngle` reports drafted faces as *generated*, so composition follows
       a face-to-face `Generated` like `Modified`.
    5. **Shell reports the open face as `Modified` into the rim** (the thin frame left in
       its plane), not as deleted. E6 must expect that.
    6. `topology()` first computed optimal boxes for every face, which cost 8 ms per torus
       face (95 % of the call). Planar faces keep exact boxes; curved faces get OCCT's fast
       box, which contains the face (documented on `FaceInfo::bbox`).
    7. STEP: OCCT prints transfer progress to stdout; the shim removes the default
       printers. Written files carry a timestamp, so STEP output is not byte-identical
       between runs (the geometry is).
    8. Keep Normal Constant sweeps use OCCT's corrected Frenet mode, as the oracle does.
       Whether that matches SOLIDWORKS is unverified (E5: mark 🔍 if it matters).
    9. The workspace denies `unsafe_code`; only the cxx `ffi` module allows it.
       `cargo test --workspace` now needs `OCCT_DIR` (`crates/README.md`).
    10. Not in the trait yet: planar wire offset (used only inside the tapered prism),
        thicken, split, imprint and face replacement. Add them when a feature needs them.
  - **Licensing** (`docs/THIRD_PARTY.md`): development, tests and CI link OCCT statically.
    LGPL 2.1 allows distributing such a binary only together with relinkable objects. The
    shared build is supported by `build.rs` and verified, and is what installers should
    ship (F3). New Rust dependencies: `cxx` and `link-cplusplus` (linked) and the
    `cxx-build` toolchain (build time), all MIT OR Apache-2.0 apart from foldhash (Zlib)
    and two Unlicense OR MIT crates; `serde_json` for tests.
  - **CI:** `.github/workflows/engine.yml` runs clippy and the workspace tests on Windows,
    macOS arm64 and x64, and Linux, with OCCT cached (falling back to the P1 cache). It is
    written but **not pushed**: that needs the owner's OK. The new crates have been tested
    on Windows only.
  - Outside the package's files: `docs/THIRD_PARTY.md`, `docs/ARCHITECTURE.md`,
    `crates/README.md`, the workflow, and sections 6 and 7 of this file.
  - **For the reviewer:**
    - Is index addressing plus history the right contract, or should the trait hand out
      entity handles that are stable for a body's lifetime (as Parasolid tags are)?
    - History has faces (or section edges) as images, with edges and vertices only as
      sources. Edge images would be needed if edges ever carry names of their own.
    - The tapered prism's split by profile type (finding 2).
    - Should static linking be allowed outside CI at all?
- 2026-11-13 **E3 reviewed and merged into main.** The owner approved the
  merge and pushing `.github/workflows/engine.yml`. Review answers:
  - Index addressing plus history stays the contract. Indices are per body; E6 derives
    persistent names from history, so lifetime-stable handles are not needed in the trait.
  - Edge images in history: add them only when edges need names of their own.
  - The tapered prism's split by profile type is accepted; E5 must expect B-spline side
    faces on the straight segments of drafted profiles with curves.
  - Static OCCT linking is for development and CI only; installers ship the shared build
    (F3).
  - The engine workflow's first run on all four targets is the cross-platform check of
    the new crates; record its result here.
- 2026-11-13 **Windows first; macOS a nice-to-have (owner decision).** The owner: "mac is
  just a nice to have. windows first", assuming the Mac build will work. Recorded in goal
  4, section 1 ("Desktop platforms"), the P2 entry and the Phase F gate.
  - Phase F is gated on P2 on Windows (passed), E6 and P4. The real-Mac P2 run and the
    two-monitor mixed-DPI check no longer gate F1 or F3.
  - macOS keeps building (OCCT, crates, CI when Actions runs) and is expected to work:
    P2's transparent WKWebView over Metal passed in macOS CI. A Mac problem found later is
    handled for macOS alone; the Electron fallback no longer applies to every OS.
  - The multi-monitor and HiDPI parts of P2's pass criterion are still checked on Windows
    before F3.
- 2026-11-13 **E5 done.** Parallel regeneration moved to G3 by owner decision (below). Crate
  `crates/oc-graph`, on branch `claude/e5-oc-graph-569e66` (not merged, not pushed). Run:
  `OCCT_DIR=C:\occt\install-8.0.1 cargo test -p oc-graph`.
  - **Modules** (each under 600 lines; 3 800 lines of source in all):
    - `sketch/*`: ports of `extractProfiles`, `regionAtPoint`, `splineThrough`,
      `expandChain`, `orderChains`, `offsetEntities` (Thin Feature walls), sweep paths,
      and sketch model references (`refs.ts`), re-solved with `oc-sketch`.
    - `refgeom`: sketch planes, reference planes, axes, points and coordinate systems.
    - `features/*`: every feature type, built on `oc_kernel::Kernel` the way `engine.ts`
      builds it, with its error and warning texts. Extrude has every end condition,
      From, Direction 2, draft, thin and contours. Patterns have Instances to Skip and
      fillet/chamfer seeds re-applied per instance.
    - `resolve`: face and edge references found by signature (`findFace`,
      `resolveEdges`) behind a `Resolver` trait. E6 replaces the implementation;
      features do not change.
    - `graph`, `regen`: the DAG, the cache, cancellation and timings.
  - **DAG and cache.** `graph::analyse` takes edges from stored references (sketch →
    extrude, reference plane → sketch, seeds → pattern, ...) plus the **body chain**:
    every feature that changes the body depends on the previous one, and features that
    read the body (sketch on a face, sketch with model references, references to edges
    and faces) depend on the body before them. References to later, suppressed or
    missing features are not edges; the feature sees them as absent, as in TypeScript.
    Tree order is therefore a topological order.
    - `Regenerator` keys each node by a 128-bit FNV-1a hash of its feature (canonical
      JSON, memoised per `Arc` so unchanged features are hashed once), its dependencies'
      keys and the key of the body it starts from.
    - An edit recomputes only what reads it. The TypeScript engine keys by tree prefix and
      recomputes everything after the edit. Example (`graph.rs`): making `refgeom`'s first
      boss taller keeps 8 of 18 features cached (sketches on standard planes, reference
      geometry from vertices and planes); editing an unused reference point recomputes
      only that point.
    - **Debug builds check every read against the declared dependencies** (`check_reads`
      in `regen.rs`), so a missing edge fails tests instead of serving a stale cache
      entry. It runs on the whole corpus.
    - A failing feature reports its error and passes its input body on; later features
      continue. Suppressed features pass the body on. `regenerate_doc` respects rollback.
  - **Cancellation and timings.** `CancelToken` (an `Arc<AtomicBool>`) is checked before
    each feature. A cancelled rebuild returns `Cancelled { done }`, and the finished
    features stay cached for the next rebuild. A kernel call in progress runs to its end:
    OCCT's `Message_ProgressRange` user break would make long operations interruptible
    (a later kernel addition). Each `Step` carries its time and whether it came from the
    cache. `regenerate_observed` reports every step as it finishes.
  - **Pass result: all 17 corpus parts match the phase-0 oracle** (`tests/corpus.rs`): 140
    steps, 124 with a body.
    - Volume, area and bounding box match to the oracle's six significant figures. Every
      compared value is exact after rounding; none needs the rounding-boundary allowance.
    - Solid, face and edge counts are exact. Statuses are equal (no errors, no warnings).
    - Reference geometry matches too: sketch frames, planes, axes, points, coordinate
      systems.
    - Exceptions, each recorded below with a reason: two corrected oracle values and one
      axis anchor point.
    - Mutation-checked: measuring an unequal chamfer on the other face changes 5 values.
      Dropping the mirror's seed dependency makes the debug dependency check panic.
  - **Other tests** (`tests/graph.rs`):
    - a warm rebuild is entirely cached and identical;
    - an edit recomputes only what reads it;
    - the dependencies match the expected ones;
    - cancellation;
    - a failing feature passes its body on, with the TypeScript messages (open sketch, no
      body to cut, missing sketch, sketch later in the tree);
    - suppression and rollback;
    - cold rebuilds give identical keys and topology;
    - sketch references follow a widened block and re-solve, and warn when not found.
  - **Kernel trait change (kernel-neutral, commit `07cae3b`).**
    - `Kernel::solids(body)` splits a body into its solids. They are ordered by their
      lowest face index, and each one's history covers every input face (`Same` or
      deleted).
    - `Target::Segment(a, b)` is a distance target.
    - Rib needs both: the TypeScript engine keeps the slab pieces that touch the sketch
      line. Up To Body needs `solids` too.
    - In the OCCT shim: `make_segment`, `solid_count` and `solid`, with tests in
      `queries.rs`.
  - **Deviations and fixes.**
    1. **Up To Body now stops at the body** (the 0.2 known divergence).
       - The document does not store which body was picked, so the part's body is used:
         the through-all prism minus the body, keeping the pieces that touch the profile.
       - If nothing is kept, or the kept piece runs the full through-all length, the
         feature fails with "The extrusion does not reach the body" (wording to confirm,
         🔍). Without a body it is Through All.
       - The test corrects the corpus oracle (`through-mid-dir2`) analytically. The
         10 × 10 profile at z = 40 stops at the top face z = 10: +3000 mm³, 28994.7 →
         31994.7.
       - The oracle's prism runs 114.288 mm further below the body: 11428.8 mm³,
         4571.52 mm² of side faces, 5 faces and 12 edges, in every later step. Every later
         step matches after that correction.
    2. **Mid plane between two faces: TypeScript bug fixed.**
       - `refPlaneFrame` averages plane A's origin with B's origin projected onto A,
         which lands on A.
       - `refgeom` Plane2, between faces at z = 0 and z = 20, is recorded at z = 0.
         oc-graph puts it at z = 10.
       - The oracle is corrected for that one value; nothing reads Plane2.
       - **The current app still has the bug** (`src/kernel/engine.ts` `refPlaneFrame`,
         outside this package): follow-up.
    3. **Unequal chamfers** (E3 finding 3): the first distance is measured on the face to
       the left of the edge as it runs start → end. That is the face whose normal turns
       counter-clockwise into the other face's normal about the edge direction. This
       reproduces Chamfer2 and Chamfer3; `flip` takes the other face. The normals are the
       faces' signature normals, so for curved faces near the edge this is approximate.
    4. **Axis from a cylindrical face:** TypeScript anchors it at replicad's first
       circular edge of the face (top rim for `refgeom` Axis3), oc-graph at the
       lowest-indexed one (bottom rim). The line is the same, so the test compares axes as
       lines (direction to six figures, point on the oracle's line). Patterns and revolves
       give identical results.
    5. Face signature matching normalises the stored normal. replicad stored the unscaled
       surface derivative cross product (length 8 for an r = 8 cylinder), so the normal
       term of the score was weighted by r² in TypeScript.
    6. Kernel failures read as the TypeScript engine's OCCT-exception text ("The geometry
       could not be built with these settings"), or its invalid-solid text for
       `InvalidResult`. replicad's own messages are not reproduced; the kernel detail is
       kept in `FeatureError::detail`.
    7. A failing feature contributes no outputs. TypeScript kept what it had written
       before throwing, e.g. an extrude's tool when the cut found no body.
    8. The kernel validates every operation, including tool bodies and intermediate
       booleans. TypeScript checked only a feature's final body.
    9. A circular sweep profile on a closed circular path starts where OCCT's
       `gp_Ax2(centre, normal)` puts a circle's start; there is no corpus case.
  - **Parallel regeneration: not implemented in E5; moved to G3 by owner decision**
    (2026-11-13, option (b) below).
    - Measured (release, `graph.rs` `timings`, Ryzen 7 7800X3D): the whole corpus
      rebuilds cold in 529 ms.
    - Nodes that need nothing from the body chain (frames and reference geometry) take
      **0.049 ms of that, 0.009 %**. The rest is the body chain.
    - A warm rebuild takes 0.003 to 0.017 ms per part. The slowest single feature is
      `patterns` Mirror2 at 41 ms.
    - In the current document model each part has one body chain. Every body feature
      reads the previous body (a boolean with it, its bounding box for Through All, its
      faces for Up To Next), so there are no independent kernel branches within a part.
    - The only parallel kernel work available is building a feature's tool body (prism,
      revolve, sweep) ahead of its boolean. That needs bodies that can cross threads, and
      E3 made bodies `!Send` (OCCT shapes share mutable caches). Geometry hosts also need
      a body transfer to hand a tool from one host to another.
    - Options for the owner:
      - (a) Add a native body transfer to the kernel trait (`export_native` /
        `import_native`: OCCT `BinTools`, Parasolid XT). Build tool bodies on worker
        hosts and booleans on the part's host. This is worth it for heavy sweeps and
        lofts, not for the corpus.
      - (b) Parallelise across parts and bodies (G3) and leave single-part regeneration
        serial.
      - (c) Multibody feature scope in the document model, which would create
        independent body chains.
    - Recommendation: (b) now, and (a) together with the geometry-host package.
    - **Owner decision (2026-11-13): option (b).** Single-part regeneration stays serial.
      Parallelism across parts and bodies is G3 (its line updated). Option (a), native body
      transfer so tool bodies are built on worker hosts, is done with the geometry-host
      work.
  - **Not done here (later packages):**
    - previews (`toolMesh`): each feature's `NodeOut` keeps its tool body and instance
      markers for F2;
    - meshes (E4);
    - the worker requests for export, measure, mass properties and Convert Entities (F2);
    - the torture scripts (E6).
  - **Follow-ups outside the package:**
    - the TypeScript mid-plane bug (item 2);
    - the TypeScript app's Up To Body still runs through all, so the FEATURE_SPEC 🟡 row
      stays until F3.
- 2026-11-13 **E4 done.** Crate `crates/oc-mesh` (dependencies `oc-kernel`, `rayon`), plus
  kernel changes in `oc-kernel` and `oc-kernel-occt`. Run:
  `OCCT_DIR=C:\occt\install-8.0.1 cargo test -p oc-mesh`. Timings:
  `cargo test --release -p oc-mesh --test timings -- --ignored --nocapture`.
  - **API.** `oc_mesh::build(kernel, body, &MeshOptions { quality, max_lods, threads })`
    returns a `RenderMesh`. `pack(levels, topology, origin)` is the data-only half. It
    takes kernel `Mesh`es, and the geometry host or a cache can call it directly.
    - Level 0 is the Image Quality: `Tolerances::display_mesh(quality, size)`, the
      current viewer's formula.
    - Level `k` is `Tolerances::display_lod(quality, size, k)`: 4^k times the chordal
      deviation and 2^k times the angle, capped at 1 rad. Both factors are constants in
      the tolerance policy.
    - A level is kept only if it has at most 3/5 of the previous level's triangles. A box
      therefore has one level.
    - `RenderMesh::select_lod(mm_per_px, max_error_px)` returns the coarsest level whose
      deviation stays under the pixel budget. It is never finer than level 0.
  - **Buffer layout** (decided from P4's shaders; documented in `oc-mesh/src/layout.rs`):
    - **Vertex, 20 bytes:** `Float32x3` position relative to the body's `f64` `origin`
      (camera-relative rendering, P4), `Snorm16x2` octahedral normal (error below 1e-4
      rad, WGSL decoder in the docs), `Uint32` face index. P4 used 32 bytes with an `f32`
      normal. A face's vertices are contiguous and never shared with another face, so
      creases stay sharp and a face is one vertex range.
    - **Index buffer:** triangle list, counter-clockwise from outside, grouped by face.
      It is `Uint16` up to 65 536 vertices per level and `Uint32` above, padded to 4 bytes.
      One draw per body and level.
    - **Edge segment, 32 bytes:** start, `u32` edge index in the `w` slot, end, reserved.
      It reads as P4's `struct Seg { a: vec4f, b: vec4f }`, and the edge id comes from
      `bitcast<u32>(a.w)` for picking. Seam and degenerate edges have no segments.
    - Per level there are `FaceRange` (vertex and index ranges) per `FaceId` and
      `EdgeRange` per `EdgeId`. Model vertices (`points`) are included for vertex picking.
    - Also included: edge flags (`EDGE_SEAM`, `EDGE_DEGENERATE`), a bounding box and
      sphere relative to `origin`, and point flags (below).
    - `RenderMesh::to_bytes`/`from_bytes` produce one little-endian blob: a header, the
      ranges, then each GPU buffer as is, each starting 4-byte aligned. This is what the
      host puts in shared memory (P3). `content_hash` is FNV-1a 64.
    - **Mapping from today's `BodyMesh`:** `positions`/`normals` → vertex buffer;
      `indices` → index buffer; `faceGroups` → `faces` (vertex and index ranges, where TS
      had index ranges only); `edgePositions`/`edgeGroups` → segments and `edges`;
      `vertices` → `points`; `bbox` → `bounds` + `origin`. Face and edge ids are
      `oc-kernel`'s canonical indices, not replicad's order (E3 finding 3). F2 sends
      `topology()` alongside for signatures.
  - **Parallelism.**
    - OCCT meshes edges, then faces, each in parallel (`OSD_Parallel`).
    - `oc-mesh` packs each face and each edge as a rayon task: compaction, normals,
      encoding. Faces then write disjoint slices of the shared buffers in parallel. The
      output is concatenated in face order, so the bytes do not depend on scheduling.
    - `Body` is not `Send`, so kernel calls stay on the caller's thread.
    - **Kernel trait addition:** `Kernel::set_threads(n) -> KResult<()>`. It is
      kernel-neutral (Parasolid has an SMP setting) and may be process-wide.
    - **Finding:** `OSD_Parallel::For` caps a job at the pool size and ignores
      `SetNbDefaultThreadsToLaunch`. The OCCT implementation therefore resizes the default
      pool (`OSD_ThreadPool::Init`), which throws while a job runs. That error is returned
      as `Failed`. The thread-count test is in its own test binary (`tests/threads.rs`).
  - **Kernel tessellation fixes** (`oc-kernel-occt`, now in `cpp/mesh.cpp`, moved out of
    `exchange.cpp`). Each fix has a test that failed first or was mutation-checked:
    1. **Warm-cache dependence.** OCCT stores triangulations on faces and edges, which
       bodies share, and reuses a stored one within ±10 % of the requested deflection.
       E3's test only covered "finer, then coarser". A mesh at 0.05 followed by one at
       0.054 returned the 0.05 mesh, and a body sharing faces with an earlier meshed one
       came out differently. `tessellate` now runs `BRepTools::Clean` before meshing and
       again afterwards. The second clean also stops cached bodies from keeping
       triangulations alive, which E5's cache would otherwise accumulate.
    2. **Exact normals.** Normals were facet averages (`Poly_Triangulation::
       ComputeNormals`, as replicad does); at a sphere's pole one was off by about 10°.
       They are now the surface normal at each node's (u, v)
       (`BRepLib_ToolTriangulatedShape::ComputeNormals`), with facet averages only where
       the surface has none.
    3. **Shared boundary nodes.** Two faces' nodes at the same vertex differed by about
       1e-14 mm (each face evaluates the vertex through its own parametrisation). In the
       corpus `patterns` part this left a 4-edge hole in the welded mesh. Each edge now
       has canonical points: its polygon on its first face in body order, with the ends
       moved onto the vertex points. Every face bounded by the edge uses those points
       for its boundary nodes, on both sides of a seam. The trait now promises boundary
       nodes bit-identical to the edge polyline, so meshes weld without a tolerance.
    4. **Algorithm.** The default `MeshAlgo` reads the `CSF_MeshAlgo` environment
       variable (a determinism hole), so it is now set explicitly. Watson, the default,
       takes time growing about as n^1.5 on one face with many holes: a plate with 432
       holes spent 3.3 s on its top face. Planar faces now use Delabella (a custom
       `IMeshTools_MeshAlgoFactory`) and all other faces Watson; that plate takes 0.29 s.
       Delabella alone left faces empty (an elliptic extrusion side always, and one
       revolve plane at quality 10, level 1). So if any face is left without triangles,
       the whole body is meshed again with Watson. The choice depends only on the result,
       so it is deterministic. The fallback was mutation-checked: without it, the
       corpus test fails on `revolve`.
  - **`display_box`.** The size in the quality formula is the union of the faces' boxes:
    tight for planes, OCCT's fast box for curved faces, as the viewer's replicad
    `boundingBox` was. It is not `Kernel::bounding_box`, whose exact `AddOptimal` took
    2.3 s of a 4.3 s build on the large part (E3 finding 6). On the corpus it contains
    the exact box and is at most 10 % larger (tested).
  - **Point flags.** `POINT_HIDDEN` marks vertices whose visible edges are all closed
    (a full circle) or which touch only seams or poles. SOLIDWORKS has no vertex on a
    full circle; today's viewer shows one. F1 decides, so mark it 🔍 there.
  - **Tests** (`oc-mesh`: 2 unit, 9 integration plus the ignored timing report;
    `oc-kernel`: 1 new unit test for `display_lod`; `oc-kernel-occt/tests/queries.rs`: 3 new):
    - `mesh.rs`, on 8 corpus final bodies rebuilt with kernel calls and checked against
      `corpus/*.json` face and edge counts (block-hole, pad-pocket, shell, revolve,
      sweep-loft, curved-profiles, patterns, imported). Each is run at quality 0, 3 and 10
      over every level:
      - **Watertight:** welded by exact f32 position, every edge is used once in each
        direction.
      - **Outward:** the signed volume is positive and within area × deviation of the
        exact volume.
      - Area is within −10 % / +1 %.
      - Layout invariants: ranges tile the buffers, each vertex carries its face id,
        indices stay inside their face, the index width is right, seams have no segments.
      - Every segment end is a vertex of a face bounded by that edge (bit-equal).
      - **Normals:** unit length; on planes the face normal; on cylinders and spheres
        radial, with one sign per face (within 2e-4); each triangle winds the same way
        as its vertex normals.
      - **LOD:** a box has one level; a sphere has ≥ 3 levels, each ≤ 3/5 of the previous
        triangles, each within its deflection of the exact sphere. Also selection, and
        `u32` indices above 65 536 vertices.
    - `determinism.rs`: the same bytes after meshing at other qualities first, mutation-
      checked (without the clean it fails); blob round trip, alignment, truncated, wrong
      magic and version.
    - `threads.rs`: the same bytes on all 8 parts, a sphere and a 30-hole filleted plate,
      with 1, 4, 16 and all threads (kernel pool and packing), run after run.
    - Kernel: `tessellation_is_independent_of_earlier_meshes`,
      `tessellation_normals_are_exact` (both failed before the fixes),
      `tessellation_shares_boundary_nodes_exactly` (fails with the override disabled).
  - **Measurements** (release, Ryzen 7 7800X3D, 16 threads unless stated; `build` =
    topology + all levels + packing):

    | part | faces | triangles per level (quality 3) | kernel L0 ms | pack ms | build ms | blob MB |
    |---|---|---|---|---|---|---|
    | block-hole | 7 | 352 / 184 / 100 | 1.5 | 0.15 | 5.0 | 0.02 |
    | patterns | 98 | 9 540 / 4 852 / 2 508 | 14.9 | 0.5 | 45.9 | 0.7 |
    | sweep-loft | 18 | 5 762 / 1 516 / 474 / 232 | 19.0 | 0.3 | 31.7 | 0.2 |
    | plate, 432 holes | 438 | 146 892 / 74 316 / 38 028 | 311 | 10.9 | 708 | 12.2 |
    | large, quality 3 | 887 | 1 677 159 / 474 495 / 129 219 / 58 043 | 1 458 | 26.6 | 2 089 | 62.4 |
    | large, quality 6 | 887 | 6 957 459 / … / 135 987 | 6 820 | 110 | 9 285 | 230 |
    | large, quality 8 | 887 | 17 837 569 / … / 344 985 | 25 400 | 326 | 31 088 | 567 |

    The other corpus parts take 4.8 to 8.8 ms to build. The large part is a 400 × 300 ×
    10 plate with 432 Ø8 holes, rim fillets r 1 and a revolved dome; it takes 2.7 s to
    model.
    - **Before the fixes**, on the large part at quality 3: kernel L0 4.3 s and build
      8.7 s. The same triangles now take 1.5 s and 2.1 s.
    - **Threads** (large part, quality 6): kernel L0 takes 30.4 s on 1 thread, 8.9 s on
      4 and 5.9 s on 16. Packing takes 222, 110 and 98 ms. A whole build takes 41.4, 12.2
      and 7.7 s.
    - **Blob:** `to_bytes` takes 61 ms, `from_bytes` 25 ms and FNV-1a 212 ms for 230 MB.
      FNV is byte-serial: switch to a faster fixed hash if dedup hashes large meshes.
    - **Kernel overhead:** a small part now takes about 1.4 ms per kernel mesh, where E3
      measured 0.5 ms. The cost is the two cleans, the custom context, exact normals and
      the boundary pass.
  - **Findings for later packages.**
    - **Torus fillets dominate the triangle count:** 91 % of the large part's triangles,
      about 3.5k per r 1 fillet at quality 3. That is roughly 4× what the angular
      tolerance implies, because OCCT's Watson mesher refines tori in both directions.
      Today's viewer uses the same mesher and has the same count. A dedicated tessellator
      for analytic surfaces (P3's suggestion) would cut the count by about 4×. Do it in
      G2/F1 if large parts are slow to draw.
    - High quality on large parts is slow in the kernel (25 s at quality 8), as P3 found
      for single faces. F2 should mesh level 0 first and the coarser levels in the
      background, cache meshes by (body hash, parameters), and possibly cap quality on
      huge bodies.
    - Not done: tangent (smooth) edge flags for SOLIDWORKS' tangent-edge display modes.
      The segment's reserved word is free for it; compute it from both faces' normals
      along the edge.
  - **Deviations.**
    - Files outside the package: the `oc-kernel` trait (`set_threads`,
      `tessellate` contract, `Mesh::normals` doc), `Tolerances::display_lod`,
      `oc-kernel-occt` (`cpp/mesh.cpp`, `exchange.cpp`, `shim.h`, `ffi.rs`, `lib.rs`,
      `build.rs`, `tests/queries.rs`), `docs/THIRD_PARTY.md` (rayon family),
      `docs/ARCHITECTURE.md` and `crates/README.md`.
    - Tests were written first for the kernel fixes. The `oc-mesh` tests were written
      alongside the crate and mutation-checked as listed above.
    - The E3 crates are not rustfmt-clean; only `oc-mesh` was formatted.
    - Tested on Windows only; macOS and Linux wait for `engine.yml`.
- 2026-11-13 **E4 and E5 merged into main** (owner approved). Both extended the kernel
  trait (`Kernel::set_threads` from E4, `Kernel::solids` and `Target::Segment` from E5);
  the merged workspace builds and passes its tests and clippy on Windows. macOS and Linux
  CI waits for GitHub Actions billing. Next: E6.
- 2026-11-13 **P1 spike code removed** (owner asked for the clean-up). `spikes/p1-occt`
  (its C++ shim, build scripts and history test) is gone: `crates/oc-kernel-occt` replaces
  it, and its results stay in this log and `spikes/results/p1.txt`. The last commit with
  the spike is `e34c811`.
  - `spikes/p3-host` now meshes its sphere through `oc-kernel-occt` (the spike used P1's
    shim); rerun on Windows: 1 038 472 triangles transferred in 2.4 ms median, crash,
    abort and hang all recovered.
  - `spikes.yml` keeps only P2 on Windows and macOS. The P1 jobs are gone (the Engine
    workflow builds OCCT), and so is the P2 Linux job, which only documented the overdraw.
  - The only C++ left in the repository is the kernel shim, `crates/oc-kernel-occt/cpp`
    (about 1 850 lines).
- 2026-11-14 **E6 done; review checkpoint.** Crate `crates/oc-naming` (dependencies
  `oc-kernel`, `oc-doc`, `smallvec`), wired into `oc-graph`; schema version 2 in `oc-doc`
  and its TypeScript mirror. Branch `claude/e6-oc-naming-e9a723`, not merged, not pushed.
  Run: `OCCT_DIR=C:\occt\install-8.0.1 cargo test -p oc-naming -p oc-graph`.
  - **Pass criteria.**
    - **Torture scripts: 9 of 9 pass** (`oc-graph/tests/naming.rs`, TT1a to TT8, replayed
      as the app would: rebuild, write the names back, edit, rebuild). Each is compared
      with its reference per step (error, body metrics, reference geometry) and by the
      final face list, as `torture.ts` does.
    - The same edits with references found **by signature alone** (the legacy path) give
      exactly the TypeScript engine's recorded outcomes: TT1b, TT2, TT3, TT4 and TT5
      fail, the other four pass. The test asserts that TT2 to TT5 fail that way, so the
      cases keep testing naming. (TT1b at height 40 is an exact tie between top and bottom
      by signature score; face order decides it in both engines.)
    - **Zero `/new/` tags across the corpus**: 1 772 named faces over the 124 bodies of
      the 17 parts, and none on any intermediate body either (`Rebuild::untracked`, which
      counts every body a rebuild makes). Tags by role: side 1 121, end 337, start 232,
      wall 34, fillet 21, import 21, chamfer 18, tip 12, offset 11, cbore 6, cboreFloor 6,
      csk 5, edge 1. Surface identity (section 3.2 step 3) was never needed.
    - **Determinism** (`tests/threads.rs`, its own binary): the 17 parts and 9 torture
      documents, each rebuilt as a legacy document and again by name, give identical
      names, statuses, cache keys, metrics and face and edge geometry (bit for bit) with
      1, 4, 16 and all kernel threads. TT6 adds cold, warm and cold on another
      regenerator.
    - **E5 parity still holds**: `tests/corpus.rs` is unchanged and passes. In addition
      every corpus document is healed and rebuilt cold on a fresh regenerator, so every
      reference is resolved by name: all 17 still match the oracle, with no warnings.
  - **What was built.**
    - `oc-naming`: `Tag`, `FaceName`, `EdgeName`, `VertexName`, `BodyNames` as in section
      3.1; `track` (3.2); `resolve::{face, edge, vertex}` (3.3) returning how the reference
      was found (`Name`, `Split`, `Position`, `Legacy`); `signature` (the scores and the
      nearest-signature search, moved here from `oc-graph`); `hash` (3.4): **FNV-1a 64**,
      keys as 16 hex digits, the byte encoding documented in the module.
    - `oc-graph/src/ops.rs`: every modelling call goes through it and names the result
      from the operation's history. `Shape` carries `names`; tools, pattern instances and
      intermediate bodies are named too. Features pass the sketch entity id of every
      profile segment and the loop key of every cap.
    - `oc-graph/src/resolve.rs` replaces the signature `Resolver` (the trait and
      `Regenerator::with_resolver` are gone; `with_rules` takes the matching limits).
      `Ctx::find_face/find_edge/find_vertex` resolve on the live body. Reference geometry
      (`plane_of`, `direction_of`, `point_of`, `axis_of`), Up To Vertex and the Hole
      Wizard's placement face no longer read the stored signature: they use what the
      reference resolves to. A picked edge's stored direction only decides which way the
      live edge runs.
    - **Heal** (3.3 step 4): a rebuild returns `Rebuild::heals`, each a feature with the
      names of its legacy references written in. `apply_heals(doc, heals)` inside
      `oc_doc::History::heal` writes them (tested: no undo step, not dirty). A feature is
      replaced only if it is still the one that was rebuilt. The cache stores a healed
      feature's result under the key the healed feature will have, so rebuilding the
      healed document is a full cache hit (tested on every corpus part).
    - `oc-doc` **schema version 2**: optional `name` on `FaceSig` and `EdgeSig`, on
      `GeomRef::Vertex` and `SketchRef::Vertex`, `vertexName` on `EndCondition`. The
      1 → 2 migration rewrites nothing; a reference without a name is a legacy one.
      `Feature::refs_mut` visits every topology reference. TypeScript: the same optional
      fields, `PartDocument.version: 1 | 2`, `src/core/migrate.ts` (load-time migration,
      refuses newer files), new documents written as version 2. The TypeScript engine
      ignores names; nothing was ported back into `engine.ts`.
    - Statuses: a missing reference is the error `Missing reference: <what>`; a list of
      which some are missing gives the warning `Missing reference: 1 of 4 fillet edges`
      and the feature uses the rest; a reference found only by position gives `A
      reference was re-attached by position`. Wording 🔍 in FEATURE_SPEC.
  - **Kernel trait change (kernel-neutral, commit `0cc9e1a`).** `History::count`: the
    number of output entities. Naming needs it to know which faces no source maps to;
    without it every intermediate body would need a topology query. Edge images were not
    needed: edges and vertices are named from faces (E3 review answer).
  - **Kernel histories found** (probed on OCCT, used for the naming policy):
    - Shell keeps each face `Same` and reports the wall on the other side of the material
      as `Generated` from it. Inward that is the inner wall; outward it is the new
      *outer* wall, the original face becoming the inside. The role is therefore
      `offset`, not `inner`. The open face is `Modified` into the rim and keeps its name.
      An outward shell rounds edges and corners (`Generated` from edges and vertices).
    - Draft reports the drafted face as `Generated`; it keeps its name.
    - A sweep along a path of several curves makes one face per path curve from each
      profile segment. They share a tag and are told apart by signature.
    - A loft reports each side face as generated from the matching segment of *every*
      section, so it carries one tag per profile.
  - **Tests** (all written against the pass criteria; mutation checks below):
    - `oc-naming`: 19 (hash vectors, name ordering, every `track` rule on hand-written
      histories with the faces in two different orders, every resolution step).
    - `oc-graph/tests/naming.rs`: 5 (torture; zero `/new/` plus heal round trip; pattern
      instance names when skipping and changing counts; a fillet on pattern instance 5
      stays there when instance 3 is skipped and reports `Missing reference` when instance
      5 is; every `side` tag checked against geometry: 88 lines lie in their face's plane
      and 11 circle centres on their cylinder's axis).
    - `oc-graph/tests/threads.rs`: 1. `tests/graph.rs`: 2 of 8 updated (below).
    - `oc-doc`: 2 new (version 1 loads as 2, names round-trip; `refs_mut` reaches every
      reference of the every-field fixture, which is now version 2 with names).
    - TypeScript: `src/core/migrate.test.ts`, 4 tests, one reading the Rust fixture.
    - Workspace: clippy clean, 154 Rust tests pass; `npm run typecheck` clean,
      88 JavaScript tests pass (corpus files unchanged).
    - **Mutation-checked:** without reversing a loop's entity list together with its
      segments, the side-tag geometry test fails (this was a real bug in `Loop.ents`,
      harmless before E6); without wrapping instance tags, both pattern tests fail;
      resolving by signature alone fails five torture cases (asserted in the test).
  - **Measurements** (release, Ryzen 7 7800X3D, `graph.rs` `timings`): the whole corpus rebuilds cold in
    531.7 ms with every body named (E5 recorded 529 ms without names): no measurable
    cost at this size, one run each. A warm rebuild takes 0.003 to 0.018 ms per part. The
    slowest feature is still `patterns` Mirror2 at 39.6 ms. Topology is queried only
    when naming needs it (an edge or vertex generated a face); `History::count` covers
    the rest.
  - **Deviations and decisions.**
    1. *(Superseded by review answer 1 below: TT1a now passes by name, with no warning.)*
       **TT1a passes through step 3, with a warning.** The script replaces every entity
       of the base sketch (ids `f1_l5..l8` become `f1_l7..l12`), so the cap's key, a hash
       of the loop's entity ids as section 3.1 specifies, changes. The pocket sketch finds
       no face sharing a tag and is re-attached by position (same plane, same normal),
       with the warning section 3.3 prescribes. The test asserts exactly this one
       warning. Implemented as written; see the first reviewer question.
    2. **Imported bodies are `<fid>/import/<rank>`, not `/new/`.** `read_step` has no
       history by nature, so every face would be `/new/` and the corpus target could not
       be met by the `imported` part. The rank is the `/new/` rank (surface type, rounded
       centre, rounded area); the data is stored in the document, so the ranks are
       stable. `/new/` keeps its meaning: a history gap.
    3. **TT3's reference face list**: the TypeScript engine's drafted box has B-spline
       side faces, the native one planes (E3 finding 2, accepted). The test maps that one
       surface type for that one case; centres and counts are compared unchanged. The
       corpus file was not edited.
    4. **Roles the table did not have** (shell, hole, rib, thin walls, two-direction caps,
       loft keys): listed under section 3.1 "As built".
    5. **Vertex references follow the model.** Reference geometry built from model
       vertices now depends on the body, so two E5 graph tests changed: in `refgeom`,
       editing the first boss keeps 4 features cached instead of 8, and Plane2 (mid plane
       of two faces) moves to z = 12.5 where E5, like TypeScript, left it at z = 10.
    6. *(Superseded by review answer 3 below: the origin is its own reference kind.)*
       **The origin.** Version 1 stores the origin as a vertex at (0, 0, 0), like a model
       vertex there. A legacy vertex at exactly the origin is treated as the origin and
       never named. A legacy vertex elsewhere is named only if a model vertex is at that
       position; otherwise it is a point in space and stays (5 such in the corpus).
    7. **Step 3 rules for edges and vertices** (the plan gives them for faces only): a
       straight edge must lie on the same line and overlap where the edge was; other
       curves must be within 25 % of the length in position and size; a vertex must be at
       the same point. Limits are `oc_naming::Rules`, in one place.
    8. A warning from step 3 stays until the reference is re-picked: the name is written
       back only for legacy references, as 3.3 says.
    9. Files outside the package list: `oc-kernel` (`History::count`), `oc-kernel-occt`
       (fills it), `oc-doc`, `src/core/types.ts`, `src/core/migrate.ts`, `src/app/app.ts`
       (two lines), `docs/THIRD_PARTY.md` (smallvec, MIT OR Apache-2.0), ARCHITECTURE,
       FEATURE_SPEC. Tests were written with the code, not strictly first; the mutation
       checks stand in for seeing them fail.
  - **Known limits.**
    - A fillet face's key hashes the names of its edge's two faces. If one of those faces
      later merges with another (its name gains a tag), the fillet face is renamed, and a
      reference *to the fillet face itself* falls back to position. References to the
      edge are unaffected (they match by shared tag).
    - A legacy document must be rebuilt once before it is edited for its references to
      get names. F2 has to apply `Rebuild::heals` after the first rebuild.
    - The PropertyManager needs to know *which* reference is missing to show
      `**Missing**`; today the status carries only the message. F2 can resolve a
      feature's references one by one with `oc_naming::resolve`.
  - **For the reviewer.**
    - Cap keys (deviation 1): when a profile is redrawn with new entities, should a cap
      still be found by name? A fallback tier "same feature and role, one candidate"
      between steps 2 and 3 would remove TT1a's warning and matches what SOLIDWORKS does
      for end faces. It is a resolution rule, not another naming scheme, but it is not in
      section 3.3, so it was not added.
    - `<fid>/import/<rank>` instead of `/new/` for imported bodies (deviation 2).
    - The origin rule (deviation 6): should version 2 store the origin as its own kind of
      reference, so a model vertex at the origin can be told apart?
    - Embedding `name` in `FaceSig`/`EdgeSig` rather than wrapping them: it keeps every
      TypeScript call site that copies `sig` working unchanged, at the cost of the type
      name no longer saying "signature only".
    - Partly missing lists warn and continue (as SOLIDWORKS does for a fillet with a
      missing edge) rather than failing the feature.
- 2026-11-14 **E6 reviewed; owner answers and follow-ups** (on the same branch; the
  entry above is as reviewed, with its two superseded items marked).
  - **Answers.**
    1. Cap keys: **add the fallback tier.** Done, below.
    2. `<fid>/import/<rank>` for imported bodies: accepted as is.
    3. The origin: **its own reference kind in schema version 2, before version 2 ships.**
       Done, below.
    4. `name` embedded in `FaceSig`/`EdgeSig`: accepted.
    5. Partly missing lists warn and continue: accepted.
  - **Follow-up 1: same feature and role (section 3.3 step 2a).**
    - `oc_naming::resolve`: when no live face shares a tag with the stored name and
      **exactly one** live face has a tag of the same feature and role (`Tag::stem`: the
      tag without its key, instance prefixes included), the reference is that face,
      reported as `How::Role`. For an edge or vertex, a stored face name with no
      tag-sharing face is stood in for by such a face.
    - No warning. The rebuild reports the reference's current name in `Rebuild::heals`,
      as it does for legacy references, so the stored key does not stay stale; `healed()`
      now replaces a name where the stored reference (old name included) is the one that
      was resolved.
    - **TT1a passes with no warning**; the torture test now asserts that no case warns. A
      new test follows TT1a through: the sketch's stored `f2/end/<old key>` becomes
      `f2/end/<new key>`, and the renamed document is a cache hit.
    - Unit test (`oc-naming/tests/resolve.rs`): one `f2/end/…` face stands in; four
      `f2/side/…` faces do not stand in for one another; an edge is found through a
      stand-in cap; two end caps give no stand-in; a face of another feature never
      stands in (TT5 still reports `Missing reference`).
    - It also softens the known limit about fillet keys: a feature's only fillet face is
      still found when its key changes.
    - **Consequence to be aware of:** the rule cannot know how many faces of that role
      the feature had before. If an extrusion had two profile loops and the loop under a
      sketch is deleted, the remaining loop's cap is now the only `end` face and the
      sketch moves to it without a warning. With both loops still present nothing stands
      in and step 3 decides.
  - **Follow-up 2: the origin.**
    - `GeomRef::Origin` (`{"kind": "origin"}`) in `oc-doc`, `{ kind: "origin" }` in
      `src/core/types.ts`. A `vertex` reference is now always model topology: in version
      2 an unnamed vertex at (0, 0, 0) is a model vertex and is named like any other.
    - **The 1 → 2 migration now rewrites:** every picked reference that is exactly
      `{"kind": "vertex", "p": [0, 0, 0]}` becomes the origin, the same rule the engine
      applied at resolve time before. Sketch references to vertices (they carry an `id`)
      and Up To Vertex (a bare position) are always model vertices in the app and are not
      touched. Same code in `crates/oc-doc/src/migrate.rs` and `src/core/migrate.ts`,
      same test cases on both sides.
    - `oc-graph`: the special case for a vertex at exactly the origin is gone; the origin
      resolves to (0, 0, 0) and does not read the body. In the corpus, `refgeom` Axis2's
      first reference is the origin after loading, leaving 4 references without a name,
      all points that are not on the model (`patterns` 2, `refgeom` 2).
    - App call sites: picking the origin stores `{ kind: "origin" }` (`app.ts` `refOf`);
      the point tests in `engine.ts`, `RefGeomPM.tsx`, `featureParams.ts` and the revolve
      axis check accept it; `ui/refs.ts` labels it "Origin". The TypeScript engine treats
      it as the point (0, 0, 0), as it treated the vertex. Measure uses its own protocol
      type and is unchanged.
    - The every-field fixture has an origin reference and keeps a version 2 vertex at
      (0, 0, 0). The oc-doc round-trip test now compares each corpus file with its JSON
      run through `MIGRATIONS` (the migration is no longer the identity).
  - **Results after the follow-ups** (Windows): workspace clippy clean, 157 Rust tests
    pass (`oc-naming` 20, `oc-doc` 34, `oc-graph` naming 6); `npm run typecheck` clean,
    89 JavaScript tests pass; corpus parity, zero `/new/` and determinism at 1, 4 and 16
    threads unchanged. Corpus files are unchanged (still version 1).
  - Not checked in the browser: the app changes are the two pick sites and the file-open
    path, covered by typecheck and unit tests only.
- 2026-11-15 **F1 done** (branch `claude/f1-oc-render-5ef421`, not merged). New crate
  `crates/oc-render` (wgpu 30.0.1, glam, bytemuck, pollster; licences in
  `docs/THIRD_PARTY.md`). No other crate changed. Not wired into Tauri or the React app (F2).
  - **Shape.** `Renderer` (own headless device, or the caller's device for F2's surface)
    draws a `Scene` with a `Camera`:
    - `Scene` is a plain description rebuilt from app state: display style, bodies
      (uploaded `MeshId`, colour, transparent, role `Part` or `Preview { cut }`, LOD choice,
      fillet/chamfer preview faces), planes (frame, extents, label image), axes, points,
      coordinate systems, inactive sketches as world segments, origin, section plane,
      `hover` and `selected` as `Hit`s, and `Options` (colours and switches with the app's
      defaults).
    - Rules that need the document (which planes are shown, consumed sketches, hidden
      bodies) stay with the caller. Helpers ported for F2: `planes::auto_extents`,
      `label_rect`, `border_width`, `fill_opacity`, `ref_axis_length`.
    - `Camera` ports `camera.ts` in `f64`: orthographic default, perspective 30°, standard
      views, fit (12 % margin), rotate about a pivot, pan, zoom about the pointer, roll,
      eased transitions, `project`, `ray`. View undo and the view selector are not ported.
    - `oc-mesh` buffers are uploaded as they are: the 20-byte vertex is the face pipeline's
      vertex buffer and edge segments are read from a storage buffer (P4's wide-line
      expansion). Two derived buffers per level: cumulative edge length at each segment
      (for dashes), and model vertices as `vec4` with their flags.
    - Highlights are a `u32` state per face and per edge in storage buffers, rewritten each
      frame (a few KB), instead of recolouring vertices as three.js does.
    - Camera-relative `f32`. Depth is reversed into `Depth32FloatStencil8` (24-bit stencil
      format where the adapter lacks it), with near and far fitted to the scene's bounds.
      This is P4's "phase F uses reverse-Z" note.
  - **Parity checklist against `src/render/viewport.ts`** (✅ same, ≈ same up to the
    noted difference, ⬜ not built):
    - ✅ Shaded With Edges: lit faces plus 1-device-pixel edges in `colors.edges`; faces
      carry a depth bias so their edges win (three.js `polygonOffset 1,1`).
    - ✅ Shaded: no edges. Highlighted edges are still drawn, as today's separate
      fat-line object does.
    - ✅ Hidden Lines Removed: flat white faces, visible edges only.
    - ✅ Hidden Lines Visible: hidden edges `#8a8a8a` behind the faces, dashed 6/4 px or
      solid (`display.hiddenEdges`).
      - ≈ The dash phase runs continuously along each edge in pixels. Today's dashes are
        world-sized at the zoom where the style was set.
    - ✅ Wireframe: no faces drawn or picked; every edge drawn and pickable through the body.
    - ✅ Transparent body (`doc.display.transparent`): 45 % opaque, no depth writes, still
      occludes in picking.
    - ✅ Preview body: `tempShaded` lerped 25 % to `#c8b400`, 42 % opaque (55 % and drawn
      over everything for cuts), yellow `#f0e000` 1.6 px edges over everything, not
      pickable, clipped by the section.
    - ✅ Fillet/chamfer preview faces tinted `#f0dc3c` 75 %.
    - ✅ Face highlights in linear light, as `THREE.Color.lerp`: selected 65 % towards
      `colors.selected`, hover 50 % towards `colors.hover`. A selected face is not
      hover-tinted. Off with `display.shadedFaceHighlight`.
    - ✅ Hovered and selected edges: 3 logical px in the hover or selection colour. Hover
      wins over selection, as today's draw order does. Hidden highlighted edges are not
      shown.
    - ✅ Vertex markers: 8 px squares over everything, selection or hover colour.
    - ✅ `display.dynamicHighlight` off: no hover on faces and edges; planes, sketches and
      references still react, as today.
    - ✅ Lighting: hemisphere (sky white, ground `#8a8f98`, 1.25) plus two directional
      lights fixed to the camera (1.6 from view (0.35, 0.55, 1), 0.5 from (−0.6, −0.3,
      0.5)). Material roughness 0.42, metalness 0.08, two-sided.
      - ≈ The BRDF is three.js' physically based model (Lambert/π, GGX, Smith correlated,
        Schlick) without its screen-space geometry-roughness term, so pixels are close to
        today's but not identical. Output sRGB, no tone mapping.
    - ✅ Background: Scene mode (CSS radial gradient at 50 %/45 %, sRGB interpolation,
      farthest-corner ellipse), Gradient, Plain. The webview is now over the viewport, so
      the renderer draws it.
    - ✅ Reference planes:
      - fill in Plane Display front/back colours at (100 − transparency) %, at least 3 %,
        no depth writes, off with `display.shadedPlanes`;
      - border `#6c9be0` 1.1 px, hover orange 2.4 px, selected blue 1.6 px;
      - name label in the plane at the top-left corner, 7.5 % of the short side (≈ see
        "Labels" below);
      - pick quad with 1.5 px radius. As today, a shown plane in front of the body hides
        faces behind it from picking.
      - ⬜ The 8 resize handles stay a 2D overlay. The renderer has no handle positions
        API yet; F2 computes them as today.
    - ✅ Reference axes: `#2a62c9`, dashed 14/5 px, 0.6 × model diagonal (at least 10 mm),
      hover and selection colours.
    - ✅ Reference points: 7 px squares over everything; 11 px in picking.
    - ✅ Coordinate systems: 40 px red/green/blue arrows over everything, selection colour
      when selected (no hover colour, as today).
    - ✅ Inactive sketches: `sketchInactive`, hover and selection colours. The previewed
      profile is `#141414` over everything.
    - ✅ Part origin: blue `#1c3fd6` arrows of 24 px model length with heads and a 2.2 px
      dot, 1.4 px (2 px when hovered or selected), over everything. Picked first by
      screen distance (5 px) when shown and no sketch is open, as today.
    - ✅ Reference triad: bottom left at (46, h − 52), 30 px arms, back to front, arrowheads.
      ≈ The X/Y/Z letters are stroked glyphs (no font dependency).
    - ✅ Section View: clip plane on bodies, edges, previews and pick geometry (not on
      planes, sketches or references, as today). The cap uses the stencil parity of all
      part bodies and is drawn lit, model colour × 0.85, roughness 0.6. The smoke test
      checks the slot through the hole is not capped.
    - ✅ Picking:
      - 13 × 13 logical-pixel ID window around the pointer;
      - nearest pixel per kind;
      - priority and radii: origin 5 (before the ID pass), vertex 5, edge 4, sketch 4,
        face 1.5, reference 4, plane 1.5 px;
      - a selection filter keeps only its kinds and doubles every radius;
      - Chamfer (edges only) skips vertices and faces;
      - occlusion from the depth buffer, with pick faces biased behind edges and vertices
        (three.js `polygonOffset 2,2`);
      - 9 px vertex squares, 1 px edges and lines.
      - `pick` blocks (clicks). `pick_async` is polled (hover), since `readRenderTargetPixels`
        stalls today.
      - Select Other and the rotate-about point use `raycast_faces` (CPU, level 0, every
        face along the ray, nearest first).
    - ✅ HiDPI: sizes are logical px × scale, as the 2D canvas and `LineMaterial` use. Plain
      edges stay one device pixel, as WebGL lines are.
    - ⬜ Not in F1, and where each goes:
      - 3D View Selector cube: F2, or a follow-up to F1.
      - View name ("*Isometric"), tooltips, sketch editor overlay, dimensions, measure
        line, box selection, rotate-about marker, plane handles: the webview's 2D canvas
        over the viewport in F2. They are 2D already.
      - Box selection: needs `Camera::project` only; F2.
  - **Labels.** The renderer has no text rasteriser. `add_label(rgba, w, h)` takes the
    plane name as an image (straight alpha, sRGB, mipmapped on upload). F2 can rasterise it
    in the webview (OffscreenCanvas, Century Gothic-style font as today) and send the pixels,
    or add a font crate (licence check). The smoke test uses a solid bar.
  - **🔍 options** (in `FEATURE_SPEC.md`):
    - `face_highlight_in_line_styles`, off by default. Today the white emissive HLR
      material washes face tints out.
    - `closed_edge_vertices`, off by default, per SOLIDWORKS. This **changes behaviour**
      once F2 switches over: today's viewer offers a vertex at the start of a full circle.
  - **Tests** (18 new; workspace 175 passed, 7 ignored (the timing test among them), clippy `-D warnings` clean):
    - unit: camera (views, projection round trip at 1e7 mm, reverse Z, zoom about pointer,
      fit, transitions), colours, plane sizing, pick rules (priority, radii, filters,
      Chamfer, id mapping, crop matrix), triad, ray–triangle;
    - `tests/smoke.rs` (L, smoke), on an OCCT block 20 × 10 × 5 with a Ø6 hole meshed by
      `oc-mesh`, with a plane, axis, point, coordinate system and sketch:
      - every display style rendered and checked by pixel: lit grey faces, dark edges, no
        edges in Shaded, white HLR faces with the hidden edge removed, a dashed grey hidden
        edge in HLV, hidden edges and no faces in Wireframe;
      - hover face orange, selected face blue, hovered edge 3 px orange, no hover with
        dynamic highlight off, vertex marker;
      - hovered plane border, dashed axis, reference point, triad, origin;
      - Section View from the top: flat cap darker than the uncut face, no cap across the
        hole; from the isometric view the removed half shows the background;
      - 18 GPU picks against the kernel's topology: face, edge, edge 3 px off onto the
        face, face-only filter, vertex over edge, Chamfer, hole wall seen through the
        opening, hidden edge occluded but pickable in Wireframe, no faces in Wireframe,
        origin over the hidden corner vertex, plane, axis, point, coordinate system,
        sketch, background, async pick, ray cast;
      - HiDPI: the triad lands at twice the physical position at scale 2.
    - Passes on the RTX 5070 Ti with **Vulkan and D3D12** (`WGPU_BACKEND`) and on **WARP**
      (`OC_RENDER_FALLBACK=1`, Microsoft Basic Render Driver). With no adapter at all it
      prints SKIP, unless `OC_RENDER_REQUIRE_GPU=1` (set that in CI on GPU runners).
      Images: `target/tmp/oc-render-smoke/`.
  - **Measurements** (release, RTX 5070 Ti, `tests/smoke.rs` `timings`, ignored by default).
    The part is a 400 × 300 × 10 plate with 400 holes: 136 012 triangles and 67 212 edge
    segments at quality 3. Viewport 1920 × 1080 with 4× MSAA:

    | backend | Shaded With Edges | HLV | sync pick, median / max |
    |---|---|---|---|
    | Vulkan | 0.36 ms | 0.43 ms | 0.24 / 5.98 ms |
    | D3D12 | 0.38 ms | 0.41 ms | 0.38 / 3.93 ms |

    Frame time is submit-to-idle over 100 frames, CPU encoding included. It rotates the
    view and moves the hover each frame.
  - **Deviations and findings.**
    - Draw order: everything drawn "over everything" (coordinate systems, the previewed
      profile, reference points, vertex markers) comes after the transparent pass. In
      three.js some of those are opaque objects drawn before plane fills, so a plane fill
      could tint them; here it cannot.
    - Line caps are square. `LineMaterial`'s are round; the difference shows only on 3 px
      highlights.
    - 1-device-pixel edges with 4× MSAA look slightly stippled at shallow angles. The
      cause is the coverage pattern, not depth: Wireframe, with no faces, looks the same.
      WebGL's native lines rasterise differently. A visual tuning item for the owner;
      options are a 1.25 px minimum or analytic line antialiasing.
    - Per-frame batches (lines, points, plane fills) and bind groups are rebuilt every
      frame. They are small. Geometry dedup by content hash and GPU-driven drawing are
      G2's job.
    - wgpu's default `gles` feature compiles the OpenGL backend in (glow, khronos-egl);
      only `Backends::PRIMARY` is used. Dropping it needs `default-features = false`.
    - `Renderer::headless` honours `WGPU_BACKEND`. wgpu picks Vulkan before D3D12 on this
      machine; F2 should choose deliberately (D3D12 composites with WebView2 most
      directly, P2).
  - **Not built from section 4's phase-1 list:** image-based lighting, SSAO,
    order-independent transparency (transparency is sorted per draw as today),
    silhouette edges, tangent-edge display modes (needs E4's smooth-edge flag),
    multi-plane sections. None is in today's viewer, so parity does not need them. Each
    is a candidate follow-up package (F1b) before or after F2; owner's call.
  - **For the owner.**
    1. Closed-edge vertices hidden by default (SOLIDWORKS) or shown (today's viewer)?
    2. Plane labels: rasterise in the webview and send pixels, or add a font crate?
    3. View Selector cube: port it into `oc-render` now (F1 follow-up) or in F2?
    4. Phase-1 extras (IBL, SSAO, OIT, silhouettes, tangent edges): an F1b package before
       F3, or after the switch-over?
    5. 1 px edge look: keep (closest to the WebGL lines) or widen slightly?
  - **Owner decisions (2026-11-15, at merge).**
    1. Hidden by default, as SOLIDWORKS. The switch-over (F3) changes today's behaviour;
       note it in the release notes.
    2. Rasterise in the webview (today's font) and send pixels; no font crate.
    3. View Selector cube in F2, with the viewport's mouse input.
    4. Phase-1 extras after the switch-over (a later package, not a gate for F3).
    5. Keep 1 px edges for now; tune once they are seen in the real app in F2.
- 2026-11-15 **F2a done** (branch `claude/f2-app-wiring-dd5e3d`, not merged). F2 was too big
  for one session and is split into F2a (this) and F2b (section 6). New crate
  `crates/oc-app` (binary `opencad`), `src/native/*`, small changes in the viewport, app,
  solver selection and one `oc-graph` field. New dependencies (Tauri 2.12.5 tree, licences
  in `docs/THIRD_PARTY.md`: permissive, plus MPL-2.0 `cssparser`/`selectors`/`dtoa-short`/
  `option-ext`).
  - **Shape.**
    - Window: Tauri 2, transparent WebView2 over a wgpu swapchain on the same window (P2's
      layout). Backend chosen deliberately: **D3D12 on Windows** (`OC_BACKEND=vulkan|dx12|metal`
      overrides; platform default elsewhere). `cargo run -p oc-app` loads the Vite dev
      server; `--features custom-protocol` embeds `dist/` (`npm run native`).
    - Engine thread (`engine.rs`): owns `OcctKernel` and the `Regenerator` (neither is
      `Send`) and serves the worker's requests one at a time: rebuild (with preview), export
      STEP/STL, mass properties, measure, Convert Entities, clear cache. The TS client
      (`src/native/engine.ts`) keeps `KernelClient`'s API and coalescing; `App.kernel` picks
      it when `__TAURI_INTERNALS__` exists. `window.opencad` is unchanged.
    - Rebuild reply: binary (`pack.rs`, JSON header + typed arrays, zero-copy views in JS).
      `BodyMesh` is level 0 of the `oc-mesh` mesh the renderer draws, so the webview's picks
      and the pixels agree. Faces and edges carry `oc_naming` signatures **with names**,
      vertices their names (`vertexNames`), and the vertices SOLIDWORKS does not have are
      listed in `hiddenVertices`: neither drawn nor picked (F1 owner decision 1). A face's
      `owner` is the latest feature among its tags.
    - Previews as `engine.ts` makes them: for extrude, revolve, sweep, loft, hole, rib,
      patterns and mirror, the part before the feature plus the tool (cuts: before ∩ tool);
      otherwise the result with the feature's faces tinted. This needs the feature's tool
      bodies: **`oc_graph::Rebuild::outs`** (new field, every feature's `NodeOut`).
    - Heals (E6 carry-over): `Rebuild::heals` come back as `{id, from, feature}`;
      `App.applyHeals` replaces a feature still equal to `from` (key-sorted JSON) without an
      undo step or the dirty flag. The engine's alias cache makes the next rebuild a hit.
    - **Missing** (E6 carry-over): for every feature with an error or warning the engine
      resolves each reference (`Feature::refs_mut`) on the body before it with
      `oc_naming::resolve` and returns the unresolved ones (`missing`). Fillet and chamfer
      items, and every box labelled by `refName`, show a bold "Missing" (🔍 wording).
    - Viewport: `Viewport` keeps navigation, the 2D overlay, the View Selector and, in F2a,
      picking (its three.js pick scene) in the webview. It does not draw the model;
      `NativeView` sends the scene (bodies by mesh handle, planes with label keys, axes,
      points, coordinate systems, sketch segments, origin, section, hover and selection as
      `Hit`s, options), the camera (the `ViewCamera` fields, which are `oc_render::Camera`'s)
      and the graphics area's rectangle. The render thread draws the newest frame into a
      texture the size of the graphics area and copies it into the swapchain at that offset
      (surface `Bgra8Unorm` with `COPY_DST`, target `Bgra8UnormSrgb`: the encoded bytes are
      copied). Meshes pass from engine to renderer by handle through a shared store; the
      renderer drops a handle once the webview has seen a newer one and no longer holds it.
      The origin and the triad come from `oc-render`; the webview no longer draws them there.
    - Plane labels (F1 owner decision 2): rasterised in the webview with today's font
      (`labelCanvas`, shared with three.js) and uploaded once per name as raw bytes.
    - Overlays (view name, tooltips, sketch editor, dimensions, measure line, box selection,
      rotate-about marker, plane handles) stay in the webview canvas, projected by
      `ViewCamera`. It has the renderer's camera fields, so they land on the renderer's
      pixels (checked: the plane handles sit on the drawn plane border).
    - Sketch solver: `setSketchSystemFactory` (new in `solver.ts`) selects
      `WasmSketchSystem` for every solve in the native app; `newSketchSystem` replaces
      `new SketchSystem` in the app and the editor. Browser and Electron keep the TS solver.
    - Input: the webview is over the viewport, so mouse and keyboard reach the same TS
      handlers as in Electron (P2).
  - **Checks** (Windows, RTX 5070 Ti, D3D12):
    - `opencad --smoke` (the Electron scenario: sketch on Front, rectangle with a hole,
      extrude, isometric): 7 faces, volume 20 858.407 = 24 000 − 1000π, no errors, rebuild
      5.3 ms. It saves the composited window and the viewport alone.
    - `opencad --corpus`: all **17 corpus parts** opened through `loadDocument`, regenerated
      and compared with the oracle's final body. Faces and edges are exact; volume and area
      are within the oracle's 6-figure rounding (with the E5 correction of
      `through-mid-dir2` and that correction's own rounding). No errors or warnings, none
      marked dirty. 787 ms for all 17, meshing and transfer included (patterns 215 ms).
      Exit code 1 on a difference.
    - Scripts (`--script`):
      - edit of block-hole (hole r 3 → 4, block 5 → 8): 1197.876 = 200·8 − 16π·8 in both apps;
      - Edit Feature of the cut shows the yellow preview;
      - refgeom planes with labels, axes, points, the coordinate system, a selected plane and edge;
      - HLV; Section View with the View Selector open;
      - STEP export (19 kB, ISO-10303-21), binary STL (352 triangles), Measure (5 mm),
        Convert Entities (2 circles, 4 lines);
      - DOM pointer events on the overlay hover and select the face under them; the wheel zooms;
      - TT5 data and a fillet whose rim edge was deleted: the box reads "Edge<1>, Missing".
    - `cargo test --workspace` 178 passed, 7 ignored (OCCT_DIR set,
      `OC_RENDER_REQUIRE_GPU=1`); workspace clippy `-D warnings` clean; `npm run typecheck`
      clean; vitest 89/89. Electron: `npm run build` and `electron . --smoke` pass (7 faces).
  - **Parity with the Electron app** (✅ same, ≈ differs as noted, ⬜ not yet):
    - ✅ Corpus results, sketch + extrude, edits, previews, statuses, reference geometry,
      display styles, highlights, planes, triad, origin, view name, section, export,
      measure, Convert Entities, mass properties, keyboard and mouse handling (same code).
      The View Selector is still three.js, drawn in the transparent WebGL canvas over the
      viewport.
    - ≈ Edit Feature preview: the native app shows the yellow tool at once. Electron shows
      none until a parameter changes (its preview hits the committed feature's cache entry,
      which has no tool). The native behaviour is SOLIDWORKS'.
    - ≈ Full-circle start vertices are not offered (owner decision; F3 release note).
    - ≈ **1 px edges (F1 owner item 5).** Seen in the app at 125 % scaling, they are
      thinner and lighter than Electron's. At 3× zoom they are slightly dotted on 30°
      lines (MSAA coverage), where ANGLE's lines read as a solid line about 1.5 device px
      wide. At 100 % they read as continuous lines. Tuning (a 1.25–1.5 px minimum or
      analytic antialiasing) is the owner's call (below).
    - ≈ The overlay is drawn at once, the viewport one IPC hop later (well under a frame by
      P2's numbers). No tearing was seen in screenshots.
    - ⬜ Picking through `oc-render` (F2b). Today's three.js pick rules apply, on the same
      level-0 mesh.
    - ⬜ Not exercised: a real mouse (P2 measured it), interactive drag-resize, a second
      monitor, macOS (Windows first).
  - **Deviations and findings.**
    - F2 is split into F2a and F2b. Picking stays in the webview for now: `pick()` is
      synchronous at about 11 call sites (the sketch editor's snaps among them) and Tauri
      IPC is asynchronous.
    - The webview's camera stays the source of truth (it handles input and projects the
      overlay); the renderer gets its fields with each frame instead of owning the camera.
    - `oc-graph` gained `Rebuild::outs`, needed for previews.
    - The app icon is the P2 spike's 32 px icon, and the binary is a console program
      (`--smoke` prints to stdout). Both are F3 items (installers).
    - `smoke.rs` uses `unsafe` GDI calls for the composited screenshot (allowed locally).
  - **For the owner.**
    1. F2b: move picking to `oc-render` now (async pick API, three.js gone from the native
       path), or keep the webview pick scene until after F3?
    2. 1 px edges: keep, or set a minimum of about 1.25–1.5 device px (or analytic AA) to
       match Electron's heavier look?
    3. View Selector cube: keep the three.js cube in the webview canvas (it works today), or
       port it into `oc-render` with F2b?
    4. "Missing" wording in selection boxes (🔍): plain bold "Missing" as built, or another
       form?
  - **Owner decisions (2026-11-15, at merge).**
    1. Move picking to `oc-render` in F2b; the native path drops the three.js pick scene.
    2. Widen plain edges to about 1.5 device px (or analytic AA of the same weight) to
       match Electron's look; check at 100 % and 125 % scaling.
    3. Port the View Selector cube into `oc-render` in F2b, so the native app has no three.js.
    4. Keep the plain bold "Missing".
- 2026-11-16 **F2b done** (branch `claude/f2b-picking-2cee4c`, not merged). Implements the
  F2a owner decisions: picking moves to `oc-render`, plain edges widened, View Selector
  ported, "Missing" unchanged. No new dependencies.
  - **Shape.**
    - `oc-render`: `Options::line_width` (device px, default 1.5) for every plain line
      (edges, hidden edges, inactive sketches, axes, coordinate systems, the cube's
      outlines); `selector.rs`, a port of `viewSelector.ts` (26 facets from the same
      half-space clipping, floating panels for faces turned away, hover tint, drawn as
      screen-space overlay geometry in the order three.js drew it, CPU facet pick,
      `up_for`), shown through `Scene::view_selector`; `Renderer::box_edges` (window:
      every segment inside; crossing: any segment meets the box).
    - `oc-app`: `view_pick` and `view_box` commands answered on the render thread
      (`pick.rs`). Frames carry a number (`seq`); a query names the newest frame the
      webview sent and waits for it (at most 250 ms), because the frame and the pick travel
      on separate IPC calls and may overtake each other; the reply names the frame that
      answered. Clicks use `pick` (waits for the GPU); hover uses `pick_async`, polled
      while the thread keeps handling messages. `ray: first|all` adds `raycast_faces`
      (rotate-about point and Hole Wizard; Select Other); `selector: true` tries the cube's
      facets first. The render thread answers queries before presenting (which can wait
      for vsync).
    - Webview: `Viewport` makes no WebGLRenderer and no three.js scene in the native app
      (it still uses the three.js math classes, as `ViewCamera` does); `NativeView` has
      `pick` and `boxEdges`; `NativeSelector` holds the cube's visibility and hovered facet.
      **The synchronous `pick()`** (12 call sites, the sketch editor's `modelPick` and
      `modelSnapAt` among them) is kept: pointer events go through a queue
      (`Viewport.dispatch`). Before an event's handler runs, the pick it needs is fetched
      at its position (decided by `pickNeed` from the state after the events before it:
      hover on plain moves and while sketching; clicks; the ray on middle-up and
      right-up; none during navigation drags); then the unchanged handler runs and
      `pick()`, `hitPoint()` and the selector read that answer. Events keep their order,
      consecutive moves are coalesced, and focus, pointer capture and preventDefault stay
      synchronous. Outside an event (a sketch tool re-running its hover from a timer)
      `pick()` returns the last answer while the camera and mesh are unchanged. The
      browser and Electron run the handlers at once with three.js picking, as before.
    - `.gitattributes`: `crates/oc-app/gen/schemas/*.json text eol=lf`. Tauri writes LF;
      with `core.autocrlf` they were checked out CRLF and showed as modified after every
      build. Clean over five builds since.
  - **Checks** (Windows 11, RTX 5070 Ti, D3D12, 125 % scaling):
    - `crates/oc-app/scripts/picking.js` (`npm run native:picking`): 21 checks through DOM
      pointer events on the overlay: hover face, the ray point on it, click select, edge
      at its middle and 3 px off it, the face-only selection filter, vertex 2 px off a
      corner, background, the hidden origin picked first, Select Other (front and back
      faces along the ray), rotate-about on a face, on a linear edge (axis) and cleared on
      the background, double-click showing the feature's dimensions, window box (all 14
      edges), crossing box over an edge's middle, View Selector hover and click (view
      becomes Front), sketch snaps to an off-plane model vertex and to an edge midpoint.
      The same script in the browser build (three.js picking, Vite dev server) passes the
      19 checks both paths share (the ray-point and crossing-box checks are native-only).
    - Latency (release, `pick` from JS to the answer): click 1.1 to 1.3 ms median, hover
      1.2 ms median (1.7 ms right after a frame); a bare IPC round trip is 0.9 ms.
    - `opencad --smoke`: 7 faces, volume 20 858.407, no errors. `--corpus`:
      **`corpusDiffs: []`** (17/17).
    - `oc-render` GPU smoke (`smoke_box_selector_and_line_weight`): box selection, the cube
      drawn and hovered, and line weight: on a 1-in-8 slope the weakest pixel column of a
      1 px line has 0.50 coverage under 4× MSAA (the stipple F1 and F2a saw), of a 1.5 px
      line 0.75, at scale 1.0 and 1.25. In the app the 30° edges are continuous at 125 %
      and, with `--force-device-scale-factor=1` for the webview, at 100 %.
    - `cargo test --workspace` 186 passed, 7 ignored (OCCT_DIR set,
      `OC_RENDER_REQUIRE_GPU=1`); workspace clippy `-D warnings` clean; `npm run typecheck`
      clean; vitest 89/89. Electron: `npm run build` and `electron . --smoke` pass (7 faces).
  - **Parity with the Electron app** (✅ same, ≈ differs as noted, ⬜ not checked):
    - ✅ Hover, click, selection filters, Chamfer edges-only, the vertex, edge, sketch,
      face, reference and plane priority and radii, origin first, Select Other,
      rotate-about (face point, edge axis, vertex), Hole Wizard positions (same ray path),
      double-click, Convert Entities picks, sketch snaps to model geometry, View Selector
      (hover, click, panels, view name), window box selection.
    - ≈ Crossing box: also takes an edge it crosses between mesh points (as SOLIDWORKS
      does); Electron tests the points only and misses a long straight edge crossed in the
      middle.
    - ≈ Hover answers arrive 1 to 2 ms after the move instead of within the handler, so the
      highlight can trail a fast pointer by a frame.
    - ≈ The cube's translucent fills blend in linear light (sRGB target); three.js blended
      in sRGB. Slightly lighter where facets overlap the model.
    - ≈ Plain lines are 1.5 device px (owner decision), a little heavier than Electron's
      at 100 %.
    - ⬜ A real mouse and interactive drag-resize of the window: computer-use access was
      declined in this session, so only synthetic DOM pointer events were used. Owner
      check: hover, click, middle-drag rotate, wheel zoom, right-click Select Other,
      Ctrl+Space cube, and dragging the window edge (the viewport should follow without
      tearing or a stale band).
  - **Deviations and findings.**
    - Windows timer resolution: the first version waited for hover picks with
      `recv_timeout(1 ms)`, which sleeps a whole 15.6 ms tick, so hover took 16 ms. The
      render thread now spins (`yield_now`) only while a hover pick is on the GPU.
    - `Renderer::box_edges` takes corner points (clippy's argument limit).
    - The browser pane used for the browser comparison runs no `requestAnimationFrame`
      while hidden; that run drove the viewport loop from a timer.
  - **For the owner.**
    1. The real-mouse and drag-resize check above.
    2. Crossing box: keep the segment test (SOLIDWORKS) in the native app and port it to
       the Electron app, or leave Electron as is until F3?
    3. `Options::line_width` is a renderer default with no UI. Expose it (System Options >
       Display, 🔍) or leave it fixed at 1.5?
    4. The three.js math classes (`Vector3`, `Quaternion` in `ViewCamera` and the overlay)
       remain in the native path; only rendering and picking left three.js. Replacing the
       math is a separate refactor: before F3, or not at all?
