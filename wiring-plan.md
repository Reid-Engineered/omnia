# Schematic wiring: plan (reference)

This file is the reference for wiring behaviour on the schematic canvas: what happens to
wires when a part moves, snapping, routing, segment editing, rubber-band selection, labels on
wires, and the tests and budgets for all of it. The work itself is in
[wiring-roadmap.md](wiring-roadmap.md), which lists tasks that cite the IDs defined here.

It extends [canvas-plan.md](canvas-plan.md) and does not replace it. Every `B`, `K`, `G`, `D`,
`INV` and `Q` id without a `W` in front is defined there and still holds. Where this plan
changes a canvas rule, section 13 lists the change, and roadmap task W0.3 applied it.

The rules to build are in sections 5 to 8 here.

Written 2026-11-16 (status updated 2026-11-16 in W0.3). In `schematic-doc`, geometry, arenas,
the point map, the spatial index, connectivity, hit testing and snapping, commands and undo, and
wiring preferences exist (T1.1, T2.1, T2.2, T4.1, T4.2, T5.1, W1.1; W1.3 snap rules open in
PR #41); wire routing is in progress (T6.1).

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `WB` | Performance budget for wiring, a hard requirement | 2 |
| `WK` | Constant every crate must agree on | 3 |
| `M` | Mechanics or policy consideration | 4 |
| `WD` | Wiring decision | 6 |
| `WINV` | Invariant: a rule that must hold in code, each with a test | 7 |
| `TC` | Scenario with its expected result | 8 |
| `WQ` | Open question for the owner, each with a default | 12 |

Crate names: this plan writes `schematic-doc`, `schematic-canvas` and `schematic-bench`. In
this repository those crates are `omnia-doc`, `omnia-canvas` and `omnia-bench` (see `crates/`);
read every `schematic-*` name as the matching `omnia-*` crate.

Status words are the canvas plan's: **DECIDED**, **DEFAULT** (the owner has not answered; build
the stated default and do not ask again), and one more:

- **GATED**: do not build until the named human checkpoint passes.

The `WD` entries define the intended behaviour. Section 4 records design alternatives and
trade-offs; the scenario table and invariants define the required results.

## 1. Scope

In scope:

- Wires following a moved, rotated or mirrored part (canvas Q6, roadmap T6.3).
- Snap priority, acceptance and ties while wiring (refines T4.2 and T6.2).
- The first-corner rule of the wire tool (refines T6.2).
- Dragging a wire segment (new; not in the canvas roadmap).
- Rubber-band selection mode (refines T4.3).
- Deleting a part's wires with the part (refines T5.3).
- Net labels staying on their wire when it is rebuilt (refines T7.2).
- An optional maze router, only if a measurement asks for it (section 9).

Out of scope: simulation, netlists, external file formats, buses, hierarchical connectors, a
preferences window, persistence of preferences, label position locking, and every rendering
choice the canvas plan already made. Stroke weight (Q1, K5) and junction dots (D2, K6) are
unchanged: section 4 records the related design considerations and adds no rendering work.

Implementation rule. Agents implement from the rules in this file. This documentation
change includes only the two wiring Markdown files; implementation work is separate (WQ8).

## 2. Budgets

Measured on the reference machine under canvas plan section 2.1. `bench` means
`cargo run --release -p schematic-bench --`. All canvas budgets still apply; these are added.

| ID | Budget | Measured by |
| --- | --- | --- |
| WB1 | Planning a follow for one part with up to 9 attached runs on the 10,000-part sheet: p99 < 0.5 ms | `bench follow --sheet 10k --parts 1` |
| WB2 | Planning a follow for 100 parts and up to 256 runs on the 10,000-part sheet: p99 < 4 ms | `bench follow --sheet 10k --parts 100` |
| WB3 | Applying a `Move` of 100 parts with follow on the 10,000-part sheet: < 2 ms (the canvas T5.1 figure without follow is 1 ms) | criterion bench in `schematic-doc` |
| WB4 | B6 holds with the follow preview on: dirty-to-present p99 ≤ one refresh, frame p99 within B3 and B4 | `bench run --sheet 1k --script drag-part --wired` and `--sheet 10k` |
| WB5 | `snap` with the section 6 rules: p99 < 0.2 ms at 10,000 parts (same figure as B7) | `bench hit --sheet 10k --snap` |
| WB6 | Dragging a segment is drawn in the next frame (B6) | `bench run --sheet dense --script drag-segment` |
| WB7 | Route refusals: under 2% of random single-part one-grid-step moves on `dense` are refused for lack of a route | `bench follow --sheet dense --refusals` |

WB7 is the measurement that decides the maze router (section 9). It is a quality figure, not
a speed figure, and missing it does not fail a gate: it opens checkpoint HW3.

WB1 to WB3 and WB7 are chosen numbers, not derived ones. WB1 and WB2 leave the planner a
small share of a 16.7 ms frame because the overlay still has to paint the result.

## 3. Constants

| ID | Constant | Value |
| --- | --- | --- |
| WK1 | Wire lattice | Every wire end lies on the pin lattice, 1,270,000 nm (K2). The cursor snaps to the 2,540,000 nm grid unless it snaps to a pin, end or body. |
| WK2 | Follow threshold (chosen; see M2) | A part follows only if it has fewer than 10 connected pins. Preference, whole number ≥ 1. |
| WK3 | Follow modifier (chosen) | Alt held at commit inverts follow and detach for the gesture. Not Space: Space pans (T1.5) and flips the wire corner (T6.2). |
| WK4 | Snap acceptance | K14: snap radius 10 px and hit tolerance 4 px on screen, converted to nm by the camera at the call. A target exactly at the radius is accepted. |
| WK5 | Dogleg search reach (chosen) | Mid-lines are tried up to 8 grid steps (20,320,000 nm, the K12 cell) beyond the box spanned by the two ends. |
| WK6 | Candidate cap per run (chosen) | 2 L-routes plus at most 128 doglegs. |
| WK7 | Preview cap (chosen) | The follow preview is drawn live for up to 256 affected runs. Above that the preview shows ghosts only and the plan is computed at commit. |
| WK8 | Router trigger (chosen) | WB7's 2%. |
| WK9 | Router costs, if built (chosen; see M11) | Step 2, bend 30, crossing a foreign wire 20. |
| WK10 | Preference defaults | `follow_on_move` true (WQ1 yes); `follow_pin_threshold` 10; `select_mode` Intersecting (WQ3); `delete_wires_with_part` false (WQ4). |

## 4. Design considerations

The `M` identifiers name mechanics and policy alternatives considered by this plan. These
entries describe options, trade-offs and unresolved details; they make no claims about the
behaviour of any external implementation. The `WD` entries specify what to build.

| ID | Design consideration | Decision or follow-up |
| --- | --- | --- |
| M1 | Following can rebuild a run or detach it. Stretching a straight segment can leave diagonals or require an additional route change. | WD3: translate, else route, else detach. |
| M2 | Count connected pins once per pin, rather than counting wire pieces or nets. A threshold of 10 permits follow for at most 9 connected pins. | WK2, WD3; the threshold is configurable. |
| M3 | A modifier can invert follow and detach. Reading it during an update and reading it from the commit event are different policies; rigid translation can ignore it. Space is an alternative binding, but already pans and flips corners. | WK3 uses Alt from the commit event; WD3 leaves rigid translation unaffected. |
| M4 | When both ends undergo the same translation, translating the whole run preserves its shape. A translated segment landing on a foreign pin would create an unintended connection. | WD3 step 1, validated by WD4. |
| M5 | A run shared by two moved parts must be processed once. Taking the follow flag from whichever part is visited first would make the result order-dependent. | WINV8; WD3 requires every boundary part to allow follow. |
| M6 | Detach could create a stored junction and a visible dot at the old end. | Not adopted: a free end is just an end (D2). |
| M7 | Snapping can query nearby pins and ends, filter eligible targets, project onto a wire body, then fall back to the grid. List-traversal priority would make the choice unstable. | WD5 defines eligibility, explicit class ranking and deterministic ties. |
| M8 | A world-unit snap radius grows on screen with zoom. | Not adopted: K14 stays in screen units (WK4). |
| M9 | Rejecting already-wired pins as reconnect targets would prevent multiple wires ending on a pin. | Not adopted: already-wired pins remain valid targets. |
| M10 | The dominant cursor axis can choose the first leg; a tie can go horizontal. Subsequent cursor motion could either track the dominant axis or keep the initial choice. | WD6 uses the dominant axis once, then keeps the choice until Space flips it or the corner is committed. |
| M11 | A four-direction grid search can use a bucket queue with step, bend and crossing costs. Pruning, direction tables and queue limits need an explicit design if introduced. | Section 9 defines the optional search; WK9 sets step 2, bend 30 and foreign-wire crossing 20. No additional costs are required. |
| M12 | Repeatedly removing the middle of three collinear points gives a simpler path. | T6.1's merge rule, except that labelled points are retained (WD4, WD9). |
| M13 | Segment dragging can insert grid-snapped jog vertices. A changed segment passing over a foreign pin must be refused; silently dropping the action would hide the reason. | WD7 requires refusal feedback and outlines the blocker. |
| M14 | Normalising the rubber-band box makes drag direction irrelevant. Intersecting and fully enclosed selection are separate policies. | WD8 implements both; WQ3 chooses the default. |
| M15 | Rebuilding a labelled wire requires keeping its label on the surviving net. Reattachment alone leaves the placement policy unspecified. | WD9 defines placement and tie-breaking. |
| M16 | Deleting a part can optionally remove its attached runs. Cut and Delete need the same explicit policy. | WD10; WQ4 chooses the default. |
| M17 | A logical stroke width scales with zoom. A minimum screen width is a separate rendering choice. | Q1's default and D4's floor remain unchanged. |
| M18 | Crossing wire interiors do not connect; a wire ending on a wire body joins it. | INV2 and T6.1's split rule. |

## 5. Architecture

```
 Move tool (schematic-canvas)                         schematic-doc (no UI framework)
 ------------------------                         -------------------
 drag: snapped delta changes
        |                         plan_follow(&Doc, items, isometry, policy)
        +------------------------>   1 boundary points        run.rs
        |                            2 runs                    run.rs
        |    FollowPlan              3 rigid | route | detach  follow.rs
        |<------------------------   4 candidates + validator  route.rs
        v                            5 label positions         follow.rs
 OverlayView: part ghosts,
 planned wires, blocked mark         (pure: the document is not touched)

 mouse up
        |    Command::Move { items, delta, follow: policy }
        +------------------------>  Doc::apply runs plan_follow again on the same
                                    inputs, applies it, pushes one inverse
             Change               <-+
```

One function plans, and both the preview and the command call it. The preview therefore
shows exactly what the commit will do (WINV5), and a refusal is known before mouse-up.

The same validator serves the follow planner, segment drag and, if it is built, the router:
a new wire is acceptable only if its interior touches no foreign end or pin and overlaps no
foreign wire (WD4).

## 6. Decisions

### WD1 Runs: the unit of wire editing. DECIDED

- The document stores single segments (D2). A **run** is derived, never stored: a chain of
  segments joined at **corners**. A corner is a point where exactly two wire ends meet and no
  pin lies. T6.1 merges collinear neighbours, so a corner is a real bend, or a point that
  carries a label.
- A run ends at an **anchor**: a pin, a junction (three or more ends or pins), a free end, or
  a boundary point of the command being planned (WD2).
- A label on a corner rides the run (WD9). A label at any other anchor stays where it is.
- A run is translated, re-routed, detached or deleted as a whole.

### WD2 What a move command carries. DECIDED

- `Move` and `Reorient` apply one isometry `T` (a translation, or a rotation or mirror about
  a pivot) to the selected items `S`.
- A **boundary point** is a point where a pin of a part in `S`, or an end of a wire in `S`,
  meets an end of a wire outside `S`.
- The **affected runs** are the runs of unselected segments that start at a boundary point.
  Each is found once, even when both of its ends are boundary points (WINV8).
- Wires in `S` are moved by `T` like any other selected item and are not re-planned.

### WD3 Wires follow moved parts. DECIDED (HW1, 2026-11-16: Q6/WQ1 answered yes)

For each affected run, in a fixed order that does not depend on the order of `items`:

1. **Rigid.** If both ends are boundary points, apply `T` to the whole run, corners and
   labels included. Accept if the result passes WD4. The modifier does not affect this step.
2. **Policy.** Otherwise, or if step 1 was not accepted, the run follows only if every part
   at its boundary ends allows it. A part allows it when
   `follow_on_move && connected_pins(part) < follow_pin_threshold`, the result inverted if
   the modifier is held. A boundary end that belongs to a moved wire, not a part, allows it
   when `follow_on_move` is true, inverted likewise.
3. **Follow.** Remove the run and route a new one between the two ends' final positions
   (WD4). The far end does not move unless it is also a boundary point.
4. **Detach.** Leave the run exactly as it was. Its boundary ends become free ends.

- `connected_pins` counts a part's pins whose point holds anything besides that pin, counted
  once per pin on the document before the command.
- If a run must follow and no candidate is valid, the whole command is refused with
  `FollowError::NoRoute`, naming the run and the item in the way. The document is unchanged
  (WINV4). The tool shows the refusal during the drag, before mouse-up (WD11).
- Follow re-routes the whole run. It does not try to keep the old shape
  (WQ5).
- A moved pin that touched another part's pin directly, with no wire, simply separates. No
  wire is created for it (WQ6).
- A moved pin or end that lands on an existing end, pin or wire body connects there, by D2
  and T6.1's split rule. That is the only way a move makes a new connection (WINV1).
- Alternatives not adopted: the first-part-wins flag (M5), the junction left behind by a
  detach (M6), polling the key during the update (M3).

Rejected: stretching the straight segment at the pin (it leaves diagonals or forces a silent
re-route anyway); a maze router as the first implementation (section 9); detach only
(follow is the proposed default once approved, and detach stays available through the
threshold and the modifier).

### WD4 Routing between two points. DECIDED

Candidates for a run from `a` (a moved end) to `b`, tried in this order:

| # | Shape | Exists when |
| --- | --- | --- |
| 0 | Straight | `a` and `b` share an x or a y |
| 1, 2 | L, leaving `a` along the axis the old run left it; then the other L | They share neither |
| 3… | Dogleg, three segments: H-V-H with the vertical at `x = m`, and V-H-V with the horizontal at `y = m` | `m` steps by one grid step (K2) outward from `a`'s coordinate, nearest first, across the span between the ends and WK5 beyond it, up to WK6 |

A candidate is **valid** when all of these hold:

- Every vertex is on the wire lattice (WK1) and inside the sheet (K1).
- No segment's interior contains a foreign wire end or a foreign pin. Foreign means not one
  of the run's own two ends.
- No segment lies along a foreign wire with a shared stretch of non-zero length.

Crossing a foreign wire's interior at one point is valid: by INV2 it is not a connection.

Among valid candidates the lowest score wins, comparing in order: segments passing through
the interior of any part's bounds (fewer is better); bends; crossings of foreign wires;
total length; candidate number. The last term makes the choice deterministic (WINV6).
When no candidate is valid, the same comparison ranks the invalid candidates, and the
refusal reports the blocker of the lowest-scoring one: the first rule above that fails,
walking its segments from `a` (clarified at W0.2, 2026-11-16).

The result is normalised by T6.1's merge rule, with one addition: a point that carries a
label is never merged away.

### WD5 Snapping. DECIDED (refines T4.2 and T6.2)

- Priority: pin, then wire end or junction, then wire body, then grid. A higher class within
  the radius always beats a lower one, however close the lower one is.
- Acceptance: Euclidean distance from the cursor's world point, at most the WK4 radius.
- Ties inside a class: smaller distance first (compared as `i64` squared distances), then
  the lower id (`PartId` then pin index; `WireId`). The result never depends on the order
  the index returns items.
- Wire body: the target is the cursor's point projected onto the segment and clamped to it,
  then moved along the segment to the nearest grid point that is still on it. Distance is
  measured to that final point.
- A pin or end that already has wires is a normal target (M9 alternative not adopted).
- Grid snap is the fallback only when no other class has a target in the radius.
- The radius is a screen distance (WK4), so it does not change with zoom (M8 alternative not adopted).
- `snap` reports which class it chose, so the tool can draw the right marker before the
  click (T6.2).

### WD6 First corner of a drawn wire. DECIDED (refines T6.2)

- The preview is an L. Its first leg is chosen once per segment being drawn: when the
  snapped cursor first leaves the start point, the leg is vertical if `|dy| > |dx|`, else
  horizontal. A tie goes horizontal (M10).
- The choice then stays, whatever the cursor does, until Space flips it or the corner is
  committed. Each new segment chooses again.
- Space still flips the corner (T6.2). Cursor motion does not automatically flip it.

### WD7 Dragging a wire segment. DECIDED (new)

- Dragging one selected wire segment with the select tool moves it at right angles to its
  own axis, in grid steps. Along its axis it does not move.
- A neighbour joined at a corner stretches or shrinks to stay attached. If an end of the
  dragged segment is an anchor other than a free end (a pin or a junction), the anchor stays
  and a new jog segment is inserted to reach it. A free end moves with the segment.
- The result must pass WD4's validity test for every segment that changed. If it does not,
  the drag is refused: the ghost is drawn in `mark/conflict`, the item in the way is
  outlined, and mouse-up changes nothing (M13; explicit refusal feedback is required).
- A stretch that would shrink a neighbour past zero length stops at zero; the neighbour is
  then removed and the merge rule applies.
- One `Command::DragSegment`, one undo step. Undo restores the original wires and ids.

### WD8 Rubber-band selection. DECIDED (WQ3 answered 2026-11-16: Intersecting)

- The box is normalised: drag direction never changes the result (M14).
- Two modes, a preference: **Enclosed** selects items wholly inside the box; **Intersecting**
  selects items the box touches. Both are built; WQ3 picks the default.
- Shift adds the result to the selection; without Shift it replaces it.
- A part counts by its symbol bounds, a wire by its segment, a label by its anchor point.
  Label text extents are not part of the test.

### WD9 Labels on wires. DECIDED (refines T6.1 and T7.2)

- A label is positional (D2: `Label { at }`). Its net is whichever net has an end or pin at
  that point. There is no name ranking on a net split: the label stays on the side its point
  is on.
- Placing a label on a wire body splits the wire there, so a label always sits on an end or
  a pin. The merge rule never removes that point.
- When a run is translated rigidly, labels on its corners move with it.
- When a run is re-routed, each label that sat on one of its corners is placed on the new
  run at the lattice point nearest (Manhattan) to its old position; a tie goes to the point
  nearer the run's unmoved end. The new run is split there.
- When a run is detached, nothing about its labels changes.
- A label on a moved part's pin moves with the pin.
- Label position locking is out of scope.

### WD10 Deleting a part's wires. DECIDED (WQ4 answered 2026-11-16: No)

- Preference `delete_wires_with_part`. When on, deleting a part also deletes every run that
  starts at one of its pins, up to that run's far anchor. Runs beyond a junction stay.
- When off, Delete removes only the selection (T5.3 as written), and the wires keep free
  ends.
- Either way it is one command and one undo step. Cut behaves like Delete.

### WD11 Preview and feedback. DECIDED

- The move tool calls `plan_follow` only when the snapped delta, the orientation or the
  modifier changes, never on a bare mouse move.
- The overlay draws the plan: part ghosts (T5.3), new and translated wires as ghosts, and
  runs that will detach left as they are. Wires that will be replaced are hidden from the
  sheet with the dragged items at drag start (D7), so the sheet still paints exactly twice
  per drag.
- A refused plan draws the ghosts in `mark/conflict` and outlines the item in the way.
  Mouse-up on a refused plan cancels the drag like Esc.
- Above WK7 the preview draws part ghosts only; planning and any refusal happen at commit,
  and a refusal is reported in the status bar.
- Colours are tokens only (INV6): `stage/snap` for snap markers, `mark/conflict` for
  refusals, the existing ghost and selection tokens for the rest.

### WD12 Preferences. DECIDED

- `WiringPrefs` is a plain struct in `schematic-doc` with the WK10 defaults. `SheetCanvas` owns
  one. Nothing is persisted and there is no settings window; both are out of scope.
- A command carries the policy it was built with (`FollowPolicy`), so redo replays the same
  decision even if a preference changed in between.
- W1.1 clarification (2026-11-16): `Default` follows WK10 as answered at HW1 (W0.2):
  `follow_on_move` true, `select_mode` Intersecting, `delete_wires_with_part` false. Threshold fields are private `u16`s with
  read accessors; checked constructors return `InvalidFollowThreshold` for zero, so
  struct literals cannot bypass WK2. Types live in `omnia_doc::prefs` in this repository.
  `follow_policy` captures the supplied Alt state; `part_follows` inverts the entire
  conjunction in WD3, including the threshold result, and `wire_follows` implements
  its moved-wire case. Counts use `usize` to avoid narrowing a document pin count.

## 7. Invariants

Each has a test; the roadmap task that adds it is in brackets.

| ID | Rule |
| --- | --- |
| WINV1 | Follow keeps connections: any two pins connected before a follow-mode `Move` or `Reorient` are connected after it, direct pin-to-pin contacts excepted (WQ6). New connections arise only where a moved pin or moved wire end lands on an existing end, pin or body. [W2.2] |
| WINV2 | Detach changes only what it must: unselected wires are identical before and after, and the pins that lose a wire are exactly the boundary pins of detached runs. [W2.2] |
| WINV3 | Normal form after every wiring command: INV1 and INV2 hold; every end is on the wire lattice; no two collinear segments meet at a point that holds nothing else and no label; no two segments share a stretch. [W1.4] |
| WINV4 | A refused command leaves the document equal to the original, ids and revision included. `apply` then `undo` of any wiring command does too (extends INV4). [W2.2, W3.2] |
| WINV5 | The plan drawn for a given document, items, isometry and policy equals what `apply` does with the same inputs. [W2.3] |
| WINV6 | `plan_follow` gives the same plan for any ordering of `items`, and for the same document rebuilt with different ids in the index's iteration order. [W2.1] |
| WINV7 | `snap` returns the highest class with a target in the radius; inside a class the nearest, then the lowest id; `snap(snap(p)) == snap(p)` (INV9). [W1.3] |
| WINV8 | Each affected run appears in a plan exactly once. [W2.1] |
| WINV9 | After a follow, every label that sat on an affected run still sits on a wire end or pin of the net it was on, with its text unchanged. [W3.6] |
| WINV10 | No wire added by the planner, a segment drag or the router has a foreign end or pin in its interior, or shares a stretch with a foreign wire. [W1.5] |
| WINV11 | Planning and previewing change neither the document revision (INV5) nor `SheetView`'s paint count beyond drag start and end (INV11). [W2.3] |

## 8. Scenarios

Each is a fixture in `tests/wiring/` with the required result. "MT" marks the scenarios the
owner also performs manually at HW2. Record the observed result and any deviation for owner
review. Parts sit on the grid; "R" is a two-pin resistor.

| ID | Setup and action | Expected result | MT |
| --- | --- | --- | --- |
| TC-move-01 | R wired at both pins to fixed parts; move R one grid step right | Both runs re-routed; nets unchanged; one undo step | yes |
| TC-move-02 | Part with 9 connected pins; move | All 9 runs follow | yes |
| TC-move-03 | Part with 10 connected pins; move | No run moves; 10 free ends | yes |
| TC-move-04 | TC-move-03 with Alt held | All 10 runs follow | yes (modifier check) |
| TC-move-05 | TC-move-01 with Alt held | Both runs detach | yes (modifier check) |
| TC-move-06 | Two parts joined by a three-segment run; move both | The run is translated, shape intact | yes |
| TC-move-07 | TC-move-06 with Alt held | Same as TC-move-06: rigid ignores the modifier | yes |
| TC-move-08 | TC-move-06, but the translated run would land on a third part's pin | Rigid not accepted; the run is re-routed; the third pin is not connected | yes |
| TC-move-09 | One run shared by two selected parts, selection order reversed | Identical result both ways | no |
| TC-move-10 | A 9-pin and a 10-pin part joined by a run that fails rigid; move both | The run detaches at both ends (every part must allow follow) | no |
| TC-move-11 | Rotate a wired R about its centre | Runs at both pins re-routed to the pins' new points | yes |
| TC-move-12 | Move a part so the only L crosses its own body | A dogleg is chosen | yes |
| TC-move-13 | Move a part into a spot where no candidate is valid | Refused; ghost in `mark/conflict`; document unchanged after mouse-up | no |
| TC-move-14 | Move a part so a pin lands on a foreign wire's body | The wire is split and connected there | yes |
| TC-move-15 | Run from the part ends on a junction; move the part | The junction stays; only that run is re-routed | yes |
| TC-move-16 | Two pins of two parts touching, no wire; move one | They separate; no wire appears | yes |
| TC-move-17 | Select a part and one segment of its run; move | The selected segment moves; the rest of the run follows from the boundary point | no |
| TC-count-01 | Part with two pins on the same net, 9 connected pins in all | Counts 9: pins, not nets | no |
| TC-count-02 | One pin with three wire ends on it | Counts 1 | no |
| TC-snap-01 | Cursor 8 px from a pin and 2 px from a wire body | Pin | yes |
| TC-snap-02 | Two pins in the radius at different distances | The nearer | yes |
| TC-snap-03 | Two pins at equal distance | The lower id, from either index order | no |
| TC-snap-04 | Target exactly at the radius | Accepted | no |
| TC-snap-05 | Wire end and wire body in the radius | The end | yes |
| TC-snap-06 | Wire body only; cursor between two grid points along it | The nearer grid point on the segment | yes |
| TC-snap-07 | Pin that already has two wires | Accepted | yes |
| TC-snap-08 | Nothing in the radius | Grid | yes |
| TC-snap-09 | The same world layout at 25% and 800% zoom | The same class wins at the same screen distance | no |
| TC-corner-01..03 | Start a wire and leave the start to the right; upward; diagonally | Horizontal first; vertical first; horizontal first | yes |
| TC-corner-04 | After the choice, move so the other axis dominates | The corner does not flip; Space flips it | no |
| TC-seg-01 | Drag the middle segment of a Z-shaped run | Its neighbours stretch; still one run | yes |
| TC-seg-02 | Drag a segment whose end is on a pin | The pin end stays; a jog appears | yes |
| TC-seg-03 | Drag a segment across a foreign pin | Refused, with the pin outlined | yes |
| TC-seg-04 | Drag a segment until a neighbour has zero length | The neighbour is removed; the result is in normal form | no |
| TC-band-01..08 | Four drag directions × two modes over a part half inside the box | Enclosed: not selected. Intersecting: selected. Direction irrelevant | yes |
| TC-band-09 | Shift held with an existing selection | The result is added | yes |
| TC-label-01 | Label on a corner of a run that is translated | Label moves with it | yes |
| TC-label-02 | Label on a corner of a run that is re-routed | Label lands on the new run, nearest lattice point, same net | yes |
| TC-label-03 | Label on a run that is detached | Label and run unchanged | yes |
| TC-label-04 | Delete the wire between two halves of a labelled net | The label stays on its own half; the other half loses the name | yes |
| TC-del-01, 02 | Delete a wired part with `delete_wires_with_part` on; off | Its runs go, up to their far anchors; nothing but the part goes | yes |
| TC-del-03 | TC-del-01 where a run ends on a junction | The run goes; the junction's other wires stay and merge if collinear | yes |

## 9. The optional router

Not built unless checkpoint HW3 opens it. WD4's candidates cover a part moved a short way in
ordinary surroundings; they fail in crowded ones. W2.4 measures how often (WB7).

If it is built: a search over (lattice cell, incoming direction) inside the box spanned by
the two ends plus WK5, with a bucket queue and the WK9 costs; cells under a part's bounds are
blocked except at the run's own ends; cells holding a foreign end or pin are blocked (WD4);
the path is turned into segments, normalised, and validated by the same test as any
candidate. It runs only after every WD4 candidate fails, so it can only turn a refusal into
a route, and it must fit WB1 and WB2 or it runs at commit only.

The search is defined by this section. Additional pruning counters, direction tables and
queue limits are outside the initial design (M11).

## 10. Crates and APIs

Crate and path names below use neutral `schematic-` identifiers. In an existing checkout,
map these to the corresponding document, canvas and benchmark crates; this document rewrite
does not rename implementation files. API symbols and task ownership remain unchanged.

New and changed files, all inside the existing crates:

```
crates/schematic-doc/src/
  prefs.rs       WiringPrefs, SelectMode, FollowPolicy                    (W1.1)
  run.rs         runs, anchors, boundary points, connected_pins           (W1.2)
  snap.rs        refined by WD5 (file from T4.2)                          (W1.3)
  route.rs       validator, normal form, candidates (file from T6.1)      (W1.4, W1.5)
  follow.rs      plan_follow, FollowPlan, label placement                 (W2.1, W3.6)
  command.rs     Move and Reorient carry a policy; DragSegment (T5.1's)   (W2.2, W3.2)
  maze.rs        section 9, only after HW3                                (W4.4)
crates/schematic-canvas/src/
  tool/move_.rs        preview, modifier, refusal (file from T5.3)        (W2.3)
  tool/wire.rs         first corner (file from T6.2)                      (W3.1)
  tool/segment.rs      segment drag                                       (W3.3)
  overlay/follow.rs    planned wires and the blocked mark                 (W2.3)
  selection.rs         rubber-band modes (file from T4.3)                 (W3.4)
crates/schematic-bench/src/
  follow.rs            `bench follow`; the `--wired` and `drag-segment` scripts (W2.4, W3.7)
tests/wiring/          scenario fixtures (RON) and their expected results  (W4.1)
```

Signatures are a sketch: names and shapes to build toward, not a frozen interface.

```rust
// ---- schematic-doc: prefs.rs ----
pub enum SelectMode { Enclosed, Intersecting }
pub struct WiringPrefs { pub follow_on_move: bool, follow_pin_threshold: u16,
                         pub select_mode: SelectMode, pub delete_wires_with_part: bool }
pub struct FollowPolicy { pub follow_on_move: bool, pin_threshold: u16, pub invert: bool }

impl WiringPrefs {
    pub const fn new(follow_on_move: bool, follow_pin_threshold: u16,
                     select_mode: SelectMode, delete_wires_with_part: bool)
        -> Result<Self, InvalidFollowThreshold>;
    pub const fn follow_pin_threshold(&self) -> u16;
    pub const fn follow_policy(&self, modifier_held: bool) -> FollowPolicy;
}
impl FollowPolicy {
    pub const fn new(follow_on_move: bool, pin_threshold: u16, invert: bool)
        -> Result<Self, InvalidFollowThreshold>;
    pub const fn pin_threshold(&self) -> u16;
    pub const fn part_follows(&self, connected_pins: usize) -> bool;
    pub const fn wire_follows(&self) -> bool;
}

// ---- run.rs ----
pub enum Anchor { Pin { part: PartId, pin: u16 }, Junction, FreeEnd, Boundary }
pub struct Run { pub wires: SmallVec<[WireId; 4]>, pub a: (Pt, Anchor), pub b: (Pt, Anchor),
                 pub labels: SmallVec<[LabelId; 1]> }
impl Doc {
    pub fn connected_pins(&self, part: PartId) -> u16;
    pub fn run_through(&self, wire: WireId, out: &mut Run);           // the run a segment is in
    pub fn runs_at(&self, at: Pt, out: &mut Vec<Run>);                // runs starting at a point
}

// ---- route.rs ----
pub enum Axis { Horizontal, Vertical }
pub enum Blocker { Pin { part: PartId, pin: u16 }, WireEnd(WireId), Overlap(WireId), Sheet }
pub fn check_new_wires(doc: &Doc, wires: &[Wire], own_ends: [Pt; 2]) -> Result<(), Blocker>;
pub fn route(doc: &Doc, a: Pt, b: Pt, first_leg: Axis, out: &mut Vec<Wire>) -> Result<(), Blocker>;

// ---- follow.rs ----
pub enum Isometry { Translate(Pt), Reorient { pivot: Pt, by: Orient } }
pub enum RunFate { Translated, Rerouted, Detached }
pub struct FollowPlan { pub remove: Vec<WireId>, pub add: Vec<Wire>,
                        pub labels: Vec<(LabelId, Pt)>, pub fates: Vec<(Pt, RunFate)> }
pub enum FollowError { NoRoute { from: Pt, to: Pt, blocker: Blocker } }
pub fn plan_follow(doc: &Doc, items: &[ItemId], t: Isometry, policy: FollowPolicy,
                   plan: &mut FollowPlan) -> Result<(), FollowError>;

// ---- command.rs (changes to the canvas plan's sketch) ----
pub enum Command {
    Move { items: Vec<ItemId>, delta: Pt, follow: FollowPolicy },
    Reorient { items: Vec<ItemId>, pivot: Pt, by: Orient, follow: FollowPolicy },
    DragSegment { wire: WireId, offset: i32 },                        // WD7; nm, signed
    Delete { items: Vec<ItemId>, with_wires: bool },                  // WD10
    /* the rest as in canvas-plan.md section 9 */
}
pub enum CommandError { /* T5.1's */ Follow(FollowError), Segment(Blocker) }

// ---- snap.rs (refines the canvas plan's Snap) ----
pub enum Snap { Pin { part: PartId, pin: usize, at: Pt }, WireEnd { wire: WireId, at: Pt },
                Junction { at: Pt }, WireBody { wire: WireId, at: Pt }, Grid(Pt) }
impl Doc { pub fn query_within(&self, area: Rect, out: &mut Vec<ItemId>); }   // WD8 Enclosed
```

`plan_follow` takes its output buffers from the caller so the move tool can reuse them
across a drag. It is not in the paint path, so B10 does not bind it, but it should allocate
nothing once its buffers are warm.

Bench additions. Every subcommand exits by itself and takes `--json <file>`.

| Command | Reports |
| --- | --- |
| `bench follow --sheet S --parts N [--moves 1000]` | `plan_follow` time, p50, p99, max, over seeded random selections and one-step moves; runs per plan |
| `bench follow --sheet S --refusals` | Share of single-part one-step moves refused (WB7), with the blocker kinds |
| `bench run --script drag-part --wired` | T5.4's script with the dragged parts chosen among wired ones and the preview on |
| `bench run --script drag-segment` | Dirty-to-present and frame times while a segment is dragged |
| `bench hit --sheet S --snap` | `snap` time, p50 and p99, cursor points seeded near pins, ends, bodies and empty space |

## 11. Dependencies and risks

No new dependency. `smallvec` is already in canvas plan section 10.

| Risk | Mitigation | Early test |
| --- | --- | --- |
| A design alternative is mistaken for a requirement (move policy, snap order, labels) | The `WD` entries specify the required behaviour; HW2 validates it by hand | W4.2 |
| Planning or routing breaks B6 on the 10,000-part sheet | Plan only when the snapped delta changes; cap the live preview (WK7); candidates before any search | W2.4, before any router work |
| L and dogleg routes fail too often on dense sheets, so moves get refused | Measure it (WB7); the router is ready as the answer (section 9) | W2.4 |
| Refusing a move annoys more than a detach would | Refusal is visible during the drag; Alt detaches; WQ7 lets the owner choose detach on failure instead | W4.2, by hand |
| A re-route silently joins a third net | WD4 validity and WINV1, WINV10 as property tests | W1.5 |
| The threshold of 10 may not suit the parts used in real sheets | It is a preference | Real sheets, later |
| Alt as the modifier collides with a window-manager binding on Linux (Alt-drag moves windows on some desktops) | The modifier is read at commit, not at drag start; check on the reference machine | W2.3 on the reference machine |
| Segment-drag refusal feels arbitrary | The blocking item is outlined | W3.3 golden |
| This plan and the canvas roadmap drift apart | W0.3 writes the changes into the canvas files; rule 12 of the canvas roadmap applies to both plans | W0.3 |

## 12. Open questions

Each had a default. The owner answered all eight at HW1 on 2026-11-16 (task W0.2); the
answer column below is binding. WQ5 stays open for review after W2.3, as recommended.

| ID | Question | Answer | Recommended |
| --- | --- | --- | --- |
| WQ1 | Canvas Q6: do attached wires follow a moved part, by the WD3 rules? | **Answered 2026-11-16: Yes.** Milestone W2 is unblocked. | Yes |
| WQ2 | Which modifier inverts follow and detach? | **Answered 2026-11-16: Alt** (default kept, WK3). | Alt |
| WQ3 | Rubber-band default: Enclosed (T4.3 as written) or Intersecting (the alternative in M14)? | **Answered 2026-11-16: Intersecting**, with Enclosed as the option. | Intersecting, with Enclosed as the option |
| WQ4 | Does deleting a part delete its wires by default? | **Answered 2026-11-16: No** (T5.3 as written). | Owner's preference; WD10 supports either choice (M16) |
| WQ5 | Should follow keep the old run's shape when only its last segment needs to change length? | **Answered 2026-11-16: No for now** (default kept: re-route the whole run, M1). Revisit after W2.3. | Decide after using W2.3; it is a candidate 0 in WD4 if wanted |
| WQ6 | When a moved pin touched another pin directly, should follow insert a wire to keep them connected? | **Answered 2026-11-16: No** (default kept). | No |
| WQ7 | When a run must follow and has no valid route: refuse the move, or detach that run? | **Answered 2026-11-16: Refuse.** Revisit with WB7's number (W4.3). | Refuse, and revisit with WB7's number |
| WQ8 | Should this documentation change include implementation or other supporting artifacts? | **Answered 2026-11-16: No** (default kept). | No |

## 13. Changes to the canvas plan and roadmap

**Applied 2026-11-16 by W0.3** (commit: see PR; [canvas-plan.md](canvas-plan.md) and
[canvas-roadmap.md](canvas-roadmap.md) are now the source of truth for these items).

| Where | Change | Status |
| --- | --- | --- |
| canvas-plan.md Q6 | Answer recorded; points to WD3. | Applied (2026-11-16) |
| canvas-plan.md section 9 | `Move`, `Reorient`, `Delete` and `Snap` as in section 10 here; `Doc::snap` returns the class (WD5). | Applied (2026-11-16) |
| canvas-plan.md INV2, T6.1 | The merge rule keeps a point that carries a label (WD4, WD9). | Applied (2026-11-16) |
| canvas-roadmap.md T4.2 | Read WD5; ties and body projection as stated there. | Applied as note/pointer (T4.2 `done`; code in W1.3, PR #41 open) |
| canvas-roadmap.md T4.3 | Rubber band per WD8: both modes, the default from WQ3. | Applied (2026-11-16) |
| canvas-roadmap.md T5.1 | `Move` and `Reorient` take a `FollowPolicy`; until W2.2 it is ignored and every run detaches. | Applied as note/pointer (T5.1 code merged; `FollowPolicy` deferred to W2.2) |
| canvas-roadmap.md T6.2 | First corner per WD6. | Applied (2026-11-16) |
| canvas-roadmap.md T6.3 | Replaced by wiring-roadmap.md W2.1 to W2.5. | Applied (2026-11-16) |
| canvas-roadmap.md T7.2 | Placing a label on a wire body splits the wire (WD9). | Applied (2026-11-16) |
| canvas-roadmap.md dependency overview | T6.3 line replaced by a pointer to the wiring roadmap. | Applied (2026-11-16) |
