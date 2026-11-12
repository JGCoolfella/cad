//! WebAssembly binding of [`oc_sketch`] for the TypeScript sketch editor.
//!
//! The TypeScript wrapper (`src/sketch/wasmSolver.ts`) gives it the same API as the
//! TypeScript `SketchSystem`. Sketches cross the boundary as JSON text (`JSON.stringify`
//! order is the map order the solver depends on), parameter vectors as `Float64Array`.
//! Build: `npm run build:sketch-wasm`, which writes `src/sketch/wasm/`.

use oc_sketch::{generators, Dimension, Pt, Rad, SketchData, SketchSystem};
use serde::Serialize;
use std::collections::HashSet;
use wasm_bindgen::prelude::*;

/// A sketch system (`SketchSystem` in TypeScript).
#[wasm_bindgen]
pub struct WasmSketchSystem {
    sys: SketchSystem,
}

/// `PRef` in TypeScript: parameter indices of a point (-1 if fixed) or its constant value.
#[derive(Serialize)]
struct PRef {
    ix: i64,
    iy: i64,
    cx: f64,
    cy: f64,
}

/// A radius parameter index (-1 if fixed) or its constant value.
#[derive(Serialize)]
struct RRef {
    i: i64,
    c: f64,
}

#[derive(Serialize)]
struct BenchOut<'a> {
    sketch: &'a SketchData,
    drag: DragOut<'a>,
}
#[derive(Serialize)]
struct DragOut<'a> {
    id: &'a str,
    to: [f64; 2],
}

fn err(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

#[wasm_bindgen]
impl WasmSketchSystem {
    /// `sketch`: `JSON.stringify(sketchData)`; `extra_fixed`: ids of points held fixed.
    #[wasm_bindgen(constructor)]
    pub fn new(sketch: &str, extra_fixed: Vec<String>) -> Result<WasmSketchSystem, JsError> {
        let sk = SketchData::from_json(sketch).map_err(err)?;
        let fixed: HashSet<String> = extra_fixed.into_iter().collect();
        Ok(Self {
            sys: SketchSystem::with_fixed(&sk, &fixed),
        })
    }

    /// Parameter values.
    pub fn x(&self) -> Vec<f64> {
        self.sys.x().to_vec()
    }

    #[wasm_bindgen(js_name = setX)]
    pub fn set_x(&mut self, x: &[f64]) -> Result<(), JsError> {
        if x.len() != self.sys.x().len() {
            return Err(err(format!(
                "expected {} parameters, got {}",
                self.sys.x().len(),
                x.len()
            )));
        }
        self.sys.set_x(x);
        Ok(())
    }

    /// `{ [pointId]: { ix, iy, cx, cy } }` as JSON, in point order.
    pub fn points(&self) -> String {
        let m: Vec<(&String, PRef)> = self
            .sys
            .points()
            .iter()
            .map(|(id, p)| {
                let r = match *p {
                    Pt::Var(i) => PRef {
                        ix: i as i64,
                        iy: i as i64 + 1,
                        cx: 0.0,
                        cy: 0.0,
                    },
                    Pt::Fixed([x, y]) => PRef {
                        ix: -1,
                        iy: -1,
                        cx: x,
                        cy: y,
                    },
                };
                (id, r)
            })
            .collect();
        to_object(&m)
    }

    /// `{ [circleId]: { i, c } }` as JSON.
    pub fn radii(&self) -> String {
        let m: Vec<(&String, RRef)> = self
            .sys
            .radii()
            .iter()
            .map(|(id, r)| {
                let r = match *r {
                    Rad::Var(i) => RRef {
                        i: i as i64,
                        c: 0.0,
                    },
                    Rad::Fixed(c) => RRef { i: -1, c },
                };
                (id, r)
            })
            .collect();
        to_object(&m)
    }

    pub fn residuals(&self) -> Vec<f64> {
        self.sys.residuals()
    }

    /// Dense Jacobian, row-major (`equations × parameters`).
    pub fn jacobian(&self) -> Vec<f64> {
        self.sys.jacobian().concat()
    }

    pub fn solve(&mut self, max_iter: u32, tol: f64) -> bool {
        self.sys.solve(max_iter as usize, tol)
    }

    /// `ids[k]` is dragged to `(to[2k], to[2k + 1])`.
    pub fn drag(&mut self, ids: Vec<String>, to: &[f64], steps: u32) -> bool {
        let targets: Vec<(&str, [f64; 2])> = ids
            .iter()
            .enumerate()
            .map(|(k, id)| (id.as_str(), [to[2 * k], to[2 * k + 1]]))
            .collect();
        self.sys.drag(&targets, steps as usize)
    }

    /// The analysis as JSON (`bad` as an array).
    pub fn analyse(&self, converged: bool) -> String {
        serde_json::to_string(&self.sys.analyse(converged)).expect("serialisable")
    }

    /// Current value a dimension (`JSON.stringify(dim)`) measures, `undefined` if none.
    pub fn measure(&self, dim: &str) -> Result<Option<f64>, JsError> {
        let d: Dimension = serde_json::from_str(dim).map_err(err)?;
        Ok(self.sys.measure(&d))
    }
}

fn to_object<T: Serialize>(entries: &[(&String, T)]) -> String {
    let mut out = String::from("{");
    for (k, (id, v)) in entries.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        out.push_str(&serde_json::to_string(id).expect("string"));
        out.push(':');
        out.push_str(&serde_json::to_string(v).expect("serialisable"));
    }
    out.push('}');
    out
}

/// A benchmark sketch (`grid`, `grid-linked`, `linkage`, `polygon`, `tangent-chain`) as
/// JSON `{ sketch, drag: { id, to } }`, or `undefined` for an unknown kind.
#[wasm_bindgen(js_name = benchSketch)]
pub fn bench_sketch(kind: &str, n: u32) -> Option<String> {
    let b = generators::bench(kind, n as usize)?;
    let out = BenchOut {
        sketch: &b.sketch,
        drag: DragOut {
            id: &b.drag.0,
            to: b.drag.1,
        },
    };
    Some(serde_json::to_string(&out).expect("serialisable"))
}
