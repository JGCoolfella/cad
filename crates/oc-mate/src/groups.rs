//! Solving per connected group of bodies (G3c). Floating bodies joined by a chain of enabled
//! mates form a group; fixed bodies and the assembly itself never move, so they join no
//! group and are shared as constants. Each group is solved, analysed (and dragged) as a
//! problem of its own, with the fixed bodies its mates refer to, and the results are put back
//! in place. Mates with no floating body (between fixed bodies or the assembly) are one more
//! problem. The results are those of solving everything together (`tests/groups.rs`); the
//! time grows with the number of groups instead of its square (G1a's note: the conflict
//! search re-solves and the drag's null space is dense).
use crate::{BodyInfo, BodyState, MateState, Owner, Problem, Solved};

/// The groups of a problem: for each, its bodies (floating ones first, then the fixed ones
/// its mates refer to, each ascending) and its mates (ascending). Mates that are disabled or
/// name a missing body are not in any group: their state is known without solving.
pub(crate) struct Split {
    pub groups: Vec<(Vec<usize>, Vec<usize>)>,
    pub states: Vec<Option<MateState>>,
}

fn find(p: &mut [usize], mut i: usize) -> usize {
    while p[i] != i {
        p[i] = p[p[i]];
        i = p[i];
    }
    i
}

pub(crate) fn split(p: &Problem) -> Split {
    let n = p.bodies.len();
    let floating = |o: Owner| match o {
        Owner::Body(b) if b < n && !p.bodies[b].fixed => Some(b),
        _ => None,
    };
    let mut states: Vec<Option<MateState>> = vec![None; p.mates.len()];
    let mut parent: Vec<usize> = (0..n).collect();
    for (i, m) in p.mates.iter().enumerate() {
        if !m.enabled {
            states[i] = Some(MateState::Off);
            continue;
        }
        if [m.a.owner, m.b.owner].iter().any(|o| matches!(o, Owner::Body(b) if *b >= n)) {
            states[i] = Some(MateState::Invalid("refers to a missing component".into()));
            continue;
        }
        if let (Some(a), Some(b)) = (floating(m.a.owner), floating(m.b.owner)) {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
    }
    // Group of each root, in order of the lowest floating body; `n` is the constants' key.
    let mut index: Vec<usize> = vec![usize::MAX; n + 1];
    let mut groups: Vec<(Vec<usize>, Vec<usize>)> = vec![];
    for b in 0..n {
        if p.bodies[b].fixed {
            continue;
        }
        let r = find(&mut parent, b);
        if index[r] == usize::MAX {
            index[r] = groups.len();
            groups.push((vec![], vec![]));
        }
        groups[index[r]].0.push(b);
    }
    for (i, m) in p.mates.iter().enumerate() {
        if states[i].is_some() {
            continue;
        }
        let key = match floating(m.a.owner).or(floating(m.b.owner)) {
            Some(b) => find(&mut parent, b),
            None => n,
        };
        if index[key] == usize::MAX {
            index[key] = groups.len();
            groups.push((vec![], vec![]));
        }
        groups[index[key]].1.push(i);
    }
    // The fixed bodies each group's mates refer to.
    for (bodies, mates) in &mut groups {
        let mut fixed: Vec<usize> = mates
            .iter()
            .flat_map(|&i| [p.mates[i].a.owner, p.mates[i].b.owner])
            .filter_map(|o| match o {
                Owner::Body(b) if p.bodies[b].fixed => Some(b),
                _ => None,
            })
            .collect();
        fixed.sort_unstable();
        fixed.dedup();
        bodies.extend(fixed);
    }
    Split { groups, states }
}

/// The problem of one group: its bodies and mates, owners renumbered.
pub(crate) fn sub(p: &Problem, bodies: &[usize], mates: &[usize]) -> Problem {
    let at: std::collections::HashMap<usize, usize> = bodies.iter().enumerate().map(|(k, &b)| (b, k)).collect();
    let local = |o: Owner| match o {
        Owner::Body(b) => Owner::Body(at[&b]),
        Owner::Ground => Owner::Ground,
    };
    Problem {
        bodies: bodies.iter().map(|&b| p.bodies[b]).collect(),
        mates: mates
            .iter()
            .map(|&i| {
                let mut m = p.mates[i];
                m.a.owner = local(m.a.owner);
                m.b.owner = local(m.b.owner);
                m
            })
            .collect(),
    }
}

/// Solves every group with `each` (the group's bodies, its problem; `None`: nothing to
/// solve) and puts the results in place.
pub(crate) fn solve_by(p: &Problem, mut each: impl FnMut(&[usize], &Problem) -> Option<Solved>) -> Solved {
    let s = split(p);
    let mut out = Solved {
        poses: p.bodies.iter().map(|b| b.pose).collect(),
        converged: true,
        dof: 0,
        bodies: p.bodies.iter().map(|b| BodyInfo { state: if b.fixed { BodyState::Fixed } else { BodyState::Under }, free: if b.fixed { 0 } else { 6 } }).collect(),
        mates: s.states.iter().map(|x| x.clone().unwrap_or(MateState::Ok)).collect(),
        newest_error: None,
    };
    for (bodies, mates) in &s.groups {
        let floating = bodies.iter().filter(|&&b| !p.bodies[b].fixed).count();
        let q = sub(p, bodies, mates);
        let Some(r) = each(bodies, &q) else {
            // A floating body with no mates: free, where it is.
            out.dof += 6 * floating;
            continue;
        };
        out.converged &= r.converged;
        out.dof += r.dof;
        for (k, &b) in bodies.iter().enumerate() {
            if !p.bodies[b].fixed {
                out.poses[b] = r.poses[k];
                out.bodies[b] = r.bodies[k].clone();
            }
        }
        for (k, &i) in mates.iter().enumerate() {
            out.mates[i] = r.mates[k].clone();
        }
    }
    out.newest_error = (0..out.mates.len()).rev().find(|&m| matches!(out.mates[m], MateState::Redundant | MateState::Conflict));
    out
}
