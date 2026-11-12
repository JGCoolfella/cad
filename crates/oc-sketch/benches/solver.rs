//! Solver benchmarks: `cargo bench -p oc-sketch` (optionally `-- <kind>` to filter).
//!
//! For each benchmark sketch at N = 10, 50, 200 it reports, as medians:
//! - `drag`: one `drag()` call (12 sub-steps) on a solved system, as one mouse move. This is
//!   the E2 pass criterion for `grid` at N = 200 (< 2 ms);
//! - `build`: constructing the system (parameters, equations, components);
//! - `analyse`: rank analysis (DOF, black/blue, redundancy);
//! - `cycle`: what the sketch editor does per mouse move today: build, solve, drag, commit,
//!   analyse.

use oc_sketch::generators::{bench, KINDS};
use oc_sketch::{SketchSystem, DRAG_STEPS};
use std::hint::black_box;
use std::time::{Duration, Instant};

fn median(mut f: impl FnMut() -> Duration, budget: Duration) -> Duration {
    let mut v = Vec::new();
    let start = Instant::now();
    while v.len() < 5 || (start.elapsed() < budget && v.len() < 2000) {
        v.push(f());
    }
    v.sort_unstable();
    v[v.len() / 2]
}

fn fmt(d: Duration) -> String {
    let us = d.as_secs_f64() * 1e6;
    if us < 1000.0 {
        format!("{us:.1} µs")
    } else {
        format!("{:.2} ms", us / 1000.0)
    }
}

fn main() {
    let filter: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let budget = Duration::from_millis(400);
    println!("| sketch | N | params | eqs | largest component | drag | build | analyse | cycle |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for kind in KINDS {
        if !filter.is_empty() && !filter.iter().any(|f| kind.contains(f.as_str())) {
            continue;
        }
        for n in [10, 50, 200] {
            let b = bench(kind, n).expect("kind");
            let (id, to) = (b.drag.0.as_str(), b.drag.1);
            let mut solved = SketchSystem::new(&b.sketch);
            assert!(solved.solve_default(), "{kind} {n} solves");
            // Warm the symbolic factorisations, as the first drag of an editing session does.
            let mut warm = solved.clone();
            warm.drag(&[(id, to)], DRAG_STEPS);
            for (i, c) in warm.clone().x().iter().enumerate() {
                let _ = (i, c);
            }
            solved = {
                let mut s = warm.clone();
                s.set_x(solved.x());
                s.solve_default();
                s
            };
            let drag = median(
                || {
                    let mut s = solved.clone();
                    let t = Instant::now();
                    black_box(s.drag(&[(id, to)], DRAG_STEPS));
                    t.elapsed()
                },
                budget,
            );
            let build = median(
                || {
                    let t = Instant::now();
                    black_box(SketchSystem::new(&b.sketch));
                    t.elapsed()
                },
                budget,
            );
            let analyse = median(
                || {
                    let t = Instant::now();
                    black_box(solved.analyse(true));
                    t.elapsed()
                },
                budget,
            );
            let cycle = median(
                || {
                    let mut sk = b.sketch.clone();
                    let t = Instant::now();
                    let mut s = SketchSystem::new(&sk);
                    s.solve_default();
                    s.drag(&[(id, to)], DRAG_STEPS);
                    s.commit(&mut sk);
                    black_box(s.analyse(true));
                    t.elapsed()
                },
                budget,
            );
            let (p, e) = solved.component_sizes().first().copied().unwrap_or((0, 0));
            println!(
                "| {kind} | {n} | {} | {} | {p} × {e} | {} | {} | {} | {} |",
                solved.x().len(),
                solved.equation_count(),
                fmt(drag),
                fmt(build),
                fmt(analyse),
                fmt(cycle)
            );
        }
    }
}
