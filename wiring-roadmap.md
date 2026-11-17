# Schematic wiring: roadmap for agents

The task list for wiring behaviour. The reference for every `W`-prefixed id used here (`WB`,
`WK`, `M`, `WD`, `WINV`, `TC`, `WQ`) is [wiring-plan.md](wiring-plan.md). Ids without a `W`
(`B`, `K`, `D`, `INV`, `Q`, `T1.1`…) are in [canvas-plan.md](canvas-plan.md) and
[canvas-roadmap.md](canvas-roadmap.md).

This roadmap runs beside the canvas roadmap, not after it. Most tasks here wait for a canvas
task; the table under "Where each task fits" says which.

## How to work a task

The same five steps as the canvas roadmap: pick a `todo` task whose dependencies are `done`,
mark it `doing`, read what **Read** names, do **Do** inside **Owns**, run the checks, mark it
`done` and add a log entry at the end of this file.

Statuses: `todo`, `doing`, `done`, `blocked (reason)`, `needs-human`.

## Rules

Canvas roadmap rules 1 to 12 all apply, with branch names `task/<id>-<slug>`, for example
`task/W1.2-runs`. Four more:

13. **Build from the plan.** The rules are in wiring-plan.md sections 5 to 8. This
    documentation change includes only the two wiring Markdown files (WQ8).
14. **Test the specified result.** A test asserts the result in the `TC` table. Manual
    observations at HW2 are logged; changes to the specified behaviour require owner review.
15. **A shared file has one owner at a time.** Several tasks here extend a file a canvas task
    creates (`snap.rs`, `route.rs`, `command.rs`, `tool/move_.rs`, `tool/wire.rs`,
    `selection.rs`). Do not start such a task until the canvas task is `done` and merged.
16. **Keep both plans true.** If a task changes a constant, decision or signature, update
    wiring-plan.md in the same pull request, and canvas-plan.md too if section 13 of the
    wiring plan lists it.

## Standard checks

The canvas roadmap's: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `cargo deny check`.

## Milestones

| # | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| W0 | References in the repository, owner decisions, both plans consistent | W0.3 | HW1 |
| W1 | Document-side foundations: preferences, runs, snap rules, validator, route candidates | W1.5 | WINV3, WINV7, WINV10, WB5 |
| W2 | Wires follow moved parts | W2.5 | WB1 to WB4, WB7, WINV1, WINV2, WINV4 to WINV6, WINV8, WINV11 |
| W3 | Wire tool corner, segment drag, rubber-band modes, delete with wires, labels on runs | W3.7 | WB6, WINV9 |
| W4 | Scenario fixtures, manual validation, the router decision, final report | W4.5 | All |

## Where each task fits

| Wiring task | Waits for (canvas) | Can start |
| --- | --- | --- |
| W0.1, W0.2 | nothing | now |
| W1.1, W1.2 | T2.1 (done) | now |
| W1.3 | T4.2 | canvas milestone 4 |
| W1.4, W1.5 | T6.1 (and T2.2 through it) | canvas milestone 6 |
| W2.1, W2.2 | T5.1, plus HW1 | canvas milestone 6 |
| W2.3 | T5.3, T6.2 | after T6.2 |
| W2.4, W2.5 | T5.4, T6.4 | after T6.4 |
| W3.1 | T6.2 | after T6.2 |
| W3.2, W3.3 | T5.1, T5.3 | after W1.4 |
| W3.4 | T4.3 | canvas milestone 4 |
| W3.5 | T5.3 | after W1.2 |
| W3.6 | T7.2 | canvas milestone 7 |
| W4.x | T6.4 | after W3.7 |

## Dependency overview

```
W0.1 -> W0.2 [HW1] -> W0.3
T2.1 -> W1.1, W1.2
T4.2 -> W1.3
T6.1 -> W1.4 -> W1.5
W1.1 + W1.2 + W1.5 + T5.1 + HW1 -> W2.1 -> W2.2 -> W2.3 -> W2.4 -> W2.5
T6.2 + W1.3 -> W3.1
W1.4 + T5.1 -> W3.2 -> W3.3
T4.3 -> W3.4            W1.2 + T5.3 -> W3.5            T7.2 + W2.2 -> W3.6
W3.1 + W3.3 + W3.4 + W3.5 + W3.6 -> W3.7
W3.7 -> W4.1 -> W4.2 [HW2]       W2.4 -> W4.3 [HW3] -> W4.4 (optional)       -> W4.5 [HW4]
```

The `doc` lane tasks (W1.1 to W1.5, W2.1, W2.2, W3.2, the doc half of W3.5 and W3.6) have no
UI framework dependency and can run ahead of the `canvas` lane.

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| HW1 | W0.2 | Answers WQ1 (canvas Q6). Answers WQ2 to WQ8 or leaves their defaults. Milestone W2 does not start without a "yes" to WQ1. |
| HW2 | W4.2 | Performs the `TC` scenarios marked "MT" manually and records the observed results. About an hour. |
| HW3 | W4.3 | Reads WB7's measured refusal rate and decides whether the router (W4.4) is built. |
| HW4 | After W4.5 | Reads the final wiring report. |

---

## Milestone W0: references and decisions

### W0.1 Check the reference · done
- Lane: bench · Depends: none · Read: wiring-plan.md section 1 (implementation rule), WQ8
- Do: check that the wiring plan contains the information needed to implement its rules.
- Done when: the plan is self-contained and the implementation rule is recorded.

### W0.2 Owner decisions · done → HW1
- Lane: any · Depends: W0.1 · Read: wiring-plan.md sections 6 and 12
- Owns: wiring-plan.md section 12
- Do: set this task to `needs-human`. The owner answers WQ1 to WQ8. Record each answer in
  section 12 with its date, the way canvas Q4 was recorded.
- Done when: WQ1 has an answer. If it is "no", set W2.1 to W2.5 to `blocked (WQ1)` and log it;
  the rest of the roadmap still stands.

### W0.3 Make both plans agree · done
- Lane: any · Depends: W0.2 · Read: wiring-plan.md section 13
- Owns: canvas-plan.md (Q6, section 9, INV2), canvas-roadmap.md (T4.2, T4.3, T5.1, T6.1, T6.2,
  T6.3, T7.2, dependency overview)
- Do: apply every row of section 13, using the owner's answers from W0.2. Change no canvas
  task that is already `done`; if a row touches one, log it and add a follow-up line to the
  wiring task that will make the change in code.
- Done when: no statement in the canvas files contradicts a `WD`; T6.3 points at W2.1 to W2.5;
  section 13 of the wiring plan is marked applied with the commit.

## Milestone W1: document-side foundations

### W1.1 Wiring preferences · done
- Lane: doc · Depends: T2.1 · Read: WD3, WD12, WK2, WK3, WK10
- Owns: `crates/schematic-doc/src/prefs.rs`
- Do: `SelectMode`, `WiringPrefs` with the WK10 defaults, `FollowPolicy`, and the function
  that turns preferences plus the modifier state into a policy. A threshold below 1 is
  rejected at construction.
- Done when: unit tests cover the defaults, the rejected threshold, and the truth table of
  "does this part follow" over follow on/off, count below/at/above the threshold, and
  modifier up/down (assert all 12 rows).

### W1.2 Runs · done
- Lane: doc · Depends: T2.1 · Read: WD1, WD2, WD3, D2
- Owns: `crates/schematic-doc/src/run.rs`, and a label-by-point lookup in `points.rs` or `doc.rs`
- Do: `Anchor`, `Run`, `Doc::run_through`, `Doc::runs_at`, `Doc::connected_pins`, and
  boundary-point discovery for a set of selected items. A label's point must be findable
  from the point map's side; add the lookup the walk needs. Use the crate-private insert
  functions to build test documents (commands are T5.1).
- Done when: property tests on generated sheets show that every wire belongs to exactly one
  run, a run's interior points are all corners in WD1's sense, and its two ends are anchors;
  `connected_pins` equals a brute-force count (TC-count-01, TC-count-02); for a random
  selection, the affected runs found are the same set whatever the order of the selection
  (WINV8); a criterion bench records `runs_at` on `10k` once T2.4's generator exists (log
  "not measured" until then).

### W1.3 Snap rules · done
- Lane: doc · Depends: T4.2 · Read: WD5, WK4, WINV7
- Owns: `crates/schematic-doc/src/snap.rs` (after T4.2)
- Do: bring `snap` to WD5: the class ranking, inclusive radius, `i64` distance ties then
  lowest id, the body projection moved to a grid point on the segment, and the `Junction`
  class in the result. Add `bench hit --snap`.
- Done when: the WINV7 property test passes, including a variant that rebuilds the same
  sheet with items inserted in reverse and expects the same snap result; TC-snap-01 to
  TC-snap-08 pass as unit tests; `bench hit --sheet 10k --snap` reports p99 < 0.2 ms (WB5;
  reference machine for the gate).
- Follow-up (W0.3, from T4.2): applies WD5 ties, grid-snapped body projection, and the
  `Junction` class to T4.2's `snap.rs` (PR #41).

### W1.4 Normal form and the validator · done
- Lane: doc · Depends: T6.1 · Read: WD4, WD9, WINV3, INV2
- Owns: `crates/schematic-doc/src/route.rs` (after T6.1); W1.4 owner-approved extension
  (2026-11-16): local Delete integration in `command.rs`, existing inverse documentation
  in `undo.rs`, and D2 in canvas-plan.md.
- Do: `check_new_wires` (WD4's three validity rules, returning the `Blocker`); make T6.1's
  merge keep a point that carries a label; a debug-only `Doc::assert_normal_form` used by
  every later wiring test.
- Done when: the WINV3 property test passes over random `AddWires` and `Delete` sequences;
  a test plants each kind of blocker (foreign pin, foreign end, overlap, off-sheet) and gets
  the matching `Blocker`; a wire that only crosses a foreign wire's interior is accepted and
  the two stay in different nets (INV2).

### W1.5 Route candidates · done
- Lane: doc · Depends: W1.4 · Read: WD4, WK1, WK5, WK6, WINV10
- Owns: `crates/schematic-doc/src/route.rs`
- Do: `route(doc, a, b, first_leg, out)`: enumerate WD4's candidates in order, validate each,
  score, return the winner or the blocker of the best-scoring invalid candidate. No
  allocation once `out` and the scratch buffers have capacity.
- Done when: property tests show that every returned route is orthogonal, on the lattice,
  in normal form, connects exactly `a` and `b`, and passes `check_new_wires` (WINV10); the
  same inputs always give the same route; hand-built cases cover straight, each L, an L
  blocked by a pin so the other L wins, both L blocked so a dogleg wins, and a part body in
  the way (TC-move-12); a criterion bench records the time for one `route` on `10k` and on
  `dense`.

## Milestone W2: wires follow moved parts

HW1 answered WQ1 "yes" on 2026-11-16 (W0.2); this milestone is unblocked.

### W2.1 The follow planner · todo
- Lane: doc · Depends: W1.1, W1.2, W1.5, T5.1, HW1 · Read: WD2, WD3, WINV6, WINV8
- Owns: `crates/schematic-doc/src/follow.rs`
- Do: `Isometry`, `FollowPlan`, `FollowError`, `plan_follow`. Steps in WD3's order: boundary
  points, affected runs, rigid, policy, follow, detach. Sort runs by a key taken from
  geometry (their boundary point, then far end), not from ids or input order. Labels are
  W3.6; here a run's labels are carried through rigid moves and otherwise left in place.
- Done when: the WINV6 and WINV8 property tests pass; unit tests reproduce TC-move-01 to
  TC-move-13 and TC-move-15 to TC-move-17 at plan level (the plan's wires and fates, no
  command yet); `plan_follow` on a selection with no boundary points returns an empty plan
  and allocates nothing.

### W2.2 Move and Reorient with follow · todo
- Lane: doc · Depends: W2.1 · Read: WD3, WINV1, WINV2, WINV4, INV4, INV10
- Owns: `crates/schematic-doc/src/command.rs`, `undo.rs` (after T5.1)
- Do: `Move` and `Reorient` carry a `FollowPolicy`; `apply` calls `plan_follow`, applies the
  plan and the move as one change, splits wires under landed pins and ends (TC-move-14),
  normalises, and pushes one inverse that restores every removed wire under its old id. A
  `FollowError` returns before anything changes.
- Done when: property tests pass over random sheets and random moves and reorientations for
  WINV1 (compare the partition of pins before and after, in follow mode), WINV2 (detach
  mode), WINV4 (refusals and undo, ids included) and INV10 after every step; a criterion
  bench shows a `Move` of 100 parts with follow on `10k` under 2 ms (WB3; reference machine
  for the gate).
- Follow-up (W0.3, from T5.1): adds `follow: FollowPolicy` to T5.1's merged `Command::Move`
  and `Command::Reorient` (until W2.2, every run detaches).

### W2.3 Move tool: preview, modifier, refusal · todo
- Lane: canvas · Depends: W2.2, T5.3, T6.2 · Read: WD11, WK3, WK7, WINV5, WINV11, D7
- Owns: `crates/schematic-canvas/src/tool/move_.rs` (after T5.3), `overlay/follow.rs`
- Do: call `plan_follow` when the snapped delta, the orientation (R, X, Y during the drag)
  or the modifier changes; draw the plan in the overlay; hide replaced wires with the
  dragged items at drag start; read the modifier from the mouse-up event for the command;
  draw a refused plan in `mark/conflict` with the blocker outlined; treat mouse-up on a
  refused plan as cancel. Above WK7, ghosts only.
- Done when: simulated-input tests show the wires in the overlay at the last preview equal
  the document's wires after mouse-up (WINV5); the revision is unchanged through the drag
  and `SheetView` paints exactly twice (WINV11); Alt at mouse-up flips the result
  (TC-move-04, TC-move-05); TC-move-13 leaves the revision unchanged; goldens show a follow
  preview and a refused one in both themes. On the reference machine, check that Alt during
  a drag reaches the app under the reference desktop session and log the result.

### W2.4 Follow benchmarks · todo
- Lane: bench · Depends: W2.3, T5.4 · Read: wiring-plan.md sections 2 and 10 (bench additions)
- Owns: `crates/schematic-bench/src/follow.rs`, the `--wired` option of `drag-part`
- Do: `bench follow` in both forms and `bench run --script drag-part --wired`. The stress
  sheets need wires attached to parts for this to mean anything: check T2.4's generators
  and log the share of parts with at least one run.
- Done when: all three write their JSON unattended on `1k`, `10k` and `dense`; the refusal
  report lists blocker kinds; raw numbers are in `docs/measurements/W2.4.md`.

### W2.5 Gate: milestone W2 · todo
- Lane: bench · Depends: W2.4, T6.4
- Do: run and record:

  | Budget | Command | Pass |
  | --- | --- | --- |
  | WB1 | `bench follow --sheet 10k --parts 1` | p99 < 0.5 ms |
  | WB2 | `bench follow --sheet 10k --parts 100` | p99 < 4 ms |
  | WB3 | criterion, `Move` of 100 parts with follow | < 2 ms |
  | WB4 | `bench run --sheet 1k --script drag-part --wired` and `--sheet 10k` | dirty-to-present p99 ≤ one refresh; frame p99 within B3, B4 |
  | WB7 | `bench follow --sheet dense --refusals` | recorded; over 2% opens HW3, it does not fail the gate |

  Then the WINV1, WINV2, WINV4, WINV6 and WINV8 property tests at 10× the default case
  count, and the canvas T2.7 table again.
- Done when: WB1 to WB4 pass on the reference machine; otherwise record and set
  `needs-human`. If WB4 misses with the preview on, lower WK7 until it passes, record the
  value that fits, and update the plan; do not relax the budget.

## Milestone W3: wire editing and selection

### W3.1 Wire tool: first corner and snap markers · todo
- Lane: canvas · Depends: T6.2, W1.3 · Read: WD5, WD6
- Owns: `crates/schematic-canvas/src/tool/wire.rs`, `overlay/snap_marker.rs` (after T6.2)
- Do: choose the first leg per WD6; keep Space as the flip; one marker shape per snap class
  including `Junction`, all in `stage/snap`.
- Done when: simulated-input tests pass for TC-corner-01 to TC-corner-04 and TC-snap-09;
  goldens show each marker kind in both themes; the preview still costs no `SheetView`
  paint.

### W3.2 Segment drag command · todo
- Lane: doc · Depends: W1.4, T5.1 · Read: WD7, WINV4
- Owns: `crates/schematic-doc/src/command.rs` (the `DragSegment` variant), `route.rs` (a pure
  `plan_segment_drag` the tool can call for its preview)
- Do: move the segment by `offset` at right angles to its axis; stretch corner neighbours;
  insert a jog at a pin or junction end; move a free end; validate every changed segment
  with `check_new_wires`; normalise; one inverse restoring ids.
- Done when: unit tests pass for TC-seg-01 to TC-seg-04; property tests show that after any
  accepted drag the partition of pins is unchanged, the document is in normal form (WINV3),
  and undo restores it exactly (WINV4); a refused drag returns the `Blocker` and changes
  nothing.

### W3.3 Segment drag tool · todo
- Lane: canvas · Depends: W3.2, T5.3 · Read: WD7, WD11
- Owns: `crates/schematic-canvas/src/tool/segment.rs`
- Do: dragging one selected wire segment in the select tool starts a segment drag, not a
  move; the overlay shows the planned wires; a refusal shows `mark/conflict` and the
  outlined blocker; Esc cancels. Add the `drag-segment` bench script.
- Done when: simulated-input tests cover an accepted drag, a refused one, Esc, and undo;
  `SheetView` paints exactly twice per drag; a golden shows the refused state.

### W3.4 Rubber-band modes · todo
- Lane: canvas (tool), doc (query) · Depends: T4.3 · Read: WD8, WQ3
- Owns: `crates/schematic-canvas/src/selection.rs` (after T4.3), `Doc::query_within` in
  `crates/schematic-doc/src/index.rs`
- Do: both modes from the preference; a normalised box; Shift adds. `query_within` filters
  the index query to items wholly inside.
- Done when: TC-band-01 to TC-band-09 pass as simulated-input tests; a property test shows
  `query_within` equal to a brute-force scan; the INV11 test still passes.

### W3.5 Delete with wires · todo
- Lane: doc, canvas · Depends: W1.2, T5.3 · Read: WD10, WQ4
- Owns: `crates/schematic-doc/src/command.rs` (the `with_wires` flag), the Del and Cut handlers
  in `crates/schematic-canvas/src/tool.rs`
- Do: with the flag set, `Delete` also removes every run starting at a deleted part's pin,
  then normalises what is left. The tool sets the flag from the preference.
- Done when: TC-del-01 to TC-del-03 pass; undo restores parts and wires with their ids
  (INV4); INV10 holds after delete and after undo.

### W3.6 Labels ride their runs · todo
- Lane: doc, canvas · Depends: T7.2, W2.2 · Read: WD9, WINV9
- Owns: `crates/schematic-doc/src/follow.rs` (label placement), `crates/schematic-canvas/src/tool/label.rs`
  (split on placement)
- Do: placing a label on a wire body splits the wire there; `plan_follow` fills
  `FollowPlan::labels` per WD9 and splits the new run at each label; the preview draws
  moved labels.
- Done when: the WINV9 property test passes; TC-label-01 to TC-label-04 pass; INV10 holds
  through each.

### W3.7 Gate: milestone W3 · todo
- Lane: bench · Depends: W3.1, W3.3, W3.4, W3.5, W3.6
- Do: `bench run --sheet dense --script drag-segment` (WB6: dirty-to-present p99 ≤ one
  refresh); every `WINV` property test at 10× the default case count; the W2.5 table and
  the canvas T2.7 table again.
- Done when: all pass on the reference machine; otherwise record and set `needs-human`.

## Milestone W4: scenarios, manual validation, the router

### W4.1 Scenario fixtures · todo
- Lane: bench · Depends: W3.7 · Read: wiring-plan.md section 8
- Owns: `tests/wiring/`
- Do: one RON fixture per `TC` row with the action and the expected document, and one test
  that runs them all through real commands. Scenarios already covered as unit tests are
  still included here: this is the list the owner reads.
- Done when: every `TC` row has a fixture and passes; the test prints a table of id and
  result.

### W4.2 Manual scenario validation · todo → HW2
- Lane: bench · Depends: W4.1 · Read: rule 14
- Owns: `docs/measurements/W4.2.md`
- Do: write the checklist of the "MT" scenarios as steps a person can follow in the
  schematic editor. TC-move-04 and TC-move-05 explicitly check Alt (WK3). Set
  `needs-human`.
  The owner performs the scenarios and records the observed results. Then tabulate:
  scenario, specified result, observed result, same or different, and whether an existing
  `WD` explains each difference.
- Done when: the table exists. Differences that no `WD` explains are listed for the owner;
  nothing is changed in code by this task.

### W4.3 Router decision · todo → HW3
- Lane: any · Depends: W2.4 · Read: wiring-plan.md section 9, WB7, WQ7
- Do: report the measured refusal rate and the blocker kinds from W2.4. If it is over 2%,
  set `needs-human` for the owner to choose: build W4.4, switch WQ7 to detach on failure,
  or accept the rate.
- Done when: the decision is in the log. At or under 2%, set W4.4 to
  `blocked (not needed)`.

### W4.4 Maze router · todo (optional)
- Lane: doc · Depends: W4.3 says build · Read: wiring-plan.md section 9, WK9, WINV10
- Owns: `crates/schematic-doc/src/maze.rs`
- Do: the search in section 9, called by `route` only after every WD4 candidate fails.
- Done when: WINV10 holds for every routed path; `bench follow --refusals` on `dense` is at
  or under 2%; WB1, WB2 and WB4 still pass, or the router is restricted to commit time and
  that is recorded in the plan.

### W4.5 Final wiring report · todo → HW4
- Lane: bench · Depends: W4.2, W4.3 (and W4.4 if built)
- Owns: `docs/wiring-report.md`
- Do: measure WB1 to WB7 on the reference machine, run every `WINV` test and the `TC`
  fixtures. Write one table: id, measured or pass, miss. Add the HW2 differences.
- Done when: the report exists. A miss is reported as a miss, with the measurement.

---

## Log

Append one entry per finished or blocked task. Newest last.

```
<date> <task id> <status> — what was done; numbers with hardware; deviations from the plan;
                             anything the next task needs to know.
```

2026-11-16 plan — wiring-plan.md and wiring-roadmap.md record the design decisions and
scenarios for wiring. Nothing measured; WB1 to WB3 and WB7 are chosen figures. The canvas
files are not yet changed (W0.3). Milestone W2 waits on HW1.

2026-11-16 W1.1 done — implemented SelectMode, checked WiringPrefs and FollowPolicy,
                             preference-to-policy capture and WD3 part/wire decisions in
                             crates/omnia-doc/src/prefs.rs; one-line public module registration.
                             Six unit tests cover defaults, zero rejection in both constructors,
                             valid threshold bounds, custom threshold, captured policy, moved
                             wires and all 12 explicit part-follow truth-table rows. Full
                             cargo test --workspace passes (59 doc unit tests + 3 allocation
                             tests); fmt --all --check, workspace all-targets clippy -D warnings,
                             cargo deny check (existing dependency warnings), diff check pass.
                             No performance measurement or new dependencies. Ambiguities:
                             WK10's true is conditional on Q6 yes; unanswered Q6/WQ1 means
                             follow defaults false until HW1, WQ3 Enclosed and WQ4 false.
                             Alt inverts the entire WD3 conjunction, including threshold;
                             rigid translation stays the follow planner's responsibility.
                             Sketch threshold fields made private with checked constructors
                             and read accessors to prevent zero; counts use usize without
                             narrowing. WD12/API sketch updated under rule 16. W1.2-owned
                             files untouched; W2 remains gated on HW1.

2026-11-16 W0.1 done — every WD, WK, WINV, WB, TC, WQ and M id is defined exactly once in
wiring-plan.md and every reference in either wiring file resolves, including the references
into canvas-plan.md and canvas-roadmap.md (B, K, D, INV, Q, T, HW ids); no dangling or
ambiguous references found. The Read lists of W1.1 to W4.5 were checked against what each
task must build and test: types, signatures and constants are in wiring-plan.md sections 3,
6, 7, 8 and 10, and the expected TC results are in section 8. Two Read lists were missing
the section that defines what the task implements, fixed here: W1.1 now also reads WD3 and
WK3 (the follow-policy rule and the modifier its 12-row truth table asserts) and W1.2 now
also reads WD3 (where `connected_pins` is defined). A note near the top of wiring-plan.md
now states that `schematic-doc`/`schematic-canvas`/`schematic-bench` mean this repo's
`omnia-doc`/`omnia-canvas`/`omnia-bench` crates. The implementation rule (wiring-plan.md
section 1, WQ8) is recorded, so the plan is self-contained. Open items for the owner, not
decided here: (a) wiring-plan.md section 1 says commands, connectivity, the spatial index
and routing do not exist in the doc crate, but canvas-roadmap.md marks T2.2 and T4.1 done
and T5.1 needs-human, and crates/omnia-doc/src already holds command.rs, undo.rs,
connectivity.rs, index.rs and snap.rs — the sentence needs rewording, wording left to the
owner; (b) W3.2 names a pure `plan_segment_drag` for the tool preview but section 10 of the
plan sketches no signature for it; (c) W1.5 says to return the blocker of the best-scoring
invalid candidate, but WD4 defines the score comparison only for valid candidates — applying
the same order to invalid ones is assumed, not stated. Nothing measured; docs only.

2026-11-16 W0.2 done — HW1: the owner answered WQ1 yes (wires follow, milestone W2
unblocked), WQ3 Intersecting (Enclosed as the option), WQ4 no, WQ7 refuse, and kept the
defaults for WQ2 (Alt), WQ5 (no; revisit after W2.3), WQ6 (no) and WQ8 (no). Recorded in
wiring-plan.md section 12. WK10 now states the answered defaults; `WiringPrefs::default()`
in crates/omnia-doc/src/prefs.rs follows it. W0.1 open item (c) resolved in WD4: with no
valid candidate, the same comparison ranks the invalid ones and the refusal reports the
lowest-scoring one's first failed rule, walking from `a`. Items (a) and (b) go to W0.3 and
W3.2. Canvas Q6, section 9 and the canvas roadmap are W0.3's.

2026-11-16 W0.3 done — applied wiring-plan.md section 13 and the W0.2/HW1 owner answers to
canvas-plan.md (Q6 answered Yes -> WD3 and W2.1–W2.5; section 9 updated for FollowPolicy on
Move/Reorient, DragSegment, Delete.with_wires, FollowError/Blocker, Snap::Junction, WD5 snap
class/ties/body projection, Doc::query_within; INV2 label-keeping merge) and
canvas-roadmap.md (dependency overview, T4.3 WD8/WQ3 Intersecting default, minimal T6.1
Read/Do reference to WD4/WD9 keeping status/log untouched, T6.2 WD5/WD6, T6.3 -> W2.1–W2.5,
T7.2 WD9 wire-body split). Left T4.2 (`done`) and T5.1 (`needs-human`, PR #29 merged)
unchanged in substance, adding pointer notes and follow-ups on W1.3 (PR #41 open, not yet
merged) and W2.2. Fixed W0.1 open item (a) in wiring-plan.md section 1 and marked section 13
applied (`commit: see PR`). Docs only; nothing measured.

2026-11-16 W1.2 done — `crates/omnia-doc/src/run.rs`: `Anchor`, `Run`, `Boundary`,
`Doc::run_through`, `Doc::runs_at`, `Doc::connected_pins`, `Doc::boundary_points` and
`Doc::affected_runs`; `Points::at` in `points.rs`; `Doc::labels_at` in `doc.rs`, and a
read-only `Connectivity::labels_at` in `connectivity.rs` (see below). Property tests
on generated sheets (built with the crate-private inserts) check that every wire is in exactly
one run, interior points are WD1 corners, ends are anchors, corner labels are collected,
`connected_pins` equals a brute-force count, and boundary points and affected runs are the same
for any order of the selection, each run once (WINV8). TC-count-01 and TC-count-02 are unit
tests. `runs_at` on `10k`: 232 to 247 ns mean, p99 0.8 µs per point; on `dense`: 1.05 to
1.21 µs mean, p99 2.7 µs (two runs; i7-14650HX, Windows 11, AC; not the reference machine;
docs/measurements/W1.2.md). The first run's 23.7 ms `dense` max did not recur.
Resolved toward WD1/WD2, and recorded in wiring-plan.md section 10: (1) one anchor kind per
point in the order Boundary, Pin, Junction, FreeEnd, so a pin on a junction is a `Pin`
(lowest part and pin); (2) a new `Anchor::Closed` for a closed loop of corners, which has no
anchor; both ends are its least point, and only `run_through` reports it; (3) `Anchor::Pin`
carries `pin: usize`, as `Doc::pins` and `Snap::Pin` do; (4) WD2 boundaries need a
selection-aware walk, so `boundary_points` and `affected_runs` were added and `runs_at` stays
selection-free (empty at a corner); (5) `connected_pins` counts a pin when its point holds a wire
end, another pin (the same part's included) or a label, reading WD3's "anything" literally.
Review follow-up (PR #38): (a) `runs_at` and `affected_runs` sort their result by `a`, `b`,
the first wire's far point, then its id, and the loop/both-boundary dedup also prefers the lesser
first step before ids, so delete+undo and save/load (which reorder a point's incidence list)
give the same ordered result; property-tested by rebuilding each generated sheet both ways.
(b) Labels are now read from the connectivity point store's per-point label list (the "point
map's side" the task asks for): one new `pub(crate)` read-only accessor,
`Connectivity::labels_at`, in `connectivity.rs`, a small ownership extension beyond this task's
files; no new label-maintenance code (the existing `add_label`/`remove_label` calls in every
insert, remove, restore, `Move`, `Reorient`, undo, redo and load path keep it current, as a
property test over those edits checks). The walk no longer queries the spatial index or
allocates a scratch vector per call. `Doc::labels_at` now fills `Vec<LabelId>`, not
`Vec<ItemId>`; wiring-plan.md section 10 updated (rule 16). (c) The run test helper now
deduplicates and asserts each wire occurs exactly once, closed runs included.

2026-11-16 W1.3 done — brought `crates/omnia-doc/src/snap.rs` to WD5: `Pin` > `WireEnd`/`Junction`
> `WireBody` > `Grid`, inclusive `i64` Euclidean radius, ties by distance then lowest id then
coordinates, the body target moved along the segment to a grid point on it, and
`Snap::Junction { at }`. Kept `Snap::Pin::pin: usize` (matching `Doc` storage and canvas plan
section 9; wiring-plan.md section 10 says so). Review fixes (PR #41): owner decision 2026-11-16
"radius wins": removed T4.2's settle loop, so a pin, end, junction or body result is never
beyond the radius. INV9 is reworded in canvas-plan.md (owner-approved change of the
invariant, not a weakened check; property ranges unchanged): snapping the result again
returns it unless a higher-class target lies within the radius of the result. Recorded in
WD5, WINV7, wiring-plan.md section 13, the canvas plan section 9 comment and a dated T4.2
note. The INV9 property tests (snap.rs, and T4.2's in crates/omnia-doc/src/hit.rs) assert the
reworded invariant exactly. The WINV7 oracle is now independent: it ranks a flat list of
every target of every class over all items. Regressions: 50% zoom junction one grid from a
pin, one-grid stub at 50% and half-grid stub at 100% (cursor 0.1 mm off the free end gives
`WireEnd`; exactly at the free end the pin is at the radius and wins), WireBody ties in both
id and index orders (dropping the id from the tie key fails it). WD5 now states the
half-grid body rule, no body target without a grid point on the segment, and the shared
end/junction class ranked by lowest `WireId`. `bench hit --snap` probes every class and
reports `probe_classes`, timed `classes` and `unreachable_classes`. On `10k` junction and
wire body cannot occur at all (half-grid stubs from pins; generator unchanged). The test
requires every other class non-zero there. Non-reference Windows dev machine (i7-14650HX, AC,
not the reference machine): `10k` p50 0.0002 ms, p99 0.0003 ms (timed 9,845 pin / 7,253
end / 0 / 0 / 2,902 grid); `dense` p99 0.0004 ms (junction 424). The WB5 gate is pending on
the reference machine. Raw JSON and hardware in docs/measurements/W1.3.md. Standard checks
(fmt, both clippy runs, cargo test --workspace, cargo deny) pass locally; no check weakened.

2026-11-16 W1.4 done - route.rs now provides check_new_wires and Blocker (WD4,
WINV10), plus the debug/test-only Doc::assert_normal_form (INV1, INV2, WINV3, WD9).
Validation queries each segment through the spatial index, collecting only local contacts;
no whole-document copy or scan. Rule priority applies across the candidate, then segment
order from a, then distance along that segment, geometry, kind and lowest id/index.
Coincident pins precede ends. Own-end coordinates exempt pins/ends but never overlaps.
Sheet also reports off-lattice or malformed segments; Pin uses usize, matching Doc::pins
without narrowing. Section 10 records these API clarifications for W1.5. No WQ defaults
changed. Pure interior crossings pass validation and remain different nets (INV2).
T6.1 already kept labelled points: no replacement of that behavior was needed; a touched
label regression verifies both AddWires splitting and retention through Delete.
Owner-approved scope extension: command.rs shares the existing wire-plan executor with
Delete, and undo.rs documents reuse of ReplaceWires. Delete merges only at the fixed
endpoints of deleted wires, after all removals; exactly two collinear ends and no pin/label
permit a join. Multiple eligible joins are united in deterministic order, keeping the
lowest id. No traversal into distant legacy chains or document-wide repair. Undo reverses
merges before restoring deleted items; redo restores exactly the same ids (INV4). D2 in
canvas-plan.md records this rule. Deleting only parts/labels introduces no merge points.
Tests: 128-case WINV3 property over lattice AddWires/wire-Delete sequences with random pins
and labels, exact undo/redo ids and INV10 rebuild checks after every step; blocker kinds,
rule precedence, reversed traversal/index order, own ends, multi-point Delete determinism,
label preservation and unrelated legacy geometry. Move remains outside normal-form
assertions, with the existing mixed-edit INV4/INV10 properties retained unchanged in range.
All five standard checks PASS locally on Windows: fmt, workspace all-target clippy,
scene-counts/readback bench clippy, workspace tests (103 doc tests plus allocation tests
and Windows rendering tests), cargo deny (existing dependency warnings); diff check passes.
No new dependencies, performance measurement or budget certification. Concurrent timing
re-freeze files in docs/measurements/command-boundary/ were read only, never edited.
Delete overhead changes may affect doc_10k_wire_delete, doc_chain_20k_delete and
doc_label_bridge_delete. By fixture inspection none gains a merge: the 10k leaf retains
a pin at its anchor and no end at its free tip; chain deletion opens a gap with one end
per side; bridge deletion leaves one end at each corner. Re-freeze those three cases on
the final binary even though their resulting geometry does not change.

2026-11-16 W1.5 done - route.rs: `route(doc, a, b, first_leg, scratch, out)` with a
caller-owned `RouteScratch` (signature change from the sketch; section 10 updated), `Axis`,
`DOGLEG_REACH_NM` (WK5), `DOGLEG_CAP` (WK6), and `check_new_wires_in`, which takes the query
buffer so validation allocates nothing warm (`check_new_wires` delegates to it). Candidates
in WD4 order (straight 0; L along first_leg 1, other L 2; doglegs 3..), each scored
(body segments, bends, crossings, length, number), sorted in place, validated in score order
through W1.4's validator; the first valid wins, else the blocker of the lowest-scoring
candidate (the same comparison ranks the invalid ones, as WD4 says). WD4's "first rule that
fails, walking its segments from a" and W1.4's "rule priority before segment order" agree
under the reading that the rule is chosen first and the walk picks within it; W1.4's
function is used unchanged in that respect. Deviation for owner review: WD4 rule 2 now
covers the run's corners (vertices that are not own ends). Without it a corner on a foreign
pin, wire end or wire body passed validation and AddWires would connect a third net, which
breaks the done-when "connects exactly a and b" and TC-move-08's rigid check. Added
`Blocker::WireBody(WireId)` for a corner on a foreign wire's interior; WD4 and section 10
record it. One W1.4 test case changed accordingly (its chained corner sat on a foreign
body); its rule-precedence intent is kept with unchained wires, and new cases cover a corner
on a pin, end and body. Enumeration choices recorded in WD4: doglegs by grid steps from a,
H-V-H before V-H-V, towards b first (lesser coordinate when level), within the ends' span
widened by WK5, zero-length shapes skipped and not counted, doglegs also when the straight
exists; body count uses open interiors (edges do not count). Tests: hand-built straight,
each L, L blocked by a pin (and by a pin on its corner), both Ls blocked so a dogleg wins,
level ends round a U, TC-move-12 body in the way, bends before crossings, refusal ranking
(including a case where a dogleg outranks both invalid Ls), candidate order and the WK6 cap
against an independent enumeration. Properties (256 cases each): every route matches a
enumeration/scoring oracle (whole-sheet scan, no index; production validator reused)
in result and blocker; is orthogonal, on the
lattice, a chain from a to b, passes check_new_wires (WINV10); adding it keeps normal form
and INV10 and joins exactly the nets at a and b; and the same route comes back with a fresh
or warm scratch and on the sheet rebuilt in reverse id order. tests/route_alloc.rs (new
integration test, needed for the counting allocator) shows zero allocations warm. Bench
(crates/omnia-bench/benches/route.rs, diagnostic, non-reference Windows machine i7-14650HX,
battery, power scheme Silent): `10k` mean 23.2 us, per-call p50 14.0 us, p99 274 us;
`dense` mean 51.2 us, p50 42.3 us, p99 455 us (every dense case refused: the moved pin sits
on its own rays, the worst case). docs/measurements/W1.5.md. For W2.1: callers remove the
run being replaced from the document view before routing, as for check_new_wires; a lone
label (not on a pin or end, which WD9 does not produce) is not a blocker.

2026-11-16 W1.5 review fixes � WD4 scores off-sheet candidates using a sheet-clipped
index query and the original segment geometry. Invalid candidates retain body and crossing
penalties, preserving refusal ranking. A regression compares every candidate against the
whole-sheet scoring oracle at all four sheet edges and asserts the specified pin blocker.
Clarified that enumeration and scoring are independent but validity and blocker selection
reuse check_new_wires; no independent-validator claim. No budget or default changed.
All five standard checks pass locally on Windows after the review fixes: fmt, workspace clippy, feature bench clippy, workspace tests and cargo deny (existing warnings only). No check weakened; no measurements run.
