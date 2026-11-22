# Agent control: plan (reference)

This file is the reference for controlling Omnia from agents and scripts: the
transport-neutral agent service, the Model Context Protocol (MCP) server, the command line
and JSON-lines surfaces, their schemas, semantics, security and tests. The work itself is in
[mcp-roadmap.md](mcp-roadmap.md), which lists tasks that cite the ids defined here.

It extends [canvas-plan.md](canvas-plan.md) and [wiring-plan.md](wiring-plan.md) and does not
replace them. Every `B`, `K`, `G`, `D`, `INV`, `Q` id and every `W`-prefixed id is defined
there and still holds. Section 17 lists the changes this plan makes to other files; task A0.1
applied them.

Written 2026-11-17, against `main` at `f4fd588`. No agent code exists yet.

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `AB` | Budget for the agent surface, a hard requirement | 3 |
| `AK` | Constant every agent crate must agree on | 4 |
| `AD` | Agent decision | 6 |
| `AINV` | Invariant: a rule that must hold in code, each with a test | 7 |
| `ATC` | Scenario with its expected result | 8 |
| `AQ` | Open question for the owner, each with a default | 16 |
| `HA` | Human checkpoint | [mcp-roadmap.md](mcp-roadmap.md) |
| `A0.1`, `A1.1`… | Task | [mcp-roadmap.md](mcp-roadmap.md) |

Status words are the canvas plan's (**DECIDED**, **DEFAULT**, **HYPOTHESIS**), the wiring
plan's **GATED**, and one more:

- **PROPOSED**: written by the plan author. Becomes DECIDED, or is amended, at HA1. Build
  nothing from a PROPOSED entry before HA1. HA1 was held on 2026-11-20 (A0.2); no entry is
  PROPOSED now.

The Model Context Protocol is named here as an open protocol standard, as Vulkan and Wayland
are named in the canvas plan (AQ8). No other company or product is named.

## 1. Scope

In scope:

- One transport-neutral agent service (`omnia-agent`) that reads, validates, previews,
  applies, undoes, opens and saves documents.
- Adapters for it: an MCP server over stdio, a one-shot JSON command line and a JSON-lines
  session (`omnia-cli`, binary `omnia`).
- Capability discovery, typed schemas, structured errors, approvals for destructive effects,
  idempotency, optimistic concurrency, jobs with progress and cancellation.
- Later, the same service hosted inside the running app on the document a person is editing.
- A tested contract document for agents (`docs/agent/`).

Out of scope:

- Simulation tools. There is no simulation plan yet (ROADMAP stage 3); the job model in
  section 11 is designed so simulation runs can use it later.
- GUI input automation: synthetic clicks, drags, keys or window screenshots (AD9).
- A network transport, until HA3 (section 12.3).
- Product file formats. Agents open and save the RON v2 test sheet only (`Doc::save`,
  `Doc::load`), with no compatibility promise (AQ12).
- Anything that edits plans, roadmaps or measurements, runs gates or answers a human
  checkpoint (AD10).

## 2. Repository facts this plan rests on

Checked at `f4fd588`.

| Fact | Where | Consequence |
| --- | --- | --- |
| `Doc::apply`, `undo` and `redo` are the only public mutators; fields and raw inserts are `pub(crate)` | `crates/omnia-doc/src/doc.rs` | A separate crate cannot bypass `Doc::apply`. INV4 holds for the agent surface by construction. |
| `apply` is transactional; a failing `Batch` rolls back and truncates the slots it reserved | `crates/omnia-doc/src/command.rs` (`apply_batch`) | Atomic multi-operation edits already exist. No second transaction system. |
| `Doc::revision()` advances by 1 on every committed apply, undo or redo; a validated no-op keeps it (D8) | `command.rs` (`apply_command`), `undo.rs` | The revision is the concurrency token. |
| Fresh ids are allocated deterministically: arena append, geometry order for splits (D2) | `arena.rs`, `route.rs` | A full copy of a document allocates the same ids as the original. |
| `Change.dirty` covers the old and new bounds of every touched item | `command.rs` (`Change` docs), `tests/change_contract.rs` | A diff can be computed exactly by querying `dirty` before and after. |
| `Doc` derives only `Debug` and `Default`: no `Clone`, no fork, no `DocSnapshot` | `doc.rs` | Missing prerequisite for preview and for D8 background jobs: task A1.1. |
| The code's `Command` has `Move { items, delta }` and `Delete(Vec<ItemId>)`; no `follow`, `with_wires`, `DragSegment` or `Paste` | `command.rs` | The canvas plan section 9 sketch is ahead of the code. Schema v1 follows the code; fields are added as W2.2, W3.2, W3.5 and T7.1 merge. Updated at A1.3 (2026-11-21): `follow`, `with_wires` and `DragSegment` are now in the code; schema v1 still has exactly the section 9.2 operations, `invert_follow` and `with_wires` arrive with A2.2 and A2.3, and no task yet exposes `DragSegment`. |
| No public type derives serde; only the private RON records in `io.rs` do | `io.rs` | The wire schema is a separate DTO layer. Internal types do not become a public contract. |
| `PlacePart` needs the caller's `local_pins` and `local_bounds` | `command.rs` | A shared helper in `omnia-symbols` builds it from a symbol (A1.2). |
| `render_to_image` exists on Windows only (G12) | AGENTS.md | Visual checks for agents use an SVG render, not a GPU screenshot (A2.4). |
| T2.8 (`blocked`) lists `crates/omnia-doc/` as owned | canvas-roadmap.md | A1.1 needs the owner's confirmation at HA1 that a disjoint doc task may proceed. |

## 3. Budgets

Measured on the reference machine under canvas plan section 2.1. All canvas and wiring
budgets still apply; these are added. AB1 to AB3 are chosen figures, not derived ones.

| ID | Budget | Measured by |
| --- | --- | --- |
| AB1 | `inspect` with `include: ["summary"]` on `10k`: p99 < 5 ms | `crates/omnia-bench/benches/agent.rs` |
| AB2 | `preview` of a `Move` of 100 parts on `10k`, fork included: p99 < 25 ms; peak extra memory recorded against B9 | same bench |
| AB3 | App mode: with an agent applying one batch per second, B3, B5 and B9 still hold on `1k` | `bench run --sheet 1k --script pan --agent-load` (A4.5) |

## 4. Constants

| ID | Constant | Value |
| --- | --- | --- |
| AK1 | Protocol revisions | Pinned at A0.2 (2026-11-20) from the published specification's versioning page, which names 2026-07-28 as current: **2026-07-28**, **2025-11-25** and **2025-06-18**, newest first. 2025-06-18 is the minimum: the first revision with structured tool output and `outputSchema`. 2026-07-28 is stateless; the two older ones are handshake revisions. The server speaks both eras (section 10.5). |
| AK2 | Request size; file size on open | 8 MiB each |
| AK3 | Operations per request | 4,096 |
| AK4 | Page size | 500 by default, 5,000 at most |
| AK5 | Idempotency cache | 256 entries per session, in memory |
| AK6 | Open documents per session | 8 |
| AK7 | Schema version | `omnia.agent/1`. Adding an optional field or a new operation is a minor change; removing or changing one is a major change and a new version string. |
| AK8 | Units | Points are `[x, y]` integer nanometres (K1). Ids are 0-based. Capabilities report `grid_nm` (K2 grid) and `pin_lattice_nm` (K2 lattice). |

## 5. Architecture

```
   MCP client (stdio)       shell / scripts          later: app UI, HTTP
        |                        |                          |
  +-----v------------------------v--------------------------v-----+
  | omnia-cli (binary `omnia`): transports only                   |
  |   mcp.rs    JSON-RPC 2.0 over stdio <-> Request/Response      |
  |   call.rs   one request in, one response out                  |
  |   jsonl.rs  long-lived JSON-lines session                     |
  +---------------------------+-----------------------------------+
                              | omnia_agent::{Request, Response}
  +---------------------------v-----------------------------------+
  | omnia-agent (library; no GPUI, no async runtime)              |
  |  schema/   typed DTOs, version AK7, error codes               |
  |  catalog   capability discovery = the dispatch table          |
  |  service   sessions, documents, revisions, idempotency        |
  |  ops       DTO operation -> omnia_doc::Command, 1:1           |
  |  preview   Doc::fork() + Doc::apply()  (the commit's code)    |
  |  diff      before/after over Change.dirty, public APIs only   |
  |  read      summaries, paging, query modes, canonical nets     |
  |  approval  tiers, effect digests, grants                      |
  |  files     roots, staged writes                               |
  |  jobs      step-driven jobs, cancel token, progress sink      |
  |  DocHost   headless store | app adapter (A4)                  |
  +------+---------------+------------------+---------------------+
         |               |                  |
     omnia-doc      omnia-symbols       omnia-theme (SVG colours, A2.4)
     (+ fork)       (+ place helper)
```

Data flows one way, as on the canvas: a request becomes at most one `Command`; `Doc::apply`
returns a `Change`; the response reports it. In app mode the same `Change` invalidates the
canvas through the path a tool commit uses (D7).

### Crate ownership

| Crate | Owns | Must not |
| --- | --- | --- |
| `omnia-doc` (doc lane) | `Doc::fork()` only (A1.1) | Gain DTOs, serde on public types for agents, agent concepts or any UI dependency (INV12) |
| `omnia-symbols` (assets lane) | `SymbolLibrary::place_command`, shared by the canvas place tool (T5.3) and the agent (A1.2) | Depend on GPUI |
| `omnia-agent` (agent lane, new) | Schema, errors, catalog, service, preview, diff, read projections, approvals, file roots, jobs, the `DocHost` trait | Depend on GPUI or an async runtime; call anything not public in `omnia-doc`; keep its own command or undo system |
| `omnia-cli` (agent lane, new) | The `omnia` binary: `mcp`, `call`, `serve`, `schema`, `capabilities` | Hold domain logic |
| `omnia-app` (canvas lane, A4 only) | `AppDocHost` over `Entity<Doc>`, the approval dialog, the local IPC listener | Run agent edits during a gesture (INV5); expose input injection |
| `omnia-canvas` (A4 only) | Ghosts of a pending agent preview in `OverlayView` (D7), colours from tokens (INV6) | Contain agent protocol code |
| `omnia-bench` | `benches/agent.rs`, the `--agent-load` script option (A1.10, A4.5) | Widen visibility to measure something |

## 6. Decisions

All DECIDED at HA1 on 2026-11-20. The owner delegated HA1 to the agent on that date: every
AD below was kept as written, except where an amendment names its date.

### AD1 One service, many transports. DECIDED

- `AgentService` in `omnia-agent` handles every request. MCP, the one-shot command line and
  JSON-lines are adapters in `omnia-cli` that only parse and print.
- A request has the same meaning and gets the same response bytes through every adapter. The
  MCP `structuredContent` of a tool result is that response.
- The service is synchronous and has no async runtime. Each transport owns its I/O; the host
  drives jobs (section 11).

### AD2 Commands are the only way to edit. DECIDED

- Agent edits are `omnia_doc::Command`s applied through `Doc::apply`, `undo` and `redo`.
- Edit operations map one to one onto `Command` variants (section 9.2). A request with more
  than one operation becomes one `Command::Batch`.
- There is no agent-specific command, transaction or undo system.
- Operations in one batch reference existing ids and points, never ids created earlier in the
  same batch: predicting them would duplicate the arena's allocation. Wiring is positional
  (D2), so an end placed at a pin's point connects to it. An agent that needs new ids previews
  first and reads the predicted ids.

### AD3 Preview runs the commit's code. DECIDED

- `preview` forks the document (`Doc::fork`, A1.1) and calls `Doc::apply` on the fork, then
  diffs the two documents over `Change.dirty`.
- `fork` copies every reserved arena slot and the revision, so a commit at the same revision
  allocates the same ids and returns the same `Change` (AINV2).
- The live document is never touched by a preview: preview takes `&Doc` (AINV3).
- Rejected: applying to the live document and rolling back (it mutates the document the user
  is looking at and needs `&mut`); a separate pure planner per command (a parallel system
  that can drift from `apply`). The canvas keeps its own fast previews, such as `plan_follow`,
  which `apply` also calls (WINV5).

### AD4 Optimistic concurrency on the revision. DECIDED

- Every mutating request carries `expected_revision`. A mismatch with `Doc::revision()` is
  refused with `stale_revision` and changes nothing.
- Every response carries `revision`. There are no locks and no leases.
- Requests to one document are handled one at a time on the thread that owns it (D8).

### AD5 One request, one undo step. DECIDED

- A request applies one `Command`, which is atomic and is one undo step.
- There is one history per document. In app mode agent commits are ordinary undo steps that
  the person can undo.

### AD6 Structured errors. DECIDED

- Every failure has a stable snake_case `code`, a `message`, `retryable`, typed `details` and
  an optional `hint` (section 9.4).
- `CommandError` maps to codes through an exhaustive `match` with no wildcard arm, so a new
  variant fails to compile until it has a code (AINV10).

### AD7 Approval tiers in the service. DECIDED

Enforced in `omnia-agent`, not in any user interface, so every surface meets the same guard.

| Tier | Effect | Headless (AQ4 default) | App mode (AQ7 default) |
| --- | --- | --- | --- |
| 0 | Read, validate, preview, open inside roots | Allowed | Allowed |
| 1 | Undoable document edit | Allowed | The person approves the preview. The grant is bound to `preview_id` and the revision, is used once and expires on any revision change. |
| 2 | Write a new file inside roots | Allowed | The person approves |
| 3 | Irreversible: overwrite a file, discard unsaved changes | Refused with `approval_required` and an `effect_digest`; the retry must echo that digest. The tool is annotated destructive so the client asks its user. | The person approves in the app. The agent cannot approve itself. |

### AD8 Determinism. DECIDED

- Arrays in responses are sorted: items by kind then id; `Change.added` and `removed` sorted,
  although `Doc` treats them as unordered sets.
- Nets are keyed canonically: lowest pin `(part id, pin index)`, else lowest wire id, else
  lowest label id. Raw `NetId`s are never exposed; a `NetId` is valid only until the next edit.
- DTOs contain no hash maps and no floats; field order is fixed by struct order.
- The same document and request give byte-identical JSON (AINV5).

### AD9 No GUI input automation. DECIDED

No tool clicks, drags, types into or screenshots the window. Agents edit through commands.
Visual checks use the SVG render (A2.4).

### AD10 The agent surface never touches the project process. DECIDED

No tool reads or writes plans, roadmaps or `docs/measurements`, runs a gate campaign, answers
an `H*`, `HW*` or `HA*` checkpoint, or marks a budget met. Human checkpoints stay explicit.

### AD11 Parts are placed by symbol name. DECIDED

`place_part` names a symbol (`Resistor-IEEE-Standard`). Pins and bounds come from
`SymbolLibrary::place_command` (A1.2), the same helper the canvas place tool uses.

### AD12 Units and ids. DECIDED

- Points are integer nanometres (AK8). Floats never appear in the schema.
- Ids are typed strings, 0-based like the arenas: `part:3`, `wire:7`, `label:2` (AQ5).
- Orientations are named after the `Orient` constants in `geom.rs`: `identity`, `rot_90`,
  `rot_180`, `rot_270`, `flip_x`, `flip_x_rot_90`, `flip_y`, `flip_x_rot_270`.

### AD13 Headless first, app-hosted later. DECIDED

Milestones A1 to A3 run headless on files. A4 hosts the same service in the app through a
`DocHost` adapter over `Entity<Doc>`. Nothing in A1 to A3 depends on GPUI.

### AD14 Document text is untrusted data. DECIDED

Designators, values and label text are returned only inside typed fields, never spliced into
tool descriptions, prompts or messages. Tool and prompt descriptions are static strings in
the binary.

## 7. Invariants

Each has a test; the roadmap task that adds it is in brackets.

| ID | Rule |
| --- | --- |
| AINV1 | Agent edits reach a document only through `Doc::apply`, `undo` and `redo`. `omnia-agent` calls no other mutator. [A1.5] |
| AINV2 | Preview equals commit: for a document at revision `r` and a request, the preview's `Change` (ids, dirty, nets_changed) and after-state equal what `apply` at `r` returns and leaves (`content_eq`). [A1.5] |
| AINV3 | Read, validate and preview leave the document `==` (persistent equality: revision and every reserved slot), with the same undo and redo results afterwards. [A1.5] |
| AINV4 | A refused request leaves the document `==` (extends WINV4). [A1.5] |
| AINV5 | The same document and request give byte-identical responses, also when the document is rebuilt with items inserted in another order (up to the id mapping) and when JSON object keys arrive in another order. [A1.4, A1.5] |
| AINV6 | No mutation happens without an `expected_revision` equal to the current revision. [A1.5] |
| AINV7 | Replaying an idempotency key with the same request returns identical bytes and applies at most once. [A1.5] |
| AINV8 | `omnia-agent` and `omnia-cli` do not depend on GPUI (extends INV12). [A1.3] |
| AINV9 | No tier 3 effect happens without a matching approval. [A1.6] |
| AINV10 | The map from `CommandError` to error code is exhaustive and gives each variant its own code. [A1.3] |
| AINV11 | The capability catalog equals the dispatch table, and the committed JSON Schemas equal the generated ones. [A1.3, A1.8] |
| AINV12 | No file outside the configured roots is read or written. [A1.6] |
| AINV13 | In app mode the revision does not change while a gesture is in progress (extends INV5). [A4.2] |

## 8. Scenarios

Each is a test with the required result. Transcript scenarios are exact request and response
files under `crates/omnia-cli/tests/transcripts/`.

| ID | Setup and action | Expected result |
| --- | --- | --- |
| ATC-read-01 | Open a generated `100` sheet; `inspect` summary | Counts equal `Doc::parts`, `wires` and `labels`; revision equals the file's |
| ATC-net-01 | Two wires crossing at their interiors | Two nets (INV2) |
| ATC-edit-01 | Place two resistors, preview a wire between their pins, apply with the `preview_id`, then undo | Apply returns the previewed `Change`; one net joins the two pins; undo gives `content_eq` with the original |
| ATC-edit-02 | Apply with an old `expected_revision` | `stale_revision {expected, actual}`; document `==` |
| ATC-edit-03 | Apply twice with the same idempotency key | Second response byte-identical; revision advanced once |
| ATC-edit-04 | Batch whose third operation names a missing item | `missing_item`; document `==`; no undo step |
| ATC-edit-05 | Apply with a `preview_id` from different operations | `preview_mismatch`; document `==` |
| ATC-edit-06 | The person's edit lands between the agent's apply and its undo | Agent undo refused with `stale_revision`; the person's edit stays |
| ATC-save-01 | Save over an existing file without, then with, the echoed digest | `approval_required`, then saved; a failed write keeps the old file |
| ATC-path-01 | Open `../x.ron`, an absolute path outside roots, and a symlink out of a root | `path_outside_root` each time; nothing read |
| ATC-mcp-01 | Handshake, `tools/list`, then the ATC-edit-01 loop over MCP | Exact transcript |
| ATC-mcp-02 | A malformed JSON-RPC message; a tool call that fails in the domain | JSON-RPC error; tool result with `isError` and the structured error |

## 9. Schemas

### 9.1 Envelope

Every response:

```json
{ "schema": "omnia.agent/1", "doc_id": "d1", "revision": 13, "result": { }, "warnings": [] }
```

On failure `result` is replaced by `error` (9.4). Requests through `omnia call` and `omnia
serve` are `{"op": "<operation>", "params": { }, "idempotency_key": "..."}`; JSON-lines adds
an `"id"` that the response echoes. All objects reject unknown fields.

Decoding rules, fixed at A1.3 (2026-11-21; amended by its three review rounds the same day):

- Two wire forms, one meaning. The envelope above is what `omnia call` and `omnia serve` read;
  `params` is always present (`{}` for `capabilities` and `new`). Over MCP a tool's
  `arguments` are the params object itself, with `idempotency_key` beside the params fields on
  the operations section 11.2 names (schema `<op>.arguments`). Both decode to the same request.
  `idempotency_key` anywhere else is a `schema_violation`.
- Order of checks, the same for both forms: raw input over AK2 is `limit_exceeded`; malformed
  JSON, a duplicate object key, or a number that is not a 64-bit integer written without
  fraction or exponent is `invalid_request` (`-0` is read as `0`); in the envelope, a missing
  `op` or `params` or a non-string `op` is `schema_violation`; an operation the catalog does
  not list is `unknown_operation`; over AK3 or AK4 is `limit_exceeded`; an `idempotency_key`
  on an operation that takes none is `schema_violation` at `/idempotency_key` ("unknown
  field"), also on `request_status`, whose own params field of that name an envelope key never
  fills; then the params, with any `idempotency_key` placed beside the params fields, are
  validated against `<op>.arguments` and any mismatch is `schema_violation {path}`; then the
  DTOs are decoded; last, AK2 on the canonical forms (below). In the envelope, an unknown field,
  and an `idempotency_key` written inside the params of an operation whose params have no such
  field (`/params/idempotency_key`), are ordered among the params fields by name; a fault on the
  params object itself comes first, and the envelope's field wins a tie.
- AK2 bounds the raw input and both canonical forms. A request is accepted, by either decoder
  or by `Request::new`, only if `Request::to_json` and `Request::to_arguments` are both at most
  8 MiB, so whatever is accepted or built can be sent in either form. The value reported is the
  larger form's size; the envelope is always the larger. MCP arguments of exactly 8 MiB are
  therefore refused, because the envelope they imply is longer.
- A refusal of well-formed JSON (a duplicate key, a number's spelling) has no position in its
  message, so both forms report it alike. Malformed JSON keeps the parser's line and column,
  which refer to the raw input.
- The two forms give the same error code, details and message for the same fault; the JSON
  Pointer differs only by the envelope's `/params`. Messages do not repeat the path. A
  property test decodes mutated inputs through both forms and compares the errors.
- The decoder refuses everything the published schema refuses (nulls for optional fields,
  arrays in place of objects, out-of-range ids and numbers). Beyond the schema it checks only:
  the request size, duplicate keys, the integer spelling of numbers, and that bounds have
  `min <= max` on both axes. Property tests mutate valid requests and responses and check
  that the decoder and the published schema agree on every result but these.
- `path` is a JSON Pointer to the first failure, found by visiting object keys in byte order;
  when no alternative of a union matches, the failure with the most pointer segments is
  reported, the first on a tie. The same request with its keys in another order gets the same
  error bytes (AINV5).
- What the API can build, it can read back. `Request`, `Response` and the stored response of
  `request_status` have private fields and checked constructors (`Request::new`,
  `Response::new`, `StoredResponse::new`), and `Bounds` holds only a valid rectangle, so every
  value serializes to a document its own decoder accepts. Property tests build values from
  parts that may break the rules and check that whatever is accepted decodes back to itself.
- A successful response whose result belongs to a document (`new`, `open`, `close`,
  `inspect`, `query`, `nets`, `validate`, `preview`, `apply`, `undo`, `redo`, `save`) carries
  `doc_id` and `revision`, which a client needs for AD4; the schema requires them and
  `Response::new` refuses a result without them. Audited at the same time: no other result has
  a field a client must have that the schema leaves optional (`path` is absent for a new
  document, `next_cursor` on the last page, `dirty` for a no-op, and `inspect` returns only
  what `include` names).
- An absent optional field is omitted from responses, never `null`. Document ids are `d<n>` and
  job ids `j<n>`, in the canonical decimal of AD12's item ids; ids fit `u32` (items) or `u64`
  (documents, jobs).

### 9.2 Edit operations

| Operation | Params | `Command` |
| --- | --- | --- |
| `place_part` | `symbol`, `at`, `orient`, `designator`, `value` | `PlacePart` via `place_command` |
| `add_wires` | `wires: [[a, b], …]` | `AddWires` (split and merge inside `apply`, T6.1) |
| `add_label` | `at`, `text` | `AddLabel` |
| `move` | `items`, `delta`; `invert_follow` from A2.2 | `Move` |
| `reorient` | `items`, `pivot`, `by`; `invert_follow` from A2.2 | `Reorient` |
| `delete` | `items`; `with_wires` from A2.3 | `Delete` |
| `set_text` | `item`, `field` (`designator`, `value`, `label`), `text` | `SetText` |

`invert_follow` means exactly what holding Alt at commit means (WK3), applied to the session's
`WiringPrefs` (WK10). It exists only once W2.2 has added `FollowPolicy` to the code.

### 9.3 Results of edits

`preview` and `apply` return:

```json
{
  "preview_id": "pv-5f0c1d2e3a4b5c6d",
  "base_revision": 12,
  "change": { "revision": 13, "dirty": {"min": [0, 0], "max": [10160000, 5080000]},
              "added": ["wire:41", "wire:42"], "removed": ["wire:7"], "nets_changed": true },
  "diff": {
    "added":    [{"id": "wire:41", "wire": {"a": [0, 0], "b": [5080000, 0]}}],
    "removed":  [{"id": "wire:7",  "wire": {"a": [0, 0], "b": [10160000, 0]}}],
    "modified": [{"id": "part:3", "before": {"at": [0, 0], "orient": "identity"},
                                  "after":  {"at": [2540000, 0], "orient": "identity"}}],
    "truncated": false
  }
}
```

`apply` returns the same shape without `preview_id` and `base_revision`. The diff is paged by
AK4; `truncated` says so.

As built at A1.3: `before` and `after` of a modified item carry all of its fields (a part's
`symbol`, `at`, `orient`, `designator` and `value`; a wire's `a` and `b`; a label's `at` and
`text`); the example above abbreviates them. `change.dirty` is omitted for a validated no-op.
`validate` returns `{base_revision, change}`; `undo` and `redo` return `{change}`.

### 9.4 Errors

```json
{
  "schema": "omnia.agent/1", "doc_id": "d1", "revision": 12,
  "error": {
    "code": "stale_revision",
    "message": "expected revision 12, document is at 14",
    "retryable": true,
    "details": { "expected": 12, "actual": 14 },
    "hint": "read the document again and rebuild the request"
  }
}
```

| Group | Codes |
| --- | --- |
| Protocol and schema | `invalid_request`, `schema_violation {path}`, `unknown_operation`, `limit_exceeded {limit, value}`, `unsupported_in_mode`, `protocol_version_unsupported` |
| Resolution | `document_not_found`, `unknown_symbol`, `unknown_item` |
| From `CommandError` | `out_of_bounds`, `invalid_wire`, `missing_item {item}`, `duplicate_item {item}`, `invalid_text_field {item, field}`, `follow_no_route {from, to, blocker}` (`Follow`), `segment_blocked {blocker}` (`Segment`), `wire_capture {wire, blocker}` (`Capture`), `off_lattice {at}` (`OffLattice`) |
| Concurrency | `stale_revision {expected, actual}`, `preview_mismatch`, `idempotency_conflict`, `user_gesture_in_progress`, `nothing_to_undo`, `nothing_to_redo` |
| Approval | `approval_required {approval_id, tier, effect, effect_digest}`, `approval_denied` |
| Files | `path_outside_root`, `file_exists`, `io_error`, `unsupported_file_version` |
| Jobs | `cancelled`, `job_not_found` |
| Other | `internal` |

Amended at A1.3 (2026-11-21), so the plan follows the code. The wiring tasks had already added
`Segment`, `Follow`, `Capture` and `OffLattice` to `CommandError`, so all of them have codes
now and AINV10's exhaustive mapping covers them; `Capture` and `OffLattice` gained the new codes
`wire_capture` and `off_lattice`. The three item codes carry the ids `CommandError` already
holds. `nothing_to_redo` is added for `omnia_redo`. `limit` is one of `request_bytes`,
`file_bytes`, `operations`, `page_size`, `open_documents`; `effect` one of `edit`,
`write_file`, `overwrite_file`, `discard_unsaved`. A `blocker` is
`{"kind": "pin", "part", "pin"}`, `{"kind": "wire_end" | "wire_body" | "overlap", "wire"}` or
`{"kind": "sheet"}`. Codes without details carry `"details": {}`. `retryable` is fixed per code:
true for `stale_revision`, `user_gesture_in_progress`, `approval_required` and `io_error`. The
schema pins it per code and the decoder refuses an error whose flag contradicts its code.

### 9.5 Schema files

JSON Schemas for every request, response and operation are generated by a hand-written module
in `omnia-agent` and committed under `crates/omnia-agent/schema/`. A test regenerates them and
fails on any difference (AINV11), the way `cargo xtask tokens` is checked. No schema crate is
added (AQ3).

As built at A1.3: dialect JSON Schema 2020-12; files `request.json` and `response.json` cover
every available operation, and each available operation has `<op>.params.json` (its `params`
in the envelope), `<op>.arguments.json` (its MCP `inputSchema`: the params plus
`idempotency_key` where 11.2 allows it) and `<op>.response.json` (its envelope, the MCP
`outputSchema`), each self-contained with `$defs`. Operations the catalog does not list yet
(the A3 job tools) have no committed files. Keys are printed sorted, so the bytes do not depend
on `serde_json` features. The schemas state the ranges the codecs hold: id patterns spell
exactly `0..=4294967295` (items) or `0..=18446744073709551615` (documents, jobs), every 64-bit
integer has its maximum, and bounds corners lie inside K1. The decoders validate against these
same schemas with a hand-written validator for the keywords the module emits (section 9.1). It
compares integers exactly, also beyond 64 bits. `response.json` ties each result to its
envelope: a document's result requires `doc_id` and `revision` there too. Every envelope schema
forbids `result` and `error` together on its own (`not`), whatever its branches require.
After a deliberate change, `OMNIA_BLESS_SCHEMAS=1 cargo test -p omnia-agent ainv11` rewrites
the directory; without the variable the test only compares. Property tests check that every
serialized DTO validates against its published schema text, with no codec in the check.

## 10. MCP mapping

### 10.1 Tools

Tier is the AD7 tier. Annotations are MCP tool annotations.

| Tool | Tier | Annotations | Milestone |
| --- | --- | --- | --- |
| `omnia_capabilities` | 0 | read-only, idempotent | A1 |
| `omnia_new` | 0 | not destructive | A1 |
| `omnia_open {path}` | 0 | read-only | A1 |
| `omnia_inspect {doc_id, include, area?, cursor?, limit?}` | 0 | read-only, idempotent | A1 |
| `omnia_query {doc_id, area, mode: envelope \| within \| touching, cursor?, limit?}` | 0 | read-only, idempotent | A1 |
| `omnia_nets {doc_id, at? \| wire?}` | 0 | read-only, idempotent | A1 |
| `omnia_validate {doc_id, expected_revision, operations}` | 0 | read-only, idempotent | A1 |
| `omnia_preview {doc_id, expected_revision, operations}` | 0 | read-only, idempotent | A1 |
| `omnia_apply {doc_id, expected_revision, operations, preview_id?, idempotency_key?}` | 1 | not destructive, not idempotent | A1 |
| `omnia_undo`, `omnia_redo {doc_id, expected_revision}` | 1 | not destructive | A1 |
| `omnia_save {doc_id, path?, approval?}` | 2 or 3 | destructive | A1 |
| `omnia_close {doc_id, discard_unsaved?, approval?}` | 0 or 3 | destructive | A1 |
| `omnia_request_status {idempotency_key}` | 0 | read-only | A1 |
| `omnia_route`, `omnia_snap`, `omnia_hit` | 0 | read-only, idempotent | A2 |
| `omnia_render_svg {doc_id, area?, theme}` | 0 | read-only, idempotent | A2 |
| `omnia_job_status`, `omnia_job_cancel` | 0 | — | A3; not listed before A3.1 |

Every tool declares an `inputSchema` and an `outputSchema`. A result carries
`structuredContent` (the 9.1 envelope) and a text block with the same JSON.

A tool's arguments are its operation's params, plus `idempotency_key` for `omnia_apply`,
`omnia_undo`, `omnia_redo` and `omnia_save` (11.2); the `inputSchema` is `<op>.arguments` and
the adapter decodes it with `Request::from_arguments`, which gives the same request as the
envelope. `tools/list` is built from the catalog, and the catalog lists only operations whose
milestone is delivered (`OpSpec::available`): an operation that is defined but not available
keeps its DTOs, is refused as `unknown_operation`, and has no committed schema. The job tools
are unavailable until A3.1 turns them on.

Tool failures are results with `isError: true` and the structured error, so the model can read
and correct them. Only malformed JSON-RPC messages and unknown tool names are JSON-RPC protocol
errors.

### 10.2 Resources

| URI | Content | Subscribable |
| --- | --- | --- |
| `omnia://capabilities` | Same as `omnia_capabilities` | No |
| `omnia://schema/{name}` | The JSON Schemas of 9.5 | No |
| `omnia://docs` | Open documents: `doc_id`, path, revision, dirty flag | `list_changed` |
| `omnia://doc/{doc_id}` | Summary | `updated` on every revision change |
| `omnia://doc/{doc_id}/items{?cursor}` | Items in id order, paged | Yes |
| `omnia://doc/{doc_id}/nets` | Canonical nets | Yes |
| `omnia://symbols`, `omnia://symbols/{name}` | Pins, bounds, placed size (K7) | No |
| `omnia://jobs/{job_id}` | Job state (A3) | Yes |

Tools duplicate the key resources because client support for resources varies.

### 10.3 Prompts

Static, versioned with the schema, each with a transcript fixture:

- `omnia_inspect_sheet {doc_id}`: read the summary and nets; report free ends and nets with a
  single pin.
- `omnia_place_and_wire {doc_id, parts, connections}`: the section 13 loop as instructions.
- `omnia_safe_edit_loop`: the verification protocol of section 13.

### 10.4 Notifications

| Direction | Message | When |
| --- | --- | --- |
| Server to client | `notifications/resources/updated` | A subscribed document's revision changes, or a subscribed job's state or progress changes (A3) |
| Server to client | `notifications/resources/list_changed` | Documents open or close; jobs start (A3) |
| Server to client | `notifications/progress` | While the request that supplied the `progressToken` is in flight (A3, 11.5) |
| Server to client | `notifications/message` | Logging, level-filtered |
| Client to server | `notifications/cancelled` | Names an in-flight request by `requestId` and sets its cancel token; a returned job is cancelled with `omnia_job_cancel` |

The catalog is fixed for a mode, so `tools/list_changed` is sent only when app mode attaches
or detaches (A4). stdout carries only protocol messages; logs go to stderr.

The table is the handshake era's (2025-11-25 and 2025-06-18). Section 10.5 gives the
stateless era's forms.

### 10.5 Protocol eras (AK1)

Added at A0.2 (2026-11-20). The pinned revisions span two eras, and one `omnia mcp` process
serves both (AQ2). The service and the application payloads (tool arguments, `structuredContent`,
resource contents, error `details`) are the same in both. The protocol envelopes differ:
`mcp.rs` serializes each era's results, errors and notifications. A1.8 reads the pinned
specification pages before coding; the summary below is what A0.2 read and is not a substitute.

| | Handshake era: 2025-11-25, 2025-06-18 | Stateless era: 2026-07-28 |
| --- | --- | --- |
| Version | `initialize` negotiates one revision for the process | Each request's `_meta` carries its revision (`io.modelcontextprotocol/protocolVersion`) and the client's capabilities (`io.modelcontextprotocol/clientCapabilities`), both required; there is no handshake |
| Unknown or missing version | Answer `initialize` with 2025-11-25, the newest handshake revision pinned | An unsupported revision gets `UnsupportedProtocolVersionError` (`-32022`), `data: {supported, requested}`, with `supported` listing AK1. Missing required `_meta` is invalid params (`-32602`) |
| Discovery | `initialize` result | `server/discover`, which the server must implement |
| Results | As in 10.1 | As in 10.1, with `resultType`: `"complete"` for a finished result (the core also defines `"input_required"`, which Omnia does not use). Complete results of `tools/list`, `prompts/list`, `resources/list`, `resources/templates/list` and `resources/read` carry the caching hints `ttlMs` and `cacheScope` |
| Subscriptions | `resources/subscribe`, then `notifications/resources/updated` | `subscriptions/listen` (requesting `notifications.resourceSubscriptions`), then `notifications/resources/updated`, tagged with the listen request |
| Logging | Optional `logging/setLevel`, then `notifications/message` | `notifications/message` only when the request carries `io.modelcontextprotocol/logLevel`. Protocol logging is deprecated in this revision; Omnia supports it because 10.4 lists it, and A1.8 may drop it here |
| Liveness | `ping` | None (removed) |
| Progress, cancel | `notifications/progress` from the server, correlated by `progressToken`; `notifications/cancelled` from the client, correlated by `requestId` | The same. stdio has one channel each way and no per-request streams: progress goes on stdout, cancellation arrives on stdin. Progress stops when its request completes |
| Server-to-client requests | None used | None: the server never sends a request |

Selection on stdio is Omnia's policy, not the specification's. The specification lets a
dual-era server serve both eras at once on one process; Omnia picks one per process, which
matches how clients open a connection. The first valid request decides: `initialize` puts the
process in the handshake era for its lifetime. Any other request carrying a `_meta` revision,
typically a client's `server/discover` probe, puts it in the stateless era, where every request
is handled by its own `_meta` revision. After that, a message of the other era is refused as a
protocol error. No era is chosen by guessing from other fields. A dual-era client that probes
with `server/discover` and gets a recognised version error stays in the stateless era and
retries with a supported revision.

The stateless era has no protocol-level session. Each request carries its own protocol
metadata. Documents and jobs are reached through explicit handles (`doc_id`, `job_id`, the
idempotency key) or resource URIs, and `omnia://docs` lists the open documents. The process is
only the storage lifetime of those handles (11.1): they expire when it exits.

## 11. Semantics

### 11.1 Sessions and documents

- A session is one transport connection: one stdio process, one JSON-lines stream, later one
  HTTP session. In the stateless protocol era (10.5) MCP has no protocol-level session; the
  stdio process is then only the storage lifetime of the handles below, which every request
  names explicitly (`doc_id`, `job_id`, idempotency key, or a resource URI) and which expire
  when the process exits.
- A session holds its open documents (`doc_id` → document, path, `saved_revision`), its
  `WiringPrefs` (WK10 defaults), its idempotency cache, its approval grants and its jobs.
- `doc_id`s are never reused within a session. A document is dirty when
  `revision != saved_revision`.
- In app mode the session attaches to the document the person has open. Both share one
  revision and one history.

### 11.2 Idempotency

- `idempotency_key` is optional on apply, undo, redo and save, and refused on every other
  operation. In the envelope it sits beside `op` and `params`; in MCP tool arguments it sits
  beside the params fields (9.1). `request_status` takes the key as its only param.
- The session caches key, canonical request hash and full response for AK5 entries.
- The same key with the same request returns the stored bytes and applies nothing (AINV7). The
  same key with another request is refused with `idempotency_conflict`.
- After a lost reply, `omnia_request_status(key)` answers `not_run`, `applied {op, revision,
  response}` or `failed {error}`. `op` names the stored request's operation, one of apply,
  undo, redo and save, and `response` is a successful response of that operation; a request
  that failed is reported as `failed`. Schema and decoder both refuse any other combination.
- `expected_revision` alone already makes a blind retry safe: it is refused as stale and never
  applies twice.

### 11.3 Preview identity

`preview_id` is a 64-bit hash (FxHash, `rustc-hash`, already a dependency) of `doc_id`,
`base_revision`, the canonical operation bytes and the follow policy. `apply` with a
`preview_id` hashes its own request; a difference is refused with `preview_mismatch`. The hash
binds a commit to its preview and an approval to an effect. It is not a security token (AQ10).

### 11.4 Undo and redo

- `omnia_undo` and `omnia_redo` require `expected_revision`.
- Every apply, undo or redo by anyone advances the revision. If `expected_revision` equals the
  revision the agent's own apply returned, the top of the undo stack is provably that commit.
  No new document API is needed.
- If the person edited in between, the agent is refused with `stale_revision` and cannot undo
  the person's work (ATC-edit-06).

### 11.5 Jobs, progress and cancellation (A3)

- A1 operations are synchronous and bounded by AK2 to AK6.
- Long work becomes a job: `job_id`; state `queued`, `running`, `succeeded`, `failed`,
  `cancelled_by_client` or `cancelled_by_system`; progress `{completed, total, unit, message}`.
- Jobs are step functions the host advances: a thread loop headless, GPUI's background
  executor on a fork in the app (D8).
- Cancellation is cooperative: the cancel token is checked between steps.
- A job that edits computes on a fork and commits once at the end, as one `Command`, only if
  the revision has not changed; otherwise it ends `failed` with `stale_revision`. Simulation
  jobs, once planned, never edit the document.
- MCP: every AK1 revision uses Omnia's job tools (`omnia_job_status`, `omnia_job_cancel`)
  and `omnia://jobs/{id}` with subscriptions, by era `resources/subscribe` or
  `subscriptions/listen` (10.5); nothing polls (AQ15). Omnia never advertises tasks: no
  `capabilities.tasks` in either handshake revision's `initialize` result, and no
  `io.modelcontextprotocol/tasks` in `server/discover`'s extensions. The published
  specifications permit all three omissions. For context, checked at A0.2 (2026-11-20):
  2025-06-18 has no task primitive; 2025-11-25 added Tasks to the core, marked experimental and
  negotiated by capability; 2026-07-28 moved them into that optional extension.
- While the request that starts a job is in flight, a `progressToken` on it maps to
  `notifications/progress`, and `notifications/cancelled` naming that request's `requestId` sets
  the job's cancel token. Progress stops when the request completes. Once the call has returned
  the `job_id`, updates go only through the job resource subscription, and the client cancels
  through `omnia_job_cancel`. A3.2 builds this.

### 11.6 App mode (A4)

- A mutating request while a gesture is in progress is refused with `user_gesture_in_progress`
  (retryable), so INV5 holds (AINV13).
- Agent work runs between frames on the UI thread. A fork for a preview or job is taken there
  and the work moves to the background executor; a stale result is dropped (D8).
- A pending agent preview is drawn as ghosts in `OverlayView` while the approval dialog is
  open (D7, INV11).

## 12. Security and trust

### 12.1 Local stdio (A1 to A3)

- The parent process is the client, running as the same OS user. There is no listener and no
  network.
- Files are limited to the `--root DIR` arguments (repeatable). Paths are canonicalised; `..`
  and symlink escapes are refused; only the RON sheet format is read, within AK2.
- Writes are staged, as a temporary file then a rename, so a failed save keeps the old file.
  Overwriting is tier 3.
- No shell, no arbitrary file read, no environment dump, no plug-ins.
- Over-limit input is refused with `limit_exceeded`, never truncated.

### 12.2 App-hosted channel (A4)

- A stdio MCP process bridges to the running app over a local channel: a named pipe on
  Windows; a Unix socket in `$XDG_RUNTIME_DIR` with mode 0600 on Linux. If loopback TCP is ever
  used, the bridge connects to `127.0.0.1` exactly, never a name that may resolve to `::1`.
- A fresh token per app session lives in an owner-only file. The bridge refuses a token file
  that is a symlink, not a plain file, or readable by other users, and has no fallback to a
  shared temporary directory.
- Approvals happen in the app window (AD7).

### 12.3 Network transport (A5, GATED by HA3)

Not built unless HA3 opens it. If built: Streamable HTTP bound to loopback by default;
`Origin` and `Host` validation against DNS rebinding; a bearer token from an owner-only file;
session ids that cannot be guessed; no tier 3 tools. Exposure beyond loopback also needs the
protocol's authorization flow, TLS and a threat-model review by the owner.

## 13. The agent contract

`docs/agent/omnia-agent.md` (A1.9) tells agents what the surface does, with evidence:

- Capabilities, each with an evidence tier: implemented and tested, implemented (read in
  code), planned.
- Known limitations: test-sheet format only, no simulation, follow only after W2.2.
- What an agent must not do: drive the GUI, treat the test sheet as a product format, invent
  ids, params or symbol names, edit the only copy of a file.
- The safe loop:
  1. `omnia_capabilities`.
  2. `omnia_open` or `omnia_new`, then `omnia_inspect`.
  3. `omnia_preview` the batch; read the predicted pins and ids.
  4. `omnia_apply` with `preview_id`, `expected_revision` and `idempotency_key`.
  5. `omnia_nets` to check connectivity.
  6. From A2, `omnia_render_svg`, and look at it.
  7. `omnia_save` to a new path; check the response; reopen and compare.

Every example in the contract is a transcript fixture that runs in CI. Counts of tools,
operations and error codes come from `omnia capabilities`, and a test fails when the document
and the catalog disagree. The contract names no other company or product (AGENTS rule 1).

## 14. Crates and APIs

Signatures are a sketch: names and shapes to build toward, not a frozen interface.

```rust
// ---- omnia-doc (A1.1) ----
impl Doc {
    /// Full copy: every arena slot (reserved ones included), index, connectivity and
    /// revision. History is empty. `fork() == self` under PartialEq.
    pub fn fork(&self) -> Doc;
}

// ---- omnia-symbols (A1.2) ----
impl SymbolLibrary {
    pub fn place_command(&self, id: SymbolId, at: Pt, orient: Orient,
                         designator: Arc<str>, value: Arc<str>) -> Command;
}

// ---- omnia-agent ----
pub const SCHEMA: &str = "omnia.agent/1";                    // AK7

pub struct AgentService<H: DocHost> { /* sessions, caches, policy, jobs */ }
impl<H: DocHost> AgentService<H> {
    pub fn new(host: H, policy: Policy) -> Self;
    pub fn catalog(&self) -> &Catalog;
    pub fn handle(&mut self, session: SessionId, req: Request,
                  cx: &mut CallContext) -> Response;         // synchronous, deterministic
    pub fn poll_job(&mut self, job: JobId, budget: Duration,
                    cx: &mut CallContext) -> JobState;       // the host drives jobs (A3)
}

pub trait DocHost {                       // headless store in A1; Entity<Doc> adapter in A4
    fn doc(&self, h: DocHandle) -> Result<&Doc, AgentError>;
    fn apply(&mut self, h: DocHandle, c: Command) -> Result<Change, CommandError>; // Doc::apply
    fn undo(&mut self, h: DocHandle) -> Option<Change>;
    fn redo(&mut self, h: DocHandle) -> Option<Change>;
    fn gesture_active(&self, h: DocHandle) -> bool;          // headless: always false
    fn request_approval(&mut self, a: &ApprovalRequest) -> ApprovalOutcome;
}

pub struct CallContext<'a> { pub cancel: &'a CancelToken, pub progress: &'a mut dyn ProgressSink }

pub struct Request { /* private: built only by decoding (9.1) */ }
impl Request {
    pub fn new(op: Operation, idempotency_key: Option<String>) -> Result<Request, AgentError>;
    pub fn from_slice(bytes: &[u8]) -> Result<Request, AgentError>;            // envelope
    pub fn from_arguments(op: OpName, bytes: &[u8]) -> Result<Request, AgentError>; // MCP
    pub fn op(&self) -> &Operation;
    pub fn idempotency_key(&self) -> Option<&str>;
    pub fn to_json(&self) -> String;
    pub fn to_arguments(&self) -> String;
}
pub enum Operation { Capabilities, New, Open(OpenArgs), Close(CloseArgs), Inspect(InspectArgs),
                     Query(QueryArgs), Nets(NetsArgs), Validate(EditArgs), Preview(EditArgs),
                     Apply(ApplyArgs), Undo(RevArgs), Redo(RevArgs), Save(SaveArgs),
                     RequestStatus(KeyArgs), JobStatus(JobArgs), JobCancel(JobArgs) }
pub struct Response { /* private: schema, doc_id, revision, outcome, warnings */ }
impl Response {
    pub fn new(doc_id: Option<DocId>, revision: Option<u64>,
               outcome: Result<Output, AgentError>) -> Result<Response, InvalidResponse>;
    pub fn with_warnings(self, warnings: Vec<Warning>) -> Response;
    pub fn from_slice(op: OpName, bytes: &[u8]) -> Result<Response, serde_json::Error>;
    pub fn schema(&self) -> &'static str;
    pub fn doc_id(&self) -> Option<DocId>;
    pub fn revision(&self) -> Option<u64>;
    pub fn outcome(&self) -> &Result<Output, AgentError>;
    pub fn warnings(&self) -> &[Warning];
    pub fn to_json(&self) -> String;
}
pub enum RequestStatus { NotRun, Applied(StoredResponse), Failed { error: AgentError } }
impl StoredResponse {                     // a success of a keyed operation, checked
    pub fn new(op: OpName, revision: u64, response: Response)
        -> Result<StoredResponse, InvalidResponse>;
}
```

Files:

```
crates/omnia-agent/
  Cargo.toml        omnia-doc, omnia-symbols, omnia-theme (A2.4), serde, serde_json, rustc-hash
  schema/           committed JSON Schemas (AINV11)
  src/lib.rs        module lines, SCHEMA
  src/schema/       DTOs, id/point/orient codecs                     (A1.3)
  src/error.rs      codes, CommandError mapping                      (A1.3)
  src/catalog.rs    capabilities = dispatch table                    (A1.3)
  src/read.rs       summary, items, query modes, nets                (A1.4)
  src/service.rs    sessions, revisions, idempotency                 (A1.5)
  src/ops.rs        operation -> Command                             (A1.5)
  src/preview.rs    fork + apply                                     (A1.5)
  src/diff.rs       dirty-bounded diff                               (A1.5)
  src/host.rs       DocHost, headless store                          (A1.5)
  src/files.rs      roots, staged writes                             (A1.6)
  src/approval.rs   tiers, digests, grants                           (A1.6)
  src/plan_tools.rs route, snap, hit                                 (A2.1)
  src/render.rs     SVG render                                       (A2.4)
  src/jobs.rs       job model                                        (A3.1)
crates/omnia-cli/
  Cargo.toml        omnia-agent, serde_json
  src/main.rs       argument parsing, written by hand                (A1.7)
  src/call.rs       one-shot request                                 (A1.7)
  src/jsonl.rs      JSON-lines session                               (A1.7)
  src/mcp.rs        MCP over stdio                                   (A1.8)
  tests/transcripts/ exact request/response fixtures                 (A1.8)
docs/agent/         the contract, evidence, changes                  (A1.9)
```

Command line:

| Command | What it does |
| --- | --- |
| `omnia capabilities` | Prints the catalog |
| `omnia schema [NAME]` | Prints a JSON Schema |
| `omnia call --root DIR '<request>'` | One request, one response; exit 0 success, 1 tool error, 2 usage error |
| `omnia serve --root DIR` | JSON-lines session on stdin and stdout |
| `omnia mcp --root DIR` | MCP server on stdio |

## 15. Dependencies and risks

No new dependency for A1 to A3. `serde`, `serde_json` and `rustc-hash` are already in
`Cargo.lock` and in canvas plan section 10, which A0.1 updated with the agent crates' use. A
dependency not listed there needs its licence logged before it is added (AGENTS.md).

| Risk | Mitigation | Early test |
| --- | --- | --- |
| The protocol changes between revisions | Pin and negotiate (AK1); one adapter file; transcript tests | A1.8 |
| A fork doubles the document's memory for a moment | Measure against B9 (AB2, AB3); in the app take forks off the paint path | A1.1, A1.10 |
| Agent edits disturb a gesture or a frame | Gesture refusal (AINV13); work between frames; AB3 on the reference machine | A4.2, A4.5 |
| The plan sketch drifts from the code | Schema follows the code; new fields only as their wiring tasks merge | A2.2, A2.3 |
| The contract drifts from the code | Generated counts; transcript fixtures in CI (AINV11) | A1.9 |
| Document text steers the model | AD14; typed fields only | A1.8 |
| An agent damages the user's only copy | Tier 3 for overwrites; staged writes; the contract's safe loop | A1.6 |

## 16. Open questions

Each has a default. Build to the default; the owner's answer replaces it here.

| ID | Question | Default | Answer (2026-11-20) |
| --- | --- | --- | --- |
| AQ1 | Hand-written JSON-RPC over stdio, or an MCP SDK crate (which pulls in an async runtime)? | Hand-written. Revisit at A5. | Default kept: hand-written. The stateless era (10.5) needs no async runtime either. |
| AQ2 | Which protocol revisions are pinned (AK1)? | The newest stable revision at A0.2, down to 2025-06-18 | **Answered:** both eras, 2026-07-28 down to 2025-06-18, as pinned in AK1 and 10.5. Chosen over handshake-only and stateless-only. |
| AQ3 | Adopt a crate that generates JSON Schemas from types? | No: a hand-written schema module and a no-diff test | Default kept. |
| AQ4 | Headless approval policy | Tier 1 allowed; tier 2 inside roots allowed; tier 3 needs the echoed digest | Default kept. |
| AQ5 | Id string format | `part:3`, `wire:7`, `label:2` | Default kept. |
| AQ6 | Units besides nanometres? | Nanometres only; capabilities report `grid_nm` | Default kept. |
| AQ7 | In app mode, does every agent edit need the person's approval? | Yes, per request, until the person turns on "approve undoable edits for this session" | Default kept. |
| AQ8 | AGENTS rule 1 and the protocol's name | Name MCP as an open protocol standard; name no vendor or product | Default kept. |
| AQ9 | Crate split | `omnia-agent` (library) and `omnia-cli` (binary `omnia`) | Default kept. |
| AQ10 | Preview and approval hash | FxHash locally; SHA-256 (new dependency, licence review) only if A5 is built | Default kept. |
| AQ11 | Idempotency retention | AK5 entries per session, in memory, not persisted | Default kept. |
| AQ12 | File format for agents | RON v2 test sheets only, labelled as not a product format, until a file plan exists | Default kept. `docs/file-format-proposal.md` now exists but is a proposal; agents move to the product format only when that proposal is decided and a task adds it to the schema. |
| AQ13 | Build a network transport? | No (HA3) | Default kept: no. HA3 still decides it before any A5 work. |
| AQ14 | May A1.1 change `omnia-doc` while T2.8 (`blocked`) lists the crate as owned? | Yes: A1.1 touches `fork` and `Clone` derives only, disjoint from T2.8's open measurement | Default kept. Moot since T2.8 is `done`: no other task now lists all of `omnia-doc` as owned. |
| AQ15 | Map jobs onto the experimental core Tasks of 2025-11-25, or the 2026-07-28 tasks extension? | No: job tools in every era (11.5) | **Answered (owner-delegated):** default adopted. One path in every era; Tasks are experimental in 2025-11-25 and only an optional extension in 2026-07-28. Revisit if a pinned revision makes tasks core and stable. |

HA1 was held on 2026-11-20: the owner delegated it to the agent, asked only AQ2 (because
the newest revision changed what the default meant), and kept every other default. AQ15 was
added after HA1, when review found that 2025-11-25 has experimental core Tasks; the owner
delegated it the same day.

## 17. Changes to other files

**Applied 2026-11-17 by A0.1.**

| Where | Change |
| --- | --- |
| AGENTS.md, sources of truth | `mcp-plan.md` beside the plans, `mcp-roadmap.md` beside the roadmaps; `A`-prefixed ids live in the agent files |
| ROADMAP.md | Links the agent plan and roadmap |
| canvas-roadmap.md, Lanes | Row `agent` |
| canvas-plan.md section 10 | `serde_json` and `rustc-hash` use extended to the agent crates |
