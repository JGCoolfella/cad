//! Serde round trip of every corpus document, the every-field fixture, and the
//! documents the loader must refuse.

mod common;

use std::collections::BTreeSet;

use common::{corpus_documents, fixture};
use oc_doc::*;

/// Canonical form of a file as the current schema: the file's JSON run through the
/// migrations (1 → 2 turns a picked origin, stored as a vertex at (0, 0, 0), into the
/// origin kind, and changes nothing else).
fn canonical_at_current_version(text: &str) -> String {
    let mut v: serde_json::Value = serde_json::from_str(text).unwrap();
    let found = v["version"].as_u64().unwrap() as u32;
    MIGRATIONS.upgrade(v.as_object_mut().unwrap(), found).unwrap();
    assert_eq!(v["version"], CURRENT_VERSION);
    canonicalize_json(&v.to_string()).unwrap()
}

/// load → save is lossless: the saved text has the same canonical bytes as the (migrated)
/// file, reloads to an equal model, and saving again gives the same text.
fn assert_round_trip(label: &str, text: &str) -> PartDocument {
    let doc = load(text).unwrap_or_else(|e| panic!("{label}: {e}")).doc;
    let saved = save(&doc).unwrap();
    let want = canonical_at_current_version(text);
    assert_eq!(
        canonical(&doc).unwrap(),
        want,
        "{label}: model differs from file"
    );
    assert_eq!(
        canonicalize_json(&saved).unwrap(),
        want,
        "{label}: saved text differs from file"
    );
    let again = load(&saved).unwrap().doc;
    assert_eq!(again, doc, "{label}: reloaded model differs");
    assert_eq!(
        save(&again).unwrap(),
        saved,
        "{label}: save is not idempotent"
    );
    assert_eq!(doc.validate(), Vec::<String>::new(), "{label}");
    doc
}

#[test]
fn every_corpus_document_round_trips() {
    let docs = corpus_documents();
    let parts = docs.iter().filter(|d| d.label.ends_with(".ocpart")).count();
    assert!(parts >= 17, "expected the 17 golden parts, found {parts}");
    assert!(
        docs.len() >= parts + 9 + 10,
        "expected torture documents and their edit steps, found {}",
        docs.len()
    );
    let mut types = BTreeSet::new();
    for d in &docs {
        let doc = assert_round_trip(&d.label, &d.text);
        types.extend(doc.features.iter().map(|f| f.kind.type_name()));
    }
    // Phase 0.2 promises every feature type in the corpus; hold it to that.
    let all: BTreeSet<_> = FeatureKind::TYPE_NAMES.into_iter().collect();
    assert_eq!(types, all);
}

/// Member order of every sketch record in a JSON document: (path, ids in order).
/// A sketch is any object with `points`, `entities` and `nextDim`.
fn sketch_record_orders(text: &str) -> Vec<(String, Vec<String>)> {
    fn walk(v: &serde_json::Value, path: String, out: &mut Vec<(String, Vec<String>)>) {
        match v {
            serde_json::Value::Object(m) => {
                if ["points", "entities", "nextDim"]
                    .iter()
                    .all(|k| m.contains_key(*k))
                {
                    for rec in ["points", "entities", "relations", "dimensions", "refs"] {
                        if let Some(serde_json::Value::Object(r)) = m.get(rec) {
                            out.push((format!("{path}.{rec}"), r.keys().cloned().collect()));
                        }
                    }
                }
                for (k, x) in m {
                    walk(x, format!("{path}.{k}"), out);
                }
            }
            serde_json::Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    walk(x, format!("{path}[{i}]"), out);
                }
            }
            _ => {}
        }
    }
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    let mut out = Vec::new();
    walk(&v, "$".into(), &mut out);
    out
}

/// The solver depends on record order (E2), so load → save must keep each sketch's
/// points, entities, relations, dimensions and refs in the file's order, not sort them
/// by id (`origin` before `f1_p1`, `f7_p2` before `f7_p11`).
#[test]
fn load_then_save_keeps_sketch_member_order() {
    let mut docs: Vec<(String, String)> = corpus_documents()
        .into_iter()
        .map(|d| (d.label, d.text))
        .collect();
    docs.push(("all-fields.ocpart".into(), fixture("all-fields.ocpart")));
    let (mut records, mut unsorted) = (0, 0);
    for (label, text) in &docs {
        let want = sketch_record_orders(text);
        let doc = load(text).unwrap_or_else(|e| panic!("{label}: {e}")).doc;
        let got = sketch_record_orders(&save(&doc).unwrap());
        assert_eq!(got, want, "{label}: sketch member order changed");
        // The in-memory model iterates in the same order.
        let mut model = Vec::new();
        for (i, f) in doc.features.iter().enumerate() {
            let Some(s) = f.sketch() else { continue };
            let mut push = |rec: &str, ids: Vec<&Id>| {
                let ids = ids.into_iter().map(Id::to_string).collect();
                model.push((format!("$.features[{i}].sketch.{rec}"), ids));
            };
            push("points", s.points.keys().collect());
            push("entities", s.entities.keys().collect());
            push("relations", s.relations.keys().collect());
            push("dimensions", s.dimensions.keys().collect());
            if let Some(r) = &s.refs {
                push("refs", r.keys().collect());
            }
        }
        assert_eq!(model, want, "{label}: model order differs from the file");
        records += want.len();
        unsorted += want
            .iter()
            .filter(|(_, ids)| ids.windows(2).any(|w| w[0] > w[1]))
            .count();
    }
    // Make sure the corpus exercises this: many records are not in id order.
    assert!(records > 100, "only {records} sketch records");
    assert!(
        unsorted > 20,
        "only {unsorted} sketch records out of id order"
    );
}

#[test]
fn every_field_and_variant_round_trips() {
    let text = fixture("all-fields.ocpart");
    let doc = assert_round_trip("all-fields", &text);

    let types: BTreeSet<_> = doc.features.iter().map(|f| f.kind.type_name()).collect();
    assert_eq!(types, FeatureKind::TYPE_NAMES.into_iter().collect());

    // Fields whose Rust spelling differs from the JSON, and the three states of
    // `field?: T | null`.
    let sketch = doc.features[0].sketch().unwrap();
    assert_eq!(sketch.points["p3"].is_virtual, Some(true));
    assert_eq!(sketch.points["p4"].ext.as_ref().map(Id::as_str), Some("r1"));
    assert!(
        matches!(sketch.refs.as_ref().unwrap()["r2"], SketchRef::Vertex { ref edges, .. } if edges.as_ref().unwrap().len() == 2)
    );
    let FeatureKind::Extrude(e) = &doc.feature("f4").unwrap().kind else {
        panic!()
    };
    assert!(matches!(e.end1.r#ref, Some(Some(GeomRef::Plane { .. }))));
    assert_eq!(e.end1.vertex, Some(None));
    assert_eq!(e.dir2.as_ref().unwrap().r#ref, Some(None));
    assert_eq!(e.dir2.as_ref().unwrap().vertex, Some(Some([1.0, 2.0, 3.0])));
    // Persistent names (schema version 2): present on some references, absent on others.
    assert_eq!(e.dir2.as_ref().unwrap().vertex_name.as_ref().map(Vec::len), Some(3));
    assert_eq!(e.end1.vertex_name, None);
    let refs = sketch.refs.as_ref().unwrap();
    assert!(matches!(&refs["r1"], SketchRef::Vertex { name: Some(n), .. } if n[2] == ["f4/side/e2", "f9/i2/f4/side/e1"]));
    assert!(matches!(&refs["r2"], SketchRef::Vertex { name: None, .. }));
    assert!(matches!(&refs["r3"], SketchRef::Edge { sig, .. } if sig.name.as_ref().unwrap()[1] == ["f4/side/e1"]));
    let FeatureKind::Sketch(s2) = &doc.feature("f2").unwrap().kind else {
        panic!()
    };
    assert!(matches!(&s2.plane, PlaneRef::Face { sig } if sig.name.as_deref() == Some(&["f4/end/00f1".to_string()][..])));
    let FeatureKind::RefPlane(p) = &doc.feature("f3").unwrap().kind else {
        panic!()
    };
    let named = |r: &GeomRef| match r {
        GeomRef::Edge { sig } => sig.name.is_some(),
        GeomRef::Face { sig } => sig.name.is_some(),
        GeomRef::Vertex { name, .. } => name.is_some(),
        _ => false,
    };
    let flags: Vec<bool> = p.refs.as_ref().unwrap().iter().map(named).collect();
    assert_eq!(flags, [false, true, false, false, false, true, false, false, false]);
    // The origin is its own kind; a version 2 vertex at (0, 0, 0) is a model vertex.
    let refs = p.refs.as_ref().unwrap();
    assert!(matches!(refs[4], GeomRef::Vertex { p, name: None } if p == [0.0; 3]));
    assert_eq!(refs[8], GeomRef::Origin);
    let FeatureKind::Extrude(e) = &doc.feature("f25").unwrap().kind else {
        panic!()
    };
    assert_eq!(e.end1.r#ref, None);
    let FeatureKind::CoordSys(c) = &doc.feature("f12").unwrap().kind else {
        panic!()
    };
    assert!(c.origin.is_none() && c.x_axis.is_some());
    assert_eq!(doc.rollback, Some(21));
    assert_eq!(doc.units, UnitSystem::Ips);
    assert!(doc.feature("f20").unwrap().is_suppressed());
}

#[test]
fn rollback_null_is_written_and_required() {
    let mut doc = PartDocument::new("p", UnitSystem::Mmgs);
    assert!(save(&doc).unwrap().contains("\"rollback\": null"));
    doc.rollback = Some(0);
    assert!(save(&doc).unwrap().contains("\"rollback\": 0"));
    let text = r#"{"format":"opencad-part","version":1,"name":"p","units":"MMGS","features":[]}"#;
    assert!(
        matches!(load(text), Err(LoadError::Schema { .. })),
        "{:?}",
        load(text)
    );
}

// ---------------------------------------------------------------- refusals

/// A small valid document with one extrude, to be broken by each case below.
const BASE: &str = r#"{"format":"opencad-part","version":1,"name":"p","units":"MMGS","rollback":null,"features":[
  {"id":"f1","name":"Sketch1","type":"sketch","plane":{"kind":"std","plane":"front"},
   "sketch":{"points":{"a":{"id":"a","x":0,"y":0}},"entities":{},"relations":{},"dimensions":{},"nextDim":1}},
  {"id":"f2","name":"Boss-Extrude1","type":"extrude","cut":false,"sketchId":"f1","reverse":false,"merge":true,
   "end1":{"type":"blind","depth":5}}]}"#;

fn broken(from: &str, to: &str) -> Result<Loaded, LoadError> {
    assert!(BASE.contains(from), "{from}");
    load(&BASE.replacen(from, to, 1))
}

fn lossy_at(r: Result<Loaded, LoadError>) -> String {
    match r {
        Err(LoadError::Lossy { path, .. }) => path,
        other => panic!("expected a lossy-load error, got {other:?}"),
    }
}

#[test]
fn base_document_loads() {
    assert_round_trip("base", BASE);
}

#[test]
fn unknown_fields_are_refused_with_their_path() {
    for (from, to, path) in [
        (r#""name":"p","#, r#""name":"p","extra":1,"#, "$.extra"),
        (
            r#""merge":true,"#,
            r#""merge":true,"glow":true,"#,
            "$.features[1].glow",
        ),
        (
            r#""x":0,"y":0}"#,
            r#""x":0,"y":0,"z":0}"#,
            "$.features[0].sketch.points.a.z",
        ),
        (
            r#""depth":5}"#,
            r#""depth":5,"why":"?"}"#,
            "$.features[1].end1.why",
        ),
        (
            r#""plane":"front"}"#,
            r#""plane":"front","tilt":0}"#,
            "$.features[0].plane.tilt",
        ),
    ] {
        assert_eq!(lossy_at(broken(from, to)), path);
    }
}

#[test]
fn null_in_an_optional_non_null_field_is_refused() {
    // `dir2?: EndCondition` cannot be null; reading it as absent would drop the null.
    assert_eq!(
        lossy_at(broken(r#""merge":true,"#, r#""merge":true,"dir2":null,"#)),
        "$.features[1].dir2"
    );
    assert_eq!(
        lossy_at(broken(
            r#""name":"Sketch1","#,
            r#""name":"Sketch1","suppressed":null,"#
        )),
        "$.features[0].suppressed"
    );
}

#[test]
fn schema_violations_name_the_field() {
    let schema = |r: Result<Loaded, LoadError>| match r {
        Err(LoadError::Schema { path, message }) => format!("{path}: {message}"),
        other => panic!("expected a schema error, got {other:?}"),
    };
    let e = schema(broken(r#""units":"MMGS""#, r#""units":"furlongs""#));
    assert!(e.starts_with("$.units:"), "{e}");
    let e = schema(broken(r#""depth":5"#, r#""depth":"5""#));
    assert!(e.contains("depth") || e.contains("features[1]"), "{e}");
    let e = schema(broken(r#""type":"blind""#, r#""type":"sideways""#));
    assert!(e.contains("sideways"), "{e}");
    let e = schema(broken(r#""type":"extrude""#, r#""type":"teleport""#));
    assert!(e.contains("teleport"), "{e}");
    let e = schema(broken(r#""nextDim":1"#, r#""nextDim":1.5"#));
    assert!(e.contains("nextDim") || e.contains("1.5"), "{e}");
    schema(broken(r#""cut":false,"#, ""));
}

#[test]
fn malformed_and_foreign_files_are_refused() {
    assert!(matches!(load("{"), Err(LoadError::Syntax(_))));
    assert!(matches!(load("{} x"), Err(LoadError::Syntax(_))));
    assert!(matches!(load("[]"), Err(LoadError::NotAPart(_))));
    assert!(matches!(
        broken(r#""opencad-part""#, r#""opencad-assembly""#),
        Err(LoadError::NotAPart(_))
    ));
    assert!(matches!(
        broken(r#""version":1"#, r#""version":"1""#),
        Err(LoadError::NotAPart(_))
    ));
    assert!(matches!(
        broken(r#""version":1"#, r#""version":1.5"#),
        Err(LoadError::NotAPart(_))
    ));
    let e = broken(r#""name":"p""#, r#""name":"p","name":"q""#).unwrap_err();
    assert!(
        matches!(&e, LoadError::Syntax(m) if m.contains("twice")),
        "{e:?}"
    );
}

#[test]
fn newer_documents_are_refused() {
    let next = CURRENT_VERSION + 1;
    let e = broken(r#""version":1"#, &format!(r#""version":{next}"#)).unwrap_err();
    assert_eq!(
        e,
        LoadError::Migration(MigrationError::TooNew {
            found: next,
            newest: CURRENT_VERSION
        })
    );
    assert!(e.to_string().contains("newer"));
}

#[test]
fn older_documents_are_migrated_before_typing() {
    // A made-up version 0 that called `nextDim` `nextDimension`.
    fn rename(root: &mut serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
        for f in root["features"].as_array_mut().ok_or("no features")? {
            if let Some(s) = f.get_mut("sketch").and_then(|s| s.as_object_mut()) {
                let n = s.remove("nextDimension").ok_or("no nextDimension")?;
                s.insert("nextDim".into(), n);
            }
        }
        Ok(())
    }
    let steps: Vec<Migration> = std::iter::once(Migration {
        from: 0,
        description: "rename nextDimension",
        apply: rename,
    })
    .chain(MIGRATIONS.steps.iter().copied())
    .collect();
    let reg = Migrations {
        current: CURRENT_VERSION,
        steps: &steps,
    };
    reg.check().unwrap();
    let v0 = BASE
        .replace(r#""version":1"#, r#""version":0"#)
        .replace("nextDim", "nextDimension");
    let loaded = io::load_with(&v0, &reg).unwrap();
    assert_eq!(loaded.source_version, 0);
    assert_eq!(loaded.doc, load(BASE).unwrap().doc);
    assert_eq!(load(BASE).unwrap().source_version, 1);
    // Without the migration, version 0 is unreadable.
    assert!(matches!(
        load(&v0),
        Err(LoadError::Migration(MigrationError::TooOld { .. }))
    ));
}

#[test]
fn values_json_cannot_hold_are_not_saved() {
    let base = load(BASE).unwrap().doc;
    let with = |depth: f64, draft: Option<f64>| {
        let mut doc = base.clone();
        let FeatureKind::Extrude(e) = &mut doc.feature_mut("f2").unwrap().kind else {
            panic!()
        };
        e.end1.depth = depth;
        e.end1.draft = draft;
        doc
    };
    assert!(save(&with(1.0, Some(0.5))).is_ok());
    assert!(save(&with(f64::NAN, None)).is_err());
    assert!(canonical(&with(f64::INFINITY, None)).is_err());
    // Optional fields too: never silently written as null (which would read back as absent).
    assert!(save(&with(1.0, Some(f64::NAN))).is_err());
}

#[test]
fn canonical_form_ignores_layout_and_number_spelling() {
    let a = r#"{"b":[1.0,2.50,1E3,-0],"a":"x"}"#;
    let b = "{\r\n  \"a\" : \"x\",\n  \"b\" : [ 1, 2.5, 1000, 0 ]\r\n}";
    assert_eq!(canonicalize_json(a).unwrap(), canonicalize_json(b).unwrap());
    assert_eq!(
        canonicalize_json(a).unwrap(),
        r#"{"a":"x","b":[1,2.5,1000,0]}"#
    );
}

#[test]
fn version_1_documents_load_as_version_2_without_names() {
    // The TypeScript app's original format: references are bare signatures.
    let v1 = BASE.replace(
        r#""plane":{"kind":"std","plane":"front"}"#,
        r#""plane":{"kind":"face","sig":{"surface":"PLANE","center":[0,0,5],"normal":[0,0,1],"area":4}}"#,
    );
    let loaded = load(&v1).unwrap();
    assert_eq!(loaded.source_version, 1);
    let FeatureKind::Sketch(s) = &loaded.doc.features[0].kind else {
        panic!()
    };
    assert!(matches!(&s.plane, PlaneRef::Face { sig } if sig.name.is_none()));
    let saved = save(&loaded.doc).unwrap();
    assert!(saved.contains(&format!("\"version\": {CURRENT_VERSION}")) && !saved.contains("\"name\": ["), "{saved}");
    // A version 2 file with a name reads back with it. (A version 1 reader refuses unknown
    // fields, which is why the version was bumped.)
    let named = v1.replace(r#""version":1"#, r#""version":2"#).replace(r#""area":4"#, r#""area":4,"name":["f2/end/0a"]"#);
    let loaded = load(&named).unwrap();
    assert_eq!(loaded.source_version, 2);
    let FeatureKind::Sketch(s) = &loaded.doc.features[0].kind else {
        panic!()
    };
    assert!(matches!(&s.plane, PlaneRef::Face { sig } if sig.name.as_ref().unwrap()[0] == "f2/end/0a"));
    assert_eq!(load(&save(&loaded.doc).unwrap()).unwrap().doc, loaded.doc);
    // A name must be a list of tags.
    let bad = named.replace(r#"["f2/end/0a"]"#, r#""f2/end/0a""#);
    assert!(matches!(load(&bad), Err(LoadError::Schema { .. })), "{:?}", load(&bad));
}

#[test]
fn version_1_origin_vertices_become_the_origin_kind() {
    // A reference axis from the origin to a model vertex, as version 1 stored it; a sketch
    // reference to a model vertex at the origin; an Up To Vertex at the origin.
    let v1 = r#"{"format":"opencad-part","version":1,"name":"p","units":"MMGS","rollback":null,"features":[
      {"id":"f1","name":"Sketch1","type":"sketch","plane":{"kind":"std","plane":"front"},
       "sketch":{"points":{},"entities":{},"relations":{},"dimensions":{},"nextDim":1,
                 "refs":{"r1":{"id":"r1","kind":"vertex","p":[0,0,0]}}}},
      {"id":"f2","name":"Boss-Extrude1","type":"extrude","cut":false,"sketchId":"f1","reverse":false,"merge":true,
       "end1":{"type":"upToVertex","depth":0,"vertex":[0,0,0]}},
      {"id":"f3","name":"Axis1","type":"refaxis","refs":[{"kind":"vertex","p":[0,0,0]},{"kind":"vertex","p":[0,-0.0,1e-9]},{"kind":"vertex","p":[40,30,20]}]},
      {"id":"f4","name":"Coordinate System1","type":"coordsys","origin":{"kind":"vertex","p":[0,0,0.0]},"xAxis":null,"yAxis":null}]}"#;
    let doc = load(v1).unwrap().doc;
    let FeatureKind::RefAxis(a) = &doc.feature("f3").unwrap().kind else {
        panic!()
    };
    assert_eq!(a.refs[0], GeomRef::Origin);
    assert!(matches!(a.refs[1], GeomRef::Vertex { .. }), "not exactly the origin");
    assert!(matches!(a.refs[2], GeomRef::Vertex { p, .. } if p == [40.0, 30.0, 20.0]));
    let FeatureKind::CoordSys(c) = &doc.feature("f4").unwrap().kind else {
        panic!()
    };
    assert_eq!(c.origin, Some(GeomRef::Origin));
    // Not picked references of that shape: left as they are.
    let sketch = doc.features[0].sketch().unwrap();
    assert!(matches!(&sketch.refs.as_ref().unwrap()["r1"], SketchRef::Vertex { p, .. } if *p == [0.0; 3]));
    let FeatureKind::Extrude(e) = &doc.feature("f2").unwrap().kind else {
        panic!()
    };
    assert_eq!(e.end1.vertex, Some(Some([0.0; 3])));
    let saved = save(&doc).unwrap();
    assert_eq!(saved.matches("\"kind\": \"origin\"").count(), 2, "{saved}");
    assert_eq!(load(&saved).unwrap().doc, doc);
    // In a version 2 file a vertex at (0, 0, 0) is a model vertex and stays one.
    let v2 = load(&v1.replace(r#""version":1"#, r#""version":2"#)).unwrap().doc;
    let FeatureKind::RefAxis(a) = &v2.feature("f3").unwrap().kind else {
        panic!()
    };
    assert!(matches!(a.refs[0], GeomRef::Vertex { name: None, .. }));
}

/// References in a JSON document: face signatures, edge signatures, vertices (vertex
/// references and the non-null `vertex` of an end condition).
fn count_refs(v: &serde_json::Value, n: &mut [usize; 3]) {
    match v {
        serde_json::Value::Object(m) => {
            if m.contains_key("surface") && m.contains_key("area") {
                n[0] += 1;
            }
            if m.contains_key("curve") && m.contains_key("length") {
                n[1] += 1;
            }
            if m.get("kind").and_then(|k| k.as_str()) == Some("vertex") && m.contains_key("p") {
                n[2] += 1;
            }
            if m.get("vertex").is_some_and(|x| x.is_array()) && m.contains_key("depth") {
                n[2] += 1;
            }
            m.values().for_each(|x| count_refs(x, n));
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| count_refs(x, n)),
        _ => {}
    }
}

#[test]
fn refs_mut_visits_every_reference_to_model_topology() {
    let mut doc = load(&fixture("all-fields.ocpart")).unwrap().doc;
    let mut want = [0; 3];
    count_refs(&serde_json::from_str(&fixture("all-fields.ocpart")).unwrap(), &mut want);
    assert!(want.iter().all(|n| *n >= 3), "{want:?}");
    let mut got = [0; 3];
    let mut features = doc.features.clone();
    for f in features.iter_mut() {
        std::sync::Arc::make_mut(f).refs_mut(&mut |slot| match slot {
            RefSlot::Face(s) => {
                got[0] += 1;
                s.name = Some(vec!["x/new/0".into()]);
            }
            RefSlot::Edge(s) => {
                got[1] += 1;
                s.name = Some([vec!["x/new/0".into()], vec!["x/new/1".into()]]);
            }
            RefSlot::Vertex { name, .. } => {
                got[2] += 1;
                *name = Some(vec![vec!["x/new/0".into()]]);
            }
        });
    }
    assert_eq!(got, want, "faces, edges, vertices");
    // What the visitor wrote is in the document: every reference now has a name.
    doc.features = features;
    let text = save(&doc).unwrap();
    let names = text.matches("\"name\": [").count() + text.matches("\"vertexName\": [").count();
    assert_eq!(names, want.iter().sum::<usize>());
    assert_eq!(load(&text).unwrap().doc, doc);
}

#[test]
fn version_2_documents_load_as_version_3_with_one_configuration() {
    // G1e: version 3 adds configurations (optional) and region counts on cap tags (inside
    // the stored strings). A version 2 document is a version 3 one without them.
    let v2 = BASE.replace(r#""version":1"#, r#""version":2"#);
    let loaded = load(&v2).unwrap();
    assert_eq!(loaded.source_version, 2);
    assert!(loaded.doc.configurations.is_none());
    assert!(save(&loaded.doc).unwrap().contains("\"version\": 3"));
    // Configurations round-trip, every field.
    let cfgs = r#","configurations":{"active":"c2","list":[
        {"id":"c1","name":"Default","values":{"f2/end1.depth":5},"suppressed":{"f2":false},
         "sketches":{"f1":{"points":{"a":[0,0]},"radii":{"c":2.5}}}},
        {"id":"c2","name":"Long","description":"longer","values":{"f2/end1.depth":12},"suppressed":{"f2":true},
         "components":{"i1":"Short"},"displayState":"d1"}]}}"#;
    let with = BASE.replace(r#""version":1"#, r#""version":3"#);
    let with = format!("{}{cfgs}", with.trim_end().strip_suffix('}').unwrap());
    let loaded = load(&with).unwrap();
    let c = loaded.doc.configurations.as_deref().unwrap();
    assert_eq!((c.active.as_str(), c.list.len()), ("c2", 2));
    assert_eq!(c.find("Long").unwrap().values["f2/end1.depth"], 12.0);
    assert_eq!(c.get("c1").unwrap().sketches["f1"].radii["c"], 2.5);
    assert_eq!(load(&save(&loaded.doc).unwrap()).unwrap().doc, loaded.doc);
    assert_eq!(canonicalize_json(&save(&loaded.doc).unwrap()).unwrap(), canonicalize_json(&with).unwrap());
    assert!(loaded.doc.validate().is_empty(), "{:?}", loaded.doc.validate());
    // Structural checks: an unknown active configuration, a repeated name, a bad key.
    let bad = with.replace(r#""active":"c2""#, r#""active":"c9""#).replace(r#""name":"Long""#, r#""name":"Default""#).replace("f2/end1.depth\":12", "nokey\":12");
    let issues = load(&bad).unwrap().doc.validate();
    assert_eq!(issues.len(), 3, "{issues:?}");
    // Unknown fields in a configuration are refused, as everywhere.
    assert!(load(&with.replace(r#""description":"longer""#, r#""descr":"longer""#)).is_err());
}

#[test]
fn version_1_assemblies_load_as_version_2() {
    let v1 = r#"{"format":"opencad-assembly","version":1,"name":"a","units":"MMGS","components":[{"id":"c1","kind":"part","path":"p.ocpart","config":"Default"}],
        "instances":[{"id":"i1","component":"c1","name":"p<1>","fixed":true,"transform":{"position":[0,0,0],"rotation":[1,0,0,0]}}],"mates":[]}"#;
    let loaded = oc_doc::load_assembly(v1).unwrap();
    assert_eq!(loaded.source_version, 1);
    assert!(loaded.doc.configurations.is_none() && loaded.doc.display_states.is_none() && loaded.doc.instances[0].config.is_none());
    let saved = oc_doc::save_assembly(&loaded.doc).unwrap();
    assert!(saved.contains("\"version\": 2"), "{saved}");
    // Version 2: an instance's referenced configuration, configurations and display states.
    let v2 = saved
        .replacen("\"fixed\": true,", "\"fixed\": true, \"config\": \"Long\",", 1)
        .trim_end()
        .strip_suffix('}')
        .unwrap()
        .to_string()
        + r##","configurations":{"active":"c1","list":[{"id":"c1","name":"Default","suppressed":{"i1":false},"components":{"i1":"Long"},"displayState":"d1"}]},
             "displayStates":{"active":"d1","list":[{"id":"d1","name":"Display State-1","instances":{"i1":{"hidden":true,"color":"#ff0000","display":"wireframe","transparent":true}}}]}}"##;
    let doc = oc_doc::load_assembly(&v2).unwrap().doc;
    assert_eq!(doc.instances[0].config.as_deref(), Some("Long"));
    assert_eq!(doc.display_states.as_ref().unwrap().list[0].instances["i1"].display, Some(DisplayMode::Wireframe));
    assert_eq!(oc_doc::load_assembly(&oc_doc::save_assembly(&doc).unwrap()).unwrap().doc, doc);
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
}
