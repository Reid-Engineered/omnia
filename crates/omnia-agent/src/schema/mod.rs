//! The wire schema, version [`crate::SCHEMA`] (AK7): DTOs, codecs and the JSON Schema module.
//!
//! The DTOs are a separate layer over `omnia_doc` types: no internal type gains serde, and
//! `omnia-doc` gains no agent concepts (mcp-plan.md section 5). DTOs hold no hash maps and no
//! floats, and their field order is their struct order (AD8).

mod codec;
mod json;
mod request;
mod response;
pub mod validate;

#[cfg(test)]
mod tests;

pub use codec::{
    Bounds, DocId, InvalidId, ItemRef, JobId, LabelRef, ORIENT_NAMES, OrientName, PartRef, Point,
    PreviewId, TEXT_FIELD_NAMES, TextFieldName, WireRef,
};
pub use json::{schema_files, schema_text, to_canonical_json};
pub use request::{
    ApplyArgs, ApprovalEcho, CloseArgs, EditArgs, EditOp, Include, InspectArgs, JobArgs, KeyArgs,
    NetsArgs, NoArgs, OpenArgs, Operation, QueryArgs, QueryMode, Request, RevArgs, SaveArgs,
    parse_strict,
};
pub use response::{
    ChangeDto, ChangeResult, CloseResult, Diff, DocInfo, EditResult, InspectResult,
    InvalidResponse, Item, ItemsPage, JobPhase, JobState, LabelBody, LabelItem, LabelModified,
    Modified, Net, NetKey, NetsResult, Output, PartBody, PartItem, PartModified, PinRef,
    PreviewResult, Progress, RequestStatus, Response, SaveResult, StoredResponse, Summary,
    ValidateResult, Warning, WireBody, WireItem, WireModified,
};
