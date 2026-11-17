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
- the effectiveness assertion (`Change.nets_changed` must be true) and all CSV output, which is
  buffered and printed only after the loop.

The repeated loop has a warm-data bias: after the first observation, each
apply immediately follows undo of the same data. The cold first sample can
be much slower; it is retained among the 1,000 samples, with no warmup
samples discarded. These results describe repeated edits on warm data, not
cold command latency. `push_undo` clears the redo entry left by undo inside
the timed span, so that cleanup cost is included after the first sample.

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
| `doc_10k_wire_add` | `AddWires` extending `WireId(0)` from its far end by 1 nm collinearly, without overlap (INV2), on actual `10k` (10,000 parts, 20,000 wires) |
| `doc_10k_wire_delete` | `Delete` of `WireId(0)`, a leaf from part 0's first pin, on actual `10k` |
| `doc_10k_move_100` | `Move` of the first 100 parts (`PartId(0)..PartId(100)`) by `(GRID_NM, GRID_NM)` on actual `10k`; the T5.1 done-when case |
| `doc_10k_move_100_connected` | Pin-count worst-case `Move` on actual `10k`: select the 100 parts with most pins, ties by lowest ID, and move by `(GRID_NM, GRID_NM)`; every selected part has a pin at a wire end that it leaves |
| `doc_chain_20k_add` | `AddWires` with the missing middle segment, on the 20k chain with that segment absent |
| `doc_chain_20k_delete` | `Delete` of the middle segment (`WireId(10000)`), splitting the 20k chain; the T2.8 split worst case |
| `doc_label_bridge_add` | `AddWires` from free end `(1,0)` to free end `(10,0)` (INV2), shorting the GND/VCC nets over 5,000 components |
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

1. Freeze source and case list before the first reference observation. Build
   only the standalone runner:
   `cargo build --manifest-path docs/measurements/command-boundary/runner/Cargo.toml --release --offline -j 2`.
   At freeze, `run.sh` also runs
   `cargo check --manifest-path docs/measurements/command-boundary/runner/Cargo.toml --offline`
   to verify current `omnia-doc` APIs. No `omnia-bench` binary is used or required.
2. Prepare section 2.1 conditions: AC, performance governors, other user apps
   closed. The script checks tools (including `sha256sum`, `python3`, and
   `vulkaninfo`), built runner, AC and governors before creating any directory;
   refusal explains what is missing. It records power/profile, governors,
   load and process inventory before and after each invocation.
3. Run one command: `bash docs/measurements/command-boundary/run.sh`.
   It runs all twelve cases in ten rotating passes, archives the raw data,
   and invokes `summarize.py`, retaining `raw/`, `raw.tar.gz`, `summary.txt`,
   conditions, hashes, order and a status file in a unique `attempt-*` directory.
4. An aborted attempt remains labelled incomplete. A later invocation uses
   a new directory and cannot overwrite it; retain and report every attempt,
   with the reason for any new campaign. This is not authorization to retry
   until green. Summarize a retained complete archive with
   `python3 docs/measurements/command-boundary/summarize.py <attempt>/raw.tar.gz`.
5. Run the functional checks separately: `cargo test --workspace`
   (`load(save(doc)) == doc`, the INV10 property and consistency checks, and
   the alloc-free `Doc::query` check).

## Windows smoke run (non-reference, not evidence)

A single-pass smoke run on the Windows development machine only shows the
runner works. It is **not** reference evidence and certifies nothing: one
pass instead of ten, an uncontrolled Windows machine, no section 2.1
conditions.

- Machine: development laptop, Core i7-14650HX, integrated UHD display
  adapter and discrete RTX 5060 Laptop GPU; Windows 11 Home version
  10.0.26200 (build 26200), 1920x1200, 60 Hz, 125% scaling.
- Power: battery (`PowerOnline=false`), Silent power profile; CPU governor
  unavailable on Windows. Other apps and concurrent workspace builds were
  uncontrolled. Environment inventory was taken immediately after the run.
- Release build, `rustc 1.98.1 (48a229cea 2026-09-01)`; updated runner source
  and binary hashes, base commit and raw samples are retained in
  [smoke-windows.tar.gz](command-boundary/smoke-windows.tar.gz).
- One fixed pass, twelve cases, 1,000 samples each; all exited successfully,
  including connectivity-effectiveness and full INV4 restoration assertions.
  This is a functional smoke only, non-reference and not gate evidence.

All times below are milliseconds; `>= 1 ms` is a diagnostic count.

| Case/operation | p50 | p99 | max | >= 1 ms |
| --- | ---: | ---: | ---: | ---: |
| `chain_20k_middle/add` | 0.138100 | 0.624800 | 16.465600 | 6 |
| `chain_20k_middle/remove` | 0.989700 | 7.203700 | 36.059000 | 468 |
| `doc_10k_move_100/apply` | 0.237000 | 0.573600 | 2.336500 | 2 |
| `doc_10k_move_100_connected/apply` | 0.445300 | 0.820100 | 3.194000 | 5 |
| `doc_10k_wire_add/apply` | 0.000900 | 0.001400 | 0.020300 | 0 |
| `doc_10k_wire_delete/apply` | 0.000900 | 0.001100 | 0.032600 | 0 |
| `doc_chain_20k_add/apply` | 0.101800 | 0.220200 | 0.661100 | 0 |
| `doc_chain_20k_delete/apply` | 0.870200 | 6.475900 | 59.510300 | 98 |
| `doc_label_bridge_add/apply` | 0.015200 | 0.057200 | 16.970300 | 2 |
| `doc_label_bridge_delete/apply` | 0.516300 | 12.304400 | 28.116800 | 23 |
| `index_10k/query` | 0.019100 | 0.032200 | 0.260900 | 0 |
| `label_5k_gnd_vcc_bridge/add` | 0.017300 | 0.053500 | 0.216200 | 0 |
| `label_5k_gnd_vcc_bridge/remove` | 0.715200 | 17.603600 | 33.523200 | 76 |
| `real_10k_leaf/add` | 0.000200 | 0.000300 | 0.003200 | 0 |
| `real_10k_leaf/remove` | 0.000200 | 0.000200 | 0.010100 | 0 |

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
