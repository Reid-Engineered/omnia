//! Fork preview (AD3) and preview identity (mcp-plan.md section 11.3).
//!
//! A preview forks the document (`Doc::fork`, A1.1) and applies the request's one `Command` to
//! the fork with `Doc::apply`, the commit's own code. The fork copies every reserved slot and the
//! revision, so applying the same command to the live document at the same revision allocates
//! the same ids and returns the same `Change` (AINV2). The live document is only read: [`run`]
//! takes `&Doc` (AINV3).
//!
//! [`preview_id`] binds a commit to its preview. It is a 64-bit FxHash (`rustc-hash`, AQ10) of
//! a domain tag, the `doc_id`, the base revision, the canonical bytes of the operations and the
//! wiring preferences that shape their commands (the follow policy and `delete_wires_with_part`,
//! see `ops`). `apply` with a `preview_id` hashes its own request the same way, and a difference
//! is `preview_mismatch`. Nothing is stored: the same inputs always give the same id, and an id
//! that matches is one a preview of exactly this request would have returned. It is not a
//! security token.

use std::hash::Hasher;

use omnia_doc::{Change, Command, CommandError, Doc, prefs::WiringPrefs};
use rustc_hash::FxHasher;

use crate::schema::{DocId, EditOp, PreviewId};

/// Separates preview hashes from any other use of the same hasher.
const DOMAIN: &[u8] = b"omnia.agent/1 preview";

/// The preview id of `operations` on `doc_id` at `base_revision` under `prefs` (section 11.3).
pub fn preview_id(
    doc_id: DocId,
    base_revision: u64,
    operations: &[EditOp],
    prefs: &WiringPrefs,
) -> PreviewId {
    let ops = serde_json::to_vec(operations).expect("operations always serialize");
    let policy = prefs.follow_policy(false);
    let mut h = FxHasher::default();
    // Each field is length-prefixed or fixed-width, so no two inputs share a byte stream.
    h.write(&(DOMAIN.len() as u64).to_le_bytes());
    h.write(DOMAIN);
    h.write(&doc_id.0.to_le_bytes());
    h.write(&base_revision.to_le_bytes());
    h.write(&(ops.len() as u64).to_le_bytes());
    h.write(&ops);
    h.write(&[
        u8::from(policy.follow_on_move),
        u8::from(policy.invert),
        u8::from(prefs.delete_wires_with_part),
    ]);
    h.write(&policy.pin_threshold().to_le_bytes());
    PreviewId(h.finish())
}

/// Applies `cmd` to a fork of `doc` and returns the fork and the `Change`; `doc` is untouched.
pub fn run(doc: &Doc, cmd: Command) -> Result<(Doc, Change), CommandError> {
    let mut fork = doc.fork();
    let change = fork.apply(cmd)?;
    Ok((fork, change))
}

#[cfg(test)]
mod tests {
    use omnia_doc::{Label, PIN_LATTICE_NM, Pt, prefs::SelectMode};

    use super::*;
    use crate::schema::Point;

    fn label(x: i32) -> EditOp {
        EditOp::AddLabel {
            at: Point(Pt {
                x: x * PIN_LATTICE_NM,
                y: 0,
            }),
            text: "N".into(),
        }
    }

    #[test]
    fn preview_ids_are_deterministic_and_bind_every_input() {
        let prefs = WiringPrefs::default();
        let ops = [label(1)];
        let id = preview_id(DocId(1), 4, &ops, &prefs);
        assert_eq!(id, preview_id(DocId(1), 4, &ops, &prefs));
        let text = id.to_string();
        assert!(text.starts_with("pv-") && text.len() == 19, "{text}");
        assert_eq!(text.parse::<PreviewId>().unwrap(), id);
        let others = [
            preview_id(DocId(2), 4, &ops, &prefs),
            preview_id(DocId(1), 5, &ops, &prefs),
            preview_id(DocId(1), 4, &[label(2)], &prefs),
            preview_id(DocId(1), 4, &[label(1), label(1)], &prefs),
            preview_id(
                DocId(1),
                4,
                &ops,
                &WiringPrefs::new(false, 10, SelectMode::Intersecting, false).unwrap(),
            ),
            preview_id(
                DocId(1),
                4,
                &ops,
                &WiringPrefs::new(true, 3, SelectMode::Intersecting, false).unwrap(),
            ),
            preview_id(
                DocId(1),
                4,
                &ops,
                &WiringPrefs::new(true, 10, SelectMode::Intersecting, true).unwrap(),
            ),
        ];
        for other in others {
            assert_ne!(other, id);
        }
    }

    #[test]
    fn run_leaves_the_document_untouched_and_matches_the_commit() {
        let mut doc = Doc::new();
        let cmd = Command::AddLabel(Label {
            at: Pt::ZERO,
            text: "A".into(),
        });
        doc.apply(cmd.clone()).unwrap();
        let before = doc.fork();
        let (fork, change) = run(&doc, cmd.clone()).unwrap();
        assert!(doc == before && doc.can_undo());
        assert_eq!(doc.apply(cmd).unwrap(), change);
        assert!(doc == fork);
    }
}
