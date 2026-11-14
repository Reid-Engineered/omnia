# Prompt: plan the Omnia workspace canvas

You are planning the schematic canvas for **Omnia**, a desktop schematic capture and simulation app written in Rust with **GPUI** (Zed's UI framework), targeting Windows first (DirectX 11 renderer), then macOS and Linux. Your job is to produce an implementation plan, not code. The plan must get a canvas that feels excellent early, and must keep it fast as features land.

## Read first
- `mockups/omnia/workspace-graphite.png` and `workspace-gray.png`: the target look. Canvas behaviour should match these exactly: paper-coloured sheet, dot grid, black wires, solder-dot junctions, part labels, net labels and floating instrument windows.
- `mockups/omnia/omnia.figcraft`: the source file with the design tokens (Graphite and Gray themes). Tokens are the only source of colour; no hex values in canvas code.
- `mockups/omnia/vendor/electronic-symbols/` (Chris Pikul, MIT): the part symbols, drawn in a 150-unit box with pins on the box edge. `vendor/tabler-icons/` (MIT): UI icons. Fonts: Inter for UI, Source Code Pro for values and designators.
- The GPUI source at the exact git revision you pin. Check every GPUI API you rely on against that source (`canvas`, `PathBuilder`, `paint_path`, `paint_quad`, `svg`, text shaping, input events, `request_animation_frame`). Do not plan from memory of GPUI's API; it changes often and has little documentation.

## Scope
In scope: the infinite canvas inside the workspace window. This covers pan, zoom, grid, placing, moving, rotating and mirroring parts, wiring with orthogonal routing and pin snapping, junctions, net labels, selection (click, shift-click, rubber band), hover, net highlighting, copy/paste, undo/redo, and the document model under all of it.
Out of scope for this plan: the simulator, instruments' internals, the libraries UI and file formats beyond a minimal save/load used for test sheets. Floating instrument windows only need a plan for how they sit above the canvas without hurting its frame time.

## Performance budgets (hard requirements)
Measure every budget on a mid-range Windows laptop with integrated graphics, at 4K and 125%/150% scaling, in release builds.
- Cold start to an interactive empty sheet: < 400 ms. Opening a 1,000-part sheet: < 300 ms more.
- Pan and zoom: 120 fps target, never below 60 fps. Measure p99 frame time, not the average: < 8 ms at 1,000 parts and < 16 ms at 10,000 parts and 20,000 wires.
- Input to photon: dragging a part or wire is drawn in the next frame, with no frame of lag.
- Hit testing under the cursor: < 0.2 ms at 10,000 parts.
- Steady-state idle: 0% CPU, with no redraws when nothing changes.
- Memory: < 300 MB for the 10,000-part sheet.
- No heap allocation in the per-frame paint path once warm. Verify this with a counting allocator in tests.

## Architecture questions the plan must answer, with a decision and a reason for each
1. **Coordinates.** Use integer world units (for example i64 nanometres like KiCad, or i32 at 1 µm) so snapping and equality are exact. Say how world maps to screen in f32 without precision loss at extreme pan and zoom.
2. **Document model and view separation.** Choose a document store (an arena or ECS-style IDs, not `Rc` graphs). Edits go in as commands, which gives undo/redo and later collaboration and RIS-C netlist export. Connectivity (nets) must update incrementally, not by rebuilding after every edit.
3. **Spatial index.** Choose one, such as an R-tree (`rstar`) or a uniform grid. It is used for culling, hit testing, rubber-band selection and wire snapping. Plan how it updates on move.
4. **Rendering strategy in GPUI.** This is the biggest risk. GPUI paths are rasterised through its path pipeline, and thousands of `paint_path` calls per frame may be slow. The plan must start with a spike that measures how many paths, quads and glyph runs GPUI can draw per frame on the target machine. Use the results to choose between:
   - (a) immediate paths with caching of tessellated symbol geometry per (symbol, rotation, mirror, zoom bucket);
   - (b) symbols as cached sprites or atlas images, re-rendered when the zoom bucket changes;
   - (c) a custom wgpu or DirectX surface embedded in GPUI for the sheet layer only;
   - a hybrid of these.
   Include how strokes stay crisp: hairline wires at 1 device pixel, pixel snapping, and consistent stroke weight across zoom.
5. **Level of detail.** Decide what drops out at low zoom: text below N px, pin stubs, then symbols simplified to boxes. Also decide how the dot grid adapts its spacing so it never turns into noise or moiré.
6. **Text.** Designators and values can number in the thousands. Plan glyph caching, culling and LOD for them, and say how GPUI's text system behaves at that count.
7. **Invalidation.** Decide what triggers a repaint and how to avoid repainting the whole sheet on hover. Consider layering: static sheet, then selection and hover overlay, then the live drag layer.
8. **Threading.** Plan what runs off the UI thread: connectivity, ERC, and later simulation sample streams. Plan how results come back without blocking a frame.

## Interaction quality (design requirements)
- Zoom anchors on the cursor. Support mouse wheel steps, smooth precision-touchpad pinch and pan, and middle-drag or space-drag to pan. Fit-to-sheet and zoom-to-selection animate in about 150 ms with easing, and can be interrupted.
- Snapping uses a 2.54 mm grid by default. Pins snap to pins and wires to pins. Show the snap target before the click.
- Wiring is orthogonal with an L-preview that follows the cursor, Space flips the corner, and junction dots are added and removed automatically. A wire never silently connects across a crossing.
- Selection, hover and net-highlight colours come only from tokens, and work in both Graphite and Gray.
- The status bar shows cursor coordinates, grid size and zoom, and updates without lag.
- Keyboard: R rotates, X/Y mirror, W starts a wire, Esc cancels, Del deletes, and Ctrl+Z/Y undo and redo. Every action works during a drag where that makes sense.

## How quality is kept as features land
- A `canvas-bench` binary with generated stress sheets: 100, 1,000 and 10,000 parts, a dense-wiring case and a text-heavy case. It runs scripted pan, zoom and drag and prints p50, p95 and p99 frame times and allocation counts.
- Profiling is built in from day one with `tracy` or `puffin` behind a feature flag.
- CI runs the bench and fails the build on a regression of more than 10%.
- Visual regression: render fixed sheets headlessly (or via screenshot) in both themes, compare them to golden PNGs, and treat the mockups as the first goldens.
- Property tests for the document model check that undo(redo(x)) equals x, that connectivity stays consistent after random edits, and that snapping is idempotent.

## What I want back
1. A one-page architecture summary with a diagram of the layers and data flow.
2. The day-1 rendering spike: exactly what to measure, the thresholds that choose option a, b or c, and what to do if GPUI can't hit the budget.
3. Milestones, each small enough to ship in about a week, each ending in a demo and a measurable gate tied to the budgets above. Order them so that pan/zoom/grid on a 10,000-part sheet hits budget before any editing feature is built.
4. Crate layout, including `omnia-doc`, `omnia-canvas` and `omnia-bench`, with the public types sketched in Rust signatures only.
5. Dependencies with licences. They must be MIT, Apache-2.0 or similarly permissive. Pin GPUI to one git revision and say how you will upgrade it.
6. A risks list (GPUI churn, path throughput, text at scale, DPI changes, touchpad gestures on Windows), each with a mitigation and an early test that would expose it.
7. The open questions you need me to decide, kept short.

Prefer the simplest design that meets the budgets. Don't add abstractions for features that are out of scope. Where you are unsure how GPUI behaves, say so and put a measurement in the plan instead of a guess.
