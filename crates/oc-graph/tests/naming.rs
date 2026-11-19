//! Persistent naming end to end (work package E6, `docs/OVERHAUL_PLAN.md` section 3):
//! the torture scripts of phase 0.3 (`corpus/torture/*.json`) replayed on the native
//! engine, no face without a traceable origin anywhere in the corpus, and the write-back
//! of names into legacy documents.
mod common;

use std::collections::BTreeMap;

use common::*;
use oc_doc::{History, PartDocument, RefSlot};
use oc_graph::{apply_heals, Rebuild, Regenerator};
use oc_kernel_occt::{Body, OcctKernel};
use oc_naming::signature::surface_name;
use serde_json::Value;

const CASES: [&str; 9] = [
    "TT1a-notch-base",
    "TT1b-taller-pad",
    "TT2-fillet-follows-corner",
    "TT3-shell-keeps-top-open",
    "TT4-pocket-fillets-follow",
    "TT5-deleted-boss",
    "TT6-determinism",
    "TT7-legacy-document",
    "TT8-unrelated-upstream-feature",
];

fn case(name: &str) -> Value {
    let text = std::fs::read_to_string(corpus_dir().join(format!("torture/{name}.json"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn doc_of(v: &Value) -> PartDocument {
    oc_doc::load(&v.to_string()).unwrap_or_else(|e| panic!("{e:?}")).doc
}

/// One torture edit: `{op: set|remove|insert, path, value}` on a JSON document
/// (`applyEdits` in `src/kernel/fixtures/torture.ts`).
fn apply_edit(doc: &mut Value, edit: &Value) {
    let op = edit["op"].as_str().unwrap();
    let path = edit["path"].as_array().unwrap();
    let (last, parents) = path.split_last().unwrap();
    let mut target = doc;
    for k in parents {
        target = match k {
            Value::String(s) => &mut target[s.as_str()],
            Value::Number(n) => &mut target[n.as_u64().unwrap() as usize],
            _ => panic!("bad path {path:?}"),
        };
    }
    match (target, last) {
        (Value::Array(a), Value::Number(n)) => {
            let i = n.as_u64().unwrap() as usize;
            match op {
                "remove" => drop(a.remove(i)),
                "insert" => a.insert(i, edit["value"].clone()),
                _ => a[i] = edit["value"].clone(),
            }
        }
        (Value::Object(m), Value::String(k)) => match op {
            "remove" => drop(m.shift_remove(k)),
            _ => drop(m.insert(k.clone(), edit["value"].clone())),
        },
        (t, k) => panic!("cannot apply {op} at {k} of {t}"),
    }
}

fn edited(doc: &PartDocument, edits: &Value) -> PartDocument {
    let mut v: Value = serde_json::from_str(&oc_doc::save(doc).unwrap()).unwrap();
    for e in edits.as_array().unwrap() {
        apply_edit(&mut v, e);
    }
    doc_of(&v)
}

/// The document as the app has it after opening and rebuilding it: the names the first
/// rebuild found for its legacy references are written back, without an undo step and
/// without making the document dirty (section 3.3 step 4).
fn open(r: &mut Regenerator<OcctKernel>, doc: PartDocument) -> (PartDocument, usize) {
    let mut h = History::new(doc);
    let first = r.regenerate_doc(&h.doc().clone(), &oc_graph::CancelToken::new()).unwrap();
    assert_eq!(first.untracked, 0);
    let mut n = 0;
    h.heal(|d| n = apply_heals(d, &first.heals));
    assert!(!h.is_dirty() && !h.can_undo(), "writing names back is not an edit");
    ((**h.doc()).clone(), n)
}

/// References to model topology without a name.
fn legacy_refs(doc: &PartDocument) -> usize {
    let mut n = 0;
    for f in doc.features.iter() {
        let mut f = (**f).clone();
        f.refs_mut(&mut |slot| {
            n += match slot {
                RefSlot::Face(s) => s.name.is_none() as usize,
                RefSlot::Edge(s) => s.name.is_none() as usize,
                RefSlot::Vertex { name, .. } => name.is_none() as usize,
            }
        });
    }
    n
}

/// A face as the scripts key it: surface type and centre rounded to 1e-3 mm.
fn face_key(surface: &str, center: [f64; 3]) -> String {
    let round = |x: f64| {
        let v = (x * 1000.0).round() / 1000.0;
        if v == 0.0 {
            0.0
        } else {
            v
        }
    };
    format!("{surface} {:?}", center.map(round))
}

/// Final faces as sorted keys (`faceKeys`).
fn face_keys(k: &OcctKernel, r: &Rebuild<Body>) -> Vec<String> {
    let Some(b) = &r.body else { return vec![] };
    let mut keys: Vec<String> = b.topo(k).unwrap().faces.iter().map(|f| face_key(surface_name(&f.surface), f.centroid)).collect();
    keys.sort();
    keys
}

/// `closeDeep` of `torture.ts`: both sides are measured values rounded to six significant
/// figures, equal within 1e-6 relative or absolute. Ours are rounded here.
fn close_deep(path: &str, ours: &Value, want: &Value, why: &mut Vec<String>) {
    match (ours, want) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, w) in b {
                match a.get(k) {
                    Some(o) => close_deep(&format!("{path}.{k}"), o, w, why),
                    None => why.push(format!("{path}.{k}: missing (reference {w})")),
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (o, w)) in a.iter().zip(b).enumerate() {
                close_deep(&format!("{path}[{i}]"), o, w, why);
            }
        }
        (Value::Number(o), Value::Number(w)) => {
            let (o, w) = (sig6(o.as_f64().unwrap()), w.as_f64().unwrap());
            if (o - w).abs() > 1e-6f64.max(1e-6 * o.abs().max(w.abs())) {
                why.push(format!("{path}: {o} vs reference {w}"));
            }
        }
        _ => {
            if ours != want {
                why.push(format!("{path}: {ours} vs reference {want}"));
            }
        }
    }
}

/// The reference's face list, with one representation corrected. The TypeScript engine
/// builds a drafted extrusion as a ruled loft, whose flat sides are B-spline surfaces;
/// the native kernel drafts a prism of a polygon and its sides are planes (E3 finding 2,
/// accepted in the E3 review). Same faces, same centres; only the surface type differs.
fn reference_faces(name: &str, want: &Value) -> Vec<String> {
    let mut keys: Vec<String> = want
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            let mut surface = f["surface"].as_str().unwrap();
            if name == "TT3-shell-keeps-top-open" && surface == "BSPLINE_SURFACE" {
                surface = "PLANE";
            }
            face_key(surface, [0, 1, 2].map(|i| f["center"][i].as_f64().unwrap()))
        })
        .collect();
    keys.sort();
    keys
}

/// Why a rebuild misses the case's expectations (empty = passes), as `evaluate` in
/// `torture.ts` decides it: per-step error, body metrics and reference geometry against
/// the reference built directly in the final state, the final face list, and the errors
/// the case demands.
fn misses(name: &str, k: &OcctKernel, expect: &Value, r: &Rebuild<Body>) -> Vec<String> {
    let mut why = vec![];
    if let Some(reference) = expect.get("reference") {
        for want in reference["steps"].as_array().unwrap() {
            let id = want["id"].as_str().unwrap();
            let label = want["name"].as_str().unwrap();
            let Some(s) = r.steps.iter().find(|s| s.status.id == id) else {
                why.push(format!("{label}: missing"));
                continue;
            };
            let (got, wanted) = (s.status.error.clone().map_or(Value::Null, Value::String), want.get("error").cloned().unwrap_or(Value::Null));
            if got != wanted {
                why.push(format!("{label}: error {got}, want {wanted}"));
            }
            match (want.get("body"), &s.body) {
                (Some(b), Some(ours)) => close_deep(&format!("{label}.body"), &body_json(k, &ours.body), b, &mut why),
                (None, None) => {}
                (b, o) => why.push(format!("{label}: body {} vs reference {}", o.is_some(), b.is_some())),
            }
            match (want.get("ref"), ref_json(r, id)) {
                (Some(x), Some(ours)) => close_deep(&format!("{label}.ref"), &ours, x, &mut why),
                (None, None) => {}
                (x, o) => why.push(format!("{label}: reference geometry {o:?}, want {x:?}")),
            }
        }
    }
    if let Some(faces) = expect.get("referenceFaces") {
        let (got, want) = (face_keys(k, r), reference_faces(name, faces));
        if got != want {
            let missing: Vec<&String> = want.iter().filter(|w| !got.contains(w)).collect();
            let extra: Vec<&String> = got.iter().filter(|g| !want.contains(g)).collect();
            why.push(format!("final faces differ from the reference: missing {missing:?}; unexpected {extra:?}"));
        }
    }
    for e in expect.get("errors").and_then(Value::as_array).into_iter().flatten() {
        let (id, pattern) = (e["id"].as_str().unwrap(), e["match"].as_str().unwrap());
        let error = r.steps.iter().find(|s| s.status.id == id).and_then(|s| s.status.error.clone());
        // The scripts use two patterns: "." (any error) and a plain phrase, case-insensitive.
        let ok = error.as_ref().is_some_and(|m| pattern == "." || m.to_lowercase().contains(&pattern.to_lowercase()));
        if !ok {
            why.push(format!("{id}: error {error:?}, want /{pattern}/"));
        }
    }
    why
}

/// Everything about a rebuild that must not vary: statuses, cache keys, metrics, every
/// face's name and signature, every edge's signature.
fn fingerprint(k: &OcctKernel, r: &Rebuild<Body>) -> Vec<String> {
    let mut out = vec![];
    for s in &r.steps {
        out.push(format!("{:?} {:?}", s.status, s.key));
        let Some(b) = &s.body else { continue };
        let t = b.topo(k).unwrap();
        out.push(body_json(k, &b.body).to_string());
        assert_eq!(b.names.faces.len(), t.faces.len());
        for (f, n) in t.faces.iter().zip(&b.names.faces) {
            out.push(format!("{n:?} {} {:?} {:?} {}", surface_name(&f.surface), f.centroid, f.normal, f.area));
        }
        for e in &t.edges {
            out.push(format!("{:?} {:?} {:?} {:?} {}", e.curve, e.start, e.end, e.mid, e.length));
        }
    }
    out
}

#[test]
fn torture_scripts_pass() {
    let mut report = vec![];
    let mut failures = vec![];
    for name in CASES {
        let c = case(name);
        let expect = &c["expect"];
        let mut r = regen();
        // As in the app: the document is built first, then edited.
        let (doc, healed) = open(&mut r, doc_of(&c["doc"]));
        let doc = edited(&doc, &c["edits"]);
        let reb = rebuild(&mut r, &doc);
        let mut why = misses(name, r.kernel(), expect, &reb);
        if reb.untracked != 0 {
            why.push(format!("{} faces without history", reb.untracked));
        }
        if expect.get("deterministic").is_some() {
            // Cold, warm, and cold again on another regenerator: identical in everything.
            let again = rebuild(&mut r, &doc);
            let warm = fingerprint(r.kernel(), &again);
            let mut other = regen();
            let again = rebuild(&mut other, &doc);
            let cold = fingerprint(other.kernel(), &again);
            let first = fingerprint(r.kernel(), &reb);
            if first != warm || first != cold {
                why.push("rebuilds differ".into());
            }
            assert!(first.len() > 500, "the fingerprint covers the part ({} lines)", first.len());
        }
        if let Some(part) = expect.get("corpus").and_then(Value::as_str) {
            assert_eq!(c["doc"]["version"], 1, "a legacy document");
            assert!(healed > 0, "its references were named");
            why.extend(diff_part(r.kernel(), part, &reb).0);
            // Rebuilt from its names alone, cold, it is the same part.
            let mut other = regen();
            let again = rebuild(&mut other, &doc);
            why.extend(diff_part(other.kernel(), part, &again).0);
            assert!(again.heals.is_empty());
        }
        let warnings: Vec<String> = reb.steps.iter().filter_map(|s| s.status.warning.clone().map(|w| format!("{}: {w}", s.status.id))).collect();
        // What the same edit does to the document as the TypeScript app has it: references
        // without names, found by nearest signature.
        let mut plain = regen();
        let legacy = edited(&doc_of(&c["doc"]), &c["edits"]);
        let legacy_rebuild = rebuild(&mut plain, &legacy);
        let by_signature = misses(name, plain.kernel(), expect, &legacy_rebuild).is_empty();
        report.push((name, why.is_empty(), by_signature, c["tsEngine"].as_str().unwrap().to_string(), healed, warnings.clone()));
        eprintln!("{name}: {} (by signature alone: {}; TypeScript engine: {}); {healed} features healed; warnings {warnings:?}", if why.is_empty() { "passes" } else { "FAILS" }, if by_signature { "passes" } else { "fails" }, c["tsEngine"]);
        for w in &why {
            eprintln!("    {w}");
        }
        if !why.is_empty() {
            failures.push(format!("{name}:\n  {}", why.join("\n  ")));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Names are what makes them pass: the cases the TypeScript engine fails also fail here
    // when references are found by signature alone.
    let by_signature: BTreeMap<&str, bool> = report.iter().map(|r| (r.0, r.2)).collect();
    for name in ["TT2-fillet-follows-corner", "TT3-shell-keeps-top-open", "TT4-pocket-fillets-follow", "TT5-deleted-boss"] {
        assert!(!by_signature[name], "{name} passes without names: the case no longer tests naming");
    }
    // No case warns. TT1a replaces every entity of the base sketch, so the cap's key (a
    // hash of the loop's entity ids, section 3.1) changes; the pad still has exactly one
    // end cap, and the sketch on it is found as that face (section 3.3 step 2a), not by
    // position.
    let warned: Vec<(&str, &Vec<String>)> = report.iter().filter(|r| !r.5.is_empty()).map(|r| (r.0, &r.5)).collect();
    assert!(warned.is_empty(), "{warned:?}");
}

#[test]
fn a_redrawn_profile_keeps_the_sketch_on_its_end_cap_and_renames_the_reference() {
    // TT1a in detail: after the base sketch is replaced, the pocket sketch's stored name
    // (`f2/end/<old key>`) matches no face. The one `f2/end/...` face stands in, without a
    // warning, and the rebuild reports the reference's new name for write-back.
    let c = case("TT1a-notch-base");
    let mut r = regen();
    let (doc, _) = open(&mut r, doc_of(&c["doc"]));
    let plane_name = |doc: &PartDocument| match &doc.feature("f3").unwrap().kind {
        oc_doc::FeatureKind::Sketch(s) => match &s.plane {
            oc_doc::PlaneRef::Face { sig } => sig.name.clone().unwrap(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    let old = plane_name(&doc);
    assert!(old.len() == 1 && old[0].starts_with("f2/end/"), "{old:?}");
    let mut doc = edited(&doc, &c["edits"]);
    let reb = rebuild(&mut r, &doc);
    assert!(reb.steps.iter().all(|s| s.status.error.is_none() && s.status.warning.is_none()), "{:?}", reb.statuses());
    assert_eq!(reb.heals.len(), 1);
    assert_eq!(apply_heals(&mut doc, &reb.heals), 1);
    let new = plane_name(&doc);
    assert!(new != old && new.len() == 1 && new[0].starts_with("f2/end/"), "{new:?}");
    // The renamed document is a cache hit and has nothing more to write back.
    let again = rebuild(&mut r, &doc);
    assert!(again.steps.iter().all(|s| s.cached) && again.heals.is_empty());
    // With a second boss from the same sketch region set (two end caps of f2) there would
    // be no single stand-in; that case is covered in `oc-naming/tests/resolve.rs`.
}

#[test]
fn the_corpus_has_no_face_without_a_traceable_origin() {
    let (mut faces, mut steps, mut imported, mut roles) = (0, 0, 0, BTreeMap::<String, usize>::new());
    let mut remaining = vec![];
    for part in PARTS {
        let mut r = regen();
        let (doc, _) = open(&mut r, load(part));
        let reb = rebuild(&mut r, &doc);
        assert!(reb.steps.iter().all(|s| s.cached), "{part}: the healed document is a cache hit");
        assert_eq!(reb.untracked, 0, "{part}: /new/ tags on some body made during the rebuild");
        for s in &reb.steps {
            let Some(b) = &s.body else { continue };
            let t = b.topo(r.kernel()).unwrap();
            assert_eq!(b.names.faces.len(), t.faces.len(), "{part} {}", s.status.id);
            steps += 1;
            for n in &b.names.faces {
                faces += 1;
                assert!(!n.is_empty(), "{part} {}: a face without a name", s.status.id);
                assert!(!n.is_untracked(), "{part} {}: {n:?}", s.status.id);
                for tag in n.tags() {
                    *roles.entry(tag.role().to_string()).or_default() += 1;
                    imported += (tag.role() == "import") as usize;
                    // Every tag starts with the id of a feature of the part.
                    let fid = tag.as_str().split('/').next().unwrap();
                    assert!(doc.feature(fid).is_some(), "{part}: {tag}");
                }
            }
        }
        // Names resolve every reference to the same geometry signatures did: rebuilt cold
        // from the healed document, the part still matches the oracle.
        let mut cold = regen();
        let again = rebuild(&mut cold, &doc);
        let (diffs, _) = diff_part(cold.kernel(), part, &again);
        assert!(diffs.is_empty(), "{part}, resolved by name:\n{}", diffs.join("\n"));
        assert!(again.heals.is_empty() && again.untracked == 0);
        assert!(again.steps.iter().all(|s| s.status.warning.is_none()), "{part}: {:?}", again.statuses());
        if legacy_refs(&doc) > 0 {
            remaining.push((part, legacy_refs(&doc)));
        }
    }
    eprintln!("{faces} named faces over {steps} bodies; tags by role: {roles:?}; references left without a name: {remaining:?}");
    assert!(faces > 1500 && steps >= 124);
    // Only the imported part has faces named by geometric rank: the faces of its STEP
    // body, in every step that has them.
    assert!(imported > 0 && roles["import"] == imported);
    // The only references left without a name are vertices that are not on the model:
    // `patterns` Axis1's two points, and in `refgeom` a point inside the body (Axis4) and
    // the mid point of a full circle (Plane5). `up-to`'s Up To Vertex is a model vertex
    // and got its name; `refgeom` Axis2's origin became the origin kind on loading.
    assert_eq!(remaining, [("patterns", 2), ("refgeom", 2)]);
    let refgeom = load("refgeom");
    let oc_doc::FeatureKind::RefAxis(axis2) = &refgeom.feature("f12").unwrap().kind else { panic!() };
    assert_eq!(axis2.refs[0], oc_doc::GeomRef::Origin);
}

/// The tags on the body after step `step` that belong to instances of pattern `fid`,
/// grouped by instance key.
fn instance_tags(r: &Rebuild<Body>, step: &str, fid: &str) -> BTreeMap<String, Vec<String>> {
    let body = r.steps.iter().find(|s| s.status.id == step).unwrap().body.as_ref().unwrap();
    let prefix = format!("{fid}/i");
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for n in &body.names.faces {
        for t in n.tags() {
            if let Some(rest) = t.as_str().strip_prefix(&prefix) {
                let (key, seed) = rest.split_once('/').unwrap();
                out.entry(key.to_string()).or_default().push(seed.to_string());
            }
        }
    }
    for v in out.values_mut() {
        v.sort();
        v.dedup();
    }
    out
}

/// `doc` with feature `id` changed through its JSON.
fn changed(doc: &PartDocument, id: &str, f: &dyn Fn(&mut Value)) -> PartDocument {
    let mut v: Value = serde_json::from_str(&oc_doc::save(doc).unwrap()).unwrap();
    let feature = v["features"].as_array_mut().unwrap().iter_mut().find(|x| x["id"] == id).unwrap();
    f(feature);
    doc_of(&v)
}

#[test]
fn pattern_instance_names_survive_skips_and_count_changes() {
    let mut r = regen();
    let (doc, _) = open(&mut r, load("patterns"));
    let keys = |m: &BTreeMap<String, Vec<String>>| m.keys().cloned().collect::<Vec<_>>();
    // CirPattern1: six instances of a round boss about an axis. The seed is instance 1 and
    // keeps its own names; the copies are 2 to 6, each with the tags of the seed's faces
    // that are still faces of the part there (the side and the top everywhere; instance 3
    // overhangs a pocket, so its underside shows too).
    let base = instance_tags(&rebuild(&mut r, &doc), "f12", "f12");
    assert_eq!(keys(&base), ["2", "3", "4", "5", "6"]);
    let has = |tags: &Vec<String>, role: &str| tags.iter().any(|t| t.starts_with(&format!("f11/{role}/")));
    assert!(base.values().all(|tags| has(tags, "side") && has(tags, "end")), "{base:?}");
    assert!(has(&base["3"], "start") && !has(&base["2"], "start"), "{base:?}");
    // Instances to Skip: instance 3 goes, nobody is renumbered.
    let skip3 = changed(&doc, "f12", &|f| f["skip"] = serde_json::json!([3]));
    let skipped = instance_tags(&rebuild(&mut r, &skip3), "f12", "f12");
    assert_eq!(keys(&skipped), ["2", "4", "5", "6"]);
    assert!(skipped.iter().all(|(k, tags)| tags == &base[k]));
    // Eight instances: every instance moves (equal spacing over 360°), every name stays.
    let eight = changed(&doc, "f12", &|f| f["count"] = serde_json::json!(8));
    let more = instance_tags(&rebuild(&mut r, &eight), "f12", "f12");
    assert_eq!(keys(&more), ["2", "3", "4", "5", "6", "7", "8"]);
    assert!(more.values().all(|tags| has(tags, "side") && has(tags, "end")), "{more:?}");
    // LPattern1: 4 × 2 instances keyed by their place in the grid.
    let grid = instance_tags(&rebuild(&mut r, &doc), "f7", "f7");
    assert_eq!(keys(&grid), ["1,2", "2,1", "2,2", "3,1", "3,2", "4,1", "4,2"]);
    let smaller = changed(&doc, "f7", &|f| {
        f["dir1"]["count"] = serde_json::json!(3);
        f["skip"] = serde_json::json!([[2, 2]]);
    });
    let fewer = instance_tags(&rebuild(&mut r, &smaller), "f7", "f7");
    assert_eq!(keys(&fewer), ["1,2", "2,1", "3,1", "3,2"]);
    assert!(fewer.iter().all(|(k, tags)| tags == &grid[k]));
    // A mirrored feature is instance 2 of the mirror.
    let mirrored = instance_tags(&rebuild(&mut r, &doc), "f14", "f14");
    assert_eq!(keys(&mirrored), ["2"]);
}

#[test]
fn a_reference_to_a_pattern_instance_survives_skipping_another() {
    // A fillet on an edge of circular-pattern instance 5 (picked on the built part), then
    // instance 3 is skipped: face and edge indices shift, the fillet stays on instance 5.
    let mut r = regen();
    let (mut doc, _) = open(&mut r, load("patterns"));
    let reb = rebuild(&mut r, &doc);
    let body = reb.steps.iter().find(|s| s.status.id == "f12").unwrap().body.clone().unwrap();
    let t = body.topo(r.kernel()).unwrap();
    // The edge between two faces of instance 5: the rim of the boss's top.
    let inst5 = |f: &oc_kernel::FaceId| body.names.faces[f.index()].tags().iter().any(|t| t.as_str().starts_with("f12/i5/"));
    let (edge, _) = t.edges.iter().enumerate().find(|(_, e)| e.faces.len() == 2 && e.faces.iter().all(inst5)).expect("an edge inside instance 5");
    let sig = oc_naming::signature::edge_sig(t, Some(&body.names), oc_kernel::EdgeId(edge as u32));
    assert!(sig.name.is_some());
    let fillet = oc_doc::FilletFeature { radius: 0.5, radius2: None, edges: vec![sig], faces: vec![] };
    let at = doc.features.iter().position(|f| f.id.as_str() == "f12").unwrap() + 1;
    doc.features.insert(at, std::sync::Arc::new(oc_doc::Feature::new("f99", "Fillet9", oc_doc::FeatureKind::Fillet(fillet))));
    let status = |reb: &Rebuild<Body>| reb.steps.iter().find(|s| s.status.id == "f99").unwrap().status.clone();
    // Where the fillet face is: the face generated from the edge.
    let fillet_centre = |reb: &Rebuild<Body>, k: &OcctKernel| {
        let b = reb.steps.iter().find(|s| s.status.id == "f99").unwrap().body.clone().unwrap();
        let i = b.names.faces.iter().position(|n| n.tags().iter().any(|t| t.as_str().starts_with("f99/fillet/"))).expect("a fillet face");
        b.topo(k).unwrap().faces[i].centroid
    };
    let with = rebuild(&mut r, &doc);
    assert_eq!((status(&with).error, status(&with).warning), (None, None));
    let before = fillet_centre(&with, r.kernel());
    let skipped = rebuild(&mut r, &changed(&doc, "f12", &|f| f["skip"] = serde_json::json!([3])));
    assert_eq!((status(&skipped).error, status(&skipped).warning), (None, None));
    let after = fillet_centre(&skipped, r.kernel());
    assert!((0..3).all(|i| (before[i] - after[i]).abs() < 1e-9), "{before:?} {after:?}");
    // Skipping instance 5 itself removes the edge: the fillet says so.
    let gone = rebuild(&mut r, &changed(&doc, "f12", &|f| f["skip"] = serde_json::json!([5])));
    assert_eq!(status(&gone).error.as_deref(), Some("Missing reference: fillet edges"));
}

#[test]
fn side_faces_are_tagged_with_the_sketch_entity_they_were_swept_from() {
    use oc_doc::{FeatureKind, SketchEntity};
    use oc_kernel::Surface;
    let (mut lines, mut circles) = (0, 0);
    for part in PARTS {
        let doc = load(part);
        let mut r = regen();
        let reb = rebuild(&mut r, &doc);
        for f in doc.features.iter() {
            // Plain extrusions from the sketch plane: a side face contains its entity.
            let FeatureKind::Extrude(e) = &f.kind else { continue };
            if e.thin.is_some() || e.from.is_some() || e.end1.draft.is_some() || e.dir2.as_ref().is_some_and(|d| d.draft.is_some()) {
                continue;
            }
            let Some(sketch) = doc.feature(e.sketch_id.as_str()).and_then(|s| s.sketch()) else { continue };
            let Some(frame) = reb.frames.get(e.sketch_id.as_str()) else { continue };
            let Some(body) = reb.steps.iter().find(|s| s.status.id == f.id.as_str()).and_then(|s| s.body.clone()) else { continue };
            let world = |id: &oc_doc::Id| {
                let p = &sketch.points[id];
                [0, 1, 2].map(|i| frame.origin[i] + frame.x[i] * p.x + frame.y[i] * p.y)
            };
            let t = body.topo(r.kernel()).unwrap();
            for (face, name) in t.faces.iter().zip(&body.names.faces) {
                for tag in name.tags() {
                    let Some(ent) = tag.as_str().strip_prefix(&format!("{}/side/", f.id.as_str())) else { continue };
                    match (sketch.entities.get(ent), &face.surface) {
                        (Some(SketchEntity::Line { p1, p2, .. }), Surface::Plane { origin, normal }) => {
                            for p in [world(p1), world(p2)] {
                                let d: f64 = (0..3).map(|i| (p[i] - origin[i]) * normal[i]).sum();
                                assert!(d.abs() < 1e-6, "{part} {tag}: line end {p:?} is {d} off its face");
                            }
                            lines += 1;
                        }
                        (Some(SketchEntity::Circle { c, r: radius, .. }), Surface::Cylinder { axis, radius: live }) => {
                            let p = world(c);
                            let v = [0, 1, 2].map(|i| p[i] - axis.origin[i]);
                            let along: f64 = (0..3).map(|i| v[i] * axis.dir[i]).sum();
                            let off = (0..3).map(|i| (v[i] - along * axis.dir[i]).powi(2)).sum::<f64>().sqrt();
                            assert!(off < 1e-6 && (radius - live).abs() < 1e-6, "{part} {tag}: circle centre {p:?} is {off} off the axis");
                            circles += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    assert!(lines >= 80 && circles >= 10, "{lines} lines, {circles} circles checked");
}

/// The plate of `corpus/assemblies` (an 60 × 30 × 8 boss, `f2`, with two holes cut from one
/// sketch, `f4`), the holes blind and 4 deep from the bottom so each has a bottom face (an
/// `end` cap of `f4` at z = 4), and empty sketches `ids` on the faces `on` picks from the
/// rebuilt body (names as picking stores them now: caps with their feature's region count).
fn plate_with_sketches_on(on: &[(&str, &dyn Fn(&oc_kernel::FaceInfo) -> bool)]) -> Value {
    let text = std::fs::read_to_string(corpus_dir().join("assemblies/plate.ocpart")).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["features"][3]["end1"] = serde_json::json!({ "type": "blind", "depth": 4 });
    v["features"][3]["reverse"] = true.into();
    let mut r = regen();
    let reb = rebuild(&mut r, &doc_of(&v));
    assert!(reb.steps.iter().all(|s| s.status.error.is_none()), "{:?}", reb.statuses());
    let body = reb.body.clone().unwrap();
    let t = body.topo(r.kernel()).unwrap();
    for (id, pick) in on {
        let f = t.faces.iter().position(|f| pick(f)).unwrap_or_else(|| panic!("{id}: no such face"));
        let sig = oc_naming::signature::face_sig(t, Some(&body.names), oc_kernel::FaceId(f as u32));
        let sketch = serde_json::json!({
            "id": id, "name": format!("Sketch-{id}"), "type": "sketch",
            "plane": { "kind": "face", "sig": serde_json::to_value(&sig).unwrap() },
            "sketch": { "points": {}, "entities": {}, "relations": {}, "dimensions": {}, "nextDim": 1 }
        });
        v["features"].as_array_mut().unwrap().push(sketch);
    }
    v
}

fn plane_tags(v: &Value, id: &str) -> Vec<String> {
    let f = v["features"].as_array().unwrap().iter().find(|f| f["id"] == id).unwrap();
    serde_json::from_value(f["plane"]["sig"]["name"].clone()).unwrap()
}

#[test]
fn only_the_cap_of_a_single_region_feature_stands_in_wherever_it_moved() {
    // Owner decision 2026-11-19 (G1d merge), built in G1e: the region count travels with the
    // stored name. A bottom face of one of two blind holes cut from one sketch is a cap of a
    // two-region feature: deleting that hole's circle makes a reference to its bottom Missing
    // instead of moving it onto the other hole's bottom (the only `f4/end` face left, 30 mm
    // away). The boss is one region: its top keeps its references when the outline is
    // redrawn with new entities and doubled.
    let bottom = |x: f64| move |f: &oc_kernel::FaceInfo| (f.centroid[0] - x).abs() < 1e-6 && (f.centroid[2] - 4.0).abs() < 1e-6;
    let (b15, b45) = (bottom(15.0), bottom(45.0));
    let top = |f: &oc_kernel::FaceInfo| (f.centroid[2] - 8.0).abs() < 1e-6 && f.normal[2] > 0.5;
    let v = plate_with_sketches_on(&[("s15", &b15), ("s45", &b45), ("stop", &top)]);
    for (id, want) in [("s15", "f4/end/"), ("s45", "f4/end/"), ("stop", "f2/end/")] {
        let tags = plane_tags(&v, id);
        let n = if id == "stop" { "#1" } else { "#2" };
        assert!(tags.len() == 1 && tags[0].starts_with(want) && tags[0].ends_with(n), "{id}: {tags:?}");
    }
    let error = |reb: &Rebuild<Body>, id: &str| reb.steps.iter().find(|s| s.status.id == id).unwrap().status.error.clone();
    // Delete the first hole's circle (and its centre point).
    let mut deleted = v.clone();
    let sk = &mut deleted["features"][2]["sketch"];
    sk["entities"].as_object_mut().unwrap().remove("f3_c2");
    sk["points"].as_object_mut().unwrap().remove("f3_p1");
    let mut r = regen();
    let reb = rebuild(&mut r, &doc_of(&deleted));
    assert!(error(&reb, "s15").is_some_and(|e| e.starts_with("Missing reference")), "{:?}", reb.statuses());
    assert_eq!((error(&reb, "s45"), error(&reb, "stop")), (None, None));
    assert!(reb.heals.is_empty(), "nothing moved onto the other hole: {:?}", reb.heals.len());
    // The same document stored before G1e (no counts): the G1d rule, the remaining bottom
    // stands in for the deleted one.
    let legacy: Value = serde_json::from_str(&deleted.to_string().replace("#2\"", "\"").replace("#1\"", "\"")).unwrap();
    let reb = rebuild(&mut r, &doc_of(&legacy));
    assert_eq!(error(&reb, "s15"), None, "{:?}", reb.statuses());
    assert_eq!(reb.heals.len(), 1);
    // The boss outline redrawn with new entities, twice as large: its top (the one `f2/end`
    // cap of a one-region feature) moved 30 mm and still carries the sketch, by role.
    let mut redrawn: Value = serde_json::from_str(&v.to_string().replace("\"f1_l", "\"f1_redrawn_l")).unwrap();
    let pts = &mut redrawn["features"][0]["sketch"]["points"];
    for (p, x, y) in [("f1_p2", 120, 0), ("f1_p3", 120, 60), ("f1_p4", 0, 60)] {
        pts[p]["x"] = x.into();
        pts[p]["y"] = y.into();
    }
    let mut doc = doc_of(&redrawn);
    let reb = rebuild(&mut r, &doc);
    assert!(reb.steps.iter().all(|s| s.status.error.is_none() && s.status.warning.is_none()), "{:?}", reb.statuses());
    assert_eq!(apply_heals(&mut doc, &reb.heals), 1);
    let healed = match &doc.feature("stop").unwrap().kind {
        oc_doc::FeatureKind::Sketch(s) => match &s.plane {
            oc_doc::PlaneRef::Face { sig } => sig.name.clone().unwrap(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    assert!(healed.len() == 1 && healed[0].starts_with("f2/end/") && healed[0].ends_with("#1") && healed != plane_tags(&v, "stop"), "{healed:?}");
}
