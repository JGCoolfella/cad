# Third-party components

Licences of what the native engine (`crates/`) depends on, per `OVERHAUL_PLAN.md` section 4
(licensing note). Check a dependency's licence before adding it and record it here. Scope:
crates under `crates/`; the throwaway `spikes/` workspace is not shipped and is not listed.
The JavaScript app's npm dependencies are listed in `package.json`. Check with
`cargo tree -p <crate> -e normal --prefix none -f "{p} {l}"`.

## Rust crates

| Crate | Version | Licence | Used by | Why |
|---|---|---|---|---|
| serde, serde_core, serde_derive | 1.0 | MIT OR Apache-2.0 | oc-doc, oc-sketch | Serialization framework |
| serde_json (+ itoa, memchr, zmij) | 1.0 | MIT OR Apache-2.0 (memchr: Unlicense OR MIT; zmij: MIT) | oc-doc, oc-sketch | JSON, `float_roundtrip` for exact numbers, `preserve_order` so sketch records load in file order |
| serde_path_to_error | 0.1 | MIT OR Apache-2.0 | oc-doc | JSON paths in schema errors |
| ryu | 1.0 | Apache-2.0 OR BSL-1.0 | oc-doc | Shortest round-trip float digits (canonical JSON numbers) |
| imbl | 6.1 | MPL-2.0+ | oc-doc | Persistent vector and ordered map (structural sharing for undo) |
| miniz_oxide (+ adler2) | 0.8 | MIT OR Zlib OR Apache-2.0 (adler2: 0BSD OR MIT OR Apache-2.0) | oc-doc | Deflate and inflate of `.ocasm` zip entries (G1a; already in the app through png and tauri) |
| crc32fast | 1.5 | MIT OR Apache-2.0 | oc-doc | CRC-32 of `.ocasm` zip entries (G1a; already in the app through png and tauri) |
| imbl-sized-chunks, bitmaps | 0.1, 3.2 | MPL-2.0+ | oc-doc | imbl internals |
| archery | 1.2 | MIT | oc-doc | imbl internals (shared pointer abstraction) |
| rand_core, rand_xoshiro | 0.9, 0.7 | MIT OR Apache-2.0 | oc-doc | imbl internals |
| indexmap (+ hashbrown, equivalent) | 2.14 | Apache-2.0 OR MIT | oc-sketch, oc-doc (through serde_json `preserve_order`) | Insertion-ordered maps (solver order = document order) |
| smallvec | 1.16 | MIT OR Apache-2.0 | oc-naming | Face names: one or two tags inline, without a heap allocation |
| wasm-bindgen (+ macro, macro-support, shared, bumpalo, cfg-if, rustversion) | =0.2.130 | MIT OR Apache-2.0 | oc-sketch-wasm, oc-web-render (and wgpu's WebGPU backend) | WebAssembly binding |
| js-sys, web-sys | 0.3.107 | MIT OR Apache-2.0 | oc-web-render, wgpu's WebGPU backend (`wasm32-unknown-unknown` only) | JavaScript and Web API bindings: `OffscreenCanvas`, callbacks, WebGPU (W1c) |
| wasm-bindgen-futures (+ futures-core, futures-task, futures-util, slab) | 0.4.80 (futures 0.3.34, slab 0.4.12) | MIT OR Apache-2.0 (slab: MIT) | oc-web-render, wgpu's WebGPU backend (`wasm32-unknown-unknown` only) | Awaits the browser's promises: adapter, device, read-backs (W1c) |
| web-time | 1.1 | MIT OR Apache-2.0 | oc-view, oc-web-render (`wasm32` only; already in the app through tauri) | `Instant` from `performance.now()`: `std::time::Instant` panics on `wasm32-unknown-unknown` (W1c) |
| proc-macro2, quote, syn, unicode-ident | build time | MIT OR Apache-2.0 (unicode-ident also Unicode-3.0) | derive macros | Compile-time only |
| proptest | 1.12 | MIT OR Apache-2.0 | oc-doc (dev only) | Property tests; not shipped |
| cxx | 1.0.205 | MIT OR Apache-2.0 | oc-kernel-occt | Rust ↔ C++ bridge to the OCCT shim (linked) |
| link-cplusplus | 1.0 | MIT OR Apache-2.0 | oc-kernel-occt (through cxx) | Links the C++ standard library |
| cxx-build, cxxbridge-flags, cxxbridge-macro, cc, find-msvc-tools, scratch, shlex, codespan-reporting, termcolor, unicode-width, foldhash, winapi-util, windows-sys, windows-link | build time | MIT OR Apache-2.0 (foldhash: Zlib; termcolor, winapi-util: Unlicense OR MIT; codespan-reporting: Apache-2.0) | oc-kernel-occt build script | Generate and compile the C++ side of the bridge; compile-time only |
| serde_json | 1.0 | MIT OR Apache-2.0 | oc-kernel-occt, oc-mesh (dev only) | Reads the corpus oracle in tests |
| rayon, rayon-core (+ crossbeam-deque, crossbeam-epoch, crossbeam-utils, either) | 1.12, 1.13 | MIT OR Apache-2.0 | oc-mesh | Work-stealing thread pool: packs faces and edges in parallel |
| wgpu, wgpu-core, wgpu-hal, wgpu-types, wgpu-naga-bridge, wgpu-core-deps-windows-linux-android | 30.0.1 | MIT OR Apache-2.0 | oc-render | GPU API over D3D12, Vulkan and Metal (plan section 1) |
| naga, naga-types (+ spirv, codespan-reporting, unicode-width, bit-set, bit-vec, half, num-traits, libm, ordered-float, arrayvec, indexmap, hashbrown, foldhash, allocator-api2, rustc-hash, thiserror, once_cell, static_assertions, log) | 30.0.1 | MIT OR Apache-2.0 (spirv, codespan-reporting: Apache-2.0; libm, ordered-float: MIT; foldhash: Zlib) | oc-render (through wgpu) | WGSL shader translation to HLSL/SPIR-V/MSL |
| ash, gpu-allocator, presser, range-alloc, renderdoc-sys, profiling, libloading, parking_lot (+ lock_api, parking_lot_core, scopeguard), smallvec, bitflags, cfg-if, libc | various | MIT OR Apache-2.0 (libloading: ISC) | oc-render (through wgpu-hal) | Vulkan bindings, D3D12 memory allocation, dynamic loading of the graphics drivers |
| windows, windows-core, windows-collections, windows-future, windows-numerics, windows-result, windows-strings, windows-threading, windows-link (+ windows-implement, windows-interface) | 0.62 | MIT OR Apache-2.0 | oc-render (through wgpu-hal, Windows only) | Direct3D 12 and DXGI bindings |
| glow, khronos-egl, glutin_wgl_sys, raw-window-handle | various | MIT OR Apache-2.0 OR Zlib (glutin_wgl_sys: Apache-2.0) | oc-render (through wgpu's default `gles` feature) | OpenGL backend; compiled in but not used (`Backends::PRIMARY`). Can be dropped with `default-features = false` if size matters |
| glam | 0.30 | MIT OR Apache-2.0 | oc-render | `f64` camera maths, `f32` matrices for the GPU |
| bytemuck (+ bytemuck_derive) | 1.25 | Zlib OR Apache-2.0 OR MIT | oc-render | Plain-data casts of uniform and vertex structs |
| pollster | 1.0 | Apache-2.0 OR MIT | oc-render | Blocks on wgpu's adapter and device futures (headless renderer) |
| zerocopy (+ zerocopy-derive), document-features, litrs, syn 3 | build time / various | BSD-2-Clause OR Apache-2.0 OR MIT; MIT OR Apache-2.0 | oc-render (through wgpu) | Compile-time and internal helpers |
| png | 0.17 | MIT OR Apache-2.0 | oc-render (dev only) | Writes the smoke test's images; not shipped |
| tauri, tauri-runtime, tauri-runtime-wry, tauri-utils, tauri-macros, tauri-codegen, tauri-build, wry, tao, muda, webview2-com (+ webview2-com-sys, -macros), dpi | 2.12.5 (tauri), 0.57 (wry), 0.37 (tao) | Apache-2.0 OR MIT (tao: Apache-2.0; webview2-com: MIT; dpi: Apache-2.0 AND MIT) | oc-app | The desktop shell: window, WebView2 webview (P2), IPC between the React UI and the engine; webview2-com also directly (Windows only): the app watches the WebView2 processes and starts again when the browser process dies (`webview_watch.rs`) |
| tokio, bytes, tracing, http, url, urlpattern, infer, cfb, plist, ico, schemars, dunce, walkdir, same-file, glob, dirs (+ option-ext) | various | MIT, MIT OR Apache-2.0, Unlicense OR MIT (walkdir, same-file), CC0-1.0 OR MIT-0 OR Apache-2.0 (dunce); option-ext: MPL-2.0 | oc-app (through tauri) | Async runtime for commands, HTTP types of the IPC, config and asset handling |
| cssparser, selectors, dtoa-short (+ cssparser-macros), dom_query, html5ever, phf (+ phf_shared, phf_macros, precomputed-hash, new_debug_unreachable) | various | MPL-2.0 (cssparser, selectors, dtoa-short); MIT (others) | oc-app (through tauri-utils) | Tauri's HTML processing of the bundled UI (CSP injection) |
| brotli, alloc-no-stdlib, alloc-stdlib, brotli-decompressor | 9.1 | BSD-3-Clause AND MIT | oc-app (through tauri-codegen) | Compresses the embedded UI assets |
| icu_* (+ litemap, tinystr, writeable, yoke, zerofrom, zerotrie, zerovec, potential_utf) | 2.4 | Unicode-3.0 | oc-app (through url / idna) | Unicode normalisation of URLs |
| windows-sys | 0.61 | MIT OR Apache-2.0 | oc-app (Windows only) | GDI screen capture for the `--smoke` screenshot; attaching to the parent console for `--smoke` output (F3, `Win32_System_Console`); the native Open and Save As dialogs (G1a, `Win32_UI_Controls_Dialogs`) |
| png | 0.17 | MIT OR Apache-2.0 | oc-app | Writes the `--smoke` screenshots (smoke mode only, but linked) |

**MPL-2.0 note (imbl family; cssparser, selectors, dtoa-short and option-ext in the Tauri
tree).** MPL-2.0 is file-level copyleft: it applies only to the
library's own source files. Using the crates unmodified in a closed-source product is
allowed; if their files are ever modified, the modified files must be published under
MPL-2.0. Ship their licence text with binaries. Everything else is permissive and only
requires keeping its notice in a distribution.

**Runtime (not shipped by us):** the Microsoft Edge WebView2 Runtime, part of Windows 10/11
(Evergreen). `webview2-com` loads it without a separate loader DLL. The installer (F3) uses
Tauri's `downloadBootstrapper` mode: where the runtime is missing it downloads Microsoft's
bootstrapper at install time; nothing of it is in the installer.

The full list for the app: `cargo tree -p oc-app -e normal --prefix none -f "{p} {l}"`
(350 lines, 2026-11-15; every licence above is permissive or MPL-2.0).

Build tool (not linked): `wasm-bindgen-cli` 0.2.130 (MIT OR Apache-2.0), version pinned to
match the `wasm-bindgen` crate.

## The Windows installer (F3)

`npm run dist:native:win` (`scripts/dist-native.mjs`) builds it. What it contains and why:

| Component | Version | Licence | In the installer | Notes |
|---|---|---|---|---|
| `opencad.exe` (our code, the crates above, the React UI) | 0.5.0 | ours; crates as above | yes | The UI (`dist/`) is embedded **without** the TypeScript engine's OCCT WebAssembly and its worker: the native app never loads them, and an embedded LGPL library could not be replaced by the user. |
| OCCT 8.0.1 DLLs (`TK*.dll`, 26 of the 48) | 8.0.1 | LGPL 2.1 + Open CASCADE Exception | yes, next to `opencad.exe` | Only the DLLs the program needs, found by following the PE imports (`scripts/native-pe.mjs`). The build stops if `opencad.exe` imports no OCCT DLL (a static build). Replaceable by any compatible OCCT 8.0.1 build. |
| `LICENSE_LGPL_21.txt`, `OCCT_LGPL_EXCEPTION.txt` | | | yes, next to `opencad.exe` | From the OCCT install. |
| `THIRD_PARTY_NOTICES.txt` | | | yes | Generated by `scripts/third-party-notices.mjs` from `cargo metadata` (oc-app's normal dependencies for the target: 282 crates on Windows) and the UI's bundled npm packages (react, react-dom, scheduler, three: MIT). Each distinct licence text appears once with the packages using it; packages that ship no text get the standard one from `crates/oc-app/licenses/` with their authors as copyright holders (7 on Windows). Includes the MPL-2.0 text and where each MPL-2.0 crate's source is. |
| Microsoft Visual C++ runtime: `msvcp140.dll`, `vcruntime140.dll`, `vcruntime140_1.dll` | 14.x (the newest installed with Visual Studio) | Microsoft Visual Studio licence, "Distributable Code" | yes, next to `opencad.exe` (app-local) | OCCT's DLLs and the C++ shim need them. App-local copies are not serviced by Windows Update; the alternative is running Microsoft's VC++ redistributable installer from ours (owner decision, plan section 8 F3). The Universal CRT (`api-ms-win-crt-*`) is part of Windows 10/11. |
| NSIS | 3.13 | zlib/libpng (plus bzip2/zlib notices in its `COPYING`) | the installer's stub and the uninstaller | Downloaded by the Tauri CLI to `%LOCALAPPDATA%\tauri\NSIS` on first use. |
| `nsis_tauri_utils.dll` | 0.5.3 | MIT OR Apache-2.0 | yes (installer plug-in) | Downloaded by the Tauri CLI. |
| `@tauri-apps/cli` (npm devDependency) | 2.12.4 | Apache-2.0 OR MIT | no (build tool) | `tauri build --no-bundle` and `tauri bundle`. |

**LGPL 2.1 source offer (owner decision, plan section 8 F3).** Section 4 of the LGPL asks
that whoever distributes the library's object code also gives access to its source "from
the same place". The notices name the exact upstream archive and the build script. To be
safe, attach the OCCT 8.0.1 source archive to each GitHub release next to the installer.

## C++ libraries

### Open CASCADE Technology (OCCT) 8.0.1

- **Licence:** LGPL 2.1 with the Open CASCADE Exception (`LICENSE_LGPL_21.txt` and
  `OCCT_LGPL_EXCEPTION.txt` in the OCCT install; ship both with any binary). The exception
  allows using OCCT's headers (inline functions, templates, macros) without the result
  becoming a derivative work; the library code itself stays under the LGPL.
- **Used by:** `crates/oc-kernel-occt` (E3), through its own C++ shim (`cpp/`, our code,
  calling OCCT's public API only). OCCT sources are not modified.
- **What is linked:** the minimal build of `crates/oc-kernel-occt/build-occt.cmd|.sh`
  (modules FoundationClasses, ModelingData, ModelingAlgorithms, DataExchange; no
  Visualization, Tcl/Tk, FreeType, FreeImage, OpenGL, TBB, VTK, RapidJSON or Draco). That
  install has 48 `TK*` libraries; the build script links all of them (DataExchange pulls in
  TKV3d/TKService/TKVCAF through TKXCAF, built without any graphics dependency). Only
  system libraries are added (Windows: user32, advapi32, gdi32, ws2_32, shell32).
- **How it is linked:** `build.rs` detects the install type.
  - **Static** (`build-occt … Static`, the default): used for development, tests and CI.
    Under LGPL 2.1 section 6 a statically linked binary may only be distributed together
    with what users need to relink it against a modified OCCT (our object files or
    sources). **Do not ship static builds** unless that is provided.
  - **Shared** (`build-occt … Shared`): the shipping configuration the plan requires
    (section 4: link dynamically). Verified on Windows (2026-11-13): the full
    `oc-kernel-occt` test suite passes against the DLLs (`win64/vc14/bin` on `PATH`).
    Installers (F3) must ship the OCCT DLLs or shared objects and their licence texts, and
    keep them replaceable. **Done for Windows in F3** (above, "The Windows installer"): the
    shared install is `C:\occt\install-8.0.1-shared` on the dev machine (`OCCT_SHARED_DIR`),
    and `scripts/dist-native.mjs` refuses a static build.
  - **WebAssembly** (W1a, plan section 8): `build-occt-wasm.cmd` builds the same modules
    as static archives with Emscripten; `scripts/build-occt-side.mjs` (`node scripts/wasm.mjs
    side`) links them into one Emscripten **side module**, `lib/libocct.so` (served to
    browsers as its own file), which the engine, an Emscripten main module, loads at start:
    dynamic linking, as the installer's DLLs. A user replaces it with any OCCT 8.0.1 built by
    those two steps with the same Emscripten. Engine tests may link the archives instead
    (`OC_WASM_LINK=static`, or before the side module exists); **do not ship that build**
    unless the owner decides for the relink-kit route (LGPL 2.1 section 6(a); plan section 8,
    W1a question 1: static is about 1.36× faster and half the download).
    The browser build (W1f) must serve OCCT's two licence texts and the notices, and the
    OCCT source archive goes with each release as for the installer.
- Version pinned: the build fails on any OCCT other than 8.0.1.

## WebAssembly toolchain and runtime (W1)

The browser build's engine is compiled by Emscripten (`wasm32-unknown-emscripten`), which
links its own runtime into every program. Pinned: emsdk / Emscripten **7.0.1**
(`build-occt-wasm.cmd`, `scripts/wasm.mjs`), installed outside the repository
(`C:\occt\emsdk` on the dev machine).

| Component | Licence | Shipped | Notes |
|---|---|---|---|
| Emscripten (compiler driver, JavaScript runtime glue in each program) | MIT OR University of Illinois/NCSA | the glue, in the engine's `.js` | `upstream/emscripten/LICENSE` |
| musl libc (Emscripten's C library) | MIT | linked | `system/lib/libc/musl/COPYRIGHT` |
| libc++, libc++abi, libunwind, compiler-rt (LLVM) | Apache-2.0 WITH LLVM-exception | linked | The LLVM exception waives the Apache notice requirement for compiled forms; listed in the notices anyway. |
| dlmalloc (Emscripten's default allocator) | CC0 1.0 (public domain) | linked | |
| Rust standard library for `wasm32-unknown-emscripten` | MIT OR Apache-2.0 | linked | `rustup target add wasm32-unknown-emscripten` (stable) |
| Node.js (runs the engine tests, `npm run test:wasm`, and the engine workers' checks, `npm run test:web-engine`) | MIT | no | Build and test only. |
| esbuild (bundles `src/web/` for the Node checks, `scripts/web-engine.mjs`) | MIT | no | Already in the tree as Vite's own bundler; not a new dependency (W1b). |

No new Rust crate is added by W1a or W1b (`oc-engine` and `oc-web` are the project's own). W1c's renderer module (`oc-web-render`, `wasm32-unknown-unknown`, built by `scripts/web-render.mjs` with `wasm-bindgen-cli`) adds the crates marked W1c above; it has no C or C++ code and no Emscripten runtime. Its smoke run uses the installed Edge or Chrome (not shipped). The web build's notices file (W1f) is generated from the
same dependency graph for the WebAssembly targets plus these runtime components.
