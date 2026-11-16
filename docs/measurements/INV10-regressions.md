# INV10 connectivity regressions (PR27 follow-up)

Durable coverage added in the `connectivity.rs` test module only; no production
algorithm, API, dependency, benchmark, or gate changed.

## Oracle

`assert_against_independent_model` recomputes connectivity from the test's own
live wire/label/part maps: point adjacency, a from-scratch geometric flood, then
a from-scratch (component, label-text) logical flood. It compares `net_count`,
the point-to-net bijection, per-net wire/label sets (as sets, no ordering), and
live/dead `net_of` probes. It does not call `Connectivity::rebuild*`, read slot
or component ids, or assert traversal order. The pre-existing reciprocal
`assert_invariants` checker additionally validates cached endpoint slots and
free lists. All pre-existing tests and the 256-case / 16-wire-id INV10 proptest
are unchanged.

## Cases

- Seeded sequences: 3 fixed xorshift seeds x 600 steps, 256 wire ids (16x the
  proptest id space), 64 label ids, 32 part ids, 16x16 points. Operations mix
  insert, remove, live replacement by re-adding an id, pin-only points with a
  duplicated pin, `clear`, and rebuild-as-restore followed by more incremental
  edits, checked after every step. One config uses unique label text per label
  id so a logical union cannot hide a geometric error; two use a 12-text pool
  for same-text merging.
- Deterministic splits: a 64-wire chain with pin-only ends and distinct end
  labels; middle, near-left, and near-right bridges (both frontier exhaustion
  sides) removed and restored reversed, repeatedly; parallel edges removed
  until the last one splits; a rectangle cycle where one removal meets (no
  split) and the opposite removal splits.
- High degree and slot reuse: two 40-spoke hubs (adjacency beyond inline
  capacities) joined by one bridge, removed/restored three times; live
  wire/label/part replacement at new geometry; five rounds of building a
  12-wire chain, freeing every point, and rebuilding elsewhere.
- Stamp wrap: `visit_gen` seeded at `u32::MAX - 3` / `u32::MAX - 1`, then
  repeated splits and a cycle meeting across the wrap, extending the existing
  exact-wrap test.

## Sample-size rationale and runtime

Sizes are fixed and bounded, not inflated from the reviewer's temporary
12,000-edit probes: 600 seeded steps per config reach steady-state replacement,
slot recycling, and split/repair mixes while each step's oracle is linear-ish
in the 256-point model. The focused connectivity suite (14 tests, including
the unchanged proptest) completes in about 5 s in a debug build on the
development machine; this is a runtime observation, not a performance
certification, and no timing threshold is asserted.
