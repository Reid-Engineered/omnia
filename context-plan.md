# Circuit context: plan (reference)

This file is the reference for the Circuit Context Engine: a derived, always-current model of
the circuit a document draws (components, pins, named nets, findings and recognised
structure), and the budgeted views through which agents and the app read it, plus the
revision-matched pin-to-net bindings and ground evidence that simulation lowering reads. The work itself is in [context-roadmap.md](context-roadmap.md), which lists tasks that cite
the ids defined here.

It sits beside [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md), [ui-plan.md](ui-plan.md), [sim-plan.md](sim-plan.md) and the
component-definitions plan `parts-plan.md` (pull request #85, open at the time of writing), and
re-plans nothing they decide except where the cross-plan decisions XD1–XD9 (section 6.1) say
so. Unprefixed ids are in the canvas plan; `W`, `A`, `U` and `S` ids are in the wiring, agent,
UI and simulation plans; `P` ids are in the parts plan. Section 15 lists the changes this plan
makes to other files.

Written 2026-11-23, against `main` at `3224084`. Updated 2026-11-23 against `main` at
`9d4b844` for the cross-plan decisions XD1–XD9. No circuit-engine code exists yet.

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
  nothing from a PROPOSED entry before HC1. Every entry below is PROPOSED until HC1 is held,
  except the cross-plan decisions in section 6.1 and the parts of entries they decide, which
  are DECIDED (coordinator, owner-delegated, 2026-11-23).

The Model Context Protocol is named as an open protocol standard (AQ8), and SPICE as the open
netlist language already named in [ROADMAP.md](ROADMAP.md). Operating systems appear only as
AGENTS.md names them for machines. No other company is named in the plan text; section 16
lists the sources behind the research claims, with links, as citations only. Citations are
written `[Ref1]`; the `Ref` prefix keeps them apart from canvas spikes `S1`–`S8` and simulation
ids (XD9).

### Why this exists

Speed and understanding are first-class requirements for Omnia's agents. Today an agent reads
geometry and ids: `part:3` at `[2540000, 0]`, and nets as lists of member ids. To answer "what
is R3 connected to?" it must page items, fetch nets (which are not paged and cover the whole
document) and join the two itself. Two facts shape everything below:

1. **Agent time is round trips and bytes, not engine time.** One tool call costs a model turn
   measured in seconds; the stdio transport costs well under a millisecond (an estimate, to be
   measured in C2.3); the engine costs microseconds. The engine must therefore answer common questions in one call, within a small
   byte budget.
2. **Models read stated attributes reliably and joins unreliably.** A 2026 netlist
   question-answering benchmark [Ref1] found six current models reading device parameters and
   terminal lists at 99 to 100%, but answering "what attaches to this node" at 15 to 98% and
   "what are this terminal's neighbours" at 2 to 97%. Graph-encoding studies [Ref2, Ref3] agree:
   listing each node's connections on one line, and naming the net on each membership, beat
   edge lists by wide margins, and models almost never infer an absence that is not stated
   [Ref2]. Accuracy also falls as input grows, well inside advertised context windows [Ref4, Ref5],
   and facts in the middle of a long input are used worst [Ref6].

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
- A small read-only interface, `Circuit::bindings` (task C1.9), that gives simulation lowering
  revision-matched pin-to-net bindings and declared ground evidence (XD4).
- A circuit question set with a deterministic CI proxy and an owner-run model evaluation.

Out of scope:

- Pin roles, pin names, typed parameters and value parsing: the parts plan owns them (PD5, PD6,
  PD8). This engine reads them from the document once they land.
- Simulation lowering, analyses and engine choice (ROADMAP stage 3). `omnia-sim` owns lowering:
  `CircuitIr`, node ordering, device expansion, source maps, diagnostics mapping and SPICE text
  (sim SD7). It reads pin-to-net bindings and ground evidence from this engine (C1.9, CQ9,
  XD4). Run preflight (`sim.erc`, ERC001–ERC008) is the simulation plan's; this plan shares its
  predicates (section 9.4, XD6).
- UI panels (inspector, findings list, net highlight). The UI plan consumes the engine later.
- Hierarchical sheets, buses and multi-unit parts: the document has none yet (PQ1). The schema
  leaves room for them (CK3).
- Learned (statistical) recognition and community detection (CD12).

## 2. Repository facts this plan rests on

Checked at `3224084`; the parts and simulation rows rechecked at `9d4b844` (parts plan at PR
#85 head `4bee8d5`).

| Fact | Where | Consequence |
| --- | --- | --- |
| `Change` carries `revision`, `dirty`, `added`, `removed` and `nets_changed: bool`. Modified items appear only through `dirty`; "an empty pair of lists does not mean nothing changed". `apply` builds it in `command.rs`; `undo` and `redo` build it in `undo.rs`. | `crates/omnia-doc/src/command.rs`, `undo.rs` | The engine cannot tell which nets or parts changed without searching `dirty`. Parts task P2.2 adds `modified: Vec<PartId>` for parts whose definition or parameters changed (PD8, XD7); C1.1 adds the net delta and widens the modified lists (CD2). |
| A `NetId` "is valid until the next edit": ids are reused, and nets are renumbered on merge and split. | `crates/omnia-doc/src/connectivity.rs` | Agents need keys that survive unrelated edits (CD3). |
| The agent read layer keys a net by its lowest member by id (AD8, `NetKey`). That key is canonical for one document state but changes with insertion order and edit history. | `mcp-plan.md` AD8; `crates/omnia-agent/src/schema/response.rs` | The circuit key (CK1) is a separate type with a different rule; AD8's `NetKey` is unchanged. |
| Connectivity is incremental: union-by-size on wire add; on removal an O(1) leaf path, otherwise a two-ended search that moves only the disconnected piece. Equal label text joins nets. `Connectivity::rebuild_with_parts` rebuilds the pin-aware state on load; an independent randomized-test oracle checks INV10. | `connectivity.rs` | The algorithmic base is right and stays (section 11.1). |
| Every case in the certified command-boundary campaign passed the < 1 ms gate statistic (median of ten run p99s). The worst, a 20k-wire chain split, had median 0.924 ms with run p99s from 0.872 to 1.051 ms. A full connectivity rebuild of `10k` measured about 9 ms on a Windows development machine (not certifying). | `docs/measurements/command-boundary/reference-2026-11-17/README.md`, `T2.8.md` [Ref20] | `Doc::apply` has about 0.08 ms of headroom on its worst case. The engine therefore updates after `apply` returns, under its own budget (CB1), and the net delta recording in `apply` is measured against the existing boundary (C1.1). |
| `omnia_symbols::Pin` is `{at, number}`; six symbols exist, one of them a ground symbol. Sim task S1.1 adds a per-symbol class (`Component`, `Ground`, `PowerRail`, `Instrument`, `Probe`). | `crates/omnia-symbols/src/lib.rs`; `sim-plan.md` section 13 | Without the parts plan, the engine knows pin numbers only (role `unspecified`, CK3). Symbol class and pin role are separate types (XD5). |
| `Part` is `{symbol, at, orient, designator, value}`, the value being free text. The parts plan adds `def`, typed `params`, `SetParam`, `UpdatePartDef` and embedded definitions with pin name, role and SPICE terminal order (PD5, PD6, PD8; PR #85, open); sim SD9's `props` is withdrawn (XD1). | `crates/omnia-doc/src/doc.rs`; `parts-plan.md` | Electrical findings wait for the parts plan's milestone P2 (task C3.1). |
| `sim-plan.md` plans `omnia-sim`: lowering to `CircuitIr` with a `NodeId` ↔ `NetId` source map (SD7), ERC001–ERC008 as Run preflight (SD8), and node `0` from a `Ground` symbol or a net labelled `0` (SK10, SQ3). | `sim-plan.md` | Lowering is sim's and reads this engine's bindings (C1.9, XD4); findings share predicates with ERC (section 9.4, XD6). |
| Labels are one kind: global by text across the sheet. | `doc.rs`, `connectivity.rs` | Name priority is simple today (CK2) and grows with label kinds. |
| The agent's `inspect` summary is three counts; a `Net` is keyed by a member (`pin`, `wire` or `label`) and lists member ids; nets are not paged. | `crates/omnia-agent/src/schema/response.rs`, `mcp-plan.md` 9.3 | The context views replace the join the agent does today (CD8). |
| mcp-plan.md 10.1 specifies that an MCP result will carry `structuredContent` and a text block with the same JSON. The MCP adapter (A1.8) is `todo`. | `mcp-plan.md` 10.1; `mcp-roadmap.md` | Possibly double the bytes per call, and clients differ on which one the model sees (CD10). |
| `Doc::fork` exists. `rustc-hash`, `smallvec`, `serde_json`, `hdrhistogram`, `criterion` and `proptest` are already dependencies; `omnia-bench/src/alloc.rs` counts allocations. | A1.1; `Cargo.lock`; canvas-plan.md 10 | No new crate is needed (section 13). |

## 3. Budgets

Measured on the reference machine under canvas plan section 2.1. All other plans' budgets still
apply; these are added. All are PROPOSED chosen figures, set or amended at HC1. Latencies are
in-process on the `10k` sheet (10,000 parts, 20,000 wires) and exclude transport. Figures on
other machines and at 100,000 parts are recorded and labelled, never gating.

| ID | Budget | Measured by |
| --- | --- | --- |
| CB1 | `Circuit::update` for one command's `Change` (net keys, journal entry, tier 0 and 1 findings, dirty marks): p99 ≤ 0.2 ms, measured paired with the same fixed command cases as the command-boundary campaign. The engine never runs inside `Doc::apply`; `apply` with net delta recording still meets the existing < 1 ms boundary (C1.1). | `crates/omnia-bench/benches/circuit.rs` |
| CB2 | `Circuit::build` from an opened `10k` document: < 20 ms | same bench |
| CB3 | Point query (pin to net, net by key or name, "is pin X on net Y"): p99 ≤ 50 µs | same bench |
| CB4 | One view page (an `outline` page of up to 500 lines, `focus` at depth 2, a `net` page, `since` over the whole journal, a `findings` page), rendered to bytes: p99 ≤ 0.5 ms | same bench |
| CB5 | A `context` request with up to 4 views at the default byte budget: p99 ≤ 2 ms | same bench |
| CB6 | Whole-sheet `outline` of `10k` with no byte budget: p99 ≤ 5 ms (same figure as AB1) | same bench |
| CB7 | Tier 2 findings, full recompute: ≤ 10 ms, cached until the next revision | same bench |
| CB8 | Recognition: incremental rematch after one edit p99 ≤ 1 ms; a full pass recorded | same bench |
| CB9 | Zero heap allocations once warm (CINV6 defines warm and the measured boundary) | allocation-counting tests (CINV6) |
| CB10 | The engine's resident memory for `10k` ≤ 40 MB, measured headless by the counting allocator. B9 is re-checked when the app hosts the engine (a UI-plan task) | same bench |
| CB11 | Bytes per channel per response: the text block and the serialized `structuredContent` each at most 12 KB by default and never above 32 KB (CD10) | `omnia-agent` tests over the CTC fixtures (CINV11) |
| CB12 | Round trips: orientation in 1 call; any question in the circuit question set's local families in at most 2 calls; checking an edit needs no call beyond the edit itself (CD14) | the question-set CI proxy (section 10) |

CB11 assumes roughly 4 bytes per token, so 12 KB is about 3,000 tokens and 32 KB stays under
a reported tool-output warning threshold (10,000 tokens) [Ref31]. The ratio is a HYPOTHESIS until
C2.4 measures it on the question set.

## 4. Constants

| ID | Constant | Value |
| --- | --- | --- |
| CK1 | Circuit key (stable identity; a type distinct from AD8's `NetKey` and from sim's solver `NodeId`, XD4) | Derived from the net at the current revision and stable across unrelated edits. Rebuilding in a different insertion order yields equivalent keys under the id remapping in CINV3. If the net has labels: `net:"<text>"`, the lowest label text by byte order. Otherwise, if it has pins: the **anchor pin** is the member pin lowest by (designator in natural order, pin number in natural order), then by pin world position (x, then y), then by (part id, pin index); the key is `net:pin:<part id>.<pin index>`. Otherwise (a bare wire island): the anchor is the wire lowest by (lower endpoint x, then y), then by id; the key is `net:wire:<wire id>`. Keys never contain a `NetId`. No conversion between a circuit key, a `NetKey` and a `NodeId` exists in any crate; C1.9's bindings carry the circuit key and the current `NetId` side by side, and lowering assigns `NodeId`s itself (sim SD7). |
| CK2 | Display name | Label nets: the key's label text. Unlabelled nets: `N(<designator>.<pin number>)` from the anchor pin, for example `N(R3.2)`; a bare wire island `N(wire:7)`. Generated names are never valid document text in the line form, because document text with parentheses is always quoted (CK9), so a label `N(R3.2)` cannot be mistaken for one. Every other label text on the net is kept as a candidate name. When label kinds arrive, the priority is: global label, power symbol, local label, hierarchical label, sheet pin, anchor-pin name; ties by lowest text, then lowest id. This order is the established convention [Ref25]. |
| CK3 | Pin roles | The parts plan's seven (PD5: `passive`, `input`, `output`, `bidir`, `power`, `ground`, `no_connect`), plus `unspecified` for parts whose definition has no pin table (unmodelled definitions, including every part migrated from RON v2). A placed part's embedded definition is authoritative for its pin roles and SPICE terminal order (XD5). Pin role is a per-pin type, separate from sim S1.1's per-symbol class (`Component`, `Ground`, `PowerRail`, `Instrument`, `Probe`), which the engine keeps as a part column; neither is derived from the other. Fields reserved, absent until their plans add them: unit, alternate function, swap group, instance no-connect marker, hierarchical instance path. |
| CK4 | Net kinds and ground evidence | `signal`, `power`, `ground`, each with the evidence that set it (CK6). **Declared ground evidence** is exactly sim SQ3's default (XD4): a part whose symbol class is `Ground`, or a label whose text is exactly `0`. `is_ground_reference` is true exactly when a net carries declared ground evidence, and C1.9 exposes the evidence items. A `ground`-role pin sets kind `ground` (declared) but not the reference; a name such as `GND` or `AGND` never sets the reference and may set kind `ground` only at `heuristic` evidence, with the finding `ground.name_only` as the hint (CD13). Until S1.1 lands the symbol class, the one existing ground symbol (`Ground-COM-General`) stands for class `Ground`. |
| CK5 | Finding tiers | 0: per pin or per net, maintained in the edit. 1: over one net's multiset of pin roles, maintained in the edit. 2: traversals across components, computed lazily per revision. 3: heuristics that need part knowledge. Ids and default severities are in section 9.4. |
| CK6 | Evidence tiers | `declared` (drawn in the document), `exact` (a role-correct structural match), `derived` (an exact match whose values satisfy the structure's predicates), `heuristic` (name-based or approximate), `suggested` (statistical). No numeric confidence. |
| CK7 | Hub nets | A net is a hub if its kind is `power` or `ground`, or it has more than 32 pins. Views never list a hub's members inline: they show `GND (412 pins)` and a handle. Inline member lists stop at 16 pins. |
| CK8 | Journal | Entries for at most the last 1,024 revisions and at most 262,144 facts in total, preallocated; the oldest entries are evicted first, and no entry is ever truncated. The journal keeps a **base revision**: the revision at open or rebuild, advanced to the revision just before the oldest retained entry on each eviction. `since r` is answerable exactly when base ≤ r ≤ now. An entry larger than the whole capacity is stored alone, evicting every older entry (the separately tested exceptional path in CINV6), so the base becomes the revision just before it and its own delta stays answerable and pageable until the next edit evicts it. |
| CK9 | Line form | ASCII only; one record per line; every line starts with a fixed tag (section 9.2). A document string (designator, value, label text, pin name or number) is written bare only if it matches `[A-Za-z0-9_+-]{1,64}` and is not a tag; otherwise it is quoted with `"`. Inside quotes, `\` and `"` are escaped as `\\` and `\"`, and every character outside printable ASCII (controls, newlines, non-ASCII) is written as `\u{hex}`; every other character, separators included, is literal. Outside quotes the characters `.` `:` `=` `|` `(` `)` `,` and space are structural only. Compound fields: a pin reference is `<designator>.<pin>`, each part in this string form, so `"A.B".C` and `A."B.C"` differ; a pin-to-net pair is `<pin>:<net name>`; a generated name is `N(<pin reference>)` or `N(wire:<id>)`, always bare, and no document string can produce it bare because `(` forces quoting. Section 9.2 gives the grammar per record. |
| CK10 | Byte budgets | `budget` is per request, default 12,288 bytes, maximum 32,768 (CB11). A request holds at most 8 views. After reserving the fully encoded envelope and mandatory metadata for each channel, each view gets an equal share of the remaining budget, and a share a view does not use passes to the next view in order. A view whose share is below CK15's minimum is not run: it returns only an elision line whose cursor runs it alone. |
| CK11 | Recognition limits | 10,000 search steps per anchor, after which the result says `truncated`. Pattern radius at most 3 hops, hubs excluded (CD12). |
| CK12 | Schema | The `context` operation is an optional addition to `omnia.agent/1` (AK7 allows adding an operation as a minor change). The line grammar has its own version, `lines/1`, stated in each view's legend. The operation adds three error codes to 9.4 of mcp-plan.md: `stale_cursor`, `unknown_revision` and `ambiguous_handle {candidate_count, candidates}`. |
| CK13 | Handles | Typed handles are unambiguous: item ids (`part:3`, `wire:7`, `label:2`, AQ5), pins `part:3#1` (pin index), and circuit keys (`net:"VCC"`, `net:pin:3.1`). Names are aliases. Resolution collects **every** entity of the types the argument accepts whose alias matches (designators `R3`, pin references `R3.2`, net display names and candidate names `VCC`, `N(R3.2)`); exactly one match binds, more than one is refused with the total candidate count and the longest sorted prefix of complete typed handles that fits the fully encoded JSON error in both channels (CK10, CD10). The prefix may be empty. Candidate details have no continuation and the count exposes any omission. The error is independent of index lookup order. Unknown-handle suggestions use the same bounded-prefix rule. The name index maps each alias to a sorted list of entities. Unknown-handle suggestions are ordered by closeness, then handle text. A pin whose part has no library metadata for it (a legacy sheet whose stored pins differ from the symbol) is shown by index as `R3#1`. |
| CK14 | Cursors | Opaque. A cursor binds the view, its arguments and the revision, and is passed back as `{"cursor": "<c>"}` in place of a view. Used at another revision it is the tool error `stale_cursor` with a hint to re-run the view; a document's pages are lossless within one revision. These ordinary cursors require the same open document. Edit-delta cursors and their text handles follow the session-scoped lifetime in CD14. Moving to a different view uses a typed handle (CK13), not a cursor: `focus` gives `net:"GND"`, and `{"net": {"name": "net:\"GND\""}}` starts the member pages. |
| CK15 | Page minimum and long records | A view's share below 2,048 bytes is not run (CK10); a request budget below 4,096 bytes is raised to 4,096. In both formats a record is split, never dropped: in `lines` a long record continues on lines tagged `+`; in `json` a long row is split into fragments with the same `key` and `part: n of m`, whose list fields (pins, members) concatenate in order. A document string longer than 1,024 bytes is shown as its first 256 bytes plus `+<n> bytes` and a `text` handle, and `{"text": {"of": "<handle>", "cursor"?}}` pages the whole string, with a UTF-8 boundary for each slice. The 256-byte preview is an upper bound. Shorten previews and split strings or list fields as needed using their final encoded sizes in both channels, including quoting, nested JSON escapes, cursor and fragment metadata. Reserve room for continuation before packing. Use bounded fragment identifiers when the logical key itself needs text fragments. Size mandatory cursor and elision metadata so eight elided views fit the minimum request budget. Each running page must advance by at least one fragment and every fragment must fit the minimum share. A dedicated continuation page may carry the remaining string via the text handle. |

## 5. Architecture

```
  omnia-doc                         Doc::apply / undo / redo -> Change { .., net_delta }   (CD2)
      |
      v
  omnia-circuit (new; no GPUI, no async runtime)
    graph.rs     dense rows, CSR net membership, interner          (11.2)
    keys.rs      net keys and display names                         (CK1, CK2)
    bindings.rs  revision-matched pin-to-net bindings, ground evidence (C1.9, XD4)
    update.rs    incremental update, revision stamps, backdating     (CD5)
    journal.rs   signed facts per revision, since, checkpoints       (CD6)
    findings.rs  tier 0 and 1 in the edit; tier 2 lazy              (CD11)
    recognise.rs pattern library, matcher, channel hash              (CD12)
    view/        outline, net, focus, since, findings, path, check   (section 9)
    render.rs    line form and JSON into a reused buffer             (CK9)
      |                     |                          |
      v                     v                          v
  omnia-agent           omnia-app (later)          omnia-sim lowering (S1.2)
  `context` op,         inspector, findings,       reads Circuit::bindings:
  apply/preview delta   net highlight              pin to net, ground evidence
      |
  omnia-cli: MCP tool `omnia_context`
```

Data flows one way. A host (the agent service's document entry, later the app host) owns one
`Circuit` beside each `Doc` and calls `Circuit::update(&doc, &change)` after every `apply`,
`undo` and `redo` returns, never inside it. The agent service updates before it answers the
request, so the agent still sees one call. The app host updates off the frame path, after
paint or at idle, so B6 is unaffected. A view is answered only when the circuit's revision
equals the document's; otherwise the host updates or rebuilds first (CINV5).

### Crate ownership

| Crate | Owns | Must not |
| --- | --- | --- |
| `omnia-doc` (doc lane) | `Change::net_delta` and its computation (C1.1) | Depend on `omnia-circuit`; gain any circuit concept beyond the net delta |
| `omnia-circuit` (circuit lane, new) | Everything in the diagram's middle box: derived circuit facts | Mutate a `Doc` or call anything not public in `omnia-doc`; depend on GPUI, an async runtime, `omnia-agent`, `omnia-parts` or `omnia-sim`; assign solver nodes or write SPICE text |
| `omnia-agent` (agent lane) | The `context` operation's DTOs, schema, catalog entry and dispatch; the delta field on `apply` and `preview` results | Re-derive circuit facts itself |
| `omnia-cli` (agent lane) | The `omnia_context` tool mapping and its result channels (CD10) | Hold domain logic |
| `omnia-sim` and its engine adapters (simulation lane) | Lowering (`CircuitIr`, node ordering, device expansion, source maps, SPICE text), ERC as Run preflight, execution and results (sim plan, XD4, XD8) | Derive connectivity or ground a second way: it reads C1.9's bindings |
| `omnia-parts` (parts lane) | Catalog and import (parts plan, XD8) | Supply pin-to-net bindings: definitions and bindings are read from the `Doc` and from C1.9 |
| `omnia-bench` | `benches/circuit.rs` | Widen visibility to measure something |

All of these crates are headless and their dependencies run one way (XD8): `omnia-doc` at the
bottom; `omnia-parts` and `omnia-circuit` above it, independent of each other; `omnia-sim`
above `omnia-circuit`; `omnia-agent` above all three. `omnia_context` serves circuit views;
`sim.erc` is Run preflight. The parts plan's `set_param` and `update_part_def` operations use
the existing preview and apply edit path, so their effects appear in CD14 deltas. Sim SD14's
finite-number amendment applies to `sim.*` payloads only: `context` views keep parameter values
as their stored text (PD6's `raw`) and never emit floating-point numbers.

## 6. Decisions

### CD1 A separate, derived crate. PROPOSED

- `omnia-circuit` reads a `Doc` through its public API and the `Change` records `apply` returns.
  It never edits. Commands stay the only way to change a document (AD2).
- The derived model has one level of dependency (connectivity, then circuit facts, then
  findings and structure) and a fixed schema, so the incremental machinery is hand-written:
  revision stamps, early cutoff and signed deltas, borrowed as ideas from query-based
  incremental computation and dataflow systems [Ref11, Ref22, Ref23, Ref24]. No incremental-computation framework is
  adopted: each brings a runtime and memo tables onto a path that must stay allocation-free.

### CD2 The document reports a net delta. PROPOSED

- `Change` gains a transaction-level `net_delta` (one per `apply`, `undo` or `redo`, covering
  every step of a batch), with:
  - `retired`: pre-command logical `NetId`s that no longer exist;
  - `created`: post-command logical `NetId`s that did not exist before;
  - `mapping`: for each post-command net that gained or lost members, the pre-command nets its
    members came from;
  - `moved`: every pin, wire and label whose logical net changed, with its before and after
    `NetId` (absent for an item added or removed by the command);
  - `labels`: labels whose text changed, which can rename a net without moving any member;
  - `renumbered`: logical nets whose membership is unchanged but whose `NetId` changed (the
    free list can swap ids when an isolated part is detached and reattached), so a reader
    indexed by `NetId` can remap without comparing members.
- `Change` carries modified lists, so readers need not search `dirty`. Parts task P2.2 adds
  `modified: Vec<PartId>`: the parts whose `def` or `params` changed, where `SetParam` lists its
  part and `UpdatePartDef` lists every instance it moves, even when topology and display text
  are unchanged (PD8, XD7). `modified` keeps exactly that meaning; C1.1 does not widen it.
  C1.1 adds its own fields beside it: `edited_parts: Vec<PartId>`, the parts whose designator,
  value, position or orientation changed; `modified_wires` and `modified_labels`, the wires and
  labels whose endpoints, position or text changed. A part can be in both part lists (a
  `SetParam` on the primary parameter also rewrites `Part::value`, PD6). Undo and redo report
  the same items in the same lists.
- `nets_changed` keeps its D2 meaning and its current value everywhere, including the agent
  boundary, so existing responses keep their bytes. `net_delta` describes the final state
  only: a batch that changes a label from `A` to `B` and back sets `nets_changed` with an empty
  delta. C1.1 proves the one-way rule `!net_delta.is_empty() ⇒ nets_changed`.
- Connectivity already knows these facts when it unions and splits; reporting them is
  bookkeeping, not search. C1.1 fixes the exact types and proves CINV12.

### CD3 Net keys are a function of the document, not of its history. PROPOSED

- Keys follow CK1. A rule such as "the larger side keeps the key" is cheaper to maintain but
  depends on edit order, so the same final document could carry different keys. That breaks
  "same inputs, same output" unless keys are saved in the file (CQ1).
- The anchor is chosen by designator, then geometry, then id, as AGENTS.md's tie-break rule
  asks. The display name derived from it is therefore independent of insertion order, and the
  key is equivalent between insertion orders under the id mapping (CINV3). Unrelated edits preserve the anchor and key. Choosing by id first
  would not: inserting R2 before R1 would name the net `N(R2.1)` instead of `N(R1.2)`.
- The cost of the anchor rule: when an unlabelled net's anchor pin leaves it, or its part is
  renamed so that another pin sorts lower, its key changes. The journal reports that as a
  rename event, so a holder of the old key can follow it.
- Label keys use the lowest text, not the "best" by priority, so that adding a power label to a
  net never re-keys it unless its text sorts lower; the display name (CK2) uses priority.

### CD4 Names are shown, never silently resolved. PROPOSED

- Every label text on a net is a candidate name, kept with its kind. Two different texts on one
  net is the finding `names.multiple`; texts that differ only in case is `names.case`.
- Designator-derived names (`N(R3.2)`) are presentation. They change when parts are renumbered,
  and are never used as keys.

### CD5 Revision stamps, backdating and change tiers. PROPOSED

- Every derived record (net row, pin row, finding, recognised structure) stores `changed_at`.
  A recomputed record equal to the old one keeps its old stamp, so a wire re-route that changes
  no membership never appears in `since`.
- Invalidation is derived from what each record reads, not listed case by case:

  | Record | Reads | Marked when |
  | --- | --- | --- |
  | Net membership | connectivity | its `NetId` is in `net_delta` |
  | Net key and anchor | members' designators, pin numbers, pin world positions, ids; label texts; for wire islands, wire endpoints | membership changes; any member part in `added`, `removed` or `edited_parts`; any member label in `labels` or `modified_labels`; any member wire in `modified_wires` |
  | Display name, candidates, kind | key, anchor's designator, label texts and kinds, member roles | the key, anchor or any of those inputs is marked; a member part in `modified` (its `def`, so its roles, may have changed) |
  | Part row and outline line | designator, value, symbol and its class, `def` and its roles, `params`, each pin's net name | the part is in `added` or `removed`, or in either part list: `edited_parts` for designator, value, position and orientation, `modified` for `def` and `params` (every `SetParam` and `UpdatePartDef` instance, XD7); any of its pins' nets has a marked name |
  | Bindings and ground evidence (C1.9) | pin to net row; symbol class; label texts equal to `0` | membership changes; a part with class `Ground` added or removed; a member label in `labels` |
  | Designator group | the set of parts with one designator | a part enters or leaves it (added, removed, or renamed through `edited_parts`: both the old and the new group) |
  | Findings | per their rule's inputs | any input record is marked; grouped findings with their group; role-based findings (tier 1 and 2) on the nets of every part in `modified` |
  | Journal facts | `PartField` from designator and value; `PartParam` and `PartDef` from `params` and `def` | `PartField` for parts in `edited_parts`; `PartParam` and `PartDef` for parts in `modified` |

  Marks are processed in table order, so a part move that keeps membership still re-checks
  its nets' anchors (two pins with the same designator and number can swap order by
  position), and adding or deleting a duplicate designator re-checks every untouched part in
  that group.
- Each row has an incremental-against-rebuild fixture, including its undo (CTC-key-04,
  CTC-key-05, CTC-find-03).
- Change tiers: geometry and wires (tier 0); part and pin properties, including `SetParam`
  (tier 1); library definitions, including `UpdatePartDef` (tier 2). A tier-0 edit never
  revisits role-only work on nets it did not touch. A parameter or definition edit bumps the
  revision, so it reaches `since` and stale-binding checks even when no net changes (XD7).

### CD6 The journal stores signed facts in stable keys. PROPOSED

- Each revision appends a sorted vector of `(fact, +1 | -1)` records. Facts are circuit
  facts, never geometry: `PinOnNet(pin, key)`, `LabelOnNet(label, key)`,
  `LabelText(label, text)`, `NetName(key, name)`, `NetKind(key, kind)`,
  `PartField(part, field, value)`, `PartParam(part, name, raw)`, `PartDef(part, source id,
  content hash)` and `Finding(id, handles)`. `PartParam` and `PartDef` make every `SetParam`
  and every instance an `UpdatePartDef` moves visible in `since` (XD7). `LabelText` records every label
  text, so renaming a secondary label (one that is neither the key nor the display name) is
  still a change. Pins and
  labels are named by item id; keys by CK1. Wire membership is geometry and is not journaled:
  a split or merge is visible through the pins and labels that moved, and a net with no pins
  or labels is reported by the finding `net.no_pins`, not by events.
- `since {revision}` is the **net effect** between the state at `revision` and now: a k-way
  merge of the sorted entries after `revision` that sums and drops zeros, in O(F log k) for F
  facts over k entries. An edit followed by its undo consolidates to nothing. There is no
  separate lineage. For endpoints r0 and r1, seed from the current fact set and reverse retained
  entries after each endpoint (undo an addition by removing it and undo a removal by restoring
  it). An implementation can restrict this reconstruction to keys touched by the queried interval and their full
  pin and label memberships, including unchanged members and display names. For a historical
  r1, include keys and members recoverable only from the retained suffix. Use the consolidated
  diff to identify changes and the reconstructed endpoint sets to classify events. CB4 times
  reconstruction and classification as part of rendering a since page. Classify in this order,
  each member used once:
  1. `split`: the members of one before-key now lie in two or more after-keys, each holding at
     least two of them or the original key; one event names all after-keys.
  2. `merge`: two or more before-keys, each with at least two members, now share one
     after-key; one event names the before-keys.
  3. `rename`: a key whose members are unchanged but whose key or display name changed.
  4. `move`: any remaining member whose key changed, one event per member. A lone pin joining
     a net is therefore a `move`, not a merge.
  5. Field events: `value`, `param`, `def`, `designator`, `label`, `add`, `remove`, `finding`.

  Example lines: `E move R3.2 N(R3.2) -> GND`, `E merge N7 N9 -> GND`,
  `E split N(R1.2) -> N(R1.2) N(R2.1)`, `E value R3 10k -> 4k7`.
- Responses at the edges, with `base` as CK8 defines it: `revision` equal to now gives an
  empty view; `revision` ahead of now is the tool error `unknown_revision`; `revision` below
  `base` gives `history truncated at <base>` followed by the summary view, never a partial
  delta. Opening or rebuilding sets `base` to the current revision, so the first edit after
  open is answerable from `base` (and CD14 works on it). C1.4 tests each boundary right after
  open, after a rebuild and after an eviction.
- Test oracles (CINV4): compare the consolidated facts to the diff of two full snapshots.
  Independently classify events from the complete endpoint memberships and names from those
  snapshots and compare the event stream, including CTC-key-06 and CTC-key-07.

### CD7 Layout for speed. PROPOSED

- Document ids map to dense `u32` rows; columns are stored separately (struct of arrays).
- Net membership is compressed sparse rows: an offset array and one flat pin array, each net's
  slice sorted by (part id, pin index). A lookup is two loads and a contiguous slice, and output
  order is deterministic by construction. Slices an edit grows move to the end behind a
  free list; compaction runs at idle, never inside `update`, once free space passes a quarter
  of the array.
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
  net lists its pins) [Ref2, Ref3], and ends with handles to fetch more, so the summary and the
  next steps sit at the ends of the input, where they are used best [Ref6]. Absences are stated
  as findings, because models do not infer them [Ref2].
- Hub nets (CK7) appear inside other views as a count and a typed handle. The `net` view is the
  one direct, pageable member list: for a hub it pages the members instead of listing them
  inline, so the handle always leads somewhere that makes progress (CK14).
- When a view does not fit its budget, it stops at a record boundary (or splits one record,
  CK15) and ends with an elision line: what was left out, how many, and the cursor that fetches
  it. At one revision, following every cursor reconstructs the unbudgeted view exactly
  (CINV7). Ordinary cursors expire on an edit (CK14). CD14 defines edit-delta continuation.
- Two formats with the same content: `lines` (CK9, the default, for models to read) and `json`
  (typed rows, for programs). Agents write JSON arguments and never have to produce the line
  form: a study of compact notations found accuracy falls when models must write them [Ref7].

### CD9 One composite agent operation. PROPOSED

- `context {doc_id, views, budget?, format?}` takes a list of views (section 9) and answers all
  of them against one revision in one call. Its MCP tool `omnia_context` carries one to five
  worked examples in its description, because examples measurably improve argument accuracy on
  complex parameters [Ref30]. Few consolidated tools with concise responses and meaningful
  names are the published practice [Ref29].
- The tool set stays fixed and in a fixed order (the 2026-07-28 revision requires that tools
  not change as a side effect of other requests [Ref19]). No tool groups unlock during a session.
- `omnia_inspect`, `omnia_query` and `omnia_nets` stay as built in A1, and their descriptions
  point to `omnia_context` for circuit questions (CQ4).

### CD10 Both result channels carry the compact view. PROPOSED

- For `omnia_context` only, `structuredContent` is the 9.1 envelope whose `result` is
  `{views: [{view, revision, counts, elided?, lines: [string]}]}` for `format: "lines"`, or
  `{views: [{view, revision, counts, elided?, rows: [...]}]}` for `format: "json"`. The tool's
  `outputSchema` is the union of the two, so the structured result always validates, as every
  pinned revision requires. The text block is the `lines` joined by newlines; for
  `format: "json"` it is the serialized JSON, as 10.1 says.
- Errors keep mcp-plan.md 9.4 and 10.1 unchanged: `isError: true`, `structuredContent` is the
  9.4 error envelope (including CK12's three codes), and the text block is its serialized JSON,
  in both formats. The `outputSchema` covers success only, as 10.1 already does for other
  tools.
- Reason: clients reportedly differ on which channel reaches the model (some forward only the
  structured object, others only text) [Ref32, Ref33]. Every pinned revision makes the
  serialized-JSON text copy a SHOULD, kept for compatibility, not a MUST [Ref17, Ref18, Ref19]. Putting the compact lines in both is
  correct whichever channel is used. This departs from that SHOULD and from mcp-plan.md 10.1
  for this tool and format only (CQ3).
- Bytes: both channels travel, so the wire carries about twice the view; the model sees one.
  CB11 bounds each channel separately. C2.3 tests schema validation, escaping, metadata and
  the per-channel limits in all three AK1 revisions. The compact text exception applies only
  to successful `omnia_context` responses in `lines` format. Edit responses and all errors
  keep the serialized JSON text copy. With CD14 opted in, pack the circuit field within the
  remaining fully encoded edit envelope in both channels. If the envelope cannot fit mandatory
  metadata and at least one fragment with a continuation when needed, return a bounded `limit_exceeded` before committing the edit.
  Use fixed-size error metadata and omit echoed input when needed to meet the ceiling.

### CD11 Findings run where their cost allows. PROPOSED

- Tier 0 and tier 1 (CK5) are maintained inside `update`, within CB1.
- Tier 2 runs lazily on the first request after a revision and is cached for that revision
  (CB7). Tier 3 arrives with part knowledge and is always at `heuristic` evidence or below.
- Severities are data keyed by finding id (section 9.4), so they can be tuned without code.
  They are advisory: the simulation plan's Error and Warning policy on ERC001–ERC008 is the
  Run gate. Findings and ERC codes share predicates through the crosswalk in section 9.4
  (XD6).
- Every finding names at least one handle and the rule that produced it (CINV8).

### CD12 Structure is recognised by rules and patterns, not by learning. PROPOSED

- Matching runs on the bipartite part-and-net graph. Edges carry pin role classes rather than
  pin numbers (both resistor pins are class `P`), so interchangeable pins match without trying
  every order. Symmetric patterns are de-duplicated by requiring lowest-id order among
  interchangeable parts, after ordering them by designator and position, which is also the
  tie-break (CINV3).
- Every search is anchored at the pattern's rarest part kind and step-capped (CK11).
  Subgraph matching is hard by structure, not size: small real instances can defeat every
  solver [Ref15], so a capped search reports `truncated` rather than dropping results.
- Hub nets are labelled terminals the search never walks through. With hubs excluded, a pattern
  of radius r can only change within r hops of an edit, so rematching after an edit re-seeds
  only that neighbourhood (CB8). A change in a net's hub status rematches every pattern that
  used it.
- Repeated channels are found by a fixed, versioned Weisfeiler-Lehman hash written in the crate
  (about 100 lines) [Ref16], and each hash group is confirmed by the matcher before it is called
  identical: such hashes can collide, and one library's hash values changed between its
  versions [Ref27]. Two levels are reported: `same topology` (values ignored)
  and `identical` (values equal); a channel differing in one value is reported as such.
- Learned classifiers are excluded: in published comparisons, plain pattern matching was as
  accurate (precision about 0.96, recall about 0.99 in one study [Ref13]; a learned annotator at
  90.9% accuracy in another [Ref14]; the reference open flow uses matching for primitives [Ref12]),
  learned output can differ across hardware, and only a match explains itself. Community
  detection is excluded from facts: it is reproducible only with a fixed seed and
  implementation [Ref28]; if ever offered, it is seeded, debounced and labelled `suggested`.

### CD13 Ground and power come from declarations. PROPOSED (ground evidence DECIDED by XD4)

- The ground reference is set only by declared ground evidence (CK4): a `Ground`-class part or
  a label whose text is exactly `0`, which is sim SQ3's default (XD4). Names like `GND` never
  set it. SPICE requires the reference to be node `0` [Ref21], and a lowering that maps only
  certain names to it leaves a net named `AGND` floating [Ref26]. A net labelled `GND` with no
  declared evidence is not a reference; it gets the hint `ground.name_only`, which sim ERC001
  shares (section 9.4).
- Rails are recognised in evidence order: declared (power symbols, `power`-role pins), then
  name heuristics, then structure (a regulator output, many decoupling capacitors).
- Name and topology heuristics feed findings, views and recognition only. They never change
  C1.9's bindings or ground evidence, so they never change solver connectivity (XD4).

### CD14 Every edit returns its own circuit delta. PROPOSED

- `preview` and `apply` gain an optional request field `circuit: {format?, budget?}`. When it
  is present, the result gains a `circuit` field: the `since` view for exactly that edit. When
  it is absent, the request and the response bytes are exactly as today, so existing strict
  decoders are unaffected. An agent can verify an edit ("R3.2 is now on GND") with no extra
  call (CB12). Both are optional fields, a minor change under AK7.
- A validated no-op returns an empty `circuit` view. `preview` computes it on the fork. The
  `circuit` option is not part of the operations a `preview_id` binds (11.3 of mcp-plan.md):
  an `apply` with a `preview_id` may ask for a different format or none. When `preview` and
  `apply` ask for the same format and budget from the same revision, their `circuit` fields
  are byte-identical (AINV2 extended). An idempotent replay returns the stored bytes,
  `circuit` included, and a replay with a different `circuit` option is
  `idempotency_conflict` (AINV7).
- A paged edit delta is an immutable result in the owning agent session. Its bounded opaque
  cursor binds the open document instance, starting revision, canonical operations, resolved
  format and budget, and fragment position, with a separate cursor namespace from live views.
  Use the same result identity for a preview and its matching apply. Store the frozen delta
  and text fragments, so a cursor submitted to `context` resolves against that result even
  before apply and after live revisions advance. A different edit at the same starting revision
  has a different identity.
- Retain the result for as long as any AK5 cache entry can replay it, including through
  `request_status`. For results without that protection, retain the most recent AK5 results
  per session by creation order. Eviction, session end or document close invalidates the
  cursor with `stale_cursor`. On close, keep replay bytes under the existing idempotency rules.
  Continuation ends at the final frozen fragment. Record retained memory and page preparation
  cost in C2.2 diagnostics without changing any budget threshold.
- The parts plan's `set_param` and `update_part_def` operations are edits on the same path, so
  they take the same `circuit` option and report `param` and `def` events (XD8).

### CD15 Determinism is proved, not assumed. PROPOSED

- Every public output (keys, names, journal facts, finding order, rendered bytes) is a pure
  function of the current document, plus the journal for `since`.
- Proved by three property tests: incremental equals rebuild (CINV2); the same document built
  in shuffled insert orders renders the views CINV3 defines; journal consolidation equals the
  snapshot diff (CINV4).
- Every visible order and tie (outline order, member order, anchor, path ties, symmetric
  pattern order) uses document content and geometry before ids, so only id-bearing tokens can
  differ between insertion orders.

### CD16 Understanding is measured. PROPOSED

- A circuit question set (section 10) is the acceptance test for "understanding". In CI, a
  scripted call plan per question must surface the answer explicitly within CB11 and CB12. At
  HC3 the owner runs the set with a model through an MCP client and records accuracy, tokens and
  calls per question family, with and without the context tool.

### CD17 Document text stays data. PROPOSED

- Extends AD14. Designators, values and label text appear only in the CK9 string form in the
  line form, bare only when they are plain identifiers, and as string fields in JSON. Each
  view's legend says that every token after the record tag is data. Because newlines,
  controls, quotes and separators force quoting and escaping, document text cannot end a line,
  start a record, imitate a generated name or the legend (CINV9).

### CD18 Bindings are bound to one exact document state and ordered by content. PROPOSED

- **State identity.** A revision number alone does not name a document state: two forks of one
  document (A1.1, AD3) start at the same revision, take different edits and reach equal
  revision numbers with different circuits. `omnia-doc` therefore gives every `Doc` value a
  **lineage token**, `Doc::lineage() -> DocLineage`: an opaque `u64` drawn from a process-wide
  atomic counter when a `Doc` is created by `new`, `Default`, `load` or `fork`, never changed by
  `apply`, `undo` or `redo`, never saved, and excluded from `PartialEq` (so `fork() == self`
  still holds; `Default` is written by hand so it does not yield a shared zero). `Doc` has no
  `Clone`, so no two live values share a token. Within one lineage every command that changes
  the document, and every undo and redo, advances the revision (`apply_command`, `undo.rs`); a
  validated no-op changes neither. So the pair (lineage, revision) names exactly one document
  state. A `Circuit` records the pair it was built or updated from;
  `bindings`, every view (CINV5) and `update` compare both and refuse or rebuild on any
  mismatch.
- **Why a token, not a content fingerprint.** A fingerprint must either be recomputed over the
  whole document on every read (O(document), against CB3's 50 µs) or be maintained inside
  `Doc::apply`, where the worst certified case has about 0.08 ms of headroom (section 2).
  Hashes can also collide, while the token is exact. The token's cost is one `u64` per `Doc`
  and one comparison per read. Its one conservative effect: the same content reached on two
  lineages, or again after an undo, is treated as a different state and rebuilt, never
  confused.
- **Net and pin order.** Bindings, and the `net` view's member list, order pins by (designator
  in natural order, pin number in natural order, pin world position x then y, part id, stored
  pin index): content and geometry before ids, as AGENTS.md's determinism rule asks, so two
  pins with equal designator and pin number never sort by insertion order. Nets are ordered in
  three groups: nets with pins, by their lowest pin in that order; then label-only nets (labels
  but no pins), by lowest label text in byte order, then that label's position (x, then y),
  then label id; then wire-only islands, by CK1's anchor wire (lower endpoint x, then y, then
  wire id). Labels within a net are ordered by text in byte order, then position, then id.
  Ids decide only between items equal in every content and geometry field (CINV3).

### 6.1 Cross-plan decisions (XD4–XD9). DECIDED (coordinator, owner-delegated, 2026-11-23)

The owner delegated pending sign-offs to the coordinating agent on 2026-11-23. After an
independent review of the parts plan (PR #85) against `sim-plan.md` and this plan, the
coordinator decided XD1–XD9. They bind this plan and are not reopened at HC1. XD1–XD3 (one
document schema, one numeric grammar, one model source) touch this plan only through the parts
and simulation tasks it waits for. The text of the ones that touch this plan is copied here,
followed by where it applies.

- **XD4 Bindings, ground and lowering.** `omnia-doc` owns connectivity. `omnia-circuit`
  (context plan) exposes revision-matched pin-to-net bindings and declared ground evidence
  through a small read-only interface task that precedes S1.2 (split from C3.1 if needed).
  `omnia-sim` owns lowering: `CircuitIr`, node ordering, device expansion, source maps (many
  nets per solver node, many devices per part), diagnostics mapping and SPICE text. CK1 net
  keys, MCP `NetKey` and solver `NodeId` stay distinct types. Ground: sim SQ3's default stands
  (a `Ground` symbol or a net labelled `0` is node 0; a `GND` label alone is not, and gets a
  hint). Context CD13's name/topology heuristics feed context findings only, never solver
  connectivity. Parts' netlist/"cards" wording becomes "definitions and bindings"; lowering is
  sim's.
  *In this plan:* section 1, section 5, CK1, CK4, CD13, CQ9, `Circuit::bindings` (section 12),
  CINV14, CD18, CTC-bind-01, CTC-bind-02, CTC-gnd-02, task C1.9.
- **XD5 Symbol class vs pin role.** Two separate types: sim S1.1's symbol class (`Component`,
  `Ground`, `PowerRail`, `Instrument`, `Probe`) and parts PD5's per-pin electrical roles (plus
  context's derived `unspecified`). A placed part's embedded definition is authoritative for
  pin roles and SPICE terminal order. Context CQ2 stays at its default (`power.undriven` off
  until PD5 is extended). `pins.toml` and the symbol compiler have one editing task at a time:
  P1.4 (starter parts), then S1.1 (remaining analog symbols), then S3.2 (instrument artwork).
  *In this plan:* CK3, CQ2. No context task edits `pins.toml` or the symbol compiler.
- **XD6 ERC crosswalk.** Shared predicates, separate severity per surface. ERC001 ↔
  `ground.missing`/`ground.name_only`; ERC002 ↔ `designator.duplicate`; ERC003 ↔ typed param
  validation and legacy `value.unparsed`; ERC004 ↔ `source_loop` where the predicates coincide;
  ERC006 ↔ `pin.unconnected`/`net.single_pin`; ERC008 is a model-required supply check on
  shared pin facts (SQ8's implicit ±15 V does not satisfy it; it downgrades to a warning when
  the implicit supply is used). ERC005 stays simulation-only; ERC007 is retired (XD2). The sim
  Error/Warning policy is the Run gate; context severities are advisory. Case folding and blank
  designators are defined once, in the context plan, and the sim plan cites it.
  *In this plan:* CD11, section 9.4 (crosswalk, case folding, blank designators).
- **XD7 File ownership order.** `omnia-doc` `doc.rs`/`command.rs`/`undo.rs`/`io.rs`: parts
  P2.1–P2.3 first, then sim S1.1 (which also lists `undo.rs`), then context C1.1 per CQ10 once no
  open task owns those files. `SetParam` and every instance an `UpdatePartDef` touches appear in
  `Change`, undo/redo, context invalidation and `since`, and SD11 stale-result handling, even
  when topology and display text are unchanged.
  *In this plan:* CD2, CD5, CD6, CQ10, task C1.1.
- **XD8 Crates and agent surface.** `omnia-parts` (catalog, import), `omnia-circuit` (derived
  circuit facts), `omnia-sim` + adapters (execution, results); all headless, dependencies
  one-way. Parts' `set_param`/`update_part_def` use the existing preview/apply edit path and
  appear in CD14 deltas. `omnia_context` serves circuit views; `sim.erc` is Run preflight.
  SD14's finite-number amendment stays scoped to `sim.*`. FH0's binary format stays separate
  from the RON migration.
  *In this plan:* section 5 (crate ownership), CD14.
- **XD9 Ids and budgets.** Context §16 uses citation ids `Ref1`–`Ref33`, distinct from canvas spike ids. ERC001–ERC008 are registered as diagnostic codes. Every budget
  keeps its threshold (PB5, CB1, SB1–SB3, B9, UB10, AB2); SB3's Run measurement includes
  context/snapshot acquisition, and SB1/SB2 evidence distinguishes cold, incremental and cached.
  *In this plan:* section 16 and every citation; section 3 (no threshold changed). C1.7
  records the whole-sheet `Circuit::bindings` call, cold and after an incremental update, so
  sim's SB3 and SB2 evidence can show the acquisition share; the record gates nothing.

## 7. Invariants

Each has a test; the roadmap task that adds it is in brackets.

| ID | Rule |
| --- | --- |
| CINV1 | `omnia-circuit` never mutates a `Doc` and does not depend on GPUI, an async runtime or `omnia-agent`; `omnia-doc` does not depend on it. [C1.2] |
| CINV2 | After every step of a random sequence of commands (including `SetText` renames and duplicate designators), undos and redos, the incrementally updated circuit equals `Circuit::build` on the same document: graph, keys, names, outline cache, tier 0 and 1 findings. [C1.3, C1.6, C3.1] |
| CINV3 | A document rebuilt with its items inserted in another order renders identical views after replacing every id-bearing token (item ids, `net:pin:` and `net:wire:` keys, `N(wire:<id>)` names, cursors) with its counterpart under the id mapping. For content-and-geometry ties, map indistinguishable items by ascending id within each tied group, preserving references and tied anchors. Use order-preserving mapped ranks for sorted typed handles within such groups. Compare decoded cursor bindings and positions under the same mapping. [C1.5] |
| CINV4 | For any r0 < r1 within the retained journal, the consolidated journal over (r0, r1] equals the diff of the two snapshots' fact sets; an edit followed by its undo consolidates to nothing. Independently classify events from full endpoint snapshots and compare with journal-derived events, including retained memberships and historical endpoints. [C1.4] |
| CINV5 | No view is answered from a circuit whose revision differs from the document's. [C1.3, C2.1] |
| CINV6 | Allocation proof, counting `alloc` and `realloc`, each test with a negative control that must count above zero. Warm means: after `build`, plus one pass of the same fixed case set. Then query and render paths allocate nothing; a rerun of the same fixed edit cases through `update` allocates nothing (the journal is preallocated, CK8; CSR slices have the spare capacity the warm pass gave them). Excluded and stated: the first computation of a lazy tier 2 result per revision, compaction (which runs only on idle), edits that introduce new strings, and allocation of the single oversized journal entry in CK8 (only when its fact count exceeds 262,144). Separately count that path with a one-over-capacity entry, verify that only its storage allocates, verify lossless paging and eviction, and rerun the warm zero-allocation check on the next ordinary edit. [C1.7] |
| CINV7 | Every elision states a count and a cursor. At one revision, following every cursor from the budgeted view reconstructs the unbudgeted view exactly, including hub member lists and records split by CK15; ordinary cursors used after an edit return `stale_cursor`; frozen edit-delta pages and text handles reconstruct the same result throughout CD14 retention. [C1.5, C2.2] |
| CINV8 | Every finding names at least one handle and its rule id; every recognised structure carries an evidence tier; nothing below `exact` is listed among facts. [C1.6, C4.1] |
| CINV9 | For arbitrary label, designator and value text, the line form parses back to the same records, and no text can produce a line that is not its own record. [C1.5] |
| CINV10 | No output depends on hash-map iteration order: enforced by the crate's `clippy.toml` and checked by CINV3. [C1.2] |
| CINV11 | Fully serialized channels, including envelopes, escaped fragments and errors, stay within CB11 and the effective CK10 budget for context and opted-in edits. [C2.1, C2.2, C2.3] |
| CINV12 | After every `apply`, `undo` and `redo`, `net_delta` applied to the previous logical partition of pins, wires and labels, with `NetId`s remapped through `renumbered`, reproduces the partition and the `NetId`s `Doc` reports; `modified` lists exactly the items whose stored fields changed; `!net_delta.is_empty() ⇒ nets_changed`. [C1.1] |
| CINV13 | An alias that matches more than one entity is refused with `ambiguous_handle` with the exact count and the bounded sorted prefix from CK13, even if no complete handle fits. [C1.5] |
| CINV14 | `Circuit::bindings` answers only when the circuit's (lineage, revision) pair equals the document's (CD18), and carries both; two forks at the same revision with different edits are told apart. Every pin of every live part appears exactly once, on the net connectivity reports for it; the ground evidence is exactly the declared set of CK4 and is unchanged by any name or topology heuristic; the result equals the one a fresh `Circuit::build` gives, and is identical across insertion orders after the id mapping (CINV3), with pins and nets in CD18's order. [C1.9] |

## 8. Scenarios

Each is a test with the required result. Agent transcripts live beside the A1 ones under
`crates/omnia-cli/tests/transcripts/`.

| ID | Setup and action | Expected result |
| --- | --- | --- |
| CTC-key-01 | Resistors R1 (`part:0`) and R2 (`part:1`); one wire from R1 pin 2 to R2 pin 1; no labels. Delete the wire | Before: the joined net has anchor R1.2, key `net:pin:0.1`, name `N(R1.2)` (R1.1 and R2.2 are single-pin nets, unchanged throughout). After: the joined net becomes `N(R1.2)` (key unchanged) and `N(R2.1)` (key `net:pin:1.0`); `since` shows `E move R2.1 N(R1.2) -> N(R2.1)`: only one member left, so CD6 rule 4 applies, not a split |
| CTC-key-06 | R1.2, R2.1 and R3.1 on one unlabelled net (wire R1.2 to R2.1, wire R2.1 to R3.1); delete the first wire | The original key `net:pin:0.1` survives on R1.2; R2.1 and R3.1 form `N(R2.1)`; `since` shows `E split N(R1.2) -> N(R1.2) N(R2.1)` |
| CTC-key-07 | Move two pins from a net to another existing net while no member stays on the original key and a single member goes to a third key | Each transferred pin is a move. Compare with CTC-key-06, where stationary membership on the original key qualifies the split. Check both classifications again with later edits beyond the queried endpoint. |
| CTC-key-02 | Net labelled `VCC`; move an unrelated part | Key `net:"VCC"` unchanged; `since` empty |
| CTC-key-03 | Build the CTC-key-01 sheet with R2 inserted before R1 | Name `N(R1.2)` in both; views identical after the id mapping (CINV3) |
| CTC-key-05 | Two parts both designated `R3` on one unlabelled net through pin 1; move them so their pin positions swap, membership unchanged; then undo | The anchor follows the pin with the lower position; key and name change as CK1 says; incremental equals rebuild after the move and after the undo |
| CTC-key-04 | CTC-key-01 before the delete; rename R1 to R9 with `SetText`, then undo | After the rename: anchor R2.1, name `N(R2.1)`, R2's outline line shows the new name although R2 was not edited; `since` shows a rename; after undo, incremental equals rebuild and `since` from before the rename is empty |
| CTC-since-01 | Re-wire R3 pin 2 from an unlabelled net to `GND` | `since` holds one event: `R3.2 moved N(R3.2) -> GND` |
| CTC-since-02 | Any edit, then undo; `since` the pre-edit revision | Empty |
| CTC-since-03 | 1,100 edits; `since` revision 1 | `history truncated at <base>` and the current summary, never a partial delta |
| CTC-since-04 | Open a document at revision 42; one edit; `since 42`. Then fill the journal until an eviction and ask `since base` and `since base - 1` | `since 42` returns the edit (CD14 works on the first edit); `since base` answers, `since base - 1` is truncated |
| CTC-since-05 | Net `A` with a second label `Z1`; rename `Z1` to `Z2` | `since` holds `E label <label id> Z1 -> Z2`; key and display name unchanged |
| CTC-since-06 | One edit that re-keys more than 262,144 pin facts | The edit's own `since` answers, paged by cursor; every older revision is truncated |
| CTC-name-01 | Labels `VCC` and `V3V3` shorted by a wire | One net, key `net:"V3V3"`, display name `V3V3` (lowest by byte order), candidate `VCC`; finding `names.multiple` naming both labels |
| CTC-gnd-01 | A net labelled `GND` with no ground part | `is_ground_reference` false; finding `ground.name_only` at `heuristic` |
| CTC-gnd-02 | Three nets: one with a `Ground`-class part, one labelled `0`, one labelled `AGND` with a `ground`-role pin (after C3.1) | The first two have `is_ground_reference` true with evidence `ground_symbol part:<id>` and `zero_label label:<id>`; the third has kind `ground` (declared) but is not a reference, and `bindings` lists no evidence for it |
| CTC-bind-01 | CTC-key-01's sheet; read `bindings`; apply `SetParam` on R1 (after parts P2.2), then a wire delete, then undo both. Then fork the document twice at one revision, delete the wire on one fork and move R2 on the other, so both forks reach the same revision; build a circuit from the first fork and ask it for the second fork's bindings | Each read carries the document's lineage and revision; a read against a circuit not yet updated is refused as stale; the `SetParam` changes no binding but advances the revision; after each step the bindings equal a fresh build's. The cross-fork read is refused as stale although the revisions are equal |
| CTC-bind-02 | Two parts both designated `R3`, each with pin 1 on one unlabelled net, built in both insertion orders; plus a label-only net `A` and a wire-only island | `pins` lists the two `R3.1` pins by position in both orders; the label-only net follows every net with pins and precedes the wire island; bindings are identical after the id mapping (CD18) |
| CTC-find-04 | Parts `r1` and `R1`; two `Ground` parts and one `PowerRail` part with blank designators; one resistor with designator `"  "` | `designator.duplicate` groups `r1` and `R1` (ASCII case folding); no duplicate among the blank designators; `designator.blank` names only the resistor |
| CTC-find-01 | A part pin alone on its net | Finding `pin.unconnected` naming the pin |
| CTC-find-02 | An op-amp input pin with no driver (after C3.1) | Finding `input.undriven` naming the pin and its net |
| CTC-find-03 | Parts R1 and R2; rename R2 to R1 with `SetText`, then undo; then place a third part R1 and delete it | `designator.duplicate` names every part in the group, including the unedited ones, after each step; it is gone after the undo and after the delete; incremental equals rebuild at each step |
| CTC-order-01 | Two parts both `R3`, values `1k` and `2k`, built in both insertion orders | Identical outline bytes: the `1k` line first in both (CINV3) |
| CTC-view-01 | `focus R3` on `10k` | One call; ≤ 1 KB; each pin's net and that net's other members (hubs as count and handle) |
| CTC-view-02 | `outline` of `10k` at the default budget, then follow every cursor | Pages concatenate to the unbudgeted outline (CINV7); each page ≤ CB11 |
| CTC-view-03 | `net` on a 400-pin hub at a 4,096-byte budget, following every cursor; then an edit between two pages; then a part with a 5,000-byte value, in both formats | Pages reconstruct all 400 members; the cursor after the edit is refused with `stale_cursor`; the value shows up to 256 bytes plus a `text` handle whose pages return all 5,000 bytes; JSON fragments concatenate to the unbudgeted row |
| CTC-view-04 | 8 views at a 4,096-byte budget | Views whose share is below 2,048 bytes return one elision line each with a cursor that runs them alone; following them reconstructs every view |
| CTC-view-05 | Page values made of control characters, quotes, backslashes and non-ASCII scalars, including strings below the raw-length cutoff | Each fully encoded channel fits, cursors advance, and reassembly recovers the exact strings in both formats. |
| CTC-hub-01 | A ground part and a label `GND` on a 400-pin net; `focus` on a part tied to it | `GND (400 pins)` and the typed handle `net:"GND"`; no inline member list; `{"net": {"name": "net:\"GND\""}}` pages the members |
| CTC-handle-01 | Two parts both designated `R3`, and a label with text `R3` | `focus R3` is refused with `ambiguous_handle` listing `net:"R3"`, `part:<a>` and `part:<b>`, sorted by handle text; `focus part:<a>` succeeds |
| CTC-handle-02 | Resolve a duplicate alias with thousands of matches, including typed keys too large to fit after JSON escaping | The error includes the exact total and only the fitting sorted prefix, possibly empty. Both channels fit even at the minimum budget. |
| CTC-inj-01 | Label text containing a newline, `|`, `.`, `:`, quotes, non-ASCII and an instruction-like sentence; a designator `A.B` with pin `C` and a designator `A` with pin name `B.C` | Each is one quoted, escaped token; `"A.B".C` and `A."B.C"` stay distinct; everything parses back to the same text (CINV9) |
| CTC-rec-01 | Two resistors in series from a declared rail to ground, tap to an op-amp input | Divider at `exact`, ratio at `derived`, citing both resistors and three nets |
| CTC-rec-02 | Eight RC channels, one with a different resistor value | One `same topology` group of 8; `identical` group of 7; the odd channel named with its differing value |
| CTC-mcp-01 | `omnia_context` with `outline` and `findings` over MCP, in both formats, plus one request that fails with each CK12 error, in all three AK1 revisions | Exact transcripts; for `lines`, the `structuredContent.result.views[*].lines` joined equal the text block; results validate against the `outputSchema`; errors carry the 9.4 envelope; each channel's serialized length is within CB11 (CD10) |
| CTC-apply-01 | From one starting revision on two forks: `preview` then `apply` with `circuit: {}` a wire joining R1.2 to `VCC`; and the same `apply` without `circuit`; then a no-op; then an idempotent replay | Preview and apply `circuit` fields are byte-identical and hold `E move R1.2 N(R1.2) -> VCC`; the request without `circuit` returns exactly today's bytes; the no-op gives an empty view; the replay returns the stored bytes |
| CTC-apply-02 | Page a large escaped preview delta before apply, apply the same operations and options, perform another edit, then replay by idempotency key and request status | Preview and apply have identical circuit bytes, including cursors. All paths page the same frozen delta and text. Check both formats, fully encoded outer envelopes, and expiry after retention ends. |

## 9. Views

### 9.1 Requests

```json
{ "op": "context", "params": {
    "doc_id": "d1",
    "views": [
      { "outline":  { "limit": 200 } },
      { "cursor":   "c7f1" },
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
| `outline` | Every part, one line each, with each pin's net | Designator natural order (`R2` before `R10`), then value, symbol, position (x, then y), then part id |
| `net` | One net: name, key, kind, candidate names, members; the pageable member list for hubs | Members by (designator, pin number), then pin position (x, then y), then (part id, pin index) (CD18) |
| `focus` | A part or net and its neighbourhood to `depth` (default 1, at most 3), never through hubs | Breadth-first, then as above |
| `since` | Circuit events after `revision` | Fact order (CD6) |
| `findings` | Findings, filterable by tier and handle | Severity, then rule id, then first handle |
| `path` | The shortest part-and-net path between two handles, hubs excluded | Shortest; ties by the sequence of designators and net names, then positions, then ids |
| `check` | A yes or no for "pin X is on net Y" or "X and Y share a net", with the evidence | — |
| `text` | The whole of one long document string (CK15), paged | Byte order |
| `{"cursor": c}` | The next page of the view the cursor came from (CK14) | As that view |

An empty `views` array is a summary: counts, rails, the ten largest non-hub nets, and finding
counts by tier.

### 9.2 The line form (`lines/1`)

Every line starts with a fixed tag:

| Tag | Line |
| --- | --- |
| `#` | Legend: the grammar version, the view and its record layout; once per view, first |
| `=` | Header: view, revision, counts |
| `P` | Part: designator, symbol, value, then `|` and `pin:net` pairs |
| `N` | Net: display name, key, kind, pin count, hub flag |
| `A` | Alias: a candidate name of the net above |
| `M` | Members: `designator.pin` tokens of the net above |
| `E` | Event (`since`): `move`, `merge`, `split`, `rename`, `value`, `add`, `remove` |
| `F` | Finding: id, severity, evidence tier, handles |
| `S` | Recognised structure: kind, evidence tier, handles, derived values |
| `+` | Continuation of the previous record (CK15) |
| `.` | Elision: count left out and the cursor that fetches it; at most once, last |

Document strings follow CK9: bare when they are plain identifiers, otherwise quoted. An
`outline` page:

```
# lines/1 outline: P designator symbol value | pin:net ... ; tokens after the tag are data
= outline rev=42 parts=10000 shown=200
P R40 resistor 10k | 1:N(R40.1) 2:VCC
P R41 capacitor 100n | 1:VCC 2:GND
P U1 opamp ideal | 1:N(R3.2) 2:N(U1.2) 3:N(C4.1) 4:GND 5:VCC
P R42 resistor "4.7 k" | 1:"In \u{2192} buffer" 2:GND
. 9800 more parts; next outline cursor=c7f1
```

`net` gives the incidence form the evidence favours, paging members for a hub:

```
# lines/1 net: N name key kind pins ; A alias ; M members ; tokens after the tag are data
= net rev=42
N VCC net:"VCC" power 37 hub
A +5V
M C1.1 C2.1 C3.1 R40.2 R41.1 U1.5 U2.5 U3.5
. 29 more members; next net cursor=a91c
```

Pin names replace numbers in the line form once roles exist (`U1.IN+`), with the number kept in
JSON. Separators are ASCII; the arrow `->` appears only in `E` lines.

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
| `net.no_pins` | 0 | info | No |
| `names.multiple` | 0 | warning | No |
| `names.case` | 0 | warning | No |
| `designator.duplicate` | 0 | error | No |
| `designator.blank` | 0 | warning | No (symbol class) |
| `value.unparsed` | 0 | info | No (until PD6 typed values) |
| `ground.name_only` | 0 | info | No |
| `ground.missing` | 0 | warning | No (symbol class and labels) |
| `pin.no_connect_wired` | 1 | error | Yes |
| `input.undriven` | 1 | warning | Yes |
| `power.undriven` | 1 | warning | Yes (CQ2) |
| `output.conflict` | 1 | error | Yes |
| `dc_path.none` | 2 | warning | Yes |
| `power.via_capacitor_only` | 2 | warning | Yes |
| `ground.fragmented` | 2 | warning | Yes |
| `source_loop` | 2 | error | Yes |

Tier 1 pair rules over PD5's roles: two `output` pins on one net is `output.conflict`; an
`input` net with no `output` or `bidir` pin is `input.undriven`; a `power` net with no source is
`power.undriven`, which needs a source role PD5 lacks (CQ2) and is off until answered.

`ground.missing`: the sheet has at least one part whose symbol class is not `Ground`, and no
net carries declared ground evidence (CK4). It is a per-sheet count kept in the edit, so it
needs no roles. `ground.name_only`: a net whose label text folds (below) to `gnd`, `agnd`,
`dgnd`, `pgnd` or `vss` carries no declared ground evidence. This five-name list is broader
than XD4, which names only the `GND` case for sim's ERC001 hint. The extra names are advisory
context hints only: they never set the reference or kind `declared`, and sim may cite or
ignore them.

**Case folding** (defined here once; the simulation plan cites it, XD6). Two designators, or
two label texts, are the same name when they are byte-equal after ASCII case folding (`A`–`Z`
to `a`–`z`); no other folding, trimming or Unicode normalisation applies. `designator.duplicate`
and `names.case` use this rule, and so does sim ERC002.

**Blank designators** (defined here once, XD6). A designator is blank when it is empty or
contains only ASCII spaces and tabs. A blank designator never joins a `designator.duplicate`
group, on any part. On a part whose symbol class is `Ground` or `PowerRail` it is expected and
raises nothing; on any other part it raises `designator.blank`.

**ERC crosswalk** (XD6). Shared predicates, separate severity per surface: the simulation
plan's Error and Warning policy is the Run gate (`sim.erc`, sim SD8); the severities above are
advisory and never block a run. Where a row says "where the predicates coincide", only the
shared cases map; each side keeps its own extra cases.

| ERC code | Context finding ids | Shared predicate |
| --- | --- | --- |
| ERC001 | `ground.missing`, `ground.name_only` (the hint for a `GND` label alone) | Declared ground evidence, CK4, read from C1.9 |
| ERC002 | `designator.duplicate` | Case folding and blank designators, as defined above |
| ERC003 | none for modelled parts (typed parameter validation, PD6 and PD10, refuses bad values at `apply`); `value.unparsed` for legacy unmodelled parts | The parts plan's typed parameter validation. The overlap is partial: `value.unparsed` covers only unparseable legacy text, while a legacy unmodelled part whose value parses still fails sim preflight because it has no model binding (`sim: None`, PD8), which no context finding reports |
| ERC004 | `source_loop`, where the predicates coincide | A loop of ideal voltage sources, read from C1.9's bindings |
| ERC005 | none: simulation only (instrument ports and probes) | — |
| ERC006 | `pin.unconnected`, `net.single_pin` | A pin alone on its net (with roles, not for `no_connect`) |
| ERC007 | none: retired (XD2) | — |
| ERC008 | none: a model-required supply check that sim owns; context exposes the shared pin facts (`power`-role pins and their nets, C1.9 and C3.1) | Sim's rule; SQ8's implicit ±15 V supply does not satisfy it and downgrades it to a warning |

`designator.blank`, `label.dangling`, `net.no_pins`, `names.*`, `pin.no_connect_wired`,
`input.undriven`, `power.undriven`, `output.conflict`, `dc_path.none`,
`power.via_capacitor_only` and `ground.fragmented` have no ERC code.

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

A 2025 experimental study of published fully dynamic connectivity structures [Ref8] found large
constants, high memory and pathological cases, and judged them not ready to deploy as is.
Omnia's union-by-size plus two-ended search, where the smaller side moves, is the classic
bounded-relabelling approach [Ref9] and stays. Two cheap refinements are planned only if
measurement asks for them (C5.2):

- A spanning-forest bit per wire, over geometric connectivity (wires and the points they
  join): set when an insert joins two components, including a wire that extends a component
  to a previously unoccupied endpoint; clear when both ends were already in one component,
  that is, when it closes a cycle. Deleting a wire with the bit clear can never split a net
  and needs no search [Ref8].
  The bit is only correct if replacements are promoted: when deleting a tree wire leaves the
  component connected, the search must mark one wire that crosses between the two sides as a
  tree wire. Otherwise, in a four-wire cycle, deleting a tree wire and then the old closing
  wire would skip a real split. If the search cannot name such a wire cheaply, the shortcut is
  not built. The two-delete cycle case joins the INV10 and CINV12 tests.
- Multi-wire commands (paste, delete selection) run all unions first, then one multi-source
  search per affected component, as batch-dynamic work does at far larger scale [Ref10].

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
    pub fn resolve(&self, handle: &str, allowed: Kinds) -> Resolution<'_>;        // CB3, CK13
                                         // Resolution::{One(Entity), Many(&[Entity]), None(near)}
    pub fn render(&self, req: &ViewRequest, budget: usize, format: Format,
                  out: &mut Vec<u8>) -> Rendered;                                 // CB4, CB5
    pub fn since(&self, revision: u64) -> Since<'_>;                              // CD6
    pub fn findings(&mut self, tier: Tier) -> Findings<'_>;                       // CD11
    pub fn bindings(&self, doc: &Doc) -> Result<Bindings<'_>, Stale>;            // C1.9, CINV14
                                         // Stale unless (doc.lineage(), doc.revision()) equals
                                         // the state this circuit was built or updated from
}

// Read-only interface for simulation lowering (XD4). Borrowed, so a read allocates nothing.
pub struct Bindings<'a> { /* lineage, revision, nets, pins, ground evidence */ }
impl<'a> Bindings<'a> {
    pub fn lineage(&self) -> DocLineage;                 // equals Doc::lineage()
    pub fn revision(&self) -> u64;                       // equals Doc::revision()
    pub fn nets(&self) -> impl Iterator<Item = BoundNet<'a>>; // CD18 net order
    pub fn net_of_pin(&self, part: PartId, pin: u32) -> BoundNet<'a>; // pin = stored pin index (PD5)
}
pub struct BoundNet<'a> {
    pub key: &'a CircuitKey,          // CK1; never a NetKey or a NodeId
    pub net: NetId,                   // valid at `revision` only
    pub pins: &'a [(PartId, u32)],    // members, CD18 pin order
    pub labels: &'a [LabelId],
    pub ground: &'a [GroundEvidence], // empty unless declared (CK4)
}
pub enum GroundEvidence { GroundSymbol(PartId), ZeroLabel(LabelId) }

pub struct NetRef { /* row + revision; resolves key, name, kind, members */ }
pub enum Format { Lines, Json }
pub struct Rendered { pub views: u8, pub elided: bool, pub bytes: usize }

// ---- omnia-doc (C1.9) ----
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DocLineage(u64);           // fresh per Doc value: new, Default, load, fork (CD18)
impl Doc { pub fn lineage(&self) -> DocLineage; }

// ---- omnia-doc (C1.1) ----
pub struct Change { /* existing fields */
    pub modified: Vec<PartId>,          // parts P2.2: parts whose def or params changed (PD8)
    pub edited_parts: Vec<PartId>,      // C1.1: designator, value, position, orientation
    pub modified_wires: Vec<WireId>,    // C1.1
    pub modified_labels: Vec<LabelId>,  // C1.1
    pub net_delta: NetDelta }           // C1.1
pub struct NetDelta { /* retired, created, mapping, moved, labels: types fixed by C1.1 (CD2) */ }
```

## 13. Dependencies and risks

No new dependency. `rustc-hash`, `smallvec`, `serde_json`, `hdrhistogram`, `criterion` and
`proptest` are already in `Cargo.lock` and canvas-plan.md section 10; C0.2 extends their use
column to `omnia-circuit`. A crate not listed there needs its licence logged first.

| Risk | Mitigation | Early test |
| --- | --- | --- |
| C1.1 changes `command.rs`, `undo.rs` and `connectivity.rs`, which wiring, parts and simulation tasks own in turn | C1.1 waits for parts P2.1–P2.3 and sim S1.1, then until no open task owns them (XD7, CQ10) | C1.1 |
| Simulation lowering waits on this engine | C1.9 is small, read-only and depends only on C1.2 and S1.1, not on the net delta or the views | C1.9 |
| Recording `net_delta` inside `apply` eats the worst case's 0.08 ms headroom | Record facts connectivity already computes; measure paired against the command-boundary cases; if the boundary fails, the task is `blocked` with numbers, never a looser threshold | C1.1 |
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
| CQ2 | Ask the parts plan to extend PD5's roles (separate power input and power output, open collector and emitter, tri-state, free, unspecified)? Without a power-source role, `power.undriven` cannot be checked. | Build to PD5's seven roles plus engine-only `unspecified`; `power.undriven` stays off; the owner raises it against the parts plan. XD5 (2026-11-23) keeps this default; symbol class (sim S1.1) is a separate type and does not answer it. |
| CQ3 | For `omnia_context`, put the compact lines in both result channels instead of a JSON text copy (CD10)? | Yes, for successful lines responses from this tool only. |
| CQ4 | Retire or alias `omnia_inspect`, `omnia_query` and `omnia_nets` once `omnia_context` exists? | Keep them as built; decide after the HC3 run. |
| CQ5 | Line syntax: `1:VCC` or `1->VCC`; designators or ids as handles? | ASCII `:`; designators and net names as handles, ids accepted in arguments. |
| CQ6 | Byte budgets 12 KB default, 32 KB ceiling? | Yes; re-set from C2.4's token measurement. |
| CQ7 | Add the spanning-forest bit and batched search to connectivity (11.1)? | Only if C1.1 or C1.8 shows split tails, on any net with many wires, including wire-only nets that CK7's pin count does not call hubs. |
| CQ8 | Hub threshold: more than 32 pins, or kind `power` or `ground`? | Yes. |
| CQ9 | Does stage 3 lowering take pin-to-net bindings and the ground reference from this engine, rather than a second derivation? | **Answered 2026-11-23 by XD4** (coordinator, owner-delegated): yes. The stage 3 lowering is `omnia-sim`'s (`CircuitIr`, node ordering, device expansion, source maps, diagnostics mapping, SPICE text; sim S1.2) and consumes C1.9's revision-matched bindings and declared ground evidence. There is no parts-plan netlister: the parts plan supplies definitions (roles, terminal order, parameters) only. Ground evidence follows sim SQ3's default (CK4); CD13's heuristics never reach the bindings. |
| CQ10 | May C1.1 change `Change`, `undo.rs` and connectivity while W2.2 is `needs-human` (its measurement only) and W3.4 to W3.6 are open? | The order is fixed by XD7 (2026-11-23): parts P2.1–P2.3, then sim S1.1, then C1.1. C1.1 starts only after those are `done` and when no other open task owns `doc.rs`, `command.rs`, `undo.rs`, `io.rs` or `connectivity.rs`; the owner confirms at HC1 which wiring tasks still hold them. |
| CQ11 | Use a graph library for the matcher? | No: a hand-written matcher in the crate, so determinism and step caps stay under our control. |

## 15. Changes to other files

Applied by C0.1 (registration only):

| Where | Change |
| --- | --- |
| AGENTS.md, sources of truth | `context-plan.md` beside the plans, `context-roadmap.md` beside the roadmaps; `C`-prefixed ids live in the circuit-context files; `HC*` in hard rule 4 |
| ROADMAP.md | Links the circuit-context plan and roadmap |
| canvas-roadmap.md, Lanes | Row `circuit` |

Applied by C0.2 after HC1, for the decisions HC1 keeps:

| Where | Change |
| --- | --- |
| mcp-plan.md 9.2, 9.3, 9.4 | The `context` operation; the optional `circuit` request and result fields on `apply` and `preview` (CD14); the error codes of CK12 |
| mcp-plan.md 10.1, 10.2 | `omnia_context` and its result channels (CD10); `omnia://doc/{doc_id}/context`, the summary view as a cacheable resource (built by C2.3) |
| mcp-plan.md 13 | The safe loop orients with `omnia_context` and verifies with the `circuit` delta |
| canvas-plan.md section 10 | Use column: `rustc-hash`, `smallvec`, `serde_json`, `hdrhistogram` in `omnia-circuit` |
| canvas-plan.md section 9 (`Change`) and D2 | `net_delta`, `edited_parts`, `modified_wires` and `modified_labels` beside P2.2's `modified` (CD2), once C1.1 fixes their shape; any amendment to `nets_changed`'s meaning |
| parts-plan.md (once merged) | Section 13: CQ2 raised as a parts question if the owner wants it. (Its "definitions and bindings" wording and XD4 are already in PR #85.) |

Recorded for the other plans' own tasks (XD1–XD9 say each plan records its own amendments;
this plan edits neither file):

| Where | Change | Made by |
| --- | --- | --- |
| sim-plan.md, sim-roadmap.md | S1.2 depends on C1.9 and lowers from `Circuit::bindings`; S1.2 adds the check that its solver `NodeId` has no conversion to or from `CircuitKey` or `NetKey` (CK1, XD4), since `NodeId` does not exist until S1.2; a bindings read records `Bindings::lineage()` and `revision()` with the `CircuitIr` so stale results are found by state, not by revision alone (CD18); SK10 and SQ3 ground evidence matches CK4; ERC002 cites section 9.4's case folding and blank designators; the ERC crosswalk of section 9.4 is cited from SD8 | Sim task S0.2 |

## 16. Sources

The research behind this plan was gathered on 2026-11-23. Each claim above cites one or more
of these. Numbers quoted from a source are that source's, on its own hardware or models; none
is an Omnia measurement. A source marked *(user report)* is not documentation and is treated
as a hint to check, not a fact.

### 16.1 Papers and standards

| Id | Source |
| --- | --- |
| Ref1 | Ma et al., "NetlistBench", arXiv 2608.12197, August 2026. <https://arxiv.org/pdf/2608.12197> |
| Ref2 | Fatemi, Halcrow, Perozzi, "Talk like a Graph: Encoding Graphs for Large Language Models", ICLR 2024, arXiv 2310.04560. <https://arxiv.org/pdf/2310.04560> |
| Ref3 | Feng et al., "Beyond Graphs: Can Large Language Models Comprehend Hypergraphs?", ICLR 2025, arXiv 2410.10083. <https://arxiv.org/html/2410.10083v1> |
| Ref4 | Modarressi et al., "NoLiMa: Long-Context Evaluation Beyond Literal Matching", ICML 2025, arXiv 2502.05167. <https://arxiv.org/html/2502.05167v3> |
| Ref5 | Du et al., "Context Length Alone Hurts LLM Performance Despite Perfect Retrieval", Findings of EMNLP 2025, arXiv 2510.05381. <https://arxiv.org/abs/2510.05381v1> |
| Ref6 | Liu et al., "Lost in the Middle: How Language Models Use Long Contexts", TACL 2024, arXiv 2307.03172. <https://arxiv.org/pdf/2307.03172> |
| Ref7 | Kutschka and Geiger, "Notation Matters", arXiv 2605.29676, June 2026. <https://arxiv.org/pdf/2605.29676> |
| Ref8 | Chen, Böhlen, Helmer, "An experimental comparison of tree-data structures for connectivity queries on fully-dynamic undirected graphs", arXiv 2501.02278, January 2025. <https://arxiv.org/abs/2501.02278> |
| Ref9 | Even and Shiloach, "An On-Line Edge-Deletion Problem", Journal of the ACM 28(1), 1981. Read through secondary excerpts only. |
| Ref10 | De Man, Dhulipala, Karczmarz, Łącki, Shun, Wang, "Towards Scalable and Practical Batch-Dynamic Connectivity", VLDB 2025, arXiv 2411.11781. <https://arxiv.org/abs/2411.11781> |
| Ref11 | Budiu et al., "DBSP: Automatic Incremental View Maintenance for Rich Query Languages", PVLDB 16, 2023. <https://www.vldb.org/pvldb/vol16/p1601-budiu.pdf> |
| Ref12 | Kunal et al., ALIGN open analog layout flow, arXiv 2008.10682. <https://arxiv.org/abs/2008.10682> |
| Ref13 | Politecnico di Torino master's thesis on analog sub-circuit annotation (pattern matching precision 0.9641, recall 0.9860). <https://webthesis.biblio.polito.it/28628/1/tesi.pdf> |
| Ref14 | Li et al., "The Application of Graph Attention Mechanism in the Automation of Analog Circuit Design", ACCV 2024 Workshops. <https://openaccess.thecvf.com/content/ACCV2024W/MLCSA2024/html/Li_The_Application_of_Graph_Attention_Mechanism_in_the_Automation_of_ACCVW_2024_paper.html> |
| Ref15 | Solnon, "Experimental Evaluation of Subgraph Isomorphism Solvers", GbR 2019. <https://iapr.org/archives/gbr2019/data/pages/gbrsolnon.pdf> |
| Ref16 | Shervashidze et al., "Weisfeiler-Lehman Graph Kernels", JMLR 12, 2011. <https://jmlr.org/papers/v12/shervashidze11a.html> |
| Ref17 | Model Context Protocol specification, revision 2025-06-18, Tools. <https://modelcontextprotocol.io/specification/2025-06-18/server/tools> |
| Ref18 | Model Context Protocol specification, revision 2025-11-25, Tools. <https://modelcontextprotocol.io/specification/2025-11-25/server/tools> |
| Ref19 | Model Context Protocol specification, revision 2026-07-28, Tools; Pagination; Caching. <https://modelcontextprotocol.io/specification/2026-07-28/server/tools>, <https://modelcontextprotocol.io/specification/2026-07-28/server/utilities/pagination>, <https://modelcontextprotocol.io/specification/2026-07-28/server/utilities/caching> |
| Ref20 | Omnia's own measurements: `docs/measurements/T2.8.md` and `docs/measurements/command-boundary/reference-2026-11-17/README.md`. |
| Ref21 | SPICE simulator manual (the open engine named in ROADMAP.md): ground node `0`, value suffixes. <https://ngspice.sourceforge.io/docs/ngspice-html-manual/manual.xhtml> |

### 16.2 Software documentation, source code and reports

The source labels describe the cited features. Ref25–Ref26 and Ref29–Ref33 retain
product-identifying destinations for traceability. The owner decides at HC1 whether those
links may remain under AGENTS.md hard rule 1.

| Id | Source | Supports |
| --- | --- | --- |
| Ref22 | Query-based incremental computation library book, "The red-green algorithm". <https://salsa-rs.github.io/salsa/reference/algorithm.html> | CD1: revision stamps, backdating, durability |
| Ref23 | Rust compiler development guide, incremental compilation. <https://rustc-dev-guide.rust-lang.org/queries/incremental-compilation.html> | CD1: early cutoff by result fingerprints |
| Ref24 | Differential dataflow README. <https://github.com/TimelyDataflow/differential-dataflow> | CD1: changes as signed records |
| Ref25 | Schematic label precedence. [Source](https://docs.kicad.org/9.0/en/eeschema/eeschema.html), [Source](https://gitlab.com/kicad/code/kicad/-/raw/master/eeschema/connection_graph.cpp) | CK2: label priority and ties |
| Ref26 | Ground mapping during export. [Source](https://gitlab.com/kicad/code/kicad/-/raw/master/eeschema/netlist_exporters/netlist_exporter_spice.cpp) | CD13: only some names map to node `0` |
| Ref27 | Open-source graph library, Weisfeiler-Lehman hash source and notes. <https://networkx.org/documentation/stable/_modules/networkx/algorithms/graph_hashing.html> | CD12: hash values changed between versions |
| Ref28 | Open-source network analysis library, Leiden clustering reference. <https://search.r-project.org/CRAN/refmans/igraph/html/cluster_leiden.html> | CD12: reproducible only with a fixed seed |
| Ref29 | Consolidated tool design. [Source](https://www.anthropic.com/engineering/writing-tools-for-agents) | CD8, CD9: consolidated tools, concise responses, meaningful names |
| Ref30 | Tool input examples. [Source](https://www.anthropic.com/engineering/advanced-tool-use) | CD9: input examples raised complex-parameter accuracy from 72% to 90% |
| Ref31 | Tool-output size guidance. [Source](https://code.claude.com/docs/en/mcp) | CB11 |
| Ref32 | Text-channel delivery report. [Source](https://claudeissues.com/issue/79944-mcp-tool-response-text-content-block-dropped-when-structuredcontent-is-also-pres) | CD10 |
| Ref33 | Structured-channel delivery report. [Source](https://forum.cursor.com/t/mcp-tool-results-containing-only-structuredcontent-are-silently-dropped/167346) | CD10 |
