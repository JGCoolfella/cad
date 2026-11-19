//! Configurations of a part (G1e): the document as a configuration has it.
//!
//! `oc_doc::config` writes a configuration's values, suppression and stored sketch
//! solutions into the features; this re-solves the sketches whose dimensions changed, with
//! `oc-sketch`, as the app's Modify box does. A configuration's own stored solution is
//! written first, so a sketch that already satisfies its dimensions does not move (the
//! solver takes no step), and activating a configuration again gives the same document
//! bit for bit: its regeneration is a cache hit ([`crate::Regenerator::pin`]).

use oc_doc::config::{apply_part_values, capture_part};
use oc_doc::PartDocument;

/// The document with configuration `id` (an id or a name) active: the active one is
/// captured first, then `id`'s entries are written and its sketches re-solved.
pub fn activate(doc: &PartDocument, id: &str) -> Result<PartDocument, String> {
    let mut d = doc.clone();
    capture_part(&mut d);
    let touched = apply_part_values(&mut d, id)?;
    for fid in touched {
        if let Some(sk) = d.feature_mut(&fid).and_then(|f| f.sketch_mut()) {
            settle(sk);
        }
    }
    Ok(d)
}

/// How far a solve may move a sketch that already satisfies its dimensions (solver noise).
const SETTLED: f64 = 1e-9;

/// Re-solves a sketch whose dimensions or stored solution were written. When the solution
/// moves nothing by more than solver noise (the configuration's own solution, still valid),
/// the written geometry is kept, so the document is the same bit for bit
/// (`settle` in `src/core/configurations.ts`).
fn settle(sk: &mut oc_doc::SketchData) {
    let mut trial = sk.clone();
    crate::sketch::refs::solve(&mut trial);
    let mut moved: f64 = 0.0;
    for (k, p) in &trial.points {
        if let Some(q) = sk.points.get(k) {
            moved = moved.max((p.x - q.x).abs()).max((p.y - q.y).abs());
        }
    }
    for (k, e) in &trial.entities {
        if let (oc_doc::SketchEntity::Circle { r, .. }, Some(oc_doc::SketchEntity::Circle { r: q, .. })) = (e, sk.entities.get(k)) {
            moved = moved.max((r - q).abs());
        }
    }
    if moved > SETTLED {
        *sk = trial;
    }
}

/// The part as configuration `name` has it, for an assembly that references it: the
/// document itself when that configuration is active (or the part has none and `name` is
/// `Default`), else [`activate`]. `Err` when the part has no such configuration.
pub fn configured(doc: &PartDocument, name: &str) -> Result<PartDocument, String> {
    let Some(cfgs) = doc.configurations.as_deref() else {
        return if name == oc_doc::DEFAULT_CONFIGURATION || name.is_empty() { Ok(doc.clone()) } else { Err(format!("the part has no configuration {name:?}")) };
    };
    let c = cfgs.find(name).ok_or_else(|| format!("the part has no configuration {name:?}"))?;
    if c.id == cfgs.active {
        return Ok(doc.clone());
    }
    activate(doc, c.id.as_str())
}
