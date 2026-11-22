//! The hand-written JSON Schema module (mcp-plan.md section 9.5, AQ3, AINV11).
//!
//! Every schema is written by hand here, mirroring the serde shape of its DTO; the tests in
//! `schema/tests.rs` check that every serialized DTO validates against its schema and that the
//! files committed under `crates/omnia-agent/schema/` equal [`schema_files`].
//!
//! Output is deterministic: object keys are printed sorted by this module's own printer, so the
//! bytes do not depend on whether a build enables `serde_json`'s `preserve_order` feature.
//! Dialect: JSON Schema 2020-12.
//!
//! Files: `request` and `response` (every available operation), and per available operation
//! `<op>.params` (its `params` object in the request envelope), `<op>.arguments` (its MCP tool
//! arguments, the `inputSchema`: the params plus `idempotency_key` where section 11.2 allows it)
//! and `<op>.response` (its envelope, the MCP `outputSchema`). Operations whose milestone has
//! not been delivered ([`crate::OpSpec::available`]) get no committed file, but their schemas
//! are still generated in memory for the decoders.
//!
//! Ranges the codecs hold are in the schemas: id patterns spell `0..=u32::MAX` (items) and
//! `0..=u64::MAX` (documents, jobs) exactly, every `u64` has its maximum, and bounds corners lie
//! inside K1. Corner ordering is the one shape rule only the decoder checks (section 9.1).

use std::{collections::BTreeMap, sync::OnceLock};

use serde_json::{Value, json};

use crate::{
    EditOpName, ErrorCode, MAX_OPERATIONS, MAX_PAGE_SIZE, OpName, SCHEMA,
    catalog::spec,
    schema::{ORIENT_NAMES, TEXT_FIELD_NAMES, validate::decimal_up_to},
};

const DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// Collects named definitions for one schema file.
struct Gen {
    defs: BTreeMap<&'static str, Value>,
}

type Build = fn(&mut Gen) -> Value;

impl Gen {
    fn new() -> Self {
        Self {
            defs: BTreeMap::new(),
        }
    }

    /// A `$ref` to definition `name`, built once per file. Recursion sees the placeholder.
    fn r(&mut self, name: &'static str, build: Build) -> Value {
        if !self.defs.contains_key(name) {
            self.defs.insert(name, Value::Null);
            let body = build(self);
            self.defs.insert(name, body);
        }
        json!({ "$ref": format!("#/$defs/{name}") })
    }

    /// A complete file: `root` with the dialect, the title and the collected definitions.
    fn finish(self, mut root: Value, title: &str) -> Value {
        let map = root.as_object_mut().expect("schema roots are objects");
        map.insert("$schema".into(), DIALECT.into());
        map.insert("title".into(), title.into());
        if !self.defs.is_empty() {
            let defs = self
                .defs
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect();
            map.insert("$defs".into(), Value::Object(defs));
        }
        root
    }
}

// ---- building blocks -----------------------------------------------------------------------

fn string() -> Value {
    json!({ "type": "string" })
}

fn boolean() -> Value {
    json!({ "type": "boolean" })
}

fn uint(max: u64) -> Value {
    json!({ "type": "integer", "minimum": 0, "maximum": max })
}

fn u64_() -> Value {
    uint(u64::MAX)
}

fn u32_() -> Value {
    uint(u32::MAX.into())
}

fn i32_() -> Value {
    json!({ "type": "integer", "minimum": i32::MIN, "maximum": i32::MAX })
}

fn array(items: Value) -> Value {
    json!({ "type": "array", "items": items })
}

fn enum_of<'a>(names: impl IntoIterator<Item = &'a str>) -> Value {
    let names: Vec<&str> = names.into_iter().collect();
    json!({ "type": "string", "enum": names })
}

fn konst(v: impl Into<Value>) -> Value {
    json!({ "const": v.into() })
}

/// A closed object: `required` fields first in the list, all fields in `props`.
fn obj(props: Vec<(&str, Value)>, required: &[&str]) -> Value {
    let mut out = json!({
        "type": "object",
        "properties": props.into_iter().map(|(k, v)| (k.to_owned(), v)).collect::<serde_json::Map<_, _>>(),
        "additionalProperties": false,
    });
    if !required.is_empty() {
        out["required"] = json!(required);
    }
    out
}

fn described(mut schema: Value, text: &str) -> Value {
    schema["description"] = text.into();
    schema
}

fn one_of(alternatives: Vec<Value>) -> Value {
    json!({ "oneOf": alternatives })
}

/// One alternative of [`tagged`]: tag value, properties, required properties.
type Variant<'a> = (&'a str, Vec<(&'a str, Value)>, &'a [&'a str]);

/// An internally tagged union: each alternative is `obj` plus `tag: const`.
fn tagged(tag: &str, variants: Vec<Variant<'_>>) -> Value {
    one_of(
        variants
            .into_iter()
            .map(|(name, mut props, required)| {
                props.insert(0, (tag, konst(name)));
                let mut req = vec![tag];
                req.extend_from_slice(required);
                obj(props, &req)
            })
            .collect(),
    )
}

/// A string `<prefix><n>` with `n` in canonical decimal, `0..=max`.
fn id_pattern(prefix: &str, max: u64, what: &str) -> Value {
    let pattern = format!("^{prefix}{}$", decimal_up_to(max));
    described(json!({ "type": "string", "pattern": pattern }), what)
}

const ITEM_MAX: u64 = u32::MAX as u64;

// ---- codecs --------------------------------------------------------------------------------

fn part_ref(_: &mut Gen) -> Value {
    id_pattern("part:", ITEM_MAX, "A part id, 0-based (AQ5).")
}

fn wire_ref(_: &mut Gen) -> Value {
    id_pattern("wire:", ITEM_MAX, "A wire id, 0-based (AQ5).")
}

fn label_ref(_: &mut Gen) -> Value {
    id_pattern("label:", ITEM_MAX, "A label id, 0-based (AQ5).")
}

fn item_ref(_: &mut Gen) -> Value {
    id_pattern("(part|wire|label):", ITEM_MAX, "An item id, 0-based (AQ5).")
}

fn doc_id(_: &mut Gen) -> Value {
    id_pattern(
        "d",
        u64::MAX,
        "A document id, never reused within a session.",
    )
}

fn job_id(_: &mut Gen) -> Value {
    id_pattern("j", u64::MAX, "A job id.")
}

fn preview_id(_: &mut Gen) -> Value {
    described(
        json!({ "type": "string", "pattern": "^pv-[0-9a-f]{16}$" }),
        "A 64-bit preview hash (section 11.3); not a security token.",
    )
}

fn point(_: &mut Gen) -> Value {
    described(
        json!({
            "type": "array",
            "prefixItems": [i32_(), i32_()],
            "items": false,
            "minItems": 2,
            "maxItems": 2,
        }),
        "[x, y] in integer nanometres (AK8).",
    )
}

fn sheet_point(_: &mut Gen) -> Value {
    let limit = omnia_doc::SHEET_LIMIT_NM;
    let coord = json!({ "type": "integer", "minimum": -limit, "maximum": limit });
    described(
        json!({
            "type": "array",
            "prefixItems": [coord.clone(), coord],
            "items": false,
            "minItems": 2,
            "maxItems": 2,
        }),
        "[x, y] in integer nanometres inside the K1 sheet limits.",
    )
}

fn bounds(g: &mut Gen) -> Value {
    let p = g.r("SheetPoint", sheet_point);
    described(
        obj(vec![("min", p.clone()), ("max", p)], &["min", "max"]),
        "An axis-aligned rectangle inside the sheet limits. The decoder also requires \
         min <= max on both axes (mcp-plan.md section 9.1).",
    )
}

fn orient(_: &mut Gen) -> Value {
    described(enum_of(ORIENT_NAMES), "An orientation (AD12).")
}

fn text_field(_: &mut Gen) -> Value {
    enum_of(TEXT_FIELD_NAMES)
}

// ---- requests ------------------------------------------------------------------------------

fn no_args(_: &mut Gen) -> Value {
    obj(vec![], &[])
}

fn open_args(_: &mut Gen) -> Value {
    obj(vec![("path", string())], &["path"])
}

fn approval_echo(_: &mut Gen) -> Value {
    obj(
        vec![("approval_id", string()), ("effect_digest", string())],
        &["approval_id", "effect_digest"],
    )
}

fn close_args(g: &mut Gen) -> Value {
    obj(
        vec![
            ("doc_id", g.r("DocId", doc_id)),
            ("discard_unsaved", boolean()),
            ("approval", g.r("ApprovalEcho", approval_echo)),
        ],
        &["doc_id"],
    )
}

fn page_limit() -> Value {
    json!({ "type": "integer", "minimum": 1, "maximum": MAX_PAGE_SIZE })
}

fn inspect_args(g: &mut Gen) -> Value {
    let include = json!({
        "type": "array",
        "items": enum_of(["summary", "items", "nets"]),
        "minItems": 1,
        "uniqueItems": true,
    });
    obj(
        vec![
            ("doc_id", g.r("DocId", doc_id)),
            ("include", include),
            ("area", g.r("Bounds", bounds)),
            ("cursor", string()),
            ("limit", page_limit()),
        ],
        &["doc_id", "include"],
    )
}

fn query_args(g: &mut Gen) -> Value {
    obj(
        vec![
            ("doc_id", g.r("DocId", doc_id)),
            ("area", g.r("Bounds", bounds)),
            ("mode", enum_of(["envelope", "within", "touching"])),
            ("cursor", string()),
            ("limit", page_limit()),
        ],
        &["doc_id", "area", "mode"],
    )
}

fn nets_args(g: &mut Gen) -> Value {
    let mut s = obj(
        vec![
            ("doc_id", g.r("DocId", doc_id)),
            ("at", g.r("Point", point)),
            ("wire", g.r("WireRef", wire_ref)),
        ],
        &["doc_id"],
    );
    s["not"] = json!({ "required": ["at", "wire"] });
    s
}

fn edit_op(g: &mut Gen) -> Value {
    let p = g.r("Point", point);
    let o = g.r("Orient", orient);
    let item = g.r("ItemRef", item_ref);
    let items = array(item.clone());
    let wire = json!({
        "type": "array",
        "prefixItems": [p.clone(), p.clone()],
        "items": false,
        "minItems": 2,
        "maxItems": 2,
    });
    let variants = EditOpName::ALL
        .iter()
        .map(|&op| -> Variant<'_> {
            match op {
                EditOpName::PlacePart => (
                    op.as_str(),
                    vec![
                        ("symbol", string()),
                        ("at", p.clone()),
                        ("orient", o.clone()),
                        ("designator", string()),
                        ("value", string()),
                    ],
                    &["symbol", "at", "orient", "designator", "value"],
                ),
                EditOpName::AddWires => (
                    op.as_str(),
                    vec![("wires", array(wire.clone()))],
                    &["wires"],
                ),
                EditOpName::AddLabel => (
                    op.as_str(),
                    vec![("at", p.clone()), ("text", string())],
                    &["at", "text"],
                ),
                EditOpName::Move => (
                    op.as_str(),
                    vec![("items", items.clone()), ("delta", p.clone())],
                    &["items", "delta"],
                ),
                EditOpName::Reorient => (
                    op.as_str(),
                    vec![
                        ("items", items.clone()),
                        ("pivot", p.clone()),
                        ("by", o.clone()),
                    ],
                    &["items", "pivot", "by"],
                ),
                EditOpName::Delete => (op.as_str(), vec![("items", items.clone())], &["items"]),
                EditOpName::SetText => (
                    op.as_str(),
                    vec![
                        ("item", item.clone()),
                        ("field", text_field(g)),
                        ("text", string()),
                    ],
                    &["item", "field", "text"],
                ),
            }
        })
        .collect();
    tagged("op", variants)
}

fn operations(g: &mut Gen) -> Value {
    json!({
        "type": "array",
        "items": g.r("EditOp", edit_op),
        "minItems": 1,
        "maxItems": MAX_OPERATIONS,
    })
}

fn edit_args(g: &mut Gen) -> Value {
    obj(
        vec![
            ("doc_id", g.r("DocId", doc_id)),
            ("expected_revision", u64_()),
            ("operations", operations(g)),
        ],
        &["doc_id", "expected_revision", "operations"],
    )
}

fn apply_args(g: &mut Gen) -> Value {
    obj(
        vec![
            ("doc_id", g.r("DocId", doc_id)),
            ("expected_revision", u64_()),
            ("operations", operations(g)),
            ("preview_id", g.r("PreviewId", preview_id)),
        ],
        &["doc_id", "expected_revision", "operations"],
    )
}

fn rev_args(g: &mut Gen) -> Value {
    obj(
        vec![
            ("doc_id", g.r("DocId", doc_id)),
            ("expected_revision", u64_()),
        ],
        &["doc_id", "expected_revision"],
    )
}

fn save_args(g: &mut Gen) -> Value {
    obj(
        vec![
            ("doc_id", g.r("DocId", doc_id)),
            ("path", string()),
            ("approval", g.r("ApprovalEcho", approval_echo)),
        ],
        &["doc_id"],
    )
}

fn key_args(_: &mut Gen) -> Value {
    obj(vec![("idempotency_key", string())], &["idempotency_key"])
}

fn job_args(g: &mut Gen) -> Value {
    obj(vec![("job_id", g.r("JobId", job_id))], &["job_id"])
}

/// The params definition of `op`. Exhaustive, with no wildcard arm.
const fn params_def(op: OpName) -> (&'static str, Build) {
    match op {
        OpName::Capabilities | OpName::New => ("NoArgs", no_args),
        OpName::Open => ("OpenArgs", open_args),
        OpName::Close => ("CloseArgs", close_args),
        OpName::Inspect => ("InspectArgs", inspect_args),
        OpName::Query => ("QueryArgs", query_args),
        OpName::Nets => ("NetsArgs", nets_args),
        OpName::Validate | OpName::Preview => ("EditArgs", edit_args),
        OpName::Apply => ("ApplyArgs", apply_args),
        OpName::Undo | OpName::Redo => ("RevArgs", rev_args),
        OpName::Save => ("SaveArgs", save_args),
        OpName::RequestStatus => ("KeyArgs", key_args),
        OpName::JobStatus | OpName::JobCancel => ("JobArgs", job_args),
    }
}

fn request(g: &mut Gen) -> Value {
    let alternatives = OpName::ALL
        .iter()
        .filter(|&&op| spec(op).available)
        .map(|&op| {
            let (name, build) = params_def(op);
            let mut props = vec![("op", konst(op.as_str())), ("params", g.r(name, build))];
            if spec(op).idempotency_key {
                props.push(("idempotency_key", string()));
            }
            obj(props, &["op", "params"])
        })
        .collect();
    let mut s = one_of(alternatives);
    s["type"] = "object".into();
    s
}

// ---- errors --------------------------------------------------------------------------------

fn blocker(g: &mut Gen) -> Value {
    let w = g.r("WireRef", wire_ref);
    tagged(
        "kind",
        vec![
            (
                "pin",
                vec![("part", g.r("PartRef", part_ref)), ("pin", u32_())],
                &["part", "pin"],
            ),
            ("wire_end", vec![("wire", w.clone())], &["wire"]),
            ("wire_body", vec![("wire", w.clone())], &["wire"]),
            ("overlap", vec![("wire", w)], &["wire"]),
            ("sheet", vec![], &[]),
        ],
    )
}

/// The `details` schema of `code`. Exhaustive, with no wildcard arm.
fn details(g: &mut Gen, code: ErrorCode) -> Value {
    let item = |g: &mut Gen| obj(vec![("item", g.r("ItemRef", item_ref))], &["item"]);
    match code {
        ErrorCode::SchemaViolation => obj(vec![("path", string())], &["path"]),
        ErrorCode::LimitExceeded => obj(
            vec![
                (
                    "limit",
                    enum_of([
                        "request_bytes",
                        "file_bytes",
                        "operations",
                        "page_size",
                        "open_documents",
                    ]),
                ),
                ("value", u64_()),
            ],
            &["limit", "value"],
        ),
        ErrorCode::MissingItem | ErrorCode::DuplicateItem => item(g),
        ErrorCode::InvalidTextField => obj(
            vec![("item", g.r("ItemRef", item_ref)), ("field", text_field(g))],
            &["item", "field"],
        ),
        ErrorCode::FollowNoRoute => obj(
            vec![
                ("from", g.r("Point", point)),
                ("to", g.r("Point", point)),
                ("blocker", g.r("Blocker", blocker)),
            ],
            &["from", "to", "blocker"],
        ),
        ErrorCode::SegmentBlocked => obj(vec![("blocker", g.r("Blocker", blocker))], &["blocker"]),
        ErrorCode::WireCapture => obj(
            vec![
                ("wire", g.r("WireRef", wire_ref)),
                ("blocker", g.r("Blocker", blocker)),
            ],
            &["wire", "blocker"],
        ),
        ErrorCode::OffLattice => obj(vec![("at", g.r("Point", point))], &["at"]),
        ErrorCode::StaleRevision => obj(
            vec![("expected", u64_()), ("actual", u64_())],
            &["expected", "actual"],
        ),
        ErrorCode::ApprovalRequired => obj(
            vec![
                ("approval_id", string()),
                ("tier", uint(u8::MAX.into())),
                (
                    "effect",
                    enum_of(["edit", "write_file", "overwrite_file", "discard_unsaved"]),
                ),
                ("effect_digest", string()),
            ],
            &["approval_id", "tier", "effect", "effect_digest"],
        ),
        ErrorCode::InvalidRequest
        | ErrorCode::UnknownOperation
        | ErrorCode::UnsupportedInMode
        | ErrorCode::ProtocolVersionUnsupported
        | ErrorCode::DocumentNotFound
        | ErrorCode::UnknownSymbol
        | ErrorCode::UnknownItem
        | ErrorCode::OutOfBounds
        | ErrorCode::InvalidWire
        | ErrorCode::PreviewMismatch
        | ErrorCode::IdempotencyConflict
        | ErrorCode::UserGestureInProgress
        | ErrorCode::NothingToUndo
        | ErrorCode::NothingToRedo
        | ErrorCode::ApprovalDenied
        | ErrorCode::PathOutsideRoot
        | ErrorCode::FileExists
        | ErrorCode::IoError
        | ErrorCode::UnsupportedFileVersion
        | ErrorCode::Cancelled
        | ErrorCode::JobNotFound
        | ErrorCode::Internal => obj(vec![], &[]),
    }
}

fn error_code(_: &mut Gen) -> Value {
    enum_of(ErrorCode::ALL.iter().map(|c| c.as_str()))
}

fn agent_error(g: &mut Gen) -> Value {
    let mut s = obj(
        vec![
            ("code", g.r("ErrorCode", error_code)),
            ("message", string()),
            ("retryable", boolean()),
            ("details", json!({ "type": "object" })),
            ("hint", string()),
        ],
        &["code", "message", "retryable", "details"],
    );
    let by_code = ErrorCode::ALL
        .iter()
        .map(|&code| {
            json!({
                "properties": {
                    "code": konst(code.as_str()),
                    "retryable": konst(code.retryable()),
                    "details": details(g, code),
                }
            })
        })
        .collect();
    s["oneOf"] = Value::Array(by_code);
    described(s, "A structured error (AD6, section 9.4).")
}

// ---- results -------------------------------------------------------------------------------

fn warning(_: &mut Gen) -> Value {
    obj(
        vec![("code", string()), ("message", string())],
        &["code", "message"],
    )
}

fn doc_info(_: &mut Gen) -> Value {
    obj(vec![("path", string()), ("dirty", boolean())], &["dirty"])
}

fn close_result(_: &mut Gen) -> Value {
    obj(vec![("discarded", boolean())], &["discarded"])
}

fn summary(_: &mut Gen) -> Value {
    obj(
        vec![("parts", u64_()), ("wires", u64_()), ("labels", u64_())],
        &["parts", "wires", "labels"],
    )
}

fn part_body(g: &mut Gen) -> Value {
    obj(
        vec![
            ("symbol", string()),
            ("at", g.r("Point", point)),
            ("orient", g.r("Orient", orient)),
            ("designator", string()),
            ("value", string()),
        ],
        &["symbol", "at", "orient", "designator", "value"],
    )
}

fn wire_body(g: &mut Gen) -> Value {
    let p = g.r("Point", point);
    obj(vec![("a", p.clone()), ("b", p)], &["a", "b"])
}

fn label_body(g: &mut Gen) -> Value {
    obj(
        vec![("at", g.r("Point", point)), ("text", string())],
        &["at", "text"],
    )
}

fn item(g: &mut Gen) -> Value {
    one_of(vec![
        obj(
            vec![
                ("id", g.r("PartRef", part_ref)),
                ("part", g.r("PartBody", part_body)),
            ],
            &["id", "part"],
        ),
        obj(
            vec![
                ("id", g.r("WireRef", wire_ref)),
                ("wire", g.r("WireBody", wire_body)),
            ],
            &["id", "wire"],
        ),
        obj(
            vec![
                ("id", g.r("LabelRef", label_ref)),
                ("label", g.r("LabelBody", label_body)),
            ],
            &["id", "label"],
        ),
    ])
}

fn items_page(g: &mut Gen) -> Value {
    obj(
        vec![
            ("items", array(g.r("Item", item))),
            ("next_cursor", string()),
        ],
        &["items"],
    )
}

fn pin_ref(g: &mut Gen) -> Value {
    obj(
        vec![("part", g.r("PartRef", part_ref)), ("pin", u32_())],
        &["part", "pin"],
    )
}

fn net_key(g: &mut Gen) -> Value {
    described(
        tagged(
            "kind",
            vec![
                (
                    "pin",
                    vec![("part", g.r("PartRef", part_ref)), ("pin", u32_())],
                    &["part", "pin"],
                ),
                ("wire", vec![("wire", g.r("WireRef", wire_ref))], &["wire"]),
                (
                    "label",
                    vec![("label", g.r("LabelRef", label_ref))],
                    &["label"],
                ),
            ],
        ),
        "Lowest pin, else lowest wire, else lowest label (AD8).",
    )
}

fn net(g: &mut Gen) -> Value {
    obj(
        vec![
            ("key", g.r("NetKey", net_key)),
            ("pins", array(g.r("PinRef", pin_ref))),
            ("wires", array(g.r("WireRef", wire_ref))),
            ("labels", array(g.r("LabelRef", label_ref))),
        ],
        &["key", "pins", "wires", "labels"],
    )
}

fn inspect_result(g: &mut Gen) -> Value {
    obj(
        vec![
            ("summary", g.r("Summary", summary)),
            ("items", g.r("ItemsPage", items_page)),
            ("nets", array(g.r("Net", net))),
        ],
        &[],
    )
}

fn nets_result(g: &mut Gen) -> Value {
    obj(vec![("nets", array(g.r("Net", net)))], &["nets"])
}

fn change(g: &mut Gen) -> Value {
    let ids = array(g.r("ItemRef", item_ref));
    obj(
        vec![
            ("revision", u64_()),
            ("dirty", g.r("Bounds", bounds)),
            ("added", ids.clone()),
            ("removed", ids),
            ("nets_changed", boolean()),
        ],
        &["revision", "added", "removed", "nets_changed"],
    )
}

fn modified(g: &mut Gen) -> Value {
    let pair = |id: Value, body: Value| {
        obj(
            vec![("id", id), ("before", body.clone()), ("after", body)],
            &["id", "before", "after"],
        )
    };
    one_of(vec![
        pair(g.r("PartRef", part_ref), g.r("PartBody", part_body)),
        pair(g.r("WireRef", wire_ref), g.r("WireBody", wire_body)),
        pair(g.r("LabelRef", label_ref), g.r("LabelBody", label_body)),
    ])
}

fn diff(g: &mut Gen) -> Value {
    let items = array(g.r("Item", item));
    obj(
        vec![
            ("added", items.clone()),
            ("removed", items),
            ("modified", array(g.r("Modified", modified))),
            ("truncated", boolean()),
        ],
        &["added", "removed", "modified", "truncated"],
    )
}

fn validate_result(g: &mut Gen) -> Value {
    obj(
        vec![("base_revision", u64_()), ("change", g.r("Change", change))],
        &["base_revision", "change"],
    )
}

fn preview_result(g: &mut Gen) -> Value {
    obj(
        vec![
            ("preview_id", g.r("PreviewId", preview_id)),
            ("base_revision", u64_()),
            ("change", g.r("Change", change)),
            ("diff", g.r("Diff", diff)),
        ],
        &["preview_id", "base_revision", "change", "diff"],
    )
}

fn edit_result(g: &mut Gen) -> Value {
    obj(
        vec![
            ("change", g.r("Change", change)),
            ("diff", g.r("Diff", diff)),
        ],
        &["change", "diff"],
    )
}

fn change_result(g: &mut Gen) -> Value {
    obj(vec![("change", g.r("Change", change))], &["change"])
}

fn save_result(_: &mut Gen) -> Value {
    obj(
        vec![("path", string()), ("saved_revision", u64_())],
        &["path", "saved_revision"],
    )
}

fn op_name(_: &mut Gen) -> Value {
    enum_of(OpName::ALL.iter().map(|op| op.as_str()))
}

/// `applied` names the operation of the stored response in `op`: one of the operations that
/// take an idempotency key (section 11.2), and the response is a success of that operation.
fn request_status(g: &mut Gen) -> Value {
    let mut alternatives = vec![obj(vec![("status", konst("not_run"))], &["status"])];
    for &op in OpName::ALL {
        if let Some((name, build)) = success_def(op) {
            alternatives.push(obj(
                vec![
                    ("status", konst("applied")),
                    ("op", konst(op.as_str())),
                    ("revision", u64_()),
                    ("response", g.r(name, build)),
                ],
                &["status", "op", "revision", "response"],
            ));
        }
    }
    alternatives.push(obj(
        vec![
            ("status", konst("failed")),
            ("error", g.r("Error", agent_error)),
        ],
        &["status", "error"],
    ));
    one_of(alternatives)
}

fn progress(_: &mut Gen) -> Value {
    obj(
        vec![
            ("completed", u64_()),
            ("total", u64_()),
            ("unit", string()),
            ("message", string()),
        ],
        &["completed", "unit"],
    )
}

fn job_state(g: &mut Gen) -> Value {
    obj(
        vec![
            ("job_id", g.r("JobId", job_id)),
            (
                "state",
                enum_of([
                    "queued",
                    "running",
                    "succeeded",
                    "failed",
                    "cancelled_by_client",
                    "cancelled_by_system",
                ]),
            ),
            ("progress", g.r("Progress", progress)),
            ("error", g.r("Error", agent_error)),
        ],
        &["job_id", "state"],
    )
}

fn catalog(g: &mut Gen) -> Value {
    let strings = array(string());
    let units = obj(
        vec![
            ("grid_nm", u32_()),
            ("pin_lattice_nm", u32_()),
            ("sheet_limit_nm", u32_()),
        ],
        &["grid_nm", "pin_lattice_nm", "sheet_limit_nm"],
    );
    let limits = obj(
        vec![
            ("request_bytes", u64_()),
            ("file_bytes", u64_()),
            ("operations_per_request", u64_()),
            ("default_page_size", u32_()),
            ("max_page_size", u32_()),
            ("idempotency_cache_entries", u32_()),
            ("open_documents", u32_()),
        ],
        &[
            "request_bytes",
            "file_bytes",
            "operations_per_request",
            "default_page_size",
            "max_page_size",
            "idempotency_cache_entries",
            "open_documents",
        ],
    );
    let operation = obj(
        vec![
            ("name", g.r("OpName", op_name)),
            ("tool", string()),
            ("tiers", array(uint(u8::MAX.into()))),
            ("read_only", boolean()),
            ("destructive", boolean()),
            ("idempotent", boolean()),
            ("idempotency_key", boolean()),
            ("params_schema", string()),
            ("arguments_schema", string()),
            ("response_schema", string()),
            ("description", string()),
        ],
        &[
            "name",
            "tool",
            "tiers",
            "read_only",
            "destructive",
            "idempotent",
            "idempotency_key",
            "params_schema",
            "arguments_schema",
            "response_schema",
            "description",
        ],
    );
    let edit_operation = obj(
        vec![
            (
                "name",
                enum_of(EditOpName::ALL.iter().map(|op| op.as_str())),
            ),
            ("command", string()),
            ("description", string()),
        ],
        &["name", "command", "description"],
    );
    let error_code = obj(
        vec![
            ("code", g.r("ErrorCode", error_code)),
            ("retryable", boolean()),
        ],
        &["code", "retryable"],
    );
    obj(
        vec![
            ("protocol_revisions", strings.clone()),
            ("units", units),
            ("limits", limits),
            ("operations", array(operation)),
            ("edit_operations", array(edit_operation)),
            ("orientations", strings.clone()),
            ("text_fields", strings),
            ("error_codes", array(error_code)),
        ],
        &[
            "protocol_revisions",
            "units",
            "limits",
            "operations",
            "edit_operations",
            "orientations",
            "text_fields",
            "error_codes",
        ],
    )
}

/// The result definition of `op`. Exhaustive, with no wildcard arm.
const fn result_def(op: OpName) -> (&'static str, Build) {
    match op {
        OpName::Capabilities => ("Catalog", catalog),
        OpName::New | OpName::Open => ("DocInfo", doc_info),
        OpName::Close => ("CloseResult", close_result),
        OpName::Inspect => ("InspectResult", inspect_result),
        OpName::Query => ("ItemsPage", items_page),
        OpName::Nets => ("NetsResult", nets_result),
        OpName::Validate => ("ValidateResult", validate_result),
        OpName::Preview => ("PreviewResult", preview_result),
        OpName::Apply => ("EditResult", edit_result),
        OpName::Undo | OpName::Redo => ("ChangeResult", change_result),
        OpName::Save => ("SaveResult", save_result),
        OpName::RequestStatus => ("RequestStatus", request_status),
        OpName::JobStatus | OpName::JobCancel => ("JobState", job_state),
    }
}

/// The envelope of section 9.1 with `result` as given.
fn envelope(g: &mut Gen, result: Value) -> Value {
    let mut s = obj(
        vec![
            ("schema", konst(SCHEMA)),
            ("doc_id", g.r("DocId", doc_id)),
            ("revision", u64_()),
            ("result", result),
            ("error", g.r("Error", agent_error)),
            ("warnings", array(g.r("Warning", warning))),
        ],
        &["schema", "warnings"],
    );
    s["oneOf"] = json!([{ "required": ["result"] }, { "required": ["error"] }]);
    s
}

/// The envelope of a successful response to `op`: `result` required, no `error`.
fn success(g: &mut Gen, op: OpName) -> Value {
    let (name, build) = result_def(op);
    obj(
        vec![
            ("schema", konst(SCHEMA)),
            ("doc_id", g.r("DocId", doc_id)),
            ("revision", u64_()),
            ("result", g.r(name, build)),
            ("warnings", array(g.r("Warning", warning))),
        ],
        &["schema", "result", "warnings"],
    )
}

fn apply_success(g: &mut Gen) -> Value {
    success(g, OpName::Apply)
}

fn undo_success(g: &mut Gen) -> Value {
    success(g, OpName::Undo)
}

fn redo_success(g: &mut Gen) -> Value {
    success(g, OpName::Redo)
}

fn save_success(g: &mut Gen) -> Value {
    success(g, OpName::Save)
}

/// The stored-success definition of an operation that takes an idempotency key. Exhaustive;
/// a test checks it agrees with `spec(op).idempotency_key`.
const fn success_def(op: OpName) -> Option<(&'static str, Build)> {
    match op {
        OpName::Apply => Some(("ApplySuccess", apply_success)),
        OpName::Undo => Some(("UndoSuccess", undo_success)),
        OpName::Redo => Some(("RedoSuccess", redo_success)),
        OpName::Save => Some(("SaveSuccess", save_success)),
        OpName::Capabilities
        | OpName::New
        | OpName::Open
        | OpName::Close
        | OpName::Inspect
        | OpName::Query
        | OpName::Nets
        | OpName::Validate
        | OpName::Preview
        | OpName::RequestStatus
        | OpName::JobStatus
        | OpName::JobCancel => None,
    }
}

/// The envelope of any available operation. `anyOf`, because two results may share a shape.
fn response_any(g: &mut Gen) -> Value {
    let mut seen = Vec::new();
    let mut results = Vec::new();
    for &op in OpName::ALL.iter().filter(|&&op| spec(op).available) {
        let (name, build) = result_def(op);
        if !seen.contains(&name) {
            seen.push(name);
            results.push(g.r(name, build));
        }
    }
    envelope(g, json!({ "anyOf": results }))
}

// ---- files ---------------------------------------------------------------------------------

/// What one per-operation file describes.
#[derive(Clone, Copy)]
enum Part {
    Params,
    Arguments,
    Response,
}

impl Part {
    const ALL: [Part; 3] = [Part::Params, Part::Arguments, Part::Response];

    const fn suffix(self) -> &'static str {
        match self {
            Part::Params => "params",
            Part::Arguments => "arguments",
            Part::Response => "response",
        }
    }
}

fn op_file(op: OpName, part: Part) -> Value {
    let mut g = Gen::new();
    let root = match part {
        Part::Params => params_def(op).1(&mut g),
        Part::Arguments => {
            let mut root = params_def(op).1(&mut g);
            if spec(op).idempotency_key {
                root["properties"]["idempotency_key"] = string();
            }
            root
        }
        Part::Response => {
            let (name, build) = result_def(op);
            let result = g.r(name, build);
            envelope(&mut g, result)
        }
    };
    g.finish(root, &format!("{op}.{}", part.suffix()))
}

fn file(title: &str, build: Build) -> Value {
    let mut g = Gen::new();
    let root = build(&mut g);
    g.finish(root, title)
}

/// Every schema, generated once: the published files and those of unavailable operations,
/// each with whether it is published.
fn registry() -> &'static BTreeMap<String, (bool, Value)> {
    static REGISTRY: OnceLock<BTreeMap<String, (bool, Value)>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut files = BTreeMap::new();
        files.insert("request".to_owned(), (true, file("request", request)));
        files.insert(
            "response".to_owned(),
            (true, file("response", response_any)),
        );
        for &op in OpName::ALL {
            for part in Part::ALL {
                let name = format!("{op}.{}", part.suffix());
                files.insert(name, (spec(op).available, op_file(op, part)));
            }
        }
        files
    })
}

/// The schema named `name` as a value, published or not; the decoders validate against it.
pub(crate) fn schema_value(name: &str) -> Option<&'static Value> {
    registry().get(name).map(|(_, v)| v)
}

/// Every published schema file as `(name, JSON text)`, sorted by name: the committed files.
pub fn schema_files() -> Vec<(String, String)> {
    registry()
        .iter()
        .filter(|(_, (published, _))| *published)
        .map(|(name, (_, value))| (name.clone(), to_canonical_json(value)))
        .collect()
}

/// The published JSON Schema named `name` (`request`, `response`, `<op>.params`,
/// `<op>.arguments` or `<op>.response`).
pub fn schema_text(name: &str) -> Option<String> {
    match registry().get(name) {
        Some((true, value)) => Some(to_canonical_json(value)),
        _ => None,
    }
}

/// Pretty JSON with object keys sorted and a final newline, independent of map ordering.
pub fn to_canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, 0, &mut out);
    out.push('\n');
    out
}

fn write_value(value: &Value, indent: usize, out: &mut String) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push_str("{\n");
            for (i, key) in keys.iter().enumerate() {
                push_indent(indent + 1, out);
                out.push_str(&serde_json::to_string(key).expect("strings serialize"));
                out.push_str(": ");
                write_value(&map[key.as_str()], indent + 1, out);
                if i + 1 < keys.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(indent, out);
            out.push('}');
        }
        Value::Array(items) if !items.is_empty() => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                push_indent(indent + 1, out);
                write_value(item, indent + 1, out);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            push_indent(indent, out);
            out.push(']');
        }
        Value::Object(_) => out.push_str("{}"),
        Value::Array(_) => out.push_str("[]"),
        scalar => out.push_str(&serde_json::to_string(scalar).expect("scalars serialize")),
    }
}

fn push_indent(level: usize, out: &mut String) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `request_status` stores exactly the operations that take an idempotency key (11.2).
    #[test]
    fn stored_successes_are_the_keyed_operations() {
        for &op in OpName::ALL {
            assert_eq!(success_def(op).is_some(), spec(op).idempotency_key, "{op}");
        }
    }
}
