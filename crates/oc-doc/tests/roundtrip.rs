//! Serde round trip of every corpus document, the every-field fixture, and the
//! documents the loader must refuse.

mod common;

use std::collections::BTreeSet;

use common::{corpus_documents, fixture};
use oc_doc::*;

/// load → save is lossless: the saved text has the same canonical bytes as the file,
/// reloads to an equal model, and saving again gives the same text.
fn assert_round_trip(label: &str, text: &str) -> PartDocument {
    let doc = load(text).unwrap_or_else(|e| panic!("{label}: {e}")).doc;
    let saved = save(&doc).unwrap();
    let want = canonicalize_json(text).unwrap();
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
    let e = broken(r#""version":1"#, r#""version":2"#).unwrap_err();
    assert_eq!(
        e,
        LoadError::Migration(MigrationError::TooNew {
            found: 2,
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
    let steps = [Migration {
        from: 0,
        description: "rename nextDimension",
        apply: rename,
    }];
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
