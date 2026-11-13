//! History semantics, operation by operation: what persistent naming (plan section 3.2)
//! relies on.
mod common;

use common::*;
use oc_kernel::*;
use std::f64::consts::PI;

const Z: P3 = [0.0, 0.0, 1.0];

fn images(op: &Op<oc_kernel_occt::Body>, s: Source) -> Vec<Image> {
    op.history.fate(s).map(|f| f.images.clone()).unwrap_or_default()
}

#[test]
fn prism_sides_and_caps() {
    let k = kernel();
    let op = prism(&k, Region::new(rect(0.0, 0.0, 20.0, 10.0, 0.0)), [0.0, 0.0, 5.0]);
    assert_tracked(&op);
    let t = k.topology(&op.body).unwrap();
    // Segment i of the rectangle (counter-clockwise from (0,0)) makes the side whose outward
    // normal is the segment direction turned clockwise.
    let normals = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
    for (i, n) in normals.iter().enumerate() {
        let im = images(&op, Source::Segment { region: 0, wire: 0, segment: i as u32 });
        assert_eq!(im.len(), 1, "segment {i}");
        assert_eq!(im[0].relation, Relation::Generated);
        assert!(v3::dist(t.faces[im[0].index as usize].normal, *n) < 1e-9, "segment {i}");
    }
    let start = images(&op, Source::Cap { region: 0, end: CapEnd::Start });
    let end = images(&op, Source::Cap { region: 0, end: CapEnd::End });
    assert_eq!((start.len(), end.len()), (1, 1));
    assert!(near(t.faces[start[0].index as usize].centroid[2], 0.0));
    assert!(near(t.faces[end[0].index as usize].centroid[2], 5.0));
    // Each face has exactly one origin.
    assert!(op.history.origins(6).iter().all(|o| o.len() == 1));
}

#[test]
fn prism_hole_wire_and_reversed_direction() {
    let k = kernel();
    // Extruding downwards must still give a positive solid, and the hole's circle is wire 1.
    let r = Region::new(rect(0.0, 0.0, 20.0, 20.0, 0.0)).with_hole(circle(10.0, 10.0, 4.0, 0.0));
    let op = prism(&k, r, [0.0, 0.0, -3.0]);
    assert_tracked(&op);
    let m = k.mass_properties(&op.body).unwrap();
    assert!((m.volume - (400.0 - 16.0 * PI) * 3.0).abs() < 1e-6);
    let hole = images(&op, Source::Segment { region: 0, wire: 1, segment: 0 });
    assert_eq!(hole.len(), 1);
    let t = k.topology(&op.body).unwrap();
    assert!(matches!(t.faces[hole[0].index as usize].surface, Surface::Cylinder { radius, .. } if near(radius, 4.0)));
}

#[test]
fn boolean_cut_box_minus_cylinder() {
    // The P1 spike's case: top and bottom modified (holed), sides kept, tool wall becomes
    // the hole, tool caps deleted.
    let k = kernel();
    let b = k.primitive(&Primitive::Block { min: [0.0; 3], size: [20.0, 10.0, 5.0] }).unwrap().body;
    let c = k.primitive(&Primitive::Cylinder { axis: Axis { origin: [10.0, 5.0, -1.0], dir: Z }, radius: 3.0, height: 7.0 }).unwrap().body;
    let op = k.boolean(BooleanOp::Cut, &b, &[&c]).unwrap();
    assert_tracked(&op);
    let tb = k.topology(&b).unwrap();
    let tr = k.topology(&op.body).unwrap();
    for (i, f) in tb.faces.iter().enumerate() {
        let im = images(&op, Source::Face { arg: 0, face: FaceId(i as u32) });
        assert_eq!(im.len(), 1);
        let horizontal = f.normal[2].abs() > 0.5;
        let want = if horizontal { Relation::Modified } else { Relation::Same };
        assert_eq!(im[0].relation, want, "box face {i}");
        if horizontal {
            assert!(near(tr.faces[im[0].index as usize].area, 200.0 - 9.0 * PI));
        }
    }
    let tc = k.topology(&c).unwrap();
    for (i, f) in tc.faces.iter().enumerate() {
        let fate = op.history.fate(Source::Face { arg: 1, face: FaceId(i as u32) }).unwrap();
        if matches!(f.surface, Surface::Cylinder { .. }) {
            assert_eq!(fate.images.len(), 1);
            assert_eq!(fate.images[0].relation, Relation::Modified);
        } else {
            assert!(fate.deleted && fate.images.is_empty(), "cylinder cap {i}");
        }
    }
}

#[test]
fn fuse_merges_coplanar_faces_and_unions_history() {
    // H17: a boss flush with three sides. The merged faces carry both inputs' faces.
    let k = kernel();
    let a = block(&k, 40.0, 20.0, 10.0);
    let b = prism(&k, Region::new(rect(0.0, 0.0, 40.0, 10.0, 10.0)), [0.0, 0.0, 5.0]).body;
    let op = k.boolean(BooleanOp::Fuse, &a, &[&b]).unwrap();
    assert_tracked(&op);
    let t = k.topology(&op.body).unwrap();
    assert_eq!(t.faces.len(), 8, "L-shaped prism");
    assert!(near(k.mass_properties(&op.body).unwrap().volume, 10000.0));
    let front = face_where(&t, planar([0.0, -1.0, 0.0], 1, 0.0));
    let origins = &op.history.origins(t.faces.len())[front.index()];
    let args: std::collections::BTreeSet<u32> = origins
        .iter()
        .map(|(s, _)| match s {
            Source::Face { arg, .. } => *arg,
            _ => 99,
        })
        .collect();
    assert_eq!(args, [0, 1].into(), "merged y = 0 face comes from both bodies");
}

#[test]
fn fillet_generates_from_edges_and_vertices() {
    let k = kernel();
    // Rim fillet: the edge between the top face and the hole generates one torus.
    let b = block(&k, 20.0, 10.0, 5.0);
    let tool = prism(&k, Region::new(circle(10.0, 5.0, 3.0, -1.0)), [0.0, 0.0, 7.0]).body;
    let b = cut(&k, &b, &tool);
    let t = k.topology(&b).unwrap();
    let top = face_where(&t, planar(Z, 2, 5.0));
    let hole = face_where(&t, |f| matches!(f.surface, Surface::Cylinder { .. }));
    let rim = t.edges_between(top, hole);
    assert_eq!(rim.len(), 1);
    let op = k.fillet(&b, &[FilletEdge { edge: rim[0], radius: Radius::Constant(1.0) }]).unwrap();
    assert_tracked(&op);
    let gen = images(&op, Source::Edge { arg: 0, edge: rim[0] });
    assert_eq!(gen.len(), 1);
    assert_eq!(gen[0].relation, Relation::Generated);
    let tr = k.topology(&op.body).unwrap();
    assert!(matches!(tr.faces[gen[0].index as usize].surface, Surface::Torus { .. }));
    for f in [top, hole] {
        let im = images(&op, Source::Face { arg: 0, face: f });
        assert_eq!(im.len(), 1);
        assert_eq!(im[0].relation, Relation::Modified);
    }
    // Three fillets meeting at a corner: the vertex generates the corner patch (H04).
    let b = block(&k, 30.0, 30.0, 30.0);
    let t = k.topology(&b).unwrap();
    let corner: Vec<_> = t
        .edges
        .iter()
        .enumerate()
        .filter(|(_, e)| e.mid.iter().filter(|x| near(**x, 30.0)).count() == 2)
        .map(|(i, _)| FilletEdge { edge: EdgeId(i as u32), radius: Radius::Constant(5.0) })
        .collect();
    assert_eq!(corner.len(), 3);
    let op = k.fillet(&b, &corner).unwrap();
    assert_tracked(&op);
    let want = 27000.0 - 3.0 * (1.0 - PI / 4.0) * 25.0 * 25.0 - 125.0 * (1.0 - PI / 6.0);
    assert!((k.mass_properties(&op.body).unwrap().volume - want).abs() < 1e-4 * want);
    let from_vertex = op.history.fates.iter().filter(|f| matches!(f.source, Source::Vertex { .. })).count();
    assert_eq!(from_vertex, 1, "the corner patch comes from the shared vertex");
}

#[test]
fn shell_removes_open_faces() {
    let k = kernel();
    let b = block(&k, 40.0, 30.0, 20.0);
    let top = face_where(&k.topology(&b).unwrap(), planar(Z, 2, 20.0));
    let op = k.shell(&b, &ShellSpec { open: vec![top], thickness: 2.0, outward: false }).unwrap();
    assert_tracked(&op);
    // OCCT reports the open face as modified into the rim: the thin frame left in its
    // plane. (Not "deleted": the rim is what remains of it.)
    let rim = images(&op, Source::Face { arg: 0, face: top });
    assert_eq!(rim.len(), 1);
    assert_eq!(rim[0].relation, Relation::Modified);
    let rim_face = &k.topology(&op.body).unwrap().faces[rim[0].index as usize];
    assert!(near(rim_face.centroid[2], 20.0) && near(rim_face.area, 1200.0 - 36.0 * 26.0));
    assert!(near(k.mass_properties(&op.body).unwrap().volume, 24000.0 - 36.0 * 26.0 * 18.0));
    // Every other face yields its outer wall and its inner offset wall.
    for i in 0..6u32 {
        if FaceId(i) != top {
            assert_eq!(images(&op, Source::Face { arg: 0, face: FaceId(i) }).len(), 2, "face {i}");
        }
    }
}

#[test]
fn revolve_caps_only_when_partial() {
    let k = kernel();
    let w = rect(40.0, 0.0, 45.0, 5.0, 0.0);
    let axis = Axis { origin: [0.0; 3], dir: [0.0, 1.0, 0.0] };
    let half = k.revolve(&RevolveSpec { profile: Profile::single(w.clone()), axis, angle: PI }).unwrap();
    assert_tracked(&half);
    assert!((k.mass_properties(&half.body).unwrap().volume - PI / 2.0 * (45.0f64.powi(2) - 1600.0) * 5.0).abs() < 1e-6);
    assert_eq!(half.body.face_count(), 6);
    assert_eq!(images(&half, Source::Cap { region: 0, end: CapEnd::Start }).len(), 1);
    assert_eq!(images(&half, Source::Cap { region: 0, end: CapEnd::End }).len(), 1);
    let full = k.revolve(&RevolveSpec { profile: Profile::single(w), axis, angle: 2.0 * PI }).unwrap();
    assert_tracked(&full);
    assert_eq!(full.body.face_count(), 4);
    assert!(full.history.fates.iter().all(|f| !matches!(f.source, Source::Cap { .. })));
}

#[test]
fn transform_compound_and_section() {
    let k = kernel();
    let b = block(&k, 20.0, 10.0, 5.0);
    let moved = k.transform(&b, &Transform::translation([100.0, 0.0, 0.0])).unwrap();
    assert_tracked(&moved);
    assert!(moved.history.fates.iter().all(|f| f.images.len() == 1 && f.images[0].relation == Relation::Modified));
    let group = k.compound(&[&b, &moved.body]).unwrap();
    assert_tracked(&group);
    assert!(group.history.fates.iter().all(|f| f.images.len() == 1 && f.images[0].relation == Relation::Same));
    assert_eq!(k.topology(&group.body).unwrap().solids, 2);
    // Section of the holed plate at mid height: 4 lines + 1 circle, each from its face.
    let tool = prism(&k, Region::new(circle(10.0, 5.0, 3.0, -1.0)), [0.0, 0.0, 7.0]).body;
    let holed = cut(&k, &b, &tool);
    let s = k.section(&holed, &Plane { origin: [0.0, 0.0, 2.5], normal: Z }).unwrap();
    assert_eq!(s.history.image_kind, TopoKind::Edge);
    let t = k.topology(&s.body).unwrap();
    assert_eq!(t.edges.len(), 5);
    let length: f64 = t.edges.iter().map(|e| e.length).sum();
    assert!((length - (60.0 + 6.0 * PI)).abs() < 1e-6);
    s.history.check(t.edges.len()).unwrap();
    assert!(s.history.untracked(t.edges.len()).is_empty());
    assert!(s.history.fates.iter().all(|f| f.images.len() == 1 && f.images[0].relation == Relation::Generated));
}

#[test]
fn loft_sections_and_caps() {
    let k = kernel();
    let op = k.loft(&LoftSpec { sections: vec![rect(0.0, 0.0, 20.0, 20.0, 0.0), rect(5.0, 5.0, 15.0, 15.0, 10.0)], ruled: true }).unwrap();
    assert_tracked(&op);
    // Ruled frustum of a square pyramid: h/3 (A1 + A2 + √(A1 A2)).
    assert!((k.mass_properties(&op.body).unwrap().volume - 10.0 / 3.0 * (400.0 + 100.0 + 200.0)).abs() < 1e-6);
    for s in 0..4 {
        assert_eq!(images(&op, Source::Segment { region: 0, wire: 0, segment: s }).len(), 1);
    }
    assert_eq!(images(&op, Source::Cap { region: 0, end: CapEnd::Start }).len(), 1);
    assert_eq!(images(&op, Source::Cap { region: 0, end: CapEnd::End }).len(), 1);
}

#[test]
fn tapered_prism_is_an_exact_frustum() {
    let k = kernel();
    let a = deg(5.0);
    let spec = |w: Wire, outward: bool| PrismSpec { profile: Profile::single(w), vector: [0.0, 0.0, 8.0], taper: Some(Taper { angle: a, outward }) };
    // Inward (default): the 10×8 rectangle shrinks by 8 tan 5° per side at the top.
    let op = k.prism(&spec(rect(5.0, 20.0, 15.0, 28.0, 10.0), false)).unwrap();
    assert_tracked(&op);
    let d = 8.0 * a.tan();
    // Prismatoid: h/6 (A1 + 4 Amid + A2) (the sections are not similar, so not the cone formula).
    let (a1, am, a2) = (80.0, (10.0 - d) * (8.0 - d), (10.0 - 2.0 * d) * (8.0 - 2.0 * d));
    let frustum = 8.0 / 6.0 * (a1 + 4.0 * am + a2);
    assert!((k.mass_properties(&op.body).unwrap().volume - frustum).abs() < 1e-6);
    let bb = k.bounding_box(&op.body).unwrap();
    assert!(near(bb.min[2], 10.0) && near(bb.max[2], 18.0));
    // Circle, outward: a cone frustum growing from r = 4.
    let op = k.prism(&spec(circle(0.0, 0.0, 4.0, 0.0), true)).unwrap();
    assert_tracked(&op);
    let r2 = 4.0 + d;
    let cone = PI * 8.0 / 3.0 * (16.0 + 4.0 * r2 + r2 * r2);
    assert!((k.mass_properties(&op.body).unwrap().volume - cone).abs() < 1e-6);
    assert!(k.topology(&op.body).unwrap().faces.iter().any(|f| matches!(f.surface, Surface::Cone { .. })));
    // Outward on a square: sharp corners (no corner blends), planar sides, exact frustum.
    let op = k.prism(&spec(rect(-4.0, -4.0, 4.0, 4.0, 0.0), true)).unwrap();
    assert_tracked(&op);
    let s = 8.0 + 2.0 * d;
    assert!((k.mass_properties(&op.body).unwrap().volume - 8.0 / 3.0 * (64.0 + s * s + 8.0 * s)).abs() < 1e-6);
    let t = k.topology(&op.body).unwrap();
    assert_eq!(t.faces.len(), 6);
    assert!(t.faces.iter().all(|f| matches!(f.surface, Surface::Plane { .. })));
    assert!(t.edges.iter().all(|e| e.curve == Curve::Line));
    // A hole widens towards the far end when the material tapers inward.
    let r = Region::new(rect(0.0, 0.0, 20.0, 20.0, 0.0)).with_hole(circle(10.0, 10.0, 4.0, 0.0));
    let op = k.prism(&PrismSpec { profile: Profile::new(vec![r]), vector: [0.0, 0.0, 8.0], taper: Some(Taper { angle: a, outward: false }) }).unwrap();
    assert_tracked(&op);
    let (s, r2) = (20.0 - 2.0 * d, 4.0 + d);
    let want = 8.0 / 3.0 * (400.0 + s * s + 20.0 * s) - PI * 8.0 / 3.0 * (16.0 + 4.0 * r2 + r2 * r2);
    assert!((k.mass_properties(&op.body).unwrap().volume - want).abs() < 1e-6);
    assert_eq!(images(&op, Source::Segment { region: 0, wire: 1, segment: 0 }).len(), 1);
}

#[test]
fn variable_fillet_starts_at_the_edge_start() {
    // Radius::Linear(a, b) puts `a` at EdgeInfo::start. A 90° fillet of radius r leaves the
    // old corner point r(√2 − 1) from the material.
    let k = kernel();
    let b = block(&k, 30.0, 30.0, 30.0);
    let t = k.topology(&b).unwrap();
    let e = edges_where(&t, true, |m| near(m[1], 0.0) && near(m[2], 30.0))[0];
    let (start, end) = (t.edges[e.index()].start, t.edges[e.index()].end);
    let op = k.fillet(&b, &[FilletEdge { edge: e, radius: Radius::Linear(1.0, 4.0) }]).unwrap();
    assert_tracked(&op);
    let gap = |p: P3| k.distance(Target::Point(p), Target::Body(&op.body)).unwrap().distance;
    let k2 = 2f64.sqrt() - 1.0;
    assert!((gap(start) - k2).abs() < 1e-3 && (gap(end) - 4.0 * k2).abs() < 1e-3, "{} {}", gap(start), gap(end));
}
