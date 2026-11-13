# Engine crates

Each directory here is one crate of the native engine (see `docs/OVERHAUL_PLAN.md`,
sections 2 and 6). Crates inherit `edition`, `rust-version`, `license`, `publish` and
`[lints]` from the root `Cargo.toml` (`edition.workspace = true`, ..., `lints.workspace = true`).

`oc-kernel-occt` links OCCT 8.0.1 from the install named by `OCCT_DIR` (build one with
`crates/oc-kernel-occt/build-occt.cmd` on Windows or `build-occt.sh` on macOS and Linux;
about 12 minutes on 16 threads). `cargo test --workspace` therefore needs `OCCT_DIR`; the
other crates build without it (`cargo test -p oc-doc -p oc-sketch -p oc-kernel`). `oc-mesh`
builds without it too, but its tests mesh real bodies through `oc-kernel-occt`. With a
shared OCCT build on Windows, put its `win64/vc14/bin` on `PATH` to run the tests.
