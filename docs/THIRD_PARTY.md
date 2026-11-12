# Third-party dependencies of the native engine

Required by `docs/OVERHAUL_PLAN.md` section 4: check every new dependency's licence
before adding it and record it here. Scope: crates under `crates/` (the throwaway
`spikes/` workspace is not shipped and is not listed). Check with
`cargo tree -p <crate> -e normal --prefix none -f "{p} {l}"`.

## oc-doc (E1)

| Crate | Version | Licence | Why |
|---|---|---|---|
| imbl | 6.1 | MPL-2.0+ | Persistent vector and ordered map (structural sharing for undo). |
| imbl-sized-chunks, bitmaps | 0.1, 3.2 | MPL-2.0+ | imbl internals. |
| archery | 1.2 | MIT | imbl internals (shared pointer abstraction). |
| rand_core, rand_xoshiro | 0.9, 0.7 | MIT OR Apache-2.0 | imbl internals. |
| serde, serde_derive, serde_core | 1.0 | MIT OR Apache-2.0 | Serialization framework. |
| serde_json | 1.0 | MIT OR Apache-2.0 | JSON parsing (`float_roundtrip` for exact numbers). |
| serde_path_to_error | 0.1 | MIT OR Apache-2.0 | JSON paths in schema errors. |
| ryu | 1.0 | Apache-2.0 OR BSL-1.0 | Shortest round-trip float digits (canonical JSON numbers). |
| itoa, zmij, memchr | 1.0, 1.0, 2.8 | MIT OR Apache-2.0 / MIT / Unlicense OR MIT | serde_json internals. |
| proc-macro2, quote, syn, unicode-ident | 1.0, 1.0, 3.0, 1.0 | MIT OR Apache-2.0 (unicode-ident also Unicode-3.0) | Build-time only (serde_derive). |
| proptest (dev only) | 1.12 | MIT OR Apache-2.0 | Property tests; not shipped. |

**MPL-2.0 note (imbl family).** MPL-2.0 is file-level copyleft: it applies only to the
library's own source files. Using the crates unmodified in a closed-source product is
allowed; if their files are ever modified, the modified files must be published under
MPL-2.0. Ship their licence text with binaries. No other copyleft licences so far.
