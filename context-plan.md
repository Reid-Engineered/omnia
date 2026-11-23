# Circuit context: plan (reference)

This file is the reference for the Circuit Context Engine: a derived, always-current model of
the circuit a document draws (components, pins, named nets, findings and recognised
structure), and the budgeted views through which agents, the app and the future simulator read
it. The work itself is in [context-roadmap.md](context-roadmap.md), which lists tasks that cite
the ids defined here.

It sits beside [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md), [ui-plan.md](ui-plan.md) and the component-definitions plan
`parts-plan.md` (pull request #85, open at the time of writing), and re-plans nothing they
decide. Unprefixed ids are in the canvas plan; `W`, `A` and `U` ids are in the wiring, agent and
UI plans; `P` ids are in the parts plan. Section 15 lists the changes this plan makes to other
files.

Written 2026-11-23, against `main` at `3224084`. No circuit-engine code exists yet.

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `CB` | Budget, a hard requirement | 3 |
| `CK` | Constant every circuit consumer must agree on | 4 |
| `CD` | Decision | 6 |
| `CINV` | Invariant: a rule that must hold in code, each with a test | 7 |
| `CTC` | Scenario with its expected result | 8 |
| `CQ` | Open question for the owner, each with a default | 14 |
| `HC` | Human checkpoint | [context-roadmap.md](context-roadmap.md) |
| `C0.1`, `C1.1`… | Task | [context-roadmap.md](context-roadmap.md) |

Status words are the other plans': **DECIDED**, **DEFAULT**, **HYPOTHESIS**, **GATED**, and:

- **PROPOSED**: written by the plan author. Becomes DECIDED, or is amended, at HC1. Build
  nothing from a PROPOSED entry before HC1. Every entry below is PROPOSED until HC1 is held.

The Model Context Protocol is named as an open protocol standard (AQ8), and SPICE as the open
netlist language already named in [ROADMAP.md](ROADMAP.md). No other company or product is
named (AGENTS rule 1).

### Why this exists

Speed and understanding are first-class requirements for Omnia's agents. Today an agent reads
geometry and ids: `part:3` at `[2540000, 0]`, and nets as lists of member ids. To answer "what
is R3 connected to?" it must page items, fetch nets (which are not paged and cover the whole
document) and join the two itself. Two facts shape everything below:

1. **Agent time is round trips and bytes, not engine time.** One tool call costs a model turn
   measured in seconds; the stdio transport costs well under a millisecond; the engine costs
   microseconds. The engine must therefore answer common questions in one call, within a small
   byte budget.
2. **Models read stated attributes reliably and joins unreliably.** A 2026 netlist
   question-answering benchmark (arXiv 2608.12197) found six current models reading device
   parameters and terminal lists at 99 to 100%, but answering "what attaches to this node" at
   15 to 98% and "what are this terminal's neighbours" at 2 to 97%. Graph-encoding studies
   (arXiv 2310.04560; hypergraph encodings, arXiv 2410.10083) agree: listing each node's
   connections on one line, and naming the net on each membership, beat edge lists by wide
   margins, and models almost never infer an absence that is not stated. Accuracy also falls as
   input grows, well inside advertised context windows (arXiv 2502.05167, 2510.05381).

So the engine does the joins, states the absences, caps the hubs and returns the deltas, so
that any model only ever has to read explicit attributes.

## 1. Scope

In scope:

- A new GPUI-free crate `omnia-circuit` that derives, from a `Doc` and its `Change` records:
  the circuit graph (components, pins, nets, pin to net), stable net keys and display names, a
  revision journal of circuit-level changes, findings, and recognised structure.
- Incremental maintenance inside the edit budget, with a full rebuild as the test oracle.
- Views for readers (`outline`, `net`, `focus`, `since`, `findings`, `path`, `check`), in a
  compact line form and in JSON, under byte budgets that never drop information silently.
- One composite agent operation, `context`, and its MCP tool `omnia_context`.
- A net delta in `omnia_doc::Change`, which the engine needs to update without searching.
- A circuit question set with a deterministic CI proxy and an owner-run model evaluation.

Out of scope:

- Pin roles, pin names, typed parameters and value parsing: the parts plan owns them (PD5, PD6,
  PD8). This engine reads them from the document once they land.
- The simulation netlister, analyses and engine choice (ROADMAP stage 3). The netlister will
  read its pin-to-net bindings from this engine (CQ9).
- UI panels (inspector, findings list, net highlight). The UI plan consumes the engine later.
- Hierarchical sheets, buses and multi-unit parts: the document has none yet (PQ1). The schema
  leaves room for them (CK3).
- Learned (statistical) recognition and community detection (CD12).

## 2. Repository facts this plan rests on

Checked at `3224084`.

| Fact | Where | Consequence |
| --- | --- | --- |
| `Change` carries `revision`, `dirty`, `added`, `removed` and `nets_changed: bool`. Modified items appear only through `dirty`; "an empty pair of lists does not mean nothing changed". | `crates/omnia-doc/src/command.rs` | The engine cannot tell which nets changed without searching `dirty`. Task C1.1 adds a net delta (CD2). |
| A `NetId` "is valid until the next edit": ids are reused, and nets are renumbered on merge and split. | `crates/omnia-doc/src/connectivity.rs` | Agents need keys that survive unrelated edits (CD3). |
| Connectivity is incremental: union-by-size on wire add; on removal an O(1) leaf path, otherwise a two-ended search that moves only the disconnected piece. Equal label text joins nets. `Connectivity::rebuild` is the INV10 oracle. | `connectivity.rs` | The algorithmic base is right and stays (section 11.1). |
| Every edit at `10k` was certified under 1 ms on the reference machine; a 20k-wire chain split had median run p99 0.924 ms. A full connectivity rebuild of `10k` measured about 9 ms on a Windows development machine (not certifying). | `docs/measurements/T2.8.md` | The engine's per-edit work has little headroom: CB1. |
| `omnia_symbols::Pin` is `{at, number}`; six symbols exist, one of them a ground symbol. | `crates/omnia-symbols/src/lib.rs` | Without the parts plan, the engine knows pin numbers only (role `unspecified`, CK3). |
| `Part` is `{symbol, at, orient, designator, value}`, the value being free text. The parts plan adds `def`, typed `params`, and embedded definitions with pin name and role (PD5, PD6, PD8; PR #85, open). | `crates/omnia-doc/src/doc.rs`; `parts-plan.md` | Electrical findings wait for the parts plan's milestone P2 (task C3.1). |
| Labels are one kind: global by text across the sheet. | `doc.rs`, `connectivity.rs` | Name priority is simple today (CK2) and grows with label kinds. |
| The agent's `inspect` summary is three counts; a `Net` is keyed by a member (`pin`, `wire` or `label`) and lists member ids; nets are not paged. | `crates/omnia-agent/src/schema/response.rs`, `mcp-plan.md` 9.3 | The context views replace the join the agent does today (CD8). |
| An MCP result carries `structuredContent` and a text block with the same JSON. | `mcp-plan.md` 10.1 | Possibly double the bytes per call, and clients differ on which one the model sees (CD10). |
| `Doc::fork` exists. `rustc-hash`, `smallvec`, `serde_json`, `hdrhistogram`, `criterion` and `proptest` are already dependencies; `omnia-bench/src/alloc.rs` counts allocations. | A1.1; `Cargo.lock`; canvas-plan.md 10 | No new crate is needed (section 13). |

## 3. Budgets

Measured on the reference machine under canvas plan section 2.1. All other plans' budgets still
apply; these are added. All are PROPOSED chosen figures, set or amended at HC1. Latencies are
in-process on the `10k` sheet (10,000 parts, 20,000 wires) and exclude transport. Figures on
other machines and at 100,000 parts are recorded and labelled, never gating.

| ID | Budget | Measured by |
| --- | --- | --- |
| CB1 | Engine maintenance (net keys, journal entry, tier 0 and 1 findings, dirty marks) adds ≤ 0.2 ms p99 to a single-command `apply`, and the edit plus engine stays under the T2.8 1 ms boundary | `crates/omnia-bench/benches/circuit.rs` |
| CB2 | `Circuit::build` from an opened `10k` document: < 20 ms | same bench |
| CB3 | Point query (pin to net, net by key or name, "is pin X on net Y"): p99 ≤ 50 µs | same bench |
| CB4 | One view page (an `outline` page of up to 500 lines, `focus` at depth 2, a `net` page, `since` over the whole journal, a `findings` page), rendered to bytes: p99 ≤ 0.5 ms | same bench |
| CB5 | A `context` request with up to 4 views at the default byte budget: p99 ≤ 2 ms | same bench |
| CB6 | Whole-sheet `outline` of `10k` with no byte budget: p99 ≤ 5 ms (same figure as AB1) | same bench |
| CB7 | Tier 2 findings, full recompute: ≤ 10 ms, cached until the next revision | same bench |
| CB8 | Recognition: incremental rematch after one edit p99 ≤ 1 ms; a full pass recorded | same bench |
| CB9 | Zero heap allocations once warm, in every query and render path, and in edits that introduce no new strings | allocation-counting tests (CINV6) |
| CB10 | B9 (< 300 MB with `10k` open) still holds with the engine resident; the engine's share is recorded | `memory_mb` in a B4 run with the engine on |
| CB11 | Bytes per response: 12 KB by default, never above 32 KB | `omnia-agent` tests over the CTC fixtures (CINV11) |
| CB12 | Round trips: orientation in 1 call; any question in the circuit question set's local families in at most 2 calls; checking an edit needs no call beyond the edit itself (CD14) | the question-set CI proxy (section 10) |

CB11 assumes roughly 4 bytes per token, so 12 KB is about 3,000 tokens and 32 KB stays under
the smallest client warning threshold found (10,000 tokens). The ratio is a HYPOTHESIS until
C2.4 measures it on the question set.

## 4. Constants

| ID | Constant | Value |
| --- | --- | --- |
| CK1 | Net key (stable identity) | A pure function of the net's members at the current revision. If the net has labels, `label:<text>`, where `<text>` is the lowest label text by byte order. Otherwise, if it has pins, `pin:<part id>.<pin number>` for its lowest pin by (part id, pin index). Otherwise `wire:<lowest wire id>`. Keys never contain a `NetId`. |
| CK2 | Display name | Label nets: the key's label text. Unlabelled nets: `N(<designator>.<pin number>)` from the key's anchor pin, for example `N(R3.2)`; a bare wire island `N(wire:7)`. Every other label text on the net is kept as a candidate name. When label kinds arrive, the priority is: global label, power symbol, local label, hierarchical label, sheet pin, anchor-pin name; ties by lowest text, then lowest id. |
| CK3 | Pin roles | The parts plan's seven (PD5: `passive`, `input`, `output`, `bidir`, `power`, `ground`, `no_connect`), plus `unspecified` for parts without a definition (RON v2 sheets). Fields reserved, absent until their plans add them: unit, alternate function, swap group, model-pin index, instance no-connect marker, hierarchical instance path. |
| CK4 | Net kinds | `signal`, `power`, `ground`, each with the evidence that set it (CK6). `is_ground_reference` is set only by a ground part or a `ground`-role pin, never by a name. |
| CK5 | Finding tiers | 0: per pin or per net, maintained in the edit. 1: over one net's multiset of pin roles, maintained in the edit. 2: traversals across components, computed lazily per revision. 3: heuristics that need part knowledge. Ids and default severities are in section 9.4. |
| CK6 | Evidence tiers | `declared` (drawn in the document), `exact` (a role-correct structural match), `derived` (an exact match whose values satisfy the structure's predicates), `heuristic` (name-based or approximate), `suggested` (statistical). No numeric confidence. |
| CK7 | Hub nets | A net is a hub if its kind is `power` or `ground`, or it has more than 32 pins. Views never list a hub's members inline: they show `GND (412 pins)` and a handle. Inline member lists stop at 16 pins. |
| CK8 | Journal | A ring of the last 1,024 revisions; a checkpoint every 64 revisions; at most 4,096 facts per entry, above which the entry is a bulk marker. |
| CK9 | Line form | ASCII only, one record per line, one legend line per view (section 9.2). Tokens that contain whitespace, control characters, `|`, `"` or `\` are quoted and escaped. |
| CK10 | Byte budgets | `budget` is per request, default 12,288 bytes, maximum 32,768 (CB11). A view's minimum share is its header and elision line. |
| CK11 | Recognition limits | 10,000 search steps per anchor, after which the result says `truncated`. Pattern radius at most 3 hops, hubs excluded (CD12). |
| CK12 | Schema | The `context` operation is an optional addition to `omnia.agent/1` (AK7 allows adding an operation as a minor change). The line grammar has its own version, `lines/1`, stated in each view's legend. |
| CK13 | Handles | Entity handles are names: designators (`R3`), pins (`R3.2`), net display names or keys (`VCC`, `N(R3.2)`, `label:VCC`). Cursors are opaque, keyset-based like AK4, and say in the tool description that they survive edits. An unknown handle is an error listing the closest valid names. |

## 5. Architecture

```
  omnia-doc                         Doc::apply / undo / redo -> Change { .., net_delta }   (CD2)
      |
      v
  omnia-circuit (new; no GPUI, no async runtime)
    graph.rs     dense rows, CSR net membership, interner          (11.2)
    keys.rs      net keys and display names                         (CK1, CK2)
    update.rs    incremental update, revision stamps, backdating     (CD5)
    journal.rs   signed facts per revision, since, checkpoints       (CD6)
    findings.rs  tier 0 and 1 in the edit; tier 2 lazy              (CD11)
    recognise.rs pattern library, matcher, channel hash              (CD12)
    view/        outline, net, focus, since, findings, path, check   (section 9)
    render.rs    line form and JSON into a reused buffer             (CK9)
      |                     |                          |
      v                     v                          v
  omnia-agent           omnia-app (later)          stage 3 netlister (later)
  `context` op,         inspector, findings,       reads pin to net and
  apply/preview delta   net highlight              ground reference
      |
  omnia-cli: MCP tool `omnia_context`
```

Data flows one way. A host (the agent service's document entry, later the app host) owns one
`Circuit` beside each `Doc` and calls `Circuit::update(&doc, &change)` after every `apply`,
`undo` and `redo`. A view is answered only when the circuit's revision equals the document's;
otherwise the host updates or rebuilds first (CINV5).

### Crate ownership

| Crate | Owns | Must not |
| --- | --- | --- |
| `omnia-doc` (doc lane) | `Change::net_delta` and its computation (C1.1) | Depend on `omnia-circuit`; gain any circuit concept beyond the net delta |
| `omnia-circuit` (circuit lane, new) | Everything in the diagram's middle box | Mutate a `Doc` or call anything not public in `omnia-doc`; depend on GPUI, an async runtime or `omnia-agent` |
| `omnia-agent` (agent lane) | The `context` operation's DTOs, schema, catalog entry and dispatch; the delta field on `apply` and `preview` results | Re-derive circuit facts itself |
| `omnia-cli` (agent lane) | The `omnia_context` tool mapping and its result channels (CD10) | Hold domain logic |
| `omnia-bench` | `benches/circuit.rs` | Widen visibility to measure something |

## 6. Decisions

### CD1 A separate, derived crate. PROPOSED

- `omnia-circuit` reads a `Doc` through its public API and the `Change` records `apply` returns.
  It never edits. Commands stay the only way to change a document (AD2).
- The derived model has one level of dependency (connectivity, then circuit facts, then
  findings and structure) and a fixed schema, so the incremental machinery is hand-written:
  revision stamps, early cutoff and signed deltas, borrowed as ideas from query-based
  incremental computation and dataflow systems. No incremental-computation framework is
  adopted: each brings a runtime and memo tables onto a path that must stay allocation-free.

### CD2 The document reports a net delta. PROPOSED

- `Change` gains `net_delta`: for each connectivity component the command created, destroyed or
  changed, its post-command `NetId`, the pre-command `NetId`s it came from, and the pins and
  labels that moved between components. Connectivity already knows these when it unions and
  splits; reporting them is bookkeeping, not search.
- `nets_changed` stays, for compatibility (`net_delta` non-empty implies it).
- Task C1.1 fixes the exact shape, with a property test that applying `net_delta` to the
  previous partition reproduces `Connectivity::rebuild`'s partition, and records that the T2.8
  edit boundary still holds.

### CD3 Net keys are a function of the document, not of its history. PROPOSED

- Keys follow CK1. A rule such as "the larger side keeps the key" is cheaper to maintain but
  depends on edit order, so the same final document could carry different keys. That breaks
  "same inputs, same output" unless keys are saved in the file (CQ1).
- The cost of the anchor rule: when an unlabelled net's anchor pin leaves it, its key changes.
  The journal reports that as a rename event, so a holder of the old key can follow it.
- Label keys use the lowest text, not the "best" by priority, so that adding a power label to a
  net never re-keys it unless its text sorts lower; the display name (CK2) uses priority.

### CD4 Names are shown, never silently resolved. PROPOSED

- Every label text on a net is a candidate name, kept with its kind. Two different texts on one
  net is the finding `names.multiple`; texts that differ only in case is `names.case`.
- Designator-derived names (`N(R3.2)`) are presentation. They change when parts are renumbered,
  and are never used as keys.

### CD5 Revision stamps, backdating and change tiers. PROPOSED

- Every derived record (net row, pin row, finding, recognised structure) stores `changed_at`.
  After an edit, only nets named in `net_delta`, and parts named in `added`, `removed` or
  modified within `dirty`, are recomputed. A recomputed net whose sorted members, name and
  findings equal the old ones keeps its old stamp, so a wire re-route that changes no membership
  never appears in `since`.
- Change tiers: geometry and wires (tier 0); part and pin properties (tier 1); library
  definitions (tier 2). A tier-0 edit never revisits role-only work on nets it did not touch.

### CD6 The journal stores signed facts in stable keys. PROPOSED

- Each revision appends a sorted vector of `(fact, +1 | -1)` records, where a fact is
  `PinOnNet(pin, key)`, `NetName(key, name)`, `PartValue(part, value)` or `Finding(id, handles)`.
  Internal ids never appear.
- `since {revision}` sums the entries after `revision`, sorts, consolidates and drops zeros, in
  time linear in the delta size. An edit followed by its undo consolidates to nothing. Facts
  are rendered as circuit events at query time: `R3.2 moved N(R3.2) -> GND`, `N7 merged into
  GND`, `R3 value 10k -> 4k7`.
- A revision older than the ring returns `history truncated at <revision>` and the current
  state's summary, never a partial delta. A diff of two full snapshots' sorted fact sets is both
  the fallback and the test oracle (CINV4).

### CD7 Layout for speed. PROPOSED

- Document ids map to dense `u32` rows; columns are stored separately (struct of arrays).
- Net membership is compressed sparse rows: an offset array and one flat pin array, each net's
  slice sorted by (part id, pin index). A lookup is two loads and a contiguous slice, and output
  order is deterministic by construction. Slices an edit grows move to the end behind a
  free list; compaction runs when free space passes a quarter of the array.
- Designators, net names and values are interned to `u32` with a small in-crate interner (one
  byte buffer, a `rustc-hash` map, a vector). Interner order depends on insertion order, so
  output is never sorted by interned id.
- No hash map is ever iterated into output. `omnia-circuit` carries a crate-level
  `clippy.toml` that disallows iterating `FxHashMap` and `HashMap` (CINV10).
- Rendering writes straight into one reused `Vec<u8>` per host: no `serde_json::Value`, no
  `format!`. JSON uses `serde_json::to_writer` over borrowed view structs.
- Each component's outline line is cached as a byte range in an arena and marked dirty by
  `update`; a whole-sheet outline copies cached ranges (CB6).

### CD8 Views are pre-joined, budgeted and lossless. PROPOSED

- Every view starts with counts, lists records in incidence form (each pin names its net; each
  net lists its pins), and ends with handles to fetch more. Hub nets appear as a count and a
  handle (CK7). Absences are stated as findings, because models do not infer them.
- When a view does not fit its budget, it stops at a record boundary and ends with an elision
  line: what was left out, how many, and the cursor or handle that fetches it. Following every
  elision handle reconstructs the unbudgeted view exactly (CINV7).
- Two formats with the same content: `lines` (CK9, the default, for models to read) and `json`
  (typed rows, for programs). Agents write JSON arguments and never have to produce the line
  form: studies of compact notations found accuracy falls when models must write them.

### CD9 One composite agent operation. PROPOSED

- `context {doc_id, views, budget?, format?}` takes a list of views (section 9) and answers all
  of them against one revision in one call. Its MCP tool `omnia_context` carries one to five
  worked examples in its description, because examples measurably improve argument accuracy on
  complex parameters.
- The tool set stays fixed and in a fixed order (the 2026-07-28 revision requires that tools
  not change as a side effect of other requests). No tool groups unlock during a session.
- `omnia_inspect`, `omnia_query` and `omnia_nets` stay as built in A1, and their descriptions
  point to `omnia_context` for circuit questions (CQ4).

### CD10 Both result channels carry the compact view. PROPOSED

- For `omnia_context` only, `structuredContent` is a small typed header (revision, views,
  counts, elisions) plus a `lines` array, and the text block is the same lines joined by
  newlines. With `format: "json"`, both carry the typed rows.
- Reason: clients reportedly differ on which channel reaches the model (some forward only the
  structured object, others only text), and the specification makes the JSON text copy a
  SHOULD, not a MUST. Sending the same compact content in both is correct whichever channel is
  used, and avoids doubling the bytes. This departs from mcp-plan.md 10.1 for this tool (CQ3).

### CD11 Findings run where their cost allows. PROPOSED

- Tier 0 and tier 1 (CK5) are maintained inside `update`, within CB1.
- Tier 2 runs lazily on the first request after a revision and is cached for that revision
  (CB7). Tier 3 arrives with part knowledge and is always at `heuristic` evidence or below.
- Severities are data keyed by finding id (section 9.4), so they can be tuned without code.
- Every finding names at least one handle and the rule that produced it (CINV8).

### CD12 Structure is recognised by rules and patterns, not by learning. PROPOSED

- Matching runs on the bipartite part-and-net graph. Edges carry pin role classes rather than
  pin numbers (both resistor pins are class `P`), so interchangeable pins match without trying
  every order. Symmetric patterns are de-duplicated by requiring lowest-id order among
  interchangeable parts, which is also the tie-break.
- Every search is anchored at the pattern's rarest part kind and step-capped (CK11).
  Subgraph matching is hard by structure, not size: small real instances can defeat every
  solver, so a capped search reports `truncated` rather than dropping results.
- Hub nets are labelled terminals the search never walks through. With hubs excluded, a pattern
  of radius r can only change within r hops of an edit, so rematching after an edit re-seeds
  only that neighbourhood (CB8). A change in a net's hub status rematches every pattern that
  used it.
- Repeated channels are found by a fixed, versioned Weisfeiler-Lehman hash written in the crate
  (about 100 lines), and each hash group is confirmed by the matcher before it is called
  identical: such hashes can collide. Two levels are reported: `same topology` (values ignored)
  and `identical` (values equal); a channel differing in one value is reported as such.
- Learned classifiers are excluded: in published comparisons, plain pattern matching was as
  accurate (precision about 0.96, recall about 0.99 in one study), learned output can differ
  across hardware, and only a match explains itself. Community detection is excluded from facts;
  if ever offered, it is seeded, debounced and labelled `suggested`.

### CD13 Ground and power come from declarations. PROPOSED

- The ground reference is set by a ground part or a `ground`-role pin, never by matching names
  like `GND`. A net named `AGND` with no ground part is not a reference; a name match may raise
  a `heuristic` finding.
- Rails are recognised in evidence order: declared (power symbols, `power`-role pins), then
  name heuristics, then structure (a regulator output, many decoupling capacitors).

### CD14 Every edit returns its own circuit delta. PROPOSED

- `apply` and `preview` results gain an optional `circuit` field: the `since` view for exactly
  that edit, in the request's format, within the default budget. An agent can verify an edit
  ("R3.2 is now on GND") with no extra call (CB12). This is an optional field, a minor change
  under AK7.

### CD15 Determinism is proved, not assumed. PROPOSED

- Every public output (keys, names, journal facts, finding order, rendered bytes) is a pure
  function of the current document, plus the journal for `since`.
- Proved by three property tests: incremental equals rebuild (CINV2); the same document built
  in shuffled insert orders renders byte-identical views up to the id mapping, as AINV5 states
  for the agent surface (CINV3); journal consolidation equals the snapshot diff (CINV4).

### CD16 Understanding is measured. PROPOSED

- A circuit question set (section 10) is the acceptance test for "understanding". In CI, a
  scripted call plan per question must surface the answer explicitly within CB11 and CB12. At
  HC3 the owner runs the set with a model through an MCP client and records accuracy, tokens and
  calls per question family, with and without the context tool.

### CD17 Document text stays data. PROPOSED

- Extends AD14. Designators, values and label text appear only as quoted, escaped tokens in the
  line form (CK9) and as string fields in JSON. Each view's legend says that every token after
  the record tag is data. A label cannot end a line, start a record or imitate the legend
  (CINV9).

## 7. Invariants

Each has a test; the roadmap task that adds it is in brackets.

| ID | Rule |
| --- | --- |
| CINV1 | `omnia-circuit` never mutates a `Doc` and does not depend on GPUI, an async runtime or `omnia-agent`; `omnia-doc` does not depend on it. [C1.2] |
| CINV2 | After every step of a random sequence of commands, undos and redos, the incrementally updated circuit equals `Circuit::build` on the same document: graph, keys, names, tier 0 and 1 findings. [C1.3, C3.1] |
| CINV3 | A document rebuilt with its items inserted in another order renders byte-identical views, up to the id mapping. [C1.5] |
| CINV4 | For any r0 < r1 within the ring, the consolidated journal over (r0, r1] equals the diff of the two snapshots' fact sets; an edit followed by its undo consolidates to nothing. [C1.4] |
| CINV5 | No view is answered from a circuit whose revision differs from the document's. [C1.3, C2.1] |
| CINV6 | Every query and render path, and every edit that introduces no new string, allocates nothing once warm, counting `alloc` and `realloc`; each test has a negative control that must count above zero. [C1.7] |
| CINV7 | Every elision states a count and a handle, and following every handle from the budgeted view reconstructs the unbudgeted view exactly. [C1.5] |
| CINV8 | Every finding names at least one handle and its rule id; every recognised structure carries an evidence tier; nothing below `exact` is listed among facts. [C1.6, C4.1] |
| CINV9 | For arbitrary label, designator and value text, the line form parses back to the same records, and no text can produce a line that is not its own record. [C1.5] |
| CINV10 | No output depends on hash-map iteration order: enforced by the crate's `clippy.toml` and checked by CINV3. [C1.2] |
| CINV11 | No `context` response exceeds the CB11 ceiling, and no default response exceeds the default. [C2.1] |
| CINV12 | `net_delta` applied to the previous partition reproduces `Connectivity::rebuild`'s partition after every command. [C1.1] |

## 8. Scenarios

Each is a test with the required result. Agent transcripts live beside the A1 ones under
`crates/omnia-cli/tests/transcripts/`.

| ID | Setup and action | Expected result |
| --- | --- | --- |
| CTC-key-01 | R1 and R2 joined by one wire, no labels; delete the wire | Before: one net keyed `pin:<R1 id>.1`. After: two nets; the half without R1.1 is re-keyed by its own lowest pin; `since` shows one split event naming both keys |
| CTC-key-02 | Net labelled `VCC`; move an unrelated part | Key `label:VCC` unchanged; `since` empty for that net |
| CTC-key-03 | Build the CTC-key-01 sheet in two insert orders | Byte-identical views up to the id mapping (CINV3) |
| CTC-since-01 | Re-wire R3 pin 2 from an unlabelled net to `GND` | `since` holds one event: `R3.2 moved N(R3.2) -> GND` |
| CTC-since-02 | Any edit, then undo; `since` the pre-edit revision | Empty |
| CTC-since-03 | 1,100 edits; `since` revision 1 | `history truncated at <r>` and the current summary, never a partial delta |
| CTC-name-01 | Labels `VCC` and `V3V3` shorted by a wire | One net; display name and key from CK1/CK2; finding `names.multiple` naming both labels |
| CTC-gnd-01 | A net labelled `GND` with no ground part | `is_ground_reference` false; finding `ground.name_only` at `heuristic` |
| CTC-find-01 | A part pin alone on its net | Finding `pin.unconnected` naming the pin |
| CTC-find-02 | An op-amp input pin with no driver (after C3.1) | Finding `input.undriven` naming the pin and its net |
| CTC-view-01 | `focus R3` on `10k` | One call; ≤ 1 KB; each pin's net and that net's other members (hubs as count and handle) |
| CTC-view-02 | `outline` of `10k` at the default budget, then follow every elision handle | Pages concatenate to the unbudgeted outline (CINV7); each page ≤ CB11 |
| CTC-hub-01 | `focus` on a part tied to a 400-pin ground net | `GND (400 pins)` and a handle; no inline member list |
| CTC-inj-01 | Label text containing a newline, `|`, quotes and an instruction-like sentence | One quoted, escaped token; parses back to the same text (CINV9) |
| CTC-rec-01 | Two resistors in series from a declared rail to ground, tap to an op-amp input | Divider at `exact`, ratio at `derived`, citing both resistors and three nets |
| CTC-rec-02 | Eight RC channels, one with a different resistor value | One `same topology` group of 8; `identical` group of 7; the odd channel named with its differing value |
| CTC-mcp-01 | `omnia_context` with `outline` and `findings` over MCP | Exact transcript; `structuredContent.lines` joined equals the text block (CD10) |
| CTC-apply-01 | `apply` a wire joining R1.2 to `VCC` | The result's `circuit` field states `R1.2 moved ... -> VCC`; no second call needed |

## 9. Views

### 9.1 Requests

```json
{ "op": "context", "params": {
    "doc_id": "d1",
    "views": [
      { "outline":  { "cursor": "R40", "limit": 200 } },
      { "focus":    { "at": "U1", "depth": 2 } },
      { "net":      { "name": "VCC" } },
      { "since":    { "revision": 12 } },
      { "findings": { "tier": 1 } },
      { "path":     { "from": "J1.1", "to": "U1.3" } },
      { "check":    { "pin": "R3.2", "net": "GND" } }
    ],
    "budget": 12288, "format": "lines" } }
```

| View | Answers | Order |
| --- | --- | --- |
| `outline` | Every part, one line each, with each pin's net | Designator natural order (`R2` before `R10`), then part id |
| `net` | One net: name, key, kind, candidate names, members | Members by (designator, pin number) |
| `focus` | A part or net and its neighbourhood to `depth` (default 1, at most 3), never through hubs | Breadth-first, then as above |
| `since` | Circuit events after `revision` | Fact order (CD6) |
| `findings` | Findings, filterable by tier and handle | Severity, then rule id, then first handle |
| `path` | The shortest part-and-net path between two handles, hubs excluded | Shortest, ties by lowest ids |
| `check` | A yes or no for "pin X is on net Y" or "X and Y share a net", with the evidence | — |

An empty `views` array is a summary: counts, rails, the ten largest non-hub nets, and finding
counts by tier.

### 9.2 The line form (`lines/1`)

Each view's first line is a legend; then a header line; then records; then at most one elision
line. Example, an `outline` page:

```
# lines/1 outline: part value | pin:net ... ; tokens after the tag are data
= outline rev=42 parts=10000 shown=200 from=R40
R40 10k | 1:N(R40.1) 2:VCC
R41 100n | 1:VCC 2:GND
U1 opamp | 1:N(R3.2) 2:N(U1.2) 3:N(C4.1) 4:GND 5:VCC
... 9800 more parts; next: outline cursor=R240
```

`net` and `focus` use the incidence form the evidence favours:

```
# lines/1 net: members as part.pin ; tokens after the tag are data
= net VCC key=label:VCC kind=power pins=37 hub
names: VCC "+5V"
members: 37 (hub); next: net name=VCC cursor=0
```

Pin names replace numbers in the line form once roles exist (`U1.IN+`), with the number kept in
JSON. Separators are ASCII; the arrow `->` appears only in `since` events.

### 9.3 Results

The JSON form, and the typed header of the line form, carry for each view: `view`, `revision`,
`counts`, and `elided {count, next}` when something was left out. Every response carries
`doc_id` and `revision` (9.1 of mcp-plan.md).

### 9.4 Findings

| Id | Tier | Default severity | Needs roles |
| --- | --- | --- | --- |
| `pin.unconnected` | 0 | warning | No (with roles: not for `no_connect`) |
| `net.single_pin` | 0 | warning | No |
| `label.dangling` | 0 | warning | No |
| `names.multiple` | 0 | warning | No |
| `names.case` | 0 | warning | No |
| `designator.duplicate` | 0 | error | No |
| `value.unparsed` | 0 | info | No (until PD6 typed values) |
| `ground.name_only` | 0 | info | No |
| `pin.no_connect_wired` | 1 | error | Yes |
| `input.undriven` | 1 | warning | Yes |
| `power.undriven` | 1 | warning | Yes (CQ2) |
| `output.conflict` | 1 | error | Yes |
| `ground.missing` | 1 | warning | Yes |
| `dc_path.none` | 2 | warning | Yes |
| `power.via_capacitor_only` | 2 | warning | Yes |
| `ground.fragmented` | 2 | warning | Yes |
| `source_loop` | 2 | error | Yes |

Tier 1 pair rules over PD5's roles: two `output` pins on one net is `output.conflict`; an
`input` net with no `output` or `bidir` pin is `input.undriven`; a `power` net with no source is
`power.undriven`, which needs a source role PD5 lacks (CQ2) and is off until answered.

## 10. Measuring understanding

The circuit question set lives in `crates/omnia-circuit/tests/questions/`. Each question has a
sheet, a scripted call plan and the answer the engine's oracle computes. Families, chosen where
models were weakest in published work:

| Family | Example |
| --- | --- |
| Orientation | What rails exist, and what feeds them? |
| Attribute | What is R3's value? |
| Node incidence | What else is on R3 pin 2's net? |
| Terminal neighbours | Which parts share a net with U1 pin 3? |
| Equivalence | Are J1.1 and U1.3 on the same net? |
| Absence | Which pins are unconnected? |
| Edit verification | After this edit, what changed in the circuit? |

Sheets: the generated `100`, `1k` and `10k` sheets, plus about 20 hand-built circuits (dividers,
filters, op-amp stages, channel arrays) whose answers a person has checked.

- **CI proxy (C2.4).** Each question's call plan runs against the engine and must return every
  answer token explicitly, within CB11 bytes and CB12 calls. This is deterministic and needs no
  model. It proves the answer is there to read; it cannot prove a model reads it.
- **Model run (HC3).** The owner runs the set through an MCP client and model of their choice,
  once with only the A1 read tools and once with `omnia_context`, and records accuracy, tokens
  and calls per family in `docs/measurements/C2.5.md`. The targets for later runs are set at
  HC3 from this baseline.

## 11. Engine internals

### 11.1 Connectivity stays as it is

A 2025 experimental study of published fully dynamic connectivity structures (arXiv
2501.02278) found large constants, high memory and pathological cases, and judged them not ready
to deploy as is. Omnia's union-by-size plus two-ended search, where the smaller side moves, is
the classic bounded-relabelling approach and stays. Two cheap refinements are planned only if
measurement asks for them (C5.2):

- A spanning-forest bit per wire: set when an insert merges two components, clear when it
  closes a cycle. Deleting a wire with the bit clear can never split a net and needs no search.
- Multi-wire commands (paste, delete selection) run all unions first, then one multi-source
  search per affected component.

### 11.2 Rows, CSR and caches

| Structure | Form |
| --- | --- |
| Parts | Columns: designator (interned), value (interned; typed once PD6 lands), symbol, pin range |
| Pins | Columns: part row, pin number, role (CK3), net row |
| Nets | Columns: key, display name, kind, flags, `changed_at`; CSR slice of member pins |
| Names | Interner (CD7); reverse map name to net row for `net {name}` |
| Outline cache | Per part: byte range in an arena, dirty bit |
| Journal | Ring of sorted fact vectors (CK8) plus checkpoints |

### 11.3 Snapshots

The headless agent service is single-threaded, so it needs no snapshot. Background jobs (A3)
and app mode (A4) read a snapshot: the circuit's columns are held in chunks of 4,096 rows behind
`Arc`, and a snapshot is published lazily, on the first reader request after the revision
moves, by replacing only dirty chunks. Publishing never happens inside `apply`, so the edit
path keeps its allocation proof. Every response echoes the snapshot's revision (C5.1).

### 11.4 Transport

One locked stdout, one `write_all` per newline-terminated message, one flush, logs on stderr
only. A pipe round trip is microseconds on Linux; Windows is measured, labelled and not
budgeted (C2.3).

## 12. Crates and APIs

```rust
// ---- omnia-circuit ----
pub struct Circuit { /* rows, CSR, interner, journal, caches */ }

impl Circuit {
    pub fn build(doc: &Doc, symbols: &SymbolLibrary) -> Circuit;                 // CB2
    pub fn update(&mut self, doc: &Doc, symbols: &SymbolLibrary, change: &Change); // CB1
    pub fn revision(&self) -> u64;
    pub fn net_of_pin(&self, part: PartId, pin: u32) -> NetRef;                   // CB3
    pub fn net_by_name(&self, name: &str) -> Option<NetRef>;                      // CB3
    pub fn render(&self, req: &ViewRequest, budget: usize, format: Format,
                  out: &mut Vec<u8>) -> Rendered;                                 // CB4, CB5
    pub fn since(&self, revision: u64) -> Since<'_>;                              // CD6
    pub fn findings(&mut self, tier: Tier) -> Findings<'_>;                       // CD11
}

pub struct NetRef { /* row + revision; resolves key, name, kind, members */ }
pub enum Format { Lines, Json }
pub struct Rendered { pub views: u8, pub elided: bool, pub bytes: usize }

// ---- omnia-doc (C1.1) ----
pub struct Change { /* existing fields */ pub net_delta: NetDelta }
pub struct NetDelta { /* shape fixed by C1.1 (CD2) */ }
```

## 13. Dependencies and risks

No new dependency. `rustc-hash`, `smallvec`, `serde_json`, `hdrhistogram`, `criterion` and
`proptest` are already in `Cargo.lock` and canvas-plan.md section 10; C0.2 extends their use
column to `omnia-circuit`. A crate not listed there needs its licence logged first.

| Risk | Mitigation | Early test |
| --- | --- | --- |
| C1.1 changes `command.rs` and `connectivity.rs`, which wiring tasks own in turn | C1.1 waits until no open task owns them (CQ10) | C1.1 |
| The parts plan is not merged, so pins have no roles | Milestones C1 and C2 need no roles; tier 1 waits for C3.1 | C1.6 |
| A giant ground net makes recompute or splits slow | Hubs are never listed or walked; split tails measured at C1.8; C5.2 if needed | C1.8 |
| The anchor rule re-keys unlabelled nets often | Rename events in `since`; churn measured on recorded sessions (CQ1) | C1.4 |
| Clients pass a different channel to the model | Same compact content in both channels (CD10); checked per client at HC3 | C2.3 |
| 4 bytes per token is wrong for the model in use | Measured on the question set before CB11 is certified | C2.4 |
| Recognition gives false positives | Evidence tiers; `derived` needs value predicates; hand-checked circuits | C4.1 |
| Document text steers the model | CD17, CINV9 | C1.5 |

## 14. Open questions

Each has a default. Build to the default; the owner's dated answer replaces it here.

| ID | Question | Default |
| --- | --- | --- |
| CQ1 | Persist net keys in the document, so unlabelled nets keep their key when their anchor pin leaves? | No: anchor rule (CK1), derived. Revisit with churn numbers from C1.4. |
| CQ2 | Ask the parts plan to extend PD5's roles (separate power input and power output, open collector and emitter, tri-state, free, unspecified)? Without a power-source role, `power.undriven` cannot be checked. | Build to PD5's seven roles plus engine-only `unspecified`; `power.undriven` stays off; the owner raises it against the parts plan. |
| CQ3 | For `omnia_context`, put the compact lines in both result channels instead of a JSON text copy (CD10)? | Yes, for this tool only. |
| CQ4 | Retire or alias `omnia_inspect`, `omnia_query` and `omnia_nets` once `omnia_context` exists? | Keep them as built; decide after the HC3 run. |
| CQ5 | Line syntax: `1:VCC` or `1->VCC`; designators or ids as handles? | ASCII `:`; designators and net names as handles, ids accepted in arguments. |
| CQ6 | Byte budgets 12 KB default, 32 KB ceiling? | Yes; re-set from C2.4's token measurement. |
| CQ7 | Add the spanning-forest bit and batched search to connectivity (11.1)? | Only if C1.8 shows split tails on hub nets. |
| CQ8 | Hub threshold: more than 32 pins, or kind `power` or `ground`? | Yes. |
| CQ9 | Does the stage 3 netlister take pin-to-net bindings and the ground reference from this engine, rather than a second derivation in the parts plan's netlist API? | Yes; the parts plan's netlist API reads them from `omnia-circuit`. |
| CQ10 | May C1.1 change `Change` and connectivity while W2.2 is `needs-human` (its measurement only) and W3.4 to W3.6 are open? | C1.1 starts only when no open task owns `command.rs` or `connectivity.rs`; the owner confirms at HC1 which open tasks hold them. |
| CQ11 | Use a graph library for the matcher? | No: a hand-written matcher in the crate, so determinism and step caps stay under our control. |

## 15. Changes to other files

Applied by C0.1 (registration only):

| Where | Change |
| --- | --- |
| AGENTS.md, sources of truth | `context-plan.md` beside the plans, `context-roadmap.md` beside the roadmaps; `C`-prefixed ids live in the circuit-context files |
| ROADMAP.md | Links the circuit-context plan and roadmap |
| canvas-roadmap.md, Lanes | Row `circuit` |

Applied by C0.2 after HC1, for the decisions HC1 keeps:

| Where | Change |
| --- | --- |
| mcp-plan.md 9.2, 9.3 | The `context` operation; the optional `circuit` field on `apply` and `preview` (CD14) |
| mcp-plan.md 10.1, 10.2 | `omnia_context` and its result channels (CD10); `omnia://doc/{doc_id}/context` resource for the summary view |
| mcp-plan.md 13 | The safe loop orients with `omnia_context` and verifies with the `circuit` delta |
| canvas-plan.md section 10 | Use column: `rustc-hash`, `smallvec`, `serde_json`, `hdrhistogram` in `omnia-circuit` |
| canvas-plan.md section 9 (`Change`) | `net_delta` (CD2), once C1.1 fixes its shape |
| parts-plan.md (once merged) | Section 1: the netlister reads pin-to-net bindings from `omnia-circuit` (CQ9); section 13: CQ2 raised as a parts question if the owner wants it |
