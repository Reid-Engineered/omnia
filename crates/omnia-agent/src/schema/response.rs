//! Responses: the envelope and the result of every operation (mcp-plan.md sections 9.1, 9.3).
//!
//! `{"schema": "omnia.agent/1", "doc_id": "d1", "revision": 13, "result": {..}, "warnings": []}`,
//! with `error` in place of `result` on failure. Absent optional fields are omitted, never
//! `null`. Lists of ids are sorted by kind and then id (AD8); [`ChangeDto::from`] sorts the
//! unordered sets of a `Change`. A result's shape depends on the operation, so
//! [`Response::from_slice`] takes the [`OpName`] the response answers.

use omnia_doc::Change;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, DeserializeOwned},
    ser::SerializeStruct,
};
use serde_json::Value;

use super::codec::{
    Bounds, DocId, ItemRef, JobId, LabelRef, OrientName, PartRef, Point, PreviewId, WireRef,
};
use crate::{AgentError, Catalog, OpName, SCHEMA};

/// A warning on a response. Schema version 1 defines no warning codes and emits none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Warning {
    pub code: String,
    pub message: String,
}

/// The result of `new` and `open`. The envelope carries `doc_id` and `revision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `revision != saved_revision` (section 11.1).
    pub dirty: bool,
}

/// The result of `close`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseResult {
    /// Whether unsaved changes were discarded.
    pub discarded: bool,
}

/// Item counts of a document (ATC-read-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub parts: u64,
    pub wires: u64,
    pub labels: u64,
}

/// A part's stored fields; `symbol` is the symbol name (AD11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartBody {
    pub symbol: String,
    pub at: Point,
    pub orient: OrientName,
    pub designator: String,
    pub value: String,
}

/// A wire's ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireBody {
    pub a: Point,
    pub b: Point,
}

/// A label's stored fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelBody {
    pub at: Point,
    pub text: String,
}

/// `{"id": "part:3", "part": {..}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartItem {
    pub id: PartRef,
    pub part: PartBody,
}

/// `{"id": "wire:41", "wire": {"a": [..], "b": [..]}}` (section 9.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireItem {
    pub id: WireRef,
    pub wire: WireBody,
}

/// `{"id": "label:2", "label": {..}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelItem {
    pub id: LabelRef,
    pub label: LabelBody,
}

/// One item with its fields; the key beside `id` names its kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Item {
    Part(PartItem),
    Wire(WireItem),
    Label(LabelItem),
}

impl Item {
    /// The item's id.
    pub fn id(&self) -> ItemRef {
        match self {
            Self::Part(i) => ItemRef(omnia_doc::ItemId::Part(i.id.into())),
            Self::Wire(i) => ItemRef(omnia_doc::ItemId::Wire(i.id.into())),
            Self::Label(i) => ItemRef(omnia_doc::ItemId::Label(i.id.into())),
        }
    }
}

/// A page of items in id order (AK4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemsPage {
    pub items: Vec<Item>,
    /// Present when more items follow; pass it back as `cursor`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// One pin: its part and its index in the symbol's pin order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinRef {
    pub part: PartRef,
    pub pin: u32,
}

/// A net's canonical key (AD8): its lowest pin, else its lowest wire, else its lowest label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetKey {
    Pin { part: PartRef, pin: u32 },
    Wire { wire: WireRef },
    Label { label: LabelRef },
}

/// One net, its members sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Net {
    pub key: NetKey,
    pub pins: Vec<PinRef>,
    pub wires: Vec<WireRef>,
    pub labels: Vec<LabelRef>,
}

/// The result of `inspect`: the parts `include` named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<ItemsPage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nets: Option<Vec<Net>>,
}

/// The result of `nets`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetsResult {
    pub nets: Vec<Net>,
}

/// A `Change` on the wire (section 9.3), with `added` and `removed` sorted (AD8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeDto {
    pub revision: u64,
    /// Absent for a validated no-op.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirty: Option<Bounds>,
    pub added: Vec<ItemRef>,
    pub removed: Vec<ItemRef>,
    pub nets_changed: bool,
}

impl From<&Change> for ChangeDto {
    fn from(change: &Change) -> Self {
        let sorted = |ids: &[omnia_doc::ItemId]| {
            let mut ids: Vec<ItemRef> = ids.iter().map(|&id| id.into()).collect();
            ids.sort_unstable();
            ids
        };
        Self {
            revision: change.revision,
            dirty: change.dirty.map(Bounds),
            added: sorted(&change.added),
            removed: sorted(&change.removed),
            nets_changed: change.nets_changed,
        }
    }
}

/// A part whose fields changed: its full fields before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartModified {
    pub id: PartRef,
    pub before: PartBody,
    pub after: PartBody,
}

/// A wire whose ends changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireModified {
    pub id: WireRef,
    pub before: WireBody,
    pub after: WireBody,
}

/// A label whose fields changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LabelModified {
    pub id: LabelRef,
    pub before: LabelBody,
    pub after: LabelBody,
}

/// One modified item; the kind follows from `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Modified {
    Part(PartModified),
    Wire(WireModified),
    Label(LabelModified),
}

/// The before and after of an edit, bounded by `Change.dirty` and paged by AK4 (section 9.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diff {
    pub added: Vec<Item>,
    pub removed: Vec<Item>,
    pub modified: Vec<Modified>,
    pub truncated: bool,
}

/// The result of `validate`: what applying at `base_revision` would report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidateResult {
    pub base_revision: u64,
    pub change: ChangeDto,
}

/// The result of `preview` (section 9.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewResult {
    pub preview_id: PreviewId,
    pub base_revision: u64,
    pub change: ChangeDto,
    pub diff: Diff,
}

/// The result of `apply`: the preview's shape without `preview_id` and `base_revision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditResult {
    pub change: ChangeDto,
    pub diff: Diff,
}

/// The result of `undo` and `redo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeResult {
    pub change: ChangeDto,
}

/// The result of `save`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveResult {
    pub path: String,
    pub saved_revision: u64,
}

/// The result of `request_status` (section 11.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestStatus {
    /// No request with this key ran.
    NotRun,
    /// It ran and succeeded; `response` is the stored response, which answered `op`.
    Applied {
        op: OpName,
        revision: u64,
        response: Box<Response>,
    },
    /// It ran and failed.
    Failed { error: AgentError },
}

impl Serialize for RequestStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::NotRun => {
                let mut s = serializer.serialize_struct("RequestStatus", 1)?;
                s.serialize_field("status", "not_run")?;
                s.end()
            }
            Self::Applied {
                op,
                revision,
                response,
            } => {
                let mut s = serializer.serialize_struct("RequestStatus", 4)?;
                s.serialize_field("status", "applied")?;
                s.serialize_field("op", op)?;
                s.serialize_field("revision", revision)?;
                s.serialize_field("response", response)?;
                s.end()
            }
            Self::Failed { error } => {
                let mut s = serializer.serialize_struct("RequestStatus", 2)?;
                s.serialize_field("status", "failed")?;
                s.serialize_field("error", error)?;
                s.end()
            }
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestStatusWire {
    status: String,
    #[serde(default)]
    op: Option<OpName>,
    #[serde(default)]
    revision: Option<u64>,
    #[serde(default)]
    response: Option<Value>,
    #[serde(default)]
    error: Option<AgentError>,
}

impl<'de> Deserialize<'de> for RequestStatus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let w = RequestStatusWire::deserialize(deserializer)?;
        match (w.status.as_str(), w.op, w.revision, w.response, w.error) {
            ("not_run", None, None, None, None) => Ok(Self::NotRun),
            ("applied", Some(op), Some(revision), Some(response), None) => {
                let response = Response::from_value(op, response).map_err(de::Error::custom)?;
                Ok(Self::Applied {
                    op,
                    revision,
                    response: Box::new(response),
                })
            }
            ("failed", None, None, None, Some(error)) => Ok(Self::Failed { error }),
            _ => Err(de::Error::custom(
                "request status is not_run {}, applied {op, revision, response} or failed {error}",
            )),
        }
    }
}

/// A job's state (section 11.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPhase {
    Queued,
    Running,
    Succeeded,
    Failed,
    CancelledByClient,
    CancelledBySystem,
}

/// A job's progress (section 11.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Progress {
    pub completed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    pub unit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The result of `job_status` and `job_cancel`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobState {
    pub job_id: JobId,
    pub state: JobPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
    /// Present when `state` is `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AgentError>,
}

/// The result of a successful request. Which variant answers which operation is fixed by
/// [`Output::decode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Capabilities(Box<Catalog>),
    Document(DocInfo),
    Closed(CloseResult),
    Inspect(InspectResult),
    Query(ItemsPage),
    Nets(NetsResult),
    Validated(ValidateResult),
    Preview(PreviewResult),
    Applied(EditResult),
    Change(ChangeResult),
    Saved(SaveResult),
    RequestStatus(RequestStatus),
    Job(JobState),
}

fn result_of<T: DeserializeOwned>(value: Value) -> Result<T, serde_json::Error> {
    serde_json::from_value(value)
}

impl Output {
    /// Decodes the result of `op`. Exhaustive over `OpName`, with no wildcard arm.
    pub fn decode(op: OpName, value: Value) -> Result<Self, serde_json::Error> {
        Ok(match op {
            OpName::Capabilities => Self::Capabilities(result_of(value)?),
            OpName::New | OpName::Open => Self::Document(result_of(value)?),
            OpName::Close => Self::Closed(result_of(value)?),
            OpName::Inspect => Self::Inspect(result_of(value)?),
            OpName::Query => Self::Query(result_of(value)?),
            OpName::Nets => Self::Nets(result_of(value)?),
            OpName::Validate => Self::Validated(result_of(value)?),
            OpName::Preview => Self::Preview(result_of(value)?),
            OpName::Apply => Self::Applied(result_of(value)?),
            OpName::Undo | OpName::Redo => Self::Change(result_of(value)?),
            OpName::Save => Self::Saved(result_of(value)?),
            OpName::RequestStatus => Self::RequestStatus(result_of(value)?),
            OpName::JobStatus | OpName::JobCancel => Self::Job(result_of(value)?),
        })
    }

    /// Whether this variant is the result of `op` (the same table as [`Self::decode`]).
    pub const fn answers(&self, op: OpName) -> bool {
        matches!(
            (self, op),
            (Self::Capabilities(_), OpName::Capabilities)
                | (Self::Document(_), OpName::New | OpName::Open)
                | (Self::Closed(_), OpName::Close)
                | (Self::Inspect(_), OpName::Inspect)
                | (Self::Query(_), OpName::Query)
                | (Self::Nets(_), OpName::Nets)
                | (Self::Validated(_), OpName::Validate)
                | (Self::Preview(_), OpName::Preview)
                | (Self::Applied(_), OpName::Apply)
                | (Self::Change(_), OpName::Undo | OpName::Redo)
                | (Self::Saved(_), OpName::Save)
                | (Self::RequestStatus(_), OpName::RequestStatus)
                | (Self::Job(_), OpName::JobStatus | OpName::JobCancel)
        )
    }
}

impl Serialize for Output {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Capabilities(v) => v.serialize(s),
            Self::Document(v) => v.serialize(s),
            Self::Closed(v) => v.serialize(s),
            Self::Inspect(v) => v.serialize(s),
            Self::Query(v) => v.serialize(s),
            Self::Nets(v) => v.serialize(s),
            Self::Validated(v) => v.serialize(s),
            Self::Preview(v) => v.serialize(s),
            Self::Applied(v) => v.serialize(s),
            Self::Change(v) => v.serialize(s),
            Self::Saved(v) => v.serialize(s),
            Self::RequestStatus(v) => v.serialize(s),
            Self::Job(v) => v.serialize(s),
        }
    }
}

/// One response (section 9.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// Always [`SCHEMA`] (AK7).
    pub schema: &'static str,
    pub doc_id: Option<DocId>,
    pub revision: Option<u64>,
    pub outcome: Result<Output, AgentError>,
    pub warnings: Vec<Warning>,
}

impl Response {
    /// A response of schema [`SCHEMA`] with no warnings.
    pub fn new(
        doc_id: Option<DocId>,
        revision: Option<u64>,
        outcome: Result<Output, AgentError>,
    ) -> Self {
        Self {
            schema: SCHEMA,
            doc_id,
            revision,
            outcome,
            warnings: Vec::new(),
        }
    }

    /// The response as JSON text: fixed field order, no whitespace (AD8).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("responses always serialize")
    }

    /// Decodes the response to a request of `op`.
    pub fn from_slice(op: OpName, bytes: &[u8]) -> Result<Self, serde_json::Error> {
        Self::from_value(op, serde_json::from_slice(bytes)?)
    }

    /// Decodes the response to a request of `op` from a JSON value.
    pub fn from_value(op: OpName, value: Value) -> Result<Self, serde_json::Error> {
        let wire: ResponseWire = serde_json::from_value(value)?;
        if wire.schema != SCHEMA {
            return Err(de::Error::custom(format_args!("schema is not {SCHEMA}")));
        }
        let outcome = match (wire.result, wire.error) {
            (Some(result), None) => Ok(Output::decode(op, result)?),
            (None, Some(error)) => Err(error),
            _ => {
                return Err(de::Error::custom(
                    "a response has exactly one of result and error",
                ));
            }
        };
        Ok(Self {
            schema: SCHEMA,
            doc_id: wire.doc_id,
            revision: wire.revision,
            outcome,
            warnings: wire.warnings,
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResponseWire {
    schema: String,
    #[serde(default)]
    doc_id: Option<DocId>,
    #[serde(default)]
    revision: Option<u64>,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<AgentError>,
    warnings: Vec<Warning>,
}

impl Serialize for Response {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let len = 3 + usize::from(self.doc_id.is_some()) + usize::from(self.revision.is_some());
        let mut s = serializer.serialize_struct("Response", len)?;
        s.serialize_field("schema", self.schema)?;
        if let Some(doc_id) = &self.doc_id {
            s.serialize_field("doc_id", doc_id)?;
        }
        if let Some(revision) = &self.revision {
            s.serialize_field("revision", revision)?;
        }
        match &self.outcome {
            Ok(result) => s.serialize_field("result", result)?,
            Err(error) => s.serialize_field("error", error)?,
        }
        s.serialize_field("warnings", &self.warnings)?;
        s.end()
    }
}
