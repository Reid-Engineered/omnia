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
  the starting workload is back;
- the effectiveness assertion (`Change.dirty` must be `Some`, so a validated
  no-op can never pose as an effective edit) and all CSV output, which is
  buffered and printed only after the loop.

Timer overhead (`Instant::now`/`elapsed`, `black_box`) is inherent to every
observation, as in the existing T2.8 runner. Repeated `AddWires` observations
allocate a fresh wire ID each time (IDs are never reused, D2); undo removes
the wire and retains the reserved slot, so arena capacity grows amortized
while live content returns to the starting workload each iteration.

## Frozen cases

The case list is frozen in [the runner](command-boundary/runner/main.rs) and
mirrored in [run.sh](command-boundary/run.sh); `summarize.py` asserts exactly
this set. Do not edit it after seeing results.

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
| `doc_10k_wire_add` | `AddWires` with the `real_10k_leaf` leaf geometry, on actual `10k` (10,000 parts, 20,000 wires) |
| `doc_10k_wire_delete` | `Delete` of `WireId(0)`, a leaf from part 0's first pin, on actual `10k` |
| `doc_10k_move_100` | `Move` of the first 100 parts (`PartId(0)..PartId(100)`) by `(GRID_NM, GRID_NM)` on actual `10k`; the T5.1 done-when case |
| `doc_chain_20k_add` | `AddWires` with the missing middle segment, on the 20k chain with that segment absent |
| `doc_chain_20k_delete` | `Delete` of the middle segment (`WireId(10000)`), splitting the 20k chain; the T2.8 split worst case |
| `doc_label_bridge_add` | `AddWires` with the bridge wire, shorting the GND/VCC nets over 5,000 components |
| `doc_label_bridge_delete` | `Delete` of the bridge wire, forcing the logical re-flood |

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

1. Freeze and record provenance: check out the campaign commit, then
   `cargo build --manifest-path docs/measurements/command-boundary/runner/Cargo.toml --release --offline -j 2`
   and `cargo build --release -p omnia-bench` (its binary is hashed by
   `run.sh`). Record source and binary SHA-256 as `run.sh` does.
2. Prepare conditions per section 2.1: on AC, other user apps closed, one
   benchmark process at a time. Record power source, CPU governors/profile,
   load and process inventory; `run.sh` snapshots them before and after every
   invocation (`*.before.txt`/`*.after.txt`, `machine.txt`, `order.txt`).
3. Run the fixed schedule: `bash docs/measurements/command-boundary/run.sh`.
   It runs all eleven cases in each of ten rotating passes into
   `docs/measurements/command-boundary/raw/`.
4. Archive the raw CSVs (`raw.tar.gz`) next to `summarize.py`, then run
   `python docs/measurements/command-boundary/summarize.py`. Keep the raw
   CSVs, `order.txt`, condition snapshots, hashes, `run.sh` and the summary
   output as the evidence bundle.
5. Run the functional checks separately: `cargo test --workspace`
   (`load(save(doc)) == doc`, the INV10 property and consistency checks, and
   the alloc-free `Doc::query` check).

## Windows smoke run (non-reference, not evidence)

A single-pass smoke run on the Windows development machine only shows the
runner works. It is **not** reference evidence and certifies nothing: one
pass instead of ten, an uncontrolled Windows machine, no section 2.1
conditions.

- Machine: ASUS TUF Gaming F16 (`FX608JMR_FX608JMR`), Intel Core
  i7-14650HX, Windows, `rustc 1.98.1 (48a229cea 2026-09-01)`, release build.
- Source at smoke time: `0af9074d44cacadb3baf52aa9a64889dbd386860` plus this
  change; runner binary SHA-256
  `10454D04311B764A5FF2294B57EEC6E2C803B1C95FAEBF0F2BA0A84108A3CAD0`.
- Command per case:
  `docs/measurements/command-boundary/runner/target/release/command-boundary.exe <case>`
  (build: `cargo build --manifest-path docs/measurements/command-boundary/runner/Cargo.toml --release --offline -j 2`).
  All eleven cases exited 0 with 1,000 observations each. Values below are
  one run's p50/p99/max in ms, recomputed from the raw CSVs; the ten-run
  median statistic does not apply.

| Case/operation | p50 | p99 | max | >= 1 ms |
| --- | ---: | ---: | ---: | ---: |
| `real_10k_leaf/add` | 0.000100 | 0.000100 | 0.000700 | 0 |
| `real_10k_leaf/remove` | 0.000100 | 0.000100 | 0.003600 | 0 |
| `label_5k_gnd_vcc_bridge/add` | 0.005400 | 0.020800 | 0.100800 | 0 |
| `label_5k_gnd_vcc_bridge/remove` | 0.231500 | 0.597500 | 1.647000 | 3 |
| `chain_20k_middle/add` | 0.052400 | 0.132200 | 0.699800 | 0 |
| `chain_20k_middle/remove` | 0.421700 | 0.792200 | 1.002800 | 1 |
| `index_10k/query` | 0.005900 | 0.026500 | 0.080200 | 0 |
| `doc_10k_wire_add/apply` | 0.000200 | 0.000500 | 0.007300 | 0 |
| `doc_10k_wire_delete/apply` | 0.000300 | 0.000400 | 0.012300 | 0 |
| `doc_10k_move_100/apply` | 0.071700 | 0.143200 | 0.442300 | 0 |
| `doc_chain_20k_add/apply` | 0.033500 | 0.093400 | 0.187800 | 0 |
| `doc_chain_20k_delete/apply` | 0.329100 | 0.496800 | 0.974600 | 0 |
| `doc_label_bridge_add/apply` | 0.005300 | 0.015800 | 0.055400 | 0 |
| `doc_label_bridge_delete/apply` | 0.199900 | 0.370300 | 0.873200 | 0 |

`summarize.py` was additionally exercised offline against a synthetic
ten-copy archive of these CSVs to verify its frozen-set, ten-run and
median/range/diagnostic reporting; that synthetic check tested the script
only and produced no measurement claims.
