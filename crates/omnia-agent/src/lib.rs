//! Transport-neutral agent service for Omnia (mcp-plan.md).
//!
//! A1.3 delivers the wire contract: the typed DTOs of schema version [`SCHEMA`] (AK7), the
//! id, point and orientation codecs (AD12), the structured errors with their exhaustive
//! `CommandError` mapping (AD6, AINV10), the capability catalog that is the dispatch table, and
//! the hand-written JSON Schema module whose output is committed under `schema/` (AINV11).
//!
//! A1.4 adds the read projections (`read`). A1.5 adds the service core: sessions and the
//! headless document host (`host`), operation-to-`Command` conversion (`ops`), fork preview
//! (`preview`), the dirty-bounded diff (`diff`) and the service itself (`service`). A1.6 adds
//! root-jailed file open and staged save (`files`) and the tier policy and effect-digest
//! approvals (`approval`).
//!
//! The crate has no UI framework and no async runtime (AINV8). It edits documents only through
//! `omnia_doc::Doc::apply`, `undo` and `redo` (AINV1).

pub mod approval;
pub mod catalog;
pub mod diff;
pub mod error;
pub mod files;
pub mod host;
pub mod ops;
pub mod preview;
pub mod read;
pub mod schema;
pub mod service;

pub use approval::{GuardedEffect, Policy, Tier, TierDecision};
pub use catalog::{Catalog, EditOpName, OpName, OpSpec};
pub use error::{AgentError, ErrorCode, ErrorKind};
pub use files::Roots;
pub use host::{DocHandle, DocHost, HeadlessHost};
pub use schema::{Operation, Output, Request, Response};
pub use service::{AgentService, Form, SessionId, in_form};

/// Schema version string carried by every response (AK7).
pub const SCHEMA: &str = "omnia.agent/1";

/// Protocol revisions pinned at A0.2, newest first (AK1).
pub const PROTOCOL_REVISIONS: [&str; 3] = ["2026-07-28", "2025-11-25", "2025-06-18"];

/// Largest request accepted, in bytes (AK2).
pub const MAX_REQUEST_BYTES: u64 = 8 * 1024 * 1024;

/// Largest file opened, in bytes (AK2).
pub const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;

/// Most edit operations in one request (AK3).
pub const MAX_OPERATIONS: u64 = 4_096;

/// Page size used when a request names none (AK4).
pub const DEFAULT_PAGE_SIZE: u32 = 500;

/// Largest page size a request may ask for (AK4).
pub const MAX_PAGE_SIZE: u32 = 5_000;

/// Idempotency cache entries per session, in memory (AK5, AQ11).
pub const IDEMPOTENCY_CACHE_ENTRIES: u32 = 256;

/// Open documents per session (AK6).
pub const MAX_OPEN_DOCUMENTS: u32 = 8;
