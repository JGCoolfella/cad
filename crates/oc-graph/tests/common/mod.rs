//! Shared test helpers: corpus documents, the phase-0 oracle and comparisons.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Arc;

use oc_doc::{Feature, PartDocument};
use oc_graph::{CancelToken, Rebuild, Regenerator};
use oc_kernel::Kernel;
use oc_kernel_occt::{Body, OcctKernel};
use serde_json::{json, Value};

pub const PARTS: [&str; 17] = [
    "block-hole",
    "curved-profiles",
    "draft",
    "fillet-chamfer",
    "holes",
    "imported",
    "pad-pocket",
    "patterns",
    "refgeom",
    "revolve",
    "rib",
    "shell-outward",
    "shell",
    "sweep-loft",
    "thin-contours",
    "through-mid-dir2",
    "up-to",
];

pub fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

pub fn load(part: &str) -> PartDocument {
    let text = std::fs::read_to_string(corpus_dir().join(format!("{part}.ocpart"))).unwrap();
    oc_doc::load(&text).unwrap_or_else(|e| panic!("{part}: {e:?}")).doc
}

pub fn features(doc: &PartDocument) -> Vec<Arc<Feature>> {
    doc.features.iter().cloned().collect()
}

pub fn oracle(part: &str) -> Value {
    let text = std::fs::read_to_string(corpus_dir().join(format!("{part}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

pub fn regen() -> Regenerator<OcctKernel> {
    Regenerator::new(OcctKernel::default())
}

pub fn rebuild(r: &mut Regenerator<OcctKernel>, doc: &PartDocument) -> Rebuild<Body> {
    r.regenerate(&features(doc), &CancelToken::new()).unwrap()
}

/// JavaScript's `Number(x.toPrecision(6))` with |x| < 1e-7 → 0, as the corpus records.
pub fn sig6(x: f64) -> f64 {
    if x.abs() < 1e-7 {
        return 0.0;
    }
    format!("{x:.5e}").parse().unwrap()
}

/// Body metrics in the corpus layout.
pub fn body_json(k: &OcctKernel, b: &Body) -> Value {
    let m = k.mass_properties(b).unwrap();
    let t = k.topology(b).unwrap();
    let bb = k.bounding_box(b).unwrap();
    json!({ "volume": m.volume, "area": m.area, "solids": t.solids, "faces": t.faces.len(), "edges": t.edges.len(), "bbox": [bb.min, bb.max] })
}

pub fn frame_json(f: &oc_graph::math::Frame) -> Value {
    json!({ "origin": f.origin, "x": f.x, "y": f.y, "normal": f.normal })
}

/// The reference geometry a step defines, in the corpus layout.
pub fn ref_json(r: &Rebuild<Body>, id: &str) -> Option<Value> {
    if let Some(f) = r.frames.get(id) {
        return Some(frame_json(f));
    }
    if let Some(f) = r.planes.get(id) {
        return Some(frame_json(f));
    }
    if let Some(a) = r.axes.get(id) {
        return Some(json!({ "p": a.p, "d": a.d }));
    }
    if let Some(p) = r.points.get(id) {
        return Some(json!(p));
    }
    r.csys.get(id).map(frame_json)
}

/// Compare `ours` with the oracle: integers exactly, reals to the oracle's precision (six
/// significant figures). Returns the differences as `path: ours vs oracle`, and counts
/// reals that agree within that precision but round to a different sixth figure.
pub fn compare(path: &str, ours: &Value, want: &Value, diffs: &mut Vec<String>, boundary: &mut usize) {
    match (ours, want) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, w) in b {
                match a.get(k) {
                    Some(o) => compare(&format!("{path}.{k}"), o, w, diffs, boundary),
                    None => diffs.push(format!("{path}.{k}: missing (oracle {w})")),
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (o, w)) in a.iter().zip(b).enumerate() {
                compare(&format!("{path}[{i}]"), o, w, diffs, boundary);
            }
        }
        (Value::Number(o), Value::Number(w)) => {
            let (o, w) = (o.as_f64().unwrap(), w.as_f64().unwrap());
            if sig6(o) == w {
                return;
            }
            // Rounding to six figures can flip the last digit for values that agree to 1e-6.
            if (o - w).abs() <= 6e-6 * w.abs() {
                *boundary += 1;
                return;
            }
            diffs.push(format!("{path}: {o} vs oracle {w}"));
        }
        _ => {
            if ours != want {
                diffs.push(format!("{path}: {ours} vs oracle {want}"));
            }
        }
    }
}

fn p3(v: &Value) -> [f64; 3] {
    [0, 1, 2].map(|i| v[i].as_f64().unwrap())
}

/// Axes are compared as lines: the direction to the oracle's precision, and our point on
/// the oracle's line. Which point of the axis is reported is a choice, not geometry: for a
/// cylindrical face the TypeScript engine takes replicad's first circular edge of the
/// face (an enumeration artefact), oc-graph the lowest-indexed one (section 8).
fn compare_axis(path: &str, ours: &Value, want: &Value, diffs: &mut Vec<String>, boundary: &mut usize) {
    compare(&format!("{path}.d"), &ours["d"], &want["d"], diffs, boundary);
    let (p, q, d) = (p3(&ours["p"]), p3(&want["p"]), p3(&want["d"]));
    let v = [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    let off = [v[1] * d[2] - v[2] * d[1], v[2] * d[0] - v[0] * d[2], v[0] * d[1] - v[1] * d[0]];
    let dist = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
    if dist > 1e-5 * (1.0 + q.iter().map(|x| x.abs()).fold(0.0, f64::max)) {
        diffs.push(format!("{path}.p: {p:?} is {dist} off the oracle axis through {q:?}"));
    }
}

/// Oracle values the native engine must not match, corrected to the right answer
/// (`docs/OVERHAUL_PLAN.md` section 8). Mid plane between two faces (`refgeom`), and, from
/// 0.2's "known divergence": in `through-mid-dir2`,
/// Boss-Extrude2 is Up To Body, which the TypeScript engine builds as Through All (to
/// z = -124.288). SOLIDWORKS stops at the body's top face (z = 10): the 10 x 10 mm profile
/// at z = 40 adds 10 x 10 x 30 mm. The oracle's extra prism below the body is
/// 114.288 mm long: 11428.8 mm³, 4 x 10 x 114.288 mm² of side faces (its 100 mm² bottom
/// replaces the 100 mm² it removed from the body's bottom face), 5 faces and 12 edges, and
/// it is in every later step.
pub fn corrected_oracle(part: &str) -> Value {
    let mut want = oracle(part);
    if part == "refgeom" {
        // Plane2 is the mid plane of the faces at z = 0 and z = 20: z = 10. The TypeScript
        // engine puts it on the first face (z = 0), a bug in its mid-plane formula.
        let s = want["steps"].as_array_mut().unwrap().iter_mut().find(|s| s["name"] == "Plane2").unwrap();
        assert_eq!(s["ref"]["origin"], json!([0, 0, 0]));
        s["ref"]["origin"] = json!([0, 0, 10]);
    }
    if part == "through-mid-dir2" {
        let mut after = false;
        for s in want["steps"].as_array_mut().unwrap() {
            after |= s["name"] == "Boss-Extrude2";
            let Some(b) = s.get_mut("body").filter(|_| after) else { continue };
            let sub = |b: &mut Value, k: &str, x: f64| b[k] = json!(sig6(b[k].as_f64().unwrap() - x));
            sub(b, "volume", 100.0 * 114.288);
            sub(b, "area", 4.0 * 10.0 * 114.288);
            b["faces"] = json!(b["faces"].as_u64().unwrap() - 5);
            b["edges"] = json!(b["edges"].as_u64().unwrap() - 12);
            b["bbox"][0][2] = json!(-10.0);
        }
    }
    want
}

/// Every difference between a rebuild and the (corrected) oracle, per step.
pub fn diff_part(k: &OcctKernel, part: &str, r: &Rebuild<Body>) -> (Vec<String>, usize) {
    let want = corrected_oracle(part);
    let steps = want["steps"].as_array().unwrap();
    let mut diffs = vec![];
    let mut boundary = 0;
    assert_eq!(steps.len(), r.steps.len(), "{part}: step count");
    for (w, s) in steps.iter().zip(&r.steps) {
        let name = w["name"].as_str().unwrap();
        let p = format!("{part}/{name}");
        assert_eq!(w["id"].as_str().unwrap(), s.status.id);
        if s.status.error.is_some() || s.status.warning.is_some() || w.get("error").is_some() || w.get("warning").is_some() {
            let st = |e: &Option<String>| e.clone().map_or(Value::Null, Value::String);
            let ours = json!({ "error": st(&s.status.error), "warning": st(&s.status.warning) });
            let theirs = json!({ "error": w.get("error").cloned().unwrap_or(Value::Null), "warning": w.get("warning").cloned().unwrap_or(Value::Null) });
            if ours != theirs {
                diffs.push(format!("{p}: status {ours} vs oracle {theirs}"));
            }
        }
        match (w.get("body"), &s.body) {
            (Some(b), Some(ours)) => compare(&format!("{p}.body"), &body_json(k, &ours.body), b, &mut diffs, &mut boundary),
            (None, None) => {}
            (b, o) => diffs.push(format!("{p}: body {} vs oracle {}", o.is_some(), b.is_some())),
        }
        match (w.get("ref"), ref_json(r, &s.status.id)) {
            (Some(x), Some(ours)) if x.get("d").is_some() => compare_axis(&format!("{p}.ref"), &ours, x, &mut diffs, &mut boundary),
            (Some(x), Some(ours)) => compare(&format!("{p}.ref"), &ours, x, &mut diffs, &mut boundary),
            (None, None) => {}
            (x, o) => diffs.push(format!("{p}: ref {o:?} vs oracle {x:?}")),
        }
    }
    (diffs, boundary)
}
