//! In-context references (G1d, Edit Part in an assembly): a sketch on another component's
//! face and sketch geometry converted from another component's edge. Both carry their
//! geometry in this part's coordinates (the assembly updates it); the rebuild uses it and
//! never looks for it on this part's body.
mod common;

use common::*;
use oc_doc::{FeatureKind, SketchEntity};
use oc_kernel::Kernel;
use serde_json::json;

/// The corpus pin (a Ø10 × 3 head and a Ø6 × 20 shank on the Front plane), with its head
/// sketch moved onto another component's face 20 mm up and a construction line converted
/// from another component's edge 30 mm up.
fn pin_in_context() -> oc_doc::PartDocument {
    let text = std::fs::read_to_string(corpus_dir().join("assemblies/pin.ocpart")).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let ext = |label: &str| json!({ "assembly": "C:/w/asm/a.ocasm", "component": ["i1"], "context": ["i2"], "entity": { "kind": "face" }, "label": label });
    let f1 = &mut v["features"][0];
    f1["plane"] = json!({ "kind": "external", "sig": { "surface": "PLANE", "center": [4, 5, 20], "normal": [0, 0, 1], "area": 100 }, "ext": ext("Face@plate<1>") });
    let sk = &mut f1["sketch"];
    sk["points"]["x_p1"] = json!({ "id": "x_p1", "x": 0, "y": 0, "ext": "x1" });
    sk["points"]["x_p2"] = json!({ "id": "x_p2", "x": 1, "y": 0, "ext": "x1" });
    sk["entities"]["x_l"] = json!({ "id": "x_l", "type": "line", "p1": "x_p1", "p2": "x_p2", "construction": true, "ext": "x1" });
    sk["refs"] = json!({ "x1": { "kind": "external", "id": "x1", "sig": { "curve": "LINE", "a": [2, 3, 30], "b": [12, 8, 30], "mid": [7, 5.5, 30], "length": 11.18 }, "ext": ext("Edge@plate<1>") } });
    oc_doc::load(&serde_json::to_string(&v).unwrap()).unwrap().doc
}

#[test]
fn a_sketch_on_another_components_face_uses_the_stored_plane() {
    let doc = pin_in_context();
    // The document round-trips the references unchanged.
    let FeatureKind::Sketch(s) = &doc.features[0].kind else { panic!() };
    assert!(matches!(&s.plane, oc_doc::PlaneRef::External { ext, .. } if ext.label == "Face@plate<1>" && ext.component.len() == 1));
    let again = oc_doc::load(&oc_doc::save(&doc).unwrap()).unwrap().doc;
    assert_eq!(again.features[0], doc.features[0]);
    // No body is read for the sketch: its references carry their geometry.
    assert!(!oc_graph::graph::inputs(&doc.features[0]).1);

    let mut r = regen();
    let out = rebuild(&mut r, &doc);
    assert!(out.steps.iter().all(|s| s.status.error.is_none()), "{:?}", out.steps.iter().map(|s| &s.status).collect::<Vec<_>>());
    let f = out.frames["f1"];
    // As for a sketch on one of the part's own faces: the part origin projected onto it.
    assert_eq!(f.origin, [0.0, 0.0, 20.0]);
    assert_eq!(f.normal, [0.0, 0.0, 1.0]);
    // The head is extruded from that plane: z from 20 to 23 (the shank stays on the Front
    // plane, z from 0 to −20).
    let k = r.kernel();
    let bb = k.bounding_box(&out.body.as_ref().unwrap().body).unwrap();
    assert!((bb.max[2] - 23.0).abs() < 1e-6 && (bb.min[2] + 20.0).abs() < 1e-6, "{bb:?}");
    // The converted line lies where the other component's edge projects onto the plane.
    let sk = &out.sketches["f1"];
    let SketchEntity::Line { p1, p2, .. } = &sk.entities["x_l"] else { panic!() };
    let (a, b) = (&sk.points[p1.as_str()], &sk.points[p2.as_str()]);
    let at = |p: &oc_doc::SketchPoint| oc_graph::math::to_world(&f, [p.x, p.y]);
    let (wa, wb) = (at(a), at(b));
    for (w, want) in [(wa, [2.0, 3.0, 20.0]), (wb, [12.0, 8.0, 20.0])] {
        assert!((0..3).all(|i| (w[i] - want[i]).abs() < 1e-9), "{w:?} vs {want:?}");
    }
}
