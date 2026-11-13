//! Test bodies (the golden corpus' final parts, rebuilt with kernel calls as
//! `oc-kernel-occt/tests/corpus.rs` does and checked against `corpus/*.json`, plus a
//! large part) and mesh checks shared by the tests.
#![allow(dead_code)]

use oc_kernel::*;
use oc_kernel_occt::{Body, OcctKernel};
use oc_mesh::RenderMesh;
use std::collections::HashMap;
use std::f64::consts::PI;

pub type K = OcctKernel;
const Z: P3 = [0.0, 0.0, 1.0];

pub fn kernel() -> K {
    OcctKernel::default()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}
fn rect(x0: f64, y0: f64, x1: f64, y1: f64, z: f64) -> Wire {
    Wire::rect_xy(x0, y0, x1, y1, z)
}
fn circle(cx: f64, cy: f64, r: f64, z: f64) -> Wire {
    Wire::new(vec![Segment::Circle {
        center: [cx, cy, z],
        normal: Z,
        radius: r,
    }])
}
fn prism(k: &K, w: Wire, v: P3) -> Body {
    k.prism(&PrismSpec {
        profile: Profile::single(w),
        vector: v,
        taper: None,
    })
    .unwrap()
    .body
}
fn block(k: &K, x: f64, y: f64, z: f64) -> Body {
    prism(k, rect(0.0, 0.0, x, y, 0.0), [0.0, 0.0, z])
}
fn fuse(k: &K, a: &Body, b: &Body) -> Body {
    k.boolean(BooleanOp::Fuse, a, &[b]).unwrap().body
}
fn cut(k: &K, a: &Body, b: &Body) -> Body {
    k.boolean(BooleanOp::Cut, a, &[b]).unwrap().body
}
fn edges_where(t: &Topology, line_only: bool, pred: impl Fn(P3) -> bool) -> Vec<EdgeId> {
    let ok = |e: &EdgeInfo| (!line_only || e.curve == Curve::Line) && pred(e.mid);
    t.edges
        .iter()
        .enumerate()
        .filter(|(_, e)| ok(e))
        .map(|(i, _)| EdgeId(i as u32))
        .collect()
}
fn deg(d: f64) -> f64 {
    d * PI / 180.0
}
fn fillets(edges: Vec<EdgeId>, r: f64) -> Vec<FilletEdge> {
    edges
        .into_iter()
        .map(|edge| FilletEdge {
            edge,
            radius: Radius::Constant(r),
        })
        .collect()
}

/// Face and edge counts of corpus step `part/step` (the step `oc-kernel-occt`'s corpus
/// replay reaches).
fn assert_corpus(k: &K, b: &Body, part: &str, step: &str) {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../corpus/{part}.json"));
    let j: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let last = &j["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == step)
        .unwrap()["body"];
    let t = k.topology(b).unwrap();
    assert_eq!(
        (t.faces.len() as u64, t.edges.len() as u64),
        (
            last["faces"].as_u64().unwrap(),
            last["edges"].as_u64().unwrap()
        ),
        "{part}/{step}"
    );
}

fn pad_pocket(k: &K) -> Body {
    let b = cut(
        k,
        &block(k, 60.0, 40.0, 10.0),
        &prism(k, rect(15.0, 10.0, 45.0, 30.0, 10.0), [0.0, 0.0, -4.0]),
    );
    let floor = edges_where(&k.topology(&b).unwrap(), true, |m| near(m[2], 6.0));
    k.fillet(&b, &fillets(floor, 1.5)).unwrap().body
}

fn shell(k: &K) -> Body {
    let b = block(k, 40.0, 30.0, 20.0);
    let e = edges_where(&k.topology(&b).unwrap(), true, |m| near(m[2], 10.0));
    let b = k.fillet(&b, &fillets(e, 4.0)).unwrap().body;
    let t = k.topology(&b).unwrap();
    let top = t.faces.iter().position(|f| {
        matches!(f.surface, Surface::Plane { .. })
            && near(f.normal[2], 1.0)
            && near(f.centroid[2], 20.0)
    });
    k.shell(
        &b,
        &ShellSpec {
            open: vec![FaceId(top.unwrap() as u32)],
            thickness: 2.0,
            outward: false,
        },
    )
    .unwrap()
    .body
}

fn revolve(k: &K) -> Body {
    let rev = |w: Wire, angle: f64, dir: f64| {
        let axis = Axis {
            origin: [0.0; 3],
            dir: [0.0, dir, 0.0],
        };
        k.revolve(&RevolveSpec {
            profile: Profile::single(w),
            axis,
            angle,
        })
        .unwrap()
        .body
    };
    let b = rev(rect(10.0, 0.0, 20.0, 50.0, 0.0), 2.0 * PI, 1.0);
    let back = Transform::rotation(
        Axis {
            origin: [0.0; 3],
            dir: [0.0, 1.0, 0.0],
        },
        -PI / 4.0,
    );
    let notch: Vec<P3> = [
        [15.0, 40.0, 0.0],
        [25.0, 40.0, 0.0],
        [25.0, 50.0, 0.0],
        [15.0, 50.0, 0.0],
    ]
    .map(|p| back.apply(p))
    .to_vec();
    let b = cut(k, &b, &rev(Wire::polygon(&notch), PI / 2.0, 1.0));
    let w = rect(20.0, 0.0, 30.0, 10.0, 0.0);
    fuse(
        k,
        &b,
        &fuse(k, &rev(w.clone(), deg(60.0), 1.0), &rev(w, deg(30.0), -1.0)),
    )
}

fn sweep_loft(k: &K) -> Body {
    let c = 10.0 - 10.0 * (PI / 4.0).cos();
    let path = Wire::new(vec![
        Segment::Line {
            a: [0.0, 0.0, 0.0],
            b: [0.0, 30.0, 0.0],
        },
        Segment::Arc {
            a: [0.0, 30.0, 0.0],
            mid: [c, 30.0 + 10.0 * (PI / 4.0).sin(), 0.0],
            b: [10.0, 40.0, 0.0],
        },
        Segment::Line {
            a: [10.0, 40.0, 0.0],
            b: [40.0, 40.0, 0.0],
        },
    ]);
    let profile = Wire::new(vec![Segment::Circle {
        center: [0.0; 3],
        normal: [0.0, 1.0, 0.0],
        radius: 3.0,
    }]);
    let s1 = k
        .sweep(&SweepSpec {
            profile: Profile::single(profile),
            path,
            orientation: SweepOrientation::FollowPath,
        })
        .unwrap()
        .body;
    let sq = Wire::polygon(&[
        [57.0, 0.0, 3.0],
        [63.0, 0.0, 3.0],
        [63.0, 0.0, -3.0],
        [57.0, 0.0, -3.0],
    ]);
    let path2 = Wire::new(vec![Segment::Line {
        a: [60.0, 0.0, 0.0],
        b: [60.0, 40.0, 0.0],
    }]);
    let s2 = k
        .sweep(&SweepSpec {
            profile: Profile::single(sq),
            path: path2,
            orientation: SweepOrientation::FollowPath,
        })
        .unwrap()
        .body;
    let l = k
        .loft(&LoftSpec {
            sections: vec![
                rect(100.0, 0.0, 120.0, 20.0, 0.0),
                circle(110.0, 10.0, 6.0, 30.0),
            ],
            ruled: false,
        })
        .unwrap()
        .body;
    fuse(k, &fuse(k, &s1, &s2), &l)
}

fn curved_profiles(k: &K) -> Body {
    let p = |x: f64, y: f64| [x, y, 0.0];
    let slot = Wire::new(vec![
        Segment::Line {
            a: p(10.0, 5.0),
            b: p(40.0, 5.0),
        },
        Segment::Arc {
            a: p(40.0, 5.0),
            mid: p(45.0, 10.0),
            b: p(40.0, 15.0),
        },
        Segment::Line {
            a: p(40.0, 15.0),
            b: p(10.0, 15.0),
        },
        Segment::Arc {
            a: p(10.0, 15.0),
            mid: p(5.0, 10.0),
            b: p(10.0, 5.0),
        },
    ]);
    let ellipse = Segment::Ellipse {
        center: p(70.0, 10.0),
        normal: Z,
        major_dir: [1.0, 0.0, 0.0],
        major: 12.0,
        minor: 6.0,
    };
    fuse(
        k,
        &prism(k, slot, [0.0, 0.0, 6.0]),
        &prism(k, Wire::new(vec![ellipse]), [0.0, 0.0, 4.0]),
    )
}

fn patterns(k: &K) -> Body {
    let boss = prism(k, circle(10.0, 10.0, 4.0, 5.0), [0.0, 0.0, 6.0]);
    let pocket = prism(k, rect(20.0, 45.0, 24.0, 55.0, 5.0), [0.0, 0.0, -3.0]);
    let b = cut(k, &fuse(k, &block(k, 100.0, 60.0, 5.0), &boss), &pocket);
    let repeat = |tool: &Body, xfs: Vec<Transform>| {
        let copies: Vec<_> = xfs
            .iter()
            .map(|t| k.transform(tool, t).unwrap().body)
            .collect();
        copies[1..]
            .iter()
            .fold(copies[0].clone(), |acc, c| fuse(k, &acc, c))
    };
    let xfs = (0..4)
        .flat_map(|i| (0..2).map(move |j| (i, j)))
        .filter(|&(i, j)| i + j > 0);
    let b = fuse(
        k,
        &b,
        &repeat(
            &boss,
            xfs.map(|(i, j)| Transform::translation([15.0 * i as f64, 20.0 * j as f64, 0.0]))
                .collect(),
        ),
    );
    let b = cut(
        k,
        &b,
        &repeat(
            &pocket,
            (1..3)
                .map(|i| Transform::translation([10.0 * i as f64, 0.0, 0.0]))
                .collect(),
        ),
    );
    let pin = prism(k, circle(70.0, 30.0, 3.0, 5.0), [0.0, 0.0, 4.0]);
    let b = fuse(k, &b, &pin);
    let axis = Axis {
        origin: [50.0, 30.0, 0.0],
        dir: Z,
    };
    let b = fuse(
        k,
        &b,
        &repeat(
            &pin,
            (1..6)
                .map(|i| Transform::rotation(axis, deg(60.0) * i as f64))
                .collect(),
        ),
    );
    let b = fuse(
        k,
        &b,
        &repeat(
            &boss,
            vec![Transform::mirror(Plane {
                origin: [50.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            })],
        ),
    );
    let whole = k
        .transform(
            &b,
            &Transform::mirror(Plane {
                origin: [0.0; 3],
                normal: Z,
            }),
        )
        .unwrap()
        .body;
    fuse(k, &b, &whole)
}

fn imported(k: &K) -> Body {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/assets/block.step");
    let b = k.read_step(&std::fs::read(path).unwrap()).unwrap().body;
    fuse(
        k,
        &b,
        &prism(k, circle(15.0, 8.0, 4.0, 10.0), [0.0, 0.0, 5.0]),
    )
}

/// Final bodies of corpus parts, checked against the corpus' face and edge counts.
pub fn corpus_parts(k: &K) -> Vec<(&'static str, Body)> {
    const STEPS: [&str; 8] = [
        "Cut-Extrude1",
        "Fillet1",
        "Shell1",
        "Revolve2",
        "Loft1",
        "Boss-Extrude2",
        "Mirror2",
        "Boss-Extrude1",
    ];
    let parts: Vec<(&'static str, Body)> = vec![
        (
            "block-hole",
            cut(
                k,
                &block(k, 20.0, 10.0, 5.0),
                &prism(k, circle(10.0, 5.0, 3.0, -50.0), [0.0, 0.0, 100.0]),
            ),
        ),
        ("pad-pocket", pad_pocket(k)),
        ("shell", shell(k)),
        ("revolve", revolve(k)),
        ("sweep-loft", sweep_loft(k)),
        ("curved-profiles", curved_profiles(k)),
        ("patterns", patterns(k)),
        ("imported", imported(k)),
    ];
    for ((name, b), step) in parts.iter().zip(STEPS) {
        assert_corpus(k, b, name, step);
    }
    parts
}

/// A sphere: the analytic case for normals and chordal deviation.
pub fn sphere(k: &K, r: f64) -> Body {
    k.primitive(&Primitive::Sphere {
        center: [5.0, -3.0, 2.0],
        radius: r,
    })
    .unwrap()
    .body
}

/// A 400 × 300 × 10 plate with an `n` × `m` grid of Ø8 holes (one planar face with
/// `n·m` holes on each side).
pub fn holed_plate(k: &K, n: usize, m: usize) -> Body {
    let plate = block(k, 400.0, 300.0, 10.0);
    let holes: Vec<Body> = (0..n)
        .flat_map(|i| (0..m).map(move |j| (i, j)))
        .map(|(i, j)| {
            prism(
                k,
                circle(
                    20.0 + 360.0 * i as f64 / (n - 1) as f64,
                    20.0 + 260.0 * j as f64 / (m - 1) as f64,
                    4.0,
                    -1.0,
                ),
                [0.0, 0.0, 12.0],
            )
        })
        .collect();
    k.boolean(BooleanOp::Cut, &plate, &holes.iter().collect::<Vec<_>>())
        .unwrap()
        .body
}

/// A large part: a 400 × 300 × 10 plate with an `n` × `m` grid of Ø8 holes whose top rims
/// are filleted (r 1), and a revolved boss in the middle.
pub fn large_part(k: &K, n: usize, m: usize) -> Body {
    let b = holed_plate(k, n, m);
    let rims = edges_where(&k.topology(&b).unwrap(), false, |p| {
        near(p[2], 10.0) && p[0] > 1.0 && p[0] < 399.0 && p[1] > 1.0 && p[1] < 299.0
    });
    let b = k.fillet(&b, &fillets(rims, 1.0)).unwrap().body;
    let axis = Axis {
        origin: [200.0, 150.0, 0.0],
        dir: Z,
    };
    let dome = k
        .revolve(&RevolveSpec {
            profile: Profile::single(Wire::new(vec![
                Segment::Line {
                    a: [200.0, 150.0, 10.0],
                    b: [230.0, 150.0, 10.0],
                },
                Segment::Arc {
                    a: [230.0, 150.0, 10.0],
                    mid: [221.21, 150.0, 31.21],
                    b: [200.0, 150.0, 40.0],
                },
                Segment::Line {
                    a: [200.0, 150.0, 40.0],
                    b: [200.0, 150.0, 10.0],
                },
            ])),
            axis,
            angle: 2.0 * PI,
        })
        .unwrap()
        .body;
    fuse(k, &b, &dome)
}

// ---------------------------------------------------------------- mesh checks

fn bits(p: [f32; 3]) -> [u32; 3] {
    p.map(|c| (c + 0.0).to_bits())
}

/// Level `level`, with vertices welded by exact position, is a closed, consistently
/// oriented 2-manifold: every edge is used once in each direction (triangles that weld
/// to a point or a line, at poles and seams, are skipped). Returns the welded index of
/// every vertex.
pub fn assert_watertight(m: &RenderMesh, level: usize, what: &str) -> Vec<u32> {
    let l = &m.lods[level];
    let mut ids: HashMap<[u32; 3], u32> = HashMap::new();
    let weld: Vec<u32> = (0..l.vertex_count as usize)
        .map(|i| {
            let n = ids.len() as u32;
            *ids.entry(bits(l.vertex(i).position)).or_insert(n)
        })
        .collect();
    let mut half: HashMap<(u32, u32), u32> = HashMap::new();
    for t in 0..l.triangle_count() {
        let w = l.triangle(t).map(|i| weld[i as usize]);
        if w[0] == w[1] || w[1] == w[2] || w[0] == w[2] {
            continue;
        }
        for k in 0..3 {
            *half.entry((w[k], w[(k + 1) % 3])).or_default() += 1;
        }
    }
    let bad: Vec<_> = half
        .iter()
        .filter(|(&(a, b), &c)| c != 1 || half.get(&(b, a)) != Some(&1))
        .take(5)
        .collect();
    assert!(
        bad.is_empty(),
        "{what} level {level}: open or non-manifold edges {bad:?} ({} edges)",
        half.len()
    );
    weld
}

/// Signed volume and area of a level (f64 sums over the f32 positions).
pub fn volume_area(m: &RenderMesh, level: usize) -> (f64, f64) {
    let l = &m.lods[level];
    let (mut v, mut a) = (0.0, 0.0);
    for t in 0..l.triangle_count() {
        let [p, q, r] = l
            .triangle(t)
            .map(|i| l.vertex(i as usize).position.map(|c| c as f64));
        v += v3::dot(p, v3::cross(q, r)) / 6.0;
        a += 0.5 * v3::len(v3::cross(v3::sub(q, p), v3::sub(r, p)));
    }
    (v, a)
}
