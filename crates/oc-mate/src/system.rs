//! Parameters, mate blocks, residuals and Jacobians, and the Gauss-Newton solve.
//!
//! Every floating body has six parameters `x = (δ, ω)`: a translation and a rotation
//! increment ([`crate::geom::increment`]) applied on top of its current pose. Jacobians
//! are always taken at `x = 0`, and after every accepted step the increments are folded
//! into the poses and reset to zero (a local parameterisation, as in manifold
//! optimisers), so no fixed angle convention and no gimbal lock exist.
//!
//! The step is the sketch solver's: minimum-norm damped Gauss-Newton
//! (`Δ = -Jᵀ (J Jᵀ + μI)⁻¹ r`, `oc_sketch::sparse`), with a halving line search, so
//! under-defined components move as little as possible.
use oc_sketch::real::Dual;
use oc_sketch::sparse::{min_norm_step, SparseRows, Symbolic};

use crate::geom::*;
use crate::rows::{place, residuals, Mode, Rule};
use crate::Owner;

pub const MAX_ITER: usize = 60;
/// Converged when the residual norm is below this (mm, or radians for directions).
pub const TOL: f64 = 1e-10;
/// Residual norm below which an unconverged solve still counts as solved.
pub const ACCEPT: f64 = 1e-7;

#[derive(Clone, Debug)]
pub(crate) struct Block {
    pub mate: usize,
    pub rule: Rule,
    pub a: Owner,
    pub b: Owner,
    pub ga: Geom,
    pub gb: Geom,
}

#[derive(Clone, Debug)]
pub(crate) struct Sys {
    pub poses: Vec<Pose>,
    /// Parameter offset of each body (`None`: fixed).
    pub var: Vec<Option<usize>>,
    pub n: usize,
    pub blocks: Vec<Block>,
}

type D = Dual<12>;

impl Sys {
    pub fn new(poses: Vec<Pose>, fixed: &[bool], blocks: Vec<Block>) -> Sys {
        let mut n = 0;
        let var = fixed
            .iter()
            .map(|&f| {
                (!f).then(|| {
                    n += 6;
                    n - 6
                })
            })
            .collect();
        Sys { poses, var, n, blocks }
    }

    fn offset(&self, o: Owner) -> Option<usize> {
        match o {
            Owner::Ground => None,
            Owner::Body(i) => self.var[i],
        }
    }

    fn pose(&self, o: Owner) -> Pose {
        match o {
            Owner::Ground => Pose::IDENTITY,
            Owner::Body(i) => self.poses[i],
        }
    }

    /// Residuals of the active blocks at increments `x` (row-major, block order).
    pub fn residuals(&self, x: &[f64], mode: Mode, active: &[bool]) -> Vec<f64> {
        let mut out = Vec::new();
        for b in self.blocks.iter().filter(|b| active[b.mate]) {
            let take = |o: Owner| self.offset(o).map(|k| <[f64; 6]>::try_from(&x[k..k + 6]).expect("six"));
            let (xa, xb) = (take(b.a), take(b.b));
            let pa = place(&b.ga, &self.pose(b.a), xa.as_ref());
            let pb = place(&b.gb, &self.pose(b.b), xb.as_ref());
            residuals(&b.rule, &pa, &pb, mode, &mut out);
        }
        out
    }

    /// Jacobian at `x = 0` of the active blocks, sparse, with the mate of each row.
    pub fn jacobian(&self, mode: Mode, active: &[bool]) -> (SparseRows, Vec<usize>) {
        let mut j = SparseRows { ptr: vec![0], ncols: self.n, ..Default::default() };
        let mut owner = Vec::new();
        let mut rows: Vec<D> = Vec::new();
        for b in self.blocks.iter().filter(|b| active[b.mate]) {
            let seed = |o: Owner, slot: usize| -> Option<[D; 6]> { self.offset(o).map(|_| std::array::from_fn(|k| D::var(0.0, slot + k))) };
            let (xa, xb) = (seed(b.a, 0), seed(b.b, 6));
            let pa = place(&b.ga, &self.pose(b.a), xa.as_ref());
            let pb = place(&b.gb, &self.pose(b.b), xb.as_ref());
            rows.clear();
            residuals(&b.rule, &pa, &pb, mode, &mut rows);
            // Columns of this block, ascending (sides may share a body, e.g. self-mates).
            let mut cols: Vec<(usize, usize)> = Vec::new();
            for (o, slot) in [(b.a, 0), (b.b, 6)] {
                if let Some(k) = self.offset(o) {
                    cols.extend((0..6).map(|c| (k + c, slot + c)));
                }
            }
            cols.sort_unstable();
            for r in &rows {
                let mut last: Option<usize> = None;
                for &(col, slot) in &cols {
                    if last == Some(col) {
                        *j.val.last_mut().expect("entry") += r.d[slot];
                        continue;
                    }
                    j.col.push(col);
                    j.val.push(r.d[slot]);
                    last = Some(col);
                }
                j.ptr.push(j.col.len());
                owner.push(b.mate);
            }
        }
        (j, owner)
    }

    /// Folds increments `x` into the poses.
    pub fn rebase(&mut self, x: &[f64]) {
        for (i, v) in self.var.iter().enumerate() {
            if let Some(k) = *v {
                let p = &mut self.poses[i];
                let qi = increment([x[k + 3], x[k + 4], x[k + 5]]);
                p.q = qnorm(qmul(qi, p.q));
                p.t = add(p.t, [x[k], x[k + 1], x[k + 2]]);
            }
        }
    }

    /// Gauss-Newton on the active blocks. True when converged.
    pub fn solve(&mut self, active: &[bool], max_iter: usize) -> bool {
        let zero = vec![0.0; self.n];
        let mut r = self.residuals(&zero, Mode::Solve, active);
        let mut err = norm2(&r);
        if r.is_empty() || err.sqrt() < TOL {
            return true;
        }
        if self.n == 0 {
            return err.sqrt() < ACCEPT;
        }
        let mut sym: Option<Symbolic> = None;
        for _ in 0..max_iter {
            if err.sqrt() < TOL {
                return true;
            }
            let (j, _) = self.jacobian(Mode::Solve, active);
            let s = sym.get_or_insert_with(|| Symbolic::new(&j));
            let step = min_norm_step(&j, s, &r);
            let mut alpha = 1.0;
            let mut improved = false;
            for _ in 0..12 {
                let x: Vec<f64> = step.iter().map(|v| -alpha * v).collect();
                let r2 = self.residuals(&x, Mode::Solve, active);
                let e2 = norm2(&r2);
                if e2 < err {
                    self.rebase(&x);
                    // Re-evaluate at the new base (the increment map is not exactly additive).
                    r = self.residuals(&zero, Mode::Solve, active);
                    err = norm2(&r);
                    improved = true;
                    break;
                }
                alpha *= 0.5;
            }
            if !improved {
                return err.sqrt() < ACCEPT;
            }
        }
        err.sqrt() < ACCEPT
    }
}

pub(crate) fn norm2(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}
