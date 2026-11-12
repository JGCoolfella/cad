//! Parametric benchmark sketches. Each starts satisfied (or nearly) and names a point to
//! drag, as a mouse move would. Used by `benches/solver.rs`, by the tests, and through the
//! WASM binding by the TypeScript parity test.

use crate::model::{Dimension, DimensionKind, Relation, SketchData, Vec2, ORIGIN_ID};
use std::f64::consts::PI;

/// A benchmark sketch and the drag applied to it.
#[derive(Clone, Debug)]
pub struct Bench {
    pub sketch: SketchData,
    /// Point dragged and its target.
    pub drag: (String, Vec2),
}

/// Benchmark kinds, by name.
pub const KINDS: [&str; 5] = ["grid", "grid-linked", "linkage", "polygon", "tangent-chain"];

/// The named benchmark at size `n`.
pub fn bench(kind: &str, n: usize) -> Option<Bench> {
    Some(match kind {
        "grid" => grid(n, false),
        "grid-linked" => grid(n, true),
        "linkage" => linkage(n),
        "polygon" => polygon(n),
        "tangent-chain" => tangent_chain(n),
        _ => return None,
    })
}

fn rel(sk: &mut SketchData, r: Relation) {
    sk.add_relation(r);
}

fn dim(sk: &mut SketchData, id: &str, kind: DimensionKind, refs: &[&str], value: f64) {
    sk.add_dimension(Dimension::new(id, kind, refs, value));
}

fn moved(sk: &SketchData, id: &str, by: Vec2) -> (String, Vec2) {
    let p = &sk.points[id];
    (id.to_string(), [p.x + by[0], p.y + by[1]])
}

/// `n` rectangles (10 × 6) on a grid, each with horizontal/vertical relations and width and
/// height dimensions, free to move (2 DOF each). `linked`: each rectangle's first corner is
/// also aligned with its left neighbour's (horizontal points) or, first in a row, with the
/// row above (vertical points), making one component of `8n` parameters.
pub fn grid(n: usize, linked: bool) -> Bench {
    let mut sk = SketchData::empty();
    let cols = (n as f64).sqrt().ceil().max(1.0) as usize;
    for k in 0..n {
        let (x0, y0) = ((k % cols) as f64 * 15.0, (k / cols) as f64 * 10.0);
        let p = |c: &str| format!("r{k}{c}");
        for (c, x, y) in [
            ("a", x0, y0),
            ("b", x0 + 10.0, y0),
            ("c", x0 + 10.0, y0 + 6.0),
            ("d", x0, y0 + 6.0),
        ] {
            sk.add_point(&p(c), x, y);
        }
        for (l, a, b) in [
            ("l1", "a", "b"),
            ("l2", "b", "c"),
            ("l3", "c", "d"),
            ("l4", "d", "a"),
        ] {
            sk.add_line(&p(l), &p(a), &p(b));
        }
        rel(
            &mut sk,
            Relation::Horizontal {
                id: p("h1"),
                e: p("l1"),
            },
        );
        rel(
            &mut sk,
            Relation::Horizontal {
                id: p("h2"),
                e: p("l3"),
            },
        );
        rel(
            &mut sk,
            Relation::Vertical {
                id: p("v1"),
                e: p("l2"),
            },
        );
        rel(
            &mut sk,
            Relation::Vertical {
                id: p("v2"),
                e: p("l4"),
            },
        );
        dim(
            &mut sk,
            &p("w"),
            DimensionKind::Distance,
            &[&p("a"), &p("b")],
            10.0,
        );
        dim(
            &mut sk,
            &p("h"),
            DimensionKind::Distance,
            &[&p("b"), &p("c")],
            6.0,
        );
        if linked && k > 0 {
            if k % cols == 0 {
                let above = format!("r{}a", k - cols);
                rel(
                    &mut sk,
                    Relation::VerticalPoints {
                        id: p("link"),
                        p1: above,
                        p2: p("a"),
                    },
                );
            } else {
                let left = format!("r{}a", k - 1);
                rel(
                    &mut sk,
                    Relation::HorizontalPoints {
                        id: p("link"),
                        p1: left,
                        p2: p("a"),
                    },
                );
            }
        }
    }
    let drag = moved(&sk, &format!("r{}c", n - 1), [0.5, 0.3]);
    Bench { sketch: sk, drag }
}

/// A closed linkage of `n` bars of fixed length (a regular polygon), joint 0 at the origin
/// and bar 0 horizontal: `n - 3` DOF.
pub fn linkage(n: usize) -> Bench {
    let mut sk = SketchData::empty();
    let r = 20.0;
    let t0 = -PI / 2.0 - PI / n as f64;
    let c = [-r * t0.cos(), -r * t0.sin()];
    let joint = |k: usize| format!("j{k}");
    for k in 0..n {
        let t = t0 + 2.0 * PI * k as f64 / n as f64;
        sk.add_point(&joint(k), c[0] + r * t.cos(), c[1] + r * t.sin());
    }
    for k in 0..n {
        let (a, b) = (joint(k), joint((k + 1) % n));
        sk.add_line(&format!("bar{k}"), &a, &b);
        let len = {
            let (pa, pb) = (&sk.points[&a], &sk.points[&b]);
            (pa.x - pb.x).hypot(pa.y - pb.y)
        };
        dim(
            &mut sk,
            &format!("len{k}"),
            DimensionKind::Distance,
            &[&a, &b],
            len,
        );
    }
    rel(
        &mut sk,
        Relation::Coincident {
            id: "ground".into(),
            p: joint(0),
            e: ORIGIN_ID.into(),
        },
    );
    rel(
        &mut sk,
        Relation::Horizontal {
            id: "base".into(),
            e: "bar0".into(),
        },
    );
    let drag = moved(&sk, &joint(n / 2), [0.5, 0.25]);
    Bench { sketch: sk, drag }
}

/// A regular `n`-gon with the relations the Polygon tool adds (inscribed): all sides equal to
/// side 0, all vertices equidistant from the centre, side 0 tangent to the construction
/// circle. 4 DOF: position, size, rotation.
pub fn polygon(n: usize) -> Bench {
    let mut sk = SketchData::empty();
    let (cx, cy, big_r) = (5.0, 5.0, 20.0);
    sk.add_point("c", cx, cy);
    for k in 0..n {
        let t = 2.0 * PI * k as f64 / n as f64;
        sk.add_point(&format!("v{k}"), cx + big_r * t.cos(), cy + big_r * t.sin());
    }
    for k in 0..n {
        sk.add_line(
            &format!("s{k}"),
            &format!("v{k}"),
            &format!("v{}", (k + 1) % n),
        );
    }
    for k in 1..n {
        rel(
            &mut sk,
            Relation::Equal {
                id: format!("eq{k}"),
                e1: "s0".into(),
                e2: format!("s{k}"),
            },
        );
    }
    sk.add_circle("circ", "c", big_r * (PI / n as f64).cos());
    for k in 1..n {
        rel(
            &mut sk,
            Relation::Equidistant {
                id: format!("ed{k}"),
                c: "c".into(),
                p1: "v0".into(),
                p2: format!("v{k}"),
            },
        );
    }
    rel(
        &mut sk,
        Relation::Tangent {
            id: "tan".into(),
            e1: "s0".into(),
            e2: "circ".into(),
            inner: None,
        },
    );
    let drag = moved(&sk, "v0", [0.5, 0.25]);
    Bench { sketch: sk, drag }
}

/// `n` semicircular arcs in an S-chain along the x axis, each tangent to the next at their
/// shared end point, all radii equal to arc 0's (dimensioned), start at the origin:
/// `n + 1` DOF.
pub fn tangent_chain(n: usize) -> Bench {
    let mut sk = SketchData::empty();
    let len = 10.0;
    let s = |k: usize| format!("s{k}");
    for k in 0..=n {
        sk.add_point(&s(k), k as f64 * len, 0.0);
    }
    for k in 0..n {
        let c = format!("c{k}");
        sk.add_point(&c, (k as f64 + 0.5) * len, 0.0);
        // Arcs run counter-clockwise from p1 to p2: lower bumps left to right, upper bumps
        // right to left.
        let (p1, p2) = if k % 2 == 0 {
            (s(k), s(k + 1))
        } else {
            (s(k + 1), s(k))
        };
        sk.add_arc(&format!("a{k}"), &c, &p1, &p2);
    }
    rel(
        &mut sk,
        Relation::Coincident {
            id: "start".into(),
            p: s(0),
            e: ORIGIN_ID.into(),
        },
    );
    dim(&mut sk, "r0", DimensionKind::Radius, &["a0"], len / 2.0);
    for k in 1..n {
        rel(
            &mut sk,
            Relation::Tangent {
                id: format!("t{k}"),
                e1: format!("a{}", k - 1),
                e2: format!("a{k}"),
                inner: None,
            },
        );
        rel(
            &mut sk,
            Relation::Equal {
                id: format!("e{k}"),
                e1: "a0".into(),
                e2: format!("a{k}"),
            },
        );
    }
    let drag = moved(&sk, &s(n), [0.5, 0.25]);
    Bench { sketch: sk, drag }
}
