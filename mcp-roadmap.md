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

### A1.1 Doc::fork · done
- Lane: doc · Depends: A0.2 · Read: D2, D8, INV4, INV10, AD3, AQ14
- Owns: `Doc::fork` in `crates/omnia-doc/src/doc.rs`; `Clone` derives in `arena.rs`,
  `points.rs`, `index.rs`, `connectivity.rs`; `crates/omnia-bench/benches/fork.rs` and its
  `[[bench]]` entry; `docs/measurements/A1.1.md` and `docs/measurements/A1.1/` (widened
  2026-11-20 by owner-delegated decision: the done-when bench needs them)
- Do: `pub fn fork(&self) -> Doc`: every arena slot (reserved ones included), the index, the
  connectivity and the revision; empty history.
- Done when: a property test over the T5.1 random command generator shows `fork == original`
  (PartialEq), and that applying the same command to both gives equal `Change`s, `PartialEq`
  documents and INV10; a criterion bench records fork time and size on `10k` (non-reference
  machine, labelled).

### A1.2 Place-part helper · done
- Lane: assets · Depends: A0.2 · Read: K2, K7, AD11, canvas-plan.md section 9 (`PlacePart`)
- Owns: `crates/omnia-symbols/src/lib.rs` (`SymbolLibrary::place_command`)
- Do: build `Command::PlacePart` from a `SymbolId`, position, orientation, designator and
  value, with local pins and bounds from the `SymbolDef`. Log for T5.3 that the place tool
  should call it.
- Done when: for every built-in symbol under all 8 orientations, applying the command gives the
  world pins `Doc::pins` reports, equal to the `SymbolDef` pins transformed by `Orient::apply`.

### A1.3 Agent crate, schema, errors, catalog · done
- Lane: agent · Depends: A0.2 · Read: mcp-plan.md sections 4, 6, 9 and 14; AINV8, AINV10, AINV11
- Owns: `crates/omnia-agent/` (`Cargo.toml`, `src/lib.rs`, `src/schema/`, `src/error.rs`,
  `src/catalog.rs`, `schema/`); the crate-list line of the INV12 test in `xtask/src/main.rs`
- Do: DTOs that reject unknown fields; the operation union; id, point and orientation codecs;
  error codes with the exhaustive `CommandError` mapping; the catalog; the schema module and
  the committed JSON Schemas.
- Done when: AINV8 (the INV12 test covers the new crates), AINV10 and AINV11 pass; property
  tests round-trip every DTO through JSON.

### A1.4 Read projections · done
- Lane: agent · Depends: A1.3 · Read: D3, WD8, AD8, AINV5
- Owns: `crates/omnia-agent/src/read.rs`
- Do: summary; items paged in id order; query modes over `Doc::query`, `query_within` and
  `query_touching`; canonical nets; symbols.
- Done when: nets equal a brute-force partition of pins, wires and labels; a sheet rebuilt in
  reverse insertion order gives byte-identical output up to the id mapping (AINV5);
  ATC-read-01 and ATC-net-01 pass.

### A1.5 Service core · done
- Lane: agent · Depends: A1.1, A1.2, A1.4 · Read: mcp-plan.md section 11; INV4; AD2 to AD5; AINV1 to AINV7
- Owns: `crates/omnia-agent/src/{service.rs, ops.rs, preview.rs, diff.rs, host.rs}`
- Do: sessions; the headless `DocHost`; operation-to-`Command` conversion; fork preview; the
  dirty-bounded diff; `expected_revision`; `preview_id`; the idempotency cache;
  `request_status`; revision-checked undo and redo.
- Done when: property tests over random sheets and random operation batches pass for AINV2,
  AINV3, AINV4, AINV6 and AINV7; a grep test shows `omnia-agent` calls no `Doc` mutator but
  `apply`, `undo` and `redo` (AINV1); ATC-edit-01 to ATC-edit-06 pass.

### A1.6 Files, roots and approvals · done
- Lane: agent · Depends: A1.5 · Read: mcp-plan.md section 12.1; AD7; AINV9, AINV12
- Owns: `crates/omnia-agent/src/{files.rs, approval.rs, service.rs}`
- Do: root canonicalisation; RON v2 open within AK2; staged save; the tier policy; effect
  digests.
- Done when: the AINV12 property test passes with `..`, absolute paths and symlinks, in both
  Windows and POSIX forms; AINV9 holds for overwriting and for discarding unsaved changes;
  ATC-save-01 and ATC-path-01 pass; a write that fails half-way keeps the old file.

### A1.7 Command line · done
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
  in both protocol eras (`resources/subscribe` and `subscriptions/listen`, section 10.5). An
  AQ15 assertion for each AK1 revision: no `capabilities.tasks` in the 2025-11-25 or 2025-06-18
  `initialize` result, and no `io.modelcontextprotocol/tasks` extension in 2026-07-28's
  `server/discover`.

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

2026-11-20 A1.1 done — `Doc::fork` in doc.rs copies all three arenas (reserved slots
included), the point map, the spatial index, the connectivity and the revision, and leaves the
history empty (AD3). `Clone` is derived on `Points`, the index's `Entry`, `Entries` and
`SpatialIndex`, and the connectivity's `PointSlot`, `PointStore`, `Comp`, `Net` and
`Connectivity` (`Arena` was already `Clone`). Tests in doc.rs: a 128-case property test over the
T5.1 generator, which checks at every reached state (undo and redo included) that
`fork == original`, that the derived state is the same (partitions, sheet query order,
junctions), and that the same command on both gives equal `Result<Change, _>`, `PartialEq`
documents and INV10 on each. Two unit tests cover reserved-slot id allocation and the original
staying untouched. Deviations: the T5.1 generator is private to `command.rs`, which A1.1 may not
touch, so the test mirrors it with the same steps, weights, ranges and mapping. The done-when
bench needed `crates/omnia-bench/benches/fork.rs` and its `[[bench]]` entry, outside **Owns**.
Bench, non-reference (Windows development laptop, i7-14650HX, on battery, Silent scheme; certifies
nothing): fork of `10k`, median of run p99s 9.419 ms (range 8.222 to 13.055) over 10 runs, and
17,296,160 incremental live heap bytes in 129,671 allocations per live fork
(docs/measurements/A1.1.md). Independent review (a model other than the author): no defect in
`fork`; added checks after undo and redo steps and a test of the unsorted junction order and
exact `NetId`s through a fork; relabelled the size. The owner delegated the follow-up
decisions: the **Owns** line was widened to the bench and measurement files, and the campaign
scripts stay as run. For
A1.10: AB2's 25 ms includes this fork, and the fork's memory goes against B9. mcp-plan.md
section 3's "no Clone, no fork" finding is now resolved by this task. No dependency added.

2026-11-20 A1.2 done — `SymbolLibrary::place_command(id, at, orient, designator, value) ->
Command` in `crates/omnia-symbols/src/lib.rs` builds `Command::PlacePart`, taking the local pins
(in pin order) and the bounds from the `SymbolDef` (AD11). The signature is as in mcp-plan.md
section 14. Test `a1_2_place_command_world_pins_for_every_symbol_and_orientation`: for every
built-in symbol under all 8 `Orient::ALL`, the applied command is accepted. `Doc::pins` equals
the `SymbolDef` pins mapped by `at + Orient::apply`, in order; the stored `Part` is the one
built; the command carries `def.bounds`. For T5.3: the canvas place tool (`tool/place.rs`)
builds its `PlacePart` with `place_command`, so the tool and the agent share one helper. The two
hand-built `PlacePart`s in `crates/omnia-canvas/src/sheet.rs` (about lines 612 and 752) are test
fixtures and may switch when that file's owner next touches it. Independent review (a model
other than the author) found no bug; it led to a doc note that an unknown `SymbolId` panics, as
`get` does, and to this corrected handoff. No dependency added; nothing measured.

2026-11-20 AQ15 (docs, not a task) — the owner delegated the question that A0.2's review
raised. Answer: job tools in every era, and tasks never advertised. This supersedes the A0.2
entry's per-era mapping of jobs onto 2025-11-25 Tasks. mcp-plan.md section 11.5 now states it
directly, without the conditional rule. Section 10.4 scopes progress and cancellation to the
in-flight request, and A3.2 asserts AQ15 for each pinned revision. Independent review by a
model other than the author. Docs only.

2026-11-21 A1.3 done — new crate `crates/omnia-agent` (deps `omnia-doc`, `serde`, `serde_json`;
dev `proptest`; all already in Cargo.lock and canvas plan section 10, so no licence to log).
`lib.rs` holds `SCHEMA` (AK7) and the AK1 to AK6 constants. `schema/` has the codecs (`part:3`,
`wire:7`, `label:2`, `d1`, `j1`, `pv-` plus 16 hex digits, points `[x, y]`, bounds
`{min, max}`, the AD12 orientation names), the strict request decoder, the operation union and
its params, the section 9.2 edit operations, every result DTO, and the hand-written JSON Schema
module. `error.rs` has 33 codes with typed details; `command_error_kind` maps `CommandError`
with no wildcard arm. `catalog.rs` is the dispatch table: `OpName::ALL` and `EditOpName::ALL`
come from the same macro list as their enums, and `spec` gives each operation its tool name,
tiers, annotations and schema names by exhaustive match. 34 committed schema files under
`crates/omnia-agent/schema/`. Tests: AINV10 (one sample per `CommandError`, `FollowError` and
`Blocker` variant, distinct codes); AINV11 (committed files equal generated, generation is
deterministic and canonical, the catalog equals `OpName::ALL`, the decoder dispatches every
catalog operation, every catalog schema name exists); property tests round-trip every request,
response and error through JSON, validate each against its schema with a small test-only
validator that refuses any keyword it does not implement, and re-parse with object keys
reversed (AINV5's key-order clause); negative tests for unknown fields at each level, malformed
JSON, floats, duplicate keys and the AK2 to AK4 limits. AINV8: the INV12 test's crate list now
includes `omnia-agent`. Deviations, all written into mcp-plan.md: section 9.4 gains
`wire_capture` and `off_lattice` (the wiring tasks added `CommandError::Capture` and
`OffLattice` after the plan), makes `follow_no_route` and `segment_blocked` current, gives the
item codes their ids, and adds `nothing_to_redo`; 9.1 records the decoding rules; 9.3 records
that `modified` carries full item fields and the `validate`, `undo` and `redo` result shapes;
9.5 the file names and the bless command; 10.1 adds `cursor?` and `limit?` to `omnia_query`
(AK4 paging); section 2's `Command` row is updated. Choices the plan leaves open, for A1.4 and
A1.5 to build on or amend in mcp-plan.md: `inspect` `include` is `summary`, `items` or `nets`;
the summary is counts of parts, wires and labels; nets are keyed by `{"kind": "pin" | "wire" |
"label", ..}`; `job_status` and `job_cancel` are in the catalog now because section 14's
`Operation` lists them (until A3 the service can answer `job_not_found`), with no annotation set
since 10.1 gives none. Known looseness: the decoder accepts `null` for an optional field, which
the schemas leave out. For A1.7: the INV12 crate-list line must gain `omnia-cli` when that crate
exists (the line is A1.3's, so A1.7 needs it in its **Owns**), and JSON-lines' `"id"` must be
stripped before `Request::from_slice`, which rejects unknown envelope fields. No GUI dependency;
nothing measured.

2026-11-21 A1.3 done (review fixes) — independent review (a model other than the author)
requested changes; findings 1 to 7 fixed on the same branch, all written into mcp-plan.md
sections 9.1, 9.4, 9.5, 10.1 and 11.2. (1) MCP tool arguments are the params with
`idempotency_key` beside them on apply, undo, redo and save: new committed `<op>.arguments`
schemas are the `inputSchema`, `Request::from_arguments` and `to_arguments` decode and write
them, and both forms give the same request (ATC-edit-03); the envelope is unchanged. (2, 3) The
decoders now validate against the published schema before typed decoding, through a new
`schema/validate.rs`: a validator for exactly the emitted keywords and a small matcher for the
emitted pattern subset, with no codec shortcut. So nulls for optional fields, positional arrays
and `"error": null` are refused. Error paths are JSON Pointers found in byte order of keys,
with the deepest failing alternative, so error bytes do not depend on `preserve_order`. Only
the request size, duplicate keys, integer spelling and bounds corner ordering are
decoder-only. Property tests mutate valid requests (both forms) and responses and check that
the decoder agrees with the published schema text, and that reversed key order gives the same
error bytes. (4) `request_status` `applied {op, revision, response}`: `op` is one of the four
keyed operations, and `response` is a success of that operation, in the schema (per-op
success definitions) and in the decoder; the strategies can only generate such pairs.
(5) Schemas encode the representable ranges: exact decimal patterns up to `u32::MAX` and
`u64::MAX`, a maximum on every `u64`, and K1 bounds corners. (6) `AgentError` no longer stores
`retryable`; the wire flag comes from the code, the schema pins it per code, and the decoder
refuses a contradiction. (7) `OpSpec::available`: `job_status` and `job_cancel` keep their DTOs
but are not listed, not dispatched (`unknown_operation`) and have no committed schema until
A3.1 sets them available; A1.8's `tools/list` builds on the catalog. 44 committed schema files.
For A1.7, in addition to the entry above: when it lifts the JSON-lines `"id"` off a line, it
must keep duplicate-key detection, so it parses the line with `parse_strict` and passes the
rest to `Request::from_envelope_value`, never round-tripping through a plain JSON value first.
Checks rerun locally; no dependency added; nothing measured.
Second review round (a fresh independent review at 78db15f confirmed fixes 1 to 7 and found six
more; all fixed and written into mcp-plan.md sections 9.1 and 9.5). (1) A document's success
(`new`, `open` and the other document operations) now requires `doc_id` and `revision` in its
schema, in `response.json` and in `Response::new`; an audit found no other result missing a
field a client needs. (2) `Request`, `Response` and the new `StoredResponse` have private fields
and checked constructors, and `Bounds` holds only valid rectangles, so the API cannot build a
value its decoder refuses (for example an `applied` status tagged `undo` around a `save`
response). Property tests build values from rule-breaking parts and check that whatever is
accepted decodes back to itself. (3) Both request forms run one order of checks (limits before
the schema; the envelope's `idempotency_key` and unknown fields ordered among the params fields
by name), and a property test compares the two forms' errors on mutated inputs. (4) `-0` is
read as `0`; a lexical scan still refuses fractions and exponents. (5) The validator compares
integers exactly, also beyond 64 bits. (6) Union failures are ranked by pointer segment count,
then first. 44 schema files re-blessed. Checks rerun locally.
Third review round (a third independent review at f2277b1 confirmed round two and found two
blockers and three minor points; all fixed, written into mcp-plan.md sections 9.1, 9.5 and 14).
(1) AK2 now bounds the raw input and both canonical forms: either decoder and `Request::new`
accept a request only if `to_json` and `to_arguments` are both at most 8 MiB, so an accepted or
built request can be sent in either form. MCP arguments whose canonical encoding is exactly 8 MiB
are refused, since the envelope they imply is longer (raw input padded to 8 MiB passes when both
canonical forms fit). Boundary tests sit at exactly the limit and one byte over, for
both forms. (2) Every per-operation envelope schema forbids `result` and `error` together on its
own, apart from the document-handle rule; a differential property test grafts errors, results
and removed handles onto valid responses and checks every per-operation schema against
`Response::from_slice` (and `response.json`). Minor: duplicate-key and number refusals carry no
position, so both forms report them alike, while malformed JSON keeps the raw input's line and
column. A key on an operation that takes none is refused right after the limits in both forms,
so `request_status` with a misplaced outer key gets the MCP form's `/idempotency_key` refusal,
and a key written inside a keyed operation's params is `/params/idempotency_key`. Section 14's
sketch shows the constructors and accessors. Ownership: `crates/omnia-agent/proptest-regressions/`
is outside the subpaths A1.3's **Owns** line lists; it is read as included, since it sits inside
the owned crate and `omnia-doc` commits its regression seeds the same way (the line itself is
unchanged). The branch is updated from `main` (T2.9, #86). Checks rerun locally.

2026-11-22 A1.4 done — new `crates/omnia-agent/src/read.rs` (plus its `mod` line in `lib.rs`
and an `omnia-symbols` dependency in the crate's `Cargo.toml`, which section 14 already lists;
GPUI-free, and the INV12/AINV8 xtask test still passes). Free functions over `&Doc` through its
public API only: `summary`; `item` and `items` (every live item in canonical order, paged);
`query_ids` and `query` over `Doc::query` (`envelope`), `query_within` (WD8 Enclosed) and
`query_touching` (WD8 Intersecting), sorted and deduplicated; `inspect` (exactly what
`include` names); `all_nets` and `nets` (`at`, `wire`, or all); `symbols` and `symbol`. All
results are the A1.3 DTOs; no schema file changed. Nets are grouped by the `NetId`s of
`Doc::net_at` (pins, labels) and `Doc::net_of` (wires), which never leave the module, then
keyed per AD8 and listed in key order. Choices, written into mcp-plan.md (AD8 and a new
"Read results as built at A1.4" note in 9.3): keyset paging with the last item's id as the
cursor; a bad cursor or a `limit` of 0 is `schema_violation` at an envelope-space pointer,
over AK4 `limit_exceeded` first; `inspect`'s `area` pages the `envelope` mode's items while
summary and nets stay whole-document; `nets {at}` with nothing at the point is empty, `nets
{wire}` on a missing wire `unknown_item`; a part whose symbol id the library lacks makes an
items read `internal`; the symbol catalog DTO is `read::SymbolInfo {name, bounds, width,
height, pins}`, with no committed schema until A1.8 publishes the resource. Tests (all in
`read.rs`): nets equal an independent union-find partition of pins, wires and labels over
random lattice sheets, before and after a `Delete` renumbers the nets, with `nets.len() ==
Doc::net_count()`; AINV5: a sheet rebuilt by `Doc::from_items` with each kind in reverse order
gives byte-identical `inspect`, `query` (all three modes), `nets {at}` and `nets {wire}`
responses after the id mapping, and the same request twice or on a `fork` gives the same bytes;
ATC-read-01 on the `100` sheet's geometry saved and re-opened through RON (counts 100, 200, 0;
revision the file's); ATC-net-01 (two interior-crossing wires added in one `AddWires` are two
nets, and the crossing point has none); query modes against the `Doc` calls (property) and a
WD8 case with a pin stub outside the body; paging at every limit from 1 to 21, the AK4 default
of 500, an exactly full last page, a cursor whose item was deleted, and refused cursors and
limits. Every produced result goes through `Response::new`, is validated against the
committed `<op>.response` schema text and decodes back equal. ATC-read-01 rebuilds the `100`
sheet's geometry in the test because `omnia-bench` depends on GPUI; opening it through the
service is A1.5's. For A1.5: map read errors' `/params/...` pointers for the MCP form as the
decoder does, and resolve `doc_id` before calling these functions. Checks: fmt, workspace
clippy, omnia-bench clippy with `scene-counts,readback`, tests of omnia-agent, xtask,
omnia-doc and omnia-symbols ran locally and pass; `cargo deny` passes bans, licences and
sources and fails only advisories, on the known `RUSTSEC-2026-0592` (`url 2.5.8`), which is
being fixed separately. The full `cargo test --workspace` is left to CI. Nothing measured.

2026-11-22 A1.5 done — new `crates/omnia-agent/src/{host.rs, ops.rs, preview.rs, diff.rs,
service.rs}`, their `mod` and `pub use` lines in `lib.rs`, and `rustc-hash = "2"` in the crate's
`Cargo.toml` (section 14 lists it; already in Cargo.lock and canvas plan section 10, MIT OR
Apache-2.0; GPUI-free and runtime-free, the INV12/AINV8 xtask test passes). `host.rs`: the
`DocHost` trait (`insert`, `remove`, `doc`, `apply`, `undo`, `redo`, `gesture_active`; an
unknown handle is `document_not_found`) and `HeadlessHost`, which forwards each edit to the
`Doc` method of the same name. `ops.rs`: operations to one `Command` (one operation is its own
command, several a `Batch`), `place_part` through `SymbolLibrary::place_command` (AD11). The
follow policy and `with_wires` come from the session's WK10 `WiringPrefs` until A2.2 and A2.3
add the fields. `preview.rs`: `run` (fork plus `Doc::apply`) and `preview_id`, an FxHash of a
domain tag, `doc_id`, base revision, the operations' canonical JSON and those preferences.
`diff.rs`: the dirty-bounded diff, one page of at most 500 entries (added, then removed, then
modified). `service.rs`: `AgentService` with sessions, dispatch of every available operation,
`expected_revision` on validate, preview, apply, undo and redo, `preview_mismatch`, the AK5
idempotency cache (oldest evicted first), `request_status`, revision-checked undo and redo, AK6
on `new`, and `in_form`/`handle_in`, which rewrite envelope-space error pointers for the MCP
arguments form (A1.4's handoff). `apply` previews on a fork first, so every refusal and the
diff come before the live document moves, then commits the same command. Choices, written into
mcp-plan.md (11.2 and 11.3 "as built" notes, a new 11.4a, section 14's sketch): a success or a
non-retryable failure is stored under its key, a retryable one is not (it applied nothing and
invites a retry), so `request_status` reports `failed` only for non-retryable errors; a
preview id is not stored, since a matching hash is by construction the id a preview of the same
request returns (11.3), so apply needs no record of previews; responses about an open document
carry `doc_id` and its revision after the request, errors included, and `document_not_found`,
`idempotency_conflict`, `capabilities` and `request_status` carry neither; an unknown session
is `internal`. Interim until A1.6: `open`, `save`, and `close` of a dirty document are
`unsupported_in_mode` (discarding is tier 3); `close` of a clean document works. Deviations
from section 14's sketch, recorded there: `handle(session, &Request)` has no `CallContext` and
there is no `poll_job` (A3.1 adds both); `new(host)` takes no policy (A1.6 adds one);
`DocHost` gains `insert` and `remove`, wraps the document results in an outer
`Result<_, AgentError>` for unknown handles, and has no `request_approval` yet (A4). Tests
(31 new; omnia-agent lib now 100): property tests with 64 cases each over random sheets built
through the service and random batches of all seven operations (missing ids, unknown symbols,
off-lattice moves, zero-length wires, wrong text fields mixed in): AINV2 (preview change and
diff equal apply's, `validate`'s change equal, the after-state `==` an independent fork, the diff
equal a brute-force comparison of every item, refusals identical); AINV3 (on twin services,
reads, validate and preview, stale ones included, leave the document `==` with the same history
cursor, and the next five undo and redo responses are byte-identical); AINV4 and AINV6 (random
apply, undo and redo at revision offsets with right, wrong or no preview ids); AINV7 (a key
pool with exact replays: identical bytes, no change; conflicts; `request_status` against a
model). AINV1 is a source scan: it finds every `&mut self` method of `impl Doc` in
`omnia-doc/src`, checks that the public ones are exactly `apply`, `undo` and `redo` and that no
public function takes `&mut Doc`, then checks that no file under `omnia-agent/src` calls any of
the others. ATC-edit-01 to ATC-edit-06 are unit tests, ATC-edit-06 with the person's edit made
through `host_mut`. Every response a test produces is validated against the committed
`schema/<op>.response.json` read from disk and decodes back equal. Also unit tests for
request_status, conflicts, cache eviction at 257 keys, AK6, close, sessions, nothing to undo or
redo, error codes per refusal, capabilities, both error-pointer forms, and AINV5 (reversed key
order gives the same bytes). The service tests are inline in `service.rs` (about 1,600 lines),
because a `service/tests.rs` file would be outside **Owns**. For A1.6: wiring `open`, `save`
and dirty `close` changes `service.rs` (dispatch, a path on the session's document entry,
`saved_revision` on save), so A1.6's **Owns** needs `service.rs` beside `files.rs` and
`approval.rs`; `AgentService::adopt` (crate-private) registers an opened document under AK6;
`approval_required` is retryable and therefore never cached. For A1.7 and A1.8: one
`open_session` per process or stream, then `handle_in` with the request's `Form`. For A1.10: a
`preview` is one fork, one apply and the diff; an `apply` is that plus the commit. Checks run
locally and passing: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D
warnings`; `cargo clippy -p omnia-bench --all-targets --features scene-counts,readback -- -D
warnings`; `cargo test` of omnia-agent, omnia-doc, omnia-symbols and xtask; `cargo deny check`
(advisories, bans, licences and sources ok). The full `cargo test --workspace` is left to CI.
Rule-1 scan of the changed files done. Nothing measured.

2026-11-23 A1.6 done — new `crates/omnia-agent/src/{files.rs, approval.rs}`, wired into
`service.rs` and re-exported from `lib.rs` (no new dependencies; INV12/AINV8 xtask test passes).
`files.rs`: `Roots` canonicalises configured root directories in configured order (`Roots::empty`
refuses every path); lexical screening runs before any filesystem or network call and rejects
empty paths, NUL bytes, `..` in both POSIX `/` and Windows `\` forms, colons in non-prefix
segments (blocking ADS/drive-relative paths), and foreign absolute, UNC, device (`\\.\`), or
verbatim (`\\?\`) prefixes unless matching a configured root; top-down ancestor checks
canonicalise every existing directory inside roots so symlinks, Windows NTFS junctions, and
dangling links that escape roots all return `path_outside_root` without probing outside targets
or leaking filesystem existence. `open_doc` enforces `MAX_FILE_BYTES` (AK2) before and during
reading, maps unsupported RON versions to `unsupported_file_version`, and maps corrupt files to
`io_error` without echoing file text (AD14). `save_doc` / `write_staged_with` serialises in
memory, enforces `MAX_FILE_BYTES` (AK2), writes a unique sibling `.omnia-save-<pid>-<seq>.tmp`
file, syncs, and renames over the target, cleaning up the temp file on failure so existing files
remain untouched. `approval.rs`: `Tier`, `TierDecision`, `Policy` (`Policy::headless` implements
AQ4; `Policy::custom` supports explicit decisions including `Deny`), `GuardedEffect` with 64-bit
FxHash digests (`ed-<16 hex>`, AQ10) bound to effect kind, `doc_id`, `revision`, and canonical
path / `preview_id`, and per-session `ApprovalStore` (one-shot `ap-<n>` challenges, reusing the
active id for repeated unapproved calls so AINV5 holds, and consuming a grant only after the
guarded operation succeeds). `service.rs`: `AgentService::new(host, policy)` /
`headless_with_roots(roots)`, `open`, `save`, and dirty `close` (always requiring
`discard_unsaved: true` regardless of policy, plus matching tier-3 approval), with `DocEntry`
tracking canonical path and `saved_revision`. Updated `mcp-plan.md` sections 11.4a, 12.1 and 14
(Rule 19). Tests (`omnia-agent` lib now 113 passing): 128-case AINV12 proptest over POSIX and
Windows traversal/absolute/UNC/drive-relative/ADS/NUL and symlink/NTFS-junction escape paths;
lexical UNC/device no-probe timing test; AINV9 tests for file overwrite and dirty close
(including `Allow` without `discard_unsaved`, `Deny` on save, revision invalidation, and
one-shot consumption); ATC-save-01 (tier-2 new file, tier-3 overwrite challenge + echo +
idempotent replay, clean close, reopen + inspect); ATC-path-01 at both `Roots` and service
levels; half-way staged-write failure keeping original bytes and cleaning temp files; AK2 size
and version checks; multi-root configured-order resolution. Checks run locally and passing on
Windows: `cargo fmt --all --check`; workspace clippy `-D warnings`; `omnia-bench` clippy with
`scene-counts,readback` `-D warnings`; `cargo test -p omnia-agent -p omnia-doc -p omnia-symbols
-p xtask`; `cargo deny check`. Full `cargo test --workspace` left to CI. Rule-1 scan done.
Nothing measured.

2026-11-23 A1.7 done — new crate `crates/omnia-cli`, binary `omnia` (deps `omnia-agent` and
`serde_json`, both in canvas plan section 10 and Cargo.lock, so no licence to log; the
workspace's `crates/*` glob already makes it a member). `main.rs` parses arguments by hand and
dispatches; `call.rs` answers one envelope (argument text, or `-` for standard input) on a
session that ends with the call; `jsonl.rs` is `omnia serve`, one session per stream, one
response line per request line, flushed after each. `omnia capabilities` prints the `result`
of a `capabilities` request byte for byte; `omnia schema` lists the published names and
`omnia schema NAME` prints a committed schema's bytes. Exit codes: 0 success, 1 a tool error
(a request that does not decode included) or an I/O failure on the standard streams, 2 a usage
error (unknown command or option, missing or extra request, a `--root` that is not a
directory, an unknown schema name); `serve` exits 0 at the end of its input. Standard output
carries only results; usage errors and diagnostics go to standard error. As the A1.3 log asked,
the JSON-lines `id` is lifted after `parse_strict` and the rest goes to
`Request::from_envelope_value`, so duplicate keys (a second `id` included) are still refused;
AK2 is checked on the raw line first. Choices, written into mcp-plan.md (a new "as built at
A1.7" note after the 9.1 decoding rules, and one under section 14's command-line table): the
`id` is a string or a 64-bit integer, anything else `schema_violation` at `/id`; the response
line puts `"id"` first and is otherwise the bytes `call` prints; a line over AK2, unparseable or
with a refused `id` is answered without one; empty and whitespace-only lines are skipped;
`\r\n` reads like `\n`; reading keeps at most one byte past AK2 and reports the full length.
AINV8: the INV12 xtask crate list now includes `omnia-cli` (the change A1.3 deferred here).
Tests (9, inline in `main.rs`, run on in-process byte streams through the same `run` that
`main` calls, since `tests/transcripts/` is A1.8's): AD1, a corpus of 26 single requests
(successes, every step of 9.1's order of checks, AK2 at exactly the limit and one byte over,
AK4, ATC-path-01's `..`, a missing and a non-ASCII path, AINV5 reversed keys) gives the same
bytes through `call REQUEST`, `call -`, and `serve` with `\n`, `\r\n` or no final line end,
each validated against its operation's published response schema, with call's exit code
following the outcome; the same lines with string, positive and negative integer ids differ
only by the echo (or, when nothing can be echoed, equal `call` on the same bytes); a session
transcript (ATC-edit-01 loop with the preview id, AINV7 replay, `request_status`, ATC-edit-02,
nets, undo, redo, a tier 2 save, a tier 3 overwrite challenged and echoed, a malformed line
mid-session, close, reopen, inspect) with mixed line ends and blank lines equals, line for
line, what the service answers in-process plus the echoed ids; an oversized line is refused
with its full length and the session goes on; bad ids; `capabilities` equals the call's
result; `schema` equals every committed file (CRLF-normalised, as AINV11 reads them); 16
usage errors exit 2 with empty stdout; a closed stdout exits 1 with the reason on stderr. The
real binary was also run by hand on Windows (call, serve, capabilities, schema). Checks run
locally on Windows and passing: `cargo fmt --all --check`; `cargo clippy --workspace
--all-targets -- -D warnings`; `cargo clippy -p omnia-bench --all-targets --features
scene-counts,readback -- -D warnings`; `cargo test --workspace`; `cargo deny check` (advisories,
bans, licences, sources ok; a first attempt failed to load the advisory database with a
Windows paging-file error during a parallel build, and passed when rerun alone). Rule-1 scan
of the changed files done. Nothing measured.
