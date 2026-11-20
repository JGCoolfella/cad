//! A large generated assembly for scale tests and measurements (G3; G2's renderer benchmark
//! as a file the app opens): `parts` distinct parts, blocks with one to four holes whose sizes
//! differ by index, and `instances` fixed occurrences of them (instance `i` uses part
//! `i % parts`) on a grid, each turned about Z by its index. No mates: every instance is
//! fixed, as in a layout imported from elsewhere.
//!
//! [`generate`] makes the documents in memory; [`write`] writes them as files (one `.ocpart`
//! per part and the `.ocasm`, without caches, so the first open regenerates every part).
//! `cargo run --release -p oc-asm --example scale -- <dir>` writes the 10 000-component
//! assembly of G2's benchmark (100 parts).
use std::path::{Path, PathBuf};

use oc_doc::container::Container;
use oc_doc::{AssemblyDocument, Component, ComponentKind, Id, Instance, Mate, MateEntity, MateRef, MateType, Transform, UnitSystem};
use serde_json::{json, Value};

/// File name of part `k`.
pub fn part_file(k: usize) -> String {
    format!("scale-p{k:03}.ocpart")
}

/// Part `k`: a block `w × d × h` on the Front plane with `1 + k % 4` holes along its middle.
pub fn part_json(k: usize) -> String {
    let w = 30.0 + (k % 10) as f64 * 2.0;
    let d = 20.0 + (k / 10 % 10) as f64 * 2.0;
    let h = 5.0 + (k % 7) as f64;
    let holes = 1 + k % 4;
    let r = 1.5 + (k % 3) as f64 * 0.4;
    let mut points = serde_json::Map::new();
    let mut entities = serde_json::Map::new();
    points.insert("origin".into(), json!({ "id": "origin", "x": 0, "y": 0 }));
    let corners = [(0.0, 0.0), (w, 0.0), (w, d), (0.0, d)];
    for (i, (x, y)) in corners.iter().enumerate() {
        let id = format!("p{i}");
        points.insert(id.clone(), json!({ "id": id, "x": x, "y": y }));
    }
    for i in 0..4 {
        let id = format!("l{i}");
        entities.insert(id.clone(), json!({ "id": id, "type": "line", "p1": format!("p{i}"), "p2": format!("p{}", (i + 1) % 4) }));
    }
    for j in 0..holes {
        let (c, id) = (format!("c{j}"), format!("k{j}"));
        let x = w * (j + 1) as f64 / (holes + 1) as f64;
        points.insert(c.clone(), json!({ "id": c, "x": x, "y": d / 2.0 }));
        entities.insert(id.clone(), json!({ "id": id, "type": "circle", "c": c, "r": r }));
    }
    let doc: Value = json!({
        "format": "opencad-part",
        "version": 1,
        "name": format!("scale-p{k:03}"),
        "units": "MMGS",
        "features": [
            { "id": "f1", "name": "Sketch1", "type": "sketch", "plane": { "kind": "std", "plane": "front" },
              "sketch": { "points": points, "entities": entities, "relations": {}, "dimensions": {}, "nextDim": 1 } },
            { "id": "f2", "name": "Boss-Extrude1", "type": "extrude", "cut": false, "sketchId": "f1", "reverse": false, "merge": true,
              "end1": { "type": "blind", "depth": h } },
        ],
        "rollback": null,
    });
    serde_json::to_string_pretty(&doc).expect("json")
}

/// The assembly: `instances` fixed occurrences of `parts` parts (files [`part_file`]) on a
/// 25 × 20 × 20 grid at 45 mm (G2's *cube* layout), wider when there are more instances.
pub fn assembly(parts: usize, instances: usize) -> AssemblyDocument {
    let mut doc = AssemblyDocument::new(format!("scale-{instances}"), UnitSystem::Mmgs);
    for k in 0..parts {
        doc.components.push_back(Component { id: Id::from(format!("c{}", k + 1)), kind: ComponentKind::Part, path: part_file(k), abs_path: None, config: oc_doc::DEFAULT_CONFIGURATION.into() });
    }
    let (nx, ny) = (25, 20);
    let mut count = vec![0usize; parts];
    for i in 0..instances {
        let k = i % parts;
        count[k] += 1;
        let (x, y, z) = (i % nx, i / nx % ny, i / (nx * ny));
        let a = i as f64 * 0.1;
        doc.instances.push_back(Instance {
            id: Id::from(format!("i{}", i + 1)),
            component: Id::from(format!("c{}", k + 1)),
            name: format!("scale-p{k:03}<{}>", count[k]),
            fixed: true,
            transform: Transform { position: [x as f64 * 45.0, y as f64 * 45.0, z as f64 * 45.0], rotation: [(a / 2.0).cos(), 0.0, 0.0, (a / 2.0).sin()] },
            suppressed: None,
            hidden: None,
            color: None,
            display: None,
            transparent: None,
            config: None,
        });
    }
    doc
}

/// [`assembly`] with mates (G3c, the mate solver at scale): instance 1 stays fixed, every
/// other one floats and is held by a Lock mate where it is, to the instance before it in
/// its chain of `chain` instances, the first of each chain to instance 1. Every chain is a
/// connected group of `chain` bodies; the assembly is fully defined.
pub fn mated(parts: usize, instances: usize, chain: usize) -> AssemblyDocument {
    let mut doc = assembly(parts, instances);
    let chain = chain.max(1);
    let placed: Vec<Transform> = doc.instances.iter().map(|i| i.transform).collect();
    for (i, inst) in doc.instances.iter_mut().enumerate() {
        inst.fixed = i == 0;
    }
    for i in 1..instances {
        let k = (i - 1) % chain;
        let to = if k == 0 { 0 } else { i - 1 };
        let rel = placed[to].inverse().compose(&placed[i]);
        let r = |j: usize| MateRef { occurrence: vec![Id::from(format!("i{}", j + 1))], entity: MateEntity::Component };
        doc.mates.push_back(Mate { id: Id::from(format!("m{i}")), name: format!("Lock{i}"), kind: MateType::Lock, refs: vec![r(to), r(i)], align: None, value: None, flip: None, relative: Some(rel), suppressed: None });
    }
    doc
}

/// A large part (G3c memory budgets; E4's large plate without fillets): a 400 × 300 plate,
/// `10 + k` mm thick, with `nx` × `ny` holes of Ø8.
pub fn plate_json(k: usize, nx: usize, ny: usize) -> String {
    let mut points = serde_json::Map::new();
    let mut entities = serde_json::Map::new();
    points.insert("origin".into(), json!({ "id": "origin", "x": 0, "y": 0 }));
    for (i, (x, y)) in [(0.0, 0.0), (400.0, 0.0), (400.0, 300.0), (0.0, 300.0)].iter().enumerate() {
        points.insert(format!("p{i}"), json!({ "id": format!("p{i}"), "x": x, "y": y }));
        entities.insert(format!("l{i}"), json!({ "id": format!("l{i}"), "type": "line", "p1": format!("p{i}"), "p2": format!("p{}", (i + 1) % 4) }));
    }
    for i in 0..nx {
        for j in 0..ny {
            let (c, e) = (format!("c{i}_{j}"), format!("k{i}_{j}"));
            points.insert(c.clone(), json!({ "id": c, "x": 400.0 * (i as f64 + 0.5) / nx as f64, "y": 300.0 * (j as f64 + 0.5) / ny as f64 }));
            entities.insert(e.clone(), json!({ "id": e, "type": "circle", "c": c, "r": 4.0 }));
        }
    }
    serde_json::to_string_pretty(&json!({
        "format": "opencad-part", "version": 1, "name": format!("plate{k}"), "units": "MMGS",
        "features": [
            { "id": "f1", "name": "Sketch1", "type": "sketch", "plane": { "kind": "std", "plane": "front" },
              "sketch": { "points": points, "entities": entities, "relations": {}, "dimensions": {}, "nextDim": 1 } },
            { "id": "f2", "name": "Boss-Extrude1", "type": "extrude", "cut": false, "sketchId": "f1", "reverse": false, "merge": true, "end1": { "type": "blind", "depth": 10.0 + k as f64 } },
        ],
        "rollback": null,
    }))
    .expect("json")
}

/// `n` plates ([`plate_json`], files `plate<k>.ocpart`), each once, side by side.
pub fn plates(n: usize, nx: usize, ny: usize) -> (Vec<(String, String)>, AssemblyDocument) {
    let mut doc = AssemblyDocument::new(format!("plates-{n}"), UnitSystem::Mmgs);
    let mut files = vec![];
    for k in 0..n {
        files.push((format!("plate{k}.ocpart"), plate_json(k, nx, ny)));
        doc.components.push_back(Component { id: Id::from(format!("c{k}")), kind: ComponentKind::Part, path: format!("plate{k}.ocpart"), abs_path: None, config: oc_doc::DEFAULT_CONFIGURATION.into() });
        doc.instances.push_back(Instance {
            id: Id::from(format!("i{k}")),
            component: Id::from(format!("c{k}")),
            name: format!("plate{k}<1>"),
            fixed: true,
            transform: Transform { position: [0.0, 0.0, 50.0 * k as f64], rotation: [1.0, 0.0, 0.0, 0.0] },
            suppressed: None,
            hidden: None,
            color: None,
            display: None,
            transparent: None,
            config: None,
        });
    }
    (files, doc)
}

/// The part files (name, text) and the assembly.
pub fn generate(parts: usize, instances: usize) -> (Vec<(String, String)>, AssemblyDocument) {
    ((0..parts).map(|k| (part_file(k), part_json(k))).collect(), assembly(parts, instances))
}

/// Writes the parts and `scale-<instances>.ocasm` (no caches) into `dir`; returns the
/// assembly's path.
pub fn write(dir: &Path, parts: usize, instances: usize) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let (files, doc) = generate(parts, instances);
    for (name, text) in files {
        std::fs::write(dir.join(name), text)?;
    }
    let mut c = Container::new();
    let text = oc_doc::save_assembly(&doc).map_err(|e| std::io::Error::other(e.to_string()))?;
    c.insert(oc_doc::container::DOCUMENT_ENTRY, text.into_bytes());
    let path = dir.join(format!("scale-{instances}.ocasm"));
    std::fs::write(&path, c.to_bytes())?;
    Ok(path)
}
