//! The exported entry points (see the crate's notes). Every function is called by the worker's
//! JavaScript, one at a time, on the worker's one thread.
//
// `#[no_mangle]` is what exports a function from the program (the workspace denies unsafe code
// and counts it as such); the functions themselves do nothing unsafe: buffers are handed out
// as addresses, which the JavaScript side reads and writes in the module's memory.
#![allow(unsafe_code)]
use std::cell::RefCell;
use std::sync::Arc;

use oc_asm::Assembler;
use oc_engine::engine::{Engine, MeshStore};
use oc_engine::hosts::Prepared;
use oc_engine::request::{request, Answer};
use oc_kernel_occt::OcctKernel;

/// The engine's working folder: the user's workspace (W1 design point 5).
pub const WORK: &str = "/work";

/// What [`oc_request`] answered.
const JSON: u32 = 0;
const BYTES: u32 = 1;
const ERROR: u32 = 2;
const DEFERRED: u32 = 3;

struct EngineState {
    engine: Engine,
    hosts: Option<Prepared>,
}

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static REPLY: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ENGINE: RefCell<Option<EngineState>> = const { RefCell::new(None) };
    static HOST: RefCell<Option<(OcctKernel, bool)>> = const { RefCell::new(None) };
}

/// The program's start: the working folder.
pub fn start() {
    let _ = std::fs::create_dir_all(WORK);
    let _ = std::env::set_current_dir(WORK);
}

fn reply(b: Vec<u8>) {
    REPLY.with(|r| *r.borrow_mut() = b);
}

fn input() -> Vec<u8> {
    INPUT.with(|b| std::mem::take(&mut *b.borrow_mut()))
}

/// The input buffer, `len` bytes, for the next call's request or job.
#[no_mangle]
pub extern "C" fn oc_buffer(len: usize) -> *mut u8 {
    INPUT.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        b.resize(len, 0);
        b.as_mut_ptr()
    })
}

/// The last reply: its address and length.
#[no_mangle]
pub extern "C" fn oc_reply() -> *const u8 {
    REPLY.with(|r| r.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn oc_reply_len() -> usize {
    REPLY.with(|r| r.borrow().len())
}

/// What this program is: the kernel's identity (the caches' key) and OCCT's linkage, as JSON.
#[no_mangle]
pub extern "C" fn oc_identity() {
    use oc_kernel::Kernel;
    let k = OcctKernel::default();
    let v = serde_json::json!({ "kernel": k.identity(), "occt": format!("{} {}", oc_kernel_occt::OCCT_VERSION, oc_kernel_occt::OCCT_LINKAGE), "app": oc_asm::cache::APP_VERSION });
    reply(v.to_string().into_bytes());
}

/// Makes this worker the engine: `hosts` geometry host workers regenerate several parts at once
/// (0: the engine regenerates every part itself, one by one); bodies are kept within an
/// estimated `budget_mb` MB (as the desktop app's 2 GB, G3c; 0: no limit).
#[no_mangle]
pub extern "C" fn oc_engine_start(hosts: u32, budget_mb: u32) {
    let mut asm = Assembler::new(OcctKernel::default());
    asm.workers = 1;
    if budget_mb > 0 {
        asm.body_budget = Some((budget_mb as usize) << 20);
    }
    let prepared = (hosts > 0).then(|| Prepared::new(hosts as usize));
    if let Some(p) = &prepared {
        asm.runner = Some(Box::new(p.clone()));
    }
    let engine = Engine::new(asm, Arc::new(MeshStore::default()), prepared.as_ref().map(Prepared::stats));
    ENGINE.with(|e| *e.borrow_mut() = Some(EngineState { engine, hosts: prepared }));
}

fn with_engine<T>(f: impl FnOnce(&mut EngineState) -> T) -> Option<T> {
    ENGINE.with(|e| e.borrow_mut().as_mut().map(f))
}

/// Answers the request in the input buffer: the command's name (`cmd_len` bytes) followed by
/// its arguments as JSON (none: no arguments). Returns what the reply is: 0 JSON, 1 binary,
/// 2 an error message, 3 deferred (fetch the wanted parts with [`oc_wanted`]).
#[no_mangle]
pub extern "C" fn oc_request(cmd_len: usize) -> u32 {
    let b = input();
    let (cmd, args) = b.split_at(cmd_len.min(b.len()));
    let cmd = String::from_utf8_lossy(cmd);
    let args = if args.is_empty() { Ok(serde_json::Value::Null) } else { serde_json::from_slice(args).map_err(|e| format!("{cmd}: arguments are not JSON: {e}")) };
    let r = with_engine(|s| {
        let r = args.and_then(|a| request(&mut s.engine, &cmd, a));
        if !matches!(r, Ok(Answer::Deferred)) {
            if let Some(h) = &s.hosts {
                h.finish();
            }
        }
        r
    })
    .unwrap_or_else(|| Err("the engine is not started".into()));
    let (kind, bytes) = match r {
        Ok(Answer::Json(v)) => (JSON, serde_json::to_vec(&v).unwrap_or_default()),
        Ok(Answer::Bytes(b)) => (BYTES, b),
        Ok(Answer::Deferred) => (DEFERRED, vec![]),
        Err(e) => (ERROR, e.into_bytes()),
    };
    reply(bytes);
    kind
}

/// The parts the deferred request wants: their count; the reply is each job (`oc_host::proto`'s
/// Job payload, its id its index) as a `u32` little-endian length and the bytes. An empty job
/// needs no host (it could not be written; the engine regenerates that part itself).
#[no_mangle]
pub extern "C" fn oc_wanted() -> u32 {
    let jobs = with_engine(|s| s.hosts.as_ref().map(Prepared::wanted).unwrap_or_default()).unwrap_or_default();
    let mut out = Vec::with_capacity(jobs.iter().map(|j| j.len() + 4).sum());
    for j in &jobs {
        out.extend_from_slice(&(j.len() as u32).to_le_bytes());
        out.extend_from_slice(j);
    }
    reply(out);
    jobs.len() as u32
}

/// Hands over wanted part `id`'s result, the input buffer its payload (`oc_engine::hosts`'
/// kinds: a host's Done or Failed reply, or the worker died or timed out). 0, or 1 with the
/// error in the reply.
#[no_mangle]
pub extern "C" fn oc_prepare(id: u32, kind: u32) -> u32 {
    let payload = input();
    let r = with_engine(|s| s.hosts.as_ref().map(|h| h.prepare(id as u64, kind, &payload)).unwrap_or_else(|| Err("no host workers".into()))).unwrap_or_else(|| Err("the engine is not started".into()));
    match r {
        Ok(()) => 0,
        Err(e) => {
            reply(e.into_bytes());
            1
        }
    }
}

/// A host worker was started (the counts each assembly build reports).
#[no_mangle]
pub extern "C" fn oc_hosts_started() {
    with_engine(|s| s.hosts.as_ref().map(Prepared::started));
}

/// Whether requests are deferred to host workers (0: the engine regenerates every part itself;
/// when no host worker can be started).
#[no_mangle]
pub extern "C" fn oc_set_defer(on: u32) {
    with_engine(|s| s.hosts.as_ref().map(|h| h.set_defer(on != 0)));
}

/// 1 if the engine asked for the host workers to be started (an assembly opened with parts to
/// regenerate) since the last call.
#[no_mangle]
pub extern "C" fn oc_take_warm() -> u32 {
    with_engine(|s| s.hosts.as_ref().is_some_and(Prepared::take_warm)).unwrap_or(false) as u32
}

/// Makes this worker a geometry host. `faults`: test faults on (`oc_host::serve::FAULTS`).
#[no_mangle]
pub extern "C" fn oc_host_start(faults: u32) {
    oc_kernel_occt::quiet_messages();
    HOST.with(|h| *h.borrow_mut() = Some((OcctKernel::default(), faults != 0)));
}

/// Regenerates the part job in the input buffer (`oc_host::proto`'s Job payload): returns the
/// reply's kind (`proto::DONE` or `proto::FAILED`), the reply its payload.
#[no_mangle]
pub extern "C" fn oc_host_job() -> u32 {
    let job = input();
    let (kind, payload) = HOST.with(|h| match h.borrow().as_ref() {
        Some((k, faults)) => oc_host::serve::answer(k, &job, *faults),
        None => (oc_host::proto::FAILED, oc_host::proto::encode_failed(0, "the host is not started")),
    });
    reply(payload);
    kind
}
