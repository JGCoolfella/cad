//! Queries against analytic values: topology geometry, mass properties, distance,
//! tessellation, STEP round trip, healing.
mod common;

use common::*;
use oc_kernel::*;
use std::f64::consts::PI;

const Z: P3 = [0.0, 0.0, 1.0];

fn holed_plate(k: &oc_kernel_occt::OcctKernel) -> oc_kernel_occt::Body {
    let b = block(k, 20.0, 10.0, 5.0);
    cut(k, &b, &prism(k, Region::new(circle(10.0, 5.0, 3.0, -1.0)), [0.0, 0.0, 7.0]).body)
}

#[test]
fn topology_geometry() {
    let k = kernel();
    let b = holed_plate(&k);
    let t = k.topology(&b).unwrap();
    assert_eq!((t.solids, t.shells, t.faces.len(), t.edges.len(), t.vertices.len()), (1, 1, 7, 15, 10));
    let hole = face_where(&t, |f| matches!(f.surface, Surface::Cylinder { .. }));
    let f = &t.faces[hole.index()];
    let Surface::Cylinder { axis, radius } = f.surface else { unreachable!() };
    assert!(near(radius, 3.0) && near(axis.origin[0], 10.0) && near(axis.origin[1], 5.0));
    assert!(near(v3::cross(axis.dir, Z)[0].abs() + v3::cross(axis.dir, Z)[1].abs(), 0.0));
    assert!(near(f.area, 6.0 * PI * 5.0));
    // A cylinder's area centroid lies on its axis; its signature normal is radial.
    assert!(near(f.centroid[0], 10.0) && near(f.centroid[1], 5.0) && near(f.centroid[2], 2.5));
    assert!(near(f.normal[2], 0.0) && near(v3::len(f.normal), 1.0));
    // Seam edge of the hole, the two rim circles.
    let seams: Vec<_> = f.edges.iter().filter(|e| t.edges[e.index()].seam).collect();
    assert_eq!(seams.len(), 1);
    let circles: Vec<_> = t.edges.iter().filter(|e| matches!(e.curve, Curve::Circle { radius, .. } if near(radius, 3.0))).collect();
    assert_eq!(circles.len(), 2);
    assert!(circles.iter().all(|c| near(c.length, 6.0 * PI) && c.faces.len() == 2));
    let top = face_where(&t, planar(Z, 2, 5.0));
    assert!(near(t.faces[top.index()].area, 200.0 - 9.0 * PI));
    assert_eq!(t.edges_between(top, hole).len(), 1);
    let bb = k.bounding_box(&b).unwrap();
    assert!(v3::dist(bb.min, [0.0; 3]) < 1e-6 && v3::dist(bb.max, [20.0, 10.0, 5.0]) < 1e-6);
}

#[test]
fn mass_properties_of_a_box() {
    let k = kernel();
    let b = block(&k, 20.0, 10.0, 5.0);
    let m = k.mass_properties(&b).unwrap();
    assert!(near(m.volume, 1000.0) && near(m.area, 700.0));
    assert!(v3::dist(m.center, [10.0, 5.0, 2.5]) < 1e-9);
    // Box about its centre: I = m (b² + c²) / 12 etc., no products of inertia.
    let want = [1000.0 * 125.0 / 12.0, 1000.0 * 425.0 / 12.0, 1000.0 * 500.0 / 12.0];
    for (i, w) in want.iter().enumerate() {
        for (j, x) in m.inertia[i].iter().enumerate() {
            let expect = if i == j { *w } else { 0.0 };
            assert!((x - expect).abs() < 1e-6, "{:?}", m.inertia);
        }
        assert!((m.principal_moments[i] - w).abs() < 1e-6);
    }
    assert!(v3::cross(m.principal_axes[0], [1.0, 0.0, 0.0]).iter().all(|x| x.abs() < 1e-9));
    // Holed plate: centre of mass unchanged by the symmetric hole.
    let m = k.mass_properties(&holed_plate(&k)).unwrap();
    assert!((m.volume - (1000.0 - 45.0 * PI)).abs() < 1e-9);
    assert!(v3::dist(m.center, [10.0, 5.0, 2.5]) < 1e-9);
}

#[test]
fn distances() {
    let k = kernel();
    let b = holed_plate(&k);
    let t = k.topology(&b).unwrap();
    let left = face_where(&t, planar([-1.0, 0.0, 0.0], 0, 0.0));
    let right = face_where(&t, planar([1.0, 0.0, 0.0], 0, 20.0));
    let hole = face_where(&t, |f| matches!(f.surface, Surface::Cylinder { .. }));
    let d = k.distance(Target::Face(&b, left), Target::Face(&b, right)).unwrap();
    assert!(near(d.distance, 20.0));
    let d = k.distance(Target::Face(&b, left), Target::Face(&b, hole)).unwrap();
    assert!(near(d.distance, 7.0) && near(d.b[0], 7.0));
    let d = k.distance(Target::Point([2.0, 2.0, 50.0]), Target::Body(&b)).unwrap();
    assert!(near(d.distance, 45.0));
    assert!(v3::dist(d.a, [2.0, 2.0, 50.0]) < 1e-9 && v3::dist(d.b, [2.0, 2.0, 5.0]) < 1e-9);
    // Above the hole the nearest material is its rim.
    let d = k.distance(Target::Point([10.0, 5.0, 50.0]), Target::Body(&b)).unwrap();
    assert!(near(d.distance, (45.0f64 * 45.0 + 9.0).sqrt()));
    let d = k.distance(Target::Point([10.0, 5.0, 2.5]), Target::Face(&b, hole)).unwrap();
    assert!(near(d.distance, 3.0));
    let other = k.transform(&b, &Transform::translation([0.0, 0.0, 12.0])).unwrap().body;
    let d = k.distance(Target::Body(&b), Target::Body(&other)).unwrap();
    assert!(near(d.distance, 7.0));
    let d = k.distance(Target::Vertex(&b, VertexId(0)), Target::Edge(&b, EdgeId(0))).unwrap();
    assert!(near(d.distance, 0.0));
}

#[test]
fn tessellation() {
    let k = kernel();
    let b = block(&k, 20.0, 10.0, 5.0);
    let m = k.tessellate(&b, &MeshParams { linear: 0.1, angular: 0.5 }).unwrap();
    assert_eq!(m.faces.len(), 6);
    assert_eq!(m.edges.len(), 12);
    assert!((m.area() - 700.0).abs() < 1e-9, "planar faces are tessellated exactly");
    assert_eq!(m.normals.len(), m.positions.len());
    // Counter-clockwise seen from outside: geometric normal agrees with the vertex normals.
    let t = k.topology(&b).unwrap();
    for (f, r) in m.faces.iter().enumerate() {
        assert!(!r.is_empty());
        for tri in &m.triangles[r.start as usize..r.end as usize] {
            let [a, bb, c] = tri.map(|i| m.positions[i as usize]);
            let n = v3::cross(v3::sub(bb, a), v3::sub(c, a));
            assert!(v3::dot(n, t.faces[f].normal) > 0.0);
            let vn = m.normals[tri[0] as usize];
            assert!(v3::dot([vn[0] as f64, vn[1] as f64, vn[2] as f64], t.faces[f].normal) > 0.999);
        }
    }
    // Curved: the mesh area converges to the exact area as the deflection shrinks.
    let c = holed_plate(&k);
    let exact = k.mass_properties(&c).unwrap().area;
    let coarse = k.tessellate(&c, &MeshParams { linear: 0.1, angular: 0.5 }).unwrap();
    let fine = k.tessellate(&c, &MeshParams { linear: 0.001, angular: 0.05 }).unwrap();
    assert!((fine.area() - exact).abs() < (coarse.area() - exact).abs());
    assert!((fine.area() - exact).abs() < 1e-3 * exact);
    // Output depends only on the parameters, not on an earlier finer mesh of the same shape.
    let fresh = k.tessellate(&holed_plate(&k), &MeshParams { linear: 0.1, angular: 0.5 }).unwrap();
    let again = k.tessellate(&c, &MeshParams { linear: 0.1, angular: 0.5 }).unwrap();
    assert_eq!(again, fresh);
    assert_eq!(again, coarse);
    // Every edge has a polyline from its start to its end.
    let tt = k.topology(&c).unwrap();
    for (i, e) in tt.edges.iter().enumerate() {
        let p = &again.edges[i];
        assert!(p.len() >= 2);
        let ends = [p[0], p[p.len() - 1]];
        assert!(ends.iter().any(|q| v3::dist(*q, e.start) < 1e-6) && ends.iter().any(|q| v3::dist(*q, e.end) < 1e-6));
    }
}

#[test]
fn step_round_trip() {
    let k = kernel();
    let b = holed_plate(&k);
    let f = k.fillet(&b, &[FilletEdge { edge: EdgeId(0), radius: Radius::Constant(1.0) }]).unwrap().body;
    let data = k.write_step(&[&f]).unwrap();
    let text = String::from_utf8_lossy(&data);
    assert!(text.starts_with("ISO-10303-21;") && text.contains("AUTOMOTIVE_DESIGN"), "AP214");
    let back = k.read_step(&data).unwrap().body;
    let (a, b2) = (metrics(&k, &f), metrics(&k, &back));
    assert!((a.volume - b2.volume).abs() < 1e-6 * a.volume && (a.area - b2.area).abs() < 1e-6 * a.area);
    assert_eq!((a.faces, a.edges, a.solids), (b2.faces, b2.edges, b2.solids));
    // Several bodies in one file come back as a multi-body result.
    let moved = k.transform(&f, &Transform::translation([50.0, 0.0, 0.0])).unwrap().body;
    let two = k.read_step(&k.write_step(&[&f, &moved]).unwrap()).unwrap().body;
    assert_eq!(k.topology(&two).unwrap().solids, 2);
}

#[test]
fn heal_keeps_a_valid_body() {
    let k = kernel();
    let b = holed_plate(&k);
    let h = k.heal(&b).unwrap();
    assert_tracked(&h);
    assert!(k.check(&h.body).unwrap().valid);
    assert!(near(k.mass_properties(&h.body).unwrap().volume, 1000.0 - 45.0 * PI));
    assert!(h.history.fates.iter().all(|f| f.images.len() == 1));
}

#[test]
fn check_reports_a_valid_body() {
    let k = kernel();
    let c = k.check(&holed_plate(&k)).unwrap();
    assert!(c.valid && c.problems.is_empty());
    assert!(k.name().starts_with("OCCT 8.0.1"));
}

#[test]
fn solids_split_a_body() {
    let k = kernel();
    // Two separate blocks, grouped, then a cut that splits the second one in two.
    let a = block(&k, 10.0, 10.0, 10.0);
    let b = prism(&k, Region::new(rect(20.0, 0.0, 40.0, 10.0, 0.0)), [0.0, 0.0, 10.0]).body;
    let g = k.compound(&[&b, &a]).unwrap().body;
    let g = cut(&k, &g, &prism(&k, Region::new(rect(29.0, -1.0, 31.0, 11.0, -1.0)), [0.0, 0.0, 12.0]).body);
    let t = k.topology(&g).unwrap();
    assert_eq!(t.solids, 3);
    let parts = k.solids(&g).unwrap();
    assert_eq!(parts.len(), 3);
    let vols: Vec<f64> = parts.iter().map(|p| k.mass_properties(&p.body).unwrap().volume).collect();
    // Ordered by the lowest face index: the first solid owns face 0.
    assert!(near(vols.iter().sum::<f64>(), 1000.0 + 2.0 * 900.0));
    let mut sorted = vols.clone();
    sorted.sort_by(f64::total_cmp);
    assert!(near(sorted[0], 900.0) && near(sorted[1], 900.0) && near(sorted[2], 1000.0));
    let mut owner = vec![usize::MAX; t.faces.len()];
    for (s, p) in parts.iter().enumerate() {
        p.history.check(p.body.face_count()).unwrap();
        for fate in &p.history.fates {
            let Source::Face { face, .. } = fate.source else { continue };
            if !fate.deleted {
                assert_eq!(fate.images.len(), 1);
                assert_eq!(fate.images[0].relation, Relation::Same);
                assert_eq!(owner[face.index()], usize::MAX, "a face belongs to one solid");
                owner[face.index()] = s;
            }
        }
        assert_eq!(k.topology(&p.body).unwrap().solids, 1);
    }
    assert!(owner.iter().all(|&o| o != usize::MAX));
    assert_eq!(owner[0], 0);
    let firsts: Vec<usize> = (0..3).map(|s| owner.iter().position(|&o| o == s).unwrap()).collect();
    assert!(firsts.windows(2).all(|w| w[0] < w[1]));
    assert!(k.solids(&g).unwrap().iter().zip(&parts).all(|(x, y)| k.topology(&x.body).unwrap() == k.topology(&y.body).unwrap()));
}

#[test]
fn distance_to_a_segment() {
    let k = kernel();
    let b = block(&k, 10.0, 10.0, 10.0);
    let d = k.distance(Target::Body(&b), Target::Segment([15.0, 5.0, -3.0], [15.0, 5.0, 3.0])).unwrap();
    assert!(near(d.distance, 5.0));
    let d = k.distance(Target::Segment([12.0, 12.0, 5.0], [20.0, 20.0, 5.0]), Target::Body(&b)).unwrap();
    assert!(near(d.distance, 8f64.sqrt()) && near(d.a[0], 12.0) && near(d.b[0], 10.0));
    let d = k.distance(Target::Body(&b), Target::Segment([5.0, 5.0, 12.0], [5.0, 5.0, 12.0])).unwrap();
    assert!(near(d.distance, 2.0));
}

/// OCCT keeps a triangulation on each (shared) face and reuses it when a later request is
/// within ±10 % of its deflection. A mesh must depend only on the body and the parameters:
/// not on what was meshed before, nor on other bodies sharing the face.
#[test]
fn tessellation_is_independent_of_earlier_meshes() {
    let k = kernel();
    let p = MeshParams { linear: 0.05, angular: 0.5 };
    let near_p = MeshParams { linear: 0.054, angular: 0.5 };
    let sphere = || k.primitive(&Primitive::Sphere { center: [0.0; 3], radius: 20.0 }).unwrap().body;
    let cold = k.tessellate(&sphere(), &near_p).unwrap();
    let s = sphere();
    let first = k.tessellate(&s, &p).unwrap();
    let warm = k.tessellate(&s, &near_p).unwrap();
    assert_ne!(first.triangles.len(), cold.triangles.len(), "the two deflections give different meshes");
    assert_eq!(warm, cold, "a nearby earlier mesh is not reused");
    // A body sharing unchanged faces with an earlier body.
    let plate = holed_plate(&k);
    let tool = prism(&k, Region::new(circle(2.0, 2.0, 1.0, -1.0)), [0.0, 0.0, 20.0]).body;
    let p = MeshParams { linear: 0.01, angular: 0.2 };
    let cold = k.tessellate(&cut(&k, &holed_plate(&k), &tool), &p).unwrap();
    k.tessellate(&plate, &p).unwrap();
    let warm = k.tessellate(&cut(&k, &plate, &tool), &p).unwrap();
    assert_eq!(warm, cold, "faces shared with an earlier body are meshed afresh");
}

/// Vertex normals are the exact surface normals (outward), not averages of facet normals.
#[test]
fn tessellation_normals_are_exact() {
    let k = kernel();
    let r = 7.0;
    let s = k.primitive(&Primitive::Sphere { center: [1.0, 2.0, 3.0], radius: r }).unwrap().body;
    let m = k.tessellate(&s, &MeshParams { linear: 0.5, angular: 0.8 }).unwrap();
    for (p, n) in m.positions.iter().zip(&m.normals) {
        let want = v3::scale(v3::sub(*p, [1.0, 2.0, 3.0]), 1.0 / r);
        let got = [n[0] as f64, n[1] as f64, n[2] as f64];
        assert!(v3::dist(got, want) < 1e-6, "normal {got:?} at {p:?}, want {want:?}");
    }
    // Inside a hole the normal points into the hole's axis (out of the material).
    let c = holed_plate(&k);
    let t = k.topology(&c).unwrap();
    let m = k.tessellate(&c, &MeshParams { linear: 0.5, angular: 0.8 }).unwrap();
    for (f, info) in t.faces.iter().enumerate() {
        if let Surface::Cylinder { axis, .. } = info.surface {
            for tri in &m.triangles[m.faces[f].start as usize..m.faces[f].end as usize] {
                for &i in tri {
                    let p = m.positions[i as usize];
                    let d = v3::sub(p, axis.origin);
                    let radial = v3::sub(d, v3::scale(axis.dir, v3::dot(d, axis.dir)));
                    let want = v3::scale(radial, -1.0 / v3::len(radial));
                    let n = m.normals[i as usize];
                    assert!(v3::dist([n[0] as f64, n[1] as f64, n[2] as f64], want) < 1e-6);
                }
            }
        }
    }
}

/// Faces sharing an edge use exactly the edge polyline's points for their boundary nodes
/// (no welding tolerance needed). Before this was enforced, a pin's top face and its
/// cylinder put the circle's vertex 1e-14 mm apart (corpus `patterns`).
#[test]
fn tessellation_shares_boundary_nodes_exactly() {
    let k = kernel();
    let plate = block(&k, 100.0, 60.0, 5.0);
    let pin = prism(&k, Region::new(circle(70.0, 30.0, 3.0, 5.0)), [0.0, 0.0, 4.0]).body;
    let axis = Axis { origin: [50.0, 30.0, 0.0], dir: Z };
    let pins: Vec<_> = (0..6).map(|i| k.transform(&pin, &Transform::rotation(axis, PI / 3.0 * i as f64)).unwrap().body).collect();
    let b = k.boolean(BooleanOp::Fuse, &plate, &pins.iter().collect::<Vec<_>>()).unwrap().body;
    let b = cut(&k, &b, &prism(&k, Region::new(circle(20.0, 20.0, 5.0, -1.0)), [0.0, 0.0, 10.0]).body);
    let t = k.topology(&b).unwrap();
    let m = k.tessellate(&b, &MeshParams { linear: 0.02, angular: 0.2 }).unwrap();
    let key = |p: P3| p.map(|c| (c + 0.0).to_bits());
    for (e, info) in t.edges.iter().enumerate() {
        assert!(m.edges[e].len() >= 2);
        for f in &info.faces {
            let r = &m.faces[f.index()];
            let on: std::collections::HashSet<_> =
                m.triangles[r.start as usize..r.end as usize].iter().flatten().map(|&i| key(m.positions[i as usize])).collect();
            for p in &m.edges[e] {
                assert!(on.contains(&key(*p)), "edge {e}: {p:?} is not a node of face {}", f.0);
            }
        }
        for end in [m.edges[e][0], *m.edges[e].last().unwrap()] {
            assert!(info.vertices.iter().any(|v| key(t.vertices[v.index()]) == key(end)), "edge {e} ends off its vertices");
        }
    }
}
