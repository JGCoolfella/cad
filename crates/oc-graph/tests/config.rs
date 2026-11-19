//! Configurations of a part (G1e): activating one writes its configured values,
//! suppression and sketch solutions into the features; activating the first again gives
//! the original document bit for bit, and the regeneration cache keeps each
//! configuration's results.
mod common;

use std::f64::consts::PI;

use common::*;
use oc_doc::{PartDocument, DEFAULT_CONFIGURATION};
use oc_graph::config::{activate, configured};
use oc_kernel::Kernel;
use serde_json::{json, Value};

/// block-hole (a 20 × 10 × 5 block, a Ø6 hole through it at (10, 5)) with dimensions on the
/// hole's sketch (Ø6, 11.18 from the origin) and two configurations: Default (as built) and
/// Long (12 deep, Ø4, the hole 6 from the origin, and the cut suppressed in Thin, a third).
fn configured_block() -> PartDocument {
    let text = std::fs::read_to_string(corpus_dir().join("block-hole.ocpart")).unwrap();
    let mut v: Value = serde_json::from_str(&text).unwrap();
    let dist = (10.0f64 * 10.0 + 5.0 * 5.0).sqrt();
    v["features"][2]["sketch"]["dimensions"] = json!({
        "d1": { "id": "d1", "name": "D1", "kind": "diameter", "refs": ["f3_c2"], "value": 6.0, "label": [14, 9] },
        "d2": { "id": "d2", "name": "D2", "kind": "distance", "refs": ["origin", "f3_p1"], "value": dist, "label": [4, 4] }
    });
    v["features"][2]["sketch"]["nextDim"] = 3.into();
    v["version"] = 3.into();
    v["configurations"] = json!({
        "active": "c1",
        "list": [
            { "id": "c1", "name": "Default",
              "values": { "f2/end1.depth": 5.0, "f3/sketch.dimensions.d1.value": 6.0, "f3/sketch.dimensions.d2.value": dist },
              "suppressed": { "f4": false } },
            { "id": "c2", "name": "Long",
              "values": { "f2/end1.depth": 12.0, "f3/sketch.dimensions.d1.value": 4.0, "f3/sketch.dimensions.d2.value": 6.0 },
              "suppressed": { "f4": false } },
            { "id": "c3", "name": "Thin",
              "values": { "f2/end1.depth": 2.0, "f3/sketch.dimensions.d1.value": 6.0, "f3/sketch.dimensions.d2.value": dist },
              "suppressed": { "f4": true } }
        ]
    });
    let doc = oc_doc::load(&v.to_string()).unwrap_or_else(|e| panic!("{e:?}")).doc;
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
    doc
}

fn volume(r: &mut oc_graph::Regenerator<oc_kernel_occt::OcctKernel>, doc: &PartDocument) -> f64 {
    let reb = r.regenerate_doc(doc, &oc_graph::CancelToken::new()).unwrap();
    assert!(reb.steps.iter().all(|s| s.status.error.is_none()), "{:?}", reb.statuses());
    r.kernel().mass_properties(&reb.body.unwrap().body).unwrap().volume
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn activating_a_configuration_writes_its_values_suppression_and_sketches() {
    let doc = configured_block();
    let mut r = regen();
    assert!(close(volume(&mut r, &doc), 20.0 * 10.0 * 5.0 - PI * 9.0 * 5.0));
    let long = activate(&doc, "c2").unwrap();
    assert_eq!(long.configurations.as_ref().unwrap().active.as_str(), "c2");
    // 12 deep, a Ø4 hole: the sketch was re-solved (the circle's radius is 2, its centre 6
    // from the origin), and the hole still goes through.
    let sk = long.feature("f3").unwrap().sketch().unwrap();
    let oc_doc::SketchEntity::Circle { r: radius, .. } = &sk.entities["f3_c2"] else { panic!() };
    assert!((radius - 2.0).abs() < 1e-9, "{radius}");
    let p = &sk.points["f3_p1"];
    assert!((p.x.hypot(p.y) - 6.0).abs() < 1e-9, "{p:?}");
    assert!(close(volume(&mut r, &long), 20.0 * 10.0 * 12.0 - PI * 4.0 * 12.0));
    // By name, and Thin: 2 deep, the cut suppressed in that configuration only.
    let thin = activate(&long, "Thin").unwrap();
    assert!(thin.feature("f4").unwrap().is_suppressed());
    assert!(close(volume(&mut r, &thin), 20.0 * 10.0 * 2.0));
    let back = activate(&thin, "c2").unwrap();
    assert!(!back.feature("f4").unwrap().is_suppressed());
    // Unknown configurations are refused.
    assert!(activate(&doc, "c9").is_err());
}

#[test]
fn going_back_gives_the_same_document_and_hits_the_cache() {
    let doc = configured_block();
    let long = activate(&doc, "Long").unwrap();
    let back = activate(&long, "Default").unwrap();
    // The features are the original's bit for bit: the stored solution was written back
    // and the solver took no step.
    for (a, b) in doc.features.iter().zip(back.features.iter()) {
        assert_eq!(oc_doc::to_canonical_string(&**a).unwrap(), oc_doc::to_canonical_string(&**b).unwrap(), "{}", a.id);
    }
    // And again through Long: the same Long.
    let long2 = activate(&back, "Long").unwrap();
    for (a, b) in long.features.iter().zip(long2.features.iter()) {
        assert_eq!(**a, **b, "{}", a.id);
    }
    // The regeneration keeps each configuration's results through eviction (no entries
    // beyond the last rebuild's otherwise).
    let mut r = regen();
    r.cache_limit = 0;
    let rebuild = |r: &mut oc_graph::Regenerator<_>, d: &PartDocument| r.regenerate_doc(d, &oc_graph::CancelToken::new()).unwrap();
    rebuild(&mut r, &doc);
    r.pin("c1");
    let l = rebuild(&mut r, &long);
    assert!(l.steps.iter().skip(1).any(|s| !s.cached));
    r.pin("c2");
    assert!(r.has_pinned("c1") && r.has_pinned("c2"));
    let d = rebuild(&mut r, &back);
    assert!(d.steps.iter().all(|s| s.cached), "Default again regenerates nothing: {:?}", d.steps.iter().map(|s| s.cached).collect::<Vec<_>>());
    r.pin("c1");
    let l = rebuild(&mut r, &long2);
    assert!(l.steps.iter().all(|s| s.cached), "Long again regenerates nothing");
    // Without pinning, the tiny cache forgets the other configuration.
    let mut plain = regen();
    plain.cache_limit = 0;
    rebuild(&mut plain, &doc);
    rebuild(&mut plain, &long);
    assert!(!rebuild(&mut plain, &back).steps.iter().all(|s| s.cached));
}

#[test]
fn an_assembly_gets_the_part_as_the_configuration_it_names() {
    let doc = configured_block();
    // The active one: the document itself.
    assert_eq!(configured(&doc, "Default").unwrap(), doc);
    let long = configured(&doc, "Long").unwrap();
    assert_eq!(long, activate(&doc, "c2").unwrap());
    assert!(configured(&doc, "Nope").is_err());
    // A part without configurations has only Default.
    let plain = load("block-hole");
    assert_eq!(configured(&plain, DEFAULT_CONFIGURATION).unwrap(), plain);
    assert!(configured(&plain, "Long").is_err());
}

#[test]
fn the_active_configuration_is_captured_before_it_is_left() {
    // An edit made in Long (the depth set to 20 in the features, as the PropertyManager or
    // the Modify box with "This configuration" does) is kept as Long's when Default is
    // activated, and comes back with Long.
    let doc = configured_block();
    let mut long = activate(&doc, "Long").unwrap();
    let mut v: Value = serde_json::to_value(long.feature("f2").unwrap()).unwrap();
    v["end1"]["depth"] = 20.0.into();
    *long.feature_mut("f2").unwrap() = serde_json::from_value(v).unwrap();
    let def = activate(&long, "Default").unwrap();
    let depth = |d: &PartDocument| match &d.feature("f2").unwrap().kind {
        oc_doc::FeatureKind::Extrude(e) => e.end1.depth,
        _ => panic!(),
    };
    assert_eq!(depth(&def), 5.0);
    assert_eq!(def.configurations.as_ref().unwrap().get("c2").unwrap().values["f2/end1.depth"], 20.0);
    assert_eq!(depth(&activate(&def, "Long").unwrap()), 20.0);
    // The round trip through the file keeps all of it.
    let text = oc_doc::save(&def).unwrap();
    let again = oc_doc::load(&text).unwrap().doc;
    assert_eq!(again, def);
}
