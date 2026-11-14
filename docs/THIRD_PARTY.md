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
| imbl-sized-chunks, bitmaps | 0.1, 3.2 | MPL-2.0+ | oc-doc | imbl internals |
| archery | 1.2 | MIT | oc-doc | imbl internals (shared pointer abstraction) |
| rand_core, rand_xoshiro | 0.9, 0.7 | MIT OR Apache-2.0 | oc-doc | imbl internals |
| indexmap (+ hashbrown, equivalent) | 2.14 | Apache-2.0 OR MIT | oc-sketch, oc-doc (through serde_json `preserve_order`) | Insertion-ordered maps (solver order = document order) |
| smallvec | 1.16 | MIT OR Apache-2.0 | oc-naming | Face names: one or two tags inline, without a heap allocation |
| wasm-bindgen (+ macro, macro-support, shared, bumpalo, cfg-if, rustversion) | =0.2.130 | MIT OR Apache-2.0 | oc-sketch-wasm | WebAssembly binding |
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

**MPL-2.0 note (imbl family).** MPL-2.0 is file-level copyleft: it applies only to the
library's own source files. Using the crates unmodified in a closed-source product is
allowed; if their files are ever modified, the modified files must be published under
MPL-2.0. Ship their licence text with binaries. Everything else is permissive and only
requires keeping its notice in a distribution.

Build tool (not linked): `wasm-bindgen-cli` 0.2.130 (MIT OR Apache-2.0), version pinned to
match the `wasm-bindgen` crate.

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
    keep them replaceable.
- Version pinned: the build fails on any OCCT other than 8.0.1.
