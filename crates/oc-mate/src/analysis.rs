//! Degrees of freedom and definition state, by the sketch solver's rank analysis
//! (`oc_sketch::analysis::rref`): the row-normalised Jacobian (in [`Mode::Analyse`]) is
//! reduced in mate order, so a row that vanishes is dependent on the rows before it. A mate
//! all of whose rows vanish adds nothing and is redundant (the newest such mate is the one
//! flagged); a mate that repeats only some rows of earlier ones is normal (three coincident
//! faces share their rotations) and is not. A parameter is determined when it is a pivot column whose reduced
//! row has no free-column components. Bodies that share no mate never meet in the
//! elimination, so it runs per connected group of bodies.
use oc_sketch::analysis::{rref, FREE_TOL, PIVOT_TOL};

use crate::rows::Mode;
use crate::system::Sys;

pub(crate) struct Ranks {
    pub dof: usize,
    /// Each body's six parameters determined (fully defined).
    pub determined: Vec<bool>,
    /// Each body's freedom with every other body held: `6 - rank` of its own columns.
    pub free: Vec<usize>,
    /// Mates all of whose rows are dependent, in order.
    pub redundant: Vec<usize>,
}

pub(crate) fn analyse(sys: &Sys, active: &[bool]) -> Ranks {
    let nb = sys.poses.len();
    let (j, owner) = sys.jacobian(Mode::Analyse, active);
    // Body of each parameter column.
    let mut col_body = vec![0usize; sys.n];
    for (b, v) in sys.var.iter().enumerate() {
        if let Some(k) = *v {
            col_body[k..k + 6].fill(b);
        }
    }
    // Union-find over bodies joined by a row.
    let mut parent: Vec<usize> = (0..nb).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for r in 0..j.rows() {
        let (cols, _) = j.row(r);
        if let Some((&first, rest)) = cols.split_first() {
            for &c in rest {
                let (a, b) = (find(&mut parent, col_body[first]), find(&mut parent, col_body[c]));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut redundant: Vec<usize> = Vec::new();
    let mut col_determined = vec![false; sys.n];
    let mut rank = 0;
    // Rows that read no parameter (both sides fixed) are constant: always dependent.
    for r in 0..j.rows() {
        if j.row(r).0.is_empty() {
            redundant.push(r);
        }
    }
    let roots: Vec<usize> = {
        let mut v: Vec<usize> = (0..nb).filter(|&b| sys.var[b].is_some()).map(|b| find(&mut parent, b)).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    for root in roots {
        let cols: Vec<usize> = (0..sys.n).filter(|&c| find(&mut parent, col_body[c]) == root).collect();
        let mut local = vec![usize::MAX; sys.n];
        for (k, &c) in cols.iter().enumerate() {
            local[c] = k;
        }
        let row_ids: Vec<usize> = (0..j.rows()).filter(|&r| j.row(r).0.first().is_some_and(|&c| local[c] != usize::MAX)).collect();
        let rows: Vec<Vec<f64>> = row_ids.iter().map(|&r| dense_row(&j, r, &local, cols.len())).collect();
        let (pivots, red, dependent) = rref(rows, cols.len());
        redundant.extend(dependent.iter().map(|&i| row_ids[i]));
        rank += pivots.len();
        let mut is_pivot = vec![None; cols.len()];
        for (k, &c) in pivots.iter().enumerate() {
            is_pivot[c] = Some(k);
        }
        let free: Vec<usize> = (0..cols.len()).filter(|&c| is_pivot[c].is_none()).collect();
        for (c, pk) in is_pivot.iter().enumerate() {
            if let Some(k) = *pk {
                if free.iter().all(|&f| red[k][f].abs() <= FREE_TOL) {
                    col_determined[cols[c]] = true;
                }
            }
        }
    }
    let mut determined = vec![false; nb];
    let mut free = vec![0usize; nb];
    for b in 0..nb {
        let Some(k) = sys.var[b] else {
            determined[b] = true;
            continue;
        };
        determined[b] = col_determined[k..k + 6].iter().all(|&d| d);
        // Own freedom: rank of the rows touching this body, restricted to its columns.
        let mut local = vec![usize::MAX; sys.n];
        for c in 0..6 {
            local[k + c] = c;
        }
        let rows: Vec<Vec<f64>> = (0..j.rows()).filter(|&r| j.row(r).0.iter().any(|&c| local[c] != usize::MAX)).map(|r| dense_row(&j, r, &local, 6)).filter(|r| r.iter().any(|v| v.abs() > PIVOT_TOL)).collect();
        let (pivots, _, _) = rref(rows, 6);
        free[b] = 6 - pivots.len();
    }
    // A mate is redundant when none of its rows adds rank. Mates routinely repeat some
    // of each other's rows (three coincident faces share the rotations between them), and
    // SOLIDWORKS accepts that; a mate that adds nothing at all over-defines.
    let mut rows_of: std::collections::BTreeMap<usize, (usize, usize)> = std::collections::BTreeMap::new();
    for &m in &owner {
        rows_of.entry(m).or_default().0 += 1;
    }
    for &r in &redundant {
        rows_of.entry(owner[r]).or_default().1 += 1;
    }
    let mates: Vec<usize> = rows_of.into_iter().filter(|(_, (n, dep))| n == dep).map(|(m, _)| m).collect();
    Ranks { dof: sys.n - rank, determined, free, redundant: mates }
}

/// Row `r` of `j` on the columns mapped by `local` (others dropped), normalised.
fn dense_row(j: &oc_sketch::sparse::SparseRows, r: usize, local: &[usize], n: usize) -> Vec<f64> {
    let (cols, vals) = j.row(r);
    let mut row = vec![0.0; n];
    for (&c, &v) in cols.iter().zip(vals) {
        if local[c] != usize::MAX {
            row[local[c]] += v;
        }
    }
    let s = row.iter().map(|v| v * v).sum::<f64>().sqrt();
    if s > 0.0 {
        row.iter_mut().for_each(|v| *v /= s);
    }
    row
}
