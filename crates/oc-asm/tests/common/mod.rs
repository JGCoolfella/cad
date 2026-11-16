//! Shared by the assembly tests: the corpus folder, picking faces on parts, and building
//! the corpus assemblies the way the app's Mate PropertyManager would (faces picked on
//! regenerated parts, so references carry persistent names; alignment the one closest to
//! the current placement).
#![allow(dead_code)]
use std::path::{Path, PathBuf};

use oc_asm::geom::face_geom;
use oc_asm::{Assembler, Built, Disk, Options};
use oc_doc::*;
use oc_kernel::FaceId;
use oc_kernel_occt::OcctKernel;

pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/assemblies")
}

pub fn assembler() -> Assembler<OcctKernel> {
    Assembler::new(OcctKernel::default())
}

pub fn opts(lightweight: bool) -> Options {
    Options { lightweight, ..Options::default() }
}

/// Rotation by `deg` about `axis` with translation `t`.
pub fn place(axis: [f64; 3], deg: f64, t: [f64; 3]) -> Transform {
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    let (s, c) = (deg.to_radians() / 2.0).sin_cos();
    Transform { position: t, rotation: [c, axis[0] / n * s, axis[1] / n * s, axis[2] / n * s] }
}

pub fn component(id: &str, path: &str) -> Component {
    Component { id: Id::new(id), kind: ComponentKind::Part, path: path.into(), abs_path: None, config: "Default".into() }
}

pub fn instance(id: &str, comp: &str, name: &str, fixed: bool, t: Transform) -> Instance {
    Instance { id: Id::new(id), component: Id::new(comp), name: name.into(), fixed, transform: t, suppressed: None, hidden: None }
}

/// Which face of a part.
pub enum Want {
    /// Planar, outward normal `n`, at height `z` (world Z of the part).
    Plane([f64; 3], f64),
    /// Cylinder of radius 3 whose axis passes through (x, y).
    Cyl(f64, f64),
}

/// The mate entity of the one face of part `hash` matching `want`.
pub fn pick(a: &mut Assembler<OcctKernel>, hash: &str, want: &Want) -> MateEntity {
    assert!(a.resolve(hash, 3.0));
    let k = a.regen.kernel();
    let part = &a.parts[hash];
    let shape = part.live.as_ref().unwrap().shape.as_ref().unwrap();
    let topo = shape.topo(k).unwrap();
    let hits: Vec<u32> = (0..topo.faces.len() as u32)
        .filter(|&i| match (face_geom(topo, FaceId(i)), want) {
            (Some(MateGeom::Plane { p, n }), Want::Plane(m, z)) => (0..3).all(|j| (n[j] - m[j]).abs() < 1e-9) && (p[2] - z).abs() < 1e-9,
            (Some(MateGeom::Cylinder { p, d, r }), Want::Cyl(x, y)) => (r - 3.0).abs() < 1e-9 && d[2].abs() > 1.0 - 1e-9 && (p[0] - x).abs() < 1e-9 && (p[1] - y).abs() < 1e-9,
            _ => false,
        })
        .collect();
    assert_eq!(hits.len(), 1, "one face of {} matches", part.name);
    a.entity(hash, "face", hits[0], 3.0).unwrap()
}

pub fn geom_dir(e: &MateEntity) -> [f64; 3] {
    match e {
        MateEntity::Face { geom: MateGeom::Cylinder { d, .. }, .. } => *d,
        MateEntity::Face { geom: MateGeom::Plane { n, .. }, .. } => *n,
        _ => panic!("not a face"),
    }
}

pub fn mref(inst: &str, entity: MateEntity) -> MateRef {
    MateRef { occurrence: vec![Id::new(inst)], entity }
}

pub fn mate(id: &str, name: &str, kind: MateType, a: MateRef, b: MateRef, align: Option<Align>) -> Mate {
    Mate { id: Id::new(id), name: name.into(), kind, refs: vec![a, b], align, value: None, flip: None, relative: None, suppressed: None }
}

/// Concentric alignment that keeps the parts' axes as they are (the identity placements
/// the corpus intends: every part's +Z up).
pub fn concentric(id: &str, name: &str, a: MateRef, b: MateRef) -> Mate {
    let dot: f64 = geom_dir(&a.entity).iter().zip(geom_dir(&b.entity)).map(|(x, y)| x * y).sum();
    mate(id, name, MateType::Concentric, a, b, Some(if dot > 0.0 { Align::Aligned } else { Align::AntiAligned }))
}

pub fn part_hash_of(built: &Built, comp: &str) -> String {
    built.components.iter().find(|c| c.id == *comp).and_then(|c| c.part.clone()).expect("part")
}

/// The corpus assemblies, built from their parts as the app would build them.
pub fn corpus_assembly(name: &str) -> (AssemblyDocument, Assembler<OcctKernel>, Built) {
    let dir = corpus_dir();
    let mut a = assembler();
    let mut doc = AssemblyDocument::new(name, UnitSystem::Mmgs);
    let top = |z: f64| Want::Plane([0.0, 0.0, 1.0], z);
    let under = Want::Plane([0.0, 0.0, -1.0], 0.0);
    match name {
        "bracket-pins" => {
            doc.components.push_back(component("c1", "plate.ocpart"));
            doc.components.push_back(component("c2", "pin.ocpart"));
            doc.instances.push_back(instance("i1", "c1", "plate<1>", true, Transform::IDENTITY));
            doc.instances.push_back(instance("i2", "c2", "pin<1>", false, place([1.0, 0.0, 0.0], 20.0, [10.0, 20.0, 30.0])));
            doc.instances.push_back(instance("i3", "c2", "pin<2>", false, place([0.0, 1.0, 1.0], -35.0, [70.0, -10.0, 20.0])));
            let b = a.build(&doc, &dir, &Disk, &opts(false));
            let (plate, pin) = (part_hash_of(&b, "c1"), part_hash_of(&b, "c2"));
            let (top8, h1, h2) = (pick(&mut a, &plate, &top(8.0)), pick(&mut a, &plate, &Want::Cyl(15.0, 15.0)), pick(&mut a, &plate, &Want::Cyl(45.0, 15.0)));
            let (head, shank) = (pick(&mut a, &pin, &under), pick(&mut a, &pin, &Want::Cyl(0.0, 0.0)));
            doc.mates.push_back(concentric("m1", "Concentric1", mref("i1", h1), mref("i2", shank.clone())));
            doc.mates.push_back(mate("m2", "Coincident1", MateType::Coincident, mref("i1", top8.clone()), mref("i2", head.clone()), Some(Align::AntiAligned)));
            doc.mates.push_back(concentric("m3", "Concentric2", mref("i1", h2), mref("i3", shank)));
            doc.mates.push_back(mate("m4", "Coincident2", MateType::Coincident, mref("i1", top8), mref("i3", head), Some(Align::AntiAligned)));
        }
        "hinge" => {
            doc.components.push_back(component("c1", "plate.ocpart"));
            doc.components.push_back(component("c2", "arm.ocpart"));
            doc.components.push_back(component("c3", "pin.ocpart"));
            doc.instances.push_back(instance("i1", "c1", "plate<1>", true, Transform::IDENTITY));
            doc.instances.push_back(instance("i2", "c2", "arm<1>", false, place([0.0, 0.0, 1.0], 30.0, [20.0, 40.0, 25.0])));
            doc.instances.push_back(instance("i3", "c3", "pin<1>", false, place([1.0, 1.0, 0.0], 15.0, [-20.0, 0.0, 40.0])));
            let b = a.build(&doc, &dir, &Disk, &opts(false));
            let (plate, arm, pin) = (part_hash_of(&b, "c1"), part_hash_of(&b, "c2"), part_hash_of(&b, "c3"));
            let (ptop, phole) = (pick(&mut a, &plate, &top(8.0)), pick(&mut a, &plate, &Want::Cyl(15.0, 15.0)));
            let (abot, atop, ahole) = (pick(&mut a, &arm, &Want::Plane([0.0, 0.0, -1.0], 0.0)), pick(&mut a, &arm, &top(6.0)), pick(&mut a, &arm, &Want::Cyl(5.0, 5.0)));
            let (head, shank) = (pick(&mut a, &pin, &under), pick(&mut a, &pin, &Want::Cyl(0.0, 0.0)));
            doc.mates.push_back(mate("m1", "Coincident1", MateType::Coincident, mref("i1", ptop), mref("i2", abot), Some(Align::AntiAligned)));
            doc.mates.push_back(concentric("m2", "Concentric1", mref("i1", phole), mref("i2", ahole.clone())));
            doc.mates.push_back(mate("m3", "Coincident2", MateType::Coincident, mref("i2", atop), mref("i3", head), Some(Align::AntiAligned)));
            doc.mates.push_back(concentric("m4", "Concentric2", mref("i2", ahole), mref("i3", shank)));
        }
        _ => panic!("no corpus assembly {name}"),
    }
    assert!(doc.validate().is_empty(), "{:?}", doc.validate());
    let built = a.build(&doc, &dir, &Disk, &opts(false));
    oc_asm::apply_placements(&mut doc, &built);
    (doc, a, built)
}

pub const ASSEMBLIES: [&str; 2] = ["bracket-pins", "hinge"];

/// What the corpus records about a built assembly (`corpus/assemblies/<name>.json`):
/// rounded to 1e-6 so the comparison is about behaviour, not the last bits.
pub fn summary(b: &Built) -> serde_json::Value {
    let r = |x: f64| (x * 1e6).round() / 1e6 + 0.0;
    serde_json::json!({
        "dof": b.dof,
        "converged": b.converged,
        "instances": b.instances.iter().map(|i| (i.id.to_string(), serde_json::json!({
            "state": format!("{:?}", i.state),
            "free": i.free,
            "position": i.transform.position.map(r),
        }))).collect::<serde_json::Map<_, _>>(),
        "mates": b.mates.iter().map(|m| (m.id.to_string(), serde_json::json!(format!("{:?}", m.status)))).collect::<serde_json::Map<_, _>>(),
        "occurrences": b.occurrences.len(),
        "parts": b.components.iter().filter_map(|c| c.part.clone()).collect::<std::collections::BTreeSet<_>>().len(),
    })
}
