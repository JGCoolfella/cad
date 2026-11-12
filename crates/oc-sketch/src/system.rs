//! [`SketchSystem`]: parameters, equations and their independent components.
//!
//! Unknowns are free point coordinates and circle radii; fixed points (origin, Fix, On Edge,
//! external references) and fixed radii are constants. Equations are partitioned into
//! connected components (two equations are connected when they read a common parameter).
//! The components are independent least-squares problems: solving, dragging and rank
//! analysis act on each separately, so their cost follows the size of the component being
//! worked on, not of the sketch.
//!
//! Equations that read no free parameter ("constant" equations, e.g. a coincidence between
//! two fixed points) cannot be changed by the solver. They count towards convergence and are
//! reported as redundant by the analysis, as zero Jacobian rows are in TypeScript.

use crate::build::{build, Equation, Owner, ParamOwner, Resolver};
use crate::equations::{gradient, Pt, Rad, Values};
use crate::model::{Dimension, Id, SketchData, SketchEntity};
use crate::sparse::{SparseRows, Symbolic};
use indexmap::IndexMap;
use std::collections::HashSet;

#[derive(Clone)]
pub(crate) struct Component {
    /// Global parameter indices, ascending.
    pub params: Vec<usize>,
    /// Global equation indices, in creation order.
    pub eqs: Vec<usize>,
    /// Structure of the component's Jacobian (local columns); values are refreshed per use.
    pub jac: SparseRows,
    pub sym: Option<Symbolic>,
    /// Squared residual norm at the current parameters, while they are unchanged.
    pub clean_err: Option<f64>,
}

#[derive(Clone)]
pub struct SketchSystem {
    pub(crate) sk: SketchData,
    pub(crate) x: Vec<f64>,
    pub(crate) owners: Vec<ParamOwner>,
    pub(crate) pts: IndexMap<Id, Pt>,
    pub(crate) radii: IndexMap<Id, Rad>,
    pub(crate) eqs: Vec<Equation>,
    pub(crate) owner_ids: Vec<Id>,
    pub(crate) comps: Vec<Component>,
    /// Component of each parameter (`None`: read by no equation).
    pub(crate) param_comp: Vec<Option<usize>>,
    /// Equations reading no free parameter.
    pub(crate) constant_eqs: Vec<usize>,
}

impl SketchSystem {
    /// Builds the system for a sketch (`new SketchSystem(sk)`).
    pub fn new(sk: &SketchData) -> Self {
        Self::with_fixed(sk, &HashSet::new())
    }

    /// Builds the system with extra points held fixed (`new SketchSystem(sk, extraFixed)`).
    pub fn with_fixed(sk: &SketchData, extra_fixed: &HashSet<Id>) -> Self {
        let b = build(sk, extra_fixed);
        let n = b.x.len();
        // Union-find over parameters.
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        for eq in &b.eqs {
            if let Some((&first, rest)) = eq.vars.split_first() {
                let a = find(&mut parent, first);
                for &v in rest {
                    let r = find(&mut parent, v);
                    if r != a {
                        // Lower index as root keeps component numbering deterministic.
                        let (lo, hi) = if r < a { (r, a) } else { (a, r) };
                        parent[hi] = lo;
                    }
                }
            }
        }
        let mut root_comp: Vec<Option<usize>> = vec![None; n];
        let mut comps: Vec<Component> = Vec::new();
        let mut param_comp: Vec<Option<usize>> = vec![None; n];
        let mut constant_eqs = Vec::new();
        for (ei, eq) in b.eqs.iter().enumerate() {
            let Some(&v) = eq.vars.first() else {
                constant_eqs.push(ei);
                continue;
            };
            let root = find(&mut parent, v);
            let ci = *root_comp[root].get_or_insert_with(|| {
                comps.push(Component {
                    params: Vec::new(),
                    eqs: Vec::new(),
                    jac: SparseRows::default(),
                    sym: None,
                    clean_err: None,
                });
                comps.len() - 1
            });
            comps[ci].eqs.push(ei);
        }
        for (p, pc) in param_comp.iter_mut().enumerate() {
            let root = find(&mut parent, p);
            if let Some(ci) = root_comp[root] {
                *pc = Some(ci);
                comps[ci].params.push(p);
            }
        }
        let mut local = vec![usize::MAX; n];
        for c in &mut comps {
            for (k, &p) in c.params.iter().enumerate() {
                local[p] = k;
            }
            let mut j = SparseRows {
                ptr: vec![0],
                ncols: c.params.len(),
                ..Default::default()
            };
            for &ei in &c.eqs {
                j.col.extend(b.eqs[ei].vars.iter().map(|&v| local[v]));
                j.ptr.push(j.col.len());
            }
            j.val = vec![0.0; j.col.len()];
            c.jac = j;
        }
        Self {
            sk: sk.clone(),
            x: b.x,
            owners: b.owners,
            pts: b.pts,
            radii: b.radii,
            eqs: b.eqs,
            owner_ids: b.owner_ids,
            comps,
            param_comp,
            constant_eqs,
        }
    }

    // ------------------------------------------------------------------ accessors

    /// Parameter values: free point coordinates (x then y, in point order), then free radii.
    pub fn x(&self) -> &[f64] {
        &self.x
    }

    /// Replaces the parameter values.
    pub fn set_x(&mut self, x: &[f64]) {
        assert_eq!(x.len(), self.x.len(), "parameter count");
        self.x.copy_from_slice(x);
        for c in &mut self.comps {
            c.clean_err = None;
        }
    }

    /// Every point's parameters or constant value (`pts`).
    pub fn points(&self) -> &IndexMap<Id, Pt> {
        &self.pts
    }

    /// Every circle's radius parameter or constant value (`radii`).
    pub fn radii(&self) -> &IndexMap<Id, Rad> {
        &self.radii
    }

    pub fn param_owners(&self) -> &[ParamOwner] {
        &self.owners
    }

    pub fn equation_count(&self) -> usize {
        self.eqs.len()
    }

    /// Owner id of each equation (`"internal"` for internal ones).
    pub fn equation_owners(&self) -> Vec<&str> {
        self.eqs.iter().map(|e| self.owner_name(e.owner)).collect()
    }

    pub(crate) fn owner_name(&self, o: Owner) -> &str {
        match o {
            Owner::Internal => "internal",
            Owner::Item(i) => &self.owner_ids[i],
        }
    }

    /// Sizes (parameters, equations) of the independent components, largest first.
    pub fn component_sizes(&self) -> Vec<(usize, usize)> {
        let mut s: Vec<_> = self
            .comps
            .iter()
            .map(|c| (c.params.len(), c.eqs.len()))
            .collect();
        s.sort_unstable_by(|a, b| b.cmp(a));
        s
    }

    /// Current value of a point.
    pub fn point_value(&self, id: &str) -> Option<[f64; 2]> {
        self.pts.get(id).map(|p| match *p {
            Pt::Var(i) => [self.x[i], self.x[i + 1]],
            Pt::Fixed(c) => c,
        })
    }

    /// Residual of every equation, in creation order.
    pub fn residuals(&self) -> Vec<f64> {
        self.eqs
            .iter()
            .map(|e| e.kind.eval(&Values(&self.x)))
            .collect()
    }

    /// Dense Jacobian (rows = equations), exact (forward-mode dual numbers).
    pub fn jacobian(&self) -> Vec<Vec<f64>> {
        self.eqs
            .iter()
            .map(|e| {
                let mut row = vec![0.0; self.x.len()];
                let mut g = vec![0.0; e.vars.len()];
                gradient(&e.kind, &self.x, &e.vars, &mut g);
                for (&v, d) in e.vars.iter().zip(g) {
                    row[v] = d;
                }
                row
            })
            .collect()
    }

    /// Current value of what a dimension measures (`measureFn(d)()`), `None` if it measures
    /// nothing.
    pub fn measure(&self, d: &Dimension) -> Option<f64> {
        let r = Resolver {
            sk: &self.sk,
            pts: &self.pts,
            radii: &self.radii,
        };
        let m = r.measure(d).ok().flatten()?;
        Some(m.eval(&Values(&self.x)))
    }

    /// Writes solved values into `sk` (the sketch this system was built from): free points,
    /// free radii, and driven dimensions' measured values.
    pub fn commit(&self, sk: &mut SketchData) {
        for (id, p) in &self.pts {
            if let (Pt::Var(i), Some(pt)) = (p, sk.points.get_mut(id)) {
                pt.x = self.x[*i];
                pt.y = self.x[*i + 1];
            }
        }
        for (id, r) in &self.radii {
            if let (Rad::Var(i), Some(SketchEntity::Circle { r, .. })) =
                (r, sk.entities.get_mut(id))
            {
                *r = self.x[*i];
            }
        }
        for d in sk.dimensions.values_mut() {
            if d.driven != Some(true) {
                continue;
            }
            if let Some(v) = self.measure(d) {
                d.value = v.abs();
            }
        }
    }

    // ------------------------------------------------------------------ numerics

    pub(crate) fn comp_residuals(&self, ci: usize) -> Vec<f64> {
        let x = Values(&self.x);
        self.comps[ci]
            .eqs
            .iter()
            .map(|&e| self.eqs[e].kind.eval(&x))
            .collect()
    }

    /// Refreshes the component's Jacobian values at the current parameters.
    pub(crate) fn comp_jacobian(&mut self, ci: usize) {
        let c = &mut self.comps[ci];
        for (row, &ei) in c.eqs.iter().enumerate() {
            let eq = &self.eqs[ei];
            gradient(&eq.kind, &self.x, &eq.vars, c.jac.row_mut(row));
        }
        if c.sym.is_none() {
            c.sym = Some(Symbolic::new(&c.jac));
        }
    }

    pub(crate) fn constant_err(&self) -> f64 {
        let x = Values(&self.x);
        self.constant_eqs
            .iter()
            .map(|&e| self.eqs[e].kind.eval(&x).powi(2))
            .sum()
    }
}

pub(crate) fn norm2(r: &[f64]) -> f64 {
    r.iter().map(|v| v * v).sum()
}
