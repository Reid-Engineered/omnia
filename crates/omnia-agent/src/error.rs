//! Structured errors (AD6, mcp-plan.md section 9.4) and the `CommandError` mapping (AINV10).
//!
//! On the wire an error is `{code, message, retryable, details, hint?}`. `code` is a stable
//! snake_case string, `details` an object typed by the code (`{}` when the code has none), and
//! `message` and `hint` are written by Omnia from ids and numbers only, never from document text
//! (AD14).

use std::fmt;

use omnia_doc::{CommandError, FollowError, route::Blocker};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, DeserializeOwned},
    ser::SerializeStruct,
};

use crate::schema::{ItemRef, PartRef, Point, TextFieldName, WireRef};

macro_rules! error_codes {
    ($($variant:ident => $code:literal, $retryable:literal;)*) => {
        /// Every error code of schema version 1, in catalog order.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ErrorCode {
            $($variant,)*
        }

        impl ErrorCode {
            /// Every code, in catalog order. Generated from the same list as the enum.
            pub const ALL: &'static [ErrorCode] = &[$(ErrorCode::$variant,)*];

            /// The stable wire spelling.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(ErrorCode::$variant => $code,)*
                }
            }

            /// Whether the same intent may succeed if retried, possibly rebuilt (`retryable`).
            pub const fn retryable(self) -> bool {
                match self {
                    $(ErrorCode::$variant => $retryable,)*
                }
            }
        }
    };
}

error_codes! {
    // Protocol and schema.
    InvalidRequest => "invalid_request", false;
    SchemaViolation => "schema_violation", false;
    UnknownOperation => "unknown_operation", false;
    LimitExceeded => "limit_exceeded", false;
    UnsupportedInMode => "unsupported_in_mode", false;
    ProtocolVersionUnsupported => "protocol_version_unsupported", false;
    // Resolution.
    DocumentNotFound => "document_not_found", false;
    UnknownSymbol => "unknown_symbol", false;
    UnknownItem => "unknown_item", false;
    // From `CommandError` (AINV10).
    OutOfBounds => "out_of_bounds", false;
    InvalidWire => "invalid_wire", false;
    MissingItem => "missing_item", false;
    DuplicateItem => "duplicate_item", false;
    InvalidTextField => "invalid_text_field", false;
    FollowNoRoute => "follow_no_route", false;
    SegmentBlocked => "segment_blocked", false;
    WireCapture => "wire_capture", false;
    OffLattice => "off_lattice", false;
    // Concurrency.
    StaleRevision => "stale_revision", true;
    PreviewMismatch => "preview_mismatch", false;
    IdempotencyConflict => "idempotency_conflict", false;
    UserGestureInProgress => "user_gesture_in_progress", true;
    NothingToUndo => "nothing_to_undo", false;
    NothingToRedo => "nothing_to_redo", false;
    // Approval.
    ApprovalRequired => "approval_required", true;
    ApprovalDenied => "approval_denied", false;
    // Files.
    PathOutsideRoot => "path_outside_root", false;
    FileExists => "file_exists", false;
    IoError => "io_error", true;
    UnsupportedFileVersion => "unsupported_file_version", false;
    // Jobs.
    Cancelled => "cancelled", false;
    JobNotFound => "job_not_found", false;
    // Other.
    Internal => "internal", false;
}

impl ErrorCode {
    /// Finds a code by its wire spelling.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|code| code.as_str() == name)
    }

    /// A fixed hint for the caller, or none (AD14: static text only).
    pub const fn hint(self) -> Option<&'static str> {
        match self {
            Self::StaleRevision => Some("read the document again and rebuild the request"),
            Self::PreviewMismatch => Some("preview the exact operations again, then apply"),
            Self::IdempotencyConflict => Some("use a new idempotency key for a different request"),
            Self::UserGestureInProgress => Some("retry after the current gesture ends"),
            Self::ApprovalRequired => Some("retry with `approval` echoing the effect digest"),
            Self::UnknownOperation => Some("call `capabilities` for the operation list"),
            Self::UnknownSymbol => Some("read `omnia://symbols` for the symbol names"),
            _ => None,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::from_name(&name)
            .ok_or_else(|| de::Error::invalid_value(de::Unexpected::Str(&name), &"an error code"))
    }
}

/// The limit a request exceeded (`limit_exceeded {limit, value}`), named after AK2 to AK6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Limit {
    /// AK2, request size in bytes.
    RequestBytes,
    /// AK2, file size on open in bytes.
    FileBytes,
    /// AK3, operations per request.
    Operations,
    /// AK4, page size.
    PageSize,
    /// AK6, open documents per session.
    OpenDocuments,
}

/// What an approval covers (`approval_required {effect}`, AD7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// Tier 1 in app mode: an undoable document edit.
    Edit,
    /// Tier 2: write a new file inside the roots.
    WriteFile,
    /// Tier 3: overwrite an existing file.
    OverwriteFile,
    /// Tier 3: discard unsaved changes.
    DiscardUnsaved,
}

/// What stopped a route, a segment drag or a moved wire (`omnia_doc::route::Blocker`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BlockerDto {
    /// A part's pin, by part and pin index.
    Pin { part: PartRef, pin: u32 },
    /// A foreign wire's end.
    WireEnd { wire: WireRef },
    /// A foreign wire's interior.
    WireBody { wire: WireRef },
    /// A foreign wire sharing a stretch.
    Overlap { wire: WireRef },
    /// Outside K1 or off the WK1 lattice.
    Sheet,
}

impl From<Blocker> for BlockerDto {
    /// Exhaustive: a new `Blocker` variant fails to compile until it has a wire form.
    fn from(blocker: Blocker) -> Self {
        match blocker {
            Blocker::Pin { part, pin } => Self::Pin {
                part: part.into(),
                pin: u32::try_from(pin).unwrap_or(u32::MAX),
            },
            Blocker::WireEnd(wire) => Self::WireEnd { wire: wire.into() },
            Blocker::WireBody(wire) => Self::WireBody { wire: wire.into() },
            Blocker::Overlap(wire) => Self::Overlap { wire: wire.into() },
            Blocker::Sheet => Self::Sheet,
        }
    }
}

impl fmt::Display for BlockerDto {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pin { part, pin } => write!(f, "pin {pin} of {part}"),
            Self::WireEnd { wire } => write!(f, "the end of {wire}"),
            Self::WireBody { wire } => write!(f, "the interior of {wire}"),
            Self::Overlap { wire } => write!(f, "an overlap with {wire}"),
            Self::Sheet => f.write_str("the sheet limit or the pin lattice"),
        }
    }
}

/// An error code with its typed details. Codes without details carry nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidRequest,
    /// `path` is a JSON Pointer into the request (`""` for the whole request).
    SchemaViolation {
        path: String,
    },
    UnknownOperation,
    LimitExceeded {
        limit: Limit,
        value: u64,
    },
    UnsupportedInMode,
    ProtocolVersionUnsupported,
    DocumentNotFound,
    UnknownSymbol,
    UnknownItem,
    OutOfBounds,
    InvalidWire,
    MissingItem {
        item: ItemRef,
    },
    DuplicateItem {
        item: ItemRef,
    },
    InvalidTextField {
        item: ItemRef,
        field: TextFieldName,
    },
    FollowNoRoute {
        from: Point,
        to: Point,
        blocker: BlockerDto,
    },
    SegmentBlocked {
        blocker: BlockerDto,
    },
    WireCapture {
        wire: WireRef,
        blocker: BlockerDto,
    },
    OffLattice {
        at: Point,
    },
    StaleRevision {
        expected: u64,
        actual: u64,
    },
    PreviewMismatch,
    IdempotencyConflict,
    UserGestureInProgress,
    NothingToUndo,
    NothingToRedo,
    ApprovalRequired {
        approval_id: String,
        tier: u8,
        effect: Effect,
        effect_digest: String,
    },
    ApprovalDenied,
    PathOutsideRoot,
    FileExists,
    IoError,
    UnsupportedFileVersion,
    Cancelled,
    JobNotFound,
    Internal,
}

impl ErrorKind {
    /// The code of this kind. Exhaustive, with no wildcard arm.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidRequest => ErrorCode::InvalidRequest,
            Self::SchemaViolation { .. } => ErrorCode::SchemaViolation,
            Self::UnknownOperation => ErrorCode::UnknownOperation,
            Self::LimitExceeded { .. } => ErrorCode::LimitExceeded,
            Self::UnsupportedInMode => ErrorCode::UnsupportedInMode,
            Self::ProtocolVersionUnsupported => ErrorCode::ProtocolVersionUnsupported,
            Self::DocumentNotFound => ErrorCode::DocumentNotFound,
            Self::UnknownSymbol => ErrorCode::UnknownSymbol,
            Self::UnknownItem => ErrorCode::UnknownItem,
            Self::OutOfBounds => ErrorCode::OutOfBounds,
            Self::InvalidWire => ErrorCode::InvalidWire,
            Self::MissingItem { .. } => ErrorCode::MissingItem,
            Self::DuplicateItem { .. } => ErrorCode::DuplicateItem,
            Self::InvalidTextField { .. } => ErrorCode::InvalidTextField,
            Self::FollowNoRoute { .. } => ErrorCode::FollowNoRoute,
            Self::SegmentBlocked { .. } => ErrorCode::SegmentBlocked,
            Self::WireCapture { .. } => ErrorCode::WireCapture,
            Self::OffLattice { .. } => ErrorCode::OffLattice,
            Self::StaleRevision { .. } => ErrorCode::StaleRevision,
            Self::PreviewMismatch => ErrorCode::PreviewMismatch,
            Self::IdempotencyConflict => ErrorCode::IdempotencyConflict,
            Self::UserGestureInProgress => ErrorCode::UserGestureInProgress,
            Self::NothingToUndo => ErrorCode::NothingToUndo,
            Self::NothingToRedo => ErrorCode::NothingToRedo,
            Self::ApprovalRequired { .. } => ErrorCode::ApprovalRequired,
            Self::ApprovalDenied => ErrorCode::ApprovalDenied,
            Self::PathOutsideRoot => ErrorCode::PathOutsideRoot,
            Self::FileExists => ErrorCode::FileExists,
            Self::IoError => ErrorCode::IoError,
            Self::UnsupportedFileVersion => ErrorCode::UnsupportedFileVersion,
            Self::Cancelled => ErrorCode::Cancelled,
            Self::JobNotFound => ErrorCode::JobNotFound,
            Self::Internal => ErrorCode::Internal,
        }
    }

    /// The default message, written from the details only (AD14).
    pub fn default_message(&self) -> String {
        match self {
            Self::InvalidRequest => "the request is not valid JSON of the expected shape".into(),
            Self::SchemaViolation { path } => {
                format!("the request does not match the schema at `{path}`")
            }
            Self::UnknownOperation => "the operation is not in the catalog".into(),
            Self::LimitExceeded { limit, value } => {
                format!("{} is {value}, over its limit", limit_name(*limit))
            }
            Self::UnsupportedInMode => "the operation is not available in this mode".into(),
            Self::ProtocolVersionUnsupported => "the protocol revision is not supported".into(),
            Self::DocumentNotFound => "no open document has this id".into(),
            Self::UnknownSymbol => "no symbol in the library has this name".into(),
            Self::UnknownItem => "no item has this id".into(),
            Self::OutOfBounds => "an item or pin would lie outside the sheet limits".into(),
            Self::InvalidWire => "a wire must be axis-aligned with distinct ends".into(),
            Self::MissingItem { item } => format!("item {item} does not exist"),
            Self::DuplicateItem { item } => format!("item {item} appears twice"),
            Self::InvalidTextField { item, field } => {
                format!("item {item} has no `{field}` text field")
            }
            Self::FollowNoRoute { from, to, blocker } => format!(
                "no route from [{}, {}] to [{}, {}]: blocked by {blocker}",
                from.0.x, from.0.y, to.0.x, to.0.y
            ),
            Self::SegmentBlocked { blocker } => format!("segment drag blocked by {blocker}"),
            Self::WireCapture { wire, blocker } => {
                format!("moved {wire} would capture {blocker}")
            }
            Self::OffLattice { at } => format!(
                "the edit would put a wire point off the pin lattice at [{}, {}]",
                at.0.x, at.0.y
            ),
            Self::StaleRevision { expected, actual } => {
                format!("expected revision {expected}, document is at {actual}")
            }
            Self::PreviewMismatch => "the preview id does not match these operations".into(),
            Self::IdempotencyConflict => {
                "the idempotency key was used for a different request".into()
            }
            Self::UserGestureInProgress => "a gesture is in progress on this document".into(),
            Self::NothingToUndo => "there is nothing to undo".into(),
            Self::NothingToRedo => "there is nothing to redo".into(),
            Self::ApprovalRequired { tier, .. } => {
                format!("this tier {tier} effect needs an approval")
            }
            Self::ApprovalDenied => "the approval was denied".into(),
            Self::PathOutsideRoot => "the path is outside the configured roots".into(),
            Self::FileExists => "the file already exists".into(),
            Self::IoError => "the file could not be read or written".into(),
            Self::UnsupportedFileVersion => "the file version is not supported".into(),
            Self::Cancelled => "the work was cancelled".into(),
            Self::JobNotFound => "no job has this id".into(),
            Self::Internal => "internal error".into(),
        }
    }
}

const fn limit_name(limit: Limit) -> &'static str {
    match limit {
        Limit::RequestBytes => "the request size in bytes",
        Limit::FileBytes => "the file size in bytes",
        Limit::Operations => "the operation count",
        Limit::PageSize => "the page size",
        Limit::OpenDocuments => "the open document count",
    }
}

/// A structured error (AD6, section 9.4).
///
/// `retryable` is not stored: the wire flag is always [`ErrorCode::retryable`] of the code, and
/// decoding refuses a flag that contradicts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentError {
    pub kind: ErrorKind,
    pub message: String,
    pub hint: Option<String>,
}

impl AgentError {
    /// An error with the kind's default message, retryability and hint.
    pub fn new(kind: ErrorKind) -> Self {
        let message = kind.default_message();
        Self::with_message(kind, message)
    }

    /// An error with its own message; retryability and hint come from the code.
    pub fn with_message(kind: ErrorKind, message: impl Into<String>) -> Self {
        let code = kind.code();
        Self {
            kind,
            message: message.into(),
            hint: code.hint().map(str::to_owned),
        }
    }

    /// The error's code.
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }

    /// The wire `retryable` flag, fixed by the code.
    pub const fn retryable(&self) -> bool {
        self.code().retryable()
    }
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message)
    }
}

impl std::error::Error for AgentError {}

impl From<CommandError> for AgentError {
    fn from(err: CommandError) -> Self {
        Self::new(command_error_kind(err))
    }
}

/// AINV10: maps every `CommandError` variant to its own code. The `match` has no wildcard arm,
/// so a new variant fails to compile until it is given a code here.
pub fn command_error_kind(err: CommandError) -> ErrorKind {
    match err {
        CommandError::OutOfBounds => ErrorKind::OutOfBounds,
        CommandError::InvalidWire => ErrorKind::InvalidWire,
        CommandError::MissingItem(item) => ErrorKind::MissingItem { item: item.into() },
        CommandError::DuplicateItem(item) => ErrorKind::DuplicateItem { item: item.into() },
        CommandError::InvalidTextField { item, field } => ErrorKind::InvalidTextField {
            item: item.into(),
            field: TextFieldName(field),
        },
        CommandError::Segment(blocker) => ErrorKind::SegmentBlocked {
            blocker: blocker.into(),
        },
        CommandError::Follow(FollowError::NoRoute { from, to, blocker }) => {
            ErrorKind::FollowNoRoute {
                from: from.into(),
                to: to.into(),
                blocker: blocker.into(),
            }
        }
        CommandError::Capture { wire, blocker } => ErrorKind::WireCapture {
            wire: wire.into(),
            blocker: blocker.into(),
        },
        CommandError::OffLattice(at) => ErrorKind::OffLattice { at: at.into() },
    }
}

// ---- wire form -------------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoDetails {}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PathDetails {
    path: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitDetails {
    limit: Limit,
    value: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemDetails {
    item: ItemRef,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextFieldDetails {
    item: ItemRef,
    field: TextFieldName,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoRouteDetails {
    from: Point,
    to: Point,
    blocker: BlockerDto,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockerDetails {
    blocker: BlockerDto,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureDetails {
    wire: WireRef,
    blocker: BlockerDto,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AtDetails {
    at: Point,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionDetails {
    expected: u64,
    actual: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalDetails {
    approval_id: String,
    tier: u8,
    effect: Effect,
    effect_digest: String,
}

/// Serializes the `details` object of a kind, in field order.
struct Details<'a>(&'a ErrorKind);

impl Serialize for Details<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0.clone() {
            ErrorKind::SchemaViolation { path } => PathDetails { path }.serialize(s),
            ErrorKind::LimitExceeded { limit, value } => LimitDetails { limit, value }.serialize(s),
            ErrorKind::MissingItem { item } | ErrorKind::DuplicateItem { item } => {
                ItemDetails { item }.serialize(s)
            }
            ErrorKind::InvalidTextField { item, field } => {
                TextFieldDetails { item, field }.serialize(s)
            }
            ErrorKind::FollowNoRoute { from, to, blocker } => {
                NoRouteDetails { from, to, blocker }.serialize(s)
            }
            ErrorKind::SegmentBlocked { blocker } => BlockerDetails { blocker }.serialize(s),
            ErrorKind::WireCapture { wire, blocker } => {
                CaptureDetails { wire, blocker }.serialize(s)
            }
            ErrorKind::OffLattice { at } => AtDetails { at }.serialize(s),
            ErrorKind::StaleRevision { expected, actual } => {
                RevisionDetails { expected, actual }.serialize(s)
            }
            ErrorKind::ApprovalRequired {
                approval_id,
                tier,
                effect,
                effect_digest,
            } => ApprovalDetails {
                approval_id,
                tier,
                effect,
                effect_digest,
            }
            .serialize(s),
            ErrorKind::InvalidRequest
            | ErrorKind::UnknownOperation
            | ErrorKind::UnsupportedInMode
            | ErrorKind::ProtocolVersionUnsupported
            | ErrorKind::DocumentNotFound
            | ErrorKind::UnknownSymbol
            | ErrorKind::UnknownItem
            | ErrorKind::OutOfBounds
            | ErrorKind::InvalidWire
            | ErrorKind::PreviewMismatch
            | ErrorKind::IdempotencyConflict
            | ErrorKind::UserGestureInProgress
            | ErrorKind::NothingToUndo
            | ErrorKind::NothingToRedo
            | ErrorKind::ApprovalDenied
            | ErrorKind::PathOutsideRoot
            | ErrorKind::FileExists
            | ErrorKind::IoError
            | ErrorKind::UnsupportedFileVersion
            | ErrorKind::Cancelled
            | ErrorKind::JobNotFound
            | ErrorKind::Internal => NoDetails {}.serialize(s),
        }
    }
}

fn details<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, serde_json::Error> {
    serde_json::from_value(value)
}

fn unit(value: serde_json::Value, kind: ErrorKind) -> Result<ErrorKind, serde_json::Error> {
    details::<NoDetails>(value).map(|_| kind)
}

/// Decodes the `details` object of `code`. Exhaustive over `ErrorCode`, with no wildcard arm.
fn decode_kind(code: ErrorCode, value: serde_json::Value) -> Result<ErrorKind, serde_json::Error> {
    let kind = match code {
        ErrorCode::SchemaViolation => {
            let d: PathDetails = details(value)?;
            ErrorKind::SchemaViolation { path: d.path }
        }
        ErrorCode::LimitExceeded => {
            let d: LimitDetails = details(value)?;
            ErrorKind::LimitExceeded {
                limit: d.limit,
                value: d.value,
            }
        }
        ErrorCode::MissingItem => ErrorKind::MissingItem {
            item: details::<ItemDetails>(value)?.item,
        },
        ErrorCode::DuplicateItem => ErrorKind::DuplicateItem {
            item: details::<ItemDetails>(value)?.item,
        },
        ErrorCode::InvalidTextField => {
            let d: TextFieldDetails = details(value)?;
            ErrorKind::InvalidTextField {
                item: d.item,
                field: d.field,
            }
        }
        ErrorCode::FollowNoRoute => {
            let d: NoRouteDetails = details(value)?;
            ErrorKind::FollowNoRoute {
                from: d.from,
                to: d.to,
                blocker: d.blocker,
            }
        }
        ErrorCode::SegmentBlocked => ErrorKind::SegmentBlocked {
            blocker: details::<BlockerDetails>(value)?.blocker,
        },
        ErrorCode::WireCapture => {
            let d: CaptureDetails = details(value)?;
            ErrorKind::WireCapture {
                wire: d.wire,
                blocker: d.blocker,
            }
        }
        ErrorCode::OffLattice => ErrorKind::OffLattice {
            at: details::<AtDetails>(value)?.at,
        },
        ErrorCode::StaleRevision => {
            let d: RevisionDetails = details(value)?;
            ErrorKind::StaleRevision {
                expected: d.expected,
                actual: d.actual,
            }
        }
        ErrorCode::ApprovalRequired => {
            let d: ApprovalDetails = details(value)?;
            ErrorKind::ApprovalRequired {
                approval_id: d.approval_id,
                tier: d.tier,
                effect: d.effect,
                effect_digest: d.effect_digest,
            }
        }
        ErrorCode::InvalidRequest => unit(value, ErrorKind::InvalidRequest)?,
        ErrorCode::UnknownOperation => unit(value, ErrorKind::UnknownOperation)?,
        ErrorCode::UnsupportedInMode => unit(value, ErrorKind::UnsupportedInMode)?,
        ErrorCode::ProtocolVersionUnsupported => {
            unit(value, ErrorKind::ProtocolVersionUnsupported)?
        }
        ErrorCode::DocumentNotFound => unit(value, ErrorKind::DocumentNotFound)?,
        ErrorCode::UnknownSymbol => unit(value, ErrorKind::UnknownSymbol)?,
        ErrorCode::UnknownItem => unit(value, ErrorKind::UnknownItem)?,
        ErrorCode::OutOfBounds => unit(value, ErrorKind::OutOfBounds)?,
        ErrorCode::InvalidWire => unit(value, ErrorKind::InvalidWire)?,
        ErrorCode::PreviewMismatch => unit(value, ErrorKind::PreviewMismatch)?,
        ErrorCode::IdempotencyConflict => unit(value, ErrorKind::IdempotencyConflict)?,
        ErrorCode::UserGestureInProgress => unit(value, ErrorKind::UserGestureInProgress)?,
        ErrorCode::NothingToUndo => unit(value, ErrorKind::NothingToUndo)?,
        ErrorCode::NothingToRedo => unit(value, ErrorKind::NothingToRedo)?,
        ErrorCode::ApprovalDenied => unit(value, ErrorKind::ApprovalDenied)?,
        ErrorCode::PathOutsideRoot => unit(value, ErrorKind::PathOutsideRoot)?,
        ErrorCode::FileExists => unit(value, ErrorKind::FileExists)?,
        ErrorCode::IoError => unit(value, ErrorKind::IoError)?,
        ErrorCode::UnsupportedFileVersion => unit(value, ErrorKind::UnsupportedFileVersion)?,
        ErrorCode::Cancelled => unit(value, ErrorKind::Cancelled)?,
        ErrorCode::JobNotFound => unit(value, ErrorKind::JobNotFound)?,
        ErrorCode::Internal => unit(value, ErrorKind::Internal)?,
    };
    Ok(kind)
}

impl Serialize for AgentError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let len = if self.hint.is_some() { 5 } else { 4 };
        let mut s = serializer.serialize_struct("AgentError", len)?;
        s.serialize_field("code", &self.code())?;
        s.serialize_field("message", &self.message)?;
        s.serialize_field("retryable", &self.retryable())?;
        s.serialize_field("details", &Details(&self.kind))?;
        if let Some(hint) = &self.hint {
            s.serialize_field("hint", hint)?;
        }
        s.end()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ErrorWire {
    code: ErrorCode,
    message: String,
    retryable: bool,
    details: serde_json::Value,
    #[serde(default)]
    hint: Option<String>,
}

impl<'de> Deserialize<'de> for AgentError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ErrorWire::deserialize(deserializer)?;
        if wire.retryable != wire.code.retryable() {
            return Err(de::Error::custom(format_args!(
                "`retryable` of {} is {}",
                wire.code,
                wire.code.retryable()
            )));
        }
        let kind = decode_kind(wire.code, wire.details).map_err(de::Error::custom)?;
        debug_assert_eq!(kind.code(), wire.code);
        Ok(Self {
            kind,
            message: wire.message,
            hint: wire.hint,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use omnia_doc::{ItemId, LabelId, PartId, Pt, TextField, WireId};

    use super::*;

    /// One sample of every `CommandError` variant, and of every nested `FollowError` and
    /// `Blocker` variant. `variant_index` has no wildcard arm, so a new variant breaks this test
    /// until it gets a sample.
    fn command_error_samples() -> Vec<CommandError> {
        let blockers = [
            Blocker::Pin {
                part: PartId(1),
                pin: 2,
            },
            Blocker::WireEnd(WireId(3)),
            Blocker::WireBody(WireId(4)),
            Blocker::Overlap(WireId(5)),
            Blocker::Sheet,
        ];
        let mut samples = vec![
            CommandError::OutOfBounds,
            CommandError::InvalidWire,
            CommandError::MissingItem(ItemId::Part(PartId(7))),
            CommandError::DuplicateItem(ItemId::Label(LabelId(8))),
            CommandError::InvalidTextField {
                item: ItemId::Wire(WireId(9)),
                field: TextField::Value,
            },
            CommandError::OffLattice(Pt { x: 1, y: -1 }),
        ];
        for blocker in blockers {
            samples.push(CommandError::Segment(blocker));
            samples.push(CommandError::Follow(FollowError::NoRoute {
                from: Pt { x: 0, y: 0 },
                to: Pt { x: 10, y: 0 },
                blocker,
            }));
            samples.push(CommandError::Capture {
                wire: WireId(6),
                blocker,
            });
        }
        samples
    }

    const COMMAND_ERROR_VARIANTS: usize = 9;

    fn variant_index(err: &CommandError) -> usize {
        match err {
            CommandError::OutOfBounds => 0,
            CommandError::InvalidWire => 1,
            CommandError::MissingItem(_) => 2,
            CommandError::DuplicateItem(_) => 3,
            CommandError::InvalidTextField { .. } => 4,
            CommandError::Segment(_) => 5,
            CommandError::Follow(FollowError::NoRoute { .. }) => 6,
            CommandError::Capture { .. } => 7,
            CommandError::OffLattice(_) => 8,
        }
    }

    #[test]
    fn ainv10_every_command_error_variant_has_its_own_code() {
        let samples = command_error_samples();
        let mut code_of_variant = vec![None; COMMAND_ERROR_VARIANTS];
        for sample in &samples {
            let code = AgentError::from(*sample).code();
            let slot = &mut code_of_variant[variant_index(sample)];
            assert!(
                slot.is_none_or(|c| c == code),
                "AINV10: one variant, two codes"
            );
            *slot = Some(code);
        }
        let codes: Vec<ErrorCode> = code_of_variant
            .into_iter()
            .map(|c| c.expect("AINV10: every CommandError variant has a sample"))
            .collect();
        let distinct: BTreeSet<_> = codes.iter().collect();
        assert_eq!(distinct.len(), codes.len(), "AINV10: codes are distinct");
        assert_eq!(
            codes,
            [
                ErrorCode::OutOfBounds,
                ErrorCode::InvalidWire,
                ErrorCode::MissingItem,
                ErrorCode::DuplicateItem,
                ErrorCode::InvalidTextField,
                ErrorCode::SegmentBlocked,
                ErrorCode::FollowNoRoute,
                ErrorCode::WireCapture,
                ErrorCode::OffLattice,
            ]
        );
    }

    #[test]
    fn ainv10_command_error_details_keep_their_ids() {
        let err = AgentError::from(CommandError::Capture {
            wire: WireId(6),
            blocker: Blocker::Pin {
                part: PartId(1),
                pin: 2,
            },
        });
        assert_eq!(
            serde_json::to_string(&err).unwrap(),
            r#"{"code":"wire_capture","message":"moved wire:6 would capture pin 2 of part:1","retryable":false,"details":{"wire":"wire:6","blocker":{"kind":"pin","part":"part:1","pin":2}}}"#
        );
    }

    #[test]
    fn error_codes_are_unique_snake_case() {
        let names: BTreeSet<_> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(names.len(), ErrorCode::ALL.len());
        for code in ErrorCode::ALL {
            let name = code.as_str();
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
                "{name}"
            );
            assert_eq!(ErrorCode::from_name(name), Some(*code));
        }
    }

    #[test]
    fn section_9_4_stale_revision_example() {
        let err = AgentError::new(ErrorKind::StaleRevision {
            expected: 12,
            actual: 14,
        });
        assert_eq!(
            serde_json::to_string(&err).unwrap(),
            r#"{"code":"stale_revision","message":"expected revision 12, document is at 14","retryable":true,"details":{"expected":12,"actual":14},"hint":"read the document again and rebuild the request"}"#
        );
    }

    #[test]
    fn unit_codes_refuse_details_and_detail_codes_require_them() {
        let bad = [
            r#"{"code":"internal","message":"m","retryable":false,"details":{"x":1}}"#,
            r#"{"code":"stale_revision","message":"m","retryable":true,"details":{}}"#,
            r#"{"code":"no_such_code","message":"m","retryable":true,"details":{}}"#,
            r#"{"code":"internal","message":"m","retryable":false,"details":{},"extra":1}"#,
            r#"{"code":"nothing_to_undo","message":"m","retryable":true,"details":{}}"#,
            r#"{"code":"stale_revision","message":"m","retryable":false,"details":{"expected":1,"actual":2}}"#,
        ];
        for text in bad {
            assert!(serde_json::from_str::<AgentError>(text).is_err(), "{text}");
        }
    }
}
