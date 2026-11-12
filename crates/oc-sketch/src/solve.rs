//! Solving and dragging (`solve`, `drag` in TypeScript), per component.
//!
//! **Solve:** damped minimum-norm Gauss-Newton, `Δ = -Jᵀ (J Jᵀ + μI)⁻¹ r`, with a halving
//! line search on `|r|²`. Minimum norm means under-defined geometry moves as little as
//! possible, so adding a dimension never sends unrelated geometry flying.
//!
//! **Drag:** the drag vector is projected onto the null space of the Jacobian
//! (`d - Jᵀ (J Jᵀ + μI)⁻¹ J d`), applied, and the system re-solved, in several sub-steps.
//!
//! Per-component differences from the TypeScript solver, which runs one global iteration:
//! the damping `μ` and line search step are chosen per component, and a component stops
//! iterating when its own residual is below tolerance. For a sketch with one component the
//! iteration is the same.

use crate::sparse::min_norm_step;
use crate::system::{norm2, SketchSystem};
use crate::Vec2;

/// Default iteration limit and tolerance of [`SketchSystem::solve`] (as TypeScript).
pub const MAX_ITER: usize = 60;
pub const TOL: f64 = 1e-11;
/// Default sub-steps of [`SketchSystem::drag`].
pub const DRAG_STEPS: usize = 12;
/// Residual norm below which an unconverged solve still counts as solved.
const ACCEPT: f64 = 1e-6;

impl SketchSystem {
    /// Solves all hard constraints with the defaults (`solve()`).
    pub fn solve_default(&mut self) -> bool {
        self.solve(MAX_ITER, TOL)
    }

    /// Solves all hard constraints. Returns true when converged (`solve(maxIter, tol)`).
    pub fn solve(&mut self, max_iter: usize, tol: f64) -> bool {
        if self.eqs.is_empty() {
            return true;
        }
        let mut ok = true;
        for ci in 0..self.comps.len() {
            ok &= self.solve_component(ci, max_iter, tol);
        }
        ok && self.constant_err().sqrt() < ACCEPT
    }

    fn solve_component(&mut self, ci: usize, max_iter: usize, tol: f64) -> bool {
        if let Some(e) = self.comps[ci].clean_err {
            if e.sqrt() < tol {
                return true;
            }
        }
        let (ok, err) = self.iterate(ci, max_iter, tol);
        self.comps[ci].clean_err = Some(err);
        ok
    }

    /// The Gauss-Newton loop on one component; returns (converged, final |r|²).
    fn iterate(&mut self, ci: usize, max_iter: usize, tol: f64) -> (bool, f64) {
        let mut r = self.comp_residuals(ci);
        let mut err = norm2(&r);
        for _ in 0..max_iter {
            if err.sqrt() < tol {
                return (true, err);
            }
            self.comp_jacobian(ci);
            let c = &self.comps[ci];
            let step = min_norm_step(&c.jac, c.sym.as_ref().expect("symbolic"), &r);
            let x0: Vec<f64> = c.params.iter().map(|&p| self.x[p]).collect();
            let mut alpha = 1.0;
            let mut improved = false;
            for _ in 0..10 {
                for (k, &p) in self.comps[ci].params.iter().enumerate() {
                    self.x[p] = x0[k] - alpha * step[k];
                }
                let r2 = self.comp_residuals(ci);
                let e2 = norm2(&r2);
                if e2 < err {
                    r = r2;
                    err = e2;
                    improved = true;
                    break;
                }
                alpha *= 0.5;
            }
            if !improved {
                for (k, &p) in self.comps[ci].params.iter().enumerate() {
                    self.x[p] = x0[k];
                }
                return (err.sqrt() < ACCEPT, err);
            }
        }
        (err.sqrt() < ACCEPT, err)
    }

    /// Drags points towards targets while keeping constraints satisfied (`drag(targets,
    /// steps)`). Returns true if anything moved. Fixed or unknown points are ignored.
    pub fn drag(&mut self, targets: &[(&str, Vec2)], steps: usize) -> bool {
        let mut idx: Vec<(usize, Vec2)> = Vec::new();
        for (id, to) in targets {
            if let Some(crate::Pt::Var(i)) = self.pts.get(*id) {
                idx.push((*i, *to));
            }
        }
        if idx.is_empty() {
            return false;
        }
        let start = self.x.clone();
        // Components holding a dragged coordinate; coordinates no equation reads move freely.
        let mut dragged: Vec<usize> = idx
            .iter()
            .flat_map(|&(i, _)| [i, i + 1])
            .filter_map(|p| self.param_comp[p])
            .collect();
        dragged.sort_unstable();
        dragged.dedup();
        for _ in 0..steps {
            let mut d = vec![0.0; self.x.len()];
            let mut remaining = 0.0;
            for &(i, to) in &idx {
                d[i] = to[0] - self.x[i];
                d[i + 1] = to[1] - self.x[i + 1];
                remaining += d[i] * d[i] + d[i + 1] * d[i + 1];
            }
            if remaining < 1e-18 {
                break;
            }
            let mut moved = 0.0;
            for &(i, _) in &idx {
                for p in [i, i + 1] {
                    if self.param_comp[p].is_none() {
                        self.x[p] += d[p];
                        moved += d[p] * d[p];
                    }
                }
            }
            for &ci in &dragged {
                self.comp_jacobian(ci);
                let c = &self.comps[ci];
                let dl: Vec<f64> = c.params.iter().map(|&p| d[p]).collect();
                let jd = c.jac.mul(&dl);
                let corr = min_norm_step(&c.jac, c.sym.as_ref().expect("symbolic"), &jd);
                for (k, &p) in self.comps[ci].params.iter().enumerate() {
                    let proj = dl[k] - corr[k];
                    self.x[p] += proj;
                    moved += proj * proj;
                }
                self.comps[ci].clean_err = None;
            }
            if !self.solve(30, TOL) {
                self.x.copy_from_slice(&start);
                for c in &mut self.comps {
                    c.clean_err = None;
                }
                self.solve(30, TOL);
                return false;
            }
            if moved < 1e-14 {
                break;
            }
        }
        self.x
            .iter()
            .zip(&start)
            .any(|(a, b)| (a - b).abs() > 1e-12)
    }
}
