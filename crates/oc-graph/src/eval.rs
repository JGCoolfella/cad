//! What one feature produces ([`NodeOut`]) and the context it is evaluated in ([`Ctx`]).

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;

use oc_doc::{EdgeSig, FaceSig, Feature, FeatureKind, SketchData};
use oc_kernel::{BBox, Kernel, KernelError, Topology};
use oc_naming::{BodyNames, Identity, Rules};

use crate::math::{v3, Frame, P3};
use crate::resolve::RefLog;
use crate::sketch::geo::Geo;

/// The message the TypeScript engine shows when the kernel returns an invalid solid.
pub const INVALID: &str = "The geometry kernel produced an invalid solid for this feature. Try different settings.";
/// The message the TypeScript engine shows for an OCCT exception.
pub const FAILED: &str = "The geometry could not be built with these settings";

/// A feature failure: the message shown in the FeatureManager tree, plus kernel detail
/// for logs.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureError {
    pub message: String,
    pub detail: Option<String>,
}

impl fmt::Display for FeatureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FeatureError {}

impl From<&str> for FeatureError {
    fn from(s: &str) -> Self {
        FeatureError { message: s.into(), detail: None }
    }
}

impl From<String> for FeatureError {
    fn from(message: String) -> Self {
        FeatureError { message, detail: None }
    }
}

impl From<KernelError> for FeatureError {
    fn from(e: KernelError) -> Self {
        let message = match e {
            KernelError::InvalidResult { .. } => INVALID,
            _ => FAILED,
        };
        FeatureError { message: message.into(), detail: Some(e.to_string()) }
    }
}

pub type FResult<T> = Result<T, FeatureError>;

/// A body with the persistent name of each of its faces, and its topology and box
/// computed on first use.
pub struct Shape<B> {
    pub body: B,
    /// Aligned with the kernel's face order for `body`.
    pub names: BodyNames,
    topo: OnceCell<Topology>,
    bbox: OnceCell<BBox>,
}

impl<B> Shape<B> {
    pub fn new(body: B, names: BodyNames) -> Rc<Self> {
        Self::with_topology(body, names, OnceCell::new())
    }

    /// With a topology that may already have been computed (while naming the body).
    pub(crate) fn with_topology(body: B, names: BodyNames, topo: OnceCell<Topology>) -> Rc<Self> {
        Rc::new(Shape { body, names, topo, bbox: OnceCell::new() })
    }

    pub fn topo<K: Kernel<Body = B>>(&self, k: &K) -> FResult<&Topology> {
        if self.topo.get().is_none() {
            let _ = self.topo.set(k.topology(&self.body)?);
        }
        Ok(self.topo.get().expect("set above"))
    }

    pub fn bbox<K: Kernel<Body = B>>(&self, k: &K) -> FResult<BBox> {
        if self.bbox.get().is_none() {
            let _ = self.bbox.set(k.bounding_box(&self.body)?);
        }
        Ok(*self.bbox.get().expect("set above"))
    }
}

/// A point and unit direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisDef {
    pub p: P3,
    pub d: P3,
}

/// Body a feature added (boss) or removed (cut): patterns and mirrors repeat these.
pub struct Tool<B> {
    pub shape: Rc<Shape<B>>,
    pub cut: bool,
}

impl<B> Clone for Tool<B> {
    fn clone(&self) -> Self {
        Tool { shape: self.shape.clone(), cut: self.cut }
    }
}

/// The fillet or chamfer parameters of a dress-up.
#[derive(Clone, Debug, PartialEq)]
pub enum DressKind {
    Fillet(oc_doc::FilletFeature),
    Chamfer(oc_doc::ChamferFeature),
}

/// A fillet or chamfer: its stored references, or, as applied, the edges it was applied to
/// in world position, so patterns and mirrors can re-apply it to the matching edges of
/// each instance.
#[derive(Clone, Debug, PartialEq)]
pub struct Dressup {
    pub kind: DressKind,
    pub edges: Vec<EdgeSig>,
    pub faces: Vec<FaceSig>,
    /// A pattern instance: `edges` and `faces` are positions computed by the pattern, not
    /// stored references, and its edges must exist exactly where the transform puts them.
    pub exact: bool,
}

/// One pattern instance marker (`PatternInstance`).
#[derive(Clone, Debug, PartialEq)]
pub struct Instance {
    /// `"i,j"` for linear patterns, `"i"` for circular ones (1-based; the seed is not listed).
    pub key: String,
    pub p: P3,
    pub skipped: bool,
}

/// Everything one feature produces besides its status.
pub struct NodeOut<B> {
    /// Sketch plane.
    pub frame: Option<Frame>,
    /// Sketch re-solved against the model (only for sketches with model references).
    pub sketch: Option<Arc<SketchData>>,
    /// Reference plane (display size 60, as in the TypeScript engine).
    pub plane: Option<Frame>,
    pub axis: Option<AxisDef>,
    pub point: Option<P3>,
    pub csys: Option<Frame>,
    pub tools: Option<Vec<Tool<B>>>,
    pub dress: Option<Vec<Dressup>>,
    pub instances: Option<Vec<Instance>>,
    /// The body an extrude/revolve/... tool or pattern added (for previews).
    pub preview: Option<B>,
    /// The part body after this feature, when the feature changed it.
    pub body: Option<Rc<Shape<B>>>,
    pub warning: Option<String>,
}

impl<B> Default for NodeOut<B> {
    fn default() -> Self {
        NodeOut {
            frame: None,
            sketch: None,
            plane: None,
            axis: None,
            point: None,
            csys: None,
            tools: None,
            dress: None,
            instances: None,
            preview: None,
            body: None,
            warning: None,
        }
    }
}

/// The context of one feature evaluation: the kernel, the outputs of earlier features
/// and the body as it stands before this feature.
pub struct Ctx<'a, K: Kernel> {
    pub k: &'a K,
    /// Limits for finding stored references and for naming by surface identity.
    pub rules: Rules,
    pub identity: Identity,
    pub features: &'a [Arc<Feature>],
    pub feature: &'a Feature,
    outs: &'a HashMap<String, Rc<NodeOut<K::Body>>>,
    body: RefCell<Option<Rc<Shape<K::Body>>>>,
    changed: std::cell::Cell<bool>,
    /// Ids of earlier features whose outputs were read (checked against the declared
    /// dependencies in debug builds), and whether the body was read.
    pub(crate) reads: RefCell<Vec<String>>,
    pub(crate) read_body: std::cell::Cell<bool>,
    /// What resolving references and naming results turned up (names to write back,
    /// warnings, faces without history).
    pub(crate) log: RefCell<RefLog>,
}

impl<'a, K: Kernel> Ctx<'a, K> {
    pub(crate) fn new(
        k: &'a K,
        rules: Rules,
        features: &'a [Arc<Feature>],
        feature: &'a Feature,
        outs: &'a HashMap<String, Rc<NodeOut<K::Body>>>,
        body: Option<Rc<Shape<K::Body>>>,
    ) -> Self {
        Ctx {
            k,
            rules,
            identity: Identity::default(),
            features,
            feature,
            outs,
            body: RefCell::new(body),
            changed: false.into(),
            reads: RefCell::new(vec![]),
            read_body: false.into(),
            log: RefCell::new(RefLog::default()),
        }
    }

    /// Output of an earlier, evaluated feature that did not fail.
    pub fn node(&self, id: &str) -> Option<&'a NodeOut<K::Body>> {
        self.reads.borrow_mut().push(id.to_string());
        self.outs.get(id).map(|r| &**r)
    }

    /// The body as it stands (before this feature, until [`Ctx::set_body`]).
    pub fn shape(&self) -> Option<Rc<Shape<K::Body>>> {
        self.read_body.set(true);
        self.body.borrow().clone()
    }

    /// Replace the body (the feature's result, or an intermediate step of it).
    pub fn set_body(&self, b: Rc<Shape<K::Body>>) {
        *self.body.borrow_mut() = Some(b);
        self.changed.set(true);
    }

    /// Id of the feature being evaluated: the `<fid>` of the tags it gives its faces.
    pub fn fid(&self) -> &'a str {
        self.feature.id.as_str()
    }

    /// The body after this feature, if the feature changed it.
    pub(crate) fn result_body(&self) -> Option<Rc<Shape<K::Body>>> {
        if self.changed.get() {
            self.body.borrow().clone()
        } else {
            None
        }
    }

    pub fn topo<'b>(&self, s: &'b Shape<K::Body>) -> FResult<&'b Topology> {
        s.topo(self.k)
    }

    /// Sketch plane of sketch `id` (`st.frames[id]`).
    pub fn frame(&self, id: &str) -> Option<Frame> {
        self.node(id).and_then(|n| n.frame)
    }

    /// A sketch as this feature sees it: re-solved against the model when it has model
    /// references (`sketchOf`). Any feature of the tree is found, as in TypeScript; its
    /// plane is resolved only if it comes earlier ([`Ctx::frame`]).
    pub fn sketch(&self, id: &str) -> FResult<(&'a Feature, Geo)> {
        let f = self.features.iter().find(|f| f.id.as_str() == id).ok_or("Sketch not found")?;
        let FeatureKind::Sketch(s) = &f.kind else { return Err("Sketch not found".into()) };
        let resolved = self.node(id).and_then(|n| n.sketch.as_deref());
        Ok((f, Geo::new(resolved.unwrap_or(&s.sketch))))
    }

    /// Name of a feature anywhere in the tree.
    pub fn name_of(&self, id: &str) -> Option<&'a str> {
        self.features.iter().find(|f| f.id.as_str() == id).map(|f| f.name.as_str())
    }

    /// Position of a feature in the tree.
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.features.iter().position(|f| f.id.as_str() == id)
    }

    /// Length that surely runs through the body (`bigDepth`).
    pub fn big_depth(&self, frame: &Frame) -> FResult<f64> {
        let Some(s) = self.shape() else { return Ok(1000.0) };
        let b = s.bbox(self.k)?;
        let c = v3::mid(b.min, b.max);
        Ok(b.diagonal() * 2.0 + v3::dot(v3::sub(c, frame.origin), frame.normal).abs() + 1.0)
    }
}
