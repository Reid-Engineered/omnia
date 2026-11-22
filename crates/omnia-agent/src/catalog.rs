//! Capability discovery: the catalog is the dispatch table (AINV11).
//!
//! [`OpName::ALL`] lists every operation the request decoder dispatches, generated from the same
//! list as the enum. [`spec`] gives each one its tool name, approval tiers (AD7), annotations
//! (mcp-plan.md section 10.1) and schema names through an exhaustive `match`, so an operation
//! cannot exist without a catalog entry. [`Catalog::current`] is what `capabilities` returns.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

use crate::{
    DEFAULT_PAGE_SIZE, ErrorCode, IDEMPOTENCY_CACHE_ENTRIES, MAX_FILE_BYTES, MAX_OPEN_DOCUMENTS,
    MAX_OPERATIONS, MAX_PAGE_SIZE, MAX_REQUEST_BYTES, PROTOCOL_REVISIONS,
    schema::{ORIENT_NAMES, TEXT_FIELD_NAMES},
};

macro_rules! names {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $wire:literal,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($variant,)*
        }

        impl $name {
            /// Every name, in catalog order. Generated from the same list as the enum.
            pub const ALL: &'static [$name] = &[$($name::$variant,)*];

            /// The wire spelling.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $wire,)*
                }
            }

            /// Finds a name by its wire spelling, searching [`Self::ALL`].
            pub fn from_name(name: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|op| op.as_str() == name)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let name = String::deserialize(deserializer)?;
                Self::from_name(&name).ok_or_else(|| {
                    de::Error::invalid_value(de::Unexpected::Str(&name), &stringify!($name))
                })
            }
        }
    };
}

names! {
    /// Every request operation (mcp-plan.md section 14, `Operation`).
    OpName {
        Capabilities => "capabilities",
        New => "new",
        Open => "open",
        Close => "close",
        Inspect => "inspect",
        Query => "query",
        Nets => "nets",
        Validate => "validate",
        Preview => "preview",
        Apply => "apply",
        Undo => "undo",
        Redo => "redo",
        Save => "save",
        RequestStatus => "request_status",
        JobStatus => "job_status",
        JobCancel => "job_cancel",
    }
}

names! {
    /// Every edit operation (mcp-plan.md section 9.2).
    EditOpName {
        PlacePart => "place_part",
        AddWires => "add_wires",
        AddLabel => "add_label",
        Move => "move",
        Reorient => "reorient",
        Delete => "delete",
        SetText => "set_text",
    }
}

/// One row of the dispatch table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpSpec {
    pub op: OpName,
    /// MCP tool name (section 10.1).
    pub tool: &'static str,
    /// AD7 tiers the operation can reach; more than one when the effect decides.
    pub tiers: &'static [u8],
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    /// Whether the request envelope may carry `idempotency_key` (section 11.2).
    pub idempotency_key: bool,
    /// Static text (AD14).
    pub description: &'static str,
}

impl OpSpec {
    /// Name of the committed schema for this operation's `params`.
    pub fn params_schema(&self) -> String {
        format!("{}.params", self.op)
    }

    /// Name of the committed schema for this operation's response envelope.
    pub fn response_schema(&self) -> String {
        format!("{}.response", self.op)
    }
}

const fn row(
    op: OpName,
    tool: &'static str,
    tiers: &'static [u8],
    flags: [bool; 4],
    description: &'static str,
) -> OpSpec {
    let [read_only, destructive, idempotent, idempotency_key] = flags;
    OpSpec {
        op,
        tool,
        tiers,
        read_only,
        destructive,
        idempotent,
        idempotency_key,
        description,
    }
}

// Flags: [read_only, destructive, idempotent, idempotency_key].
const READ: [bool; 4] = [true, false, true, false];
const READ_ONCE: [bool; 4] = [true, false, false, false];
const EDIT: [bool; 4] = [false, false, false, true];

/// The dispatch table row of `op`. Exhaustive, with no wildcard arm.
pub const fn spec(op: OpName) -> OpSpec {
    match op {
        OpName::Capabilities => row(
            op,
            "omnia_capabilities",
            &[0],
            READ,
            "List the operations, edit operations, error codes, units and limits of this server.",
        ),
        OpName::New => row(
            op,
            "omnia_new",
            &[0],
            [false, false, false, false],
            "Create an empty document and return its doc_id.",
        ),
        OpName::Open => row(
            op,
            "omnia_open",
            &[0],
            READ_ONCE,
            "Open a RON v2 test sheet inside the configured roots and return its doc_id.",
        ),
        OpName::Close => row(
            op,
            "omnia_close",
            &[0, 3],
            [false, true, false, false],
            "Close a document. Discarding unsaved changes needs an approval.",
        ),
        OpName::Inspect => row(
            op,
            "omnia_inspect",
            &[0],
            READ,
            "Read a document: its summary, its items in id order (paged) and its canonical nets.",
        ),
        OpName::Query => row(
            op,
            "omnia_query",
            &[0],
            READ,
            "List the items in an area by envelope, within or touching (paged).",
        ),
        OpName::Nets => row(
            op,
            "omnia_nets",
            &[0],
            READ,
            "Read canonical nets: all of them, the net at a point, or the net of a wire.",
        ),
        OpName::Validate => row(
            op,
            "omnia_validate",
            &[0],
            READ,
            "Check edit operations against a revision without changing the document.",
        ),
        OpName::Preview => row(
            op,
            "omnia_preview",
            &[0],
            READ,
            "Apply edit operations to a fork and return the change, the diff and a preview_id.",
        ),
        OpName::Apply => row(
            op,
            "omnia_apply",
            &[1],
            EDIT,
            "Apply edit operations as one undo step at the expected revision.",
        ),
        OpName::Undo => row(
            op,
            "omnia_undo",
            &[1],
            EDIT,
            "Undo the last step if the document is at the expected revision.",
        ),
        OpName::Redo => row(
            op,
            "omnia_redo",
            &[1],
            EDIT,
            "Redo the last undone step if the document is at the expected revision.",
        ),
        OpName::Save => row(
            op,
            "omnia_save",
            &[2, 3],
            [false, true, false, true],
            "Save a document inside the roots. Overwriting a file needs an approval.",
        ),
        OpName::RequestStatus => row(
            op,
            "omnia_request_status",
            &[0],
            READ_ONCE,
            "Report whether the request with this idempotency key ran, and its response.",
        ),
        OpName::JobStatus => row(
            op,
            "omnia_job_status",
            &[0],
            [false, false, false, false],
            "Report a job's state and progress.",
        ),
        OpName::JobCancel => row(
            op,
            "omnia_job_cancel",
            &[0],
            [false, false, false, false],
            "Ask a job to stop; it stops at its next step.",
        ),
    }
}

/// The `Command` and description of an edit operation. Exhaustive, with no wildcard arm.
pub const fn edit_spec(op: EditOpName) -> (&'static str, &'static str) {
    match op {
        EditOpName::PlacePart => (
            "PlacePart",
            "Place a symbol by name at a point with an orientation, designator and value.",
        ),
        EditOpName::AddWires => (
            "AddWires",
            "Add axis-aligned wires; the document splits and merges them into normal form.",
        ),
        EditOpName::AddLabel => ("AddLabel", "Add a net label at a point."),
        EditOpName::Move => ("Move", "Move items by a delta."),
        EditOpName::Reorient => ("Reorient", "Reorient items about a pivot."),
        EditOpName::Delete => ("Delete", "Delete items."),
        EditOpName::SetText => (
            "SetText",
            "Set the designator or value of a part, or the text of a label.",
        ),
    }
}

/// The capability catalog (section 10.1, AK8), as `capabilities` returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// AK1, newest first.
    pub protocol_revisions: Vec<String>,
    pub units: Units,
    pub limits: Limits,
    pub operations: Vec<OperationEntry>,
    pub edit_operations: Vec<EditOperationEntry>,
    pub orientations: Vec<String>,
    pub text_fields: Vec<String>,
    pub error_codes: Vec<ErrorCodeEntry>,
}

/// AK8 units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Units {
    /// The K2 grid in nanometres.
    pub grid_nm: u32,
    /// The K2 pin lattice in nanometres.
    pub pin_lattice_nm: u32,
    /// K1: coordinates lie in `[-sheet_limit_nm, sheet_limit_nm]`.
    pub sheet_limit_nm: u32,
}

/// AK2 to AK6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub request_bytes: u64,
    pub file_bytes: u64,
    pub operations_per_request: u64,
    pub default_page_size: u32,
    pub max_page_size: u32,
    pub idempotency_cache_entries: u32,
    pub open_documents: u32,
}

/// One operation of the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationEntry {
    pub name: OpName,
    pub tool: String,
    pub tiers: Vec<u8>,
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
    pub idempotency_key: bool,
    pub params_schema: String,
    pub response_schema: String,
    pub description: String,
}

/// One edit operation of the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditOperationEntry {
    pub name: EditOpName,
    /// The `omnia_doc::Command` variant it becomes (AD2).
    pub command: String,
    pub description: String,
}

/// One error code of the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorCodeEntry {
    pub code: ErrorCode,
    pub retryable: bool,
}

fn nm(v: i32) -> u32 {
    v.unsigned_abs()
}

impl Catalog {
    /// The catalog of this build, built from [`OpName::ALL`], [`EditOpName::ALL`] and
    /// [`ErrorCode::ALL`]. The same every call.
    pub fn current() -> Self {
        Self {
            protocol_revisions: PROTOCOL_REVISIONS.iter().map(|&r| r.to_owned()).collect(),
            units: Units {
                grid_nm: nm(omnia_doc::GRID_NM),
                pin_lattice_nm: nm(omnia_doc::PIN_LATTICE_NM),
                sheet_limit_nm: nm(omnia_doc::SHEET_LIMIT_NM),
            },
            limits: Limits {
                request_bytes: MAX_REQUEST_BYTES,
                file_bytes: MAX_FILE_BYTES,
                operations_per_request: MAX_OPERATIONS,
                default_page_size: DEFAULT_PAGE_SIZE,
                max_page_size: MAX_PAGE_SIZE,
                idempotency_cache_entries: IDEMPOTENCY_CACHE_ENTRIES,
                open_documents: MAX_OPEN_DOCUMENTS,
            },
            operations: OpName::ALL
                .iter()
                .map(|&op| {
                    let s = spec(op);
                    OperationEntry {
                        name: op,
                        tool: s.tool.to_owned(),
                        tiers: s.tiers.to_vec(),
                        read_only: s.read_only,
                        destructive: s.destructive,
                        idempotent: s.idempotent,
                        idempotency_key: s.idempotency_key,
                        params_schema: s.params_schema(),
                        response_schema: s.response_schema(),
                        description: s.description.to_owned(),
                    }
                })
                .collect(),
            edit_operations: EditOpName::ALL
                .iter()
                .map(|&op| {
                    let (command, description) = edit_spec(op);
                    EditOperationEntry {
                        name: op,
                        command: command.to_owned(),
                        description: description.to_owned(),
                    }
                })
                .collect(),
            orientations: ORIENT_NAMES.iter().map(|&n| n.to_owned()).collect(),
            text_fields: TEXT_FIELD_NAMES.iter().map(|&n| n.to_owned()).collect(),
            error_codes: ErrorCode::ALL
                .iter()
                .map(|&code| ErrorCodeEntry {
                    code,
                    retryable: code.retryable(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn ainv11_catalog_rows_follow_the_dispatch_table() {
        let catalog = Catalog::current();
        assert_eq!(catalog.operations.len(), OpName::ALL.len());
        let mut tools = BTreeSet::new();
        for (entry, &op) in catalog.operations.iter().zip(OpName::ALL) {
            let s = spec(op);
            assert_eq!(entry.name, op);
            assert_eq!(s.op, op, "spec({op}) names its own operation");
            assert_eq!(OpName::from_name(op.as_str()), Some(op));
            assert_eq!(entry.tool, format!("omnia_{op}"), "section 10.1 tool name");
            assert!(tools.insert(entry.tool.clone()));
            assert!(!entry.tiers.is_empty() && entry.tiers.iter().all(|&t| t <= 3));
            assert!(!(entry.read_only && entry.destructive));
        }
        let edits: Vec<_> = catalog.edit_operations.iter().map(|e| e.name).collect();
        assert_eq!(edits, EditOpName::ALL);
        let codes: Vec<_> = catalog.error_codes.iter().map(|e| e.code).collect();
        assert_eq!(codes, ErrorCode::ALL);
        assert_eq!(OpName::from_name("omnia_inspect"), None);
    }

    #[test]
    fn section_10_1_tiers_and_annotations() {
        let s = spec(OpName::Apply);
        assert_eq!(
            (s.tiers, s.read_only, s.destructive, s.idempotent),
            (&[1u8][..], false, false, false)
        );
        let s = spec(OpName::Save);
        assert_eq!((s.tiers, s.destructive), (&[2u8, 3][..], true));
        let s = spec(OpName::Close);
        assert_eq!((s.tiers, s.destructive), (&[0u8, 3][..], true));
        for op in [
            OpName::Inspect,
            OpName::Query,
            OpName::Nets,
            OpName::Preview,
        ] {
            let s = spec(op);
            assert!(s.read_only && s.idempotent, "{op}");
        }
        let keyed: Vec<_> = OpName::ALL
            .iter()
            .copied()
            .filter(|&op| spec(op).idempotency_key)
            .collect();
        assert_eq!(
            keyed,
            [OpName::Apply, OpName::Undo, OpName::Redo, OpName::Save],
            "section 11.2"
        );
    }

    #[test]
    fn ak8_units_and_ak2_to_ak6_limits() {
        let c = Catalog::current();
        assert_eq!(c.units.grid_nm, 2_540_000);
        assert_eq!(c.units.pin_lattice_nm, 1_270_000);
        assert_eq!(c.limits.request_bytes, 8 * 1024 * 1024);
        assert_eq!(c.limits.operations_per_request, 4_096);
        assert_eq!(
            (c.limits.default_page_size, c.limits.max_page_size),
            (500, 5_000)
        );
        assert_eq!(c.limits.idempotency_cache_entries, 256);
        assert_eq!(c.limits.open_documents, 8);
        assert_eq!(
            c.protocol_revisions,
            ["2026-07-28", "2025-11-25", "2025-06-18"]
        );
    }
}
