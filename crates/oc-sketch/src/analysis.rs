//! Degrees of freedom and definition state (`analyse` in TypeScript).
//!
//! The row-normalised Jacobian is reduced to RREF, processing equations in creation order,
//! so a row that vanishes is redundant and the newest conflicting relation is the one
//! flagged. A parameter is *determined* when it is a pivot column whose RREF row has no
//! free-column components (black vs blue). Rows of different components never interact in
//! the elimination, so reducing each component on its own gives exactly the global result.

use crate::build::Owner;
use crate::equations::{gradient, Pt, Rad, Values};
use crate::model::{Id, SketchEntity};
use crate::system::SketchSystem;
use indexmap::{IndexMap, IndexSet};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DefState {
    Under,
    Full,
    Over,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Under,
    Full,
    Over,
    Unsolved,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub dof: usize,
    pub status: Status,
    pub point_state: IndexMap<Id, DefState>,
    pub entity_state: IndexMap<Id, DefState>,
    /// Relations and dimensions that are redundant or conflicting, in the order found.
    pub bad: IndexSet<Id>,
}

/// Pivot tolerance of the elimination and the free-column tolerance of "determined".
pub const PIVOT_TOL: f64 = 1e-8;
pub const FREE_TOL: f64 = 1e-7;

/// Row-reduces `rows` (each of length `n`) in order. Returns the pivot column of each
/// independent row with the reduced rows, and the indices of dependent rows. Rows should
/// be normalised (the pivot tolerance is absolute). Also used by the assembly mate solver
/// (`oc-mate`).
pub fn rref(rows: Vec<Vec<f64>>, n: usize) -> (Vec<usize>, Vec<Vec<f64>>, Vec<usize>) {
    let mut pivot_col: Vec<usize> = Vec::new();
    let mut out: Vec<Vec<f64>> = Vec::new();
    let mut dependent = Vec::new();
    for (i, mut row) in rows.into_iter().enumerate() {
        for (k, &c) in pivot_col.iter().enumerate() {
            let f = row[c];
            if f != 0.0 {
                for (x, y) in row.iter_mut().zip(&out[k]) {
                    *x -= f * y;
                }
            }
        }
        let (mut best, mut bv) = (None, PIVOT_TOL);
        for (j, v) in row.iter().enumerate() {
            if v.abs() > bv {
                bv = v.abs();
                best = Some(j);
            }
        }
        let Some(best) = best else {
            dependent.push(i);
            continue;
        };
        let pv = row[best];
        for x in &mut row {
            *x /= pv;
        }
        for other in &mut out {
            let f = other[best];
            if f != 0.0 {
                for (x, y) in other.iter_mut().zip(&row) {
                    *x -= f * y;
                }
            }
        }
        out.push(row);
        pivot_col.push(best);
    }
    debug_assert!(out.iter().all(|r| r.len() == n));
    (pivot_col, out, dependent)
}

impl SketchSystem {
    /// Degrees of freedom and definition state. Call after [`SketchSystem::solve`].
    pub fn analyse(&self, converged: bool) -> Analysis {
        let n = self.x.len();
        let mut determined = vec![false; n];
        let mut rank = 0;
        // (equation index) of rows found redundant, across components.
        let mut bad_eqs: Vec<usize> = self.constant_eqs.clone();
        for c in &self.comps {
            let nc = c.params.len();
            let mut local = std::collections::HashMap::with_capacity(nc);
            for (k, &p) in c.params.iter().enumerate() {
                local.insert(p, k);
            }
            let rows: Vec<Vec<f64>> = c
                .eqs
                .iter()
                .map(|&ei| {
                    let eq = &self.eqs[ei];
                    let mut g = vec![0.0; eq.vars.len()];
                    gradient(&eq.kind, &self.x, &eq.vars, &mut g);
                    let mut row = vec![0.0; nc];
                    for (&v, d) in eq.vars.iter().zip(g) {
                        row[local[&v]] = d;
                    }
                    // Row-normalise so the tolerance is scale independent.
                    let s = row.iter().map(|v| v * v).sum::<f64>().sqrt();
                    if s > 0.0 {
                        for v in &mut row {
                            *v /= s;
                        }
                    }
                    row
                })
                .collect();
            let (pivots, red, dependent) = rref(rows, nc);
            bad_eqs.extend(dependent.into_iter().map(|i| c.eqs[i]));
            rank += pivots.len();
            let mut is_pivot = vec![None; nc];
            for (k, &col) in pivots.iter().enumerate() {
                is_pivot[col] = Some(k);
            }
            let free: Vec<usize> = (0..nc).filter(|&j| is_pivot[j].is_none()).collect();
            for (j, pk) in is_pivot.iter().enumerate() {
                if let Some(k) = *pk {
                    if free.iter().all(|&f| red[k][f].abs() <= FREE_TOL) {
                        determined[c.params[j]] = true;
                    }
                }
            }
        }
        bad_eqs.sort_unstable();
        let mut bad: IndexSet<Id> = IndexSet::new();
        for ei in bad_eqs {
            if let Owner::Item(i) = self.eqs[ei].owner {
                bad.insert(self.owner_ids[i].clone());
            }
        }
        let dof = n - rank;
        if !converged {
            // Every equation still violated is conflicting.
            let x = Values(&self.x);
            for eq in &self.eqs {
                if let Owner::Item(i) = eq.owner {
                    if eq.kind.eval(&x).abs() > 1e-6 {
                        bad.insert(self.owner_ids[i].clone());
                    }
                }
            }
        }
        let mut point_state: IndexMap<Id, DefState> = IndexMap::new();
        for (id, p) in &self.pts {
            let full = match *p {
                Pt::Fixed(_) => true,
                Pt::Var(i) => determined[i] && determined[i + 1],
            };
            point_state.insert(
                id.clone(),
                if full {
                    DefState::Full
                } else {
                    DefState::Under
                },
            );
        }
        let mut entity_state: IndexMap<Id, DefState> = IndexMap::new();
        for (id, e) in &self.sk.entities {
            let mut full = e
                .points()
                .iter()
                .all(|p| point_state.get(*p) == Some(&DefState::Full));
            if let SketchEntity::Circle { .. } = e {
                if let Some(Rad::Var(i)) = self.radii.get(id) {
                    full = full && determined[*i];
                }
            }
            entity_state.insert(
                id.clone(),
                if full {
                    DefState::Full
                } else {
                    DefState::Under
                },
            );
        }
        // Mark geometry touched by bad constraints as over defined.
        if !bad.is_empty() {
            let mut mark = |id: &str| {
                if let Some(s) = entity_state.get_mut(id) {
                    *s = DefState::Over;
                }
                if let Some(s) = point_state.get_mut(id) {
                    *s = DefState::Over;
                }
            };
            for r in self.sk.relations.values() {
                if r.id().is_some_and(|id| bad.contains(id)) {
                    r.refs().into_iter().for_each(&mut mark);
                }
            }
            for d in self.sk.dimensions.values() {
                if bad.contains(&d.id) {
                    d.refs.iter().for_each(|r| mark(r));
                }
            }
        }
        let status = if !converged {
            if bad.is_empty() {
                Status::Unsolved
            } else {
                Status::Over
            }
        } else if !bad.is_empty() {
            Status::Over
        } else if dof == 0 {
            Status::Full
        } else {
            Status::Under
        };
        Analysis {
            dof,
            status,
            point_state,
            entity_state,
            bad,
        }
    }
}
