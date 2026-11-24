//! `omnia mcp`: MCP over stdio (mcp-plan.md section 10, AK1, AQ1: hand-written JSON-RPC 2.0,
//! no async runtime).
//!
//! One process serves both protocol eras but picks one for its lifetime (section 10.5): the
//! first valid request decides. `initialize` selects the handshake era (2025-11-25 and
//! 2025-06-18); any other request carrying a `_meta` revision, typically a `server/discover`
//! probe, selects the stateless era (2026-07-28), where every request is then handled by its
//! own `_meta`. A message of the other era after that is a protocol error. No era is guessed
//! from other fields.
//!
//! The adapter holds no domain logic (AD1): tool calls decode through
//! `Request::from_arguments_value` and are answered by `AgentService::handle_in` in
//! [`Form::Arguments`], so a tool result's `structuredContent` is exactly the 9.1 envelope
//! the JSON-lines transport would print, and its text block is the same JSON. Tool failures
//! are results with `isError: true`; only malformed JSON-RPC messages and unknown tool names
//! are JSON-RPC protocol errors (section 10.1).
//!
//! Resources (section 10.2) are read from the same service: capabilities and schemas from the
//! catalog, documents through `inspect`/`nets` requests on the session, symbols from
//! `omnia_agent::read`. The open-document list is tracked here from the responses this
//! process itself returns (path and saved revision included), because the MCP process is
//! the only client of its session. Subscriptions are likewise tracked here: a document
//! revision change reported by a tool response emits `notifications/resources/updated` for
//! subscribed document URIs and `notifications/resources/list_changed` when documents open
//! or close, in each era's form (section 10.5).
//!
//! Prompts (section 10.3) are static text versioned with the schema; each has a transcript
//! fixture under `tests/transcripts/`. Logging is level-filtered `notifications/message`
//! (handshake era: `logging/setLevel`; stateless era: a request's
//! `io.modelcontextprotocol/logLevel` meta). `notifications/cancelled` names an in-flight
//! request: A1 operations are synchronous, so a cancellation that arrives is recorded and
//! reported on stderr, and the request it names has always already finished; A3.2 gives it
//! a job to cancel. Standard output carries only protocol messages; diagnostics go to
//! standard error.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, BufRead, Write},
};

use omnia_agent::{
    AgentService, Form, OpName, Operation, Request, SessionId,
    catalog::spec,
    read,
    schema::{Include, InspectArgs, NetsArgs, schema_text},
};
use serde_json::{Map, Value, json};

use crate::jsonl::{Frame, read_frame};

/// JSON-RPC error codes (JSON-RPC 2.0 and the pinned MCP revisions).
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
const SERVER_NOT_INITIALIZED: i64 = -32002;
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

/// The newest handshake revision, answered when `initialize` asks for anything else
/// (section 10.5).
const HANDSHAKE_REVISION: &str = "2025-11-25";
/// The stateless revision (AK1).
const STATELESS_REVISION: &str = "2026-07-28";

/// `_meta` keys of the stateless era.
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
const META_LOG_LEVEL: &str = "io.modelcontextprotocol/logLevel";
const META_PROGRESS_TOKEN: &str = "progressToken";

/// Server identity reported in both eras.
const SERVER_NAME: &str = "omnia";
/// Caching hints of the stateless era (section 10.5): the catalog and schemas never change
/// in a process, document resources are tracked below with their own shorter hint.
const STATIC_TTL_MS: u64 = 3_600_000;
const DOC_TTL_MS: u64 = 0;

/// Which protocol era this process serves (section 10.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Era {
    Handshake,
    Stateless,
}

/// An open document as this process last reported it, for `omnia://docs` and for the
/// revision-change notifications.
#[derive(Debug, Clone)]
struct TrackedDoc {
    path: Option<String>,
    revision: u64,
    saved_revision: u64,
}

/// One MCP stdio session: one era, one service session, its subscriptions and log level.
pub(crate) struct McpServer<'a> {
    service: &'a mut AgentService,
    session: SessionId,
    era: Option<Era>,
    /// Handshake era: the negotiated revision once `initialize` has been answered.
    negotiated: Option<String>,
    /// Resource URIs subscribed in the handshake era (`resources/subscribe`).
    subscribed: BTreeSet<String>,
    /// Stateless era: whether the listen filter asked for document list changes.
    listen_list_changed: bool,
    /// Stateless era: resource URIs from `subscriptions/listen`, tagged with the listen
    /// request's id in each notification's `_meta` (section 10.5).
    listen_uris: BTreeMap<String, Value>,
    /// The lowest log level emitted, as a rank in [`LEVELS`]; `None` emits nothing.
    log_level: Option<usize>,
    docs: BTreeMap<String, TrackedDoc>,
    /// Ids cancelled by `notifications/cancelled`; A1 calls are synchronous, so these have
    /// always already finished (section 10.4). Kept for the stderr diagnostic.
    cancelled: Vec<Value>,
}

/// Log levels in increasing severity (the pinned revisions' `LoggingLevel`).
const LEVELS: [&str; 8] = [
    "debug",
    "info",
    "notice",
    "warning",
    "error",
    "critical",
    "alert",
    "emergency",
];

/// Serves MCP on `input`/`out` until the input ends, on one new session of `service`.
/// Diagnostics go to `err`. Returns at the end of the input; the session is closed either
/// way. Standard output carries only protocol messages.
pub(crate) fn serve(
    service: &mut AgentService,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<()> {
    let session = service.open_session();
    let mut server = McpServer {
        service,
        session,
        era: None,
        negotiated: None,
        subscribed: BTreeSet::new(),
        listen_list_changed: false,
        listen_uris: BTreeMap::new(),
        log_level: None,
        docs: BTreeMap::new(),
        cancelled: Vec::new(),
    };
    let result = server.run(input, out, err);
    server.service.close_session(server.session);
    result
}

impl McpServer<'_> {
    fn run(
        &mut self,
        input: &mut dyn BufRead,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> io::Result<()> {
        while let Some(frame) = read_frame(input, Some(b'\n'))? {
            let frame = strip_cr(frame);
            if frame
                .iter()
                .all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
            {
                continue;
            }
            for line in self.handle_line(&frame, err) {
                out.write_all(line.as_bytes())?;
                out.write_all(b"\n")?;
                out.flush()?;
            }
        }
        Ok(())
    }

    /// Handles one input line and returns the output lines it produces (a response, then
    /// any notifications the call caused). Notifications and malformed messages without an
    /// id produce no direct response.
    fn handle_line(&mut self, bytes: &[u8], err: &mut dyn Write) -> Vec<String> {
        let value = match parse_line(bytes) {
            Ok(value) => value,
            Err(message) => {
                return vec![error_line(
                    &Value::Null,
                    PARSE_ERROR,
                    &message,
                    None,
                    self.era,
                )];
            }
        };
        let Some(object) = value.as_object() else {
            return vec![error_line(
                &Value::Null,
                INVALID_REQUEST,
                "a JSON-RPC message is an object",
                None,
                self.era,
            )];
        };
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let id = object.get("id").cloned();
        let params = object.get("params").cloned().unwrap_or_else(|| json!({}));

        // A notification: no id, no response of its own (a log line may follow).
        let Some(id) = id else {
            return self.handle_notification(method.as_deref(), &params, err);
        };
        let Some(method) = method else {
            return vec![error_line(
                &id,
                INVALID_REQUEST,
                "a request has a method",
                None,
                self.era,
            )];
        };
        // Era selection on the first valid request (section 10.5). `initialize` is a
        // handshake-era message even when its params carry no `_meta`; a `server/discover`
        // or any request whose `_meta` names a revision is a stateless-era message.
        if self.era.is_none() {
            if method == "initialize" {
                self.era = Some(Era::Handshake);
            } else if method == "server/discover" || meta_version(&params).is_some() {
                self.era = Some(Era::Stateless);
            } else {
                return vec![error_line(
                    &id,
                    SERVER_NOT_INITIALIZED,
                    "the first request must be initialize or carry a _meta protocol revision",
                    None,
                    self.era,
                )];
            }
        }
        let era = self.era.expect("selected above");
        match era {
            Era::Handshake => self.handshake_request(&id, &method, &params, err),
            Era::Stateless => self.stateless_request(&id, &method, &params, err),
        }
    }

    fn handle_notification(
        &mut self,
        method: Option<&str>,
        params: &Value,
        err: &mut dyn Write,
    ) -> Vec<String> {
        match method {
            // Both eras: cancellation names an in-flight request (section 10.4). A1 calls
            // are synchronous, so the request has finished; record it, log it to standard
            // error, and say so in a log notification when the client asked for that level.
            Some("notifications/cancelled") => {
                if let Some(request_id) = params.get("requestId") {
                    self.cancelled.push(request_id.clone());
                    let _ = writeln!(
                        err,
                        "omnia mcp: cancellation for {request_id} arrived after it finished"
                    );
                    return self
                        .log(
                            "warning",
                            json!(format!(
                                "cancellation for {request_id} arrived after it finished"
                            )),
                        )
                        .into_iter()
                        .collect();
                }
                Vec::new()
            }
            Some("notifications/initialized") if self.era == Some(Era::Handshake) => Vec::new(),
            Some("notifications/initialized") => {
                let _ = writeln!(
                    err,
                    "omnia mcp: notifications/initialized ignored outside the handshake era"
                );
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// A `notifications/message` log line at `level`, emitted only when the client asked
    /// for that level or a lower one (section 10.4: handshake `logging/setLevel`, stateless
    /// per-request log-level meta).
    fn log(&self, level: &str, data: Value) -> Option<String> {
        let rank = LEVELS.iter().position(|l| *l == level)?;
        let threshold = self.log_level?;
        (rank >= threshold).then(|| {
            notification_line(
                "notifications/message",
                json!({ "level": level, "logger": SERVER_NAME, "data": data }),
                self.era,
                None,
            )
        })
    }

    // ---- handshake era ----

    fn handshake_request(
        &mut self,
        id: &Value,
        method: &str,
        params: &Value,
        err: &mut dyn Write,
    ) -> Vec<String> {
        if method == "server/discover" || meta_version(params) == Some(STATELESS_REVISION) {
            return vec![error_line(
                id,
                SERVER_NOT_INITIALIZED,
                "this process serves the handshake era; stateless-era messages are refused",
                None,
                self.era,
            )];
        }
        match method {
            "initialize" => {
                let requested = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let revision = if omnia_agent::PROTOCOL_REVISIONS[1..].contains(&requested) {
                    requested.to_owned()
                } else {
                    HANDSHAKE_REVISION.to_owned()
                };
                self.negotiated = Some(revision.clone());
                vec![result_line(
                    id,
                    json!({
                        "protocolVersion": revision,
                        "capabilities": server_capabilities(),
                        "serverInfo": server_info(),
                    }),
                    self.era,
                )]
            }
            _ if self.negotiated.is_none() => vec![error_line(
                id,
                SERVER_NOT_INITIALIZED,
                "the first request must be initialize",
                None,
                self.era,
            )],
            "ping" => vec![result_line(id, json!({}), self.era)],
            "logging/setLevel" => match params.get("level").and_then(Value::as_str) {
                Some(level) => {
                    self.log_level = LEVELS.iter().position(|l| *l == level);
                    if self.log_level.is_none() {
                        return vec![invalid_params(id, "unknown logging level", self.era)];
                    }
                    vec![result_line(id, json!({}), self.era)]
                }
                None => vec![invalid_params(
                    id,
                    "logging/setLevel needs a level",
                    self.era,
                )],
            },
            _ => self.common_request(id, method, params, err),
        }
    }

    // ---- stateless era ----

    fn stateless_request(
        &mut self,
        id: &Value,
        method: &str,
        params: &Value,
        err: &mut dyn Write,
    ) -> Vec<String> {
        if method == "initialize" {
            return vec![error_line(
                id,
                SERVER_NOT_INITIALIZED,
                "this process serves the stateless era; initialize is refused",
                None,
                self.era,
            )];
        }
        // Every request carries its revision and the client's capabilities (section 10.5).
        let meta = params.get("_meta");
        let requested = meta_version(params);
        let Some(requested) = requested else {
            return vec![invalid_params(
                id,
                &format!("missing required _meta {META_VERSION}"),
                self.era,
            )];
        };
        if requested != STATELESS_REVISION {
            if omnia_agent::PROTOCOL_REVISIONS.contains(&requested) {
                return vec![error_line(
                    id,
                    SERVER_NOT_INITIALIZED,
                    "this process serves the stateless era; handshake-era revisions are refused",
                    None,
                    self.era,
                )];
            }
            return vec![error_line(
                id,
                UNSUPPORTED_PROTOCOL_VERSION,
                "Unsupported protocol version",
                Some(json!({
                    "supported": omnia_agent::PROTOCOL_REVISIONS,
                    "requested": requested,
                })),
                self.era,
            )];
        }
        let capabilities_ok = meta
            .and_then(|m| m.get(META_CLIENT_CAPABILITIES))
            .is_some_and(Value::is_object);
        if !capabilities_ok {
            return vec![invalid_params(
                id,
                &format!("missing required _meta {META_CLIENT_CAPABILITIES}"),
                self.era,
            )];
        }
        // Protocol logging in this era only when the request asks for a level (10.5).
        if let Some(level) = meta
            .and_then(|m| m.get(META_LOG_LEVEL))
            .and_then(Value::as_str)
        {
            self.log_level = LEVELS.iter().position(|l| *l == level);
        }
        let _ = META_CLIENT_INFO; // identity is carried but not needed by any A1 answer
        match method {
            "server/discover" => vec![result_line(
                id,
                json!({
                    "supportedVersions": omnia_agent::PROTOCOL_REVISIONS,
                    "capabilities": server_capabilities(),
                    "_meta": { META_SERVER_INFO: server_info() },
                    "ttlMs": STATIC_TTL_MS,
                    "cacheScope": "public",
                }),
                self.era,
            )],
            "ping" => vec![error_line(
                id,
                METHOD_NOT_FOUND,
                "ping was removed in this protocol revision",
                None,
                self.era,
            )],
            "subscriptions/listen" => self.listen(id, params),
            _ => self.common_request(id, method, params, err),
        }
    }

    /// Registers the listen filter (section 10.5). On stdio there is one channel, so the
    /// result acknowledges the honoured subset and later notifications go on the same
    /// stream, each tagged with this request's id as its subscription id.
    fn listen(&mut self, id: &Value, params: &Value) -> Vec<String> {
        let Some(notifications) = params.get("notifications").and_then(Value::as_object) else {
            return vec![invalid_params(
                id,
                "subscriptions/listen needs a notifications filter",
                self.era,
            )];
        };
        self.listen_list_changed = notifications
            .get("resourcesListChanged")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if let Some(uris) = notifications
            .get("resourceSubscriptions")
            .and_then(Value::as_array)
        {
            for uri in uris {
                if let Some(uri) = uri.as_str() {
                    self.listen_uris.insert(uri.to_owned(), id.clone());
                }
            }
        }
        vec![result_line(
            id,
            json!({
                "notifications": {
                    "resourcesListChanged": self.listen_list_changed,
                    "resourceSubscriptions": self.listen_uris.keys().collect::<Vec<_>>(),
                }
            }),
            self.era,
        )]
    }

    // ---- both eras ----

    fn common_request(
        &mut self,
        id: &Value,
        method: &str,
        params: &Value,
        err: &mut dyn Write,
    ) -> Vec<String> {
        match method {
            "tools/list" => vec![result_line(id, self.tools_list(params), self.era)],
            "tools/call" => self.tools_call(id, params, err),
            "resources/list" => vec![result_line(id, self.resources_list(), self.era)],
            "resources/templates/list" => {
                vec![result_line(id, self.resource_templates(), self.era)]
            }
            "resources/read" => match params.get("uri").and_then(Value::as_str) {
                Some(uri) => match self.resource_read(uri) {
                    Ok(contents) => {
                        let dynamic = uri.starts_with("omnia://doc") || uri == "omnia://docs";
                        let result = if dynamic {
                            with_cache(json!({ "contents": contents }), DOC_TTL_MS, self.era)
                        } else {
                            with_cache(json!({ "contents": contents }), STATIC_TTL_MS, self.era)
                        };
                        vec![result_line(id, result, self.era)]
                    }
                    Err(message) => vec![invalid_params(id, &message, self.era)],
                },
                None => vec![invalid_params(id, "resources/read needs a uri", self.era)],
            },
            "resources/subscribe" if self.era == Some(Era::Handshake) => {
                match params.get("uri").and_then(Value::as_str) {
                    Some(uri) => {
                        self.subscribed.insert(uri.to_owned());
                        vec![result_line(id, json!({}), self.era)]
                    }
                    None => vec![invalid_params(
                        id,
                        "resources/subscribe needs a uri",
                        self.era,
                    )],
                }
            }
            "resources/unsubscribe" if self.era == Some(Era::Handshake) => {
                match params.get("uri").and_then(Value::as_str) {
                    Some(uri) => {
                        self.subscribed.remove(uri);
                        vec![result_line(id, json!({}), self.era)]
                    }
                    None => vec![invalid_params(
                        id,
                        "resources/unsubscribe needs a uri",
                        self.era,
                    )],
                }
            }
            "prompts/list" => vec![result_line(id, prompts_list(), self.era)],
            "prompts/get" => match prompt_get(params) {
                Ok(result) => vec![result_line(id, result, self.era)],
                Err(message) => vec![invalid_params(id, &message, self.era)],
            },
            _ => vec![error_line(
                id,
                METHOD_NOT_FOUND,
                &format!("unknown method {method}"),
                None,
                self.era,
            )],
        }
    }

    fn tools_list(&self, _params: &Value) -> Value {
        let tools: Vec<Value> = omnia_agent::OpName::ALL
            .iter()
            .copied()
            .filter(|&op| spec(op).available)
            .map(|op| {
                let s = spec(op);
                json!({
                    "name": s.tool,
                    "description": s.description,
                    "inputSchema": schema_value(&s.arguments_schema()),
                    "outputSchema": schema_value(&s.response_schema()),
                    "annotations": {
                        "readOnlyHint": s.read_only,
                        "destructiveHint": s.destructive,
                        "idempotentHint": s.idempotent,
                        "openWorldHint": false,
                    },
                })
            })
            .collect();
        with_cache(json!({ "tools": tools }), STATIC_TTL_MS, self.era)
    }

    /// One tool call. The progress token, if any, is remembered only for the call's
    /// duration: A1 calls are synchronous and emit no progress (A3.2 adds it).
    fn tools_call(&mut self, id: &Value, params: &Value, _err: &mut dyn Write) -> Vec<String> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return vec![invalid_params(id, "tools/call needs a tool name", self.era)];
        };
        let Some(op) = omnia_agent::OpName::ALL
            .iter()
            .copied()
            .find(|&op| spec(op).tool == name)
        else {
            return vec![error_line(
                id,
                METHOD_NOT_FOUND,
                &format!("unknown tool {name}"),
                None,
                self.era,
            )];
        };
        if !spec(op).available {
            return vec![error_line(
                id,
                METHOD_NOT_FOUND,
                &format!("unknown tool {name}"),
                None,
                self.era,
            )];
        }
        let _progress_token = params
            .get("_meta")
            .and_then(|m| m.get(META_PROGRESS_TOKEN))
            .cloned();
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let before_docs = self.docs.clone();
        let before_revisions: BTreeMap<String, u64> = self
            .docs
            .iter()
            .map(|(d, t)| (d.clone(), t.revision))
            .collect();
        let response_json = match Request::from_arguments_value(op, arguments) {
            Ok(request) => {
                let response = self
                    .service
                    .handle_in(self.session, &request, Form::Arguments);
                response.to_json()
            }
            Err(error) => {
                // `from_arguments_value` already reports in arguments space (9.1).
                omnia_agent::Response::new(None, None, Err(error))
                    .expect("an error response needs no document")
                    .to_json()
            }
        };
        let envelope: Value = serde_json::from_str(&response_json).expect("responses are JSON");
        let is_error = envelope.get("error").is_some();
        self.track(op, &envelope);
        let result = json!({
            "content": [{ "type": "text", "text": response_json }],
            "structuredContent": envelope,
            "isError": is_error,
        });
        let mut lines = vec![result_line(id, result, self.era)];
        lines.extend(self.change_notifications(&before_docs, &before_revisions));
        lines
    }

    /// Updates the tracked open-document list from a tool response (see the module docs).
    fn track(&mut self, op: OpName, envelope: &Value) {
        let result = envelope.get("result");
        let doc_id = envelope
            .get("doc_id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        match op {
            OpName::New | OpName::Open => {
                if let (Some(doc_id), Some(result)) = (doc_id, result) {
                    let revision = envelope
                        .get("revision")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    self.docs.insert(
                        doc_id,
                        TrackedDoc {
                            path: result
                                .get("path")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            revision,
                            saved_revision: revision,
                        },
                    );
                }
            }
            OpName::Close => {
                if let Some(doc_id) = doc_id
                    && envelope.get("error").is_none()
                {
                    self.docs.remove(&doc_id);
                }
            }
            OpName::Save => {
                if let (Some(doc_id), Some(result)) = (doc_id, result)
                    && envelope.get("error").is_none()
                {
                    let saved = result
                        .get("saved_revision")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    if let Some(tracked) = self.docs.get_mut(&doc_id) {
                        tracked.saved_revision = saved;
                        if let Some(path) = result.get("path").and_then(Value::as_str) {
                            tracked.path = Some(path.to_owned());
                        }
                    }
                }
            }
            _ => {
                if let (Some(doc_id), Some(revision)) =
                    (doc_id, envelope.get("revision").and_then(Value::as_u64))
                    && let Some(tracked) = self.docs.get_mut(&doc_id)
                {
                    tracked.revision = revision;
                }
            }
        }
    }

    /// The notifications a finished call causes: document list changes, then revision
    /// updates for subscribed URIs, each era in its own form (sections 10.4 and 10.5).
    fn change_notifications(
        &self,
        before_docs: &BTreeMap<String, TrackedDoc>,
        before_revisions: &BTreeMap<String, u64>,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        let opened_or_closed =
            before_docs.len() != self.docs.len() || before_docs.keys().ne(self.docs.keys());
        if opened_or_closed {
            let wanted = match self.era {
                Some(Era::Handshake) => true,
                Some(Era::Stateless) => self.listen_list_changed,
                None => false,
            };
            if wanted {
                lines.push(notification_line(
                    "notifications/resources/list_changed",
                    json!({}),
                    self.era,
                    None,
                ));
            }
        }
        for (doc_id, tracked) in &self.docs {
            let changed = before_revisions.get(doc_id) != Some(&tracked.revision)
                && before_revisions.contains_key(doc_id);
            if !changed {
                continue;
            }
            for uri in [
                format!("omnia://doc/{doc_id}"),
                format!("omnia://doc/{doc_id}/items"),
                format!("omnia://doc/{doc_id}/nets"),
            ] {
                match self.era {
                    Some(Era::Handshake) if self.subscribed.contains(&uri) => {
                        lines.push(notification_line(
                            "notifications/resources/updated",
                            json!({ "uri": uri }),
                            self.era,
                            None,
                        ));
                    }
                    Some(Era::Stateless) => {
                        if let Some(listen_id) = self.listen_uris.get(&uri) {
                            lines.push(notification_line(
                                "notifications/resources/updated",
                                json!({ "uri": uri }),
                                self.era,
                                Some(listen_id.clone()),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        lines
    }

    // ---- resources ----

    fn resources_list(&self) -> Value {
        let mut resources = vec![
            resource(
                "omnia://capabilities",
                "capabilities",
                "The capability catalog.",
            ),
            resource("omnia://docs", "docs", "Open documents."),
            resource("omnia://symbols", "symbols", "The symbol library."),
        ];
        for doc_id in self.docs.keys() {
            resources.push(resource(
                &format!("omnia://doc/{doc_id}"),
                &format!("doc {doc_id}"),
                "A document summary.",
            ));
            resources.push(resource(
                &format!("omnia://doc/{doc_id}/items"),
                &format!("doc {doc_id} items"),
                "A document's items, paged.",
            ));
            resources.push(resource(
                &format!("omnia://doc/{doc_id}/nets"),
                &format!("doc {doc_id} nets"),
                "A document's canonical nets.",
            ));
        }
        with_cache(json!({ "resources": resources }), STATIC_TTL_MS, self.era)
    }

    fn resource_templates(&self) -> Value {
        with_cache(
            json!({
                "resourceTemplates": [
                    { "uriTemplate": "omnia://schema/{name}", "name": "schema",
                      "description": "A published JSON Schema.", "mimeType": "application/json" },
                    { "uriTemplate": "omnia://doc/{doc_id}", "name": "doc",
                      "description": "A document summary.", "mimeType": "application/json" },
                    { "uriTemplate": "omnia://doc/{doc_id}/items{?cursor}", "name": "doc items",
                      "description": "A document's items, paged.", "mimeType": "application/json" },
                    { "uriTemplate": "omnia://doc/{doc_id}/nets", "name": "doc nets",
                      "description": "A document's canonical nets.", "mimeType": "application/json" },
                    { "uriTemplate": "omnia://symbols/{name}", "name": "symbol",
                      "description": "One symbol.", "mimeType": "application/json" },
                ]
            }),
            STATIC_TTL_MS,
            self.era,
        )
    }

    fn resource_read(&mut self, uri: &str) -> Result<Vec<Value>, String> {
        let text = |uri: &str, text: String| {
            vec![json!({ "uri": uri, "mimeType": "application/json", "text": text })]
        };
        match uri {
            "omnia://capabilities" => {
                let catalog =
                    serde_json::to_string(self.service.catalog()).expect("the catalog serializes");
                Ok(text(uri, catalog))
            }
            "omnia://docs" => {
                let docs: Vec<Value> = self
                    .docs
                    .iter()
                    .map(|(doc_id, tracked)| {
                        json!({
                            "doc_id": doc_id,
                            "path": tracked.path,
                            "revision": tracked.revision,
                            "dirty": tracked.revision != tracked.saved_revision,
                        })
                    })
                    .collect();
                Ok(text(uri, canonical(&Value::Array(docs))))
            }
            "omnia://symbols" => {
                let symbols = read::symbols(self.service.symbols());
                Ok(text(
                    uri,
                    serde_json::to_string(&symbols).expect("serializes"),
                ))
            }
            _ => {
                if let Some(name) = uri.strip_prefix("omnia://schema/") {
                    return match schema_text(name.strip_suffix(".json").unwrap_or(name)) {
                        Some(schema) => Ok(text(uri, schema)),
                        None => Err(format!("no schema named {name}")),
                    };
                }
                if let Some(name) = uri.strip_prefix("omnia://symbols/") {
                    return match read::symbol(self.service.symbols(), name) {
                        Some(symbol) => Ok(text(
                            uri,
                            serde_json::to_string(&symbol).expect("serializes"),
                        )),
                        None => Err(format!("no symbol named {name}")),
                    };
                }
                if let Some(rest) = uri.strip_prefix("omnia://doc/") {
                    return self.doc_resource(uri, rest).map(|t| text(uri, t));
                }
                Err(format!("unknown resource {uri}"))
            }
        }
    }

    /// `omnia://doc/{id}`, `.../items{?cursor}` and `.../nets`, answered through the
    /// session's own read operations so the bytes match the tools'.
    fn doc_resource(&mut self, _uri: &str, rest: &str) -> Result<String, String> {
        let (doc_part, query) = match rest.split_once('?') {
            Some((head, query)) => (head, Some(query)),
            None => (rest, None),
        };
        let (doc_id_text, kind) = match doc_part.split_once('/') {
            Some((doc_id, kind)) => (doc_id, kind),
            None => (doc_part, ""),
        };
        let doc_id =
            parse_doc_id(doc_id_text).ok_or_else(|| format!("no open document {doc_id_text}"))?;
        if !self.docs.contains_key(doc_id_text) {
            return Err(format!("no open document {doc_id_text}"));
        }
        let cursor = query.and_then(|q| {
            q.split('&')
                .find_map(|pair| pair.strip_prefix("cursor="))
                .map(str::to_owned)
        });
        let (op, request) = match kind {
            "" => (
                OpName::Inspect,
                Request::new(
                    Operation::Inspect(InspectArgs {
                        doc_id,
                        include: vec![Include::Summary, Include::Nets],
                        area: None,
                        cursor: None,
                        limit: None,
                    }),
                    None,
                ),
            ),
            "items" => (
                OpName::Inspect,
                Request::new(
                    Operation::Inspect(InspectArgs {
                        doc_id,
                        include: vec![Include::Items],
                        area: None,
                        cursor,
                        limit: None,
                    }),
                    None,
                ),
            ),
            "nets" => (
                OpName::Nets,
                Request::new(
                    Operation::Nets(NetsArgs {
                        doc_id,
                        at: None,
                        wire: None,
                    }),
                    None,
                ),
            ),
            _ => return Err(format!("unknown resource omnia://doc/{doc_part}")),
        };
        let request = request.map_err(|e| e.message.clone())?;
        let _ = op;
        let response = self.service.handle(self.session, &request);
        Ok(response.to_json())
    }
}

// ---- wire helpers ----

/// Removes a consumed `\r\n` line end, mirroring the JSON-lines transport.
fn strip_cr(frame: Frame) -> Vec<u8> {
    let mut bytes = frame.bytes().to_vec();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    bytes
}

/// Strict JSON for one line: duplicate keys and non-integer numbers are parse errors here,
/// as they are for the service's decoders (section 9.1).
fn parse_line(bytes: &[u8]) -> Result<Value, String> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > omnia_agent::MAX_REQUEST_BYTES {
        return Err("the message is over the request size limit".to_owned());
    }
    omnia_agent::schema::parse_strict(bytes).map_err(|e| e.message.clone())
}

fn meta_version(params: &Value) -> Option<&str> {
    params
        .get("_meta")
        .and_then(|m| m.get(META_VERSION))
        .and_then(Value::as_str)
}

fn server_info() -> Value {
    json!({ "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") })
}

fn server_capabilities() -> Value {
    json!({
        "logging": {},
        "prompts": { "listChanged": false },
        "resources": { "subscribe": true, "listChanged": true },
        "tools": { "listChanged": false },
    })
}

fn schema_value(name: &str) -> Value {
    schema_text(name)
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({ "type": "object" }))
}

/// Adds the stateless era's `resultType` and caching hints (section 10.5); a no-op in the
/// handshake era.
fn with_cache(mut result: Value, ttl_ms: u64, era: Option<Era>) -> Value {
    if era == Some(Era::Stateless)
        && let Value::Object(map) = &mut result
    {
        map.insert("ttlMs".to_owned(), json!(ttl_ms));
        map.insert(
            "cacheScope".to_owned(),
            json!(if ttl_ms == 0 { "private" } else { "public" }),
        );
    }
    result
}

/// Compact canonical JSON: object keys sorted at every depth, so the bytes do not depend
/// on `serde_json`'s `preserve_order` feature, which other workspace crates turn on (AD8).
fn canonical(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("strings serialize"));
                out.push(':');
                write_canonical(&map[key.as_str()], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar => out.push_str(&serde_json::to_string(scalar).expect("scalars serialize")),
    }
}

fn result_line(id: &Value, mut result: Value, era: Option<Era>) -> String {
    if era == Some(Era::Stateless)
        && let Value::Object(map) = &mut result
    {
        let mut with_type = Map::new();
        with_type.insert("resultType".to_owned(), json!("complete"));
        for (key, value) in std::mem::take(map) {
            with_type.insert(key, value);
        }
        result = Value::Object(with_type);
    }
    canonical(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn error_line(
    id: &Value,
    code: i64,
    message: &str,
    data: Option<Value>,
    _era: Option<Era>,
) -> String {
    let mut error = json!({ "code": code, "message": message });
    if let Some(data) = data {
        error["data"] = data;
    }
    canonical(&json!({ "jsonrpc": "2.0", "id": id, "error": error }))
}

fn invalid_params(id: &Value, message: &str, era: Option<Era>) -> String {
    error_line(id, INVALID_PARAMS, message, None, era)
}

fn notification_line(
    method: &str,
    params: Value,
    _era: Option<Era>,
    subscription_id: Option<Value>,
) -> String {
    let mut params = params;
    if let Some(subscription_id) = subscription_id {
        params["_meta"] = json!({ "io.modelcontextprotocol/subscriptionId": subscription_id });
    }
    canonical(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
}

fn resource(uri: &str, name: &str, description: &str) -> Value {
    json!({
        "uri": uri,
        "name": name,
        "description": description,
        "mimeType": "application/json",
    })
}

fn parse_doc_id(text: &str) -> Option<omnia_agent::schema::DocId> {
    let number: u64 = text.strip_prefix('d')?.parse().ok()?;
    let value = json!(format!("d{number}"));
    serde_json::from_value(value).ok()
}

// ---- prompts (section 10.3) ----

struct PromptSpec {
    name: &'static str,
    description: &'static str,
    arguments: &'static [(&'static str, &'static str, bool)],
    text: &'static str,
}

const PROMPTS: &[PromptSpec] = &[
    PromptSpec {
        name: "omnia_inspect_sheet",
        description: "Read the summary and nets; report free ends and nets with a single pin.",
        arguments: &[("doc_id", "The document to inspect, for example d1.", true)],
        text: "Call omnia_inspect with doc_id {doc_id} and include [\"summary\", \"nets\"]. \
               Report the part, wire and label counts, then list every net that has a single \
               pin and every pin that is a net of its own (a free end). Do not edit anything.",
    },
    PromptSpec {
        name: "omnia_place_and_wire",
        description: "Place parts and wire them, following the safe loop of section 13.",
        arguments: &[
            ("doc_id", "The document to edit, for example d1.", true),
            (
                "parts",
                "The parts to place, by symbol name with designators.",
                true,
            ),
            ("connections", "The connections to wire, by part pin.", true),
        ],
        text: "In document {doc_id}, place these parts: {parts}. Wire these connections: \
               {connections}. First omnia_inspect the document, then omnia_preview the whole \
               batch, read the predicted change, and only then omnia_apply it with the \
               preview_id, the expected_revision and an idempotency_key. Finish with \
               omnia_nets and report the nets that joined.",
    },
    PromptSpec {
        name: "omnia_safe_edit_loop",
        description: "The verification protocol of section 13, as instructions.",
        arguments: &[],
        text: "Work in this loop: 1. omnia_capabilities. 2. omnia_open or omnia_new, then \
               omnia_inspect. 3. omnia_preview the batch; read the predicted pins and ids. \
               4. omnia_apply with preview_id, expected_revision and idempotency_key. \
               5. omnia_nets to check connectivity. 6. omnia_save to a new path; check the \
               response; reopen and compare. Never invent ids, params or symbol names, and \
               never edit the only copy of a file.",
    },
];

fn prompts_list() -> Value {
    let prompts: Vec<Value> = PROMPTS
        .iter()
        .map(|prompt| {
            json!({
                "name": prompt.name,
                "description": prompt.description,
                "arguments": prompt.arguments.iter().map(|(name, description, required)| {
                    json!({ "name": name, "description": description, "required": required })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "prompts": prompts })
}

fn prompt_get(params: &Value) -> Result<Value, String> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err("prompts/get needs a prompt name".to_owned());
    };
    let Some(prompt) = PROMPTS.iter().find(|p| p.name == name) else {
        return Err(format!("unknown prompt {name}"));
    };
    let arguments = params.get("arguments").and_then(Value::as_object);
    let mut text = prompt.text.to_owned();
    for (argument, _, required) in prompt.arguments {
        let value = arguments
            .and_then(|args| args.get(*argument))
            .and_then(Value::as_str);
        match value {
            Some(value) => text = text.replace(&format!("{{{argument}}}"), value),
            None if *required => {
                return Err(format!("prompt {name} needs argument {argument}"));
            }
            None => text = text.replace(&format!("{{{argument}}}"), ""),
        }
    }
    Ok(json!({
        "description": prompt.description,
        "messages": [{
            "role": "user",
            "content": { "type": "text", "text": text },
        }],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_published_schema_is_a_resource_name() {
        let names: Vec<String> = omnia_agent::schema::schema_files()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert!(!names.is_empty());
        for name in names {
            assert!(schema_text(&name).is_some(), "{name}");
        }
    }

    /// Runs one transcript fixture pair and returns the output lines.
    fn run_transcript(name: &str) -> Vec<String> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/transcripts");
        let input = std::fs::read(format!("{dir}/{name}.in.jsonl")).expect("input fixture");
        let expected =
            std::fs::read_to_string(format!("{dir}/{name}.expected.jsonl")).expect("expected");
        let mut service = AgentService::headless();
        let mut input_slice: &[u8] = &input;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        serve(&mut service, &mut input_slice, &mut out, &mut err).expect("serve");
        let actual = String::from_utf8(out).expect("output is UTF-8");
        if actual != expected {
            let actual_lines: Vec<&str> = actual.lines().collect();
            let expected_lines: Vec<&str> = expected.lines().collect();
            for (index, (a, e)) in actual_lines.iter().zip(&expected_lines).enumerate() {
                assert_eq!(a, e, "{name} line {} differs", index + 1);
            }
            panic!(
                "{name}: {} actual lines, {} expected lines",
                actual_lines.len(),
                expected_lines.len()
            );
        }
        actual.lines().map(str::to_owned).collect()
    }

    /// ATC-mcp-01: handshake, tools/list, then the ATC-edit-01 loop over MCP, exact bytes.
    #[test]
    fn atc_mcp_01_handshake_edit_loop_exact_transcript() {
        let lines = run_transcript("atc-mcp-01");
        // Independent of the fixture bytes: tools/list names are the catalog's tools.
        let tools_line: Value = serde_json::from_str(&lines[1]).expect("tools/list line");
        let names: Vec<&str> = tools_line["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|t| t["name"].as_str().expect("tool name"))
            .collect();
        let catalog: Vec<&str> = omnia_agent::OpName::ALL
            .iter()
            .copied()
            .filter(|&op| spec(op).available)
            .map(|op| spec(op).tool)
            .collect();
        assert_eq!(names, catalog);
        // The wire joined the two resistors into one net of two pins.
        let nets_line: Value = serde_json::from_str(&lines[7]).expect("nets line");
        let nets = nets_line["result"]["structuredContent"]["result"]["nets"]
            .as_array()
            .expect("nets");
        assert!(
            nets.iter()
                .any(|net| net["pins"].as_array().is_some_and(|pins| pins.len() == 2))
        );
    }

    /// ATC-mcp-02: a malformed JSON-RPC message is a protocol error; a tool call that fails
    /// in the domain is a result with `isError` and the structured error.
    #[test]
    fn atc_mcp_02_malformed_and_domain_failure_exact_transcript() {
        let lines = run_transcript("atc-mcp-02");
        let malformed: Value = serde_json::from_str(&lines[6]).expect("parse error line");
        assert_eq!(malformed["error"]["code"], PARSE_ERROR);
        let domain: Value = serde_json::from_str(&lines[2]).expect("domain failure line");
        assert_eq!(domain["result"]["isError"], true);
        assert_eq!(
            domain["result"]["structuredContent"]["error"]["code"],
            "document_not_found"
        );
    }

    /// The stateless era: discover, per-request `_meta`, caching hints, listen-tagged
    /// notifications, and the era refusals of section 10.5.
    #[test]
    fn atc_mcp_03_stateless_era_exact_transcript() {
        let lines = run_transcript("atc-mcp-03-stateless");
        let discover: Value = serde_json::from_str(&lines[0]).expect("discover line");
        assert_eq!(discover["result"]["resultType"], "complete");
        assert_eq!(
            discover["result"]["supportedVersions"],
            json!(omnia_agent::PROTOCOL_REVISIONS)
        );
        let unsupported: Value = serde_json::from_str(&lines[10]).expect("unsupported line");
        assert_eq!(unsupported["error"]["code"], UNSUPPORTED_PROTOCOL_VERSION);
        assert_eq!(unsupported["error"]["data"]["requested"], "1900-01-01");
    }

    /// Every prompt of section 10.3 has a fixture (prompts/list and prompts/get).
    #[test]
    fn prompts_exact_transcript() {
        let lines = run_transcript("prompts");
        let list: Value = serde_json::from_str(&lines[1]).expect("prompts/list line");
        let names: Vec<&str> = list["result"]["prompts"]
            .as_array()
            .expect("prompts")
            .iter()
            .map(|p| p["name"].as_str().expect("prompt name"))
            .collect();
        assert_eq!(names, PROMPTS.iter().map(|p| p.name).collect::<Vec<_>>());
    }

    #[test]
    fn prompts_each_have_a_get_result() {
        for prompt in PROMPTS {
            let mut params = json!({ "name": prompt.name, "arguments": {} });
            for (argument, _, _) in prompt.arguments {
                params["arguments"][argument] = json!("d1");
            }
            let result = prompt_get(&params).expect("prompt get");
            assert!(result["messages"][0]["content"]["text"].is_string());
        }
    }
}
