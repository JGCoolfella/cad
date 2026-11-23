//! OpenCAD's engine in the browser (OVERHAUL_PLAN W1b): one Emscripten program that runs in
//! two kinds of Web Worker (`src/web/`), with OCCT in WebAssembly (W1a: its side module
//! `libocct.so`, or linked in with `OC_WASM_LINK=static`).
//!
//! - **The engine worker** ([`web::oc_engine_start`], [`web::oc_request`]): `oc-engine`'s jobs
//!   behind one request entry, the webview's commands by name with their JSON arguments, the
//!   desktop app's replies (JSON or its binary layouts). Requests run one at a time. A request
//!   whose parts go to host workers is deferred ([`oc_engine::hosts`]): the worker fetches
//!   the wanted jobs ([`web::oc_wanted`]), has them regenerated, hands the results over
//!   ([`web::oc_prepare`]) and sends the request again.
//! - **A geometry host worker** ([`web::oc_host_start`], [`web::oc_host_job`]): one part job
//!   in, one `oc_host::proto` reply out, as a host process answers a frame. A trap or OCCT
//!   abort ends the worker (the pool starts another); nothing survives between jobs anyway.
//!
//! Data crosses through two buffers owned here: the worker writes a request or job into the
//! input buffer ([`web::oc_buffer`]) and copies the reply out ([`web::oc_reply`],
//! [`web::oc_reply_len`]). The engine's file system is Emscripten's in memory: `/work` (the
//! working folder) holds the user's workspace, which the browser mirrors (W1e).
//!
//! Natively this program does nothing: it is built for `wasm32-unknown-emscripten` only
//! (`node scripts/wasm.mjs web`).
#[cfg(target_os = "emscripten")]
mod web;

fn main() {
    #[cfg(target_os = "emscripten")]
    web::start();
}
