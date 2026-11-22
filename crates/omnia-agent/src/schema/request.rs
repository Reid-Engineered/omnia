//! Requests: the envelope, the operation union and its params (mcp-plan.md sections 9.1, 9.2).
//!
//! A request is `{"op": "<operation>", "params": {..}, "idempotency_key": ".."}`, or, over MCP,
//! a tool's arguments (see [`Request`]). Every object rejects unknown fields. Decoding is strict
//! and reports a typed error, in this order: `limit_exceeded` for AK2 (size), `invalid_request`
//! for malformed JSON, duplicate keys or any number that is not a 64-bit integer (AD12),
//! `unknown_operation` for a name the catalog does not list, `limit_exceeded` for AK3 and AK4,
//! then `schema_violation {path}` from validation against the published schema. Only after the
//! schema accepts are the params decoded into DTOs, where the one remaining decoder-only rule,
//! bounds corner ordering, is checked (mcp-plan.md section 9.1).

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
use super::{
    json::schema_value,
    validate::{escape, validate},
};
use crate::{
    EditOpName, ErrorKind, MAX_OPERATIONS, MAX_PAGE_SIZE, MAX_REQUEST_BYTES, OpName,
    catalog::spec,
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
    ///
    /// `params` has been validated against the operation's schema; what serde can still refuse
    /// is a decoder-only rule (section 9.1), reported at `base`.
    fn decode(op: OpName, params: Value, base: &str) -> Result<Self, AgentError> {
        Ok(match op {
            OpName::Capabilities => Self::Capabilities(typed(params, base)?),
            OpName::New => Self::New(typed(params, base)?),
            OpName::Open => Self::Open(typed(params, base)?),
            OpName::Close => Self::Close(typed(params, base)?),
            OpName::Inspect => Self::Inspect(typed(params, base)?),
            OpName::Query => Self::Query(typed(params, base)?),
            OpName::Nets => Self::Nets(typed(params, base)?),
            OpName::Validate => Self::Validate(typed(params, base)?),
            OpName::Preview => Self::Preview(typed(params, base)?),
            OpName::Apply => Self::Apply(typed(params, base)?),
            OpName::Undo => Self::Undo(typed(params, base)?),
            OpName::Redo => Self::Redo(typed(params, base)?),
            OpName::Save => Self::Save(typed(params, base)?),
            OpName::RequestStatus => Self::RequestStatus(typed(params, base)?),
            OpName::JobStatus => Self::JobStatus(typed(params, base)?),
            OpName::JobCancel => Self::JobCancel(typed(params, base)?),
        })
    }
}

fn violation(path: &str, message: &str) -> AgentError {
    AgentError::with_message(
        ErrorKind::SchemaViolation { path: path.into() },
        format!("schema violation at `{path}`: {message}"),
    )
}

fn typed<T: DeserializeOwned>(params: Value, base: &str) -> Result<T, AgentError> {
    serde_json::from_value(params).map_err(|e| violation(base, &e.to_string()))
}

/// AK3 and AK4, checked on the raw params before the schema so an over-limit request is
/// `limit_exceeded`, never a schema violation, and is refused, never truncated.
fn check_limits(op: OpName, params: &Value) -> Result<(), AgentError> {
    let exceeded = |limit, value| Err(AgentError::new(ErrorKind::LimitExceeded { limit, value }));
    let edits = matches!(op, OpName::Validate | OpName::Preview | OpName::Apply);
    let paged = matches!(op, OpName::Inspect | OpName::Query);
    if edits && let Some(ops) = params.get("operations").and_then(Value::as_array) {
        let count = u64::try_from(ops.len()).unwrap_or(u64::MAX);
        if count > MAX_OPERATIONS {
            return exceeded(Limit::Operations, count);
        }
    }
    if paged
        && let Some(n) = params.get("limit").and_then(Value::as_u64)
        && n > u64::from(MAX_PAGE_SIZE)
    {
        return exceeded(Limit::PageSize, n);
    }
    Ok(())
}

/// Validates `value` against the schema `name`, reporting paths below `base`.
fn conform(name: &str, value: &Value, base: &str) -> Result<(), AgentError> {
    let schema = schema_value(name).expect("every operation has generated schemas");
    validate(schema, value).map_err(|v| violation(&format!("{base}{}", v.path), &v.message))
}

/// Resolves an operation name the catalog lists; anything else is `unknown_operation`.
fn available(name: &str) -> Result<OpName, AgentError> {
    OpName::from_name(name)
        .filter(|&op| spec(op).available)
        .ok_or_else(|| AgentError::new(ErrorKind::UnknownOperation))
}

fn check_size(bytes: &[u8]) -> Result<(), AgentError> {
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len > MAX_REQUEST_BYTES {
        return Err(AgentError::new(ErrorKind::LimitExceeded {
            limit: Limit::RequestBytes,
            value: len,
        }));
    }
    Ok(())
}

/// One request (section 9.1).
///
/// It has two wire forms with the same meaning. The envelope `{op, params, idempotency_key?}`
/// is what `omnia call` and `omnia serve` read ([`Request::from_slice`]). The MCP form is a
/// tool's `arguments` object: the params with `idempotency_key` beside them, on the operations
/// section 11.2 names ([`Request::from_arguments`], schema `<op>.arguments`). Both decode to
/// the same `Request`, and [`Request::to_json`] and [`Request::to_arguments`] write them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub op: Operation,
    /// Only on operations whose catalog row allows it (section 11.2).
    pub idempotency_key: Option<String>,
}

impl Request {
    /// Decodes one envelope strictly (see the module docs).
    pub fn from_slice(bytes: &[u8]) -> Result<Self, AgentError> {
        check_size(bytes)?;
        Self::from_envelope_value(parse_strict(bytes)?)
    }

    /// Decodes the MCP tool arguments of `op` strictly (see the module docs).
    pub fn from_arguments(op: OpName, bytes: &[u8]) -> Result<Self, AgentError> {
        check_size(bytes)?;
        Self::from_arguments_value(op, parse_strict(bytes)?)
    }

    /// Decodes an envelope already parsed by [`parse_strict`]. A value parsed any other way
    /// has lost its duplicate keys and so cannot be checked for them.
    pub fn from_envelope_value(value: Value) -> Result<Self, AgentError> {
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
            return Err(violation(&format!("/{}", escape(key)), "unknown field"));
        }
        let op = match map.remove("op") {
            Some(Value::String(name)) => available(&name)?,
            Some(_) => return Err(violation("/op", "expected string")),
            None => return Err(violation("", "missing field `op`")),
        };
        let idempotency_key = match map.remove("idempotency_key") {
            None => None,
            Some(_) if !spec(op).idempotency_key => {
                return Err(violation("/idempotency_key", "unknown field"));
            }
            Some(Value::String(key)) => Some(key),
            Some(_) => return Err(violation("/idempotency_key", "expected string")),
        };
        let params = map
            .remove("params")
            .ok_or_else(|| violation("", "missing field `params`"))?;
        check_limits(op, &params)?;
        conform(&spec(op).params_schema(), &params, "/params")?;
        Ok(Self {
            op: Operation::decode(op, params, "/params")?,
            idempotency_key,
        })
    }

    /// Decodes tool arguments already parsed by [`parse_strict`].
    pub fn from_arguments_value(op: OpName, value: Value) -> Result<Self, AgentError> {
        let op = available(op.as_str())?;
        check_limits(op, &value)?;
        conform(&spec(op).arguments_schema(), &value, "")?;
        let mut params = value;
        // On the keyed operations the key sits beside the params; `request_status` has a
        // params field of the same name, which stays.
        let idempotency_key = if spec(op).idempotency_key {
            params
                .as_object_mut()
                .and_then(|m| m.remove("idempotency_key"))
                .and_then(|k| k.as_str().map(str::to_owned))
        } else {
            None
        };
        Ok(Self {
            op: Operation::decode(op, params, "")?,
            idempotency_key,
        })
    }

    /// The envelope as JSON text, in the canonical field order.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("requests always serialize")
    }

    /// The MCP tool arguments as JSON text: the params' fields, then `idempotency_key`.
    pub fn to_arguments(&self) -> String {
        serde_json::to_string(&Arguments {
            params: Params(&self.op),
            idempotency_key: self.idempotency_key.as_deref(),
        })
        .expect("arguments always serialize")
    }
}

impl std::str::FromStr for Request {
    type Err = AgentError;

    /// Decodes one envelope from text; see [`Request::from_slice`].
    fn from_str(text: &str) -> Result<Self, AgentError> {
        Self::from_slice(text.as_bytes())
    }
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

#[derive(Serialize)]
struct Arguments<'a> {
    #[serde(flatten)]
    params: Params<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    idempotency_key: Option<&'a str>,
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

/// Parses JSON refusing duplicate object keys and every number that is not a 64-bit integer
/// written without fraction or exponent (`invalid_request`).
pub fn parse_strict(bytes: &[u8]) -> Result<Value, AgentError> {
    parse_strict_raw(bytes)
        .map_err(|e| AgentError::with_message(ErrorKind::InvalidRequest, e.to_string()))
}

/// [`parse_strict`] with the parser's own error.
pub(crate) fn parse_strict_raw(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue::deserialize(&mut de)?;
    de.end()?;
    Ok(value.0)
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
            "numbers are 64-bit integers without fraction or exponent (AD12)",
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
