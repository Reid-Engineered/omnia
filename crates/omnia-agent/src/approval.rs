//! Approval tiers, policy, effect digests and one-shot grants (AD7, AQ4, AQ7, AQ10, AINV9).
//!
//! Approvals are enforced inside `omnia-agent`, never in a transport or UI, so every surface
//! meets the same guard (AD7):
//!
//! - **Tier 0**: reads, validate, preview, `open` inside roots (always allowed).
//! - **Tier 1**: undoable document edits (allowed headless; person approves in app mode, A4).
//! - **Tier 2**: writing a new file inside roots (allowed headless; person approves in app mode).
//! - **Tier 3**: irreversible effects — overwriting an existing file or discarding unsaved
//!   changes. Under the headless default ([`Policy::headless`], AQ4), every tier 3 effect is
//!   refused with `approval_required {approval_id, tier: 3, effect, effect_digest}`, and the
//!   retry must echo both `approval_id` and `effect_digest`.
//!
//! Effect digests are deterministic 64-bit FxHashes (`rustc-hash`, AQ10) formatted as
//! `ed-<16 hex digits>`, binding the approval to the effect kind, document id, document
//! revision and target (canonical path for file effects, or `preview_id` for edits).

use std::{
    collections::BTreeMap,
    hash::Hasher,
    path::{Path, PathBuf},
};

use rustc_hash::FxHasher;

use crate::{
    AgentError, ErrorKind,
    error::Effect,
    files::Roots,
    schema::{ApprovalEcho, DocId, PreviewId},
};

/// Separates effect digests from preview ids and request hashes.
const DOMAIN: &[u8] = b"omnia.agent/1 effect";

/// Maximum pending approval challenges kept per session.
const MAX_PENDING_APPROVALS: usize = 64;

/// The four approval tiers of AD7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Read = 0,
    Edit = 1,
    WriteFile = 2,
    Destructive = 3,
}

impl Tier {
    /// Wire integer value (`0..=3`).
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

/// How a [`Policy`] handles an effect at a given tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TierDecision {
    /// Proceed without asking.
    Allow,
    /// Refuse with `approval_required`; accept a retry echoing `{approval_id, effect_digest}`.
    RequireEcho,
    /// Refuse unconditionally with `approval_denied`.
    Deny,
}

/// Service security policy: configured file roots and per-tier approval rules (AD7, AQ4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    roots: Roots,
    tier1: TierDecision,
    tier2: TierDecision,
    tier3: TierDecision,
}

impl Default for Policy {
    /// Headless policy (AQ4) with no roots configured (no files accessible until roots are set).
    fn default() -> Self {
        Self::headless(Roots::empty())
    }
}

impl Policy {
    /// The headless default policy (AD7, AQ4): tier 1 and tier 2 inside `roots` are allowed;
    /// tier 3 requires an echoed `{approval_id, effect_digest}`.
    pub fn headless(roots: Roots) -> Self {
        Self {
            roots,
            tier1: TierDecision::Allow,
            tier2: TierDecision::Allow,
            tier3: TierDecision::RequireEcho,
        }
    }

    /// A policy with explicit decisions for tiers 1, 2 and 3. Tier 0 is always allowed.
    pub fn custom(
        roots: Roots,
        tier1: TierDecision,
        tier2: TierDecision,
        tier3: TierDecision,
    ) -> Self {
        Self {
            roots,
            tier1,
            tier2,
            tier3,
        }
    }

    /// The configured file roots (`--root DIR`, AINV12).
    pub fn roots(&self) -> &Roots {
        &self.roots
    }

    /// The policy's decision for `tier`.
    pub const fn decision(&self, tier: Tier) -> TierDecision {
        match tier {
            Tier::Read => TierDecision::Allow,
            Tier::Edit => self.tier1,
            Tier::WriteFile => self.tier2,
            Tier::Destructive => self.tier3,
        }
    }
}

/// A concrete guarded effect being evaluated against [`Policy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardedEffect {
    /// Tier 1: undoable document edit bound to `preview_id` and `revision`.
    Edit {
        doc_id: DocId,
        revision: u64,
        preview_id: PreviewId,
    },
    /// Tier 2: create a new file at canonical `path` inside roots.
    WriteFile {
        doc_id: DocId,
        revision: u64,
        path: PathBuf,
    },
    /// Tier 3: overwrite an existing file at canonical `path` inside roots.
    OverwriteFile {
        doc_id: DocId,
        revision: u64,
        path: PathBuf,
    },
    /// Tier 3: close a document and discard unsaved changes.
    DiscardUnsaved { doc_id: DocId, revision: u64 },
}

impl GuardedEffect {
    /// The approval tier of this effect (AD7).
    pub const fn tier(&self) -> Tier {
        match self {
            Self::Edit { .. } => Tier::Edit,
            Self::WriteFile { .. } => Tier::WriteFile,
            Self::OverwriteFile { .. } | Self::DiscardUnsaved { .. } => Tier::Destructive,
        }
    }

    /// The wire [`Effect`] tag for `approval_required` details.
    pub const fn kind(&self) -> Effect {
        match self {
            Self::Edit { .. } => Effect::Edit,
            Self::WriteFile { .. } => Effect::WriteFile,
            Self::OverwriteFile { .. } => Effect::OverwriteFile,
            Self::DiscardUnsaved { .. } => Effect::DiscardUnsaved,
        }
    }

    /// Deterministic 64-bit FxHash digest binding this exact effect (AQ10).
    pub fn digest(&self) -> String {
        let mut h = FxHasher::default();
        write_bytes(&mut h, DOMAIN);
        match self {
            Self::Edit {
                doc_id,
                revision,
                preview_id,
            } => {
                write_bytes(&mut h, b"edit");
                h.write(&doc_id.0.to_le_bytes());
                h.write(&revision.to_le_bytes());
                h.write(&preview_id.0.to_le_bytes());
            }
            Self::WriteFile {
                doc_id,
                revision,
                path,
            } => {
                write_bytes(&mut h, b"write_file");
                h.write(&doc_id.0.to_le_bytes());
                h.write(&revision.to_le_bytes());
                write_path(&mut h, path);
            }
            Self::OverwriteFile {
                doc_id,
                revision,
                path,
            } => {
                write_bytes(&mut h, b"overwrite_file");
                h.write(&doc_id.0.to_le_bytes());
                h.write(&revision.to_le_bytes());
                write_path(&mut h, path);
            }
            Self::DiscardUnsaved { doc_id, revision } => {
                write_bytes(&mut h, b"discard_unsaved");
                h.write(&doc_id.0.to_le_bytes());
                h.write(&revision.to_le_bytes());
            }
        }
        format!("ed-{:016x}", h.finish())
    }
}

fn write_bytes(h: &mut FxHasher, bytes: &[u8]) {
    h.write(&(bytes.len() as u64).to_le_bytes());
    h.write(bytes);
}

fn write_path(h: &mut FxHasher, path: &Path) {
    write_bytes(h, path.to_string_lossy().as_bytes());
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingChallenge {
    effect: Effect,
    digest: String,
    order: u64,
}

/// Per-session store of issued one-shot approval challenges (AD7, AINV9).
///
/// Issuing the same `(effect, digest)` twice returns the existing `approval_id`, keeping
/// unapproved retries deterministic (AINV5). Matching an echo consumes the challenge so a grant
/// cannot be replayed on a second destructive operation.
#[derive(Debug, Default)]
pub struct ApprovalStore {
    next: u64,
    pending: BTreeMap<String, PendingChallenge>,
}

impl ApprovalStore {
    /// Checks `effect` against `policy` and an optional echoed `approval` (AD7, AINV9).
    ///
    /// Returns `Ok(())` when allowed or when `echo` matches and consumes a pending challenge.
    pub fn check(
        &mut self,
        policy: &Policy,
        effect: &GuardedEffect,
        echo: Option<&ApprovalEcho>,
    ) -> Result<(), AgentError> {
        let tier = effect.tier();
        match policy.decision(tier) {
            TierDecision::Allow => Ok(()),
            TierDecision::Deny => Err(AgentError::new(ErrorKind::ApprovalDenied)),
            TierDecision::RequireEcho => {
                let kind = effect.kind();
                let digest = effect.digest();
                if let Some(echo) = echo
                    && let Some(entry) = self.pending.get(&echo.approval_id)
                    && entry.effect == kind
                    && entry.digest == digest
                    && echo.effect_digest == digest
                {
                    self.pending.remove(&echo.approval_id);
                    return Ok(());
                }
                let approval_id = self.issue(kind, &digest);
                Err(AgentError::new(ErrorKind::ApprovalRequired {
                    approval_id,
                    tier: tier.as_u8(),
                    effect: kind,
                    effect_digest: digest,
                }))
            }
        }
    }

    /// Returns the existing `approval_id` for `(effect, digest)`, or issues a fresh one.
    fn issue(&mut self, effect: Effect, digest: &str) -> String {
        if let Some((id, _)) = self
            .pending
            .iter()
            .find(|(_, entry)| entry.effect == effect && entry.digest == digest)
        {
            return id.clone();
        }
        if self.pending.len() >= MAX_PENDING_APPROVALS
            && let Some(oldest) = self
                .pending
                .iter()
                .min_by_key(|(_, entry)| entry.order)
                .map(|(id, _)| id.clone())
        {
            self.pending.remove(&oldest);
        }
        self.next += 1;
        let id = format!("ap-{}", self.next);
        self.pending.insert(
            id.clone(),
            PendingChallenge {
                effect,
                digest: digest.to_owned(),
                order: self.next,
            },
        );
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    #[test]
    fn effect_digests_are_deterministic_and_bind_every_input() {
        let a = GuardedEffect::OverwriteFile {
            doc_id: DocId(1),
            revision: 3,
            path: PathBuf::from("/root/a.ron"),
        };
        let d = a.digest();
        assert!(d.starts_with("ed-") && d.len() == 19, "{d}");
        assert_eq!(d, a.digest());

        let others = [
            GuardedEffect::OverwriteFile {
                doc_id: DocId(2),
                revision: 3,
                path: PathBuf::from("/root/a.ron"),
            },
            GuardedEffect::OverwriteFile {
                doc_id: DocId(1),
                revision: 4,
                path: PathBuf::from("/root/a.ron"),
            },
            GuardedEffect::OverwriteFile {
                doc_id: DocId(1),
                revision: 3,
                path: PathBuf::from("/root/b.ron"),
            },
            GuardedEffect::WriteFile {
                doc_id: DocId(1),
                revision: 3,
                path: PathBuf::from("/root/a.ron"),
            },
            GuardedEffect::DiscardUnsaved {
                doc_id: DocId(1),
                revision: 3,
            },
            GuardedEffect::Edit {
                doc_id: DocId(1),
                revision: 3,
                preview_id: PreviewId(42),
            },
        ];
        for other in others {
            assert_ne!(other.digest(), d);
        }
    }

    #[test]
    fn ainv9_tier3_requires_matching_echo_and_consumes_grant() {
        let policy = Policy::headless(Roots::empty());
        let mut store = ApprovalStore::default();
        let overwrite = GuardedEffect::OverwriteFile {
            doc_id: DocId(1),
            revision: 2,
            path: PathBuf::from("/root/a.ron"),
        };
        let other_file = GuardedEffect::OverwriteFile {
            doc_id: DocId(1),
            revision: 2,
            path: PathBuf::from("/root/b.ron"),
        };

        // Without approval -> ApprovalRequired.
        let err1 = store.check(&policy, &overwrite, None).unwrap_err();
        let ErrorKind::ApprovalRequired {
            approval_id,
            tier,
            effect,
            effect_digest,
        } = err1.kind.clone()
        else {
            panic!("expected approval_required");
        };
        assert_eq!(tier, 3);
        assert_eq!(effect, Effect::OverwriteFile);

        // Repeating without approval returns the exact same challenge (AINV5).
        let err2 = store.check(&policy, &overwrite, None).unwrap_err();
        assert_eq!(err2, err1);

        // Wrong digest, wrong id, or wrong target all fail.
        for bad in [
            ApprovalEcho {
                approval_id: approval_id.clone(),
                effect_digest: "ed-0000000000000000".into(),
            },
            ApprovalEcho {
                approval_id: "ap-999".into(),
                effect_digest: effect_digest.clone(),
            },
        ] {
            assert_eq!(
                store
                    .check(&policy, &overwrite, Some(&bad))
                    .unwrap_err()
                    .code(),
                ErrorCode::ApprovalRequired
            );
        }
        let valid = ApprovalEcho {
            approval_id,
            effect_digest,
        };
        assert_eq!(
            store
                .check(&policy, &other_file, Some(&valid))
                .unwrap_err()
                .code(),
            ErrorCode::ApprovalRequired
        );

        // Exact echo succeeds once and is consumed.
        store.check(&policy, &overwrite, Some(&valid)).unwrap();
        assert_eq!(
            store
                .check(&policy, &overwrite, Some(&valid))
                .unwrap_err()
                .code(),
            ErrorCode::ApprovalRequired
        );
    }

    #[test]
    fn deny_policy_returns_approval_denied() {
        let policy = Policy::custom(
            Roots::empty(),
            TierDecision::Allow,
            TierDecision::Allow,
            TierDecision::Deny,
        );
        let mut store = ApprovalStore::default();
        let discard = GuardedEffect::DiscardUnsaved {
            doc_id: DocId(1),
            revision: 1,
        };
        assert_eq!(
            store.check(&policy, &discard, None).unwrap_err().code(),
            ErrorCode::ApprovalDenied
        );
    }
}
