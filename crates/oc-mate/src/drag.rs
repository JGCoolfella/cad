//! Dragging a component within its free degrees of freedom.
//!
//! Each sub-step asks for the parameter change `Δx` that moves the grabbed point by the
//! remaining distance `d` while keeping the mates to first order (`J Δx = 0`), with the
//! smallest weighted size: rotation parameters count `lever` mm per radian. In weighted
//! parameters `y`: `y = P Gᵀ (G P Gᵀ + μI)⁻¹ d`, where `G` is the grabbed point's 3×n
//! Jacobian and `P` the projector onto the null space of the mate Jacobian, applied as
//! `P v = v − Jᵀ (J Jᵀ + μI)⁻¹ J v` (the sketch solver's minimum-norm step). `G P Gᵀ` is
//! inverted on its eigenvectors above a relative cutoff (directions the point cannot move
//! in are dropped, not amplified from the projector's rounding). The mates are
//! then solved again to remove the second-order drift. A sub-step that cannot be re-solved
//! is undone and the drag stops there.
use oc_sketch::sparse::{min_norm_step, Symbolic};

use crate::geom::*;
use crate::rows::Mode;
use crate::system::{norm2, Sys};

const STEPS: usize = 12;

pub(crate) fn drag(sys: &mut Sys, active: &[bool], body: usize, grab: V3, target: V3, lever: f64) {
    let Some(k) = sys.var[body] else { return };
    let n = sys.n;
    for _ in 0..STEPS {
        let pose = sys.poses[body];
        let at = pose.point(grab);
        let d = sub(target, at);
        if norm2(&d) < 1e-18 {
            break;
        }
        // Weighted mate Jacobian: rotation columns divided by `lever`.
        let (mut j, _) = sys.jacobian(Mode::Analyse, active);
        let w: Vec<f64> = (0..n).map(|c| if (c % 6) >= 3 { lever } else { 1.0 }).collect();
        for r in 0..j.rows() {
            let (a, b) = (j.ptr[r], j.ptr[r + 1]);
            for e in a..b {
                j.val[e] /= w[j.col[e]];
            }
        }
        let sym = (j.rows() > 0).then(|| Symbolic::new(&j));
        let project = |v: &[f64]| -> Vec<f64> {
            match &sym {
                Some(s) => {
                    let jv = j.mul(v);
                    let c = min_norm_step(&j, s, &jv);
                    v.iter().zip(c).map(|(a, b)| a - b).collect()
                }
                None => v.to_vec(),
            }
        };
        // Columns of Gᵀ (weighted): moving the point by e_i needs δ = e_i, or ω with
        // ω × v = e_i (v = the point relative to the body's origin).
        let v = sub(at, pose.t);
        let gt: Vec<Vec<f64>> = (0..3)
            .map(|i| {
                let mut col = vec![0.0; n];
                let e = {
                    let mut e = [0.0; 3];
                    e[i] = 1.0;
                    e
                };
                col[k..k + 3].copy_from_slice(&e);
                // ∂p/∂ω = −[v]×: row i of it is (e_i × v)… as a column of Gᵀ: v × e_i.
                let c = cross(v, e);
                for a in 0..3 {
                    col[k + 3 + a] = c[a] / lever;
                }
                col
            })
            .collect();
        let pg: Vec<Vec<f64>> = gt.iter().map(|c| project(c)).collect();
        let mut a = vec![0.0; 9];
        for r in 0..3 {
            for c in 0..3 {
                a[r * 3 + c] = gt[r].iter().zip(&pg[c]).map(|(x, y)| x * y).sum();
            }
        }
        let Some(y3) = pinv3(a, d) else {
            break; // the point cannot move at all
        };
        let mut x = vec![0.0; n];
        for (i, col) in pg.iter().enumerate() {
            for (xc, v) in x.iter_mut().zip(col) {
                *xc += y3[i] * v;
            }
        }
        for (xc, wc) in x.iter_mut().zip(&w) {
            *xc /= wc;
        }
        let moved = norm2(&x);
        let keep = sys.poses.clone();
        sys.rebase(&x);
        if !sys.solve(active, 30) {
            sys.poses = keep;
            break;
        }
        if moved < 1e-20 {
            break;
        }
    }
}

/// `A⁺ d` for a symmetric 3×3 `A` (row-major), dropping eigenvalues below `1e-9 · λmax`.
/// `None` when `A` is (numerically) zero.
fn pinv3(a: Vec<f64>, d: V3) -> Option<[f64; 3]> {
    let (vals, vecs) = jacobi3([[a[0], a[1], a[2]], [a[3], a[4], a[5]], [a[6], a[7], a[8]]]);
    let max = vals.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    if max < 1e-18 {
        return None;
    }
    let mut y = [0.0; 3];
    for k in 0..3 {
        if vals[k].abs() <= 1e-9 * max {
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
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
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
        let y = pinv3(a, [0.6 * 4.0, 0.8 * 4.0, 7.0]).unwrap();
        assert!((y[0] - 0.6).abs() < 1e-12 && (y[1] - 0.8).abs() < 1e-12 && y[2].abs() < 1e-12, "{y:?}");
    }
}
