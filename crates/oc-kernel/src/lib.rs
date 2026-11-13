//! The geometry kernel interface of the native engine (`docs/OVERHAUL_PLAN.md`, E3).
//!
//! [`Kernel`] is what features (`oc-graph`) and naming (`oc-naming`) program against.
//! `oc-kernel-occt` implements it on OCCT; a Parasolid implementation must be able to
//! satisfy it too, so nothing here names a kernel type, and every operation states intent
//! rather than an algorithm.
//!
//! Contract for every implementation:
//! - **Results are validated.** Every operation that returns a body has checked it
//!   ([`Kernel::check`]); an invalid result is healed only if healing provably keeps the
//!   part (see [`Tolerances::heal_acceptance`]), otherwise the call fails with
//!   [`KernelError::InvalidResult`]. A body handed out is always valid.
//! - **History is complete for faces** ([`History`]): every input face has a fate, and
//!   profile segments, caps, edges and vertices are reported where they create faces.
//! - **Deterministic:** the same inputs give the same body with the same entity order
//!   and the same history, whatever the thread count.
//! - **Errors, not panics or aborts**, for anything the kernel reports. (A kernel that
//!   crashes the process is contained by the geometry host, section 1.)
//! - Model units are millimetres; angles are radians.

pub mod error;
pub mod geom;
pub mod history;
pub mod spec;
pub mod tolerance;
pub mod topo;
pub mod validate;

pub use error::{KResult, KernelError, OpKind};
pub use geom::{v3, Axis, Plane, Profile, Region, Segment, Transform, Wire, P3};
pub use history::{CapEnd, Fate, History, Image, Relation, Source};
pub use spec::{
    BooleanOp, ChamferEdge, ChamferSize, DraftSpec, FilletEdge, LoftSpec, MeshParams, PrismSpec, Primitive, Radius,
    RevolveSpec, ShellSpec, SweepOrientation, SweepSpec, Taper,
};
pub use tolerance::Tolerances;
pub use topo::{
    BBox, Check, Curve, Distance, EdgeId, EdgeInfo, FaceId, FaceInfo, MassProps, Mesh, Problem, Surface, Target,
    TopoKind, Topology, VertexId,
};

/// Result of a modelling operation: the new body and how it came from the inputs.
#[derive(Clone, Debug)]
pub struct Op<B> {
    pub body: B,
    pub history: History,
    /// The raw result was invalid and was healed (accepted under
    /// [`Tolerances::heal_acceptance`]); the history already includes the healing.
    pub healed: bool,
}

/// A geometry kernel.
///
/// `Body` is an immutable handle to a solid (or several solids, or for
/// [`Kernel::section`] a set of edges). Cloning it is cheap and shares geometry.
/// Operations never modify their inputs. Bodies need not be `Send`: a kernel runs on its
/// geometry host's thread and parallelises inside its own calls.
pub trait Kernel {
    type Body: Clone;

    /// Name and version, e.g. `"OCCT 8.0.1"`.
    fn name(&self) -> String;
    fn tolerances(&self) -> &Tolerances;
    /// Most threads one call may use for its own parallelism (booleans, meshing); 0 = one
    /// per logical processor. Results never depend on it (determinism contract above).
    /// The setting may be process-wide (OCCT's thread pool is) and may be refused while
    /// another call is running; a geometry host sets it once, before work starts.
    fn set_threads(&self, threads: usize) -> KResult<()>;

    // ---- creation (history sources: profile segments and caps)
    fn primitive(&self, p: &Primitive) -> KResult<Op<Self::Body>>;
    fn prism(&self, s: &PrismSpec) -> KResult<Op<Self::Body>>;
    fn revolve(&self, s: &RevolveSpec) -> KResult<Op<Self::Body>>;
    fn sweep(&self, s: &SweepSpec) -> KResult<Op<Self::Body>>;
    fn loft(&self, s: &LoftSpec) -> KResult<Op<Self::Body>>;

    // ---- modification (history sources: entities of the input bodies)
    /// `target` is history argument 0, `tools[k]` argument `k + 1`. Faces that end up on
    /// one surface are merged ([`Tolerances::unify_angular`]).
    fn boolean(&self, op: BooleanOp, target: &Self::Body, tools: &[&Self::Body]) -> KResult<Op<Self::Body>>;
    /// Fillets propagate along tangent edge chains, as in SOLIDWORKS.
    fn fillet(&self, body: &Self::Body, edges: &[FilletEdge]) -> KResult<Op<Self::Body>>;
    fn chamfer(&self, body: &Self::Body, edges: &[ChamferEdge]) -> KResult<Op<Self::Body>>;
    fn shell(&self, body: &Self::Body, s: &ShellSpec) -> KResult<Op<Self::Body>>;
    fn draft(&self, body: &Self::Body, s: &DraftSpec) -> KResult<Op<Self::Body>>;
    /// Offset every face by `distance` (outwards if positive), rounding convex edges.
    fn offset(&self, body: &Self::Body, distance: f64) -> KResult<Op<Self::Body>>;
    /// Rigid motion or reflection; every face's image is `Modified`.
    fn transform(&self, body: &Self::Body, t: &Transform) -> KResult<Op<Self::Body>>;
    /// Group bodies into one multi-body result without merging them (body `k` is
    /// argument `k`).
    fn compound(&self, bodies: &[&Self::Body]) -> KResult<Op<Self::Body>>;
    /// Intersection curves of the body with a plane, as a body of edges. History images
    /// are edges, generated from the faces they lie on.
    fn section(&self, body: &Self::Body, plane: &Plane) -> KResult<Op<Self::Body>>;
    /// Repair a body (tolerances, orientation, small gaps). Used on imports; results of
    /// modelling operations are healed automatically when needed.
    fn heal(&self, body: &Self::Body) -> KResult<Op<Self::Body>>;
    /// The body's solids as separate bodies, ordered by the lowest index of their faces in
    /// `body`. Each result's history covers every face of `body`: `Same` if the face
    /// belongs to that solid, deleted otherwise. A body without solids gives an empty list.
    fn solids(&self, body: &Self::Body) -> KResult<Vec<Op<Self::Body>>>;

    // ---- queries
    fn topology(&self, body: &Self::Body) -> KResult<Topology>;
    fn bounding_box(&self, body: &Self::Body) -> KResult<BBox>;
    fn mass_properties(&self, body: &Self::Body) -> KResult<MassProps>;
    fn distance(&self, a: Target<'_, Self::Body>, b: Target<'_, Self::Body>) -> KResult<Distance>;
    fn check(&self, body: &Self::Body) -> KResult<Check>;
    /// Triangles per face (faces meshed in parallel) and a polyline per edge. Faces that
    /// share an edge have boundary nodes at exactly (bit for bit) that edge's polyline
    /// points, and edges meeting at a vertex end at exactly the same point, so the mesh
    /// welds into a closed surface without tolerances. The result depends only on the body
    /// and `p`, never on meshes made earlier of this or another body.
    fn tessellate(&self, body: &Self::Body, p: &MeshParams) -> KResult<Mesh>;

    // ---- data exchange
    /// Read every body in a STEP file, healed and validated, as one (multi-body) result.
    /// History is empty.
    fn read_step(&self, data: &[u8]) -> KResult<Op<Self::Body>>;
    /// Write bodies to STEP (AP214, millimetres).
    fn write_step(&self, bodies: &[&Self::Body]) -> KResult<Vec<u8>>;
}
