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

Out of scope: the simulator, instrument internals, the libraries UI, real file formats.
Floating instrument windows are in scope only as "views above the sheet that must not hurt
its frame time".

Target: Windows first (GPUI's DirectX 11 renderer), then macOS and Linux.

Repository facts:

- Design sources are at the repository root on `origin/main`: `workspace-graphite.png`,
  `workspace-gray.png`, `omnia.figcraft`, `build-omnia.mjs` (the script that drew the mockup,
  and the source of the numbers in section 3), `vendor/electronic-symbols/`,
  `vendor/tabler-icons/`.
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
- **Power and noise.** On AC, the bench window in the foreground, other apps closed. Each
  result records the power source, CPU governor and power profile.
- **Repetitions.** Ten repetitions per case, interleaved (case order rotates each pass). The
  gate uses the median of run p99s and reports the range.
- **Windows.** Windows numbers (the owner's ASUS TUF F16, Intel UHD adapter) are recorded as
  the Windows development machine. They gate nothing. Windows-only behaviour (DirectX,
  DirectManipulation, `render_to_image`) is still checked there.

| ID | Budget | Measured by |
| --- | --- | --- |
| B1 | Cold start to an interactive empty sheet < 400 ms | `bench start` |
| B2 | Opening the 1,000-part sheet adds < 300 ms | `bench start --sheet 1k` |
| B3 | Pan and zoom, 1,000 parts: 60 fps. p99 frame time < 16.7 ms, and zero missed refreshes: no present interval longer than 1.5 refresh intervals | `bench run --sheet 1k --script pan` and `--script zoom`: `frame_ms.p99`, `missed_refreshes` |
| B4 | Pan and zoom, 10,000 parts and 20,000 wires: 30 fps. p99 frame time < 33.3 ms, and no present interval longer than 2.5 refresh intervals (it never misses its 30 fps slot) | `bench run --sheet 10k --script pan` and `--script zoom`: `frame_ms.p99`, `present_intervals_over_2_5_refreshes` |
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

## 3. Constants

All from `build-omnia.mjs` unless marked "chosen". Screen sizes are logical pixels at 100% zoom.

| ID | Constant | Value |
| --- | --- | --- |
| K1 | World unit | 1 nm, stored as `i32`. Sheet limit ±1,000,000,000 nm on each axis. |
| K2 | Grid; pin lattice | 2,540,000 nm; 1,270,000 nm |
| K3 | 100% zoom | 20 px per grid step: `scale = 20.0 / 2_540_000.0` px per nm |
| K4 | Zoom range; wheel step | 5% to 3200%; one notch multiplies by 2^(1/4) (chosen). A notch is 3 `ScrollDelta::Lines`, Windows' default wheel setting, which GPUI multiplies in (T0.5, T1.5); merged or fractional notches zoom by the matching power. |
| K5 | Wire stroke; symbol stroke | 1.8 px (228,600 nm); 2 px (254,000 nm). Floor: 1 device pixel (Q1). |
| K6 | Junction dot; grid dot | 8 px diameter (1,016,000 nm); 1.2 px, constant on screen, at least 1 device pixel |
| K7 | Symbol art box | 150 units square. Placed size: resistor, capacitor, diode 80 px (4 grid steps); op-amp 180 px (9); ground 48 px (2.4, anchored by its pin). |
| K8 | Text | Designator and value: Inter SemiBold 17 px (Q3). Net label and pin number: Inter Medium 13 px. Secondary value text: Inter Medium 12 px. |
| K9 | LOD thresholds (chosen) | Text hidden below 6.5 px font size. Pin numbers and junction dots hidden below 2.5 px. Symbol drawn as one filled quad when its box is under 8 px. Visible-label cap (T0.6): at most 700 labels per frame; past that, labels hide smallest first. |
| K10 | Grid density (chosen) | Show every 2^k-th grid point with k keeping on-screen pitch in [20, 40) px. Dots about to drop fade over the top quarter of that range: while the pitch is in [35, 40) the next finer level's extra dots are drawn with alpha rising from 0 at 35 px to full at 40 px. Above 200% zoom k is negative (binary subdivisions of the grid, k = −1 being the K2 pin lattice); k runs from −5 at 3200% to 5 at 5%, every level a whole number of nm (2,540,000 = 2^5 × 79,375). (T1.4) |
| K11 | Cache buckets (chosen) | Font size quantised to 0.5 px. Symbol sprite sizes on a ladder of ratio 2^(1/4), matching K4. |
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
| G15 | `text_system.shape_line(text, font_size, runs, force_width) -> ShapedLine`; `ShapedLine::paint(origin, line_height, align, align_width, window, cx)`. | `text_system.rs`; `text_system/line.rs` | Cache one `ShapedLine` per label and size bucket. |
| G16 | Apps start with `gpui_platform::application().run(..)`; `gpui_platform::headless()` also exists. `window.request_animation_frame()` notifies the current view on the next frame. | `gpui_platform/src/gpui_platform.rs`; `window.rs` | Entry point and animation tick. |

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
- Every edit is a `Command`. `apply` returns a `Change` and pushes the inverse on the undo stack.
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
  Orientation goes in the sprite matrix.
- Junction and grid dots: plain square quads under about 3 device px, one dot sprite above.
- No `paint_path` and no `canvas()` in the sheet layer (G1, G2, G8). Paths are allowed in the
  overlay, where counts are in the tens.
- Crispness: compute wire rectangles in device pixels ourselves, width
  `max(1, round(world_width × scale × scale_factor))`, origin rounded so the width never
  varies by a pixel; pass GPUI logical values its own snap leaves unchanged.
- Sheet layers, bottom to top: paper and grid; wires and LOD boxes; symbols and dots; text.
- Far zoom (from T0.6, because the reference GPU is the limit on the 10,000-part sheet): merge
  collinear wire segments into one quad, drop the dot grid when its pitch is under K10's
  threshold, and draw symbols as LOD boxes. Labels use direct glyph emission, not one
  `ShapedLine::paint` per label (S5: half the cost), with the visible-label cap in K9.
- Fallback if B4 still misses after far zoom: Plan B (section 8). Ask the owner first.

### D5 Level of detail. DECIDED (thresholds K9, K10)

- Order of dropping as zoom falls: text, then pin numbers and junction dots, then symbols
  become boxes. Pin stubs are part of the symbol art and go with it.
- Grid levels are nested, so dots never move, only thin out. Every dot is snapped to a
  device pixel, which prevents moiré at fractional scale factors.

### D6 Text: cached shaped lines, bucketed sizes. DECIDED

- Each label keeps one `ShapedLine` for the current size bucket (K11, G15). A frame only
  calls `paint`.
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
| INV2 | The document never holds a wire end on another wire's interior; `apply` splits. Crossing interiors never connect. [T6.1] |
| INV3 | A junction dot exists exactly where three or more ends or pins meet. Dots are never stored. [T2.1] |
| INV4 | The document changes only through `Doc::apply`, `undo`, `redo`. `apply(c)` then `undo()` gives a document equal to the original, ids included; `undo` then `redo` likewise. [T5.1] |
| INV5 | During a gesture the document revision does not change. A committed gesture is exactly one undo step. Esc leaves the revision unchanged. [T5.2] |
| INV6 | `omnia-canvas` contains no colour literal; colours come from `omnia-theme`. [T1.2] |
| INV7 | `SheetView`'s paint calls neither `paint_path` nor `canvas()` and makes no heap allocation once warm. [T2.5] |
| INV8 | No `f32` holds a world coordinate. World to screen subtracts the camera centre in integers first. [T1.3] |
| INV9 | `snap(snap(p)) == snap(p)`. [T1.1, T4.2] |
| INV10 | After any command sequence, incremental connectivity equals a from-scratch rebuild. [T4.1] |
| INV11 | A hover, selection or drag-move change does not notify `SheetView`. [T4.3] |
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
    omnia-app/          window shell: menus, toolbars, status bar
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
pub struct Wire { pub a: Pt, pub b: Pt }                  // INV1
pub struct Label { pub at: Pt, pub text: Arc<str> }

pub enum TextField { Designator, Value, Label }
pub enum CommandError { OutOfBounds, InvalidWire, MissingItem(ItemId),
                        DuplicateItem(ItemId), InvalidTextField { item: ItemId, field: TextField } }
pub enum Command {
    PlacePart { part: Part, local_pins: Vec<Pt>, local_bounds: Rect },
    AddWires(Vec<Wire>),
    AddLabel(Label),
    Move { items: Vec<ItemId>, delta: Pt },
    Reorient { items: Vec<ItemId>, pivot: Pt, by: Orient },
    Delete(Vec<ItemId>),
    SetText { item: ItemId, field: TextField, text: Arc<str> },
    Paste { clip: Clip, at: Pt },
    Batch(Vec<Command>),
}
pub struct Change { pub revision: u64, pub dirty: Rect,
                    pub added: Vec<ItemId>, pub removed: Vec<ItemId>, pub nets_changed: bool }
pub enum Snap { Pin { part: PartId, pin: usize, at: Pt }, WireEnd { wire: WireId, at: Pt },
                WireBody { wire: WireId, at: Pt }, Grid(Pt) }
pub struct Hit { pub item: ItemId, pub at: Pt }

pub struct Doc { /* private, includes undo/redo history */ }
impl Doc {
    pub fn new() -> Self;
    // T2.8: validated bulk construction; local pins and art bounds come from the caller's symbol library.
    pub fn from_items(parts: impl IntoIterator<Item = (Part, Vec<Pt>, Rect)>,
                      wires: impl IntoIterator<Item = Wire>,
                      labels: impl IntoIterator<Item = Label>) -> Result<Self, StoreError>;
    pub fn revision(&self) -> u64; // advances monotonically (+1) on every committed apply, undo, or redo (D8)
    pub fn apply(&mut self, cmd: Command) -> Result<Change, CommandError>;
    pub fn undo(&mut self) -> Option<Change>;
    pub fn redo(&mut self) -> Option<Change>;
    pub fn part(&self, id: PartId) -> Option<&Part>;
    pub fn wire(&self, id: WireId) -> Option<&Wire>;
    pub fn label(&self, id: LabelId) -> Option<&Label>;
    pub fn query(&self, area: Rect, out: &mut Vec<ItemId>);
    pub fn junctions(&self, area: Rect, out: &mut Vec<Pt>);
    pub fn part_body(&self, id: PartId) -> Option<Rect>; // clamped world body
    pub fn pins(&self, id: PartId) -> Option<&[Pt]>; // world coordinates
    pub fn parts(&self) -> impl Iterator<Item = (PartId, &Part)>;
    pub fn wires(&self) -> impl Iterator<Item = (WireId, &Wire)>;
    pub fn labels(&self) -> impl Iterator<Item = (LabelId, &Label)>;
    pub fn net_of(&self, wire: WireId) -> Option<NetId>;
    pub fn net_at(&self, at: Pt) -> Option<NetId>;
    pub fn net_wires(&self, net: NetId, out: &mut Vec<WireId>);
    pub fn copy(&self, items: &[ItemId]) -> Clip;
    pub fn snapshot(&self) -> DocSnapshot;
    pub fn save(&self, w: impl io::Write) -> io::Result<()>;
    pub fn load(r: impl io::Read) -> io::Result<Self>;
    // Version 2 RON embeds local bounds and pins (omnia-symbols depends on omnia-doc).
    // Saves retain reserved arena slots and revision; loads validate inserts and rebuild points, index and connectivity.
    // T5.1 (INV4 + D2 + D8): Equality compares authoritative slots (exact live IDs and interior gaps),
    // local bounds and pins, excluding monotonic revision, trailing undo-reserved holes, history, and derived state.
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
// Inclusive Euclidean distance; hit ties: part/wire/label then ID. Snap ties:
// priority, distance, ID, pin index, endpoint coordinates. Reusable scratch buffers.
// Snap settles through higher-priority targets after rounding/projection (INV9),
// so the final point can exceed the initial radius. Grid clips to in-sheet grid lines.
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
pub enum Tool { Select, Wire, Place(SymbolId), Label }
pub struct CanvasStatus { pub cursor: Option<Pt>, pub grid: i32, pub zoom: f32 } // None: pointer not yet
                                                     // over the sheet; grid: drawn K10 step (T1.5)
pub enum CanvasEvent { Status(CanvasStatus), SelectionChanged }

pub struct SheetView { /* camera: Camera, theme: &'static Theme */ }  // implements Element (G8)
impl SheetView {
    pub fn new(camera: Camera, theme: &'static Theme) -> Self;
    pub fn paint_quads(&self, bounds: Bounds<Pixels>, scale_factor: f32, sink: impl FnMut(PaintQuad));
}
pub fn grid_level(scale: f64) -> GridLevel;              // K10: level, step_nm, pitch, fade
// snap_px: to_device, round_device, snap_logical, stroke_device_px, device_to_logical,
//          device_rect, centered_square — whole device pixels GPUI's snap leaves unchanged (D4)

pub struct Navigator { /* private */ }                    // T1.5 pan/zoom state machine (input.rs)
pub struct SheetCanvas { /* private */ }                  // T1.5: camera, Navigator, SheetView
impl SheetCanvas {
    pub fn new(doc: Entity<Doc>, theme: &'static Theme,
               window: &mut Window, cx: &mut Context<Self>) -> Self;
    // Until the document is wired in (T2.5) it is `new(camera: Camera, theme, cx)`.
    pub fn camera(&self) -> &Camera;
    pub fn status(&self) -> CanvasStatus;
    pub fn set_tool(&mut self, tool: Tool, cx: &mut Context<Self>);
    pub fn set_theme(&mut self, theme: &'static Theme, cx: &mut Context<Self>);
    pub fn fit_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>);
    pub fn zoom_to_selection(&mut self, window: &mut Window, cx: &mut Context<Self>);
    pub fn selection(&self) -> &[ItemId];
}
impl Render for SheetCanvas { /* ... */ }
impl EventEmitter<CanvasEvent> for SheetCanvas {}

// ---- omnia-bench ----
pub struct CountingAlloc;                                  // #[global_allocator]
pub fn count_allocs<R>(f: impl FnOnce() -> R) -> (R, u64);
pub enum StressSheet { Parts100, Parts1k, Parts10k, DenseWiring, TextHeavy }
pub fn generate(sheet: StressSheet, seed: u64) -> Doc;
// generate module: SplitMix64 seeds; counts (parts, wire segments): 100=(100,200),
// 1k=(1_000,2_000), 10k=(10_000,20_000), dense=(1_000,12_000), text=(1_000,2_000).
// Text has six characters per designator/value, 12,000 total; no extra labels.
// All built-in symbols/orientations, K2 pin-lattice placements; unit-lattice wire segments
// extend from pins in rays (no end on a segment interior, INV2).
```

Bench command line. Every subcommand exits by itself and takes `--json <file>`.

| Command | Reports |
| --- | --- |
| `bench run --sheet S --script pan\|zoom\|drag-part\|drag-wire\|hover [--frames 600]` | Frame, draw and dirty-to-present ms (p50, p95, p99, max); `allocs_per_frame` (ours, total); `memory_mb`; primitive counts by kind (build with `--features scene-counts`, which needs GPUI's `bench-support`) |
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
| `smallvec` | Per-point end lists | MIT or Apache-2.0 |
| `rustc-hash` | Point map | MIT or Apache-2.0 |
| `serde`, `ron` | Test-sheet save and load | MIT or Apache-2.0 |
| `profiling`, `tracy-client` (feature `tracy`) | Spans, on the same timeline as GPUI's | MIT or Apache-2.0 |
| `hdrhistogram` | Bench percentiles | MIT or Apache-2.0 |
| `serde_json` | Bench JSON reports | MIT or Apache-2.0 |
| `resvg` 0.46 (`omnia-bench` feature `readback` only, never default; GPUI already depends on it) | S3/S8 readback reference renders | Apache-2.0 OR MIT |
| GPUI's Linux `wayland` feature, enabled by `omnia-bench` and `omnia-app` on Linux (transitive, listed below) | Wayland window backend | MIT, MIT OR Apache-2.0, or MIT OR Apache-2.0 OR Zlib |
| `usvg`, `zip`, `serde_json`, `toml` (xtask only) | Codegen | Apache-2.0 or MIT; `zip` MIT |
| `proptest`, `criterion`, `image` (dev only) | Property tests, micro-benchmarks, golden comparison | MIT or Apache-2.0 |
| electronic-symbols, Tabler Icons | Symbols, icons | MIT |
| Inter, Source Code Pro | Fonts | SIL OFL 1.1 |

The crates.io licences are from memory and GPUI's transitive tree is unaudited. Roadmap T0.7
adds `cargo deny` with an allow-list of MIT, Apache-2.0, BSD, ISC, Zlib, Unicode and MPL-2.0
(MPL-2.0 added by the owner on 2026-11-14 for cbindgen, dwrote and option-ext, all transitive
through GPUI). Five "unmaintained" advisories reached through GPUI (rustybuzz, ttf-parser,
zbus_names, zvariant, zvariant_derive) are ignored by id in `deny.toml` and re-checked at each
GPUI upgrade. A dependency not in this table needs its licence recorded in the roadmap log
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

## 12. Open questions

Each has a default. Build to the default; the owner's answer replaces it here.

| ID | Question | Default until answered |
| --- | --- | --- |
| Q1 | Stroke weight across zoom: scale with zoom, or constant on screen? | Scale with zoom, floor of one device pixel (K5). |
| Q2 | Canvas colours are not tokens. Only `stage/paper` is; `build-omnia.mjs` hard-codes the rest. Will tokens be added to the `Theme` collection? | Keep them in `canvas-tokens.toml`, same value in both themes: `stage/ink` `#1D1D1F`, `stage/ink-secondary` `#6B6B70`, `stage/grid` `#00000022`, `stage/select` `#2F6FEB`, `stage/select-halo` `#9DBDFF`, `stage/hover` = select-halo, `stage/net-highlight` = select, `stage/snap` = `accent/mint`, `stage/tag-bg` `#111112`, `stage/tag-text` `#F2F2F2`, `stage/instrument-body` `#FBFAF6`, `probe/a` = select, `probe/b` `#7A5AF8`. A token that later appears in `omnia.figcraft` wins. |
| Q3 | Label font: the brief says Source Code Pro for designators and values; the mockup uses Inter SemiBold. | Follow the mockup (K8). |
| Q4 | Which laptop is the reference machine (model, GPU, panel refresh rate)? Can it run gates as a self-hosted CI runner? | **Answered 2026-11-15:** the Orion laptop (Lenovo ThinkPad T470, i5-7300U, Intel HD 620, Arch Linux on Wayland, Vulkan, native 1920x1080 at 60 Hz, 100% and 125%), under the protocol in section 2.1. Self-hosted CI runner: not decided; frame-time gates run by hand on it until then. |
| Q5 | Does B10 cover GPUI's internal allocations? | No: our code only. Total is reported, not gated. |
| Q6 | When a part moves, do attached wires stretch? | No. Roadmap T6.3 stays blocked. |
| Q7 | If the spike lands on Plan C, is a patched GPUI fork acceptable? | No default. Stop and ask. |
