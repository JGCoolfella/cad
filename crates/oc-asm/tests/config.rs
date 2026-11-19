//! Configurations in assemblies (G1e): each instance uses the configuration of its part it
//! references (a mesh and a regeneration per part and configuration), and the assembly's
//! own configurations suppress instances and mates, set mate values, choose the instances'
//! configurations and their linked display states.
mod common;

use std::path::{Path, PathBuf};

use common::*;
use oc_asm::{InstanceState, MateStatus, Memory};
use oc_doc::config::{apply_assembly, capture_assembly};
use oc_doc::*;
use serde_json::json;

/// The corpus parts in memory, the pin with two configurations: Default (the 20 mm shank)
/// and Long (30 mm).
fn parts_with_long_pin() -> Memory {
    let mut m = Memory::default();
    for n in ["plate", "pin"] {
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(corpus_dir().join(format!("{n}.ocpart"))).unwrap()).unwrap();
        if n == "pin" {
            v["version"] = 3.into();
            v["configurations"] = json!({ "active": "c1", "list": [
                { "id": "c1", "name": "Default", "values": { "f4/end1.depth": 20.0 } },
                { "id": "c2", "name": "Long", "values": { "f4/end1.depth": 30.0 } }
            ]});
        }
        m.0.insert(PathBuf::from(format!("/w/asm/{n}.ocpart")), serde_json::to_vec(&v).unwrap());
    }
    m
}

fn mates_ok(b: &oc_asm::Built) -> bool {
    b.mates.iter().all(|m| m.status == MateStatus::Ok)
}

#[test]
fn each_instance_uses_the_configuration_it_references() {
    let (mut doc, _, _) = corpus_assembly("bracket-pins");
    let m = parts_with_long_pin();
    let mut a = assembler();
    let b = a.build(&doc, Path::new("/w/asm"), &m, &opts(false));
    assert!(mates_ok(&b), "{:?}", b.mates);
    let part_of = |b: &oc_asm::Built, i: &str| b.occurrences.iter().find(|o| o.path == [Id::new(i)]).unwrap().part.clone();
    assert_eq!(part_of(&b, "i2"), part_of(&b, "i3"), "both pins in Default: one part");
    assert_eq!(a.regenerations, 2);
    // pin<2> references Long: its own part (hash), regenerated once, mated as before.
    doc.instances.iter_mut().find(|i| i.id == *"i3").unwrap().config = Some("Long".into());
    let b = a.build(&doc, Path::new("/w/asm"), &m, &opts(false));
    assert!(mates_ok(&b), "{:?}", b.mates);
    let (p2, p3) = (part_of(&b, "i2"), part_of(&b, "i3"));
    assert_ne!(p2, p3);
    assert_eq!(a.regenerations, 3, "Long regenerated; plate and Default pin were known");
    let len = |a: &oc_asm::Assembler<_>, h: &str| {
        let mesh = a.parts[h].mesh.clone().unwrap();
        mesh.origin[2] + mesh.bounds[0][2] as f64
    };
    assert!((len(&a, &p2) + 20.0).abs() < 1e-4 && (len(&a, &p3) + 30.0).abs() < 1e-4, "{} {}", len(&a, &p2), len(&a, &p3));
    let at = |b: &oc_asm::Built, id: &str| b.instances.iter().find(|i| i.id == *id).unwrap().transform.position.map(|x| (x * 1e6).round() / 1e6);
    assert_eq!(at(&b, "i3"), [45.0, 15.0, 8.0]);
    // The container caches a mesh per part and configuration in use.
    let c = a.container(&doc, &b, 3.0).unwrap();
    let cache = oc_asm::Cache::read(&c);
    assert!(cache.meshes.contains_key(&p2) && cache.meshes.contains_key(&p3) && cache.meshes.len() == 3);
    // A configuration the part does not have: its active one, with a warning on the
    // component.
    doc.instances.iter_mut().find(|i| i.id == *"i3").unwrap().config = Some("Missing".into());
    let b = a.build(&doc, Path::new("/w/asm"), &m, &opts(false));
    assert_eq!(part_of(&b, "i3"), p2);
    let pin = b.components.iter().find(|c| c.id == *"c2").unwrap();
    assert!(pin.warning.as_deref().is_some_and(|w| w.contains("Missing")), "{:?}", pin.warning);
}

#[test]
fn assembly_configurations_suppress_set_values_and_choose_configurations() {
    let (mut doc, _, _) = corpus_assembly("bracket-pins");
    // Default: everything; "One pin": pin<2> and its mates suppressed, pin<1> Long, and its
    // own display state (pin<1> red).
    doc.display_states = Some(DisplayStates {
        active: Id::new("d1"),
        list: vec![DisplayState { id: Id::new("d1"), name: "Display State-1".into(), instances: Default::default() }, DisplayState {
            id: Id::new("d2"),
            name: "Display State-2".into(),
            instances: [(Id::new("i2"), Look { color: Some("#ff0000".into()), ..Look::default() })].into_iter().collect(),
        }],
    });
    let mut one = Configuration::new("c2", "One pin");
    for id in ["i3", "m3", "m4"] {
        one.suppressed.insert(Id::new(id), true);
    }
    one.components.insert(Id::new("i2"), "Long".into());
    one.display_state = Some(Id::new("d2"));
    let mut def = Configuration::new("c1", "Default");
    for id in ["i3", "m3", "m4"] {
        def.suppressed.insert(Id::new(id), false);
    }
    def.components.insert(Id::new("i2"), "Default".into());
    def.display_state = Some(Id::new("d1"));
    doc.configurations = Some(Configurations { active: Id::new("c1"), list: vec![def, one] });
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
    let original = doc.clone();

    let mut d = doc.clone();
    capture_assembly(&mut d);
    apply_assembly(&mut d, "One pin").unwrap();
    let inst = |d: &AssemblyDocument, id: &str| d.instances.iter().find(|i| i.id == *id).unwrap().clone();
    assert!(inst(&d, "i3").is_suppressed());
    assert!(d.mates.iter().filter(|m| m.id == *"m3" || m.id == *"m4").all(|m| m.is_suppressed()));
    assert_eq!(inst(&d, "i2").config.as_deref(), Some("Long"));
    assert_eq!(inst(&d, "i2").color.as_deref(), Some("#ff0000"));
    assert_eq!(d.display_states.as_ref().unwrap().active.as_str(), "d2");
    let m = parts_with_long_pin();
    let mut a = assembler();
    let b = a.build(&d, Path::new("/w/asm"), &m, &opts(false));
    assert_eq!(b.instances.iter().find(|i| i.id == *"i3").unwrap().state, InstanceState::Suppressed);
    assert_eq!(b.occurrences.len(), 2);
    assert!(b.mates.iter().filter(|x| x.id == *"m1" || x.id == *"m2").all(|x| x.status == MateStatus::Ok));
    assert_eq!(b.dof, 1);

    // Back to Default: the document as it was (looks captured into d2 on the way).
    capture_assembly(&mut d);
    apply_assembly(&mut d, "c1").unwrap();
    let mut want = original.clone();
    capture_assembly(&mut want);
    assert_eq!(d.instances, want.instances);
    assert_eq!(d.mates, want.mates);
    // A look changed while One pin is active stays with its display state.
    capture_assembly(&mut d);
    apply_assembly(&mut d, "c2").unwrap();
    d.instances.iter_mut().find(|i| i.id == *"i1").unwrap().hidden = Some(true);
    capture_assembly(&mut d);
    apply_assembly(&mut d, "c1").unwrap();
    assert_eq!(inst(&d, "i1").hidden, None);
    capture_assembly(&mut d);
    apply_assembly(&mut d, "c2").unwrap();
    assert_eq!(inst(&d, "i1").hidden, Some(true));
    // A configured mate value: a Distance mate's value per configuration.
    let mut dd = original.clone();
    dd.mates.iter_mut().find(|m| m.id == *"m2").unwrap().value = Some(0.0);
    let cfgs = dd.configurations.as_mut().unwrap();
    cfgs.list[0].values.insert("m2/value".into(), 0.0);
    cfgs.list[1].values.insert("m2/value".into(), 4.0);
    capture_assembly(&mut dd);
    apply_assembly(&mut dd, "c2").unwrap();
    assert_eq!(dd.mates.iter().find(|m| m.id == *"m2").unwrap().value, Some(4.0));
    // And the file keeps all of it.
    let text = save_assembly(&d).unwrap();
    assert_eq!(load_assembly(&text).unwrap().doc, d);
}
