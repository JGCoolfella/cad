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
    // cap is that face, wherever it is now (owner decision 2026-11-19, G1d: caps are named
    // by feature, as in SOLIDWORKS); the four `f2/side/...` faces cannot stand in for one
    // another.
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

/// A plate's top face (`f1/end/k`) with round holes cut by `f3` from one sketch: one per
/// `(circle id, x)`, radius 5, 8 deep, at y = 15. Faces: the top, then each hole's wall;
/// edges: each hole's rim (a full circle, one vertex).
fn plate_with_holes(holes: &[(&str, f64)]) -> (Topology, BodyNames) {
    use oc_kernel::{Axis, BBox, Curve, EdgeId, EdgeInfo, FaceInfo, Surface};
    let (r, depth) = (5.0, 8.0);
    let mut faces = vec![FaceInfo {
        surface: Surface::Plane { origin: [30.0, 15.0, depth], normal: [0.0, 0.0, 1.0] },
        area: 60.0 * 30.0,
        centroid: [30.0, 15.0, depth],
        normal: [0.0, 0.0, 1.0],
        bbox: BBox { min: [0.0, 0.0, depth], max: [60.0, 30.0, depth] },
        edges: vec![],
    }];
    let mut edges = vec![];
    let mut vertices = vec![];
    let mut names = vec![name(&["f1/end/k"])];
    for (i, (circle, x)) in holes.iter().enumerate() {
        let f = FaceId(faces.len() as u32);
        let e = EdgeId(edges.len() as u32);
        let start = [x + r, 15.0, depth];
        vertices.push(start);
        edges.push(EdgeInfo {
            curve: Curve::Circle { center: [*x, 15.0, depth], normal: [0.0, 0.0, 1.0], radius: r },
            start,
            end: start,
            mid: [x - r, 15.0, depth],
            length: 2.0 * std::f64::consts::PI * r,
            faces: vec![FaceId(0), f],
            seam: false,
            vertices: vec![VertexId(i as u32)],
        });
        faces[0].edges.push(e);
        faces.push(FaceInfo {
            surface: Surface::Cylinder { axis: Axis { origin: [*x, 15.0, 0.0], dir: [0.0, 0.0, 1.0] }, radius: r },
            area: 2.0 * std::f64::consts::PI * r * depth,
            centroid: [*x, 15.0, depth / 2.0],
            normal: [-1.0, 0.0, 0.0],
            bbox: BBox { min: [x - r, 10.0, 0.0], max: [x + r, 20.0, depth] },
            edges: vec![e],
        });
        names.push(name(&[&format!("f3/side/{circle}")]));
    }
    (Topology { solids: 1, shells: 1, faces, edges, vertices }, BodyNames { faces: names })
}

#[test]
fn a_stand_in_that_moved_is_not_the_same_entity() {
    // Owner decision 2026-11-16 (G1c), found by G1b: two holes cut from one sketch, a mate
    // on the first. Deleting the first hole's circle leaves the second as the only
    // `f3/side/...` face; it is 30 mm away, so the reference is missing (SOLIDWORKS: the
    // mate's reference is missing), not healed onto the other hole.
    let (t0, n0) = plate_with_holes(&[("c1", 15.0), ("c2", 45.0)]);
    let live0 = Live::new(&t0, &n0);
    let wall = face_sig(&t0, Some(&n0), FaceId(1));
    let rim = edge_sig(&t0, Some(&n0), oc_kernel::EdgeId(0));
    let seam = live0.vertex_name(VertexId(0)).to_stored();
    assert_eq!(face(&live0, &wall, &rules()), Some(Found { id: FaceId(1), how: How::Name }));
    assert_eq!(edge(&live0, &rim, &rules()).map(|f| f.how), Some(How::Name));

    let (t, n) = plate_with_holes(&[("c2", 45.0)]);
    let live = Live::new(&t, &n);
    assert_eq!(face(&live, &wall, &rules()), None, "the other hole's wall must not stand in");
    assert_eq!(edge(&live, &rim, &rules()), None, "the other hole's rim must not stand in");
    assert_eq!(vertex(&live, [20.0, 15.0, 8.0], Some(&seam), &rules()), None, "the other hole's vertex must not stand in");

    // The circle redrawn as a new entity where it was (or a little off): still the hole,
    // found by role. Its rim's start point is elsewhere on the circle; its centre decides.
    for x in [15.0, 16.0] {
        let (t, n) = plate_with_holes(&[("c9", x), ("c2", 45.0)]);
        // Only c9 and c2 are `f3/side`: two of that role, so no stand-in by role, and the
        // strict position match (step 3) finds the redrawn hole.
        let live = Live::new(&t, &n);
        assert_eq!(face(&live, &wall, &rules()).map(|f| (f.id, f.how)), Some((FaceId(1), How::Position)), "x = {x}");
        let (t, n) = plate_with_holes(&[("c9", x)]);
        let mut t = t;
        // Start the redrawn rim on the far side of the circle.
        t.edges[0].start = [x - 5.0, 15.0, 8.0];
        t.edges[0].end = t.edges[0].start;
        t.edges[0].mid = [x + 5.0, 15.0, 8.0];
        let live = Live::new(&t, &n);
        assert_eq!(face(&live, &wall, &rules()), Some(Found { id: FaceId(1), how: How::Role }), "x = {x}");
        assert_eq!(edge(&live, &rim, &rules()).map(|f| (f.id.0, f.how)), Some((0, How::Role)), "x = {x}");
    }

    // The block of the test above, profile redrawn and grown to 45 mm tall: the one end cap
    // is 35 mm from where the reference was, more than a quarter of its size. Caps are
    // exempt (owner decision 2026-11-19, G1d): it is still the cap.
    let sig = picked_face("+z");
    let far = cuboid([0.0, 0.0, 5.0], [70.0, 30.0, 45.0], B);
    let renamed = BodyNames { faces: extruded("f2", B).faces.iter().map(|n| name(&[&format!("{}-redrawn", n.tags()[0])])).collect() };
    assert_eq!(face(&Live::new(&far, &renamed), &sig, &rules()), Some(Found { id: face_at(B, "+z"), how: How::Role }));
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
