# Command-boundary timing cases for T2.8/T5.1

Preparation only. This change adds the public command-boundary cases that the
[gate-blocker audit](gate-blocker-audit.md) lists as next task (1) (audit
sections 3.4, 4.2 and 5). It marks no gate or budget as met and changes no
roadmap task status: T2.8 and T5.1 keep their current status until a complete
campaign runs on the reference machine under canvas-plan section 2.1 and a
separate gate decision records it.

## What is timed

The new cases time exactly one public `Doc::apply` call per observation on
`crates/omnia-doc` (D8: incremental connectivity inline with the command,
target under 1 ms per command at 10,000 parts; INV4/INV10: the document
changes only through `apply`/`undo`/`redo`, with connectivity equal to a
from-scratch rebuild). Only public APIs are used (`Doc::apply`, `Doc::undo`,
`Doc::from_items`, `Command`, and the read-only accessors). The crate-private
`Doc::insert_wire`/`remove_wire` helpers are never called and their visibility
is unchanged; no public API was missing, so none was added.

Inside the measured span (one `Instant` pair around `Doc::apply`):

- command validation: K1 bounds, INV1 axis/distinct checks, item
  existence/uniqueness;
- arena insert/remove or in-place move, `SpatialIndex` maintenance and
  point-incidence maintenance;
- incremental `Connectivity` maintenance, including the split search on delete
  and the logical re-flood on bridge delete;
- `Change`/dirty-rectangle computation, inverse construction, undo-history
  push (including redo clear) and the monotonic revision bump.

Outside the measured span:

- workload construction (`generate(StressSheet::Parts10k, 42)` or
  `Doc::from_items`) and any setup command (for example adding the bridge
  before `doc_label_bridge_delete`, whose issued ID is read back from the
  public `Change`);
- all `Command`, `Vec<Wire>` and `Vec<ItemId>` construction and cloning;
- the `Doc::undo()` that restores the starting workload after each timed
  `apply` (undo restores exact IDs, INV4), plus the post-loop assertions that
  `doc.content_eq(&reference)` with an independently built reference, including
  exact live IDs (INV4);
- public `net_at`/`net_of` topology assertions before apply, after apply and
  after undo: bridge/chain probe nets join on add and separate on delete;
  the leaf endpoint appears/disappears; each moved part's first pin leaves
  its outgoing wire's net. `nets_changed` is not used as proof;
- for every `doc_*` case, the exact change-set guard described under
  "Frozen cases" (`Insert`, `Delete`, `Merge` or `Move`); all CSV output
  is buffered until after the loop.

The repeated loop has a warm-data bias: after the first observation, each
apply immediately follows undo of the same data. The cold first sample can
be much slower; it is retained among the 1,000 samples, with no warmup
samples discarded. These results describe repeated edits on warm data, not
cold command latency. Untimed topology probes also touch connectivity data
before and after each command. `push_undo` clears the redo entry left by undo inside
the timed span, so that cleanup cost is included after the first sample.

Timer overhead (`Instant::now`/`elapsed`, `black_box`) is inherent to every
observation, as in the existing T2.8 runner. Repeated plain-insert `AddWires`
observations allocate fresh wire IDs each time (IDs are never reused, D2);
undo removes the wires and retains the reserved slots, so arena capacity grows amortized
while live content returns to the starting workload each iteration.

## Frozen cases

The case list is frozen in [the runner](command-boundary/runner/main.rs) and
mirrored in [run.sh](command-boundary/run.sh); `summarize.py` asserts exactly
this set. Case names, `run.sh` and `summarize.py` are unchanged by the
re-freeze below.

**Re-freeze after T6.1 (canvas T6.1, PR #43).** T6.1's local `AddWires`
planner (D2 split/merge/id rules, INV2) changed what two cases time, so the
cases were re-frozen once, before any timed run (none has happened). The freeze
is the runner at commit `a7f6ec9` on top of main `f04fcd6` (merge of PR #43);
later commits in this change are documentation and the smoke archive only. The
case list is frozen again from here: do not redefine a case after seeing
reference results. If a later change alters what any case does (see the
W1.4/W2.2 notes below), re-freeze before any timed run.

Every `doc_*` case now declares an exact expected change set
(`Expect` in the runner), asserted on every one of the 1,000 observations
against the public `Change` (added/removed IDs are unordered sets, compared
sorted; a merge survivor keeps its ID and is in neither list, so merges also
pin the survivor's exact geometry) plus the live-wire-count delta. A case that
starts doing different work fails loudly instead of being timed silently:

| Guard | Asserts |
| --- | --- |
| `Insert` (adds) | nothing removed; exactly `n` fresh wire IDs, consecutive from the first free ID on the first observation and never reused afterwards (D2); no split or merge |
| `Delete` (deletes) | removed is exactly the deleted wire IDs; nothing added; live wires drop by exactly that many, so no merge or split of any remaining wire |
| `Merge` (`doc_chain_20k_add`) | removed is exactly the absorbed IDs; nothing added; the surviving lowest ID holds exactly the merged geometry; live wires drop by exactly the absorbed count |
| `Move` | no wire added or removed and the live-wire count unchanged |

The `Move` guard is new in this re-freeze (the two move cases are otherwise
unchanged); it makes the Move normalisation planned for W2.2 fail loudly too.

Connectivity-only cases, copied unchanged from the T2.8 runner for comparison
(CSV `sample,add_ns,remove_ns`, except `index_10k` with
`sample,query_ns,count`):

| Case | Workload |
| --- | --- |
| `real_10k_leaf` | `Connectivity` rebuilt from T2.4 `10k` (seed 42); leaf wire at the first wire's start point |
| `label_5k_gnd_vcc_bridge` | 5,000 one-wire components labelled alternately GND/VCC; bridge wire |
| `chain_20k_middle` | 20,000-segment chain; middle segment |
| `index_10k` | `Doc::query` of a 4K-viewport rectangle on `10k` (no 1 ms target) |

New public `Doc::apply` cases (CSV `sample,apply_ns`, one operation per row,
restored by untimed `undo()`):

| Case | Command and starting workload |
| --- | --- |
| `doc_10k_wire_add` | `AddWires` extending `WireId(0)` from its far end by 1 nm perpendicularly, without overlap or crossed ends (INV2), on actual `10k` (10,000 parts, 20,000 wires). Definition unchanged; under T6.1 it is still a plain insert (see below) |
| `doc_10k_wire_delete` | `Delete` of `WireId(0)`, a leaf from part 0's first pin, on actual `10k` |
| `doc_10k_move_100` | `Move` of the first 100 parts (`PartId(0)..PartId(100)`) by `(GRID_NM, GRID_NM)` on actual `10k`; the T5.1 done-when case |
| `doc_10k_move_100_connected` | Pin-count worst-case `Move` on actual `10k`: select the 100 parts with most pins, ties by lowest ID, and move by `(GRID_NM, GRID_NM)`; every selected part has a pin at a wire end that it leaves |
| `doc_chain_20k_add` | **Re-defined as the T6.1 merge worst case:** `AddWires` with the missing middle segment on the 20k chain with that segment absent; the five collinear segments around the gap merge into one wire (see below) |
| `doc_chain_20k_delete` | `Delete` of the middle segment (`WireId(10000)`), splitting the 20k chain; the T2.8 split worst case |
| `doc_label_bridge_add` | `AddWires` with the non-collinear three-segment route `(1,0)-(1,5)-(10,5)-(10,0)` (INV2), shorting the GND/VCC nets over 5,000 components |
| `doc_label_bridge_delete` | `Delete` of the middle segment of that route (starting with 5,003 wires), forcing the logical re-flood |

The perpendicular 1 nm leaf remains off-lattice but differs in attachment
point and direction from `real_10k_leaf`; the routed bridge adds three wires
rather than the comparison's one. These are component-vs-command comparisons,
not identical geometry or operation counts. The new geometry is never
collinear with an existing wire and no route crosses an existing end or pin.

### What changed under T6.1, case by case

Found by building the pre-freeze runner against main `f04fcd6` and running one
pass of all twelve cases: eleven ran clean, and `doc_chain_20k_add` failed its
old "no removal" guard.

- `doc_10k_wire_add`: **kept, definition unchanged.** The earlier concern was
  that the generated `10k` sheet is not in T6.1 normal form (46 duplicate wire
  pairs, from 23 pairs of parts whose first pins coincide, for example wire
  IDs 386/588 and 387/589) and that T6.1 might renormalise it. The final local
  planner does not: it only gathers segments touching the added segment. The
  added 1 nm stub touches only `WireId(0)`'s far end, where wire 0 is the sole
  wire end, is not in a duplicate pair and has no collinear neighbour (the stub
  is perpendicular to it). The case passes its guards on every observation:
  nothing removed, one fresh wire (`WireId(20000)` first), no duplicate
  touched. What is timed now is the plain insert plus the planner's local query
  and plan for one added segment, on top of the earlier
  validation/index/connectivity work. The generator's non-normal geometry stays
  untouched by this command (T2.4 follow-up: duplicates remain in `10k`; a
  generator change needs new baselines and a further re-freeze).
- `doc_chain_20k_add`: **re-defined as a merge worst case, not replaced.** Under
  T6.1 the missing middle segment merges the chain, which is what users will hit
  (T2.8/D8 care about worst cases), so the case keeps its command and workload
  and only its expected result changes. D2 fixes the touched points (the added
  ends and the direct contacts' far ends) and lets each side extend by one
  collinear neighbour; it does not walk the remaining chain. In `chain_doc` the
  segment `x = i..i+1` has ID `i` for `i <= 9999` and `i - 1` for `i >= 10001`,
  so the add merges 9998..9999 (ID 9998), 9999..10000 (ID 9999), the new
  segment, 10001..10002 (ID 10000) and 10002..10003 (ID 10001) into one wire
  `(9998,0)-(10003,0)`. The lowest ID, `WireId(9998)`, survives; `WireId(9999)`,
  `WireId(10000)` and `WireId(10001)` are absorbed (`Change::removed` exactly
  these, `Change::added` empty), and live wires go 19,999 -> 19,996. The guard
  is this exact result, not a looser check, asserted on every observation; undo
  restores the absorbed wires with their exact IDs (INV4, asserted after the
  loop). The timed span now contains the T6.1 split/merge plan, three wire
  removals, the survivor's re-insertion, index and connectivity updates for
  those edits and the `ReplaceWires` inverse. The topology probe changed with
  it: the joined points become wire interiors, so it checks that the chain's
  outer ends `(0,0)` and `(20000,0)` are in different nets before, the same
  net after, and different again after undo.
- `doc_label_bridge_add`: unchanged definition; the non-collinear route has no
  split, overlap or merge, and the guard now also asserts its three fresh IDs
  (5,000..5,002 on the first observation).
- All other cases are unchanged, including the four Connectivity-only cases
  (they do not call `AddWires`); the two Move cases gain only the new `Move`
  guard. `doc_label_bridge_delete` now finds the bridge's middle wire by its
  geometry instead of by position in the unordered `Change::added` set.

### Delete cases and W1.4 (merge on delete)

Wiring task W1.4 is expected to make `Delete` normalise locally: after deleting
wires, it re-merges collinear segments at the points where deleted wires ended,
with T6.1's merge and id rules (deleting one branch of a T merges the two
remaining halves). It is not merged here. A merge needs exactly two collinear
ends meeting at a point that holds no pin or label after the delete. None of
the frozen delete cases can trigger one:

- `doc_10k_wire_delete` (`Delete` `WireId(0)`): the wire's start is part 0's
  first pin, which also carries `WireId(1)` perpendicular to it (+y vs +x) plus
  the pin, so the one remaining wire end there is perpendicular and the pin
  blocks any merge; its far end has no other wire end. No merge possible.
- `doc_chain_20k_delete` (`Delete` `WireId(10000)`): each end of the deleted
  segment leaves exactly one end behind (the neighbouring chain segment), so
  there is nothing to merge; separating the halves is the case.
- `doc_label_bridge_delete` (`Delete` the middle segment `(1,5)-(10,5)`): each
  end of the deleted segment leaves exactly one end behind (a vertical leg),
  alone at that point and perpendicular to the removed segment; the labels sit
  at `(i*10, 0)`, not at the deleted ends. No merge possible.

Because the analysis could still miss a W1.4 behaviour, each delete is guarded
by `Expect::Delete`: if a merge on delete ever absorbs another wire, the
absorbed ID appears in `Change::removed` (or a replacement in `Change::added`)
and the live-wire count no longer drops by exactly the deleted count, so the
runner panics instead of timing a different operation. If W1.4 changes any
case's meaning, re-freeze the case list before any timed run; the guard
failing is the signal.

## Acceptance statistic (unchanged)

Each case runs ten interleaved passes with rotating order (pass order rotates
by case index), 1,000 observations per operation per pass, and the existing
30 s timeout with TERM then KILL after 5 s per invocation. The statistic is
the median of the ten run p99s (nearest-rank p99, sample 990 of 1,000),
reported with its range by [summarize.py](command-boundary/summarize.py),
against the unchanged strict under-1 ms target. The `>= 1 ms` counts are
diagnostics only; there is no new all-samples gate. `index_10k/query` carries
no 1 ms target and is reported without a verdict. Complete all ten passes
before computing the median; an interim p99 is not a verdict. A timeout,
crash or invalid sample aborts the campaign and is reported as incomplete,
never retried until green and never with cases dropped or replaced.

## Full campaign on the reference machine (Orion, Linux)

Only the reference machine (the Orion laptop on Linux) under section 2.1 can
certify the target; numbers from any other machine, or from Orion outside
that protocol, gate nothing. The CPU-only scope question in audit section 3.2
is still open: this preparation adopts no protocol exception, so record the
conditions and let the gate decision weigh them.

One-time setup on Orion, while online:
`cargo fetch --manifest-path docs/measurements/command-boundary/runner/Cargo.toml`.
This populates the dependency cache required by the offline check/build below.

1. Freeze source and case list before the first reference observation. Build
   only the standalone runner:
   `cargo build --manifest-path docs/measurements/command-boundary/runner/Cargo.toml --release --offline -j 2`.
   At freeze, `run.sh` also runs
   `cargo check --manifest-path docs/measurements/command-boundary/runner/Cargo.toml --offline`
   to verify current `omnia-doc` APIs. No `omnia-bench` binary is used or required.
2. Prepare section 2.1 conditions: AC, performance governors, other user apps
   closed. The script checks tools (including `sha256sum`, `python3`, and
   `vulkaninfo`), built runner, AC and governors before creating any directory;
   refusal explains what is missing. A later AC/governor failure reports
   "Campaign incomplete; retained in ..." through the EXIT trap. It records power/profile, governors,
   load and process inventory before and after each invocation.
3. Run one command: `bash docs/measurements/command-boundary/run.sh`.
   It runs all twelve cases in ten rotating passes, archives the raw data,
   and invokes `summarize.py`, retaining `raw/`, `raw.tar.gz`, `summary.txt`,
   conditions, hashes, order and a status file in a unique `attempt-*` directory.
   Git-status snapshots exclude these generated attempt directories.
4. An aborted attempt remains labelled incomplete. A later invocation uses
   a new directory and cannot overwrite it; retain and report every attempt,
   with the reason for any new campaign. This is not authorization to retry
   until green. Summarize a retained complete archive with
   `python3 docs/measurements/command-boundary/summarize.py <attempt>/raw.tar.gz`.
5. Run the functional checks separately: `cargo test --workspace`
   (`load(save(doc)) == doc`, the INV10 property and consistency checks, and
   the alloc-free `Doc::query` check).

## Windows smoke run (non-reference, not evidence)

A single-pass smoke run of the re-frozen runner on the Windows development
machine only shows that it works. It is **not** reference evidence and
certifies nothing: one pass instead of ten, an uncontrolled Windows machine, no
section 2.1 conditions, no budget claimed.

- Machine: development laptop, Core i7-14650HX, integrated UHD display adapter
  and discrete RTX 5060 Laptop GPU; Windows 11 Home 10.0.26200.9974,
  1920x1200, 60 Hz, 125% scaling (display settings from the earlier smoke
  record; not changed or re-read for this run, which draws nothing).
- Power: **AC** (`PowerOnline=true`, `Charging=true` before and after the run),
  Silent power profile; CPU governor unavailable on Windows. Other apps were
  uncontrolled. The run took about six seconds.
- Release build, `rustc 1.98.1 (48a229cea 2026-09-01)`, built from runner
  commit `a7f6ec9` on main `f04fcd6`; source commit, SHA-256 hashes of the
  committed source blobs (`git show <commit>:<path>`), binary hash, order file,
  before/after power snapshots and raw samples are in
  [smoke-windows.tar.gz](command-boundary/smoke-windows.tar.gz), which replaces
  the pre-T6.1 smoke archive (still in git history).
- One fixed pass, twelve cases in the frozen order, 1,000 samples each; all
  exited successfully, so every case passed its exact change-set guard
  (including the `Merge` guard on `doc_chain_20k_add`), the public topology
  checks and the full INV4 restoration assertion.

All times below are milliseconds; `>= 1 ms` is a diagnostic count. One
`>= 1 ms` sample (max 1.56 ms) appears in `doc_chain_20k_add/apply`, one cold or
noisy observation in a single uncontrolled pass; it is neither a verdict nor a
finding.

| Case/operation | p50 | p99 | max | >= 1 ms |
| --- | ---: | ---: | ---: | ---: |
| `real_10k_leaf/add` | 0.000100 | 0.000100 | 0.001500 | 0 |
| `real_10k_leaf/remove` | 0.000000 | 0.000100 | 0.010700 | 0 |
| `label_5k_gnd_vcc_bridge/add` | 0.004000 | 0.014200 | 0.072500 | 0 |
| `label_5k_gnd_vcc_bridge/remove` | 0.159700 | 0.261900 | 0.553300 | 0 |
| `chain_20k_middle/add` | 0.021800 | 0.046100 | 0.102900 | 0 |
| `chain_20k_middle/remove` | 0.195400 | 0.265100 | 0.515200 | 0 |
| `index_10k/query` | 0.003400 | 0.003600 | 0.008200 | 0 |
| `doc_10k_wire_add/apply` | 0.002600 | 0.003000 | 0.047100 | 0 |
| `doc_10k_wire_delete/apply` | 0.000300 | 0.000400 | 0.014600 | 0 |
| `doc_10k_move_100/apply` | 0.055600 | 0.071100 | 0.166000 | 0 |
| `doc_10k_move_100_connected/apply` | 0.112300 | 0.152000 | 0.333400 | 0 |
| `doc_chain_20k_add/apply` | 0.169100 | 0.297000 | 1.560500 | 1 |
| `doc_chain_20k_delete/apply` | 0.203100 | 0.256100 | 0.375100 | 0 |
| `doc_label_bridge_add/apply` | 0.101300 | 0.180100 | 0.514900 | 0 |
| `doc_label_bridge_delete/apply` | 0.131800 | 0.205200 | 0.391000 | 0 |

Before/after for the cases that changed meaning, from single non-reference
smoke passes on the same laptop (before = the earlier pre-T6.1 smoke in the
previous version of this file; not comparable as evidence): p50/p99,
`doc_10k_wire_add` 0.0003/0.0005 -> 0.0026/0.0030 ms (plain insert plus the
planner's local query), `doc_chain_20k_add` 0.0428/0.0868 -> 0.1691/0.2970 ms
(now a five-segment merge instead of an insert), `doc_label_bridge_add`
0.0068/0.0227 -> 0.1013/0.1801 ms (same operation, now planned by T6.1).
`doc_label_bridge_add` did not change meaning but the planner roughly fifteens
its cost; this is a recorded observation, not a budget statement.

Validation of this change: runner `cargo fmt --check`, `cargo clippy
--all-targets -- -D warnings` and `bash -n run.sh` pass. `run.sh` and
`summarize.py` are unchanged (names and expected keys are the same). No
`crates/` code, roadmap status or budget changed, and no reference campaign
ran.

### Earlier validation (pre-T6.1 smoke, kept for the record)

Validation after review fixes: runner fmt, clippy (`-D warnings`), offline
check and release build passed. Workspace fmt and clippy passed. Shell
syntax and refusal before evidence creation were checked on the development
machine. Synthetic archives verified summary acceptance for exactly 01-10
and rejection of missing, duplicate and out-of-range passes; synthetic data
is script validation only.

Feature-enabled bench clippy and `cargo deny check` passed. Workspace tests
initially failed with paging-file exhaustion while mapping GPUI metadata;
`cargo test --workspace -j 1` then passed without changing any checks.
No reference campaign ran and no budget or roadmap status changed.

Re-review validation: all twelve smoke cases reran on AC. The phase-aware
refusal and EXIT trap were exercised with unavailable AC, and the evidence
pathspec was checked against a generated attempt marker. Workspace standard
checks and runner fmt/clippy passed again. The prior paging-file failure
above belongs to the earlier validation round; this round's single-job
workspace test run passed directly.
