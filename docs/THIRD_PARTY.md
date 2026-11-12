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
| wasm-bindgen (+ macro, macro-support, shared, bumpalo, cfg-if, rustversion) | =0.2.130 | MIT OR Apache-2.0 | oc-sketch-wasm | WebAssembly binding |
| proc-macro2, quote, syn, unicode-ident | build time | MIT OR Apache-2.0 (unicode-ident also Unicode-3.0) | derive macros | Compile-time only |
| proptest | 1.12 | MIT OR Apache-2.0 | oc-doc (dev only) | Property tests; not shipped |

**MPL-2.0 note (imbl family).** MPL-2.0 is file-level copyleft: it applies only to the
library's own source files. Using the crates unmodified in a closed-source product is
allowed; if their files are ever modified, the modified files must be published under
MPL-2.0. Ship their licence text with binaries. Everything else is permissive and only
requires keeping its notice in a distribution.

Build tool (not linked): `wasm-bindgen-cli` 0.2.130 (MIT OR Apache-2.0), version pinned to
match the `wasm-bindgen` crate.

## Planned

- OCCT 8.0.1: LGPL 2.1 with exception. Link dynamically and ship its licence (phase E3).
