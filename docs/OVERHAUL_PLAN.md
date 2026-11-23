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
     Owner decision 2026-11-16 (G1c): a stand-in that moved by more than the size tolerance
     of step 3 (25 % of the stored size: a face's centroid against √area, an edge's middle,
     or a closed edge's centre, against its length; a vertex against the smallest face
     around it) is not the same entity, so step 3 decides. Deleting one of two holes cut
     from one sketch then makes a reference to it missing, as SOLIDWORKS does, instead of
     moving it onto the other hole.
     Owner decision 2026-11-19 (G1d): **caps are exempt.** A `start`, `end` or `mid` stand-in
     (the one cap of that role of the feature) is that cap wherever it moved or however it
     was resized, as SOLIDWORKS names caps by feature. An edge or vertex is exempt only when
     every stand-in face it needed was a cap.
     Owner decision 2026-11-19 (G1d merge, built in G1e): only caps of a feature that had a
     **single region** when the reference was stored are exempt; the region count travels
     with the stored name (`#<n>` on the cap tag; none stored: the rule above). A stand-in
     ruled out for having moved is not taken by step 3 either, so a deleted blind hole's
     bottom is Missing rather than re-attached to the other hole's coplanar bottom.
3. None: strict signature match (same type; planes coplanar with the same normal; others
   within 25% of √area). Accept with warning "re-attached by position", else error
   `Missing reference`, shown in the tree and as `**Missing**` in the feature's selection
   box for re-picking (mark 🔍 in FEATURE_SPEC until wording is confirmed).
4. Legacy (imported v1 document, no name): loose signature match, then the resolved name
   is written back as a non-undoable, non-dirtying heal.

The kernel never reads geometry from a stored signature; it always resolves to live
topology first. The one exception is an in-context reference (G1d: a sketch plane or a
converted edge from another component of an assembly), which stores its geometry in the
part's coordinates and is updated by the assembly, not resolved on the part's body.

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
- **Since F3 the Electron app and the TypeScript engine (`desktop/`, `src/kernel/*`, the
  three.js viewport) are frozen: bug fixes only. New modelling goes into the Rust crates.**
  The shared React UI keeps changing. The Electron app stays buildable (`npm run desktop`,
  `electron . --smoke`), and the browser build keeps the TypeScript engine until W1.

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
- [x] **F2 IPC and app wiring** (L, smoke) — Tauri commands replacing `kernel/client.ts`;
  React UI moved into the Tauri app; sketch editor on the WASM `oc-sketch`;
  `window.opencad` debug hook kept. Split in two (section 8, 2026-11-15):
  - [x] **F2a native app slice** — **done 2026-11-15 (section 8).** `crates/oc-app` (Tauri 2,
    D3D12 swapchain under the transparent WebView2); kernel requests served by `oc-graph` on
    OCCT with heals and per-reference Missing; viewport drawn by `oc-render` from a scene the
    webview sends; plane labels rasterised in the webview; WASM solver; `--smoke`,
    `--script`, `--corpus` (17/17 corpus parts equal the oracle through the UI).
  - [x] **F2b picking and polish** — **done 2026-11-16 (section 8), merged.** Picking,
    hover, Select Other, rotate-about, box selection, sketch snaps and the View Selector go
    through `oc-render`; no WebGL context in the native app; plain lines 1.5 device px.
    The owner's real-mouse and drag-resize check passed on 2026-11-16. The decisions
    listed in section 8 were made at merge.
- [x] **F3 Switch-over** (M, smoke) — **done 2026-11-16 for Windows (section 8), merged;
  review checkpoint. Owner decisions made at merge. Released as 0.5.0 on 2026-11-16 (GitHub release
  v0.5.0 with the OCCT source; web build redeployed, still on the TS engine).** — new app passes the Electron smoke-equivalent
  end-to-end test and a manual checklist from FEATURE_SPEC; installers ~~for the three
  OSes~~ for Windows (macOS a nice-to-have, Linux deferred: section 1) in the release
  workflow (ask the owner before changing release/signing).
  Old Electron app frozen (section 5). **Review checkpoint.**

### Phase G: scale

- [ ] **G1 Assemblies data model** — components, instances, mates (3D solver from E2's
  core), content-hash geometry sharing, lightweight loading from caches. Too big for one
  session; split (2026-11-16, section 8):
  - [x] **G1a assembly engine and a first native slice** — **done 2026-11-16 (section 8), merged** (with a fix at merge: `Assembler::entity` refuses an index the part does not have instead of panicking the engine request). — `oc-doc` assembly document
    (schema version line and migrations of its own), the `.ocasm` zip container with
    content-hashed caches, the mate solver `oc-mate` on `oc-sketch`'s core, assembly
    regeneration `oc-asm` (part files found by relative path with a fallback search, one
    regeneration and mesh per distinct part, mate references by persistent name, lightweight
    open from cached meshes), instance transforms in `oc-render`, and in the native app:
    open/save an assembly, the assembly FeatureManager, instances drawn and picked,
    New Assembly, Insert Components, a basic Mate PropertyManager, dragging within the free
    degrees of freedom, an assembly corpus and an assembly `--script` smoke.
  - [x] **G1b assembly UI to SOLIDWORKS parity** — **done 2026-11-16 (section 8),
    merged; owner decisions there.** — what G1a leaves (its section 8 entry lists it): the Mate pop-up toolbar,
    Edit Mate, re-picking Missing references, opening a part from the assembly and
    returning, Insert Components with a placement preview, sub-assembly editing, SmartMates,
    mate references on sub-assembly geometry from the UI; also Move/Rotate Component,
    Hide/Show, rename, appearance, mates to assembly planes, assembly Mass Properties.
  - [x] **G1c assembly evaluate and remaining parity** — **done 2026-11-19 (section 8),
    merged; owner decisions there.** Split from G1b: the `oc-naming` same-role fix (owner decision 4), Interference
    Detection (owner decision 1), assembly-level Measure, the Save Modified Documents dialog
    (owner decision 2), Isolate, Component Properties. The rest moved to G1d.
  - [x] **G1d assembly parity, remaining** — **done 2026-11-19 (section 8), merged; owner
    decisions there.**
    Split from G1c: the end-cap exemption in `oc_naming::resolve` (G1c owner decision 1);
    SmartMates with the component following the pointer while Alt-dragging and the Move
    Component SmartMates button; Edit Part in context with in-context (external)
    references; component display modes and Change Transparency, a true Wireframe for
    Isolate and Non-interfering Components, Interference Detection's Component view and
    Excluded Components; mate animation (owner request during the package). Configurations
    and named display states moved to G1e.
  - [x] **G1e configurations** — **done 2026-11-20 (section 8), merged; owner decisions
    there.** Split from G1d
    (section 8): first the single-region cap
    exemption (G1d owner decision 2); then the ConfigurationManager for
    parts and assemblies; per-configuration suppression, dimension values and component
    states (`Component.config` chosen per instance, the assembly's own configurations);
    named display states tied to configurations (the per-occurrence display mode, colour,
    transparency and hide/show of G1b–G1d become the active display state's); Interference
    Detection and Mass Properties of the active configuration; configurations in the
    `.ocasm` caches (a mesh per part and configuration). Then G2. Left for later (section 8):
    design tables, Specified Configurations, mate values in the Modify box, part display
    states, the DisplayManager.
- [x] **G2 GPU-driven rendering** — **done 2026-11-20 (section 8), merged; owner decisions
  there. Measured on an RTX 5070 Ti (accepted by the owner; no mid-range GPU available).** —
  culling compute, indirect draws, Hi-Z, LOD selection.
  Pass: 10k-component assembly ≥ 60 fps on a mid-range GPU (record hardware).
  - [x] **G2b level of detail while the view moves** — **done 2026-11-20 (section 8),
    merged; owner decisions there.** (G2 owner decision 2): frames say
    whether the view is moving (rotate, pan, zoom, view transitions); while it moves the
    renderer uses `LodChoice::Auto` as Options > Performance > Level of detail sets it
    (SOLIDWORKS' slider, stored since F2 and not active), and a full-detail frame is drawn
    when the motion stops. Picks always use the level drawn. Small; native scripts check
    the levels during and after a turn of the view.
- [x] **G3 Parallel and out-of-core regeneration** — **G3a, G3b and G3c done 2026-11-20 (section 8), merged; G3d done 2026-11-23 (section 8), merged. Phase G complete.** — across parts and bodies (moved here from
  E5, owner decision 2026-11-13: a single part's body chain stays serial); memory budget
  tests. With the geometry-host work: a native body transfer in the kernel trait
  (`export_native` / `import_native`: OCCT `BinTools`, Parasolid XT), so tool bodies can be
  built on worker hosts ahead of their booleans (E5 section 8, option (a)). Too big for one
  session; split (2026-11-20, section 8):
  - [x] **G3a parallel part regeneration** — **done 2026-11-20 (section 8), merged.** —
    independent parts (and sub-assemblies' parts)
    regenerated at once on worker threads, each with its own kernel, when an assembly opens or
    rebuilds and when lightweight parts are resolved (Mass Properties, Interference Detection),
    the native app's engine included; the native body transfer in the kernel trait
    (`export_native` / `import_native`, OCCT `BinTools`) that brings each part's body back to
    the engine thread; the generated 10 000-component assembly (100 distinct parts, G2's
    benchmark) and a `--script` that opens, rebuilds and turns it in the app, measuring open,
    rebuild and frame times and memory end to end (G2 owner decision 4).
  - [x] **G3b geometry hosts** — **done 2026-11-20 (section 8), merged.** — the kernel isolation of section 1: the part workers become
    child processes (P3: control pipe, shared-memory transfer, kill and restart on a crash or
    timeout) running the same jobs with the same transfer, so a crash or hang in OCCT fails
    that part with an error and the app survives; tool bodies of heavy features built on
    worker hosts ahead of their booleans (E5 option (a)) where a measurement shows it pays.
  - [x] **G3c out-of-core and memory budgets** — **done 2026-11-20 (section 8), merged.** — native B-rep caches in the `.ocasm` (the body
    blob beside each cached mesh, so resolving a lightweight part reads it instead of
    regenerating), bodies and meshes evicted under a memory budget and read back from the
    container, memory budget tests on the generated large assemblies; the mate solver's
    conflict search and drag per connected group of bodies (G1a note) for mated large
    assemblies.
  - [x] **G3d large-assembly follow-ups** — **done 2026-11-23 (section 8), merged.**
    (owner decisions at the G3 merge, section 8) — the
    build reply sends only what changed between builds (placements, states) in binary, as part
    rebuilds do, and the assembly FeatureManager draws only the rows in view (G3a question 2:
    about 1.1 s of the 1.6 s 10 000-component open is IPC and webview); part workers and
    geometry hosts default to half the logical processors (G3a question 4); geometry hosts
    end with the app even when it is killed (a Windows job object, kill on close) and speak
    the protocol on a handle of their own, with standard output sent to `NUL`, so nothing the
    kernel prints can break it (G3b review); and the G3c owner answers that need code.

### Phase H: modules

- [ ] **H1 Evaluate:** exact mass properties, measure, interference/clearance.
- [ ] **H2 Drawings:** exact HLR on the kernel, views, dimensions.
- [ ] **H3 FEA linear static** with CPU path, then optional CUDA/HIP solvers; validated
  against analytic and NAFEMS benchmarks.
- [ ] **H4 Ray-traced rendering** (hardware RT via ray queries; OptiX/HIP RT optional).
- [ ] **H5 Motion study** on the mate solver.
- [ ] **H6 STEP AP242 semantic PMI** (native OCCT has XCAF GD&T classes) on top of names.

### Phase W: browser (W1 before phase H, owner 2026-11-20)

- [ ] **W1** — crates to WASM, kernel host as Web Worker with OCCT's WASM build, wgpu on
  WebGPU, same React UI; features needing vendor compute disabled. Too big for one session;
  designed and split on 2026-11-23 (design below, section 8 for the reasons and numbers):
  - [x] **W1a engine core on WebAssembly** — **done 2026-11-23 (section 8), merged.** — Emscripten
    pinned (emsdk 7.0.1); OCCT 8.0.1 built for `wasm32-unknown-emscripten`
    (`build-occt-wasm.cmd`), linked either as an Emscripten **side module** of its own
    (`libocct.so`, the browser's equivalent of the installer's DLLs; the default) or
    statically; `oc-kernel-occt` and every engine crate (`oc-doc`, `oc-sketch`, `oc-kernel`,
    `oc-naming`, `oc-mesh`, `oc-graph`, `oc-mate`, `oc-asm`) build for that target; their
    tests run in WebAssembly under Node (`npm run test:wasm`: 233 pass both ways, the corpus
    equal to the oracle); sizes, build times, start-up and speed against native measured.
  - [ ] **W1b engine and geometry hosts as Web Workers** — first a pure move of `oc-app`'s
    engine (`engine.rs`, `asm.rs`, `pack.rs`, `bodymesh.rs`) into a Tauri-free crate
    `oc-engine` that `oc-app` uses unchanged; then `oc-web` (an Emscripten program: the
    engine's job loop behind one exported request entry, and the `oc_host` part job for host
    workers), `src/web/` (a worker per engine and per geometry host, `invoke` routed to them
    in the browser), the engine's file system (below) and `oc_asm`'s worker pool on workers.
    Checks: the request round trips and the corpus through the worker, under Node.
  - [x] **W1c renderer on WebGPU** — **done 2026-11-23 (section 8), not merged.** — a pure move of `scene_dto.rs`, `view.rs`' frame logic
    and `pick.rs` into a Tauri-free crate `oc-view`; `oc-render` built for
    `wasm32-unknown-unknown` with wgpu's WebGPU backend (asynchronous device creation and
    picks only: no blocking on the browser's GPU); `oc-web-render` (wasm-bindgen) drawing into
    an `OffscreenCanvas` in a render worker under the transparent UI, fed by the unchanged
    `NativeView` frames; meshes sent by the engine worker to the render worker directly
    (`RenderMesh::to_bytes`, transferred buffers). Can run beside W1b after both pure moves.
  - [ ] **W1d browser checks** — the native scripts (`--smoke`, `--corpus`, `picking.js`) in
    headless Chromium with WebGPU: Edge or Chrome started by a Node script over the DevTools
    protocol (Node's own WebSocket; no new dependency), the same `window.opencad` scenarios,
    `SMOKE {...}` reports and screenshots; `npm run web:smoke`, `web:corpus`, `web:picking`.
    The browser gets its oracle: the corpus through the UI equals `corpus/*.json`.
  - [ ] **W1e files and assemblies in the browser** — the workspace (below), Open / Save /
    Save As by path, assemblies and configurations in the browser, `assembly.js`,
    `configurations.js`, `lod.js` and `scale.js` in the browser checks (memory and times of
    the 10 000-component assembly against the native app).
  - [ ] **W1f switch the web build** — the browser build runs the native engine; the
    fallback without WebGPU (below); the TypeScript engine (`src/kernel/*`, replicad and its
    23 MB OCCT 7 WebAssembly) leaves the web build; the web build's licence notices, OCCT
    source and replacement instructions; `deploy-web.sh` uploads the new files (the owner
    deploys). Electron stays frozen on the TypeScript engine (it is not the web build).

  Order: W1a → W1b ∥ W1c → W1d → W1e → W1f. W1b and W1c touch disjoint files after their
  pure moves.

  **W1 design** (2026-11-23; reasons and the measurements behind them in section 8):
  1. **OCCT in WebAssembly.** OCCT 8.0.1, the C++ shim and the Rust engine are compiled by
     Emscripten (target `wasm32-unknown-emscripten`, stable Rust, emsdk pinned in
     `build-occt-wasm.cmd`), so `cxx` and the shim work unchanged. wgpu's WebGPU backend needs
     `wasm32-unknown-unknown` (wasm-bindgen), so the renderer is a separate module; the two
     never call each other, they exchange messages (as the engine and render threads do).
     Exceptions are WebAssembly's own (`-fwasm-exceptions`: the model Rust's target uses;
     every current browser runs it). Relocatable code for the side module (point 2).
     **Threads: none in W1.** Rust's standard library for this target is built without
     atomics (threads would need nightly `-Zbuild-std`), and threads need SharedArrayBuffer,
     hence cross-origin isolation headers (COOP/COEP) on CloudFront. Each worker is
     single-threaded (OCCT's pool and rayon run on the calling thread); parallelism comes from
     several workers (geometry hosts), which exchange transferred buffers and need no
     SharedArrayBuffer, so the site's headers stay as they are. Memory: 4 GB per worker
     (wasm32); memory64 is not in Safari.
  2. **LGPL.** Default: OCCT is its own file, `libocct.so`, an Emscripten **side module**
     the engine (the main module, `-sMAIN_MODULE=2`) loads at start: dynamic linking, as
     section 4 asks and as the installer's DLLs. A user replaces it with any OCCT 8.0.1 built
     by `build-occt-wasm.cmd` and `node scripts/wasm.mjs side` with the same Emscripten. The
     web build serves OCCT's licence texts, the notices and the OCCT source archive. W1a
     measured the price: the side module makes the kernel about 1.36× slower than the same
     program linked statically, doubles the download (9.5 against 4.8 MB with Brotli) and
     adds about 0.85 s to every engine or host worker's start. The alternative is one
     module plus a **relink kit** (LGPL 2.1 section 6(a): the engine's object files and the
     link command, so a user can link a modified OCCT), which departs from section 4's
     "link dynamically" and needs the owner (W1a question 1). Both forms build and pass the
     same tests; the choice is needed before W1f.
  3. **Where the engine runs.** Tauri's IPC is the one seam: `src/native/tauri.ts`'s `invoke`.
     In the browser `invoke` goes to `src/web/` instead, which routes `engine_*` and `asm_*`
     commands to the **engine worker** (the code of `oc-app`'s engine thread, moved to
     `oc-engine`, on the same requests and binary replies) and `view_*` to the **render
     worker**; `NativeKernelClient`, `NativeView` and the assembly session run unchanged.
     Geometry hosts become **host workers** running the same part job and plain-data replies
     (`oc_host::proto`'s data, not its pipes): a trap or OCCT abort ends that worker, which is
     started again, and only its part fails, as with host processes. As natively, one changed
     part (the usual edit) regenerates in the engine worker.
  4. **Rendering.** `oc-render` on WebGPU in the render worker (`OffscreenCanvas`); the UI
     thread keeps navigation and the 2D overlay, as in the native app. Hover and click picks
     are both asynchronous replies (the UI's pick queue already waits on a promise). Optional
     WebGPU features are used when the adapter has them (`indirect-first-instance`,
     `depth32float-stencil8`), with the existing fallbacks (`set_culling(false)`, 24-bit
     depth). **Without WebGPU** (Linux browsers, mostly, in late 2025: Chrome and Firefox ship
     it on Windows and macOS, Safari 26 on Apple systems): proposed, the frozen three.js
     viewport drawing level 0 of the engine's meshes (the `BodyMesh` every rebuild reply
     already carries for picking), parts only, assemblies greyed with a note; the three.js
     viewport retires when WebGPU is on by default in Linux Chrome and Firefox (owner
     question). The TypeScript engine leaves the web build in W1f either way.
  5. **Files.** The engine sees an Emscripten in-memory file system whose `/work` mirrors the
     user's **workspace**, so paths, `oc_asm::locate`, relative component paths and `.ocasm`
     containers work as natively and files move between the desktop and the browser as they
     are. The workspace is a folder the user picks through the File System Access API where
     the browser has it (Chromium: read and write, the permission kept between visits), and
     otherwise the browser's private file system (OPFS) with folder import
     (`<input webkitdirectory>`) and download of a file or a zip of the workspace. Writes are
     flushed from `/work` to the workspace after each save. Single parts keep today's open
     and download as well. Parts stay JSON `.ocpart` (owner decision 2026-11-16 deferred
     containers "until W1"): proposed to keep them so through W1, since Electron, frozen,
     still reads parts, and the browser needs no part container for files to work.
  6. **Checks.** Headless Edge or Chrome over the DevTools protocol (W1d) runs the native
     scripts' scenarios against `vite preview` of the web build; WebGPU on the real GPU
     (the headless flags it needs are settled in W1d; a run with WebGPU switched off checks
     the fallback). Reports have the native app's shape, and the corpus through the
     browser UI must equal the oracle, so the browser has the same oracle as the Windows app.
     The engine crates' own tests also run in WebAssembly under Node (`npm run test:wasm`, W1a).
  7. **Disabled in the browser:** features that need vendor compute (none exist yet; phase H's
     CUDA/HIP paths will report themselves unavailable and use their CPU paths), native file
     dialogs (replaced by the workspace), Windows-only process features (job objects, the
     WebView2 watch, `--geometry-host` processes).
  Owner decisions deferred "until W1" and where they land: parts as containers (point 5,
  question); request-side document edits instead of the whole document (after W1: the
  browser has the same request; W1e measures it); an out-of-core renderer for meshes (after
  W1; the browser's 4 GB per worker makes it more pressing, W1e measures the scale
  assembly); kernel crashes in the engine (after W1, G3b question 1).

### Order

```
0.1 → 0.2 → 0.3 → 0.4
P1, P2, P3, P4 (parallel; P1 first if only one)
E1, E2 (parallel) → E3 → E4 → E5 → E6
F1 → F2 → F3 → G → W1 → H
```

W1 (the browser on the native engine) comes before phase H (owner, 2026-11-20).

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
  support for ray queries per backend, state of existing
  Rust OCCT bindings (likely too partial; plan assumes our own `cxx` shim).
- (2026-11-20, G2) wgpu 30.0.1: `multi_draw_indirect` / `multi_draw_indexed_indirect` need no
  feature; `MULTI_DRAW_INDIRECT_COUNT` is reported on D3D12, but there its count variant
  does not pass `first_instance` to `instance_index` (wgpu-hal's D3D12 command signature
  without root constants), so G2 uses the plain multi-draw. `INDIRECT_FIRST_INSTANCE` is
  reported by the RTX 5070 Ti (D3D12, Vulkan) and WARP.

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
  - **Owner decisions (2026-11-16, at merge).**
    1. Real-mouse and drag-resize check: the owner tried the merged native app
       (hover, click, navigation, Select Other, View Selector, box selection, sketch
       snaps, resize): confirmed fine, 2026-11-16.
    2. Crossing box: leave the Electron app as is (frozen at F3); the segment test stays in
       the native app.
    3. `Options::line_width`: fixed at 1.5 device px, no UI.
    4. three.js math classes: keep; replacing them does not gate F3.
- 2026-11-16 **F3 done for Windows** (branch `claude/f3-switchover-ba3318`, not merged;
  review checkpoint). The native app (`crates/oc-app`, binary `opencad`) becomes the shipped
  Windows desktop app as **0.5.0**. Nothing was released, tagged, pushed or deployed.
  - **Installer: NSIS** through the Tauri bundler (`bundle.targets: ["nsis"]`), per user
    (`installMode: currentUser`, `%LOCALAPPDATA%\OpenCAD`, no administrator rights).
    Why NSIS rather than MSI: a per-user install with no UAC prompt, a silent install into
    any folder (`/S /D=…`, which the end-to-end check uses), the same installer type as the
    Electron releases, and no WiX toolset (Tauri's MSI installs per machine). Tauri
    downloads NSIS 3.13 and `nsis_tauri_utils` 0.5.3 on first use. The WebView2 runtime is
    not bundled: `downloadBootstrapper` fetches Microsoft's bootstrapper only where it is
    missing (Windows 10 and 11 have it).
  - **`npm run dist:native:win`** (`scripts/dist-native.mjs`) →
    `release/OpenCAD-0.5.0-win-x64-setup.exe`, **16.7 MB** (17 520 259 bytes for the
    tested build); 66 MB installed.
    1. `npm run build`, copied to `target/dist/web` **without** `replicad_single-*.wasm`
       (23 MB, OCCT 7 in WebAssembly) and `kernel.worker-*.js`. The native app never loads
       them, and an embedded LGPL library could not be replaced by the user.
    2. `tauri build --no-bundle` with `OCCT_DIR` set to the **shared** OCCT 8.0.1
       (`OCCT_SHARED_DIR`, default `C:\occt\install-8.0.1-shared`). That install already
       existed on this machine, built 2026-11-13 with `build-occt.cmd`'s options, so
       nothing was rebuilt. `CARGO_TARGET_DIR=target/dist/cargo` keeps the static
       development build untouched.
    3. Staging:
       - The exe must import an OCCT DLL. If it doesn't, the script stops, because a static
         build must not ship.
       - The DLLs it needs are found by following the PE imports (`scripts/native-pe.mjs`):
         **26 of the 48** `TK*.dll`.
       - `msvcp140.dll`, `vcruntime140.dll` and `vcruntime140_1.dll` are copied app-local
         from the newest Visual Studio redistributable folder (`VC_REDIST_DIR` overrides).
       - OCCT's `LICENSE_LGPL_21.txt` and `OCCT_LGPL_EXCEPTION.txt`.
       - `THIRD_PARTY_NOTICES.txt`, generated by `scripts/third-party-notices.mjs`:
         - from `cargo metadata`: 282 crates, the normal dependencies of `oc-app` with
           `custom-protocol` for x86_64-pc-windows-msvc;
         - plus the UI's npm packages: react, react-dom, scheduler and three;
         - each distinct licence text appears once, with the packages that use it;
         - 7 crates ship no licence text and get the standard one from
           `crates/oc-app/licenses/`, with their authors filled in;
         - the OCCT section names the upstream source archive and the build script.
    4. `tauri bundle --bundles nsis` installs those files as resources in the install
       folder, next to `opencad.exe`.
    5. The installer is copied to `release/`, and its size and SHA-256 are printed.
  - **App icon:** generated by `tauri icon` from `build/icon.png`, the original 1024 px
    artwork the Electron app uses: `icon.ico` (16 to 256 px), `icon.icns` and PNGs. It
    replaces the P2 spike's 32 px icon.
  - **GUI subsystem:** `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`.
    Debug builds keep a console for logs.
    - With `--smoke`, `--script` or `--corpus`, the program attaches to the parent console
      when its output is not redirected (`smoke::attach_console`).
    - It also writes the `SMOKE {...}` line to `<screenshot>.log` (`--log=` overrides).
    - The report now carries `app` (the version) and `occt` (`"8.0.1 shared"`).
    - `--corpus` defaults to `./corpus` when the working folder has one.
  - **Version 0.5.0** is in `package.json`, `crates/oc-app/tauri.conf.json` and
    `crates/oc-app/Cargo.toml`. `cargo test -p oc-app` (`versions_agree`) and the dist
    script check that they agree. The other crates stay at 0.1.0: they are internal and
    not versioned with the app.
  - **Found while testing, and fixed (app behaviour):**
    - **Closing a modified part lost it without asking.** Tauri doesn't consult the UI's
      `beforeunload` guard when a window closes. With an unsaved part, a normal close
      (WM_CLOSE) ended the program in 151 ms.
      - Now the webview reports the title and the unsaved state (`app_doc`), and the
        program holds a close request on a modified document.
      - The webview then asks "<name> has unsaved changes. Close OpenCAD without saving
        them?". OK closes through `app_close`; Cancel keeps the window open.
      - Unmodified documents close at once.
      - 🔍 SOLIDWORKS offers Save / Don't Save / Cancel; this prompt is OK / Cancel.
    - **The window title stayed "OpenCAD".** It now follows the document ("Part1 * -
      OpenCAD"), as in Electron, so the taskbar shows the part and the unsaved marker.
    - **Help > About credited "Open CASCADE via replicad".** In the native app it now names
      OCCT 8.0.1 (LGPL with the exception, replaceable DLLs) and `THIRD_PARTY_NOTICES.txt`.
    - **`electron . --smoke` never exited after printing its report.** The same
      `beforeunload` guard cancelled `app.quit()`, because the test part is unsaved. It now
      calls `app.exit(0)`, a bug fix in the frozen app.
  - **Release path.**
    - **Before** (local, while Actions is blocked):
      1. Bump `package.json` and `RELEASE_NOTES.md`.
      2. `npm run typecheck`, `npx vitest run`, `npm run dist:win`.
      3. Commit and push.
      4. `gh release create vX.Y.Z release/OpenCAD-X.Y.Z-win-x64.exe --target main …`.
      5. `npm run deploy:web`.
    - **Now** (written into `CLAUDE.md`, "Commands"):
      1. Bump the version in the three files and in `RELEASE_NOTES.md`.
      2. `cargo test --workspace`, clippy, `npm run typecheck`, `npx vitest run`.
      3. `npm run dist:native:win`, then `npm run test:native:install`.
      4. Commit and push.
      5. `gh release create vX.Y.Z release/OpenCAD-X.Y.Z-win-x64-setup.exe --target main
         --notes-file RELEASE_NOTES.md`, plus the OCCT source archive if the owner decides
         so (below).
      6. `npm run deploy:web`. The browser build is unchanged: it still runs the
         TypeScript engine.
    - `npm run dist:win` (Electron) still works. Its output is renamed
      `OpenCAD-Electron-<version>-…` so it can't be confused with the native installer, and
      it is no longer released on Windows.
    - `.github/workflows/release.yml`:
      - The Windows job builds and uploads the native installer
        (`release/OpenCAD-*-win-x64-setup.exe`). Its steps: MSVC environment, ninja, a
        cached **shared** OCCT build (`occt-shared-8.0.1-…`), rust-cache on
        `target/dist/cargo`, `npm ci`, `npm test`, `npm run dist:native:win`.
      - The macOS job still builds the frozen Electron dmg. The native app is not packaged
        for macOS yet, and Linux is deferred.
      - Not run: Actions is blocked by billing, so the workflow is untested.
    - New npm devDependency: `@tauri-apps/cli` 2.12.4 (Apache-2.0 OR MIT). The lock file's
      stale `0.4.1` is now `0.5.0`.
  - **Checks** (Windows 11, RTX 5070 Ti, D3D12, 125 % scaling).
    - **`npm run test:native:install`** on the final installer: **20/20**.
      - Silent install into a new temporary folder: 1.8 s, 34 files.
      - `opencad.exe` has subsystem 2 (GUI) and imports 16 OCCT DLLs. All of them are next
        to it, with the licences, the notices, the C++ runtime and the uninstaller.
      - The uninstall registry entry exists.
      - The **installed** exe was run from the checkout, with PATH set to Windows' folders
        only:
        - `--smoke`: **7 faces, volume 20 858.407** (24 000 − 1000π), no errors, rebuild
          5.6 ms, `occt: "8.0.1 shared"`; 5.6 s for the run.
        - `--corpus`: **`corpusDiffs: []`**, 17 parts; 4.7 s.
        - `--script=crates/oc-app/scripts/picking.js`: **all 21 checks pass**; 10.1 s.
          `hoverMs` (move until settled) has a median of 5.6 ms and a max of 7.5 ms; main's
          own release build gives 5.7 ms on the same script, so nothing changed.
        - Each run's `SMOKE` line is on stdout and identical in its `.log`.
      - Silent uninstall: 1 s; the folder is empty and the registry entry is gone.
    - **Console attach:** the release exe, started from a new PowerShell console with no
      redirection, printed `viewport: …` and the `SMOKE` line into that console (read back
      from the console buffer).
    - **Close guard** on the release exe:
      - modified part (title "Part1 * - OpenCAD"): the first close was cancelled at the
        prompt; the second was confirmed and closed;
      - unmodified part ("Part1 - OpenCAD"): closed at once.
    - **Test suites:**
      - `cargo test --workspace`: 188 passed, 7 ignored (static `OCCT_DIR`,
        `OC_RENDER_REQUIRE_GPU=1`; 2 new tests in `oc-app`);
      - workspace clippy `-D warnings`: clean;
      - `npm run typecheck`: clean;
      - vitest: 89/89;
      - `npm run build`: passes;
      - `electron . --smoke`: 7 faces, no errors, rebuild 17.5 to 17.9 ms (native: 5.3 to
        5.6 ms). It now exits by itself, in 8 s.
  - **Manual checklist for the owner.** Try these in the installed app. They come from
    FEATURE_SPEC; ✱ marks what the native path changed, so those are most worth a look.
    1. **Install, start, uninstall.**
       - ✱ Run `OpenCAD-0.5.0-win-x64-setup.exe` (SmartScreen: More info → Run anyway).
         There is no administrator prompt.
       - It installs beside the Electron 0.4.2 install, as a separate entry in Settings →
         Apps.
       - ✱ The icon in the installer, the Start menu (`OpenCAD` folder), the desktop
         shortcut, the title bar, the taskbar and Settings → Apps (OpenCAD 0.5.0, publisher
         JGCoolfella).
       - ✱ No console window.
       - ✱ Help > About names OCCT 8.0.1.
       - The install folder holds `TK*.dll`, both LGPL texts and `THIRD_PARTY_NOTICES.txt`.
       - Run the installer again over the install (reinstall or upgrade), then uninstall
         from Settings → Apps.
    2. **Window.**
       - ✱ The title follows the part ("Part1 * - OpenCAD").
       - ✱ Closing a modified part asks first; an unmodified one closes at once.
       - Maximise, restore and drag-resize: the viewport follows, with no stale band.
       - If you can, drag the window to a second monitor with different scaling.
    3. **UI (FEATURE_SPEC 1).**
       - Menus and sub-menus; CommandManager tabs and flyouts (the last-used tool is
         remembered); rich tooltips.
       - FeatureManager hover, click, double-click and right-click; the rollback bar.
       - PropertyManager ✓/✗ and group boxes; the Confirmation Corner.
       - Heads-up toolbar flyouts; the S shortcut bar; the context toolbar after a click;
         right-click menus; mouse gestures; status bar coordinates.
    4. **View (2).**
       - Middle-drag rotates. Middle-click a vertex, edge or face, then drag: it rotates
         about it.
       - Ctrl, Shift and Alt + middle-drag; the wheel zooms toward the pointer; a middle
         double-click fits.
       - Arrows, Shift+arrows, Alt+arrows, Ctrl+arrows; F, Z, Shift+Z, Ctrl+Shift+Z.
       - The Space dialog. ✱ The Ctrl+Space View Selector: hover tint, click to orient,
         floating panels.
       - Ctrl+1…8; Normal To twice.
       - ✱ All five display styles: ✱ 1.5 px edges, ✱ dashed hidden edges in HLV.
       - Perspective; ✱ Section View with the cap.
    5. **Selection (3).**
       - ✱ Hover highlights faces, edges, vertices, planes, sketches and axes.
       - Click, Ctrl, Shift, Esc, and a click on empty space.
       - ✱ Box selection left→right (inside) and right→left. ✱ A crossing box takes a long
         edge crossed in its middle.
       - ✱ Select Other (right-click), faces behind included.
       - Selection filters (F5, F6, E/V/X).
       - ✱ No vertex at the start of a full circle.
       - Planes show on hover; their handles resize them.
    6. **Sketching (4).**
       - Sketch on a plane and ✱ on a model face (Normal To on start).
       - Line, rectangle, circle, arc, spline, slot.
       - Inference lines and snaps; the midpoint highlights after a pause.
       - ✱ Snaps to a model vertex and to an edge midpoint off the sketch plane
         (Coincident/Midpoint to the model).
       - Relations and Smart Dimension; black/blue/red state.
       - Drag under-defined geometry (✱ the Rust solver in WebAssembly).
       - Trim, extend, offset, mirror, ✱ Convert Entities; exit the sketch.
    7. **Features (5).**
       - Extrude: end conditions, thin, draft; ✱ Edit Feature shows the yellow preview at
         once.
       - Cut, revolve, sweep, loft.
       - Fillet (constant and variable), chamfer, shell, draft, rib.
       - Hole Wizard (✱ positions from face picks).
       - Linear and circular pattern (instances to skip), mirror.
       - Reference planes, axes, points and coordinate systems.
       - Suppress, rollback, delete with dependants, error icons.
       - Regeneration feels instant on the patterns part.
    8. **✱ Persistent names (new behaviour).**
       - Pad 10 → 40 with a sketch on its top face: the sketch stays on top.
       - Widen a box with a filleted corner: the fillet follows.
       - Shell a box open at the top, then make it taller: the top stays open.
       - Move a filleted pocket: the fillets follow.
       - Delete a boss a sketch sits on: *Missing reference*, and "Missing" in the box.
    9. **Files (7).**
       - New.
       - ✱ Open a 0.4 `.ocpart`.
       - Save and Save As through the file picker; ✱ reopen.
       - ✱ Open a 0.5.0 file in the Electron 0.4.2: it opens, with names ignored.
       - ✱ Export STEP and STL; Import STEP.
    10. **Evaluate (8).** ✱ Mass Properties, ✱ Measure, Section View.
    11. **Options (9) and keys (10).**
        - ✱ Tools > Options start from the defaults (the webview's storage is new) and
          persist across a restart.
        - Ctrl+Z/Y, Ctrl+S/O/N; Enter repeats the last command; Esc.
  - **Deviations and findings.**
    - Tauri reads an absolute Windows path in `build.frontendDist` (`C:\…`) as a URL with
      the scheme `c`, so the first build opened the folder's directory listing instead of
      the UI. The installed-app test caught it; the dist script now passes a relative path.
    - macOS (nice-to-have) is not packaged, and `dist-native.mjs` stops on other systems.
      Packaging needs OCCT's dylibs in `Contents/Frameworks` with `@rpath` install names
      (the build script adds an absolute rpath to the install today) and a dmg target.
    - The installed app and development builds share one WebView2 profile
      (`%LOCALAPPDATA%\nz.co.greendealer.opencad`, from the identifier), so they share
      System Options; the Electron app's are separate. Uninstall keeps the profile unless
      its "delete app data" box is ticked.
    - The Visual C++ runtime came from Visual Studio 18's redistributable folder (14.51).
      That is newer than the 14.42 toolset OCCT was built with, a direction Microsoft
      supports.
    - Not done:
      - code signing;
      - a real-mouse pass of the installed app (the checklist above);
      - a clean machine without Visual Studio, or with an old WebView2.
  - **For the owner.**
    1. **Code signing** (needs a certificate and your account). Unsigned, the installer and
       `opencad.exe` get SmartScreen's warning, as 0.4.x did. Options: an OV or EV
       certificate, or Azure Trusted Signing. Tauri signs through
       `bundle.windows.signCommand` or `certificateThumbprint`.
    2. **LGPL source.** Attach the OCCT 8.0.1 source archive to each GitHub release next to
       the installer (the safest reading of LGPL 2.1 section 4), or rely on the notices'
       link to the upstream archive?
    3. **Visual C++ runtime.** App-local DLLs, as built (simple, no admin rights, but not
       serviced by Windows Update), or run Microsoft's `vc_redist.x64.exe` from the
       installer (needs admin rights)?
    4. **Upgrade from 0.4.x.** 0.5.0 installs beside the Electron app, and both are called
       "OpenCAD". Leave it to the release note (as written), or add an installer hook that
       offers to uninstall the Electron version? Its uninstall key is the fixed GUID
       `9fbb911e-74b3-572a-9027-5a05969d32bd`; testing the hook would uninstall your 0.4.2.
    5. **Install scope.** Per user (as built, like the Electron app), or per machine / the
       user's choice (`installMode: both`)?
    6. **Close prompt.** OK/Cancel "Close without saving?" as built, or a SOLIDWORKS-style
       Save / Don't Save / Cancel dialog in the UI?
    7. **macOS.** Keep shipping the frozen Electron dmg from `release.yml` (as written; it
       would say 0.5.0 but run the old engine), or offer no macOS download until the native
       app is packaged for macOS?
  - **Owner decisions (2026-11-16, at merge).**
    1. Release unsigned for now (SmartScreen warning, as 0.4.x). Signing later.
    2. Attach the OCCT 8.0.1 source archive to every release (`occt-8.0.1-source.tar.gz`;
       local: `C:\occt\dl\occt-8.0.1.tar.gz`; `release.yml` downloads and uploads it).
    3. Visual C++ runtime app-local, as built.
    4. Upgrade from 0.4.x: explained in the release notes; no uninstall hook.
    5. Per-user install, as built.
    6. Close prompt: OK/Cancel ships in 0.5.0; a SOLIDWORKS-style Save / Don't Save /
       Cancel dialog follows as a small fix. Done on main 2026-11-16 (`SaveChangesDialog`,
       `UiHooks.whenSaved`; also File > New and Open), for the next release.
    7. No macOS download until the native app is packaged for macOS: `release.yml`'s
       Electron dmg job is removed.
- 2026-11-16 **G1 split; G1a done** (branch `claude/g1-assemblies-506315`, not merged). G1
  was too big for one session and is split into G1a (this) and G1b (section 6). New crates
  `oc-mate` (mate solver) and `oc-asm` (assembly building); assembly document and `.ocasm`
  container in `oc-doc`; instance placements in `oc-render`; assemblies in the native app
  (`crates/oc-app/src/asm.rs`, `dialog.rs`, `src/assembly/*`). New dependencies:
  `miniz_oxide`, `crc32fast` in `oc-doc` (both already in the app through png and tauri;
  `docs/THIRD_PARTY.md`), the `windows-sys` feature `Win32_UI_Controls_Dialogs`.
  - **Document (`oc_doc::AssemblyDocument`, `assembly.rs`).** Its own version line
    (`ASM_CURRENT_VERSION = 1`, `ASM_MIGRATIONS`, `load_assembly`, `save_assembly`), read as
    strictly as parts (the same lossless check, through a shared `load_typed`). Part
    documents are unchanged.
    - `components`: id, kind (part or assembly), `path` relative to the assembly's folder,
      `absPath` (where it was last found), `config` (`"Default"` until configurations).
    - `instances`: component, name (`pin<1>`), `fixed`, `transform` (position and unit
      quaternion, normalised on load), suppressed, hidden. **Deviation from the brief:** the
      fixed/floating flag is on the instance, not the component. In SOLIDWORKS the (f) and
      (-) prefixes belong to occurrences: two instances of one part can be one fixed and one
      floating.
    - `mates`: type, two references, `align`, `value`, `flip`, `relative` (Lock), suppressed.
      A reference is an occurrence path (instance ids down through sub-assemblies; empty
      means the assembly itself) and an entity: a face or edge by persistent name and
      signature (E6's `FaceSig` and `EdgeSig`), a vertex, a standard or reference plane, an
      axis or point, the origin, or the whole component. Faces, edges and reference geometry
      also store their analytic geometry (`MateGeom`: plane, line, point, cylinder, cone,
      sphere, circle) in part coordinates, which lightweight builds solve with.
    - `History` is now generic (`History<D: Snapshot = PartDocument>`); the tests use it for
      assemblies. The app keeps its TypeScript undo, as for parts.
  - **File (`oc_doc::container`).** An `.ocasm` is a zip: `document.json`,
    `cache/manifest.json` (cache version, quality, component to part hash, part names) and
    `cache/mesh/<hash>.ocm` (`RenderMesh::to_bytes`). It is a minimal zip writer and reader
    of our own (stored or deflated entries, UTF-8 names, no zip64 or encryption; about 250
    lines) instead of the `zip` crate: the bytes are deterministic (fixed timestamps,
    insertion order) and CRCs are checked on reading. Python's `zipfile` reads the files.
    The content hash is a length-prefixed 128-bit FNV-1a of the part's canonical JSON, the
    image quality and `CACHE_VERSION`. There is no B-rep cache: the kernel trait has no
    native body transfer yet (G3's `export_native`), and regenerating these parts is faster
    than reading STEP.
    - **Decision: parts stay `.ocpart` JSON for now.** The Electron app and the web build
      read them, and the container would only add caches to small parts. Move parts to the
      container with W1, or when a part's cache pays (large imported parts).
    - **Part references:** the relative path, then the stored absolute path, then the file
      name in the assembly's folder, then search folders and the folders where earlier
      components were found (`oc_asm::locate`; 🔍 SOLIDWORKS' exact order). Saving rewrites
      paths relative to the new location and records the absolute one.
  - **Mate solver (`oc-mate`).** Built on E2's core: `Real` and `Dual` (exact Jacobians),
    `sparse::min_norm_step` (damped minimum-norm Gauss-Newton), and `analysis::rref` (now
    public) for degrees of freedom and redundancy.
    - A rigid body has 6 parameters: a translation, and a rotation increment
      `q = (1, ω/2)/√(1+|ω|²/4)` applied on top of the pose. Jacobians are taken at `ω = 0`
      and the increment is folded in after every accepted step (a local
      parameterisation): no Euler angles, so no gimbal lock (tested from 90° pitches and
      from 179° away).
    - Every mate is a `Rule` chosen from the geometry classes (plane, line, round, circle,
      sphere, point). Parallel directions are three rows `b − s·a` when solving: only the
      requested alignment solves them, so Aligned and Anti-Aligned need no pre-flip. For
      the rank analysis they are two projected rows; with three, every such mate would look
      redundant against itself. Mates: Coincident (plane/plane, plane/line, plane/point,
      plane/circle, line/line, line/point, point/point, circle/circle), Parallel,
      Perpendicular, Concentric (axes, axis and point, spheres), Tangent (plane/cylinder,
      plane/sphere, cylinder/cylinder, cylinder/sphere, sphere/sphere), Distance (planes,
      plane/point, plane/axis, axes, axis/point, points), Angle, and Lock (relative pose;
      the vector part of the error quaternion).
    - **Finding: the sketch rule "a dependent row is redundant" is wrong for mates.** Three
      coincident faces share rotation rows (6 direction rows for 3 rotations), and the
      sketch rule flagged the second and third mates. A mate is redundant only when *none*
      of its rows adds rank; partial repeats are accepted, as SOLIDWORKS accepts them.
    - Conflicts are found by solving. If the full solve fails, the mates are added one at
      a time and each one that breaks the solve is set aside, so the newest conflicting
      mate is the one reported and the rest stay satisfied.
    - State per body: fixed, under, full (all six parameters are pivots with no free
      components), over (touched by a redundant or conflicting mate). `free` is 6 minus the
      rank of the body's own columns.
    - **Drag:** the motion lies in the null space of the mate Jacobian, from an explicit
      basis out of the same RREF. A first version projected with the damped
      `min_norm_step`; its rounding swamped the tangent direction near the end of a drag,
      so the arm stopped short. Rotation is weighted as a lever (mm per radian), so a free
      body translates and a hinged one turns. Sub-steps turn at most 0.1 rad and are retried
      shorter when they overshoot. The drag is dense (`O(n²)` per step): fine for G1; G2
      must drag one connected group of bodies at a time.
    - Tests (`crates/oc-mate/tests/mates.rs`, 19): every mate type with its DOF; aligned
      and anti-aligned from 170° away; a hinge (1 DOF); drags on a hinge and on a plane;
      a redundant mate (the newest flagged, its body Over); a conflict (set aside, the others
      solved, the order decides which); mates between fixed components; invalid pairs;
      suppressed mates; a 2R chain; many start orientations.
  - **Building (`oc-asm`).** The `Assembler` lives on the engine thread and owns the
    regenerator that part documents use too. It locates and reads every component, hashes
    each part (`part_hash`), and regenerates and meshes each distinct hash once: two pins,
    or the same pin saved under two file names, are one regeneration and one mesh. It
    resolves mate references on regenerated parts by name (`oc_naming::resolve`), solves,
    and lists the occurrences (instance path, part, world placement). Heals (a reference
    found under a new name or with new geometry) are written back without an undo step.
    Sub-assemblies are rigid (built recursively, with a cycle guard); mates may reference
    their parts by occurrence path.
    - **Lightweight:** a part whose hash matches the container's cache is shown with the
      cached mesh and not regenerated, and its mates use the stored geometry. That is
      consistent because the stored geometry was refreshed against the same content when
      the file was saved. Picking its geometry resolves the part. A missing part file with
      a cache is shown from the cache, with a warning.
    - Tests (`tests/assembly.rs` 10, `tests/corpus.rs` 2): shared regeneration; mates follow
      an edited part (hole moved to (20, 12), plate 8 → 10 thick: the pin follows, heals are
      reported, and the next build heals and regenerates nothing); Missing (holes deleted:
      the concentric mates are Missing on the plate's side and the pins keep 3 DOF); the
      file search; lightweight opening (0 regenerations; an edited part alone is
      regenerated); a byte-identical save; strict loading, a made-up version 0 migration,
      rotation normalisation; History; suppressed instances and mates; dragging the hinge
      arm.
  - **Corpus (`corpus/assemblies/`).** The parts `plate`, `pin` and `arm` come from the TS
    corpus builder (`src/kernel/fixtures/assemblyParts.test.ts`, face counts asserted). The
    assemblies are `bracket-pins.ocasm` (plate fixed, two pins concentric with its holes and
    seated on it: 2 DOF, one pin mesh) and `hinge.ocasm` (an arm turning about the plate's
    hole, a pin turning in the arm: 2 DOF). `crates/oc-asm/tests/corpus.rs` builds them with
    faces picked on the regenerated parts (`UPDATE_GOLDEN=1` re-records), and
    `<name>.json` records DOF, states, positions and mate statuses. The stored files must
    equal a fresh build byte for byte and open to the same state, resolved and lightweight.
    - Finding: a one-direction Through All cut from the Front plane missed the plate (it
      went away from the body) **without an error**; SOLIDWORKS reports a cut that removes
      nothing. The parts use Through All Both, and the test asserts face counts. Follow-up
      for both engines.
  - **Renderer (`oc-render`).** `BodyItem::placement` (rotation and translation, f64; the
    shader gets the rotation and the placed origin relative to the camera target).
    Drawing, GPU picking, vertex markers, model bounds, ray casts and box selection apply
    it, and one uploaded mesh is drawn by every occurrence. GPU test
    `smoke_instances_share_one_mesh`.
  - **Native app.** Engine jobs `asm_open`, `asm_build`, `asm_drag`, `asm_entity` and
    `asm_save`; `app_pick_file` (the Windows common Open and Save As dialogs),
    `app_read_text` and `app_write_text`. Each build hands one mesh handle per distinct
    part to the renderer; drag replies carry only placements.
    - UI (`src/assembly/*`): `App.asm`, the open assembly. The Assembly tab has Insert
      Components, Mate, Fix and Float; Linear Component Pattern, Smart Fasteners,
      Move/Rotate Component, Show Hidden, Assembly Features, Motion Study, BOM, Exploded
      View and Interference Detection are visible as todo. File > New Assembly and
      Insert > Component. The assembly FeatureManager has the (f)/(-)/(+) prefixes, the
      Mates folder, error icons and **Missing**. The Mate PropertyManager has Entities to
      Mate, the Standard Mates, Distance/Angle with Flip, and Aligned/Anti-Aligned; the type
      and alignment default from the picks, and the assembly re-solves as the mate is set
      up. Left-drag moves an under-defined component. Delete, Suppress and Edit Feature are
      routed to assemblies, and undo keeps document snapshots. Graphics selections carry the
      occurrence index (`body`).
    - In the native app File > Open and Save use the native dialog, because assemblies need
      paths; parts saved that way remember their path. Assembly commands are greyed outside
      the native app (`src/assembly/commands.test.ts`).
  - **Checks** (Windows 11, RTX 5070 Ti, D3D12). `opencad --script=crates/oc-app/scripts/assembly.js`
    (`npm run native:assembly`): **36 checks pass.**
    - Opening the hinge lightweight takes 256 ms with every part from the cache; states,
      mates, DOF 2 and the tree's (f)/(-) are right.
    - Hover and click select a face of an instance.
    - Dragging the arm with pointer events turns it about the pin to the pointer's angle
      (77.7°, as expected from the screen-plane target), keeping its radius and height;
      the pin follows; one undo step, and undo restores it.
    - bracket-pins draws 3 occurrences with 2 meshes.
    - A new assembly from the corpus parts: Insert Components; Concentric and Coincident are
      proposed from face picks; the pin sits at (15, 15, 8) with 1 DOF; a contradicting
      Distance mate is flagged as the newest conflict with (+) on the pin and the others
      solved; undo; Save As with relative paths; reopen.
    - `cargo test --workspace`: 226 passed, 7 ignored (static `OCCT_DIR`,
      `OC_RENDER_REQUIRE_GPU=1`); workspace clippy `-D warnings` clean; `npm run typecheck`
      clean; vitest 96/96. `--smoke`: 7 faces, volume 20 858.407; `--corpus`:
      **`corpusDiffs: []`** (17 parts); `native:picking`: 21/21.
    - Making `oc_sketch::analysis::rref` public changed oc-sketch's sources, so the committed
      WebAssembly was rebuilt (`npm run build:sketch-wasm`; only the binary and its hash
      changed).
  - **Parity with SOLIDWORKS** (✅ same, ≈ differs, ⬜ not yet; 🔍 marked in FEATURE_SPEC 6):
    - ✅ The (f)/(-)/(+) prefixes, the Mates folder, the standard mate types with alignment
      and Flip, the first component fixed at the origin, mates following part edits,
      Missing references, the newest conflicting mate flagged, dragging within the free
      DOF.
    - ≈ Redundant but satisfiable mates are accepted when they add anything and over
      defining when they add nothing (🔍 SOLIDWORKS' exact tolerance). A satisfied mate
      between two fixed components counts as over defining (🔍).
    - ≈ Insert Components places later components beside the others instead of where you
      click. No Mate pop-up toolbar, no SmartMates.
    - ⬜ Open Part from the assembly; Edit Mate with re-picking of Missing references in the
      graphics area; Move/Rotate Component PropertyManagers; mates to assembly planes from
      picks (the document and the solver support them); mates on sub-assembly geometry
      from picks; Hide/Show, rename, configurations, component colours.
  - **Left for G1b:** the ⬜ items above, the Mate pop-up toolbar, Insert Components with a
    placement preview, sub-assembly editing, and assembly-level Measure and Mass
    Properties (the Evaluate tab shows Measure, which measures part geometry only).
  - **Notes for G2 and G3.** The mate solver's conflict search re-solves once per mate when
    the full solve fails, and the drag is dense: both must work per connected group of
    bodies (the analysis already splits them). Every build adds one mesh handle per part
    again, so mate edits re-upload meshes; G2 should keep handles across builds.
    Lightweight opening is serial per part; G3 can regenerate the parts that need it in
    parallel (their hashes are independent), and a native B-rep cache would make resolving
    lightweight parts cheap.
  - **For the owner.**
    1. Fixed/floating on instances rather than components, as SOLIDWORKS does: agree?
    2. Redundant mates: keep "over defining only when it adds nothing", or flag every
       partially redundant mate as the sketch solver does?
    3. Parts stay JSON `.ocpart` until W1 (assemblies are containers): agree?
    4. File > Open and Save in the native app now use the Windows dialog with real paths
       (needed for assemblies); parts opened with the old WebView picker still save there.
       Keep this, or move all native file access to paths?
  - **Owner decision (2026-11-16): "do like SOLIDWORKS, or as close as possible for now"**,
    applied on the same branch:
    1. Fixed/floating stays on instances (SOLIDWORKS).
    2. Over-defining mates: the Mate PropertyManager refuses a mate that would over-define
       the assembly (conflicting, adding nothing, or an invalid pair), says so in the
       PropertyManager ("This mate would over define the assembly. Consider deleting some
       of the over defining mates.", 🔍 exact wording) and keeps OK disabled, as SOLIDWORKS
       does. Mates already in the document that become conflicting stay in the tree with
       their error, the newest flagged (unchanged). Partial repeats stay accepted.
    3. Parts stay `.ocpart` JSON for now. SOLIDWORKS parts carry their own display data;
       the user-visible effect (an assembly opens without regenerating its parts) is
       already there through the assembly's caches, and JSON keeps the browser and
       Electron builds reading parts until W1, when parts can become containers too.
    4. All native file access is by path, as SOLIDWORKS: Open, Save, Save As and now STEP
       and STL export use the Windows dialogs (`engine_export_to` writes the export where
       the dialog says). Assembly STEP export is greyed in an assembly until it exists.
    - Checks after the change: `native:assembly` 39 checks (new: the PropertyManager
      refuses a contradicting Distance mate and keeps the others solved; a conflicting mate
      put in the document is still flagged; a part opened and exported to STEP by path).
      Found while adding them: a syntax error in a `--script` file (a repeated `const`) makes
      the app wait for its 900 s watchdog without saying why; worth reporting script
      syntax errors at once (follow-up in `smoke.rs`).

- 2026-11-16 **G1b done** (branch `claude/g1b-asm-ui-36d481`, not merged). G1b was split:
  this slice is the assembly UI; G1c (section 6) keeps assembly Measure, Interference
  Detection (pending the owner) and the smaller items listed there. No new dependencies.
  - **Engine.**
    - `oc_asm::place(built, id, transform)`: puts a body at a pose and solves with it held
      (the floating components mated to it follow); if that leaves more mates in error than
      before, it solves again with the body free, so the mates take it to the nearest pose
      they allow. Used by Move/Rotate Component by values, Along XYZ / Along Entity
      (pure translations, exact) and rotation drags. Free drag stays `oc_mate::drag`: its
      null-space step is not an exact translation for a free body (a constrained drag along
      X through it drifted 0.002 mm across), so constrained moves go through `place`.
      Test `placing_a_component_respects_its_mates` (hinge: a 30° turn about the pin is kept
      and the pin stays; 10 mm off the axis is taken back; a fixed body refuses).
    - `oc_doc::Instance::color` (optional `#rrggbb`; optional field, version line unchanged).
    - Engine jobs `asm_place` and `asm_mass` (per occurrence: unit-density volume, area,
      centre at its placement, the part's material; the UI applies densities). `asm_build`
      and `asm_save` take `overrides` (file → document text): `asm::Overlay` is a `Store`
      that reads them before the disk; a sub-assembly's document is put into its container
      from disk so its caches still apply.
    - **Fixed (G1a bugs found here):** (1) parts inside sub-assemblies got no mesh handle in
      the build reply (only top-level components' parts were listed), so sub-assemblies drew
      nothing in the native app; (2) `asm_save` took the found component paths from the last
      build, whatever document that was: saving a sub-assembly edited from its parent
      rewrote its paths with the parent's components. `asm_save` now takes `from` (where the
      document was) and locates its own components first.
    - `app_write_text` creates the parent folder (scripts; the dialogs give existing ones).
    - **`smoke.rs`:** the scenario is passed as a string and compiled with the
      `AsyncFunction` constructor inside the `try`, so a syntax error in a `--script` file is
      reported at once (`{"exception":"SyntaxError: ..."}`, 6 s for a repeated `const`)
      instead of the 900 s watchdog. Unit test `script_compiled_inside_try`. The new
      assembly script itself hit this once (a repeated `const t0`) and reported it at once.
  - **UI** (`src/assembly/*`, `MovePM.tsx` new).
    - Mate pop-up toolbar (`MatePopup`) at the last press in the graphics area once two
      entities are picked: the proposed mate highlighted, the mates the pair allows
      (`validMates`, mirroring `oc-mate`'s rule table, either order) and the rest greyed,
      distance/angle value, Flip Mate Alignment, OK, Cancel (clears the selections).
    - Edit Mate: references that no longer resolve read **Missing**, the PropertyManager
      says so, OK stays disabled and the next pick replaces them (`MatePM.active`);
      clicking an entry re-picks it, × removes it.
    - Insert Components: after the first (fixed at the origin) the component's box centre
      follows the pointer in the plane through the view's centre facing the viewer; a click
      drops it; Esc undoes the insertion. `insertComponent(file, at)` drops at once.
    - Move/Rotate Component PropertyManager: Move (Free Drag, Along Assembly XYZ, Along
      Entity, By Delta XYZ, To XYZ Position), Rotate (Free Drag about the box centre, About
      Entity, By Delta XYZ about X then Y then Z through the component's origin), Options
      (Standard Drag; Collision Detection and Physical Dynamics greyed). Undo works while it
      is open (one step per drag or Apply). Options > Assemblies "Move components by
      dragging" is honoured.
    - Component context menu (tree and graphics): Open Part / Open Assembly, Hide / Show
      Components, Fix / Float, Suppress / Unsuppress, Appearance (colour per occurrence,
      drawn by `oc-render`), Remove Appearance, Move / Rotate Component, Rename tree item
      (inline; `<n>` kept), Delete. Mates: Edit Feature, Suppress, Rename, Delete.
    - FeatureManager: the assembly's Front/Top/Right Planes and Origin; a component
      expands to its own. While an assembly PropertyManager is open the tree is the fly-out
      in the graphics area and those rows are mate picks (`treeRef`: `$plane:top`,
      `i3/$origin`).
    - SmartMates: Alt-drag a face, edge or vertex onto another component's: the Mate
      PropertyManager opens with both picked and the pop-up toolbar to confirm.
    - Open Part / Open Assembly and Return to Assembly (Window menu lists the assemblies
      on `App.asmStack`). Unsaved edits travel back as `AssemblySession.overrides`; the
      assembly shows them at once, mates follow by persistent name, and saving the assembly
      saves them first.
    - Mates on sub-assembly geometry from graphics picks (occurrence paths; labels
      `Face@plate@sub<1>`).
    - Mass Properties in an assembly (`AssemblyMassProps`): mass, volume, area, centre of
      mass over the occurrences at their placements with each part's density; "Include
      hidden bodies/components" (default from Options > Assemblies, 🔍).
    - Commands: `asm.moveComponent`, `asm.rotateComponent`, `asm.openPart`,
      `asm.returnToAssembly`, `asm.hide`, `asm.show`, `asm.appearance`,
      `asm.removeAppearance`, `asm.rename` (all greyed outside the native app); Mass
      Properties on the assembly Evaluate tab. Still todo: Linear Component Pattern, Smart
      Fasteners, Show Hidden Components, Assembly Features, Motion Study, BOM, Exploded View,
      Interference Detection, New Part.
  - **Checks** (Windows 11, RTX 5070 Ti, D3D12). `npm run native:assembly`: **120 checks
    pass** (39 from G1a, 81 new), driven through DOM pointer events, context menus, the
    CommandManager, the fly-out tree, the Window menu and PropertyManager inputs: insert
    preview (follows, drops, undo/redo, Esc); SmartMates by Alt-drag with hover checks, the
    pop-up (proposal, greyed types, flip, OK); Open Part → move a hole → Return (the pin
    follows; the file untouched; the assembly dirty) → delete the cut → the mate Missing →
    Edit Mate re-picks a plate edge → solved; every Move and Rotate mode, including mates
    holding a component and taking back a forbidden turn; Hide/Show, rename, appearance,
    Fix/Float, Suppress; mates to the assembly's Top Plane and Origin and the arm's Front
    Plane and Origin from the fly-out; save with the edited part (paths relative); a
    sub-assembly inserted, drawn, mated on its plate's face, opened, edited, returned and
    saved with the top; Mass Properties (dialog; the pin's volume equals the part's; centres
    move with the placement).
    - `cargo test --workspace`: 228 passed, 7 ignored (static `OCCT_DIR`,
      `OC_RENDER_REQUIRE_GPU=1`); clippy `-D warnings` clean; `npm run typecheck` clean;
      vitest 100/100; `--corpus`: **`corpusDiffs: []`** (17 parts); `native:picking` ok.
  - **Finding (naming, for the owner / E6):** with two holes cut from one sketch, deleting
    the circle of the hole a concentric mate uses does not make the mate Missing: its
    reference resolves onto the other hole (same feature, same role, best score) and is
    healed. SOLIDWORKS reports the mate's reference missing. The script deletes the whole
    cut instead. Worth a rule in `oc_naming::resolve`: a candidate of the same role whose
    position moved by more than the size tolerance is not the same entity.
  - **Parity with SOLIDWORKS** (✅ same, ≈ differs, ⬜ not yet; 🔍 marked in FEATURE_SPEC 6):
    - ✅ Mate pop-up toolbar with valid mates only; Edit Mate with Missing references
      re-picked; Insert Components preview and drop; Move/Rotate Component modes within
      the mates' freedom; Hide/Show, Fix/Float, Suppress, rename, appearance from the
      context menu; mates to assembly and component planes and origins from the fly-out
      tree; mates on sub-assembly geometry; Open Part / Open Assembly and back with mates
      following unsaved edits; assembly Mass Properties with densities.
    - ≈ One document window: Open Part replaces the assembly in the window (Window menu to
      return) instead of a second window. Saving the assembly saves edited components
      without the Save Modified Documents dialog (🔍). Insert drops in the view plane, not
      at the depth of geometry under the pointer (🔍). Rotate Free Drag turns about the box
      centre (🔍); By Delta XYZ order X, Y, Z about the component origin (🔍). SmartMates:
      the component does not follow the pointer while Alt-dragging. A part inside a
      sub-assembly opens the sub-assembly. Hidden components counted in Mass Properties by
      default (🔍).
    - ⬜ (G1c) assembly Measure; Interference Detection; Edit Part in context; Isolate;
      Component Properties; configurations; Collision Detection / Physical Dynamics.
  - **For the owner.**
    1. Interference Detection: G1c (exact B-rep common of each overlapping pair's
       bodies, bounding-box pre-screen; GPU mesh screening later) or keep it in H1 with
       exact mass properties and clearance? Recommendation: G1c, since it is assembly UI
       parity and the kernel already has booleans and mass properties.
    2. Saving an assembly with components edited from it: always save them (as built), or
       a SOLIDWORKS-style Save Modified Documents dialog listing them (Save All / choose)?
    3. Open Part in place (one window, Window menu to return) as built, or real multiple
       document windows (a larger change to `App`)?
    4. The naming finding above: fix in `oc-naming` now (small E6 follow-up) or later?
  - **Owner decisions (2026-11-16, at merge).**
    1. Interference Detection goes in G1c (exact B-rep with a bounding-box pre-screen).
    2. Add SOLIDWORKS' Save Modified Documents dialog when saving an assembly with edited
       components (G1c).
    3. Keep one document window for now (Window menu to return). Real multiple document
       windows become their own package later.
    4. Fix the naming finding now, in G1c: a same-role candidate that moved by more than the
       size tolerance is not the same entity, so the mate reports Missing, as SOLIDWORKS does.

- 2026-11-19 **G1c done** (branch `claude/zealous-lederberg-6c5684`, not merged). Items 1 to 4 of
  the brief and two of item 5 (Isolate, Component Properties); the rest of item 5 is split
  out as G1d (section 6). No new dependencies.
  - **Naming (owner decision 4; `oc_naming::resolve`, section 3.3 step 2a amended).** A
    stand-in of the same feature and role must be within the size tolerance (25 % of the
    stored size) of where the reference was: a face by its centroid against √area, an edge by
    its middle (a closed edge by its centre, the point halfway between its start and its
    middle, since a redrawn circle may start elsewhere) against its length, a vertex against
    the smallest face around it. Otherwise step 3 decides, and with two holes cut from one
    sketch, deleting one circle makes its mate **Missing** instead of healing it onto the
    other hole.
    - **Expected value changed (own commit, `33b837a`):** the unit test "the one face of the
      same feature and role stands in for a lost key" moved the redrawn cap 35 mm (a 40 × 30
      block grown to 70 × 30 × 45) and expected it to stand in "wherever it is now". Under the
      owner's rule that is not the same face; the test now redraws the profile a little wider
      and a new assertion checks that the far move is missing. Consequence for the owner: a
      profile redrawn with new entities **and** moved or resized by more than a quarter of the
      cap's size loses references to its cap (SOLIDWORKS keeps them; it names caps by feature,
      not by loop). TT1a (a notched corner) is unaffected.
    - Tests: `a_stand_in_that_moved_is_not_the_same_entity` (a plate with two holes from one
      sketch: face, rim edge and vertex of the deleted hole are missing; a circle redrawn in
      place or 1 mm off is still found, by role or by position); oc-asm
      `deleting_one_of_two_holes_from_one_sketch_makes_its_mate_missing`. It fails without the
      fix (the face resolves by role onto the other hole). E6's torture scripts TT1 to TT7,
      the zero-`/new/` check and the thread-determinism test pass unchanged.
  - **Fixed (G1a bug found here): a mate reference keeps its axis sense.** After deleting one
    of the two holes, OCCT built the remaining hole with its cylinder axis reversed; the
    concentric mate's alignment flipped and it conflicted with the coincident mate on the
    pin's head (the pin turned over and showed (+)). An axis has no direction of its own, so
    `oc_asm::build` keeps the stored sense for cylinders, lines and circles (planes keep their
    outward normals, cones their apex side).
  - **Interference Detection (owner decision 1; `oc_asm::evaluate`, engine job
    `asm_interference`).** The whole assembly or the selected components (one: every pair
    involving it; two or more: pairs among them), hidden components ignored unless included,
    Treat subassemblies as components. Each pair's world boxes (the part's box placed, eight
    corners) are compared first; candidates get the exact `BooleanOp::Common` of the two
    placed bodies and its volume. An empty common (`KernelError::Empty`) is volume 0; with
    Treat coincidence as interference, a pair at distance ≤ tolerance is listed as Coincident.
    The common bodies are meshed for the graphics area (`BodyRole::Highlight` in `oc-render`:
    its own colour, drawn over everything like a cut preview, not pickable).
    - Tests (`crates/oc-asm/tests/evaluate.rs`): the seated bracket-pins has none (3 pairs, 2
      candidates after the boxes) and 2 coincidences; a pin through the plate between the
      holes: π·3²·8 = 226.195 mm³ to 1e-6; a pin 7 mm from the other: the shank's 226.195 mm³
      and the heads' lens 3·(2r²·acos(d/2r) − (d/2)·√(4r² − d²)) = 44.33 mm³; the selection
      rules; hidden components; a lightweight hinge resolved to check it (3 coincidences).
    - GPU mesh screening of pairs is left for G2/H1 (the box pre-screen is quadratic in the
      number of shown occurrences; fine for hundreds).
  - **Assembly Measure (engine job `asm_measure`).** Faces, edges and vertices picked on
    occurrences, at their placements: area, length, circle diameter and centre, XYZ, a plane's
    normal or a line's direction; the minimum distance between the first two on the placed
    bodies (Delta X/Y/Z and the angle in the dialog, as for parts).
    - **Finding: OCCT renumbers a transformed body's faces.** `BRepBuilderAPI_Transform`
      (copy = false) on a translated pin gave faces 0, 1, 2 as 1, 2, 0; measuring "face 1"
      of the placed pin measured another face (distance 0 instead of 3). Entities are now
      found on the placed body by geometry (`Assembler::placed_entity`: centroid and area,
      middle and length, point), test `entities_are_found_on_placed_bodies_by_geometry`.
      Worth knowing for anything that indexes a transformed body (patterns use history).
  - **Save Modified Documents (owner decision 2; `SaveModifiedDialog` in
    `src/ui/Dialogs.tsx`, wiring in `src/ui/Shell.tsx`).** Saving an assembly with components
    opened from it and edited lists the assembly (always saved, its box fixed) and the
    components with boxes: Save All, Save Selected once a box is cleared, Cancel. Components
    not chosen stay edited in memory and keep the document modified
    (`AssemblySession.save(path, components)`). New, Open and closing the window
    (`UiHooks.whenSaved`, `app_doc` now reports these edits as unsaved) list the open
    document, the edited components and the assemblies waiting on `asmStack`: Save All /
    Save Selected, Don't Save, Cancel; leaving clears `asmStack`. Without edited components
    the usual "Save changes?" dialog.
  - **Isolate** (component context menu): only the selected components drawn; "Visibility of
    removed components" Wireframe / Transparent / Hidden (Wireframe drawn transparent) and
    Exit Isolate in a bar over the graphics area. View only. **Component Properties...**:
    Component Name (the `<n>` kept), Instance Id, Full Name, Model Document Path,
    Referenced configuration, Suppressed / Resolved; OK is one undo step.
  - Commands: `asm.interference` (was todo), `asm.isolate`, `asm.exitIsolate`,
    `asm.properties`; `eval.measure` measures the current selection when opened. Esc closes
    Interference Detection. `NativeView.lastBodies` exposes the last frame's bodies to
    scripts.
  - **Checks** (Windows 11, RTX 5070 Ti, D3D12). `npm run native:assembly`: **165
    checks pass** (120 from G1a/G1b, 45 new in section 6 of the script, through DOM pointer
    events, the Evaluate tab, PropertyManager inputs, context menus, Ctrl+S and the New
    button): the seated assembly (none, coincidences); the naming case through Open Part,
    edit, Return; Save Modified Documents (Save Selected leaves the part edited in memory,
    Save All writes it; New → Cancel, Don't Save leaves the files untouched); exact volumes,
    red volumes in the frame, interfering parts transparent, a selected result highlighting
    its components, Ignore, Show ignored, Non-interfering Hidden, Selected Components picked
    in the graphics area (one, two, × to remove), Esc; Measure (the plate's face area
    1771.73 mm², the plate's top to a pin's head 3 mm, parallel); Isolate (hidden,
    transparent, Exit); Component Properties (rename keeps `<1>`, suppress, undo).
    - Found while adding them: the Mass Properties dialog of G1b's step 5h stays open, and
      while a dialog is open the shell ignores shortcuts (Ctrl+S, Esc); section 6 closes it
      first. `menu()` in the script now waits up to 2 s for a context-menu item.
    - `cargo test --workspace`: 235 passed, 7 ignored (static `OCCT_DIR`,
      `OC_RENDER_REQUIRE_GPU=1`); workspace clippy `-D warnings` clean; `npm run typecheck`
      clean; vitest 100/100; `--corpus`: **`corpusDiffs: []`** (17 parts); `native:picking`
      ok.
  - **Parity with SOLIDWORKS** (✅ same, ≈ differs, ⬜ not yet; 🔍 marked in FEATURE_SPEC 6):
    - ✅ A deleted entity's mate is Missing even when a sibling of the same feature remains;
      Interference Detection's selection rules, Calculate, Results with volumes, Ignore and
      Show ignored, volumes shown in red, Treat coincidence / subassemblies, Ignore hidden;
      Measure across components; Save Modified Documents with Save All / Save Selected;
      Isolate; Component Properties' name and suppression.
    - ≈ Coincidences labelled "Coincident" with volume 0 (🔍 SOLIDWORKS' label). Make
      interfering parts transparent on and Non-interfering Use current by default (🔍).
      Wireframe (Isolate and Non-interfering) is drawn transparent. Results are listed in
      pair order. The dialog's wording and whether the saved document's box can be cleared
      (🔍). A redrawn and moved cap loses its references (above).
    - ⬜ (G1d) SmartMates with the component following the pointer, the Move Component
      SmartMates button, Edit Part in context, configurations, Component view and Excluded
      Components in Interference Detection, Solve as Flexible, display states.
  - **For the owner.**
    1. The step 2a rule also applies to end caps: a profile redrawn with new entities that
       also moves or resizes the cap by more than 25 % of its size now loses references to
       the cap (the old unit test expected it to follow). Keep, or exempt caps (`start`,
       `end`, `mid`) whose feature still has exactly one cap of that role, as SOLIDWORKS names
       caps by feature?
    2. Save Selected leaves unchosen components edited in memory and the assembly modified
       (they are asked about again on the next save or when leaving). Agree, or discard them
       (🔍 what SOLIDWORKS does with an unchosen document while its window stays open)?
    3. G1d (SmartMates following the pointer, Edit Part in context, configurations) next, or
       G2 first?
  - **Owner decisions (2026-11-19, at merge).**
    1. Exempt end caps, as SOLIDWORKS names caps by feature: a `start`, `end` or `mid` cap
       whose feature still has exactly one cap of that role keeps its references when the
       profile is redrawn and the cap moves or resizes. First item of G1d.
    2. Save Selected: unchosen components stay edited in memory, as built.
    3. G1d next, then G2.

- 2026-11-19 **G1d done** (branch `claude/g1d-asm-parity-c092b6`, not merged). Items 1 to 3 of
  the brief and most of item 5; configurations (item 4) and named display states are split
  out as G1e (section 6). No new dependencies.
  - **Naming: end caps exempt (G1c owner decision 1; `oc_naming::resolve`, section 3.3 step
    2a amended).** A stand-in found by feature and role whose stored tag is a cap (`start`,
    `end`, `mid`) is not held to the 25 % distance rule: the one cap of that role of the
    feature is that cap wherever it moved or however it was resized. An edge or vertex is
    exempt only when every stand-in face it needed was a cap, so a side stand-in is still
    held to the rule and deleting one of two holes from one sketch still makes its mate
    Missing.
    - **Expected values changed (own commit, `b346f4b`):** G1c's `33b837a` moved the redrawn
      cap of "the one face of the same feature and role stands in for a lost key" back
      within 25 %; it is restored to the 70 × 30 × 45 block (35 mm away, found by role),
      and G1c's far-move assertion in `a_stand_in_that_moved_is_not_the_same_entity` now
      expects the cap to be found. The rule follows in `09488bc`.
    - Tests: `a_redrawn_cap_keeps_its_references_however_far_it_moved` (both caps moved 100
      mm and shrunk; a `mid` cap; an edge and a vertex through the cap; a side stand-in
      still missing; two caps of a role still none); oc-asm
      `a_mate_on_an_end_cap_follows_a_redrawn_moved_profile` (the plate's outline redrawn
      with new entities, twice as large, 12 mm thick: the pins' coincident mates stay Ok and
      the pins sit at z = 12; Missing before the rule). E6's TT1–TT7, the zero-`/new/` check,
      the thread-determinism test and G1c's naming case in `assembly.js` pass unchanged.
    - **Finding (🔍):** the exemption follows the owner's wording ("the feature still has
      exactly one cap of that role"). A blind cut of two circles has two bottom caps;
      deleting one circle leaves one, so a reference to the deleted hole's bottom face now
      moves onto the other hole's bottom. The concentric mates in the corpus are on side
      faces, which keep G1c's rule. Whether SOLIDWORKS does the same is not confirmed.
  - **SmartMates following the pointer (`src/assembly/smart.ts`).** Alt-drag is a drag of
    the component (`asm_drag`, within its mates' freedom) with the component transparent
    and not pickable (new `oc_render::BodyItem::pickable`; picks, ray casts and box
    selection skip it). Over a face, edge or vertex of another component the pair can mate,
    the proposed mate (`proposeMate`, the Mate PropertyManager's defaults, factored out of
    `defaults`) is solved by a **probe build** (`asm_build` `probe: true`: the engine's last
    build, which drags and placing use, is kept) from the drag's placements, and the
    component snaps to it; the pointer shows the type (`[data-smart-pointer]`). Off the
    target the drag's placements come back and it follows the pointer again. Dropped on
    the target: the drag is one undo step and the Mate PropertyManager opens with both
    references and the pop-up toolbar. Dropped on nothing: a drag. Esc undoes the drag.
    Move Component's SmartMates button and the `asm.smartMates` command (was todo):
    double-click an entity of the component, hover the target (previewed), click it.
    - Found while testing: the viewport made no picks for moves with a button down
      (`pickNeed`), so SmartMates saw nothing until release; it now picks during an
      Alt-drag. An Alt-drag follows only moves with the button down (WebView2 also sends
      moves at the real cursor when the page changes).
  - **Edit Part in context (`src/assembly/context.ts`).** Edit Part (component context menu
    and toolbar) and Edit Component (Assembly tab, a toggle) put the part's document in
    front with the assembly on `asmStack` (the entry carries `context`), as Open Part does;
    the confirmation corner's Exit Edit Component, Edit Component and the part context
    menu's "Edit Assembly: <name>" return through `returnToAssembly`, so unsaved edits,
    Save Modified Documents and leaving work as for Open Part. The part is edited in its
    own coordinates and the camera is re-expressed (`viewHooks.transformView`), so nothing
    moves on screen. `NativeView` draws the other occurrences (bodies from `CTX_BODY`,
    transparent per Options > Display > Assembly transparency for in context edit, other
    occurrences of the same part with its live mesh, not pickable); their picks are
    selection items with that `body`, taken only by plain selection (for Sketch) and
    Convert Entities. The assembly must be saved first (the references name its file).
    - **In-context references (owner question 1).** Kept with the part, as SOLIDWORKS
      keeps them: `PlaneRef { kind: "external", sig, ext }` (Sketch on another component's
      planar face) and `SketchRef { kind: "external", id, sig, ext }` (Convert Entities of
      another component's edge; the profile entity shares the points of the fixed reference
      geometry, a circle has an Equal relation to it, so it follows and still makes
      profiles). `ext` (`oc_doc::ExternalRef`) is the assembly file, the referenced
      occurrence path, this part's occurrence path, the entity as a mate reference stores
      it (persistent name and signature in its own part's coordinates; JSON kept as
      written) and a label; `sig` is the geometry in this part's coordinates as last
      updated. `oc-graph` takes the plane / edge from `sig` and reads no body for them, so
      the part rebuilds the same with or without the assembly (`->?`). After every
      assembly build, parts flagged `external` in the reply are updated: each entity is
      re-found by name on its occurrence (engine job `asm_ref`,
      `Assembler::occurrence_entity`), placed relative to the part, and the part becomes an
      unsaved component edit if anything moved; another build follows (at most three
      rounds). FeatureManager: `->` on the sketch and the feature made from it (🔍) and on
      the component's row in the assembly, `->?` when the part is open without that
      assembly. Options > External References "Allow creation of references external to
      the model" is honoured. The frozen TypeScript engine takes an external sketch plane
      from its stored geometry (compatibility only; the browser cannot edit in context).
  - **Display (item 5, partly).** `oc_render::BodyItem::style` draws a body in its own
    display style. Component Display (component context menu: Wireframe, Hidden Lines
    Visible, Hidden Lines Removed, Shaded With Edges, Shaded, Default Display;
    `oc_doc::Instance::display`, `DisplayMode`) and Change Transparency
    (`Instance::transparent`, `asm.transparency`), both optional fields (version line
    unchanged) and one undo step each. Isolate's and Interference Detection's
    Non-interfering "Wireframe" are now a real wireframe. Interference Detection: Excluded
    Components (`InterferenceOptions::excluded`: left out as if absent; picks go to the box
    while it is active; Hide excluded components) and Component view (Results by
    component).
  - **Mate animation (owner request, added during the package; `src/assembly/animate.ts`).**
    A build that previews or adds a mate (`check`, `acceptMate`) and a SmartMates proposal
    start an animation: each instance that moved eases (smoothstep) from where it is drawn
    to its solved placement, the translation lerped and the rotation slerped the short way;
    an occurrence inside a sub-assembly keeps its place in its instance. Only the drawn
    placement animates: `AssemblySession.built` (and so the document, mate references and
    picks) has the solved placements at once, undo and redo build without animating, and a
    drag (`takePlacements`) stops any animation. `NativeView` sends the drawn placement and,
    while it differs, `pickPlacement`: `oc-app` converts a second scene with those
    placements (`SceneDto::pick_scene`) for picks and box selection, so picks find the
    components where they are solved. A newer solve starts from the pose being drawn.
    Options > Assemblies > Mate animation speed (`asm.mateAnimation`, Off 0 … Fast 10,
    default 5: 1 s at 1, 0.6 s at 5, 0.1 s at 10), all 🔍. (The option lives with the other
    System Options > Assemblies entries in `src/app/optionsSchema.ts`, which holds that page,
    rather than `settings.ts`.)
  - **Checks** (Windows 11, RTX 5070 Ti, D3D12). `npm run native:assembly`: **232 checks
    pass** (168 from G1a–G1c, 64 new in section 7 of the script, through DOM pointer
    events, double-clicks, context menus, the CommandManager, the confirmation corner,
    PropertyManager inputs, Enter and Ctrl+S): SmartMates (the pin follows the pointer to
    1e-3 mm, transparent and unpickable; Concentric proposed over the hole and previewed
    with the document untouched; off the hole it follows again; dropped: the Mate
    PropertyManager and pop-up; undo removes the mate, then the drag; dropped on nothing a
    drag along the mated axis, one undo step; Esc); Move Component's SmartMates button
    (double-click the head, hover the plate's top previewed, click, Coincident added); Edit
    Part in context (the view unmoved within 0.5 px on entry and return, the plate
    transparent at its place, a sketch on the plate's top as an external plane, the second
    hole's rim converted to a circle following it, `->` in the tree, a boss extruded, back
    with the corner, the boss in the assembly unsaved, `->` on the component, the plate's
    hole moved 5 mm and the reference followed, Save All writes the pin with its
    references, opened alone `->?` and it still rebuilds); Component Display Wireframe and
    undo, Change Transparency on and off, Isolate's Wireframe; Interference Detection
    (two interferences, Component view, Excluded Components picked in the graphics area,
    Non-interfering Wireframe, Hide excluded, Esc); mate animation (at the slowest speed
    frames show the pin between its start and its solved placement while picks use the
    solved one, it ends exactly there, the mate is one undo step, undo is instant, Off lands
    at once).
    - `cargo test --workspace`: **241 passed**, 7 ignored (static `OCCT_DIR`,
      `OC_RENDER_REQUIRE_GPU=1`); workspace clippy `-D warnings` clean; `npm run typecheck`
      clean; vitest **114/114**; `--corpus`: **`corpusDiffs: []`** (17 parts); `native:picking`
      ok.
    - Found while adding them: section 6 leaves the CommandManager on the Evaluate tab, so
      section 7 selects the Assembly tab before using its buttons.
  - **Parity with SOLIDWORKS** (✅ same, ≈ differs, ⬜ not yet; 🔍 marked in FEATURE_SPEC 6):
    - ✅ A redrawn, moved or resized cap keeps its mates; SmartMates by Alt-drag with the
      component following the pointer, the mate previewed over a target, the pop-up to
      confirm; Move Component's SmartMates; Edit Part / Edit Component with the assembly in
      view and transparent, sketching on another component's face and converting its
      edges, `->` / `->?`, references updated when the assembly changes; Component Display
      and Change Transparency; Interference Detection's Excluded Components and Component
      view; a real Wireframe for non-interfering and isolated components.
    - ≈ The SmartMates pointer shows the mate's name next to an icon (🔍 SOLIDWORKS shows
      an icon); cancelling the Mate PropertyManager after a SmartMates drop leaves the
      component where it was dropped (🔍). In context the FeatureManager is the part's own
      tree, not the assembly's tree with the part in blue; the transparency amount slider
      is not applied (45 %); the assembly must be saved before Edit Part (🔍 SOLIDWORKS
      asks). `->` also on the feature made from an in-context sketch (🔍). A cap of a
      feature with two caps of that role (two blind holes) is still a stand-in for the
      other (🔍, above).
    - ⬜ (G1e) configurations and named display states. ⬜ (later) SmartMates on insertion
      and peg-in-hole / multiple-mate SmartMates; animating Cancel back (🔍); in-context end conditions (Up To Surface
      of another component), List External References, Lock / Break references (`->*`,
      `->x`), multiple contexts, parts inside sub-assemblies in context; Remember excluded
      components; Solve as Flexible; Collision Detection / Physical Dynamics.
  - **For the owner.**
    1. The in-context reference design above (stored with the part, geometry in the
       part's coordinates updated by the assembly, the part rebuilding the same without
       it): agree, or should a part out of context refuse to rebuild its in-context
       features until the assembly is open (SOLIDWORKS keeps the last values, as here)?
    2. The cap exemption with two caps of a role (two blind holes from one sketch, one
       deleted): keep the owner's wording (the remaining cap stands in) or exempt only a
       feature that had one region (needs the region count stored with the name)?
    3. Edit Part requires a saved assembly. Keep, or allow it and write the references
       when the assembly is first saved?
    4. G1e (configurations) next, or G2 first?
  - **Owner decisions (2026-11-19, at merge).**
    1. In-context references out of context: keep the last values and rebuild the same, as
       built (SOLIDWORKS).
    2. Narrow the cap exemption to features that had a single region (store the region
       count with the name), so two blind holes from one sketch keep separate bottom faces
       and a deleted hole's bottom goes Missing. First item of G1e.
    3. Edit Part requires a saved assembly, as built (🔍).
    4. G1e next, then G2.

- 2026-11-20 **G1e done** (branch `claude/g1e-configurations-7e910a`, not merged). Both items of
  the brief: the single-region cap exemption, and configurations for parts and assemblies
  with named display states. Design tables and a few smaller items are left (below). No new
  dependencies.
  - **Naming: only caps of a single-region feature are exempt (G1d owner decision 2;
    `oc_naming`, section 3.3 step 2a).**
    - The region count is stored with the name: `oc_naming::Tag` carries the number of
      regions of the feature that made a cap (`Tag::cap`, `Tag::regions`), outside the tag's
      identity (equality, order, hashes and every key derived from names ignore it, so no
      fillet or shell key changes). A stored name writes it as a suffix on the cap tag,
      `f2/end/<key>#1` (`Tag::to_stored`, `Tag::from_stored`). Extrude, revolve, sweep and
      loft caps carry their region count (after Selected Contours); a rib's flanks carry its
      line count.
    - Compatibility as E6 handled names: a tag stored without a count (any document before
      G1e) keeps the G1d rule (a cap stands in wherever it moved). Part schema 3 and
      assembly schema 2 (below) carry the counts; the migrations rewrite nothing.
    - **Added rule (finding):** a stand-in ruled out by the 25 % rule is not taken back by
      step 3 either. A blind hole's bottom is planar, and step 3 accepts any coplanar face
      facing the same way: without this the deleted hole's bottom re-attached by position
      (with a warning) onto the other hole's bottom. Now it is **Missing**.
    - Tests: `only_the_cap_of_a_single_region_feature_stands_in_wherever_it_moved` (oc-graph,
      OCCT: the corpus plate with its two holes made blind; sketches on both bottoms and on
      the top; deleting the first circle makes the first sketch Missing, nothing healed, the
      second untouched; the same document without counts keeps the G1d rule; the boss
      outline redrawn with new entities and doubled keeps the sketch on its top, renamed with
      `#1`), `only_a_cap_of_a_single_region_feature_is_exempt` (oc-naming: `#1` exempt, `#2`
      missing when moved, no count exempt, `#2` in place still found, an edge through a moved
      `#2` cap missing), `a_cap_carries_its_region_count_outside_its_identity`. E6's TT1–TT7,
      the zero-`/new/` corpus check and determinism pass unchanged; G1c's and G1d's oc-asm
      naming tests pass unchanged.
    - **Expected values changed (own commit, `40eae99`):** the corpus assemblies pick their
      mate references fresh, so their stored cap names now end in `#1`
      (`bracket-pins.ocasm`, `hinge.ocasm` re-recorded; the solved states in `<name>.json`
      are unchanged). Re-recorded again in `5c49e18` for the assembly version line (1 → 2).
  - **Document (`oc_doc::config`, `src/core/configurations.ts`, `src/assembly/configurations.ts`).**
    - The features (an assembly's instances and mates) always hold the **active**
      configuration's values, so the regeneration, the UI and the frozen TypeScript engine
      read one plain document. `Configuration` stores only what is configured: `suppressed`
      (feature, or instance and mate, ids), `values` by `"<id>/<path>"` (a dotted path into
      that feature's or mate's JSON: `f3/end1.depth`, `f1/sketch.dimensions.d1.value`,
      `m2/value`), `sketches` (the solved points and circle radii of sketches with configured
      dimensions, as that configuration last had them), `components` (assemblies: the
      configuration each configured instance references) and `displayState`. Invariant: every
      configured key has an entry in every configuration; a value nobody configured is
      shared.
    - Activating: capture the active configuration's entries from the document, write the
      target's, re-solve the sketches whose dimensions were written. A stored solution that
      still satisfies its dimensions is kept as it is (the TypeScript solver nudged a
      satisfied sketch by 5e-10; a solve that moves nothing by more than 1e-9 is discarded),
      so going back gives the same document **bit for bit** and the regeneration is a cache
      hit. When a sketch dimension becomes configured, the configurations keeping the old
      value also keep the sketch's solution from before the change.
    - `PartDocument::configurations` (part schema **3**), `AssemblyDocument::configurations`
      and `display_states`, `Instance::config` (assembly schema **2**). Absent: one
      configuration, Default, and one display state. Migrations 2 → 3 and 1 → 2 rewrite
      nothing (the version moved because older readers refuse the new fields).
      `validate()` checks the active id, repeated ids and names, malformed keys.
    - Display states hold every instance's look (hidden, colour, display mode,
      transparency); the active one's looks live in the instances, so G1b–G1d's
      per-occurrence settings are now the active display state's. Each configuration links
      one display state (SOLIDWORKS' "Link Display States to Configurations", on).
  - **Engine.**
    - `oc_graph::config::activate` (the part as another configuration has it) and
      `configured` (for an assembly instance: the document itself when the configuration is
      active, else activated in Rust with `oc-sketch`). Same algorithm as the TypeScript one;
      the part window activates in TypeScript (synchronous, one undo step).
    - `Regenerator::pin(config)`: the keys a configuration's last rebuild used survive
      eviction (16 configurations kept); `engine_rebuild` takes the active configuration and
      pins after every rebuild without a preview. The rebuild reply reports `cachedSteps` and
      `evaluatedSteps`.
    - `oc_asm`: each (component, configuration) the instances reference is built once: a
      part hash, regeneration and mesh **per part and configuration**, shared by every
      instance using it; a sub-assembly instance applies its configuration
      (`apply_assembly`). A configuration the part does not have: the part's active one, with
      a warning on the component (🔍). `container()` caches the meshes of every occurrence's
      part, so the configurations in use at save open lightweight. `ComponentOut::configs`
      lists a component's configuration names.
  - **UI (native app; greyed in the browser and Electron builds).**
    - ConfigurationManager tab (`src/ui/ConfigurationManager.tsx`): the configurations, the
      active one checked and bold, description in brackets; double-click shows one; context
      menu Show Configuration, Add Configuration..., Properties..., Delete (not the active
      one, nor the last). Assemblies: the Display States list (activate, Add Display State,
      Properties, Delete; not the active one or one a configuration uses).
    - Add Configuration copies the active configuration and makes the new one active (in an
      assembly with its own display state). Activating is one undo step (🔍).
    - The Modify box shows This Configuration / All Configurations when the part has several
      configurations (🔍 default This Configuration, Options > FeatureManager). Suppress (tree,
      context toolbar, component menu) applies to This Configuration by default (🔍, same
      page); Edit > Suppress / Unsuppress > This Configuration / All Configurations;
      Specified Configurations and Edit > Design Table visible as todo.
    - Component Properties: Referenced configuration (a list of the component's
      configurations), and with several assembly configurations whether the dialog applies to
      This configuration or All configurations.
    - The part tree's header names the active configuration; the assembly's
      `Assem (Default<Display State-1>)` and each component `pin<1> (Long)`.
    - Switching assembly configurations turns lightweight off for the session: a
      lightweight part's mates use the geometry stored with the references, which the last
      resolved build of another configuration may have healed to that configuration's.
    - Interference Detection, Mass Properties and Measure read the last build, which is the
      active configuration's.
  - **Checks** (Windows 11, RTX 5070 Ti, D3D12).
    - `npm run native:configurations` (new, `crates/oc-app/scripts/configurations.js`):
      **30 checks pass**, through the ConfigurationManager tab, its context menus and dialog,
      the Modify box's configurations option, the tree's context toolbar and the Edit menu:
      Add Configuration (a name in use refused), a feature depth and a sketch diameter in
      This Configuration (the sketch re-solved), Suppress in Long only, double-click to
      Default and back (the document bit for bit, **0 features evaluated** both ways), undo
      and redo of activation, All Configurations, Edit > Unsuppress > All Configurations,
      Properties (rename), Delete (refused while active), undo, save (schema 3) and reopen.
    - `npm run native:assembly`: **244 checks pass** (229 from G1a–G1d, 15 new in section
      8): Referenced configuration (a part per configuration, 30 mm shank, one undo step,
      the row shows `(Long)`), Add Configuration with its own display state, Suppress and
      Hide Components in that configuration, Mass Properties and Interference Detection of
      the active configuration (2 occurrences and none; in Default 3 and the pins
      interfering), double-click back to Default (Display State-1's looks), a look changed in
      Default staying there, undo, save and reopen.
    - `cargo test --workspace`: **253 passed**, 7 ignored (static `OCCT_DIR`,
      `OC_RENDER_REQUIRE_GPU=1`); workspace clippy `-D warnings` clean; `npm run typecheck`
      clean; vitest **125/125**; `--corpus`: **`corpusDiffs: []`** (17 parts); `native:picking`
      21/21; `--smoke` 7 faces, 20 858.407.
    - New tests: oc-doc `version_2_documents_load_as_version_3_with_one_configuration`,
      `version_1_assemblies_load_as_version_2`, the every-field fixture with
      configurations (schema 3, read by both sides); oc-graph `tests/config.rs` (values,
      suppression and sketches written; back bit for bit; pinned cache hits with
      `cache_limit` 0 and misses without pinning; capture keeps an edit with its
      configuration; the file round trip); oc-asm `tests/config.rs` (an instance in Long gets
      its own part and mesh, cached in the container; an unknown configuration warns;
      assembly configurations suppress, set mate values, choose configurations and display
      states, and go back exactly); vitest `src/core/configurations.test.ts`,
      `src/assembly/configurations.test.ts`, migration 2 → 3.
    - Existing tests changed for the schema bump, not loosened: oc-doc's "newer documents
      are refused" uses `CURRENT_VERSION + 1`, the made-up version 0 chains every migration,
      the version 1 document saves at the current version; oc-asm's strict-loading test
      chains the assembly migrations; vitest's "keeps the names of a version 2 document"
      expects the current version.
  - **Parity with SOLIDWORKS** (✅ same, ≈ differs, ⬜ not yet; 🔍 marked in FEATURE_SPEC 1, 5, 6):
    - ✅ ConfigurationManager with the active configuration checked, double-click to show,
      Add / Properties / Delete (not the active one); per-configuration suppression and
      dimension values with This Configuration / All Configurations; unconfigured values
      shared; Referenced configuration per component instance; assembly configurations
      suppressing components and mates; display states linked to configurations holding
      hide/show, colour, display mode and transparency; Interference Detection and Mass
      Properties of the active configuration; a deleted hole's bottom goes Missing.
    - ≈ Add Configuration and Configuration Properties are dialogs, not PropertyManagers
      (🔍). Activation is an undo step (🔍). Suppress and Modify default to This
      Configuration (🔍 options). An edit to a configured value in a PropertyManager stays
      with the active configuration (🔍). Component rows show `(Long)`, not
      `(Long<<Long>_Display State 1>)`. Open Part opens the part in its own active
      configuration, not the one the instance references. Display states are always linked.
      The `.ocasm` caches the configurations in use when saved, not every configuration.
    - ⬜ Design tables; Specified Configurations; mate values in a Modify box (the document
      and the engine configure `m2/value`); configuration-specific custom properties;
      derived configurations; "Suppress new features and mates" in other configurations;
      display states for parts; the DisplayManager; Flexible sub-assemblies.
  - **For the owner.**
    1. The region count is stored as a `#<n>` suffix inside cap tags (one change that reaches
       every stored name: faces, edges, vertices, mates, in-context references) rather than
       a field beside each name. Agree?
    2. The added step-3 rule (a stand-in ruled out for moving is not re-attached by
       position) goes slightly beyond decision 2's wording; without it a deleted blind hole's
       bottom re-attached, with a warning, onto the other hole's coplanar bottom. Keep?
    3. Activating a configuration is an undo step here. SOLIDWORKS (🔍) may treat it as a
       view change outside Undo. Keep, or take it out of Undo?
    4. Defaults: Suppress and the Modify box apply to This Configuration (🔍); PropertyManager
       edits of a configured value stay with the active configuration. Agree?
    5. Display states are always linked to configurations (one per configuration, the
       SOLIDWORKS default). Unlinked display states and the DisplayManager: later, or G2 first?
    6. Next: design tables / Specified Configurations as a G1f, or G2 (GPU-driven rendering)?
  - **Owner decisions (2026-11-20, at merge).**
    1. The `#<n>` region suffix inside cap tags: agreed, as built.
    2. Keep the step-3 rule (a stand-in ruled out for moving is not re-attached by position).
    3. Activating a configuration is not an undo step (as SOLIDWORKS 🔍). Done at merge:
       `App.activateConfiguration` and `AssemblySession.activateConfiguration` no longer
       checkpoint; Undo and Redo show the restored snapshot in the configuration active now
       (`keepActiveConfiguration` / `keepActive`) and the document is marked modified.
       `configurations.js` and `assembly.js` check it.
    4. Suppress and the Modify box default to This Configuration; PropertyManager edits of a
       configured value stay with the active configuration: agreed, as built.
    5. Unlinked display states and the DisplayManager: later.
    6. G2 next. Design tables and Specified Configurations later (a G1f when wanted).
- 2026-11-20 **G2 done** (branch `claude/cool-wright-9db917`, not merged). GPU-driven rendering
  in `oc-render`: culling in compute passes, indirect draws, two-phase Hi-Z occlusion
  culling and per-instance level-of-detail selection, plus the `oc-app` wiring. No new
  dependencies.
  - **Shape.**
    - **Geometry pool** (`pool.rs`): every mesh level lives in five shared buffers (20-byte
      vertices, indices widened to 32 bits, 32-byte edge segments, segment distances, model
      vertices as `vec4`), so one draw call can cover every mesh. Removed meshes leave holes;
      the pool is compacted from the CPU copies when holes pass 16 MB and half of it.
      Buffers grow by doubling (copied on the GPU) up to the adapter's limits, which the
      renderer now requests (`Renderer::limits`).
    - **GPU scene** (`gpu_scene.rs`, `instances.rs`): every body is an `ObjectU` (160 bytes:
      placement relative to the camera target, colours, flags, highlight state ranges,
      scene mesh, draw-kind bits, LOD mode) in one storage buffer, indexed by its place in
      `Scene::bodies`. Part bodies are grouped by mesh into *draw slots*, one per (mesh,
      level). Nine draw kinds: prepass, opaque faces, transparent faces, edges, hidden edges
      (HLV), section parity, and the three pick kinds. Highlight states exist only for
      bodies that have a highlight. Each buffer is written only when its bytes change
      (orbiting rewrites nothing; panning rewrites the objects).
    - **Culling** (`cull.rs`, `shaders/cull.wgsl`, `compact.wgsl`, `hiz.wgsl`): one
      invocation per instance tests the placed box (8 corners through the instance's
      rotation) against the frustum with a margin for wide lines, the section plane (a box
      wholly on the cut side) and the Hi-Z, picks the level, and writes a code (kind bits,
      level). A compaction pass (one workgroup per draw slot) turns the codes into lists
      **in scene order** and writes the instance counts into the indirect draws, so
      coplanar faces of different instances resolve the same way every frame and the order
      does not depend on GPU timing. Each pass then draws every slot of a kind with one
      `multi_draw_indexed_indirect` / `multi_draw_indirect`.
    - **Occlusion** is the usual two-phase scheme: the instances visible last frame that are
      large on screen (at least 1/40 of the view's smaller side across) are drawn into the
      depth buffer first (depth only, same bias as the faces), the pyramid is built from it
      (half resolution, minimum over pixels and MSAA samples; reduction reads 3 × 3 texels
      because mip sizes round down), then every instance is tested; the ones in the prepass
      are always drawn, the others only if some part of their box may be in front. Only
      boxes wholly behind geometry already in the depth buffer are left out, so **culling
      never changes the image**: the tests compare culled and unculled images pixel for
      pixel. Hidden edges (HLV) and the section cap's stencil parity are culled by the
      frustum only (they show what is behind); transparent faces and edges of hidden
      instances are culled. Picks cull by the pick window's frustum and read the same lists.
      From a cold start the first frame culls nothing, the second learns what is hidden,
      the third culls (`culling_follows_the_section_and_the_view`).
    - **Level of detail**: `LodChoice::Auto` is now chosen per instance on the GPU, from
      millimetres per pixel at the instance's nearest distance in perspective (the CPU rule
      for bodies drawn one by one is the same formula in `f64`). Picks draw the same level.
    - Previews and interference volumes stay one draw per body (through the identity range
      of the instance list). `Renderer::set_culling(Culling { gpu, frustum, occlusion })`
      turns parts off; without `INDIRECT_FIRST_INSTANCE` the renderer draws one body per
      draw call (still without per-body bind groups, which were the old cost).
      `Renderer::draw_stats` reads the counts back (tests, benchmark).
    - **Mesh handles kept across builds:** `Renderer::add_mesh` shares an upload by the
      mesh's `Arc` (reference counted; `remove_mesh` releases one holder). An assembly
      rebuild hands out new handles for unchanged parts with the same `Arc`s
      (`oc-asm`'s `a_rebuild_keeps_the_meshes_of_unchanged_parts`), so nothing is uploaded
      again. Reusing the handles themselves in `asm.rs` was not done: a frame whose `seen` is
      past a reused handle and whose `retain` list misses it would drop the mesh while a
      build reply naming it is in flight; sharing in the renderer has no such race.
    - **App** (`oc-app`): the viewport device requests `Renderer::features` /
      `Renderer::limits`. The webview sends `bodies: null` when a frame's bodies equal the
      previous frame's (exact comparison of every field the renderer reads; a frame replaced
      in the queue passes its bodies on) and the render thread keeps the previous list
      (`SceneDto::keep_bodies`). `src/native/view.ts` looks instances up in a `Map` (it was
      a linear search per occurrence). Box selection skips bodies whose screen box misses
      the selection box (`box_edges`).
  - **Tests first** (the protocol's order, with one deviation: the benchmark was written and
    run on `main` first for the baseline, and the correctness tests were written once the
    API they need, `Culling` and `draw_stats`, was fixed, before the implementation was
    debugged against them):
    - `oc-render/tests/culling.rs` (5): `culling_never_changes_the_image` (a wall covering
      the view over a grid of 72 blocks, five blocks in front of it, five far away; every
      display style with one draw per body, GPU lists without culling, frustum, frustum and
      occlusion; identical images; counts: grid culled, far ones culled, hidden edges and
      Wireframe edges not occlusion-culled, Shaded draws only highlighted edges);
      `culling_follows_the_section_and_the_view` (a section that cuts the wall away shows
      the grid in the same frame; six turns of the view equal unculled images);
      `picks_go_through_the_gpu_lists`; `automatic_detail_coarsens_far_instances`;
      `the_same_mesh_is_uploaded_once`.
    - `oc-render/tests/scale.rs` `scale_10k_benchmark` (ignored; `--release -- --ignored
      --nocapture`; `OC_BENCH_QUICK=1`, `OC_BENCH_ADAPTER=<name>`).
    - `oc-asm` `a_rebuild_keeps_the_meshes_of_unchanged_parts`; `oc-app`
      `assembly_frames_convert` and the ignored `assembly_frame_cost`.
    - The F1/G1 renderer smoke tests (`tests/smoke.rs`: every display style, highlights,
      planes, section cap, 18 GPU picks, instances, per-body style and pickability, box
      selection, View Selector, line weight) pass unchanged on the GPU path.
  - **Measurements** (`scale_10k_benchmark`, release, RTX 5070 Ti, driver 32.0.16.2101 / Vulkan
    621.01, Ryzen 7 7800X3D). 100 distinct OCCT parts (blocks with 1 to 4 holes: 86 188
    triangles and 43 205 edge segments at level 0, three levels each), 10 000 instances,
    each mesh 100 times, turned about Z by their index: **8.6 M triangles and 4.3 M edge
    segments** without culling. Layouts: *cube* 25 × 20 × 20 at 45 mm (many seen through the
    gaps), *dense* the same at 24 mm (components touch and interpenetrate), *sheet* 100 × 100
    seen at an angle (nothing hidden). Isometric fit, the camera turned 1.5° per frame, 4×
    MSAA, hover moving each frame. Frame time is submit to idle over 60 frames, CPU encoding
    included (1000 / ms is the frame rate).

    Before G2 (`main` at bfc1aca, D3D12): **about 345 ms per frame (3 fps) in every layout,
    size and style**, CPU bound (`render()` 340–347 ms: a bind group, an object uniform and
    full face and edge state buffers per body per frame); a synchronous pick **487 ms**.

    After, D3D12, ms per frame (ShadedEdges / Hidden Lines Visible):

    | layout, size | one draw per body | GPU lists | + frustum | + occlusion (default) | + auto LOD 0.5 px |
    |---|---|---|---|---|---|
    | cube 1920 × 1080 | 7.98 / 11.37 | 3.65 / 5.71 | 3.67 / 5.73 | 3.60 / 5.81 | 1.67 / 1.83 |
    | dense 1920 × 1080 | 8.10 / 11.26 | 3.86 / 6.08 | 3.86 / 6.10 | **3.19 / 5.43** | 1.71 / 1.74 |
    | sheet 1920 × 1080 | 7.92 / 11.36 | 3.68 / 6.23 | 3.69 / 6.23 | 3.76 / 6.33 | 1.67 / 1.73 |
    | cube 2560 × 1440 | 8.12 / 11.58 | 3.77 / 5.99 | 3.78 / 5.99 | 3.88 / 6.11 | 1.71 / 1.98 |
    | dense 2560 × 1440 | 8.03 / 11.47 | 4.14 / 6.36 | 4.14 / 6.36 | **3.46 / 5.68** | 1.73 / 1.94 |
    | sheet 2560 × 1440 | 8.01 / 11.34 | 3.62 / 6.05 | 3.63 / 6.07 | 3.73 / 6.18 | 1.74 / 1.80 |

    Vulkan is within ±0.3 ms of these with the GPU lists (dense with occlusion 3.02 / 5.25
    at 1080 p) and faster one draw per body (5.6–6.0 / 8.3–8.4 ms). With the default culling
    `render()` costs 1.6–2.0 ms of CPU, a synchronous pick 1.5–1.6 ms (was 487), a crossing
    box selection over a tenth of the view 12–42 ms. Occlusion drew 4 877 of 10 000 in
    *dense* (4 625 in the prepass), every instance in *cube* (62 occluders) and *sheet* (none):
    whole-assembly views of open layouts have little hidden and no large occluders, so it
    costs 0.1 ms there. Every instance in view, with no culling at all, takes 3.6–6.4 ms:
    **the pass criterion (≥ 60 fps, 16.7 ms) holds with 2.6× to 4.6× headroom on this GPU**.
    Automatic LOD at 0.5 px halves that again (every instance at level 2 at this zoom).
    WARP (Microsoft Basic Render Driver, software, `OC_RENDER_FALLBACK=1`): 1.5 to 4.6 s per frame at 1920 × 1080 with the default culling (a CPU rasterising 8.6 M triangles: a floor, not a GPU), `render()` 48–93 ms; dense drew 4 877 of 10 000 there too.
  - **The app path** (not the GPU): per frame the webview builds the 10 000 bodies (1.2 ms
    in V8) and serialised them (4.9 ms, 1.4 MB of JSON); the render thread parsed them
    (3.9 ms) and converted them (0.9 ms). Since this package an unchanged body list is left
    out (`bodies: null`): turning the view of a 10 000-component assembly costs the webview
    about 1.5 ms (build and compare) and the render thread 0.9 ms plus `render()`. A change
    of the bodies (a hover does not change them; a selection colour or a drag does) still
    sends them all. Not measured end to end in the app: no 10 000-component assembly
    exists to open (building one needs a script that writes the `.ocasm`; worth adding to
    the corpus with G3).
  - **Checks:** `cargo test --workspace` with `OC_RENDER_REQUIRE_GPU=1`: 260 passed, 9 ignored (the timing tests among them); workspace
    clippy `-D warnings` clean; `npm run typecheck` clean; vitest 125/125. Native scripts on
    the release build (D3D12): `--smoke` 7 faces, volume 20 858.407; `--corpus`
    **`corpusDiffs: []`**; `picking.js` every check true (hover median 5.6 ms);
    `assembly.js` and `configurations.js` `ok: true`, no false check. The renderer tests pass
    on D3D12 and Vulkan, debug and release.
  - **Deviations and findings.**
    - **D3D12 miscompiled `vec4<bool> & vec4<bool>` in WGSL with optimisation on**: the
      frustum test culled everything in release builds (shaders are compiled without
      optimisation when wgpu's debug flags are on, so debug passed), on D3D12 only. The
      outcodes are integer masks now (comment in `cull.wgsl`). Worth a minimal repro for
      wgpu/naga's HLSL backend; not filed (no external posts without the owner).
    - **Multi-draw-indirect-count is not used.** Section 7 listed it as unverified. In
      wgpu 30.0.1 on D3D12, `draw_indexed_indirect_count` uses the command signature
      without root constants (`wgpu-hal` `dx12/command.rs`), so `first_instance` would not
      reach `instance_index`; plain `multi_draw_*_indirect` does (with wgpu-core's indirect
      validation on, the default). Draws with no instances cost a few GPU cycles each; with
      one slot per (mesh, level) that is fine at thousands of slots.
      `INDIRECT_FIRST_INSTANCE` is reported by the RTX 5070 Ti on D3D12 and Vulkan and by
      WARP.
    - **No mid-range GPU here.** The only working GPU is the RTX 5070 Ti (upper mid-range in
      its generation, not a mid-range card in the plan's sense); the Ryzen's integrated
      Radeon shows "Error" in Device Manager and is not enumerated. With no culling the
      frame is vertex-bound (8.6 M triangles, 4.3 M edge quads); a card with a third of the
      5070 Ti's throughput would be expected near 11–19 ms with the finest level and 5–6 ms
      with automatic LOD. That is an estimate, not a measurement: question 1.
    - Occlusion culling with every visible instance as an occluder (the textbook two-phase
      scheme) cost more than it saved in the open layouts (cube 3.64 → 4.64 ms, sheet 3.66 →
      4.81 ms; dense 3.85 → 3.10): the prepass redraws everything visible. Occluders are now
      the instances at least 1/40 of the view across (dense keeps its gain, the others cost
      0.1 ms). A one-pass variant (prepass instances shaded in a first colour pass, the
      newly visible ones in a second, no redraw) would remove most of the remaining cost
      but stores and reloads the MSAA targets mid-frame and makes coplanar faces of
      instances in different passes resolve by visibility history. Not done.
    - Hi-Z culling is conservative near occluder silhouettes (a 3 × 3 texel footprint at the
      level where a texel is at least half the box): in the culling test's open layout the
      third frame still drew 23 of the 72 blocks behind a wall covering two thirds of the view
      (those near its edges).
    - The new benchmark measures the renderer, not the app (above); P4's spike measured
      13.9 M triangles at 3.1 ms with one `draw_indexed` per mesh and no per-instance data
      or highlights.
    - Picks and frames share the GPU scene; a pick of a scene drawn elsewhere (mate
      animation's `pick_scene`) rewrites the objects for the pick and the next frame
      rewrites them back.
    - `MeshId` derives `Default` (oc-app's tests need one without a renderer).
  - **For the owner.**
    1. The pass criterion asks for a mid-range GPU; only the RTX 5070 Ti was available.
       Run `cargo test -p oc-render --release --test scale -- --ignored --nocapture` on a
       mid-range card (RTX 4060 / RX 7600 class) and record it here, or accept the 5070 Ti
       numbers with the headroom above?
    2. Level of detail in the app: the renderer chooses levels per component, but the app
       draws the finest. SOLIDWORKS' Options > Performance > Level of detail (stored here,
       not active) uses coarser levels while the view moves. Wire it that way (a follow-up:
       a `moving` flag in frames and a full-detail frame when the motion stops), or keep the
       finest always?
    3. Occlusion culling on by default with the 1/40 occluder size: agree?
    4. Should the 10 000-component benchmark also exist as an assembly file the native app
       opens (a `--script` measuring frame times end to end)? It needs a generator for the
       `.ocasm`; with G3 or now?
  - **Owner decisions (2026-11-20, at merge).**
    1. The RTX 5070 Ti measurements are accepted (2.6× to 4.6× headroom over 60 fps with
       no culling). The benchmark stays in the repository for a mid-range card later.
    2. Coarser levels while the view moves, as SOLIDWORKS does: a small follow-up, G2b
       (section 6).
    3. Occlusion culling on by default with the 1/40 occluder size: agreed, as built.
    4. The 10 000-component assembly file and an end-to-end `--script` come with G3.
    - The D3D12 `vec4<bool>` miscompile is not reported upstream for now (no external posts
      without the owner).
- 2026-11-20 **G2b done** (branch `claude/g2b-moving-lod-950106`, not merged). Coarser levels
  of detail while the view moves, full detail when it stops, as SOLIDWORKS' Options >
  Performance > Level of detail. No new dependencies; no renderer change.
  - **What SOLIDWORKS does** (help pages read 2026-11-20, 2022 edition): the slider runs
    from Off, then More (slower) to Less (faster), and sets the detail during dynamic view
    operations (zoom, pan, rotate) in assemblies, multibody parts and draft-quality drawing
    views; it is unavailable in Large Assembly Mode. The API (`swLevelOfDetail`) is an
    integer from 0 to 20, described as a target frame rate for spinning, from 0 (off) to 20
    (the most detail removed): SOLIDWORKS drops detail until that rate is reached. The help
    also says Performance changes do not affect documents already open. The factory default
    position is not documented.
  - **Shape.**
    - Webview (`src/native/motion.ts`, `view.ts`, `viewport.ts`): `ViewMotion` decides per
      frame whether the view is moving. A navigation drag that has moved (middle button,
      or the Rotate/Pan/Zoom/Roll view tools) and a camera transition (standard views, the
      View Selector, Zoom to Fit, Normal To) are moving until they end, and the frame after
      them is at rest at once: the release, or the transition's last frame. A camera
      change with no end event (a wheel step, arrow keys, Zoom to Area) is moving for
      200 ms, and `NativeView` draws one more frame when that has passed. A resize of the
      graphics area is not a view operation. Frames carry `moving` and `lod` (the slider,
      0 to 20; 0 when the option does not apply: a single-body part, known from `solids`,
      now sent with each mesh by `bodymesh.rs`).
    - `oc-app`: `SceneDto::to` gives part bodies `moving_lod(lod)` while moving, else
      `Finest`; previews and interference volumes stay `Finest`. `moving_lod`: Off is
      `Finest`; 1 to 20 is `Auto` with 0.5 to 16 px of chordal deviation, evenly on a log
      scale (×1.2 a step; 10 is 2.6 px). `view_stats` (render thread, `FrameStats`) reports
      what the last frame drew: its number, `moving`, `lod` and the part instances per level;
      the first call turns counting on (one GPU read-back per frame), for scripts only.
    - Picks: the render thread answers a pick with the scene of the frame it draws, whose
      bodies carry that frame's `LodChoice`, and the GPU pick runs the same level rule with
      the same camera, so picks and hover find what is on screen. No change was needed.
    - Options: the slider is 0 to 20 (was 0 to 10, inert), default 10 (🔍), labelled
      "Off  More (slower)" … "Less (faster)" like the View transition slider, and live.
  - **Tests first.** `oc-render/tests/lod.rs` (2): `picks_find_what_the_level_drawn_shows`
    (a ring zoomed onto its rim: the threshold selects the level it should; at 177 pixels
    where the coarsest level and level 0 differ by at least 3 px all round, a face pick of
    the coarse scene hits exactly where the coarse image shows the face, and of the fine
    scene where the fine image does, alternating scenes); `the_frame_after_the_motion_is_full_detail`
    (ShadedEdges, HLV, Wireframe: frames at level 2 while the camera turns, then at rest the
    image equals, pixel for pixel, that of a view that never moved). Both passed against
    G2's renderer as it was: they pin the behaviour G2b relies on (the renderer's
    write-on-change object buffer, slot tables and Hi-Z history across level switches). The
    first versions failed on the tests' own mistakes (the Top view of an XY disc is
    edge-on; HLV and Wireframe count no opaque faces per level; the coarsest level's chords
    were only 3.5 px from the circle at a whole-part zoom). `oc-app` (2):
    `the_level_of_detail_slider_sets_the_threshold`, `moving_frames_coarsen_the_parts`
    (did not compile before the implementation). Vitest `src/native/motion.test.ts` (7:
    drags, transitions, wheel settling, resize, scope, slider range and the Options schema).
    Native script `crates/oc-app/scripts/lod.js` (`npm run native:lod`), below.
  - **Measurements** (`native:lod`, release, D3D12, RTX 5070 Ti, graphics area 1429 × 922):
    the hinge at its isometric fit is drawn at level 0 at rest; during a middle-button turn
    every frame is moving with all three components at level 2 (their coarsest; the
    hinge's parts have three levels) at slider 10, and stays so while the button is held
    still for 600 ms; the frame after the release is level 0, drawn within the 40 ms the
    script waits after `pointerup`. A pick during the turn finds the pin. A wheel step draws
    coarse and the settle frame 200 ms later full detail; a transition to Front draws coarse
    frames on the way and level 0 at the end. Mean level of the three components at the
    end of a turn (two runs; the turn ends at slightly different angles):

    | zoom (from the fit) | slider 1 | slider 10 | slider 20 |
    |---|---|---|---|
    | 1× | 0.67 | 1.67–2 | 2 |
    | 4× | 0 | 0.67–1 | 2 |
    | 16× | 0 | 0 | 0 |

    Off draws level 0 while moving. A two-disc multibody part (two solids) filling the view
    stays at level 0 at slider 10 (its level 1 is about as far off as 2.6 px allows) and is
    drawn at a coarser level at 20; a single-body part is not coarsened at 20. Frame cost
    was not measured in the app (G2's renderer benchmark: automatic LOD at 0.5 px halves a
    10 000-instance frame, 3.6 → 1.7 ms; the app's end-to-end measurement comes with G3).
  - **Checks:** `cargo test --workspace` with `OC_RENDER_REQUIRE_GPU=1`: 264 passed, 9
    ignored; workspace clippy `-D warnings` clean; `npm run typecheck` clean; vitest 132/132.
    Native scripts on the release build (D3D12): `--smoke` 7 faces, volume 20 858.407;
    `--corpus` **`corpusDiffs: []`**; `picking.js` every check true (hover median 5.6 ms, as
    in G2); `assembly.js`, `configurations.js` and `lod.js` `ok: true`, no false check.
  - **Deviations and findings.**
    - SOLIDWORKS aims at a frame rate and drops detail until it is reached; this maps the
      slider to a fixed error, so a view always draws the same levels and tests can check
      them (🔍, question 2). The renderer draws 10 000 components at full detail in under
      7 ms (G2), so a frame-rate target would rarely coarsen anything on this GPU.
    - Scope as SOLIDWORKS' help: assemblies (and a part edited in context, shown in the
      assembly's window) and multibody parts; a single-body part is never coarsened
      (question 1).
    - A drag held still stays coarse until it is released (SOLIDWORKS' dynamic view
      operation lasts until the button is released; 🔍, question 4). The 200 ms settle time
      for the wheel and arrow keys is a choice (🔍).
    - The option applies at once, also to open documents (SOLIDWORKS' help says Performance
      changes do not; 🔍). The browser build (three.js) ignores it.
    - Ray queries on the CPU (`raycast.rs`: Select Other's faces, the rotate-about point,
      Hole Wizard positions; box selection) stay on level 0, as G2 left them. They answer
      clicks and boxes, made on a view at rest; on a moving view they differ from the drawn
      level by at most the threshold in pixels.
    - At slider 10 a whole small assembly is drawn at its coarsest levels while turning:
      a component's levels are relative to its own size (`display_lod`: ×4 per level), so
      small components in a large view reach their coarsest early. With at most 2.6 px of
      deviation this is barely visible; the default is question 3.
    - One `native:lod` run timed out after 600 s without a log (cargo ran with `-q`, so
      whether it waited for the build lock or the app hung is not known); the reruns took
      under a minute.
    - Builds used the G2 worktree's target directory (`CARGO_TARGET_DIR`), the disk being
      nearly full.
  - **For the owner.**
    1. Level of detail on single-body parts: SOLIDWORKS' help names assemblies and multibody
       parts only (as built). Keep that, or coarsen every part?
    2. A fixed error per slider position (as built, deterministic), or SOLIDWORKS' frame
       rate target (adaptive: coarser only when frames are slow)?
    3. Default slider position 10 (2.6 px), or another? SOLIDWORKS' factory position is not
       documented; a SOLIDWORKS install would tell.
    4. A drag held still: stay coarse until release (as built), or full detail after
       200 ms without movement?
  - **Owner decisions (2026-11-20, at merge).**
    1. Level of detail applies to assemblies and multibody parts only, as SOLIDWORKS' help
       says: agreed, as built.
    2. A fixed error per slider position (deterministic, testable): agreed, as built (🔍
       against SOLIDWORKS' frame-rate target).
    3. Default slider position 10 (2.6 px): agreed (🔍 until checked on a SOLIDWORKS install).
    4. A drag held still stays coarse until release: agreed, as built.
- 2026-11-20 **G3 split; G3a done** (branch `claude/g3-parallel-regen-a6a37d`, not merged). G3 was
  too big for one session and is split into G3a (this), G3b and G3c (section 6). Reasons for the
  split: the geometry-host processes (G3b) need the same jobs and the same body transfer as
  in-process workers, so building those first, on threads, gives G3b a tested protocol to move
  into processes; out-of-core caches and memory budgets (G3c) need a canonical B-rep blob, which
  this package found OCCT does not give (below); the end-to-end measurement belongs with the
  first slice so G3b and G3c are measured against it. No new dependencies (the `windows-sys`
  features `Win32_System_ProcessStatus`, `Win32_System_Threading` and
  `Win32_System_Diagnostics_ToolHelp` for memory and thread counts; already licensed).
  - **Kernel: native body transfer.** `Kernel::export_native` / `import_native` (and
    `OpKind::Transfer`): OCCT `BinTools` without triangulations, behind a header (`OCNB`, format
    1, the OCCT version, the payload length), so another format or version, a truncated blob or
    trailing bytes is an `Io` error, never a crash. The shape read back has the same entities in
    the same canonical order (body.cpp's geometric order, ties by traversal order, which
    `BinTools` keeps), the same geometry, mass properties and meshes, and fillets and booleans on
    it give the same results and history (`oc-kernel-occt/tests/transfer.rs`, 4 tests, written
    before the implementation).
    - **Finding: the bytes are not canonical.** A negative zero reads back as zero, and OCCT
      writes shared locations in an order that follows memory addresses, so equal bodies built
      separately (and a body after a second round trip) can export different bytes. The trait
      says so; bodies are compared by topology, never by blob. A B-rep cache in the `.ocasm`
      would break G1a's byte-identical saves unless the blob is made canonical (G3c: e.g. write
      the shape with its locations applied, or hash the topology instead of the bytes).
    - **Kernels on several threads at once agree** with one kernel alone (eight threads, three
      rounds, every operation family, meshing included). The one process-global state the shim
      touches, STEP translation's `Interface_Static`, is now taken in turns (a mutex around
      `read_step` and `write_step`): an imported part regenerating on a worker reads STEP.
  - **Parallel part regeneration (`oc_asm::parallel`).** `Assembler::build` first runs a
    planning pass: the same reading code (components, sub-assemblies, configurations) with
    `ensure_part` recording the parts to regenerate instead of regenerating them, and no mates.
    Two or more are regenerated at once by `parallel::run`: scoped threads (`part-worker-<n>`),
    one per logical processor (`Assembler::workers`, 0 = auto, 1 = none) and at most one per
    part, each with a clone of the kernel and its own `Regenerator` (same `Rules`), dropping its
    cache after every part. A job is plain data (hash, configured document, quality); a result
    too (the body blob and its `BodyNames`, the reference geometry, the feature errors, the
    `RenderMesh`), which is what a geometry host will exchange (G3b). The engine thread reads
    each body back and keeps it as if it had regenerated the part; a worker that panics, or a
    blob that does not read back, makes the part regenerate on the engine thread. Then the
    usual build runs and finds every part known. A single part to regenerate (the usual edit)
    stays on the engine thread, so its results stay in the regeneration cache part windows
    share (G1a). `Assembler::resolve_many` does the same for lightweight parts that Mass
    Properties and Interference Detection resolve. `Assembler::stats` (`BuildStats`: parts
    regenerated, workers, time, of which taking bodies over) goes into the build reply
    (`regenerated`, `workers`, `regenMs`).
    - Reading twice must not cost twice: component files are read once per build (`Memo`
      store), and parsed part documents and their configured versions with their part hashes are
      memoised by the bytes' content hash across builds. The instances' (component,
      configuration) list is a set, not a linear search (10 000 instances × 100 components).
      Together this made a rebuild with nothing changed faster than before G3a (19 → 13 ms
      for the 10 000-component assembly).
    - Tests (`oc-asm/tests/parallel.rs`, 5, written first, then mutation-checked: scrambling the
      transferred names fails 4 of them; the fifth compares only errors): serial and parallel
      builds give equal `Built`s, meshes byte for byte, bodies (topology), names, reference
      geometry and errors, three rounds; the corpus hinge and bracket-pins with mates solved on
      transferred bodies, and every face picked on them equal; a sub-assembly's parts join the
      top assembly's batch (8 parts on 8 workers in one batch); lightweight parts resolve in
      parallel to the same parts; failing parts report the same errors, without body or mesh.
      The existing assembly, configuration, evaluate and corpus tests now run on the parallel
      path (the default) and pass unchanged.
  - **Generated scale assembly (`oc_asm::scale`).** 100 distinct parts (blocks 30–48 × 20–38
    × 5–11 mm with one to four holes, as G2's benchmark) and 10 000 fixed instances (instance
    `i` uses part `i % 100`) on G2's 25 × 20 × 20 grid at 45 mm, turned about Z by `0.1 i`
    rad; no mates. `cargo run --release -p oc-asm --example scale -- <dir> [parts]
    [instances]` writes the `.ocpart` files and `scale-<n>.ocasm` without caches (2.7 MB with
    the saved copy). `tests/scale.rs`: the parts are distinct blocks with the right face counts,
    and the ignored `scale_10k_timings` report.
  - **End to end in the app: `npm run native:scale`** (`crates/oc-app/scripts/scale.js`) writes
    the assembly into `target/scale`, then opens it cold, rebuilds it (nothing changed, one part
    edited, ten parts edited, back), turns the view 60 steps at Level of detail 10 and Off,
    saves it with caches and opens the saved file lightweight; it reports times, what the engine
    regenerated on how many workers, the process' memory and thread count (`app_process_stats`,
    new) and the render thread's time per frame (`view_stats.renderMs`, new).
    `OC_PART_WORKERS=1` makes the engine regenerate on its own thread (same binary, for
    comparison).
  - **Measurements** (Windows 11, Ryzen 7 7800X3D with SMT off: 8 cores, 8 logical processors;
    RTX 5070 Ti, D3D12; graphics area 1429 × 922; release).
    - `oc-asm` alone (`cargo test --release -p oc-asm --test scale -- --ignored --nocapture`),
      100 parts, 10 000 instances, in memory:

      | | before (main) | G3a, 1 worker | G3a, 8 workers |
      |---|---|---|---|
      | cold open | 740 ms | 758 ms | 250–285 ms (of which taking 100 bodies over: 14 ms) |
      | rebuild, nothing changed | 19 ms | | 13 ms |
      | rebuild, one part edited | 28 ms | | 22 ms (on the engine thread) |
      | rebuild, ten parts edited | 77 ms | | 36 ms |
      | resolve 100 lightweight parts | 709 ms | | 234–242 ms |
      | lightweight open (read + build) | 93 + 19 ms | | 85–92 + 22–29 ms |

      Workers: 2 → 479 ms, 3 → 367, 4 → 302, 6 → 271, 8 → 285, 12 → 269, 16 → 231 ms. The
      serial path already runs OCCT's own threads in meshing and booleans, so eight workers give
      about 3×, not 8×; past four they share the cores with OCCT's pool. Peak working set of the
      cold open: +29 MB on 8 workers (`tests/memory.rs`, budget 96 MB), +36 MB serially.
    - In the app (`native:scale`), before (main), and after with 1 and 8 workers:

      | | before | after, 1 worker | after, 8 workers |
      |---|---|---|---|
      | open cold: engine build | 876 ms | 923 ms (regeneration 857) | **350 ms** (regeneration 246) |
      | open cold: until the reply is handled | 1620 ms | 1639 ms | 1578 ms |
      | open cold: until the first frame with every instance | 1696 ms | 1725 ms | 1658 ms |
      | `asm_open` (file read, document to the webview) | | 110 ms | 112 ms |
      | rebuild, nothing changed: engine / request alone / as the UI runs it | 95 / – / 561 ms | 77 / 191 / 533 ms | 92 / 186 / 542 ms |
      | rebuild, ten parts edited: engine | 164 ms | 139 ms | 113 ms |
      | lightweight open of the saved file: engine / handled / first frame | 103 / 747 / 818 ms | 77 / 695 / 772 ms | 82 / 683 / 759 ms |
      | save with caches | 773 ms | 637 ms | 643 ms |
      | turn, slider 10: pointer to drawn (median / p95), frame rate, render thread median | 16.6 / 18.2 ms, 57.5 fps, 4.6 ms | 16.7 / 18.0, 60.0, 4.4 | 16.7 / 17.6, 59.7, 4.3 |
      | turn, Off (finest level) | 16.8 / 21.5 ms, 57.8 fps, 7.7 ms | 16.7 / 18.1, 59.4, 7.6 | 16.7 / 20.4, 59.2, 7.6 |
      | working set after open / at the end (peak) | 244 / 267 (326) MB | 243 / 267 (326) | 244 / 267 (326) |
      | process threads before / after open | 36 / 53 | 36 / 53 | 36 / 54 |

      Frames during a turn are paced by the display (FIFO present, 60 Hz): the render thread
      needs 4.4 ms at slider 10 and 7.6 ms at the finest level for 10 000 components. Frames
      drawn at rest after `invalidate()` have a median of 7.4 ms on the render thread with some
      at 40–58 ms (p95), not investigated. Part workers are scoped: the thread count after the
      open is the same as serially.
    - **Finding: end to end, regeneration is no longer what an open or rebuild of this assembly
      waits for.** Of the cold open's 1.58 s the engine takes 0.35 s; `asm_open` 0.11 s; the
      rest, about 1.1 s, is the build request and reply over IPC (the document of 10 000
      instances goes to the engine and 10 000 instances and occurrences come back as JSON) and
      the webview handling the reply (the session, the FeatureManager with 10 000 components,
      the scene). A rebuild with nothing changed: engine 80–90 ms, the request alone 186 ms, as
      the UI runs it 540 ms, so about 350 ms is the webview's. Outside G3a (the shared UI and
      the IPC protocol); question 2.
  - **Deviations.**
    - The plan's architecture puts the kernel in geometry-host processes. G3a runs it on
      threads in the app's process; a crash in OCCT on a worker still takes the app down, as it
      did on the engine thread. G3b moves the same jobs into processes.
    - Files outside a single crate, as the package needs: `oc-kernel` (trait, `OpKind`),
      `oc-kernel-occt` (shim, FFI, mutex), `oc-graph` (`Regenerator::rules`), `oc-app`
      (`process.rs`, `view_stats.renderMs`, the build reply's stats, `OC_PART_WORKERS`),
      `package.json` (`native:scale`).
    - `oc-asm/src/build.rs` was 778 lines and is 864 (planning pass, memoised documents,
      statistics); the worker code itself is in `parallel.rs` (259). Splitting `build.rs`
      (reading components / mates / evaluation helpers) is a pure move for a later commit.
    - The memory budget test is coarse: these parts are small, and a worker keeping its
      regeneration cache adds under 1 MB here (mutation-checked: not caught). Budgets that
      catch it need large parts: G3c.
    - The worktree's `node_modules` is a junction to the main checkout's (same lockfile), to
      save disk; builds used `C:\Users\joshu\cad\target\check`.
  - **Checks:** `cargo test --workspace` with `OC_RENDER_REQUIRE_GPU=1`: 275 passed, 10 ignored;
    workspace clippy `-D warnings` clean; `npm run typecheck` clean; vitest 132/132. Native scripts
    on the release build (D3D12): `--smoke` 7 faces, volume 20 858.407; `--corpus`
    **`corpusDiffs: []`**; `picking.js` 21 checks true (hover median 5.7 ms); `assembly.js` (248
    checks), `configurations.js` (31), `lod.js` (23) and `scale.js` `ok: true`, no false check.
  - **For the owner.**
    1. Regenerating parts on threads in the app's process (G3a) before moving them into
       geometry-host processes (G3b): agree with the order, and with G3b next?
    2. The end-to-end open and rebuild of a 10 000-component assembly now wait mostly on the
       IPC and the webview (about 1.1 s of a 1.6 s open, 350 ms of a 540 ms rebuild). A
       follow-up for large assemblies: send only what changed between builds (placements,
       states) in a binary reply as part rebuilds do, and virtualise the assembly
       FeatureManager. Before G3b, after it, or as part of G3c?
    3. The native blob is not canonical (shared locations in memory order), so B-rep caches in
       the `.ocasm` would make saves differ byte for byte. Accept non-identical saves when B-rep
       caches arrive (G3c), or require a canonical blob (more work in the shim)?
    4. `Assembler::workers` defaults to one per logical processor. On this machine more workers
       than four gain little because OCCT's own pool shares the cores. Keep the default, or cap
       it (for example at half the processors) to leave the UI and renderer room?
  - **Owner decision (2026-11-20).** "Yes, finish G before proceeding to W and stuff; we'll approach
    the browser version questions at that point." Question 1: agreed, G3b next, then G3c, and all of
    phase G before phase W. Questions 2 to 4 are still open; until answered, G3b keeps the defaults
    as built (the IPC and webview cost recorded, not addressed; non-canonical blobs; one worker per
    logical processor).
- 2026-11-20 **G3b done** (same branch, `claude/g3-parallel-regen-a6a37d`, not merged; after the owner's
  decision above). The part workers of G3a are geometry-host processes in the app. New crate
  `crates/oc-host`; no new dependencies.
  - **Shape.**
    - `oc_asm::parallel::Runner`: where several parts regenerate at once (`width`, `run`, and
      `warm` / `idle` for hosts). `Threads` is G3a's; `Assembler::runner` replaces it when set.
      A runner returns `PartOut::failed(message)` for a part whose kernel stopped: the error
      stays on that part and it is **not** regenerated in the app's process (where the same
      crash would end the app); `Err` still means "regenerate it here" (a thread that panicked,
      a host that could not start, a document the host could not read).
    - `oc_host::proto`: binary frames (`OCHP`, kind, length) on the host's standard input and
      output. Job: id, Image Quality, the naming `Rules`, the part document (`oc_doc::save`).
      Done: the native body blob with its face names (stored tags), the reference geometry (JSON
      of `MateGeom`, sorted), feature errors, the `RenderMesh` blob, the host's time. Hello
      (protocol version, kernel name) when a host starts; Failed for a job that could not run.
    - `oc_host::serve`: the host loop, `oc_asm::parallel::work` on its own `OcctKernel`, a
      fresh regenerator per part, OCCT's console printers removed first
      (`oc_kernel_occt::quiet_messages`: its stdout carries the protocol). It returns at the end
      of its input, so hosts end when the app does. The app is the host:
      `opencad --geometry-host` (checked first in `main`, before any window), so the installer
      ships nothing new; tests use the crate's `oc-host` binary.
    - `oc_host::Pool`: one part per host at a time, `size` hosts (0: one per logical
      processor). Hosts start when an assembly whose parts are not all cached is opened
      (`asm_open` calls `warm`, overlapping the file read and the webview round trip) or at the
      first batch, and are kept; the engine thread, idle, checks every 30 s and stops hosts
      unused for 5 minutes (`keep`). A host that exits, crashes, breaks the protocol or does
      not answer within `timeout` (10 minutes) is killed and started again when its slot takes
      its next part; its part gets "The geometry kernel stopped while rebuilding this part. The
      part may contain geometry the kernel cannot build; the rest of the assembly is
      unaffected." or "The rebuild of this part did not finish within N s and was stopped."
      (both 🔍), shown as the component's warning in the assembly tree. Hosts are started with
      `CREATE_NO_WINDOW`. The build reply carries `hosts` (started, died, timed out, failed to
      start); `app_process_stats` reports the hosts' count and memory. `OC_GEOMETRY_HOSTS=0`
      keeps G3a's threads.
    - Unchanged: one changed part (the usual edit), part documents and every other engine job
      (previews, export, measure, evaluation's B-rep work) run on the engine thread in the
      app's process, so a kernel crash there still ends the app (question 1).
  - **Tests** (`oc-host/tests/host.rs`, 6, written with the pool before its first run; the first
    run failed only on one wrong assertion of the test, how many hosts had been restarted by
    then): hosts build what the app builds (`Built`, meshes byte for byte, bodies, names,
    geometry, errors) and are kept for the next batch; the corpus hinge and bracket-pins with
    mates; a host that crashes (access violation), aborts or exits fails only its part (no body,
    no mesh, the message), the other parts are unchanged, and the next batch runs on hosts again
    with each failed host started once more; a host that hangs is stopped after the timeout (3 s
    in the test); a missing host program makes the app regenerate the parts itself; idle hosts
    stop and start again with the same results. Mutation-checked: returning `Err` for a dead
    host (the in-process fallback) fails the crash test. The faults are a test hook, active only
    when the host's environment has `OC_HOST_FAULTS=1` (`fault:crash|abort|exit|hang` part names).
  - **Measurements** (same machine as G3a; release).
    - `oc-host` alone (`cargo test --release -p oc-host --test timings -- --ignored --nocapture`),
      the 10 000-component assembly cold, 100 parts on 8: threads 236–259 ms; hosts already
      running 244–274 ms; hosts started by the batch 301–326 ms (580 ms once, the first run after
      the build, with the executable not yet in the file cache). One small part per host first
      ("priming", `OC_HOST_PRIME=1`) did not help reliably (254 and 302 ms), so it is not done.
      Starting a host and answering one small part: 41–51 ms for eight at once (`oc-host.exe`
      and `opencad.exe` alike), 326–330 ms for the very first.
    - Transfer through pipes: two 432-hole plates (E4's large plate without fillets): threads
      1270 ms, hosts 1042 ms, with 29.5 MB of meshes and 1.0 MB of bodies through the pipes. P3's
      shared memory is therefore not used (deviation): plain pipes cost a few percent of the
      regeneration even for meshes of this size; revisit if meshes of hundreds of MB cross.
    - In the app (`native:scale`, two runs with hosts, one with `OC_GEOMETRY_HOSTS=0`):

      | | threads (G3a) | hosts (G3b) |
      |---|---|---|
      | open cold: `asm_open` / engine build (regeneration) | 117 / 349 (245) ms | 137–142 / 515–535 (417–436) ms |
      | open cold: reply handled / first frame with every instance | 1574 / 1669 ms | 1579–1598 / 1669–1690 ms |
      | rebuild, ten parts edited: engine (regeneration) | 100 (28) ms | 103–105 (26–28) ms |
      | rebuild, nothing changed: engine / request alone / as the UI runs it | 84 / 191 / 441 ms | 86 / 200 / 538–542 ms |
      | lightweight open of the saved file: engine / handled | 87 / 577 ms | 79–80 / 710–721 ms |
      | app working set after the open; host processes and their working sets | 244 MB; none | 238 MB; 8, 198–199 MB together |

      The hosts' first batch costs the engine about 170–190 ms more than threads (starting eight
      processes while the app reads the file and the webview works); end to end the first frame
      is the same, as the open waits mostly on the IPC and the webview (G3a's finding). Later
      batches cost the same as threads. The UI-side differences between runs (`as the UI runs
      it`, `handled`) vary by ±100 ms run to run and are not the hosts'. Eight idle hosts hold
      about 25 MB each until they are stopped after 5 idle minutes.
    - **E5 option (a), tool bodies built ahead of their booleans on another host: measured, not
      worth it now** (`oc-kernel-occt/tests/transfer.rs` `tools_against_booleans`). A 400 × 300
      plate: the 432-hole cutting prism takes 140–145 ms to build, the cut 443–448 ms, its
      transfer 17 ms; a small block 1.8 ms against a 4.8–5.1 ms cut; a swept circle 1.1–1.3 ms
      against 10.8–10.9 ms. Building a tool elsewhere only overlaps it with the previous
      feature's boolean, saving at most the tool's time minus the transfer: about a fifth of a
      cut-heavy feature, while needing speculative scheduling (a tool often depends on faces of
      the body before it). Revisit with Parasolid or heavy sweeps and lofts if profiles show
      tool building dominating.
  - **Deviations.**
    - Pipes instead of P3's shared memory (above).
    - The engine thread keeps its own kernel: only the batch regeneration of parts is isolated
      (the section 1 decision covers all kernel work; question 1).
    - `configurations.js` once ended with "no report within 600 s" in the full run (it opens no
      assembly, so no host was involved); it passed in the run before and twice on its own right
      after. G2b recorded the same unexplained watchdog once for `lod.js`.
    - Files outside `oc-host`: `oc-asm` (`Runner`, `PartOut::failed`, `Assembler::runner`),
      `oc-kernel-occt` (`quiet_messages`, the tool timing), `oc-app` (`--geometry-host`, the
      pool, idle stops, `hosts` in replies and process stats), `scale.js`.
  - **Checks:** `cargo test --workspace` with `OC_RENDER_REQUIRE_GPU=1`: 281 passed, 13 ignored;
    workspace clippy `-D warnings` clean; `npm run typecheck` and vitest not run again (no
    TypeScript changed in G3b; 132/132 in G3a). Native scripts on the release build (D3D12): `--smoke` 7 faces, volume 20 858.407;
    `--corpus` **`corpusDiffs: []`**; `picking.js` 21 checks true (hover median 5.8 ms);
    `assembly.js` (248), `configurations.js` (31, see above), `lod.js` (23) and `scale.js` (hosts
    twice, threads once) `ok: true`, no false check.
  - **For the owner.**
    1. All engine work in hosts (part documents, previews, export, measure, evaluation), so no
       kernel crash ends the app: a package of its own after G3c (it needs bodies kept in hosts
       and queried there, or moved back by the native transfer for every request), or not
       needed?
    2. Hosts kept 5 minutes after their last batch (about 200 MB for eight here), started when
       an assembly needing regeneration opens: agree, or keep them for the whole session, or
       start them with the app?
    3. The part messages for a crash and a hang (🔍 above): SOLIDWORKS' own wording for a part
       that failed to rebuild would be better if known.
  - **Owner decision (2026-11-20), G3a question 3:** "Accept differing saves." B-rep caches in the
    `.ocasm` (G3c) store the kernel's native blob as it is; two saves of the same assembly may then
    differ byte for byte in those entries. Tests compare such files by their content, not their
    bytes.
- 2026-11-20 **G3c done; G3 complete** (same branch, `claude/g3-parallel-regen-a6a37d`, not merged; after
  the owner's decision above). Three parts, in the order measured: the mate solver at scale (the
  evidence gate), B-rep caches, the body budget. No new dependencies (`windows-sys` features for
  `oc-host`'s memory report, already licensed).
  - **Mate solver per connected group (`oc-mate/src/groups.rs`).** Measured first
    (`oc-asm/tests/scale.rs` `scale_mated_timings`; `oc_asm::scale::mated`: every floating
    instance Lock-mated where it is, in chains of `chain` instances hanging off a fixed one):
    with the whole-problem solver a rebuild took 45 ms at 1 000 instances and 384 ms at 3 000,
    a drag of the last instance 316 ms and 2 984 ms (quadratic: the analysis scanned every row
    and column per group and per body, the drag's null space is dense over all columns). Now
    `oc_mate::solve` and `drag` split the problem into groups of floating bodies joined by
    enabled mates (fixed bodies and the assembly are constants any group may refer to; mates
    between constants form one more problem; disabled mates and mates naming a missing body are
    decided without solving) and solve, analyse and drag each on its own; `solve_together` /
    `drag_together` are the previous solver, kept for comparison. `oc-asm` also looked instances
    up linearly per mate side, per placement and per drag step; those are maps now.

    | Lock-mated instances, chain length | before: rebuild / drag | after: rebuild / drag (of which the solver) |
    |---|---|---|
    | 1 000, 1 | 45 / 316 ms | 9 / 8 ms |
    | 1 000, 100 | 70 / 372 ms | 31 / 30 ms |
    | 3 000, 1 | 384 / 2 984 ms | 19 / 14 ms (14 / 14) |
    | 10 000, 1 | (about 4 s / 30 s, extrapolated) | 63 / 48 ms (46 / 47) |
    | 10 000, 100 | | 295 / 265 ms (245 / 231) |

    One group of thousands of bodies is still dense (O(n²) per group); real assemblies are
    groups of tens. Tests (`oc-mate/tests/groups.rs`, 3, written before the code; the first run
    failed only on a wrong expectation of the test itself): a problem mixing a hinge, a conflict,
    a lock, a redundant mate between fixed bodies, a free body, a suppressed and two invalid
    mates and a chain sharing a fixed body solves like the whole problem in every rotation of
    the mate order (states, DOF, newest error, convergence; poses within 1e-7: the whole problem
    stops when its total residual is small enough, a group when its own is, 1.3e-9 mm apart was
    seen); drags of a hinged, a free, a chained and a fixed body likewise; 2 000 groups take at
    most 8× the time of 500.
    - **Golden change, own commit (6aab9f8):** `bracket-pins.ocasm` re-recorded. Its two pins are
      independent groups; solved per group, each pin's free spin about its axis ends 1e-9 apart
      from the whole-problem solve (quaternion component −4.58e-8 against −4.64e-8), so the
      stored placements' last digits changed. Positions, DOF, states and mates
      (`bracket-pins.json`) are unchanged; `hinge.ocasm` (one group) did not change.
  - **B-rep caches (`oc_asm::cache`, `oc_asm::outofcore`).** Saving an assembly writes, beside
    each part's mesh, `cache/brep/<hash>.ocb`: `OCBC`, a format number, a JSON head (the faces'
    persistent names in the kernel's face order, reference planes, axes and points, feature
    errors) and the native blob. Resolving a lightweight part (a pick, Mass Properties,
    Interference Detection, `resolve_many`, a build after a configuration switch) reads it
    (`import_native`) instead of regenerating; a part whose file is missing but whose entry is
    in the file resolves too (before, it could not be picked). An entry that does not read is
    dropped and the part regenerated. A resolved (non-lightweight) build of a freshly opened file
    still regenerates its parts (cache entries are trusted for lightweight work only: the part
    hash does not cover engine changes outside `CACHE_VERSION`).
    - Owner decision applied: the corpus test (`oc-asm/tests/corpus.rs`) compares document and
      meshes byte for byte and B-rep entries by what they hold (names, geometry, errors and the
      imported body's topology), and requires a B-rep per mesh. The corpus files were
      re-recorded with their B-rep entries in their own commit (6589287); documents, meshes and
      summaries unchanged.
    - Two G1a/G1c tests expected a pick or Interference Detection on a lightweight corpus part
      to regenerate it; they now expect it read (`regenerations` 0, `stats.read` 1 and 3). G3a's
      parallel-resolve test drops the B-rep entries first so it still tests regeneration on
      workers.
  - **Body budget.** `Assembler::body_budget` (bytes; `None` unlimited) bounds the bodies kept,
    estimated at `BODY_PER_BLOB_BYTE` = 7 times their native blobs (measured: 12 plates of 108 to
    207 holes, 10 MB of bodies for 1.4 MB of blobs; `oc-host/tests/timings.rs`). After every build,
    resolve and `keep_budget()` call the least recently used bodies beyond it are dropped (their
    B-rep entries made first) and their parts are lightweight again: mates use the stored
    geometry, picks and evaluation read them back, a resolved build reads them back. The app sets
    2 GB (`OC_BODY_BUDGET_MB`); build replies report `read` and `evicted`, Mass Properties replies
    `read` and `regenerated`.
    - Tests (`oc-asm/tests/outofcore.rs`, 5, written after the implementation and before its
      first run, mutation-checked: resolving without reading the entries fails all five): caches resolve 12
      parts with nothing regenerated and the same bodies, names, geometry, errors and meshes;
      picks and Interference Detection on the corpus assemblies read the parts and give the
      regenerated answers; a missing part file resolves from its entry; damaged entries are
      dropped and regenerated; a budget of three bodies keeps one to four live, picks on evicted
      parts read them back without regenerating, lightweight and resolved rebuilds give the same
      build. `tests/memory_budget.rs` (its own binary): twelve large plates under a budget of
      three, every part resolved six times over: the estimate never exceeds the budget, nothing is
      regenerated, the working set stays at 74 MB every round.
  - **Measurements** (same machine; release).
    - 10 000-component scale assembly, `oc-asm` alone: resolving the 100 lightweight parts 14–15 ms
      (read from the B-reps; G3a: 234 ms on 8 workers, 709 ms before G3); save 187 ms, 2.0 MB
      (1.9 MB without B-reps; a first version encoded each part once per occurrence, 748 ms, fixed);
      lightweight open 84 + 20 ms.
    - In the app (`native:scale`, hosts): Mass Properties of the reopened file (the engine's parts
      forgotten first: `engine_clear_cache`) **143 ms with all 100 parts read, none regenerated**;
      cold open engine 498 ms (391 regenerating on hosts), first frame 1 660 ms; lightweight open
      first frame 765 ms; save 651 ms (as before: the IPC); turn 60 fps, render thread 4.3 ms at
      slider 10 and 7.1 ms Off.
    - 24 plates of 216 holes (`oc-asm/tests/scale.rs` `body_budget_memory`): built in 2.9 s, working set
      185 MB; a budget of four bodies evicts 20, the working set falls only to 177 MB; reading the
      20 back takes 81 ms (regenerating them took most of the 2.9 s). **Finding: in these
      assemblies the display meshes, not the bodies, hold the memory** (the meshes of four levels
      of detail are several times the bodies), and the renderer holds every mesh it draws. The
      body budget bounds what B-rep work keeps; bounding meshes needs the renderer to drop CPU
      copies it keeps for compaction and stream levels (question 2).
  - **Deviations.**
    - The budget is applied after each operation, not during a cold open: a cold open still holds
      every regenerated body until the build ends (the peak), and is then trimmed. Evicting during
      the build would leave mates on stored geometry after a configuration switch, which is why
      those builds resolve parts.
    - Meshes are not evicted (above).
    - Files beyond `oc-asm`/`oc-mate`: `oc-app` (budget, replies, `scale.js` reopens and runs Mass
      Properties), `oc-host` (timing reports), the corpus files.
  - **Checks:** `cargo test --workspace` with `OC_RENDER_REQUIRE_GPU=1`: 290 passed, 16 ignored;
    workspace clippy `-D warnings` clean; no TypeScript changed (typecheck and vitest 132/132 as in
    G3a). Native scripts on the release build (D3D12): `--smoke` 7 faces, volume 20 858.407;
    `--corpus` **`corpusDiffs: []`**; `picking.js` 21 checks true (hover median 5.7 ms);
    `assembly.js` (248), `configurations.js` (31), `lod.js` (23) and `scale.js` `ok: true`, no
    false check.
  - **For the owner.**
    1. The app's body budget: 2 GB of estimated bodies (about 300 MB of native blobs). Keep, tie
       it to the machine's memory, or expose it in Options (SOLIDWORKS has no such option; its
       Large Assembly Mode threshold is the nearest)?
    2. Meshes hold most of a large assembly's memory and the renderer keeps them all. An
       out-of-core renderer package (drop CPU copies after upload, stream levels of detail by
       distance, evict meshes of components out of view) is the next step for very large
       assemblies: as a G-phase package before H, or later?
    3. A resolved build of a freshly opened file regenerates its parts rather than reading their
       B-reps (the content hash does not cover engine changes outside `CACHE_VERSION`). Bump
       `CACHE_VERSION` whenever modelling changes and trust the B-reps there too (faster resolved
       opens), or keep regenerating?
- 2026-11-20 **G3 merged into main** (G3a, G3b and G3c together, after review: every check run
  again on the branch and on the merge).
  - **Owner decisions** (the reviewer's recommendations, accepted: "Merge, then G3b" for G3a,
    "Merge, then G3c" for G3b).
    - G3a question 2: the delta binary build reply and a FeatureManager that draws only the
      rows in view become **G3d** (G3c was built before this was answered).
    - G3a question 3: superseded by the owner's "Accept differing saves" above.
    - G3a question 4: part workers and geometry hosts default to half the logical processors
      (4 workers took 302 ms against 285 ms for 8 here; the rest is left to the UI and the
      renderer). G3d.
    - G3b question 1: not now. Only the batch regeneration of parts runs in hosts; decide
      after W1 whether kernel crashes in the engine thread (one-part edits, previews, export)
      happen often enough to move the rest.
    - G3b question 2: hosts kept 5 minutes after their last batch, started when an assembly
      that needs regeneration opens, as built.
    - G3b question 3: the crash and hang messages stay as written, 🔍 (SOLIDWORKS has no
      equivalent: a kernel crash there ends the application).
    - From the G3b review, in G3d: hosts end with the app even when it is killed (a hung host
      outlived it), and the protocol moves off standard output.
  - **Package order (owner, 2026-11-20):** all of phase G (G3d included), then **W1 before
    phase H** ("proceed with W before H").
  - G3c questions 1 to 3 are open (the body budget; an out-of-core renderer for meshes;
    trusting B-rep caches in resolved opens).
  - **Owner decisions on G3c's questions (owner, 2026-11-23, accepting the reviewer's
    recommendations).**
    1. The body budget stays a fixed 2 GB of estimated bodies, with no Options entry.
    2. An out-of-core renderer for meshes comes later, after W1, not in phase G.
    3. Resolved builds of a freshly opened file trust the B-rep caches. The caches are keyed
       to the engine build automatically (the app version and the kernel's name and version
       in the manifest, checked on read) instead of relying on manual `CACHE_VERSION` bumps.
       G3d.
  - The review's three cold opens of the 10 000-component assembly with hosts took 537, 961 and
    1139 ms in the engine (threads: about 350): starting eight hosts varies; the first frame
    (1.67 to 1.98 s) mostly waits on the IPC and the webview, which G3d addresses.
  - Checks on the merge (main): `cargo test --workspace` and clippy, typecheck, vitest, and every
    native script passed. `scale.js` once exited with -1 after its complete `ok: true` report (the machine was under
    another session's CPU load); two reruns exited 0. Recorded with the intermittent script
    watchdog under investigation (`picking.js` once on the G3b branch).
- 2026-11-20 **Check-run "no report within 600 s": found and fixed** (branch
  `claude/competent-ramanujan-24a0e0`, merged 2026-11-23 after review; every check run again
  on the merge). The intermittent exit 2 of the native
  scripts (`lod.js` in G2b, `configurations.js` and `picking.js` in G3b).
  - **Cause: WebView2's browser process crashes 3 to 4 s after it starts, inside RivaTuner
    Statistics Server's `RTSSHooks64.dll`** (MSI Afterburner's on-screen display, running on
    this machine with its global profile, so injected into every process). Its window hook
    (`RTSSCBTProc`) faults when the browser process creates a COM window (network cost and
    capability-access activation): access violation, buckets `RTSSHooks64.dll!Unknown` and
    `dxgi.dll!Unloaded` (the same RTSS code writing where dxgi was). With the browser process
    gone the page and the script die; the window is transparent, so nothing shows it (the
    screenshot of a hung run has the viewport and no UI) and the app waited for its timer.
    WebView2 kept a crash dump of each: 2026-11-20 14:55 (the G3 `configurations.js` hang),
    15:17 (the main session's `picking.js` hang), 2026-11-17 01:35 (unnoticed), and two of
    mine (`lod.js`, 2 of 20 rounds of the final loop before the fix; `lod.js` opens an
    assembly, whose geometry hosts start in those same seconds). The G2b `lod.js` case left no
    log (an outer `timeout 600` ended it) and no dump in the shared profile, so it is not
    confirmed to be the same.
  - **Found with a stuck-state dump**, now part of every check run (ARCHITECTURE.md):
    "no answer within 15 s" from the page, the render thread idle with 36 frames drawn,
    nothing pending, led to the screenshot and the crash dumps. It reports the run's events,
    the commands the webview waits for, the engine's job, the render thread's phase and
    frames, and from the page the scenario's line, a heartbeat, pending invokes, app and
    viewport state and console errors since the page started; both screenshots are saved.
    `OC_SMOKE_TIMEOUT=<s>` shortens the wait.
  - **Fix (`webview_watch.rs`).** The app watches WebView2's `ProcessFailed`. The browser
    process exiting starts the app again with the same arguments, at most twice in a row,
    then exit code 4 with the reason; a check run hides its window, waits for the new run and
    exits with its code. A page's renderer exiting reloads the page. This also covers users:
    without it, RTSS users would now and then get a window with no UI at start (🔍 for the
    owner: SOLIDWORKS has nothing comparable). Tested by killing the browser process 3.5 s
    into a run: once and twice pass (16 and 22 s), three times gives up at once (exit 4); a
    killed renderer reloads and passes. webview2-com (already in the app through wry) is now a
    direct dependency (THIRD_PARTY.md).
  - **Also fixed on the way.** (1) Check runs shared the owner's WebView2 profile and stored
    options: `lod.js` left Level of detail at 20 for later runs and for the owner's own app.
    They now use their own profile (`checks` in the app's local data folder) and clear the
    options on their first load. (2) Under load the renderer's shaders compile for many
    seconds (`Renderer::new` 1.4 s idle, 5 to 20 s with 8 busy threads and other instances;
    one window took 9.8 s to be built) and `lod.js`'s first `view_stats` timed out ("timed
    out waiting on channel"): the script wrapper now waits until the viewport answers. A
    timed-out viewport query names the render thread's phase.
  - **Ruled out on the way:** a missed frame or IPC reply in the scripts (their waits are
    bounded while the page runs), `app_ready` before the window is known, the window being
    minimised, covered or the display asleep (WebView2 kept the page visible and its timers
    running), two runs at once, CPU and disk load (about 300 runs before the reproduction).
  - **Owner.** To avoid the crash itself, give `msedgewebview2.exe` (and `opencad.exe`) an
    RTSS profile with Application detection level None, or close RTSS while checks run; the
    app now recovers either way.
  - **Owner decision (2026-11-23, "Merge both").** Starting the app again when WebView2's
    browser process dies (at most twice in a row, counted for the session) is accepted as
    built, 🔍.
  - **Seen while looping (follow-ups, not this hang).** Under heavy load `assembly.js` once
    failed "pick: hover a face of the pin instance" (a hover pick past its 1.5 s). The G3
    merge's `scale.js` exit -1 after a complete report suggests an occasional crash while
    the app exits.
  - Checks: `cargo test -p oc-app` 14 passed, clippy `-p oc-app --all-targets -D warnings`
    clean, `npm run typecheck` clean; 20 rounds each of `picking.js`, `configurations.js` and `lod.js`
    on the final build: 60 of 60 passed, none near the timer; one of them (`configurations.js`,
    round 14) met the RTSS crash for real (dump 2026-11-20 20:39, `RTSSHooks64.dll`), started
    again and passed in 18 s. `smoke`, `corpus` (`corpusDiffs: []`) and `assembly.js` pass on
    the final build. Not run: the full workspace tests and clippy, `scale.js` (only `oc-app`
    changed).
- 2026-11-23 **G3d done; G3 complete** (branch `claude/g3d-large-asm-79680a`, merged 2026-11-23 after review; after the
  owner's decisions above). Six items; no new dependencies (`windows-sys` features for
  `oc-host`, already licensed).
  - **Smaller build replies** (`asm::encode_build`, `src/assembly/buildReply.ts`). Build replies
    are binary in the packed layout of part rebuilds (`pack.rs` gains 8-byte aligned `f64`
    arrays): a JSON header (components, parts, mates or their changes, DOF, heals, timings) and
    typed arrays of placements and `state | free << 8`. Each kept reply has a generation; the
    next request names the one the webview holds (`base`), and the engine, which records what
    it sent (`asm::Sent`), sends only the instances, occurrences and mates that differ, by
    index (a list whose ids changed is sent whole). Components and parts are sent whole: one
    per file, and every build hands out new mesh handles (the renderer does not upload the same
    `Arc` again, G2), so an occurrence's mesh is its part's handle. Drag and placing replies
    update the record (the webview writes their placements into the build it holds);
    placements the webview changes on its own (a SmartMates proposal, Insert Components'
    preview) make the next reply whole, and a difference against a build the webview no
    longer holds is refused (`StaleBase`) and asked for whole. Probes (SmartMates) are
    differences too, with generation 0 (never a base).
    - Also in the webview: absorbing a build into the document, drags, the mate animation,
      SmartMates and the tree looked instances up with a linear `find` per item (quadratic on
      10 000 instances); they use maps now (`AssemblySession.instanceState` / `mateStatus`
      are indexed per build).
    - Tests: `asm.rs` (4: a first reply is whole; against the build held only the changed
      entries go, 10 000 unchanged instances in under 4 KB; a list whose ids changed is
      whole; drags keep the record in step, another build drops it), `pack.rs` (`f64`
      alignment), `buildReply.test.ts` (4: whole, a difference sharing unchanged entries and
      leaving the build held as it was, a difference against another build refused, 10 000
      decoded quickly). `scale.js` checks that the build kept from differences equals a whole
      reply of the same document (instances, occurrences, parts, mates, DOF).
  - **Visible rows only** (`AssemblyTree.tsx`, `treeRows.ts`). The rows are laid out as data
    (map lookups: linear in the components; 10 000 in a few ms) and only those in view, plus
    12 either side, are rendered between two spacers; the list scrolls itself (also in the
    fly-out, at most 60 % of the window high). Expand and collapse, selection, rename (an
    `input` in the row) and context menus are unchanged; the assembly tree has no drag to
    reorder. A row selected elsewhere (the graphics area) or being renamed from a menu is
    scrolled into view, as Options > FeatureManager > Scroll selected item into view says
    (on by default, as in SOLIDWORKS; the part tree does not do it yet). Tests:
    `treeRows.test.ts` (6: the window, scrolling a row into view, the SOLIDWORKS layout,
    expanding and collapsing, 10 000 components laid out in linear time); `scale.js` checks
    that 10 007 rows draw fewer than 200, that scrolling to the end draws the Mates folder, and
    that a component selected in the graphics area is scrolled into view and highlighted.
  - **Worker count** (`oc_asm::parallel::default_workers`): half the logical processors, at
    least one, for threads and geometry hosts; `OC_PART_WORKERS` still overrides. Four here.
    Test: `parallel.rs` `default_workers_are_half_the_logical_processors`.
  - **Hosts end with the app** (`oc_host::win::Job`): every host is put in a job object with
    `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; when the app ends, killed or crashed included, the
    last handle closes and Windows ends the hosts. Test (`host.rs`
    `hosts_end_with_the_app_even_when_it_is_killed`, failed before the change): the test binary
    starts itself as an "app" with one host hanging in a part (`OC_HOST_FAULTS`), kills it,
    and the host ends within 10 s.
  - **Protocol off standard output** (`oc_host::win`, protocol 2): a host's standard output
    goes to `NUL`; the app makes an anonymous pipe per host after starting it, duplicates the
    write end into the host (`DuplicateHandle`) and names it in a first `Output` frame on the
    host's input, keeping only the read end. Nothing is inherited, so hosts started at once
    from several threads never hold each other's pipes (which would hide a host's death).
    Test (`host.rs` `a_host_that_prints_to_standard_output_still_answers`, failed before the
    change: the printing part came back broken): a part named `fault:print` makes the host
    write frame-like bytes and text through Rust's and the C runtime's standard output before
    building it; every part builds as the app builds it and no host restarts.
  - **Trusting the B-rep caches** (owner decision 3). The manifest names the engine build
    (`"engine": {"app": "0.8.0", "kernel": "OCCT 8.0.1"}`: `oc_asm::cache::APP_VERSION`, read
    from `crates/oc-app/Cargo.toml` by `oc-asm`'s build script so every assembler keys caches
    alike, and `Kernel::identity`, the kernel's name and version without its linkage, since a
    static and a shared OCCT 8.0.1 make the same geometry). `Assembler::set_cache` (and
    sub-assemblies' caches) ignore caches of another build or of files saved before caches
    named their engine: their parts regenerate and the next save writes this build's. A
    resolved build of a freshly opened file reads each part's B-rep cache instead of
    regenerating it (a part edited since, whose hash differs, regenerates). `CACHE_VERSION`
    stays for layout changes. Tests (`outofcore.rs`, 3, the first two written before the
    code): the engine is recorded; caches of another app version, another kernel version or
    none are ignored and regenerated, lightweight or resolved, and re-saved with this build's;
    resolved builds read the B-reps (12 scale parts read, none regenerated, the same bodies,
    names, geometry, errors and meshes; one edited part regenerated; the corpus assemblies with
    mates). `cargo test -p oc-app` checks `APP_VERSION` against the app's version.
    - **Expected value changed by the decision (same commit):** `corpus.rs` expected a resolved
      open of a stored corpus file to regenerate every part; it now expects none regenerated
      and every part read. It also requires the stored caches to be this engine build's, with
      the re-record command in its message.
    - **Golden change, own commit (c1737e4):** `hinge.ocasm` and `bracket-pins.ocasm`
      re-recorded: only `cache/manifest.json` gains the engine; documents, meshes, summaries
      unchanged, B-rep entries equal by content.
  - **Measurements** (`native:scale`, same machine as G3a–G3c: Ryzen 7 7800X3D, 8 logical
    processors, RTX 5070 Ti, D3D12; release; geometry hosts). Before: main 6d5075d (8 hosts);
    after: this branch (4 hosts); two runs (the second with the tree checks).

    | 10 000 components, 100 parts | before | after |
    |---|---|---|
    | cold open: reply handled / first frame with every instance | 1660 / 1755 ms | 626–694 / 690–763 ms |
    | cold open: `asm_open` / engine (regeneration) | 169 / 837 (621) ms | 122–133 / 392–431 (353–380) ms |
    | rebuild, nothing changed: as the UI runs it / request alone / engine | 574 / 220 / 114 ms | 90 / 79–82 / 20–22 ms |
    | rebuild, one part / ten parts edited: as the UI runs it | 567 / 606 ms | 93 / 117–119 ms |
    | build reply, nothing changed | JSON, about 3.5 MB | 38.8 KB (whole: 1.46 MB) |
    | lightweight open of the saved file: reply / first frame | 713 / 789 ms | 241–250 / 303–317 ms |
    | save | 656 ms | 323–325 ms |
    | turn at slider 10 / Off: frames per second | 60 / 59.9 | 88 / 60–61 |
    | geometry hosts after the open: count, working sets | 8, 197 MB | 4, 102–103 MB |

    Outside the file read and the engine, the cold open took 654 ms before and 112–130 ms
    after; an unchanged rebuild's handling in the webview (as the UI runs it, less the request
    alone) went from 354 ms to under 10 ms. Most of the old cost was the webview's handling
    (10 000 tree rows rendered, each finding its state by a linear search, and the build
    absorbed into the document the same way), the rest the JSON reply. The engine's reported
    time (`timeMs`) no longer includes building the reply (before, it built 20 000 JSON values;
    an unchanged rebuild's engine time fell from 102–114 to 20–22 ms, mostly that), so compare
    "as the UI runs it", not the engine column.
    What is left of an unchanged rebuild is mostly the request: the whole document goes to the
    engine as JSON (about 1.3 MB for 10 000 instances) and is parsed there (question 2).
    With four hosts the open's regeneration took 353–380 ms against 621 ms with eight in the
    one run before (G3a measured 302 against 285 ms with threads, and the G3 review 537–1139 ms
    engine time with eight hosts: starting them varies most); the hosts hold half the memory.
  - **Deviations.**
    - The reply pipe and the job object are Windows only; elsewhere hosts still answer on
      standard output and are not in a job (there is no macOS app yet).
    - The request is unchanged (the whole document per build); only the reply was in scope.
    - `scale.js` grew checks of the tree and of the differences (above); `smoke.rs`'s
      `versions_agree` checks `APP_VERSION`.
    - Files outside the package's list: `oc-kernel` (`Kernel::identity`), `oc-kernel-occt`,
      `src/native/engine.ts` (`unpack`, `f64` arrays), `src/assembly/smart.ts`, `styles.css`.
  - **Checks:** `cargo test --workspace` with `OC_RENDER_REQUIRE_GPU=1`: 301 passed, 17
    ignored; workspace clippy `-D warnings` clean; `npm run typecheck` clean; vitest 142/142
    (132 before; 10 new). Native scripts on the release build (D3D12): `--smoke` 7 faces, volume
    20 858.407; `--corpus` **`corpusDiffs: []`**; `picking.js` 21 checks true (hover median
    5.6 ms); `assembly.js` (246), `configurations.js` (32), `lod.js` (29) and `scale.js` (twice)
    `ok: true`, no false check. No script hit the RivaTuner watchdog this time.
  - **For the owner.**
    1. The assembly corpus now has to be re-recorded whenever the app's version (or OCCT's)
       changes: `UPDATE_GOLDEN=1 cargo test -p oc-asm --test corpus`, and `corpus.rs` fails with
       that command until it is done. Add the step to the release instructions in `CLAUDE.md`
       (not done here: release workflow), or keep only the kernel and a modelling version
       (bumped by hand when modelling changes) in the key?
    2. Development builds between releases share the app's version, so a modelling change
       without a version bump still trusts caches written before it. Acceptable (they are
       caches; deleting them or bumping `CACHE_VERSION` regenerates), or add the commit to the
       key in development builds?
    3. The request still sends the whole document as JSON every build (most of what is left of
       an unchanged rebuild, about 60 ms at 10 000 instances). Send document edits instead, as
       a package after W1 (the browser has the same request), or leave it?
  - **Owner decisions (2026-11-23, "Merge both", accepting the reviewer's recommendations).**
    1. Caches stay keyed to the app version; re-recording the assembly corpus is a release
       step in `CLAUDE.md`.
    2. Development builds trusting caches of the same version: acceptable (caches).
    3. Document edits instead of the whole document per request: decided after W1.
  - Review: every check run again on the branch (301 Rust tests, vitest 142, all native
    scripts; `scale.js` first frame 703 ms, unchanged rebuild 90 ms) and on the merge with the
    check-run hang fix. `assembly.js` reports 246 checks on main and on this branch alike.
- 2026-11-23 **W1 designed and split; W1a done** (branch `claude/strange-cannon-86dc85`, merged 2026-11-23 after review).
  - **Read first:** section 6 Phase W (W1 split and design), the decisions deferred "until W1"
    (parts as JSON `.ocpart`, G1a/G1b; the request-side document edits, G3d question 3; the
    out-of-core renderer, G3c question 2; kernel crashes in the engine, G3b question 1), the
    frozen TypeScript engine (`src/kernel/*`, replicad 1.2.1 with OCCT 7 in a 23 MB
    WebAssembly, the three.js viewport) and Tauri's IPC seam (`src/native/tauri.ts` `invoke`).
  - **Design** (section 6, "W1 design"): the engine crates, `cxx` and the C++ shim unchanged,
    compiled by Emscripten (`wasm32-unknown-emscripten`, stable Rust); the renderer a separate
    `wasm32-unknown-unknown` module (wgpu's WebGPU backend needs wasm-bindgen); no threads and
    no SharedArrayBuffer (Rust's standard library for the target has no atomics without
    nightly `-Zbuild-std`; parallelism from several workers instead, so CloudFront needs no
    COOP/COEP headers); the engine thread and geometry hosts become workers behind the same
    `invoke` commands; WebGPU with a fallback; a workspace file system mirrored into the
    engine's; headless Chromium over the DevTools protocol for the browser's oracle. W1 split
    into W1a–W1f (section 6).
  - **Toolchain** (outside the repository): emsdk 7.0.1 in `C:\occt\emsdk` (2.1 GB; the
    recommended release on 2026-11-23); `rustup target add wasm32-unknown-emscripten` (Rust
    1.99.1 stable). Emscripten 7 ships `emcc.exe`/`em++.exe` launchers, not the `.bat` files
    cc-rs and rustc look for on Windows, so `scripts/wasm.mjs` names compiler, archiver and
    linker (`em++`, `-sDEFAULT_TO_CXX` for libc++). Every link prints a `linker_messages`
    warning: rustc passes `-sWASM_BIGINT`, which Emscripten 7 deprecates (harmless).
  - **OCCT 8.0.1 for WebAssembly** (`crates/oc-kernel-occt/build-occt-wasm.cmd`): the native
    build's modules, static, `-O2 -fwasm-exceptions`, no pthreads. 5 527 build steps, 18.9 min
    (16.8 min for the rebuild below, both beside other builds on the Ryzen 7 7800X3D); install
    196 MB (48 archives, 148 MB), build tree 348 MB (`C:\occt\install-8.0.1-wasm`,
    `C:\occt\build-8.0.1-wasm`). Two fixes, no OCCT source change:
    - `-include mutex`: `NCollection_IncAllocator.cxx` uses `std::lock_guard` without
      `<mutex>`, which Emscripten's libc++ does not pull in through other headers.
    - **`-UOCC_CONVERT_SIGNALS`.** OCCT defines it on every compiler but MSVC; it turns
      `OCC_CATCH_SIGNALS` into setjmp/longjmp, and with WebAssembly exceptions Emscripten
      7.0.1's clang then emits invalid code: V8 refused every test program ("Compiling function
      `ShapeUpgrade_ShapeDivide::Perform(bool)` failed: br_table: label arity inconsistent",
      Node 22 and 24 alike, linked at `-O0` too). Isolated on that one file: valid without the
      define, invalid with it, whatever the optimisation level or `-fPIC`. The Windows build
      never defines it; WebAssembly has no signals to convert (a trap is a trap).
    - Exceptions: Emscripten 7's `-fwasm-exceptions` and Rust's target both use the legacy
      WebAssembly exception instructions (checked: `-wasm-use-legacy-eh` in both), which every
      current browser and Node 22 run. `Standard_Failure` still reaches Rust as `Err`.
  - **`oc-kernel-occt`**: the build script compiles the shim with `-fwasm-exceptions -fPIC` for
    the target and links OCCT's side module `lib/libocct.so` when the install has it
    (`node scripts/wasm.mjs side`, `scripts/build-occt-side.mjs`: all 48 archives,
    `-sSIDE_MODULE=1`, 40 s), else the archives (`OC_WASM_LINK=static` forces them); it now
    reruns when OCCT's library folder changes (a rebuilt OCCT of the same version was not
    relinked before). The shim's `set_threads` keeps OCCT's pool at one thread on a target
    without threads: `oc-graph`'s thread-count test asked for 4 and 16 and the pool waited
    forever, spinning, for threads that cannot start (stopped after 18 min of CPU).
  - **`oc-asm`**: `parallel::THREADS` (false on WebAssembly); an assembler without a `Runner`
    regenerates its parts on its own thread there (`Assembler::parallel`) instead of failing
    to spawn workers. Native behaviour unchanged. The browser's workers come in as a `Runner`
    (W1b).
  - **Tests in WebAssembly** (`npm run test:wasm` = `node scripts/wasm.mjs test`: the nine
    engine crates, debug profile, Node 22.14 through `scripts/wasm-runner.mjs`, which puts the
    side module next to each program; `-sNODERAWFS` so tests read the corpus): **233 passed, 0
    failed, 18 ignored**, with the side module and statically, about 1 min 50 s. The 18: the 10
    ignored natively too (timing reports) and 8 that need threads, ignored on this target with
    the reason in the attribute: five `oc-asm` `parallel.rs` tests (worker threads; the
    browser's hosts are W1b), two `oc-kernel-occt` `transfer.rs` tests (a body moved to another
    thread; bodies still move between kernels in the third, single-threaded one) and
    `oc-mesh` `threads.rs` (rayon pools of 4 and 16). rayon's global pool falls back to the
    calling thread by itself. Among those that pass: **`oc-graph` `corpus.rs`, every corpus
    part equal to the phase-0 oracle**, `oc-kernel-occt`'s corpus and robustness tests, the
    torture scripts (`naming.rs`), determinism with any requested thread count, and every
    `oc-asm` assembly, configuration, evaluation and out-of-core test.
  - **Finding: results are not bit for bit the native ones.** `oc-asm` `corpus.rs` compares
    the stored `.ocasm` files (recorded on Windows) byte for byte, and in WebAssembly
    `bracket-pins`' `document.json` had placements a few units in the last place apart
    (`-0.217607030470743` against `-0.21760703047074292`), and five meshes of the two corpus
    assemblies differed: same counts per face and per edge, same B-rep entries, same summaries,
    but other triangulations (a circle's vertices start elsewhere). Cause: musl's `sin`/`cos`
    (Emscripten's C library) and the Windows runtime's round differently in the last place, in
    the mate solver and in OCCT's mesher. The test now compares those two kinds of entries by
    value **in WebAssembly only** (numbers within 1e-12 relative; meshes: model vertices within
    1e-5 of the radius, flags, levels, vertex/index/segment counts, face and edge ranges
    exactly, each face's triangle area and each edge's length within 1e-6); natively it is
    byte for byte as before. Not a loosened golden: the stored files are unchanged and still
    checked exactly where they were recorded. Consequence: an assembly saved in the browser
    and one saved on the desktop differ in those bytes; each accepts the other's caches (same
    engine identity), which are equivalent (question 5).
  - **Measurements** (release builds; `oc-asm` example `regen`: the geometry host's part job,
    `parallel::work` = regenerate, export the body, mesh, on the 17 corpus parts, a fresh
    regenerator per part, best of 5 rounds; Ryzen 7 7800X3D; WebAssembly on Node 22.14, V8
    12.4, Chrome's engine):

    | 17 corpus part jobs | sum of best | first round | start-up (no parts) |
    |---|---|---|---|
    | native, OCCT pool on all processors | 653 ms | 690 ms | |
    | native, OCCT on one thread | 759 ms | 784 ms | |
    | WebAssembly, OCCT static | 993 ms (1.31× native, one thread) | 1420 ms | about 0.04 s |
    | WebAssembly, OCCT side module | 1347 ms (1.77×; 1.36× static) | 1822 ms | about 0.9 s |
    | the same, side module linked `-O3` | 1328 ms | 1927 ms | |

    Every part gave the same body blob size, levels of detail and triangle count in all four.
    The first round pays V8's compilation. The side module's cost is structural (calls between
    the modules go through imports and tables, OCCT is relocatable code, no whole-program
    optimisation), not its optimisation level.

    | files of the `regen` program | raw | gzip -9 | Brotli 11 |
    |---|---|---|---|
    | static: `regen.wasm` (17.7 MB code, 2.3 MB data, 2.3 MB names that a shipped build strips) | 22.5 MB | 6.9 MB | 4.83 MB |
    | static: `regen.js` (Emscripten's loader) | 97 KB | 25 KB | 22 KB |
    | side: `regen.wasm` | 3.3 MB | 1.0 MB | 0.72 MB |
    | side: `regen.js` (a JavaScript stub per OCCT symbol) | 19.0 MB | 1.5 MB | 0.77 MB |
    | side: `libocct.so` (OCCT, every symbol exported) | 46.0 MB | 11.5 MB | 7.99 MB |

    For comparison, today's web build downloads replicad's OCCT 7 WebAssembly, 23 MB raw. The
    side module could shrink by exporting only what the engine imports (`-sSIDE_MODULE=2` with
    a list read from the main module's imports; not done: it changes size, not speed).
  - **Checks:** `cargo test --workspace` with `OC_RENDER_REQUIRE_GPU=1`: 304 passed, 0 failed,
    17 ignored; workspace clippy `-D warnings` clean natively and for
    `wasm32-unknown-emscripten` (the nine engine crates, all targets; two Windows-only `oc-asm`
    test files had unused imports on any other target, now allowed there); `npm run typecheck`
    clean; vitest 142/142; `npm run test:wasm` 233 passed (above). Native scripts on the
    release build (D3D12): `--smoke` 7 faces, volume 20 858.407; `--corpus` **`corpusDiffs:
    []`**; `picking.js` ok (hover median 6 ms); `assembly.js` (246 checks),
    `configurations.js` (32), `lod.js` (29) and `scale.js` `ok: true`, no false check. No
    RivaTuner relaunch was needed.
  - **Deviations.** Files beyond the engine crates: `scripts/wasm.mjs`, `wasm-runner.mjs`,
    `build-occt-side.mjs`, `package.json` (`test:wasm`), `CLAUDE.md` (the command),
    `docs/ARCHITECTURE.md`, `docs/THIRD_PARTY.md` (the WebAssembly toolchain and runtime
    licences; no new crate). The OCCT build needed two flags (above). The browser itself is
    not touched yet: W1a's checks run on Node (V8); the browser checks are W1d's.
  - **For the owner.**
    1. **LGPL form in the browser.** (a) The side module as built (default; section 4's "link
       dynamically", replaceable like the installer's DLLs): 9.5 MB Brotli, kernel 1.36× slower
       than static, about 0.85 s more per worker start. (b) One static module plus a relink
       kit (LGPL 2.1 section 6(a): the engine's object files and link command published with
       each release, and a check that relinks them with the OCCT archives and runs the
       corpus): 4.8 MB and the fastest; it departs from section 4 and from `CLAUDE.md`'s
       "never ship a static OCCT build", for the browser only. Recommendation: (b) with the
       relink check in W1f, since the licence allows it and the browser pays for the side
       module on every worker; otherwise (a), trimmed with `-sSIDE_MODULE=2`. Needed before W1f.
    2. **Browsers without WebGPU** (mostly Linux in late 2025): the frozen three.js viewport
       drawing the native engine's level-0 meshes, parts only, assemblies greyed with a note
       (recommended); or require WebGPU and show a message; or keep the whole TypeScript
       engine as the fallback until WebGPU is everywhere.
    3. **Files:** the workspace as a folder through the File System Access API (Chromium),
       otherwise the browser's private file system with folder import and zip download,
       mirrored into the engine's file system (design point 5): agree?
    4. **Parts stay JSON `.ocpart` through W1** (Electron, frozen, reads them; the browser does
       not need part containers): agree, or make parts containers in W1e?
    5. **Platform rounding:** accept that desktop and browser agree to the oracle's figures but
       not bit for bit (caches shared, tests compare by value in WebAssembly), or make the
       engine's own maths platform-independent (a portable libm in the mate solver; OCCT's
       mesher would still differ)? Recommendation: accept.
    6. **No threads in W1** (no SharedArrayBuffer, no COOP/COEP headers on CloudFront;
       parallelism through workers): agree?
  - **Owner decisions (2026-11-23, "continue" after the reviewer's recommendations).**
    Question 1 (the LGPL form in the browser) stays **open** until before W1f: the reviewer
    recommends (b), one static module and a relink kit checked each release, as a browser
    user can no more replace a file on the site than relink (both mean hosting a modified
    copy), but it relaxes the owner's rule "never ship a static OCCT build", so the owner
    decides. W1b–W1e build and test both forms. Questions 2 to 6 accepted as recommended:
    2. Without WebGPU: the frozen three.js viewport drawing the engine's level-0 meshes, parts
       only, assemblies greyed with a note.
    3. Files: the workspace design (File System Access API, else OPFS with folder import and
       zip download), mirrored into the engine's file system.
    4. Parts stay JSON `.ocpart` through W1.
    5. Desktop and browser agree by value, not bit for bit; tests compare by value in
       WebAssembly.
    6. No threads in W1; parallelism through workers, no COOP/COEP headers.
  - Review: every check run again on the branch (native 304 passed, 17 ignored; WebAssembly
    233 passed, 18 ignored; clippy, typecheck, vitest 142, all seven native scripts). The
    merge is the branch on an unchanged main.
- 2026-11-23 **W1c done: the renderer on WebGPU** (branch `claude/funny-jackson-4a07d6`, not merged).
  - **Commits:** the pure move (`oc-view`), then the renderer for the browser in its own
    commit. Two owner requests made during the session are separate commits on the same
    branch: Help > About shows the version, build and edition in every app; SOLIDWORKS and
    Dassault Systèmes no longer appear in anything users see (UI, README, release notes,
    package and installer descriptions; code comments and design documents keep them).
  - **The pure move.** `scene_dto.rs` and `pick.rs` moved unchanged (git renames) from
    `oc-app` to the new Tauri-free crate `oc-view`; `view.rs`' frame logic (meshes synced,
    the scene converted, queries waiting for their frame, hover picks polled, the frame drawn
    into the off-screen target, capture) became `oc_view::View`. `oc-app` keeps the
    swapchain, the message loop, `present` and its diagnostics (`oc_view::Hooks` calls
    `diag::view` / `view_drawn` where the loop did); replies are closures
    (`oc_view::Reply`), so `View` knows nothing of channels or JavaScript. `oc-app`'s module
    list loses `mod pick; mod scene_dto;` for `use oc_view::{pick, scene_dto};` (W1b's
    edits to the same list touch neighbouring lines). Checked on that commit alone:
    `--smoke` 7 faces and `picking.js` ok (hover median 5.8 ms, as before).
  - **`oc-render` for `wasm32-unknown-unknown`** (wgpu 30's WebGPU backend, default
    features): it compiled unchanged; what changed is that it **never blocks** there. On
    `wasm32` the blocking calls are compiled out (`Renderer::headless`, `pick`,
    `render_image`, `draw_stats`, the read-back helper, `View::capture`; `pollster` is a
    native-only dependency), so nothing can wait on the browser's GPU by mistake (wgpu's
    WebGPU `poll` returns at once, so a blocking pick would silently answer nothing). New:
    `Renderer::on_adapter` (the device requested asynchronously; `headless` now uses it
    through `pollster`), `Renderer::draw_stats_async` / `PendingStats` (the draw counts copied
    into one staging buffer, collected when mapped; the blocking `draw_stats` is now that
    plus a wait: one submit and one map instead of two). `oc-view`: a click is answered as
    hover is on `wasm32`; draw counts for scripts are collected after the frame
    (`render_ms` is then the time until the counts are back); `Instant` comes from
    `web-time` there (`std::time::Instant` panics on that target). Native behaviour is
    unchanged.
  - **Optional features.** `Renderer::features` already asks for
    `DEPTH32FLOAT_STENCIL8 | INDIRECT_FIRST_INSTANCE` where the adapter has them and falls
    back to 24-bit depth and bodies one by one (`Renderer::culling`), so nothing new was
    needed. New `oc-render` test `tests/web.rs` (native, D3D12): a device made
    asynchronously draws; a device **without** both features draws Shaded With Edges and
    Hidden Lines Visible with **0 pixels different** from one with them and picks the same;
    async picks and draw counts equal the blocking ones. Edge on this machine offers both
    features (`depth32: true, indirectFirstInstance: true`). wgpu emulates
    `multi_draw_*_indirect` on WebGPU with one `draw*Indirect` per slot (mesh × level).
  - **`oc-web-render`** (new crate, wasm-bindgen, `cdylib`; empty on other targets so the
    workspace builds): `WebRender::create(OffscreenCanvas)` (async; fails without WebGPU,
    adapter or device), `frame` / `label` / `mesh(handle, bytes)` / `pick` / `box_select` /
    `stats` / `drawn` / `resize`, and `tick()`: prepare, serve, draw and present, poll, as one
    turn of the native render loop; it returns true while the GPU owes answers. The canvas
    covers the window and the frame is copied at the graphics area, as natively (WebGPU
    copies between formats that differ only in sRGB). The surface format is the browser's
    `bgra8unorm`, the target its sRGB form.
  - **The render worker** (`src/web/render.worker.ts`) loads the module and ticks it after
    its messages through a `MessageChannel` (browsers clamp nested `setTimeout(0)`s to
    4 ms), spinning while the GPU owes answers for at most 200 ticks, then every
    millisecond (the native thread spins likewise). `src/web/render.ts`
    (`WebRenderClient`) answers `view_frame`, `view_add_label`, `view_pick`, `view_box`,
    `view_stats` and `app_info` with Tauri's argument and reply shapes, so `NativeView` runs
    unchanged once W1b/W1f route `invoke` to it; `engineChannel()` gives the engine worker
    its `MessagePort` for meshes (`{t: "mesh", handle, bytes}`, buffer transferred). The
    engine's and the UI's channels are not ordered with each other, so a frame naming a mesh
    still on its way is drawn without it and again when it arrives (checked). WebGPU tells
    wgpu no adapter name: the worker reads `GPUAdapter.info` (vendor, architecture) for
    `app_info` ("nvidia blackwell (WebGPU)").
  - **Fallback seam** (owner decision 2026-11-23): `startViewport(canvas)` answers
    `{kind: "webgpu", client}` or `{kind: "three", reason, assemblies: false}`, and
    `FALLBACK_NOTE` is the note for greyed assemblies. Nothing switches yet: W1f does, along
    with the 21 `isNative()` gates that must then mean "native engine" (assemblies) or
    "WebGPU viewport" (`NativeView`).
  - **Checks without the full browser suite (W1d):** `npm run web:render-smoke`
    (`scripts/web-render.mjs`: builds the module and the harness meshes, starts Vite, opens
    `src/web/harness/render.html` in headless Edge 157 over the DevTools protocol with
    Node's own WebSocket, no new dependency) on the RTX 5070 Ti: first frame drawn; a click
    and a hover find the block's top face; the origin first; empty space; a window box takes
    the block's 14 edges; draw counts come back; a late mesh is drawn; 10 000 instances in
    the GPU lists; screenshots `web-render-smoke.png` (10 000 instances) and
    `web-render-smoke-part.png` (the block, hover highlight). A second run hides
    `navigator.gpu` (Chromium has no switch that removes WebGPU: `--disable-features=WebGPU`
    and `--disable-webgpu` left it on) and checks the fallback is reported. Meshes come from
    an example (`cargo run -p oc-web-render --example mesh_blobs`) until W1b's engine worker
    sends them. Headless Edge needed only `--enable-unsafe-webgpu` and ran on the real GPU.
  - **Measurements** (headless Edge 157, 1280×800 at scale 1, RTX 5070 Ti, Ryzen 7 7800X3D;
    ranges over runs):

    | | |
    |---|---|
    | `oc_web_render_bg.wasm` (release, wasm-bindgen 0.2.130, no wasm-opt) | 1 011 KB raw, 295 KB gzip -9, **220 KB Brotli 11** |
    | `oc_web_render.js` (bindings) | 77 KB raw, 12 KB Brotli |
    | module build (release, warm) | 26 s |
    | start: worker, module, adapter, device, every pipeline | 0.6–1.6 s |
    | first frame of the part (the browser compiles shaders at first use) | 1.0 s cold, 6 ms with the browser's shader cache |
    | part: frame message to drawn (median / max) | 0.3 / 2–18 ms; GPU done 1.6–2 ms |
    | part: hover pick round trip from the UI thread (median / max) | 4.9–6.0 / 11–52 ms |
    | 10 000 instances (two meshes): first frame with 1.4 MB of bodies | 20–21 ms |
    | 10 000 instances turning: frame to drawn (median / max) | 3.6–3.8 / 57–80 ms; **GPU 8 ms** |
    | 10 000 instances: hover pick (median / max) | 6.7–7.0 / 27–52 ms |

    For comparison, natively (D3D12, `picking.js`) hover is 5.8 ms median; G2's 10 000
    instances drew in 3.6 ms at 1920×1080 (another scene: 100 meshes, 8.6 M triangles).
    The browser's hover time is WebGPU's `mapAsync` round trip (removing the 4 ms timer clamp
    did not move the median); the maximums are single outliers. Why 10 000 small instances
    take 8 ms on the GPU in the browser is not known yet; W1e measures the scale assembly in
    the browser against the native app.
  - **Checks:** `cargo test --workspace` with `OC_RENDER_REQUIRE_GPU=1`: **307 passed, 0
    failed, 17 ignored** (304 before, plus `tests/web.rs`); workspace clippy `-D warnings`
    clean, and for `wasm32-unknown-unknown` on `oc-render`, `oc-view`, `oc-web-render`
    (libraries: their tests need OCCT); `npm run typecheck` clean; vitest **145/145**;
    native scripts on the release build (D3D12): `--smoke` 7 faces, volume 20 858.407; `--corpus` **`corpusDiffs: []`**; `picking.js` ok (hover median 5.6 ms); `assembly.js`, `configurations.js`, `lod.js` and `scale.js` `ok: true`, no false check; the browser smoke `ok: true` in three runs; `npm run test:wasm` 233 passed, 0 failed, 18 ignored (the engine crates are untouched).
  - **Deviations.** Files beyond the package's list: `oc-render`'s `cull.rs`, `renderer.rs`,
    `frame.rs`, `gpu_pick.rs`, `lib.rs`, `Cargo.toml` (the wasm gates and async calls),
    `scripts/web-render.mjs`, `package.json` (`web:render-build`, `web:render-smoke`),
    `.gitignore` (`web-render-*.png`), `CLAUDE.md`, `docs/ARCHITECTURE.md`,
    `docs/THIRD_PARTY.md` (js-sys, web-sys, wasm-bindgen-futures with the futures crates
    and slab, web-time; all MIT or Apache-2.0). The module is built into
    `target/web-render/` and the harness loads it from there through Vite; where the web
    build serves it from (`DEFAULT_MODULE` is `<base>web-render/oc_web_render.js`) is W1f's.
    `wasm-opt` is not installed.
  - **For the owner.**
    1. **The module's place in the web build** (W1f): copied into `dist/web-render/` by the
       web build only, so the native app's embedded UI does not carry it? Recommended.
    2. **wasm-opt** (Binaryen, Apache-2.0) as a build tool for the web build's modules,
       renderer and engine alike: usually 10–20 % smaller, a download outside npm.
       Recommended for W1f, measured there.
    3. **Hover picks in the browser** answer in 5–7 ms, like the desktop app's 5.8 ms:
       accept? (Faster would need picks on the CPU, as Select Other's ray casts are.)
