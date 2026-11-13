//! Names (`docs/OVERHAUL_PLAN.md` section 3.1): only faces carry history; an edge is
//! named by the two faces it lies between and a vertex by the faces around it.

use std::fmt;
use std::sync::Arc;

use oc_kernel::{EdgeId, FaceId, Topology, VertexId};
use smallvec::SmallVec;

use crate::hash::Fnv64;

/// One origin of a face: `"<featureId>/<role>[/<key>]"`.
///
/// | Tag | Face |
/// |---|---|
/// | `<fid>/side/<entity>` | swept from a profile segment (sketch entity id, or a key the feature gives a generated segment) |
/// | `<fid>/start/<loopKey>`, `<fid>/end/<loopKey>` | cap of a region; the key hashes the entity ids of its outer loop |
/// | `<fid>/fillet/<edgeKey>`, `<fid>/chamfer/<edgeKey>` | generated from an edge; the key hashes the edge's name |
/// | `<fid>/corner/<vertexKey>` | generated from a vertex; the key hashes the vertex's name |
/// | `<fid>/inner/<faceKey>` | offset copy of a face (shell wall); the key hashes the face's name |
/// | `<fid>/i<n>/<seedTag>` | instance `n` of a pattern or mirror |
/// | `<fid>/import/<rank>` | face of an imported body, ranked by geometry |
/// | `<fid>/new/<rank>` | no traceable origin (a history gap), ranked by geometry |
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tag(Arc<str>);

impl Tag {
    pub fn new(s: &str) -> Self {
        Tag(Arc::from(s))
    }

    /// `"<feature>/<role>/<key>"`.
    pub fn birth(feature: &str, role: &str, key: &str) -> Self {
        Tag(Arc::from(format!("{feature}/{role}/{key}")))
    }

    /// This tag as instance `instance` of pattern or mirror `feature`.
    pub fn instance(&self, feature: &str, instance: &str) -> Self {
        Tag(Arc::from(format!("{feature}/i{instance}/{}", self.0)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The role of the tag, looking through instance prefixes: `side` for both
    /// `f2/side/l1` and `f7/i2,1/f2/side/l1`.
    pub fn role(&self) -> &str {
        let mut parts = self.0.split('/').skip(1);
        loop {
            let Some(role) = parts.next() else { return "" };
            let is_instance = role.strip_prefix('i').is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()));
            if !is_instance {
                return role;
            }
            // Skip the seed tag's feature id.
            parts.next();
        }
    }

    /// A `/new/` tag: the face (or the face this instance copies) had no traceable origin.
    /// The corpus target is none of these.
    pub fn is_untracked(&self) -> bool {
        self.role() == "new"
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A face's name: its tags, sorted and without repeats. Splitting a face keeps its tags
/// on every piece; merging faces unions them.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FaceName(SmallVec<[Tag; 2]>);

impl FaceName {
    pub fn new(tags: impl IntoIterator<Item = Tag>) -> Self {
        let mut v: SmallVec<[Tag; 2]> = tags.into_iter().collect();
        v.sort_unstable();
        v.dedup();
        FaceName(v)
    }

    pub fn single(tag: Tag) -> Self {
        FaceName(SmallVec::from_iter([tag]))
    }

    pub fn tags(&self) -> &[Tag] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The two names have a tag in common: they descend from the same original face.
    pub fn shares_tag(&self, other: &FaceName) -> bool {
        let (mut i, mut j) = (0, 0);
        while i < self.0.len() && j < other.0.len() {
            match self.0[i].cmp(&other.0[j]) {
                std::cmp::Ordering::Equal => return true,
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }
        false
    }

    pub fn is_untracked(&self) -> bool {
        self.0.iter().any(Tag::is_untracked)
    }

    /// The name as documents store it.
    pub fn to_stored(&self) -> oc_doc::FaceName {
        self.0.iter().map(|t| t.as_str().to_string()).collect()
    }

    pub fn from_stored(tags: &[String]) -> Self {
        FaceName::new(tags.iter().map(|t| Tag::new(t)))
    }

    pub(crate) fn feed(&self, mut h: Fnv64) -> Fnv64 {
        for t in &self.0 {
            h = h.str(t.as_str());
        }
        h.bytes(&[1])
    }

    /// Key of this name for a tag derived from the face (`<fid>/inner/<key>`).
    pub fn key(&self) -> String {
        self.feed(Fnv64::default()).key()
    }
}

impl fmt::Debug for FaceName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.0.iter()).finish()
    }
}

/// An edge's name: the names of the two faces it lies between, sorted (the same face
/// twice for a seam). Several edges can share a name, e.g. when a face is split; a
/// reference then tells them apart by its signature.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeName(pub [FaceName; 2]);

impl EdgeName {
    pub fn new(a: FaceName, b: FaceName) -> Self {
        EdgeName(if a <= b { [a, b] } else { [b, a] })
    }

    /// Key for a face generated from this edge (`<fid>/fillet/<key>`).
    pub fn key(&self) -> String {
        self.0.iter().fold(Fnv64::default(), |h, n| n.feed(h)).key()
    }

    pub fn to_stored(&self) -> oc_doc::EdgeName {
        [self.0[0].to_stored(), self.0[1].to_stored()]
    }

    pub fn from_stored(n: &oc_doc::EdgeName) -> Self {
        EdgeName::new(FaceName::from_stored(&n[0]), FaceName::from_stored(&n[1]))
    }
}

/// A vertex's name: the names of the faces around it, sorted, each once.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VertexName(pub Vec<FaceName>);

impl VertexName {
    pub fn new(faces: impl IntoIterator<Item = FaceName>) -> Self {
        let mut v: Vec<FaceName> = faces.into_iter().collect();
        v.sort_unstable();
        v.dedup();
        VertexName(v)
    }

    /// Key for a face generated from this vertex (`<fid>/corner/<key>`).
    pub fn key(&self) -> String {
        self.0.iter().fold(Fnv64::default(), |h, n| n.feed(h)).key()
    }

    pub fn to_stored(&self) -> oc_doc::VertexName {
        self.0.iter().map(FaceName::to_stored).collect()
    }

    pub fn from_stored(n: &[oc_doc::FaceName]) -> Self {
        VertexName::new(n.iter().map(|f| FaceName::from_stored(f)))
    }
}

/// Names aligned with the kernel's face order for one body.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BodyNames {
    pub faces: Vec<FaceName>,
}

impl BodyNames {
    pub fn face(&self, f: FaceId) -> &FaceName {
        &self.faces[f.index()]
    }

    /// Name of edge `e` of the body with topology `t`.
    pub fn edge(&self, t: &Topology, e: EdgeId) -> EdgeName {
        let mut around: Vec<&FaceName> = t.edges[e.index()].faces.iter().filter_map(|f| self.faces.get(f.index())).collect();
        around.sort_unstable();
        match around.as_slice() {
            [] => EdgeName::new(FaceName::default(), FaceName::default()),
            [one] => EdgeName::new((*one).clone(), (*one).clone()),
            [a, b, ..] => EdgeName::new((*a).clone(), (*b).clone()),
        }
    }

    /// Name of vertex `v`, given the faces around every vertex ([`vertex_faces`]).
    pub fn vertex(&self, around: &[Vec<FaceId>], v: VertexId) -> VertexName {
        VertexName::new(around[v.index()].iter().filter_map(|f| self.faces.get(f.index())).cloned())
    }

    /// Faces without a traceable origin.
    pub fn untracked(&self) -> usize {
        self.faces.iter().filter(|n| n.is_untracked()).count()
    }
}

/// For every vertex, the faces around it (ascending, each once): the faces of the edges
/// that end at it.
pub fn vertex_faces(t: &Topology) -> Vec<Vec<FaceId>> {
    let mut out: Vec<Vec<FaceId>> = vec![vec![]; t.vertices.len()];
    for e in &t.edges {
        for v in &e.vertices {
            if let Some(slot) = out.get_mut(v.index()) {
                slot.extend(e.faces.iter().copied());
            }
        }
    }
    for f in &mut out {
        f.sort_unstable();
        f.dedup();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(tags: &[&str]) -> FaceName {
        FaceName::new(tags.iter().map(|t| Tag::new(t)))
    }

    #[test]
    fn face_names_are_sorted_sets() {
        let n = name(&["f2/side/l2", "f2/end/k", "f2/side/l2"]);
        assert_eq!(n.tags().iter().map(Tag::as_str).collect::<Vec<_>>(), ["f2/end/k", "f2/side/l2"]);
        assert_eq!(n, name(&["f2/end/k", "f2/side/l2"]));
        assert!(n.shares_tag(&name(&["a/b", "f2/side/l2"])));
        assert!(!n.shares_tag(&name(&["f2/side/l1", "f2/side/l3"])));
        assert!(!n.shares_tag(&FaceName::default()));
        assert_eq!(FaceName::from_stored(&n.to_stored()), n);
    }

    #[test]
    fn untracked_tags_are_recognised_through_instances() {
        assert!(Tag::birth("f3", "new", "0").is_untracked());
        assert!(Tag::birth("f3", "new", "0").instance("f7", "2,1").is_untracked());
        assert_eq!(Tag::birth("f3", "side", "l1").instance("f7", "2,1").as_str(), "f7/i2,1/f3/side/l1");
        // A feature or entity may be called "new"; only the role counts.
        assert!(!Tag::birth("new", "side", "l1").is_untracked());
        assert!(!Tag::birth("f3", "side", "new").is_untracked());
        assert!(!Tag::birth("f3", "side", "new").instance("f7", "3").is_untracked());
        assert_eq!(Tag::birth("f3", "inner", "0a").instance("f7", "3").instance("f9", "2").role(), "inner");
        assert_eq!(Tag::birth("f3", "import", "4").role(), "import");
    }

    #[test]
    fn edge_and_vertex_names_do_not_depend_on_order() {
        let (a, b, c) = (name(&["f/side/a"]), name(&["f/side/b"]), name(&["f/end/k"]));
        assert_eq!(EdgeName::new(a.clone(), b.clone()), EdgeName::new(b.clone(), a.clone()));
        assert_eq!(EdgeName::new(a.clone(), b.clone()).key(), EdgeName::new(b.clone(), a.clone()).key());
        assert_ne!(EdgeName::new(a.clone(), b.clone()).key(), EdgeName::new(a.clone(), c.clone()).key());
        let v = VertexName::new([c.clone(), a.clone(), b.clone(), a.clone()]);
        assert_eq!(v, VertexName::new([a.clone(), b.clone(), c.clone()]));
        assert_eq!(v.0.len(), 3);
        assert_eq!(VertexName::from_stored(&v.to_stored()), v);
        assert_eq!(EdgeName::from_stored(&EdgeName::new(b.clone(), a.clone()).to_stored()), EdgeName::new(a, b));
        // A face name's key is not the key of the edge between the face and itself.
        assert_ne!(c.key(), EdgeName::new(c.clone(), c.clone()).key());
    }
}
