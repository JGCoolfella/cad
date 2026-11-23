//! The Emscripten program's link flags (W1b): an ES module the browser's workers (and Node, in
//! the checks) import, its exported entry points, and the runtime's memory and file system for
//! the JavaScript side. Natively the program is an empty stub (it only exists in the browser).
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("emscripten") {
        return;
    }
    let exports = [
        "_main", "_oc_buffer", "_oc_reply", "_oc_reply_len", "_oc_engine_start", "_oc_request", "_oc_wanted", "_oc_prepare", "_oc_hosts_started", "_oc_set_defer",
        "_oc_take_warm", "_oc_take_meshes", "_oc_host_start", "_oc_host_job", "_oc_identity",
    ];
    for f in [
        "-sMODULARIZE=1".to_string(),
        "-sEXPORT_ES6=1".into(),
        "-sEXPORT_NAME=createOcWeb".into(),
        "-sENVIRONMENT=web,worker,node".into(),
        format!("-sEXPORTED_FUNCTIONS={}", exports.join(",")),
        "-sEXPORTED_RUNTIME_METHODS=HEAPU8,FS".into(),
        "-sFORCE_FILESYSTEM=1".into(),
    ] {
        println!("cargo:rustc-link-arg-bin=oc-web={f}");
    }
}
