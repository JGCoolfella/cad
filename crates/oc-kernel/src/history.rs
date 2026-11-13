//! Operation history: what became of each input entity. This is the raw material for
//! persistent naming (`docs/OVERHAUL_PLAN.md` section 3.2), so it is complete for faces
//! and kernel-neutral (OCCT's `Modified`/`Generated`/`IsDeleted`, Parasolid's tracking
//! records).

use crate::topo::{EdgeId, FaceId, TopoKind, VertexId};

/// An input entity of an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// Face of input body `arg` (0 = the target or only body, 1.. = tools in order).
    Face { arg: u32, face: FaceId },
    Edge { arg: u32, edge: EdgeId },
    Vertex { arg: u32, vertex: VertexId },
    /// Segment of a profile wire: `region` index (or loft section), `wire` (0 = outer,
    /// k = hole k − 1), `segment` index in the wire.
    Segment { region: u32, wire: u32, segment: u32 },
    /// Start or end cap of a region (or of the whole loft: region 0).
    Cap { region: u32, end: CapEnd },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapEnd {
    Start,
    End,
}

/// How an output entity relates to its source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Relation {
    /// The very same entity passed through.
    Same,
    /// The source was changed into it (trimmed, split, moved, merged).
    Modified,
    /// It was created from the source (fillet face from an edge, side face from a
    /// profile segment, cap from a region).
    Generated,
}

/// One output entity of a [`Fate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Image {
    pub index: u32,
    pub relation: Relation,
}

/// What happened to one source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fate {
    pub source: Source,
    /// The kernel reports the source as removed. A deleted source has no images.
    pub deleted: bool,
    /// Output entities (faces, or edges for sections), ascending by index.
    pub images: Vec<Image>,
}

/// History of one operation.
///
/// Completeness rules (checked by [`History::check`] in tests):
/// - every face of every input body has a fate, in input order;
/// - edges and vertices appear only where they generate or become output entities;
/// - profile segments and caps appear when they produce faces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    /// Kind of the output entities images refer to: faces, or edges for a section.
    pub image_kind: TopoKind,
    pub fates: Vec<Fate>,
}

impl Default for History {
    fn default() -> Self {
        History { image_kind: TopoKind::Face, fates: vec![] }
    }
}

impl History {
    /// For each output entity `0..count`, its sources and relations, in fate order.
    pub fn origins(&self, count: usize) -> Vec<Vec<(Source, Relation)>> {
        let mut out = vec![vec![]; count];
        for f in &self.fates {
            for im in &f.images {
                if let Some(slot) = out.get_mut(im.index as usize) {
                    slot.push((f.source, im.relation));
                }
            }
        }
        out
    }

    /// Output entities with no source at all: what naming would have to call `/new/`.
    pub fn untracked(&self, count: usize) -> Vec<u32> {
        self.origins(count).iter().enumerate().filter(|(_, o)| o.is_empty()).map(|(i, _)| i as u32).collect()
    }

    pub fn fate(&self, source: Source) -> Option<&Fate> {
        self.fates.iter().find(|f| f.source == source)
    }

    /// Images of `source`, or empty.
    pub fn images(&self, source: Source) -> Vec<u32> {
        self.fate(source).map(|f| f.images.iter().map(|i| i.index).collect()).unwrap_or_default()
    }

    /// Structural checks: image indices below `count`, sorted and unique; deleted
    /// sources without images; each source once.
    pub fn check(&self, count: usize) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for f in &self.fates {
            if !seen.insert(f.source) {
                return Err(format!("{:?} appears twice", f.source));
            }
            if f.deleted && !f.images.is_empty() {
                return Err(format!("{:?} is deleted but has images", f.source));
            }
            if f.images.windows(2).any(|w| w[0].index >= w[1].index) {
                return Err(format!("{:?}: images not ascending/unique", f.source));
            }
            if let Some(im) = f.images.iter().find(|im| im.index as usize >= count) {
                return Err(format!("{:?}: image {} out of range ({count})", f.source, im.index));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_and_untracked() {
        let h = History {
            image_kind: TopoKind::Face,
            fates: vec![
                Fate {
                    source: Source::Face { arg: 0, face: FaceId(0) },
                    deleted: false,
                    images: vec![Image { index: 0, relation: Relation::Same }],
                },
                Fate {
                    source: Source::Edge { arg: 0, edge: EdgeId(3) },
                    deleted: false,
                    images: vec![Image { index: 2, relation: Relation::Generated }],
                },
                Fate { source: Source::Face { arg: 1, face: FaceId(0) }, deleted: true, images: vec![] },
            ],
        };
        assert_eq!(h.untracked(4), vec![1, 3]);
        assert_eq!(h.origins(4)[2], vec![(Source::Edge { arg: 0, edge: EdgeId(3) }, Relation::Generated)]);
        assert_eq!(h.images(Source::Face { arg: 0, face: FaceId(0) }), vec![0]);
        assert!(h.check(4).is_ok());
        assert!(h.check(2).is_err());
        let mut dup = h.clone();
        dup.fates.push(dup.fates[0].clone());
        assert!(dup.check(4).is_err());
    }
}
