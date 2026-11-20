//! What the app is doing, for the stuck-state dump of a check run that gets no report
//! (`finish_smoke` in main.rs): the commands the webview is waiting for, the engine's job,
//! what the render thread is doing and how many frames it has drawn, and the last events of
//! the run (app_ready, the script's evaluation, its report).
//!
//! Always on: a few atomics and short locks per command, frame or job.
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

fn ms() -> u64 {
    START.elapsed().as_millis() as u64
}

/// Milliseconds since the app started (call early so the clock starts with the app).
pub fn now() -> u64 {
    ms()
}

const EVENTS: usize = 64;

static LOG: Mutex<VecDeque<(u64, String)>> = Mutex::new(VecDeque::new());
static CALLS: LazyLock<Mutex<BTreeMap<u64, (&'static str, u64)>>> = LazyLock::new(Default::default);
static NEXT: AtomicU64 = AtomicU64::new(1);
static ENGINE: Mutex<Option<(&'static str, u64)>> = Mutex::new(None);
static ENGINE_JOBS: AtomicU64 = AtomicU64::new(0);

/// The render thread: what it is doing since when, frames drawn, loop turns.
static VIEW_PHASE: Mutex<(&'static str, u64)> = Mutex::new(("starting", 0));
static VIEW_FRAMES: AtomicU64 = AtomicU64::new(0);
static VIEW_SEQ: AtomicU64 = AtomicU64::new(0);
static VIEW_LOOPS: AtomicU64 = AtomicU64::new(0);
static VIEW_QUERIES: AtomicU64 = AtomicU64::new(0);

/// Records an event of the run (kept: the last 64).
pub fn event(what: impl Into<String>) {
    let mut l = LOG.lock().expect("diag log");
    if l.len() == EVENTS {
        l.pop_front();
    }
    l.push_back((ms(), what.into()));
}

/// A command the webview waits for, from now until the guard is dropped.
pub struct Call(u64);

pub fn call(name: &'static str) -> Call {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    CALLS.lock().expect("diag calls").insert(id, (name, ms()));
    Call(id)
}

impl Drop for Call {
    fn drop(&mut self) {
        CALLS.lock().expect("diag calls").remove(&self.0);
    }
}

/// The engine thread starts (`Some`) or ends (`None`) a job.
pub fn engine(job: Option<&'static str>) {
    if job.is_some() {
        ENGINE_JOBS.fetch_add(1, Ordering::Relaxed);
    }
    *ENGINE.lock().expect("diag engine") = job.map(|j| (j, ms()));
}

/// The render thread enters a phase of its loop.
pub fn view(phase: &'static str) {
    *VIEW_PHASE.lock().expect("diag view") = (phase, ms());
}

pub fn view_loop(queries: usize) {
    VIEW_LOOPS.fetch_add(1, Ordering::Relaxed);
    VIEW_QUERIES.store(queries as u64, Ordering::Relaxed);
}

pub fn view_drawn(seq: u32) {
    VIEW_FRAMES.fetch_add(1, Ordering::Relaxed);
    VIEW_SEQ.store(seq as u64, Ordering::Relaxed);
}

/// What the render thread is doing, for error messages: `present for 5200 ms, 31 frames drawn`.
pub fn view_state() -> String {
    let (phase, since) = *VIEW_PHASE.lock().expect("diag view");
    format!("{phase} for {} ms, {} frames drawn", ms() - since, VIEW_FRAMES.load(Ordering::Relaxed))
}

/// The state of the native side, ages in ms.
pub fn dump() -> Value {
    let t = ms();
    let calls: Vec<Value> = CALLS
        .lock()
        .expect("diag calls")
        .values()
        .map(|(n, s)| json!({ "cmd": n, "ageMs": t - s }))
        .collect();
    let engine = ENGINE.lock().expect("diag engine").map(|(j, s)| json!({ "job": j, "ageMs": t - s }));
    let (phase, since) = *VIEW_PHASE.lock().expect("diag view");
    let events: Vec<Value> = LOG.lock().expect("diag log").iter().map(|(s, e)| json!([s, e])).collect();
    json!({
        "atMs": t,
        "pendingCalls": calls,
        "engine": { "job": engine, "jobsRun": ENGINE_JOBS.load(Ordering::Relaxed) },
        "view": {
            "phase": phase,
            "phaseAgeMs": t - since,
            "framesDrawn": VIEW_FRAMES.load(Ordering::Relaxed),
            "lastSeq": VIEW_SEQ.load(Ordering::Relaxed),
            "loops": VIEW_LOOPS.load(Ordering::Relaxed),
            "queriesWaiting": VIEW_QUERIES.load(Ordering::Relaxed),
        },
        "events": events,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_tracked_until_dropped() {
        let g = call("engine_rebuild");
        event("x");
        let d = dump();
        assert!(d["pendingCalls"].as_array().unwrap().iter().any(|c| c["cmd"] == "engine_rebuild"));
        drop(g);
        let d = dump();
        assert!(!d["pendingCalls"].as_array().unwrap().iter().any(|c| c["cmd"] == "engine_rebuild"));
        assert!(d["events"].as_array().unwrap().iter().any(|e| e[1] == "x"));
    }
}
