# Omnia canvas: plan (reference)

This file is the reference for the schematic canvas: budgets, constants, verified GPUI facts,
decisions, invariants, the day-1 spike and the crate APIs. The work itself is in
[canvas-roadmap.md](canvas-roadmap.md), which lists tasks that cite the IDs defined here.
The original brief is `canvas-plan-prompt.md`; you do not need to read it to do a task.

Written 2026-11-14. No code exists yet.

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `B` | Performance budget, a hard requirement | 2 |
| `K` | Constant: a number every crate must agree on | 3 |
| `G` | Fact about GPUI, read from source at the pinned revision | 4 |
| `D` | Architecture decision | 6 |
| `INV` | Invariant: a rule that must hold in code, each with a test | 7 |
| `S`, `T` | Spike scene and spike threshold | 8 |
| `Q` | Open question for the owner, each with a default | 12 |

Three status words are used:

- **DECIDED**: build to it.
- **HYPOTHESIS**: build to it only after the spike confirms it (section 8).
- **DEFAULT**: the owner has not answered; use the stated default and do not ask again.

## 1. Scope

In scope: the infinite canvas inside the workspace window. Pan, zoom, grid; placing, moving,
rotating and mirroring parts; orthogonal wiring with pin snapping; junctions; net labels;
selection by click, shift-click and rubber band; hover; net highlighting; copy and paste;
undo and redo; the document model under all of it; a minimal save and load for test sheets.

Out of scope: the simulator, instrument internals, the libraries UI, real file formats, the application shell: see [ui-plan.md](ui-plan.md).
Floating instrument windows are in scope only as "views above the sheet that must not hurt
its frame time".

Target: Windows first (GPUI's DirectX 11 renderer), then macOS and Linux.

Repository facts:

- Design sources are under `design/` on `origin/main`: `design/mockups/workspace-graphite.png`,
  `design/mockups/workspace-gray.png`, `design/omnia.figcraft`, `design/build-omnia.mjs` (the
  script that drew the mockup, and the source of the numbers in section 3). Symbols and icons
  are in `vendor/electronic-symbols/` and `vendor/tabler-icons/`. Bare names such as
  `omnia.figcraft` and `build-omnia.mjs` elsewhere in this plan mean these `design/` files.
- `omnia.figcraft` is a zip. `document.json` inside it holds the variable collection `Theme`
  with modes `Graphite` and `Gray`.

## 2. Budgets

Measured on the reference machine (Q4): the Orion laptop, a Lenovo ThinkPad T470 (Intel Core
i5-7300U, 2 cores / 4 threads, Intel HD Graphics 620, 8 GB), Arch Linux, KDE Plasma on
Wayland, GPUI's Linux renderer on Vulkan, on its native 1920x1080 panel at 100% and 125%
scaling, release build, plugged in. Every timed run follows the reference protocol in
section 2.1. `bench` means
`cargo run --release -p omnia-bench --`. Sheets: `empty`, `100`, `1k`, `10k` (10,000 parts and
20,000 wires), `dense`, `text`.

### 2.1 Reference protocol

Answer to Q4 (2026-11-15). The reference machine is below the brief's "mid-range" laptop and
runs Linux, while Windows stays the first shipping platform, so these rules apply:

- **Adapter.** GPUI must render through the Intel HD 620 hardware Vulkan driver (Mesa ANV),
  never llvmpipe. Every result records the Vulkan device; a run on any other device is not a
  reference run.
- **Viewport.** The native panel, 1920x1080 at 60 Hz: a full-screen window at 100% scaling
  (1920x1080 logical) and at 125% (1536x864 logical). No window larger than the panel. The
  owner chose native 1080p over the brief's 4K; thresholds and budgets are otherwise unchanged.
- **Refresh.** The panel refreshes at 60 Hz; B3 and B4 are judged against it (60 and 30 fps).
- **Power and noise.** On AC, with the `performance` CPU governor (owner decision 2026-11-20,
  matching CPU-only runs after the T2.7 paint-stall A/B in PR #80 showed `powersave` clock
  drops caused the paint stalls), the bench window in the foreground, other apps closed. Each
  result records the power source, CPU governor and power profile. Note: Orion GUI gates
  certify under `performance`; laptops on default balanced/`powersave` policies can still hit
  lower-clock paint stretches (T2.9 adds far-zoom headroom).
- **CPU-only runs.** Owner decision 2026-11-17 (gate-blocker audit section 3.2): a timed run
  with no window or GPU work (for example the command-boundary runner) keeps the reference
  hardware, AC, the `performance` governor, other user apps closed, before/after records of
  power, governor, profile and processes, and the repetitions rule below. Adapter, Viewport,
  Refresh and the foreground-window rule do not apply to it. Desktop background services
  (for example the file indexer) are recorded, not stopped.
- **Repetitions.** Ten repetitions per case, interleaved (case order rotates each pass). The
  gate uses the median of run p99s and reports the range.
- **Windows.** Windows numbers (the owner's ASUS TUF F16, Intel UHD adapter) are recorded as
  the Windows development machine. They gate nothing. Windows-only behaviour (DirectX,
  DirectManipulation, `render_to_image`) is still checked there.

| ID | Budget | Measured by |
| --- | --- | --- |
| B1 | Cold start to an interactive empty sheet < 400 ms | `bench start` |
| B2 | Opening the 1,000-part sheet adds < 300 ms | `bench start --sheet 1k` |
| B3 | Pan and zoom, 1,000 parts: 60 fps. p99 frame time < 16.7 ms, and zero missed refreshes: no present interval longer than 1.5 refresh intervals, except compositor-side ones (below) | `bench run --sheet 1k --script pan` and `--script zoom`: `frame_ms.p99`, `missed_refreshes`, attributed from `diagnostic_trace` |
| B4 | Pan and zoom, 10,000 parts and 20,000 wires: 30 fps. p99 frame time < 33.3 ms, and no present interval longer than 2.5 refresh intervals (it never misses its 30 fps slot), except compositor-side ones (below) | `bench run --sheet 10k --script pan` and `--script zoom`: `frame_ms.p99`, `present_intervals_over_2_5_refreshes`, attributed from `diagnostic_trace` |
| B5 | No long stalls: no frame over 33.3 ms in a B3 run, none over 50 ms in a B4 run | `max_frame_ms` in the same report |
| B6 | A dragged part or wire is drawn in the next frame | `bench run --script drag-part` and `drag-wire`: dirty-to-present p99 ≤ one refresh interval |
| B7 | Hit test under the cursor < 0.2 ms at 10,000 parts | `bench hit --sheet 10k` |
| B8 | Idle: 0% CPU and no redraws | `bench idle --seconds 10`: 0 frames drawn |
| B9 | Memory < 300 MB with the 10,000-part sheet open | `memory_mb` in a B4 run |
| B10 | No heap allocation in the paint path once warm | `allocs_per_frame.ours == 0` in every run (scope: Q5) |

A number from any other machine is recorded and labelled, and never marks a budget as met.

B3 to B5 were relaxed by the owner on 2026-11-15 at H1 (the brief asked for 120 fps, never
below 60, with p99 < 8 ms and < 16 ms). Reason: the reference machine's panel refreshes at 60 Hz,
and its HD 620 is GPU-bound on very large sheets. Frame time is GPUI's `work` histogram;
present interval is the spacing of completed presents (G13). Both are CPU-side timestamps,
not compositor scan-out times.

The present-interval halves of B3 and B4 were changed by the owner on 2026-11-16 at T1.6.
They read "median present interval ≤ one refresh" (B3) and "≤ two refreshes" (B4). Reason:
on Orion the median of these CPU-side timestamps sits 14 to 31 µs above one 60 Hz refresh
(16.664 ms) through jitter alone, so the old wording could never pass on a 60 Hz panel,
while missed refreshes measure the intent directly. A present interval longer than 1.5
refresh intervals counts as a missed refresh; `bench run` reports `missed_refreshes`,
`present_intervals_over_2_5_refreshes` and `max_present_interval_ms`. GPUI's own count of
refreshes that a frame's work overran is reported apart, as `work_missed_refreshes`.

Owner decision 2026-11-17 at T1.6, verbatim: "A refresh that the compositor skips while Omnia
is idle does NOT count as a missed refresh against B3. The same exclusion applies to B4's
interval rule." The owner also chose to close T1.6 with a fresh untraced-Wayland gate run in
which every miss is attributed automatically, not by reusing the traced diagnostic campaign.
Reason: in that campaign the only miss (125% zoom run 6, frame 195, 33.13 ms) came after
Omnia had presented frame 194 and waited 31.97 ms for its next frame signal, then took
1.2 ms from signal to present end.

The exclusion is mechanical. Gate runs of B3 and B4 set `OMNIA_BENCH_TRACE=1` on the motion
`run`s, so each report carries `diagnostic_trace`, on one CPU clock: per frame, the signal
time and source, draw start and end, present start and end; and `foreground`, the
main-thread work GPUI's foreground journal recorded (task polls of 100 µs or more, spans of
folded shorter polls, actions, inputs, draws, presents and callbacks that drew nothing),
with `journal_lost`. Let R be one refresh interval, 1/`refresh_hz`, and take a present
interval over the budget's limit (1.5 R for B3, 2.5 R for B4). It is **compositor-side**,
and excluded, only if the trace shows all of:

- (a) the previous frame's present had ended before the late frame's signal;
- (b) the late frame's signal arrived more than R after that present end (strictly);
- (c) the late frame's signal-to-present-end time is under R (strictly);
- (d) Omnia's main thread was idle for more than R before the signal (strictly): take the
  latest end, clipped to the signal, of any `foreground` work that started before the
  signal, or the previous present end if later; the signal comes more than R after it;
- (e) the signal's source is a native platform frame callback, not a locally scheduled
  request (refresh timer, retry or other wakeup); a missing or unmatched source fails;
- and, so that these describe one idle wait and one frame of work: the late present has
  exactly one draw since the previous present, that draw has a signal time, and
  signal ≤ draw start ≤ draw end ≤ present start ≤ present end.

Every other interval over the limit counts. A miss without a usable trace counts: the run
fails closed when its trace is missing, truncated or inconsistent (a measured frame without
its draw-present pair, or a traced interval that differs from the present-end difference),
when the journal is absent or lost entries, and when the bench count (`missed_refreshes`,
`present_intervals_over_2_5_refreshes`) is below the traced count. Bench-counted intervals
the trace does not show count. The limit stays zero counted misses in every run. The
journal is recorded whether or not tracing is on; the bench reads it once, after the last
measured frame. (d) sees what the journal records; the platform event loop's own dispatch
outside a foreground turn, and folded sub-100 µs polls that ended in true idle, are not
recorded; the owner accepted this limit on 2026-11-17. Frame p99 and B10 in a gate run are judged with the trace on. The criterion
is implemented in `docs/measurements/T1.6/attribute.py`, with its tests.

## 3. Constants

All from `build-omnia.mjs` unless marked "chosen". Screen sizes are logical pixels at 100% zoom.

| ID | Constant | Value |
| --- | --- | --- |
| K1 | World unit | 1 nm, stored as `i32`. Sheet limit ±1,000,000,000 nm on each axis. |
| K2 | Grid; pin lattice | 2,540,000 nm; 1,270,000 nm |
| K3 | 100% zoom | 20 px per grid step: `scale = 20.0 / 2_540_000.0` px per nm |
| K4 | Zoom range; wheel step | 5% to 3200%; one notch multiplies by 2^(1/4) (chosen). A notch is 3 `ScrollDelta::Lines`, Windows' default wheel setting, which GPUI multiplies in (T0.5, T1.5); merged or fractional notches zoom by the matching power. |
| K5 | Wire stroke (chosen); symbol stroke | 2 px (254,000 nm) for both wires and symbols (unified 2026-11-26 from `build-omnia.mjs`'s 1.8 px wires so wire and symbol strokes match in world units; wire device-pixel width breaks exact half-pixel ties to even so 125% wires stay centred with anti-aliased pins and junctions, while odd whole-pixel widths remain quantized per D4/G3). Floor: 1 device pixel (Q1). |
| K6 | Junction dot; grid dot | 8 px diameter (1,016,000 nm); 1.2 px, constant on screen, at least 1 device pixel |
| K7 | Symbol art box | 150 units square. Placed size: resistor, capacitor, diode 80 px (4 grid steps); op-amp 180 px (9); ground 48 px (2.4, anchored by its pin). |
| K8 | Text | Designator and value: Inter SemiBold 17 px (Q3), stacked on a shared left edge. Net label and pin number: Inter Medium 13 px (single-pin symbols such as ground omit pin numbers; multi-pin symbols show every pin number). Secondary value text: Inter Medium 12 px (reserved for future multi-field parts; `Part` currently holds one `value` field). |
| K9 | LOD thresholds (chosen) | Text hidden below 6.5 px font size. Pin numbers and junction dots hidden below 2.5 px. Far-zoom merged wire runs hidden below 2.5 px on-screen length once junctions are hidden (D4, T2.9). Symbol drawn as one filled quad when its box is under 8 px. Visible-label cap (T0.6): at most 700 labels per frame; past that, labels hide smallest first. |
| K10 | Grid density (chosen) | Show every 2^k-th grid point with k keeping on-screen pitch in [20, 40) px. Dots about to drop fade over the top quarter of that range: while the pitch is in [35, 40) the next finer level's extra dots are drawn with alpha rising from 0 at 35 px to full at 40 px. Above 200% zoom k is negative (binary subdivisions of the grid, k = −1 being the K2 pin lattice); k runs from −5 at 3200% to 5 at 5%, every level a whole number of nm (2,540,000 = 2^5 × 79,375). (T1.4) |
| K11 | Cache buckets (chosen) | Font size quantised to 0.5 px, capped at 128 px to bound the non-evicting glyph atlas across K4 (T3.1). Symbol sprite sizes on a ladder of ratio 2^(1/4), matching K4. |
| K12 | Spatial index cell (chosen) | 20,320,000 nm (8 grid steps) |
| K13 | Timing | View animations 150 ms, ease-out, interruptible. Background-job debounce 150 ms (chosen). |
| K14 | Pointer tolerances (chosen, tunable) | Hit tolerance 4 px. Snap radius 10 px. |

## 4. GPUI facts

Pinned revision: Zed commit `15d136bf8e72ae43b284183ec46940c009460f5c` (2026-11-14), `gpui`
0.2.2, Apache-2.0. After `cargo fetch` the source is at
`%USERPROFILE%\.cargo\git\checkouts\zed-*\15d136b\crates\gpui`. GPUI changes often and is
thinly documented: before relying on any API, open it at this revision. If a fact below no
longer matches the source, stop and record it in the roadmap log.

| ID | Fact | Where | Consequence |
| --- | --- | --- | --- |
| G1 | `paint_path(path, color)` bins the path's curves into tiles on the CPU on every call, and allocates. Coverage is computed per pixel in the shader; there is no MSAA pass. | `scene.rs` `PaintedPath::new`; `path_winding.rs` `bin` | Paths cost CPU per frame and break B10. |
| G2 | A built `Path` is `Arc<[PathCurve]>` in absolute window pixels. Transforms and stroking exist only on `PathBuilder`, before `build()`. | `path_builder.rs` | A cached path is reusable only while the camera is still. |
| G3 | `paint_quad` snaps bounds to device pixels. A plain rectangle is one primitive; corner radii or a border make up to five. | `window.rs` `paint_quad`, `insert_quad` | Wires are plain quads. Do not draw thousands of rounded quads. |
| G4 | `paint_svg(bounds, key, data: Option<&[u8]>, transformation, color, cx)` rasterises an alpha mask with resvg at 2× device size, caches it in the sprite atlas by (key, size), and emits one tinted `MonochromeSprite` with a rotation and scale matrix. Sampling is linear. | `window.rs` `paint_svg`; `svg_renderer.rs` `SMOOTH_SVG_SCALE_FACTOR` | One primitive per symbol, colour from a token, orientation from the matrix, SVG bytes of our own. |
| G5 | `window.paint_layer(f)` gives everything painted inside one draw order: one bounds-tree insert for the layer, not one per primitive. Inside a layer kinds draw in a fixed order: quads, paths, underlines, monochrome sprites, subpixel sprites, polychrome sprites. Same-kind sprites sort by atlas texture, not paint order. | `window.rs` `paint_layer`; `scene.rs` `push_layer`, `finish` | Paint the sheet in a few layers. Do not rely on paint order inside one. |
| G6 | `AnyView::cached(style)` replays a view's primitives from the previous frame unless that entity was notified. Replay copies every primitive. | `view.rs` `cached`; `scene.rs` `replay` | Hover and instrument frames can skip the sheet's paint code. Replay cost: spike S6. |
| G7 | A frame is drawn only when there is demand: dirty window, next-frame callback, or scene animation. | `window.rs`, search `has_frame_demand` | B8 holds unless we leave a callback running. |
| G8 | `canvas(prepaint, paint)` boxes two `FnOnce` closures per frame. | `elements/canvas.rs` | The sheet implements `Element` directly. |
| G9 | `ScrollEvent { delta: ScrollDelta::{Pixels, Lines}, touch_phase, source, modifiers, .. }`; `ScrollWheelEvent` is a deprecated alias. `PinchEvent { position, delta, phase, modifiers }`. | `interactive.rs` | Use the new names. |
| G10 | On Windows the mouse wheel gives `Lines`. Precision touchpads go through DirectManipulation and give `Pixels` scrolls and pinches; a gesture is classed as scroll or pinch, never both. | `gpui_windows/src/events.rs`, `direct_manipulation.rs` | Lines mean zoom steps, pixels mean pan. Pan during a pinch is not delivered. |
| G11 | `paint_surface` is compiled for macOS and iOS only. | `window.rs` `paint_surface` | A custom GPU surface on Windows needs a patched GPUI (Plan C). |
| G12 | `Window::render_to_image()` (feature `test-support`) renders the current scene to an RGBA image without presenting; the DirectX renderer implements it. | `window.rs`; `gpui_windows/src/directx_renderer.rs` | Golden-image tests need no screenshots. |
| G13 | `window.frame_duration_snapshot()` (feature `profiler`) returns histograms of draw time, dirty-to-present time and present interval. GPUI's spans use the `profiling` crate. | `profiler.rs` `FrameDurationSnapshot` | The bench reads GPUI's own histograms. Tracy works through `profiling`. |
| G14 | Glyph atlas entries are keyed by font, glyph, font size, scale factor and one of 4 horizontal subpixel offsets. `paint_glyph` always uses a unit transform. | `window.rs` `paint_glyph`; `text_system.rs` `SUBPIXEL_VARIANTS_X` | Bucket font sizes (K11). Text cannot be rotated. |
| G15 | `text_system.shape_line(text, font_size, runs, force_width) -> ShapedLine`; `ShapedLine::paint(origin, line_height, align, align_width, window, cx)`. Both Linux (`cosmic_text` `Hinting::Disabled`) and Windows (`DirectWrite` natural measuring) produce size-linear glyph offsets. | `text_system.rs`; `text_system/line.rs` | Cache one base-size `ShapedLine` per label (K8) and scale linearly to the active K11 bucket on direct glyph emission (D6). |
| G16 | Apps start with `gpui_platform::application().run(..)`; `gpui_platform::headless()` also exists. `window.request_animation_frame()` notifies the current view on the next frame. | `gpui_platform/src/gpui_platform.rs`; `window.rs` | Entry point and animation tick. |
| G17 | See ui-plan.md UG17 (input-modality refresh) and UQ15's answer: at most one `SheetView` paint per switch between pointer and keyboard input, counted apart from canvas-caused paints. | ui-plan.md UG17 | Tests that press a key after pointer input bound those paints by the switches (T5.2). |

Not settled by reading, so measured in the spike: whether the sprite atlas ever evicts,
whether a mirrored sprite matrix renders correctly, what `ShapedLine::paint` allocates, and
whether `render_to_image` works on a machine with no GPU.

## 5. Architecture

```
 GPUI input: mouse, keys, ScrollEvent, PinchEvent
        |
        v
 +-- omnia-canvas (GPUI) --------------------------------------------------+
 |  Tool state machine: select | wire | place | drag                       |
 |     | preview state (document untouched)        | Command on commit     |
 |     v                                           v                       |
 |  OverlayView                               Entity<Doc> --> Change       |
 |   selection, hover, net highlight,              |             |         |
 |   snap marker, rubber band, drag ghosts         |             v         |
 |                                                 |      SheetView (cached)
 |  Camera { center: Pt (nm), scale: f64 } --------+--->  grid, wires,     |
 |                                                        symbols, text    |
 +-------------------------------------------------------------------------+
        | plain quads + atlas sprites + glyph runs, inside paint_layer
        v
 GPUI Scene --> DirectX 11 renderer --> present
        ^
        | floating instrument windows: ordinary cached GPUI views above the sheet

 omnia-doc (no GPUI): arenas, commands and undo, connectivity, uniform-grid index
 Background executor: ERC, netlist export, load   <-- snapshot + revision -->  UI thread
```

Data flows one way. Input goes to the active tool. During a gesture the tool holds preview
state and the document does not change. On commit the tool sends one `Command`; `Doc::apply`
returns a `Change` that the views use to decide what to repaint.

## 6. Decisions

### D1 Coordinates: `i32` nanometres. DECIDED

- Exact for metric and mil grids (1 mil = 25,400 nm). The ±1 m limit (K1) means the sum of
  two coordinates fits in `i32`; products use `i64`.
- Camera keeps an integer `center: Pt` reference, an f64 screen translation (including
  fractional-nm gesture movement), and scale in logical px per nm (K3).
- World to screen: subtract the camera centre in integers, multiply in `f64`, then narrow to
  `f32` only at the paint boundary (`world_to_pixels`). The reversible mapping uses
  `Point<f64>` logical pixels: f32 screen coordinates cannot preserve individual nm
  at K3 even within a normal viewport. The `f32` only holds on-screen magnitudes.
- Screen to world: compute in `f64`, round to the nearest nm, then snap in integers.

### D2 Document: arenas with stable ids, commands in, changes out. DECIDED

- One hand-written `Arena<T>` per item kind: a `Vec<Option<T>>` indexed by a `u32` id. Ids
  are never reused in a session, and `restore(id, value)` puts a removed item back under its
  old id. Undo and redo need this; a generational slot map would hand back a new key.
- A wire is one axis-aligned segment. A routed wire is several segments.
- Every edit is a `Command`. `apply` returns a `Change` and pushes the inverse on the undo stack for effective edits. Validated no-ops return `dirty: None` and preserve revision, undo and redo.
- Connectivity, geometric level: a hash map from point to the wire ends and pins at that
  point. The map resolves stable internal point slots; wire endpoints and component membership
  cache those slots for traversal. These slots may be reused after their last incidence is removed
  and are distinct from document IDs. Components grow by union on add. On remove, search from
  both ends in the touched component; a meeting proves a cycle, otherwise transfer only the
  first fully explored disconnected piece and repair the logical net.
- Connectivity, logical level: components carrying the same net label share a `NetId`.
- A connection exists only where an end or pin lies. A wire ending on another wire's body
  splits that wire on commit. A crossing with no end there is not a connection.
- Junction dots are derived, not stored: a dot is drawn where three or more ends or pins meet.
- `AddWires` normalises locally: direct contacts are existing segments with an endpoint
  on an added segment, or containing an added endpoint (inclusive, including collinear
  overlap). Query each added segment's bounds through `Doc::query`; gather pins through
  `Doc::pins` and labels on those segments. Pure interior crossings are excluded.
- Fix the touched points before merging: ends of added segments and direct contacts,
  plus pins and labels on added segments. Collinear existing neighbours incident at
  those points may be included using the point store, but their far ends do not expand
  the touched points. Split this local set at its feature points and unite collinear
  overlaps. Merge only at touched points with exactly two resulting ends and no pin or
  label, counting ends outside the local set as blockers. Junctions and labelled points
  are retained. This preserves normal form for normal input, while leaving pre-existing
  non-normal geometry outside this neighbourhood unchanged; Move normalisation is W2.2.
- W1.4: Delete also merges locally, only at the fixed endpoints of deleted wires.
  After all requested removals, exactly two collinear ends with no pin or label
  may merge there; other points never expand the neighbourhood. Multiple eligible
  joins are united deterministically. The same lowest-id rule applies; a batched
  ReplaceWires inverse restores the exact pre-delete geometry and ids before
  restoring deleted items, and redo reproduces those same ids. Deleting only a
  part or label does not introduce wire endpoints into this merge neighbourhood.
- The lowest existing id survives a merge. The first piece in geometry order keeps a
  split wire's id; fresh ids are assigned in geometry order. All other live ids and
  geometry stay unchanged. Undo restores every replaced wire with its exact id (INV4).

### D3 Spatial index: hand-written uniform grid. DECIDED

- Cell size K12. Lives in `omnia-doc`.
- Schematic items are near one size and grid-aligned, so a uniform grid beats an R-tree:
  no rebalancing, and viewport culling walks cells in order.
- A query deduplicates with a per-item generation stamp, so it allocates nothing.
- Serves culling, hit testing, rubber band and snapping. Doc owns it (T2.8); each part stores
  local art bounds and world pins supplied by the caller, without a symbols dependency.
- A move command removes and reinserts the moved items. During a drag the index is untouched
  because the document is untouched.

### D4 Rendering: plain quads and atlas sprites. DECIDED 2026-11-15 (Plan A, section 8)

- Wires and LOD boxes: plain quads (G3).
- Symbols: monochrome atlas sprites through `paint_svg` (G4). SVG bytes are generated per
  (symbol, size bucket) with the stroke widened so it never falls below one device pixel.
  The size ladder is in device pixels and tops out at 1024 px before the 2× raster.
  The sprite matrix maps that raster to the placed size and orientation.
- Junction and grid dots: plain square quads under about 3 device px, one dot sprite above.
- No `paint_path` and no `canvas()` in the sheet layer (G1, G2, G8). Paths are allowed in the
  overlay, where counts are in the tens.
- Crispness: compute wire rectangles in device pixels ourselves, width
  `max(1, round(world_width × scale × scale_factor))`, origin rounded so the width never
  varies by a pixel; pass GPUI logical values its own snap leaves unchanged.
- Sheet layers, bottom to top: paper and grid; wires and LOD boxes; symbols and dots; text.
- Far zoom (from T0.6, because the reference GPU is the limit on the 10,000-part sheet): merge
  touching collinear wire segments into one quad once K9 hides junctions. At those zooms,
  drop the document grid if the base lattice pitch is below K10's lower bound. Keep the
  T1.6 empty-sheet grid baseline. Draw symbols under K9's box threshold as LOD boxes.
  Labels use direct glyph emission, not one
  `ShapedLine::paint` per label (S5: half the cost), with the visible-label cap in K9.
- Far-zoom wire stubs (T2.9, owner decision 2026-11-18): once K9 hides junctions, draw a
  merged wire run only if its on-screen length is at least K9's stub threshold (2.5 logical
  px), measured after merging, so a long wire made of short segments still draws. On the 10k
  sheet at 5% to 10% zoom every one of the ~20,000 runs is under the threshold and ~70% of
  the quads; dropping them leaves ~8,300 quads (box-only), about the load the 1k and
  mid-zoom frames carry at 60 Hz. The picture changes: at far zoom, parts show without their
  short stubs.
- Fallback if B4 still misses after T2.9: Plan B (section 8). Ask the owner first.

### D5 Level of detail. DECIDED (thresholds K9, K10)

- Order of dropping as zoom falls: text, then pin numbers and junction dots, then symbols
  become boxes. Pin stubs are part of the symbol art and go with it.
- Grid levels are nested, so dots never move, only thin out. Every dot is snapped to a
  device pixel, which prevents moiré at fractional scale factors.

### D6 Text: cached shaped lines, bucketed sizes. DECIDED

- Each label shapes one `ShapedLine` at its 100%-zoom base size (K8) and caches it until its
  text changes or the item is deleted (G15). On paint, metrics and glyph offsets scale linearly
  to the active K11 0.5 px size bucket (capped at `MAX_FONT_PX`) and glyphs emit directly at
  that bucket's font size, so zooming across buckets or moving across scale factors never
  reshapes on the hot path.
- Positions are exact; only the glyph size is bucketed, so the error is under a quarter pixel.
- Labels stay horizontal for every part orientation (G14).
- Labels are culled by the same index query as everything else.

### D7 Invalidation: two views and a cache. DECIDED

- `SheetView` is notified only by: a camera change, a `Change` whose dirty rectangle meets
  the viewport, a theme or scale-factor change, the start and the end of a drag.
- `OverlayView` holds everything transient: selection, hover, net highlight, snap marker,
  rubber band, drag ghosts. Hover, selection and drag moves notify only it.
- `SheetView` is embedded with `.cached()` (G6).
- At drag start the dragged items are hidden from the sheet (one sheet repaint); the overlay
  draws them as ghosts until commit or cancel.
- Net highlight repaints that net's wires in the overlay. It does not recolour the sheet.
- The canvas registers one hitbox for the whole sheet and does its own hit testing.

### D8 Threading. DECIDED

- The UI thread owns `Doc`. Incremental connectivity runs inline with the command; target
  under 1 ms per command at 10,000 parts.
- ERC, netlist export, load, and the full connectivity build after load run on GPUI's
  background executor against a `DocSnapshot` tagged with the document revision. A result
  for a stale revision is dropped and the job re-queued after the debounce (K13).
- Later, simulation samples arrive in a single-producer ring buffer per channel, drained by
  the instrument view on its own frame. Not built in this plan.

Floating instrument windows are ordinary GPUI views above the sheet, each cached. GPUI redraws
the whole window every frame, so their gate is a frame time (roadmap T7.3).

## 7. Invariants

Each has a test; the roadmap task that adds it is in brackets.

| ID | Rule |
| --- | --- |
| INV1 | A wire is axis-aligned and its ends differ. [T2.1] |
| INV2 | The document never holds a wire end on another wire's interior; `apply` splits. Crossing interiors never connect. Merging collinear segments keeps any point that carries a label ([wiring-plan.md](wiring-plan.md) WD4, WD9). [T6.1, W1.4] |
| INV3 | A junction dot exists exactly where three or more ends or pins meet. Dots are never stored. [T2.1] |
| INV4 | The document changes only through `Doc::apply`, `undo`, `redo`. `apply(c)` then `undo()` restores `content_eq` with the original, exact live ids included; `undo` then `redo` likewise. Revision advances monotonically and issued ids remain reserved (D2/D8); persistent-state equality remains stricter. [T5.1] |
| INV5 | During a gesture the document revision does not change. A committed gesture is exactly one undo step. Esc leaves the revision unchanged. [T5.2] |
| INV6 | `omnia-canvas` and `omnia-app` ([ui-plan.md](ui-plan.md) UINV1) contain no colour literal; colours come from `omnia-theme`. [T1.2, U1.1] |
| INV7 | `SheetView`'s paint calls neither `paint_path` nor `canvas()` and makes no heap allocation once warm. [T2.5] |
| INV8 | No `f32` holds a world coordinate. World to screen subtracts the camera centre in integers first. [T1.3] |
| INV9 | Grid snapping is idempotent: `snap_to(snap_to(p, s), s) == snap_to(p, s)`. Priority snapping keeps a snapped point: with `r = snap(p)` and `q = r.at()`, `snap(q) == r` unless a target of a higher WD5 class than `r` lies within the snap radius of `q`; then `snap(q)` is of a higher class. A pin result is therefore always kept. Owner decision 2026-11-16, "radius wins": `snap` never returns a pin, end, junction or body target farther than the radius from `p`, so the earlier `snap(snap(p)) == snap(p)`, which off-grid geometry met only by settling past the radius, no longer applies to `snap` (wiring-plan.md WD5, WINV7). [T1.1, T4.2, W1.3] |
| INV10 | After any command sequence, incremental connectivity equals a from-scratch rebuild. [T4.1] |
| INV11 | A hover, selection or drag-move change, or any shell-only change ([ui-plan.md](ui-plan.md) UINV2), does not notify `SheetView`. [T4.3] |
| INV12 | `omnia-doc` and `omnia-symbols` do not depend on GPUI. [T0.1] |
| INV13 | The GPUI revision changes only through the upgrade routine in section 10. |

## 8. Day-1 spike

`bench spike s1` to `s8`. Scenes are painted from a custom `Element` inside `paint_layer`,
with the camera moving one device pixel per frame for 600 frames, at the reference viewport
(section 2.1: 1920x1080, 100% and 125%). Each run records p50, p95, p99 and max of our paint time, `Window::draw` time,
dirty-to-present and present interval (G13); allocations per frame, ours and total; memory.

| ID | Scene | Sizes | Answers |
| --- | --- | --- | --- |
| S1 | Plain wire-shaped quads plus a dot grid | 5k, 20k, 60k, 100k | Quad throughput |
| S2 | Dots as rounded quads, square quads, one dot sprite | 1k, 5k | Which dot form to use |
| S3 | `paint_svg` sprites of 30 symbols in all 8 orientations | 1k, 5k, 15k | Throughput; pixel difference against a resvg reference at 90° and mirrored; hairline softness beside quad wires; hitch on first use of a size; atlas bytes after sweeping every bucket twice |
| S4 | Stroked symbol paths rebuilt and painted each frame | 100, 500, 1.5k, 5k | Microseconds and allocations per path |
| S5 | Six-glyph labels from cached `ShapedLine`s, then a zoom sweep in 0.5 px buckets | 2k, 6k, 12k | Text throughput; allocations in `paint`; atlas growth; hitch per new size |
| S6 | Cached sheet view under an overlay that notifies every frame | 20k, 60k | Replay cost against repaint cost |
| S7 | Logged real input: wheel, touchpad pan, pinch, mouse drag | n/a | Event shapes, rates and phase order; dirty-to-present during a drag. Needs a person at the touchpad. |
| S8 | Bare GPUI window | n/a | Cold start; idle CPU and frames over 10 s; baseline memory; move between monitors of different scaling; `render_to_image` with and without a GPU |

Frame loads after LOD, whole sheet in view: about 19k primitives at 1,000 parts (13k grid
dots, 4k wire quads, 1k symbols, 1k junctions); about 30k for the text-heavy case (12k
glyphs); about 63k quads at 10,000 parts and 20,000 wires.

Thresholds, p99 on the reference machine. Retuned 2026-11-15 with B3 and B4 (the originals
were 25k quads ≤ 3 ms, 70k ≤ 9 ms, 15k sprites add ≤ 2 ms, 12k glyphs add ≤ 2 ms, from
`Window::draw` getting 5 ms of 8 and 10 of 16). `Window::draw` now gets 12 ms of B3's 16.7 ms
and 25 ms of B4's 33.3 ms, and the loads are the frame loads above instead of padded ones.

| ID | Pass |
| --- | --- |
| T-quad | 19k quads draw in ≤ 12 ms, and 65k (with grid) in ≤ 25 ms |
| T-sprite | 1k sprites add ≤ 2 ms; no visible softness at 90° or mirrored; atlas ≤ 64 MB after the second sweep |
| T-text | 12k glyphs add ≤ 4 ms; zero allocations per painted label once warm |
| T-path | Rebuild and paint ≤ 3 µs per path, zero allocations |
| T-replay | Replaying 60k cached primitives costs ≤ 40% of repainting them |
| T-present | Dirty-to-present ≤ one refresh interval under the 19k load |
| T-gpu | Median present interval ≤ one refresh under the 19k load and ≤ two under the 65k load |

| Outcome | Choice |
| --- | --- |
| T-quad and T-sprite pass | **Plan A** (D4). Expected. |
| T-sprite fails only on rotated or mirrored quality | Plan A with one atlas entry per orientation and a unit matrix |
| T-sprite fails on throughput, T-path passes | Immediate paths for symbols. Unlikely given G1. |
| T-quad passes at 19k, fails at 65k, or T-gpu fails at 65k | Plan A plus a simpler far zoom: merge collinear wire segments and drop the grid below a zoom threshold. Re-measure. |
| Sprites and paths both fail, quads pass | **Plan B**: rasterise sheet content into 512 px tiles on a background thread, show them with `paint_image`, draw live only what is being dragged. About two more weeks. Stop and ask before starting. |
| T-quad fails at 19k | **Plan C**: patch GPUI's renderer to composite an external Direct3D 11 texture and write our own instanced sheet renderer (G11). Stop and ask (Q7). |
| T-text fails | Raise the text threshold in K9 until the visible label count fits; record the count that fits |
| T-replay fails | Hover frames repaint the sheet. Fine if they still meet B3 and B4; otherwise Plan B. |

If a budget is still missed after Plan B, the choice between Plan C, relaxing the budget, and
rendering the sheet outside GPUI belongs to the owner.

The spike's output is `docs/spike-results.md`: numbers, hardware, and the outcome row.

## 9. Crates and APIs

```
omnia/
  Cargo.toml            workspace; gpui and gpui_platform pinned once in [workspace.dependencies]
  rust-toolchain.toml   1.98.1, the version Zed pins at this revision
  crates/
    omnia-doc/          geometry, arenas, commands, connectivity, spatial index. No GPUI.
    omnia-symbols/      symbol definitions compiled from vendor SVGs plus pins.toml. No GPUI.
    omnia-theme/        colour tokens generated from omnia.figcraft plus canvas-tokens.toml
    omnia-canvas/       camera, SheetView, OverlayView, tools
    omnia-bench/        the bench binary: generators, counting allocator, spike scenes
    omnia-app/          window shell; see ui-plan.md section 10
  xtask/                `cargo xtask tokens | symbols | golden [--update]`
  tests/golden/         golden PNGs
  docs/                 spike-results.md, measurements/
```

Signatures are a sketch: names and shapes to build toward, not a frozen interface.

```rust
// ---- omnia-doc ----
pub struct Pt { pub x: i32, pub y: i32 }                 // nanometres
pub struct Rect { pub min: Pt, pub max: Pt }
pub struct Orient(u8);                                    // the 8 rotations and mirrors
impl Orient {
    pub fn apply(self, p: Pt) -> Pt;
    pub fn then(self, next: Orient) -> Orient;
    pub fn inverse(self) -> Orient;
}
pub fn snap_to(p: Pt, step: i32) -> Pt;

pub struct PartId(u32);  pub struct WireId(u32);  pub struct LabelId(u32);  pub struct NetId(u32);
pub enum ItemId { Part(PartId), Wire(WireId), Label(LabelId) }

pub struct Part { pub symbol: SymbolId, pub at: Pt, pub orient: Orient,
                  pub designator: Arc<str>, pub value: Arc<str> }
// P2.1 (parts-plan.md PD8, amended 2026-11-23): Part keeps these five fields; each StoredPart also
// holds def: PartDefId and params (non-default overrides), read via part_def_id/part_params.
pub struct Wire { pub a: Pt, pub b: Pt }                  // INV1
pub struct Label { pub at: Pt, pub text: Arc<str> }

pub enum TextField { Designator, Value, Label }
pub enum CommandError { OutOfBounds, InvalidWire, MissingItem(ItemId),
                        DuplicateItem(ItemId), InvalidTextField { item: ItemId, field: TextField },
                        Follow(FollowError), Segment(Blocker) } // Follow/Segment: wiring-plan.md W2.2, W3.2
pub enum Command {
    PlacePart { part: Part, local_pins: Vec<Pt>, local_bounds: Rect }, // P2.2 adds def: PlaceDef and params: Params (parts-plan.md PD8)
    AddWires(Vec<Wire>),
    AddLabel(Label),
    Move { items: Vec<ItemId>, delta: Pt, follow: FollowPolicy }, // follow added in W2.2 (WD3); until then T5.1 detaches
    Reorient { items: Vec<ItemId>, pivot: Pt, by: Orient, follow: FollowPolicy }, // follow added in W2.2 (WD3)
    Transform { items: Vec<ItemId>, delta: Pt, pivot: Pt, by: Orient, follow: FollowPolicy }, // T5.5: pivot + by(p - pivot) + delta, planned and normalised once
    DragSegment { wire: WireId, offset: i32 }, // wiring-plan.md WD7 (W3.2); nm, signed
    Delete { items: Vec<ItemId>, with_wires: bool }, // with_wires added in W3.5 (WD10, WQ4 default false)
    SetText { item: ItemId, field: TextField, text: Arc<str> },
    // P2.2 (parts-plan.md PD8): SetParam { part, name, value } and UpdatePartDef { from, to }
    Paste { clip: Clip, at: Pt },
    Batch(Vec<Command>),
}
// T7.1 (clip.rs): self-contained copy/paste payload and deterministic origin/designator rules.
// P2.2 will extend ClipPart/Command::Paste to carry embedded PartDef + Params alongside parts.
pub struct ClipPart { pub part: Part, pub local_pins: Vec<Pt>, pub local_bounds: Rect }
pub struct Clip { /* parts: Vec<ClipPart>, wires: Vec<Wire>, labels: Vec<Label> */ }
impl Clip {
    pub fn is_empty(&self) -> bool;
    pub fn item_count(&self) -> usize;
    pub fn parts(&self) -> &[ClipPart];
    pub fn wires(&self) -> &[Wire];
    pub fn labels(&self) -> &[Label];
    pub fn anchor_pin(&self) -> Option<Pt>;              // K2: pin 0 of first pinned part
    pub fn reoriented(&self, by: Orient) -> Self;        // reorients around clip-local (0, 0)
}
pub struct Change { pub revision: u64, pub dirty: Option<Rect>,
                    pub added: Vec<ItemId>, pub removed: Vec<ItemId>, pub nets_changed: bool }
// P2.2 (parts-plan.md PD8, XD7): adds modified: Vec<PartId>, the parts whose params or definition
// changed (SetParam, every instance of an UpdatePartDef) even when geometry and text did not.
// added/removed are membership deltas only: an item that keeps its id with new geometry
// (moved, reoriented, merge survivor, first piece of a split) is in neither; `dirty` covers
// its old and new bounds. Consumers invalidate from dirty/revision, not the id lists.
pub enum Snap { Pin { part: PartId, pin: usize, at: Pt }, WireEnd { wire: WireId, at: Pt },
                Junction { at: Pt }, WireBody { wire: WireId, at: Pt }, Grid(Pt) } // Junction added in W1.3 (WD5)
pub struct Hit { pub item: ItemId, pub at: Pt }

pub struct Doc { /* private, includes undo/redo history */ }
impl Doc {
    pub fn new() -> Self;
    // T2.8: validated bulk construction; local pins and art bounds come from the caller's symbol library.
    pub fn from_items(parts: impl IntoIterator<Item = (Part, Vec<Pt>, Rect)>,
                      wires: impl IntoIterator<Item = Wire>,
                      labels: impl IntoIterator<Item = Label>) -> Result<Self, StoreError>;
    pub fn revision(&self) -> u64; // advances monotonically (+1) on every committed apply, undo, or redo (D8)
    // Validated no-ops return dirty=None with unchanged revision/history/redo.
    pub fn apply(&mut self, cmd: Command) -> Result<Change, CommandError>;
    pub fn undo(&mut self) -> Option<Change>;
    pub fn redo(&mut self) -> Option<Change>;
    pub fn history_cursor(&self) -> u64;      // UD6 (U2.1): current state's id; effective apply → fresh value, undo/redo move to the stored value
    pub fn can_undo(&self) -> bool;           // U2.1
    pub fn can_redo(&self) -> bool;           // U2.1
    pub fn part_count(&self) -> usize;        // U2.1
    pub fn net_count(&self) -> usize;         // U2.1: Connectivity::net_count
    pub fn part(&self, id: PartId) -> Option<&Part>;
    pub fn wire(&self, id: WireId) -> Option<&Wire>;
    pub fn label(&self, id: LabelId) -> Option<&Label>;
    pub fn query(&self, area: Rect, out: &mut Vec<ItemId>); // index envelope (body+pins); used by hit_test and snap
    pub fn query_within(&self, area: Rect, out: &mut Vec<ItemId>); // wiring-plan.md WD8 Enclosed (W3.4; symbol bounds)
    pub fn query_touching(&self, area: Rect, out: &mut Vec<ItemId>); // wiring-plan.md WD8 Intersecting (W3.4; symbol bounds)
    pub fn junctions(&self, area: Rect, out: &mut Vec<Pt>);
    pub fn part_body(&self, id: PartId) -> Option<Rect>; // clamped world body
    pub fn pins(&self, id: PartId) -> Option<&[Pt]>; // world coordinates
    pub fn parts(&self) -> impl Iterator<Item = (PartId, &Part)>;
    pub fn wires(&self) -> impl Iterator<Item = (WireId, &Wire)>;
    pub fn labels(&self) -> impl Iterator<Item = (LabelId, &Label)>;
    pub fn part_def(&self, id: PartDefId) -> Option<&EmbeddedPartDef>; // P2.1 (parts-plan.md PD8): referenced or retained
    pub fn part_defs(&self) -> impl Iterator<Item = (PartDefId, &EmbeddedPartDef)>; // P2.1: referenced only, id order
    pub fn part_def_id(&self, part: PartId) -> Option<PartDefId>;      // P2.1
    pub fn part_params(&self, part: PartId) -> Option<&Params>;        // P2.1 (parts-plan.md PD6): overrides only
    pub fn resolved_param(&self, part: PartId, name: &str) -> Option<&ParamValue>; // P2.1: override, else default
    pub fn net_of(&self, wire: WireId) -> Option<NetId>;
    pub fn net_at(&self, at: Pt) -> Option<NetId>;
    pub fn net_wires(&self, net: NetId, out: &mut Vec<WireId>);
    pub fn copy(&self, items: &[ItemId]) -> Clip;
    pub fn snapshot(&self) -> DocSnapshot;
    pub fn save(&self, w: impl io::Write) -> io::Result<()>;
    pub fn load(r: impl io::Read) -> io::Result<Self>;
    // Version 2 RON embeds local bounds and pins (omnia-symbols depends on omnia-doc).
    // P2.3 (parts-plan.md PD3, PD8, PK9): Version 3 adds the deduplicated part_defs table and per-part def/params;
    // v2 loads by migration to unmodelled definitions. P2.1 extended PartialEq/content_eq per parts-plan.md PD8
    // and added temporary save refusal until v3 lands.
    // Saves retain reserved arena slots and revision; loads validate inserts and rebuild points, index and connectivity.
    pub fn content_eq(&self, other: &Self) -> bool; // live items, exact live IDs, interior gaps, bounds/pins
    // PartialEq compares persistent state, including revision and every reserved slot (pre-T5.1 contract).
    // content_eq ignores revision and trailing undo reservations for INV4 + D2 + D8.
    // History and derived caches are excluded from both comparisons and serialization.
}
// T4.2 interaction functions (re-exported by omnia-doc).
// T2.8: free functions use Doc's owned index and stored body geometry.
pub fn hit_test(doc: &Doc, at: Pt, tolerance: i32,
                scratch: &mut Vec<ItemId>) -> Option<Hit>;
pub fn snap(doc: &Doc, at: Pt, grid: i32, radius: i32,
            scratch: &mut Vec<ItemId>) -> Snap;
impl Snap { pub fn at(self) -> Pt; }
// Live iterators skip holes and preserve ID order. Pins use usize, matching storage.
// Interaction index part entries union transformed symbol bounds and world pins;
// transformed part body bounds are clamped to K1 for both indexing and picking,
// so valid parts whose art straddles the sheet edge remain indexable/pickable.
// Picking uses the clamped body only, labels use anchors, wires use segments.
// Callers convert K14 pixels to nm using camera scale (ceil for inclusive radius).
// Inclusive Euclidean distance; hit ties: part/wire/label then ID.
// snap returns the chosen class (Pin, WireEnd, Junction, WireBody, Grid) per wiring-plan.md WD5 (W1.3).
// Snap ties: class priority (Pin > WireEnd/Junction > WireBody > Grid), i64 squared distance,
// lowest ID, pin index, endpoint coordinates; WireBody projects onto the segment and snaps to
// the nearest grid point still on it (WD5). Reusable scratch buffers.
// Radius wins (owner decision 2026-11-16, WD5): no settling; a non-grid result lies
// within the radius of the input (INV9 as reworded). Grid clips to in-sheet grid lines.
// Doc updates body+pin bounds and connectivity on every validated insert/remove/restore.
// Restore accepts only reserved empty slots; new inserts never reuse arena IDs (D2).
// NetId is valid until the next edit; resolve again from a wire or point after edits.
// RON v2 requires local_bounds; v1 is rejected rather than guessing symbol geometry.
pub const INDEX_CELL_NM: i32;                              // K12
pub struct SpatialIndex { /* private: K12 cells, per-item bounds and generation stamp */ }
impl SpatialIndex {                                        // D3 (T2.2); Send, not Sync
    pub fn new() -> Self;
    pub fn insert(&mut self, item: ItemId, bounds: Rect) -> Result<Option<Rect>, GeomError>;
    pub fn remove(&mut self, item: ItemId) -> Option<Rect>;
    pub fn bounds(&self, item: ItemId) -> Option<Rect>;
    pub fn query(&self, area: Rect, out: &mut Vec<ItemId>); // replaces out; no alloc if room
}
pub struct Connectivity { /* private: point map, components, logical nets */ }
impl Connectivity {                                        // D2, INV10 (T4.1)
    pub fn new() -> Self;
    pub fn clear(&mut self);
    pub fn net_count(&self) -> usize;
    pub fn net_of(&self, wire: WireId) -> Option<NetId>;
    pub fn net_at(&self, at: Pt) -> Option<NetId>;
    pub fn net_wires(&self, net: NetId, out: &mut Vec<WireId>); // replaces out; no alloc if room
    pub fn net_labels(&self, net: NetId, out: &mut Vec<Arc<str>>);
    pub fn rebuild<W, L, LB>(wires: W, labels: L) -> Self
        where W: IntoIterator<Item = (WireId, Wire)>,
              L: IntoIterator<Item = (LabelId, LB)>, LB: std::borrow::Borrow<Label>;
    pub fn rebuild_with_parts<W, L, LB, P, PB>(wires: W, labels: L, parts: P) -> Self
        where W: IntoIterator<Item = (WireId, Wire)>,
              L: IntoIterator<Item = (LabelId, LB)>, LB: std::borrow::Borrow<Label>,
              P: IntoIterator<Item = (PartId, PB)>, PB: AsRef<[Pt]>;
    pub fn add_part(&mut self, id: PartId, world_pins: &[Pt]);
    pub fn remove_part(&mut self, id: PartId) -> Option<Vec<Pt>>;
    pub fn add_wire(&mut self, id: WireId, wire: Wire);
    pub fn remove_wire(&mut self, id: WireId) -> Option<Wire>;
    pub fn add_label(&mut self, id: LabelId, label: &Label);
    pub fn remove_label(&mut self, id: LabelId) -> Option<Label>;
}
pub fn l_route(a: Pt, b: Pt, flip: bool) -> [Option<Wire>; 2];    // T6.1 (route.rs): the L-route;
                                                           // flip=false leaves a horizontally,
                                                           // flip=true vertically. AddWires splits
                                                           // and merges to the INV2 normal form
                                                           // inside apply; a point carrying a net
                                                           // label is never merged away (WD4, WD9).

// ---- omnia-symbols ----
pub struct SymbolId(u16);
pub struct Pin { pub at: Pt, pub number: &'static str }
pub struct SymbolDef { pub id: SymbolId, pub name: &'static str, pub bounds: Rect,
                       pub pins: &'static [Pin], pub art: &'static SymbolArt }
pub struct SymbolLibrary { /* private */ }
impl SymbolLibrary {
    pub fn builtin() -> Arc<Self>;
    pub fn get(&self, id: SymbolId) -> &SymbolDef;
    pub fn by_name(&self, name: &str) -> Option<SymbolId>;
}
impl SymbolArt { pub fn write_svg(&self, stroke_units: f32, out: &mut Vec<u8>); }

// ---- omnia-theme ----
pub struct Rgba { pub r: f32, pub g: f32, pub b: f32, pub a: f32 }
pub struct Theme { pub stage_paper: Rgba, pub stage_ink: Rgba, /* one field per token */ }
pub static GRAPHITE: Theme;
pub static GRAY: Theme;

// ---- omnia-canvas ----
pub struct Camera { /* center: Pt, zoom: f64, translation: Point<f64>, viewport: Bounds<Pixels> */ }
impl Camera {
    pub fn new(center: Pt, zoom: f64, viewport: Bounds<Pixels>) -> Self;
    pub fn world_to_screen(&self, p: Pt) -> Point<f64>;
    pub fn world_to_pixels(&self, p: Pt) -> Point<Pixels>; // final, lossy paint conversion
    pub fn screen_to_world(&self, p: Point<f64>) -> Pt;   // nearest nm, clipped to K1
    pub fn visible(&self) -> Rect;
    pub fn pan_by(&mut self, delta: Point<f64>);
    pub fn zoom_about(&mut self, anchor: Point<f64>, factor: f64);
    pub fn zoom(&self) -> f64;
    pub fn scale(&self) -> f64;
    pub fn set_viewport(&mut self, viewport: Bounds<Pixels>);
}
pub enum Tool { Select, Wire, Place(SymbolId), Label, ZoomArea } // T5.2 (tool.rs); ZoomArea delivered in T5.6
pub enum ToolPhase { Idle, Hovering, Dragging, Placing, Pasting, Wiring, ZoomingArea }
// T5.2 (tool.rs): pure tool state machine. One gesture owner, OverlayView, holds it; the owner
// applies a gesture's one Command only to the document and revision it was planned on (INV5).
pub enum ToolState { Idle, Hovering(ItemId), Dragging(DragGesture),
                     DraggingSegment(SegmentDragGesture), // W3.3
                     Placing(PlaceGesture),
                     Pasting(PasteGesture),               // T7.1
                     Wiring(WireGesture),
                     ZoomingArea(ZoomAreaGesture) } // T5.6
pub struct DocStamp { pub key: u64, pub revision: u64 }   // key: doc_key(&Entity<Doc>)
pub fn doc_key(doc: &Entity<Doc>) -> u64;
pub struct DragGesture { pub items: Vec<ItemId>, pub pressed: ItemId, pub anchor: Pt,
                         pub current: Pt, pub pivot: Pt, pub by: Orient, pub base: DocStamp,
                         /* private anchor_pin, lifted */ }
impl DragGesture {
    pub fn is_lifted(&self) -> bool;                     // T5.3: true once moved/reoriented off start
    pub fn delta(&self) -> Pt;                           // T5.3: anchor-pin snapped to GRID_NM (K2)
    pub fn isometry(&self) -> Isometry;                  // T5.3/T5.5: Transform { delta, pivot, by }
    pub fn command(&self, follow: FollowPolicy) -> Option<Command>; // T5.3/T5.5: Move, Transform, or None
}
// T5.3/T5.5: R, X and Y during a drag compose into `DragGesture.by` (previewed as ghosts) and
// commit as one `Command::Transform` so one undo reverts the whole gesture (INV5).
// W3.3 (tool/segment.rs, re-exported from tool.rs): single-wire perpendicular segment drag.
pub enum SegmentPreview { Accepted, Refused(Blocker) }
pub fn segment_offset(horizontal: bool, anchor: Pt, current: Pt) -> i32;
pub struct SegmentDragGesture { pub wire: WireId, pub original: Wire, pub horizontal: bool,
                                pub anchor: Pt, pub current: Pt, pub base: DocStamp,
                                /* private endpoints, hidden, offset, plan, fallback_wires, preview, lifted, preview_only */ }
impl SegmentDragGesture {
    pub fn start(doc: &Doc, wire: WireId, anchor: Pt, base: DocStamp) -> Option<Self>;
    pub fn for_preview(doc: &Doc, wire: WireId, offset: i32, base: DocStamp) -> Option<Self>;
    pub fn is_pointer_drag(&self) -> bool;
    pub fn is_lifted(&self) -> bool;
    pub fn offset(&self) -> i32;
    pub fn preview(&self) -> SegmentPreview;
    pub fn blocker(&self) -> Option<Blocker>;
    pub fn is_refused(&self) -> bool;
    pub fn hidden_items(&self) -> &[ItemId];
    pub fn preview_wires(&self) -> &[Wire];
    pub fn update(&mut self, doc: Option<&Doc>, at: Pt) -> bool;
    pub fn command(&self) -> Option<Command>;
}
pub struct PlaceGesture { pub symbol: SymbolId, pub at: Option<Pt>, pub orient: Orient,
                          pub pressed: bool, /* private raw_cursor */ } // only a press made in Place places
// T7.1 (tool/paste.rs, re-exported from lib.rs): in-app clipboard Global and ghost paste gesture.
pub struct CanvasClipboard { /* private clip: Option<Arc<Clip>> */ } // impl gpui::Global
pub struct PasteGesture { pub clip: Arc<Clip>, pub at: Option<Pt>, pub orient: Orient,
                          pub pressed: bool, /* private anchor_pin, raw_cursor */ }
impl PasteGesture {
    pub fn new(clip: Arc<Clip>, raw_cursor: Option<Pt>) -> Self;
    pub fn anchor_pin(&self) -> Option<Pt>;
    pub fn in_bounds_at(&self, at: Pt) -> bool;
    pub fn is_in_bounds(&self) -> bool;
}
// T6.2 (tool/wire.rs, re-exported from tool.rs): interactive wire tool with K14/WD5 snapping and WD6 first-corner routing.
pub const SNAP_RADIUS_PX: f64;                            // 10.0 logical px (K14, WK4)
pub fn snap_radius_nm(camera: &Camera) -> i32;
pub fn choose_first_leg(start: Pt, to: Pt) -> Option<Axis>; // WD6: vertical if |dy| > |dx|, else horizontal
pub fn is_terminating_snap(target: Snap) -> bool;         // Pin | WireEnd | Junction | WireBody
pub struct WireGesture { pub start: Option<Pt>, pub cursor: Option<Pt>,
                         pub snap: Option<Snap>, pub first_leg: Option<Axis> }
impl WireGesture {
    pub fn for_preview(start: Option<Pt>, snap: Snap, first_leg: Option<Axis>) -> Self;
    pub fn target_point(&self) -> Option<Pt>;
    pub fn preview_wires(&self) -> [Option<Wire>; 2];
    pub fn update_cursor(&mut self, doc: Option<&Doc>, raw_at: Pt, snap_radius: i32, scratch: &mut Vec<ItemId>) -> bool;
    pub fn flip_corner(&mut self) -> bool;
    pub fn drop_start(&mut self) -> bool;
    pub fn click(&mut self, doc: &Doc, doc_key: u64, raw_at: Pt, snap_radius: i32, scratch: &mut Vec<ItemId>) -> (Option<Commit>, bool);
}
pub struct PointerRadii { pub hit: i32, pub snap: i32 }   // K14/WK4 hit tolerance and snap radius in nm
pub struct ZoomAreaGesture { pub anchor: Option<Pt>, pub current: Option<Pt> } // T5.6
pub enum ToolRequest { Click { item: ItemId, base: DocStamp }, Place { symbol: SymbolId, at: Pt, orient: Orient },
                       Paste { clip: Arc<Clip>, at: Pt, orient: Orient }, // T7.1
                       ReorientSelection(Orient), DeleteSelection, ZoomArea(Rect) } // T5.3, T5.6: handled by OverlayView
pub struct Commit { pub command: Command, pub base: DocStamp }
pub struct ToolOutcome { pub handled: bool, pub preview_changed: bool, pub capture: bool,
                         pub release: bool, pub commit: Option<Commit>,
                         pub request: Option<ToolRequest> }
pub struct ToolMachine { /* tool, state, WiringPrefs, hit scratch */ }
impl ToolMachine {
    pub fn with_prefs(prefs: WiringPrefs) -> Self;
    pub fn tool(&self) -> Tool;
    pub fn state(&self) -> &ToolState;
    pub fn phase(&self) -> ToolPhase;
    pub fn routes_pointer(&self) -> bool;                // T7.1: Placing, Pasting, Wiring, or ZoomingArea
    pub fn holds_capture(&self) -> bool;                 // T5.6: Dragging, pointer-driven DraggingSegment, or active ZoomArea
    pub(crate) fn has_pending_press(&self) -> bool;             // T7.1: Placing or Pasting with pressed=true
    pub fn start_paste(&mut self, clip: Arc<Clip>, raw_cursor: Option<Pt>) -> ToolOutcome; // T7.1
    pub fn set_tool(&mut self, tool: Tool) -> ToolOutcome;  // cancels any gesture first
    pub fn document_replaced(&mut self) -> ToolOutcome;     // drops every gesture, keeps the tool
    pub fn pointer_lost(&mut self) -> ToolOutcome;          // window deactivated
    pub fn hover(&mut self, item: Option<ItemId>) -> ToolOutcome;
    pub fn pointer_down(&mut self, doc: &Doc, doc_key: u64, at: Pt, tolerance: i32,
                        selection: &[ItemId], shift: bool) -> ToolOutcome; // ends a stale drag; wiring uses tolerance*5/2 (100% K14 ratio)
    pub fn pointer_down_with_radii(&mut self, doc: &Doc, doc_key: u64, at: Pt, radii: PointerRadii,
                                   selection: &[ItemId], shift: bool) -> ToolOutcome; // T6.2: live camera K14/WK4 radii
    pub fn pointer_move(&mut self, at: Pt, left_held: bool) -> ToolOutcome;
    pub fn pointer_move_with_doc(&mut self, doc: &Doc, at: Pt, left_held: bool) -> ToolOutcome; // W3.3; wiring defaults to 100% K14 radius (PIN_LATTICE_NM)
    pub fn pointer_move_with_snap(&mut self, doc: &Doc, at: Pt, snap_radius: i32, left_held: bool) -> ToolOutcome; // T6.2: live camera K14/WK4 snap radius
    pub fn pointer_up(&mut self, at: Pt, alt: bool) -> ToolOutcome; // alt: WK3 at commit
    pub fn pointer_up_with_doc(&mut self, doc: &Doc, at: Pt, alt: bool) -> ToolOutcome;       // W3.3
    pub fn flip_wire_corner(&mut self) -> ToolOutcome;    // T6.2: Space while wiring (WD6)
    pub fn cancel(&mut self) -> ToolOutcome;              // Esc; never a commit
    pub fn reorient(&mut self, by: Orient) -> ToolOutcome; // R, X, Y
    pub fn delete(&mut self) -> ToolOutcome;              // Del
}
// T5.2/T5.3: history emitted by the gesture owner (OverlayView) after every effective edit (drag
// commit, place, reorient, delete, undo, redo) and re-emitted by SheetCanvas while both show one
// document. A separate event type rather than a `CanvasEvent` variant, so existing exhaustive
// matches build.
pub struct CanvasHistory { pub doc: u64, pub can_undo: bool, pub can_redo: bool, pub cursor: u64 }
pub struct CanvasEdit { pub doc: u64, pub dirty: Option<Rect> } // OverlayView -> SheetCanvas repaint (D7)
// doc: the source document's doc_key. Events queued in one GPUI update arrive after any
// document switch in it, so SheetCanvas delivers or re-emits one only if doc is the document
// it shows at delivery, through the owner still attached and showing it. Both OverlayView and
// SheetCanvas emit history sealed in `CanvasHistoryEvent` (opaque outside omnia-canvas: no
// public fields, no accessors, opaque Debug); `SheetCanvas::subscribe_history` is the only way
// outside omnia-canvas to receive a `CanvasHistory` (contract C2):
pub struct CanvasHistoryEvent(/* opaque */);              // readable only via subscribe_history
// T5.2 (keymap.rs): actions in namespace `canvas`, resolved in key context "Sheet" (UD3). The
// shell reuses Rotate, MirrorHorizontal, MirrorVertical and PlaceWire in its ACTIONS table and
// generates their R, X, Y and W bindings once from it; it registers nothing from keymap.rs.
pub const SHEET_CONTEXT: &str;                            // "Sheet"
actions!(canvas, [Rotate, MirrorHorizontal, MirrorVertical, PlaceWire, Cancel, Delete, Undo, Redo]);
pub fn standalone_bindings() -> Vec<KeyBinding>;          // r, x, y, w, escape, delete, ctrl-z,
                                                          // ctrl-y; for a canvas without the shell
pub struct CanvasStatus { pub cursor: Option<Pt>, pub grid: i32, pub zoom: f32 } // None: pointer not yet
                                                     // over the sheet; grid: drawn K10 step (T1.5)
pub enum CanvasEvent { Status(CanvasStatus), SelectionChanged }

pub struct SheetDocument { /* entity and persistent paint resources */ }
impl SheetDocument {
    pub fn new(doc: Entity<Doc>) -> Self;
}
pub trait SheetPaintScope {
    fn omnia(&self, paint: impl FnOnce());               // T2.6: count Omnia allocations
    fn gpui(&self, paint: impl FnOnce());                // Q5: exclude GPUI allocations from ours
}
pub struct SheetView { /* camera, theme, optional SheetDocument */ }  // implements Element (G8)
impl SheetView {
    pub fn new(camera: Camera, theme: &'static Theme) -> Self;
    pub fn with_document(camera: Camera, theme: &'static Theme, document: SheetDocument) -> Self;
    pub fn paint_quads(&self, bounds: Bounds<Pixels>, scale_factor: f32, sink: impl FnMut(PaintQuad));
    pub fn paint_with_scope(&self, bounds: Bounds<Pixels>, window: &mut Window, cx: &App,
                           scope: &impl SheetPaintScope);
}
pub fn grid_level(scale: f64) -> GridLevel;              // K10: level, step_nm, pitch, fade
// snap_px: to_device, round_device, snap_logical, stroke_device_px, device_to_logical,
//          device_rect, centered_square — whole device pixels GPUI's snap leaves unchanged (D4)

pub struct Navigator { /* private */ }                    // T1.5 pan/zoom state machine (input.rs)
pub struct SheetCanvas { /* private */ }                  // T1.5: camera, Navigator, SheetView
// T4.3 (selection.rs, overlay.rs): pure selection/hover/WD8 rubber-band state machine and
// OverlayView embedding CachedSheet via `.cached()` so transient updates never repaint SheetView.
// T4.4 (overlay/net.rs): NetHighlight derived from hovered/selected wires (`stage/net-highlight`).
pub struct Selection { /* canonical sorted, deduplicated ItemIds */ }
pub struct RubberBand { pub anchor: Pt, pub current: Pt, pub mode: SelectMode, pub additive: bool }
pub struct SelectionState { /* selection, hovered, rubber_band, SelectMode (WQ3 default Intersecting) */ }
// SelectionState::from_prefs(&WiringPrefs), SelectionState::apply_prefs(&WiringPrefs)
pub struct NetHighlight { /* resolved NetIds and deduplicated WireIds */ }
pub enum OverlayEvent { SelectionChanged }
pub struct CachedSheet { /* SheetView wrapper with paint/render counters (D7, INV11) */ }
pub trait OverlayPaintScope { fn omnia(&self, work: &mut dyn FnMut()); fn gpui(&self, work: &mut dyn FnMut()); }
pub struct OverlayView { /* doc, CachedSheet (.cached()), camera, theme, SelectionState, NetHighlight */ }
impl OverlayView {
    pub fn new(doc: Entity<Doc>, camera: Camera, theme: &'static Theme, cx: &mut Context<Self>) -> Self;
    pub fn with_prefs(doc: Entity<Doc>, camera: Camera, theme: &'static Theme, prefs: &WiringPrefs, cx: &mut Context<Self>) -> Self;
    pub fn with_paint_scope(self, scope: Rc<dyn OverlayPaintScope>) -> Self;
    pub fn set_paint_scope(&mut self, scope: Option<Rc<dyn OverlayPaintScope>>);
    pub fn doc(&self) -> &Entity<Doc>;
    pub fn camera(&self) -> &Camera;
    pub fn theme(&self) -> &'static Theme;
    pub fn focus_handle(&self) -> &FocusHandle;
    pub fn selection(&self) -> &[ItemId];
    pub fn hovered(&self) -> Option<ItemId>;
    pub fn highlighted_nets(&self) -> &[NetId];
    pub fn highlighted_wires(&self) -> &[WireId];
    pub fn rubber_band(&self) -> Option<RubberBand>;
    pub fn select_mode(&self) -> SelectMode;
    pub fn sheet_entity(&self) -> &Entity<CachedSheet>;
    pub fn sheet_paint_count(&self, cx: &App) -> u64;
    pub fn sheet_render_count(&self, cx: &App) -> u64;
    pub fn overlay_paint_count(&self) -> u64;
    pub fn overlay_render_count(&self) -> u64;
    pub fn set_select_mode(&mut self, mode: SelectMode, cx: &mut Context<Self>);
    pub fn apply_prefs(&mut self, prefs: &WiringPrefs, cx: &mut Context<Self>);
    pub fn set_hovered(&mut self, item: Option<ItemId>, cx: &mut Context<Self>);
    pub fn set_selection(&mut self, items: impl IntoIterator<Item = ItemId>, cx: &mut Context<Self>);
    pub fn clear_selection(&mut self, cx: &mut Context<Self>);
    pub fn select_all(&mut self, cx: &mut Context<Self>);
    pub fn cancel_rubber_band(&mut self, cx: &mut Context<Self>);
    pub fn set_rubber_band_preview(&mut self, anchor: Pt, current: Pt, mode: SelectMode, cx: &mut Context<Self>);
    pub fn set_segment_drag_preview(&mut self, wire: WireId, offset: i32, cx: &mut Context<Self>) -> bool; // W3.3
    pub fn set_wire_preview(&mut self, start: Option<Pt>, snap: Snap, first_leg: Option<Axis>, cx: &mut Context<Self>); // T6.2
    pub fn flip_wire_corner(&mut self, cx: &mut Context<Self>) -> bool; // T6.2: Space while wiring (WD6)
    pub fn set_camera(&mut self, camera: Camera, cx: &mut Context<Self>);
    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>);
    pub fn set_document(&mut self, doc: Entity<Doc>, cx: &mut Context<Self>);
    pub fn on_doc_change(&mut self, change: &Change, cx: &mut Context<Self>);
    pub fn viewport(&self) -> Bounds<Pixels>;
    pub fn is_animating(&self) -> bool;
    pub fn cancel_animation(&mut self) -> bool;
    pub fn animate_to(&mut self, target: Camera, cx: &mut Context<Self>);
    pub fn fit_rect(&mut self, rect: Rect, cx: &mut Context<Self>);
    pub fn fit_area(&mut self, rect: Rect, cx: &mut Context<Self>); // T5.6
    pub fn fit_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>);
    pub fn zoom_to_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool;
    pub fn zoom_step(&mut self, steps: i32, window: &mut Window, cx: &mut Context<Self>);
    // T5.2 (tool.rs): the gesture owner. The `Sheet` key context on the focus root; the tool sees
    // the pointer first; set_document and window deactivation drop gestures; emits CanvasHistory
    // and CanvasEdit after every effective edit.
    pub fn tool(&self) -> Tool;
    pub fn tool_state(&self) -> &ToolState;
    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>); // cancels gesture and band
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool; // tool, then band
    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool;
    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool;
    // T5.3 (tool/place.rs, tool/move_.rs): placement provider (parts-plan.md section 14) and delete.
    pub fn set_place_provider(&mut self, provider: Option<PlaceProvider>);
    pub fn delete_selection(&mut self, window: &mut Window, cx: &mut Context<Self>);
    pub fn select_all(&mut self, cx: &mut Context<Self>);
    pub fn has_clip(cx: &App) -> bool;                                // T7.1
    pub fn copy(&self, cx: &mut Context<Self>) -> bool;               // T7.1: never paints SheetView (INV11)
    pub fn cut(&mut self, cx: &mut Context<Self>) -> bool;            // T7.1: one Command::Delete w/ WD10 delete_wires_with_part
    pub fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool; // T7.1: ghost placement (D2, INV11)
}
// T5.3 (tool/place.rs): placement request and shell-supplied provider (parts-plan.md section 14).
pub struct PlaceRequest { pub symbol: SymbolId, pub at: Pt, pub orient: Orient, pub designator: Arc<str> }
pub type PlaceProvider = Rc<dyn Fn(PlaceRequest) -> Option<Command>>;
// T4.5 (camera_anim.rs): 150 ms ease-out view animations driven by `request_animation_frame` (K13, G16).
pub const VIEW_ANIM_DURATION: Duration;                    // 150 ms (K13)
pub const FIT_FILL_FRACTION: f64;                          // 0.9 (5% margin per side)
pub fn ease_out(t: f64) -> f64;
pub fn doc_bounds(doc: &Doc) -> Option<Rect>;
pub fn selection_bounds(doc: &Doc, selection: &[ItemId]) -> Option<Rect>;
pub fn fit_rect_camera(rect: Rect, viewport: Bounds<Pixels>) -> Camera;
pub fn fit_area_camera(rect: Rect, viewport: Bounds<Pixels>) -> Camera;
pub fn fit_sheet_camera(doc: Option<&Doc>, viewport: Bounds<Pixels>) -> Camera;
pub fn zoom_to_selection_camera(doc: &Doc, selection: &[ItemId], viewport: Bounds<Pixels>) -> Option<Camera>;
pub struct CameraAnimation { /* pure K13 ease-out transition, exact target completion */ }
pub struct ViewAnimation { /* GPUI clock-bound wrapper over CameraAnimation */ }
impl SheetCanvas {
    pub fn new(doc: Entity<Doc>, theme: &'static Theme,
               window: &mut Window, cx: &mut Context<Self>) -> Self;
    // The shell document integration (U2.2) supplies the live entity to the T2.5 renderer.
    // Until then the navigation shell uses `new(camera: Camera, theme, cx)` with optional `with_doc`/`set_doc`.
    pub fn with_doc(self, doc: Entity<Doc>) -> Self;
    pub fn set_doc(&mut self, doc: Option<Entity<Doc>>, cx: &mut Context<Self>);
    pub fn doc(&self) -> Option<&Entity<Doc>>;
    pub fn camera(&self) -> &Camera;
    pub fn viewport(&self) -> Bounds<Pixels>;
    pub fn status(&self) -> CanvasStatus;
    pub fn is_animating(&self) -> bool;
    pub fn cancel_animation(&mut self) -> bool;
    pub fn animate_to(&mut self, target: Camera, cx: &mut Context<Self>);
    pub fn fit_rect(&mut self, rect: Rect, cx: &mut Context<Self>);
    pub fn fit_area(&mut self, rect: Rect, cx: &mut Context<Self>); // T5.6
    pub fn fit_doc(&mut self, doc: &Doc, cx: &mut Context<Self>);
    pub fn zoom_to_selection_in(&mut self, doc: &Doc, selection: &[ItemId], cx: &mut Context<Self>) -> bool;
    // T5.2 (tool.rs): tool calls delegate to the attached gesture owner only while it shows this
    // canvas's document; set_doc and set_overlay keep the two aligned (set_doc(None) detaches),
    // and a detached or replaced owner drops its gestures and releases its capture.
    // Without an owner there are no tools, and undo/redo act on the canvas's own document.
    // Emits CanvasHistory.
    pub fn set_overlay(&mut self, overlay: Option<Entity<OverlayView>>, cx: &mut Context<Self>);
    pub fn overlay(&self) -> Option<Entity<OverlayView>>;
    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>);   // T5.2
    pub fn tool(&self, cx: &App) -> Tool;                             // T5.2
    pub fn tool_state(&self, cx: &App) -> ToolState;                  // T5.2
    pub fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool; // T5.2: Esc (UQ14)
    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>); // T5.2: emits CanvasHistory
    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>); // T5.2: emits CanvasHistory
    pub fn focus_handle(&self) -> &FocusHandle;                       // T5.2 (UINV12)
    pub fn subscribe_history<T: 'static>(canvas: &Entity<Self>, cx: &mut Context<T>,
        on_history: impl FnMut(&mut T, &CanvasHistory, &mut Context<T>) + 'static)
        -> Subscription;              // T5.2: drops an event for a document no longer shown
    pub fn is_current_history(&self, history: &CanvasHistory) -> bool; // T5.2
    // T5.3: a shell-supplied placement provider (see parts-plan.md section 14) builds the
    // Command used at Tool::Place(symbol) commit for the snapped point and chosen orientation;
    // defaults to SymbolLibrary::place_command when None.
    pub fn set_place_provider(&mut self, provider: Option<PlaceProvider>, cx: &mut Context<Self>); // T5.3
    pub fn delete_selection(&mut self, window: &mut Window, cx: &mut Context<Self>); // T5.3 (ui-plan §14)
    pub fn select_all(&mut self, cx: &mut Context<Self>); // T5.6 (T4.3 follow-up, ui-plan §14; forwards to OverlayView::select_all)
    pub fn has_clip(cx: &App) -> bool;                   // T7.1: true when CanvasClipboard is non-empty
    pub fn cut(&mut self, cx: &mut Context<Self>);        // T7.1 (ui-plan §14)
    pub fn copy(&self, cx: &mut Context<Self>);           // T7.1 (ui-plan §14)
    pub fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>); // T7.1 (ui-plan §14)
    pub fn set_wiring_prefs(&mut self, prefs: WiringPrefs, cx: &mut Context<Self>); // W2.3 (WD12, ui-plan §14)
    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>);
    pub fn fit_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>);
    pub fn zoom_to_selection(&mut self, window: &mut Window, cx: &mut Context<Self>);
    pub fn zoom_step(&mut self, steps: i32, window: &mut Window, cx: &mut Context<Self>);
    pub fn selection(&self) -> &[ItemId];
}
impl Render for SheetCanvas { /* ... */ }
impl EventEmitter<CanvasEvent> for SheetCanvas {}
impl EventEmitter<CanvasHistoryEvent> for SheetCanvas {}

// T7.3 (instrument_host.rs): animated instrument stub content only (frame/drag/stacking/focus live in U4.1).
pub const DEFAULT_INSTRUMENT_SIZE: (f32, f32);              // (576.0, 468.0), UK10
pub const DEFAULT_TRACE_COLUMNS: usize;                     // 512
pub enum InstrumentRate { Hz60, Hz30 }
pub fn frame_stride(refresh_interval: Option<Duration>, rate: InstrumentRate) -> u32;
pub trait InstrumentPaintScope { fn omnia(&self, work: &mut dyn FnMut()); fn gpui(&self, work: &mut dyn FnMut()); }
pub struct InstrumentStub { /* theme, rate, columns, running, tick, render/paint counters */ }
impl InstrumentStub {
    pub fn new(theme: &'static Theme) -> Self;
    pub fn with_rate(self, rate: InstrumentRate) -> Self;
    pub fn with_columns(self, columns: usize) -> Self;
    pub fn with_running(self, running: bool) -> Self;
    pub fn with_paint_scope(self, scope: Rc<dyn InstrumentPaintScope>) -> Self;
    pub fn set_paint_scope(&mut self, scope: Option<Rc<dyn InstrumentPaintScope>>);
    pub fn theme(&self) -> &'static Theme;
    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>);
    pub fn rate(&self) -> InstrumentRate;
    pub fn set_rate(&mut self, rate: InstrumentRate, cx: &mut Context<Self>);
    pub fn is_running(&self) -> bool;
    pub fn set_running(&mut self, running: bool, cx: &mut Context<Self>);
    pub fn tick_count(&self) -> u64;
    pub fn render_count(&self) -> u64;
    pub fn paint_count(&self) -> u64;
    pub fn phase_rad(&self) -> f64;
    pub fn paint_quads(&self, bounds: Bounds<Pixels>, scale_factor: f32, sink: impl FnMut(PaintQuad));
}

// ---- omnia-bench ----
pub struct CountingAlloc;                                  // #[global_allocator]
pub fn count_allocs<R>(f: impl FnOnce() -> R) -> (R, u64);
pub enum StressSheet { Parts100, Parts1k, Parts10k, DenseWiring, TextHeavy }
// Parts P2.5 adds Embedded10k (--sheet embedded-10k) for the B9 regression run (parts PD11).
pub fn generate(sheet: StressSheet, seed: u64) -> Doc;
// generate module: SplitMix64 seeds; counts (parts, wire segments): 100=(100,200),
// 1k=(1_000,2_000), 10k=(10_000,20_000), dense=(1_000,12_000), text=(1_000,2_000).
// Text has six characters per designator/value, 12,000 total; no extra labels.
// All built-in symbols/orientations, K2 pin-lattice placements; unit-lattice wire segments
// extend from pins in rays (no end on a segment interior, INV2).
```

### Supported-use contract (`OverlayView` and `SheetCanvas`, T5.2)

Preconditions of the public canvas API. INV5 and INV11 remain the primary invariants.

- **C1.** An `OverlayView` belongs to exactly one window. It is mounted in that window for as
  long as it is attached to a `SheetCanvas`. Remounting it in another window is unsupported
  (`debug_assert!` in debug builds, no-op in release on paint or capture).
- **C2.** Outside `omnia-canvas`, history reaches a subscriber only through
  `SheetCanvas::subscribe_history`. That helper delivers an event only if the canvas still
  shows the event's document at delivery time. `CanvasHistoryEvent` is opaque outside the
  crate (no public fields, no accessors, opaque `Debug`); a raw subscription yields nothing
  usable.
- **C3.** A `SheetCanvas` does not subscribe to its own history. (`subscribe_history` reads the
  canvas's current document key from a shared `Rc<Cell<Option<u64>>>` rather than re-borrowing
  the emitting entity, so self-subscription never triggers a GPUI double-borrow panic.)
- **C4.** Every pointer-captured or pending gesture (drag, rubber band, pending place or paste
  press) ends
  by exactly one of: commit to its own document at its stamped revision while it still holds
  capture, or end with no edit. Either way it releases the capture it took, in the window it took
  it in, and never a newer capture. Enforced structurally by a `{ window, hitbox, generation }`
  capture record and a single `OverlayView::end_gesture` path. (`Tool::Wire` takes no pointer
  capture and commits each segment on left press; Esc drops the active chain origin.)

Bench command line. Every subcommand exits by itself and takes `--json <file>`.

| Command | Reports |
| --- | --- |
| `bench run --sheet S --script pan\|zoom\|drag-part\|drag-wire\|hover [--instrument [--instrument-hz 60\|30]] [--frames 600]` | Frame, draw and dirty-to-present ms (p50, p95, p99, max); `allocs_per_frame` (ours, total); `memory_mb`; primitive counts by kind (build with `--features scene-counts`, which needs GPUI's `bench-support`) |
| `bench run --headless --sheet S --script pan\|zoom [--instrument [--instrument-hz 60\|30]]` | Peak and sweep-wide primitive counts and Omnia allocations per frame over the script on the test platform (build with `--features headless`). No frame-time metrics. |
| `bench start [--sheet S] [--runs 10]` | Process start to first interactive frame, median and max |
| `bench idle --seconds N` | Frames drawn; CPU percent |
| `bench hit --sheet S` | Hit-test time, p50 and p99 |
| `bench compare base.json new.json --max-regression 10` | Non-zero exit on a regression over the limit |
| `bench spike s1..s8` | Section 8 |

`omnia-app --frames N` exits after N presented frames, so the app can be run unattended.

Build-time steps:

- **Tokens** (`cargo xtask tokens`). Reads the `Theme` collection from `omnia.figcraft` and
  the overlay `crates/omnia-theme/canvas-tokens.toml` (Q2), writes
  `crates/omnia-theme/src/generated.rs`.
- **Symbols** (`cargo xtask symbols`). The 116 SVGs in `vendor/electronic-symbols/SVG/` carry
  no pin data (`manifest.json` has id, name, category, standard and filename only), and
  their lead ends are slightly off centre (74.94, 74.97, 75.25 in the 150 box). The step
  reads a hand-written `crates/omnia-symbols/pins.toml` giving each symbol's placed size and
  pins, snaps lead ends to the pin lattice (K2), and fails if any pin is off it. The mockup
  puts the op-amp's inputs and the ground pin on half-grid points, which is why the lattice
  is 1.27 mm.

## 10. Dependencies

| Crate | Use | Licence |
| --- | --- | --- |
| `gpui`, `gpui_platform` (git, pinned) | UI, rendering, text, input | Apache-2.0, confirmed in source, including the in-repo crates they pull in |
| `smallvec` | Per-point end lists; `omnia-circuit` member lists ([context-plan.md](context-plan.md)) | MIT or Apache-2.0 |
| `rustc-hash` | Point map; agent preview ids ([mcp-plan.md](mcp-plan.md) section 11.3); `omnia-circuit` key maps ([context-plan.md](context-plan.md)) | MIT or Apache-2.0 |
| `serde`, `ron` | Test-sheet save and load | MIT or Apache-2.0 |
| `profiling`, `tracy-client` (feature `tracy`) | Spans, on the same timeline as GPUI's | MIT or Apache-2.0 |
| `hdrhistogram` | Bench percentiles; `omnia-circuit` diagnostics ([context-plan.md](context-plan.md)) | MIT or Apache-2.0 |
| `serde_json` | Bench JSON reports; the agent wire schema in `omnia-agent` and `omnia-cli` ([mcp-plan.md](mcp-plan.md) section 15); `omnia-circuit` JSON views ([context-plan.md](context-plan.md)) | MIT or Apache-2.0 |
| `libc` 0.2 (`omnia-bench` on Linux only; already in the graph through GPUI) | Diagnostic paint probe: thread CPU clock, `getrusage(RUSAGE_THREAD)`, `sched_getcpu` (T2.7 paint-stall investigation, logged 2026-11-18) | MIT OR Apache-2.0 |
| `resvg` 0.46 (`omnia-bench` feature `readback` only, never default; GPUI already depends on it) | S3/S8 readback reference renders | Apache-2.0 OR MIT |
| GPUI's Linux `wayland` feature, enabled by `omnia-bench` and `omnia-app` on Linux (transitive, listed below) | Wayland window backend | MIT, MIT OR Apache-2.0, or MIT OR Apache-2.0 OR Zlib |
| `usvg`, `zip`, `serde_json`, `toml` (xtask only) | Codegen | Apache-2.0 or MIT; `zip` MIT |
| `proptest`, `criterion`, `image` (dev only, except `image` is a normal dependency of the test-only `omnia-golden` crate, which no product crate depends on) | Property tests, micro-benchmarks, golden comparison | MIT or Apache-2.0 |
| electronic-symbols, Tabler Icons | Symbols, icons | MIT |
| Inter, Source Code Pro | Fonts | SIL OFL 1.1 |

The crates.io licences are from memory and GPUI's transitive tree is unaudited. Roadmap T0.7
adds `cargo deny` with an allow-list of MIT, Apache-2.0, BSD, ISC, Zlib, Unicode and MPL-2.0
(MPL-2.0 added by the owner on 2026-11-14 for cbindgen, dwrote and option-ext, all transitive
through GPUI). Six "unmaintained" advisories reached through GPUI (rustybuzz, ttf-parser,
zbus_names, zvariant, zvariant_derive; chrono added by the owner on 2026-11-17) are ignored by
id in `deny.toml` and re-checked at each GPUI upgrade. A dependency not in this table needs its licence recorded in the roadmap log
before it is added.

Crates the Linux `wayland` feature added to `Cargo.lock` (T0.8; licences from `cargo metadata`,
all inside the `deny.toml` allow-list). MIT: calloop-wayland-source 0.4.1, filedescriptor
0.8.3, is-docker 0.2.0, is-wsl 0.4.0, open 5.4.5, phf, phf_generator, phf_macros and phf_shared
0.13.1, wayland-backend 0.3.17, wayland-client 0.31.15, wayland-cursor 0.31.14,
wayland-protocols 0.32.13, wayland-protocols-plasma 0.3.12, wayland-protocols-wlr 0.3.12,
wayland-scanner 0.31.11, xcursor 0.3.11, xkbcommon 0.8.0, zbus-lockstep and
zbus-lockstep-macros 0.5.2, zbus_xml 5.2.1. MIT OR Apache-2.0: accesskit_atspi_common 0.19.1,
accesskit_unix 0.22.1, atspi 0.29.0, atspi-common and atspi-proxies 0.13.0, downcast-rs 1.2.1,
scoped-tls 1.0.1. MIT OR Apache-2.0 OR Zlib: xkeysym 0.2.1.

GPUI pin, in `[workspace.dependencies]` only:

```toml
gpui = { git = "https://github.com/zed-industries/zed", rev = "15d136bf8e72ae43b284183ec46940c009460f5c" }
gpui_platform = { git = "https://github.com/zed-industries/zed", rev = "15d136bf8e72ae43b284183ec46940c009460f5c" }
```

Upgrade routine, run about monthly or when a needed fix lands, as its own task:

1. Bump the revision on a branch.
2. Diff the files named in section 4 between the two revisions and update the G table.
3. Fix the build.
4. Run the bench and goldens on the reference machine.
5. Merge only if nothing regresses by more than 10% and the goldens match.

## 11. Risks

| Risk | Mitigation | Early test |
| --- | --- | --- |
| GPUI churn | One pinned revision, GPUI confined to three crates, the upgrade routine | T0.1 builds at the pin |
| Primitive throughput on integrated graphics (HD 620 on Linux) | No paths in the sheet layer, layers, LOD, Plans B and C | S1, S3, S4 |
| Text at scale: atlas growth while zooming, cost per label | D6 | S5 |
| Sprites look soft or wrong when mirrored | Per-bucket SVG with adjusted stroke; per-orientation entries | S3 |
| The atlas never evicts | Buckets bound the entry count; cap the largest raster and draw the few visible symbols as overlay paths above it | S3, S5 second sweep |
| Scale-factor changes | Snap in device pixels from the live `scale_factor()`; key caches by it | S8; goldens at 100%, 125%, 150% |
| Touchpad gestures on Windows | G10 | S7 |
| Cold start | Bundle the fonts, rasterise symbols on first use | S8. If a bare window is already near 400 ms, B1 is at risk: record it. |
| GPUI's own allocations against B10 | Report ours and total separately | Counting allocator from T0.3 |
| CI noise: hosted Windows runners have no GPU | Hosted CI gates on deterministic numbers (primitive counts, allocation counts); frame-time gates run on the reference machine | T0.3 runs the bench ten times and records the spread |
| An instrument updating at 60 Hz redraws the whole window | Cached sheet view; throttle instruments to 30 Hz | S6, T7.3 |

T2.6 uses the seeded T2.4 stress sheets (seed 42). Hosted CI checks the `1k` and `10k`
pan and zoom scripts at 1280×800 logical pixels and the test platform's fixed 200% scale,
with 600 warm-up frames and 600 measured frames. The gate checks the Omnia allocation summary
and peak and sweep-wide primitive counts. The manual workflow compares median run p99s and
prints the ranges. Collect the reports by hand on Orion under section 2.1, consistent with Q4.
The local runner interleaves both revisions and retains the order, binary hashes and conditions.

## 12. Open questions

Each has a default. Build to the default; the owner's answer replaces it here.

| ID | Question | Default until answered |
| --- | --- | --- |
| Q1 | Stroke weight across zoom: scale with zoom, or constant on screen? | Scale with zoom, floor of one device pixel (K5). |
| Q2 | Canvas colours are not tokens. Only `stage/paper` is; `build-omnia.mjs` hard-codes the rest. Will tokens be added to the `Theme` collection? | Keep them in `canvas-tokens.toml`, same value in both themes: `stage/ink` `#1D1D1F`, `stage/ink-secondary` `#6B6B70`, `stage/grid` `#00000022`, `stage/select` `#2F6FEB`, `stage/select-halo` `#9DBDFF`, `stage/hover` = select-halo, `stage/net-highlight` = select, `stage/snap` = `accent/mint`, `stage/tag-bg` `#111112`, `stage/tag-text` `#F2F2F2`, `stage/instrument-body` `#FBFAF6`, `probe/a` = select, `probe/b` `#7A5AF8`. A token that later appears in `omnia.figcraft` wins. Shell overlay tokens ([ui-plan.md](ui-plan.md) UQ6: `brand/tile`, `brand/mark`, `instrument/screen`, `focus/ring`) live in the same `canvas-tokens.toml` file. |
| Q3 | Label font: the brief says Source Code Pro for designators and values; the mockup uses Inter SemiBold. | Follow the mockup (K8). |
| Q4 | Which laptop is the reference machine (model, GPU, panel refresh rate)? Can it run gates as a self-hosted CI runner? | **Answered 2026-11-15:** the Orion laptop (Lenovo ThinkPad T470, i5-7300U, Intel HD 620, Arch Linux on Wayland, Vulkan, native 1920x1080 at 60 Hz, 100% and 125%), under the protocol in section 2.1. Self-hosted CI runner: not decided; frame-time gates run by hand on it until then. |
| Q5 | Does B10 cover GPUI's internal allocations? | No: our code only. Total is reported, not gated. |
| Q6 | When a part moves, do attached wires stretch? | **Answered 2026-11-16:** Yes (WQ1) — attached wires follow a moved part per [wiring-plan.md](wiring-plan.md) WD3 (implemented in [wiring-roadmap.md](wiring-roadmap.md) W2.1 to W2.5, replacing T6.3). |
| Q7 | If the spike lands on Plan C, is a patched GPUI fork acceptable? | No default. Stop and ask. |
