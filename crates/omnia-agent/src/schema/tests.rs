//! A1.3 done-when tests: every DTO round-trips through JSON, every serialized DTO validates
//! against its published JSON Schema, the committed schemas equal the generated ones (AINV11),
//! and the decoders and the published schemas refuse the same inputs.
//!
//! Strategies pick variants through exhaustive `match`es over the catalog's name lists, so a new
//! operation, edit operation, result or error code fails to compile here until it has a
//! generator. Schema checks read the published text ([`schema_files`]), never a codec.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::OnceLock,
};

use omnia_doc::{ItemId, LabelId, Orient, PartId, Pt, Rect, TextField, WireId};
use proptest::{
    collection::vec,
    option,
    prelude::*,
    sample::{Index, select},
};
use serde_json::{Value, json};

use super::{
    validate::{KEYWORDS, Pattern, validate},
    *,
};
use crate::{
    AgentError, Catalog, EditOpName, ErrorCode, ErrorKind, OpName,
    catalog::spec,
    error::{BlockerDto, Effect, Limit},
};

// ---- strategies ----------------------------------------------------------------------------

fn text() -> impl Strategy<Value = String> {
    vec(any::<char>(), 0..6).prop_map(String::from_iter)
}

fn point() -> impl Strategy<Value = Point> {
    (any::<i32>(), any::<i32>()).prop_map(|(x, y)| Point(Pt { x, y }))
}

fn k1() -> impl Strategy<Value = i32> {
    -omnia_doc::SHEET_LIMIT_NM..=omnia_doc::SHEET_LIMIT_NM
}

fn bounds() -> impl Strategy<Value = Bounds> {
    (k1(), k1(), k1(), k1()).prop_map(|(x0, y0, x1, y1)| {
        let min = Pt {
            x: x0.min(x1),
            y: y0.min(y1),
        };
        let max = Pt {
            x: x0.max(x1),
            y: y0.max(y1),
        };
        Bounds(Rect::new(min, max).expect("ordered corners inside K1"))
    })
}

fn orient() -> impl Strategy<Value = OrientName> {
    select(Orient::ALL.to_vec()).prop_map(OrientName)
}

fn text_field() -> impl Strategy<Value = TextFieldName> {
    select(vec![
        TextField::Designator,
        TextField::Value,
        TextField::Label,
    ])
    .prop_map(TextFieldName)
}

fn part_ref() -> impl Strategy<Value = PartRef> {
    any::<u32>().prop_map(PartRef)
}

fn wire_ref() -> impl Strategy<Value = WireRef> {
    any::<u32>().prop_map(WireRef)
}

fn label_ref() -> impl Strategy<Value = LabelRef> {
    any::<u32>().prop_map(LabelRef)
}

fn item_ref() -> impl Strategy<Value = ItemRef> {
    prop_oneof![
        any::<u32>().prop_map(|i| ItemRef(ItemId::Part(PartId(i)))),
        any::<u32>().prop_map(|i| ItemRef(ItemId::Wire(WireId(i)))),
        any::<u32>().prop_map(|i| ItemRef(ItemId::Label(LabelId(i)))),
    ]
}

fn doc_id() -> impl Strategy<Value = DocId> {
    any::<u64>().prop_map(DocId)
}

fn approval_echo() -> impl Strategy<Value = ApprovalEcho> {
    (text(), text()).prop_map(|(approval_id, effect_digest)| ApprovalEcho {
        approval_id,
        effect_digest,
    })
}

fn page_limit() -> impl Strategy<Value = Option<u32>> {
    option::of(1..=crate::MAX_PAGE_SIZE)
}

fn edit_op_for(name: EditOpName) -> BoxedStrategy<EditOp> {
    match name {
        EditOpName::PlacePart => (text(), point(), orient(), text(), text())
            .prop_map(
                |(symbol, at, orient, designator, value)| EditOp::PlacePart {
                    symbol,
                    at,
                    orient,
                    designator,
                    value,
                },
            )
            .boxed(),
        EditOpName::AddWires => vec((point(), point()).prop_map(|(a, b)| [a, b]), 0..3)
            .prop_map(|wires| EditOp::AddWires { wires })
            .boxed(),
        EditOpName::AddLabel => (point(), text())
            .prop_map(|(at, text)| EditOp::AddLabel { at, text })
            .boxed(),
        EditOpName::Move => (vec(item_ref(), 0..3), point())
            .prop_map(|(items, delta)| EditOp::Move { items, delta })
            .boxed(),
        EditOpName::Reorient => (vec(item_ref(), 0..3), point(), orient())
            .prop_map(|(items, pivot, by)| EditOp::Reorient { items, pivot, by })
            .boxed(),
        EditOpName::Delete => vec(item_ref(), 0..3)
            .prop_map(|items| EditOp::Delete { items })
            .boxed(),
        EditOpName::SetText => (item_ref(), text_field(), text())
            .prop_map(|(item, field, text)| EditOp::SetText { item, field, text })
            .boxed(),
    }
}

fn edit_op() -> impl Strategy<Value = EditOp> {
    select(EditOpName::ALL.to_vec()).prop_flat_map(edit_op_for)
}

fn edit_args() -> impl Strategy<Value = EditArgs> {
    (doc_id(), any::<u64>(), vec(edit_op(), 1..4)).prop_map(
        |(doc_id, expected_revision, operations)| EditArgs {
            doc_id,
            expected_revision,
            operations,
        },
    )
}

fn rev_args() -> impl Strategy<Value = RevArgs> {
    (doc_id(), any::<u64>()).prop_map(|(doc_id, expected_revision)| RevArgs {
        doc_id,
        expected_revision,
    })
}

fn job_args() -> impl Strategy<Value = JobArgs> {
    any::<u64>().prop_map(|j| JobArgs { job_id: JobId(j) })
}

fn include() -> impl Strategy<Value = Vec<Include>> {
    proptest::sample::subsequence(vec![Include::Summary, Include::Items, Include::Nets], 1..=3)
        .prop_shuffle()
}

fn query_mode() -> impl Strategy<Value = QueryMode> {
    select(vec![
        QueryMode::Envelope,
        QueryMode::Within,
        QueryMode::Touching,
    ])
}

/// A valid `Operation` named `op`. Exhaustive, with no wildcard arm.
fn operation_for(op: OpName) -> BoxedStrategy<Operation> {
    match op {
        OpName::Capabilities => Just(Operation::Capabilities(NoArgs {})).boxed(),
        OpName::New => Just(Operation::New(NoArgs {})).boxed(),
        OpName::Open => text()
            .prop_map(|path| Operation::Open(OpenArgs { path }))
            .boxed(),
        OpName::Close => (doc_id(), any::<bool>(), option::of(approval_echo()))
            .prop_map(|(doc_id, discard_unsaved, approval)| {
                Operation::Close(CloseArgs {
                    doc_id,
                    discard_unsaved,
                    approval,
                })
            })
            .boxed(),
        OpName::Inspect => (
            doc_id(),
            include(),
            option::of(bounds()),
            option::of(text()),
            page_limit(),
        )
            .prop_map(|(doc_id, include, area, cursor, limit)| {
                Operation::Inspect(InspectArgs {
                    doc_id,
                    include,
                    area,
                    cursor,
                    limit,
                })
            })
            .boxed(),
        OpName::Query => (
            doc_id(),
            bounds(),
            query_mode(),
            option::of(text()),
            page_limit(),
        )
            .prop_map(|(doc_id, area, mode, cursor, limit)| {
                Operation::Query(QueryArgs {
                    doc_id,
                    area,
                    mode,
                    cursor,
                    limit,
                })
            })
            .boxed(),
        OpName::Nets => (
            doc_id(),
            prop_oneof![
                Just((None, None)),
                point().prop_map(|p| (Some(p), None)),
                wire_ref().prop_map(|w| (None, Some(w))),
            ],
        )
            .prop_map(|(doc_id, (at, wire))| Operation::Nets(NetsArgs { doc_id, at, wire }))
            .boxed(),
        OpName::Validate => edit_args().prop_map(Operation::Validate).boxed(),
        OpName::Preview => edit_args().prop_map(Operation::Preview).boxed(),
        OpName::Apply => (edit_args(), option::of(any::<u64>().prop_map(PreviewId)))
            .prop_map(|(e, preview_id)| {
                Operation::Apply(ApplyArgs {
                    doc_id: e.doc_id,
                    expected_revision: e.expected_revision,
                    operations: e.operations,
                    preview_id,
                })
            })
            .boxed(),
        OpName::Undo => rev_args().prop_map(Operation::Undo).boxed(),
        OpName::Redo => rev_args().prop_map(Operation::Redo).boxed(),
        OpName::Save => (doc_id(), option::of(text()), option::of(approval_echo()))
            .prop_map(|(doc_id, path, approval)| {
                Operation::Save(SaveArgs {
                    doc_id,
                    path,
                    approval,
                })
            })
            .boxed(),
        OpName::RequestStatus => text()
            .prop_map(|idempotency_key| Operation::RequestStatus(KeyArgs { idempotency_key }))
            .boxed(),
        OpName::JobStatus => job_args().prop_map(Operation::JobStatus).boxed(),
        OpName::JobCancel => job_args().prop_map(Operation::JobCancel).boxed(),
    }
}

fn available_ops() -> Vec<OpName> {
    OpName::ALL
        .iter()
        .copied()
        .filter(|&op| spec(op).available)
        .collect()
}

/// A request to an available operation.
fn request() -> impl Strategy<Value = Request> {
    select(available_ops()).prop_flat_map(|op| {
        let key = if spec(op).idempotency_key {
            option::of(text()).boxed()
        } else {
            Just(None).boxed()
        };
        (operation_for(op), key).prop_map(|(op, idempotency_key)| Request {
            op,
            idempotency_key,
        })
    })
}

fn blocker() -> impl Strategy<Value = BlockerDto> {
    prop_oneof![
        (part_ref(), any::<u32>()).prop_map(|(part, pin)| BlockerDto::Pin { part, pin }),
        wire_ref().prop_map(|wire| BlockerDto::WireEnd { wire }),
        wire_ref().prop_map(|wire| BlockerDto::WireBody { wire }),
        wire_ref().prop_map(|wire| BlockerDto::Overlap { wire }),
        Just(BlockerDto::Sheet),
    ]
}

/// An `ErrorKind` with code `code`. Exhaustive, with no wildcard arm.
fn kind_for(code: ErrorCode) -> BoxedStrategy<ErrorKind> {
    match code {
        ErrorCode::InvalidRequest => Just(ErrorKind::InvalidRequest).boxed(),
        ErrorCode::SchemaViolation => text()
            .prop_map(|path| ErrorKind::SchemaViolation { path })
            .boxed(),
        ErrorCode::UnknownOperation => Just(ErrorKind::UnknownOperation).boxed(),
        ErrorCode::LimitExceeded => (
            select(vec![
                Limit::RequestBytes,
                Limit::FileBytes,
                Limit::Operations,
                Limit::PageSize,
                Limit::OpenDocuments,
            ]),
            any::<u64>(),
        )
            .prop_map(|(limit, value)| ErrorKind::LimitExceeded { limit, value })
            .boxed(),
        ErrorCode::UnsupportedInMode => Just(ErrorKind::UnsupportedInMode).boxed(),
        ErrorCode::ProtocolVersionUnsupported => {
            Just(ErrorKind::ProtocolVersionUnsupported).boxed()
        }
        ErrorCode::DocumentNotFound => Just(ErrorKind::DocumentNotFound).boxed(),
        ErrorCode::UnknownSymbol => Just(ErrorKind::UnknownSymbol).boxed(),
        ErrorCode::UnknownItem => Just(ErrorKind::UnknownItem).boxed(),
        ErrorCode::OutOfBounds => Just(ErrorKind::OutOfBounds).boxed(),
        ErrorCode::InvalidWire => Just(ErrorKind::InvalidWire).boxed(),
        ErrorCode::MissingItem => item_ref()
            .prop_map(|item| ErrorKind::MissingItem { item })
            .boxed(),
        ErrorCode::DuplicateItem => item_ref()
            .prop_map(|item| ErrorKind::DuplicateItem { item })
            .boxed(),
        ErrorCode::InvalidTextField => (item_ref(), text_field())
            .prop_map(|(item, field)| ErrorKind::InvalidTextField { item, field })
            .boxed(),
        ErrorCode::FollowNoRoute => (point(), point(), blocker())
            .prop_map(|(from, to, blocker)| ErrorKind::FollowNoRoute { from, to, blocker })
            .boxed(),
        ErrorCode::SegmentBlocked => blocker()
            .prop_map(|blocker| ErrorKind::SegmentBlocked { blocker })
            .boxed(),
        ErrorCode::WireCapture => (wire_ref(), blocker())
            .prop_map(|(wire, blocker)| ErrorKind::WireCapture { wire, blocker })
            .boxed(),
        ErrorCode::OffLattice => point().prop_map(|at| ErrorKind::OffLattice { at }).boxed(),
        ErrorCode::StaleRevision => (any::<u64>(), any::<u64>())
            .prop_map(|(expected, actual)| ErrorKind::StaleRevision { expected, actual })
            .boxed(),
        ErrorCode::PreviewMismatch => Just(ErrorKind::PreviewMismatch).boxed(),
        ErrorCode::IdempotencyConflict => Just(ErrorKind::IdempotencyConflict).boxed(),
        ErrorCode::UserGestureInProgress => Just(ErrorKind::UserGestureInProgress).boxed(),
        ErrorCode::NothingToUndo => Just(ErrorKind::NothingToUndo).boxed(),
        ErrorCode::NothingToRedo => Just(ErrorKind::NothingToRedo).boxed(),
        ErrorCode::ApprovalRequired => (
            text(),
            0u8..=3,
            select(vec![
                Effect::Edit,
                Effect::WriteFile,
                Effect::OverwriteFile,
                Effect::DiscardUnsaved,
            ]),
            text(),
        )
            .prop_map(
                |(approval_id, tier, effect, effect_digest)| ErrorKind::ApprovalRequired {
                    approval_id,
                    tier,
                    effect,
                    effect_digest,
                },
            )
            .boxed(),
        ErrorCode::ApprovalDenied => Just(ErrorKind::ApprovalDenied).boxed(),
        ErrorCode::PathOutsideRoot => Just(ErrorKind::PathOutsideRoot).boxed(),
        ErrorCode::FileExists => Just(ErrorKind::FileExists).boxed(),
        ErrorCode::IoError => Just(ErrorKind::IoError).boxed(),
        ErrorCode::UnsupportedFileVersion => Just(ErrorKind::UnsupportedFileVersion).boxed(),
        ErrorCode::Cancelled => Just(ErrorKind::Cancelled).boxed(),
        ErrorCode::JobNotFound => Just(ErrorKind::JobNotFound).boxed(),
        ErrorCode::Internal => Just(ErrorKind::Internal).boxed(),
    }
}

fn agent_error() -> impl Strategy<Value = AgentError> {
    select(ErrorCode::ALL.to_vec())
        .prop_flat_map(kind_for)
        .prop_flat_map(|kind| {
            prop_oneof![
                Just(AgentError::new(kind.clone())),
                (text(), option::of(text())).prop_map(move |(message, hint)| AgentError {
                    kind: kind.clone(),
                    message,
                    hint,
                }),
            ]
        })
}

fn part_body() -> impl Strategy<Value = PartBody> {
    (text(), point(), orient(), text(), text()).prop_map(
        |(symbol, at, orient, designator, value)| PartBody {
            symbol,
            at,
            orient,
            designator,
            value,
        },
    )
}

fn wire_body() -> impl Strategy<Value = WireBody> {
    (point(), point()).prop_map(|(a, b)| WireBody { a, b })
}

fn label_body() -> impl Strategy<Value = LabelBody> {
    (point(), text()).prop_map(|(at, text)| LabelBody { at, text })
}

fn item() -> impl Strategy<Value = Item> {
    prop_oneof![
        (part_ref(), part_body()).prop_map(|(id, part)| Item::Part(PartItem { id, part })),
        (wire_ref(), wire_body()).prop_map(|(id, wire)| Item::Wire(WireItem { id, wire })),
        (label_ref(), label_body()).prop_map(|(id, label)| Item::Label(LabelItem { id, label })),
    ]
}

fn modified() -> impl Strategy<Value = Modified> {
    prop_oneof![
        (part_ref(), part_body(), part_body())
            .prop_map(|(id, before, after)| Modified::Part(PartModified { id, before, after })),
        (wire_ref(), wire_body(), wire_body())
            .prop_map(|(id, before, after)| Modified::Wire(WireModified { id, before, after })),
        (label_ref(), label_body(), label_body()).prop_map(|(id, before, after)| {
            Modified::Label(LabelModified { id, before, after })
        }),
    ]
}

fn items_page() -> impl Strategy<Value = ItemsPage> {
    (vec(item(), 0..3), option::of(text()))
        .prop_map(|(items, next_cursor)| ItemsPage { items, next_cursor })
}

fn net() -> impl Strategy<Value = Net> {
    let key = prop_oneof![
        (part_ref(), any::<u32>()).prop_map(|(part, pin)| NetKey::Pin { part, pin }),
        wire_ref().prop_map(|wire| NetKey::Wire { wire }),
        label_ref().prop_map(|label| NetKey::Label { label }),
    ];
    let pin = (part_ref(), any::<u32>()).prop_map(|(part, pin)| PinRef { part, pin });
    (
        key,
        vec(pin, 0..3),
        vec(wire_ref(), 0..3),
        vec(label_ref(), 0..3),
    )
        .prop_map(|(key, pins, wires, labels)| Net {
            key,
            pins,
            wires,
            labels,
        })
}

fn change() -> impl Strategy<Value = ChangeDto> {
    (
        any::<u64>(),
        option::of(bounds()),
        vec(item_ref(), 0..3),
        vec(item_ref(), 0..3),
        any::<bool>(),
    )
        .prop_map(
            |(revision, dirty, added, removed, nets_changed)| ChangeDto {
                revision,
                dirty,
                added,
                removed,
                nets_changed,
            },
        )
}

fn diff() -> impl Strategy<Value = Diff> {
    (
        vec(item(), 0..2),
        vec(item(), 0..2),
        vec(modified(), 0..2),
        any::<bool>(),
    )
        .prop_map(|(added, removed, modified, truncated)| Diff {
            added,
            removed,
            modified,
            truncated,
        })
}

fn job_state() -> impl Strategy<Value = JobState> {
    let phase = select(vec![
        JobPhase::Queued,
        JobPhase::Running,
        JobPhase::Succeeded,
        JobPhase::Failed,
        JobPhase::CancelledByClient,
        JobPhase::CancelledBySystem,
    ]);
    let progress = (
        any::<u64>(),
        option::of(any::<u64>()),
        text(),
        option::of(text()),
    )
        .prop_map(|(completed, total, unit, message)| Progress {
            completed,
            total,
            unit,
            message,
        });
    (
        any::<u64>(),
        phase,
        option::of(progress),
        option::of(agent_error()),
    )
        .prop_map(|(job, state, progress, error)| JobState {
            job_id: JobId(job),
            state,
            progress,
            error,
        })
}

/// A stored response, as `request_status` reports it: a success of an operation that takes an
/// idempotency key (section 11.2). Nothing else can be generated, so nothing else is blessed.
fn stored_response() -> impl Strategy<Value = (OpName, Response)> {
    let keyed: Vec<OpName> = OpName::ALL
        .iter()
        .copied()
        .filter(|&op| spec(op).idempotency_key)
        .collect();
    select(keyed).prop_flat_map(|op| {
        let outcome = output_for(op, Just(RequestStatus::NotRun).boxed()).prop_map(Ok);
        envelope_with(outcome.boxed()).prop_map(move |r| (op, r))
    })
}

fn request_status() -> BoxedStrategy<RequestStatus> {
    prop_oneof![
        Just(RequestStatus::NotRun),
        (stored_response(), any::<u64>()).prop_map(|((op, response), revision)| {
            RequestStatus::Applied {
                op,
                revision,
                response: Box::new(response),
            }
        }),
        agent_error().prop_map(|error| RequestStatus::Failed { error }),
    ]
    .boxed()
}

/// A result of `op`. Exhaustive, with no wildcard arm.
fn output_for(op: OpName, status: BoxedStrategy<RequestStatus>) -> BoxedStrategy<Output> {
    match op {
        OpName::Capabilities => Just(Output::Capabilities(Box::new(Catalog::current()))).boxed(),
        OpName::New | OpName::Open => (option::of(text()), any::<bool>())
            .prop_map(|(path, dirty)| Output::Document(DocInfo { path, dirty }))
            .boxed(),
        OpName::Close => any::<bool>()
            .prop_map(|discarded| Output::Closed(CloseResult { discarded }))
            .boxed(),
        OpName::Inspect => (
            option::of((any::<u64>(), any::<u64>(), any::<u64>())),
            option::of(items_page()),
            option::of(vec(net(), 0..2)),
        )
            .prop_map(|(summary, items, nets)| {
                Output::Inspect(InspectResult {
                    summary: summary.map(|(parts, wires, labels)| Summary {
                        parts,
                        wires,
                        labels,
                    }),
                    items,
                    nets,
                })
            })
            .boxed(),
        OpName::Query => items_page().prop_map(Output::Query).boxed(),
        OpName::Nets => vec(net(), 0..3)
            .prop_map(|nets| Output::Nets(NetsResult { nets }))
            .boxed(),
        OpName::Validate => (any::<u64>(), change())
            .prop_map(|(base_revision, change)| {
                Output::Validated(ValidateResult {
                    base_revision,
                    change,
                })
            })
            .boxed(),
        OpName::Preview => (any::<u64>(), any::<u64>(), change(), diff())
            .prop_map(|(id, base_revision, change, diff)| {
                Output::Preview(PreviewResult {
                    preview_id: PreviewId(id),
                    base_revision,
                    change,
                    diff,
                })
            })
            .boxed(),
        OpName::Apply => (change(), diff())
            .prop_map(|(change, diff)| Output::Applied(EditResult { change, diff }))
            .boxed(),
        OpName::Undo | OpName::Redo => change()
            .prop_map(|change| Output::Change(ChangeResult { change }))
            .boxed(),
        OpName::Save => (text(), any::<u64>())
            .prop_map(|(path, saved_revision)| {
                Output::Saved(SaveResult {
                    path,
                    saved_revision,
                })
            })
            .boxed(),
        OpName::RequestStatus => status.prop_map(Output::RequestStatus).boxed(),
        OpName::JobStatus | OpName::JobCancel => job_state().prop_map(Output::Job).boxed(),
    }
}

fn envelope_with(outcome: BoxedStrategy<Result<Output, AgentError>>) -> BoxedStrategy<Response> {
    let warning = (text(), text()).prop_map(|(code, message)| Warning { code, message });
    (
        option::of(doc_id()),
        option::of(any::<u64>()),
        outcome,
        vec(warning, 0..2),
    )
        .prop_map(|(doc_id, revision, outcome, warnings)| Response {
            warnings,
            ..Response::new(doc_id, revision, outcome)
        })
        .boxed()
}

fn response_for(op: OpName) -> BoxedStrategy<Response> {
    let outcome = prop_oneof![
        output_for(op, request_status()).prop_map(Ok),
        agent_error().prop_map(Err),
    ];
    envelope_with(outcome.boxed())
}

fn response() -> impl Strategy<Value = (OpName, Response)> {
    select(OpName::ALL.to_vec()).prop_flat_map(|op| response_for(op).prop_map(move |r| (op, r)))
}

// ---- published schemas ---------------------------------------------------------------------

/// The published schema files, parsed from their text.
fn published(name: &str) -> &'static Value {
    static SCHEMAS: OnceLock<BTreeMap<String, Value>> = OnceLock::new();
    let schemas = SCHEMAS.get_or_init(|| {
        schema_files()
            .into_iter()
            .map(|(n, text)| (n, serde_json::from_str(&text).unwrap()))
            .collect()
    });
    schemas
        .get(name)
        .unwrap_or_else(|| panic!("published schema {name}"))
}

fn conforms(name: &str, v: &Value) -> bool {
    validate(published(name), v).is_ok()
}

fn check(name: &str, v: &Value) {
    if let Err(e) = validate(published(name), v) {
        panic!("AINV11: {v} does not validate against {name}: {e:?}");
    }
}

fn refuse(name: &str, v: &Value) {
    assert!(!conforms(name, v), "{name} accepts {v}");
}

/// Fails on any keyword the validator does not implement, and on any unsupported pattern.
fn audit(schema: &Value, at: &str) {
    match schema {
        Value::Bool(_) => {}
        Value::Object(map) => {
            for (key, sub) in map {
                assert!(KEYWORDS.contains(&key.as_str()), "{at}: keyword `{key}`");
                match key.as_str() {
                    "$defs" | "properties" => {
                        for (name, s) in sub.as_object().expect("map") {
                            audit(s, &format!("{at}/{key}/{name}"));
                        }
                    }
                    "prefixItems" | "oneOf" | "anyOf" => {
                        for (i, s) in sub.as_array().expect("list").iter().enumerate() {
                            audit(s, &format!("{at}/{key}/{i}"));
                        }
                    }
                    "items" | "not" | "additionalProperties" => audit(sub, &format!("{at}/{key}")),
                    "pattern" => assert!(
                        Pattern::parse(sub.as_str().unwrap()).is_some(),
                        "{at}: pattern"
                    ),
                    _ => {}
                }
            }
        }
        other => panic!("{at}: a schema is an object or a boolean, not {other}"),
    }
}

/// Compact JSON with object keys in reverse byte order (AINV5: key order does not matter).
fn reversed_keys(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            keys.reverse();
            let body: Vec<String> = keys
                .into_iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap(),
                        reversed_keys(&map[k])
                    )
                })
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(items) => {
            let body: Vec<String> = items.iter().map(reversed_keys).collect();
            format!("[{}]", body.join(","))
        }
        scalar => serde_json::to_string(scalar).unwrap(),
    }
}

// ---- mutations -----------------------------------------------------------------------------

/// One change to a valid document, for the parity tests.
#[derive(Debug, Clone)]
enum Mutation {
    Remove,
    AddField(&'static str),
    Null,
    ToArray,
    ToObject,
    Str(&'static str),
    Num(i128),
    Bool,
}

fn mutation() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        Just(Mutation::Remove),
        select(vec!["aa", "zz", "x", "with_wires"]).prop_map(Mutation::AddField),
        Just(Mutation::Null),
        Just(Mutation::ToArray),
        Just(Mutation::ToObject),
        select(vec![
            "",
            "x",
            "part:01",
            "part:4294967296",
            "wire:4294967295",
            "d18446744073709551616",
            "pv-0",
            "identity",
            "summary",
            "applied",
        ])
        .prop_map(Mutation::Str),
        select(vec![
            -1i128,
            0,
            1,
            5_001,
            1_000_000_001,
            4_294_967_296,
            i128::from(u64::MAX),
            i128::from(i64::MIN),
        ])
        .prop_map(Mutation::Num),
        Just(Mutation::Bool),
    ]
}

/// Every node below the root, as a path of object keys and array indexes.
fn node_paths(v: &Value, prefix: &mut Vec<Value>, out: &mut Vec<Vec<Value>>) {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                prefix.push(Value::String(k.clone()));
                out.push(prefix.clone());
                node_paths(&map[k.as_str()], prefix, out);
                prefix.pop();
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                prefix.push(json!(i));
                out.push(prefix.clone());
                node_paths(item, prefix, out);
                prefix.pop();
            }
        }
        _ => {}
    }
}

fn apply_mutation(doc: &Value, at: Index, m: &Mutation) -> Value {
    let mut doc = doc.clone();
    let mut paths = Vec::new();
    node_paths(&doc, &mut Vec::new(), &mut paths);
    if paths.is_empty() {
        return doc;
    }
    let path = &paths[at.index(paths.len())];
    let (last, parent_path) = path.split_last().unwrap();
    let mut parent = &mut doc;
    for seg in parent_path {
        parent = match seg {
            Value::String(k) => &mut parent[k.as_str()],
            _ => &mut parent[seg.as_u64().unwrap() as usize],
        };
    }
    if let Mutation::Remove = m {
        match (parent, last) {
            (Value::Object(map), Value::String(k)) => {
                map.remove(k);
            }
            (Value::Array(items), i) => {
                items.remove(i.as_u64().unwrap() as usize);
            }
            _ => unreachable!(),
        }
        return doc;
    }
    let node = match last {
        Value::String(k) => &mut parent[k.as_str()],
        i => &mut parent[i.as_u64().unwrap() as usize],
    };
    *node = match (m, node.take()) {
        (Mutation::AddField(k), Value::Object(mut map)) => {
            map.insert((*k).to_owned(), json!(1));
            Value::Object(map)
        }
        (Mutation::AddField(_), other) => other,
        (Mutation::Null, _) => Value::Null,
        (Mutation::ToArray, Value::Object(map)) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort();
            Value::Array(keys.iter().map(|k| map[k.as_str()].clone()).collect())
        }
        (Mutation::ToArray, other) => json!([other]),
        (Mutation::ToObject, Value::Array(items)) => {
            let map = items
                .into_iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect();
            Value::Object(map)
        }
        (Mutation::ToObject, other) => json!({ "v": other }),
        (Mutation::Str(s), _) => json!(s),
        (Mutation::Num(n), _) => match (i64::try_from(*n), u64::try_from(*n)) {
            (Ok(i), _) => json!(i),
            (_, Ok(u)) => json!(u),
            _ => unreachable!(),
        },
        (Mutation::Bool, _) => json!(true),
        (Mutation::Remove, _) => unreachable!(),
    };
    doc
}

/// The decoder refuses a document the schema accepts only for a decoder-only rule (9.1).
fn decoder_only(message: &str) -> bool {
    message.contains("inverted rectangle")
}

// ---- tests ---------------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Every request round-trips through both wire forms, validates against `request`,
    /// `<op>.params` and `<op>.arguments`, and decodes the same with object keys in another
    /// order (AINV5, AINV11).
    #[test]
    fn requests_round_trip_and_validate(req in request()) {
        let op = req.op.name();
        let text = req.to_json();
        prop_assert_eq!(&text.parse::<Request>().unwrap(), &req);
        let value: Value = serde_json::from_str(&text).unwrap();
        check("request", &value);
        check(&format!("{op}.params"), &value["params"]);
        prop_assert_eq!(&reversed_keys(&value).parse::<Request>().unwrap(), &req);
        prop_assert_eq!(&text, &req.to_json());

        let args = req.to_arguments();
        prop_assert_eq!(&Request::from_arguments(op, args.as_bytes()).unwrap(), &req);
        let args_value: Value = serde_json::from_str(&args).unwrap();
        check(&format!("{op}.arguments"), &args_value);
        let reordered = reversed_keys(&args_value);
        prop_assert_eq!(&Request::from_arguments(op, reordered.as_bytes()).unwrap(), &req);
    }

    /// Every response round-trips through JSON and validates against `response` and its
    /// `<op>.response` when published (AINV11).
    #[test]
    fn responses_round_trip_and_validate((op, resp) in response()) {
        let text = resp.to_json();
        prop_assert_eq!(&Response::from_slice(op, text.as_bytes()).unwrap(), &resp);
        if let Ok(output) = &resp.outcome {
            prop_assert!(output.answers(op));
        }
        let value: Value = serde_json::from_str(&text).unwrap();
        if spec(op).available {
            check("response", &value);
            check(&format!("{op}.response"), &value);
        }
        let reordered = reversed_keys(&value);
        prop_assert_eq!(&Response::from_slice(op, reordered.as_bytes()).unwrap(), &resp);
    }

    /// Every error kind round-trips and keeps its code and its fixed `retryable`.
    #[test]
    fn errors_round_trip(err in agent_error()) {
        let text = serde_json::to_string(&err).unwrap();
        let back: AgentError = serde_json::from_str(&text).unwrap();
        prop_assert_eq!(back.code(), err.code());
        prop_assert_eq!(back, err);
    }

    /// Parity, envelope form: after any one mutation of a valid request the decoder accepts it
    /// exactly when the published `request` schema does, except for a decoder-only rule.
    /// AINV5 for invalid requests: reordering keys does not change the error bytes.
    #[test]
    fn envelope_decoder_and_schema_agree(req in request(), at in any::<Index>(), m in mutation()) {
        let doc = apply_mutation(&serde_json::from_str(&req.to_json()).unwrap(), at, &m);
        let text = serde_json::to_string(&doc).unwrap();
        let decoded = text.parse::<Request>();
        let schema_ok = conforms("request", &doc);
        match &decoded {
            Ok(_) => prop_assert!(schema_ok, "decoder accepts what the schema refuses: {}", text),
            Err(e) => prop_assert!(
                !schema_ok || decoder_only(&e.message),
                "decoder refuses what the schema accepts: {} -> {}", text, e
            ),
        }
        let again = reversed_keys(&doc).parse::<Request>();
        prop_assert_eq!(
            decoded.map(|r| r.to_json()).map_err(|e| serde_json::to_string(&e).unwrap()),
            again.map(|r| r.to_json()).map_err(|e| serde_json::to_string(&e).unwrap())
        );
    }

    /// Parity, MCP arguments form, against `<op>.arguments`.
    #[test]
    fn arguments_decoder_and_schema_agree(req in request(), at in any::<Index>(), m in mutation()) {
        let op = req.op.name();
        let doc = apply_mutation(&serde_json::from_str(&req.to_arguments()).unwrap(), at, &m);
        let text = serde_json::to_string(&doc).unwrap();
        let decoded = Request::from_arguments(op, text.as_bytes());
        let schema_ok = conforms(&format!("{op}.arguments"), &doc);
        match &decoded {
            Ok(_) => prop_assert!(schema_ok, "decoder accepts what the schema refuses: {}", text),
            Err(e) => prop_assert!(
                !schema_ok || decoder_only(&e.message),
                "decoder refuses what the schema accepts: {} -> {}", text, e
            ),
        }
        let again = Request::from_arguments(op, reversed_keys(&doc).as_bytes());
        prop_assert_eq!(
            decoded.map(|r| r.to_arguments()).map_err(|e| serde_json::to_string(&e).unwrap()),
            again.map(|r| r.to_arguments()).map_err(|e| serde_json::to_string(&e).unwrap())
        );
    }

    /// Parity, responses, against `<op>.response`.
    #[test]
    fn response_decoder_and_schema_agree(
        (op, resp) in response(),
        at in any::<Index>(),
        m in mutation(),
    ) {
        prop_assume!(spec(op).available);
        let doc = apply_mutation(&serde_json::from_str(&resp.to_json()).unwrap(), at, &m);
        let text = serde_json::to_string(&doc).unwrap();
        let decoded = Response::from_slice(op, text.as_bytes());
        let schema_ok = conforms(&format!("{op}.response"), &doc);
        match &decoded {
            Ok(_) => prop_assert!(schema_ok, "decoder accepts what the schema refuses: {}", text),
            Err(e) => prop_assert!(
                !schema_ok || decoder_only(&e.to_string()),
                "decoder refuses what the schema accepts: {} -> {}", text, e
            ),
        }
    }

    /// The published id patterns accept exactly what the codecs hold.
    #[test]
    fn id_patterns_agree_with_the_codecs(n in any::<u64>(), digits in "[0-9]{1,21}") {
        for s in [n.to_string(), digits] {
            prop_assert_eq!(
                conforms("undo.params", &json!({ "doc_id": format!("d{s}"), "expected_revision": 0 })),
                format!("d{s}").parse::<DocId>().is_ok()
            );
            prop_assert_eq!(
                conforms("nets.params", &json!({ "doc_id": "d1", "wire": format!("wire:{s}") })),
                format!("wire:{s}").parse::<WireRef>().is_ok()
            );
        }
    }
}

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema")
}

/// AINV11: the committed JSON Schemas equal the generated ones. Run with
/// `OMNIA_BLESS_SCHEMAS=1` to rewrite `crates/omnia-agent/schema/` after a deliberate change.
#[test]
fn ainv11_committed_schemas_equal_generated() {
    let dir = schema_dir();
    let files = schema_files();
    if std::env::var_os("OMNIA_BLESS_SCHEMAS").is_some() {
        for entry in fs::read_dir(&dir).expect("schema dir") {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json") {
                fs::remove_file(path).unwrap();
            }
        }
        for (name, text) in &files {
            fs::write(dir.join(format!("{name}.json")), text).unwrap();
        }
    }
    let committed: BTreeSet<String> = fs::read_dir(&dir)
        .expect("crates/omnia-agent/schema exists")
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    let generated: BTreeSet<String> = files.iter().map(|(n, _)| format!("{n}.json")).collect();
    assert_eq!(
        committed, generated,
        "AINV11: schema file set differs; run `OMNIA_BLESS_SCHEMAS=1 cargo test -p omnia-agent ainv11`"
    );
    for (name, text) in &files {
        let actual = fs::read_to_string(dir.join(format!("{name}.json"))).unwrap();
        // A checkout with CRLF line endings holds the same schema.
        assert!(
            actual.replace("\r\n", "\n") == *text,
            "AINV11: {name}.json is stale; run `OMNIA_BLESS_SCHEMAS=1 cargo test -p omnia-agent ainv11`"
        );
    }
}

#[test]
fn ainv11_generation_is_deterministic_and_covers_the_catalog() {
    let files = schema_files();
    assert_eq!(files, schema_files());
    let names: BTreeSet<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
    let catalog = Catalog::current();
    for entry in &catalog.operations {
        for name in [
            &entry.params_schema,
            &entry.arguments_schema,
            &entry.response_schema,
        ] {
            assert!(names.contains(name.as_str()), "{name}");
        }
    }
    assert_eq!(names.len(), 2 + 3 * catalog.operations.len());
    for (name, text) in &files {
        let value: Value = serde_json::from_str(text).unwrap();
        audit(&value, name);
        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(value["type"], "object", "{name}: MCP needs an object root");
        assert_eq!(*text, to_canonical_json(&value), "{name} is canonical");
    }
    assert!(
        schema_text("job_status.params").is_none(),
        "A3 operations are unpublished"
    );
}

/// AINV11: the request decoder dispatches exactly the catalog's operations.
#[test]
fn ainv11_decoder_dispatches_exactly_the_catalog() {
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let listed: BTreeSet<OpName> = Catalog::current()
        .operations
        .iter()
        .map(|e| e.name)
        .collect();
    for &op in OpName::ALL {
        let sample = operation_for(op).new_tree(&mut runner).unwrap().current();
        assert_eq!(sample.name(), op);
        let req = Request {
            op: sample,
            idempotency_key: None,
        };
        let envelope = req.to_json().parse::<Request>();
        let arguments = Request::from_arguments(op, req.to_arguments().as_bytes());
        if listed.contains(&op) {
            assert_eq!(envelope.unwrap().op.name(), op);
            assert_eq!(arguments.unwrap().op.name(), op);
        } else {
            assert_eq!(
                envelope.unwrap_err().code(),
                ErrorCode::UnknownOperation,
                "{op}"
            );
            assert_eq!(
                arguments.unwrap_err().code(),
                ErrorCode::UnknownOperation,
                "{op}"
            );
        }
    }
    let err = r#"{"op":"omnia_inspect","params":{}}"#.parse::<Request>().unwrap_err();
    assert_eq!(err.code(), ErrorCode::UnknownOperation);
}

#[test]
fn the_capabilities_result_validates() {
    let resp = Response::new(
        None,
        None,
        Ok(Output::Capabilities(Box::new(Catalog::current()))),
    );
    let value: Value = serde_json::from_str(&resp.to_json()).unwrap();
    check("capabilities.response", &value);
}

fn code_of(text: &str) -> (ErrorCode, ErrorKind) {
    let err = text.parse::<Request>().unwrap_err();
    (err.code(), err.kind)
}

fn violation_at(text: &str) -> String {
    match code_of(text) {
        (ErrorCode::SchemaViolation, ErrorKind::SchemaViolation { path }) => path,
        other => panic!("{text}: {other:?}"),
    }
}

#[test]
fn section_9_1_objects_reject_unknown_fields() {
    let doc = r#""doc_id":"d1""#;
    assert_eq!(violation_at(r#"{"op":"new","params":{},"x":1}"#), "/x");
    assert_eq!(
        violation_at(r#"{"op":"new","params":{"x":1}}"#),
        "/params/x"
    );
    assert_eq!(
        violation_at(&format!(
            r#"{{"op":"nets","params":{{{doc},"at":[0,0],"z":0}}}}"#
        )),
        "/params/z"
    );
    assert_eq!(
        violation_at(&format!(
            r#"{{"op":"preview","params":{{{doc},"expected_revision":0,"operations":[{{"op":"delete","items":[]}},{{"op":"delete","items":[],"with_wires":true}}]}}}}"#
        )),
        "/params/operations/1/with_wires",
        "with_wires arrives with A2.3"
    );
    assert_eq!(
        violation_at(&format!(
            r#"{{"op":"inspect","params":{{{doc},"include":["summary"],"area":{{"min":[0,0],"max":[1,1],"w":0}}}}}}"#
        )),
        "/params/area/w"
    );
    refuse("new.params", &json!({ "x": 1 }));
    refuse("request", &json!({ "op": "new", "params": {}, "x": 1 }));
    refuse(
        "preview.params",
        &json!({ "doc_id": "d1", "expected_revision": 0,
            "operations": [{ "op": "delete", "items": [], "with_wires": true }] }),
    );
}

/// Review finding 3: the reported field does not depend on the order keys arrive in.
#[test]
fn ainv5_error_paths_do_not_depend_on_key_order() {
    let a = r#"{"op":"inspect","params":{"doc_id":"d1","include":["summary"],"x":1,"y":2}}"#;
    let b = r#"{"op":"inspect","params":{"y":2,"x":1,"include":["summary"],"doc_id":"d1"}}"#;
    assert_eq!(violation_at(a), "/params/x");
    let (ea, eb) = (
        a.parse::<Request>().unwrap_err(),
        b.parse::<Request>().unwrap_err(),
    );
    assert_eq!(
        serde_json::to_string(&ea).unwrap(),
        serde_json::to_string(&eb).unwrap()
    );
    let c = r#"{"zz":1,"op":"new","params":{},"aa":1}"#;
    assert_eq!(violation_at(c), "/aa");
}

/// Review finding 2: shapes the schemas refuse are refused by the decoder too.
#[test]
fn decoder_refuses_nulls_positional_arrays_and_null_errors() {
    for (text, path) in [
        (
            r#"{"op":"save","params":{"doc_id":"d1","path":null}}"#,
            "/params/path",
        ),
        (r#"{"op":"open","params":["sheet.ron"]}"#, "/params"),
        (
            r#"{"op":"query","params":{"doc_id":"d1","area":[[0,0],[1,1]],"mode":"within"}}"#,
            "/params/area",
        ),
        (
            r#"{"op":"close","params":{"doc_id":"d1","approval":["a","b"]}}"#,
            "/params/approval",
        ),
        (
            r#"{"op":"preview","params":{"doc_id":"d1","expected_revision":0,"operations":[["delete",[]]]}}"#,
            "/params/operations/0",
        ),
    ] {
        assert_eq!(violation_at(text), path, "{text}");
        let value: Value = serde_json::from_str(text).unwrap();
        refuse("request", &value);
    }
    let null_error =
        r#"{"schema":"omnia.agent/1","result":{"discarded":true},"error":null,"warnings":[]}"#;
    assert!(Response::from_slice(OpName::Close, null_error.as_bytes()).is_err());
    refuse("close.response", &serde_json::from_str(null_error).unwrap());
    let positional = r#"{"schema":"omnia.agent/1","result":[true],"warnings":[]}"#;
    assert!(Response::from_slice(OpName::Close, positional.as_bytes()).is_err());
}

#[test]
fn malformed_requests_get_typed_errors() {
    for text in [
        "",
        "{",
        "[]",
        r#"{"op":"new","params":{},"op":"new"}"#,
        r#"{"op":"undo","params":{"doc_id":"d1","expected_revision":1.0}}"#,
        r#"{"op":"undo","params":{"doc_id":"d1","expected_revision":1e3}}"#,
        r#"{"op":"undo","params":{"doc_id":"d1","expected_revision":18446744073709551616}}"#,
        r#"{"op":"new","params":{}} x"#,
    ] {
        assert_eq!(code_of(text).0, ErrorCode::InvalidRequest, "{text}");
    }
    assert_eq!(
        code_of(r#"{"op":"fly","params":{}}"#).0,
        ErrorCode::UnknownOperation
    );
    assert_eq!(
        code_of(r#"{"op":"job_status","params":{"job_id":"j1"}}"#).0,
        ErrorCode::UnknownOperation,
        "job tools wait for A3"
    );
    assert_eq!(violation_at(r#"{"params":{}}"#), "");
    assert_eq!(violation_at(r#"{"op":3,"params":{}}"#), "/op");
    assert_eq!(violation_at(r#"{"op":"new"}"#), "");
    assert_eq!(
        violation_at(
            r#"{"op":"inspect","params":{"doc_id":"d1","include":["summary"]},"idempotency_key":"k"}"#
        ),
        "/idempotency_key",
        "section 11.2: only apply, undo, redo and save take a key"
    );
    for (text, path) in [
        (
            r#"{"op":"undo","params":{"doc_id":"d01","expected_revision":1}}"#,
            "/params/doc_id",
        ),
        (
            r#"{"op":"inspect","params":{"doc_id":"d1","include":[]}}"#,
            "/params/include",
        ),
        (
            r#"{"op":"inspect","params":{"doc_id":"d1","include":["nets","nets"]}}"#,
            "/params/include",
        ),
        (
            r#"{"op":"inspect","params":{"doc_id":"d1","include":["items"],"limit":0}}"#,
            "/params/limit",
        ),
        (
            r#"{"op":"nets","params":{"doc_id":"d1","at":[0,0],"wire":"wire:1"}}"#,
            "/params",
        ),
        (
            r#"{"op":"preview","params":{"doc_id":"d1","expected_revision":0,"operations":[]}}"#,
            "/params/operations",
        ),
        (
            r#"{"op":"preview","params":{"doc_id":"d1","expected_revision":0,"operations":[{"op":"move","items":["part:01"],"delta":[0,0]}]}}"#,
            "/params/operations/0/items/0",
        ),
    ] {
        assert_eq!(violation_at(text), path, "{text}");
    }
}

/// Review finding 5: the published schemas hold only what the codecs can represent.
#[test]
fn schemas_refuse_values_the_codecs_cannot_hold() {
    let nets = |wire: &str| json!({ "doc_id": "d1", "wire": wire });
    check("nets.params", &nets("wire:4294967295"));
    refuse("nets.params", &nets("wire:4294967296"));
    assert_eq!(
        violation_at(r#"{"op":"nets","params":{"doc_id":"d1","wire":"wire:4294967296"}}"#),
        "/params/wire"
    );
    let undo = |doc: &str, rev: Value| json!({ "doc_id": doc, "expected_revision": rev });
    check(
        "undo.params",
        &undo("d18446744073709551615", json!(u64::MAX)),
    );
    refuse("undo.params", &undo("d18446744073709551616", json!(0)));
    let too_big: Value = serde_json::from_str("100000000000000000000").unwrap();
    refuse("undo.params", &undo("d1", too_big));
    let query = |max: i64| json!({ "doc_id": "d1", "area": { "min": [0, 0], "max": [max, 0] }, "mode": "within" });
    check("query.params", &query(1_000_000_000));
    refuse("query.params", &query(1_000_000_001));
    assert_eq!(
        violation_at(&format!(
            r#"{{"op":"query","params":{}}}"#,
            query(1_000_000_001)
        )),
        "/params/area/max/0"
    );
    // The one decoder-only shape rule: corner ordering.
    let inverted =
        json!({ "doc_id": "d1", "area": { "min": [5, 0], "max": [0, 0] }, "mode": "within" });
    check("query.params", &inverted);
    let (code, _) = code_of(&format!(r#"{{"op":"query","params":{inverted}}}"#));
    assert_eq!(code, ErrorCode::SchemaViolation);
}

/// Review finding 6: `retryable` is fixed by the code, in the schema and the decoder.
#[test]
fn retryable_cannot_contradict_the_code() {
    let resp = |retryable: bool| {
        json!({ "schema": "omnia.agent/1", "warnings": [],
            "error": { "code": "nothing_to_undo", "message": "m", "retryable": retryable,
                       "details": {} } })
    };
    check("undo.response", &resp(false));
    refuse("undo.response", &resp(true));
    let text = serde_json::to_string(&resp(true)).unwrap();
    assert!(Response::from_slice(OpName::Undo, text.as_bytes()).is_err());
}

/// Review finding 4: `applied` holds a success of the operation it names, and only for the
/// operations that take an idempotency key.
#[test]
fn request_status_applied_is_a_matching_success() {
    let status =
        |status: Value| json!({ "schema": "omnia.agent/1", "result": status, "warnings": [] });
    let change = json!({ "revision": 3, "added": [], "removed": [], "nets_changed": false });
    let ok_undo = json!({ "schema": "omnia.agent/1", "revision": 3,
        "result": { "change": change }, "warnings": [] });
    let good = status(json!({ "status": "applied", "op": "undo", "revision": 3,
        "response": ok_undo }));
    check("request_status.response", &good);
    let text = serde_json::to_string(&good).unwrap();
    assert!(Response::from_slice(OpName::RequestStatus, text.as_bytes()).is_ok());

    let failed_undo = json!({ "schema": "omnia.agent/1", "warnings": [],
        "error": { "code": "nothing_to_undo", "message": "m", "retryable": false,
                   "details": {} } });
    let new_ok = json!({ "schema": "omnia.agent/1", "doc_id": "d1", "revision": 0,
        "result": { "dirty": false }, "warnings": [] });
    for bad in [
        status(
            json!({ "status": "applied", "op": "undo", "revision": 3, "response": failed_undo }),
        ),
        status(json!({ "status": "applied", "op": "undo", "revision": 0, "response": new_ok })),
        status(json!({ "status": "applied", "op": "new", "revision": 0, "response": new_ok })),
    ] {
        refuse("request_status.response", &bad);
        let text = serde_json::to_string(&bad).unwrap();
        assert!(
            Response::from_slice(OpName::RequestStatus, text.as_bytes()).is_err(),
            "{text}"
        );
    }
}

/// Review finding 1: the MCP arguments carry `idempotency_key` beside the params, and both
/// forms give the same request (ATC-edit-03).
#[test]
fn mcp_arguments_carry_the_idempotency_key() {
    let envelope = r#"{"op":"apply","params":{"doc_id":"d1","expected_revision":4,"operations":[{"op":"delete","items":["part:1"]}]},"idempotency_key":"k-1"}"#;
    let arguments = r#"{"doc_id":"d1","expected_revision":4,"operations":[{"op":"delete","items":["part:1"]}],"idempotency_key":"k-1"}"#;
    let a = envelope.parse::<Request>().unwrap();
    let b = Request::from_arguments(OpName::Apply, arguments.as_bytes()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.idempotency_key.as_deref(), Some("k-1"));
    assert_eq!(a.to_arguments(), arguments);
    check("apply.arguments", &serde_json::from_str(arguments).unwrap());
    refuse("apply.params", &serde_json::from_str(arguments).unwrap());
    for op in [OpName::Undo, OpName::Redo, OpName::Save] {
        let args = json!({ "doc_id": "d1", "expected_revision": 1, "idempotency_key": "k" });
        let args = if op == OpName::Save {
            json!({ "doc_id": "d1", "idempotency_key": "k" })
        } else {
            args
        };
        check(&format!("{op}.arguments"), &args);
        let text = serde_json::to_string(&args).unwrap();
        let req = Request::from_arguments(op, text.as_bytes()).unwrap();
        assert_eq!(req.idempotency_key.as_deref(), Some("k"));
    }
    let inspect = json!({ "doc_id": "d1", "include": ["summary"], "idempotency_key": "k" });
    refuse("inspect.arguments", &inspect);
    let err = Request::from_arguments(
        OpName::Inspect,
        serde_json::to_string(&inspect).unwrap().as_bytes(),
    )
    .unwrap_err();
    assert_eq!(
        err.kind,
        ErrorKind::SchemaViolation {
            path: "/idempotency_key".into()
        }
    );
    let err = Request::from_arguments(OpName::Apply, br#"{"doc_id":"d1","expected_revision":4,"operations":[{"op":"delete","items":[]}],"idempotency_key":7}"#).unwrap_err();
    assert_eq!(
        err.kind,
        ErrorKind::SchemaViolation {
            path: "/idempotency_key".into()
        }
    );
}

#[test]
fn ak2_to_ak4_limits_are_refused_not_truncated() {
    let page = r#"{"op":"query","params":{"doc_id":"d1","area":{"min":[0,0],"max":[1,1]},"mode":"within","limit":5001}}"#;
    assert_eq!(
        code_of(page).1,
        ErrorKind::LimitExceeded {
            limit: Limit::PageSize,
            value: 5001
        }
    );
    let op = r#"{"op":"delete","items":[]}"#;
    let ops = vec![op; 4_097].join(",");
    let many = format!(
        r#"{{"op":"apply","params":{{"doc_id":"d1","expected_revision":0,"operations":[{ops}]}}}}"#
    );
    assert_eq!(
        code_of(&many).1,
        ErrorKind::LimitExceeded {
            limit: Limit::Operations,
            value: 4_097
        }
    );
    let at_limit = many.replacen(&format!("{op},"), "", 1);
    assert!(at_limit.parse::<Request>().is_ok(), "AK3 allows 4,096");
    let padding = " ".repeat(8 * 1024 * 1024);
    let big = format!(r#"{{"op":"new","params":{{}}}}{padding}"#);
    assert_eq!(
        code_of(&big).1,
        ErrorKind::LimitExceeded {
            limit: Limit::RequestBytes,
            value: big.len() as u64
        }
    );
}

#[test]
fn section_9_3_example_shapes() {
    let change = omnia_doc::Change {
        revision: 13,
        dirty: Some(
            Rect::new(
                Pt { x: 0, y: 0 },
                Pt {
                    x: 10_160_000,
                    y: 5_080_000,
                },
            )
            .unwrap(),
        ),
        added: vec![
            ItemId::Wire(WireId(42)),
            ItemId::Part(PartId(1)),
            ItemId::Wire(WireId(41)),
        ],
        removed: vec![ItemId::Wire(WireId(7))],
        nets_changed: true,
    };
    let dto = ChangeDto::from(&change);
    assert_eq!(
        serde_json::to_string(&dto).unwrap(),
        r#"{"revision":13,"dirty":{"min":[0,0],"max":[10160000,5080000]},"added":["part:1","wire:41","wire:42"],"removed":["wire:7"],"nets_changed":true}"#,
        "AD8: added and removed sorted by kind then id"
    );
    let item = Item::Wire(WireItem {
        id: WireRef(41),
        wire: WireBody {
            a: Point(Pt { x: 0, y: 0 }),
            b: Point(Pt { x: 5_080_000, y: 0 }),
        },
    });
    assert_eq!(
        serde_json::to_string(&item).unwrap(),
        r#"{"id":"wire:41","wire":{"a":[0,0],"b":[5080000,0]}}"#
    );
    let resp = Response::new(
        Some(DocId(1)),
        Some(13),
        Ok(Output::Change(ChangeResult { change: dto })),
    );
    assert!(
        resp.to_json()
            .starts_with(r#"{"schema":"omnia.agent/1","doc_id":"d1","revision":13,"result":"#)
    );
    assert!(resp.to_json().ends_with(r#","warnings":[]}"#));
}

#[test]
fn responses_refuse_unknown_fields_and_wrong_shapes() {
    let ok = r#"{"schema":"omnia.agent/1","result":{"discarded":true},"warnings":[]}"#;
    assert!(Response::from_slice(OpName::Close, ok.as_bytes()).is_ok());
    for bad in [
        r#"{"schema":"omnia.agent/2","result":{"discarded":true},"warnings":[]}"#,
        r#"{"schema":"omnia.agent/1","result":{"discarded":true},"warnings":[],"x":1}"#,
        r#"{"schema":"omnia.agent/1","result":{"discarded":true,"x":1},"warnings":[]}"#,
        r#"{"schema":"omnia.agent/1","warnings":[]}"#,
    ] {
        assert!(
            Response::from_slice(OpName::Close, bad.as_bytes()).is_err(),
            "{bad}"
        );
    }
    assert!(Response::from_slice(OpName::Save, ok.as_bytes()).is_err());
}

/// The validator is not vacuous: mutations of valid documents are refused.
#[test]
fn schema_checks_refuse_mutated_documents() {
    let caps = Response::new(
        None,
        None,
        Ok(Output::Capabilities(Box::new(Catalog::current()))),
    );
    let mut v: Value = serde_json::from_str(&caps.to_json()).unwrap();
    v["result"]["operations"][0]["tool"] = 5.into();
    refuse("capabilities.response", &v);

    let stale = Response::new(
        Some(DocId(1)),
        Some(14),
        Err(AgentError::new(ErrorKind::StaleRevision {
            expected: 12,
            actual: 14,
        })),
    );
    let good: Value = serde_json::from_str(&stale.to_json()).unwrap();
    check("undo.response", &good);
    let mut v = good.clone();
    v["error"]["details"] = json!({});
    refuse("undo.response", &v);
    let mut v = good.clone();
    v["result"] = json!({});
    refuse("undo.response", &v);
    let mut v = good;
    v["doc_id"] = "d01".into();
    refuse("undo.response", &v);

    let mv = |items: Value, delta: Value, rev: Value| {
        json!({ "doc_id": "d1", "expected_revision": rev,
            "operations": [{ "op": "move", "items": items, "delta": delta }] })
    };
    check(
        "apply.params",
        &mv(json!(["part:1"]), json!([0, 0]), json!(0)),
    );
    refuse(
        "apply.params",
        &mv(json!(["part:1"]), json!([0, 0, 0]), json!(0)),
    );
    refuse(
        "apply.params",
        &mv(json!(["part:01"]), json!([0, 0]), json!(0)),
    );
    refuse(
        "apply.params",
        &mv(json!(["part:1"]), json!([0, 0]), json!(-1)),
    );
}

#[test]
fn job_dtos_round_trip_before_they_are_published() {
    let args = JobArgs { job_id: JobId(7) };
    let text = serde_json::to_string(&args).unwrap();
    assert_eq!(text, r#"{"job_id":"j7"}"#);
    assert_eq!(serde_json::from_str::<JobArgs>(&text).unwrap(), args);
}
