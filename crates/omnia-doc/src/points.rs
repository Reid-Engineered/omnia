//! Geometric incidence only: wire bodies and labels never contribute ends.

use std::collections::HashMap;

use crate::{PartId, Pt, Rect, WireId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    Wire(WireId),
    Pin { part: PartId, pin: usize },
}

#[derive(Debug, Default)]
pub(crate) struct Points(HashMap<Pt, Vec<End>>);

impl Points {
    pub(crate) fn insert(&mut self, at: Pt, end: End) {
        self.0.entry(at).or_default().push(end);
    }

    pub(crate) fn remove(&mut self, at: Pt, end: End) {
        if let Some(ends) = self.0.get_mut(&at) {
            ends.retain(|e| *e != end);
            if ends.is_empty() {
                self.0.remove(&at);
            }
        }
    }

    /// Wire ends and pins at `at`, in insertion order; empty when nothing is there.
    pub(crate) fn at(&self, at: Pt) -> &[End] {
        self.0.get(&at).map_or(&[], Vec::as_slice)
    }

    pub(crate) fn junctions(&self, area: Rect, out: &mut Vec<Pt>) {
        out.clear();
        out.extend(
            self.0
                .iter()
                .filter_map(|(&at, ends)| (ends.len() >= 3 && area.contains(at)).then_some(at)),
        );
    }
}
