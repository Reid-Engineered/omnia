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
   Only the reference machine (Q4: the Orion laptop on Linux, under plan section 2.1) can mark
   a budget met. On any other machine, or on it outside the section 2.1 protocol, a gate task
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
| `agent` | `crates/omnia-agent/`, `crates/omnia-cli/`, `docs/agent/`, `mcp-plan.md`, `mcp-roadmap.md` ([mcp-roadmap.md](mcp-roadmap.md)) | No |

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
              +-> T0.3 -> T0.4 -> T0.5 -> T0.8 -> T0.6 [H1] -+
              +-> T0.7                                       |
              +-> T1.1 (doc) --+-> T2.1 -> T2.2      |
              +-> T1.2 (assets)|                     |
              +-> T1.3 --------+-> T1.4 -> T1.5 -> T1.6
T0.6 + T1.6 + T2.1 + T2.2 + T2.3 + T2.4 + T2.8 -> T2.5 -> T2.6 -> T2.7
T2.7 -> T3.1 -+-> T3.3 [H2] -> T3.4
T2.7 -> T2.9 (removes the B4 known limit)
T2.5 -> T3.2 -+
T2.2 -> T4.1, T4.2 (doc)    T3.4 + T4.2 -> T4.3 -> T4.4, T4.5 -> T4.6
T2.2 + T2.4 + T4.1 + T4.2 -> T2.8 (doc) -> T2.5
T4.1 -> T5.1 (doc)          T4.6 + T5.1 -> T5.2 -> T5.3 -> T5.4
T5.1 -> T6.1 (doc)          T5.4 + T6.1 -> T6.2 -> T6.4      T6.3 -> wiring-roadmap.md W2.1..W2.5
T6.4 -> T7.1, T7.2, T7.3, T7.4 -> T7.5 [H4]
```

The `doc` lane (T1.1, T2.1, T2.2, T2.8, T4.1, T4.2, T5.1, T6.1) has no GPUI in it and can run ahead
of the `canvas` lane.

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| H1 | After T0.6 | Reads `docs/spike-results.md`. If the outcome is not Plan A, decides how to proceed before any milestone 1 or 2 rendering task starts. |
| H2 | During T3.3 | Approves the first golden images against the mockup. |
| H3 | During T0.5 | Performs the touchpad and mouse gestures for S7 while the logger runs. |
| H4 | After T7.5 | Reads the final budget report. |
| — | Any time | Answers Q1 to Q7 (Q4 answered 2026-11-15). |

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

### T0.2 Fonts and vendored assets · done
- Lane: assets · Depends: T0.1
- Owns: `vendor/fonts/`, `THIRD_PARTY.md`, `crates/omnia-app/` (font loading and lib split)
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

### T0.4 Spike scenes S1 to S6 and S8 · done
- Lane: bench · Depends: T0.3 · Read: plan section 8, G1 to G8, G12, G14, G15
- Owns: `crates/omnia-bench/src/spike/`
- Do: implement `bench spike s1` to `s6` and `s8` exactly as the S table describes, each
  painted from a custom `Element` inside `paint_layer`. S3 includes the pixel comparison
  against a resvg reference and the atlas-size readout. S8 includes `render_to_image`.
- Done when: every scene runs unattended at every listed size and writes its JSON; raw
  results are in `docs/measurements/T0.4.md`.

### T0.5 Spike S7: real input log · done
- Lane: bench · Depends: T0.3 · Read: G9, G10
- Owns: `crates/omnia-bench/src/spike/s7.rs`
- Do: `bench spike s7 --seconds 60` opens a window, logs every `ScrollEvent`, `PinchEvent`,
  mouse move and their timestamps and phases to a file, then exits. The owner performs:
  wheel up and down, two-finger pan, pinch in and out, pan then pinch in one gesture, a
  fast mouse drag. Then summarise the log.
- Done when: `docs/measurements/T0.5.md` states, from the log, the delta kind for each
  device, events per frame, the phase sequence of a pinch, and dirty-to-present p99 during
  the drag.

### T0.6 Spike results and rendering decision · done → H1
- Lane: bench · Depends: T0.4, T0.5, T0.8 · Read: plan section 8 (thresholds and outcome tables)
- Owns: `docs/spike-results.md`
- Do: tabulate each `T-` threshold against the measured value, state pass or fail, and name
  the outcome row. Record the answers to the four "not settled by reading" items in plan
  section 4. Update D4's status in the plan.
- Done when: the file exists with hardware, numbers and one outcome row. If the outcome is
  Plan A on the reference machine, set D4 to DECIDED. Otherwise set this task to
  `needs-human` and stop: no rendering task may start.

### T0.8 Reference machine bring-up (Linux) · done
- Lane: bench · Depends: T0.4 · Read: plan sections 2.1, 4 and 8
- Owns: `docs/measurements/T0.8.md`, `docs/measurements/T0.8/`, a POSIX copy of the spike
  measurement script, and the Linux side of `omnia-bench` features
- Do: on the Orion laptop, build the workspace at the pinned GPUI revision; confirm GPUI renders
  through the HD 620 Vulkan device, not llvmpipe; re-check the G facts that are Windows-specific
  (G10 input, G11 surfaces, G12 `render_to_image`, G13 frame stats) against GPUI's Linux and
  Vulkan code and log each difference; then run S1 to S6 and S8 under the section 2.1 protocol
  at 100% and 125%.
- Done when: the measurements file lists hardware, Vulkan device, power state and the S1 to S6
  and S8 numbers from the reference machine; T0.6 reads its thresholds from these, not T0.4's.

### T0.7 Continuous integration · done
- Lane: bench · Depends: T0.1 · Read: plan section 10
- Owns: `.github/workflows/`, `deny.toml`
- Do: a Windows workflow running the standard checks; `cargo deny` with the allow-list from
  plan section 10; cache `~/.cargo` so the Zed git dependency is not cloned on every run.
- Done when: the workflow passes on a pull request; any transitive dependency outside the
  allow-list is listed in the log with its licence, not silently allowed.

## Milestone 1: empty sheet

### T1.1 Geometry · done
- Lane: doc · Depends: T0.1 · Read: D1, K1, K2
- Owns: `crates/omnia-doc/src/geom.rs`
- Do: `Pt`, `Rect`, `Orient` (the 8 orientations, composition, inverse), `snap_to`, and
  checked constructors that reject coordinates outside K1.
- Done when: property tests pass for: `Orient` forms a group (identity, inverse, closure);
  `snap_to` is idempotent (INV9) and never moves a point more than half a step; no
  operation on in-range points overflows.

### T1.2 Theme tokens · done
- Lane: assets · Depends: T0.1 · Read: plan section 9 (Tokens), Q2
- Owns: `crates/omnia-theme/`, `xtask/src/tokens.rs`
- Do: `cargo xtask tokens` reads the `Theme` collection from `omnia.figcraft` and the
  overlay `canvas-tokens.toml` with the Q2 defaults, and writes `generated.rs` with
  `GRAPHITE` and `GRAY`. Add the INV6 test: fail if any file under `crates/omnia-canvas/src`
  contains a hex colour or a call to `rgb`, `rgba` or `hsla`.
- Done when: `generated.rs` has all 22 colour tokens from the file plus the 13 overlay
  tokens, in both themes; running the xtask twice gives no diff; the INV6 test passes and
  fails when a literal is planted.

### T1.3 Camera · done
- Lane: canvas · Depends: T1.1 · Read: D1, K3, K4, INV8
- Owns: `crates/omnia-canvas/src/camera.rs`
- Do: `Camera` as sketched in plan section 9, with zoom clamped to K4.
- Done when: tests pass for: after `zoom_about(anchor, f)` the world point under `anchor` is
  unchanged to within 1 nm; `screen_to_world(world_to_screen(p))` returns `p` at 100% and
  above, at the sheet limits and at both ends of the zoom range; a pan of d pixels moves
  every on-screen point by exactly d.

### T1.4 Sheet element: paper and grid · done
- Lane: canvas · Depends: T1.2, T1.3, T0.6 (Plan A confirmed) · Read: D4, D5, K6, K10, G3, G5, G8
- Owns: `crates/omnia-canvas/src/sheet.rs`, `crates/omnia-canvas/src/snap_px.rs`
- Do: `SheetView` as a type implementing `Element` directly. Paint the paper and the
  nested, fading dot grid inside `paint_layer`. Put the device-pixel rounding helpers in
  `snap_px.rs`; every later task uses them.
- Done when: a `render_to_image` test shows every dot covering whole device pixels at scale
  factors 1.0, 1.25 and 1.5; on-screen pitch stays in [20, 40) px across the zoom range;
  the paint makes zero allocations after the first frame.

### T1.5 Pan, zoom and status bar · done
- Lane: canvas · Depends: T1.4 · Read: G9, G10, K4, T0.5 results
- Owns: `crates/omnia-canvas/src/input.rs`, `crates/omnia-app/src/status_bar.rs`
- Do: wheel zoom anchored on the cursor; touchpad pixel-scroll pan; pinch zoom about the
  pinch position; middle-drag and space-drag pan. The status bar shows cursor position in
  mm, grid size and zoom, as its own cached view fed by `CanvasEvent::Status`.
- Done when: simulated-input tests show the world point under the cursor fixed across a
  zoom, and a status update notifies neither `SheetView` nor the window root.

### T1.6 Gate: milestone 1 · done
- Lane: bench · Depends: T1.5
- Do: run and record (`docs/measurements/T1.6/gate.sh`, then `summarize.py`):

  | Budget | Command | Pass |
  | --- | --- | --- |
  | B1 | `bench start` | < 400 ms |
  | B3 | `OMNIA_BENCH_TRACE=1 bench run --sheet empty --script pan` and `zoom` | p99 < 16.7 ms; 0 counted missed refreshes in every run: a present interval > 1.5 refreshes counts unless the trace shows it compositor-side (plan 2.1, owner decision 2026-11-17); a run without a usable trace fails |
  | B8 | `bench idle --seconds 10` | 0 frames |
  | B10 | same `run` reports | `allocs_per_frame.ours == 0` |

- Done when: all four pass on the reference machine. On another machine: record and set
  `needs-human`.

## Milestone 2: read-only stress sheets

### T2.1 Document store · done
- Lane: doc · Depends: T1.1 · Read: D2, INV1, INV3
- Owns: `crates/omnia-doc/src/arena.rs`, `doc.rs`, `points.rs`
- Do: `Arena<T>` with `insert`, `remove`, `restore`, `get` and stable ids; `Part`, `Wire`,
  `Label`; `Doc` with crate-private insert and remove functions (commands come in T5.1);
  the point map of wire ends and pins; `junctions()`; the revision counter.
- Done when: tests pass for: `restore` after `remove` returns the same id; INV1 is enforced
  at insert; INV3 holds on generated sheets against a brute-force count.

### T2.2 Spatial index · done
- Lane: doc · Depends: T2.1 · Read: D3, K12
- Owns: `crates/omnia-doc/src/index.rs`
- Do: the uniform grid with insert, remove and `query(area, out)`; long wires registered in
  every cell they cross; deduplication by generation stamp.
- Done when: a property test shows `query` equal to a brute-force scan for random items and
  rectangles; `query` allocates nothing when `out` has capacity; a criterion bench records
  the time to query a 4K viewport on the 10,000-part sheet.

### T2.3 Symbol compiler · done
- Lane: assets · Depends: T0.1, T1.1 · Read: plan section 9 (Symbols), K2, K7
- Owns: `crates/omnia-symbols/`, `xtask/src/symbols.rs`
- Do: `cargo xtask symbols` turns the SVGs named in `pins.toml` into `SymbolDef`s. Write
  `pins.toml` for the symbols the mockup uses: `Resistor-IEEE-Standard`,
  `Resistor-IEEE-Potentiometer`, `Capacitor-IEEE-NonPolarized`, `Diode-COM-Standard`,
  `IC-COM-OpAmp`, `Ground-COM-General`. Implement `SymbolArt::write_svg`.
- Done when: the xtask fails on a pin off the 1.27 mm lattice; for each symbol, the SVG from
  `write_svg` at the original stroke matches the vendor SVG when both are rasterised with
  resvg (difference under 1% of pixels); pin positions match `pinAt` in `symbols.mjs`.

### T2.4 Stress sheets, save and load · done
- Lane: bench (generator), doc (save and load) · Depends: T2.1, T2.3
- Owns: `crates/omnia-bench/src/generate.rs`, `crates/omnia-doc/src/io.rs`
- Do: seeded generators for the five `StressSheet`s; `10k` has exactly 10,000 parts and
  20,000 wires. `Doc::save` and `Doc::load` in RON.
- Done when: the same seed gives the same sheet; `load(save(doc)) == doc`; `10k` generates
  in under 2 s.
- Follow-up (2026-11-16, from W1.3 and T6.1 reviews): no generated sheet produces a
  wire-body snap and only `dense` produces a junction (every `10k` wire is a half-grid stub
  from a pin), so WB5's class coverage rests on `dense` and unit tests; `10k` also has about
  23 duplicate wire pairs where parts' first pins coincide, so it is not in T6.1 normal form.
  A future generator change to emit realistic, normal-form nets needs new B7 and WB5
  baselines. See wiring-roadmap.md W2.4.

### T2.8 Doc owns index and connectivity · done
- Lane: doc · Depends: T2.2, T2.4, T4.1, T4.2 · Read: D2, D3, K12, INV1, INV3, INV9, INV10,
  plan section 9
- Owns: `crates/omnia-doc/`, necessary generator/hit callers and CPU benchmark harnesses
  in `crates/omnia-bench/`, plan/roadmap and `docs/measurements/T2.8.md`
- Do: `Doc` owns a `SpatialIndex` and a `Connectivity`, kept in sync inside every
  crate-private insert, remove and restore (parts, wires, labels) and rebuilt in `from_items` and
  `load`. `Doc::query(area, out)` for T2.5. Part bounds come from symbol-local art bounds
  supplied at insert (`omnia-symbols` depends on `omnia-doc`). Pins join the point map of
  connectivity (D2), so abutting pins share a net. `hit_test` and `snap` use the owned index.
  Move the T2.2 and T4.1 benches onto T2.4's `10k` generator and add label-heavy and
  20k-wire worst cases.
- Done when: `load(save(doc)) == doc`; the INV10 property test (pins, id reuse through
  remove and restore, adds over live ids, label-only components) and an internal consistency
  check pass after every step; `Doc::query` allocates nothing with capacity; benches record
  whether one add and one remove stay under 1 ms on `10k` and in the worst cases.

### T2.5 Sheet rendering · done
- Lane: canvas · Depends: T0.6, T1.6, T2.2, T2.3, T2.4, T2.8 · Read: D4, D5, K5, K6, K9, K11, G3 to G5, INV7
- Owns: `crates/omnia-canvas/src/sheet.rs`, `sheet/wires.rs`, `sheet/symbols.rs`, `sheet/lod.rs`
- Do: cull with `Doc::query`; wires as device-pixel-snapped quads; symbols as `paint_svg`
  sprites with prebuilt keys per (symbol, size bucket) and orientation in the matrix;
  junction dots; the LOD ladder; reusable scratch buffers. Far zoom per D4 (T0.6): merge
  collinear wire segments, drop the grid below K10, symbols as LOD boxes.
- Done when: a `render_to_image` test at 1.0, 1.25 and 1.5 shows every wire a whole number
  of device pixels wide with constant width along its length; the INV7 test passes (no
  `paint_path` or `canvas(` in `sheet*`; zero allocations per frame once warm on `1k`).

### T2.6 Bench scripts and CI gate · done
- Lane: bench · Depends: T2.5 · Read: plan section 11 (CI noise)
- Owns: `crates/omnia-bench/src/scripts.rs`, `.github/workflows/bench.yml`, `bench-baseline/`.
  Owner-approved additions: `crates/omnia-bench/src/main.rs` and `crates/omnia-bench/src/scene.rs`
  (real-sheet CLI and scene wiring), and `crates/omnia-canvas/src/sheet.rs` (B10/Q5 paint accounting).
- Do: `pan` and `zoom` scripts over real sheets; primitive counts by kind in the report. In
  hosted CI, compare the deterministic numbers (primitive counts, `allocs_per_frame.ours`)
  to a committed baseline with `bench compare`. Add a manually triggered job for the
  frame-time comparison on the reference machine.
- Done when: CI fails on a pull request that adds one allocation per frame or 11% more
  primitives, and passes on a no-op pull request.

### T2.7 Gate: milestone 2 · done
- Lane: bench · Depends: T2.6
- Do: run and record:

  | Budget | Command | Pass |
  | --- | --- | --- |
  | B2 | `bench start --sheet 1k` minus `bench start` | < 300 ms |
  | B3, B5 | `bench run --sheet 1k --script pan` and `zoom` | p99 < 16.7 ms, 0 missed refreshes (no present interval > 1.5 refreshes, compositor-side ones excluded per plan 2.1), max ≤ 33.3 ms |
  | B4, B5 | `bench run --sheet 10k --script pan` and `zoom` | p99 < 33.3 ms, no present interval > 2.5 refreshes (compositor-side ones excluded per plan 2.1; 10k zoom far-zoom intervals reported as the known limit until T2.9), max ≤ 50 ms |
  | B9 | `memory_mb` in the `10k` run | < 300 |
  | B10 | all reports | `allocs_per_frame.ours == 0` |

- Done when: all pass on the reference machine at 100% and 125%. On another machine:
  record and set `needs-human`. If B4 misses, apply the "fails at 70k" row of the outcome
  table before reporting.

### T2.9 Far-zoom wire stubs · todo
- Lane: canvas · Depends: T2.7 · Read: D4, D5, K9, plan 2.1 "B4 known limit",
  `docs/measurements/T2.7-wire.md`, G3, G12
- Owns: `crates/omnia-canvas/src/sheet.rs`, `crates/omnia-canvas/src/sheet/lod.rs`,
  `crates/omnia-canvas/src/sheet/wires.rs`
- Do: build D4's far-zoom wire-stub rule (owner decision 2026-11-18).
  1. In `lod.rs`, add the stub threshold to K9 (default 2.5 logical px) and
     `lod::wire_run(length_px) -> bool`.
  2. In the far-zoom branch of `DocumentCache::draw` (junctions hidden), filter the merged
     runs from `MergedSegments::visible` by on-screen length `(end - start) × scale`, after
     merging and before `Segment::bounds`. Keep `MergedSegments` and its cache as they are:
     the filter depends on the camera and runs every frame, without allocating.
  3. Leave the near-zoom path, the junction and box rules, and the sprite layer unchanged.
  4. Record primitive counts with `--features scene-counts` on the 10k zoom script at frames
     376, 420, 461, 500 and 545 at 100% and 125%, before and after, in
     `docs/measurements/T2.9.md`. The T2.7 investigation measured 28,278 quads before and
     8,324 with all 1 to 2 device-px runs removed at 125%.
  5. Remove the B4 known limit: delete it from plan 2.1, the B4 row and T2.7's table, and
     delete `far_zoom`/`scope_known_limit` from `docs/measurements/T2.7-wire/gate.py`
     (or freeze a T2.9 copy of the gate without them) before the timed run.
- Done when: a unit test (D4, K9) shows a far-zoom run below the threshold is not drawn,
  one at or above it is, and a long wire made of short collinear segments still draws;
  B10 holds on 10k zoom (`allocs_per_frame.ours == 0`); goldens unchanged (the T3.2
  fixture is not at far zoom; if a far-zoom fixture exists by then, update it on Windows
  and say so); and the full T2.7 table with **no** B4 known limit passes on Orion at 100%
  and 125%. On another machine: record counts and set `needs-human`. If B4 still misses,
  stop and ask the owner about Plan B (D4).

## Milestone 3: text and goldens

### T3.1 Labels · todo
- Lane: canvas · Depends: T2.7 · Read: D6, K8, K9, K11, G14, G15
- Owns: `crates/omnia-canvas/src/sheet/text.rs`
- Do: designators, values, net labels and pin numbers. One cached `ShapedLine` per label
  and size bucket, reshaped only when the bucket changes. Hide below the K9 threshold.
- Done when: zero allocations per frame once warm on `text` at a fixed zoom; a zoom sweep
  run twice leaves the atlas the same size after the second pass as after the first.

### T3.2 Golden-image harness · done
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
- Do: `bench run --sheet text --script pan` and `zoom`: p99 < 16.7 ms (B3). Re-run the whole
  T2.7 table with text on. Run the golden tests.
- Done when: all pass on the reference machine; otherwise record and set `needs-human`.

## Milestone 4: selection and nets

### T4.1 Connectivity · done
- Lane: doc · Depends: T2.2 · Read: D2, INV10
- Owns: `crates/omnia-doc/src/connectivity.rs`
- Do: components over the point map, grown by union on add and repaired by re-flooding the
  touched component on remove; `NetId` by net label; `net_of` and `net_wires`; a
  from-scratch rebuild used after load and as the test oracle.
- Done when: the INV10 property test passes over random add and remove sequences; a
  criterion bench shows one wire add and one wire remove under 1 ms on `10k`.

### T4.2 Hit testing and snapping · done
- Lane: doc · Depends: T2.2, T2.3 · Read: D3, K14, INV9
- Owns: `crates/omnia-doc/src/hit.rs`, `snap.rs`
- Do: `hit_test` (nearest part, wire or label within tolerance, parts by their bounds) and
  `snap` (priority: pin, wire end, wire body, grid). Add `bench hit`.
- Done when: property tests show `hit_test` equal to brute force and INV9 for `snap`;
  `bench hit --sheet 10k` reports p99 < 0.2 ms (B7; reference machine for the gate).
- Approved T4.2 integration scope: read-only `Doc::pins` (world pins, optional for
  missing IDs), `parts`, `wires`, and `labels` (live ID/reference iterators).
  Signatures are in plan section 9; no public mutation added.
- `build_hit_index` takes a symbol-local bounds closure to avoid the symbols/doc
  dependency cycle. Its part entries include world pins; picking uses body bounds.
  Pass this index explicitly until Doc owns it; then maintain it on edits and wrap
  the interaction functions as Doc methods. Labels currently pick by anchor.
- Snapping settles through higher priorities after grid rounding or wire projection
  to satisfy INV9 even with off-grid geometry. The final position can exceed the
  initial radius; plan section 9 documents this and deterministic tie rules.
- Note (W0.3): [wiring-plan.md](wiring-plan.md) WD5 refines snap ties, grid-snapped body
  projection, and the `Junction` class; implemented by [wiring-roadmap.md](wiring-roadmap.md)
  W1.3 (PR #41).
- Note (2026-11-16, W1.3): superseded by the owner decision "radius wins". `snap` no
  longer settles; a non-grid result is always within the radius, and INV9 is reworded in
  plan section 7 (wiring-plan.md WD5, WINV7). The settling bullet above is historical.

### T4.3 Overlay, hover and selection · todo
- Lane: canvas · Depends: T3.4, T4.2 · Read: D7, INV11, G6, wiring-plan.md WD8
- Owns: `crates/omnia-canvas/src/overlay.rs`, `selection.rs`
- Do: `OverlayView`; hover outline; click and shift-click selection; rubber band per WD8
  (normalised box; both Intersecting and Enclosed modes, defaulting to Intersecting per WQ3;
  Shift adds; W3.4 adds `Doc::query_within` and `TC-band-*`). `SheetView` embedded with
  `.cached()`. Colours from `stage/select`, `stage/select-halo`, `stage/hover`.
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

### T5.1 Commands, undo and redo · done
- Lane: doc · Depends: T4.1 · Read: D2, INV4
- Owns: `crates/omnia-doc/src/command.rs`, `undo.rs`
- Do: every `Command` variant except `Paste`; validation (K1 limits, INV1); inverse
  commands; the undo and redo stacks; `Change` with a correct dirty rectangle; index and
  connectivity updated inside `apply`.
- Done when: the INV4 property test passes over random command sequences, comparing
  documents including ids; INV10 still holds after every step; one `Move` of 100 items on
  `10k` applies in under 1 ms.
- Note (W0.3): T5.1's code is merged without `follow: FollowPolicy`; [wiring-roadmap.md](wiring-roadmap.md)
  W2.2 adds `FollowPolicy` to `Move` and `Reorient` (WD3; until W2.2, every run detaches).

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

### T6.1 Wire routing rules · done
- Lane: doc · Depends: T5.1 · Read: D2, INV2, wiring-plan.md WD4, WD9
- Owns: `crates/omnia-doc/src/route.rs`
- Do: the L-route between two points with a `flip` flag for the corner; `AddWires` splits
  any wire whose interior an added end lands on, and splits the added wire where it passes
  over an existing end or pin; segments that end up collinear and joined with no third end
  or label between them (WD4, WD9) are merged.
- Done when: property tests pass for INV2 (two wires crossing at interiors are in different
  nets; a wire ending on another's body joins its net) and INV10; undo restores the
  pre-split wires with their ids (INV4).

### T6.2 Wire tool · todo
- Lane: canvas · Depends: T5.4, T6.1 · Read: K14, D7, wiring-plan.md WD5, WD6
- Owns: `crates/omnia-canvas/src/tool/wire.rs`, `overlay/snap_marker.rs`
- Do: W starts wiring; an L-shaped preview follows the cursor with its first corner chosen
  per WD6 (Space flips the corner); the snap target (pin, wire end, junction, wire body per
  WD5) is marked in `stage/snap` before the click; click commits the segment and continues
  from its end; a click on a pin or wire ends the wire; Esc cancels.
- Done when: simulated-input tests wire two resistors pin to pin and get one net; the
  preview costs no `SheetView` paint; goldens show the preview and each snap marker kind.

### T6.3 Wires follow moved parts · replaced by wiring-roadmap.md W2.1 to W2.5
- Replaced by [wiring-roadmap.md](wiring-roadmap.md) W2.1 to W2.5 (Q6/WQ1 answered "yes"
  on 2026-11-16; no longer blocked). See [wiring-plan.md](wiring-plan.md) WD3.

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
- Lane: canvas · Depends: T6.4 · Read: D2, wiring-plan.md WD9
- Owns: `crates/omnia-canvas/src/tool/label.rs`
- Do: `Tool::Label` places a label on a wire or pin (placing a label on a wire body splits
  the wire there per WD9, and the merge rule keeps that point); text entry through GPUI's
  input handling (read `crates/gpui/examples/input.rs` at the pinned revision first); editing
  an existing label is one `SetText` command.
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
- Do: measure B1 to B10 on the reference machine at 100% and 125%, plus every invariant
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
2026-11-14 T0.7 blocked — added .github/workflows/ci.yml and deny.toml only. The workflow
                       runs on windows-latest for pull requests and pushes to main: pinned
                       toolchain from rust-toolchain.toml, actions/cache over
                       ~/.cargo/registry, ~/.cargo/git and target/ (keyed on Cargo.lock, so
                       the pinned Zed git dependency is not re-cloned), MSVC via
                       ilammy/msvc-dev-cmd plus a step that fails if link.exe, the Windows
                       SDK or fxc.exe is missing (hosted windows-latest images ship all
                       three; fxc is found on PATH or under the SDK bin\x64), then
                       cargo fetch --locked before the checks so the offline INV12 test
                       in xtask finds a populated cache, then the standard checks
                       (cargo fmt --all --check; cargo clippy --workspace --all-targets
                       -- -D warnings; cargo test --workspace; cargo deny check, with
                       cargo-deny 0.20.2 installed via taiki-e/install-action). The test
                       step is labelled headless: hosted runners have no interactive
                       desktop, GPUI cannot open windows there, and no window-opening or
                       render_to_image test exists yet (the only test is INV12); later
                       tasks must gate such tests to the reference machine instead of
                       weakening them. deny.toml allows exactly the plan section 10
                       licences (MIT, Apache-2.0, BSD as BSD-2-Clause/BSD-3-Clause, ISC,
                       Zlib, Unicode as Unicode-3.0), ignores only the private workspace
                       crates, and allows only the six git sources pinned in Cargo.lock
                       (zed plus font-kit, oo7, scap, wasm_thread, gfx-rs/wgpu).
                       Plan section 10 names only the Zed checkout; the other five git
                       sources are pulled in by that checkout through Cargo.lock and are
                       recorded here with the licence of every crate they provide (from
                       cargo metadata, cross-checked against each repo's licence file):
                       gfx-rs/wgpu at cddb2f5d — naga, naga-types, wgpu, wgpu-core,
                       wgpu-hal, wgpu-types and the wgpu-core-deps-* crates, all
                       MIT OR Apache-2.0; zed-industries/font-kit at 94b0f281 —
                       zed-font-kit 0.14.1-zed, MIT OR Apache-2.0; zed-industries/oo7
                       at 731c1215 — oo7 0.6.0, MIT; zed-industries/scap at b5c86ec5 —
                       zed-scap 0.0.8-zed, MIT; zed-industries/wasm_thread at
                       0cf96c77 — wasm_thread 0.3.3, Apache-2.0 OR MIT; the Zed
                       checkout itself (15d136bf) — gpui, gpui_platform and the other
                       in-repo crates, all Apache-2.0. Every git-sourced crate is
                       inside the section 10 allow-list; none adds a licence exception.
                       Local run on the T0.1 host (Windows 11 x64, i7-14650HX, RTX 5060
                       Laptop + Intel UHD, 100% DPI): fmt passed; clippy passed in
                       1m 47s; cargo test --workspace passed (INV12 ok); cargo deny check
                       FAILED (exit 5) and the CI deny step will fail the same way until
                       the owner decides. Licences outside the allow-list, not silently
                       allowed: cbindgen 0.28.0 MPL-2.0 (build dependency of gpui_apple),
                       dwrote 0.11.5 MPL-2.0 (via zed-font-kit), option-ext 0.2.0
                       MPL-2.0 (via dirs). Advisories also failing, all transitive
                       through the pinned GPUI tree, which rule 8 forbids changing here:
                       unmaintained rustybuzz (RUSTSEC-2026-0206), ttf-parser
                       (RUSTSEC-2026-0192), zbus_names (RUSTSEC-2026-0371), zvariant
                       (RUSTSEC-2026-0370), zvariant_derive (RUSTSEC-2026-0372). Bans
                       passed; sources passed after the six git sources were listed.
                       Unblocking needs an owner decision: add MPL-2.0 to the plan
                       section 10 allow-list, or replace/patch the three crates (a GPUI
                       revision change is its own task). No check was weakened and the
                       GPUI revision is unchanged. CI on PR #2 confirms the local result:
                       every step passes except cargo deny check, which fails with the
                       same three MPL-2.0 rejections and five unmaintained advisories
                       (bans and sources ok). Run times on windows-latest: 3m 53s with
                       the workflow's Cargo/target cache missing, 2m 34s with an exact
                       cache hit (the toolchain action's own cache was warm in both).
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
2026-11-14 T0.5 needs-human (H3) — the S7 logger is ready; nothing measured yet. `bench spike s7
                       [--seconds 60] [--out FILE]` opens a 1100x760 window listing the five
                       gestures, logs every ScrollEvent (delta kind, dx/dy, touch_phase, source,
                       modifiers), PinchEvent (delta, phase), mouse move/down/up, each with t_ns
                       since start (Instant at dispatch, not the OS time stamp) and the index of
                       the frame that shows it, as JSON lines (schema omnia-bench/s7-log/1). Every
                       event dirties the view; a left drag draws a dot at the cursor. One `frame`
                       line per drawn frame holds dirty-to-present and draw ms from the difference
                       of two frame_duration_snapshot()s (G13), with the draw count it covers.
                       Exits by itself after N s (watchdog N+30 s), then prints the summary.
                       `bench spike s7 --summarise FILE`: delta kind and phases per input shape
                       with the device inferred from GPUI's Windows code, a timeline of bursts,
                       events per frame p50/p99/max (all and per shape, plus GPUI's own
                       InputLatencySnapshot), each pinch's phase sequence, scroll->pinch switches,
                       and dirty-to-present p50/p95/p99/max over frames with a left-button move.
                       Unit tests cover a synthetic log, an empty file and a log with no input.
                       It never overwrites an existing --out (exit 2). Smoke: `spike s7 --seconds 5`
                       exits 0; the owner's mouse happened to be over the window, so that run
                       logged incidental hover moves and wheel notches (not a measurement); the
                       summariser handles a 0-byte log and the same log with input lines removed.
                       G10 is imprecise: direct_manipulation.rs OnContentUpdated allows Scroll ->
                       Pinch within one gesture (it emits scroll Ended, then pinch Started), never
                       Pinch -> Scroll. Wheel events are Lines with phase Moved only; touchpad
                       events are drained once per WM_PAINT, and inertia arrives as more Moved
                       scrolls. Plan section 4 is not changed here (outside T0.5's files).
                       Owner, at the touchpad and mouse (about 60 s; keep the pointer over the
                       window, pause between steps):
                         cargo run --release -p omnia-bench -- spike s7 --seconds 60 --out target/s7-input.jsonl
                       Then, or any time later:
                         cargo run --release -p omnia-bench -- spike s7 --summarise target/s7-input.jsonl
                       The next agent writes docs/measurements/T0.5.md from that summary and sets
                       T0.5 to done.
2026-11-14 T0.2 done — vendored Inter v4.1 (400 Regular, 500 Medium, 600 SemiBold) from
                       rsms/inter and Source Code Pro 2.042R-u/1.062R-i (400 Regular, 500
                       Medium, 600 Semibold) from adobe-fonts/source-code-pro with their SIL
                       OFL 1.1 licences under vendor/fonts/, and recorded both in
                       THIRD_PARTY.md. Split omnia-app window setup and bundled font
                       registration (cx.text_system().add_fonts) into crates/omnia-app/src/lib.rs
                       while keeping main.rs thin with the --frames CLI and wall-clock watchdog;
                       Workspace draws one line in Inter SemiBold 17 px and one in Source Code
                       Pro Medium 13 px. Added gpui_platform/test-support as dev-dependency only
                       with a harness = false integration test (tests/fonts.rs). render_to_image
                       worked on this Windows 11 machine (Intel Core i7-14650HX, NVIDIA RTX 5060
                       Laptop GPU + Intel UHD Graphics, 125% DPI in test window) using
                       gpui_platform::application().run with hidden windows (show: false,
                       focus: false), window.resize() in open_window, and window.draw(cx).clear(cx)
                       followed by window.render_to_image() inside cx.spawn; verified distinct
                       FontIds across weights 400/500/600, two non-blank ink bands in Workspace,
                       and differing renders between Inter and Source Code Pro. Checks passed:
                       cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D
                       warnings; cargo test --workspace (fonts + INV12); cargo run -p omnia-app
                       -- --frames 60 (exit 0); git diff --check.
2026-11-14 T0.7 done — owner decisions (orchestrator, PR #2): MPL-2.0 added to the allow-list
                       (plan section 10 and deny.toml); the five GPUI-transitive "unmaintained"
                       advisories ignored by id in deny.toml, to re-check at each GPUI upgrade.
                       Merged main (T0.2 added the first window-opening test,
                       omnia-app/tests/fonts.rs): it now skips when OMNIA_HEADLESS_CI is set,
                       which only the hosted CI test step sets; it still runs locally and on
                       the reference machine.
2026-11-14 T0.5 blocked — owner ran H3 (laptop: i7-14650HX, RTX 5060 Laptop + UHD, precision
                       touchpad, 60 Hz, scale 1.25; not the reference machine). Results are in
                       docs/measurements/T0.5.md: wheel gives Lines (±3, merged -6); touchpad
                       pan gives Pixels with started/moved/ended phases; events per frame are
                       p50 1, p99 15, max 21, with 0 dropped mid-draw; drag dirty-to-present is
                       p50 16.1 ms, p99 26.6 ms, with draw time 1.3 ms. Blocked because GPUI
                       delivered no PinchEvent at all, although the owner pinched and pinch
                       works in other apps; two vertical-only, back-and-forth Pixels gestures
                       suggest the pinches arrived as scroll. Root cause is under investigation.
                       PR #5 review notes for a follow-up: move the logger's file writes off the
                       UI thread before B6 is judged, and log the final frame before quitting.
2026-11-14 T0.5 done — pinch was recorded in a follow-up owner session; details are in
                       docs/measurements/T0.5.md. A standalone DM probe showed Windows reports
                       pinch scale on this touchpad (×0.42 to ×1.47). S7, built against GPUI
                       15d136bf with trace logging only, got 3 pinch gestures and 213
                       PinchEvents: Started (delta 0), Moved (delta = scale ratio - 1), Ended.
                       Nothing was dropped. The first session's zero pinches are unexplained;
                       the likely cause is that a gesture which starts as a pan stays a pan
                       under DM rails. T1.5 must not expect "pan then pinch" in one gesture.
                       Two GPUI defects were found that did not bite here: events.rs:1376 can
                       skip DM update() via an early return, and events.rs:1380 drops drained
                       events when there is no input callback. Both are candidates for an
                       upstream issue at the next GPUI pin task.

2026-11-14 T0.4 needs-human - Implemented S1-S6/S8; integrated landed S7 and PR #4 fixes.
                       Read pinned GPUI 15d136bf before writing GPUI code; supplied G
                       corrections apply. resvg 0.46.0: Apache-2.0 OR MIT, verified from
                       Cargo.toml; optional readback, never default test-support.
                       39 cases x three 600-frame runs at 3840x2160, 125%, 60 Hz;
                       i7-14650HX, RTX 5060 Laptop + UHD; active Intel UHD, driver
                       32.0.101.7088. window_min_size avoids desktop clamping. Q4:
                       non-reference, no budget marked met. Raw results and spreads:
                       docs/measurements/T0.4.md. Median p99: 25k quads 5.235 ms;
                       70k 7.971 ms [7.488,18.301]; 15k sprites add 6.806 ms;
                       inferred atlas 11.52 GB/1494 pages after two sweeps; paths
                       4.849 us/path, 0 ours/30 total allocations; 12k glyphs add
                       6.298 ms, zero warm paint allocations; 60k replay 75.0%.
                       S8 GPU 4K readback works; cold median 139.318 ms, idle zero
                       frames/0.781% CPU; visible DPI 125->100->125 recorded.
                       G1's B10 consequence needs Q5 qualification: GPUI allocations
                       count in total, not ours. SVG keys include both snapped
                       dimensions despite width-only raster sizing; atlas retention
                       and count-dependent payload are findings. Provisional numbers
                       point to Plan C's 25k-quad row; T0.6 decides.
                       Live 150% and no-GPU render_to_image: not measured on this
                       machine; to do on the reference machine (Q4). No settings
                       changed; orchestrator requested wrap-up without more movement.
                       Standard fmt/Clippy/tests/deny and feature Clippy/release passed;
                       S7 summariser dispatch smoke passed. No check was weakened.
2026-11-15 Q4 answered  — owner named the Orion laptop as the reference machine: Lenovo
                       ThinkPad T470 (i5-7300U, Intel HD 620), Arch Linux on Wayland, Vulkan,
                       native 1920x1080 at 60 Hz, measured at 100% and 125%. Plan section 2.1
                       adds the reference protocol (hardware Vulkan device only, native panel,
                       AC and foreground, ten interleaved repetitions, power state recorded).
                       Windows stays the first shipping platform; the owner's ASUS TUF F16 is
                       the Windows development machine and gates nothing. New task T0.8 brings
                       the bench up on Orion and re-runs the spikes there; T0.6 now depends on
                       it. An S1 re-run on the ASUS under the same protocol (T0.4-ref) showed
                       T-quad passing there; it is Windows evidence, not a reference result.
2026-11-15 T0.8 done — Orion ThinkPad T470, i5-7300U/HD 620, Arch/KDE Wayland,
                       Mesa ANV 26.2.3; pinned GPUI uses wgpu, not blade. Enabled
                       Linux Wayland in app/bench, gated unsupported image tests
                       and Windows readback, added Linux process counters and a
                       Bash reference runner. GPUI revision unchanged. G10/G12
                       differ; G11 same; G13 common CPU stats; G16 headless lacks
                       frame callbacks. Source lines: docs/measurements/T0.8.md.
                       370 successful runs, ten rotating repetitions per case,
                       740 power snapshots; AC, powersave, native 1080p/60 Hz.
                       S1 at 100% and 125%; requested fixed S3/S4/S5/S6 and S8
                       at 100%. 100% restored and verified. No fallback used.
                       25k/70k draw p99 medians: 13.455/19.661 ms at 100%,
                       13.312/19.628 ms at 125%; both T-quad limits fail.
                       Sprite adds 15.988 ms; 12k glyphs add 16.299 ms fixed /
                       10.765 ms direct; paths 10.771–34.000 us/path; replay
                       49.043% of repaint. Exact 19k dirty-to-present p99
                       13.210 ms passes GPUI's CPU submission metric only.
                       S8 startup median 194.340 ms, idle zero frames, CPU
                       median 0% [0,0.1]; Linux render_to_image unsupported.
                       Numerical condition matches the Plan C row; T0.6 and
                       owner decide. No rendering decision or threshold changed.
                       Sleeping-panel watchdog failures and a script transition
                       error are preserved; no completed sample was removed.
                       fmt, workspace Clippy/tests, deny and scene-counts/readback
                       Clippy passed on Linux. No check was weakened.

2026-11-15 T1.1 done    — implemented crates/omnia-doc/src/geom.rs (wired and re-exported in
                       crates/omnia-doc/src/lib.rs) with K1/K2 constants (SHEET_LIMIT_NM,
                       GRID_NM, PIN_LATTICE_NM), GeomError, Pt, Rect, Orient (8 D4
                       orientations matching symbols.mjs: X-mirror then quarter-turn CW in
                       Y-down, with then/inverse/apply/checked_apply_around), and snap_to
                       (round-half-away-from-zero in i64 for D4 equivariance and INV9).
                       Checked constructors reject coordinates outside ±1,000,000,000 nm
                       and inverted rects. Added proptest as dev-dependency in omnia-doc
                       (already in Cargo.lock and plan section 10, MIT/Apache-2.0). Unit
                       tests cover checked constructors, all 8 orientations on a point, and
                       snap_to edge cases; proptest covers D4 group axioms, INV9 idempotence
                       and half-step bound, and overflow-freedom on in-range points/rects.
                       Checks passed: cargo fmt --all --check; cargo clippy --workspace
                       --all-targets -- -D warnings; cargo test --workspace (37 passed: 6
                       omnia-doc, 27+2 omnia-bench, 1 omnia-app fonts, 1 xtask INV12);
                       cargo deny check (advisories ok, bans ok, licenses ok, sources ok).
2026-11-15 T1.3 done    — implemented and exported Camera in omnia-canvas with K3 scale,
                       K4 zoom clamping, anchored zoom, fractional-pixel pan, viewport
                       resizing and conservative visible bounds clipped to K1. Mapping
                       subtracts the integer reference in i64 before conversion (INV8).
                       Uses Point<f64> logical pixels for reversible mapping and a separate
                       world_to_pixels paint conversion: GPUI Pixels stores f32 and cannot
                       retain individual nm at K3. Updated D1 and the section 9 sketch
                       under rule 12; no tolerance or check was weakened. Read GPUI geometry
                       at pinned 15d136b before use. Dependencies are all in plan section 10;
                       Cargo.lock only records the canvas dependency edges. Eight camera
                       tests include proptest across the sheet and zoom range, anchor
                       invariance within 1 nm and exact pixel pan; edge cases cover adjacent
                       nm at sheet limits, 5%/3200%, fractional pan, 1,000 zoom cycles,
                       visible bounds and resize. All standard checks passed: cargo fmt
                       --all --check; cargo clippy --workspace --all-targets -- -D warnings;
                       cargo test --workspace (45 passed); cargo deny check (advisories,
                       bans, licenses and sources ok; existing dependency warnings).
2026-11-15 T1.2 done — `cargo xtask tokens` reads the `Theme` collection of omnia.figcraft
                       (repo root; modes Graphite and Gray) and the new overlay
                       crates/omnia-theme/canvas-tokens.toml (the Q2 default values, unchanged),
                       and writes crates/omnia-theme/src/generated.rs: 35 colour tokens per
                       theme, 22 from the file (its one number variable, space/rail-shell, is
                       not a colour and is left out) and 13 from the overlay. Overlay values are
                       hex or a token name resolved per theme, so stage/snap follows accent/mint
                       in each; a token the file also has is taken from the file. Output is
                       sorted and rustfmt-clean; a second run gives no diff, and an xtask test
                       fails if generated.rs is stale. INV6 test is omnia-theme/tests/inv6.rs; a
                       scratch run with "#2F6FEB" planted in omnia-canvas/src/lib.rs, and with
                       gpui::rgb(..) in a nested file, failed it as expected. Dependencies: zip
                       9 (MIT; zip 8.6 fails cargo deny on RUSTSEC-2026-0367/0368/0369/0429),
                       serde_json, toml, all from plan section 10 and xtask only. zip adds
                       typed-path (MIT or Apache-2.0), nix (MIT) and constant_time_eq (CC0-1.0,
                       MIT-0 or Apache-2.0), all inside the deny allow-list. Outside the owned
                       files: xtask/src/main.rs gains the `tokens` dispatch and
                       .cargo/config.toml adds the `cargo xtask` alias; both are needed for the
                       command to exist. crates/omnia-theme/.gitattributes keeps generated.rs LF
                       on Windows checkouts.
2026-11-15 T0.8 review fixes — measure-ref.sh restores the eDP-1 scale it found at
                       start (header now says the campaign changes it temporarily),
                       takes the S1 duration as S1_SECONDS instead of a parent
                       runs.jsonl, and asserts HD 620/1080p only at campaign sizes.
                       validate.py, summarize.py and measure-ref.sh reject runs with
                       warnings or missing frames; recorded data passes and tables,
                       validation.txt and thresholds.json are unchanged. S5 sweeps
                       shape all 22 buckets on the first warm-up frame. Reports
                       carry paint_samples/paint_sampling, warn on missed paints and
                       mark S6 cached as sparse by design. Spike reports name the
                       mode; s6 --mode fixed is an error. The fonts test checks TTF
                       patching on Linux and prints an explicit render_to_image skip.
                       CI lints omnia-bench with scene-counts,readback. resvg 0.46 and
                       the 29 Wayland transitive crates are in plan section 10.
                       fmt, workspace Clippy/tests, bench feature Clippy and deny pass
                       on Linux. No measurement rerun; no check was weakened.
2026-11-15 T0.6 done    — H1: owner relaxed B3 to 60 fps (p99 < 16.7 ms) and B4 to 30 fps
                       (p99 < 33.3 ms), both also judged on median present interval; B5 to no
                       frame over 33.3 ms (1k) / 50 ms (10k). Thresholds retuned to the plan's
                       frame loads. On Orion (T0.8): 19k quads draw 10.6 ms, present every
                       16.7 ms; 65k with grid draw 23.2 ms, present every 32.7 ms; 70k quads
                       alone present every 67 ms (GPU-bound). Outcome: Plan A with far-zoom
                       simplification (merge wires, drop grid, LOD boxes), direct glyph
                       emission and a visible-label cap. D4 DECIDED. Plan B stays the fallback
                       if B4 misses at T2.7. Details in docs/spike-results.md.
                       T0.4 set to done: its open items are settled by Q4 and T0.8 (the
                       reference runs at 100% and 125%, not 150%; render_to_image is
                       unsupported on Linux, so goldens run on Windows).
2026-11-16 T2.1 done    — added append-only Arena<T> slots with stable u32 ids and checked
                       restoration; typed part/wire/label ids and records; crate-private
                       insert/remove operations, revision counter and an end/pin point map.
                       Junctions are derived on query (INV3); insert rejects degenerate or
                       diagonal wires (INV1) and out-of-K1 items/pins before changing state.
                       Property tests cover remove/restore identity, no id reuse, INV1 and
                       INV3 against brute-force counts after generated insert/remove edits;
                       focused tests cover crossings, body contacts, coincident pins,
                       rejected edits and revisions. All eight orientations are exercised.
                       Staged API detail: omnia-symbols is still a stub (T2.3), so SymbolId
                       lives in doc.rs and Doc::new() takes no library yet; internal part
                       insertion takes local pin positions and caches transformed world pins
                       for removal. Library-backed construction remains for symbol integration.
                       Commands/Change and undo/redo remain T5.1, as T2.1's Do line specifies;
                       Arena::restore supplies the stable-id primitive they need.
                       No dependencies added. No existing check changed. Scoped dead-code
                       expectations cover the six internal editing entry points until T5.1.
                       Windows checks passed: cargo fmt --all --check; cargo clippy --workspace
                       --all-targets -- -D warnings; cargo test --workspace (12 omnia-doc tests,
                       including property tests, and xtask INV12); cargo deny check (existing
                       dependency warnings only; advisories/bans/licenses/sources all pass).
2026-11-16 T2.3 done    — `cargo xtask symbols` (xtask/src/symbols.rs, wired in xtask/src/main.rs)
                       parses crates/omnia-symbols/pins.toml and vendor/electronic-symbols/SVG/
                       with usvg 0.46, verifies every pin lands on the 1.27 mm (PIN_LATTICE_NM)
                       lattice and inside K1 bounds, snaps open stroked lead ends within 1.0
                       box unit of declared pins while preserving axis-aligned lead stubs, and
                       writes deterministic rustfmt-clean statics to
                       crates/omnia-symbols/src/generated.rs (crates/omnia-symbols/.gitattributes
                       keeps LF on Windows). pins.toml defines the six mockup symbols at K7
                       placed sizes: Resistor-IEEE-Standard, Resistor-IEEE-Potentiometer,
                       Capacitor-IEEE-NonPolarized and Diode-COM-Standard at 80 px, IC-COM-OpAmp
                       at 180 px (pins 3/2/1/8/4 matching symbols.mjs), and Ground-COM-General at
                       48 px with anchor = [75.0, 0.0] so its pin stays at Pt::ZERO on the K2
                       lattice under any Orient. crates/omnia-symbols/src/lib.rs defines
                       SymbolId, Pin, LineCap, LineJoin, ArtPath, SymbolArt::write_svg,
                       SymbolDef and SymbolLibrary. Dependencies: omnia-doc in omnia-symbols and
                       xtask, usvg 0.46 in xtask, resvg 0.46 in omnia-symbols dev-dependencies;
                       usvg/resvg are already in plan section 10 and Cargo.lock (zero new
                       crates added; INV12 holds). Integration tests (omnia-symbols/tests/symbols.rs)
                       verify library lookup, K2 lattice and symbols.mjs pinAt correspondence
                       across all 8 orientations, and resvg pixel diff < 1% against vendor SVGs
                       at 150 px and placed 100% sizes plus widened stroke renders; xtask tests
                       verify idempotent generated.rs, off-lattice rejection, and compilation of
                       all 116 vendored SVGs. Standard checks passed on Windows 11 x64 dev
                       machine: cargo fmt --all --check; cargo clippy --workspace --all-targets
                       -- -D warnings; cargo test --workspace (65 passed); cargo deny check
                       (advisories, bans, licenses, sources ok); cargo xtask symbols (up to date).
2026-11-16 T1.4 done    — SheetView (crates/omnia-canvas/src/sheet.rs) implements Element
                       directly and paints paper (stage/paper) and the dot grid (stage/grid)
                       inside one paint_layer, under a content mask of its bounds. Dots are
                       plain square quads of max(1, round(1.2 × scale factor)) device px
                       (K6: 1, 2, 2 at 1.0, 1.25, 1.5), centred on the nearest device pixel.
                       snap_px.rs holds the rounding helpers (to_device, round_device,
                       snap_logical, stroke_device_px, device_to_logical, device_rect,
                       centered_square): pick whole device pixels, pass device/scale to
                       GPUI, whose own snap (util.rs round_to_device_pixel, read at the pin)
                       maps them straight back; a proptest checks that against a restatement
                       of GPUI's formula at seven scale factors. grid_level(scale) is K10:
                       pitch in [20, 40) px; over [35, 40) the next finer level's extra dots
                       fade in (alpha 0 at 35 px, full at 40 px), so no dot pops. Plan K10
                       clarified (rule 12): above 200% k is negative (grid subdivisions,
                       k = -1 is the pin lattice), k runs -5..5 over K4, all whole nm.
                       Paint walks the lattice and emits quads directly: no buffers at all.
                       Tests: pitch unit test (100,001-point log sweep of K4 plus every wheel
                       notch both ways and the ends); sheet_alloc (test-local counting
                       allocator; 600 frames panning/zooming across K4 at 1.0/1.25/1.5,
                       0 allocations in every frame; the first frame also 0); sheet_render
                       (harness = false, hidden window, Window::set_scale_factor and
                       render_to_image, both test-support): at 1.0, 1.25 and 1.5, at zoom 1,
                       1.875 (faded level), 0.37 and 6.1 with fractional pans, every dot in
                       the image is a solid one-colour square of exactly the requested device
                       rectangle and size, and no other ink: 5,001 dots compared. A planted
                       unsnapped 1.2 px dot made it fail (2x1 dot at 1.0). Skips under
                       OMNIA_HEADLESS_CI and on Linux like fonts.rs; the comment in ci.yml
                       naming only fonts.rs is T0.7's file and was left as is. The scene
                       scale is overridden, not the monitor's; the image is the window at
                       its real DPI, so only the overlap is checked.
                       omnia-app: Workspace::with_sheet() adds a SheetView (fixed camera,
                       100%, GRAPHITE) under the two text lines; open_workspace uses it.
                       Workspace::new stays text-only so the fonts test's text-band check is
                       not satisfied by grid dots. `cargo run -p omnia-app -- --frames 60`
                       exits 0 by itself (4.4 s wall incl. cargo). New Cargo entries:
                       omnia-canvas → omnia-theme; dev gpui/gpui_platform test-support (and
                       wayland on Linux); omnia-app → omnia-canvas, omnia-doc, omnia-theme.
                       No new external crate. Standard checks pass (fmt, clippy -D warnings,
                       test --workspace three times in a row, deny). Host: Windows 11 x64,
                       i7-14650HX, RTX 5060 Laptop + Intel UHD, 1920x1200 at 125%, 60 Hz;
                       nothing timed, no budget gated. Next: T1.5 replaces the fixed camera.
2026-11-16 T1.5 done    — crates/omnia-canvas/src/input.rs: Navigator, a pure state machine over
                       Camera (read at pin 15d136b: interactive.rs ScrollEvent/PinchEvent/
                       TouchPhase, gpui_windows events.rs wheel path, div.rs listeners,
                       window.rs hitboxes and capture, test_context.rs). Lines zoom about
                       the cursor, 3 lines = one notch of 2^(1/4) (Windows' default wheel
                       setting, which GPUI multiplies in; T0.5 saw ±3 and -6), fractional
                       and merged notches by the matching power; Pixels pan by exactly the
                       delta; pinch zooms by 1 + delta about its position; middle-drag and
                       Space + left-drag pan; a Cancelled touch gesture restores the camera
                       it started with. Shift+wheel (horizontal Lines) is ignored. SheetCanvas
                       (same file) owns camera + Navigator, renders SheetView inside a small
                       Element that adds the one sheet hitbox and window-level mouse
                       listeners, notifies only when the camera changed, and emits
                       CanvasEvent::Status (cursor mm, drawn K10 grid step, zoom) only when
                       the status changed. GPUI finding: hitbox ids are never reused, so a
                       pan step that repaints the sheet must re-capture the pointer on the
                       new hitbox or the drag stops at the sheet edge; the element does this
                       in paint (a test dragging off the window failed without it).
                       crates/omnia-app/src/status_bar.rs: StatusBar view (bg/panel, rule,
                       text/secondary, Source Code Pro 11.5 px, 40 px, per the mockup), fed
                       only by CanvasEvent::Status. Workspace::with_sheet(cx) embeds canvas
                       and status bar as two .cached() views. Plan updated (rule 12):
                       CanvasStatus.cursor is Option<Pt> (None until the pointer has been
                       over the sheet); K4 says what a notch is; section 9 sketches Navigator
                       and the interim SheetCanvas::new(camera, theme, cx) until T2.5 wires
                       the Doc. Tests: 9 Navigator unit tests (anchor fixed within 1 nm at
                       5%..3200% for wheel and pinch, exact pixel pan, clamps report no
                       change, cancel, drags); omnia-app/tests/navigation.rs, 6 tests on
                       GPUI's test platform (TestAppContext, no OS window): wheel and pinch
                       keep the world point under the cursor; cursor moves notify only the
                       status bar (SheetCanvas and the root: 0 notifies, SheetCanvas 0
                       re-renders; a planted notify made it fail); touchpad, middle-drag off
                       the window, and Space-drag pan. Not done, for later tasks: no grab
                       cursor while panning. Outside owned files: lib.rs `pub mod input`,
                       omnia-app lib.rs wiring, omnia-app dev-dep gpui test-support (no new
                       crate). Workspace's header still shows T0.2's placeholder line and
                       rgb(STAGE_*) colours for the fonts test; left as is. Standard checks
                       pass on the Windows dev machine (i7-14650HX, RTX 5060 Laptop, 1920x1200
                       at 125%, 60 Hz; nothing timed, no budget gated, Q4): fmt, clippy -D
                       warnings, test --workspace three times in a row (all exit 0), deny.
                       `cargo run -p omnia-app -- --frames 60` exits 0 by itself. The earlier
                       unexplained `cargo test` exit 101 did not reproduce in 3 runs. Lead:
                       during this task drive C: hit 0 bytes free and the build failed with
                       os error 112 and LNK1318; cargo exits 101 on such failures. Freed
                       ~29 GB by deleting the target/ dirs of merged, idle worktrees (T1.4,
                       T2.3, T1.2, T0.4). The window tests (fonts, sheet_render) are separate
                       harness = false binaries that cargo runs one after another, so they
                       do not open windows in parallel with each other.
2026-11-16 T2.2 done — crates/omnia-doc/src/index.rs: SpatialIndex, a uniform grid of K12
                       cells (INDEX_CELL_NM = 20,320,000 nm; 99 x 99 cover K1). insert(item,
                       bounds) registers an ItemId in every cell its bounds meet (a long wire
                       in every cell it crosses) and moves an item already present; remove;
                       bounds; query(area, out) replaces out with items whose bounds meet area
                       (edges inclusive, as Rect::intersects), order unspecified, deduplicated
                       by a per-item generation stamp (wrap at 2^32 resets all stamps). Per-kind
                       tables are indexed by arena id, so no hash map. Stamps are Cells so query
                       takes &self as Doc::query does: the index is Send, not Sync (D8: the UI
                       thread owns Doc; jobs use a snapshot). Plan section 9 sketch updated.
                       Not wired into Doc: doc.rs is T2.1's file. Whoever adds Doc::query
                       (T2.5's culling needs it; likely T4.1/T5.1 in the doc lane) keeps a
                       SpatialIndex in Doc and calls insert/remove next to the arenas, with
                       part bounds from the symbol box and wire bounds from the segment plus
                       half the K5 stroke. Tests: property test (256 cases, up to 200 random
                       inserts/moves/removes/queries over the whole K1 sheet, rectangles from
                       points to sheet-spanning, ids colliding) equals a brute-force scan, no
                       duplicates, cell occupancy exact; a planted removal of the stamp check
                       made it fail. tests/index_alloc.rs (test-local counting allocator, the
                       sheet_alloc pattern): 0 allocations per query with capacity, control
                       query without capacity allocates. Bench benches/index.rs (criterion 0.8,
                       plan section 10, dev only, already locked through GPUI; default features
                       off): T2.4's generator does not exist yet, so the bench builds a seeded
                       synthetic sheet of exactly 10,000 parts and 20,000 wires (2% long buses).
                       4K viewport at 100%: 3.3 to 4.8 µs (1,314 items); whole sheet 0.18 to
                       0.27 ms (30,000 items). i7-14650HX, Windows 11, on battery, Silent
                       scheme, machine shared with other builds; docs/measurements/T2.2.md. Not
                       the reference machine; no budget gated. No Q ids needed. New files
                       outside index.rs: tests/index_alloc.rs, benches/index.rs and the
                       measurements file, all named by this task's Done-when line. During the
                       run C: hit 0 bytes free once (cause not checked); it cleared
                       without action from this task.

2026-11-16 T2.4 doing — Seeded five StressSheets in omnia-bench/src/generate.rs (SplitMix64);
                       10k exactly 10,000 parts / 20,000 wires; dense 1,000 / 12,000;
                       text 1,000 / 2,000 with 12,000 designator/value characters. All use
                       built-in symbol pins, eight orientations and K2-lattice placement.
                       Wires are unit lattice segments, avoiding ends on interiors (INV2).
                       io.rs adds version-1 RON save/load, preserving reserved slots, revision
                       and local pin geometry; load uses validated inserts to rebuild incidence.
                       Doc equality compares authoritative state. Public Doc::from_items is a
                       validated bulk constructor; raw insert methods remain crate-private.
                       Minimal supporting changes explicitly approved by owner: arena.rs slots
                       made crate-visible for stable-ID save/load; doc.rs declares io as a child
                       module for private store access and removes obsolete dead_code expects
                       from the three now-used insert methods. No other doc internals changed.
                       Plan section 9 updated: new/load signatures reflect GPUI-independent
                       self-contained pin geometry (avoids a symbols/doc dependency cycle),
                       from_items signature, workload counts and equality semantics. Module and
                       Cargo entries plus lockfile updated. serde and ron are in section 10;
                       ron 0.12.3 introduces transitive typeid 1.0.3 (MIT OR Apache-2.0, verified
                       in its registry Cargo.toml); no new licence exception or weakened check.
                       Tests: all five sheets at 0/42/u64::MAX, same/different seeds, exact
                       counts, save/load equality, K1/K2/INV1, brute-force INV3; IO tests cover
                       holes of all item kinds, future IDs, Unicode, revisions, corrupt inputs
                       and write failure. Optimized 10k: median 12.928 ms, range 11.495–14.613 ms,
                       i7-14650HX / RTX 5060 Laptop / Windows, 10 seeds. Non-reference (Q4);
                       details in docs/measurements/T2.4.md; no reference budget gated. C: had
                       95.8 GiB free before full build. Synced latest origin/main including
                       T2.2 PR #18; kept its roadmap log. T2.2 benches/index.rs can switch from
                       its synthetic 10k to this generator later; left untouched (doc bench
                       depending on the GPUI bench crate needs a separate dependency decision).
                       Standard checks pass on final main-synced source: cargo fmt --all --check,
                       cargo clippy --workspace --all-targets -- -D warnings, cargo test
                       --workspace, cargo deny check (existing dependency warnings only).
2026-11-16 T4.1 done — crates/omnia-doc/src/connectivity.rs: Connectivity and NetId (D2,
                       INV10), re-exported from lib.rs; plan section 9 updated. Two-level
                       incremental model: geometric components over the FxHashMap<Pt, PointSlot>
                       point map (SmallVec wire/label lists, union-by-size on add_wire, O(1) leaf
                       fast path on remove_wire and stamp-based BFS re-flood of only the touched
                       component otherwise) and logical NetIds over (Comp, LabelText) (O(1) fast
                       path for single-label nets, bipartite re-flood only when a multi-label
                       shorted net splits). Connectivity::rebuild performs an independent
                       from-scratch BFS over wires and labels for load and as the INV10 oracle.
                       Added rustc-hash = "2" and smallvec = "1" to omnia-doc (both already in
                       plan section 10 and Cargo.lock, MIT OR Apache-2.0; zero new crates).
                       Not wired into Doc (doc.rs/arena.rs left untouched for T2.4): whoever
                       wires Connectivity (and SpatialIndex) into Doc (T5.1) should add
                       Arena::iter(&self) -> impl Iterator<Item = (u32, &T)>, hold
                       connectivity: Connectivity in Doc, call add_wire/remove_wire in
                       insert_wire/remove_wire, add_label/remove_label in insert_label/
                       remove_label (and remove+add or replace on SetText), expose
                       Doc::net_of / net_at / net_wires, and rebuild via Connectivity::rebuild(
                       self.wires.iter().map(|(id, &w)| (WireId(id), w)),
                       self.labels.iter().map(|(id, l)| (LabelId(id), l))) after Doc::load.
                       Tests: 6 unit tests (crossings, loops/splits, label join/split, shorted
                       multi-labels, labels before wires, visit_gen wraparound, zero-alloc
                       net_wires with capacity) + INV10 proptest (256 cases, up to 200 random
                       wire/label adds and removes) matching rebuild at every step.
                       Bench benches/connectivity.rs (criterion 0.8): T2.4's generator is not on
                       origin/main yet, so it builds a seeded synthetic 10,000-part / 20,000-wire
                       sheet with chained nets, 2% long buses, and GND/VCC/CLK labels (switch to
                       T2.4's 10k generator once landed). On ASUS TUF F16 (i7-14650HX, Windows 11,
                       on battery, Silent scheme; not the Q4 reference machine, no budget gated;
                       docs/measurements/T4.1.md): leaf add 93.9 ns, leaf remove 54.2 ns, bridge
                       add 284 ns, bridge split 1.04 us, 200-wire chain bridge 676 ns, 200-wire
                       split 7.09 us (all >130x under 1 ms); full 10k rebuild 8.44 ms.
                       Standard checks passed: cargo fmt --all --check; cargo clippy --workspace
                       --all-targets -- -D warnings; cargo test --workspace; cargo deny check.
2026-11-16 T1.6 needs-human — Milestone-1 gate on Orion (HD 620, Mesa ANV 26.2.3, not
                       llvmpipe; full-screen 1920x1080 at 60.008 Hz, at 100% and 125%; AC;
                       governor powersave; no power profile available), commit 20d9f9e,
                       10 interleaved repetitions per case. Medians of run values:
                       B1 start 198.1 / 192.2 ms (< 400): PASS. B3 frame p99 pan 3.734 /
                       2.554 ms, zoom 3.637 / 2.355 ms (< 16.7): PASS. B8 idle 0 frames in
                       every run: PASS. B10 allocs_per_frame.ours 0 in all 40 runs: PASS.
                       B3's second half, by owner decision 2026-11-16 (plan section 2 and
                       the T1.6/T2.7 tables updated; B4 likewise: no present interval
                       > 2.5 refreshes): zero missed refreshes, a present interval > 1.5
                       refreshes being one. Judged per run from the reports' max interval:
                       0 in 39 of 40 runs; 100% pan run 7 has one 30.87 ms interval (>= 1
                       miss; frame work max 9.38 ms, 0 late frames). The old median wording
                       read 14 to 31 us over 16.664 ms through timestamp jitter. Still
                       needs-human: T3 Code desktop and other user apps were running (not
                       closed over SSH; bench window active in every run). A person reruns
                       gate.sh at the desk with them closed; that rerun decides B3.
                       Bench changes (lane bench): `--sheet empty` paints omnia-canvas
                       SheetView via paint_quads (T0.1 window kept as `--sheet t01`, spike
                       scripts' baseline updated); `--script zoom` (K4 sweep, geometric,
                       about centre, 600-frame cycle); `idle --sheet`; run reports now carry
                       missed_refreshes (owner definition), present_intervals_over_2_5_
                       refreshes and max_present_interval_ms, with GPUI's work-based count
                       renamed work_missed_refreshes. omnia-bench gains workspace deps only.
                       Numbers and rerun steps: docs/measurements/T1.6.md.

2026-11-16 T4.2 done: implemented GPUI-independent hit_test and priority snap with
                       stable ties, reusable scratch and fixed-point settling for INV9.
                       Approved supporting accessors: Doc::pins, parts, wires, labels;
                       public module/re-exports only otherwise. No connectivity.rs edits.
                       Symbol-local bounds closure avoids the dependency cycle; interaction
                       index envelopes include world pins. Pass &SpatialIndex explicitly,
                       rebuild after edits until Doc owns/incrementally maintains the index.
                       Plan section 9 records all signatures, usize pin indices, label-anchor
                       picking, K1 grid clipping and settling beyond the original radius.
                       Added bench hit in the bench lane using T2.4's 10k generator; JSON
                       follows the existing comparison schema. No new dependencies/licences.
                       Properties: indexed hit == independent brute force across orientations
                       and full-sheet coordinates; INV9 over full grid range and dense
                       off-grid geometry. Boundary, circle, tie, pin-envelope and removal
                       regressions covered. Standard checks pass: fmt, workspace clippy
                       -D warnings, workspace tests, cargo deny (existing warnings only).
                       One overlapping test rebuild encountered a Windows executable lock;
                       completed the earlier run and reran sequentially without weakening checks.
                       bench hit --sheet 10k (20,000 samples, seed 42): p50 0.0008 ms,
                       p99 0.0016 ms, max 0.0553 ms on i7-14650HX / RTX 5060 Laptop /
                       Windows. Non-reference Q4 machine: B7 remains pending on Orion;
                       hardware/protocol/raw report in docs/measurements/T4.2.md and JSON.
                       Started fresh from fetched origin/main f7df2b0. Preserved the old
                       worktree's roadmap edit when detaching its obsolete task branch.

2026-11-16 T4.2 PR #21 review fix: pulled merged main at 976d734. Clamp translated
                       symbol art bounds to K1 before indexing and hit testing; legal
                       edge-straddling parts no longer invalidate the document index or
                       disappear from picking. Regressions cover every sheet edge/corner
                       in all eight orientations, unrelated document items, picking and
                       stable pin snapping. Extended the hit property with overlapping
                       items and exact/just-outside tolerance-boundary probes, retaining
                       its full-sheet ranges and independent geometry/distance oracle.
                       Corrected dense benchmark probe wording and documented clamping
                       in plan section 9. Standard checks rerun; no checks weakened.

2026-11-16 T2.8 needs-human - Doc now owns SpatialIndex and Connectivity, updated on
                       every validated insert/remove/restore and from_items/load. Local art
                       bounds and pins avoid a doc-to-symbols cycle; edge-clamped bodies and
                       pin envelopes preserve T4.2 behavior. Doc::query is allocation-free
                       with capacity; hit/snap and bench hit use the owned index. NetId is
                       valid until the next edit. RON v2 embeds bounds (v1 rejected); equality
                       retains authoritative slots/geometry/revision. Plan signatures updated.
                       INV10 now checks full geometric/logical partitions and internal indices,
                       incidence, counts and free slots, including pins, equal-text label-only
                       components, connectivity ID replacement and legal Doc restore. 38 doc
                       tests and both allocation tests pass. Standard fmt/clippy all-targets
                       -D warnings/workspace tests/deny and diff checks pass; deny only has
                       existing dependency warnings. Overlapping test runs hit Windows exe
                       locks; the full serial rerun passed. No checks weakened.
                       CPU benches moved to omnia-bench with real T2.4 10k generation; added
                       5k-component GND/VCC short/split and 20k-wire middle split. Windows
                       i7-14650HX / RTX 5060 Laptop + UHD, Silent scheme, non-reference:
                       large split mean 805 us, but p99 1.083 ms, max 1.478 ms and 25/1000
                       observations >=1 ms. Not done: reference confirmation and optimization
                       if reproduced remain. Hardware/protocol/raw evidence and other results
                       in docs/measurements/T2.8.md. No T2.5 rendering or gate bypass.
                       T2.4 heading corrected to done: merged PR #19, existing acceptance
                       log/measurements and current generator/round-trip tests justify it.
                       Q4 applies; no new dependencies/licence exceptions. Other worktrees
                       and unrelated untracked parent documents were left unchanged.

2026-11-16 T1.6 blocked — Clean rerun t16-gate2 completed on Orion, 14:01:17–14:14:04
                       EST, measured checkout 1248b7e2df8f9ac2feefac5dd0e47acca60fc148;
                       existing release binary rebuilt 13:53, no concurrent rebuild/run.
                       Q4: i5-7300U / HD 620 Mesa ANV, Wayland, native 1080p at 60.008 Hz,
                       full screen at 100%/125%, AC, powersave, no profile available.
                       80 reports, 160 paired snapshots; all 164 collected files match remote
                       hashes. Protocol checks pass; named apps absent by comm fields
                       (Konsole and desktop services remain). Service success/exit 0;
                       panel scale restored to 1.0; laptop safe to use.
                       B1 median 206.703 / 214.315 ms: PASS (125% max start 545.962 ms
                       retained). B3 frame p99 pan 3.118 / 1.985 ms, zoom 3.507 / 2.282 ms:
                       PASS. B8 zero frames/renders in all 20 runs: PASS. B10 ours 0 in
                       all 40 motion runs: PASS. B3 presents FAIL: 125% pan run 1 has
                       exactly one missed refresh, max interval 49.7746 ms; work p99
                       1.9446 ms, max 3.0802 ms, late/work misses 0. Other 39 runs miss 0.
                       Keep all-run zero-miss rule; no budgets/aggregation weakened.
                       summarize.py exits 1 solely for that miss; cause unproven.
                       Replaced old reports, preserved in git history; updated T1.6.md,
                       retained summary and post-run provenance. gate.sh tracked +x;
                       documented explicit bash invocation for Windows checkout safety.
                       Local Windows standard checks PASS: fmt, workspace clippy with
                       -D warnings, workspace tests (no headless override), cargo deny
                       (existing warnings). git diff --check passes. Performance gate
                       remains blocked; no CI watching or merge in this follow-up.

2026-11-16 T2.8 PR #24 review fix: reject inverted/out-of-K1 local bounds before
                       insert/restore mutates Doc. Regression covers both inverted axes,
                       failed construction, preserved serialized/index state and valid
                       restore/save/load. 39 doc tests plus two allocation tests pass.

2026-11-16 T2.8 blocked — Orion CPU validation of merged main 13b4809 (PR #24 bounds
                       fix + PR #25 trace), isolated source/build, original 1248b7e
                       checkout/gate binary hashes preserved. Q4 i5-7300U, AC,
                       powersave, no power profile service; ten rotating passes/case,
                       1000 observations/operation/run, bounded CPU-only invocations.
                       Section 2.1 gates median run p99, not Criterion mean or max.
                       20k split remove median p99 3.025587 ms, range 2.858351–3.173120,
                       all 10000 removals >=1 ms. Real 10k leaf and 5k GND/VCC bridge
                       below target by that statistic; actual 10k index query recorded.
                       Full protocol not certified: Konsole remains; no foreground
                       rendered window/two-scale viewport verification in CPU-only run.
                       Geometric split/reflood isolated; profile and optimize that path
                       before fresh acceptance, no broad rewrite or budget relaxation.
                       docs/measurements/T2.8-reference.md and raw archive contain
                       provenance, all observations, process/power snapshots and limits.
                       Standard fmt/clippy/workspace tests/deny and runner fmt/clippy
                       pass; diff check passes. No T1.6 outcome/threshold changes.
                       No apps killed or system settings changed; runs exited, Orion
                       safe to use. No rendering work, merge or CI watch.

2026-11-16 T5.1 needs-human — implemented crates/omnia-doc/src/command.rs and undo.rs with
                       minimal Doc/arena/hit/io/lib wiring: TextField, CommandError, all
                       Command variants except Paste (PlacePart, AddWires, AddLabel, Move,
                       Reorient, Delete, SetText, Batch), Change (dirty union of old and new
                       clamped art/pin/wire/label bounds, net-added/removed items,
                       nets_changed), and private History stacks (new committed edit clears
                       redo). Transactional validation checks duplicate/missing IDs, K1
                       limits, INV1 wires, part pins/bounds, and SetText target kinds before
                       mutating Doc; nested Batch failures roll back and truncate freshly
                       appended undo-reserved slots. Inverse operations restore exact arena
                       IDs and keep SpatialIndex, Points, and Connectivity synchronized.
                       INV4 + D2 + D8 reconciliation: Doc::revision() advances monotonically
                       (+1) on every committed apply, undo, and redo (D8), and History stays
                       private outside serialization. Review correction preserves PartialEq
                       over persistent state including revision and every reserved slot;
                       explicit Doc::content_eq compares live items/IDs, interior holes,
                       local bounds and pins for INV4, excluding monotonic revision and
                       trailing undo reservations (D2). History/caches are excluded from both.
                       RON v2 save/load preserves revision and all slots verbatim. PlacePart
                       takes caller-supplied local_pins and local_bounds (no doc->symbols
                       cycle). Plan section 9 updated for Command signatures and equality.
                       Tests (53 omnia-doc unit/proptest + 3 index_alloc integration, including merged PR #28):
                       compound 100-item Move success and failure rollback (including OOB
                       preserving redo stack), duplicate/missing IDs, delete/restore IDs for
                       parts/wires/labels, off-origin Reorient pivots across all 8 Orients
                       including K1-edge-straddling parts, SetText net merge/split, partial
                       dirty rects, nested Batch rollback, proptest INV4+INV10+RONv2 over
                       random legal+invalid sequences, and zero-alloc query/net_wires after
                       commands. Added focused command bench on T2.4 StressSheet::Parts10k
                       (10,000 parts / 20,000 wires, seed 42) in omnia-bench/benches/command.rs
                       (docs/measurements/T5.1.md). Windows dev machine only (ASUS TUF F16
                       FX608JMR, i7-14650HX, AC 100%, Silent scheme, rustc 1.98.1; non-Q4,
                       1 ms threshold unchanged, not certified on Orion): 1,000-obs p50/p99/max
                       for Move 100 parts 0.0588/0.0708/0.1593 ms (undo 0.0582/0.0695/0.1611),
                       100 wires 0.0335/0.0457/0.1327 ms (undo 0.0294/0.0410/0.3586), 100
                       mixed 0.0385/0.0466/0.1435 ms (undo 0.0381/0.0425/0.1063), 0/1000 >= 1 ms.
                       No new deps. All standard checks pass (fmt, clippy -D warnings, test
                       --workspace, deny, git diff --check). Remaining for done: measure and
                       certify the 1 ms Move-100-items benchmark on the Orion reference machine.

2026-11-16 T5.1 PR #29 transaction review fix: validated no-ops preserve revision,
                       undo and redo; Change::dirty is Option<Rect>, with None for
                       no invalidation. Invalid targets still reject without mutation.
                       Tests cover nested empty batches, empty selection operations,
                       identity/zero transforms and unchanged text with redo available.

2026-11-16 W0.3 done — synced canvas-plan.md and canvas-roadmap.md with wiring-plan.md
                       section 13 and the HW1/W0.2 answers: recorded Q6 (Yes, WD3,
                       replaced by W2.1–W2.5); updated section 9 (Move/Reorient
                       FollowPolicy, DragSegment, Delete.with_wires, Snap::Junction,
                       WD5 snap class/ties/projection, Doc::query_within) and INV2
                       (merge keeps labelled points, WD4/WD9); updated T4.3 (WD8/WQ3
                       Intersecting default), T6.1 Read/Do minimally (WD4/WD9 label
                       merge), T6.2 (WD5/WD6 first corner), T6.3 + dependency overview
                       (points to W2.1–W2.5), and T7.2 (WD9 wire-body split). Left
                       T4.2 (`done`) and T5.1 (`needs-human`, code merged) unchanged in
                       substance with pointer notes to W1.3 (PR #41 open) and W2.2.
                       Docs only; nothing measured.

2026-11-16 T6.1 done — added crates/omnia-doc/src/route.rs with `l_route(a, b, flip)`
                       (flip=false leaves `a` horizontally, flip=true vertically; plan
                       section 9 sketch updated) and a pure, deterministic split/merge
                       planner for `AddWires`. `Doc::apply` now splits any wire whose
                       interior an added end lands on, splits an added wire where it
                       passes over an existing end, pin or label point, deduplicates
                       overlapping stretches, and merges collinear segments joined at a
                       point holding nothing else; crossing interiors never split or
                       connect (INV2, M18). One `UndoOp::ReplaceWires` inverse restores
                       the pre-split wires under their old ids (INV4); a split keeps the
                       original id on its first piece and a merge keeps the lowest id,
                       with fresh ids assigned in geometry order, so the result does not
                       depend on the caller's wire order or hash-map order. The merge
                       predicate already keeps a point that carries a net label (WD4,
                       WD9), so W1.4's "make T6.1's merge keep a labelled point" part is
                       done here and W1.4 only needs to verify it. Dependency note: the
                       owner approved starting T6.1 on 2026-11-16 although T5.1 is
                       `needs-human`, because only its Move-of-100-items timing check on
                       the Orion reference machine is outstanding; the commands/undo code
                       is merged. T6.1 changes AddWires timing (Move and Reorient paths
                       are untouched); PR #40's `doc_10k_wire_add` and
                       `doc_chain_20k_add` cases must be re-frozen. Tests: 8 new route
                       tests including a
                       128-case proptest over random AddWires batches asserting INV2, the
                       wire normal form and INV10 against a from-scratch rebuild after
                       every step, plus INV4 undo/redo with exact ids; the existing
                       INV4+INV10 command proptest also passes. All standard checks pass
                       on this Windows development machine (fmt, both clippy gates,
                       cargo test --workspace, cargo deny check); no budget is certified
                       here. Files outside route.rs: command.rs (AddWires only), undo.rs
                       (the ReplaceWires variant), lib.rs (the `route` module line) and
                       canvas-plan.md section 9. No new dependencies. One existing
                       fixture updated: `crates/omnia-doc/tests/index_alloc.rs` built 64
                       contiguous collinear wires and sliced the added ids, which the
                       new merge rule correctly reduces to two wires. Those wires
                       were initially given gaps; this weakened the connected-net
                       workload and is corrected by the T6.1 rework entry below.

2026-11-16 T6.1 rework done — merged origin/main through W0.3/W1.2, retaining
                       main's T6.1 Read/Do wording and all roadmap log entries.
                       AddWires now plans only spatial-query contacts with added
                       segments; fixed touched points permit one boundary extension
                       into collinear neighbours without walking unrelated chains.
                       Point-store ends outside the plan block junction merges.
                       D2 documents this boundary, overlap union and the reviewed id
                       rules: lowest id on merge, first geometry piece on split,
                       fresh ids in geometry order. Labelled points remain (WD4/WD9).
                       Replaced quadratic id membership with hash sets and streamed
                       split coordinates without per-wire cuts/bounds allocations.
                       INV4/INV10 tests preserve ids, geometry and net membership on
                       unrelated Move/Delete/duplicate non-normal states; 128-case
                       properties now generate pins/labels, mix Move/Delete, reverse
                       AddWires inputs and compare redo directly with post-apply ids.
                       Restored the allocation test's connected 64-wire net using
                       an alternating-axis staircase, keeping its allocation checks
                       and asserting all 64 wires are enumerated. l_route now returns
                       [Option<Wire>; 2] without allocation; section 9 updated.
                       WD4 distinguishes Blocker::Overlap for router/follow/segment
                       drag from overlap-unifying user AddWires. No new dependencies.
                       Windows diagnostic (i7-14650HX, UHD Graphics / RTX 5060 Laptop
                       GPU, 1920x1200, 125%, 165 Hz, AC, Silent): median run p99 before
                       -> after: isolated 2k 12.6541 ms -> 16.35 us; isolated 10k
                       187.85155 ms -> 15.75 us; isolated 20k 676.7207 ms -> 17.0 us;
                       bridge 5k 51.50015 ms -> 28.05 us. Ten rotating repetitions,
                       20 samples each; raw data, full ranges, binary hashes, order,
                       condition records and script: docs/measurements/T6.1.md and
                       T6.1/. Shared development session; baseline overlapped builds
                       and tests. Initial out-of-bounds fixture campaign retained as
                       incomplete. Diagnostic only: no reference budget certified.
                       AddWires timing changed; PR #40's doc_10k_wire_add and
                       doc_chain_20k_add must be re-frozen. Follow-up only: T2.4's
                       generator emits non-normal sheets (review report: about 23
                       duplicate wire pairs on 10k and coincident first pins); no
                       generator change here. Changes outside route.rs are the
                       authorized command.rs integration, regression/timing fixtures,
                       plans and measurement records. All five standard checks pass
                       locally: fmt, workspace clippy, feature-enabled bench clippy,
                       workspace tests (including Windows goldens), cargo deny.
2026-11-17 T2.8 done — Orion command-boundary reference campaign attempt-1ORwADgV, source
                       f4fd588, under section 2.1 with the CPU-only rule (owner decision
                       2026-11-17; plan section 2.1 updated). AC, performance governor, no
                       user apps; file indexer at ~8% CPU, accepted by the owner. 20k chain
                       split through Doc::apply: median run p99 0.924 ms (0.872 to 1.051);
                       connectivity-only 0.847 ms; every other add/remove case under 1 ms.
                       Functional done-when checks pass in cargo test --workspace on main.
                       Report: docs/measurements/command-boundary/reference-2026-11-17/.
2026-11-17 T5.1 done — Same campaign: one Move of 100 items on 10k, median run p99 0.200 ms
                       (0.126 to 0.246); the 100 most-connected parts 0.495 ms. Code was
                       merged earlier (needs-human until this reference timing).
2026-11-17 T1.6 blocked — Traced diagnostic campaign (attempt-LBKFmccq) on Orion at the desk,
                       source f8f56b8 (main + PR #56 tty fix), AC, performance governor,
                       other apps closed, 100% then 125%. Complete: 40/40 motion traces, 1
                       missed refresh (125% zoom run 6, frame 195, 33.13 ms interval); all
                       other gate lines pass under tracing. The trace is consistent with a
                       compositor-side skip (temporal association only): Omnia had presented
                       frame 194 and was waiting, two frame callbacks arrived 33 ms apart, and frame 195 took 1.2 ms from
                       signal to present. Diagnostic only; B3, its threshold and the status
                       are unchanged. Owner decides whether a compositor-skipped callback
                       counts against B3. Report: docs/measurements/T1.6/traced/results-2026-11-17/.
2026-11-17 T1.6 blocked — Owner decision 2026-11-17: a refresh that the compositor skips
                       while Omnia is idle does not count as a missed refresh against B3;
                       the same exclusion applies to B4's interval rule. T1.6 closes on a
                       fresh untraced-Wayland gate run with every miss attributed, not on
                       the traced campaign. Plan 2.1 records the decision and the
                       criterion: an interval over the limit is compositor-side only if the
                       trace shows (a) previous present ended before the late signal, (b)
                       signal more than one refresh after it, (c) signal to present end
                       under one refresh, (d) main thread idle (GPUI foreground journal)
                       for more than one refresh before the signal, (e) a native frame
                       callback as signal source, one draw since the previous present and
                       ordered timestamps; else it counts; no usable trace fails the run.
                       omnia-bench trace.rs adds each draw's signal source and the journal's
                       work spans, read once after the last measured frame (journal always
                       on; no per-frame work added). gate.sh sets OMNIA_BENCH_TRACE=1 on
                       pan/zoom only (no WAYLAND_DEBUG), same schedule, new attributed/
                       directory, source and binary hashes, runs summarize.py. attribute.py
                       + 22 tests (real zoom-6 timings: compositor-side; slow draw, stall,
                       non-native source, missing trace, (a)-(d) boundaries: counted).
                       Frame p99 and B10 are judged with the trace on. Windows ABBA
                       diagnostic, pan 125%, 5+5: frame p99 median 0.795 ms traced vs
                       0.848 ms untraced, ours 0 (i7-14650HX, UHD; certifies nothing).
                       Awaiting the Orion rerun.
2026-11-17 T1.6 done — Attributed gate on Orion, merged source 4edac17, all 80 runs
                       and protocol checks passed. B1 startup medians 188.4/192.1 ms
                       (100%/125%); B3 highest median of run p99s 3.499 ms, zero counted
                       missed refreshes across 40 motion runs. The one over-limit
                       interval, s100 pan-3 frame 218 (26.620 ms), met the compositor
                       exclusion in plan 2.1. B8 zero idle frames across 20 runs and
                       B10 zero paint-path allocations per frame in all motion runs.
                       Raw reports, conditions, and verdict: docs/measurements/T1.6.md.
2026-11-17 T2.5 done — D4/D5, K5/K6/K9/K10/K11, G3–G5 and INV7. The document
                       sheet culls through Doc::query, paints snapped wire quads and
                       level-of-detail boxes, and reuses bucketed symbol masks with
                       device-space placement and orientation. Far zoom merges touching
                       collinear wire quads and suppresses the grid below the base pitch
                       cutoff. Windows render_to_image verified horizontal and vertical
                       stroke coverage and a rotated ground at 1.0, 1.25 and 1.5 with
                       fractional pan. The warmed 1k-part paint test counted zero Omnia
                       allocations. T2.6 picks up the renderer for real-sheet benchmarks.
                       U2.2 supplies the live document and invalidation to SheetCanvas
                       through SheetDocument and SheetView::with_document.
2026-11-17 T2.6 done — Real T2.4 sheets in the pan and zoom bench (D4, B10, Q5).
                       Bench scenes reuse T2.5's renderer and attribute Omnia and GPUI
                       allocations separately. The Windows headless gate checks four
                       committed cases (1k/10k × pan/zoom) against the PR base.
                       All reproduced zero Omnia allocations with unchanged primitive
                       peaks and sweep totals. The workflow script
                       rejected one planted allocation per frame and 311 extra quads
                       (2820 to 3131, +11%). Temporary probes were removed. Standard
                       checks passed. The gate also detects growth in the sweep total
                       with unchanged peaks. The local Orion runner records two
                       interleaved revisions, fixed order, binary hashes and conditions.
                       The manual workflow compares recorded campaigns under Q4.
                       Both paths validate full untraced samples, AC power and governor
                       evidence. Runner and comparator were checked with fixtures; no
                       reference timing was run.
2026-11-17 T3.2 done — G12. New test-support crate omnia-golden at tests/golden/ (workspace
                       member; no product crate depends on it). Rule in src/lib.rs: an image
                       passes when at most 1 pixel per 1,000 has some channel (alpha
                       included) more than 2/255 off the golden. The library uses GPUI
                       without test-support, so workspace builds of app and bench do not
                       gain that feature; test targets pass set_scale_factor and
                       render_to_image in through Hooks. render_to_image returns the window
                       at its real DPI, so views are laid out in a fixed logical box in a
                       window 1.5x that size and cropped to the box at each scale factor:
                       goldens are 640x400, 800x500 and 960x600 on any display of 100% or
                       more. tests/sheet.rs (harness = false) loads inverting-amp.ron (all
                       six symbols, FLIP_X and ROT_90, six junctions, a rail across the
                       view; made once from the symbol library through Doc::from_items and
                       Doc::save) and compares 6 PNGs: GRAPHITE and GRAY at 1.0/1.25/1.5.
                       Both themes give byte-identical images: the tokens this fixture
                       paints (stage/paper, stage/grid, stage/ink) are equal in the two
                       themes today (stage/snap differs but is not painted here). cargo xtask golden [--update] runs each
                       suite in xtask/src/golden.rs SUITES with OMNIA_GOLDEN_UPDATE=1 or
                       with it removed. Failures leave .actual.png and .diff.png in
                       target/tmp/golden-sheet. Skips under OMNIA_HEADLESS_CI and on
                       Linux/FreeBSD (no render_to_image, T0.8); updating there is an error.
                       S8 never measured no-GPU readback, so the test runs on Windows
                       development machines as AGENTS.md says, not reference-only.
                       Done-when, Windows dev machine (i7-14650HX, Intel UHD active,
                       Windows 11, 100%): unchanged code 0 differing pixels, largest
                       channel difference 0, in all 6 images on three runs; the rail
                       segment moved down one logical px (127,000 nm, planted in the RON
                       and reverted) failed all 6: 746/953/1147 differing pixels against
                       limits 256/400/576 at 1.0/1.25/1.5. image (plan section 10) is a
                       normal dependency of this test-only crate, already in the graph
                       through GPUI; Cargo.lock gains only omnia-golden. For U1.9: the
                       harness exists now (ui-plan section 14 expected U1.9 to create it);
                       U1.9 dev-depends on omnia-golden, calls omnia_golden::run with its
                       shell view and Suite::new("tests/golden/shell", ...), and adds its
                       (package, test) line to SUITES. T3.1's labels will change these
                       goldens; rerun --update there. T3.3 adds a FIXTURES line.
                       Deviations from Owns: xtask/src/main.rs gains the `golden` and
                       `golden --update` dispatch arms and usage text (beyond the mod line,
                       needed for the cargo xtask golden command); the workspace Cargo.toml
                       gains the tests/golden member; canvas-plan.md section 10 now lists
                       image as a normal dependency of the test-only omnia-golden crate.
2026-11-18 T2.7 needs-human — Owner decision: scope B4 so 10k far zoom is a known limit,
                       and plan option B (T2.9). Investigation of the 2026-11-17 wire-cache
                       campaign: all five counted B4 intervals (49.6 to 54.8 ms) fall in
                       frames 432 to 487 of the 10k zoom sweep, zoom 5.3% to 9.3%. There the
                       whole sheet is in view, 28,279 quads + 1,676 sprites (scene-counts,
                       Windows dev machine, 125%), against 2k to 13k at mid zoom; Orion's
                       present rises from ~1.8 to ~6 ms and frames alternate 20/30 ms
                       (mean interval 24 to 25 ms in frames 440 to 479). Each miss is one paint
                       of 24 to 31 ms (normally ~7.5 ms) at a varying frame (spikes over 15 ms
                       at frames 408 to 519 in 12 of 20 runs); paint max tracks draw max within
                       0.7 ms; Omnia allocations 0. Wire merging joins 46 of 20,000 wires:
                       every far-zoom run is 1 to 2 device px. Row/column rect coalescing
                       removed 1%; an exact pixel-coverage decomposition would leave ~20k to
                       22k quads; dropping runs under 3 device px leaves 8,324. Plan 2.1 now
                       reports 10k-zoom far-zoom (zoom < 31.25%, frames 376 to 545) B4
                       intervals as KNOWN until T2.9; everything else still gates. gate.py
                       implements it. Replaying the archived campaign under the scoped gate
                       passes (4 + 1 known), but it is not a verdict: the rule was written
                       after the results, so T2.7 closes on a fresh Orion run of the scoped
                       gate. D4 records the planned stub rule; T2.9 added.
2026-11-18 T2.7 blocked — Fresh Orion campaign under the scoped B4 (source eda6643, binary
                       235daecc…, 00:15 to 00:45, AC, powersave, same protocol): status fail.
                       B4 passed at both scales: 10k zoom 10/10 with 4 (100%) + 1 (125%)
                       known-limit far-zoom intervals, 41.9 to 52.0 ms at 6.0% to 9.6% zoom;
                       p99 8.704 / 8.544 ms; B5 max 24.183 ms; B9 max 113.008 MB. B2, B5,
                       B9, B10 passed. B3 failed: 100% 1k pan run 9, frame 236, interval
                       26.886 ms (limit 25.0), clause (b); its draw took 12.55 ms against
                       3.5 to 3.8 ms around it, with a normal present (1.92 ms). This is the
                       same one-frame paint stall as the far-zoom misses, at 1k load, so
                       it is not explained by primitive count; c80be1c's campaign had one
                       1k-pan B3 miss too. Not re-run (measurement rules). Next: a diagnostic
                       Orion run that records per-frame main-thread CPU time against wall
                       time around paint, plus scheduler or frequency tracing, to tell
                       preemption from clock changes. Then decide the fix with the owner.
                       Evidence: docs/measurements/T2.7-wire/orion-2026-11-18-scoped/.
2026-11-20 T2.7 done — Owner decision: require the performance CPU governor for Orion GUI
                       gate runs (plan 2.1, matching CPU-only runs) after the T2.7 paint-stall
                       A/B campaign (PR #80) showed powersave clock drops caused the 20 stalls
                       (20 -> 0 under performance; 0 intervals > 2.5 R). Fresh 120-invocation
                       Orion gate under performance (source bd2c036, binary 235daecc…,
                       22:49 to 23:19 EST, AC, performance on all 4 CPUs, fullscreen eDP-1
                       at 60.008 Hz, HD 620 / Mesa Vulkan, kde-inhibit): status pass.
                       Every budget passed at both 100% and 125%: B2 2.415 / 9.033 ms;
                       B3 1k-pan p99 1.226 / 0.918 ms, 1k-zoom 1.606 / 1.119 ms;
                       B4 10k-pan p99 1.563 / 1.186 ms, 10k-zoom 8.055 / 7.952 ms;
                       B5 max 3.877 ms (1k) / 19.464 ms (10k); B9 max 112.719 MB; B10 0 in
                       all 80 motion runs. Zero intervals over each budget's limit across all
                       80 motion runs (1k: 0 over 1.5 R; 10k: 0 over 2.5 R, 0 known-limit
                       far-zoom intervals, 0 compositor-side exclusions); 10k-zoom still had
                       451 two-refresh (1.5 to 2.5 R) intervals at far zoom (worst 40.27 ms /
                       2.42 R in s125/10k-zoom-8), within B4. §2.1 disclosure: harness/agent
                       processes (t3code, cloudflared, agy_acp_server in all 240 snapshots;
                       claude in 23; baloo_file_extr in 8; gh in 2) ran during the campaign
                       (conservative toward failure; noted for owner acknowledgement). Archive
                       replays byte-identically (exit 0). T2.9 remains planned for balanced/
                       powersave laptop headroom and to remove the B4 known limit.
                       Evidence: docs/measurements/T2.7-wire/orion-2026-11-20-performance/.
