//! A1.3 done-when tests: every DTO round-trips through JSON, every serialized DTO validates
//! against its committed JSON Schema, and the committed schemas equal the generated ones
//! (AINV11). Strategies pick variants through exhaustive `match`es over the catalog's name lists,
//! so a new operation, edit operation, result or error code fails to compile here until it has
//! a generator.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::OnceLock,
};

use omnia_doc::{ItemId, LabelId, Orient, PartId, Pt, Rect, TextField, WireId};
use proptest::{collection::vec, option, prelude::*, sample::select};
use serde_json::Value;

use super::*;
use crate::{
    AgentError, Catalog, EditOpName, ErrorCode, ErrorKind, OpName,
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

fn request() -> impl Strategy<Value = Request> {
    select(OpName::ALL.to_vec()).prop_flat_map(|op| {
        let key = if crate::catalog::spec(op).idempotency_key {
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
                (text(), any::<bool>(), option::of(text())).prop_map(
                    move |(message, retryable, hint)| AgentError {
                        kind: kind.clone(),
                        message,
                        retryable,
                        hint,
                    }
                ),
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

/// A non-recursive response to `op`, for `request_status`'s stored response.
fn stored_response() -> impl Strategy<Value = (OpName, Response)> {
    let ops: Vec<OpName> = OpName::ALL
        .iter()
        .copied()
        .filter(|&op| op != OpName::RequestStatus)
        .collect();
    select(ops).prop_flat_map(|op| {
        response_for(op, Just(RequestStatus::NotRun).boxed()).prop_map(move |r| (op, r))
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

fn response_for(op: OpName, status: BoxedStrategy<RequestStatus>) -> BoxedStrategy<Response> {
    let outcome = prop_oneof![
        output_for(op, status).prop_map(Ok),
        agent_error().prop_map(Err),
    ];
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

fn response() -> impl Strategy<Value = (OpName, Response)> {
    select(OpName::ALL.to_vec())
        .prop_flat_map(|op| response_for(op, request_status()).prop_map(move |r| (op, r)))
}

// ---- a JSON Schema validator for the keywords the module emits ------------------------------

const KEYWORDS: &[&str] = &[
    "$schema",
    "$defs",
    "$ref",
    "title",
    "description",
    "type",
    "properties",
    "required",
    "additionalProperties",
    "items",
    "prefixItems",
    "minItems",
    "maxItems",
    "uniqueItems",
    "enum",
    "const",
    "oneOf",
    "anyOf",
    "not",
    "minimum",
    "maximum",
    "pattern",
];

/// Fails on any keyword the validator does not implement, so none is silently ignored.
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
                    "pattern" => assert!(pattern_check(sub.as_str().unwrap()).is_some(), "{at}"),
                    _ => {}
                }
            }
        }
        other => panic!("{at}: a schema is an object or a boolean, not {other}"),
    }
}

/// The patterns the module emits, checked with the codecs that own them.
fn pattern_check(pattern: &str) -> Option<fn(&str) -> bool> {
    Some(match pattern {
        "^part:(0|[1-9][0-9]*)$" => |s| s.parse::<PartRef>().is_ok(),
        "^wire:(0|[1-9][0-9]*)$" => |s| s.parse::<WireRef>().is_ok(),
        "^label:(0|[1-9][0-9]*)$" => |s| s.parse::<LabelRef>().is_ok(),
        "^(part|wire|label):(0|[1-9][0-9]*)$" => |s| s.parse::<ItemRef>().is_ok(),
        "^d(0|[1-9][0-9]*)$" => |s| s.parse::<DocId>().is_ok(),
        "^j(0|[1-9][0-9]*)$" => |s| s.parse::<JobId>().is_ok(),
        "^pv-[0-9a-f]{16}$" => |s| s.parse::<PreviewId>().is_ok(),
        _ => return None,
    })
}

fn type_matches(ty: &str, v: &Value) -> bool {
    match ty {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        "integer" => v.is_i64() || v.is_u64(),
        other => panic!("type {other}"),
    }
}

fn as_i128(v: &Value) -> Option<i128> {
    v.as_i64()
        .map(i128::from)
        .or_else(|| v.as_u64().map(i128::from))
}

fn validate(root: &Value, schema: &Value, v: &Value, at: &str) -> Result<(), String> {
    let map = match schema {
        Value::Bool(true) => return Ok(()),
        Value::Bool(false) => return Err(format!("{at}: no value is allowed")),
        Value::Object(map) => map,
        _ => unreachable!("audited"),
    };
    if let Some(r) = map.get("$ref") {
        let name = r.as_str().unwrap().strip_prefix("#/$defs/").unwrap();
        validate(root, &root["$defs"][name], v, at)?;
    }
    if let Some(ty) = map.get("type")
        && !type_matches(ty.as_str().unwrap(), v)
    {
        return Err(format!("{at}: not of type {ty}"));
    }
    if let Some(c) = map.get("const")
        && c != v
    {
        return Err(format!("{at}: not {c}"));
    }
    if let Some(e) = map.get("enum")
        && !e.as_array().unwrap().contains(v)
    {
        return Err(format!("{at}: {v} not in enum"));
    }
    if let (Some(min), Some(n)) = (map.get("minimum"), as_i128(v))
        && n < as_i128(min).unwrap()
    {
        return Err(format!("{at}: below minimum"));
    }
    if let (Some(max), Some(n)) = (map.get("maximum"), as_i128(v))
        && n > as_i128(max).unwrap()
    {
        return Err(format!("{at}: above maximum"));
    }
    if let (Some(p), Some(s)) = (map.get("pattern"), v.as_str())
        && !pattern_check(p.as_str().unwrap()).unwrap()(s)
    {
        return Err(format!("{at}: {s:?} does not match {p}"));
    }
    if let Some(obj) = v.as_object() {
        let props = map.get("properties").and_then(Value::as_object);
        if let Some(required) = map.get("required") {
            for name in required.as_array().unwrap() {
                if !obj.contains_key(name.as_str().unwrap()) {
                    return Err(format!("{at}: missing {name}"));
                }
            }
        }
        for (key, value) in obj {
            match props.and_then(|p| p.get(key)) {
                Some(s) => validate(root, s, value, &format!("{at}/{key}"))?,
                None => {
                    if let Some(extra) = map.get("additionalProperties") {
                        validate(root, extra, value, &format!("{at}/{key}"))?;
                    }
                }
            }
        }
    }
    if let Some(items) = v.as_array() {
        let prefix = map
            .get("prefixItems")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        for (i, item) in items.iter().enumerate() {
            let s = prefix.get(i).or_else(|| map.get("items"));
            if let Some(s) = s {
                validate(root, s, item, &format!("{at}/{i}"))?;
            }
        }
        let len = items.len() as u64;
        if map
            .get("minItems")
            .is_some_and(|m| len < m.as_u64().unwrap())
        {
            return Err(format!("{at}: too few items"));
        }
        if map
            .get("maxItems")
            .is_some_and(|m| len > m.as_u64().unwrap())
        {
            return Err(format!("{at}: too many items"));
        }
        if map.get("uniqueItems") == Some(&Value::Bool(true)) {
            for (i, a) in items.iter().enumerate() {
                if items[..i].contains(a) {
                    return Err(format!("{at}: repeated item"));
                }
            }
        }
    }
    if let Some(alternatives) = map.get("oneOf") {
        let passing = alternatives
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| validate(root, s, v, at).is_ok())
            .count();
        if passing != 1 {
            return Err(format!("{at}: {passing} oneOf alternatives match"));
        }
    }
    if let Some(alternatives) = map.get("anyOf")
        && !alternatives
            .as_array()
            .unwrap()
            .iter()
            .any(|s| validate(root, s, v, at).is_ok())
    {
        return Err(format!("{at}: no anyOf alternative matches"));
    }
    if let Some(not) = map.get("not")
        && validate(root, not, v, at).is_ok()
    {
        return Err(format!("{at}: matches `not`"));
    }
    Ok(())
}

fn schema_value(name: &str) -> &'static Value {
    static SCHEMAS: OnceLock<BTreeMap<String, Value>> = OnceLock::new();
    let schemas = SCHEMAS.get_or_init(|| {
        schema_files()
            .into_iter()
            .map(|(n, text)| (n, serde_json::from_str(&text).unwrap()))
            .collect()
    });
    schemas.get(name).unwrap_or_else(|| panic!("schema {name}"))
}

fn check(name: &str, v: &Value) {
    let schema = schema_value(name);
    if let Err(e) = validate(schema, schema, v, "") {
        panic!("AINV11: {v} does not validate against {name}: {e}");
    }
}

fn refuse(name: &str, v: &Value) {
    let schema = schema_value(name);
    assert!(
        validate(schema, schema, v, "").is_err(),
        "{name} accepts {v}"
    );
}

/// Compact JSON with object keys in reverse sorted order (AINV5: key order does not matter).
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

// ---- tests ---------------------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Every request DTO round-trips through JSON, validates against `request` and its
    /// `<op>.params`, and decodes the same with object keys in another order (AINV5, AINV11).
    #[test]
    fn requests_round_trip_and_validate(req in request()) {
        let text = req.to_json();
        prop_assert_eq!(&text.parse::<Request>().unwrap(), &req);
        let value: Value = serde_json::from_str(&text).unwrap();
        check("request", &value);
        check(&format!("{}.params", req.op.name()), &value["params"]);
        prop_assert_eq!(&reversed_keys(&value).parse::<Request>().unwrap(), &req);
        prop_assert_eq!(text, req.to_json());
    }

    /// Every response DTO round-trips through JSON and validates against `response` and its
    /// `<op>.response` (AINV11).
    #[test]
    fn responses_round_trip_and_validate((op, resp) in response()) {
        let text = resp.to_json();
        prop_assert_eq!(&Response::from_slice(op, text.as_bytes()).unwrap(), &resp);
        if let Ok(output) = &resp.outcome {
            prop_assert!(output.answers(op));
        }
        let value: Value = serde_json::from_str(&text).unwrap();
        check("response", &value);
        check(&format!("{op}.response"), &value);
        let reordered = reversed_keys(&value);
        prop_assert_eq!(&Response::from_slice(op, reordered.as_bytes()).unwrap(), &resp);
    }

    /// Every error kind round-trips and keeps its code.
    #[test]
    fn errors_round_trip(err in agent_error()) {
        let text = serde_json::to_string(&err).unwrap();
        let back: AgentError = serde_json::from_str(&text).unwrap();
        prop_assert_eq!(back.code(), err.code());
        prop_assert_eq!(back, err);
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
        assert!(
            names.contains(entry.params_schema.as_str()),
            "{}",
            entry.params_schema
        );
        assert!(
            names.contains(entry.response_schema.as_str()),
            "{}",
            entry.response_schema
        );
    }
    assert_eq!(names.len(), 2 + 2 * catalog.operations.len());
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
}

/// AINV11: the request decoder dispatches exactly the catalog's operations.
#[test]
fn ainv11_decoder_dispatches_every_catalog_operation() {
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    for &op in OpName::ALL {
        let sample = operation_for(op).new_tree(&mut runner).unwrap().current();
        assert_eq!(sample.name(), op);
        let req = Request {
            op: sample,
            idempotency_key: None,
        };
        assert_eq!(req.to_json().parse::<Request>().unwrap().op.name(), op);
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
    assert_eq!(violation_at(r#"{"op":"new","params":{"x":1}}"#), "/params");
    assert_eq!(
        violation_at(&format!(
            r#"{{"op":"nets","params":{{{doc},"at":[0,0],"z":0}}}}"#
        )),
        "/params"
    );
    assert_eq!(
        violation_at(&format!(
            r#"{{"op":"preview","params":{{{doc},"expected_revision":0,"operations":[{{"op":"delete","items":[]}},{{"op":"delete","items":[],"with_wires":true}}]}}}}"#
        )),
        "/params/operations/1",
        "with_wires arrives with A2.3"
    );
    assert_eq!(
        violation_at(&format!(
            r#"{{"op":"inspect","params":{{{doc},"include":["summary"],"area":{{"min":[0,0],"max":[1,1],"w":0}}}}}}"#
        )),
        "/params"
    );
    refuse("new.params", &serde_json::json!({ "x": 1 }));
    refuse(
        "request",
        &serde_json::json!({ "op": "new", "params": {}, "x": 1 }),
    );
    refuse(
        "preview.params",
        &serde_json::json!({ "doc_id": "d1", "expected_revision": 0,
            "operations": [{ "op": "delete", "items": [], "with_wires": true }] }),
    );
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
        r#"{"op":"new","params":{}} x"#,
    ] {
        assert_eq!(code_of(text).0, ErrorCode::InvalidRequest, "{text}");
    }
    assert_eq!(
        code_of(r#"{"op":"fly","params":{}}"#).0,
        ErrorCode::UnknownOperation
    );
    assert_eq!(violation_at(r#"{"params":{}}"#), "/op");
    assert_eq!(violation_at(r#"{"op":3,"params":{}}"#), "/op");
    assert_eq!(violation_at(r#"{"op":"new"}"#), "/params");
    assert_eq!(
        violation_at(
            r#"{"op":"inspect","params":{"doc_id":"d1","include":["summary"]},"idempotency_key":"k"}"#
        ),
        "/idempotency_key",
        "section 11.2: only apply, undo, redo and save take a key"
    );
    assert_eq!(
        violation_at(r#"{"op":"undo","params":{"doc_id":"d01","expected_revision":1}}"#),
        "/params"
    );
    assert_eq!(
        violation_at(r#"{"op":"inspect","params":{"doc_id":"d1","include":[]}}"#),
        "/params/include"
    );
    assert_eq!(
        violation_at(r#"{"op":"inspect","params":{"doc_id":"d1","include":["nets","nets"]}}"#),
        "/params/include"
    );
    assert_eq!(
        violation_at(r#"{"op":"inspect","params":{"doc_id":"d1","include":["items"],"limit":0}}"#),
        "/params/limit"
    );
    assert_eq!(
        violation_at(r#"{"op":"nets","params":{"doc_id":"d1","at":[0,0],"wire":"wire:1"}}"#),
        "/params"
    );
    assert_eq!(
        violation_at(
            r#"{"op":"preview","params":{"doc_id":"d1","expected_revision":0,"operations":[]}}"#
        ),
        "/params/operations"
    );
    assert_eq!(
        violation_at(
            r#"{"op":"preview","params":{"doc_id":"d1","expected_revision":0,"operations":[{"op":"move","items":["part:01"],"delta":[0,0]}]}}"#
        ),
        "/params/operations/0"
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
    v["error"]["details"] = serde_json::json!({});
    refuse("undo.response", &v);
    let mut v = good.clone();
    v["result"] = serde_json::json!({});
    refuse("undo.response", &v);
    let mut v = good;
    v["doc_id"] = "d01".into();
    refuse("undo.response", &v);

    let mv = serde_json::json!({ "doc_id": "d1", "expected_revision": 0,
        "operations": [{ "op": "move", "items": ["part:1"], "delta": [0, 0, 0] }] });
    refuse("apply.params", &mv);
    let mv = serde_json::json!({ "doc_id": "d1", "expected_revision": 0,
        "operations": [{ "op": "move", "items": ["part:01"], "delta": [0, 0] }] });
    refuse("apply.params", &mv);
    let mv = serde_json::json!({ "doc_id": "d1", "expected_revision": -1,
        "operations": [{ "op": "move", "items": ["part:1"], "delta": [0, 0] }] });
    refuse("apply.params", &mv);
}
