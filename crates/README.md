# Engine crates

Each directory here is one crate of the native engine (see `docs/OVERHAUL_PLAN.md`,
sections 2 and 6). Crates inherit `edition`, `rust-version`, `license`, `publish` and
`[lints]` from the root `Cargo.toml` (`edition.workspace = true`, ..., `lints.workspace = true`).
