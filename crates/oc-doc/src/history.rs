//! Transactions and undo/redo by structural sharing.
//!
//! The current document is an immutable snapshot (`Arc<PartDocument>`). A transaction
//! edits a private copy, which is cheap because the copy shares every feature and every
//! sketch map node with the snapshot until it is written (`Arc::make_mut`, persistent
//! maps). Committing swaps the snapshot pointer and keeps the old one on the undo stack;
//! undo and redo swap pointers back. A step therefore costs memory proportional to what
//! the edit changed, not to the document, and an undone document is the very snapshot
//! it was before (pointer-equal), not a re-parsed copy.
//!
//! Readers (renderer, regeneration) take a snapshot with [`History::doc`] and keep it as
//! long as they like; nothing they hold is ever mutated.

use std::sync::Arc;

use crate::{AssemblyDocument, PartDocument};

/// A document [`History`] can hold: cloneable, with a cheap "same data" test.
pub trait Snapshot: Clone {
    /// True when both hold the same data (shared parts may be compared by pointer).
    fn same_as(&self, other: &Self) -> bool;
}

impl Snapshot for PartDocument {
    fn same_as(&self, other: &Self) -> bool {
        PartDocument::same_as(self, other)
    }
}

impl Snapshot for AssemblyDocument {
    fn same_as(&self, other: &Self) -> bool {
        AssemblyDocument::same_as(self, other)
    }
}

#[derive(Clone, Debug)]
struct Step<D> {
    label: String,
    /// The document on the other side of this step.
    doc: Arc<D>,
}

/// The document with its undo and redo stacks.
#[derive(Clone, Debug)]
pub struct History<D = PartDocument> {
    current: Arc<D>,
    undo: Vec<Step<D>>,
    redo: Vec<Step<D>>,
    saved: Option<Arc<D>>,
    limit: Option<usize>,
}

impl<D: Snapshot> History<D> {
    /// Starts a history at `doc`, which counts as saved (just opened).
    pub fn new(doc: D) -> Self {
        let current = Arc::new(doc);
        History {
            saved: Some(current.clone()),
            current,
            undo: Vec::new(),
            redo: Vec::new(),
            limit: None,
        }
    }

    /// The current snapshot. Cheap to clone and safe to keep across later edits.
    pub fn doc(&self) -> &Arc<D> {
        &self.current
    }

    /// Keeps at most `limit` undo steps (oldest dropped first); `None` = unlimited.
    pub fn set_limit(&mut self, limit: Option<usize>) {
        self.limit = limit;
        self.trim();
    }

    /// Opens a transaction. Edit through [`Transaction::doc_mut`], then
    /// [`Transaction::commit`]; dropping it without committing discards the edits.
    pub fn begin(&mut self, label: impl Into<String>) -> Transaction<'_, D> {
        let draft = (*self.current).clone();
        Transaction {
            history: self,
            label: label.into(),
            draft,
        }
    }

    /// Runs `edit` as one transaction: committed if it returns `Ok`, discarded (document
    /// and stacks untouched) if it returns `Err` or panics.
    pub fn transact<T, E>(
        &mut self,
        label: impl Into<String>,
        edit: impl FnOnce(&mut D) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut tx = self.begin(label);
        let out = edit(tx.doc_mut())?;
        tx.commit();
        Ok(out)
    }

    /// Applies `edit` to the current document without an undo step and without making
    /// the document dirty: for automatic repairs the user did not ask for (section 3.3,
    /// legacy references re-attached by name). Earlier undo steps keep their own data.
    pub fn heal(&mut self, edit: impl FnOnce(&mut D)) {
        let mut doc = (*self.current).clone();
        edit(&mut doc);
        if doc.same_as(&self.current) {
            return;
        }
        let was_saved = !self.is_dirty();
        self.current = Arc::new(doc);
        if was_saved {
            self.saved = Some(self.current.clone());
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undoes the last step; returns its label.
    pub fn undo(&mut self) -> Option<&str> {
        let step = self.undo.pop()?;
        let doc = std::mem::replace(&mut self.current, step.doc);
        self.redo.push(Step {
            label: step.label,
            doc,
        });
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Redoes the last undone step; returns its label.
    pub fn redo(&mut self) -> Option<&str> {
        let step = self.redo.pop()?;
        let doc = std::mem::replace(&mut self.current, step.doc);
        self.undo.push(Step {
            label: step.label,
            doc,
        });
        self.undo.last().map(|s| s.label.as_str())
    }

    /// Labels of the undo stack, most recent first (the Undo drop-down list).
    pub fn undo_labels(&self) -> impl Iterator<Item = &str> {
        self.undo.iter().rev().map(|s| s.label.as_str())
    }

    /// Labels of the redo stack, next redo first.
    pub fn redo_labels(&self) -> impl Iterator<Item = &str> {
        self.redo.iter().rev().map(|s| s.label.as_str())
    }

    /// Records that the current document has been saved.
    pub fn mark_saved(&mut self) {
        self.saved = Some(self.current.clone());
    }

    /// True when the current document is not the one last saved (or opened). Undoing
    /// back to the saved snapshot makes it clean again.
    pub fn is_dirty(&self) -> bool {
        !self
            .saved
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(s, &self.current))
    }

    fn trim(&mut self) {
        if let Some(limit) = self.limit {
            let excess = self.undo.len().saturating_sub(limit);
            self.undo.drain(..excess);
        }
    }
}

/// An open edit of a [`History`]'s document.
pub struct Transaction<'a, D: Snapshot = PartDocument> {
    history: &'a mut History<D>,
    label: String,
    draft: D,
}

impl<D: Snapshot> Transaction<'_, D> {
    /// The document as edited so far (for previews).
    pub fn doc(&self) -> &D {
        &self.draft
    }

    pub fn doc_mut(&mut self) -> &mut D {
        &mut self.draft
    }

    /// Makes the edits the current document. Returns false, and records nothing, when the
    /// edits left the document unchanged.
    pub fn commit(self) -> bool {
        let Transaction {
            history,
            label,
            draft,
        } = self;
        if draft.same_as(&history.current) {
            return false;
        }
        let before = std::mem::replace(&mut history.current, Arc::new(draft));
        history.undo.push(Step { label, doc: before });
        history.redo.clear();
        history.trim();
        true
    }
}
