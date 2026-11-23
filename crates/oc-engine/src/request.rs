//! The engine behind one entry (OVERHAUL_PLAN W1b): a command as the webview invokes it
//! (Tauri's command name and its arguments, camelCase as the webview sends them) answered as
//! the desktop app's commands answer it: JSON, or the binary replies of `engine_rebuild`,
//! `engine_export` and `asm_build`. The browser's engine worker (`oc-web`) serves `invoke`
//! with it; the desktop app keeps its Tauri commands (`oc-app`'s `main.rs`), which take the
//! same arguments (tested there against [`COMMANDS`]).
//!
//! File paths are the engine's file system's: on the desktop the disk, in the browser an
//! in-memory file system whose `/work` holds the user's workspace (W1 design point 5).
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use oc_doc::{AssemblyDocument, EdgeSig, FaceSig, Feature, Id, MateEntity, Transform};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use crate::asm::{self, Overrides, Pick};
use crate::engine::{BytesReply, Engine, Frame, Job, MeasureRef, Reply};

/// The commands [`request`] serves: every `engine_*` and `asm_*` command of the desktop app
/// and its text file commands.
pub const COMMANDS: [&str; 19] = [
    "engine_rebuild",
    "engine_export",
    "engine_export_to",
    "engine_mass_props",
    "engine_measure",
    "engine_convert",
    "engine_clear_cache",
    "asm_open",
    "asm_build",
    "asm_place",
    "asm_mass",
    "asm_interference",
    "asm_measure",
    "asm_drag",
    "asm_entity",
    "asm_ref",
    "asm_save",
    "app_read_text",
    "app_write_text",
];

/// A request's answer.
#[derive(Debug)]
pub enum Answer {
    Json(Value),
    /// A binary reply (`crate::pack`'s layout, or an exported file).
    Bytes(Vec<u8>),
    /// The request needs parts regenerated elsewhere first ([`crate::hosts`]); run it again
    /// once they are prepared.
    Deferred,
}

fn args<T: DeserializeOwned>(cmd: &str, v: Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|e| format!("{cmd}: invalid arguments: {e}"))
}

fn arcs(v: Vec<Feature>) -> Vec<Arc<Feature>> {
    v.into_iter().map(Arc::new).collect()
}

fn path(p: Option<String>) -> Option<PathBuf> {
    p.filter(|p| !p.is_empty()).map(PathBuf::from)
}

fn ids(v: Vec<Vec<String>>) -> Vec<Vec<Id>> {
    v.into_iter().map(|p| p.iter().map(|x| Id::new(x)).collect()).collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rebuild {
    features: Vec<Feature>,
    preview: Option<Feature>,
    quality: f64,
    config: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Export {
    features: Vec<Feature>,
    format: String,
    quality: f64,
}

#[derive(Deserialize)]
struct Features {
    features: Vec<Feature>,
}

#[derive(Deserialize)]
struct Measure {
    features: Vec<Feature>,
    refs: Vec<MeasureRef>,
}

#[derive(Deserialize)]
struct Convert {
    features: Vec<Feature>,
    frame: Frame,
    edges: Vec<EdgeSig>,
    faces: Vec<FaceSig>,
}

#[derive(Deserialize)]
struct PathArg {
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Build {
    doc: Box<AssemblyDocument>,
    path: Option<String>,
    lightweight: bool,
    quality: f64,
    overrides: Option<Overrides>,
    probe: Option<bool>,
    base: Option<u64>,
}

#[derive(Deserialize)]
struct Place {
    instance: String,
    to: Transform,
}

#[derive(Deserialize)]
struct Quality {
    quality: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Interference {
    selected: Vec<Vec<String>>,
    coincidence: bool,
    subassemblies: bool,
    include_hidden: bool,
    excluded: Option<Vec<Vec<String>>>,
    quality: f64,
}

#[derive(Deserialize)]
struct AsmMeasure {
    picks: Vec<Pick>,
    quality: f64,
}

#[derive(Deserialize)]
struct Drag {
    instance: String,
    grab: [f64; 3],
    target: [f64; 3],
    lever: f64,
}

#[derive(Deserialize)]
struct Entity {
    part: String,
    kind: String,
    index: u32,
    quality: f64,
}

#[derive(Deserialize)]
struct Ref {
    path: Vec<String>,
    entity: Box<MateEntity>,
    quality: f64,
}

#[derive(Deserialize)]
struct Save {
    doc: Box<AssemblyDocument>,
    path: String,
    from: Option<String>,
    quality: f64,
    overrides: Option<Overrides>,
}

#[derive(Deserialize)]
struct WriteText {
    path: String,
    text: String,
}

/// What a job's reply comes back on.
enum Pending {
    Json(Receiver<Reply>),
    Bytes(Receiver<BytesReply>),
}

/// The job a command asks for.
fn job(cmd: &str, a: Value) -> Result<(Job, Pending), String> {
    let (tj, rj) = channel::<Reply>();
    let (tb, rb) = channel::<BytesReply>();
    let (json, bytes) = (Pending::Json(rj), Pending::Bytes(rb));
    Ok(match cmd {
        "engine_rebuild" => {
            let x: Rebuild = args(cmd, a)?;
            (Job::Rebuild { features: arcs(x.features), preview: x.preview.map(Arc::new), quality: x.quality, config: x.config, reply: tb }, bytes)
        }
        "engine_export" | "engine_export_to" => {
            let x: Export = args(cmd, a)?;
            (Job::Export { features: arcs(x.features), format: x.format, quality: x.quality, reply: tb }, bytes)
        }
        "engine_mass_props" => {
            let x: Features = args(cmd, a)?;
            (Job::MassProps { features: arcs(x.features), reply: tj }, json)
        }
        "engine_measure" => {
            let x: Measure = args(cmd, a)?;
            (Job::Measure { features: arcs(x.features), refs: x.refs, reply: tj }, json)
        }
        "engine_convert" => {
            let x: Convert = args(cmd, a)?;
            (Job::Convert { features: arcs(x.features), frame: x.frame, edges: x.edges, faces: x.faces, reply: tj }, json)
        }
        "asm_open" => {
            let x: PathArg = args(cmd, a)?;
            (Job::AsmOpen { path: PathBuf::from(x.path), reply: tj }, json)
        }
        "asm_build" => {
            let x: Build = args(cmd, a)?;
            // A probe (the SmartMates preview) leaves the last build as drags know it.
            let keep = !x.probe.unwrap_or(false);
            let job = Job::AsmBuild { doc: x.doc, path: path(x.path), lightweight: x.lightweight, quality: x.quality, overrides: x.overrides.unwrap_or_default(), keep, base: x.base, reply: tb };
            (job, bytes)
        }
        "asm_place" => {
            let x: Place = args(cmd, a)?;
            (Job::AsmPlace { instance: x.instance, to: x.to, reply: tj }, json)
        }
        "asm_mass" => {
            let x: Quality = args(cmd, a)?;
            (Job::AsmMass { quality: x.quality, reply: tj }, json)
        }
        "asm_interference" => {
            let x: Interference = args(cmd, a)?;
            let opts = oc_asm::InterferenceOptions {
                selected: ids(x.selected),
                coincidence: x.coincidence,
                subassemblies_as_components: x.subassemblies,
                include_hidden: x.include_hidden,
                excluded: ids(x.excluded.unwrap_or_default()),
            };
            (Job::AsmInterference { opts, quality: x.quality, reply: tj }, json)
        }
        "asm_measure" => {
            let x: AsmMeasure = args(cmd, a)?;
            (Job::AsmMeasure { picks: x.picks, quality: x.quality, reply: tj }, json)
        }
        "asm_drag" => {
            let x: Drag = args(cmd, a)?;
            (Job::AsmDrag { instance: x.instance, grab: x.grab, target: x.target, lever: x.lever, reply: tj }, json)
        }
        "asm_entity" => {
            let x: Entity = args(cmd, a)?;
            (Job::AsmEntity { part: x.part, kind: x.kind, index: x.index, quality: x.quality, reply: tj }, json)
        }
        "asm_ref" => {
            let x: Ref = args(cmd, a)?;
            (Job::AsmRef { path: x.path.iter().map(|p| Id::new(p)).collect(), entity: x.entity, quality: x.quality, reply: tj }, json)
        }
        "asm_save" => {
            let x: Save = args(cmd, a)?;
            (Job::AsmSave { doc: x.doc, path: PathBuf::from(x.path), from: path(x.from), quality: x.quality, overrides: x.overrides.unwrap_or_default(), reply: tj }, json)
        }
        _ => return Err(format!("unknown command {cmd}")),
    })
}

/// Answers `cmd` with arguments `a` (a JSON object, or null for none) on `e`.
pub fn request(e: &mut Engine, cmd: &str, a: Value) -> Result<Answer, String> {
    let a = if a.is_null() { Value::Object(Default::default()) } else { a };
    match cmd {
        "engine_clear_cache" => {
            e.run(Job::ClearCache);
            return Ok(Answer::Json(Value::Null));
        }
        "app_read_text" => {
            let x: PathArg = args(cmd, a)?;
            return asm::read_text(std::path::Path::new(&x.path)).map(Answer::Json);
        }
        "app_write_text" => {
            let x: WriteText = args(cmd, a)?;
            return asm::write_text(std::path::Path::new(&x.path), &x.text).map(Answer::Json);
        }
        _ => {}
    }
    // File > Export to a path (`engine_export_to`): written there, as the desktop app does.
    let to = match cmd {
        "engine_export_to" => Some(args::<PathArg>(cmd, a.clone())?.path),
        _ => None,
    };
    let (job, pending) = job(cmd, a)?;
    e.run(job);
    let answer = match pending {
        Pending::Json(rx) => rx.try_recv().map(|r| r.map(Answer::Json)),
        Pending::Bytes(rx) => rx.try_recv().map(|r| r.map(Answer::Bytes)),
    };
    match (answer, to) {
        (Ok(Ok(Answer::Bytes(b))), Some(path)) => {
            std::fs::write(&path, &b).map_err(|e| format!("{path}: {e}"))?;
            Ok(Answer::Json(Value::from(b.len() as u64)))
        }
        (Ok(r), _) => r,
        // No reply: the job was deferred (or failed in a way that dropped it).
        (Err(_), _) if e.deferred() => Ok(Answer::Deferred),
        (Err(_), _) => Err(format!("{cmd}: the engine gave no answer")),
    }
}
