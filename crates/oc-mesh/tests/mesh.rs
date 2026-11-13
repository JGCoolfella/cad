//! Display meshes of the corpus parts: layout invariants, watertightness, orientation,
//! normals, edge polylines on the face boundaries, levels of detail.
mod common;

use common::*;
use oc_kernel::*;
use oc_mesh::*;
use std::collections::HashSet;

fn opts() -> MeshOptions {
    MeshOptions::default()
}

/// Ranges tile the buffers in face (edge) order; every vertex carries its face's index;
/// indices stay inside their face's vertices; the index width fits.
fn assert_layout(m: &RenderMesh, topo: &Topology, what: &str) {
    assert_eq!(m.face_count(), topo.faces.len(), "{what}");
    assert_eq!(m.edge_count(), topo.edges.len(), "{what}");
    assert_eq!(m.points.len(), topo.vertices.len(), "{what}");
    for (lv, l) in m.lods.iter().enumerate() {
        assert_eq!(l.vertices.len(), l.vertex_count as usize * VERTEX_STRIDE);
        assert_eq!(
            l.indices.len(),
            (l.index_count as usize * l.index_format.bytes()).next_multiple_of(4)
        );
        assert_eq!(l.segments.len(), l.segment_count as usize * SEGMENT_STRIDE);
        assert_eq!(
            l.index_format,
            IndexFormat::for_vertex_count(l.vertex_count as usize)
        );
        let (mut v, mut i) = (0, 0);
        for (f, r) in l.faces.iter().enumerate() {
            assert_eq!(
                (r.first_vertex, r.first_index),
                (v, i),
                "{what} level {lv} face {f}"
            );
            assert!(
                r.index_count > 0,
                "{what} level {lv}: face {f} has no triangles"
            );
            v += r.vertex_count;
            i += r.index_count;
            for k in r.first_vertex..r.first_vertex + r.vertex_count {
                assert_eq!(l.vertex(k as usize).face, f as u32);
            }
            for k in r.first_index..r.first_index + r.index_count {
                let x = l.index(k as usize);
                assert!(x >= r.first_vertex && x < r.first_vertex + r.vertex_count);
            }
        }
        assert_eq!((v, i), (l.vertex_count, l.index_count));
        let mut s = 0;
        for (e, r) in l.edges.iter().enumerate() {
            assert_eq!(r.first_segment, s);
            s += r.segment_count;
            let hidden = m.edge_flags[e] != 0;
            assert_eq!(
                hidden,
                r.segment_count == 0,
                "{what} level {lv} edge {e}: flags {}",
                m.edge_flags[e]
            );
            for k in r.first_segment..r.first_segment + r.segment_count {
                assert_eq!(l.segment(k as usize).edge, e as u32);
            }
        }
        assert_eq!(s, l.segment_count);
    }
    for (e, info) in topo.edges.iter().enumerate() {
        assert_eq!(m.edge_flags[e] & EDGE_SEAM != 0, info.seam);
    }
}

/// At the lowest, default and highest Image Quality, every level of every part. (At
/// quality 10, level 1 of `revolve` is a case where the fast planar triangulator fails and
/// the kernel re-meshes with its fallback.)
#[test]
fn corpus_parts_are_watertight_and_outward() {
    let k = kernel();
    for (name, b) in corpus_parts(&k)
        .into_iter()
        .flat_map(|p| [0.0, 3.0, 10.0].map(|q| (p.clone(), q)))
        .map(|((n, b), q)| (n, (b, q)))
    {
        let (b, quality) = b;
        let m = build(&k, &b, &MeshOptions { quality, ..opts() }).unwrap();
        let topo = k.topology(&b).unwrap();
        assert_layout(&m, &topo, name);
        let exact = k.mass_properties(&b).unwrap();
        for lv in 0..m.lods.len() {
            assert_watertight(&m, lv, name);
            // Outward winding gives a positive volume; the mesh surface stays within the
            // chordal deviation d of the exact one, so |ΔV| ≤ area × d.
            let (v, a) = volume_area(&m, lv);
            let d = m.lods[lv].params.linear;
            assert!(
                (v - exact.volume).abs() <= exact.area * d * 1.01 + 1e-4 * exact.volume.abs(),
                "{name} level {lv}: volume {v} vs {}",
                exact.volume
            );
            assert!(
                a > 0.9 * exact.area && a < 1.01 * exact.area,
                "{name} level {lv}: area {a} vs {}",
                exact.area
            );
        }
    }
}

/// Every segment end is a vertex of a face bounded by its edge (exactly the same f32
/// position): the drawn edges sit on the shaded faces' boundaries, with no gap.
#[test]
fn edges_lie_on_the_face_boundaries() {
    let k = kernel();
    for (name, b) in corpus_parts(&k) {
        let m = build(&k, &b, &opts()).unwrap();
        let topo = k.topology(&b).unwrap();
        for l in &m.lods {
            for (e, r) in l.edges.iter().enumerate() {
                let mut on: HashSet<[u32; 3]> = HashSet::new();
                for f in &topo.edges[e].faces {
                    let fr = l.faces[f.index()];
                    for v in fr.first_vertex..fr.first_vertex + fr.vertex_count {
                        on.insert(l.vertex(v as usize).position.map(|c| (c + 0.0).to_bits()));
                    }
                }
                for s in r.first_segment..r.first_segment + r.segment_count {
                    let seg = l.segment(s as usize);
                    for p in [seg.a, seg.b] {
                        assert!(
                            on.contains(&p.map(|c| (c + 0.0).to_bits())),
                            "{name} edge {e}: {p:?} is not on its faces"
                        );
                    }
                }
            }
        }
        // Seams are hidden but the parts with cylinders have some.
        if name == "block-hole" {
            assert_eq!(
                m.edge_flags.iter().filter(|&&f| f & EDGE_SEAM != 0).count(),
                1
            );
        }
    }
}

#[test]
fn normals_are_unit_outward_and_exact_on_analytic_faces() {
    let k = kernel();
    let mut parts = corpus_parts(&k);
    parts.push(("sphere", sphere(&k, 12.0)));
    for (name, b) in parts {
        let m = build(&k, &b, &opts()).unwrap();
        let topo = k.topology(&b).unwrap();
        for l in &m.lods {
            for (f, r) in l.faces.iter().enumerate() {
                let info = &topo.faces[f];
                // Planes: exactly the face normal. Cylinders and spheres: the radial
                // direction, with one sign over the whole face (which sign is outward is
                // proven by the positive volume and the winding check below).
                let mut sign = 0.0;
                for v in r.first_vertex..r.first_vertex + r.vertex_count {
                    let x = l.vertex(v as usize);
                    let n = x.normal.map(|c| c as f64);
                    assert!(
                        (v3::len(n) - 1.0).abs() < 1e-5,
                        "{name}: |n| = {}",
                        v3::len(n)
                    );
                    let p = v3::add(x.position.map(|c| c as f64), m.origin);
                    let want = match info.surface {
                        Surface::Plane { .. } => Some(info.normal),
                        Surface::Cylinder { axis, .. } => {
                            let d = v3::sub(p, axis.origin);
                            v3::unit(v3::sub(d, v3::scale(axis.dir, v3::dot(d, axis.dir))))
                        }
                        Surface::Sphere { center, .. } => v3::unit(v3::sub(p, center)),
                        _ => None,
                    };
                    let Some(w) = want else { continue };
                    let d = v3::dot(n, w);
                    if sign == 0.0 {
                        sign = if matches!(info.surface, Surface::Plane { .. }) {
                            1.0
                        } else {
                            d.signum()
                        };
                    }
                    assert!(
                        v3::dist(n, v3::scale(w, sign)) < 2e-4,
                        "{name} face {f} ({}): normal {n:?}, want {w:?} × {sign}",
                        info.surface.name()
                    );
                }
                // Winding agrees with the normals: each triangle's geometric normal points
                // the same way as its vertex normals.
                for t in r.first_index / 3..(r.first_index + r.index_count) / 3 {
                    let tri = l.triangle(t as usize);
                    let [a, bb, c] = tri.map(|i| l.vertex(i as usize).position.map(|c| c as f64));
                    let g = v3::cross(v3::sub(bb, a), v3::sub(c, a));
                    if v3::len(g) < 1e-9 {
                        continue;
                    }
                    let avg = tri.iter().fold([0.0; 3], |s, &i| {
                        v3::add(s, l.vertex(i as usize).normal.map(|c| c as f64))
                    });
                    assert!(
                        v3::dot(g, avg) > 0.0,
                        "{name} face {f}: triangle {t} winds against its normals"
                    );
                }
            }
        }
    }
}

#[test]
fn levels_of_detail() {
    let k = kernel();
    let tol = *k.tolerances();
    // A box is exact at any tolerance: one level only.
    let cube = k
        .primitive(&Primitive::Block {
            min: [0.0; 3],
            size: [10.0, 20.0, 30.0],
        })
        .unwrap()
        .body;
    let m = build(&k, &cube, &opts()).unwrap();
    assert_eq!(m.lods.len(), 1);
    assert_eq!(m.lods[0].triangle_count(), 12);
    // A sphere: each level has at most 3/5 of the previous level's triangles, level 0 is
    // the Image Quality formula, and every triangle stays within its level's deflection.
    let r = 20.0;
    let s = sphere(&k, r);
    let m = build(&k, &s, &opts()).unwrap();
    let size = display_box(&k.topology(&s).unwrap()).diagonal();
    assert_eq!(m.lods[0].params, tol.display_mesh(3.0, size));
    assert!(m.lods.len() >= 3, "{} levels", m.lods.len());
    let counts: Vec<usize> = m.lods.iter().map(|l| l.triangle_count()).collect();
    for (i, l) in m.lods.iter().enumerate() {
        assert_eq!(l.params, tol.display_lod(3.0, size, i as u32));
        if i > 0 {
            assert!(counts[i] * 5 <= counts[i - 1] * 3, "{counts:?}");
        }
        let centre = v3::sub([5.0, -3.0, 2.0], m.origin);
        let mut worst: f64 = 0.0;
        for t in 0..l.triangle_count() {
            let c = l.triangle(t).iter().fold([0.0; 3], |s, &i| {
                v3::add(
                    s,
                    v3::scale(l.vertex(i as usize).position.map(|c| c as f64), 1.0 / 3.0),
                )
            });
            worst = worst.max(r - v3::dist(c, centre));
        }
        assert!(
            worst <= l.params.linear * 1.05 + 1e-5,
            "level {i}: deviation {worst} > {}",
            l.params.linear
        );
    }
    println!("sphere r {r}: triangles per level {counts:?}");
    // Selection: zoomed in → level 0; far away → the coarsest.
    assert_eq!(m.select_lod(1e-3, 0.5), 0);
    assert_eq!(m.select_lod(1e3, 0.5), m.lods.len() - 1);
    let mid = m.lods[1].params.linear / 0.5;
    assert_eq!(m.select_lod(mid, 0.5), 1);
    // max_lods = 1 gives the Image Quality mesh only; higher quality, more triangles.
    let one = build(
        &k,
        &s,
        &MeshOptions {
            max_lods: 1,
            ..opts()
        },
    )
    .unwrap();
    assert_eq!(one.lods, m.lods[..1]);
    let fine = build(
        &k,
        &s,
        &MeshOptions {
            quality: 6.0,
            max_lods: 1,
            ..opts()
        },
    )
    .unwrap();
    assert!(fine.lods[0].triangle_count() > 2 * counts[0]);
}

#[test]
fn points_bounds_and_flags() {
    let k = kernel();
    let parts = corpus_parts(&k);
    let (_, b) = &parts[0]; // block-hole: 8 box corners, 2 vertices on the hole's circles
    let m = build(&k, b, &opts()).unwrap();
    let topo = k.topology(b).unwrap();
    let hidden = m
        .point_flags
        .iter()
        .filter(|&&f| f & POINT_HIDDEN != 0)
        .count();
    assert_eq!((m.points.len(), hidden), (10, 2));
    for (p, q) in m.points.iter().zip(&topo.vertices) {
        assert!(v3::dist(v3::add(p.map(|c| c as f64), m.origin), *q) < 1e-5);
    }
    let bb = k.bounding_box(b).unwrap();
    // The display box contains the exact box; for this part (planes, a hole inside) it is
    // the exact box.
    let db = display_box(&topo);
    assert!(
        v3::dist(db.min, bb.min) < 1e-6 && v3::dist(db.max, bb.max) < 1e-6,
        "{db:?} {bb:?}"
    );
    for (name, b) in &parts {
        let (db, bb) = (
            display_box(&k.topology(b).unwrap()),
            k.bounding_box(b).unwrap(),
        );
        assert!(
            (0..3).all(|c| db.min[c] <= bb.min[c] + 1e-6 && db.max[c] >= bb.max[c] - 1e-6),
            "{name}"
        );
        assert!(
            db.diagonal() < 1.1 * bb.diagonal(),
            "{name}: {} vs {}",
            db.diagonal(),
            bb.diagonal()
        );
    }
    assert!(
        v3::dist(m.origin, [10.0, 5.0, 2.5]) < 1e-3,
        "{:?} {bb:?}",
        m.origin
    );
    for l in &m.lods {
        for i in 0..l.vertex_count as usize {
            let p = l.vertex(i).position;
            assert!((0..3).all(|c| p[c] >= m.bounds[0][c] && p[c] <= m.bounds[1][c]));
            assert!(v3::len(p.map(|c| c as f64)) <= m.radius as f64);
        }
    }
}

#[test]
fn large_vertex_counts_use_32_bit_indices() {
    let k = kernel();
    let s = sphere(&k, 20.0);
    let m = build(
        &k,
        &s,
        &MeshOptions {
            quality: 10.0,
            max_lods: 2,
            ..opts()
        },
    )
    .unwrap();
    assert!(
        m.lods[0].vertex_count as usize > layout::MAX_U16_VERTICES,
        "{}",
        m.lods[0].vertex_count
    );
    assert_eq!(m.lods[0].index_format, IndexFormat::U32);
    assert_eq!(m.lods.last().unwrap().index_format, IndexFormat::U16);
    assert_watertight(&m, 0, "fine sphere");
}
