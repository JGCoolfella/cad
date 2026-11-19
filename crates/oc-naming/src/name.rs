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
///
/// A cap tag also knows how many regions (closed profiles) its feature had when it was
/// born ([`Tag::regions`]; owner decision 2026-11-19, G1d): only the cap of a single-region
/// feature stands in for itself wherever it moved (`resolve`, step 2a). The count is not
/// part of the tag's identity (equality, order, hashes and the keys derived from names
/// ignore it); a stored name carries it as a `#<n>` suffix on the tag ([`Tag::to_stored`]).
#[derive(Clone)]
pub struct Tag {
    s: Arc<str>,
    /// Regions of the feature that made this cap; 0: not a cap, or not known (a name
    /// stored before the count existed).
    regions: u32,
}

impl PartialEq for Tag {
    fn eq(&self, other: &Self) -> bool {
        self.s == other.s
    }
}

impl Eq for Tag {}

impl PartialOrd for Tag {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tag {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.s.cmp(&other.s)
    }
}

impl std::hash::Hash for Tag {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.s.hash(h)
    }
}

/// Separates a stored tag from its cap's region count: `f2/end/9f6f#1`.
const REGIONS_MARK: char = '#';

impl Tag {
    pub fn new(s: &str) -> Self {
        Tag { s: Arc::from(s), regions: 0 }
    }

    /// `"<feature>/<role>/<key>"`.
    pub fn birth(feature: &str, role: &str, key: &str) -> Self {
        Tag::new(&format!("{feature}/{role}/{key}"))
    }

    /// A cap (`start`, `end`, `mid`) of a feature that had `regions` profile regions.
    pub fn cap(feature: &str, role: &str, key: &str, regions: usize) -> Self {
        Tag::birth(feature, role, key).with_regions(regions)
    }

    /// The same tag with its feature's region count.
    pub fn with_regions(mut self, regions: usize) -> Self {
        self.regions = u32::try_from(regions).unwrap_or(u32::MAX);
        self
    }

    /// How many profile regions the feature that made this cap had; `None` when the tag is
    /// not a cap or the count is not known.
    pub fn regions(&self) -> Option<u32> {
        (self.regions > 0).then_some(self.regions)
    }

    /// This tag as instance `instance` of pattern or mirror `feature`.
    pub fn instance(&self, feature: &str, instance: &str) -> Self {
        Tag { s: Arc::from(format!("{feature}/i{instance}/{}", self.s)), regions: self.regions }
    }

    pub fn as_str(&self) -> &str {
        &self.s
    }

    /// The tag as documents store it: with `#<regions>` when the count is known.
    pub fn to_stored(&self) -> String {
        match self.regions() {
            Some(n) => format!("{}{REGIONS_MARK}{n}", self.s),
            None => self.s.to_string(),
        }
    }

    /// A stored tag, with its region count if it has one.
    pub fn from_stored(s: &str) -> Self {
        match s.rsplit_once(REGIONS_MARK) {
            Some((tag, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => Tag::new(tag).with_regions(n.parse().unwrap_or(0)),
            _ => Tag::new(s),
        }
    }

    /// The role of the tag, looking through instance prefixes: `side` for both
    /// `f2/side/l1` and `f7/i2,1/f2/side/l1`.
    pub fn role(&self) -> &str {
        let mut parts = self.s.split('/').skip(1);
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

    /// The tag without its key: `f2/end` for `f2/end/<loopKey>`, `f7/i2/f4/side` for
    /// `f7/i2/f4/side/l1`. Two tags with the same stem are faces of the same kind from the
    /// same feature (and the same pattern instance).
    pub fn stem(&self) -> &str {
        self.s.rsplit_once('/').map_or(&*self.s, |(stem, _)| stem)
    }

    /// A `/new/` tag: the face (or the face this instance copies) had no traceable origin.
    /// The corpus target is none of these.
    pub fn is_untracked(&self) -> bool {
        self.role() == "new"
    }
}

impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.s)
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.s)
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

    /// The two names have a tag of the same feature and role, whatever its key.
    pub fn shares_stem(&self, other: &FaceName) -> bool {
        self.0.iter().any(|a| other.0.iter().any(|b| a.stem() == b.stem()))
    }

    /// The name as documents store it (caps with their feature's region count).
    pub fn to_stored(&self) -> oc_doc::FaceName {
        self.0.iter().map(Tag::to_stored).collect()
    }

    pub fn from_stored(tags: &[String]) -> Self {
        FaceName::new(tags.iter().map(|t| Tag::from_stored(t)))
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
        assert_eq!(Tag::birth("f2", "end", "9f6f").stem(), "f2/end");
        assert_eq!(Tag::birth("f4", "side", "l1").instance("f7", "2,1").stem(), "f7/i2,1/f4/side");
        assert!(name(&["f2/end/aa", "f2/side/l1"]).shares_stem(&name(&["f2/end/bb"])));
        assert!(!name(&["f2/end/aa"]).shares_stem(&name(&["f2/start/aa", "f3/end/aa"])));
    }

    #[test]
    fn a_cap_carries_its_region_count_outside_its_identity() {
        let cap = Tag::cap("f2", "end", "9f6f", 2);
        assert_eq!((cap.as_str(), cap.regions(), cap.to_stored()), ("f2/end/9f6f", Some(2), "f2/end/9f6f#2".to_string()));
        // Not part of equality, order, hashes or keys.
        assert_eq!(cap, Tag::birth("f2", "end", "9f6f"));
        assert_eq!(FaceName::single(cap.clone()).key(), FaceName::single(Tag::birth("f2", "end", "9f6f")).key());
        // Stored and read back; a tag stored without a count (before G1e) has none.
        let back = Tag::from_stored("f2/end/9f6f#2");
        assert_eq!((back.as_str(), back.regions()), ("f2/end/9f6f", Some(2)));
        assert_eq!(Tag::from_stored("f2/end/9f6f").regions(), None);
        assert_eq!(Tag::birth("f2", "side", "l1").to_stored(), "f2/side/l1");
        // Pattern instances keep it; a key that merely contains `#` is left alone.
        assert_eq!(cap.instance("f7", "2").to_stored(), "f7/i2/f2/end/9f6f#2");
        assert_eq!(Tag::from_stored("f2/side/a#b").as_str(), "f2/side/a#b");
        let n = FaceName::new([cap, Tag::birth("f3", "side", "l1")]);
        assert_eq!(n.to_stored(), vec!["f2/end/9f6f#2".to_string(), "f3/side/l1".into()]);
        assert_eq!(FaceName::from_stored(&n.to_stored()).tags()[0].regions(), Some(2));
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
