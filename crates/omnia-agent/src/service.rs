//! The service core (A1.5): sessions, revisions, idempotency and dispatch (mcp-plan.md sections
//! 5, 11 and 14).
//!
//! [`AgentService::handle`] answers one decoded [`Request`] for one session with one
//! [`Response`]. It is synchronous and deterministic (AD1): the same session state and request
//! give the same response bytes. Requests to a document run one at a time, since `handle` takes
//! `&mut self` (AD4).
//!
//! - **Commands only** (AD2, AINV1). Edits reach a document only through [`DocHost::apply`],
//!   `undo` and `redo`, which forward to `Doc::apply`, `Doc::undo` and `Doc::redo`; reads,
//!   `validate` and `preview` only borrow the document (AINV3).
//! - **Revisions** (AD4, AINV6). `validate`, `preview`, `apply`, `undo` and `redo` are refused
//!   with `stale_revision {expected, actual}` unless `expected_revision` is the document's
//!   revision, before anything else about the edit is looked at. Every response about an open
//!   document carries its `doc_id` and its revision after the request.
//! - **Preview equals commit** (AD3, AINV2). `preview` and `validate` apply the request's one
//!   `Command` to a fork. `apply` does the same first, so the diff is computed and every refusal
//!   is found before the live document is touched (AINV4), and then commits the same command.
//! - **Idempotency** (section 11.2, AK5, AINV7). A keyed request is answered from the session's
//!   cache when its key was seen with the same canonical request, and refused with
//!   `idempotency_conflict` when the key was seen with another. A success or a non-retryable
//!   failure is stored; a retryable failure (`stale_revision`, `user_gesture_in_progress`) is
//!   not, since it applied nothing and the caller is invited to retry. The cache keeps the
//!   newest AK5 entries; the oldest is evicted first.
//! - **Undo and redo** (section 11.4). They need `expected_revision` too, so an agent can only
//!   undo a step it can prove is on top (ATC-edit-06).
//!
//! Errors are in envelope space: a `schema_violation` from the read layer points at
//! `/params/...`. [`in_form`] rewrites a response for the MCP arguments form, where the leading
//! `/params` is dropped (section 9.1).
//!
//! `open`, `save` and `close` enforce the configured [`Policy`] (AD7, AINV9, AINV12): file paths
//! are canonicalised inside the configured [`Roots`], writes are staged through a sibling
//! temporary file, and tier 3 effects (overwriting a file or discarding unsaved changes) require
//! a matching approval echo.

use std::{
    collections::{BTreeMap, VecDeque},
    hash::Hasher,
    path::PathBuf,
    sync::Arc,
};

use omnia_doc::{Doc, prefs::WiringPrefs};
use omnia_symbols::SymbolLibrary;
use rustc_hash::FxHasher;

use crate::{
    AgentError, Catalog, ErrorKind, IDEMPOTENCY_CACHE_ENTRIES, MAX_OPEN_DOCUMENTS, OpName,
    Operation, Output, Request, Response,
    approval::{ApprovalStore, GuardedEffect, Policy},
    diff,
    error::Limit,
    files::{self, Roots},
    host::{DocHandle, DocHost, HeadlessHost},
    ops, preview, read,
    schema::{
        ApplyArgs, ChangeResult, CloseArgs, CloseResult, DocId, DocInfo, EditArgs, EditResult,
        KeyArgs, OpenArgs, PreviewResult, RequestStatus, RevArgs, SaveArgs, SaveResult,
        StoredResponse, ValidateResult,
    },
};

/// A session: one transport connection (section 11.1). Issued by
/// [`AgentService::open_session`] and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);

/// The wire form a request arrived in (section 9.1), which decides how error pointers read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `{op, params, idempotency_key?}`: pointers start at the envelope (`/params/cursor`).
    Envelope,
    /// MCP tool arguments: pointers start at the params (`/cursor`).
    Arguments,
}

/// An open document of a session (section 11.1).
#[derive(Debug, Clone)]
struct DocEntry {
    handle: DocHandle,
    /// Canonical path on disk (from `open` or last `save`), if backed by a file.
    path: Option<PathBuf>,
    /// The revision at open or last save; the document is dirty when its revision differs.
    saved_revision: u64,
}

/// One stored keyed request (section 11.2).
#[derive(Debug, Clone)]
struct CacheEntry {
    key: String,
    /// [`request_hash`] of the canonical request.
    request: u64,
    op: OpName,
    response: Response,
}

/// The idempotency cache of one session: at most AK5 entries, oldest first.
#[derive(Debug, Default)]
struct IdempotencyCache {
    entries: VecDeque<CacheEntry>,
}

impl IdempotencyCache {
    fn get(&self, key: &str) -> Option<&CacheEntry> {
        self.entries.iter().find(|e| e.key == key)
    }

    /// Stores an entry for a key the cache does not hold, evicting the oldest when full.
    fn insert(&mut self, entry: CacheEntry) {
        debug_assert!(self.get(&entry.key).is_none());
        let capacity = usize::try_from(IDEMPOTENCY_CACHE_ENTRIES).unwrap_or(usize::MAX);
        while self.entries.len() >= capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

#[derive(Debug, Default)]
struct Session {
    docs: BTreeMap<DocId, DocEntry>,
    /// The next `doc_id`; ids start at `d1` and are never reused (section 11.1).
    next_doc: u64,
    /// WK10 defaults (section 11.1).
    prefs: WiringPrefs,
    cache: IdempotencyCache,
    approvals: ApprovalStore,
}

/// An open document resolved for one request.
#[derive(Debug, Clone, Copy)]
struct Target {
    doc_id: DocId,
    handle: DocHandle,
    /// The revision when the request started.
    revision: u64,
}

/// Which way a revision-checked history step goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Undo,
    Redo,
}

/// The transport-neutral agent service (AD1).
#[derive(Debug)]
pub struct AgentService<H: DocHost = HeadlessHost> {
    host: H,
    policy: Policy,
    symbols: Arc<SymbolLibrary>,
    catalog: Catalog,
    sessions: BTreeMap<SessionId, Session>,
    next_session: u64,
}

impl<H: DocHost> AgentService<H> {
    /// A service over `host` with `policy` and the built-in symbol library.
    pub fn new(host: H, policy: Policy) -> Self {
        Self::with_symbols(host, policy, SymbolLibrary::builtin())
    }

    /// A service over `host` and `policy` that places and names parts from `symbols` (AD11).
    pub fn with_symbols(host: H, policy: Policy, symbols: Arc<SymbolLibrary>) -> Self {
        Self {
            host,
            policy,
            symbols,
            catalog: Catalog::current(),
            sessions: BTreeMap::new(),
            next_session: 1,
        }
    }

    /// The configured file-root and approval policy (AD7, section 12.1).
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// The capability catalog, which is also the dispatch table (AINV11).
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The symbol library.
    pub fn symbols(&self) -> &Arc<SymbolLibrary> {
        &self.symbols
    }

    /// The document host.
    pub fn host(&self) -> &H {
        &self.host
    }

    /// The document host, for the host's own edits (in app mode, the person's). Those go
    /// through `DocHost` too, and advance the revision every agent request is checked against.
    pub fn host_mut(&mut self) -> &mut H {
        &mut self.host
    }

    /// Starts a session.
    pub fn open_session(&mut self) -> SessionId {
        let id = SessionId(self.next_session);
        self.next_session += 1;
        self.sessions.insert(
            id,
            Session {
                next_doc: 1,
                ..Session::default()
            },
        );
        id
    }

    /// Ends a session and drops its documents from the host. False if there was no such
    /// session.
    pub fn close_session(&mut self, session: SessionId) -> bool {
        let Some(s) = self.sessions.remove(&session) else {
            return false;
        };
        for entry in s.docs.values() {
            // A handle the host no longer holds has nothing left to release.
            let _ = self.host.remove(entry.handle);
        }
        true
    }

    /// The host handle of an open document.
    pub fn doc_handle(&self, session: SessionId, doc_id: DocId) -> Option<DocHandle> {
        self.sessions
            .get(&session)?
            .docs
            .get(&doc_id)
            .map(|e| e.handle)
    }

    /// An open document, read-only.
    pub fn document(&self, session: SessionId, doc_id: DocId) -> Option<&Doc> {
        self.host.doc(self.doc_handle(session, doc_id)?).ok()
    }

    /// Answers one request for `session`, with error pointers in envelope space.
    pub fn handle(&mut self, session: SessionId, req: &Request) -> Response {
        if !self.sessions.contains_key(&session) {
            return failure(
                None,
                None,
                AgentError::with_message(ErrorKind::Internal, "no such session"),
            );
        }
        match req.idempotency_key() {
            None => self.dispatch(session, req.op()),
            Some(key) => self.keyed(session, key, req),
        }
    }

    /// Answers one request that arrived in `form` (see [`in_form`]).
    pub fn handle_in(&mut self, session: SessionId, req: &Request, form: Form) -> Response {
        in_form(self.handle(session, req), form)
    }

    /// Registers `doc` as an open document of `session` (`new` and `open`), refusing a
    /// session that already has AK6 documents open.
    pub(crate) fn adopt(
        &mut self,
        session: SessionId,
        doc: Doc,
        path: Option<PathBuf>,
    ) -> Result<(DocId, u64), AgentError> {
        let revision = doc.revision();
        self.check_open_capacity(session)?;
        let handle = self.host.insert(doc)?;
        let s = self.session_mut(session);
        let doc_id = DocId(s.next_doc);
        s.next_doc += 1;
        s.docs.insert(
            doc_id,
            DocEntry {
                handle,
                path,
                saved_revision: revision,
            },
        );
        Ok((doc_id, revision))
    }

    fn check_open_capacity(&self, session: SessionId) -> Result<(), AgentError> {
        let s = self
            .sessions
            .get(&session)
            .ok_or_else(|| AgentError::with_message(ErrorKind::Internal, "no such session"))?;
        let open = s.docs.len();
        if open >= usize::try_from(MAX_OPEN_DOCUMENTS).unwrap_or(usize::MAX) {
            return Err(AgentError::new(ErrorKind::LimitExceeded {
                limit: Limit::OpenDocuments,
                value: u64::try_from(open).unwrap_or(u64::MAX) + 1,
            }));
        }
        Ok(())
    }

    fn session(&self, session: SessionId) -> &Session {
        self.sessions
            .get(&session)
            .expect("handle checked the session")
    }

    fn session_mut(&mut self, session: SessionId) -> &mut Session {
        self.sessions
            .get_mut(&session)
            .expect("handle checked the session")
    }

    /// Section 11.2: replay, conflict, or run and store.
    fn keyed(&mut self, session: SessionId, key: &str, req: &Request) -> Response {
        let hash = request_hash(req);
        if let Some(entry) = self.session(session).cache.get(key) {
            return if entry.request == hash {
                entry.response.clone()
            } else {
                failure(None, None, AgentError::new(ErrorKind::IdempotencyConflict))
            };
        }
        let response = self.dispatch(session, req.op());
        let store = match response.outcome() {
            Ok(_) => true,
            Err(error) => !error.retryable(),
        };
        if store {
            self.session_mut(session).cache.insert(CacheEntry {
                key: key.to_owned(),
                request: hash,
                op: req.op().name(),
                response: response.clone(),
            });
        }
        response
    }

    /// The dispatch half of the catalog. Exhaustive, with no wildcard arm.
    fn dispatch(&mut self, session: SessionId, op: &Operation) -> Response {
        match op {
            Operation::Capabilities(_) => success(
                None,
                None,
                Output::Capabilities(Box::new(self.catalog.clone())),
            ),
            Operation::New(_) => match self.adopt(session, Doc::new(), None) {
                Ok((doc_id, revision)) => success(
                    Some(doc_id),
                    Some(revision),
                    Output::Document(DocInfo {
                        path: None,
                        dirty: false,
                    }),
                ),
                Err(e) => failure(None, None, e),
            },
            Operation::Open(a) => self.open(session, a),
            Operation::Save(a) => self.on_doc(session, a.doc_id, |svc, t| svc.save(session, t, a)),
            Operation::Close(a) => {
                self.on_doc(session, a.doc_id, |svc, t| svc.close(session, t, a))
            }
            Operation::Inspect(a) => self.on_doc(session, a.doc_id, |svc, t| {
                read::inspect(svc.host.doc(t.handle)?, &svc.symbols, a).map(Output::Inspect)
            }),
            Operation::Query(a) => self.on_doc(session, a.doc_id, |svc, t| {
                read::query(svc.host.doc(t.handle)?, &svc.symbols, a).map(Output::Query)
            }),
            Operation::Nets(a) => self.on_doc(session, a.doc_id, |svc, t| {
                read::nets(svc.host.doc(t.handle)?, a).map(Output::Nets)
            }),
            Operation::Validate(a) => {
                self.on_doc(session, a.doc_id, |svc, t| svc.validate(session, t, a))
            }
            Operation::Preview(a) => {
                self.on_doc(session, a.doc_id, |svc, t| svc.preview(session, t, a))
            }
            Operation::Apply(a) => {
                self.on_doc(session, a.doc_id, |svc, t| svc.apply(session, t, a))
            }
            Operation::Undo(a) => {
                self.on_doc(session, a.doc_id, |svc, t| svc.step(t, a, Step::Undo))
            }
            Operation::Redo(a) => {
                self.on_doc(session, a.doc_id, |svc, t| svc.step(t, a, Step::Redo))
            }
            Operation::RequestStatus(a) => success(
                None,
                None,
                Output::RequestStatus(self.request_status(session, a)),
            ),
            // Not available until A3.1, so the decoder never yields them.
            Operation::JobStatus(_) | Operation::JobCancel(_) => {
                failure(None, None, AgentError::new(ErrorKind::UnknownOperation))
            }
        }
    }

    /// Resolves `doc_id`, runs `f`, and answers with the document's id and its revision after
    /// `f` (the revision it had, if `f` closed it). An unknown `doc_id` is `document_not_found`
    /// with neither.
    fn on_doc(
        &mut self,
        session: SessionId,
        doc_id: DocId,
        f: impl FnOnce(&mut Self, Target) -> Result<Output, AgentError>,
    ) -> Response {
        let target = match self.target(session, doc_id) {
            Ok(t) => t,
            Err(e) => return failure(None, None, e),
        };
        let outcome = f(self, target);
        let revision = self
            .host
            .doc(target.handle)
            .map_or(target.revision, Doc::revision);
        respond(Some(doc_id), Some(revision), outcome)
    }

    fn target(&self, session: SessionId, doc_id: DocId) -> Result<Target, AgentError> {
        let entry = self
            .session(session)
            .docs
            .get(&doc_id)
            .ok_or_else(|| AgentError::new(ErrorKind::DocumentNotFound))?;
        let revision = self.host.doc(entry.handle)?.revision();
        Ok(Target {
            doc_id,
            handle: entry.handle,
            revision,
        })
    }

    /// AD4: refuses a mutation, or a forecast of one, at any other revision.
    fn at_revision(t: Target, expected: u64) -> Result<(), AgentError> {
        if expected == t.revision {
            Ok(())
        } else {
            Err(AgentError::new(ErrorKind::StaleRevision {
                expected,
                actual: t.revision,
            }))
        }
    }

    /// INV5, AINV13: no mutation while a person's gesture is in progress.
    fn no_gesture(&self, t: Target) -> Result<(), AgentError> {
        if self.host.gesture_active(t.handle) {
            Err(AgentError::new(ErrorKind::UserGestureInProgress))
        } else {
            Ok(())
        }
    }

    fn validate(
        &mut self,
        session: SessionId,
        t: Target,
        a: &EditArgs,
    ) -> Result<Output, AgentError> {
        Self::at_revision(t, a.expected_revision)?;
        let cmd = ops::to_command(&a.operations, &self.symbols, &self.session(session).prefs)?;
        let (_, change) = preview::run(self.host.doc(t.handle)?, cmd)?;
        Ok(Output::Validated(ValidateResult {
            base_revision: t.revision,
            change: (&change).into(),
        }))
    }

    fn preview(
        &mut self,
        session: SessionId,
        t: Target,
        a: &EditArgs,
    ) -> Result<Output, AgentError> {
        Self::at_revision(t, a.expected_revision)?;
        let prefs = self.session(session).prefs;
        let cmd = ops::to_command(&a.operations, &self.symbols, &prefs)?;
        let doc = self.host.doc(t.handle)?;
        let (after, change) = preview::run(doc, cmd)?;
        let diff = diff::diff(doc, &after, &change, &self.symbols)?;
        Ok(Output::Preview(PreviewResult {
            preview_id: preview::preview_id(t.doc_id, t.revision, &a.operations, &prefs),
            base_revision: t.revision,
            change: (&change).into(),
            diff,
        }))
    }

    fn apply(
        &mut self,
        session: SessionId,
        t: Target,
        a: &ApplyArgs,
    ) -> Result<Output, AgentError> {
        self.no_gesture(t)?;
        Self::at_revision(t, a.expected_revision)?;
        let prefs = self.session(session).prefs;
        if let Some(id) = a.preview_id
            && id != preview::preview_id(t.doc_id, t.revision, &a.operations, &prefs)
        {
            return Err(AgentError::new(ErrorKind::PreviewMismatch));
        }
        let cmd = ops::to_command(&a.operations, &self.symbols, &prefs)?;
        // The preview first (AD3): every refusal and the diff before the live document moves.
        let doc = self.host.doc(t.handle)?;
        let (after, forecast) = preview::run(doc, cmd.clone())?;
        let diff = diff::diff(doc, &after, &forecast, &self.symbols)?;
        drop(after);
        let change = self.host.apply(t.handle, cmd)??;
        debug_assert_eq!(
            change, forecast,
            "AINV2: the commit returns the previewed change"
        );
        Ok(Output::Applied(EditResult {
            change: (&change).into(),
            diff,
        }))
    }

    fn step(&mut self, t: Target, a: &RevArgs, step: Step) -> Result<Output, AgentError> {
        self.no_gesture(t)?;
        Self::at_revision(t, a.expected_revision)?;
        let change = match step {
            Step::Undo => self
                .host
                .undo(t.handle)?
                .ok_or_else(|| AgentError::new(ErrorKind::NothingToUndo))?,
            Step::Redo => self
                .host
                .redo(t.handle)?
                .ok_or_else(|| AgentError::new(ErrorKind::NothingToRedo))?,
        };
        Ok(Output::Change(ChangeResult {
            change: (&change).into(),
        }))
    }

    fn open(&mut self, session: SessionId, a: &OpenArgs) -> Response {
        let canon = match self.policy.roots().resolve_read(&a.path) {
            Ok(path) => path,
            Err(e) => return failure(None, None, e),
        };
        if let Err(e) = self.check_open_capacity(session) {
            return failure(None, None, e);
        }
        let doc = match files::open_doc(&canon) {
            Ok(doc) => doc,
            Err(e) => return failure(None, None, e),
        };
        let display = files::display_path(&canon);
        match self.adopt(session, doc, Some(canon)) {
            Ok((doc_id, revision)) => success(
                Some(doc_id),
                Some(revision),
                Output::Document(DocInfo {
                    path: Some(display),
                    dirty: false,
                }),
            ),
            Err(e) => failure(None, None, e),
        }
    }

    fn save(&mut self, session: SessionId, t: Target, a: &SaveArgs) -> Result<Output, AgentError> {
        let stored_path = self
            .session(session)
            .docs
            .get(&t.doc_id)
            .ok_or_else(|| AgentError::new(ErrorKind::DocumentNotFound))?
            .path
            .clone();
        let target = match (&a.path, stored_path.as_deref()) {
            (Some(raw), _) => self.policy.roots().resolve_write(raw)?,
            (None, Some(stored)) => self.policy.roots().resolve_write_stored(stored)?,
            (None, None) => {
                return Err(AgentError::with_message(
                    ErrorKind::SchemaViolation {
                        path: "/params/path".into(),
                    },
                    "untitled document requires `path`",
                ));
            }
        };
        let effect = if target.exists {
            GuardedEffect::OverwriteFile {
                doc_id: t.doc_id,
                revision: t.revision,
                path: target.path.clone(),
            }
        } else {
            GuardedEffect::WriteFile {
                doc_id: t.doc_id,
                revision: t.revision,
                path: target.path.clone(),
            }
        };
        let policy = &self.policy;
        self.sessions
            .get_mut(&session)
            .expect("handle checked the session")
            .approvals
            .check(policy, &effect, a.approval.as_ref())?;
        files::save_doc(self.host.doc(t.handle)?, &target.path)?;
        let entry = self
            .session_mut(session)
            .docs
            .get_mut(&t.doc_id)
            .expect("target checked the document");
        entry.path = Some(target.path.clone());
        entry.saved_revision = t.revision;
        Ok(Output::Saved(SaveResult {
            path: files::display_path(&target.path),
            saved_revision: t.revision,
        }))
    }

    fn close(
        &mut self,
        session: SessionId,
        t: Target,
        a: &CloseArgs,
    ) -> Result<Output, AgentError> {
        let saved_revision = self
            .session(session)
            .docs
            .get(&t.doc_id)
            .ok_or_else(|| AgentError::new(ErrorKind::DocumentNotFound))?
            .saved_revision;
        let dirty = t.revision != saved_revision;
        if dirty {
            // Discarding unsaved changes is tier 3 (AD7, AINV9). Both `discard_unsaved: true`
            // and a matching approval are required; without `discard_unsaved`, no echo is
            // accepted so the caller must explicitly opt in on retry.
            let effect = GuardedEffect::DiscardUnsaved {
                doc_id: t.doc_id,
                revision: t.revision,
            };
            let echo = if a.discard_unsaved {
                a.approval.as_ref()
            } else {
                None
            };
            let policy = &self.policy;
            self.sessions
                .get_mut(&session)
                .expect("handle checked the session")
                .approvals
                .check(policy, &effect, echo)?;
        }
        self.host.remove(t.handle)?;
        self.session_mut(session).docs.remove(&t.doc_id);
        Ok(Output::Closed(CloseResult { discarded: dirty }))
    }

    fn request_status(&self, session: SessionId, a: &KeyArgs) -> RequestStatus {
        match self.session(session).cache.get(&a.idempotency_key) {
            None => RequestStatus::NotRun,
            Some(entry) => match entry.response.outcome() {
                Ok(_) => {
                    let revision = entry
                        .response
                        .revision()
                        .expect("a keyed operation's success belongs to a document");
                    RequestStatus::Applied(
                        StoredResponse::new(entry.op, revision, entry.response.clone())
                            .expect("only keyed successes are stored"),
                    )
                }
                Err(error) => RequestStatus::Failed {
                    error: error.clone(),
                },
            },
        }
    }
}

impl AgentService<HeadlessHost> {
    /// A headless service with no configured roots (every file path is `path_outside_root`) and
    /// the built-in symbol library (A1 to A3).
    pub fn headless() -> Self {
        Self::new(HeadlessHost::new(), Policy::default())
    }

    /// A headless service with `roots` under the AQ4 default policy (tiers 1 and 2 allowed;
    /// tier 3 requires an echoed approval digest).
    pub fn headless_with_roots(roots: Roots) -> Self {
        Self::new(HeadlessHost::new(), Policy::headless(roots))
    }
}

/// The idempotency identity of a request: a 64-bit FxHash of its canonical envelope
/// (`Request::to_json`), which both wire forms decode to (section 11.2).
fn request_hash(req: &Request) -> u64 {
    let mut h = FxHasher::default();
    h.write(b"omnia.agent/1 request");
    h.write(req.to_json().as_bytes());
    h.finish()
}

fn respond(
    doc_id: Option<DocId>,
    revision: Option<u64>,
    outcome: Result<Output, AgentError>,
) -> Response {
    Response::new(doc_id, revision, outcome)
        .expect("a document's result is answered with its doc_id and revision")
}

fn success(doc_id: Option<DocId>, revision: Option<u64>, output: Output) -> Response {
    respond(doc_id, revision, Ok(output))
}

fn failure(doc_id: Option<DocId>, revision: Option<u64>, error: AgentError) -> Response {
    respond(doc_id, revision, Err(error))
}

/// The pointer of an envelope-space error in the arguments form: `/params/x` becomes `/x` and
/// `/params` the whole document, `""`.
fn arguments_error(error: &AgentError) -> AgentError {
    let mut error = error.clone();
    if let ErrorKind::SchemaViolation { path } = &mut error.kind {
        if path == "/params" {
            path.clear();
        } else if let Some(rest) = path.strip_prefix("/params/") {
            *path = format!("/{rest}");
        }
    }
    error
}

/// `response` as the answer to a request in `form` (section 9.1): in the arguments form, the
/// pointer of a `schema_violation` drops the envelope's leading `/params`, also inside a
/// `request_status` that reports a failure. Everything else is unchanged.
pub fn in_form(response: Response, form: Form) -> Response {
    if form == Form::Envelope {
        return response;
    }
    let outcome = match response.outcome() {
        Err(error) => Err(arguments_error(error)),
        Ok(Output::RequestStatus(RequestStatus::Failed { error })) => {
            Ok(Output::RequestStatus(RequestStatus::Failed {
                error: arguments_error(error),
            }))
        }
        Ok(_) => return response,
    };
    respond(response.doc_id(), response.revision(), outcome)
        .with_warnings(response.warnings().to_vec())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        path::{Path, PathBuf},
        sync::OnceLock,
    };

    use omnia_doc::{
        Command, ItemId, Label, LabelId, Orient, PIN_LATTICE_NM, PartId, Pt, Rect, TextField,
        WireId,
    };
    use proptest::{collection::vec, prelude::*};
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        ErrorCode,
        schema::{
            Bounds, ChangeDto, EditOp, Include, InspectArgs, ItemRef, NetKey, NetsArgs, NoArgs,
            OrientName, PartRef, PinRef, Point, PreviewId, QueryArgs, QueryMode, TextFieldName,
            WireRef, validate::validate,
        },
    };

    const L: i32 = PIN_LATTICE_NM;
    const RESISTOR: &str = "Resistor-IEEE-Standard";
    const TEXTS: [&str; 3] = ["A", "B", "VCC"];
    const FIELDS: [TextField; 3] = [TextField::Designator, TextField::Value, TextField::Label];
    const CASES: u32 = 64;

    type Service = AgentService<HeadlessHost>;

    fn pt(x: i32, y: i32) -> Pt {
        Pt { x: x * L, y: y * L }
    }

    fn p(x: i32, y: i32) -> Point {
        Point(pt(x, y))
    }

    // ---- responses checked against the committed schemas ----

    /// The committed `<op>.response.json` files (A1.3), read from disk.
    fn committed(op: OpName) -> &'static Value {
        static FILES: OnceLock<BTreeMap<OpName, Value>> = OnceLock::new();
        FILES
            .get_or_init(|| {
                let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
                OpName::ALL
                    .iter()
                    .filter(|&&op| crate::catalog::spec(op).available)
                    .map(|&op| {
                        let path = dir.join(format!("{op}.response.json"));
                        let text = std::fs::read_to_string(&path)
                            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                        (op, serde_json::from_str(&text).unwrap())
                    })
                    .collect()
            })
            .get(&op)
            .unwrap_or_else(|| panic!("no committed schema for {op}"))
    }

    /// Validates a produced response against its committed schema with the test-only
    /// validator, decodes it back, and returns its bytes.
    fn checked(op: OpName, r: &Response) -> String {
        let json = r.to_json();
        let value: Value = serde_json::from_str(&json).unwrap();
        validate(committed(op), &value)
            .unwrap_or_else(|v| panic!("{op} off schema: {v:?}\n{json}"));
        assert_eq!(&Response::from_slice(op, json.as_bytes()).unwrap(), r);
        json
    }

    fn send(svc: &mut Service, s: SessionId, req: &Request) -> Response {
        let r = svc.handle(s, req);
        checked(req.op().name(), &r);
        r
    }

    /// Decodes an envelope written as JSON, as `omnia call` would, and sends it.
    fn call(svc: &mut Service, s: SessionId, request: Value) -> Response {
        let text = request.to_string();
        let req = Request::from_slice(text.as_bytes()).unwrap_or_else(|e| panic!("{text}: {e}"));
        send(svc, s, &req)
    }

    fn result(r: &Response) -> &Output {
        match r.outcome() {
            Ok(o) => o,
            Err(e) => panic!("{e}"),
        }
    }

    fn error(r: &Response) -> &AgentError {
        match r.outcome() {
            Err(e) => e,
            Ok(_) => panic!("expected an error: {}", r.to_json()),
        }
    }

    fn doc(svc: &Service, s: SessionId, d: DocId) -> &Doc {
        svc.document(s, d).unwrap()
    }

    fn new_doc(svc: &mut Service, s: SessionId) -> DocId {
        call(svc, s, json!({"op": "new", "params": {}}))
            .doc_id()
            .unwrap()
    }

    fn edit(d: DocId, expected_revision: u64, operations: &[EditOp]) -> EditArgs {
        EditArgs {
            doc_id: d,
            expected_revision,
            operations: operations.to_vec(),
        }
    }

    fn preview_req(d: DocId, rev: u64, ops: &[EditOp]) -> Request {
        Request::new(Operation::Preview(edit(d, rev, ops)), None).unwrap()
    }

    fn validate_req(d: DocId, rev: u64, ops: &[EditOp]) -> Request {
        Request::new(Operation::Validate(edit(d, rev, ops)), None).unwrap()
    }

    fn apply_req(
        d: DocId,
        rev: u64,
        ops: &[EditOp],
        preview_id: Option<PreviewId>,
        key: Option<&str>,
    ) -> Request {
        let args = ApplyArgs {
            doc_id: d,
            expected_revision: rev,
            operations: ops.to_vec(),
            preview_id,
        };
        Request::new(Operation::Apply(args), key.map(str::to_owned)).unwrap()
    }

    fn step_req(step: Step, d: DocId, rev: u64, key: Option<&str>) -> Request {
        let args = RevArgs {
            doc_id: d,
            expected_revision: rev,
        };
        let op = match step {
            Step::Undo => Operation::Undo(args),
            Step::Redo => Operation::Redo(args),
        };
        Request::new(op, key.map(str::to_owned)).unwrap()
    }

    fn status_req(key: &str) -> Request {
        Request::new(
            Operation::RequestStatus(KeyArgs {
                idempotency_key: key.to_owned(),
            }),
            None,
        )
        .unwrap()
    }

    fn place(symbol: &str, at: Point, orient: Orient, designator: &str) -> EditOp {
        EditOp::PlacePart {
            symbol: symbol.to_owned(),
            at,
            orient: OrientName(orient),
            designator: designator.to_owned(),
            value: "10k".to_owned(),
        }
    }

    fn label_op(x: i32, y: i32, text: &str) -> EditOp {
        EditOp::AddLabel {
            at: p(x, y),
            text: text.to_owned(),
        }
    }

    /// The document and its history, to show a request changed neither (AINV3, AINV4).
    struct Snapshot {
        doc: Doc,
        cursor: u64,
        can_undo: bool,
        can_redo: bool,
    }

    fn snapshot(d: &Doc) -> Snapshot {
        Snapshot {
            doc: d.fork(),
            cursor: d.history_cursor(),
            can_undo: d.can_undo(),
            can_redo: d.can_redo(),
        }
    }

    fn assert_unchanged(d: &Doc, s: &Snapshot) {
        assert!(d == &s.doc, "the document changed");
        assert_eq!(d.history_cursor(), s.cursor, "the history moved");
        assert_eq!((d.can_undo(), d.can_redo()), (s.can_undo, s.can_redo));
    }

    /// Two resistors side by side at revision 1: `part:0` with pins at `[-4, 0]` and `[4, 0]`,
    /// `part:1` with pins at `[8, 0]` and `[16, 0]` (lattice units).
    fn two_resistors() -> (Service, SessionId, DocId) {
        let mut svc = Service::headless();
        let s = svc.open_session();
        let d = new_doc(&mut svc, s);
        let ops = [
            place(RESISTOR, p(0, 0), Orient::IDENTITY, "R1"),
            place(RESISTOR, p(12, 0), Orient::IDENTITY, "R2"),
        ];
        let r = send(&mut svc, s, &apply_req(d, 0, &ops, None, None));
        assert_eq!(r.revision(), Some(1));
        let doc = doc(&svc, s, d);
        assert_eq!(doc.pins(PartId(0)).unwrap(), &[pt(-4, 0), pt(4, 0)]);
        assert_eq!(doc.pins(PartId(1)).unwrap(), &[pt(8, 0), pt(16, 0)]);
        (svc, s, d)
    }

    fn wire_between_resistors() -> EditOp {
        EditOp::AddWires {
            wires: vec![[p(4, 0), p(8, 0)]],
        }
    }

    // ---- ATC-edit-01 to ATC-edit-06 ----

    #[test]
    fn atc_edit_01_preview_apply_with_the_preview_id_then_undo() {
        let (mut svc, s, d) = two_resistors();
        let original = doc(&svc, s, d).fork();
        let wire = [wire_between_resistors()];
        let pv = send(&mut svc, s, &preview_req(d, 1, &wire));
        let Output::Preview(preview) = result(&pv).clone() else {
            panic!("a preview result");
        };
        assert_eq!(pv.revision(), Some(1), "a preview leaves the revision");
        assert_eq!(preview.base_revision, 1);
        assert_eq!(preview.change.revision, 2);
        assert_eq!(preview.change.added, vec![ItemRef(ItemId::Wire(WireId(0)))]);
        assert!(preview.change.nets_changed);
        assert!(doc(&svc, s, d) == &original, "AINV3");

        let ap = send(
            &mut svc,
            s,
            &apply_req(d, 1, &wire, Some(preview.preview_id), Some("k1")),
        );
        let Output::Applied(applied) = result(&ap) else {
            panic!("an apply result");
        };
        assert_eq!(
            applied.change, preview.change,
            "apply returns the previewed change"
        );
        assert_eq!(applied.diff, preview.diff);
        assert_eq!(ap.revision(), Some(2));

        // One net joins the two pins.
        let nets = call(
            &mut svc,
            s,
            json!({"op": "nets", "params": {"doc_id": "d1", "wire": "wire:0"}}),
        );
        let Output::Nets(nets) = result(&nets) else {
            panic!("a nets result");
        };
        assert_eq!(nets.nets.len(), 1);
        let pin = |part, pin| PinRef {
            part: PartRef(part),
            pin,
        };
        assert_eq!(nets.nets[0].pins, vec![pin(0, 1), pin(1, 0)]);
        assert_eq!(nets.nets[0].wires, vec![WireRef(0)]);
        assert_eq!(
            nets.nets[0].key,
            NetKey::Pin {
                part: PartRef(0),
                pin: 1
            }
        );

        let undo = send(&mut svc, s, &step_req(Step::Undo, d, 2, None));
        let Output::Change(undone) = result(&undo) else {
            panic!("an undo result");
        };
        assert_eq!(undone.change.revision, 3);
        assert_eq!(undone.change.removed, preview.change.added);
        assert!(
            doc(&svc, s, d).content_eq(&original),
            "undo restores the content"
        );
    }

    #[test]
    fn atc_edit_02_an_old_expected_revision_is_stale() {
        let (mut svc, s, d) = two_resistors();
        let before = snapshot(doc(&svc, s, d));
        let r = send(
            &mut svc,
            s,
            &apply_req(d, 0, &[wire_between_resistors()], None, None),
        );
        assert_eq!(
            error(&r).kind,
            ErrorKind::StaleRevision {
                expected: 0,
                actual: 1
            }
        );
        assert!(error(&r).retryable());
        assert_eq!((r.doc_id(), r.revision()), (Some(d), Some(1)));
        let json = r.to_json();
        let expected = concat!(
            r#""code":"stale_revision","message":"expected revision 0, document is at 1","#,
            r#""retryable":true,"details":{"expected":0,"actual":1}"#
        );
        assert!(json.contains(expected), "{json}");
        assert_unchanged(doc(&svc, s, d), &before);
    }

    #[test]
    fn atc_edit_03_the_same_key_twice_is_byte_identical_in_both_forms() {
        let (mut svc, s, d) = two_resistors();
        let ops = serde_json::to_value([wire_between_resistors()]).unwrap();
        let envelope = json!({
            "op": "apply",
            "params": {"doc_id": "d1", "expected_revision": 1, "operations": ops},
            "idempotency_key": "wire-1",
        })
        .to_string();
        let arguments = json!({
            "doc_id": "d1", "expected_revision": 1, "operations": ops,
            "idempotency_key": "wire-1",
        })
        .to_string();
        let a = Request::from_slice(envelope.as_bytes()).unwrap();
        let b = Request::from_arguments(OpName::Apply, arguments.as_bytes()).unwrap();
        assert_eq!(a, b, "both forms give the same request");

        let first = send(&mut svc, s, &a);
        assert!(first.outcome().is_ok());
        let after_first = snapshot(doc(&svc, s, d));
        assert_eq!(after_first.doc.revision(), 2);
        let second = svc.handle_in(s, &b, Form::Arguments);
        let third = send(&mut svc, s, &a);
        assert_eq!(second.to_json(), first.to_json(), "byte-identical");
        assert_eq!(third.to_json(), first.to_json());
        assert_unchanged(doc(&svc, s, d), &after_first);
        assert_eq!(doc(&svc, s, d).wires().count(), 1, "applied once");
    }

    #[test]
    fn atc_edit_04_a_missing_item_in_the_third_operation_refuses_the_batch() {
        let (mut svc, s, d) = two_resistors();
        let before = snapshot(doc(&svc, s, d));
        let ops = [
            label_op(0, 6, "A"),
            wire_between_resistors(),
            EditOp::Move {
                items: vec![ItemRef(ItemId::Part(PartId(99)))],
                delta: p(1, 0),
            },
        ];
        let r = send(&mut svc, s, &apply_req(d, 1, &ops, None, None));
        assert_eq!(
            error(&r).kind,
            ErrorKind::MissingItem {
                item: ItemRef(ItemId::Part(PartId(99)))
            }
        );
        assert_unchanged(doc(&svc, s, d), &before);
        // No undo step: the next undo is the placement's.
        let undo = send(&mut svc, s, &step_req(Step::Undo, d, 1, None));
        let Output::Change(c) = result(&undo) else {
            panic!("an undo result");
        };
        assert_eq!(c.change.removed.len(), 2, "the two resistors");
    }

    #[test]
    fn atc_edit_05_a_preview_id_from_other_operations_is_a_mismatch() {
        let (mut svc, s, d) = two_resistors();
        let a = [wire_between_resistors()];
        let b = [label_op(0, 6, "A")];
        let pv = send(&mut svc, s, &preview_req(d, 1, &a));
        let Output::Preview(preview) = result(&pv).clone() else {
            panic!("a preview result");
        };
        let before = snapshot(doc(&svc, s, d));
        let r = send(
            &mut svc,
            s,
            &apply_req(d, 1, &b, Some(preview.preview_id), None),
        );
        assert_eq!(error(&r).code(), ErrorCode::PreviewMismatch);
        assert_unchanged(doc(&svc, s, d), &before);
        // The preview's own operations still apply with it.
        let ok = send(
            &mut svc,
            s,
            &apply_req(d, 1, &a, Some(preview.preview_id), None),
        );
        assert!(ok.outcome().is_ok());
    }

    #[test]
    fn atc_edit_06_a_persons_edit_between_apply_and_undo_makes_the_undo_stale() {
        let (mut svc, s, d) = two_resistors();
        let ap = send(
            &mut svc,
            s,
            &apply_req(d, 1, &[wire_between_resistors()], None, None),
        );
        assert_eq!(ap.revision(), Some(2));
        // The person adds a label through the host, as the app would (A4).
        let h = svc.doc_handle(s, d).unwrap();
        let label = Command::AddLabel(Label {
            at: pt(0, 8),
            text: "P".into(),
        });
        svc.host_mut().apply(h, label).unwrap().unwrap();
        let before = snapshot(doc(&svc, s, d));
        let undo = send(&mut svc, s, &step_req(Step::Undo, d, 2, None));
        assert_eq!(
            error(&undo).kind,
            ErrorKind::StaleRevision {
                expected: 2,
                actual: 3
            }
        );
        assert_unchanged(doc(&svc, s, d), &before);
        assert_eq!(
            doc(&svc, s, d).labels().count(),
            1,
            "the person's edit stays"
        );
    }

    // ---- other service rules ----

    #[test]
    fn request_status_reports_not_run_applied_and_failed() {
        let (mut svc, s, d) = two_resistors();
        let status = |svc: &mut Service, key: &str| {
            let r = send(svc, s, &status_req(key));
            assert_eq!((r.doc_id(), r.revision()), (None, None));
            let Output::RequestStatus(status) = result(&r).clone() else {
                panic!("a status result");
            };
            status
        };
        assert_eq!(status(&mut svc, "k"), RequestStatus::NotRun);

        let applied = send(
            &mut svc,
            s,
            &apply_req(d, 1, &[label_op(0, 6, "A")], None, Some("k")),
        );
        let RequestStatus::Applied(stored) = status(&mut svc, "k") else {
            panic!("applied");
        };
        assert_eq!(stored.op(), OpName::Apply);
        assert_eq!(stored.revision(), 2);
        assert_eq!(stored.response().to_json(), applied.to_json());

        // A non-retryable failure is stored and reported.
        let missing = EditOp::Delete {
            items: vec![ItemRef(ItemId::Label(LabelId(42)))],
        };
        let failed = send(&mut svc, s, &apply_req(d, 2, &[missing], None, Some("m")));
        assert_eq!(error(&failed).code(), ErrorCode::MissingItem);
        assert_eq!(
            status(&mut svc, "m"),
            RequestStatus::Failed {
                error: error(&failed).clone()
            }
        );

        // A retryable failure is not: it applied nothing and invites a retry with the key.
        let stale = send(&mut svc, s, &step_req(Step::Undo, d, 1, Some("u")));
        assert_eq!(error(&stale).code(), ErrorCode::StaleRevision);
        assert_eq!(status(&mut svc, "u"), RequestStatus::NotRun);
        let retried = send(&mut svc, s, &step_req(Step::Undo, d, 2, Some("u")));
        assert!(retried.outcome().is_ok());
        let RequestStatus::Applied(stored) = status(&mut svc, "u") else {
            panic!("applied");
        };
        assert_eq!(stored.op(), OpName::Undo);
        assert_eq!(stored.revision(), 3);
    }

    #[test]
    fn the_same_key_with_another_request_is_a_conflict() {
        let (mut svc, s, d) = two_resistors();
        let first = send(
            &mut svc,
            s,
            &apply_req(d, 1, &[label_op(0, 6, "A")], None, Some("k")),
        );
        assert!(first.outcome().is_ok());
        let before = snapshot(doc(&svc, s, d));
        for other in [
            apply_req(d, 2, &[label_op(0, 6, "A")], None, Some("k")),
            apply_req(d, 1, &[label_op(0, 7, "A")], None, Some("k")),
            step_req(Step::Undo, d, 2, Some("k")),
        ] {
            let r = send(&mut svc, s, &other);
            assert_eq!(error(&r).code(), ErrorCode::IdempotencyConflict);
            assert_eq!((r.doc_id(), r.revision()), (None, None));
            assert_unchanged(doc(&svc, s, d), &before);
        }
        // Keys belong to a session.
        let other = svc.open_session();
        let d2 = new_doc(&mut svc, other);
        let r = send(
            &mut svc,
            other,
            &apply_req(d2, 0, &[label_op(0, 6, "A")], None, Some("k")),
        );
        assert!(r.outcome().is_ok());
    }

    #[test]
    fn the_idempotency_cache_keeps_the_newest_ak5_entries() {
        let mut svc = Service::headless();
        let s = svc.open_session();
        let d = new_doc(&mut svc, s);
        let n = u64::from(IDEMPOTENCY_CACHE_ENTRIES);
        for i in 0..=n {
            let key = format!("k{i}");
            let r = send(
                &mut svc,
                s,
                &apply_req(d, i, &[label_op(0, 0, "A")], None, Some(&key)),
            );
            assert!(r.outcome().is_ok());
        }
        assert_eq!(
            svc.session(s).cache.len(),
            IDEMPOTENCY_CACHE_ENTRIES as usize
        );
        let not_run = send(&mut svc, s, &status_req("k0"));
        assert_eq!(
            result(&not_run),
            &Output::RequestStatus(RequestStatus::NotRun),
            "the oldest entry is evicted first"
        );
        let kept = send(&mut svc, s, &status_req("k1"));
        assert!(matches!(
            result(&kept),
            Output::RequestStatus(RequestStatus::Applied(_))
        ));
        // A blind retry of the evicted request is still safe: its revision is stale.
        let retry = send(
            &mut svc,
            s,
            &apply_req(d, 0, &[label_op(0, 0, "A")], None, Some("k0")),
        );
        assert_eq!(error(&retry).code(), ErrorCode::StaleRevision);
        assert_eq!(doc(&svc, s, d).revision(), n + 1);
    }

    #[test]
    fn new_is_limited_to_ak6_documents_and_doc_ids_are_never_reused() {
        let mut svc = Service::headless();
        let s = svc.open_session();
        let ids: Vec<DocId> = (0..MAX_OPEN_DOCUMENTS)
            .map(|_| new_doc(&mut svc, s))
            .collect();
        assert_eq!(ids, (1..=8).map(DocId).collect::<Vec<_>>());
        let r = call(&mut svc, s, json!({"op": "new", "params": {}}));
        assert_eq!(
            error(&r).kind,
            ErrorKind::LimitExceeded {
                limit: Limit::OpenDocuments,
                value: 9
            }
        );
        let closed = call(
            &mut svc,
            s,
            json!({"op": "close", "params": {"doc_id": "d3"}}),
        );
        assert_eq!(
            result(&closed),
            &Output::Closed(CloseResult { discarded: false })
        );
        assert_eq!(
            (closed.doc_id(), closed.revision()),
            (Some(DocId(3)), Some(0))
        );
        assert_eq!(new_doc(&mut svc, s), DocId(9), "d3 is not reused");
        let gone = call(
            &mut svc,
            s,
            json!({"op": "inspect", "params": {"doc_id": "d3", "include": ["summary"]}}),
        );
        assert_eq!(error(&gone).code(), ErrorCode::DocumentNotFound);
        assert_eq!((gone.doc_id(), gone.revision()), (None, None));
        assert!(svc.close_session(s));
        assert!(
            svc.host().is_empty(),
            "closing a session drops its documents"
        );
        assert!(!svc.close_session(s));
    }

    #[test]
    fn ainv9_close_dirty_document_requires_discard_flag_and_matching_approval() {
        let (mut svc, s, d) = two_resistors();
        let before = snapshot(doc(&svc, s, d));

        // 1. Without discard_unsaved -> ApprovalRequired (tier 3, discard_unsaved); doc stays open.
        let r1 = call(
            &mut svc,
            s,
            json!({"op": "close", "params": {"doc_id": "d1"}}),
        );
        let ErrorKind::ApprovalRequired {
            approval_id,
            tier,
            effect,
            effect_digest,
        } = error(&r1).kind.clone()
        else {
            panic!("expected approval_required: {}", r1.to_json());
        };
        assert_eq!(tier, 3);
        assert_eq!(effect, crate::error::Effect::DiscardUnsaved);
        assert_eq!((r1.doc_id(), r1.revision()), (Some(d), Some(1)));
        assert_unchanged(doc(&svc, s, d), &before);

        // 2. Echoing approval WITHOUT discard_unsaved: true is still refused (AINV9).
        let r2 = call(
            &mut svc,
            s,
            json!({
                "op": "close",
                "params": {
                    "doc_id": "d1",
                    "approval": {
                        "approval_id": approval_id,
                        "effect_digest": effect_digest,
                    }
                }
            }),
        );
        assert_eq!(error(&r2).code(), ErrorCode::ApprovalRequired);
        assert_unchanged(doc(&svc, s, d), &before);

        // 3. discard_unsaved: true without approval -> same ApprovalRequired challenge (AINV5).
        let r3 = call(
            &mut svc,
            s,
            json!({"op": "close", "params": {"doc_id": "d1", "discard_unsaved": true}}),
        );
        assert_eq!(r3.to_json(), r1.to_json());

        // 4. Wrong digest -> refused; doc stays open.
        let r4 = call(
            &mut svc,
            s,
            json!({
                "op": "close",
                "params": {
                    "doc_id": "d1",
                    "discard_unsaved": true,
                    "approval": {
                        "approval_id": approval_id,
                        "effect_digest": "ed-0000000000000000",
                    }
                }
            }),
        );
        assert_eq!(error(&r4).code(), ErrorCode::ApprovalRequired);
        assert_unchanged(doc(&svc, s, d), &before);

        // 5. Editing the document invalidates the old revision's digest (AINV9).
        send(
            &mut svc,
            s,
            &apply_req(d, 1, &[label_op(0, 4, "E")], None, None),
        );
        let stale_echo = call(
            &mut svc,
            s,
            json!({
                "op": "close",
                "params": {
                    "doc_id": "d1",
                    "discard_unsaved": true,
                    "approval": {
                        "approval_id": approval_id,
                        "effect_digest": effect_digest,
                    }
                }
            }),
        );
        let ErrorKind::ApprovalRequired {
            approval_id: new_id,
            effect_digest: new_digest,
            ..
        } = error(&stale_echo).kind.clone()
        else {
            panic!("expected fresh approval_required for revision 2");
        };
        assert_ne!(new_digest, effect_digest);

        // 6. Echoing the current revision's digest with discard_unsaved: true succeeds.
        let closed = call(
            &mut svc,
            s,
            json!({
                "op": "close",
                "params": {
                    "doc_id": "d1",
                    "discard_unsaved": true,
                    "approval": {
                        "approval_id": new_id,
                        "effect_digest": new_digest,
                    }
                }
            }),
        );
        assert_eq!(
            result(&closed),
            &Output::Closed(CloseResult { discarded: true })
        );
        assert_eq!((closed.doc_id(), closed.revision()), (Some(d), Some(2)));
        assert!(svc.document(s, d).is_none());
    }

    #[test]
    fn atc_save_01_and_ainv9_save_new_then_overwrite_with_echoed_digest_and_idempotency() {
        let root_dir =
            std::env::temp_dir().join(format!("omnia-agent-atc-save-01-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root_dir);
        std::fs::create_dir_all(&root_dir).unwrap();

        let roots = Roots::new([&root_dir]).unwrap();
        let mut svc = Service::headless_with_roots(roots);
        let s = svc.open_session();
        let d = new_doc(&mut svc, s);

        // Untitled save without path -> schema_violation at /params/path (and /path in Arguments).
        let no_path = call(
            &mut svc,
            s,
            json!({"op": "save", "params": {"doc_id": "d1"}}),
        );
        assert_eq!(
            error(&no_path).kind,
            ErrorKind::SchemaViolation {
                path: "/params/path".into()
            }
        );
        let no_path_args = in_form(no_path, Form::Arguments);
        assert_eq!(
            error(&no_path_args).kind,
            ErrorKind::SchemaViolation {
                path: "/path".into()
            }
        );

        // Add two resistors and save to a new file (tier 2, allowed headless).
        let ops = [
            place(RESISTOR, p(0, 0), Orient::IDENTITY, "R1"),
            place(RESISTOR, p(12, 0), Orient::IDENTITY, "R2"),
        ];
        send(&mut svc, s, &apply_req(d, 0, &ops, None, None));
        let saved_new = call(
            &mut svc,
            s,
            json!({"op": "save", "params": {"doc_id": "d1", "path": "amp.ron"}}),
        );
        let Output::Saved(info) = result(&saved_new).clone() else {
            panic!("expected SaveResult");
        };
        assert_eq!(info.saved_revision, 1);
        assert_eq!(
            (saved_new.doc_id(), saved_new.revision()),
            (Some(d), Some(1))
        );
        let disk_v1 = std::fs::read(root_dir.join("amp.ron")).unwrap();

        // Now edit the document (revision 2) and overwrite amp.ron using an idempotency key.
        send(
            &mut svc,
            s,
            &apply_req(d, 1, &[wire_between_resistors()], None, None),
        );

        // 1. Overwrite without approval -> approval_required (retryable, so NOT cached under k).
        let req_unapproved = json!({
            "op": "save",
            "params": {"doc_id": "d1"},
            "idempotency_key": "save-v2",
        });
        let r_req = call(&mut svc, s, req_unapproved);
        let ErrorKind::ApprovalRequired {
            approval_id,
            tier,
            effect,
            effect_digest,
        } = error(&r_req).kind.clone()
        else {
            panic!("expected approval_required: {}", r_req.to_json());
        };
        assert_eq!(tier, 3);
        assert_eq!(effect, crate::error::Effect::OverwriteFile);
        assert!(error(&r_req).retryable());
        // Disk file still holds v1 untouched (AINV9).
        assert_eq!(std::fs::read(root_dir.join("amp.ron")).unwrap(), disk_v1);
        // Retryable approval_required was NOT cached under `save-v2`.
        let st = send(&mut svc, s, &status_req("save-v2"));
        assert_eq!(result(&st), &Output::RequestStatus(RequestStatus::NotRun));

        // 2. Retry with matching approval and the same idempotency key -> succeeds & caches.
        let req_approved = json!({
            "op": "save",
            "params": {
                "doc_id": "d1",
                "approval": {
                    "approval_id": approval_id,
                    "effect_digest": effect_digest,
                }
            },
            "idempotency_key": "save-v2",
        });
        let saved_v2 = call(&mut svc, s, req_approved.clone());
        let Output::Saved(info2) = result(&saved_v2).clone() else {
            panic!("expected SaveResult");
        };
        assert_eq!(info2.saved_revision, 2);
        assert_eq!(info2.path, info.path);

        // Replay with same key returns byte-identical response without needing a new grant.
        let replay = call(&mut svc, s, req_approved);
        assert_eq!(replay.to_json(), saved_v2.to_json());

        // Document is now clean, so `close` succeeds without discard_unsaved or approval.
        let closed = call(
            &mut svc,
            s,
            json!({"op": "close", "params": {"doc_id": "d1"}}),
        );
        assert_eq!(
            result(&closed),
            &Output::Closed(CloseResult { discarded: false })
        );

        // Re-open the saved file and verify content, revision and summary match.
        let opened = call(
            &mut svc,
            s,
            json!({"op": "open", "params": {"path": "amp.ron"}}),
        );
        let d2 = opened.doc_id().unwrap();
        assert_eq!(opened.revision(), Some(2));
        let Output::Document(doc_info) = result(&opened) else {
            panic!("expected DocInfo");
        };
        assert!(!doc_info.dirty);
        assert_eq!(doc_info.path.as_deref(), Some(info.path.as_str()));
        let insp = call(
            &mut svc,
            s,
            json!({"op": "inspect", "params": {"doc_id": d2.to_string(), "include": ["summary"]}}),
        );
        let Output::Inspect(ir) = result(&insp) else {
            panic!("expected InspectResult");
        };
        let sum = ir.summary.unwrap();
        assert_eq!((sum.parts, sum.wires, sum.labels), (2, 1, 0));

        let _ = std::fs::remove_dir_all(&root_dir);
    }

    #[test]
    fn atc_path_01_service_refuses_outside_paths_without_reading() {
        let base =
            std::env::temp_dir().join(format!("omnia-agent-atc-path-01-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("root");
        let outside = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let secret = outside.join("x.ron");
        files::save_doc(&Doc::new(), &secret).unwrap();

        let roots = Roots::new([&root]).unwrap();
        let mut svc = Service::headless_with_roots(roots);
        let s = svc.open_session();
        let d = new_doc(&mut svc, s);

        let abs_secret = secret.to_string_lossy().into_owned();
        let mut paths = vec![
            "../x.ron".to_owned(),
            "..\\outside\\x.ron".to_owned(),
            abs_secret,
            "/etc/passwd".to_owned(),
            r"C:\Windows\win.ini".to_owned(),
        ];

        // Symlink/junction escaping root (must always run on both Unix and Windows).
        let link_dir = root.join("escape_dir");
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, &link_dir).is_ok();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, &link_dir).is_ok()
            || std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&link_dir)
                .arg(&outside)
                .output()
                .is_ok_and(|out| out.status.success());
        assert!(linked, "escape link must be created for ATC-path-01");
        paths.push("escape_dir/x.ron".to_owned());
        paths.push("escape_dir\\x.ron".to_owned());

        let link_file = root.join("link.ron");
        #[cfg(unix)]
        let linked_file = std::os::unix::fs::symlink(&secret, &link_file).is_ok();
        #[cfg(windows)]
        let linked_file = std::os::windows::fs::symlink_file(&secret, &link_file).is_ok();
        if linked_file {
            paths.push("link.ron".to_owned());
        }

        for path in paths {
            let open_r = call(&mut svc, s, json!({"op": "open", "params": {"path": path}}));
            assert_eq!(
                error(&open_r).code(),
                ErrorCode::PathOutsideRoot,
                "open {path}"
            );
            assert_eq!((open_r.doc_id(), open_r.revision()), (None, None));

            let save_r = call(
                &mut svc,
                s,
                json!({"op": "save", "params": {"doc_id": d.to_string(), "path": path}}),
            );
            assert_eq!(
                error(&save_r).code(),
                ErrorCode::PathOutsideRoot,
                "save {path}"
            );
            assert_eq!((save_r.doc_id(), save_r.revision()), (Some(d), Some(0)));
        }

        // Default headless() has empty roots: any open/save is path_outside_root.
        let mut no_roots = Service::headless();
        let s2 = no_roots.open_session();
        let r = call(
            &mut no_roots,
            s2,
            json!({"op": "open", "params": {"path": "a.ron"}}),
        );
        assert_eq!(error(&r).code(), ErrorCode::PathOutsideRoot);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn sessions_are_separate_and_an_unknown_session_is_refused() {
        let mut svc = Service::headless();
        let a = svc.open_session();
        let b = svc.open_session();
        let da = new_doc(&mut svc, a);
        let db = new_doc(&mut svc, b);
        assert_eq!((da, db), (DocId(1), DocId(1)));
        send(
            &mut svc,
            a,
            &apply_req(da, 0, &[label_op(0, 0, "A")], None, None),
        );
        assert_eq!(doc(&svc, a, da).labels().count(), 1);
        assert_eq!(doc(&svc, b, db).labels().count(), 0);
        assert!(svc.close_session(b));
        let r = svc.handle(b, &status_req("k"));
        assert_eq!(error(&r).code(), ErrorCode::Internal);
        checked(OpName::RequestStatus, &r);
    }

    #[test]
    fn undo_and_redo_need_a_step_and_the_current_revision() {
        let mut svc = Service::headless();
        let s = svc.open_session();
        let d = new_doc(&mut svc, s);
        let r = send(&mut svc, s, &step_req(Step::Undo, d, 0, None));
        assert_eq!(error(&r).code(), ErrorCode::NothingToUndo);
        let r = send(&mut svc, s, &step_req(Step::Redo, d, 0, None));
        assert_eq!(error(&r).code(), ErrorCode::NothingToRedo);
        send(
            &mut svc,
            s,
            &apply_req(d, 0, &[label_op(0, 0, "A")], None, None),
        );
        let r = send(&mut svc, s, &step_req(Step::Undo, d, 1, None));
        assert!(r.outcome().is_ok());
        let r = send(&mut svc, s, &step_req(Step::Redo, d, 1, None));
        assert_eq!(error(&r).code(), ErrorCode::StaleRevision);
        let r = send(&mut svc, s, &step_req(Step::Redo, d, 2, None));
        let Output::Change(c) = result(&r) else {
            panic!("a redo result");
        };
        assert_eq!(c.change.revision, 3);
        assert_eq!(c.change.added, vec![ItemRef(ItemId::Label(LabelId(0)))]);
    }

    #[test]
    fn unknown_symbols_and_document_rules_are_refused_with_their_codes() {
        let (mut svc, s, d) = two_resistors();
        let before = snapshot(doc(&svc, s, d));
        let cases: [(EditOp, ErrorCode); 4] = [
            (
                place("No-Such-Symbol", p(0, 0), Orient::IDENTITY, "U1"),
                ErrorCode::UnknownSymbol,
            ),
            (
                EditOp::AddWires {
                    wires: vec![[p(0, 0), p(3, 3)]],
                },
                ErrorCode::InvalidWire,
            ),
            (
                EditOp::SetText {
                    item: ItemRef(ItemId::Part(PartId(0))),
                    field: TextFieldName(TextField::Label),
                    text: "x".into(),
                },
                ErrorCode::InvalidTextField,
            ),
            (
                EditOp::AddLabel {
                    at: Point(Pt { x: i32::MAX, y: 0 }),
                    text: "A".into(),
                },
                ErrorCode::OutOfBounds,
            ),
        ];
        for (op, code) in cases {
            let ops = std::slice::from_ref(&op);
            for req in [
                validate_req(d, 1, ops),
                preview_req(d, 1, ops),
                apply_req(d, 1, ops, None, None),
            ] {
                let r = send(&mut svc, s, &req);
                assert_eq!(error(&r).code(), code, "{op:?}");
                assert_eq!((r.doc_id(), r.revision()), (Some(d), Some(1)));
                assert_unchanged(doc(&svc, s, d), &before);
            }
        }
    }

    #[test]
    fn capabilities_answer_the_catalog() {
        let mut svc = Service::headless();
        let s = svc.open_session();
        let r = call(&mut svc, s, json!({"op": "capabilities", "params": {}}));
        assert_eq!(
            result(&r),
            &Output::Capabilities(Box::new(Catalog::current()))
        );
        assert_eq!(svc.catalog(), &Catalog::current());
        assert_eq!((r.doc_id(), r.revision()), (None, None));
    }

    #[test]
    fn read_errors_point_into_the_form_the_request_came_in() {
        let mut svc = Service::headless();
        let s = svc.open_session();
        new_doc(&mut svc, s);
        let params =
            r#"{"doc_id":"d1","area":{"min":[0,0],"max":[1,1]},"mode":"within","cursor":"nope"}"#;
        let envelope =
            Request::from_slice(format!(r#"{{"op":"query","params":{params}}}"#).as_bytes())
                .unwrap();
        let arguments = Request::from_arguments(OpName::Query, params.as_bytes()).unwrap();
        assert_eq!(envelope, arguments);
        let path = |r: &Response| match &error(r).kind {
            ErrorKind::SchemaViolation { path } => path.clone(),
            other => panic!("{other:?}"),
        };
        let e = svc.handle_in(s, &envelope, Form::Envelope);
        let a = svc.handle_in(s, &arguments, Form::Arguments);
        checked(OpName::Query, &e);
        checked(OpName::Query, &a);
        assert_eq!(path(&e), "/params/cursor");
        assert_eq!(path(&a), "/cursor");
        assert_eq!(error(&e).message, error(&a).message);
        // A fault on the params object itself points at the whole arguments.
        let whole = failure(
            Some(DocId(1)),
            Some(0),
            AgentError::new(ErrorKind::SchemaViolation {
                path: "/params".into(),
            }),
        );
        assert_eq!(path(&in_form(whole.clone(), Form::Envelope)), "/params");
        assert_eq!(path(&in_form(whole, Form::Arguments)), "");
        // A success is the same in both forms.
        let ok = Request::from_arguments(OpName::Nets, br#"{"doc_id":"d1"}"#).unwrap();
        assert_eq!(
            svc.handle_in(s, &ok, Form::Arguments).to_json(),
            svc.handle_in(s, &ok, Form::Envelope).to_json()
        );
        // A stored failure's pointer is rewritten inside `request_status` too.
        let stored = AgentError::new(ErrorKind::SchemaViolation {
            path: "/params/limit".into(),
        });
        let wrapped = success(
            None,
            None,
            Output::RequestStatus(RequestStatus::Failed { error: stored }),
        );
        let Ok(Output::RequestStatus(RequestStatus::Failed { error: e })) =
            in_form(wrapped, Form::Arguments).outcome().clone()
        else {
            panic!("a failed status");
        };
        assert_eq!(
            e.kind,
            ErrorKind::SchemaViolation {
                path: "/limit".into()
            }
        );
    }

    #[test]
    fn ainv5_the_same_state_and_request_give_the_same_bytes() {
        let ops = [
            wire_between_resistors(),
            EditOp::Move {
                items: vec![ItemRef(ItemId::Part(PartId(1)))],
                delta: p(0, 2),
            },
        ];
        let run = |preview: &Request| {
            let (mut svc, s, d) = two_resistors();
            let a = send(&mut svc, s, preview).to_json();
            let b = send(&mut svc, s, preview).to_json();
            assert_eq!(a, b);
            let c = send(&mut svc, s, &apply_req(d, 1, &ops, None, None)).to_json();
            (a, c)
        };
        let typed = preview_req(DocId(1), 1, &ops);
        // The same request with every object's keys written in reverse order (AINV5), by hand
        // so the order does not depend on how `serde_json` keeps a map.
        fn reversed(v: &Value) -> String {
            match v {
                Value::Object(m) => {
                    let fields: Vec<String> = m
                        .iter()
                        .rev()
                        .map(|(k, v)| format!("{}:{}", Value::from(k.as_str()), reversed(v)))
                        .collect();
                    format!("{{{}}}", fields.join(","))
                }
                Value::Array(a) => {
                    let items: Vec<String> = a.iter().map(reversed).collect();
                    format!("[{}]", items.join(","))
                }
                other => other.to_string(),
            }
        }
        let text = reversed(&serde_json::from_str(&typed.to_json()).unwrap());
        assert_ne!(text, typed.to_json());
        let decoded = Request::from_slice(text.as_bytes()).unwrap();
        assert_eq!(run(&typed), run(&decoded));
    }

    // ---- AINV1: commands only ----

    fn rust_files(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
        out.sort();
        out
    }

    /// The text of `line` before any `//` comment.
    fn code(line: &str) -> &str {
        line.split("//").next().unwrap_or_default()
    }

    /// One function signature found by [`signatures`].
    struct Signature {
        name: String,
        /// Plainly `pub`, so other crates can call it.
        public: bool,
        /// From `fn` to the first `{` or `;`.
        text: String,
        /// Inside a top-level `impl Doc {` block.
        on_doc: bool,
    }

    fn signatures(source: &str) -> Vec<Signature> {
        let lines: Vec<&str> = source.lines().collect();
        let mut out = Vec::new();
        let mut on_doc = false;
        for (i, line) in lines.iter().enumerate() {
            if line.starts_with("impl") {
                on_doc = line.starts_with("impl Doc {");
            } else if line.starts_with('}') {
                on_doc = false;
            }
            let t = code(line).trim_start();
            let Some(at) = t.find("fn ") else { continue };
            let head = &t[..at];
            let public = matches!(head, "pub " | "pub const ");
            if !public && !matches!(head, "" | "const " | "pub(crate) " | "pub(super) ") {
                continue;
            }
            let name: String = t[at + 3..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let mut text = String::new();
            for l in &lines[i..] {
                let l = code(l);
                text.push_str(l);
                if l.contains('{') || l.contains(';') {
                    break;
                }
            }
            out.push(Signature {
                name,
                public,
                text,
                on_doc,
            });
        }
        out
    }

    #[test]
    fn ainv1_omnia_agent_calls_no_doc_mutator_but_apply_undo_and_redo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut mutators = BTreeSet::new();
        let mut public = BTreeSet::new();
        for file in rust_files(&root.join("../omnia-doc/src")) {
            let source = std::fs::read_to_string(&file).unwrap();
            for sig in signatures(&source) {
                if sig.on_doc && sig.text.contains("&mut self") {
                    if sig.public {
                        public.insert(sig.name.clone());
                    }
                    mutators.insert(sig.name);
                } else {
                    assert!(
                        !(sig.public && sig.text.contains("&mut Doc")),
                        "{}: public `{}` takes `&mut Doc`",
                        file.display(),
                        sig.name
                    );
                }
            }
        }
        // The only public mutators, so another crate can reach no other (INV4, AD2).
        let allowed = ["apply", "redo", "undo"].map(str::to_owned);
        assert_eq!(public, BTreeSet::from(allowed.clone()));
        // The scan reads the impl blocks: it sees the crate-private mutators too.
        for name in [
            "bump_revision",
            "insert_part_raw",
            "remove_wire",
            "set_text_raw",
        ] {
            assert!(mutators.contains(name), "{name} not found: {mutators:?}");
        }
        let forbidden: Vec<&String> = mutators.iter().filter(|m| !allowed.contains(*m)).collect();
        let files = rust_files(&root.join("src"));
        assert!(files.len() >= 10, "the scan reads the crate's sources");
        for file in files {
            let source = std::fs::read_to_string(&file).unwrap();
            for (n, line) in source.lines().enumerate() {
                let line = code(line);
                for name in &forbidden {
                    for call in [format!(".{name}("), format!("::{name}(")] {
                        assert!(
                            !line.contains(&call),
                            "{}:{}: calls the Doc mutator `{name}`",
                            file.display(),
                            n + 1
                        );
                    }
                }
            }
        }
    }

    // ---- random sheets and random batches ----

    #[derive(Debug, Clone)]
    enum Seed {
        Part {
            sym: usize,
            x: i32,
            y: i32,
            orient: usize,
        },
        Wire {
            x: i32,
            y: i32,
            len: i32,
            horizontal: bool,
        },
        Label {
            x: i32,
            y: i32,
            text: usize,
        },
    }

    fn coord() -> std::ops::RangeInclusive<i32> {
        -12..=12
    }

    fn seed() -> impl Strategy<Value = Seed> {
        prop_oneof![
            (0usize..64, coord(), coord(), 0usize..8).prop_map(|(sym, x, y, orient)| Seed::Part {
                sym,
                x,
                y,
                orient
            }),
            (coord(), coord(), 1i32..=5, any::<bool>(), any::<bool>()).prop_map(
                |(x, y, len, neg, horizontal)| Seed::Wire {
                    x,
                    y,
                    len: if neg { -len } else { len },
                    horizontal,
                }
            ),
            (coord(), coord(), 0usize..3).prop_map(|(x, y, text)| Seed::Label { x, y, text }),
        ]
    }

    fn symbol_name(symbols: &SymbolLibrary, sym: usize) -> String {
        symbols
            .iter()
            .nth(sym % symbols.len())
            .unwrap()
            .name
            .to_owned()
    }

    fn wire_op(x: i32, y: i32, len: i32, horizontal: bool) -> EditOp {
        let b = if horizontal {
            p(x + len, y)
        } else {
            p(x, y + len)
        };
        EditOp::AddWires {
            wires: vec![[p(x, y), b]],
        }
    }

    fn seed_op(seed: &Seed, symbols: &SymbolLibrary, n: usize) -> EditOp {
        match *seed {
            Seed::Part { sym, x, y, orient } => place(
                &symbol_name(symbols, sym),
                p(x, y),
                Orient::ALL[orient],
                &format!("U{n}"),
            ),
            Seed::Wire {
                x,
                y,
                len,
                horizontal,
            } => wire_op(x, y, len, horizontal),
            Seed::Label { x, y, text } => label_op(x, y, TEXTS[text]),
        }
    }

    /// A random sheet built through the service, one apply per seed, so it has a history.
    fn sheet(seeds: &[Seed]) -> (Service, SessionId, DocId) {
        let mut svc = Service::headless();
        let s = svc.open_session();
        let d = new_doc(&mut svc, s);
        let symbols = svc.symbols().clone();
        for (n, seed) in seeds.iter().enumerate() {
            let rev = doc(&svc, s, d).revision();
            let op = seed_op(seed, &symbols, n);
            send(&mut svc, s, &apply_req(d, rev, &[op], None, None));
        }
        (svc, s, d)
    }

    /// One random edit operation, resolved against the document by [`materialize`].
    #[derive(Debug, Clone)]
    enum Raw {
        Place {
            sym: usize,
            x: i32,
            y: i32,
            orient: usize,
            unknown: bool,
        },
        Wires {
            x: i32,
            y: i32,
            len: i32,
            horizontal: bool,
        },
        Label {
            x: i32,
            y: i32,
            text: usize,
        },
        Move {
            picks: Vec<u32>,
            dx: i32,
            dy: i32,
            off_lattice: bool,
        },
        Reorient {
            picks: Vec<u32>,
            px: i32,
            py: i32,
            orient: usize,
        },
        Delete {
            picks: Vec<u32>,
        },
        SetText {
            pick: u32,
            field: usize,
            text: usize,
            /// Any field, often one the item does not have (`invalid_text_field`).
            wrong: bool,
        },
    }

    fn raw() -> impl Strategy<Value = Raw> {
        let picks = || vec(any::<u32>(), 1..4);
        prop_oneof![
            (
                0usize..64,
                coord(),
                coord(),
                0usize..8,
                prop::bool::weighted(0.15)
            )
                .prop_map(|(sym, x, y, orient, unknown)| Raw::Place {
                    sym,
                    x,
                    y,
                    orient,
                    unknown,
                }),
            (coord(), coord(), -5i32..=5, any::<bool>()).prop_map(|(x, y, len, horizontal)| {
                Raw::Wires {
                    x,
                    y,
                    len,
                    horizontal,
                }
            }),
            (coord(), coord(), 0usize..3).prop_map(|(x, y, text)| Raw::Label { x, y, text }),
            (picks(), -4i32..=4, -4i32..=4, prop::bool::weighted(0.15)).prop_map(
                |(picks, dx, dy, off_lattice)| Raw::Move {
                    picks,
                    dx,
                    dy,
                    off_lattice,
                }
            ),
            (picks(), coord(), coord(), 0usize..8).prop_map(|(picks, px, py, orient)| {
                Raw::Reorient {
                    picks,
                    px,
                    py,
                    orient,
                }
            }),
            picks().prop_map(|picks| Raw::Delete { picks }),
            (
                any::<u32>(),
                0usize..3,
                0usize..3,
                prop::bool::weighted(0.15)
            )
                .prop_map(|(pick, field, text, wrong)| Raw::SetText {
                    pick,
                    field,
                    text,
                    wrong,
                }),
        ]
    }

    fn live(doc: &Doc) -> Vec<ItemRef> {
        doc.parts()
            .map(|(id, _)| ItemRef(ItemId::Part(id)))
            .chain(doc.wires().map(|(id, _)| ItemRef(ItemId::Wire(id))))
            .chain(doc.labels().map(|(id, _)| ItemRef(ItemId::Label(id))))
            .collect()
    }

    /// A live item, or now and then an id no item has (`missing_item`).
    fn pick(items: &[ItemRef], k: u32) -> ItemRef {
        if items.is_empty() || k % 16 == 15 {
            let n = 1_000 + k % 5;
            ItemRef(match k % 3 {
                0 => ItemId::Part(PartId(n)),
                1 => ItemId::Wire(WireId(n)),
                _ => ItemId::Label(LabelId(n)),
            })
        } else {
            items[k as usize % items.len()]
        }
    }

    /// The picked items without repeats (the schema's `uniqueItems`).
    fn picked(items: &[ItemRef], picks: &[u32]) -> Vec<ItemRef> {
        let mut out: Vec<ItemRef> = Vec::new();
        for &k in picks {
            let id = pick(items, k);
            if !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    fn materialize(raws: &[Raw], doc: &Doc, symbols: &SymbolLibrary) -> Vec<EditOp> {
        let items = live(doc);
        raws.iter()
            .enumerate()
            .map(|(n, raw)| match raw {
                &Raw::Place {
                    sym,
                    x,
                    y,
                    orient,
                    unknown,
                } => {
                    let name = if unknown {
                        "No-Such-Symbol".to_owned()
                    } else {
                        symbol_name(symbols, sym)
                    };
                    place(&name, p(x, y), Orient::ALL[orient], &format!("V{n}"))
                }
                &Raw::Wires {
                    x,
                    y,
                    len,
                    horizontal,
                } => wire_op(x, y, len, horizontal),
                &Raw::Label { x, y, text } => label_op(x, y, TEXTS[text]),
                Raw::Move {
                    picks,
                    dx,
                    dy,
                    off_lattice,
                } => EditOp::Move {
                    items: picked(&items, picks),
                    delta: Point(Pt {
                        x: dx * L + if *off_lattice { L / 2 } else { 0 },
                        y: dy * L,
                    }),
                },
                Raw::Reorient {
                    picks,
                    px,
                    py,
                    orient,
                } => EditOp::Reorient {
                    items: picked(&items, picks),
                    pivot: p(*px, *py),
                    by: OrientName(Orient::ALL[*orient]),
                },
                Raw::Delete { picks } => EditOp::Delete {
                    items: picked(&items, picks),
                },
                &Raw::SetText {
                    pick: k,
                    field,
                    text,
                    wrong,
                } => {
                    let item = pick(&items, k);
                    let field = match item.0 {
                        _ if wrong => FIELDS[field],
                        ItemId::Part(_) => FIELDS[field % 2],
                        ItemId::Label(_) => TextField::Label,
                        ItemId::Wire(_) => FIELDS[field],
                    };
                    EditOp::SetText {
                        item,
                        field: TextFieldName(field),
                        text: TEXTS[text].to_owned(),
                    }
                }
            })
            .collect()
    }

    fn area(a: (i32, i32, i32, i32)) -> Bounds {
        let (x0, x1) = (a.0.min(a.2), a.0.max(a.2));
        let (y0, y1) = (a.1.min(a.3), a.1.max(a.3));
        Bounds::new(Rect::new(pt(x0, y0), pt(x1, y1)).unwrap()).unwrap()
    }

    /// A mutating request with an `expected_revision` off by `offset` from the current one.
    #[derive(Debug, Clone)]
    enum Mutation {
        Apply {
            raws: Vec<Raw>,
            offset: i64,
            /// 0: no `preview_id`; 1: the matching one; 2: a wrong one.
            preview: u8,
        },
        Undo {
            offset: i64,
        },
        Redo {
            offset: i64,
        },
    }

    fn offset() -> impl Strategy<Value = i64> {
        prop_oneof![3 => Just(0i64), 1 => -3i64..=3]
    }

    fn mutation() -> impl Strategy<Value = Mutation> {
        prop_oneof![
            3 => (vec(raw(), 1..4), offset(), 0u8..3).prop_map(|(raws, offset, preview)| {
                Mutation::Apply { raws, offset, preview }
            }),
            1 => offset().prop_map(|offset| Mutation::Undo { offset }),
            1 => offset().prop_map(|offset| Mutation::Redo { offset }),
        ]
    }

    /// The request for `m` against the document's current state, and its `expected_revision`.
    fn mutation_req(
        svc: &Service,
        s: SessionId,
        d: DocId,
        m: &Mutation,
        key: Option<&str>,
    ) -> (Request, u64) {
        let doc = doc(svc, s, d);
        let shift = |offset: i64| u64::try_from(doc.revision() as i64 + offset).unwrap_or(0);
        match m {
            Mutation::Apply {
                raws,
                offset,
                preview,
            } => {
                let ops = materialize(raws, doc, svc.symbols());
                let expected = shift(*offset);
                let id = preview::preview_id(d, expected, &ops, &WiringPrefs::default());
                let preview_id = match preview {
                    0 => None,
                    1 => Some(id),
                    _ => Some(PreviewId(id.0 ^ 1)),
                };
                (apply_req(d, expected, &ops, preview_id, key), expected)
            }
            Mutation::Undo { offset } => {
                let expected = shift(*offset);
                (step_req(Step::Undo, d, expected, key), expected)
            }
            Mutation::Redo { offset } => {
                let expected = shift(*offset);
                (step_req(Step::Redo, d, expected, key), expected)
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(CASES))]

        /// AINV2: the preview's change and after-state are what `apply` at the same revision
        /// returns and leaves; `validate` reports the same change, and every refusal is the
        /// same refusal. The dirty-bounded diff equals a comparison of every item.
        #[test]
        fn ainv2_preview_equals_commit(seeds in vec(seed(), 0..10), raws in vec(raw(), 1..4)) {
            let (mut svc, s, d) = sheet(&seeds);
            let symbols = svc.symbols().clone();
            let ops = materialize(&raws, doc(&svc, s, d), &symbols);
            let r = doc(&svc, s, d).revision();
            let before = snapshot(doc(&svc, s, d));
            let pv = send(&mut svc, s, &preview_req(d, r, &ops));
            let va = send(&mut svc, s, &validate_req(d, r, &ops));
            assert_unchanged(doc(&svc, s, d), &before);
            // The reference: the commit's code on an independent fork.
            let reference = ops::to_command(&ops, &symbols, &WiringPrefs::default())
                .and_then(|cmd| preview::run(doc(&svc, s, d), cmd).map_err(AgentError::from));
            match (pv.outcome().clone(), reference) {
                (Ok(Output::Preview(preview)), Ok((fork, change))) => {
                    prop_assert_eq!(preview.base_revision, r);
                    prop_assert_eq!(&preview.change, &ChangeDto::from(&change));
                    let brute = diff::brute_force(doc(&svc, s, d), &fork, &symbols).unwrap();
                    prop_assert_eq!(&preview.diff, &brute);
                    let validated = Output::Validated(ValidateResult {
                        base_revision: r,
                        change: preview.change.clone(),
                    });
                    prop_assert_eq!(result(&va), &validated);
                    let req = apply_req(d, r, &ops, Some(preview.preview_id), None);
                    let ap = send(&mut svc, s, &req);
                    let Output::Applied(applied) = result(&ap) else {
                        panic!("an apply result")
                    };
                    prop_assert_eq!(&applied.change, &preview.change);
                    prop_assert_eq!(&applied.diff, &preview.diff);
                    prop_assert!(doc(&svc, s, d) == &fork, "the after-state is the fork's");
                    prop_assert!(doc(&svc, s, d).content_eq(&fork));
                    prop_assert_eq!(ap.revision(), Some(preview.change.revision));
                }
                (Err(refused), Err(expected)) => {
                    prop_assert_eq!(&refused, &expected);
                    prop_assert_eq!(error(&va), &expected);
                    let ap = send(&mut svc, s, &apply_req(d, r, &ops, None, None));
                    prop_assert_eq!(error(&ap), &expected);
                    assert_unchanged(doc(&svc, s, d), &before);
                }
                (outcome, reference) => {
                    panic!("preview {outcome:?} but the commit gives {reference:?}")
                }
            }
        }

        /// AINV3: reads, `validate` and `preview` leave the document `==` and its history
        /// where it was, so undo and redo afterwards answer as on a twin that saw none of them.
        #[test]
        fn ainv3_reads_leave_the_document_and_its_history_unchanged(
            seeds in vec(seed(), 1..10),
            raws in vec(raw(), 1..4),
            undone in 0usize..3,
            rect in (-14i32..=14, -14i32..=14, -14i32..=14, -14i32..=14),
            mode in 0usize..3,
            at in (coord(), coord()),
            wire in 0u32..12,
            limit in prop::option::of(1u32..8),
        ) {
            let (mut a, sa, da) = sheet(&seeds);
            let (mut b, sb, db) = sheet(&seeds);
            for _ in 0..undone {
                for (svc, s, d) in [(&mut a, sa, da), (&mut b, sb, db)] {
                    let rev = doc(svc, s, d).revision();
                    send(svc, s, &step_req(Step::Undo, d, rev, None));
                }
            }
            let ops = materialize(&raws, doc(&a, sa, da), a.symbols());
            let r = doc(&a, sa, da).revision();
            let include = vec![Include::Summary, Include::Items, Include::Nets];
            let mode = [QueryMode::Envelope, QueryMode::Within, QueryMode::Touching][mode];
            let reads = [
                Operation::Capabilities(NoArgs {}),
                Operation::Inspect(InspectArgs {
                    doc_id: da,
                    include,
                    area: None,
                    cursor: None,
                    limit,
                }),
                Operation::Query(QueryArgs {
                    doc_id: da,
                    area: area(rect),
                    mode,
                    cursor: None,
                    limit,
                }),
                Operation::Nets(NetsArgs { doc_id: da, at: None, wire: None }),
                Operation::Nets(NetsArgs { doc_id: da, at: Some(p(at.0, at.1)), wire: None }),
                Operation::Nets(NetsArgs { doc_id: da, at: None, wire: Some(WireRef(wire)) }),
                Operation::Validate(edit(da, r, &ops)),
                Operation::Preview(edit(da, r, &ops)),
                Operation::Preview(edit(da, r + 1, &ops)),
                Operation::RequestStatus(KeyArgs { idempotency_key: "k".into() }),
            ];
            for op in reads {
                send(&mut a, sa, &Request::new(op, None).unwrap());
            }
            let (x, y) = (doc(&a, sa, da), doc(&b, sb, db));
            prop_assert!(x == y);
            prop_assert_eq!(x.history_cursor(), y.history_cursor());
            prop_assert_eq!((x.can_undo(), x.can_redo()), (y.can_undo(), y.can_redo()));
            for step in [Step::Undo, Step::Undo, Step::Redo, Step::Redo, Step::Redo] {
                let (ra, rb) = (doc(&a, sa, da).revision(), doc(&b, sb, db).revision());
                let ra = send(&mut a, sa, &step_req(step, da, ra, None));
                let rb = send(&mut b, sb, &step_req(step, db, rb, None));
                prop_assert_eq!(ra.to_json(), rb.to_json());
                prop_assert!(doc(&a, sa, da) == doc(&b, sb, db));
            }
        }

        /// AINV4: every refused `apply`, `undo` or `redo` leaves the document `==` and its
        /// history where it was, and answers with the unchanged revision.
        #[test]
        fn ainv4_a_refused_request_changes_nothing(
            seeds in vec(seed(), 0..10),
            mutations in vec(mutation(), 1..8),
        ) {
            let (mut svc, s, d) = sheet(&seeds);
            for m in &mutations {
                let (req, _) = mutation_req(&svc, s, d, m, None);
                let before = snapshot(doc(&svc, s, d));
                let r = send(&mut svc, s, &req);
                if r.outcome().is_err() {
                    assert_unchanged(doc(&svc, s, d), &before);
                    prop_assert_eq!(r.revision(), Some(before.doc.revision()));
                }
            }
        }

        /// AINV6: no mutation without `expected_revision` equal to the current revision; any
        /// other is `stale_revision {expected, actual}`.
        #[test]
        fn ainv6_no_mutation_without_the_current_revision(
            seeds in vec(seed(), 0..10),
            mutations in vec(mutation(), 1..8),
        ) {
            let (mut svc, s, d) = sheet(&seeds);
            for m in &mutations {
                let (req, expected) = mutation_req(&svc, s, d, m, None);
                let before = snapshot(doc(&svc, s, d));
                let actual = before.doc.revision();
                let r = send(&mut svc, s, &req);
                let after = doc(&svc, s, d);
                let moved = after != &before.doc || after.history_cursor() != before.cursor;
                if moved {
                    prop_assert_eq!(expected, actual, "a mutation at another revision");
                }
                if expected != actual {
                    let stale = ErrorKind::StaleRevision { expected, actual };
                    prop_assert_eq!(&error(&r).kind, &stale);
                    prop_assert!(!moved);
                }
            }
        }

        /// AINV7: replaying a key with the same request returns identical bytes and applies
        /// nothing more; the key with another request is `idempotency_conflict`; and
        /// `request_status` reports what the cache holds.
        #[test]
        fn ainv7_replaying_a_key_returns_identical_bytes_and_applies_at_most_once(
            seeds in vec(seed(), 0..8),
            steps in vec((0usize..3, prop::option::of(0usize..6), mutation()), 1..12),
        ) {
            let (mut svc, s, d) = sheet(&seeds);
            let mut sent: Vec<Request> = Vec::new();
            let mut model: BTreeMap<String, (String, String, OpName)> = BTreeMap::new();
            for (key, repeat, m) in &steps {
                let req = match repeat.and_then(|i| sent.get(i)) {
                    Some(earlier) => earlier.clone(),
                    None => mutation_req(&svc, s, d, m, Some(&format!("key-{key}"))).0,
                };
                let key = req.idempotency_key().unwrap().to_owned();
                let before = snapshot(doc(&svc, s, d));
                let r = send(&mut svc, s, &req);
                match model.get(&key) {
                    Some((request, response, _)) if *request == req.to_json() => {
                        prop_assert_eq!(&r.to_json(), response, "a replay is byte-identical");
                        assert_unchanged(doc(&svc, s, d), &before);
                    }
                    Some(_) => {
                        prop_assert_eq!(error(&r).code(), ErrorCode::IdempotencyConflict);
                        assert_unchanged(doc(&svc, s, d), &before);
                    }
                    None => {
                        let stored = r.outcome().as_ref().map_or_else(|e| !e.retryable(), |_| true);
                        if stored {
                            model.insert(key.clone(), (req.to_json(), r.to_json(), req.op().name()));
                        }
                    }
                }
                let status = send(&mut svc, s, &status_req(&key));
                let Output::RequestStatus(status) = result(&status) else {
                    panic!("a status")
                };
                match (model.get(&key), status) {
                    (None, RequestStatus::NotRun) => {}
                    (Some((_, response, _)), RequestStatus::Applied(stored)) => {
                        prop_assert_eq!(&stored.response().to_json(), response);
                    }
                    (Some((_, response, op)), RequestStatus::Failed { error }) => {
                        let stored = Response::from_slice(*op, response.as_bytes()).unwrap();
                        prop_assert_eq!(stored.outcome().as_ref().unwrap_err(), error);
                    }
                    (model, status) => panic!("model {model:?}, status {status:?}"),
                }
                sent.push(req);
            }
        }
    }
}
