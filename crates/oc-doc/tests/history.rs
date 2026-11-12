//! Transactions, undo/redo and structural sharing, including the E1 property test:
//! random edit sequences on corpus documents, then undo all, give back the original.

mod common;

use std::sync::{Arc, OnceLock};

use oc_doc::*;
use proptest::prelude::*;

fn corpus_parts() -> &'static [PartDocument] {
    static PARTS: OnceLock<Vec<PartDocument>> = OnceLock::new();
    PARTS.get_or_init(|| {
        common::corpus_documents()
            .iter()
            .filter(|d| d.label.ends_with(".ocpart"))
            .map(|d| load(&d.text).unwrap().doc)
            .collect()
    })
}

fn part(name: &str) -> PartDocument {
    corpus_parts()
        .iter()
        .find(|d| d.name == name)
        .unwrap()
        .clone()
}

fn depth_of(doc: &PartDocument, id: &str) -> f64 {
    match &doc.feature(id).unwrap().kind {
        FeatureKind::Extrude(e) => e.end1.depth,
        _ => panic!("{id} is not an extrude"),
    }
}

fn set_depth(doc: &mut PartDocument, id: &str, depth: f64) {
    match &mut doc.feature_mut(id).unwrap().kind {
        FeatureKind::Extrude(e) => e.end1.depth = depth,
        _ => panic!("{id} is not an extrude"),
    }
}

// ---------------------------------------------------------------- examples

#[test]
fn commit_undo_redo() {
    let mut h = History::new(part("block-hole"));
    let original = h.doc().clone();
    assert!(!h.can_undo() && !h.can_redo() && !h.is_dirty());

    h.transact("Depth 7", |d| -> Result<(), ()> {
        set_depth(d, "f2", 7.0);
        Ok(())
    })
    .unwrap();
    h.transact("Rename", |d| -> Result<(), ()> {
        d.feature_mut("f2").unwrap().name = "Base".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(depth_of(h.doc(), "f2"), 7.0);
    assert_eq!(h.undo_labels().collect::<Vec<_>>(), ["Rename", "Depth 7"]);
    assert!(h.is_dirty());

    assert_eq!(h.undo(), Some("Rename"));
    assert_eq!(h.doc().feature("f2").unwrap().name, "Boss-Extrude1");
    assert_eq!(h.redo_labels().collect::<Vec<_>>(), ["Rename"]);
    assert_eq!(h.undo(), Some("Depth 7"));
    assert_eq!(h.undo(), None);
    // The undone document is the original snapshot itself, not a copy.
    assert!(Arc::ptr_eq(h.doc(), &original));
    assert!(!h.is_dirty(), "back at the opened document");

    assert_eq!(h.redo(), Some("Depth 7"));
    assert_eq!(depth_of(h.doc(), "f2"), 7.0);
    // A new edit after undo drops the redo stack.
    h.transact("Depth 9", |d| -> Result<(), ()> {
        set_depth(d, "f2", 9.0);
        Ok(())
    })
    .unwrap();
    assert!(!h.can_redo());
    assert_eq!(h.undo_labels().collect::<Vec<_>>(), ["Depth 9", "Depth 7"]);
}

#[test]
fn failed_and_empty_transactions_leave_no_trace() {
    let mut h = History::new(part("block-hole"));
    let before = h.doc().clone();
    let r: Result<(), &str> = h.transact("Bad", |d| {
        set_depth(d, "f2", 99.0);
        Err("rejected")
    });
    assert_eq!(r, Err("rejected"));
    assert!(Arc::ptr_eq(h.doc(), &before) && !h.can_undo());

    // Touched but unchanged: no undo step.
    let mut tx = h.begin("Nothing");
    let depth = depth_of(tx.doc(), "f2");
    set_depth(tx.doc_mut(), "f2", depth);
    assert!(!tx.commit());
    assert!(Arc::ptr_eq(h.doc(), &before) && !h.can_undo());

    // Dropped without commit (a cancelled PropertyManager): discarded.
    {
        let mut tx = h.begin("Cancelled");
        set_depth(tx.doc_mut(), "f2", 1.0);
        assert_eq!(depth_of(tx.doc(), "f2"), 1.0);
    }
    assert!(Arc::ptr_eq(h.doc(), &before) && !h.can_undo());

    // A panicking edit cannot leave a half-edited document behind.
    let mut h2 = h.clone();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = h2.transact("Boom", |d| -> Result<(), ()> {
            set_depth(d, "f2", 5.5);
            panic!("edit failed half way");
        });
    }));
    assert!(caught.is_err());
    assert!(Arc::ptr_eq(h2.doc(), &before) && !h2.can_undo());
}

#[test]
fn an_edit_copies_only_what_it_changes() {
    let mut h = History::new(part("patterns"));
    let before = h.doc().clone();
    let target = before.features[1].id.clone();
    h.transact("Depth", |d| -> Result<(), ()> {
        set_depth(d, &target, 33.0);
        Ok(())
    })
    .unwrap();
    let after = h.doc();
    for (a, b) in before.features.iter().zip(after.features.iter()) {
        assert_eq!(Arc::ptr_eq(a, b), a.id != target, "{}", a.id);
    }

    // Moving one point of a sketch shares the sketch's other maps and points.
    let sketch_id = before.features[0].id.clone();
    let point = before.features[0]
        .sketch()
        .unwrap()
        .points
        .keys()
        .nth(1)
        .unwrap()
        .clone();
    h.transact("Drag", |d| -> Result<(), ()> {
        let s = d.feature_mut(&sketch_id).unwrap().sketch_mut().unwrap();
        s.points.get_mut(&point).unwrap().x += 1.0;
        Ok(())
    })
    .unwrap();
    let (old, new) = (
        before.features[0].sketch().unwrap(),
        h.doc().features[0].sketch().unwrap(),
    );
    assert!(old.entities.ptr_eq(&new.entities));
    assert!(!old.points.ptr_eq(&new.points));
    assert_eq!(new.points[&point].x, old.points[&point].x + 1.0);
}

#[test]
fn saved_marker_limit_and_heal() {
    let mut h = History::new(part("block-hole"));
    for i in 0..5 {
        h.transact(format!("Edit {i}"), |d| -> Result<(), ()> {
            set_depth(d, "f2", 10.0 + f64::from(i));
            Ok(())
        })
        .unwrap();
    }
    h.mark_saved();
    assert!(!h.is_dirty());
    h.undo();
    assert!(h.is_dirty());
    h.redo();
    assert!(!h.is_dirty());

    h.set_limit(Some(2));
    assert_eq!(h.undo_labels().collect::<Vec<_>>(), ["Edit 4", "Edit 3"]);

    // A heal changes the document without an undo step and without dirtying it.
    h.heal(|d| d.feature_mut("f2").unwrap().name = "Healed".into());
    assert_eq!(h.doc().feature("f2").unwrap().name, "Healed");
    assert!(!h.is_dirty());
    assert_eq!(h.undo_labels().count(), 2);
    // ...and keeps a dirty document dirty.
    h.undo();
    h.heal(|d| d.feature_mut("f2").unwrap().name = "Healed again".into());
    assert!(h.is_dirty());
}

// ---------------------------------------------------------------- property test

/// One user action. Indices are taken modulo the current sizes, so every op applies.
#[derive(Clone, Debug)]
enum Op {
    Rename {
        f: usize,
        name: String,
    },
    ToggleSuppress {
        f: usize,
    },
    SetRollback {
        at: Option<usize>,
    },
    Remove {
        f: usize,
    },
    Duplicate {
        f: usize,
        to: usize,
    },
    Move {
        from: usize,
        to: usize,
    },
    MovePoint {
        f: usize,
        p: usize,
        dx: f64,
        dy: f64,
    },
    AddPoint {
        f: usize,
        x: f64,
        y: f64,
    },
    RemoveEntity {
        f: usize,
        e: usize,
    },
    Scale {
        f: usize,
        k: f64,
    },
    SetMaterial(Option<String>),
    SetProp {
        key: String,
        value: i32,
    },
    /// Several edits in one transaction (one undo step).
    Batch(Vec<Op>),
    /// An edit whose transaction then fails: must change nothing.
    Failing(Box<Op>),
    Undo,
    Redo,
}

fn edit_op() -> impl Strategy<Value = Op> {
    let i = || 0..1000usize;
    let num = || -50.0..50.0f64;
    prop_oneof![
        (i(), "[A-Za-z0-9 ]{0,12}").prop_map(|(f, name)| Op::Rename { f, name }),
        i().prop_map(|f| Op::ToggleSuppress { f }),
        proptest::option::of(i()).prop_map(|at| Op::SetRollback { at }),
        i().prop_map(|f| Op::Remove { f }),
        (i(), i()).prop_map(|(f, to)| Op::Duplicate { f, to }),
        (i(), i()).prop_map(|(from, to)| Op::Move { from, to }),
        (i(), i(), num(), num()).prop_map(|(f, p, dx, dy)| Op::MovePoint { f, p, dx, dy }),
        (i(), num(), num()).prop_map(|(f, x, y)| Op::AddPoint { f, x, y }),
        (i(), i()).prop_map(|(f, e)| Op::RemoveEntity { f, e }),
        (i(), prop_oneof![Just(1.0), 0.1..10.0f64]).prop_map(|(f, k)| Op::Scale { f, k }),
        proptest::option::of("[a-z]{1,8}").prop_map(Op::SetMaterial),
        ("[a-c]", -3..3i32).prop_map(|(key, value)| Op::SetProp { key, value }),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => edit_op(),
        1 => prop::collection::vec(edit_op(), 0..4).prop_map(Op::Batch),
        1 => edit_op().prop_map(|o| Op::Failing(Box::new(o))),
        2 => Just(Op::Undo),
        1 => Just(Op::Redo),
    ]
}

fn unique_id(doc: &PartDocument, base: &str) -> Id {
    (0..)
        .map(|n| format!("{base}_{n}"))
        .find(|c| doc.feature(c).is_none())
        .unwrap()
        .into()
}

fn sketch_indices(doc: &PartDocument) -> Vec<usize> {
    (0..doc.features.len())
        .filter(|&i| doc.features[i].sketch().is_some())
        .collect()
}

fn feature(doc: &mut PartDocument, f: usize) -> Option<&mut Feature> {
    let len = doc.features.len();
    (len > 0).then(move || Arc::make_mut(&mut doc.features[f % len]))
}

/// Applies an edit as the app would (keeping `rollback` in range).
fn apply(doc: &mut PartDocument, op: &Op) {
    let n = doc.features.len();
    match op {
        Op::Rename { f, name } => {
            if let Some(x) = feature(doc, *f) {
                x.name = name.clone();
            }
        }
        Op::ToggleSuppress { f } => {
            if let Some(x) = feature(doc, *f) {
                x.suppressed = if x.is_suppressed() { None } else { Some(true) };
            }
        }
        Op::SetRollback { at } => doc.rollback = at.map(|a| (a % (n + 1)) as u32),
        Op::Remove { f } => {
            if n > 0 {
                doc.features.remove(f % n);
                if doc.rollback.is_some_and(|r| r as usize > n - 1) {
                    doc.rollback = None;
                }
            }
        }
        Op::Duplicate { f, to } => {
            if n > 0 {
                let mut copy = (*doc.features[f % n]).clone();
                copy.id = unique_id(doc, copy.id.as_str());
                copy.name.push_str(" copy");
                doc.features.insert(to % (n + 1), Arc::new(copy));
            }
        }
        Op::Move { from, to } => {
            if n > 0 {
                let x = doc.features.remove(from % n);
                doc.features.insert(to % n, x);
            }
        }
        Op::MovePoint { f, p, dx, dy } => {
            let sk = sketch_indices(doc);
            if sk.is_empty() {
                return;
            }
            let s = Arc::make_mut(&mut doc.features[sk[f % sk.len()]])
                .sketch_mut()
                .unwrap();
            if let Some(key) = s.points.keys().nth(p % s.points.len().max(1)).cloned() {
                let pt = s.points.get_mut(&key).unwrap();
                pt.x += dx;
                pt.y += dy;
            }
        }
        Op::AddPoint { f, x, y } => {
            let sk = sketch_indices(doc);
            if sk.is_empty() {
                return;
            }
            let s = Arc::make_mut(&mut doc.features[sk[f % sk.len()]])
                .sketch_mut()
                .unwrap();
            let id: Id = (0..)
                .map(|n| format!("new_p{n}"))
                .find(|c| !s.points.contains_key(c.as_str()))
                .unwrap()
                .into();
            let mut pt = SketchPoint::new(id.clone(), *x, *y);
            pt.standalone = Some(true);
            s.points.insert(id, pt);
        }
        Op::RemoveEntity { f, e } => {
            let sk = sketch_indices(doc);
            if sk.is_empty() {
                return;
            }
            let s = Arc::make_mut(&mut doc.features[sk[f % sk.len()]])
                .sketch_mut()
                .unwrap();
            if let Some(key) = s.entities.keys().nth(e % s.entities.len().max(1)).cloned() {
                s.entities.remove(&key);
            }
        }
        Op::Scale { f, k } => {
            if let Some(x) = feature(doc, *f) {
                scale(&mut x.kind, *k);
            }
        }
        Op::SetMaterial(m) => doc.material = m.clone(),
        Op::SetProp { key, value } => {
            let props = Arc::make_mut(doc.props.get_or_insert_with(Default::default));
            if *value == 0 {
                props.remove(key);
            } else {
                props.insert(key.clone(), (*value).into());
            }
        }
        Op::Batch(ops) => ops.iter().for_each(|o| apply(doc, o)),
        Op::Failing(_) | Op::Undo | Op::Redo => unreachable!(),
    }
}

/// Scales the main size parameter of a feature (all coordinates, for a sketch).
fn scale(kind: &mut FeatureKind, k: f64) {
    match kind {
        FeatureKind::Sketch(s) => {
            let keys: Vec<Id> = s.sketch.points.keys().cloned().collect();
            for key in keys {
                let p = s.sketch.points.get_mut(&key).unwrap();
                p.x *= k;
                p.y *= k;
            }
        }
        FeatureKind::Extrude(e) => e.end1.depth *= k,
        FeatureKind::Revolve(r) => r.angle *= k,
        FeatureKind::Fillet(f) => f.radius *= k,
        FeatureKind::Chamfer(c) => c.distance *= k,
        FeatureKind::RefPlane(p) => p.offset *= k,
        FeatureKind::LinearPattern(p) => p.dir1.spacing *= k,
        FeatureKind::CircularPattern(p) => p.angle *= k,
        FeatureKind::Shell(s) => s.thickness *= k,
        FeatureKind::Rib(r) => r.thickness *= k,
        FeatureKind::Draft(d) => d.angle *= k,
        FeatureKind::Hole(h) => h.end.depth *= k,
        FeatureKind::Sweep(s) => s.diameter *= k,
        FeatureKind::Mirror(_)
        | FeatureKind::Imported(_)
        | FeatureKind::Loft(_)
        | FeatureKind::RefAxis(_)
        | FeatureKind::RefPoint(_)
        | FeatureKind::CoordSys(_) => {}
    }
}

/// Reference model of the history: full canonical-JSON snapshots, as the TypeScript
/// app keeps them, and a cursor.
struct Oracle {
    states: Vec<String>,
    pos: usize,
}

fn canon(doc: &PartDocument) -> String {
    canonical(doc).unwrap()
}

fn run(doc_index: usize, ops: &[Op]) -> Result<(), TestCaseError> {
    let parts = corpus_parts();
    let original = parts[doc_index % parts.len()].clone();
    let mut h = History::new(original);
    let first = h.doc().clone();
    let mut oracle = Oracle {
        states: vec![canon(&first)],
        pos: 0,
    };

    for (step, op) in ops.iter().enumerate() {
        match op {
            Op::Undo => {
                let undone = h.undo().is_some();
                prop_assert_eq!(undone, oracle.pos > 0);
                oracle.pos = oracle.pos.saturating_sub(1);
            }
            Op::Redo => {
                let redone = h.redo().is_some();
                prop_assert_eq!(redone, oracle.pos + 1 < oracle.states.len());
                if redone {
                    oracle.pos += 1;
                }
            }
            Op::Failing(inner) => {
                let before = h.doc().clone();
                let r: Result<(), ()> = h.transact("fails", |d| {
                    apply(d, inner);
                    Err(())
                });
                prop_assert!(r.is_err());
                prop_assert!(Arc::ptr_eq(&before, h.doc()));
            }
            edit => {
                h.transact(format!("{step}"), |d| -> Result<(), ()> {
                    apply(d, edit);
                    Ok(())
                })
                .unwrap();
                // The oracle applies the edit to its own deep copy (re-read from JSON,
                // so nothing is shared with the history's snapshots).
                let mut expected = load(&oracle.states[oracle.pos]).unwrap().doc;
                apply(&mut expected, edit);
                let want = canon(&expected);
                if want != oracle.states[oracle.pos] {
                    oracle.states.truncate(oracle.pos + 1);
                    oracle.states.push(want);
                    oracle.pos += 1;
                }
            }
        }
        prop_assert_eq!(
            &canon(h.doc()),
            &oracle.states[oracle.pos],
            "after op {} {:?}",
            step,
            op
        );
        prop_assert_eq!(h.can_undo(), oracle.pos > 0);
        prop_assert_eq!(h.can_redo(), oracle.pos + 1 < oracle.states.len());
        prop_assert_eq!(h.doc().validate(), Vec::<String>::new());
    }

    // The edited document survives a save and load.
    let saved = save(h.doc()).unwrap();
    prop_assert_eq!(&load(&saved).unwrap().doc, &**h.doc());

    // Undo all: the original document, as the very same snapshot.
    while h.undo().is_some() {}
    prop_assert!(Arc::ptr_eq(h.doc(), &first));
    prop_assert_eq!(&canon(h.doc()), &oracle.states[0]);
    prop_assert!(!h.is_dirty());
    // Redo all: the newest state.
    while h.redo().is_some() {}
    prop_assert_eq!(&canon(h.doc()), oracle.states.last().unwrap());
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn random_edits_then_undo_all_give_back_the_original(
        doc in 0..64usize,
        ops in prop::collection::vec(op(), 1..40),
    ) {
        run(doc, &ops)?;
    }
}
