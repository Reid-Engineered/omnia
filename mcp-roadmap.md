# Agent control: roadmap for agents

The task list for the agent surface. The reference for every `A`-prefixed id used here (`AB`,
`AK`, `AD`, `AINV`, `ATC`, `AQ`, `HA`) is [mcp-plan.md](mcp-plan.md). Ids without an `A` are in
the canvas and wiring files.

This roadmap runs beside the canvas and wiring roadmaps. Most of milestones A1 to A3 touch only
the new `agent` lane; the table under "Where each task fits" says which tasks wait for canvas or
wiring work.

## How to work a task

The same steps as the canvas roadmap: pick a `todo` task whose dependencies are `done`, mark it
`doing`, read what **Read** names, do **Do** inside **Owns**, run the checks, mark it `done`
and add a log entry at the end of this file.

Statuses: `todo`, `doing`, `done`, `blocked (reason)`, `needs-human`.

## Rules

Canvas roadmap rules 1 to 12 and wiring roadmap rules 13 to 16 all apply, with branch names
`task/<id>-<slug>`, for example `task/A1.1-doc-fork`. Three more:

17. **Commands only.** Agent code edits a document only through `Doc::apply`, `undo` and
    `redo` (AD2, AINV1). If a task seems to need another mutator, stop and log it.
18. **The agent surface never touches the project process.** No tool, command or fixture
    edits plans, roadmaps or measurements, runs a gate or answers a checkpoint (AD10).
19. **Keep three plans true.** A task that changes a constant, decision or signature updates
    mcp-plan.md in the same pull request, and the canvas or wiring plan too if section 17 of
    mcp-plan.md lists the change.

## Standard checks

As in AGENTS.md: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo clippy -p omnia-bench --all-targets --features scene-counts,readback -- -D warnings`,
`cargo test --workspace`, `cargo deny check`.

## Milestones

| # | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| A0 | mcp-plan.md and this roadmap; owner decisions; all plans consistent | A0.2 | HA1 |
| A1 | Headless read, validate, preview, apply, undo, open and save over MCP and the command line | A1.10 | AB1, AB2, AINV1 to AINV12, HA2 |
| A2 | Planner-backed tools; follow and delete-with-wires parity; SVG render | A2.5 | — |
| A3 | Jobs, progress, cancellation, subscriptions | A3.4 | — |
| A4 | App-hosted mode: shared document, in-app approval | A4.5 | AB3, AINV13, HA4, HA5 |
| A5 | Network transport | — | GATED (HA3) |

## Where each task fits

| Agent task | Waits for (canvas, wiring) | Can start |
| --- | --- | --- |
| A0.1, A0.2 | nothing | now |
| A1.1 to A1.10 | nothing beyond HA1 | after HA1 |
| A2.1, A2.4 | nothing (W1.3 and W1.5 are done) | after A1.10 |
| A2.2 | W2.2 | after W2.2 |
| A2.3 | W3.5 | after W3.5 |
| A3.x | nothing | after A1.10 |
| A4.1 | T4.3, T5.2 | after T5.2 |
| A4.3 | T5.3 | after T5.3 |

## Dependency overview

```
A0.1 -> A0.2 [HA1]
HA1 -> A1.1 (doc), A1.2 (assets), A1.3 (agent)
A1.3 -> A1.4
A1.1 + A1.2 + A1.4 -> A1.5 -> A1.6 -> A1.7 -> A1.8 -> A1.9 -> A1.10 [HA2]
A1.10 -> A2.1, A2.4       W2.2 + A1.10 -> A2.2       W3.5 + A1.10 -> A2.3
A2.1 + A2.2 + A2.3 + A2.4 -> A2.5
A1.10 -> A3.1 -> A3.2 -> A3.3 -> A3.4
T4.3 + T5.2 + A3.4 -> A4.1 [HA4] -> A4.2 -> A4.3 (+T5.3), A4.4 -> A4.5 [HA5]
HA3 -> A5 (not planned in tasks until HA3 opens it)
```

The `doc` task A1.1 and the `assets` task A1.2 have no UI framework dependency and can run
beside the wiring `doc` lane, but neither may touch `command.rs`, which W2.2, W3.2 and W3.5
own in turn.

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| HA1 | A0.2 | Answers AQ1 to AQ14 or keeps their defaults; turns AD1 to AD14 from PROPOSED to DECIDED or amends them. Milestone A1 does not start before HA1. |
| HA2 | A1.10 | Runs the mcp-plan.md section 13 loop from an MCP client of their choice on a throwaway sheet and records the transcript. About half an hour. |
| HA3 | Before any A5 work | Decides whether a network transport is built, and its exposure. |
| HA4 | A4.1 | Approves the in-app approval dialog and the agent-preview ghosts. |
| HA5 | A4.5 | Runs the AB3 campaign on the reference machine (canvas plan section 2.1). |

---

## Milestone A0: plan and decisions

### A0.1 Agent plan and roadmap · done
- Lane: agent · Depends: none · Read: AGENTS.md; canvas-plan.md sections 5 to 10; wiring-plan.md section 10
- Owns: mcp-plan.md, mcp-roadmap.md; AGENTS.md (sources of truth); ROADMAP.md (links);
  canvas-roadmap.md (Lanes row `agent`); canvas-plan.md section 10 (use column)
- Do: write both files; apply mcp-plan.md section 17; scan for names of other companies and
  their products (AGENTS rule 1).
- Done when: every `A`-prefixed id is defined once and every reference resolves; no statement
  contradicts a `D`, `WD`, `INV` or `WINV`; the rule-1 scan is logged.

### A0.2 Owner decisions · done → HA1
- Lane: any · Depends: A0.1 · Read: mcp-plan.md sections 6 and 16
- Owns: mcp-plan.md sections 4 (AK1), 6 and 16
- Do: set this task to `needs-human`. The owner answers AQ1 to AQ14 and reviews AD1 to AD14.
  Record each answer with its date, the way canvas Q4 was recorded. Pin AK1 from the published
  protocol specification.
- Done when: every AQ has an answer or a kept default; every AD is DECIDED or amended; AK1
  lists the pinned revisions.

## Milestone A1: headless read, preview, apply, undo

### A1.1 Doc::fork · todo
- Lane: doc · Depends: A0.2 · Read: D2, D8, INV4, INV10, AD3, AQ14
- Owns: `Doc::fork` in `crates/omnia-doc/src/doc.rs`; `Clone` derives in `arena.rs`,
  `points.rs`, `index.rs`, `connectivity.rs`
- Do: `pub fn fork(&self) -> Doc`: every arena slot (reserved ones included), the index, the
  connectivity and the revision; empty history.
- Done when: a property test over the T5.1 random command generator shows `fork == original`
  (PartialEq), and that applying the same command to both gives equal `Change`s, `PartialEq`
  documents and INV10; a criterion bench records fork time and size on `10k` (non-reference
  machine, labelled).

### A1.2 Place-part helper · todo
- Lane: assets · Depends: A0.2 · Read: K2, K7, AD11, canvas-plan.md section 9 (`PlacePart`)
- Owns: `crates/omnia-symbols/src/lib.rs` (`SymbolLibrary::place_command`)
- Do: build `Command::PlacePart` from a `SymbolId`, position, orientation, designator and
  value, with local pins and bounds from the `SymbolDef`. Log for T5.3 that the place tool
  should call it.
- Done when: for every built-in symbol under all 8 orientations, applying the command gives the
  world pins `Doc::pins` reports, equal to the `SymbolDef` pins transformed by `Orient::apply`.

### A1.3 Agent crate, schema, errors, catalog · todo
- Lane: agent · Depends: A0.2 · Read: mcp-plan.md sections 4, 6, 9 and 14; AINV8, AINV10, AINV11
- Owns: `crates/omnia-agent/` (`Cargo.toml`, `src/lib.rs`, `src/schema/`, `src/error.rs`,
  `src/catalog.rs`, `schema/`); the crate-list line of the INV12 test in `xtask/src/main.rs`
- Do: DTOs that reject unknown fields; the operation union; id, point and orientation codecs;
  error codes with the exhaustive `CommandError` mapping; the catalog; the schema module and
  the committed JSON Schemas.
- Done when: AINV8 (the INV12 test covers the new crates), AINV10 and AINV11 pass; property
  tests round-trip every DTO through JSON.

### A1.4 Read projections · todo
- Lane: agent · Depends: A1.3 · Read: D3, WD8, AD8, AINV5
- Owns: `crates/omnia-agent/src/read.rs`
- Do: summary; items paged in id order; query modes over `Doc::query`, `query_within` and
  `query_touching`; canonical nets; symbols.
- Done when: nets equal a brute-force partition of pins, wires and labels; a sheet rebuilt in
  reverse insertion order gives byte-identical output up to the id mapping (AINV5);
  ATC-read-01 and ATC-net-01 pass.

### A1.5 Service core · todo
- Lane: agent · Depends: A1.1, A1.2, A1.4 · Read: mcp-plan.md section 11; INV4; AD2 to AD5; AINV1 to AINV7
- Owns: `crates/omnia-agent/src/{service.rs, ops.rs, preview.rs, diff.rs, host.rs}`
- Do: sessions; the headless `DocHost`; operation-to-`Command` conversion; fork preview; the
  dirty-bounded diff; `expected_revision`; `preview_id`; the idempotency cache;
  `request_status`; revision-checked undo and redo.
- Done when: property tests over random sheets and random operation batches pass for AINV2,
  AINV3, AINV4, AINV6 and AINV7; a grep test shows `omnia-agent` calls no `Doc` mutator but
  `apply`, `undo` and `redo` (AINV1); ATC-edit-01 to ATC-edit-06 pass.

### A1.6 Files, roots and approvals · todo
- Lane: agent · Depends: A1.5 · Read: mcp-plan.md section 12.1; AD7; AINV9, AINV12
- Owns: `crates/omnia-agent/src/{files.rs, approval.rs}`
- Do: root canonicalisation; RON v2 open within AK2; staged save; the tier policy; effect
  digests.
- Done when: the AINV12 property test passes with `..`, absolute paths and symlinks, in both
  Windows and POSIX forms; AINV9 holds for overwriting and for discarding unsaved changes;
  ATC-save-01 and ATC-path-01 pass; a write that fails half-way keeps the old file.

### A1.7 Command line · todo
- Lane: agent · Depends: A1.6 · Read: mcp-plan.md sections 9.1 and 14
- Owns: `crates/omnia-cli/` (`Cargo.toml`, `src/main.rs`, `src/call.rs`, `src/jsonl.rs`)
- Do: `omnia capabilities`, `omnia schema`, `omnia call`, `omnia serve`; exit codes 0, 1, 2;
  logs to stderr only.
- Done when: transcript tests show the same request gives byte-identical results through
  `call` and `serve`.

### A1.8 MCP over stdio · todo
- Lane: agent · Depends: A1.7 · Read: mcp-plan.md section 10; AK1; AQ1
- Owns: `crates/omnia-cli/src/mcp.rs`, `crates/omnia-cli/tests/transcripts/`
- Do: both protocol eras per mcp-plan.md section 10.5 (amended at A0.2): Omnia's one-era-per-
  process selection on the first valid request; `initialize` and `ping` for the handshake era;
  per-request `_meta` revision and client capabilities, `server/discover`, caching hints and
  `UnsupportedProtocolVersionError` for the stateless era; `tools/list`
  with input and output schemas and annotations; `tools/call` with `structuredContent` and a
  text block; resources (list, read, and subscribe in each era's form); prompts; logging;
  `notifications/cancelled`. Read the pinned specification pages first.
- Done when: ATC-mcp-01 and ATC-mcp-02 pass as exact transcripts on the CI runner; the
  prompts each have a fixture.

### A1.9 Agent contract and evidence · todo
- Lane: agent · Depends: A1.8 · Read: mcp-plan.md section 13; AGENTS rule 1
- Owns: `docs/agent/omnia-agent.md`, `docs/agent/evidence.md`, `docs/agent/CHANGES.md`
- Do: write the contract; point every example at a transcript fixture; take counts from the
  catalog.
- Done when: a test fails if a documented tool, operation or error code is missing from the
  catalog, or the reverse; the rule-1 scan passes.

### A1.10 Gate: milestone A1 · todo → HA2
- Lane: bench · Depends: A1.9
- Owns: `crates/omnia-bench/benches/agent.rs`, `docs/measurements/A1.10.md`
- Do: run and record:

  | Budget | Command | Pass |
  | --- | --- | --- |
  | AB1 | `inspect` summary on `10k` | p99 < 5 ms |
  | AB2 | `preview` of a 100-part `Move` on `10k` | p99 < 25 ms; extra memory recorded against B9 |

  Then every AINV test of milestone A1 at 10× the default case count. Then set `needs-human`
  for HA2.
- Done when: AB1 and AB2 pass on the reference machine (otherwise record and leave
  `needs-human`); the HA2 transcript is in the log.

## Milestone A2: planner-backed tools and parity

### A2.1 Route, snap and hit tools · todo
- Lane: agent · Depends: A1.10 · Read: WD4, WD5, WINV10
- Owns: `crates/omnia-agent/src/plan_tools.rs`
- Do: `omnia_route`, `omnia_snap`, `omnia_hit` over `omnia_doc::route`, `snap` and `hit_test`.
- Done when: results equal direct calls on the same document; WINV10 holds for every returned
  route.

### A2.2 Follow on move and reorient · todo
- Lane: agent · Depends: W2.2, A1.10 · Read: WD3, WK3, WK10
- Owns: `crates/omnia-agent/src/ops.rs` (the `invert_follow` field), the schema files
- Do: `invert_follow` on `move` and `reorient`, built into a `FollowPolicy` from the session's
  `WiringPrefs`; `follow_no_route` with its `Blocker`.
- Done when: TC-move-01 to TC-move-05 pass through the agent surface with the same documents
  the canvas commands give.

### A2.3 Delete with wires · todo
- Lane: agent · Depends: W3.5, A1.10 · Read: WD10
- Owns: `crates/omnia-agent/src/ops.rs` (the `with_wires` field), the schema files
- Done when: TC-del-01 to TC-del-03 pass through the agent surface.

### A2.4 SVG render · todo
- Lane: agent · Depends: A1.10 · Read: INV6, K5 to K8
- Owns: `crates/omnia-agent/src/render.rs`
- Do: `omnia_render_svg` from `SymbolArt::write_svg`, wires and labels, with colours from
  `omnia-theme` tokens only.
- Done when: output is byte-identical across runs; a golden SVG test passes in both themes; a
  colour-literal scan of `render.rs` finds none.

### A2.5 Gate: milestone A2 · todo
- Lane: bench · Depends: A2.1, A2.2, A2.3, A2.4
- Do: the A1.10 table again; every AINV test at 10× the default case count.
- Done when: all pass on the reference machine; otherwise record and set `needs-human`.

## Milestone A3: jobs

### A3.1 Job model · todo
- Lane: agent · Depends: A1.10 · Read: mcp-plan.md section 11.5; D8
- Owns: `crates/omnia-agent/src/jobs.rs`
- Done when: cancelling at every step leaves the document `==`; a commit after a revision
  change is refused with `stale_revision`; every terminal state is reached in a test.

### A3.2 Progress, cancellation and subscriptions over MCP · todo
- Lane: agent · Depends: A3.1 · Read: mcp-plan.md section 10.4
- Owns: `crates/omnia-cli/src/mcp.rs` (jobs and subscriptions)
- Done when: transcript tests cover progress, cancellation and resource-updated notifications,
  in both protocol eras (`resources/subscribe` and `subscriptions/listen`, section 10.5).

### A3.3 First long job: large atomic batch · todo
- Lane: agent · Depends: A3.2
- Done when: progress never decreases; cancelling before the commit leaves the document `==`.

### A3.4 Gate: milestone A3 · todo
- Lane: bench · Depends: A3.3
- Done when: the A1.10 table and every AINV test at 10× pass on the reference machine;
  otherwise record and set `needs-human`.

## Milestone A4: app-hosted mode

### A4.1 App host design · todo → HA4
- Lane: canvas · Depends: T4.3, T5.2, A3.4 · Read: mcp-plan.md sections 11.6 and 12.2; D7, D8, INV5, INV11
- Owns: mcp-plan.md section 11.6
- Do: specify the approval dialog, the agent ghosts and the gesture rule; set `needs-human`.
- Done when: the owner has approved the design at HA4.

### A4.2 App host and gesture gate · todo
- Lane: canvas · Depends: A4.1
- Owns: `crates/omnia-app/src/agent_host.rs`
- Done when: AINV13 passes as a simulated-input test: a mutation during a drag is refused with
  `user_gesture_in_progress` and the revision does not change.

### A4.3 Agent ghosts and approval dialog · todo
- Lane: canvas · Depends: A4.2, T5.3
- Owns: `crates/omnia-canvas/src/overlay/agent.rs`, `crates/omnia-app/src/approval.rs`
- Done when: the INV11 paint counter does not move while ghosts change; goldens exist in both
  themes (Windows).

### A4.4 Local channel bridge · todo
- Lane: agent · Depends: A4.2
- Owns: `crates/omnia-cli/src/bridge.rs`, `crates/omnia-app/src/agent_ipc.rs`
- Done when: tests show the bridge refuses a token file that is a symlink, not a plain file or
  readable by others, with no fallback path.

### A4.5 Gate: milestone A4 · todo → HA5
- Lane: bench · Depends: A4.3, A4.4
- Owns: the `--agent-load` option of `bench run`, `docs/measurements/A4.5.md`
- Do: prepare the AB3 campaign as one command and set `needs-human` for HA5.
- Done when: B3, B5 and B9 under agent load are recorded from the reference machine by the
  owner; a miss is recorded as a miss.

---

## Log

Append one entry per finished or blocked task. Newest last.

```
<date> <task id> <status> — what was done; numbers with hardware; deviations from the plan;
                             anything the next task needs to know.
```

2026-11-17 A0.1 done — wrote mcp-plan.md and mcp-roadmap.md from a review of AGENTS.md,
ROADMAP.md, both canvas and both wiring files, the workspace manifests and the omnia-doc,
omnia-symbols, omnia-canvas and omnia-app sources at f4fd588. Applied mcp-plan.md section 17:
AGENTS.md sources of truth and id prefixes, ROADMAP.md links, the canvas roadmap's `agent`
lane, and canvas plan section 10's use column for serde_json and rustc-hash. Findings for the
owner: the code's `Command` lacks the `follow`, `with_wires`, `DragSegment` and `Paste` forms
the canvas plan sketches (schema v1 follows the code); `Doc` has no fork or snapshot (A1.1);
T2.8 lists all of `omnia-doc` as owned while blocked (AQ14). No id collides with an existing
one. Rule-1 scan: the new and changed files name no other company or product; MCP is named as
an open protocol standard (AQ8). Docs only; nothing measured; no dependency added.

2026-11-20 A0.2 done — HA1 held: the owner delegated it to the agent this date. Every AD
(AD1 to AD14) is DECIDED as written. AQ1 and AQ3 to AQ14 keep their defaults; AQ12 notes the
new `docs/file-format-proposal.md` (still a proposal, so RON test sheets stay); AQ14 is moot
now that T2.8 is `done`. AQ2 was put to the owner, because the published specification now
names 2026-07-28 as current, and that revision is stateless (no `initialize`, version in each
request's `_meta`, `server/discover`, no `ping`, `subscriptions/listen`). Answer: both eras.
AK1 pins 2026-07-28, 2025-11-25 and 2025-06-18. With the owner's approval A0.2 also amended
sections 10 (new 10.5, protocol eras), 11.1 and 11.5 of mcp-plan.md and the A1.8 and A3.2 task
text, so the plan stays true (AGENTS "keep the plans true"). 11.5 records the task timeline:
none in 2025-06-18, experimental Tasks in the 2025-11-25 core, an optional extension in
2026-07-28. Its existing rule therefore maps jobs onto Tasks only in a 2025-11-25 session that
negotiates them; job tools everywhere else. Independent review (a model other than the
author) checked 10.5, AK1 and 11.5 against the published specification. Fixed from it: the
2025-11-25 Tasks claim; progress and cancel share one stdio channel each way; the envelopes,
not the payloads, differ by era; the full caching-hint list; required `clientCapabilities`
metadata and the `-32602` and `data.requested` error details; era selection labelled as Omnia's
policy; "session" in the stateless era as a storage lifetime only; protocol logging deprecated
in 2026-07-28. Next: A1.8 reads the pinned specification pages before coding; 10.5 is a
summary. Rule-1 scan done on the changed files. Docs only; nothing measured; no dependency
added.
