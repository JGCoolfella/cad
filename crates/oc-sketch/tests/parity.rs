//! Native parity with the TypeScript solver on the cases of `src/sketch/wasmSolver.test.ts`
//! (the `solver.test.ts` and `refs.test.ts` cases, the Slot and Polygon tools, every relation
//! and dimension kind, every sketch of the golden corpus, the benchmark sketches at N = 10).
//!
//! `fixtures/ts-parity.json` holds, per case, the input and two TypeScript results:
//! - `tsExact`: the TypeScript iteration with the exact Jacobian. Parameters and measures
//!   must match within 1e-9.
//! - `ts`: the shipped TypeScript solver (central differences, ~1e-9 relative Jacobian
//!   error). Parameters within 1e-6.
//!
//! Return values and the analysis (DOF, status, black/blue/red state, redundant relations in
//! order) must be identical to both. Re-record with `UPDATE_GOLDEN=1 npx vitest run
//! wasmSolver`.

use oc_sketch::{SketchData, SketchSystem, DRAG_STEPS};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    sketch: SketchData,
    #[serde(default)]
    extra_fixed: Vec<String>,
    drag: Option<Drag>,
    ts_exact: Expect,
    ts: Expect,
}

#[derive(Deserialize)]
struct Drag {
    id: String,
    to: [f64; 2],
}

#[derive(Deserialize)]
struct Expect {
    solved: bool,
    x1: Vec<f64>,
    dragged: Option<bool>,
    x2: Vec<f64>,
    analysis: serde_json::Value,
    measures: Vec<Option<f64>>,
}

struct Got {
    solved: bool,
    x1: Vec<f64>,
    dragged: Option<bool>,
    x2: Vec<f64>,
    analysis: serde_json::Value,
    measures: Vec<Option<f64>>,
}

fn run(c: &Case) -> Got {
    let fixed: HashSet<String> = c.extra_fixed.iter().cloned().collect();
    let mut sk = c.sketch.clone();
    let mut sys = SketchSystem::with_fixed(&sk, &fixed);
    let solved = sys.solve_default();
    let x1 = sys.x().to_vec();
    let dragged = c
        .drag
        .as_ref()
        .map(|d| sys.drag(&[(d.id.as_str(), d.to)], DRAG_STEPS));
    let x2 = sys.x().to_vec();
    sys.commit(&mut sk);
    let analysis = serde_json::to_value(sys.analyse(solved)).expect("serialisable");
    let measures = sk.dimensions.values().map(|d| sys.measure(d)).collect();
    Got {
        solved,
        x1,
        dragged,
        x2,
        analysis,
        measures,
    }
}

fn max_diff(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len(), "parameter count");
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

/// Returns the largest parameter difference.
fn check(name: &str, got: &Got, want: &Expect, tol: f64) -> f64 {
    assert_eq!(got.solved, want.solved, "{name}: solve result");
    assert_eq!(got.dragged, want.dragged, "{name}: drag result");
    let d = max_diff(&got.x1, &want.x1).max(max_diff(&got.x2, &want.x2));
    assert!(
        d <= tol,
        "{name}: parameters differ by {d:e} (tolerance {tol:e})"
    );
    assert_eq!(got.analysis, want.analysis, "{name}: analysis");
    assert_eq!(got.measures.len(), want.measures.len(), "{name}: measures");
    for (k, (g, w)) in got.measures.iter().zip(&want.measures).enumerate() {
        match (g, w) {
            (Some(g), Some(w)) => assert!((g - w).abs() <= tol, "{name}: measure {k}: {g} vs {w}"),
            _ => assert_eq!(g, w, "{name}: measure {k}"),
        }
    }
    d
}

#[test]
fn matches_the_typescript_solver() {
    let text = include_str!("fixtures/ts-parity.json");
    let cases: Vec<Case> = serde_json::from_str(text).expect("fixture parses");
    assert!(cases.len() > 30, "fixture has {} cases", cases.len());
    let (mut worst_exact, mut worst_fd) = (0.0f64, 0.0f64);
    for c in &cases {
        let got = run(c);
        worst_exact = worst_exact.max(check(&c.name, &got, &c.ts_exact, 1e-9));
        worst_fd = worst_fd.max(check(&format!("{} (shipped)", c.name), &got, &c.ts, 1e-6));
    }
    println!(
        "{} cases; largest difference: {worst_exact:e} (exact Jacobian), {worst_fd:e} (shipped)",
        cases.len()
    );
}
