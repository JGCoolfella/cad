//! Evaluate in an assembly (G1c): Interference Detection volumes (known overlaps,
//! coincident faces, none), the selection rules, hidden components, and the naming case
//! G1b found (deleting one of two holes cut from one sketch makes its mate Missing).
mod common;

use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use common::*;
use oc_asm::{InterferenceOptions, MateStatus, Memory};
use oc_doc::*;
use oc_kernel::Kernel;

/// bracket-pins with pin<2> (i3) unmated and put at `at` (upright).
fn with_pin2_at(at: [f64; 3]) -> (AssemblyDocument, oc_asm::Assembler<oc_kernel_occt::OcctKernel>, oc_asm::Built) {
    let (mut doc, mut a, _) = corpus_assembly("bracket-pins");
    doc.mates = doc.mates.iter().filter(|m| m.id != *"m3" && m.id != *"m4").cloned().collect();
    doc.instances.iter_mut().find(|i| i.id == *"i3").unwrap().transform = Transform { position: at, rotation: [1.0, 0.0, 0.0, 0.0] };
    let b = a.build(&doc, &corpus_dir(), &oc_asm::Disk, &opts(false));
    (doc, a, b)
}

fn occurrence(b: &oc_asm::Built, inst: &str) -> usize {
    b.occurrences.iter().position(|o| o.path == [Id::new(inst)]).unwrap()
}

fn pair(b: &oc_asm::Built, x: &oc_asm::Interference<impl Sized>) -> (String, String) {
    (b.occurrences[x.a].path[0].to_string(), b.occurrences[x.b].path[0].to_string())
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn a_seated_assembly_has_no_interference_but_coincidences() {
    let (_, mut a, b) = corpus_assembly("bracket-pins");
    let r = a.interferences(&b, &InterferenceOptions::default(), 3.0).unwrap();
    assert!(r.interferences.is_empty(), "{:?}", r.interferences.iter().map(|x| (x.a, x.b, x.volume)).collect::<Vec<_>>());
    assert_eq!(r.pairs, 3);
    // The pins are 30 mm apart: their boxes do not meet, so only plate/pin pairs are checked.
    assert_eq!(r.candidates, 2);
    // Treat coincidence as interference: each pin's shank fills its hole exactly and its
    // head sits on the plate.
    let opts = InterferenceOptions { coincidence: true, ..Default::default() };
    let r = a.interferences(&b, &opts, 3.0).unwrap();
    let found: Vec<(String, String)> = r.interferences.iter().map(|x| pair(&b, x)).collect();
    assert_eq!(found, vec![("i1".into(), "i2".into()), ("i1".into(), "i3".into())]);
    assert!(r.interferences.iter().all(|x| x.coincident && x.volume == 0.0 && x.body.is_none()));
}

#[test]
fn interference_volumes_are_exact() {
    // pin<2> stands between the holes: its shank (r 3) goes through the 8 mm plate.
    let (_, mut a, b) = with_pin2_at([30.0, 15.0, 8.0]);
    let r = a.interferences(&b, &InterferenceOptions::default(), 3.0).unwrap();
    assert_eq!(r.interferences.len(), 1);
    let x = &r.interferences[0];
    assert_eq!(pair(&b, x), ("i1".into(), "i3".into()));
    assert!(close(x.volume, PI * 9.0 * 8.0), "{}", x.volume);
    assert!(!x.coincident && x.body.is_some());
    assert!(close(x.center[0], 30.0) && close(x.center[1], 15.0) && close(x.center[2], 4.0), "{:?}", x.center);

    // 7 mm from pin<1>: the heads (r 5, 3 high) overlap in a lens, and the shank is in solid
    // plate (7 mm from the hole's axis, 6 mm apart at most: no overlap with the hole).
    let (_, mut a, b) = with_pin2_at([22.0, 15.0, 8.0]);
    let r = a.interferences(&b, &InterferenceOptions::default(), 3.0).unwrap();
    let lens = |r: f64, d: f64| 2.0 * r * r * (d / (2.0 * r)).acos() - d / 2.0 * (4.0 * r * r - d * d).sqrt();
    let got: Vec<((String, String), f64)> = r.interferences.iter().map(|x| (pair(&b, x), x.volume)).collect();
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0].0, ("i1".into(), "i3".into()));
    assert!(close(got[0].1, PI * 9.0 * 8.0), "{got:?}");
    assert_eq!(got[1].0, ("i2".into(), "i3".into()));
    assert!(close(got[1].1, 3.0 * lens(5.0, 7.0)), "{got:?}: lens {}", 3.0 * lens(5.0, 7.0));
}

#[test]
fn selected_components_and_hidden_ones() {
    let (mut doc, mut a, b) = with_pin2_at([22.0, 15.0, 8.0]);
    let sel = |ids: &[&str]| ids.iter().map(|i| vec![Id::new(i)]).collect::<Vec<_>>();
    let run = |a: &mut oc_asm::Assembler<_>, b: &oc_asm::Built, o: InterferenceOptions| -> Vec<(String, String)> { a.interferences(b, &o, 3.0).unwrap().interferences.iter().map(|x| pair(b, x)).collect() };
    // One component: everything that involves it.
    assert_eq!(run(&mut a, &b, InterferenceOptions { selected: sel(&["i2"]), ..Default::default() }), vec![("i2".to_string(), "i3".to_string())]);
    assert_eq!(run(&mut a, &b, InterferenceOptions { selected: sel(&["i3"]), ..Default::default() }).len(), 2);
    // Two or more: only between them.
    assert_eq!(run(&mut a, &b, InterferenceOptions { selected: sel(&["i1", "i2"]), ..Default::default() }), Vec::<(String, String)>::new());
    assert_eq!(run(&mut a, &b, InterferenceOptions { selected: sel(&["i1", "i3"]), ..Default::default() }), vec![("i1".to_string(), "i3".to_string())]);
    // A hidden component is ignored unless hidden components are included.
    doc.instances.iter_mut().find(|i| i.id == *"i3").unwrap().hidden = Some(true);
    let b = a.build(&doc, &corpus_dir(), &oc_asm::Disk, &opts(false));
    assert!(b.occurrences[occurrence(&b, "i3")].hidden);
    assert!(run(&mut a, &b, InterferenceOptions::default()).is_empty());
    assert_eq!(run(&mut a, &b, InterferenceOptions { include_hidden: true, ..Default::default() }).len(), 2);
}

#[test]
fn entities_are_found_on_placed_bodies_by_geometry() {
    // Measure in an assembly picks by the part's entity index; the placed (transformed)
    // body may number entities differently, so they are matched by geometry.
    let (_, mut a, b) = with_pin2_at([30.0, 15.0, 8.0]);
    let k = occurrence(&b, "i3");
    let placed = a.placed_body(&b, k, 3.0).unwrap();
    let hash = b.occurrences[k].part.clone();
    let shape = a.parts[&hash].live.as_ref().unwrap().shape.clone().unwrap();
    let kernel = a.regen.kernel();
    let part = shape.topo(kernel).unwrap().clone();
    let world = kernel.topology(&placed).unwrap();
    for f in 0..part.faces.len() as u32 {
        let at = a.placed_entity(&b, k, &placed, "face", f, 3.0).unwrap();
        let (want, got) = (b.occurrences[k].world.apply(part.faces[f as usize].centroid), world.faces[at as usize].centroid);
        assert!((0..3).all(|i| (want[i] - got[i]).abs() < 1e-6), "face {f}: {want:?} vs {got:?}");
    }
    // The pin's head top (z = 11) is 3 mm from the plate's top (z = 8).
    let head = (0..part.faces.len() as u32).find(|&f| part.faces[f as usize].normal[2] > 0.999 && (part.faces[f as usize].centroid[2] - 3.0).abs() < 1e-9).unwrap();
    let plate = occurrence(&b, "i1");
    let plate_body = a.placed_body(&b, plate, 3.0).unwrap();
    let plate_topo = a.regen.kernel().topology(&plate_body).unwrap();
    let top = (0..plate_topo.faces.len()).find(|&f| plate_topo.faces[f].normal[2] > 0.999 && (plate_topo.faces[f].centroid[2] - 8.0).abs() < 1e-9).unwrap() as u32;
    let at = a.placed_entity(&b, k, &placed, "face", head, 3.0).unwrap();
    let d = a.regen.kernel().distance(oc_kernel::Target::Face(&plate_body, oc_kernel::FaceId(top)), oc_kernel::Target::Face(&placed, oc_kernel::FaceId(at))).unwrap();
    assert!((d.distance - 3.0).abs() < 1e-9, "{}", d.distance);
}

#[test]
fn lightweight_parts_are_resolved_to_check_them() {
    let bytes = std::fs::read(corpus_dir().join("hinge.ocasm")).unwrap();
    let c = oc_doc::container::Container::from_bytes(&bytes).unwrap();
    let doc = load_assembly(c.document().unwrap()).unwrap().doc;
    let mut a = assembler();
    a.set_cache(oc_asm::Cache::read(&c));
    let b = a.build(&doc, &corpus_dir(), &oc_asm::Disk, &opts(true));
    assert_eq!(a.regenerations, 0);
    let r = a.interferences(&b, &InterferenceOptions { coincidence: true, ..Default::default() }, 3.0).unwrap();
    assert!(r.skipped.is_empty());
    assert!(r.interferences.iter().all(|x| x.coincident), "the hinge is seated, nothing overlaps");
    assert_eq!(r.interferences.len(), 3, "arm on plate, pin in arm, pin in plate's hole");
    assert_eq!(a.regenerations, 3);
}

#[test]
fn deleting_one_of_two_holes_from_one_sketch_makes_its_mate_missing() {
    // Owner decision 4 (G1b's naming finding): the plate's two holes are one cut from one
    // sketch. Deleting the circle of the first hole must not move its concentric mate onto
    // the second hole (the only face of that cut and role left); the mate is Missing, as in
    // SOLIDWORKS, and the other pin's mates are untouched.
    let (doc, _, _) = corpus_assembly("bracket-pins");
    let mut m = Memory::default();
    for n in ["plate", "pin"] {
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(corpus_dir().join(format!("{n}.ocpart"))).unwrap()).unwrap();
        if n == "plate" {
            let sketch = &mut v["features"][2]["sketch"];
            sketch["entities"].as_object_mut().unwrap().remove("f3_c2");
            sketch["points"].as_object_mut().unwrap().remove("f3_p1");
        }
        m.0.insert(PathBuf::from(format!("/w/asm/{n}.ocpart")), serde_json::to_vec(&v).unwrap());
    }
    let mut a = assembler();
    let b = a.build(&doc, Path::new("/w/asm"), &m, &opts(false));
    let status = |id: &str| b.mates.iter().find(|x| x.id == *id).unwrap().clone();
    assert_eq!(status("m1").status, MateStatus::Missing, "{:?}", b.mates);
    assert_eq!(status("m1").missing, vec![0], "the plate's side");
    for id in ["m2", "m3", "m4"] {
        assert_eq!(status(id).status, MateStatus::Ok, "{id}");
    }
    assert!(!b.heals.iter().any(|h| h.mate == *"m1"), "nothing written back for the missing hole: {:?}", b.heals);
    // The second hole's axis came out reversed from the kernel after the edit; the mate
    // keeps the sense it was picked with, so its alignment still holds.
    let healed = b.heals.iter().find(|h| h.mate == *"m3").map(|h| h.entity.clone());
    if let Some(MateEntity::Face { geom: MateGeom::Cylinder { d, .. }, .. }) = healed {
        assert!(d[2] < 0.0, "{d:?}");
    }
    // pin<2> is still in the second hole; pin<1> keeps only its face on the plate.
    let at = |id: &str| b.instances.iter().find(|i| i.id == *id).unwrap().clone();
    assert_eq!(at("i3").transform.position.map(|x| (x * 1e6).round() / 1e6), [45.0, 15.0, 8.0]);
    assert_eq!(at("i2").free, 3);
}
