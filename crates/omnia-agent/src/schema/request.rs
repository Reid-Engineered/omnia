//! Requests: the envelope, the operation union and its params (mcp-plan.md sections 9.1, 9.2).
//!
//! A request is `{"op": "<operation>", "params": {..}, "idempotency_key": ".."}`. Every object
//! rejects unknown fields. [`Request::from_slice`] decodes one strictly and reports a typed error:
//! `invalid_request` for malformed JSON, duplicate keys or any non-integer number (AD12),
//! `unknown_operation` for a name outside the catalog, `schema_violation {path}` for a shape that
//! does not match, and `limit_exceeded` for AK2 to AK4.

use std::fmt;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor},
    ser::SerializeStruct,
};
use serde_json::{Map, Value};

use super::codec::{
    Bounds, DocId, ItemRef, JobId, OrientName, Point, PreviewId, TextFieldName, WireRef,
};
use crate::{
    EditOpName, ErrorKind, MAX_OPERATIONS, MAX_PAGE_SIZE, MAX_REQUEST_BYTES, OpName,
    error::{AgentError, Limit},
};

/// Params of an operation that takes none: `{}`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoArgs {}

/// `open {path}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenArgs {
    pub path: String,
}

/// The echo of an `approval_required` error (AD7, AQ4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalEcho {
    pub approval_id: String,
    pub effect_digest: String,
}

/// `close {doc_id, discard_unsaved?, approval?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseArgs {
    pub doc_id: DocId,
    #[serde(default, skip_serializing_if = "is_false")]
    pub discard_unsaved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalEcho>,
}

fn is_false(v: &bool) -> bool {
    !*v
}

/// What `inspect` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Include {
    Summary,
    Items,
    Nets,
}

/// `inspect {doc_id, include, area?, cursor?, limit?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectArgs {
    pub doc_id: DocId,
    /// Non-empty, no repeats.
    pub include: Vec<Include>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<Bounds>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Page size, 1 to AK4's maximum; AK4's default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// The spatial query of `query` (`Doc::query`, `query_within`, `query_touching`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryMode {
    Envelope,
    Within,
    Touching,
}

/// `query {doc_id, area, mode, cursor?, limit?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryArgs {
    pub doc_id: DocId,
    pub area: Bounds,
    pub mode: QueryMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// `nets {doc_id, at? | wire?}`: all nets, the net at a point, or the net of a wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetsArgs {
    pub doc_id: DocId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<Point>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire: Option<WireRef>,
}

/// One edit operation (section 9.2), tagged by `op`. Each maps to one `Command` (AD2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum EditOp {
    /// `PlacePart` through `SymbolLibrary::place_command` (AD11).
    PlacePart {
        symbol: String,
        at: Point,
        orient: OrientName,
        designator: String,
        value: String,
    },
    /// `AddWires`; each wire is `[a, b]`.
    AddWires { wires: Vec<[Point; 2]> },
    /// `AddLabel`.
    AddLabel { at: Point, text: String },
    /// `Move`.
    Move { items: Vec<ItemRef>, delta: Point },
    /// `Reorient`.
    Reorient {
        items: Vec<ItemRef>,
        pivot: Point,
        by: OrientName,
    },
    /// `Delete`.
    Delete { items: Vec<ItemRef> },
    /// `SetText`.
    SetText {
        item: ItemRef,
        field: TextFieldName,
        text: String,
    },
}

impl EditOp {
    /// The catalog name. Exhaustive, with no wildcard arm.
    pub const fn name(&self) -> EditOpName {
        match self {
            Self::PlacePart { .. } => EditOpName::PlacePart,
            Self::AddWires { .. } => EditOpName::AddWires,
            Self::AddLabel { .. } => EditOpName::AddLabel,
            Self::Move { .. } => EditOpName::Move,
            Self::Reorient { .. } => EditOpName::Reorient,
            Self::Delete { .. } => EditOpName::Delete,
            Self::SetText { .. } => EditOpName::SetText,
        }
    }
}

/// `validate` and `preview {doc_id, expected_revision, operations}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditArgs {
    pub doc_id: DocId,
    pub expected_revision: u64,
    /// 1 to AK3 operations.
    pub operations: Vec<EditOp>,
}

/// `apply {doc_id, expected_revision, operations, preview_id?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyArgs {
    pub doc_id: DocId,
    pub expected_revision: u64,
    pub operations: Vec<EditOp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_id: Option<PreviewId>,
}

/// `undo` and `redo {doc_id, expected_revision}` (section 11.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevArgs {
    pub doc_id: DocId,
    pub expected_revision: u64,
}

/// `save {doc_id, path?, approval?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveArgs {
    pub doc_id: DocId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval: Option<ApprovalEcho>,
}

/// `request_status {idempotency_key}` (section 11.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyArgs {
    pub idempotency_key: String,
}

/// `job_status` and `job_cancel {job_id}` (section 11.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobArgs {
    pub job_id: JobId,
}

/// The operation union (mcp-plan.md section 14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Capabilities(NoArgs),
    New(NoArgs),
    Open(OpenArgs),
    Close(CloseArgs),
    Inspect(InspectArgs),
    Query(QueryArgs),
    Nets(NetsArgs),
    Validate(EditArgs),
    Preview(EditArgs),
    Apply(ApplyArgs),
    Undo(RevArgs),
    Redo(RevArgs),
    Save(SaveArgs),
    RequestStatus(KeyArgs),
    JobStatus(JobArgs),
    JobCancel(JobArgs),
}

impl Operation {
    /// The catalog name. Exhaustive, with no wildcard arm.
    pub const fn name(&self) -> OpName {
        match self {
            Self::Capabilities(_) => OpName::Capabilities,
            Self::New(_) => OpName::New,
            Self::Open(_) => OpName::Open,
            Self::Close(_) => OpName::Close,
            Self::Inspect(_) => OpName::Inspect,
            Self::Query(_) => OpName::Query,
            Self::Nets(_) => OpName::Nets,
            Self::Validate(_) => OpName::Validate,
            Self::Preview(_) => OpName::Preview,
            Self::Apply(_) => OpName::Apply,
            Self::Undo(_) => OpName::Undo,
            Self::Redo(_) => OpName::Redo,
            Self::Save(_) => OpName::Save,
            Self::RequestStatus(_) => OpName::RequestStatus,
            Self::JobStatus(_) => OpName::JobStatus,
            Self::JobCancel(_) => OpName::JobCancel,
        }
    }

    /// Decodes `params` for `op`: the decoder half of the dispatch table. Exhaustive.
    fn decode(op: OpName, params: Value) -> Result<Self, AgentError> {
        Ok(match op {
            OpName::Capabilities => Self::Capabilities(params_of(params)?),
            OpName::New => Self::New(params_of(params)?),
            OpName::Open => Self::Open(params_of(params)?),
            OpName::Close => Self::Close(params_of(params)?),
            OpName::Inspect => Self::Inspect(params_of(params)?),
            OpName::Query => Self::Query(params_of(params)?),
            OpName::Nets => Self::Nets(params_of(params)?),
            OpName::Validate => Self::Validate(edit_params(params)?),
            OpName::Preview => Self::Preview(edit_params(params)?),
            OpName::Apply => Self::Apply(edit_params(params)?),
            OpName::Undo => Self::Undo(params_of(params)?),
            OpName::Redo => Self::Redo(params_of(params)?),
            OpName::Save => Self::Save(params_of(params)?),
            OpName::RequestStatus => Self::RequestStatus(params_of(params)?),
            OpName::JobStatus => Self::JobStatus(params_of(params)?),
            OpName::JobCancel => Self::JobCancel(params_of(params)?),
        })
    }

    /// Checks the rules a derive cannot state: AK3 and AK4 limits, non-empty and unique lists,
    /// and `nets` taking at most one of `at` and `wire`.
    fn check(&self) -> Result<(), AgentError> {
        match self {
            Self::Inspect(args) => {
                if args.include.is_empty() {
                    return Err(violation(
                        "/params/include",
                        "include lists at least one part",
                    ));
                }
                let mut seen = args.include.clone();
                seen.sort_unstable();
                seen.dedup();
                if seen.len() != args.include.len() {
                    return Err(violation("/params/include", "include repeats an entry"));
                }
                check_page(args.limit)
            }
            Self::Query(args) => check_page(args.limit),
            Self::Nets(args) if args.at.is_some() && args.wire.is_some() => {
                Err(violation("/params", "nets takes `at` or `wire`, not both"))
            }
            Self::Validate(args) | Self::Preview(args) => check_operations(args.operations.len()),
            Self::Apply(args) => check_operations(args.operations.len()),
            _ => Ok(()),
        }
    }
}

fn violation(path: &str, message: &str) -> AgentError {
    AgentError::with_message(
        ErrorKind::SchemaViolation { path: path.into() },
        format!("schema violation at `{path}`: {message}"),
    )
}

fn check_page(limit: Option<u32>) -> Result<(), AgentError> {
    match limit {
        Some(0) => Err(violation("/params/limit", "limit is at least 1")),
        Some(n) if n > MAX_PAGE_SIZE => Err(AgentError::new(ErrorKind::LimitExceeded {
            limit: Limit::PageSize,
            value: n.into(),
        })),
        _ => Ok(()),
    }
}

fn check_operations(count: usize) -> Result<(), AgentError> {
    let count = u64::try_from(count).unwrap_or(u64::MAX);
    if count == 0 {
        Err(violation(
            "/params/operations",
            "operations lists at least one operation",
        ))
    } else if count > MAX_OPERATIONS {
        Err(AgentError::new(ErrorKind::LimitExceeded {
            limit: Limit::Operations,
            value: count,
        }))
    } else {
        Ok(())
    }
}

fn params_of<T: DeserializeOwned>(params: Value) -> Result<T, AgentError> {
    serde_json::from_value(params).map_err(|e| violation("/params", &e.to_string()))
}

/// Decodes edit params, naming the first operation that does not match (`/params/operations/i`).
fn edit_params<T: DeserializeOwned>(params: Value) -> Result<T, AgentError> {
    serde_json::from_value::<T>(params.clone()).map_err(|e| {
        let ops = params.get("operations").and_then(Value::as_array);
        let bad = ops.and_then(|ops| {
            ops.iter().enumerate().find_map(|(i, op)| {
                serde_json::from_value::<EditOp>(op.clone())
                    .err()
                    .map(|e| (i, e))
            })
        });
        match bad {
            Some((i, e)) => violation(&format!("/params/operations/{i}"), &e.to_string()),
            None => violation("/params", &e.to_string()),
        }
    })
}

/// One request (section 9.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub op: Operation,
    /// Only on operations whose catalog row allows it (section 11.2).
    pub idempotency_key: Option<String>,
}

impl Request {
    /// Decodes one request strictly (see the module docs).
    pub fn from_slice(bytes: &[u8]) -> Result<Self, AgentError> {
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if len > MAX_REQUEST_BYTES {
            return Err(AgentError::new(ErrorKind::LimitExceeded {
                limit: Limit::RequestBytes,
                value: len,
            }));
        }
        let value = strict_value(bytes)?;
        Self::from_value(value)
    }

    fn from_value(value: Value) -> Result<Self, AgentError> {
        let Value::Object(mut map) = value else {
            return Err(AgentError::with_message(
                ErrorKind::InvalidRequest,
                "a request is a JSON object",
            ));
        };
        if let Some(key) = map
            .keys()
            .filter(|k| !matches!(k.as_str(), "op" | "params" | "idempotency_key"))
            .min()
        {
            return Err(violation(
                &format!("/{}", escape_pointer(key)),
                "unknown field",
            ));
        }
        let op = match map.remove("op") {
            Some(Value::String(name)) => OpName::from_name(&name)
                .ok_or_else(|| AgentError::new(ErrorKind::UnknownOperation))?,
            Some(_) => return Err(violation("/op", "op is a string")),
            None => return Err(violation("/op", "missing field `op`")),
        };
        let idempotency_key = match map.remove("idempotency_key") {
            None => None,
            Some(_) if !crate::catalog::spec(op).idempotency_key => {
                return Err(violation(
                    "/idempotency_key",
                    "this operation takes no idempotency key",
                ));
            }
            Some(Value::String(key)) => Some(key),
            Some(_) => return Err(violation("/idempotency_key", "idempotency_key is a string")),
        };
        let params = map
            .remove("params")
            .ok_or_else(|| violation("/params", "missing field `params`"))?;
        let op = Operation::decode(op, params)?;
        op.check()?;
        Ok(Self {
            op,
            idempotency_key,
        })
    }

    /// The request as JSON text, in the canonical field order.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("requests always serialize")
    }
}

impl std::str::FromStr for Request {
    type Err = AgentError;

    /// Decodes one request from text; see [`Request::from_slice`].
    fn from_str(text: &str) -> Result<Self, AgentError> {
        Self::from_slice(text.as_bytes())
    }
}

fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Serializes the params of an operation. Exhaustive.
struct Params<'a>(&'a Operation);

impl Serialize for Params<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Operation::Capabilities(a) | Operation::New(a) => a.serialize(s),
            Operation::Open(a) => a.serialize(s),
            Operation::Close(a) => a.serialize(s),
            Operation::Inspect(a) => a.serialize(s),
            Operation::Query(a) => a.serialize(s),
            Operation::Nets(a) => a.serialize(s),
            Operation::Validate(a) | Operation::Preview(a) => a.serialize(s),
            Operation::Apply(a) => a.serialize(s),
            Operation::Undo(a) | Operation::Redo(a) => a.serialize(s),
            Operation::Save(a) => a.serialize(s),
            Operation::RequestStatus(a) => a.serialize(s),
            Operation::JobStatus(a) | Operation::JobCancel(a) => a.serialize(s),
        }
    }
}

impl Serialize for Request {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let len = if self.idempotency_key.is_some() { 3 } else { 2 };
        let mut s = serializer.serialize_struct("Request", len)?;
        s.serialize_field("op", &self.op.name())?;
        s.serialize_field("params", &Params(&self.op))?;
        if let Some(key) = &self.idempotency_key {
            s.serialize_field("idempotency_key", key)?;
        }
        s.end()
    }
}

// ---- strict JSON ---------------------------------------------------------------------------

/// Parses JSON refusing duplicate object keys and every number that is not an integer.
fn strict_value(bytes: &[u8]) -> Result<Value, AgentError> {
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue::deserialize(&mut de)
        .and_then(|v| de.end().map(|()| v.0))
        .map_err(|e| AgentError::with_message(ErrorKind::InvalidRequest, e.to_string()))?;
    Ok(value)
}

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(StrictVisitor).map(StrictValue)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("JSON without floats or duplicate keys")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
        Ok(Value::Number(v.into()))
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Value, E> {
        Err(E::custom(
            "numbers are integers; floats never appear (AD12)",
        ))
    }

    fn visit_str<E>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }

    fn visit_string<E>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }

    fn visit_unit<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_none<E>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(StrictValue(v)) = seq.next_element()? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let StrictValue(v) = map.next_value()?;
            if out.contains_key(&key) {
                return Err(de::Error::custom(format_args!("duplicate key `{key}`")));
            }
            out.insert(key, v);
        }
        Ok(Value::Object(out))
    }
}
