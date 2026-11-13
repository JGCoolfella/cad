//! Finding stored references on the body as it stands (`docs/OVERHAUL_PLAN.md` section
//! 3.3), through `oc-naming`: by persistent name; among split pieces by signature; by
//! position, with a warning, when the name is gone; else the reference is missing.
//!
//! A legacy reference (no name: a version 1 document) is found by signature the way the
//! TypeScript engine found it, and the name it resolved to is reported so the caller can
//! write it into the document ([`Heal`], [`apply_heals`]) without an undo step
//! (`oc_doc::History::heal`). From then on the reference follows the model by name.
//!
//! Features use only what a reference resolves to on the live body. Nothing reads
//! geometry out of a stored signature.

use std::rc::Rc;
use std::sync::Arc;

use oc_doc::{EdgeSig, FaceSig, Feature, PartDocument, RefSlot};
use oc_kernel::{EdgeId, FaceId, Kernel};
use oc_naming::resolve as naming;
use oc_naming::{How, Live};

use crate::eval::{Ctx, FResult, FeatureError, Shape};
use crate::math::P3;

pub use oc_naming::signature::{curve_name, edge_sig, face_sig, surface_name};

/// The error of a feature whose reference no longer exists. Shown in the tree; the
/// feature's selection box shows `**Missing**` for re-picking (wording 🔍, FEATURE_SPEC).
pub fn missing(what: &str) -> FeatureError {
    format!("Missing reference: {what}").into()
}

/// A reference found: the body it was found on and the face or edge there. `None`: missing.
pub type Found<B, I> = Option<(Rc<Shape<B>>, I)>;

/// A name found for a legacy reference, keyed by the reference as stored.
#[derive(Clone, Debug, PartialEq)]
pub enum NameFound {
    Face(FaceSig, oc_doc::FaceName),
    Edge(EdgeSig, oc_doc::EdgeName),
    /// A vertex at the stored position.
    Vertex(P3, oc_doc::VertexName),
}

/// What resolving references and naming results turned up during one feature.
#[derive(Default)]
pub struct RefLog {
    pub(crate) found: Vec<NameFound>,
    /// References whose name is gone and that were accepted by position.
    pub(crate) reattached: u32,
    /// Warnings about lists of references of which some are missing.
    pub(crate) missing: Vec<String>,
    /// Faces that got a `/new/` tag: history gaps.
    pub(crate) untracked: u32,
}

impl RefLog {
    /// The warnings to show on the feature, after its own.
    pub(crate) fn warnings(&self) -> Vec<String> {
        let mut w = self.missing.clone();
        match self.reattached {
            0 => {}
            1 => w.push("A reference was re-attached by position".into()),
            n => w.push(format!("{n} references were re-attached by position")),
        }
        w
    }
}

impl<'a, K: Kernel> Ctx<'a, K> {
    fn note(&self, how: How, found: impl FnOnce() -> NameFound) {
        let mut log = self.log.borrow_mut();
        match how {
            How::Position => log.reattached += 1,
            How::Legacy => log.found.push(found()),
            How::Name | How::Split => {}
        }
    }

    /// The face a stored reference names, on the body as it stands. `None`: missing (or
    /// there is no body).
    pub fn find_face(&self, sig: &FaceSig) -> FResult<Found<K::Body, FaceId>> {
        let Some(s) = self.shape() else { return Ok(None) };
        let live = Live::new(self.topo(&s)?, &s.names);
        let Some(f) = naming::face(&live, sig, &self.rules) else { return Ok(None) };
        self.note(f.how, || NameFound::Face(sig.clone(), s.names.face(f.id).to_stored()));
        Ok(Some((s, f.id)))
    }

    /// The edge a stored reference names, on the body as it stands.
    pub fn find_edge(&self, sig: &EdgeSig) -> FResult<Found<K::Body, EdgeId>> {
        let Some(s) = self.shape() else { return Ok(None) };
        let live = Live::new(self.topo(&s)?, &s.names);
        let Some(e) = naming::edge(&live, sig, &self.rules) else { return Ok(None) };
        self.note(e.how, || NameFound::Edge(sig.clone(), live.edge_name(e.id).to_stored()));
        Ok(Some((s, e.id)))
    }

    /// Where a stored vertex is now, and its name. A named vertex follows the model
    /// (`None` when it no longer exists). A legacy one is named if a model vertex is at
    /// `p`; otherwise `p` is a point off the model (a sketch point, the origin) and stays.
    pub fn find_vertex(&self, p: P3, name: Option<&oc_doc::VertexName>) -> FResult<Option<(P3, Option<oc_doc::VertexName>)>> {
        // The origin is stored as a vertex at exactly (0, 0, 0); it is not model topology.
        if name.is_none() && p == [0.0; 3] {
            return Ok(Some((p, None)));
        }
        let off_model = || if name.is_none() { Some((p, None)) } else { None };
        let Some(s) = self.shape() else { return Ok(off_model()) };
        let t = self.topo(&s)?;
        let live = Live::new(t, &s.names);
        let Some(v) = naming::vertex(&live, p, name.map(Vec::as_slice), &self.rules) else { return Ok(off_model()) };
        let live_name = live.vertex_name(v.id).to_stored();
        self.note(v.how, || NameFound::Vertex(p, live_name.clone()));
        Ok(Some((t.vertices[v.id.index()], Some(live_name))))
    }

    /// The name of the model vertex at `at`, for a legacy vertex reference stored at
    /// `stored` that was found there by other means; the name is reported for write-back.
    pub fn name_vertex_at(&self, stored: P3, at: P3) -> FResult<Option<oc_doc::VertexName>> {
        let Some(s) = self.shape() else { return Ok(None) };
        let live = Live::new(self.topo(&s)?, &s.names);
        let Some(v) = naming::vertex(&live, at, None, &self.rules) else { return Ok(None) };
        let name = live.vertex_name(v.id).to_stored();
        self.log.borrow_mut().found.push(NameFound::Vertex(stored, name.clone()));
        Ok(Some(name))
    }

    /// The faces a list of stored references names, each once, in list order. Missing
    /// ones are left out with a warning; if none is found the feature fails.
    pub fn find_faces(&self, sigs: &[FaceSig], what: &str) -> FResult<Vec<FaceId>> {
        let mut out: Vec<FaceId> = vec![];
        let mut lost = 0;
        for sig in sigs {
            match self.find_face(sig)? {
                Some((_, f)) if !out.contains(&f) => out.push(f),
                Some(_) => {}
                None => lost += 1,
            }
        }
        self.partly_missing(lost, sigs.len(), out.is_empty(), what)?;
        Ok(out)
    }

    /// The edges a list of stored references names, each once, in list order.
    pub fn find_edges(&self, sigs: &[EdgeSig], what: &str) -> FResult<Vec<EdgeId>> {
        let mut out: Vec<EdgeId> = vec![];
        let mut lost = 0;
        for sig in sigs {
            match self.find_edge(sig)? {
                Some((_, e)) if !out.contains(&e) => out.push(e),
                Some(_) => {}
                None => lost += 1,
            }
        }
        self.partly_missing(lost, sigs.len(), out.is_empty(), what)?;
        Ok(out)
    }

    /// `lost` of `of` references of a list are missing: a warning, or the feature's error
    /// when nothing at all was found.
    pub(crate) fn partly_missing(&self, lost: usize, of: usize, none_found: bool, what: &str) -> FResult<()> {
        if lost == 0 {
            return Ok(());
        }
        if none_found {
            return Err(missing(what));
        }
        self.log.borrow_mut().missing.push(format!("Missing reference: {lost} of {of} {what}"));
        Ok(())
    }
}

/// A feature whose legacy references were named during a rebuild: `to` is `from` with the
/// names written in.
#[derive(Clone, Debug)]
pub struct Heal {
    pub from: Arc<Feature>,
    pub to: Arc<Feature>,
}

/// `f` with the names found for its legacy references written in, if any applies.
pub(crate) fn healed(f: &Feature, found: &[NameFound]) -> Option<Feature> {
    if found.is_empty() {
        return None;
    }
    let mut out = f.clone();
    let mut changed = false;
    out.refs_mut(&mut |slot| match slot {
        RefSlot::Face(sig) if sig.name.is_none() => {
            let n = found.iter().find_map(|x| match x {
                NameFound::Face(s, n) if s == sig => Some(n.clone()),
                _ => None,
            });
            changed |= n.is_some();
            sig.name = n;
        }
        RefSlot::Edge(sig) if sig.name.is_none() => {
            let n = found.iter().find_map(|x| match x {
                NameFound::Edge(s, n) if s == sig => Some(n.clone()),
                _ => None,
            });
            changed |= n.is_some();
            sig.name = n;
        }
        RefSlot::Vertex { p, name } if name.is_none() => {
            let n = found.iter().find_map(|x| match x {
                NameFound::Vertex(q, n) if q == p => Some(n.clone()),
                _ => None,
            });
            changed |= n.is_some();
            *name = n;
        }
        _ => {}
    });
    changed.then_some(out)
}

/// Write the names a rebuild found into `doc`. A feature is replaced only if it is still
/// the one that was rebuilt, so a rebuild that finishes after a newer edit cannot undo
/// it. Call inside `oc_doc::History::heal`: the write is not an undo step and does not
/// make the document dirty. Returns the number of features updated.
pub fn apply_heals(doc: &mut PartDocument, heals: &[Heal]) -> usize {
    let mut n = 0;
    for h in heals {
        let Some(i) = doc.features.iter().position(|f| f.id == h.from.id) else { continue };
        if Arc::ptr_eq(&doc.features[i], &h.from) || *doc.features[i] == *h.from {
            doc.features.set(i, h.to.clone());
            n += 1;
        }
    }
    n
}
