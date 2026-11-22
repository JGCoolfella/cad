# OpenCAD: notes for maintainers and agents

Goal: SOLIDWORKS-identical user experience (UI, commands, mouse/keyboard, snapping,
inference), rebuilt cleanly. Read `docs/ARCHITECTURE.md` first. `docs/FEATURE_SPEC.md` is the
parity checklist. Update its status column whenever behaviour changes.
`docs/OVERHAUL_PLAN.md` is the active engineering plan (native Rust engine, OCCT behind a
kernel trait, wgpu renderer, persistent naming). When asked to do a work package from it,
follow its session protocol.

## Commands
- `npm run dev`: Vite dev server (http://localhost:5173). `window.opencad` exposes the App for debugging.
- `npm test`: vitest (solver/profile unit tests live next to the code: `*.test.ts`).
- `npm run typecheck`, `npm run build`.
- **Native desktop app (the shipped Windows app since 0.5.0, `crates/oc-app`: Tauri + the Rust engine).**
  - Build env: `OCCT_DIR` (static OCCT 8.0.1 for development and tests, `C:\occt\install-8.0.1`), cargo in `%USERPROFILE%\.cargoin`, `OC_RENDER_REQUIRE_GPU=1` for the render tests.
  - `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
  - `npm run native`: run it on the built UI (`cargo run -p oc-app` alone loads the Vite dev server).
  - `npm run native:smoke` / `native:corpus` / `native:picking` / `native:assembly`: end-to-end checks (`SMOKE {...}` on stdout and in `<screenshot>.log`; `--corpus` compares all corpus parts with the oracle). Release builds are GUI programs: from cmd use `start /wait`, from PowerShell `Start-Process -Wait`, or read the `.log`.
  - `npm run dist:native:win`: the Windows installer, `release/OpenCAD-<version>-win-x64-setup.exe` (NSIS, per-user). Needs a **shared** OCCT 8.0.1 (`OCCT_SHARED_DIR`, default `C:\occt\install-8.0.1-shared`; build with `crates\oc-kernel-occtuild-occt.cmd <src> <build> <install> Shared`). Never ship a static OCCT build (LGPL, `docs/THIRD_PARTY.md`).
  - `npm run test:native:install`: installs that installer into a temporary folder, runs smoke, corpus and picking on the installed exe, uninstalls.
- **Engine on WebAssembly (plan W1, browser build in progress).** `npm run test:wasm` runs the engine crates' tests (`oc-doc` … `oc-asm`) for `wasm32-unknown-emscripten` under Node (`scripts/wasm.mjs`; `node scripts/wasm.mjs cargo <args>` for anything else). Needs emsdk 7.0.1 (`EMSDK_ROOT`, default `C:\occt\emsdk`), `rustup target add wasm32-unknown-emscripten`, and OCCT 8.0.1 for WebAssembly (`OCCT_WASM_DIR`, default `C:\occt\install-8.0.1-wasm`, built by `crates\oc-kernel-occtuild-occt-wasm.cmd`, then `node scripts/wasm.mjs side` for OCCT's side module `lib/libocct.so`, the replaceable shipping form). Keep emsdk and OCCT build trees outside the repository.
- **Electron app (frozen, see Rules):** `npm run desktop` (build + Electron), `npx electron . --smoke=out.png` (prints `SMOKE {...}`), `npm run dist:win` / `dist:mac` (`release/OpenCAD-Electron-*`; no longer released on Windows).
- `npm run deploy:web`: build and publish the browser version to https://cad.greendealer.co.nz (S3 + CloudFront, needs AWS CLI credentials). The browser version still runs the TypeScript engine (OCCT in WebAssembly) until plan package W1.
- Release (local while GitHub Actions is unavailable; the owner does it): bump `version` in `package.json`, `crates/oc-app/tauri.conf.json` and `crates/oc-app/Cargo.toml` (`cargo test -p oc-app` checks they agree) and `RELEASE_NOTES.md`; re-record the assembly corpus, whose caches are keyed to the app version (`UPDATE_GOLDEN=1 cargo test -p oc-asm --test corpus`, commit the changed `corpus/assemblies/*.ocasm`); run the checks above; `npm run dist:native:win` and `npm run test:native:install`; commit and push; `cp C:/occt/dl/occt-8.0.1.tar.gz release/occt-8.0.1-source.tar.gz`; `gh release create vX.Y.Z release/OpenCAD-X.Y.Z-win-x64-setup.exe release/occt-8.0.1-source.tar.gz --target main --notes-file RELEASE_NOTES.md` (the OCCT source goes with every release: LGPL); `npm run deploy:web`. Unsigned for now; no macOS download until the native app is packaged for macOS. With Actions working, pushing a `v*` tag runs `.github/workflows/release.yml` (native Windows installer and the OCCT source).
- After upgrading `replicad` / `replicad-opencascadejs`, delete `node_modules/.vite`. Stale pre-bundles cause errors like "gp_Vec_4 is not a constructor". The two packages must have matching versions.

## Rules
- Every user action is a command in `src/commands/commands.ts`. Menus, toolbars, shortcuts and gestures reference command ids.
- Unimplemented SOLIDWORKS commands stay visible as `todo(...)` entries (greyed), so the layout matches SOLIDWORKS.
- The document is plain JSON (`src/core/types.ts`). Never keep geometry state outside it. Call `app.checkpoint()` before mutating the document and `app.sketchCheckpoint()` before mutating a sketch.
- Kernel code runs only in `src/kernel/kernel.worker.ts`. The UI thread must never call OCCT.
- Defaults mirror SOLIDWORKS. If unsure of a SOLIDWORKS behaviour, make it an option in `src/app/settings.ts` and mark it 🔍 in FEATURE_SPEC.md.
- Original artwork only (`src/ui/icons.tsx`). Don't copy SOLIDWORKS icons, logos or text assets.
- **The Electron app and the TypeScript engine are frozen (since 0.5.0, plan F3): bug fixes only; new modelling goes into the Rust crates.** The React UI is shared and keeps changing; the Electron shell (`desktop/`), `src/kernel/*` and the three.js viewport get fixes only. They stay buildable (`npm run desktop`, `electron . --smoke`) and the web build keeps using them until W1.
- Every new dependency's licence goes in `docs/THIRD_PARTY.md`; the installer's `THIRD_PARTY_NOTICES.txt` is generated from the dependency graph (`scripts/third-party-notices.mjs`).
