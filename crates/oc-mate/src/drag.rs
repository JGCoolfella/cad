//! Dragging a component within its free degrees of freedom.
//!
//! The motions the mates allow to first order are the null space of the mate Jacobian
//! `J` (rank-analysis rows, [`Mode::Analyse`]). Its basis `N` comes from the same RREF the
//! analysis uses (`oc_sketch::analysis::rref`): one vector per free column. Each sub-step
//! asks for the allowed motion `Δx = N a` that moves the grabbed point by the remaining
//! distance `d` with the smallest weighted size, rotation counting `lever` mm per radian
//! (`W`): with `G` the grabbed point's 3×n Jacobian, `M = G N` and `H = Nᵀ W² N`,
//! `a = H⁻¹ Mᵀ (M H⁻¹ Mᵀ)⁺ d`. The 3×3 pseudo-inverse drops directions the point cannot
//! move in. The mates are then solved again to remove the second-order drift.
//!
//! A sub-step that cannot be re-solved is undone and the drag stops there; one that ends
//! further from the target is undone and retried shorter; long turns are taken in pieces.
//! The drag ends at the target or the nearest place the mates allow.
//!
//! Dense: fine for G1's assemblies; G2's 10k-component assemblies would drag one connected
//! group of bodies at a time (the analysis already splits them).
use oc_sketch::analysis::rref;
use oc_sketch::sparse::gauss;

use crate::geom::*;
use crate::rows::Mode;
use crate::system::{norm2, Sys};

/// At most this many sub-steps.
const STEPS: usize = 200;
/// Largest turn of any body in one sub-step (radians).
const MAX_TURN: f64 = 0.1;

pub(crate) fn drag(sys: &mut Sys, active: &[bool], body: usize, grab: V3, target: V3, lever: f64) {
    let Some(k) = sys.var[body] else { return };
    let n = sys.n;
    let w: Vec<f64> = (0..n).map(|c| if (c % 6) >= 3 { lever } else { 1.0 }).collect();
    let mut best = (f64::INFINITY, sys.poses.clone());
    let mut scale = 1.0;
    for _ in 0..STEPS {
        let pose = sys.poses[body];
        let at = pose.point(grab);
        let d = sub(target, at);
        let dist = norm2(&d).sqrt();
        if dist < best.0 - 1e-10 * (1.0 + dist) {
            best = (dist, sys.poses.clone());
        } else {
            // Further away than before: undo, and try a shorter step.
            sys.poses = best.1.clone();
            scale *= 0.5;
            if scale < 1e-3 {
                break;
            }
            continue;
        }
        if dist < 1e-9 {
            break;
        }
        let basis = null_space(sys, active);
        if basis.is_empty() {
            break;
        }
        let kk = basis.len();
        // M = G N: how each basis motion moves the point (∂p/∂δ = I, ∂p/∂ω · ω = ω × v).
        let v = sub(at, pose.t);
        let m: Vec<V3> = basis.iter().map(|b| add([b[k], b[k + 1], b[k + 2]], cross([b[k + 3], b[k + 4], b[k + 5]], v))).collect();
        // H = Nᵀ W² N, and H⁻¹ Mᵀ column by column.
        let mut h = vec![0.0; kk * kk];
        for i in 0..kk {
            for j in i..kk {
                let s: f64 = (0..n).map(|c| basis[i][c] * basis[j][c] * w[c] * w[c]).sum();
                h[i * kk + j] = s;
                h[j * kk + i] = s;
            }
        }
        let hm: Vec<Vec<f64>> = (0..3).map(|r| gauss(&mut h.clone(), &mut m.iter().map(|x| x[r]).collect::<Vec<_>>(), kk)).collect();
        let mut a3 = vec![0.0; 9];
        for r in 0..3 {
            for c in 0..3 {
                a3[r * 3 + c] = (0..kk).map(|i| m[i][r] * hm[c][i]).sum();
            }
        }
        let Some(y) = pinv3(a3, d, 0.0) else { break };
        let coef: Vec<f64> = (0..kk).map(|i| (0..3).map(|r| hm[r][i] * y[r]).sum()).collect();
        let mut x = vec![0.0; n];
        for (b, c) in basis.iter().zip(&coef) {
            for (xc, bv) in x.iter_mut().zip(b) {
                *xc += c * bv;
            }
        }
        let turn = (0..n).step_by(6).map(|q| norm([x[q + 3], x[q + 4], x[q + 5]])).fold(0.0, f64::max);
        let f = scale * if turn * scale > MAX_TURN { MAX_TURN / (turn * scale) } else { 1.0 };
        x.iter_mut().for_each(|v| *v *= f);
        let keep = sys.poses.clone();
        sys.rebase(&x);
        if !sys.solve(active, 30) {
            sys.poses = keep;
            break;
        }
    }
}

/// Basis of the null space of the analysis Jacobian, one vector per free column.
fn null_space(sys: &Sys, active: &[bool]) -> Vec<Vec<f64>> {
    let n = sys.n;
    let (j, _) = sys.jacobian(Mode::Analyse, active);
    let rows: Vec<Vec<f64>> = (0..j.rows())
        .map(|r| {
            let (cols, vals) = j.row(r);
            let mut row = vec![0.0; n];
            for (&c, &v) in cols.iter().zip(vals) {
                row[c] += v;
            }
            let s = row.iter().map(|v| v * v).sum::<f64>().sqrt();
            if s > 0.0 {
                row.iter_mut().for_each(|v| *v /= s);
            }
            row
        })
        .collect();
    let (pivots, red, _) = rref(rows, n);
    let mut is_pivot = vec![false; n];
    pivots.iter().for_each(|&c| is_pivot[c] = true);
    (0..n)
        .filter(|&f| !is_pivot[f])
        .map(|f| {
            let mut v = vec![0.0; n];
            v[f] = 1.0;
            for (row, &pc) in red.iter().zip(&pivots) {
                v[pc] = -row[f];
            }
            v
        })
        .collect()
}

/// `A⁺ d` for a symmetric 3×3 `A` (row-major), dropping eigenvalues below `floor` and
/// below `1e-9 · λmax`. `None` when nothing is left.
fn pinv3(a: Vec<f64>, d: V3, floor: f64) -> Option<[f64; 3]> {
    let (vals, vecs) = jacobi3([[a[0], a[1], a[2]], [a[3], a[4], a[5]], [a[6], a[7], a[8]]]);
    let max = vals.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let cut = floor.max(1e-9 * max);
    if max <= cut || max < 1e-300 {
        return None;
    }
    let mut y = [0.0; 3];
    for k in 0..3 {
        if vals[k].abs() <= cut {
            continue;
        }
        let v = [vecs[0][k], vecs[1][k], vecs[2][k]];
        let c = dot(v, d) / vals[k];
        for i in 0..3 {
            y[i] += c * v[i];
        }
    }
    Some(y)
}

/// Eigen-decomposition of a symmetric 3×3 matrix by cyclic Jacobi rotations: eigenvalues,
/// and eigenvectors as columns.
fn jacobi3(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        let off = a[0][1].abs() + a[0][2].abs() + a[1][2].abs();
        if off < 1e-300 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-300 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = if theta == 0.0 { 1.0 } else { theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt()) };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for row in &mut a {
                let (akp, akq) = (row[p], row[q]);
                row[p] = c * akp - s * akq;
                row[q] = s * akp + c * akq;
            }
            let (rp, rq) = (a[p], a[q]);
            for k in 0..3 {
                a[p][k] = c * rp[k] - s * rq[k];
                a[q][k] = s * rp[k] + c * rq[k];
            }
            for row in &mut v {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    ([a[0][0], a[1][1], a[2][2]], v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pseudo_inverse_drops_the_null_direction() {
        // A = diag(4, 1, 0) rotated: the third direction cannot be reached.
        let r = [[0.6, -0.8, 0.0], [0.8, 0.6, 0.0], [0.0, 0.0, 1.0]];
        let l = [4.0, 1.0, 1e-14];
        let mut a = vec![0.0; 9];
        for i in 0..3 {
            for j in 0..3 {
                a[i * 3 + j] = (0..3).map(|k| r[i][k] * l[k] * r[j][k]).sum();
            }
        }
        let y = pinv3(a, [0.6 * 4.0, 0.8 * 4.0, 7.0], 0.0).unwrap();
        assert!((y[0] - 0.6).abs() < 1e-12 && (y[1] - 0.8).abs() < 1e-12 && y[2].abs() < 1e-12, "{y:?}");
    }
}
