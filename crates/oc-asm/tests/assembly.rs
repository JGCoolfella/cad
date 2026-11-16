//! Assembly building: shared geometry, mates following part edits, Missing references,
//! the file search, lightweight opening, and the file format.
mod common;

use std::path::{Path, PathBuf};

use common::*;
use oc_asm::{Cache, InstanceState, MateStatus, Memory};
use oc_doc::container::Container;
use oc_doc::*;

fn part_text(name: &str) -> String {
    std::fs::read_to_string(corpus_dir().join(format!("{name}.ocpart"))).unwrap()
}

/// The corpus parts in memory under `/w/asm/`, with `edit` applied to their JSON.
fn memory(edit: impl Fn(&str, &mut serde_json::Value)) -> Memory {
    let mut m = Memory::default();
    for n in ["plate", "pin", "arm"] {
        let mut v: serde_json::Value = serde_json::from_str(&part_text(n)).unwrap();
        edit(n, &mut v);
        m.0.insert(PathBuf::from(format!("/w/asm/{n}.ocpart")), serde_json::to_vec(&v).unwrap());
    }
    m
}

fn instance_of<'a>(b: &'a oc_asm::Built, id: &str) -> &'a oc_asm::InstanceOut {
    b.instances.iter().find(|i| i.id == *id).unwrap()
}

fn mate_of<'a>(b: &'a oc_asm::Built, id: &str) -> &'a oc_asm::MateOut {
    b.mates.iter().find(|m| m.id == *id).unwrap()
}

#[test]
fn identical_parts_share_one_regeneration_and_mesh() {
    let (doc, a, built) = corpus_assembly("bracket-pins");
    assert_eq!(a.parts.len(), 2);
    let pins: Vec<&oc_asm::Occurrence> = built.occurrences.iter().filter(|o| o.path[0] != *"i1").collect();
    assert_eq!(pins.len(), 2);
    assert_eq!(pins[0].part, pins[1].part, "both pins draw one mesh");
    // The same pin saved under another file name is still the same part.
    let mut m = memory(|_, _| {});
    m.0.insert(PathBuf::from("/w/asm/pin-copy.ocpart"), part_text("pin").into_bytes());
    let mut d = doc.clone();
    d.components.push_back(component("c3", "pin-copy.ocpart"));
    d.instances.push_back(instance("i4", "c3", "pin-copy<1>", false, place([0.0, 0.0, 1.0], 0.0, [0.0, 0.0, 50.0])));
    let mut a = assembler();
    let b = a.build(&d, Path::new("/w/asm"), &m, &opts(false));
    assert_eq!(a.regenerations, 2, "plate and pin, once each, for four instances");
    assert_eq!(part_hash_of(&b, "c2"), part_hash_of(&b, "c3"));
    assert_eq!(instance_of(&b, "i4").state, InstanceState::Under);
    assert_eq!(instance_of(&b, "i4").free, 6);
    assert_eq!(b.dof, 2 + 6);
}

#[test]
fn mates_follow_an_edited_part() {
    let (doc, _, _) = corpus_assembly("bracket-pins");
    // Move the plate's first hole from (15, 15) to (20, 12) and make the plate 10 thick.
    let m = memory(|n, v| {
        if n == "plate" {
            v["features"][2]["sketch"]["points"]["f3_p1"]["x"] = 20.into();
            v["features"][2]["sketch"]["points"]["f3_p1"]["y"] = 12.into();
            v["features"][1]["end1"]["depth"] = 10.into();
        }
    });
    let mut a = assembler();
    let b = a.build(&doc, Path::new("/w/asm"), &m, &opts(false));
    assert!(b.mates.iter().all(|m| m.status == MateStatus::Ok), "{:?}", b.mates);
    assert_eq!(instance_of(&b, "i2").transform.position.map(|x| (x * 1e6).round() / 1e6), [20.0, 12.0, 10.0]);
    assert_eq!(instance_of(&b, "i3").transform.position.map(|x| (x * 1e6).round() / 1e6), [45.0, 15.0, 10.0]);
    // The changed references are reported for writing back.
    let healed: Vec<(String, usize)> = b.heals.iter().map(|h| (h.mate.to_string(), h.side)).collect();
    assert!(healed.contains(&("m1".into(), 0)), "the moved hole: {healed:?}");
    assert!(healed.contains(&("m2".into(), 0)) && healed.contains(&("m4".into(), 0)), "the raised top face: {healed:?}");
    assert!(!healed.iter().any(|(_, s)| *s == 1), "nothing on the pins changed");
    // After writing them back, the same build has nothing left to heal.
    let mut d = doc.clone();
    oc_asm::apply_heals(&mut d, &b.heals);
    oc_asm::apply_placements(&mut d, &b);
    let b2 = a.build(&d, Path::new("/w/asm"), &m, &opts(false));
    assert!(b2.heals.is_empty());
    assert_eq!(a.regenerations, 2, "the second build regenerated nothing");
}

#[test]
fn a_reference_that_is_gone_is_missing() {
    let (doc, _, _) = corpus_assembly("bracket-pins");
    // Delete the holes (the cut and its sketch).
    let m = memory(|n, v| {
        if n == "plate" {
            let f = v["features"].as_array_mut().unwrap();
            f.truncate(2);
        }
    });
    let mut a = assembler();
    let b = a.build(&doc, Path::new("/w/asm"), &m, &opts(false));
    for id in ["m1", "m3"] {
        assert_eq!(mate_of(&b, id).status, MateStatus::Missing, "{id}");
        assert_eq!(mate_of(&b, id).missing, vec![0], "{id}: the hole side");
    }
    assert_eq!(mate_of(&b, "m2").status, MateStatus::Ok);
    // Each pin keeps only its face on the plate: three degrees of freedom.
    assert_eq!(instance_of(&b, "i2").free, 3);
    assert_eq!(b.dof, 6);
}

#[test]
fn files_are_found_by_the_fallback_search() {
    let (mut doc, _, _) = corpus_assembly("hinge");
    // The arm moved next to the assembly; the pin is only at its old absolute path.
    let mut m = Memory::default();
    m.0.insert(PathBuf::from("/w/asm/plate.ocpart"), part_text("plate").into_bytes());
    m.0.insert(PathBuf::from("/w/asm/arm.ocpart"), part_text("arm").into_bytes());
    m.0.insert(PathBuf::from("/old/pin.ocpart"), part_text("pin").into_bytes());
    doc.components[1].path = "parts/arm.ocpart".into();
    doc.components[2].path = "gone/pin.ocpart".into();
    doc.components[2].abs_path = Some("/old/pin.ocpart".into());
    let mut a = assembler();
    let b = a.build(&doc, Path::new("/w/asm"), &m, &opts(false));
    let found: Vec<Option<oc_asm::Found>> = b.components.iter().map(|c| c.found).collect();
    assert_eq!(found, vec![Some(oc_asm::Found::Relative), Some(oc_asm::Found::AssemblyFolder), Some(oc_asm::Found::Absolute)]);
    assert!(b.mates.iter().all(|m| m.status == MateStatus::Ok));
    // Not found at all: an error on the component, and its mates are missing.
    m.0.remove(Path::new("/old/pin.ocpart"));
    let b = a.build(&doc, Path::new("/w/asm"), &m, &opts(false));
    assert!(b.components[2].error.as_deref().unwrap().contains("not found"));
    assert_eq!(mate_of(&b, "m4").status, MateStatus::Missing);
}

#[test]
fn lightweight_open_uses_the_caches() {
    let bytes = std::fs::read(corpus_dir().join("hinge.ocasm")).unwrap();
    let c = Container::from_bytes(&bytes).unwrap();
    let doc = load_assembly(c.document().unwrap()).unwrap().doc;
    let m = memory(|_, _| {});
    let mut a = assembler();
    a.set_cache(Cache::read(&c));
    let b = a.build(&doc, Path::new("/w/asm"), &m, &opts(true));
    assert_eq!(a.regenerations, 0, "every part from its cached mesh");
    assert!(a.parts.values().all(|p| p.live.is_none() && p.mesh.is_some()));
    assert_eq!(b.dof, 2);
    // Picking geometry resolves just that part.
    let arm = part_hash_of(&b, "c2");
    assert!(a.entity(&arm, "face", 0, 3.0).is_some());
    assert_eq!(a.regenerations, 1);
    // An index the part does not have is refused, not a panic.
    for kind in ["face", "edge", "vertex"] {
        assert!(a.entity(&arm, kind, 100_000, 3.0).is_none(), "{kind}");
    }
    assert!(a.entity(&arm, "solid", 0, 3.0).is_none());
    // An edited part no longer matches its cache and is regenerated; the others are not.
    let edited = memory(|n, v| {
        if n == "pin" {
            v["features"][1]["end1"]["depth"] = 4.into();
        }
    });
    let mut a = assembler();
    a.set_cache(Cache::read(&c));
    a.build(&doc, Path::new("/w/asm"), &edited, &opts(true));
    assert_eq!(a.regenerations, 1);
    // A part file that is gone is shown from the cache, with a warning.
    let mut gone = memory(|_, _| {});
    gone.0.remove(Path::new("/w/asm/pin.ocpart"));
    let mut a = assembler();
    a.set_cache(Cache::read(&c));
    let b = a.build(&doc, Path::new("/w/asm"), &gone, &opts(true));
    assert!(b.components[2].warning.as_deref().unwrap().contains("cache"));
    assert_eq!(b.occurrences.len(), 3);
    assert!(b.mates.iter().all(|m| m.status == MateStatus::Ok), "lightweight mates use their stored geometry");
}

#[test]
fn assembly_files_round_trip() {
    let bytes = std::fs::read(corpus_dir().join("bracket-pins.ocasm")).unwrap();
    let c = Container::from_bytes(&bytes).unwrap();
    let text = c.document().unwrap();
    let doc = load_assembly(text).unwrap().doc;
    assert!(doc.validate().is_empty());
    let saved = save_assembly(&doc).unwrap();
    assert_eq!(saved, text, "saving is byte-identical");
    assert_eq!(load_assembly(&saved).unwrap().doc, doc);
    // The container written again is the same bytes.
    let mut again = Container::new();
    for n in c.names() {
        again.insert(n, c.get(n).unwrap().to_vec());
    }
    assert_eq!(again.to_bytes(), bytes);
}

#[test]
fn strict_loading_and_migration() {
    let text = Container::from_bytes(&std::fs::read(corpus_dir().join("hinge.ocasm")).unwrap()).unwrap().document().unwrap().to_string();
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["mates"][0]["colour"] = "red".into();
    match load_assembly(&v.to_string()) {
        Err(LoadError::Lossy { path, .. }) => assert_eq!(path, "$.mates[0].colour"),
        other => panic!("unknown field accepted: {other:?}"),
    }
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["format"] = "opencad-part".into();
    assert!(matches!(load_assembly(&v.to_string()), Err(LoadError::NotAnAssembly(_))));
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["version"] = 99.into();
    assert!(matches!(load_assembly(&v.to_string()), Err(LoadError::Migration(_))));
    // A made-up version 0 that called instances "occurrences" migrates to version 1.
    fn rename(root: &mut serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
        let v = root.remove("occurrences").ok_or("no occurrences")?;
        root.insert("instances".into(), v);
        Ok(())
    }
    let reg = Migrations { current: ASM_CURRENT_VERSION, steps: &[Migration { from: 0, description: "test", apply: rename }] };
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let inst = v.as_object_mut().unwrap().remove("instances").unwrap();
    v["occurrences"] = inst;
    v["version"] = 0.into();
    let l = oc_doc::io::load_assembly_with(&v.to_string(), &reg).unwrap();
    assert_eq!(l.source_version, 0);
    assert_eq!(save_assembly(&l.doc).unwrap(), text);
    // Rotations are normalised when read.
    let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
    v["instances"][0]["transform"]["rotation"] = serde_json::json!([2.0, 0.0, 0.0, 0.0]);
    assert_eq!(load_assembly(&v.to_string()).unwrap().doc.instances[0].transform.rotation, [1.0, 0.0, 0.0, 0.0]);
}

#[test]
fn history_holds_assemblies() {
    let (doc, _, _) = corpus_assembly("hinge");
    let mut h: History<AssemblyDocument> = History::new(doc.clone());
    h.transact("Fix", |d| -> Result<(), ()> {
        d.instances[1].fixed = true;
        Ok(())
    })
    .unwrap();
    assert!(h.is_dirty());
    h.undo();
    assert_eq!(**h.doc(), doc);
    assert!(!h.is_dirty());
}

#[test]
fn suppressed_instances_and_mates() {
    let (mut doc, _, _) = corpus_assembly("hinge");
    doc.instances[2].suppressed = Some(true);
    doc.mates[0].suppressed = Some(true);
    let mut a = assembler();
    let b = a.build(&doc, &corpus_dir(), &oc_asm::Disk, &opts(false));
    assert_eq!(instance_of(&b, "i3").state, InstanceState::Suppressed);
    assert_eq!(b.occurrences.len(), 2);
    assert_eq!(mate_of(&b, "m1").status, MateStatus::Suppressed);
    assert_eq!(mate_of(&b, "m3").status, MateStatus::Suppressed, "a mate to a suppressed component is off");
    // The arm keeps only the concentric mate: slide and turn on the axis.
    assert_eq!(instance_of(&b, "i2").free, 2);
}

#[test]
fn dragging_turns_the_hinge_arm_about_its_pin() {
    let (_, _, mut built) = corpus_assembly("hinge");
    let axis = [15.0, 15.0, 8.0];
    let before = instance_of(&built, "i2").transform;
    // Grab the arm's far end (local (45, 5, 6), 40 mm from the hole) and pull it round.
    let grab = [45.0, 5.0, 6.0];
    let target = [15.0, 60.0, 14.0];
    assert!(oc_asm::drag(&mut built, "i2", grab, target, 1000.0));
    let t = instance_of(&built, "i2").transform;
    let p = t.apply(grab);
    let r = ((p[0] - axis[0]).powi(2) + (p[1] - axis[1]).powi(2)).sqrt();
    assert!((r - 40.0).abs() < 1e-6, "turns about the hole: radius {r}");
    assert!((p[2] - 14.0).abs() < 1e-6, "stays on the plate");
    // The target is 45 mm from the axis; the arm's end gets as near as its circle allows
    // (to 1e-4 mm: near the nearest point the distance changes only to second order).
    assert!((p[0] - 15.0).abs() < 1e-4 && (p[1] - 55.0).abs() < 1e-6, "points at the target: {p:?}");
    assert_ne!(t, before);
    // The pin turns about its own axis only; the fixed plate cannot be dragged.
    assert!(!oc_asm::drag(&mut built, "i1", [0.0; 3], [10.0, 0.0, 0.0], 1000.0));
    // The occurrence of the arm follows its instance.
    let occ = built.occurrences.iter().find(|o| o.path[0] == *"i2").unwrap();
    assert_eq!(occ.world, t);
}
