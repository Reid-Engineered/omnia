//! Transport-neutral agent service for Omnia (mcp-plan.md).
//!
//! A1.3 delivers the wire contract: the typed DTOs of schema version [`SCHEMA`] (AK7), the
//! id, point and orientation codecs (AD12), the structured errors with their exhaustive
//! `CommandError` mapping (AD6, AINV10), the capability catalog that is the dispatch table, and
//! the hand-written JSON Schema module whose output is committed under `schema/` (AINV11).
//!
//! The crate has no UI framework and no async runtime (AINV8). It edits documents only through
//! `omnia_doc::Doc::apply`, `undo` and `redo` (AINV1); the service that does so is A1.5.

pub mod catalog;
pub mod error;
pub mod read;
pub mod schema;

pub use catalog::{Catalog, EditOpName, OpName, OpSpec};
pub use error::{AgentError, ErrorCode, ErrorKind};
pub use schema::{Operation, Output, Request, Response};

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
