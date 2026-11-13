//! [`oc_kernel::Kernel`] on Open CASCADE Technology (OCCT) 8.0.1.
//!
//! The C++ shim (`cpp/`) holds every OCCT call; this crate checks arguments with
//! `oc_kernel::validate` (so every kernel rejects the same inputs), converts data, and
//! maps errors. OCCT is linked from the install named by `OCCT_DIR` at build time
//! (`build.rs`, `build-occt.cmd` / `build-occt.sh`); see `docs/THIRD_PARTY.md` for how it
//! is linked and why.
//!
//! [`Body`] is neither `Send` nor `Sync`: OCCT shapes share mutable caches (meshes,
//! tolerances), so a kernel and its bodies live on one geometry-host thread. OCCT
//! parallelises inside booleans and meshing.

mod convert;
// The workspace denies unsafe code; the cxx-generated glue in this one module is the only
// exception (it is the FFI boundary, checked by cxx's static assertions).
#[allow(unsafe_code)]
mod ffi;

use cxx::UniquePtr;
use oc_kernel::validate as v;
use oc_kernel::{
    BBox, BooleanOp, ChamferEdge, ChamferSize, Check, Distance, DraftSpec, FilletEdge, KResult, Kernel, KernelError,
    LoftSpec, MassProps, Mesh, MeshParams, Op, OpKind, Plane, PrismSpec, Primitive, Radius, RevolveSpec, ShellSpec,
    SweepOrientation, SweepSpec, Target, Tolerances, Topology, Transform,
};

/// OCCT version this crate was built against, and whether it is linked statically or as
/// shared libraries.
pub const OCCT_VERSION: &str = env!("OC_OCCT_VERSION");
pub const OCCT_LINKAGE: &str = env!("OC_OCCT_LINKAGE");

/// A body: an immutable OCCT shape with its face/edge/vertex enumeration.
pub struct Body {
    ptr: UniquePtr<ffi::Body>,
    counts: [u32; 3],
}

impl Body {
    fn new(ptr: UniquePtr<ffi::Body>) -> Self {
        let counts = ffi::body_counts(&ptr);
        Body { ptr, counts }
    }
    pub fn face_count(&self) -> usize {
        self.counts[0] as usize
    }
    pub fn edge_count(&self) -> usize {
        self.counts[1] as usize
    }
    pub fn vertex_count(&self) -> usize {
        self.counts[2] as usize
    }
    fn raw(&self) -> &ffi::Body {
        &self.ptr
    }
}

impl Clone for Body {
    fn clone(&self) -> Self {
        Body { ptr: ffi::clone_body(&self.ptr), counts: self.counts }
    }
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Body({} faces, {} edges, {} vertices)", self.counts[0], self.counts[1], self.counts[2])
    }
}

/// The OCCT kernel. Stateless apart from its tolerances.
#[derive(Clone, Debug)]
pub struct OcctKernel {
    tol: Tolerances,
    ffi_tol: ffi::Tol,
}

impl Default for OcctKernel {
    fn default() -> Self {
        Self::new(Tolerances::DEFAULT)
    }
}

fn op(kind: OpKind, r: Result<ffi::OpOut, cxx::Exception>) -> KResult<Op<Body>> {
    let o = r.map_err(|e| convert::error(kind, e))?;
    Ok(Op { history: convert::history(&o.fates, o.image_kind), healed: o.healed, body: Body::new(o.body) })
}

fn list(bodies: &[&Body]) -> UniquePtr<ffi::BodyList> {
    let mut l = ffi::body_list();
    for b in bodies {
        ffi::body_list_push(l.pin_mut(), b.raw());
    }
    l
}

fn plane6(p: &Plane) -> [f64; 6] {
    [p.origin[0], p.origin[1], p.origin[2], p.normal[0], p.normal[1], p.normal[2]]
}

const QUARTER: f64 = std::f64::consts::FRAC_PI_2;

impl OcctKernel {
    pub fn new(tol: Tolerances) -> Self {
        OcctKernel { ffi_tol: convert::tol(&tol), tol }
    }

    fn edge_ids(&self, kind: OpKind, body: &Body, ids: impl Iterator<Item = u32>) -> KResult<()> {
        v::indices(kind, "edge", ids, body.edge_count())
    }
}

impl Kernel for OcctKernel {
    type Body = Body;

    fn name(&self) -> String {
        format!("OCCT {OCCT_VERSION} ({OCCT_LINKAGE})")
    }

    fn tolerances(&self) -> &Tolerances {
        &self.tol
    }

    fn primitive(&self, p: &Primitive) -> KResult<Op<Body>> {
        let k = OpKind::Primitive;
        let (kind, vals): (u8, Vec<f64>) = match p {
            Primitive::Block { min, size } => {
                for s in size {
                    v::positive(k, "block size", *s)?;
                }
                (0, [min.as_slice(), size.as_slice()].concat())
            }
            Primitive::Cylinder { axis, radius, height } => {
                v::direction(k, "cylinder axis", axis.dir)?;
                v::positive(k, "cylinder radius", *radius)?;
                v::positive(k, "cylinder height", *height)?;
                (1, [axis.origin.as_slice(), axis.dir.as_slice(), &[*radius, *height]].concat())
            }
            Primitive::Sphere { center, radius } => {
                v::positive(k, "sphere radius", *radius)?;
                (2, [center.as_slice(), &[*radius]].concat())
            }
        };
        op(k, ffi::primitive(&self.ffi_tol, kind, &vals))
    }

    fn prism(&self, s: &PrismSpec) -> KResult<Op<Body>> {
        let k = OpKind::Prism;
        v::profile(k, &s.profile, &self.tol)?;
        v::direction(k, "extrusion vector", s.vector)?;
        let (angle, outward) = match s.taper {
            Some(t) if t.angle.is_finite() && t.angle > 0.0 && t.angle < QUARTER => (t.angle, t.outward),
            Some(t) => return Err(KernelError::input(k, format!("taper angle {} is not between 0 and 90°", t.angle))),
            None => (0.0, false),
        };
        op(k, ffi::prism(&self.ffi_tol, &convert::regions(&s.profile.regions), &s.vector, angle, outward))
    }

    fn revolve(&self, s: &RevolveSpec) -> KResult<Op<Body>> {
        let k = OpKind::Revolve;
        v::profile(k, &s.profile, &self.tol)?;
        v::direction(k, "axis", s.axis.dir)?;
        v::positive(k, "angle", s.angle)?;
        let axis = [s.axis.origin, s.axis.dir].concat();
        op(k, ffi::revolve(&self.ffi_tol, &convert::regions(&s.profile.regions), &axis, s.angle))
    }

    fn sweep(&self, s: &SweepSpec) -> KResult<Op<Body>> {
        let k = OpKind::Sweep;
        v::profile(k, &s.profile, &self.tol)?;
        v::wire(k, &s.path, self.tol.confusion)?;
        let frenet = s.orientation == SweepOrientation::FollowPath;
        op(k, ffi::sweep(&self.ffi_tol, &convert::regions(&s.profile.regions), &convert::wire(&s.path), frenet))
    }

    fn loft(&self, s: &LoftSpec) -> KResult<Op<Body>> {
        let k = OpKind::Loft;
        if s.sections.len() < 2 {
            return Err(KernelError::input(k, "a loft needs at least two sections"));
        }
        for w in &s.sections {
            v::closed_wire(k, w, self.tol.confusion)?;
        }
        let sections: Vec<ffi::WireIn> = s.sections.iter().map(convert::wire).collect();
        op(k, ffi::loft(&self.ffi_tol, &sections, s.ruled))
    }

    fn boolean(&self, b: BooleanOp, target: &Body, tools: &[&Body]) -> KResult<Op<Body>> {
        if tools.is_empty() {
            return Err(KernelError::input(OpKind::Boolean, "a boolean needs at least one tool body"));
        }
        let kind = match b {
            BooleanOp::Fuse => 0,
            BooleanOp::Cut => 1,
            BooleanOp::Common => 2,
        };
        op(OpKind::Boolean, ffi::boolean(&self.ffi_tol, kind, target.raw(), &list(tools)))
    }

    fn fillet(&self, body: &Body, edges: &[FilletEdge]) -> KResult<Op<Body>> {
        let k = OpKind::Fillet;
        if edges.is_empty() {
            return Err(KernelError::input(k, "no edges to fillet"));
        }
        self.edge_ids(k, body, edges.iter().map(|e| e.edge.0))?;
        let (mut r1, mut r2) = (vec![], vec![]);
        for e in edges {
            let (a, b) = match e.radius {
                Radius::Constant(r) => (r, r),
                Radius::Linear(a, b) => (a, b),
            };
            v::positive(k, "fillet radius", a)?;
            v::positive(k, "fillet radius", b)?;
            r1.push(a);
            r2.push(b);
        }
        let ids: Vec<u32> = edges.iter().map(|e| e.edge.0).collect();
        op(k, ffi::fillet(&self.ffi_tol, body.raw(), &ids, &r1, &r2))
    }

    fn chamfer(&self, body: &Body, edges: &[ChamferEdge]) -> KResult<Op<Body>> {
        let k = OpKind::Chamfer;
        if edges.is_empty() {
            return Err(KernelError::input(k, "no edges to chamfer"));
        }
        self.edge_ids(k, body, edges.iter().map(|e| e.edge.0))?;
        let n = edges.len();
        let (mut kinds, mut d1, mut d2, mut faces) = (Vec::with_capacity(n), vec![], vec![], vec![]);
        for e in edges {
            let (kind, a, b, f) = match e.size {
                ChamferSize::Equal(d) => (0u8, d, d, 0),
                ChamferSize::Distances { d1, d2, face } => (1, d1, d2, face.0),
                ChamferSize::DistanceAngle { distance, angle, face } => {
                    if !(angle > 0.0 && angle < QUARTER) {
                        return Err(KernelError::input(k, format!("chamfer angle {angle} is not between 0 and 90°")));
                    }
                    (2, distance, angle, face.0)
                }
            };
            v::positive(k, "chamfer distance", a)?;
            if kind == 1 {
                v::positive(k, "chamfer distance", b)?;
            }
            if kind != 0 && f as usize >= body.face_count() {
                return Err(KernelError::input(k, format!("face {f} does not exist")));
            }
            kinds.push(kind);
            d1.push(a);
            d2.push(b);
            faces.push(f);
        }
        let ids: Vec<u32> = edges.iter().map(|e| e.edge.0).collect();
        op(k, ffi::chamfer(&self.ffi_tol, body.raw(), &ids, &kinds, &d1, &d2, &faces))
    }

    fn shell(&self, body: &Body, s: &ShellSpec) -> KResult<Op<Body>> {
        let k = OpKind::Shell;
        v::positive(k, "shell thickness", s.thickness)?;
        v::indices(k, "face", s.open.iter().map(|f| f.0), body.face_count())?;
        let off = if s.outward { s.thickness } else { -s.thickness };
        let faces: Vec<u32> = s.open.iter().map(|f| f.0).collect();
        op(k, ffi::shell(&self.ffi_tol, body.raw(), &faces, off))
    }

    fn draft(&self, body: &Body, s: &DraftSpec) -> KResult<Op<Body>> {
        let k = OpKind::Draft;
        if s.faces.is_empty() {
            return Err(KernelError::input(k, "no faces to draft"));
        }
        v::indices(k, "face", s.faces.iter().map(|f| f.0), body.face_count())?;
        v::direction(k, "neutral plane normal", s.neutral.normal)?;
        if !(s.angle.is_finite() && s.angle.abs() < QUARTER && s.angle != 0.0) {
            return Err(KernelError::input(k, format!("draft angle {} is not between 0 and 90°", s.angle)));
        }
        let faces: Vec<u32> = s.faces.iter().map(|f| f.0).collect();
        op(k, ffi::draft(&self.ffi_tol, body.raw(), &faces, &plane6(&s.neutral), s.angle))
    }

    fn offset(&self, body: &Body, distance: f64) -> KResult<Op<Body>> {
        if !(distance.is_finite() && distance.abs() > self.tol.confusion) {
            return Err(KernelError::input(OpKind::Offset, "the offset distance must be non-zero"));
        }
        op(OpKind::Offset, ffi::offset(&self.ffi_tol, body.raw(), distance))
    }

    fn transform(&self, body: &Body, t: &Transform) -> KResult<Op<Body>> {
        let k = OpKind::Transform;
        let m = &t.m;
        let orthogonal = (0..3).all(|i| {
            (0..3).all(|j| {
                let dot: f64 = (0..3).map(|r| m[r][i] * m[r][j]).sum();
                (dot - if i == j { 1.0 } else { 0.0 }).abs() < 1e-9
            })
        });
        if !orthogonal || m.iter().flatten().any(|x| !x.is_finite()) {
            return Err(KernelError::input(k, "the transform is not a rigid motion or reflection"));
        }
        let flat: Vec<f64> = m.iter().flatten().copied().collect();
        op(k, ffi::transform(&self.ffi_tol, body.raw(), &flat))
    }

    fn compound(&self, bodies: &[&Body]) -> KResult<Op<Body>> {
        if bodies.is_empty() {
            return Err(KernelError::input(OpKind::Compound, "nothing to group"));
        }
        op(OpKind::Compound, ffi::compound(&self.ffi_tol, &list(bodies)))
    }

    fn section(&self, body: &Body, plane: &Plane) -> KResult<Op<Body>> {
        v::direction(OpKind::Section, "section plane normal", plane.normal)?;
        op(OpKind::Section, ffi::section(&self.ffi_tol, body.raw(), &plane6(plane)))
    }

    fn heal(&self, body: &Body) -> KResult<Op<Body>> {
        op(OpKind::Heal, ffi::heal(&self.ffi_tol, body.raw()))
    }

    fn solids(&self, body: &Body) -> KResult<Vec<Op<Body>>> {
        (0..ffi::solid_count(body.raw())).map(|i| op(OpKind::Query, ffi::solid(&self.ffi_tol, body.raw(), i))).collect()
    }

    fn topology(&self, body: &Body) -> KResult<Topology> {
        let t = ffi::topology(body.raw()).map_err(|e| convert::error(OpKind::Query, e))?;
        Ok(convert::topology(&t))
    }

    fn bounding_box(&self, body: &Body) -> KResult<BBox> {
        Ok(convert::bbox(ffi::bbox(body.raw()).map_err(|e| convert::error(OpKind::Query, e))?))
    }

    fn mass_properties(&self, body: &Body) -> KResult<MassProps> {
        Ok(convert::mass(&ffi::mass(body.raw()).map_err(|e| convert::error(OpKind::Query, e))?))
    }

    fn distance(&self, a: Target<'_, Body>, b: Target<'_, Body>) -> KResult<Distance> {
        // Points and segments become temporary vertex and edge bodies.
        let pa = point_body(&a)?;
        let pb = point_body(&b)?;
        let (ba, ka, ia) = target(&a, pa.as_ref());
        let (bb, kb, ib) = target(&b, pb.as_ref());
        let d = ffi::distance(ba, ka, ia, bb, kb, ib).map_err(|e| convert::error(OpKind::Query, e))?;
        let p = &d.points;
        Ok(Distance { distance: d.distance, a: [p[0], p[1], p[2]], b: [p[3], p[4], p[5]] })
    }

    fn check(&self, body: &Body) -> KResult<Check> {
        Ok(convert::check(&ffi::check(&self.ffi_tol, body.raw()).map_err(|e| convert::error(OpKind::Query, e))?))
    }

    fn tessellate(&self, body: &Body, p: &MeshParams) -> KResult<Mesh> {
        v::positive(OpKind::Tessellate, "linear deflection", p.linear)?;
        v::positive(OpKind::Tessellate, "angular deflection", p.angular)?;
        let m = ffi::tessellate(body.raw(), p.linear, p.angular).map_err(|e| convert::error(OpKind::Tessellate, e))?;
        Ok(convert::mesh(&m))
    }

    fn read_step(&self, data: &[u8]) -> KResult<Op<Body>> {
        op(OpKind::StepRead, ffi::read_step(&self.ffi_tol, data))
    }

    fn write_step(&self, bodies: &[&Body]) -> KResult<Vec<u8>> {
        if bodies.is_empty() {
            return Err(KernelError::input(OpKind::StepWrite, "nothing to export"));
        }
        ffi::write_step(&list(bodies)).map_err(|e| convert::error(OpKind::StepWrite, e))
    }
}

fn point_body(t: &Target<'_, Body>) -> KResult<Option<UniquePtr<ffi::Body>>> {
    match t {
        Target::Point(p) => Ok(Some(ffi::make_point(p[0], p[1], p[2]))),
        Target::Segment(a, b) => {
            let ab = [*a, *b].concat();
            ffi::make_segment(&ab).map(Some).map_err(|e| convert::error(OpKind::Query, e))
        }
        _ => Ok(None),
    }
}

fn target<'a>(t: &Target<'a, Body>, point: Option<&'a UniquePtr<ffi::Body>>) -> (&'a ffi::Body, u8, u32) {
    match (t, point) {
        (Target::Face(b, f), _) => (b.raw(), 0, f.0),
        (Target::Edge(b, e), _) => (b.raw(), 1, e.0),
        (Target::Vertex(b, v), _) => (b.raw(), 2, v.0),
        (Target::Body(b), _) => (b.raw(), 3, 0),
        (Target::Point(_) | Target::Segment(..), Some(p)) => (p, 3, 0),
        (Target::Point(_) | Target::Segment(..), None) => unreachable!("point and segment bodies are made by point_body"),
    }
}
