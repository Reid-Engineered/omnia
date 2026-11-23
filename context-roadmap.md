# Circuit context: roadmap for agents

The task list for the Circuit Context Engine. The reference for every `C`-prefixed id used here
(`CB`, `CK`, `CD`, `CINV`, `CTC`, `CQ`, `HC`) is [context-plan.md](context-plan.md). Unprefixed
ids are in the canvas files; `W`, `A`, `U` and `P` ids are in the wiring, agent, UI and parts
files.

This roadmap runs beside the others. Milestone C1 touches the new `circuit` lane and one `doc`
task; C2 adds to the agent lane after A1.8; C3 waits for the parts plan's milestone P2.

## How to work a task

The same steps as AGENTS.md: pick a `todo` task whose dependencies are all `done` (a
`needs-human` dependency is not `done`), branch from current `main` as `task/<id>-<slug>`, mark
it `doing`, read what **Read** names, do **Do** inside **Owns**, run the standard checks and
**Done when** checks, set the status, and append one log entry at the end of this file.

Statuses: `todo`, `doing`, `done`, `blocked (reason)`, `needs-human`.

## Rules

AGENTS.md, canvas roadmap rules 1 to 12, wiring roadmap rules 13 to 16 and agent roadmap rules
17 to 19 apply, with branch names `task/<id>-<slug>`, for example `task/C1.2-graph`. Three more:

20. **Derived only.** `omnia-circuit` never edits a document (CD1, CINV1). If a task seems to
    need a mutator, stop and log it.
21. **Every output is proved deterministic.** A task that adds an output adds it to the CINV2
    and CINV3 property tests in the same pull request.
22. **Keep the plans true.** A task that changes a constant, decision or signature updates
    context-plan.md in the same pull request, and any file section 15 lists for it.

## Standard checks

As in AGENTS.md: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo clippy -p omnia-bench --all-targets --features scene-counts,readback -- -D warnings`,
`cargo test --workspace`, `cargo deny check`.

## Lanes

| Lane | Owns | Needs GPUI |
| --- | --- | --- |
| `circuit` (new) | `crates/omnia-circuit/`, `context-plan.md`, `context-roadmap.md` | No |
| `doc` | `Change::net_delta` in `crates/omnia-doc/src/command.rs` and its source in `connectivity.rs` (C1.1 only) | No |
| `agent` | The `context` operation in `crates/omnia-agent/` and its MCP mapping in `crates/omnia-cli/` (C2) | No |
| `bench` | `crates/omnia-bench/benches/circuit.rs`, `docs/measurements/C*.md` | No |

## Milestones

| # | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| C0 | context-plan.md and this roadmap; owner decisions | C0.2 | HC1 |
| C1 | Net delta; `omnia-circuit`: graph, keys, names, journal, views, tier 0 findings | C1.8 | CB1 to CB6, CB9, CB10, CINV1 to CINV10, CINV12, CINV13, HC2 |
| C2 | `context` operation, edit deltas, `omnia_context` over MCP, question set | C2.5 | CB11, CB12, CINV11, HC3 |
| C3 | Electrical semantics: roles, ground, tier 1 and tier 2 findings | C3.3 | CB7, HC4 |
| C4 | Structure recognition: patterns, channels, rails | C4.4 | CB8, HC5 |
| C5 | Snapshots for jobs and app mode; connectivity refinements if measured | — | as each task says |

## Where each task fits

| Circuit task | Waits for (other roadmaps) | Can start |
| --- | --- | --- |
| C0.1, C0.2 | nothing | now |
| C1.1 | no open task owning `command.rs`, `undo.rs` or `connectivity.rs` (CQ10) | after HC1 |
| C1.2 to C1.7 | nothing beyond C1.1 for C1.3 onward | after HC1 |
| C2.1, C2.2 | A1.8 | after C1.8 and A1.8 |
| C2.5 | A1.9 (creates the agent contract) | after C2.4 and A1.9 |
| C3.1 | parts P2 (embedded definitions with pin roles in `Doc`) | after C1.8 and P2 |
| C4.1 | C3.1 (role-correct matches need pin roles) | after C3.1 |
| C4.3 | parts P2 (test-only regulator definitions are embedded through it) | after C4.2 |
| C5.1 | A3.1 or A4.1 | when either needs a snapshot |

## Dependency overview

```
C0.1 -> C0.2 [HC1]
HC1 -> C1.1 (doc), C1.2 (circuit)
C1.1 + C1.2 -> C1.3 -> C1.4 -> C1.5 -> C1.6 -> C1.7 -> C1.8 [HC2]
C1.8 + A1.8 -> C2.1 -> C2.2 -> C2.3 -> C2.4;  C2.4 + A1.9 -> C2.5 [HC3]
C1.8 + parts P2 -> C3.1 -> C3.2 -> C3.3 [HC4]
C3.1 -> C4.1 -> C4.2 -> C4.3 -> C4.4 [HC5]
A3.1 or A4.1 -> C5.1          C1.1 or C1.8 (if split tails, CQ7) -> C5.2
```

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| HC1 | C0.2 | Answers CQ1 to CQ11 or keeps their defaults; turns CB, CK and CD entries from PROPOSED to DECIDED or amends them; confirms which open tasks hold `command.rs` and `connectivity.rs` (CQ10). |
| HC2 | C1.8 | Runs the CB1 to CB6, CB9 and CB10 campaign on the reference machine (canvas plan section 2.1). |
| HC3 | C2.5 | Runs the circuit question set through an MCP client and model of their choice, with and without `omnia_context`, and records accuracy, tokens and calls per family; checks which result channel the client forwards. About an hour. |
| HC4 | C3.3 | Runs the CB1 and CB7 campaign on the reference machine with tier 1 and tier 2 findings on. |
| HC5 | C4.4 | Runs the CB1 and CB8 campaign on the reference machine with recognition on. |

---

## Milestone C0: plan and decisions

### C0.1 Circuit context plan and roadmap · done
- Lane: circuit · Depends: none · Read: AGENTS.md; mcp-plan.md sections 9, 10 and 13;
  `parts-plan.md` (PR #85) sections 6 and 10; canvas-plan.md section 10; wiring-plan.md section 10
- Owns: context-plan.md, context-roadmap.md; AGENTS.md (sources of truth); ROADMAP.md (links);
  canvas-roadmap.md (Lanes row `circuit`)
- Do: write both files from the research summary; apply the C0.1 rows of context-plan.md
  section 15; scan for names of other companies and their products (AGENTS rule 1).
- Done when: every `C`-prefixed id is defined once and every reference resolves; no statement
  contradicts a decided `D`, `WD`, `AD`, `UD` or `PD`; the rule-1 scan is logged.

### C0.2 Owner decisions · todo → HC1
- Lane: any · Depends: C0.1 · Read: context-plan.md sections 3, 4, 6 and 14
- Owns: context-plan.md sections 3, 4, 6 and 14; the C0.2 rows of section 15 in the files they
  name
- Do: set this task to `needs-human`. The owner answers CQ1 to CQ11 and reviews the PROPOSED
  entries. Record each answer with its date. Then apply the C0.2 rows of section 15 for the
  decisions kept.
- Done when: every CQ has an answer or a kept default; no entry is PROPOSED; mcp-plan.md and
  canvas-plan.md agree with the decisions kept.

## Milestone C1: the engine

### C1.1 Net delta in `Change` · todo
- Lane: doc · Depends: C0.2 and CQ10 · Read: D2, INV10, CD2, CINV12; T2.8 measurements
- Owns: `Change` and `ChangeAcc` in `crates/omnia-doc/src/command.rs`; the `Change` that
  `undo` and `redo` build in `crates/omnia-doc/src/undo.rs`; delta recording in
  `crates/omnia-doc/src/connectivity.rs`; `crates/omnia-doc/tests/net_delta.rs`
- Do: add the transaction-level `net_delta` (`retired`, `created`, `mapping`, `moved` for pins,
  wires and labels, `labels`) and `modified` (CD2). Fix the types in context-plan.md section 12
  and canvas-plan.md section 9; amend D2 there if `nets_changed` needs it.
- Done when: CINV12 holds over the T5.1 random command generator, including batches, undo and
  redo; `nets_changed == !net_delta.is_empty()` holds or D2 is amended; the command-boundary
  cases are re-run paired (delta off, delta on) on a development machine and recorded,
  labelled non-certifying, and the reference re-run is requested at HC2.

### C1.2 Crate and circuit graph · todo
- Lane: circuit · Depends: C0.2 · Read: CD1, CD7, CK1 to CK4, CINV1, CINV10; section 11.2
- Owns: `crates/omnia-circuit/` (`Cargo.toml`, `clippy.toml`, `src/lib.rs`, `src/graph.rs`,
  `src/keys.rs`, `src/intern.rs`); the crate-list line of the INV12 test in `xtask/src/main.rs`
- Do: `Circuit::build`: rows, CSR membership, interner, net keys and display names, roles from
  the symbol library (`unspecified` until C3.1).
- Done when: CINV1 and CINV10 pass; keys and names match CK1 and CK2 on CTC-key-01 and
  CTC-name-01 built from scratch; a unit test per key form.

### C1.3 Incremental update · todo
- Lane: circuit · Depends: C1.1, C1.2 · Read: CD5, CINV2, CINV5
- Owns: `crates/omnia-circuit/src/update.rs`
- Do: `Circuit::update` from `Change` and `net_delta`: recompute only touched nets, backdate
  unchanged ones, keep revision agreement.
- Done when: CINV2 holds over the T5.1 random generator with undo and redo; CINV5 has a test
  that a stale circuit refuses to answer.

### C1.4 Journal and `since` · todo
- Lane: circuit · Depends: C1.3 · Read: CD6, CK8, CINV4
- Owns: `crates/omnia-circuit/src/journal.rs`
- Do: signed facts per revision, the ring, checkpoints, consolidation, truncation; key-churn
  counter for CQ1.
- Done when: CINV4 holds under property test; CTC-key-01, CTC-key-02, CTC-since-01 to 03 pass.

### C1.5 Views and rendering · todo
- Lane: circuit · Depends: C1.4 · Read: section 9; CD8, CD17, CK7, CK9, CK10, CK13 to CK15,
  CINV3, CINV7, CINV9, CINV13
- Owns: `crates/omnia-circuit/src/view/`, `src/render.rs`
- Do: `outline`, `net`, `focus`, `since`, `path`, `check` and the summary, in `lines/1` and
  JSON, with budgets, cursors, continuation, elisions, hub capping, handle resolution and the
  outline cache.
- Done when: CINV3, CINV7, CINV9 and CINV13 hold under property tests; CTC-key-03,
  CTC-view-01 to 03, CTC-hub-01, CTC-handle-01 and CTC-inj-01 pass.

### C1.6 Tier 0 findings · todo
- Lane: circuit · Depends: C1.3, C1.5 · Read: CD11, CK5, section 9.4, CINV8
- Owns: `crates/omnia-circuit/src/findings.rs`; the `findings` view
- Do: the tier 0 rows of section 9.4, maintained in `update`.
- Done when: CINV8 holds; CTC-key-04, CTC-find-01, CTC-find-03 and CTC-gnd-01 pass; tier 0
  findings and the outline cache are in CINV2.

### C1.7 Benches and allocation proofs · todo
- Lane: bench · Depends: C1.6 · Read: CB1 to CB6, CB9, CB10, CINV6; canvas plan section 2.1
- Owns: `crates/omnia-bench/benches/circuit.rs` and its `[[bench]]` entry; allocation tests in
  `crates/omnia-circuit/tests/alloc_*.rs`
- Do: per-query timing into `hdrhistogram` with varied, seeded targets that include the
  largest net and the densest neighbourhood; whole-sheet outline; `update` timed paired with
  the command-boundary cases; resident memory by the counting allocator (CB10).
- Done when: CINV6 passes with negative controls and with warm-up as CINV6 defines it; the
  bench runs on a development machine and its numbers are recorded and labelled.

### C1.8 Gate: milestone C1 · todo → HC2
- Lane: bench · Depends: C1.7 · Read: section 3
- Owns: `docs/measurements/C1.8.md` and `docs/measurements/C1.8/`
- Do: prepare the reference campaign as one command; record development-machine numbers;
  measure split tails on the largest net (CQ7) and key churn (CQ1).
- Done when: the campaign script, order file and case list are fixed; the owner has run HC2 or
  the task is `needs-human` with the script ready.

## Milestone C2: agent surface

### C2.1 The `context` operation · todo
- Lane: agent · Depends: C1.8, A1.8 · Read: CD9, CK12, CK13, CINV5, CINV11; mcp-plan.md 9
- Owns: `crates/omnia-agent/src/context.rs`; the `context` DTOs in `src/schema/`; its schema
  file and catalog entry; `omnia-circuit` ownership in the service's document entry
- Do: decode, dispatch, render through `omnia-circuit`, enforce budgets; the host updates the
  circuit after every `apply`, `undo` and `redo`.
- Done when: AINV5 and AINV11 cover `context`; CINV11 holds; unknown handles list the closest
  names.

### C2.2 Circuit delta on `apply` and `preview` · todo
- Lane: agent · Depends: C2.1 · Read: CD14, AD3, AINV2
- Owns: the `circuit` field in `src/schema/response.rs` and its schema; the service's apply and
  preview paths
- Do: render `since` for the edit into the result; preview computes it on the fork.
- Done when: AINV2 still holds with the field; CTC-apply-01 passes.

### C2.3 `omnia_context` over MCP · todo
- Lane: agent · Depends: C2.2 · Read: CD10, CQ3; mcp-plan.md 10 and 10.5
- Owns: the `omnia_context` tool mapping and the `omnia://doc/{doc_id}/context` resource in
  `crates/omnia-cli/`; their transcript fixtures
- Do: the tool with one to five worked examples, its `outputSchema`, both result channels as
  CD10 says, in all three AK1 revisions; the resource; measure a stdio round trip on Windows,
  labelled.
- Done when: CTC-mcp-01 passes in every AK1 revision; structured results validate against the
  `outputSchema`; each channel stays within CB11; the tool list order is fixed and tested.

### C2.4 Circuit question set · todo
- Lane: circuit · Depends: C2.3 · Read: section 10; CB11, CB12, CQ6
- Owns: `crates/omnia-circuit/tests/questions/`; `docs/measurements/C2.4.md`
- Do: the question families and sheets of section 10, each with a call plan and oracle answer;
  the CI proxy; token counts for the default budget with the tokenizers available.
- Done when: every question passes the proxy within CB11 and CB12; the bytes-per-token ratio is
  recorded.

### C2.5 Gate: milestone C2 · todo → HC3
- Lane: agent · Depends: C2.4, A1.9 · Read: CD16; section 10
- Owns: `docs/measurements/C2.5.md`; `docs/agent/omnia-agent.md` (the context section)
- Do: write the HC3 run sheet; add `omnia_context` to the agent contract with evidence tiers.
- Done when: the owner has run HC3 and the results are recorded, or the task is `needs-human`
  with the run sheet ready.

## Milestone C3: electrical semantics

### C3.1 Roles, kinds and ground · todo
- Lane: circuit · Depends: C1.8, parts P2 · Read: CK3, CK4, CD13, PD5, PD6
- Owns: role and kind columns in `crates/omnia-circuit/src/graph.rs`; tier 1 in `findings.rs`
- Do: read pin names and roles from embedded definitions; set net kinds and the ground
  reference; tier 1 rows of section 9.4. A part's effective value is its override, else its
  definition's default (PD6); unmodelled legacy parts keep their free text, and only those can
  raise `value.unparsed`.
- Done when: CTC-find-02 passes; tier 1 findings are in CINV2; pin names appear in the line form.

### C3.2 Tier 2 findings · todo
- Lane: circuit · Depends: C3.1 · Read: CD11, CB7
- Owns: `crates/omnia-circuit/src/findings_lazy.rs`
- Do: the tier 2 rows of section 9.4, computed lazily and cached per revision.
- Done when: each rule has a positive and a negative fixture; CB7 recorded on a development
  machine.

### C3.3 Gate: milestone C3 · todo → HC4
- Lane: bench · Depends: C3.2 · Owns: `docs/measurements/C3.3.md` and `docs/measurements/C3.3/`
- Do: prepare the CB1 and CB7 reference campaign as one command; record development-machine
  numbers; list the new findings in the agent contract.
- Done when: the owner has run HC4, or the task is `needs-human` with the script ready.

## Milestone C4: structure recognition

### C4.1 Patterns and matcher · todo
- Lane: circuit · Depends: C3.1 · Read: CD12, CK6, CK11, CB8, CINV8
- Owns: `crates/omnia-circuit/src/recognise.rs`, `src/patterns/`
- Do: the anchored, step-capped matcher over role classes with hub exclusion and local
  rematch; patterns: divider, RC and LC filter, pull-up and pull-down, decoupling, op-amp
  inverting, non-inverting and follower.
- Done when: CTC-rec-01 passes; every pattern has positive, negative and symmetric fixtures;
  recognition output is in CINV2 and CINV3.

### C4.2 Channels · todo
- Lane: circuit · Depends: C4.1 · Read: CD12
- Owns: `crates/omnia-circuit/src/channels.rs`
- Do: the versioned hash, grouping and confirmation; `same topology` and `identical`.
- Done when: CTC-rec-02 passes; a hash-collision fixture is caught by confirmation.

### C4.3 Rails and power tree · todo
- Lane: circuit · Depends: C4.2 · Read: CD13; PD4, PD8
- Owns: `crates/omnia-circuit/src/rails.rs`; test-only regulator definitions in
  `crates/omnia-circuit/tests/fixtures/`, embedded through `PlacePart` as PD8 allows (the
  starter library promises no regulator)
- Do: rails in evidence order; regulator edges; a power graph allowing several sources.
- Done when: fixtures for a single rail, two regulators in series and two sources on one rail.

### C4.4 Gate: milestone C4 · todo → HC5
- Lane: bench · Depends: C4.3 · Owns: `docs/measurements/C4.4.md` and `docs/measurements/C4.4/`
- Do: prepare the CB1 and CB8 reference campaign as one command; record development-machine
  numbers and precision and recall on the hand-built circuits.
- Done when: the owner has run HC5, or the task is `needs-human` with the script ready.

## Milestone C5: as needed

### C5.1 Snapshots · todo
- Lane: circuit · Depends: A3.1 or A4.1 · Read: section 11.3
- Owns: `crates/omnia-circuit/src/snapshot.rs`
- Do: chunked copy-on-write publication on first read after a revision change.
- Done when: CINV6 still holds for `apply`; a reader on another thread sees one consistent
  revision.

### C5.2 Connectivity refinements · todo (optional)
- Lane: doc · Depends: C1.1 or C1.8 showing split tails (CQ7)
- Owns: `crates/omnia-doc/src/connectivity.rs` (spanning-forest bit with replacement
  promotion, batched search)
- Done when: INV10 and CINV12 hold, including the two-delete cycle case (context-plan.md
  11.1); the measured tail drops on the case that triggered it.

## Log

Append one entry per finished or blocked task. Newest last.

```
<date> <task id> <status> — what was done; numbers with hardware; deviations from the plan;
                             anything the next task needs to know.
```

2026-11-23 C0.1 done — wrote context-plan.md and context-roadmap.md from a review of
AGENTS.md, ROADMAP.md, mcp-plan.md, the parts plan in PR #85, the T2.8 measurements, and the
omnia-doc, omnia-symbols and omnia-agent sources at 3224084, plus a research summary kept
outside the repository (incremental connectivity, data layout, electrical conventions,
structure recognition, model accuracy on graph and netlist encodings, and MCP tool design).
Built on the parts plan's decided pin roles (PD5) and typed values (PD6) instead of
re-planning them; CQ2 records the roles some checks would need. Applied context-plan.md
section 15's C0.1 rows: AGENTS.md sources of truth and id prefixes, ROADMAP.md link, canvas
roadmap Lanes row `circuit`. Rule-1 scan done: no company or product named; MCP and SPICE named
as open standards; research citations given by arXiv number only. Nothing measured.

2026-11-23 C0.1 review — independent review (different model) requested changes: 5 blockers,
10 important, 1 nit. Fixed in the plan: CK1 anchor chosen by designator, then geometry, then
id, so names do not depend on insertion order, and the circuit key is a type distinct from
AD8's `NetKey`; CD2 net delta made transaction-level (retired, created, mapping, moved pins,
wires and labels, label text) plus a `modified` list, owned with `undo.rs` by C1.1; CD5
invalidation rules for renames and duplicate designators; CD6 `since` defined as a net effect
with events derived at query time, no truncated entries, defined edge responses, merge cost
O(F log k); CK14 and CK15 revision-bound cursors and continuation lines for lossless paging;
CK13 typed handles and `ambiguous_handle` (CINV13); CB1 moved out of `Doc::apply` (about
0.08 ms headroom on the worst certified case); CD10 schema and per-channel byte limits; CD14
opt-in; CK9 tagged, escaped line grammar; roadmap dependencies, ownership, HC4 and HC5.
The review also flagged the commit trailer and PR footer, which name the authoring tool, under
rule 1; the owner confirmed on 2026-11-23 that authoring-tool attribution is an exception.
