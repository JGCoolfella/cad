# Engine crates

Each directory here is one crate of the native engine (see `docs/OVERHAUL_PLAN.md`,
sections 2 and 6). Crates inherit `edition`, `rust-version`, `license`, `publish` and
`[lints]` from the root `Cargo.toml` (`edition.workspace = true`, ..., `lints.workspace = true`).

`oc-kernel-occt` links OCCT 8.0.1 from the install named by `OCCT_DIR` (build one with
`crates/oc-kernel-occt/build-occt.cmd` on Windows or `build-occt.sh` on macOS and Linux;
about 12 minutes on 16 threads). `cargo test --workspace` therefore needs `OCCT_DIR`; the
other crates build without it (`cargo test -p oc-doc -p oc-sketch -p oc-kernel -p oc-naming`). `oc-mesh`
builds without it too, but its tests mesh real bodies through `oc-kernel-occt`. With a
shared OCCT build on Windows, put its `win64/vc14/bin` on `PATH` to run the tests.

`oc-render` builds without OCCT; its smoke test (`tests/smoke.rs`) meshes a real part through
`oc-kernel-occt` and needs a GPU adapter (hardware, or WARP on Windows). Without one it prints
`SKIP` and passes, unless `OC_RENDER_REQUIRE_GPU=1`. `WGPU_BACKEND=dx12|vulkan` picks the
backend, `OC_RENDER_FALLBACK=1` the software adapter. Images go to `target/tmp/oc-render-smoke/`.

`oc-host` runs parts in geometry-host processes (plan G3b). Its tests start its own `oc-host`
binary as the host; the app starts itself with `--geometry-host` instead.

`oc-app` is the native desktop app (Tauri 2; plan F2). It links OCCT, so it needs
`OCCT_DIR`. `cargo run -p oc-app` loads the Vite dev server (`npm run dev` first);
`npm run native` builds the UI and runs it embedded (`--features custom-protocol`).
End-to-end checks (they open a window; Windows): `npm run native:smoke` and
`npm run native:corpus` print `SMOKE {...}` and write screenshots next to the working directory.
