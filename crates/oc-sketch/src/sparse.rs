//! Sparse linear algebra for the minimum-norm Gauss-Newton step.
//!
//! The step solves `(J Jᵀ + μI) y = r` and takes `Jᵀ y`. `J` has one row per equation and a
//! handful of non-zeros per row, so `J Jᵀ` is sparse: rows are coupled only when their
//! equations share a parameter. The pattern depends only on the sketch's structure, so the
//! ordering and symbolic factorisation ([`Symbolic`]) are computed once per component, and
//! each iteration only refills the numbers ([`Symbolic::factor`]).
//!
//! Ordering: minimum degree with lowest-index tie-breaking (deterministic). Chains,
//! linkages and grids then factor with little fill; a constraint that couples many others
//! (a polygon's "all sides equal side 0") makes a dense block, which is inherent to the
//! dual form and costs what the dense TypeScript solver costs.
//!
//! If a pivot is not positive (the matrix is not numerically positive definite), the caller
//! falls back to dense Gaussian elimination with partial pivoting, as the TypeScript solver
//! does.

use std::collections::BTreeSet;

/// Rows of a sparse matrix with sorted column indices (`J`, one row per equation).
#[derive(Clone, Debug, Default)]
pub struct SparseRows {
    pub ptr: Vec<usize>,
    pub col: Vec<usize>,
    pub val: Vec<f64>,
    pub ncols: usize,
}

impl SparseRows {
    pub fn rows(&self) -> usize {
        self.ptr.len().saturating_sub(1)
    }
    pub fn row(&self, i: usize) -> (&[usize], &[f64]) {
        let (a, b) = (self.ptr[i], self.ptr[i + 1]);
        (&self.col[a..b], &self.val[a..b])
    }
    pub fn row_mut(&mut self, i: usize) -> &mut [f64] {
        let (a, b) = (self.ptr[i], self.ptr[i + 1]);
        &mut self.val[a..b]
    }
    /// `J v`.
    pub fn mul(&self, v: &[f64]) -> Vec<f64> {
        (0..self.rows())
            .map(|i| {
                let (c, x) = self.row(i);
                c.iter().zip(x).map(|(&j, &a)| a * v[j]).sum()
            })
            .collect()
    }
    /// `Jᵀ y`.
    pub fn mul_t(&self, y: &[f64]) -> Vec<f64> {
        let mut out = vec![0.0; self.ncols];
        for (i, &yi) in y.iter().enumerate() {
            if yi == 0.0 {
                continue;
            }
            let (c, x) = self.row(i);
            for (&j, &a) in c.iter().zip(x) {
                out[j] += a * yi;
            }
        }
        out
    }
}

/// Sorted union of `a` and `b`, without `u` and `v`.
fn merge_without(a: &[usize], b: &[usize], u: usize, v: usize) -> Vec<usize> {
    let mut out = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut k) = (0, 0);
    while i < a.len() || k < b.len() {
        let x = if k == b.len() || (i < a.len() && a[i] <= b[k]) {
            if k < b.len() && a[i] == b[k] {
                k += 1;
            }
            i += 1;
            a[i - 1]
        } else {
            k += 1;
            b[k - 1]
        };
        if x != u && x != v {
            out.push(x);
        }
    }
    out
}

fn sparse_dot(a: (&[usize], &[f64]), b: (&[usize], &[f64])) -> f64 {
    let (mut i, mut k, mut s) = (0, 0, 0.0);
    while i < a.0.len() && k < b.0.len() {
        match a.0[i].cmp(&b.0[k]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => k += 1,
            std::cmp::Ordering::Equal => {
                s += a.1[i] * b.1[k];
                i += 1;
                k += 1;
            }
        }
    }
    s
}

/// Ordering and the non-zero pattern of the Cholesky factor `L` of `P (J Jᵀ) Pᵀ`.
#[derive(Clone, Debug)]
pub struct Symbolic {
    m: usize,
    /// `perm[k]` = equation eliminated at step `k`.
    perm: Vec<usize>,
    /// Strictly-lower pattern of column `k` of `L` (positions > k, ascending).
    lcol_ptr: Vec<usize>,
    lrow: Vec<usize>,
    /// For column `j`: the earlier columns `k` with `L[j, k] ≠ 0`, as indices into `lrow`.
    rowref_ptr: Vec<usize>,
    rowref: Vec<(usize, usize)>,
    /// Lower part of column `j` of `P A Pᵀ`, including the diagonal: (position, equation
    /// pair) to compute it from `J`.
    a_ptr: Vec<usize>,
    a_ent: Vec<(usize, usize, usize)>,
}

/// Numeric factor.
pub struct Factor<'a> {
    s: &'a Symbolic,
    diag: Vec<f64>,
    lval: Vec<f64>,
}

impl Symbolic {
    /// Pattern of `J Jᵀ` for the given structure of `J`.
    pub fn new(j: &SparseRows) -> Self {
        let m = j.rows();
        // Adjacency of A = J Jᵀ: equations sharing a column.
        let mut by_col: Vec<Vec<usize>> = vec![Vec::new(); j.ncols];
        for i in 0..m {
            for &c in j.row(i).0 {
                by_col[c].push(i);
            }
        }
        // Sorted adjacency lists.
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); m];
        for rows in &by_col {
            for (a, &r) in rows.iter().enumerate() {
                for &s in &rows[a + 1..] {
                    adj[r].push(s);
                    adj[s].push(r);
                }
            }
        }
        for a in &mut adj {
            a.sort_unstable();
            a.dedup();
        }
        let a_pattern = adj.clone();
        // Minimum degree on the elimination graph: eliminating v joins its neighbours into a
        // clique. Ties go to the lowest index, so the ordering is deterministic.
        let mut queue: BTreeSet<(usize, usize)> = (0..m).map(|i| (adj[i].len(), i)).collect();
        let mut perm = Vec::with_capacity(m);
        let mut pattern_nodes: Vec<Vec<usize>> = Vec::with_capacity(m);
        while let Some((_, v)) = queue.pop_first() {
            let nb = std::mem::take(&mut adj[v]);
            for &u in &nb {
                queue.remove(&(adj[u].len(), u));
                adj[u] = merge_without(&adj[u], &nb, u, v);
                queue.insert((adj[u].len(), u));
            }
            perm.push(v);
            pattern_nodes.push(nb);
        }
        let mut pos = vec![0; m];
        for (k, &v) in perm.iter().enumerate() {
            pos[v] = k;
        }
        let mut lcol_ptr = vec![0];
        let mut lrow = Vec::new();
        for nodes in &pattern_nodes {
            let mut p: Vec<usize> = nodes.iter().map(|&v| pos[v]).collect();
            p.sort_unstable();
            lrow.extend(p);
            lcol_ptr.push(lrow.len());
        }
        let mut rowrefs: Vec<Vec<(usize, usize)>> = vec![Vec::new(); m];
        for k in 0..m {
            for idx in lcol_ptr[k]..lcol_ptr[k + 1] {
                rowrefs[lrow[idx]].push((k, idx));
            }
        }
        let mut rowref_ptr = vec![0];
        let mut rowref = Vec::new();
        for r in rowrefs {
            rowref.extend(r);
            rowref_ptr.push(rowref.len());
        }
        let mut a_ptr = vec![0];
        let mut a_ent = Vec::new();
        for (jpos, &eq) in perm.iter().enumerate() {
            a_ent.push((jpos, eq, eq));
            let mut lower: Vec<(usize, usize, usize)> = a_pattern[eq]
                .iter()
                .filter(|&&o| pos[o] > jpos)
                .map(|&o| (pos[o], o, eq))
                .collect();
            lower.sort_unstable();
            a_ent.extend(lower);
            a_ptr.push(a_ent.len());
        }
        Self {
            m,
            perm,
            lcol_ptr,
            lrow,
            rowref_ptr,
            rowref,
            a_ptr,
            a_ent,
        }
    }

    /// Non-zeros in `L` below the diagonal.
    pub fn fill(&self) -> usize {
        self.lrow.len()
    }

    /// Left-looking Cholesky of `P (J Jᵀ + μI) Pᵀ`. `None` if a pivot is not positive.
    pub fn factor(&self, j: &SparseRows, mu: f64) -> Option<Factor<'_>> {
        let m = self.m;
        let mut diag = vec![0.0; m];
        let mut lval = vec![0.0; self.lrow.len()];
        let mut w = vec![0.0; m];
        for col in 0..m {
            for &(p, a, b) in &self.a_ent[self.a_ptr[col]..self.a_ptr[col + 1]] {
                w[p] = sparse_dot(j.row(a), j.row(b));
            }
            w[col] += mu;
            for &(k, idx) in &self.rowref[self.rowref_ptr[col]..self.rowref_ptr[col + 1]] {
                let ljk = lval[idx];
                w[col] -= ljk * ljk;
                for t in idx + 1..self.lcol_ptr[k + 1] {
                    w[self.lrow[t]] -= lval[t] * ljk;
                }
            }
            let d = w[col];
            if d.is_nan() || d <= 1e-300 {
                return None;
            }
            let ljj = d.sqrt();
            diag[col] = ljj;
            w[col] = 0.0;
            let span = self.lcol_ptr[col]..self.lcol_ptr[col + 1];
            for (l, &r) in lval[span.clone()].iter_mut().zip(&self.lrow[span]) {
                *l = w[r] / ljj;
                w[r] = 0.0;
            }
        }
        Some(Factor {
            s: self,
            diag,
            lval,
        })
    }
}

impl Factor<'_> {
    /// Solves `(J Jᵀ + μI) y = b` (in equation order).
    pub fn solve(&self, b: &[f64]) -> Vec<f64> {
        let s = self.s;
        let mut z: Vec<f64> = s.perm.iter().map(|&e| b[e]).collect();
        for k in 0..s.m {
            z[k] /= self.diag[k];
            let zk = z[k];
            for t in s.lcol_ptr[k]..s.lcol_ptr[k + 1] {
                z[s.lrow[t]] -= self.lval[t] * zk;
            }
        }
        for k in (0..s.m).rev() {
            let mut v = z[k];
            for t in s.lcol_ptr[k]..s.lcol_ptr[k + 1] {
                v -= self.lval[t] * z[s.lrow[t]];
            }
            z[k] = v / self.diag[k];
        }
        let mut y = vec![0.0; s.m];
        for (k, &e) in s.perm.iter().enumerate() {
            y[e] = z[k];
        }
        y
    }
}

/// Dense Gaussian elimination with partial pivoting (`gauss` in TypeScript), for
/// semi-definite systems the Cholesky factorisation rejects. Singular columns give 0.
pub fn gauss(a: &mut [f64], b: &mut [f64], m: usize) -> Vec<f64> {
    for c in 0..m {
        let mut p = c;
        for r in c + 1..m {
            if a[r * m + c].abs() > a[p * m + c].abs() {
                p = r;
            }
        }
        if a[p * m + c].abs() < 1e-300 {
            continue;
        }
        if p != c {
            for k in 0..m {
                a.swap(c * m + k, p * m + k);
            }
            b.swap(c, p);
        }
        for r in c + 1..m {
            let f = a[r * m + c] / a[c * m + c];
            if f == 0.0 {
                continue;
            }
            for k in c..m {
                a[r * m + k] -= f * a[c * m + k];
            }
            b[r] -= f * b[c];
        }
    }
    let mut x = vec![0.0; m];
    for i in (0..m).rev() {
        let mut s = b[i];
        for k in i + 1..m {
            s -= a[i * m + k] * x[k];
        }
        x[i] = if a[i * m + i].abs() < 1e-300 {
            0.0
        } else {
            s / a[i * m + i]
        };
    }
    x
}

/// `y = (J Jᵀ + μI)⁻¹ b` with `μ = max(1e-12, 1e-9 · trace(J Jᵀ) / m)`, then `Jᵀ y`: the
/// minimum-norm damped Gauss-Newton step for residual `b` (`minNormStep`).
pub fn min_norm_step(j: &SparseRows, sym: &Symbolic, b: &[f64]) -> Vec<f64> {
    let m = j.rows();
    let trace: f64 = j.val.iter().map(|v| v * v).sum();
    let mu = (trace / m.max(1) as f64 * 1e-9).max(1e-12);
    let y = match sym.factor(j, mu) {
        Some(f) => f.solve(b),
        None => {
            let mut a = vec![0.0; m * m];
            for i in 0..m {
                for k in i..m {
                    let s = sparse_dot(j.row(i), j.row(k));
                    a[i * m + k] = s;
                    a[k * m + i] = s;
                }
                a[i * m + i] += mu;
            }
            gauss(&mut a, &mut b.to_vec(), m)
        }
    };
    j.mul_t(&y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(r: &[&[(usize, f64)]], ncols: usize) -> SparseRows {
        let mut s = SparseRows {
            ptr: vec![0],
            ncols,
            ..Default::default()
        };
        for row in r {
            for &(c, v) in *row {
                s.col.push(c);
                s.val.push(v);
            }
            s.ptr.push(s.col.len());
        }
        s
    }

    fn dense_solve(j: &SparseRows, mu: f64, b: &[f64]) -> Vec<f64> {
        let m = j.rows();
        let mut a = vec![0.0; m * m];
        for i in 0..m {
            for k in 0..m {
                a[i * m + k] = sparse_dot(j.row(i), j.row(k));
            }
            a[i * m + i] += mu;
        }
        gauss(&mut a, &mut b.to_vec(), m)
    }

    #[test]
    fn sparse_cholesky_matches_dense_elimination() {
        // A chain plus a hub row touching everything: exercises fill and ordering.
        let n = 12;
        let mut r: Vec<Vec<(usize, f64)>> = (0..n - 1)
            .map(|i| vec![(i, 1.0 + i as f64 * 0.1), (i + 1, -0.7)])
            .collect();
        r.push(
            (0..n)
                .step_by(3)
                .map(|c| (c, 0.3 + c as f64 * 0.05))
                .collect(),
        );
        r.push(vec![(2, 1.0), (7, 2.0)]);
        let refs: Vec<&[(usize, f64)]> = r.iter().map(|v| v.as_slice()).collect();
        let j = rows(&refs, n);
        let sym = Symbolic::new(&j);
        let b: Vec<f64> = (0..j.rows()).map(|i| (i as f64 * 0.37).sin()).collect();
        let f = sym.factor(&j, 1e-3).expect("positive definite");
        let y = f.solve(&b);
        let yd = dense_solve(&j, 1e-3, &b);
        for (a, d) in y.iter().zip(&yd) {
            assert!((a - d).abs() < 1e-10 * d.abs().max(1.0), "{a} vs {d}");
        }
    }

    #[test]
    fn minimum_degree_keeps_a_chain_free_of_fill() {
        let n = 50;
        let r: Vec<Vec<(usize, f64)>> = (0..n - 1).map(|i| vec![(i, 1.0), (i + 1, -1.0)]).collect();
        let refs: Vec<&[(usize, f64)]> = r.iter().map(|v| v.as_slice()).collect();
        let sym = Symbolic::new(&rows(&refs, n));
        assert!(sym.fill() <= n, "fill {}", sym.fill());
    }
}
