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
- `npm run desktop`: build + Electron. `npx electron . --smoke=out.png`: headless-ish end-to-end check (prints `SMOKE {...}`).
- `npm run native`: the native desktop app (`crates/oc-app`, Tauri + the Rust engine; needs `OCCT_DIR`). `npm run native:smoke` / `native:corpus`: its end-to-end checks (`SMOKE {...}`; `--corpus` compares all corpus parts with the oracle). `cargo run -p oc-app` uses the Vite dev server.
- `npm run deploy:web`: build and publish the browser version to https://cad.greendealer.co.nz (S3 + CloudFront, needs AWS CLI credentials).
- Desktop release: push a `v*` tag (or run the Release workflow with a tag). GitHub Actions builds the Windows .exe and macOS .dmg and attaches them to the release.
- After upgrading `replicad` / `replicad-opencascadejs`, delete `node_modules/.vite`. Stale pre-bundles cause errors like "gp_Vec_4 is not a constructor". The two packages must have matching versions.

## Rules
- Every user action is a command in `src/commands/commands.ts`. Menus, toolbars, shortcuts and gestures reference command ids.
- Unimplemented SOLIDWORKS commands stay visible as `todo(...)` entries (greyed), so the layout matches SOLIDWORKS.
- The document is plain JSON (`src/core/types.ts`). Never keep geometry state outside it. Call `app.checkpoint()` before mutating the document and `app.sketchCheckpoint()` before mutating a sketch.
- Kernel code runs only in `src/kernel/kernel.worker.ts`. The UI thread must never call OCCT.
- Defaults mirror SOLIDWORKS. If unsure of a SOLIDWORKS behaviour, make it an option in `src/app/settings.ts` and mark it 🔍 in FEATURE_SPEC.md.
- Original artwork only (`src/ui/icons.tsx`). Don't copy SOLIDWORKS icons, logos or text assets.
