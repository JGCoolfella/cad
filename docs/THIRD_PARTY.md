# Third-party components

Licences of what the native engine (`crates/`) depends on, per `OVERHAUL_PLAN.md` section 4
(licensing note). Check a dependency's licence before adding it and record it here. The
JavaScript app's npm dependencies are listed in `package.json`.

## Rust crates

All are permissive. None requires anything beyond keeping its notice in a distribution.

| Crate | Version | Licence | Used by | Why |
|---|---|---|---|---|
| serde, serde_core, serde_derive | 1.0.229 | MIT OR Apache-2.0 | oc-sketch | Sketch data (de)serialisation |
| serde_json (+ itoa, memchr, zmij) | 1.0.154 | MIT OR Apache-2.0 (memchr: Unlicense OR MIT; zmij: MIT) | oc-sketch | JSON, `float_roundtrip` for exact numbers |
| indexmap (+ hashbrown, equivalent) | 2.14.2 | Apache-2.0 OR MIT | oc-sketch | Insertion-ordered maps (solver order = document order) |
| wasm-bindgen (+ macro, macro-support, shared, bumpalo, cfg-if, rustversion) | 0.2.130 | MIT OR Apache-2.0 | oc-sketch-wasm | WebAssembly binding |
| proc-macro2, quote, syn, unicode-ident | build time | MIT OR Apache-2.0 (unicode-ident also Unicode-3.0) | derive macros | Compile-time only |

Build tool (not linked): `wasm-bindgen-cli` 0.2.130 (MIT OR Apache-2.0), version pinned to
match the `wasm-bindgen` crate.

## Planned

- OCCT 8.0.1: LGPL 2.1 with exception. Link dynamically and ship its licence (phase E3).
