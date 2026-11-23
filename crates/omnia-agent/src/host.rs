//! Where documents live (mcp-plan.md sections 5 and 14, AD13): the [`DocHost`] trait and the
//! headless store of milestones A1 to A3.
//!
//! The service never holds a `Doc` itself. It names documents by [`DocHandle`] and reaches them
//! through a host, so the same service runs headless on files now and, from A4, inside the app on
//! the document a person is editing (an adapter over the app's document entity). A host edits a
//! document only through `Doc::apply`, `undo` and `redo` (AD2, AINV1): those are the only three
//! mutating calls this trait has, and [`HeadlessHost`] forwards each to the `Doc` method of the
//! same name.

use std::collections::BTreeMap;

use omnia_doc::{Change, Command, CommandError, Doc};

use crate::{AgentError, ErrorKind};

/// A host's handle to one document. Handles are never reused by a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocHandle(pub u64);

/// A document store the service edits through (section 14).
///
/// Every method that names a handle the host does not hold answers `document_not_found`; the
/// inner result of [`DocHost::apply`] is the document's own verdict.
pub trait DocHost {
    /// Takes ownership of a document built by the service (`new`, and from A1.6 `open`).
    fn insert(&mut self, doc: Doc) -> Result<DocHandle, AgentError>;
    /// Drops a document (`close`).
    fn remove(&mut self, h: DocHandle) -> Result<(), AgentError>;
    /// Reads a document.
    fn doc(&self, h: DocHandle) -> Result<&Doc, AgentError>;
    /// `Doc::apply`.
    fn apply(
        &mut self,
        h: DocHandle,
        c: Command,
    ) -> Result<Result<Change, CommandError>, AgentError>;
    /// `Doc::undo`.
    fn undo(&mut self, h: DocHandle) -> Result<Option<Change>, AgentError>;
    /// `Doc::redo`.
    fn redo(&mut self, h: DocHandle) -> Result<Option<Change>, AgentError>;
    /// Whether a person's gesture is in progress on the document (INV5, AINV13). A mutating
    /// request is refused while it is. Headless: always false.
    fn gesture_active(&self, h: DocHandle) -> bool;
}

fn not_found() -> AgentError {
    AgentError::new(ErrorKind::DocumentNotFound)
}

/// The headless store (A1 to A3): documents in memory, keyed by handle, with no gestures.
#[derive(Debug, Default)]
pub struct HeadlessHost {
    docs: BTreeMap<DocHandle, Doc>,
    next: u64,
}

impl HeadlessHost {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of documents held.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    /// Whether the store holds no document.
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    fn doc_mut(&mut self, h: DocHandle) -> Result<&mut Doc, AgentError> {
        self.docs.get_mut(&h).ok_or_else(not_found)
    }
}

impl DocHost for HeadlessHost {
    fn insert(&mut self, doc: Doc) -> Result<DocHandle, AgentError> {
        let h = DocHandle(self.next);
        self.next = self.next.checked_add(1).ok_or_else(|| {
            AgentError::with_message(ErrorKind::Internal, "document handles exhausted")
        })?;
        self.docs.insert(h, doc);
        Ok(h)
    }

    fn remove(&mut self, h: DocHandle) -> Result<(), AgentError> {
        self.docs.remove(&h).map(drop).ok_or_else(not_found)
    }

    fn doc(&self, h: DocHandle) -> Result<&Doc, AgentError> {
        self.docs.get(&h).ok_or_else(not_found)
    }

    fn apply(
        &mut self,
        h: DocHandle,
        c: Command,
    ) -> Result<Result<Change, CommandError>, AgentError> {
        Ok(self.doc_mut(h)?.apply(c))
    }

    fn undo(&mut self, h: DocHandle) -> Result<Option<Change>, AgentError> {
        Ok(self.doc_mut(h)?.undo())
    }

    fn redo(&mut self, h: DocHandle) -> Result<Option<Change>, AgentError> {
        Ok(self.doc_mut(h)?.redo())
    }

    fn gesture_active(&self, _h: DocHandle) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use omnia_doc::{Label, PIN_LATTICE_NM, Pt};

    use super::*;
    use crate::ErrorCode;

    fn label() -> Command {
        Command::AddLabel(Label {
            at: Pt {
                x: PIN_LATTICE_NM,
                y: 0,
            },
            text: "N".into(),
        })
    }

    #[test]
    fn headless_host_forwards_to_the_document_and_never_reuses_handles() {
        let mut host = HeadlessHost::new();
        let a = host.insert(Doc::new()).unwrap();
        let change = host.apply(a, label()).unwrap().unwrap();
        assert_eq!(change.revision, 1);
        assert_eq!(host.doc(a).unwrap().revision(), 1);
        assert!(host.undo(a).unwrap().is_some());
        assert!(host.undo(a).unwrap().is_none());
        assert!(host.redo(a).unwrap().is_some());
        assert!(!host.gesture_active(a));
        host.remove(a).unwrap();
        let b = host.insert(Doc::new()).unwrap();
        assert_ne!(a, b, "a handle is never reused");
        assert_eq!(host.len(), 1);
        for err in [
            host.doc(a).unwrap_err(),
            host.remove(a).unwrap_err(),
            host.apply(a, label()).unwrap_err(),
            host.undo(a).unwrap_err(),
            host.redo(a).unwrap_err(),
        ] {
            assert_eq!(err.code(), ErrorCode::DocumentNotFound);
        }
    }
}
