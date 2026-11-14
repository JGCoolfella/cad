//! Resolution (plan section 3.3): by name, by signature among split pieces, by position
//! with a warning, missing, and the legacy path for references without names.
mod common;

use common::*;
use oc_doc::{EdgeSig, FaceSig};
use oc_kernel::{FaceId, Topology, VertexId};
use oc_naming::resolve::{edge, face, vertex};
use oc_naming::signature::{edge_sig, face_sig};
use oc_naming::{BodyNames, Found, How, Live, Rules};

const A: [usize; 6] = [0, 1, 2, 3, 4, 5];
const B: [usize; 6] = [5, 3, 0, 4, 2, 1];

fn rules() -> Rules {
    Rules::default()
}

/// A reference to `side` picked on the 40 × 30 × 10 block built by `f2`.
fn picked_face(side: &str) -> FaceSig {
    let t = cuboid([0.0; 3], [40.0, 30.0, 10.0], A);
    face_sig(&t, Some(&extruded("f2", A)), face_at(A, side))
}

fn picked_edge(a: &str, b: &str) -> EdgeSig {
    let t = cuboid([0.0; 3], [40.0, 30.0, 10.0], A);
    edge_sig(&t, Some(&extruded("f2", A)), edge_between(&t, face_at(A, a), face_at(A, b)))
}

#[test]
fn a_named_face_is_found_wherever_it_moved_and_however_faces_are_numbered() {
    let sig = picked_face("+z");
    assert_eq!(sig.name.as_deref(), Some(&["f2/end/k".to_string()][..]));
    // The block is much taller and its faces are renumbered. The best face by signature is
    // now the bottom (10 mm away, facing the other way: score 30) rather than the top
    // (35 mm away: score 35); the name finds the top. This is torture case TT1b, where at
    // 40 mm the two scores tie and face order decided.
    let t = cuboid([0.0; 3], [40.0, 30.0, 45.0], B);
    let names = extruded("f2", B);
    let live = Live::new(&t, &names);
    assert_eq!(face(&live, &sig, &rules()), Some(Found { id: face_at(B, "+z"), how: How::Name }));
    let legacy = FaceSig { name: None, ..sig.clone() };
    let by_signature = face(&live, &legacy, &rules()).unwrap();
    assert_eq!(by_signature.how, How::Legacy);
    assert_ne!(by_signature.id, face_at(B, "+z"), "position alone picks the wrong face here");
}

#[test]
fn split_pieces_are_told_apart_by_signature_and_merged_faces_still_match() {
    let sig = picked_face("+z");
    // Two bodies' worth of faces: the top was split into two pieces with the same name.
    let left = cuboid([0.0; 3], [15.0, 30.0, 10.0], A);
    let right = cuboid([15.0, 0.0, 0.0], [40.0, 30.0, 10.0], A);
    let mut t = left.clone();
    t.faces.extend(right.faces.iter().cloned());
    let mut names = extruded("f2", A);
    names.faces.extend(extruded("f2", A).faces);
    let live = Live::new(&t, &names);
    // The larger piece is nearer the stored centre (20, 15) and size.
    let top_right = FaceId(6 + face_at(A, "+z").0);
    assert_eq!(face(&live, &sig, &rules()), Some(Found { id: top_right, how: How::Split }));
    // Merged with another face: the name gained a tag, the stored one is still among them.
    let t = cuboid([0.0; 3], [40.0, 30.0, 10.0], A);
    let mut names = extruded("f2", A);
    names.faces[face_at(A, "+z").index()] = name(&["f2/end/k", "f6/side/l9"]);
    assert_eq!(face(&Live::new(&t, &names), &sig, &rules()), Some(Found { id: face_at(A, "+z"), how: How::Name }));
}

#[test]
fn a_lost_name_re_attaches_only_at_the_same_position() {
    let sig = picked_face("+z");
    // The feature was replaced (its faces are now f9's); the block is otherwise unchanged
    // but wider. The top is still in the plane z = 10 facing up: accepted by position.
    let t = cuboid([0.0; 3], [80.0, 30.0, 10.0], B);
    let names = extruded("f9", B);
    assert_eq!(face(&Live::new(&t, &names), &sig, &rules()), Some(Found { id: face_at(B, "+z"), how: How::Position }));
    // No face in that plane (torture case TT5: the boss is gone, the block's top is lower):
    // missing, not "the nearest face".
    let t = cuboid([0.0; 3], [40.0, 30.0, 6.0], B);
    assert_eq!(face(&Live::new(&t, &names), &sig, &rules()), None);
    // The plane z = 10 exists but faces the other way.
    let t = cuboid([0.0, 0.0, 10.0], [40.0, 30.0, 20.0], B);
    assert_eq!(face(&Live::new(&t, &names), &sig, &rules()), None);
}

#[test]
fn the_one_face_of_the_same_feature_and_role_stands_in_for_a_lost_key() {
    let sig = picked_face("+z");
    // The profile of f2 was redrawn with new entities (torture case TT1a): every key
    // changed. The body has exactly one `f2/end/...` face, so the reference to the old end
    // cap is that face, wherever it is now; the four `f2/side/...` faces cannot stand in
    // for one another.
    let renamed = |order: [usize; 6]| -> BodyNames {
        BodyNames { faces: extruded("f2", order).faces.iter().map(|n| name(&[&format!("{}-redrawn", n.tags()[0])])).collect() }
    };
    let t = cuboid([0.0, 0.0, 5.0], [70.0, 30.0, 45.0], B);
    let names = renamed(B);
    let live = Live::new(&t, &names);
    assert_eq!(face(&live, &sig, &rules()), Some(Found { id: face_at(B, "+z"), how: How::Role }));
    assert_eq!(face(&live, &picked_face("+x"), &rules()), None, "four side faces: no single stand-in, and none at the old position");
    // An edge between the end cap and a side whose key survived: found through the cap's
    // stand-in. With the side's key gone too, there is nothing to go by.
    let mut mixed = renamed(B);
    mixed.faces[face_at(B, "+x").index()] = name(&["f2/side/l1"]);
    let edge_sig = picked_edge("+z", "+x");
    let found = edge(&Live::new(&t, &mixed), &edge_sig, &rules()).unwrap();
    assert_eq!((found.id, found.how), (edge_between(&t, face_at(B, "+z"), face_at(B, "+x")), How::Role));
    assert_eq!(edge(&live, &edge_sig, &rules()), None);
    // Two faces of that feature and role (a second profile loop): no stand-in; position
    // decides, and here nothing is in the old plane.
    let mut two = renamed(B);
    two.faces[face_at(B, "-z").index()] = name(&["f2/end/another-loop"]);
    assert_eq!(face(&Live::new(&t, &two), &sig, &rules()), None);
    // A face of another feature never stands in (torture case TT5).
    assert_eq!(face(&Live::new(&t, &extruded("f9", B)), &sig, &rules()), None);
}

#[test]
fn edges_resolve_through_the_faces_they_lie_between() {
    let sig = picked_edge("+x", "+y");
    assert_eq!(sig.name.as_ref().unwrap(), &[vec!["f2/side/l1".to_string()], vec!["f2/side/l2".to_string()]]);
    // Torture case TT2: the block is twice as wide. The stored edge was at x = 40; the
    // nearest vertical edge there no longer exists, the named one is at x = 80.
    let t = cuboid([0.0; 3], [80.0, 30.0, 10.0], B);
    let names = extruded("f2", B);
    let live = Live::new(&t, &names);
    let want = edge_between(&t, face_at(B, "+x"), face_at(B, "+y"));
    assert_eq!(edge(&live, &sig, &rules()), Some(Found { id: want, how: How::Name }));
    // Legacy: the loose signature search has nothing within its limit (the edge's length
    // or 5 mm) and gives up; that was the TypeScript engine's error in TT2.
    assert_eq!(edge(&live, &EdgeSig { name: None, ..sig.clone() }, &rules()), None);
    // A legacy reference on the unchanged body is found and reported for write-back.
    let t0 = cuboid([0.0; 3], [40.0, 30.0, 10.0], B);
    let found = edge(&Live::new(&t0, &names), &EdgeSig { name: None, ..sig.clone() }, &rules()).unwrap();
    assert_eq!((found.id, found.how), (edge_between(&t0, face_at(B, "+x"), face_at(B, "+y")), How::Legacy));
    assert_eq!(Live::new(&t0, &names).edge_name(found.id).to_stored(), sig.name.clone().unwrap());
    // One of its faces no longer exists under any tag: by position if the same line is
    // still an edge there, else missing.
    let mut renamed = names.clone();
    renamed.faces[face_at(B, "+y").index()] = name(&["f9/side/q"]);
    assert_eq!(edge(&Live::new(&t0, &renamed), &sig, &rules()).map(|f| f.how), Some(How::Position));
    assert_eq!(edge(&Live::new(&t, &renamed), &sig, &rules()), None);
}

#[test]
fn vertices_resolve_through_the_faces_around_them() {
    let t0 = cuboid([0.0; 3], [40.0, 30.0, 10.0], A);
    let names0 = extruded("f2", A);
    let live0 = Live::new(&t0, &names0);
    let corner = VertexId(t0.vertices.iter().position(|p| *p == [40.0, 30.0, 10.0]).unwrap() as u32);
    let stored = live0.vertex_name(corner).to_stored();
    assert_eq!(stored.len(), 3);
    // Taller and wider, renumbered: the corner is found by its three faces.
    let t = cuboid([0.0; 3], [55.0, 30.0, 25.0], B);
    let names = extruded("f2", B);
    let live = Live::new(&t, &names);
    let found = vertex(&live, [40.0, 30.0, 10.0], Some(&stored), &rules()).unwrap();
    assert_eq!((t.vertices[found.id.index()], found.how), ([55.0, 30.0, 25.0], How::Name));
    // Legacy: only a vertex exactly there counts. A point off the model (the origin of a
    // part that does not touch it, a sketch point) is not a topology reference.
    assert_eq!(vertex(&live0, [40.0, 30.0, 10.0], None, &rules()).map(|f| (f.id, f.how)), Some((corner, How::Legacy)));
    assert_eq!(vertex(&live, [40.0, 30.0, 10.0], None, &rules()), None);
    // A named vertex whose faces are gone: by position if a vertex is still there.
    let other = extruded("f9", A);
    assert_eq!(vertex(&Live::new(&t0, &other), [40.0, 30.0, 10.0], Some(&stored), &rules()).map(|f| f.how), Some(How::Position));
    assert_eq!(vertex(&Live::new(&t, &extruded("f9", B)), [40.0, 30.0, 10.0], Some(&stored), &rules()), None);
}

#[test]
fn resolution_does_not_depend_on_face_order() {
    let build = |order: [usize; 6]| -> (Topology, BodyNames) { (cuboid([0.0; 3], [70.0, 30.0, 22.0], order), extruded("f2", order)) };
    for side in SIDES {
        let sig = picked_face(side);
        let mut centres = vec![];
        for order in [A, B] {
            let (t, names) = build(order);
            let f = face(&Live::new(&t, &names), &sig, &rules()).unwrap();
            centres.push(t.faces[f.id.index()].centroid);
        }
        assert_eq!(centres[0], centres[1], "{side}");
    }
}
