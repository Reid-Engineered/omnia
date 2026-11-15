# Omnia canvas: roadmap for agents

The task list for building the canvas. The reference for every ID used here (`B`, `K`, `G`,
`D`, `INV`, `S`, `T-`, `Q`) is [canvas-plan.md](canvas-plan.md). Read section "How to read
this file" there first, then only the sections a task names.

## How to work a task

1. Pick a task whose status is `todo` and whose dependencies are all `done`.
2. Change its status to `doing` in this file.
3. Read what its **Read** line names. Do what **Do** says, inside the files **Owns** names.
4. Run the standard checks and the task's **Done when** checks.
5. Change the status to `done` and add one entry to the log at the end of this file.

Statuses: `todo`, `doing`, `done`, `blocked (reason)`, `needs-human`.

## Rules

1. **One task per branch and pull request.** Branch name `task/<id>-<slug>`, for example
   `task/T2.2-spatial-index`. Do not commit to `main`.
2. **Do not write GPUI code from memory.** Open the API at the pinned revision first (plan
   section 4 says where). If the source contradicts a `G` fact, stop and log it.
3. **Never make a check pass by weakening it.** Do not loosen a threshold, a tolerance, a
   budget, a property-test range or a lint. If a check fails and you cannot fix the cause,
   set the task to `blocked` and log what you measured.
4. **Report numbers with their hardware.** Write measurements to
   `docs/measurements/<task-id>.md` with CPU, GPU, resolution, scale factor and refresh rate.
   Only the reference machine (Q4) can mark a budget met. On any other machine a gate task
   ends as `needs-human`, with the numbers recorded.
5. **Open questions have defaults.** Use the default in plan section 12. Do not ask again and
   do not pick a different answer. Name the `Q` id in the pull request description.
6. **Stay in your lane.** Touch only the files the task owns, plus `Cargo.toml` entries the
   task needs. If you need a change elsewhere, log it and leave it for that file's task.
7. **No new dependency** outside plan section 10 without logging its licence first.
8. **Do not change the GPUI revision.** That is its own task (plan section 10).
9. **Every GUI run must end by itself.** Use `omnia-app --frames N` or a `bench` subcommand.
   Never start a window that waits for a person.
10. **Colours come from `omnia-theme`.** No colour literal in `omnia-canvas` (INV6).
11. **Stop at human checkpoints** (listed below). Do not work past one.
12. **Keep the plan true.** If a task changes a constant, decision or signature, update
    canvas-plan.md in the same pull request.

## Standard checks

Every task passes these before it is `done`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check          # from T0.7 on
```

`bench` below means `cargo run --release -p omnia-bench --`.

## Lanes

Tasks in different lanes own different files and can run at the same time.

| Lane | Owns | Needs GPUI |
| --- | --- | --- |
| `doc` | `crates/omnia-doc/` | No |
| `assets` | `crates/omnia-symbols/`, `crates/omnia-theme/`, `xtask/` | No |
| `canvas` | `crates/omnia-canvas/`, `crates/omnia-app/` | Yes |
| `bench` | `crates/omnia-bench/`, `.github/`, `tests/golden/`, `docs/` | Yes |

## Milestones

Each is about a week and ends in a gate task. Pan, zoom and grid reach budget on the
10,000-part sheet in milestone 2, before any editing exists.

| # | Delivers | Gate task | Budgets gated |
| --- | --- | --- | --- |
| 0 | Workspace, bench harness, spike, rendering decision | T0.6 | Spike thresholds |
| 1 | Empty sheet: camera, grid, pan, zoom, status bar | T1.6 | B1, B3 (empty), B8, B10 |
| 2 | Read-only stress sheets: symbols, wires, dots, LOD | T2.7 | B2, B3, B4, B5, B9, B10 |
| 3 | Text and golden images | T3.4 | B3 (text), B4, goldens |
| 4 | Hit testing, hover, selection, nets, view animations | T4.6 | B7, INV10, INV11 |
| 5 | Commands, undo, place, move, rotate, mirror, delete | T5.4 | B6, INV4, INV5 |
| 6 | Wiring | T6.4 | B6 (wire), INV2, INV9 |
| 7 | Labels, copy and paste, instrument stub, scale changes, final report | T7.5 | All |

## Dependency overview

```
T0.0 -> T0.1 -+-> T0.2 (assets)
              +-> T0.3 -> T0.4 -> T0.5 -> T0.6 [H1] -+
              +-> T0.7                               |
              +-> T1.1 (doc) --+-> T2.1 -> T2.2      |
              +-> T1.2 (assets)|                     |
              +-> T1.3 --------+-> T1.4 -> T1.5 -> T1.6
T0.6 + T1.6 + T2.1 + T2.2 + T2.3 + T2.4 -> T2.5 -> T2.6 -> T2.7
T2.7 -> T3.1 -+-> T3.3 [H2] -> T3.4
T2.5 -> T3.2 -+
T2.2 -> T4.1, T4.2 (doc)    T3.4 + T4.2 -> T4.3 -> T4.4, T4.5 -> T4.6
T4.1 -> T5.1 (doc)          T4.6 + T5.1 -> T5.2 -> T5.3 -> T5.4
T5.1 -> T6.1 (doc)          T5.4 + T6.1 -> T6.2 -> T6.4      T6.3 blocked (Q6)
T6.4 -> T7.1, T7.2, T7.3, T7.4 -> T7.5 [H4]
```

The `doc` lane (T1.1, T2.1, T2.2, T4.1, T4.2, T5.1, T6.1) has no GPUI in it and can run ahead
of the `canvas` lane.

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| H1 | After T0.6 | Reads `docs/spike-results.md`. If the outcome is not Plan A, decides how to proceed before any milestone 1 or 2 rendering task starts. |
| H2 | During T3.3 | Approves the first golden images against the mockup. |
| H3 | During T0.5 | Performs the touchpad and mouse gestures for S7 while the logger runs. |
| H4 | After T7.5 | Reads the final budget report. |
| — | Any time | Answers Q1 to Q7; names the reference machine (Q4). |

---

## Milestone 0: scaffold and spike

### T0.0 Sync the checkout · done
- Lane: any · Depends: none
- Do: the local checkout has no commits; `origin/main` has the design files and `vendor/`.
  Bring the working tree to `origin/main`, keeping the untracked `canvas-plan.md` and
  `canvas-roadmap.md`.
- Done when: `vendor/electronic-symbols/SVG/` holds 116 SVGs and `omnia.figcraft` is present.

### T0.1 Workspace scaffold · done
- Lane: canvas · Depends: T0.0 · Read: plan sections 4, 9, 10
- Owns: root `Cargo.toml`, `rust-toolchain.toml`, a stub `lib.rs` or `main.rs` per crate
- Do: create the workspace and the six crates plus `xtask`. Pin `gpui` and `gpui_platform`
  as in plan section 10. `omnia-app` opens one window with a `stage/paper`-coloured
  rectangle and supports `--frames N`. Add a test that fails if `omnia-doc` or
  `omnia-symbols` gains a GPUI dependency (INV12).
- Done when: `cargo build --workspace --release` succeeds; `cargo run -p omnia-app -- --frames 60`
  exits 0; the build time and any build prerequisites found are logged.

### T0.2 Fonts and vendored assets · todo
- Lane: assets · Depends: T0.1
- Owns: `vendor/fonts/`, `THIRD_PARTY.md`
- Do: add Inter and Source Code Pro (OFL 1.1) with their licence files; load them at start
  the way `crates/gpui/examples/example_support/fonts.rs` does at the pinned revision.
- Done when: `omnia-app --frames 60` draws a line of each font (checked with
  `render_to_image` in a test, G12); `THIRD_PARTY.md` lists both fonts.

### T0.3 Bench harness · done
- Lane: bench · Depends: T0.1 · Read: plan sections 2, 9 (bench command line), G13
- Owns: `crates/omnia-bench/`
- Do: `CountingAlloc` as the global allocator with a scope that separates "ours" from
  "total"; frame statistics from `frame_duration_snapshot()`; the `run`, `start`, `idle`
  and `compare` subcommands with `--json`; a scripted camera driver that moves one device
  pixel per frame. Use a placeholder scene until T2.5.
- Done when: `bench idle --seconds 10` reports 0 frames for the T0.1 window;
  `bench compare a.json a.json --max-regression 10` exits 0 and exits non-zero on a
  doctored file; ten repeated `bench run` results and their spread are in
  `docs/measurements/T0.3.md`.

### T0.4 Spike scenes S1 to S6 and S8 · todo
- Lane: bench · Depends: T0.3 · Read: plan section 8, G1 to G8, G12, G14, G15
- Owns: `crates/omnia-bench/src/spike/`
- Do: implement `bench spike s1` to `s6` and `s8` exactly as the S table describes, each
  painted from a custom `Element` inside `paint_layer`. S3 includes the pixel comparison
  against a resvg reference and the atlas-size readout. S8 includes `render_to_image`.
- Done when: every scene runs unattended at every listed size and writes its JSON; raw
  results are in `docs/measurements/T0.4.md`.

### T0.5 Spike S7: real input log · needs-human (H3)
- Lane: bench · Depends: T0.3 · Read: G9, G10
- Owns: `crates/omnia-bench/src/spike/s7.rs`
- Do: `bench spike s7 --seconds 60` opens a window, logs every `ScrollEvent`, `PinchEvent`,
  mouse move and their timestamps and phases to a file, then exits. The owner performs:
  wheel up and down, two-finger pan, pinch in and out, pan then pinch in one gesture, a
  fast mouse drag. Then summarise the log.
- Done when: `docs/measurements/T0.5.md` states, from the log, the delta kind for each
  device, events per frame, the phase sequence of a pinch, and dirty-to-present p99 during
  the drag.

### T0.6 Spike results and rendering decision · todo → H1
- Lane: bench · Depends: T0.4, T0.5 · Read: plan section 8 (thresholds and outcome tables)
- Owns: `docs/spike-results.md`
- Do: tabulate each `T-` threshold against the measured value, state pass or fail, and name
  the outcome row. Record the answers to the four "not settled by reading" items in plan
  section 4. Update D4's status in the plan.
- Done when: the file exists with hardware, numbers and one outcome row. If the outcome is
  Plan A on the reference machine, set D4 to DECIDED. Otherwise set this task to
  `needs-human` and stop: no rendering task may start.

### T0.7 Continuous integration · todo
- Lane: bench · Depends: T0.1 · Read: plan section 10
- Owns: `.github/workflows/`, `deny.toml`
- Do: a Windows workflow running the standard checks; `cargo deny` with the allow-list from
  plan section 10; cache `~/.cargo` so the Zed git dependency is not cloned on every run.
- Done when: the workflow passes on a pull request; any transitive dependency outside the
  allow-list is listed in the log with its licence, not silently allowed.

## Milestone 1: empty sheet

### T1.1 Geometry · todo
- Lane: doc · Depends: T0.1 · Read: D1, K1, K2
- Owns: `crates/omnia-doc/src/geom.rs`
- Do: `Pt`, `Rect`, `Orient` (the 8 orientations, composition, inverse), `snap_to`, and
  checked constructors that reject coordinates outside K1.
- Done when: property tests pass for: `Orient` forms a group (identity, inverse, closure);
  `snap_to` is idempotent (INV9) and never moves a point more than half a step; no
  operation on in-range points overflows.

### T1.2 Theme tokens · todo
- Lane: assets · Depends: T0.1 · Read: plan section 9 (Tokens), Q2
- Owns: `crates/omnia-theme/`, `xtask/src/tokens.rs`
- Do: `cargo xtask tokens` reads the `Theme` collection from `omnia.figcraft` and the
  overlay `canvas-tokens.toml` with the Q2 defaults, and writes `generated.rs` with
  `GRAPHITE` and `GRAY`. Add the INV6 test: fail if any file under `crates/omnia-canvas/src`
  contains a hex colour or a call to `rgb`, `rgba` or `hsla`.
- Done when: `generated.rs` has all 22 colour tokens from the file plus the 13 overlay
  tokens, in both themes; running the xtask twice gives no diff; the INV6 test passes and
  fails when a literal is planted.

### T1.3 Camera · todo
- Lane: canvas · Depends: T1.1 · Read: D1, K3, K4, INV8
- Owns: `crates/omnia-canvas/src/camera.rs`
- Do: `Camera` as sketched in plan section 9, with zoom clamped to K4.
- Done when: tests pass for: after `zoom_about(anchor, f)` the world point under `anchor` is
  unchanged to within 1 nm; `screen_to_world(world_to_screen(p))` returns `p` at 100% and
  above, at the sheet limits and at both ends of the zoom range; a pan of d pixels moves
  every on-screen point by exactly d.

### T1.4 Sheet element: paper and grid · todo
- Lane: canvas · Depends: T1.2, T1.3, T0.6 (Plan A confirmed) · Read: D4, D5, K6, K10, G3, G5, G8
- Owns: `crates/omnia-canvas/src/sheet.rs`, `crates/omnia-canvas/src/snap_px.rs`
- Do: `SheetView` as a type implementing `Element` directly. Paint the paper and the
  nested, fading dot grid inside `paint_layer`. Put the device-pixel rounding helpers in
  `snap_px.rs`; every later task uses them.
- Done when: a `render_to_image` test shows every dot covering whole device pixels at scale
  factors 1.0, 1.25 and 1.5; on-screen pitch stays in [20, 40) px across the zoom range;
  the paint makes zero allocations after the first frame.

### T1.5 Pan, zoom and status bar · todo
- Lane: canvas · Depends: T1.4 · Read: G9, G10, K4, T0.5 results
- Owns: `crates/omnia-canvas/src/input.rs`, `crates/omnia-app/src/status_bar.rs`
- Do: wheel zoom anchored on the cursor; touchpad pixel-scroll pan; pinch zoom about the
  pinch position; middle-drag and space-drag pan. The status bar shows cursor position in
  mm, grid size and zoom, as its own cached view fed by `CanvasEvent::Status`.
- Done when: simulated-input tests show the world point under the cursor fixed across a
  zoom, and a status update notifies neither `SheetView` nor the window root.

### T1.6 Gate: milestone 1 · todo
- Lane: bench · Depends: T1.5
- Do: run and record:

  | Budget | Command | Pass |
  | --- | --- | --- |
  | B1 | `bench start` | < 400 ms |
  | B3 | `bench run --sheet empty --script pan` and `zoom` | p99 < 8 ms |
  | B8 | `bench idle --seconds 10` | 0 frames |
  | B10 | same `run` reports | `allocs_per_frame.ours == 0` |

- Done when: all four pass on the reference machine. On another machine: record and set
  `needs-human`.

## Milestone 2: read-only stress sheets

### T2.1 Document store · todo
- Lane: doc · Depends: T1.1 · Read: D2, INV1, INV3
- Owns: `crates/omnia-doc/src/arena.rs`, `doc.rs`, `points.rs`
- Do: `Arena<T>` with `insert`, `remove`, `restore`, `get` and stable ids; `Part`, `Wire`,
  `Label`; `Doc` with crate-private insert and remove functions (commands come in T5.1);
  the point map of wire ends and pins; `junctions()`; the revision counter.
- Done when: tests pass for: `restore` after `remove` returns the same id; INV1 is enforced
  at insert; INV3 holds on generated sheets against a brute-force count.

### T2.2 Spatial index · todo
- Lane: doc · Depends: T2.1 · Read: D3, K12
- Owns: `crates/omnia-doc/src/index.rs`
- Do: the uniform grid with insert, remove and `query(area, out)`; long wires registered in
  every cell they cross; deduplication by generation stamp.
- Done when: a property test shows `query` equal to a brute-force scan for random items and
  rectangles; `query` allocates nothing when `out` has capacity; a criterion bench records
  the time to query a 4K viewport on the 10,000-part sheet.

### T2.3 Symbol compiler · todo
- Lane: assets · Depends: T0.1, T1.1 · Read: plan section 9 (Symbols), K2, K7
- Owns: `crates/omnia-symbols/`, `xtask/src/symbols.rs`
- Do: `cargo xtask symbols` turns the SVGs named in `pins.toml` into `SymbolDef`s. Write
  `pins.toml` for the symbols the mockup uses: `Resistor-IEEE-Standard`,
  `Resistor-IEEE-Potentiometer`, `Capacitor-IEEE-NonPolarized`, `Diode-COM-Standard`,
  `IC-COM-OpAmp`, `Ground-COM-General`. Implement `SymbolArt::write_svg`.
- Done when: the xtask fails on a pin off the 1.27 mm lattice; for each symbol, the SVG from
  `write_svg` at the original stroke matches the vendor SVG when both are rasterised with
  resvg (difference under 1% of pixels); pin positions match `pinAt` in `symbols.mjs`.

### T2.4 Stress sheets, save and load · todo
- Lane: bench (generator), doc (save and load) · Depends: T2.1, T2.3
- Owns: `crates/omnia-bench/src/generate.rs`, `crates/omnia-doc/src/io.rs`
- Do: seeded generators for the five `StressSheet`s; `10k` has exactly 10,000 parts and
  20,000 wires. `Doc::save` and `Doc::load` in RON.
- Done when: the same seed gives the same sheet; `load(save(doc)) == doc`; `10k` generates
  in under 2 s.

### T2.5 Sheet rendering · todo
- Lane: canvas · Depends: T0.6, T1.6, T2.2, T2.3, T2.4 · Read: D4, D5, K5, K6, K9, K11, G3 to G5, INV7
- Owns: `crates/omnia-canvas/src/sheet.rs`, `sheet/wires.rs`, `sheet/symbols.rs`, `sheet/lod.rs`
- Do: cull with `Doc::query`; wires as device-pixel-snapped quads; symbols as `paint_svg`
  sprites with prebuilt keys per (symbol, size bucket) and orientation in the matrix;
  junction dots; the LOD ladder; reusable scratch buffers. Use the outcome variant chosen
  in T0.6 if it was not plain Plan A.
- Done when: a `render_to_image` test at 1.0, 1.25 and 1.5 shows every wire a whole number
  of device pixels wide with constant width along its length; the INV7 test passes (no
  `paint_path` or `canvas(` in `sheet*`; zero allocations per frame once warm on `1k`).

### T2.6 Bench scripts and CI gate · todo
- Lane: bench · Depends: T2.5 · Read: plan section 11 (CI noise)
- Owns: `crates/omnia-bench/src/scripts.rs`, `.github/workflows/bench.yml`, `bench-baseline/`
- Do: `pan` and `zoom` scripts over real sheets; primitive counts by kind in the report. In
  hosted CI, compare the deterministic numbers (primitive counts, `allocs_per_frame.ours`)
  to a committed baseline with `bench compare`. Add a manually triggered job for the
  frame-time comparison on the reference machine.
- Done when: CI fails on a pull request that adds one allocation per frame or 11% more
  primitives, and passes on a no-op pull request.

### T2.7 Gate: milestone 2 · todo
- Lane: bench · Depends: T2.6
- Do: run and record:

  | Budget | Command | Pass |
  | --- | --- | --- |
  | B2 | `bench start --sheet 1k` minus `bench start` | < 300 ms |
  | B3, B5 | `bench run --sheet 1k --script pan` and `zoom` | p99 < 8 ms, max ≤ 16.7 ms |
  | B4 | `bench run --sheet 10k --script pan` and `zoom` | p99 < 16 ms |
  | B9 | `memory_mb` in the `10k` run | < 300 |
  | B10 | all reports | `allocs_per_frame.ours == 0` |

- Done when: all pass on the reference machine at 125% and 150%. On another machine:
  record and set `needs-human`. If B4 misses, apply the "fails at 70k" row of the outcome
  table before reporting.

## Milestone 3: text and goldens

### T3.1 Labels · todo
- Lane: canvas · Depends: T2.7 · Read: D6, K8, K9, K11, G14, G15
- Owns: `crates/omnia-canvas/src/sheet/text.rs`
- Do: designators, values, net labels and pin numbers. One cached `ShapedLine` per label
  and size bucket, reshaped only when the bucket changes. Hide below the K9 threshold.
- Done when: zero allocations per frame once warm on `text` at a fixed zoom; a zoom sweep
  run twice leaves the atlas the same size after the second pass as after the first.

### T3.2 Golden-image harness · todo
- Lane: bench · Depends: T2.5 · Read: G12, T0.4's S8 result
- Owns: `tests/golden/`, `xtask/src/golden.rs`
- Do: a test that loads a sheet, renders it with `render_to_image` in both themes at scale
  factors 1.0, 1.25 and 1.5, and compares with a committed PNG: at most 0.1% of pixels may
  differ by more than 2/255 per channel. `cargo xtask golden --update` rewrites the PNGs.
  If S8 showed `render_to_image` fails without a GPU, mark the test to run only on the
  reference machine and log that.
- Done when: the test passes on unchanged code and fails when a wire is moved one pixel.

### T3.3 Wien oscillator fixture · todo → H2
- Lane: bench · Depends: T3.1, T3.2 · Read: `build-omnia.mjs` lines 159 to 234
- Owns: `tests/golden/wien-oscillator.ron` and its PNGs
- Do: rebuild the mockup's sheet as a fixture from the coordinates in `build-omnia.mjs`
  (20 px there is one grid step). Leave out the floating oscilloscope window, the scope's
  on-sheet instrument symbol and the supply tags; they are not canvas items yet. Render at
  the mockup's 100% zoom and produce a side-by-side image with `workspace-graphite.png`
  cropped to the sheet.
- Done when: the side-by-side is in `docs/measurements/T3.3.md` with a list of every visible
  difference. Set `needs-human`; after the owner approves, commit the renders as goldens
  and set `done`.

### T3.4 Gate: milestone 3 · todo
- Lane: bench · Depends: T3.3
- Do: `bench run --sheet text --script pan` and `zoom`: p99 < 8 ms (B3). Re-run the whole
  T2.7 table with text on. Run the golden tests.
- Done when: all pass on the reference machine; otherwise record and set `needs-human`.

## Milestone 4: selection and nets

### T4.1 Connectivity · todo
- Lane: doc · Depends: T2.2 · Read: D2, INV10
- Owns: `crates/omnia-doc/src/connectivity.rs`
- Do: components over the point map, grown by union on add and repaired by re-flooding the
  touched component on remove; `NetId` by net label; `net_of` and `net_wires`; a
  from-scratch rebuild used after load and as the test oracle.
- Done when: the INV10 property test passes over random add and remove sequences; a
  criterion bench shows one wire add and one wire remove under 1 ms on `10k`.

### T4.2 Hit testing and snapping · todo
- Lane: doc · Depends: T2.2, T2.3 · Read: D3, K14, INV9
- Owns: `crates/omnia-doc/src/hit.rs`, `snap.rs`
- Do: `hit_test` (nearest part, wire or label within tolerance, parts by their bounds) and
  `snap` (priority: pin, wire end, wire body, grid). Add `bench hit`.
- Done when: property tests show `hit_test` equal to brute force and INV9 for `snap`;
  `bench hit --sheet 10k` reports p99 < 0.2 ms (B7; reference machine for the gate).

### T4.3 Overlay, hover and selection · todo
- Lane: canvas · Depends: T3.4, T4.2 · Read: D7, INV11, G6
- Owns: `crates/omnia-canvas/src/overlay.rs`, `selection.rs`
- Do: `OverlayView`; hover outline; click and shift-click selection; rubber band (window
  selection of items fully inside). `SheetView` embedded with `.cached()`. Colours from
  `stage/select`, `stage/select-halo`, `stage/hover`.
- Done when: the INV11 test passes: a paint counter on `SheetView` does not move during 100
  simulated hover moves, a selection change and a rubber-band drag; goldens for selected
  and hovered states exist in both themes.

### T4.4 Net highlight · todo
- Lane: canvas · Depends: T4.1, T4.3 · Read: D7
- Owns: `crates/omnia-canvas/src/overlay/net.rs`
- Do: hovering or selecting a wire repaints its whole net in the overlay in
  `stage/net-highlight`.
- Done when: the `SheetView` paint counter does not move when the highlight changes; a
  golden shows a highlighted net.

### T4.5 View animations · todo
- Lane: canvas · Depends: T4.3 · Read: K13, G16
- Owns: `crates/omnia-canvas/src/camera_anim.rs`
- Do: fit-to-sheet and zoom-to-selection, 150 ms ease-out driven by
  `request_animation_frame`; any pan or zoom input cancels the animation from the current
  position.
- Done when: tests show the animation ends exactly on the target, input mid-way stops it
  with no jump, and `bench idle` still reports 0 frames one second after it ends.

### T4.6 Gate: milestone 4 · todo
- Lane: bench · Depends: T4.4, T4.5
- Do: `bench hit --sheet 10k` (B7 < 0.2 ms); `bench run --sheet 10k --script hover`
  (p99 < 16 ms, sheet paint count 0); the INV10 and INV11 tests; the T2.7 table again.
- Done when: all pass on the reference machine; otherwise record and set `needs-human`.

## Milestone 5: editing

### T5.1 Commands, undo and redo · todo
- Lane: doc · Depends: T4.1 · Read: D2, INV4
- Owns: `crates/omnia-doc/src/command.rs`, `undo.rs`
- Do: every `Command` variant except `Paste`; validation (K1 limits, INV1); inverse
  commands; the undo and redo stacks; `Change` with a correct dirty rectangle; index and
  connectivity updated inside `apply`.
- Done when: the INV4 property test passes over random command sequences, comparing
  documents including ids; INV10 still holds after every step; one `Move` of 100 items on
  `10k` applies in under 1 ms.

### T5.2 Tools and keys · todo
- Lane: canvas · Depends: T4.6, T5.1 · Read: plan section 5, INV5
- Owns: `crates/omnia-canvas/src/tool.rs`, `keymap.rs`
- Do: the tool state machine with states idle, hovering, dragging, placing, wiring; GPUI
  actions and key bindings for R, X, Y, W, Esc, Del, Ctrl+Z, Ctrl+Y. Check how actions and
  key contexts work in `crates/gpui/docs/key_dispatch.md` at the pinned revision.
- Done when: the INV5 test passes: the revision is unchanged through a simulated drag until
  mouse-up, one undo reverts the whole drag, and Esc mid-drag leaves the revision unchanged.

### T5.3 Place, move, rotate, mirror, delete · todo
- Lane: canvas · Depends: T5.2 · Read: D7, K2
- Owns: `crates/omnia-canvas/src/tool/place.rs`, `tool/move_.rs`, `overlay/ghosts.rs`
- Do: place a part from `Tool::Place`; drag-move a selection with ghosts in the overlay and
  the dragged items hidden from the sheet; R, X and Y apply to the ghosts during a drag
  and as a command otherwise; Del deletes. Positions snap to the grid by the part's anchor
  pin.
- Done when: simulated-input tests cover place, move, rotate during drag, mirror, delete
  and undo of each; `SheetView` paints exactly twice per drag (start and end).

### T5.4 Gate: milestone 5 · todo
- Lane: bench · Depends: T5.3
- Do: add the `drag-part` script. `bench run --sheet 1k --script drag-part` and
  `--sheet 10k`: dirty-to-present p99 ≤ one refresh interval (B6) and frame p99 within B3
  and B4. Re-run the T2.7 table.
- Done when: all pass on the reference machine; otherwise record and set `needs-human`.

## Milestone 6: wiring

### T6.1 Wire routing rules · todo
- Lane: doc · Depends: T5.1 · Read: D2, INV2
- Owns: `crates/omnia-doc/src/route.rs`
- Do: the L-route between two points with a `flip` flag for the corner; `AddWires` splits
  any wire whose interior an added end lands on, and splits the added wire where it passes
  over an existing end or pin; segments that end up collinear and joined with no third end
  between them are merged.
- Done when: property tests pass for INV2 (two wires crossing at interiors are in different
  nets; a wire ending on another's body joins its net) and INV10; undo restores the
  pre-split wires with their ids (INV4).

### T6.2 Wire tool · todo
- Lane: canvas · Depends: T5.4, T6.1 · Read: K14, D7
- Owns: `crates/omnia-canvas/src/tool/wire.rs`, `overlay/snap_marker.rs`
- Do: W starts wiring; an L-shaped preview follows the cursor; Space flips the corner; the
  snap target (pin, wire end, wire body) is marked in `stage/snap` before the click; click
  commits the segment and continues from its end; a click on a pin or wire ends the wire;
  Esc cancels.
- Done when: simulated-input tests wire two resistors pin to pin and get one net; the
  preview costs no `SheetView` paint; goldens show the preview and each snap marker kind.

### T6.3 Wires follow moved parts · blocked (Q6)
- Lane: doc, canvas · Depends: T6.2
- Do: not specified until Q6 is answered "yes". Do not start.

### T6.4 Gate: milestone 6 · todo
- Lane: bench · Depends: T6.2
- Do: add the `drag-wire` script. `bench run --sheet 1k --script drag-wire` and
  `--sheet dense`: B6 and B3. The INV2, INV9 and INV10 property tests at 10× the default
  case count. Re-run the T2.7 table.
- Done when: all pass on the reference machine; otherwise record and set `needs-human`.

## Milestone 7: finish

### T7.1 Copy and paste · todo
- Lane: doc, canvas · Depends: T6.4
- Owns: `crates/omnia-doc/src/clip.rs`, `crates/omnia-canvas/src/tool/paste.rs`
- Do: `Doc::copy`, the `Paste` command, Ctrl+C, Ctrl+X, Ctrl+V. A paste is placed with
  ghosts like a move. Designators of pasted parts are made unique.
- Done when: paste then undo satisfies INV4; a pasted group has the same internal
  connectivity as its source and no connection to it.

### T7.2 Net labels · todo
- Lane: canvas · Depends: T6.4 · Read: D2
- Owns: `crates/omnia-canvas/src/tool/label.rs`
- Do: `Tool::Label` places a label on a wire or pin; text entry through GPUI's input
  handling (read `crates/gpui/examples/input.rs` at the pinned revision first); editing an
  existing label is one `SetText` command.
- Done when: two unconnected wires given the same label share a `NetId`, and separate again
  when one label is deleted (INV10 holds).

### T7.3 Instrument window stub · todo
- Lane: canvas · Depends: T6.4 · Read: plan section 6 (last paragraph), G6
- Owns: `crates/omnia-canvas/src/instrument_host.rs`
- Do: a draggable floating view above the sheet, styled from `bg/card`, `rule` and `shadow`,
  whose content is an animated trace notifying itself at 60 Hz. Add
  `bench run --script pan --instrument`.
- Done when: `SheetView`'s paint count stays 0 while only the stub animates;
  `bench run --sheet 10k --script pan --instrument` gives p99 < 16 ms on the reference
  machine (otherwise record; then measure again at 30 Hz).

### T7.4 Scale-factor changes · todo
- Lane: canvas · Depends: T6.4 · Read: plan section 11, T0.4's S8 result
- Owns: `crates/omnia-canvas/src/sheet.rs` (cache keys only)
- Do: when the window's scale factor changes, drop the size-keyed caches and repaint.
- Done when: a test that changes the scale factor from 1.25 to 1.5 mid-run renders an image
  identical to a fresh window at 1.5.

### T7.5 Gate: final report · todo → H4
- Lane: bench · Depends: T7.1, T7.2, T7.3, T7.4
- Owns: `docs/canvas-report.md`
- Do: measure B1 to B10 on the reference machine at 125% and 150%, plus every invariant
  test and the goldens. Write one table: budget, measured value, pass or miss.
- Done when: the report exists. A miss is reported as a miss, with the measurement.

---

## Log

Append one entry per finished or blocked task. Newest last.

```
<date> <task id> <status> — what was done; numbers with hardware; deviations from the plan;
                             anything the next task needs to know.
```

2026-11-14 plan — canvas-plan.md and canvas-roadmap.md written. GPUI facts G1 to G16 read
from source at 15d136bf. Nothing measured; D4 is a hypothesis until T0.6.
2026-11-14 T0.0 done — local main reset to origin/main (172be3c); canvas-plan.md and
                       canvas-roadmap.md kept untracked. vendor/electronic-symbols/SVG/ has 116
                       SVGs; omnia.figcraft is at the repo root.
2026-11-14 T0.1 done — created the resolver-3, edition-2024 workspace with omnia-doc,
                       omnia-symbols, omnia-theme, omnia-canvas, omnia-bench, omnia-app and
                       xtask; Rust pinned to 1.98.1. Cargo.lock records the resolved graph.
                       gpui and gpui_platform are pinned only in workspace.dependencies to
                       15d136bf8e72ae43b284183ec46940c009460f5c. Read hello_world.rs,
                       gpui_platform.rs, window.rs, app.rs and example_support/fonts.rs at
                       that pin before using their APIs; no conflicting G facts found.
                       omnia-app opens one paper rectangle. Its temporary #EFEDE6 constant
                       matches Theme/stage/paper in both omnia.figcraft modes and stays in
                       omnia-app only until T1.2 replaces it with omnia-theme tokens.
                       --frames N requests frames only for the bounded run, then quits on
                       the next-frame callback after the final presentation; ordinary idle
                       mode schedules no continuous callbacks. Invalid limits exit 2.
                       INV12 in xtask checks resolved names, all targets/features and normal,
                       build/dev/transitive edges. Negative tests rejected an optional,
                       renamed Windows-only GPUI dependency in omnia-doc and a transitive
                       dependency via omnia-canvas in omnia-symbols; plants were restored.
                       Checks passed: cargo build --workspace --release; cargo run -p
                       omnia-app -- --frames 60 (exit 0); cargo fmt --all --check;
                       cargo clippy --workspace --all-targets -- -D warnings;
                       cargo test --workspace (INV12 passed); git diff --check.
                       Release build after prerequisites: 150.47 s (Cargo: 2m 30s), dependency
                       fetch excluded, partially warm cache from failed prerequisites.
                       First attempt: 127.18 s including fetch/cache wait, failed without
                       link.exe. Intermediate attempts during setup failed for missing SDK
                       libraries; no check or build profile was weakened.
                       Host: Windows 11 x64, Intel Core i7-14650HX, NVIDIA RTX 5060 Laptop
                       GPU + Intel UHD Graphics; displays 1920x1080 and 1920x1200, both 60 Hz;
                       system DPI 96 (100%). Q4: no reference machine named, no budget gated.
                       Prerequisites installed: VS 2022 Build Tools 17.14.37906.3, VCTools
                       workload with recommended components (MSVC 14.44.35207), Windows SDK
                       10.0.26100.0; x64 fxc.exe file version 10.0.26100.9457. Release GPUI
                       compiles HLSL with fxc (PATH, GPUI_FXC_PATH, or SDK registry lookup).
                       Setup completed successfully; plain cargo commands then worked.
                       T0.2: bundle Inter/Source Code Pro bytes and explicitly call
                       cx.text_system().add_fonts(Vec<Cow<'static, [u8]>>) before opening the
                       window. The pinned example helper only loads fonts on wasm; its
                       native branch is a no-op. Wire other bundled assets with
                       gpui_platform::application().with_assets(AssetSource) before run;
                       none are needed for the solid rectangle. T1.2 owns token generation.
                       T0.3: omnia-bench is an empty binary; enable GPUI profiler/test-support
                       where needed, preserve callback-free idle mode. No fonts are bundled
                       yet. T0.7: cache Cargo git/registry data and run cargo fetch --locked
                       before the offline all-target INV12 check; audit transitive licences
                       with cargo-deny (not required in T0.1). No new direct dependency
                       outside plan section 10. Measurements are kept in this task log to
                       stay within T0.1's owned files; no docs/ lane files were changed.
2026-11-14 T0.3 done — omnia-bench is a lib plus the `bench` binary: CountingAlloc (global
                       allocator; `alloc::ours` / `alloc::not_ours` scopes, count_allocs),
                       frame stats from Window::frame_duration_snapshot (draw, dirty-to-present,
                       present interval, and per-frame work as frame_ms), `run`, `start`,
                       `idle`, `compare` with --json, a pan driver moving one device pixel per
                       frame, and a placeholder scene (paper, K3 dot grid, 2,000 seeded wire
                       quads) painted from a custom Element inside one paint_layer. Measured on
                       i7-14650HX, RTX 5060 Laptop + UHD (adapter in use unknown), 125%
                       (scale 1.25), 60 Hz, plugged in; not the reference machine (Q4), no
                       budget marked. idle --seconds 10 on the T0.1 window: 0 frames, CPU
                       0.3-1.1% when nothing touches it; every non-zero frame seen was a focus
                       change, which the report's per-render log shows (window_active flips).
                       Ten `bench run` (600 frames, 1600x1000 device): frame_ms p99 0.76-1.19
                       (median 0.93), draw p99 0.74-1.11, dirty-to-present p99 1.43-2.80,
                       allocs_per_frame.ours 0 in every frame, total about 8 per frame (GPUI);
                       timing p50 bimodal (0.29-0.40 or about 0.76), spread 71% at p50, 40-46%
                       at p99. Two real runs differ beyond 10% on 25 of 50 metrics, so gate
                       timings only on the reference machine. start --runs 10: median 299 ms,
                       max 500 ms. compare: a.json against itself exits 0; a doctored file
                       (+11% draw p99, ours 0 to 1, a missing metric) exits 1; mismatched
                       kind or config exits 2. Details in docs/measurements/T0.3.md.
                       Deviations: per the PR #1 review, gpui is built with `profiler` only;
                       Window::rendered_scene (primitive counts) needs GPUI's bench-support
                       (criterion, bench_metrics), so it is behind the opt-in omnia-bench
                       feature `scene-counts` and reports record `config.scene_counts`. Plan
                       section 9 updated to say so. serde_json (MIT or Apache-2.0, already in
                       GPUI's tree) is now a bench dependency for JSON reports; plan section 10
                       updated. hdrhistogram 7 matches GPUI's. Run windows disable GPUI's
                       inactive/low-power/thermal frame caps. Windows' foreground lock can
                       leave a run window inactive, and GPUI then records no present
                       intervals, so present_interval_ms is missing and the report warns.
                       T0.4: implement scene::BenchScene and add a `spike` arm to main.rs;
                       paint runs inside alloc::ours, wrap GPUI calls in alloc::not_ours;
                       use --features scene-counts for primitive counts and record it.
                       T2.6: owns scripts.rs (the driver lives in driver.rs); gate in hosted CI
                       with `compare --metrics allocs_per_frame.ours,primitives`.
