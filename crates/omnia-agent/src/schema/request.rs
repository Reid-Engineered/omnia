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
    validate::{Violation, escape, validate},
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

/// A schema violation at `path` in the request. The message does not repeat the path, so the
/// two request forms report the same message for the same fault.
fn violation(path: &str, message: &str) -> AgentError {
    AgentError::with_message(
        ErrorKind::SchemaViolation { path: path.into() },
        format!("schema violation: {message}"),
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

/// Validates tool arguments against `<op>.arguments`; the path is in arguments space.
fn conform_arguments(op: OpName, args: &Value) -> Result<(), Violation> {
    let schema =
        schema_value(&spec(op).arguments_schema()).expect("every operation has generated schemas");
    validate(schema, args)
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

/// The fault for an `idempotency_key` on an operation that takes none (section 11.2). Both
/// forms report it right after the limits, before the params are checked.
fn refused_key() -> AgentError {
    violation("/idempotency_key", "unknown field")
}

/// Whether the params of `op` have a field `name` (only `request_status` has
/// `idempotency_key`).
fn params_field(op: OpName, name: &str) -> bool {
    schema_value(&spec(op).params_schema())
        .and_then(|s| s.get("properties"))
        .is_some_and(|p| p.get(name).is_some())
}

/// The first reference token of a JSON Pointer, unescaped; `None` for the whole document.
fn first_token(path: &str) -> Option<String> {
    let rest = path.strip_prefix('/')?;
    let token = rest.split('/').next().unwrap_or_default();
    Some(token.replace("~1", "/").replace("~0", "~"))
}

/// One request (section 9.1).
///
/// It has two wire forms with the same meaning. The envelope `{op, params, idempotency_key?}`
/// is what `omnia call` and `omnia serve` read ([`Request::from_slice`]). The MCP form is a
/// tool's `arguments` object: the params with `idempotency_key` beside them, on the operations
/// section 11.2 names ([`Request::from_arguments`], schema `<op>.arguments`). Both decode to
/// the same `Request` through one order of checks, so the same fault gives the same error code,
/// details and message in both; only the JSON Pointer differs, by the envelope's `/params`.
///
/// Fields are private: a `Request` exists only if it decodes, so everything [`Request::to_json`]
/// and [`Request::to_arguments`] write decodes back to the same value. Build one with
/// [`Request::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    op: Operation,
    idempotency_key: Option<String>,
}

impl Request {
    /// A request, checked by the decoder: the result is exactly what decoding its envelope
    /// gives, or the error decoding reports.
    pub fn new(op: Operation, idempotency_key: Option<String>) -> Result<Self, AgentError> {
        let candidate = Self {
            op,
            idempotency_key,
        };
        let value = serde_json::to_value(&candidate).expect("requests always serialize");
        Self::from_envelope_value(value)
    }

    /// The operation and its params.
    pub fn op(&self) -> &Operation {
        &self.op
    }

    /// The idempotency key, on the operations that take one (section 11.2).
    pub fn idempotency_key(&self) -> Option<&str> {
        self.idempotency_key.as_deref()
    }

    /// The operation and the key.
    pub fn into_parts(self) -> (Operation, Option<String>) {
        (self.op, self.idempotency_key)
    }

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
    ///
    /// The envelope is checked in the arguments form's order: `op` and `params` must be
    /// present and `op` listed; then AK3 and AK4; then a key on an operation that takes none is
    /// refused; then the params, with the envelope's `idempotency_key` placed beside the params
    /// fields, against `<op>.arguments`. An unknown envelope field is ordered among the params
    /// fields by name (a fault on the params object itself comes first, and the envelope's
    /// field wins a tie), and the first fault is reported. Last, AK2 on the canonical forms.
    pub fn from_envelope_value(value: Value) -> Result<Self, AgentError> {
        let Value::Object(mut map) = value else {
            return Err(AgentError::with_message(
                ErrorKind::InvalidRequest,
                "a request is a JSON object",
            ));
        };
        let op = match map.remove("op") {
            Some(Value::String(name)) => available(&name)?,
            Some(_) => return Err(violation("/op", "expected string")),
            None => return Err(violation("", "missing field `op`")),
        };
        let mut params = map
            .remove("params")
            .ok_or_else(|| violation("", "missing field `params`"))?;
        check_limits(op, &params)?;
        let key = map.remove("idempotency_key");
        if key.is_some() && !spec(op).idempotency_key {
            return Err(refused_key());
        }
        // Envelope-only faults, each with the field name it is ordered by.
        let mut own: Vec<(String, String)> = map
            .into_iter()
            .map(|(k, _)| (k.clone(), format!("/{}", escape(&k))))
            .collect();
        // A key written inside the params, where the params have no such field, is an unknown
        // params field; it must not stand in for the envelope's key.
        if let Some(fields) = params.as_object_mut()
            && !params_field(op, "idempotency_key")
            && fields.remove("idempotency_key").is_some()
        {
            own.push((
                "idempotency_key".to_owned(),
                "/params/idempotency_key".to_owned(),
            ));
        }
        own.sort();
        let merged = match (&key, params.as_object_mut()) {
            (Some(k), Some(fields)) => {
                fields.insert("idempotency_key".to_owned(), k.clone());
                true
            }
            _ => false,
        };
        let fault = conform_arguments(op, &params).err();
        let to_envelope = |path: &str| {
            let own_key = path == "/idempotency_key" || path.starts_with("/idempotency_key/");
            if merged && own_key {
                path.to_owned()
            } else {
                format!("/params{path}")
            }
        };
        match (fault, own.first()) {
            (None, None) => {}
            (Some(f), first)
                if first.is_none_or(|(name, _)| first_token(&f.path).is_none_or(|t| t < *name)) =>
            {
                return Err(violation(&to_envelope(&f.path), &f.message));
            }
            (_, Some((_, path))) => return Err(violation(path, "unknown field")),
            (Some(_), None) => unreachable!("handled by the arm above"),
        }
        let idempotency_key = if merged {
            params
                .as_object_mut()
                .and_then(|m| m.remove("idempotency_key"))
                .and_then(|k| k.as_str().map(str::to_owned))
        } else {
            None
        };
        Self {
            op: Operation::decode(op, params, "/params")?,
            idempotency_key,
        }
        .within_size()
    }

    /// Decodes tool arguments already parsed by [`parse_strict`], in the same order of checks
    /// as [`Self::from_envelope_value`].
    pub fn from_arguments_value(op: OpName, value: Value) -> Result<Self, AgentError> {
        let op = available(op.as_str())?;
        check_limits(op, &value)?;
        if !spec(op).idempotency_key
            && !params_field(op, "idempotency_key")
            && value.get("idempotency_key").is_some()
        {
            return Err(refused_key());
        }
        conform_arguments(op, &value).map_err(|f| violation(&f.path, &f.message))?;
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
        Self {
            op: Operation::decode(op, params, "")?,
            idempotency_key,
        }
        .within_size()
    }

    /// AK2 on the canonical forms: a request is accepted only if both [`Self::to_json`] and
    /// [`Self::to_arguments`] fit, so whatever the API accepts or builds can be sent in either
    /// form and decoded again. The value reported is the larger form's size.
    fn within_size(self) -> Result<Self, AgentError> {
        let len = self.to_json().len().max(self.to_arguments().len());
        let len = u64::try_from(len).unwrap_or(u64::MAX);
        if len > MAX_REQUEST_BYTES {
            return Err(AgentError::new(ErrorKind::LimitExceeded {
                limit: Limit::RequestBytes,
                value: len,
            }));
        }
        Ok(self)
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
/// written without fraction or exponent (`invalid_request`). `-0` is read as `0`.
///
/// A refusal of well-formed JSON (a duplicate key, a number) has a message with no position,
/// so the two request forms report it alike. Malformed JSON keeps the parser's line and
/// column, which refer to the raw input.
pub fn parse_strict(bytes: &[u8]) -> Result<Value, AgentError> {
    parse_strict_raw(bytes)
        .map_err(|e| AgentError::with_message(ErrorKind::InvalidRequest, describe(&e)))
}

fn describe(e: &serde_json::Error) -> String {
    let text = e.to_string();
    if e.classify() == serde_json::error::Category::Data {
        let at = format!(" at line {} column {}", e.line(), e.column());
        if let Some(bare) = text.strip_suffix(&at) {
            return bare.to_owned();
        }
    }
    text
}

/// [`parse_strict`] with the parser's own error.
pub(crate) fn parse_strict_raw(bytes: &[u8]) -> Result<Value, serde_json::Error> {
    check_number_spelling(bytes)?;
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue::deserialize(&mut de)?;
    de.end()?;
    Ok(value.0)
}

/// Refuses a number token with a fraction or an exponent, outside strings. The parser hands
/// `-0` and integers beyond 64 bits to the visitor as doubles; after this scan a double can
/// only be one of those, so the visitor can tell them apart.
fn check_number_spelling(bytes: &[u8]) -> Result<(), serde_json::Error> {
    let (mut i, mut in_string, mut escaped) = (0, false, false);
    while let Some(&b) = bytes.get(i) {
        if in_string {
            match b {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'-' | b'0'..=b'9' => {
                let start = i;
                while bytes
                    .get(i)
                    .is_some_and(|c| matches!(c, b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'))
                {
                    i += 1;
                }
                if bytes[start..i]
                    .iter()
                    .any(|c| matches!(c, b'.' | b'e' | b'E'))
                {
                    return Err(de::Error::custom(
                        "numbers are integers written without fraction or exponent (AD12)",
                    ));
                }
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    Ok(())
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

    /// After [`check_number_spelling`] a double is `-0`, read as `0`, or an integer outside the
    /// 64-bit range, which is refused.
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Value, E> {
        if v == 0.0 {
            Ok(Value::Number(0u64.into()))
        } else {
            Err(E::custom("numbers are 64-bit integers (AD12)"))
        }
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
