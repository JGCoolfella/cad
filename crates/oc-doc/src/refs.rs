//! References from the document to model geometry (`references` section of
//! `src/core/types.ts`, plus `SketchRef`).
//!
//! A stored reference is a **persistent name** plus a **geometric signature**
//! (`docs/OVERHAUL_PLAN.md` section 3.3). The name says where the face, edge or vertex
//! came from in the feature history and is how a rebuild finds it again; the signature
//! tells split pieces apart and is the fallback when the name no longer exists. A
//! reference without a name is a legacy one (schema version 1, or picked in an app that
//! does not name yet): it is found by signature, and the rebuild reports the name to
//! write back ([`crate::History::heal`], [`Feature::refs_mut`]).

use serde::{Deserialize, Serialize};

use crate::{EndCondition, Feature, FeatureKind, Id, Vec3};

/// Persistent name of a face as stored: its origin tags (`"<featureId>/<role>[/<key>]"`),
/// sorted and without duplicates. `oc-naming` produces and reads them.
pub type FaceName = Vec<String>;
/// Persistent name of an edge: the names of the two faces it lies between, sorted (a
/// seam edge has the same face twice).
pub type EdgeName = [FaceName; 2];
/// Persistent name of a vertex: the names of the faces around it, sorted, each once.
pub type VertexName = Vec<FaceName>;

/// Reference to a face: its geometric signature as picked, and its persistent name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceSig {
    /// OCCT surface type name: `"PLANE"`, `"CYLINDRE"`, ...
    pub surface: String,
    pub center: Vec3,
    pub normal: Vec3,
    pub area: f64,
    /// Absent on legacy references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<FaceName>,
}

/// Reference to an edge: its geometric signature as picked, and its persistent name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeSig {
    pub curve: String,
    pub mid: Vec3,
    pub a: Vec3,
    pub b: Vec3,
    pub length: f64,
    /// Absent on legacy references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<EdgeName>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StdPlane {
    Front,
    Top,
    Right,
}

/// A plane: one of the three standard planes, a planar model face, or a reference plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PlaneRef {
    Std {
        plane: StdPlane,
    },
    Face {
        sig: FaceSig,
    },
    #[serde(rename = "refplane")]
    RefPlane {
        feature_id: Id,
    },
    /// A planar face of another component of an assembly the part was edited in (G1d,
    /// in-context reference): `sig` is that face in this part's coordinates as it was when
    /// last updated in the assembly. The rebuild takes the plane from `sig` (no topology of
    /// this part is involved); the assembly updates it.
    External {
        sig: FaceSig,
        ext: ExternalRef,
    },
}

/// Where an in-context reference (G1d, Edit Part in an assembly) comes from, as SOLIDWORKS
/// keeps it with the part: the assembly, the referenced component's occurrence and this
/// part's occurrence in it (the context), and the referenced entity as the assembly's mate
/// references store it (a face, edge or vertex by persistent name and signature in its own
/// part's coordinates), for re-finding it when the assembly updates the reference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalRef {
    /// The assembly file (absolute, `/`-separated).
    pub assembly: String,
    /// The referenced component's occurrence path in that assembly.
    pub component: Vec<Id>,
    /// This part's occurrence path in that assembly.
    pub context: Vec<Id>,
    /// The referenced entity (an `oc_doc::MateEntity` as JSON; kept as written).
    pub entity: serde_json::Value,
    /// How the FeatureManager names it: `Face@plate<1>`.
    #[serde(default)]
    pub label: String,
}

/// A model reference picked in the graphics area or the tree: a direction (linear edge,
/// planar face normal, plane normal), an axis (linear or circular edge, cylindrical face,
/// sketch line) or a plane (planar face, plane).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GeomRef {
    Edge {
        sig: EdgeSig,
    },
    Face {
        sig: FaceSig,
    },
    Plane {
        plane: PlaneRef,
    },
    SketchLine {
        sketch_id: Id,
        entity: Id,
    },
    /// A model vertex (position captured when picked) with its persistent name. Without
    /// a name it is a legacy reference, named by the first rebuild if a model vertex is
    /// there.
    Vertex {
        p: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<VertexName>,
    },
    /// The part origin. (Version 1 stored it as a vertex at (0, 0, 0), which could not be
    /// told from a model vertex there; the 1 → 2 migration converts those.)
    Origin,
    /// Reference axis feature.
    Axis {
        feature_id: Id,
    },
    /// Reference point feature.
    Point {
        feature_id: Id,
    },
}

/// External reference from a sketch to model geometry, as SOLIDWORKS relations to model
/// edges and vertices. The rebuild re-finds it by signature, projects it onto the sketch
/// plane and moves the fixed sketch geometry that stands for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SketchRef {
    /// Re-found by `name`. A legacy reference is re-found as the common end of its edges
    /// (`edges`), else the nearest vertex.
    Vertex {
        id: Id,
        p: Vec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        edges: Option<Vec<EdgeSig>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<VertexName>,
    },
    Edge {
        id: Id,
        sig: EdgeSig,
    },
    /// An edge of another component (G1d, in-context reference, Convert Entities in Edit
    /// Part): `sig` is that edge in this part's coordinates as last updated in the assembly;
    /// the rebuild projects it onto the sketch plane like a model edge. Never re-found on
    /// this part's body.
    External {
        id: Id,
        sig: EdgeSig,
        ext: ExternalRef,
    },
}

impl SketchRef {
    pub fn id(&self) -> &Id {
        match self {
            SketchRef::Vertex { id, .. } | SketchRef::Edge { id, .. } | SketchRef::External { id, .. } => id,
        }
    }
}

/// One place in a feature that refers to model topology, for [`Feature::refs_mut`].
pub enum RefSlot<'a> {
    Face(&'a mut FaceSig),
    Edge(&'a mut EdgeSig),
    /// A vertex: the position captured when picked, and its name.
    Vertex {
        p: &'a Vec3,
        name: &'a mut Option<VertexName>,
    },
}

type Visit<'v> = &'v mut dyn FnMut(RefSlot<'_>);

fn plane_slots(p: &mut PlaneRef, f: Visit<'_>) {
    if let PlaneRef::Face { sig } = p {
        f(RefSlot::Face(sig));
    }
}

fn geom_slots(r: &mut GeomRef, f: Visit<'_>) {
    match r {
        GeomRef::Edge { sig } => f(RefSlot::Edge(sig)),
        GeomRef::Face { sig } => f(RefSlot::Face(sig)),
        GeomRef::Plane { plane } => plane_slots(plane, f),
        GeomRef::Vertex { p, name } => f(RefSlot::Vertex { p, name }),
        GeomRef::SketchLine { .. } | GeomRef::Axis { .. } | GeomRef::Point { .. } | GeomRef::Origin => {}
    }
}

fn end_slots(e: &mut EndCondition, f: Visit<'_>) {
    if let Some(Some(r)) = &mut e.r#ref {
        geom_slots(r, f);
    }
    if let Some(Some(p)) = &e.vertex {
        f(RefSlot::Vertex { p, name: &mut e.vertex_name });
    }
}

impl Feature {
    /// Visits every reference to model topology stored in the feature (faces, edges and
    /// vertices; not references to other features), in a fixed order. This is how
    /// resolved names are written back into a document.
    pub fn refs_mut(&mut self, f: Visit<'_>) {
        match &mut self.kind {
            FeatureKind::Sketch(s) => {
                plane_slots(&mut s.plane, f);
                let Some(refs) = s.sketch.refs.as_mut() else { return };
                let ids: Vec<Id> = refs.keys().cloned().collect();
                for id in ids {
                    match refs.get_mut(&id) {
                        Some(SketchRef::Edge { sig, .. }) => f(RefSlot::Edge(sig)),
                        Some(SketchRef::Vertex { p, edges, name, .. }) => {
                            f(RefSlot::Vertex { p, name });
                            for e in edges.iter_mut().flatten() {
                                f(RefSlot::Edge(e));
                            }
                        }
                        // Another part's geometry: nothing of this part's topology.
                        Some(SketchRef::External { .. }) | None => {}
                    }
                }
            }
            FeatureKind::RefPlane(p) => {
                plane_slots(&mut p.base, f);
                for r in p.refs.iter_mut().flatten() {
                    geom_slots(r, f);
                }
            }
            FeatureKind::RefAxis(a) => a.refs.iter_mut().for_each(|r| geom_slots(r, f)),
            FeatureKind::RefPoint(p) => p.refs.iter_mut().for_each(|r| geom_slots(r, f)),
            FeatureKind::CoordSys(c) => {
                for r in [&mut c.origin, &mut c.x_axis, &mut c.y_axis].into_iter().flatten() {
                    geom_slots(r, f);
                }
            }
            FeatureKind::Extrude(e) => {
                if let Some(Some(r)) = e.from.as_mut().and_then(|x| x.r#ref.as_mut()) {
                    geom_slots(r, f);
                }
                end_slots(&mut e.end1, f);
                if let Some(d) = &mut e.dir2 {
                    end_slots(d, f);
                }
            }
            FeatureKind::Revolve(r) => {
                if let Some(Some(a)) = &mut r.axis_ref {
                    geom_slots(a, f);
                }
            }
            FeatureKind::Fillet(x) => {
                x.edges.iter_mut().for_each(|e| f(RefSlot::Edge(e)));
                x.faces.iter_mut().for_each(|s| f(RefSlot::Face(s)));
            }
            FeatureKind::Chamfer(x) => x.edges.iter_mut().for_each(|e| f(RefSlot::Edge(e))),
            FeatureKind::LinearPattern(p) => {
                for d in std::iter::once(&mut p.dir1).chain(&mut p.dir2) {
                    if let Some(r) = &mut d.r#ref {
                        geom_slots(r, f);
                    }
                }
            }
            FeatureKind::CircularPattern(p) => p.axis.iter_mut().for_each(|r| geom_slots(r, f)),
            FeatureKind::Mirror(m) => m.plane.iter_mut().for_each(|r| geom_slots(r, f)),
            FeatureKind::Hole(h) => h.face.iter_mut().for_each(|s| f(RefSlot::Face(s))),
            FeatureKind::Draft(d) => {
                d.neutral.iter_mut().for_each(|r| geom_slots(r, f));
                d.faces.iter_mut().for_each(|s| f(RefSlot::Face(s)));
            }
            FeatureKind::Shell(s) => s.faces.iter_mut().for_each(|x| f(RefSlot::Face(x))),
            FeatureKind::Sweep(_) | FeatureKind::Loft(_) | FeatureKind::Rib(_) | FeatureKind::Imported(_) => {}
        }
    }
}
