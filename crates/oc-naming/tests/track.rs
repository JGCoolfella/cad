//! Propagation (plan section 3.2) on hand-written histories: each rule, and that names
//! never depend on the kernel's face numbering (section 3.4).
mod common;

use common::*;
use oc_kernel::{CapEnd, EdgeId, FaceId, Fate, History, Image, Relation, Source, Surface, TopoKind, Topology, VertexId};
use oc_naming::{track, Births, BodyNames, Identity, Input, NoTopo, Tag, Topo};

const A: [usize; 6] = [0, 1, 2, 3, 4, 5];
const B: [usize; 6] = [5, 3, 0, 4, 2, 1];

fn img(index: u32, relation: Relation) -> Image {
    Image { index, relation }
}

fn fate(source: Source, images: Vec<Image>) -> Fate {
    Fate { source, deleted: images.is_empty(), images }
}

fn history(count: u32, mut fates: Vec<Fate>) -> History {
    for f in &mut fates {
        f.images.sort();
    }
    History { image_kind: TopoKind::Face, count, fates }
}

fn run(b: &Births<'_>, h: &History, inputs: &[(&BodyNames, &dyn Topo)], result: &dyn Topo) -> oc_naming::Tracked {
    let inputs: Vec<Input<'_>> = inputs.iter().map(|(names, topo)| Input { names, topo: *topo }).collect();
    track(b, h, &inputs, result, &Identity::default())
}

/// The prism of a rectangle, with result faces in `order`.
fn prism_history(order: [usize; 6]) -> History {
    let seg = |segment: u32, side: &str| fate(Source::Segment { region: 0, wire: 0, segment }, vec![img(face_at(order, side).0, Relation::Generated)]);
    let cap = |end, side: &str| fate(Source::Cap { region: 0, end }, vec![img(face_at(order, side).0, Relation::Generated)]);
    history(6, vec![seg(0, "-y"), seg(1, "+x"), seg(2, "+y"), seg(3, "-x"), cap(CapEnd::Start, "-z"), cap(CapEnd::End, "+z")])
}

#[test]
fn profile_segments_and_caps_get_the_features_tags() {
    let explicit: Vec<(Source, Tag)> = (0..4)
        .map(|s| (Source::Segment { region: 0, wire: 0, segment: s }, Tag::birth("f2", "side", &format!("l{s}"))))
        .chain([
            (Source::Cap { region: 0, end: CapEnd::Start }, Tag::birth("f2", "start", "k")),
            (Source::Cap { region: 0, end: CapEnd::End }, Tag::birth("f2", "end", "k")),
        ])
        .collect();
    let b = Births { explicit: &explicit, ..Births::of("f2") };
    for order in [A, B] {
        let r = run(&b, &prism_history(order), &[], &NoTopo);
        assert_eq!(r.names, extruded("f2", order), "{order:?}");
        assert_eq!((r.by_identity, r.untracked), (0, 0));
    }
    // A segment the feature did not key still gets a stable, index-free tag.
    let r = run(&Births::of("f2"), &prism_history(B), &[], &NoTopo);
    assert_eq!(r.names.face(face_at(B, "+x")), &name(&["f2/side/s0.0.1"]));
    assert_eq!(r.names.face(face_at(B, "+z")), &name(&["f2/end/r0"]));
    assert_eq!(r.untracked, 0);
}

#[test]
fn kept_modified_split_and_merged_faces_inherit() {
    let body = extruded("f2", A);
    let tool = extruded("f4", A);
    // A cut: the top is split in two, a side passes through, the tool's end cap becomes a
    // floor, the bottom merges with a tool side (coplanar), the tool's other faces vanish.
    let top = face_at(A, "+z");
    let h = history(
        6,
        vec![
            fate(Source::Face { arg: 0, face: top }, vec![img(0, Relation::Modified), img(3, Relation::Modified)]),
            fate(Source::Face { arg: 0, face: face_at(A, "-x") }, vec![img(1, Relation::Same)]),
            fate(Source::Face { arg: 0, face: face_at(A, "-z") }, vec![img(4, Relation::Modified)]),
            fate(Source::Face { arg: 0, face: face_at(A, "+x") }, vec![]),
            fate(Source::Face { arg: 1, face: face_at(A, "+z") }, vec![img(2, Relation::Modified)]),
            fate(Source::Face { arg: 1, face: face_at(A, "-y") }, vec![img(4, Relation::Modified), img(5, Relation::Modified)]),
            fate(Source::Face { arg: 1, face: face_at(A, "-z") }, vec![]),
        ],
    );
    let r = run(&Births::of("f4"), &h, &[(&body, &NoTopo), (&tool, &NoTopo)], &NoTopo);
    let n = &r.names.faces;
    assert_eq!(n[0], name(&["f2/end/k"]));
    assert_eq!(n[3], name(&["f2/end/k"]), "both pieces of a split keep the tag");
    assert_eq!(n[1], name(&["f2/side/l3"]));
    assert_eq!(n[2], name(&["f4/end/k"]));
    assert_eq!(n[4], name(&["f2/start/k", "f4/side/l0"]), "a merge unions the tags");
    assert_eq!(n[5], name(&["f4/side/l0"]));
    assert_eq!(r.untracked, 0);
}

/// Fillet of the edge between +x and +y on a cuboid stored in `order`: the result keeps
/// every face (index unchanged) and adds the fillet face last; the corner vertex at
/// (+x, +y, +z) generates a patch.
fn fillet_history(t: &Topology, order: [usize; 6]) -> History {
    let e: EdgeId = edge_between(t, face_at(order, "+x"), face_at(order, "+y"));
    let corner = VertexId(t.vertices.iter().position(|p| *p == [4.0, 3.0, 2.0]).unwrap() as u32);
    let mut fates: Vec<Fate> = (0..6).map(|i| fate(Source::Face { arg: 0, face: FaceId(i) }, vec![img(i, Relation::Modified)])).collect();
    fates.push(fate(Source::Edge { arg: 0, edge: e }, vec![img(6, Relation::Generated)]));
    fates.push(fate(Source::Vertex { arg: 0, vertex: corner }, vec![img(7, Relation::Generated)]));
    history(8, fates)
}

#[test]
fn faces_from_edges_and_vertices_are_keyed_by_names_not_indices() {
    let b = Births { edge_role: "fillet", ..Births::of("f3") };
    let mut tags = vec![];
    for order in [A, B] {
        let t = cuboid([0.0; 3], [4.0, 3.0, 2.0], order);
        let names = extruded("f2", order);
        let r = run(&b, &fillet_history(&t, order), &[(&names, &t)], &NoTopo);
        assert_eq!(&r.names.faces[..6], &names.faces[..]);
        assert_eq!(r.untracked, 0);
        tags.push((r.names.faces[6].clone(), r.names.faces[7].clone()));
    }
    assert_eq!(tags[0], tags[1], "the same edge and vertex give the same tags in any face order");
    let (fillet, corner) = &tags[0];
    // The keys are the documented hashes of the edge's and the vertex's names.
    let edge = oc_naming::EdgeName::new(name(&["f2/side/l1"]), name(&["f2/side/l2"]));
    assert_eq!(fillet, &name(&[&format!("f3/fillet/{}", edge.key())]));
    let vertex = oc_naming::VertexName::new([name(&["f2/side/l1"]), name(&["f2/side/l2"]), name(&["f2/end/k"])]);
    assert_eq!(corner, &name(&[&format!("f3/corner/{}", vertex.key())]));
    // Another edge gets another tag; the same edge under another feature too.
    let t = cuboid([0.0; 3], [4.0, 3.0, 2.0], A);
    let names = extruded("f2", A);
    let other = history(1, vec![fate(Source::Edge { arg: 0, edge: edge_between(&t, face_at(A, "+x"), face_at(A, "+z")) }, vec![img(0, Relation::Generated)])]);
    assert_ne!(&run(&b, &other, &[(&names, &t)], &NoTopo).names.faces[0], fillet);
    let chamfer = Births { edge_role: "chamfer", ..Births::of("f9") };
    let r = run(&chamfer, &fillet_history(&t, A), &[(&names, &t)], &NoTopo);
    assert_eq!(r.names.faces[6], name(&[&format!("f9/chamfer/{}", edge.key())]));
}

#[test]
fn a_face_generated_from_a_face_replaces_it_or_is_born_beside_it() {
    let body = extruded("f2", A);
    let side = face_at(A, "+x");
    let top = face_at(A, "+z");
    // Shell: the side yields its outer wall (modified) and an inner wall (generated); the
    // open top is modified into the rim.
    let h = history(
        3,
        vec![
            fate(Source::Face { arg: 0, face: side }, vec![img(0, Relation::Modified), img(1, Relation::Generated)]),
            fate(Source::Face { arg: 0, face: top }, vec![img(2, Relation::Modified)]),
        ],
    );
    let shell = Births { face_role: Some("inner"), ..Births::of("f5") };
    let r = run(&shell, &h, &[(&body, &NoTopo)], &NoTopo);
    assert_eq!(r.names.faces[0], name(&["f2/side/l1"]));
    assert_eq!(r.names.faces[1], name(&[&format!("f5/inner/{}", name(&["f2/side/l1"]).key())]));
    assert_eq!(r.names.faces[2], name(&["f2/end/k"]), "the rim is what is left of the open face");
    // Draft: the kernel reports the drafted face as generated from the original; it is
    // still that face.
    let h = history(1, vec![fate(Source::Face { arg: 0, face: side }, vec![img(0, Relation::Generated)])]);
    let r = run(&Births::of("f6"), &h, &[(&body, &NoTopo)], &NoTopo);
    assert_eq!(r.names.faces[0], name(&["f2/side/l1"]));
}

#[test]
fn pattern_instances_wrap_the_seed_tags() {
    let seed = BodyNames { faces: vec![name(&["f4/side/l0"]), name(&["f4/end/k", "f4/side/l1"])] };
    let h = history(2, vec![fate(Source::Face { arg: 0, face: FaceId(0) }, vec![img(1, Relation::Modified)]), fate(Source::Face { arg: 0, face: FaceId(1) }, vec![img(0, Relation::Modified)])]);
    let inst = |key: &'static str| Births { instance: Some(key), ..Births::of("f7") };
    let r = run(&inst("2,1"), &h, &[(&seed, &NoTopo)], &NoTopo);
    assert_eq!(r.names.faces[1], name(&["f7/i2,1/f4/side/l0"]));
    assert_eq!(r.names.faces[0], name(&["f7/i2,1/f4/end/k", "f7/i2,1/f4/side/l1"]));
    // The tag depends on the instance's own key only: skipping or adding other instances
    // cannot rename it. A pattern of a pattern nests.
    let again = run(&Births { instance: Some("3"), ..Births::of("f9") }, &h, &[(&r.names, &NoTopo)], &NoTopo);
    assert_eq!(again.names.faces[0], name(&["f9/i3/f7/i2,1/f4/side/l0"]));
}

#[test]
fn a_face_without_history_takes_the_name_of_the_surface_it_lies_on() {
    // The result is the input cuboid, shortened; the kernel lost the top face's history.
    let input = cuboid([0.0; 3], [4.0, 3.0, 2.0], A);
    let names = extruded("f2", A);
    let result = cuboid([0.0; 3], [4.0, 3.0, 1.0], B);
    let fates = (0..6u32)
        .filter(|&i| i != face_at(A, "+z").0 && i != face_at(A, "+x").0)
        .map(|i| fate(Source::Face { arg: 0, face: FaceId(i) }, vec![img(face_at(B, SIDES[A[i as usize]]).0, Relation::Modified)]))
        .collect();
    let r = run(&Births::of("f8"), &history(6, fates), &[(&names, &input)], &result);
    // +x of the result lies on the input's +x plane and overlaps it: surface identity.
    assert_eq!(r.names.face(face_at(B, "+x")), &name(&["f2/side/l1"]));
    // The new top (z = 1) lies on no input surface: no traceable origin.
    assert_eq!(r.names.face(face_at(B, "+z")), &name(&["f8/new/0"]));
    assert_eq!((r.by_identity, r.untracked), (1, 1));
    assert_eq!(r.names.untracked(), 1);
    assert_eq!(r.names.face(face_at(B, "-y")), &name(&["f2/side/l0"]));
}

#[test]
fn new_ranks_and_import_ranks_follow_geometry_not_face_order() {
    let mut ranks = vec![];
    for order in [A, B] {
        let t = cuboid([0.0; 3], [4.0, 3.0, 2.0], order);
        let r = run(&Births::of("f1"), &history(6, vec![]), &[], &t);
        assert_eq!(r.untracked, 6);
        let imported = oc_naming::import_names("f1", &t);
        assert_eq!(imported.untracked(), 0, "an import is an origin, not a gap");
        ranks.push(SIDES.map(|s| (r.names.face(face_at(order, s)).clone(), imported.face(face_at(order, s)).clone())));
    }
    assert_eq!(ranks[0], ranks[1]);
    let mut new: Vec<String> = ranks[0].iter().map(|(n, _)| n.tags()[0].to_string()).collect();
    new.sort();
    assert_eq!(new, ["f1/new/0", "f1/new/1", "f1/new/2", "f1/new/3", "f1/new/4", "f1/new/5"]);
    assert!(ranks[0].iter().all(|(_, i)| i.tags()[0].role() == "import"));
}

#[test]
fn surfaces_are_compared_by_their_parameters() {
    use oc_kernel::Axis;
    use oc_naming::track::same_surface;
    let tol = Identity::default();
    let plane = |z: f64, n: f64| Surface::Plane { origin: [1.0, 2.0, z], normal: [0.0, 0.0, n] };
    assert!(same_surface(&plane(5.0, 1.0), &Surface::Plane { origin: [9.0, -3.0, 5.0], normal: [0.0, 0.0, -1.0] }, &tol));
    assert!(!same_surface(&plane(5.0, 1.0), &plane(5.001, 1.0), &tol));
    let cyl = |o: [f64; 3], r: f64| Surface::Cylinder { axis: Axis { origin: o, dir: [0.0, 0.0, 1.0] }, radius: r };
    assert!(same_surface(&cyl([1.0, 1.0, 0.0], 3.0), &cyl([1.0, 1.0, 7.0], 3.0), &tol));
    assert!(!same_surface(&cyl([1.0, 1.0, 0.0], 3.0), &cyl([1.0, 1.5, 0.0], 3.0), &tol));
    assert!(!same_surface(&cyl([1.0, 1.0, 0.0], 3.0), &cyl([1.0, 1.0, 0.0], 3.5), &tol));
    let cone = |z: f64, r: f64| Surface::Cone { axis: Axis { origin: [0.0, 0.0, z], dir: [0.0, 0.0, 1.0] }, radius: r, half_angle: 0.5 };
    assert!(same_surface(&cone(0.0, 1.0), &cone(2.0, 1.0 + 2.0 * 0.5f64.tan()), &tol));
    assert!(!same_surface(&cone(0.0, 1.0), &cone(2.0, 1.0), &tol));
    assert!(!same_surface(&Surface::BSpline, &Surface::BSpline, &tol), "free-form surfaces have nothing to compare");
    assert!(!same_surface(&plane(0.0, 1.0), &cyl([0.0; 3], 1.0), &tol));
}
