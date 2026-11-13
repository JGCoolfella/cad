//! Kernel operations with names: every body a feature makes goes through here, so its
//! faces are named from the operation's history (`oc_naming::track`,
//! `docs/OVERHAUL_PLAN.md` section 3.2) the moment it exists. Features never call a
//! modelling operation on the kernel directly.

use std::cell::OnceCell;
use std::rc::Rc;

use oc_kernel::{
    BooleanOp, ChamferEdge, DraftSpec, FilletEdge, Kernel, LoftSpec, Op, PrismSpec, RevolveSpec, ShellSpec, Source, SweepSpec, Topology, Transform,
};
use oc_naming::{import_names, track, Births, Input, Tag, Topo};

use crate::eval::{Ctx, FResult, Shape};

/// Explicit tags for the faces an operation makes from profile segments and caps.
pub type SegmentTags = [(Source, Tag)];

/// A shape's topology, computed when naming asks for it.
struct Lazy<'s, K: Kernel> {
    k: &'s K,
    shape: &'s Shape<K::Body>,
}

impl<K: Kernel> Topo for Lazy<'_, K> {
    fn topology(&self) -> Option<&Topology> {
        self.shape.topo(self.k).ok()
    }
}

/// The topology of a result that is not a [`Shape`] yet.
struct Fresh<'s, K: Kernel> {
    k: &'s K,
    body: &'s K::Body,
    cell: &'s OnceCell<Topology>,
}

impl<K: Kernel> Topo for Fresh<'_, K> {
    fn topology(&self) -> Option<&Topology> {
        if self.cell.get().is_none() {
            let _ = self.cell.set(self.k.topology(self.body).ok()?);
        }
        self.cell.get()
    }
}

impl<'a, K: Kernel> Ctx<'a, K> {
    /// Name the result of `op` from its history and the names of its inputs (history
    /// argument `k` is `inputs[k]`).
    fn named(&self, op: Op<K::Body>, births: &Births<'_>, inputs: &[&Shape<K::Body>]) -> Rc<Shape<K::Body>> {
        let lazy: Vec<Lazy<'_, K>> = inputs.iter().map(|s| Lazy { k: self.k, shape: s }).collect();
        let ins: Vec<Input<'_>> = inputs.iter().zip(&lazy).map(|(s, l)| Input { names: &s.names, topo: l }).collect();
        let cell = OnceCell::new();
        let tracked = track(births, &op.history, &ins, &Fresh { k: self.k, body: &op.body, cell: &cell }, &self.identity);
        self.log.borrow_mut().untracked += tracked.untracked;
        Shape::with_topology(op.body, tracked.names, cell)
    }

    fn created(&self, op: Op<K::Body>, tags: &SegmentTags) -> Rc<Shape<K::Body>> {
        self.named(op, &Births { explicit: tags, ..Births::of(self.fid()) }, &[])
    }

    // ---- creation: the feature says which sketch entity each segment and cap is

    pub fn prism(&self, spec: &PrismSpec, tags: &SegmentTags) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.created(self.k.prism(spec)?, tags))
    }

    pub fn revolve(&self, spec: &RevolveSpec, tags: &SegmentTags) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.created(self.k.revolve(spec)?, tags))
    }

    pub fn sweep(&self, spec: &SweepSpec, tags: &SegmentTags) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.created(self.k.sweep(spec)?, tags))
    }

    pub fn loft(&self, spec: &LoftSpec, tags: &SegmentTags) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.created(self.k.loft(spec)?, tags))
    }

    /// A body read from STEP data. It has no history: its faces are named by geometric
    /// rank (`<fid>/import/<rank>`).
    pub fn import(&self, data: &[u8]) -> Result<Rc<Shape<K::Body>>, oc_kernel::KernelError> {
        let op = self.k.read_step(data)?;
        let topo = self.k.topology(&op.body)?;
        let names = import_names(self.fid(), &topo);
        Ok(Shape::with_topology(op.body, names, OnceCell::from(topo)))
    }

    // ---- modification: names follow the history

    /// `target` and `tools` keep their names; split pieces share a name, merged faces
    /// union theirs.
    pub fn boolean(&self, op: BooleanOp, target: &Shape<K::Body>, tools: &[&Shape<K::Body>]) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.try_boolean(op, target, tools)?)
    }

    /// [`Ctx::boolean`] with the kernel's own error, for callers that treat an empty
    /// result as an answer rather than a failure.
    pub fn try_boolean(&self, op: BooleanOp, target: &Shape<K::Body>, tools: &[&Shape<K::Body>]) -> Result<Rc<Shape<K::Body>>, oc_kernel::KernelError> {
        let bodies: Vec<&K::Body> = tools.iter().map(|t| &t.body).collect();
        let r = self.k.boolean(op, &target.body, &bodies)?;
        let mut inputs = vec![target];
        inputs.extend_from_slice(tools);
        Ok(self.named(r, &Births::of(self.fid()), &inputs))
    }

    pub fn fuse(&self, a: &Shape<K::Body>, b: &Shape<K::Body>) -> FResult<Rc<Shape<K::Body>>> {
        self.boolean(BooleanOp::Fuse, a, &[b])
    }

    pub fn cut(&self, a: &Shape<K::Body>, b: &Shape<K::Body>) -> FResult<Rc<Shape<K::Body>>> {
        self.boolean(BooleanOp::Cut, a, &[b])
    }

    /// Fuse bodies in order (`a.fuse(b).fuse(c)...`).
    pub fn fuse_all(&self, bodies: Vec<Rc<Shape<K::Body>>>) -> FResult<Option<Rc<Shape<K::Body>>>> {
        let mut acc: Option<Rc<Shape<K::Body>>> = None;
        for b in bodies {
            acc = Some(match acc {
                None => b,
                Some(a) => self.fuse(&a, &b)?,
            });
        }
        Ok(acc)
    }

    /// Combine a tool with the body: cut, fuse (merge) or keep as a separate body.
    pub fn combine(&self, tool: &Rc<Shape<K::Body>>, cut: bool, merge: bool) -> FResult<Rc<Shape<K::Body>>> {
        match self.shape() {
            None if cut => Err("There is no body to cut".into()),
            Some(b) if cut => self.cut(&b, tool),
            None => Ok(tool.clone()),
            Some(b) if merge => self.fuse(&b, tool),
            Some(b) => {
                let r = self.k.compound(&[&b.body, &tool.body])?;
                Ok(self.named(r, &Births::of(self.fid()), &[&b, tool]))
            }
        }
    }

    /// Each fillet face is `<fid>/fillet/<key of its edge's name>`, a corner patch
    /// `<fid>/corner/<key of its vertex's name>`.
    pub fn fillet(&self, s: &Shape<K::Body>, edges: &[FilletEdge]) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.named(self.k.fillet(&s.body, edges)?, &Births { edge_role: "fillet", ..Births::of(self.fid()) }, &[s]))
    }

    pub fn chamfer(&self, s: &Shape<K::Body>, edges: &[ChamferEdge]) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.named(self.k.chamfer(&s.body, edges)?, &Births { edge_role: "chamfer", ..Births::of(self.fid()) }, &[s]))
    }

    /// Every face keeps its name. The wall a face gains on the other side of the material
    /// is `<fid>/offset/<key of the face's name>`; an open face becomes the rim and keeps
    /// its name; rounded outer edges and corners are named from their edge or vertex.
    pub fn shell(&self, s: &Shape<K::Body>, spec: &ShellSpec) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.named(self.k.shell(&s.body, spec)?, &Births { face_role: Some("offset"), ..Births::of(self.fid()) }, &[s]))
    }

    /// A drafted face is still that face: it keeps its name.
    pub fn draft(&self, s: &Shape<K::Body>, spec: &DraftSpec) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.named(self.k.draft(&s.body, spec)?, &Births::of(self.fid()), &[s]))
    }

    /// A moved or mirrored copy. `instance` makes it instance `n` of this pattern or
    /// mirror: every tag `t` becomes `<fid>/i<n>/t`.
    pub fn transform(&self, s: &Shape<K::Body>, t: &Transform, instance: Option<&str>) -> FResult<Rc<Shape<K::Body>>> {
        Ok(self.named(self.k.transform(&s.body, t)?, &Births { instance, ..Births::of(self.fid()) }, &[s]))
    }

    /// The body's solids as separate shapes; faces keep their names.
    pub fn solids(&self, s: &Shape<K::Body>) -> FResult<Vec<Rc<Shape<K::Body>>>> {
        Ok(self.k.solids(&s.body)?.into_iter().map(|op| self.named(op, &Births::of(self.fid()), &[s])).collect())
    }
}
