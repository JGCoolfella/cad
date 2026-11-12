//! Insertion-ordered persistent map (`Record<Id, T>` in `src/core/types.ts`).
//!
//! A TypeScript record keeps its members in insertion order, and the sketch solver
//! depends on that order (parameter order, equation order, which relation is flagged
//! redundant). [`Record`] keeps it too, with structural sharing: entries live in an
//! [`OrdMap`] keyed by a sequence number that only grows, plus an id → sequence index.
//! Every operation is O(log n), and editing one entry copies one path of map nodes.

use std::borrow::Borrow;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Index;

use imbl::OrdMap;
use serde::de::{self, Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

use crate::Id;

/// Newtype name that tells the document writer to keep a map's member order
/// (see [`crate::to_pretty_string`]). Other serializers treat it as a plain map.
pub(crate) const ORDERED_MAP: &str = "$oc_doc::Record";

/// Map from [`Id`] to `T` that iterates, serializes and compares in insertion order.
///
/// Inserting an existing id replaces its value in place; removing an id keeps the order
/// of the others (JavaScript's `delete`).
#[derive(Clone)]
pub struct Record<T> {
    /// Entries by sequence number, i.e. in insertion order.
    entries: OrdMap<u64, (Id, T)>,
    /// Sequence number of each id.
    slots: OrdMap<Id, u64>,
    /// Sequence number of the next new entry.
    next: u64,
}

impl<T: Clone> Default for Record<T> {
    fn default() -> Self {
        Record {
            entries: OrdMap::new(),
            slots: OrdMap::new(),
            next: 0,
        }
    }
}

impl<T: Clone> Record<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn contains_key<Q>(&self, id: &Q) -> bool
    where
        Q: Ord + ?Sized,
        Id: Borrow<Q>,
    {
        self.slots.contains_key(id)
    }

    pub fn get<Q>(&self, id: &Q) -> Option<&T>
    where
        Q: Ord + ?Sized,
        Id: Borrow<Q>,
    {
        let seq = self.slots.get(id)?;
        self.entries.get(seq).map(|(_, v)| v)
    }

    /// Copies only the path to this entry if it is shared with another version.
    pub fn get_mut<Q>(&mut self, id: &Q) -> Option<&mut T>
    where
        Q: Ord + ?Sized,
        Id: Borrow<Q>,
    {
        let seq = *self.slots.get(id)?;
        self.entries.get_mut(&seq).map(|(_, v)| v)
    }

    /// Adds `value` at the end, or replaces the value of an existing `id` in place.
    /// Returns the replaced value.
    pub fn insert(&mut self, id: Id, value: T) -> Option<T> {
        if let Some(&seq) = self.slots.get(&id) {
            let slot = self.entries.get_mut(&seq).expect("indexed entry exists");
            return Some(std::mem::replace(&mut slot.1, value));
        }
        let seq = self.next;
        self.next += 1;
        self.slots.insert(id.clone(), seq);
        self.entries.insert(seq, (id, value));
        None
    }

    /// Removes `id`, keeping the order of the other entries.
    pub fn remove<Q>(&mut self, id: &Q) -> Option<T>
    where
        Q: Ord + ?Sized,
        Id: Borrow<Q>,
    {
        let seq = self.slots.remove(id)?;
        self.entries.remove(&seq).map(|(_, v)| v)
    }

    /// Entries in insertion order.
    pub fn iter(&self) -> Iter<'_, T> {
        Iter(self.entries.values())
    }

    pub fn keys(&self) -> Keys<'_, T> {
        self.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> Values<'_, T> {
        self.iter().map(|(_, v)| v)
    }

    /// True if both are the same version (no copy happened between them). Cheap, and
    /// false negatives are possible; use `==` to compare contents.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.entries.ptr_eq(&other.entries) && self.slots.ptr_eq(&other.slots)
    }
}

/// Ids of a [`Record`] in insertion order.
pub type Keys<'a, T> = std::iter::Map<Iter<'a, T>, fn((&'a Id, &'a T)) -> &'a Id>;
/// Values of a [`Record`] in insertion order.
pub type Values<'a, T> = std::iter::Map<Iter<'a, T>, fn((&'a Id, &'a T)) -> &'a T>;

/// Iterator over a [`Record`] in insertion order.
pub struct Iter<'a, T>(imbl::ordmap::Values<'a, u64, (Id, T), imbl::shared_ptr::DefaultSharedPtr>);

impl<'a, T: 'a> Iterator for Iter<'a, T> {
    type Item = (&'a Id, &'a T);
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(k, v)| (k, v))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<'a, T: 'a> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(|(k, v)| (k, v))
    }
}

impl<'a, T: Clone> IntoIterator for &'a Record<T> {
    type Item = (&'a Id, &'a T);
    type IntoIter = Iter<'a, T>;
    fn into_iter(self) -> Iter<'a, T> {
        self.iter()
    }
}

impl<T: Clone> FromIterator<(Id, T)> for Record<T> {
    fn from_iter<I: IntoIterator<Item = (Id, T)>>(iter: I) -> Self {
        let mut r = Record::new();
        r.extend(iter);
        r
    }
}

impl<T: Clone> Extend<(Id, T)> for Record<T> {
    fn extend<I: IntoIterator<Item = (Id, T)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

impl<T: Clone, Q> Index<&Q> for Record<T>
where
    Q: Ord + ?Sized,
    Id: Borrow<Q>,
{
    type Output = T;
    fn index(&self, id: &Q) -> &T {
        self.get(id).expect("no entry with this id")
    }
}

/// Equal when the entries and their order are equal (order matters to the solver).
impl<T: Clone + PartialEq> PartialEq for Record<T> {
    fn eq(&self, other: &Self) -> bool {
        if self.len() != other.len() {
            return false;
        }
        // Same ids at the same sequence numbers (two versions of one record whose
        // values were edited): then the records are equal iff the entry maps are, and
        // `OrdMap`'s `==` skips shared subtrees, so detecting a one-point edit in an
        // undo step is O(log n), not a walk over the whole sketch.
        if self.slots == other.slots {
            return self.entries == other.entries;
        }
        // Different numbering (built separately, or an id removed and re-added).
        self.iter().eq(other.iter())
    }
}

impl<T: Clone + fmt::Debug> fmt::Debug for Record<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<T: Clone + Serialize> Serialize for Record<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_struct(ORDERED_MAP, &Members(self))
    }
}

struct Members<'a, T: Clone>(&'a Record<T>);

impl<T: Clone + Serialize> Serialize for Members<'_, T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl<'de, T: Clone + Deserialize<'de>> Deserialize<'de> for Record<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_map(RecordVisitor(PhantomData))
    }
}

struct RecordVisitor<T>(PhantomData<T>);

impl<'de, T: Clone + Deserialize<'de>> Visitor<'de> for RecordVisitor<T> {
    type Value = Record<T>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object keyed by id")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Record<T>, A::Error> {
        let mut r = Record::new();
        while let Some((k, v)) = map.next_entry::<Id, T>()? {
            let key = k.to_string();
            if r.insert(k, v).is_some() {
                return Err(de::Error::custom(format!("id {key:?} appears twice")));
            }
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(r: &Record<i32>) -> Vec<&str> {
        r.keys().map(Id::as_str).collect()
    }

    #[test]
    fn keeps_insertion_order() {
        let mut r = Record::new();
        for k in ["p2", "p10", "origin", "p1"] {
            r.insert(Id::from(k), k.len() as i32);
        }
        assert_eq!(ids(&r), ["p2", "p10", "origin", "p1"]);
        // Replacing keeps the position; removing keeps the others' order.
        assert_eq!(r.insert(Id::from("p10"), 7), Some(3));
        assert_eq!(r.remove("origin"), Some(6));
        assert_eq!(r.remove("origin"), None);
        assert_eq!(ids(&r), ["p2", "p10", "p1"]);
        assert_eq!(r["p10"], 7);
        // A re-added id goes to the end.
        r.insert(Id::from("origin"), 0);
        assert_eq!(ids(&r), ["p2", "p10", "p1", "origin"]);
        assert_eq!(r.len(), 4);
        assert_eq!(r.keys().next_back().map(Id::as_str), Some("origin"));
    }

    #[test]
    fn equality_includes_order() {
        let a: Record<i32> = [(Id::from("a"), 1), (Id::from("b"), 2)]
            .into_iter()
            .collect();
        let b: Record<i32> = [(Id::from("b"), 2), (Id::from("a"), 1)]
            .into_iter()
            .collect();
        assert_ne!(a, b);
        let mut c = a.clone();
        assert!(c.ptr_eq(&a));
        *c.get_mut("a").unwrap() = 1;
        assert_eq!(c, a);
        *c.get_mut("a").unwrap() = 5;
        assert_ne!(c, a);
        // Equal content, different sequence numbers.
        let mut d = a.clone();
        d.remove("a");
        d.remove("b");
        d.insert(Id::from("a"), 1);
        d.insert(Id::from("b"), 2);
        assert_eq!(d, a);
        d.insert(Id::from("b"), 3);
        assert_ne!(d, a);
    }

    #[test]
    fn json_round_trip_keeps_order() {
        let text = r#"{"z":1,"p10":2,"p2":3,"a":4}"#;
        let r: Record<i32> = serde_json::from_str(text).unwrap();
        assert_eq!(ids(&r), ["z", "p10", "p2", "a"]);
        assert_eq!(serde_json::to_string(&r).unwrap(), text);
        assert!(serde_json::from_str::<Record<i32>>(r#"{"a":1,"a":2}"#).is_err());
    }
}
