# Omnia application UI: roadmap for agents

The task list for the application shell. The reference for every `U`-prefixed id used here
(`UB`, `UK`, `UG`, `UD`, `UINV`, `UTC`, `US`, `UQ`) is [ui-plan.md](ui-plan.md). Ids without a
`U` are in [canvas-plan.md](canvas-plan.md) and [canvas-roadmap.md](canvas-roadmap.md);
`W`-prefixed ids are in [wiring-plan.md](wiring-plan.md) and [wiring-roadmap.md](wiring-roadmap.md).

This roadmap runs beside the canvas and wiring roadmaps. Most of it needs nothing from them; the
table under "Where each task fits" says which tasks wait for which canvas or wiring task.

## How to work a task

The same steps as the canvas roadmap: pick a `todo` task whose dependencies are all `done` (a
`needs-human` dependency is not `done`), mark it `doing`, read what **Read** names, do **Do**
inside **Owns**, run the standard checks and the **Done when** checks, set the status, and add
one log entry at the end of this file.

Statuses: `todo`, `doing`, `done`, `blocked (reason)`, `needs-human`.

## Rules

Canvas roadmap rules 1 to 12 and AGENTS.md apply, with branch names `task/<id>-<slug>`, for
example `task/U1.4-actions`. Five more:

13. **Build from the plan.** The decisions are ui-plan.md section 6; the scenarios in section 8
    are the expected results. A test asserts the `UTC` result, not a variant of it.
14. **The shell never edits a document directly.** It calls `SheetCanvas` methods, or
    `Doc::undo` and `Doc::redo` until U5.1 (UINV14). A canvas method that does not exist yet is
    a disabled menu item, not a workaround.
15. **One registry.** No menu item, toolbar button, tooltip shortcut or key binding is written
    outside `actions.rs` and `keymap.rs` (UD3).
16. **No colour literals, no new crates.** Colours come from `omnia-theme` through
    `theme::color` (UINV1). A dependency outside ui-plan.md section 11 needs its licence logged
    first.
17. **Keep the plans true.** A task that changes a constant, decision or signature updates
    ui-plan.md in the same pull request, and the canvas or wiring plan too if section 14 lists
    the item.

## Standard checks

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p omnia-bench --all-targets --features scene-counts,readback -- -D warnings
cargo test --workspace
cargo deny check
```

Window tests skip themselves when `OMNIA_HEADLESS_CI` is set; simulated-input tests on GPUI's
test platform run everywhere, CI included (ui-plan.md UD21). Golden tests run on the Windows
development machine. Reference runs need the owner at the Orion laptop: prepare a one-command
script and ask for the run.

## Lanes

| Lane | Owns | Needs GPUI |
| --- | --- | --- |
| `shell` (new) | `crates/omnia-app/` | Yes |
| `doc` | `crates/omnia-doc/` files a task names | No |
| `assets` | `crates/omnia-theme/` overlay tokens, `vendor/tabler-icons/`, `THIRD_PARTY.md` | No |
| `bench` | `crates/omnia-bench/src/ui.rs` and the `--shell` options, `xtask/src/golden.rs`, `tests/golden/shell/`, `docs/measurements/U*.md` | Yes |

The `canvas` lane keeps `crates/omnia-canvas/`. Shell tasks touch no file there; what the shell
needs from the canvas is requested through ui-plan.md section 14 and lands in canvas tasks.

## Milestones

Each is about a week and ends in a gate. The order puts a shell that matches the workspace
mockup first (U1), then File and Edit actions on real documents (U2).

| # | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| U0 | Owner review, plan changes applied, spikes US1 to US5 | U0.6 | UD1 (Linux), UD2 popover placement, UD18 readout design |
| U1 | Workspace shell matching the mockup: chrome, menus, toolbar, tabs, strip, docks, status bar; action registry; shell goldens | U1.10 | UB1, UB2 (menus, panels), UB3, UB4, UB5; UINV1 to UINV4; UH2 |
| U2 | Documents: tabs, new, open, save, save as, close and quit prompts, Edit actions, recent files, autosave and recovery | U2.8 | UB10; UINV5 to UINV10, UINV13, UINV14; the file `UTC`s |
| U3 | Text field, theme switching, Preferences (with WD12), Home, First boot, contrast | U3.8 | UB1 (Home), UB7; UINV11; UH3 |
| U4 | Instrument frames, part picker, small sizes, scale factors, keyboard and focus, Home previews | U4.7 | UB2 (popovers, dialogs), UB3 (instrument drag, resize), UB8; UINV12 |
| U5 | Integration as canvas tasks land; final report on `1k` and `10k` sheets | U5.6 | All UB with real sheets; UH4 |

## Dependency overview

```
U0.1 [UH0] -> U0.2
U0.3 [UH1], U0.4, U0.5 -> U0.6
U1.1 -> U1.2 -> U1.3 -+-> U1.5 (needs U0.6) -+
U1.1 -> U1.4 ---------+-> U1.6 ------------+-+-> U1.7 -> U1.8 -> U1.9 [UH2] -> U1.10
U1.7 -> U2.2 -> U2.3 -> U2.4 -> U2.5 -> U2.8          T5.1 -> U2.1 -> (U2.2 switches to cursors)
U1.7 -> U2.6 -> U2.7 -> U2.8
U1.3 -> U3.1 ; U1.7 -> U3.2 ; U3.1 + U2.6 -> U3.3 ; U3.1 + U2.6 -> U3.4 -> U3.5
U3.2 -> U3.6 ; U3.4 + U3.5 + U3.6 -> U3.7 [UH3] -> U3.8
U1.7 -> U4.1, U4.3, U4.4, U4.5 ; U1.6 -> U4.2 ; U3.4 -> U4.6 ; U4.1..U4.6 -> U4.7
T4.3 + T5.2 + T5.3 + U2.5 -> U5.1      T5.3 + U4.2 -> U5.2      T7.1 + U5.1 -> U5.3
W2.3 + U3.3 -> U5.4      T7.3 + U4.1 -> U5.5      T2.7 + U5.1 + U4.7 + U3.8 + U2.8 -> U5.6 [UH4]
```

## Where each task fits

Canvas rendering of document content (T2.5 and later) is blocked behind gates T1.6 and T2.8.
Nothing in milestones U0 to U4 waits for it: the shell runs around today's paper-and-grid sheet
(T1.4, T1.5), and documents load, save, count and undo through `omnia-doc` (T2.4, T5.1 code)
without being drawn.

| UI task | Waits for (canvas or wiring) | Can start |
| --- | --- | --- |
| U0.1, U0.3, U0.4 | nothing | **now** |
| U0.5 | T2.4 (done) | **now** |
| U0.2 | UH0 | after UH0 |
| U1.1 to U1.4, U1.6 to U1.10 | T1.5 (done) | **now** |
| U1.5 | U0.6 (spikes) | after U0.6 |
| U2.1 | T5.1 (done) | done (PR #70) |
| U2.2 to U2.8 | T2.4 (done) | **now** (after U1.7) |
| U3.1 to U3.8 | nothing | **now** (after their U dependencies) |
| U4.1, U4.3 to U4.7 | nothing | **now** (after U1.7) |
| U4.2 | T2.3 (done) for the symbol list | **now**; placing waits for U5.2 |
| U5.1 | T4.3 (`select_all`), T5.2 (`undo`, `redo`, `CanvasEvent::History`), T5.3 (`delete_selection`) | after T5.3 |
| U5.2 | T5.3 (place tool) | after T5.3 |
| U5.3 | T7.1 (`cut`, `copy`, `paste`) | after T7.1 |
| U5.4 | W2.3 (`set_wiring_prefs`, first consumer of the preferences) | after W2.3 |
| U5.5 | T7.3 (animated instrument content) | after T7.3 |
| U5.6 | T2.7 (sheets render at `10k`), and T3.4 for text | after T2.7 |

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| UH0 | U0.1 | Reads ui-plan.md. Answers UQ1 to UQ13 or leaves their defaults. Approves the section 14 changes, which U0.2 then applies. Nothing else waits for it. |
| UH1 | During U0.3 | At the Orion laptop, tries the spike window's drag, double-click, resize edges and buttons on KDE Wayland, and on the Windows machine its snap layouts. About fifteen minutes. |
| UH2 | During U1.9 | Approves the first workspace goldens against `workspace-graphite.png` and `workspace-gray.png`, including every UQ1 substitution. |
| UH3 | During U3.7 | Approves Home and First boot goldens against their mockups. |
| UH4 | After U5.6 | Reads the final UI report. |
| — | Each gate | Runs the prepared reference script at the Orion laptop with other apps closed. |

---

## Milestone U0: decisions and spikes

### U0.1 Owner review · done → UH0 (signed 2026-11-23)
- Lane: any · Depends: none · Read: ui-plan.md in full
- Owns: ui-plan.md section 13
- Do: set this task to `needs-human`. The owner answers UQ1 to UQ13 or keeps the defaults, and
  approves section 14. Record each answer in section 13 with its date.
- Done when: every UQ row has an answer or "default kept", dated.

### U0.2 Make the plans agree · todo
- Lane: any · Depends: U0.1 · Read: ui-plan.md section 14
- Owns: the canvas-plan.md, canvas-roadmap.md, wiring-plan.md, wiring-roadmap.md and ROADMAP.md
  rows section 14 names
- Do: apply every row of section 14 as approved. Change no task that is already `done`; where a
  row touches one, add a dated note instead, as W0.3 did. Add the W2.3 follow-up for
  `set_wiring_prefs` to wiring-roadmap.md.
- Done when: no statement in the canvas or wiring files contradicts a `UD`; section 14 is marked
  applied with the commit.

### U0.3 Spike US1 and US5: chrome and goldens · needs-human → UH1
- Lane: shell · Depends: none · Read: UD1, UG5, UG12, UD21, T0.8's G12 finding
- Owns: `crates/omnia-bench/src/spike/chrome.rs`, `docs/measurements/U0.3.md`
- Do: `bench spike chrome --seconds 60` opens a 1440x900 window with the 36 px bar: drag area,
  three buttons, a menu label that opens a deferred popover, resize edges on Linux. Log the
  granted decorations, every resize and move, and button results. On Windows, render the window
  with the popover open through `render_to_image` and save the PNG.
- Done when: the measurement file records, per machine, which gestures worked (UH1) and whether
  the PNG shows the bar and the popover. Set `needs-human` until UH1 is done.

### U0.4 Spike US2 and US3: cached popover and readout allocations · done
- Lane: shell · Depends: none · Read: UD2, UD18, G6, B10, Q5, T1.5's `status_bar.rs`
- Owns: `crates/omnia-bench/src/spike/shell_cache.rs`, `docs/measurements/U0.4.md`
- Do: over the `empty` sheet, a cached prototype menu bar whose popover is a `deferred` child,
  and a prototype `Readout` element in a cached status view fed by `CanvasEvent::Status`. Hover
  across labels for 300 frames; run the `pan` script for 600 frames. Count `SheetView` paints,
  view renders and allocations in our render and paint scopes.
- Done when: the file states sheet paints during the hover (expected 0), whether the popover
  occludes the sheet's hitbox, and allocations per frame of our code in the readout path.

### U0.5 Spike US4: save and load timing · done
- Lane: bench · Depends: none · Read: UB10, D8, T2.4
- Owns: `crates/omnia-bench/benches/doc_io.rs`, `docs/measurements/U0.5.md`
- Do: criterion benches of `Doc::save` into a `Vec<u8>` and `Doc::load` from bytes for `1k` and
  `10k`; file sizes. Run on the Windows machine and, under section 2.1, on Orion.
- Done when: the file has p50, p99 and size per sheet and machine, and says whether UI-thread
  serialisation meets UB10 on Orion.

### U0.6 Spike results · todo
- Lane: any · Depends: U0.3, U0.4, U0.5
- Owns: `docs/measurements/U0-results.md`, ui-plan.md (status words of UD1, UD2, UD18)
- Do: tabulate US1 to US5 against what each decides. Set UD1's Linux half to DECIDED or switch
  to its fallback; place popovers per US2; confirm or change UD18's readout design per US3; if US4
  misses UB10, log the request for a `DocSnapshot` in the doc lane.
- Done when: every `US` row has an outcome and ui-plan.md matches it.

## Milestone U1: the workspace shell

### U1.1 Theme plumbing and no literals · done
- Lane: shell, assets · Depends: none · Read: UD13, UINV1, UQ6, INV6, T1.2
- Owns: `crates/omnia-app/src/theme.rs`, `lib.rs`, `crates/omnia-theme/canvas-tokens.toml` (UQ6
  entries only) and the regenerated `generated.rs`
- Do: `ActiveTheme` global and `theme::color`; replace `STAGE_PAPER`, `STAGE_INK` and `rgb(..)`
  in `lib.rs` with tokens; route `status_bar.rs`'s colour helper through `theme::color`. Add the
  UQ6 overlay tokens and run `cargo xtask tokens`. Add the UINV1 test over
  `crates/omnia-app/src`.
- Done when: the UINV1 test passes and fails when a literal is planted; `xtask tokens` twice gives
  no diff; `omnia-app --frames 60` exits 0.

### U1.2 Icons and assets · done
- Lane: shell, assets · Depends: U1.1 · Read: UD17, UG14, `THIRD_PARTY.md`
- Owns: `crates/omnia-app/src/assets.rs`, `widgets/icon.rs`, `vendor/tabler-icons/outline/` (the
  UD17 additions), `THIRD_PARTY.md`
- Do: vendor the UD17 icons from Tabler 3.19.0; an `AssetSource` over `include_bytes!` for every
  icon; `Icon` drawing an icon at a size with a token colour.
- Done when: a test renders every icon name the registry and mockup use without a missing-asset
  error; `THIRD_PARTY.md` lists the additions.

### U1.3 Widgets · done
- Lane: shell · Depends: U1.2 · Read: UK3, UK12, UG7, UG8, UG9
- Owns: `crates/omnia-app/src/widgets/` (`button.rs`, `popover.rs`, `splitter.rs`,
  `tooltip.rs`)
- Do: icon button and group (UK3 sizes, active, hover, disabled, focus ring, `role`); text
  button (primary and plain); anchored popover with Esc and click-outside close; tooltip after
  500 ms showing label and shortcut; splitter with a 6 px hit area.
- Done when: headless tests cover each state, keyboard activation and Esc; hover changes notify
  only the widget's view.

### U1.4 Action registry, command state and keymap · done
- Lane: shell · Depends: U1.1 · Read: UD3, UINV3, UINV4, UINV5, T5.2, `gpui/docs/key_dispatch.md`
- Owns: `crates/omnia-app/src/actions.rs`, `keymap.rs`
- Do: the stage 2 action set, `ActionSpec`, `ACTIONS`, `CommandState` with `CanvasEdits` flags
  all off, `KeyContext`, and `default_bindings()`. Canvas tool actions are added as T5.2 lands
  (U5.1); until then the table lists them with `enabled` false.
- Done when: tests pass for UINV3 (every action in a menu; every mouse-reachable action has a menu
  path or binding), UINV4 (no conflicting bindings; a planted conflict fails), and a table test
  of `enabled` for every action over representative states.

### U1.5 Title bar and menu bar · todo
- Lane: shell · Depends: U1.3, U1.4, U0.6 · Read: UD1, UD2, UK2, UG1 to UG6
- Owns: `crates/omnia-app/src/chrome/title_bar.rs`, `chrome/menu_bar.rs`, `key_target.rs` (UQ14);
  the `Nav*` actions and their `Shell` bindings in `keymap.rs` (`default_bindings()`, beside
  `FocusMenuBar`, not `ACTIONS`; including `VALID_CONTEXT_STACKS`); mouse-down focus suppression
  and pointer activation without `on_click` (UG16) in `widgets/button.rs`; yielding navigation
  keys in `widgets/text_field.rs`'s interceptor while an overlay is the key target; the focus calls of `parts/picker.rs` and the analysis picker in
  `chrome/toolbar.rs`
- Do: the `Nav*` actions bound once in `Shell` and the `KeyTarget` router of UD3 (UQ14), as a
  component the root installs; migrate the part picker and analysis picker, which today call
  `Window::focus` on pointer open, on a second press, on Esc and on click-outside dismissal, to
  the router, so no pointer path or dismissal moves GPUI focus. Suppress GPUI's mouse-down defaults in the
  capture phase (a stateful `Div`'s pressed-state handler can refresh in bubble before a custom
  `on_mouse_down` runs); a click-outside dismissal that lands on the sheet must not let the sheet's
  mouse-down take focus while another chrome control holds it. UG17's modality refresh is UQ15:
  if a check fails only because of it, set `blocked (UQ15)`. The 36 px row with logo, menus, title and window controls per UD1 and US1's outcome; menus
  from the registry with shortcut text from the live keymap; F10, Alt+letter, arrows, Enter,
  Esc; the close button goes through the close path (U2.4 completes it; until then it closes).
- Done when: UTC-menu-keys and UTC-menu-mouse pass headless with the menu bar and a focused
  `SheetCanvas` mounted under a test root that installs the `KeyTarget` router; the menu's paint
  count on `SheetView` is 0; opening and driving the part picker and analysis picker by pointer and
  keys, including closing them by second press, Esc and click-outside, leaves `SheetView`'s paint
  count at 0; Esc closes an open menu with the sheet focused and with a Tab-focused toolbar
  control focused; Enter activates the open menu's item (not the button) with a Tab-focused
  toolbar button focused, and Enter, Esc and arrows reach an open menu with a `TextField` focused;
  pointer press and release on a chrome button, and opening, driving and dismissing the part picker
  by pointer (second press, click-outside, Esc; place flag off, so no row starts placement) with
  GPUI focus on another chrome control, leave that focus unchanged and `SheetView`'s paint count
  at 0 across press and release; the UINV4 conflict test
  passes with the `Nav*` bindings; `debug_bounds` of the
  bar matches UK2.
- Note (2026-11-23, U4.2): Place > Part is built from `parts::picker::part_menu()` (categories in
  strip order, empty ones disabled) and the "In use" entries from `in_use_entries()`, so the menu
  mirrors the picker's popovers exactly.

### U1.6 Toolbar · done
- Lane: shell · Depends: U1.3, U1.4 · Read: UD3, UD19, UK3
- Owns: `crates/omnia-app/src/chrome/toolbar.rs`
- Do: the groups in UK3 order, generated from the registry; the place strip showing the twelve
  category buttons (disabled until U4.2); the "In use · N parts" picker; the analysis picker, Run,
  Pause, Stop and time pill in the UD19 unavailable state. The toolbar observes `CommandState`
  and renders only when a value it shows changes.
- Done when: a test flips each `CommandState` field and checks the toolbar renders exactly when a
  shown value changes; `debug_bounds` matches UK3.

### U1.7 Workspace layout, tabs, strip and docks · todo
- Lane: shell · Depends: U1.5, U1.6 · Read: UD4, UD5, UD12, UK4, UK5, UK11, D7, G6
- Owns: `crates/omnia-app/src/shell.rs`, `workspace.rs`, `chrome/tab_strip.rs`,
  `instruments/strip.rs`, `panels/`
- Do: `Shell` with the workspace screen and a `ModalLayer` slot; the sheet area with the existing
  `SheetCanvas`; the instruments strip (buttons only); the tab strip with one tab, "+",
  modified dot and the three panel toggles; the Toolbox and bottom docks with stub panels,
  splitters and UK11 limits. Every region is a cached view (UD4).
- Done when: the real `Workspace` installs U1.5's `KeyTarget` router and UTC-menu-keys passes
  again against it; UINV2's test passes for chrome hover, toggles and strip hover (sheet paint count 0);
  UTC-dock passes headless (a splitter drag paints the sheet at most once per frame); `debug_bounds` matches UK4, UK5, UK11
  at 1440x900.

### U1.8 Status bar beyond T1.5 · todo
- Lane: shell · Depends: U1.7, U0.6 · Read: UD18, UK6, UQ8, T1.5's log entry
- Owns: `crates/omnia-app/src/status_bar.rs`, `readout.rs`
- Do: the UD18 slots; `Readout` for cursor, grid, zoom, simulation time and counts (counts read
  "— parts · — nets" until U2.2 supplies them); `StatusBar::follow`; status messages with UK12's
  lifetime. Keep T1.5's tests passing.
- Done when: a status update renders only the readouts view, never `SheetView` or the root;
  allocation-counting test: zero allocations from our code per readout update once warm (UB5).

### U1.9 Golden harness and workspace goldens · todo → UH2
- Lane: bench · Depends: U1.8 · Read: UD21, T3.2, G12, U0.3's result
- Owns: `xtask/src/golden.rs`, `tests/golden/shell/`, `crates/omnia-app/tests/golden_shell.rs`,
  `docs/measurements/U1.9.md`
- Do: the shared harness T3.2 will extend: render with `render_to_image` in both themes at 1.0,
  1.25 and 1.5; compare with committed PNGs (at most 0.1% of pixels differ by more than 2/255);
  `cargo xtask golden --update`. Render the workspace at 1440x900 with the paper-and-grid sheet,
  and produce side-by-side images with both workspace mockups and a list of every visible
  difference, including UQ1 substitutions.
- Done when: the test passes on unchanged code and fails when a toolbar group moves 1 px; the
  side-by-side is in the measurement file. Set `needs-human`; after UH2 approves, commit the
  goldens and set `done`.

### U1.10 Gate: milestone U1 · todo
- Lane: bench · Depends: U1.9 · Read: ui-plan.md section 2, canvas plan section 2.1
- Owns: `crates/omnia-bench/src/ui.rs`, the `--shell` options of `start`, `idle` and `run`,
  `docs/measurements/U1.10.md` and its raw data and script
- Do: build `bench ui` with the `menus`, `panels`, `chrome-hover` and `resize` scripts and
  `sheet_paints` in its report. Run and record:

  | Budget | Command | Pass |
  | --- | --- | --- |
  | UB1 | `bench start --shell` and `bench start` | median < 400 ms; difference ≤ 80 ms |
  | UB2 | `bench ui --sheet empty --script menus\|panels` | dirty-to-present p99 ≤ one refresh |
  | UB3 | `bench ui --script chrome-hover\|resize` | `sheet_paints` 0; resize ≤ 1 per frame |
  | UB4 | `bench idle --shell --seconds 10` | 0 frames, 0 renders |
  | UB5 | `bench run --shell --sheet empty --script pan\|zoom` | `allocs_per_frame.ours == 0` |

- Done when: all pass on the reference machine at 100% and 125%. Elsewhere, or on a miss: record
  and set `needs-human`.

## Milestone U2: documents and files

### U2.1 History cursor and counts · done
- Lane: doc · Depends: T5.1 (`done` and merged) · Read: UD6, D2, INV4
- Owns: `crates/omnia-doc/src/undo.rs`, `doc.rs` (the five methods only), canvas-plan.md section 9
- Do: `history_cursor`, `can_undo`, `can_redo`, `part_count`, `net_count` as sketched in
  ui-plan.md section 10. The cursor is stored with each undo and redo entry; no-ops change
  nothing.
- Done when: a property test over random command, undo and redo sequences shows: two states have
  equal cursors exactly when one is reached from the other by undo and redo alone; the INV4 test
  still passes; UTC-saved-point and UTC-branch pass at the doc level.

### U2.2 Documents and tabs · todo
- Lane: shell · Depends: U1.7 · Read: UD4, UD6, UINV6, UINV8
- Owns: `crates/omnia-app/src/document.rs`, `workspace.rs` (documents and tabs),
  `chrome/tab_strip.rs`
- Do: `Document` with its own `Doc` entity and `SheetCanvas`; several tabs; activate, next and
  previous tab; `HistoryMark::Revision` until U2.1, then `Cursor`; modified dot, title dot and
  `CommandState` from `DocumentEvent`; status counts from the active document (parts by
  `Doc::parts().count()` on change until U2.1; nets shown from U2.1).
- Done when: UTC-new and UTC-tabs pass headless; UINV8's property test (random switches with
  camera and selection changes between them) passes.
- Note (2026-11-23, U4.2): on activation and on each document change, call
  `PartPicker::follow_document` (through `Toolbar::picker()`) with the active `Doc`; until then the
  "In use" picker shows 0 parts.

### U2.3 File actions · todo
- Lane: shell · Depends: U2.2 · Read: UD7, UD8, UG4, UG10, D8
- Owns: `crates/omnia-app/src/files.rs`, `main.rs`, `workspace.rs` (file handlers)
- Do: New, Open (picker, several files, already-open), background load with a loading tab, Save,
  Save As with extension, atomic write, Close Tab, drop to open, paths on the command line,
  "Saved …" status message.
- Done when: UTC-open, UTC-open-twice, UTC-open-bad, UTC-open-drop, UTC-save-untitled,
  UTC-save-cancel and UTC-save-fail pass headless; `write_atomic` leaves no temp file on any
  failure path (tested with a failing writer).

### U2.4 Dialogs, modal layer and toasts · todo
- Lane: shell · Depends: U2.3 · Read: UD5, UD7, UD11, UINV7, UINV13
- Owns: `crates/omnia-app/src/dialogs/unsaved.rs`, `dialogs/error.rs`, `toast.rs`, `shell.rs`
  (`ModalLayer`)
- Do: the modal layer (scrim, focus, Esc, occlusion); the unsaved-changes dialog for one document
  and for several; the error dialog; toasts; close and quit through UD7, including
  `on_window_should_close`.
- Done when: UTC-close-dirty, UTC-quit-dirty, UTC-close-window and UTC-toast pass headless; a test
  shows keystrokes and clicks beneath an open dialog change nothing (UINV13).

### U2.5 Edit actions and enabled states · todo
- Lane: shell · Depends: U2.4 · Read: UD3, UINV5, UINV14
- Owns: `crates/omnia-app/src/workspace.rs` (edit handlers), `actions.rs` (`enabled` functions)
- Do: Undo and Redo through `Doc::undo` and `Doc::redo` on the active document, notifying its
  canvas; Cut, Copy, Paste, Delete and Select All wired to the `CanvasEdits` flags, which stay off
  until U5.1 and U5.3. Add the UINV14 test (no `Doc::apply` in `omnia-app` outside tests).
- Done when: UTC-disabled passes; tests apply commands to a document through `Doc::apply` and
  check that the Undo menu item, Ctrl+Z and the toolbar button each undo once and update the
  modified state; UINV5 holds for every disabled action.

### U2.6 Preferences, recent files and window state on disk · todo
- Lane: shell · Depends: U1.7 · Read: UD10, UD14, UINV10, WD12, WK10
- Owns: `crates/omnia-app/src/persist.rs`
- Do: `Dirs::from_env`, `Prefs` with `WiringPrefsFile` validated through `WiringPrefs`'s
  checked constructor, `RecentFiles` (UK13, camera per path), `WindowState`; atomic writes;
  missing and unreadable files per UD10. Tests use a temporary directory, never the real one.
- Done when: UINV10's round-trip tests pass; UTC-recent passes; an invalid threshold in a file is
  rejected and reported.

### U2.7 Autosave and recovery · todo
- Lane: shell · Depends: U2.6, U2.4 · Read: UD9, UINV9, UK12
- Owns: `crates/omnia-app/src/recovery.rs`
- Do: the autosave timer (armed only while needed, deferred during input and gestures), recovery
  files and metadata, deletion on save, discard and clean quit, the launch-time offer (dialog in
  the workspace; Home's card comes with U3.4).
- Done when: UTC-recovery passes with GPUI's test clock; a test shows no timer exists while every
  document is saved or already autosaved (UINV9).

### U2.8 Gate: milestone U2 · todo
- Lane: bench · Depends: U2.5, U2.7 · Read: UB6, UB10
- Owns: `crates/omnia-bench/src/ui.rs` (`open`, `save`, `autosave`, `tabs` scripts),
  `docs/measurements/U2.8.md`
- Do: run every file `UTC` and UINV5 to UINV10, UINV13, UINV14 tests. On Orion: UB10 for `1k` and
  `10k` files (generated by T2.4); UB2's `tabs` script between two `1k` documents; UB4 with two
  documents, one modified (UTC-idle).
- Done when: all pass on the reference machine. The sheets draw only paper and grid until T2.5,
  so UB6 at `10k` is recorded here and gated in U5.6.

## Milestone U3: Home, First boot, preferences and theme

### U3.1 Text field · done
- Lane: shell · Depends: U1.3 · Read: UD16, UG13, `gpui/examples/input.rs`
- Owns: `crates/omnia-app/src/widgets/text_field.rs`, `THIRD_PARTY.md` (attribution)
- Do: a single-line field per UD16 in key context `TextField`.
- Done when: headless tests cover typing, selection, clipboard, Backspace and Delete, and
  UTC-type-r; paint makes no allocation once warm with unchanged text.

### U3.2 Theme switching · todo
- Lane: shell · Depends: U1.7 · Read: UD13, UG12, D7
- Owns: `crates/omnia-app/src/theme.rs`, every shell view's theme observation
- Do: View > Theme and `SetTheme`; `System` following the window appearance; `set_theme` on the
  active canvas and on others at activation.
- Done when: UTC-theme passes headless (each view renders once per switch; each open sheet paints
  once); the golden after a switch equals a fresh start in that theme.

### U3.3 Preferences and Help dialogs · todo
- Lane: shell · Depends: U3.1, U2.6 · Read: UD14, WD12, WK10
- Owns: `crates/omnia-app/src/dialogs/prefs.rs`, `dialogs/shortcuts.rs`, `dialogs/about.rs`
- Do: Preferences per UD14, written on change; Help > Keyboard Shortcuts generated from the
  registry; About with the version and third-party notices from `THIRD_PARTY.md`. Wiring
  preferences are stored and validated; applying them to canvases is U5.4.
- Done when: UTC-wiring-prefs passes for storage and validation; the shortcuts list equals the
  registry's bindings.

### U3.4 Home screen · todo
- Lane: shell · Depends: U3.1, U2.7 · Read: UD15, UK8, UQ3, UQ7, `design/build-omnia.mjs` Home
- Owns: `crates/omnia-app/src/home/mod.rs`, `home/sidebar.rs`, `home/cards.rs`
- Do: the sidebar and main panel per UD15 with plain thumbnails, Grid and List, search, Projects
  as folders, recovery cards, missing-file entries, the template row when samples exist; Home
  shown at launch, by Ctrl+Shift+H and when the last tab closes.
- Done when: headless tests cover opening from a card, the list view, search filtering, a missing
  entry and a recovery card; columns follow UD12 as the window narrows.

### U3.5 First boot · todo
- Lane: shell · Depends: U3.4 · Read: UD15, UK9, UQ9
- Owns: `crates/omnia-app/src/first_boot.rs`
- Do: the screen per UD15 with working Appearance (live previews) and Schematic symbols, reserved
  rows for sections 3 and 4, Enter to accept, "Start designing".
- Done when: UTC-first-boot passes headless; choosing Gray before Enter writes it and the next
  screen is in Gray.

### U3.6 Contrast check · todo
- Lane: shell · Depends: U3.2 · Read: UD20, UINV11, UQ1
- Owns: `crates/omnia-app/src/theme.rs` (the declared pair list), its test
- Do: every view declares the (foreground, background) token pairs it draws, as a constant list;
  the test computes WCAG contrast from `omnia-theme` in both themes. Apply UQ1's answer, or its
  default substitutions.
- Done when: the UINV11 test passes and fails when a failing pair is declared.

### U3.7 Home and First boot goldens · todo → UH3
- Lane: bench · Depends: U3.4, U3.5, U3.6 · Read: UD21, U1.9
- Owns: `tests/golden/shell/` (Home, First boot, dialogs), `docs/measurements/U3.7.md`
- Do: goldens of Home (with fixtures for recent files), First boot, and each dialog in both
  themes at three scales; side-by-side images with the Home and First boot mockups and a list of
  differences.
- Done when: set `needs-human`; after UH3 approves, commit the goldens and set `done`.

### U3.8 Gate: milestone U3 · todo
- Lane: bench · Depends: U3.7 · Read: UB1, UB7
- Owns: `crates/omnia-bench/src/ui.rs` (`theme` script, `start --home`), `docs/measurements/U3.8.md`
- Do: `bench start --home --recent 20` (UB1); `bench ui --sheet 1k --script theme` (UB7, the
  sheet paper and grid only until T2.5); the UINV11 test.
- Done when: all pass on the reference machine; otherwise record and set `needs-human`.

## Milestone U4: instruments, placement, small sizes, scale and focus

### U4.1 Floating instrument frames · todo
- Lane: shell · Depends: U1.7 · Read: UD5, UK10, D7, G6, T7.3
- Owns: `crates/omnia-app/src/instruments/layer.rs`, `instruments/strip.rs`
- Do: `InstrumentLayer` and `FloatingFrame` per UD5: open from the strip with a static stub body,
  drag by the header, raise and make the key target on click without moving GPUI focus (UQ14),
  Esc returns the key target to the sheet, close, clamping after resizes.
- Done when: UTC-instrument passes headless (sheet paint count 0 throughout).

### U4.2 Part picker · done
- Lane: shell · Depends: U1.6 · Read: UD2, UK3, T2.3's symbol list
- Owns: `crates/omnia-app/src/parts/picker.rs`
- Do: each place-strip button opens a popover of that category's built-in symbols (empty
  categories disabled); the "In use" picker lists symbols already in the active document with
  counts; Place > Part mirrors both. Choosing a symbol dispatches `PlacePart`, whose `enabled` is
  the `CanvasEdits::place` flag (off until U5.2).
- Done when: headless tests open each popover by mouse and keyboard; the In use count matches the
  document; with the flag off, choosing changes nothing (UINV5).

### U4.3 Small sizes · todo
- Lane: shell · Depends: U1.7 · Read: UD12, UK1, UK11
- Owns: `crates/omnia-app/src/chrome/toolbar.rs` (overflow), `chrome/tab_strip.rs` (scrolling),
  `panels/dock.rs` (collapse), `status_bar.rs` (dropping order)
- Do: the UD12 give-way order for toolbar, tabs, strip, status bar and docks; the 960x600 minimum.
- Done when: UTC-small passes headless at 1.0 and 1.5 (no overlapping `debug_bounds`); goldens
  at 960x600.

### U4.4 Scale factors · todo
- Lane: shell · Depends: U1.7 · Read: UB8, T7.4, plan section 11 of the canvas plan
- Owns: `crates/omnia-app/tests/scale.rs`
- Do: verify every chrome view at 1.0, 1.25 and 1.5 and across a live change; fix any
  device-pixel misalignment through `omnia-canvas`'s `snap_px` helpers (read-only use).
- Done when: UTC-scale passes for the shell (the sheet part is T7.4's).

### U4.5 Keyboard reach and focus · todo
- Lane: shell · Depends: U1.7 · Read: UD20, UINV3, UINV12, UG8, UG9
- Owns: focus handles and roles in `crates/omnia-app/src/chrome/`, `instruments/`, `dialogs/`;
  `crates/omnia-app/tests/focus.rs`
- Do: tab order per UINV12 (keyboard focus moves only; pointer input keeps UQ14's key-target
  routing), visible focus ring, Esc returning focus, `id` and `role` on every
  chrome control.
- Done when: a headless test tabs through the whole workspace and lands on each stop in order; a
  test executes every registry action from the keyboard alone.

### U4.6 Home previews · todo
- Lane: shell · Depends: U3.4 · Read: UD15, D4, D5
- Owns: `crates/omnia-app/src/home/preview.rs`, `persist.rs` (preview summary in `recent.ron`)
- Do: at save, store a capped summary (at most 2,000 wire segments and part boxes, chosen) next
  to the recent entry; draw it on the card as quads in `stage/ink` on `stage/paper`.
- Done when: a golden of a card with a preview matches; `bench start --home --recent 20` still
  meets UB1.

### U4.7 Gate: milestone U4 · todo
- Lane: bench · Depends: U4.1 to U4.6
- Owns: `crates/omnia-bench/src/ui.rs` (`popovers`, `dialogs`, `instrument-drag` scripts),
  `docs/measurements/U4.7.md`
- Do: UB2 for popovers and dialogs; UB3 for `instrument-drag` and `resize`; UB8 goldens and the
  scale-change test; UINV12.
- Done when: all pass on the reference machine (goldens on Windows); otherwise record and set
  `needs-human`.

## Milestone U5: integration and final report

### U5.1 Edit actions through the canvas · todo
- Lane: shell · Depends: T4.3, T5.2, T5.3, U2.5 · Read: UD3, UINV14, T5.2's log entry
- Owns: `crates/omnia-app/src/workspace.rs` (edit handlers), `actions.rs`, `keymap.rs`
- Do: route Undo, Redo, Delete and Select All to the canvas methods; drive `can_undo`,
  `can_redo` and the modified state from `CanvasEvent::History`; add T5.2's tool actions to the
  registry and menus (Rotate, Mirror, Wire); turn on the matching `CanvasEdits` flags.
- Done when: a simulated drag, then Ctrl+Z from the menu with focus in a dock, undoes it once
  (INV5 unchanged); UINV4 still passes with T5.2's bindings.

### U5.2 Placement · todo
- Lane: shell · Depends: T5.3, U4.2 · Read: T5.3
- Owns: `crates/omnia-app/src/parts/picker.rs`, `actions.rs` (`place` flag)
- Do: `PlacePart` sets the canvas tool to `Tool::Place(symbol)` and makes the sheet the key target
  (UQ14). Placement needs `Sheet`'s bindings (R, X, Y), so if GPUI focus is elsewhere (a
  Tab-focused chrome control) it then focuses the sheet: starting a canvas tool is not a
  shell-only change, and this costs at most one sheet repaint. With the sheet already focused no
  focus call is made. The In use count updates after placement.
- Done when: a headless test places a resistor from the toolbar and from Place > Part, and Ctrl+Z
  removes it; with the sheet focused beforehand, choosing the part paints the sheet 0 times
  before the placement preview; with a toolbar control focused, at most once.

### U5.3 Clipboard · todo
- Lane: shell · Depends: T7.1, U5.1 · Read: T7.1
- Owns: `crates/omnia-app/src/workspace.rs` (clipboard handlers), `actions.rs` (`clipboard` flag)
- Do: Cut, Copy and Paste to the canvas methods; `has_clip` in `CommandState`.
- Done when: headless copy and paste between two tabs gives the same internal connectivity and no
  connection to the source (T7.1's rule).

### U5.4 Wiring preferences applied · todo
- Lane: shell · Depends: W2.3, U3.3 · Read: WD12, UD14
- Owns: `crates/omnia-app/src/workspace.rs` (preference propagation)
- Do: pass `WiringPrefs` to every canvas at creation and on change.
- Done when: UTC-wiring-prefs passes end to end, including a follow decision made at the
  threshold the dialog set.

### U5.5 Instrument content · todo
- Lane: shell · Depends: T7.3, U4.1 · Read: T7.3
- Owns: `crates/omnia-app/src/instruments/layer.rs` (hosting T7.3's content)
- Do: host T7.3's animated stub inside the frame from U4.1.
- Done when: T7.3's done-when holds with the frame from U4.1 and the full shell.

### U5.6 Gate: final UI report · todo → UH4
- Lane: bench · Depends: T2.7, U2.8, U3.8, U4.7, U5.1 · Read: ui-plan.md section 2
- Owns: `docs/ui-report.md`
- Do: measure UB1 to UB10 on the reference machine at 100% and 125% with real `1k` and `10k`
  sheets (after T3.4, with text), plus every UINV test and the goldens. One table: budget,
  measured value, pass or miss.
- Done when: the report exists. A miss is reported as a miss, with the measurement.

---

## Log

Append one entry per finished or blocked task. Newest last.

```
<date> <task id> <status> — what was done; numbers with hardware; deviations from the plan;
                             anything the next task needs to know.
```

2026-11-16 plan — ui-plan.md and ui-roadmap.md written. GPUI facts UG1 to UG14 read from source at
15d136bf. Contrast measured from the generated tokens (ui-plan.md UD20). Nothing else measured;
UD1's Linux half, UD2's popover placement and UD18's readout design wait for spikes US1 to US3.

2026-11-17 U1.1 done — `ActiveTheme` global, `ThemeChoice` and `theme::color` in
`crates/omnia-app/src/theme.rs` (UD13); `STAGE_PAPER`/`STAGE_INK` and the colour literals in
`lib.rs` replaced with `stage/paper`/`stage/ink` tokens; `status_bar.rs` colour helper routed
through `theme::color`. UQ6 default overlay tokens added (`brand/tile`, `brand/mark`,
`instrument/screen`, `focus/ring` as `stage/select`; 39 tokens total) and `generated.rs`
regenerated with `cargo xtask tokens` (twice, no diff). UINV1 source test over
`crates/omnia-app/src` passes and fails on a planted literal (verified, probe removed).
`omnia-app --frames 60` exits 0. All five standard checks pass on a Windows development
machine (certifies no reference budget). Deviations: `crates/omnia-app/tests/fonts.rs` and
`crates/omnia-theme/tests/tokens.rs` updated only to follow the removed constants and the
new token count. `System` resolves to Graphite until U3.2 follows window appearance.

2026-11-17 U1.2 done — Vendored the 13 UD17 Tabler Icons v3.19.0 outline SVGs into
`vendor/tabler-icons/outline/` and listed the additions in `THIRD_PARTY.md`. Implemented
`Assets` (`gpui::AssetSource` over `include_bytes!` for all 61 vendored icons, with
`icon_asset_path` and `icon_bytes`) in `crates/omnia-app/src/assets.rs`, and `Icon`
(`RenderOnce` over `svg()` at a logical size tinted with `omnia_theme::Rgba` via
`theme::color`) in `crates/omnia-app/src/widgets/icon.rs`. Added `omnia_app::application()`
calling `.with_assets(assets::Assets)` in `crates/omnia-app/src/lib.rs` and wired
`crates/omnia-app/src/main.rs` to use it. Verified all 61 mockup and registry icons via
`SvgRenderer` rasterization, `HeadlessAppContext` element drawing, and real hidden-window
`render_to_image` (`crates/omnia-app/tests/icons.rs`, skipping window readback when
`OMNIA_HEADLESS_CI` is set). On a Windows development machine (certifies no reference budget):
`cargo fmt --all --check`, `cargo deny check`, `cargo test -p omnia-app` (12 unit, `fonts`,
`icons` headless + `render_to_image`, 6 `navigation`), `OMNIA_HEADLESS_CI=1 cargo test -p
omnia-app --test icons`, and `cargo run -p omnia-app -- --frames 60` all pass. `cargo clippy`
matrix and full `cargo test --workspace` left to CI because C: dropped to ~0.6 GB free.
Deviations: added `crates/omnia-app/src/widgets/mod.rs`, updated `crates/omnia-app/src/main.rs`
for entry-point hookup, and added `crates/omnia-app/tests/icons.rs` (`[[test]]` in `Cargo.toml`)
for real-window readback mirroring `tests/fonts.rs`.
2026-11-17 U0.3 needs-human (UH1) — Added the standalone chrome spike (US1/US5, UD1,
UG5, UG12, UD21). The 1440 × 900 Windows window exited on schedule and the 1800 × 1125
readback at 125% shows the title bar, all three controls and the deferred File menu.
Owner approved the minimal CLI registration outside the listed files. Linux gestures
and Windows snap layouts await UH1. Report and raw evidence: docs/measurements/U0.3.md.
2026-11-17 U0.4 done — Windows non-reference spike `bench spike shell-cache` in
`crates/omnia-bench/src/spike/shell_cache.rs` (results in `docs/measurements/U0.4.md`, raw JSON
in `docs/measurements/U0.4/shell-cache-{nested,sibling}.json`; hardware: Windows 11 Home 26200,
Intel Core i7-14650HX, Intel UHD Graphics driver 32.0.101.7088, 1440x900 @ 1.25 scale, 165 Hz).
US2: 300-frame menu-label hover over the `empty` sheet gave 0 `SheetView` paints; the `deferred`
`.occlude()` popover inside `.cached()` `PrototypeMenuBar` occluded move/middle-drag/scroll
over the sheet on both fresh and root-notified cached replay (`replay_menu_renders = 0`,
`replay_popover_prepaints = 0`, `replay_popover_paints = 1`), confirming UD2's in-view placement
on Windows. US3: 600-frame `pan` (1 device px/frame) gave 0 `ours` allocations/bytes per frame
across `subscribe`, `render`, `layout`, `prepaint`, and `paint` using stack `FixedText<128>` and
cached Source Code Pro `ShapedLine` glyphs (`total` was 0 in `subscribe`/`render`/`layout`/
`prepaint`, and 0 p99 / max 1 [324 B on 4 of 600 frames] in `paint`). Note for U0.6/U1.8: GPUI's
`Window::mark_view_dirty` marks ancestor views dirty, so nesting `ReadoutsView` inside `.cached()`
`StatusBar` caused `status_bar_renders = 600`, whereas placing `StatusBar` and `ReadoutsView` as
sibling `.cached()` views in the bottom row dropped `status_bar_renders` to 0 and whole-frame
`total` allocs from 59 to 51 p50. All five standard checks pass on Windows; Orion certification
remains outside U0.4.
2026-11-17 U0.5 needs-human — added Criterion bench `crates/omnia-bench/benches/doc_io.rs` (`save_1k`,
`load_1k`, `save_10k`, `load_10k` over T2.4 `generate(..., 42)`), registered `doc_io` in
`crates/omnia-bench/Cargo.toml`, and prepared the frozen one-command Orion campaign
(`docs/measurements/U0.5/run.sh`, `summarize.py`) under canvas-plan section 2.1 CPU-only rules.
File sizes (RON v2): `1k` 505,548 B (`0.4821 MiB`), `10k` 5,079,273 B (`4.8440 MiB`).
Windows 10-pass non-reference diagnostic (i7-14650HX, 32 GB, Windows 11 Home 10.0.26200, AC,
Silent power scheme, shared session, certifies nothing): median of 10 run `p50`/`p99` (`n=100`
per pass) `save_1k` `1.330`/`1.480` ms, `load_1k` `4.771`/`5.211` ms, `save_10k` `13.724`/`15.082` ms
(range `13.863–50.079` ms), `load_10k` `50.710`/`52.954` ms. Nothing run on Orion; `needs-human`
until the owner runs `bash docs/measurements/U0.5/run.sh` on Orion to decide US4 / `UB10`
(`save_10k` < 33.3 ms p99 vs `DocSnapshot`).

2026-11-17 U2.1 doing — History cursor and counts (UD6, D2, INV4; ui-plan section 10 sketch). `History` in `crates/omnia-doc/src/undo.rs` now stores a `before`/`after` cursor pair with each undo and redo entry plus a never-reissued counter: an effective `apply` issues a fresh cursor, `undo`/`redo` move to the stored value, and validated no-ops and rejected commands change nothing (they never reach `push_undo`). Added the five `Doc` methods in `crates/omnia-doc/src/doc.rs` (`history_cursor`, `can_undo`, `can_redo`, `part_count`, `net_count`) and the matching canvas-plan.md section 9 sketch lines (applied after T2.5 PR #69 merged and this branch was updated to `a7e7b76`). Tests in `undo.rs`: a generative property test over random command/undo/redo sequences asserting undo/redo return to the exact earlier cursor, an effective apply always issues a fresh cursor (so a discarded branch's save point can never be reissued), equal cursor implies `content_eq`, and `can_undo`/`can_redo` match a model stack; plus doc-level UTC-saved-point and UTC-branch and no-op/count coverage. Status stays `doing` pending independent review.
2026-11-17 U1.4 done — Implemented the single action registry (`crates/omnia-app/src/actions.rs`)
and default keymap/conflict validator (`crates/omnia-app/src/keymap.rs`), exposed via
`crates/omnia-app/src/lib.rs` (UD2, UD3, UD19, UK2, UK3, UQ11). `ACTIONS` covers the full
stage 2 menu/toolbar/instrument set (`ActionSpec`, `TopMenu`, `ToolbarGroup`, `MenuPlace`,
`ToolbarPlace`, `KeyContext`, `Binding`, `CommandState`, `SimState`, and `CanvasEdits` with all
edit flags off by default; canvas tool actions `Rotate`, `MirrorHorizontal`, `MirrorVertical`,
`PlaceWire`, and `PlaceNetLabel` stay disabled via `enabled: never` until T5.2/U5.1 land).
`keymap::default_bindings()` builds GPUI `KeyBinding`s from `ACTIONS` plus `F10` ->
`FocusMenuBar` in `Shell`; `keymap::conflicts()` validates UINV4 using GPUI's
`KeyBindingContextPredicate::depth_of` against `VALID_CONTEXT_STACKS` and restricts unmodified
letters/digits/Space to `{Sheet, Menu, Dialog, TextField}`; `format_keystroke`, `format_keybinding`,
and `tooltip_for_action` format OS shortcuts via `Keystroke::unparse` (`Ctrl+S` style per UQ11).
Tests cover UINV3 completeness/menu/toolbar order/icons, UINV4 default pass and planted
same-context, parent-child co-active, global, chord-prefix, and disallowed bare-key failures,
a representative enabled/checked table across every action in `ACTIONS`, and headless GPUI key
dispatch across `Shell`/`Workspace`/`Sheet`. Deviations: `PlacePart` uses `omnia_doc::SymbolId`
(instead of `omnia_symbols::SymbolId`) matching `omnia-doc`'s type. All five standard CI checks
pass on a Windows development machine (certifies no reference budget).
2026-11-17 U1.3 done — Implemented the shell widget set in `crates/omnia-app/src/widgets/`
(`button.rs`, `popover.rs`, `splitter.rs`, `tooltip.rs`, exported from `mod.rs`): `ButtonGroup`
and `IconButton` (UK3 34 px group / 30x30 px buttons, active, hover, disabled with
`aria_description("unavailable")` and `tab_stop(false)`, 2 px `focus/ring` focus-visible ring,
`Role` / ARIA attributes), `TextButton` (`ButtonVariant::Primary` and `Plain`, optional icon,
shortcut hint, tooltip), `Popover` (`deferred` + `anchored` with `occlude()`, window-margin
clamping, Esc and click-outside dismissal, `tab_stop(false)` on container focus handle),
`Tooltip` (`TOOLTIP_DELAY` = 500 ms UK12, label + Source Code Pro shortcut hint, `Role::Tooltip`),
and `Splitter` (`SPLITTER_HIT_SIZE` = 6 px hit target, centered 1 px `rule` line, hover and
drag feedback, `Role::Splitter`). Headless tests in all four modules verify dimensions, states,
keyboard activation (`Enter`/`Space`), Tab skipping disabled controls, nested/child `Escape` and
click-outside close, 500 ms tooltip delay/hide/cancel, vertical/horizontal splitter drag, and
hover isolation (notifying only the widget's view with 0 root/sibling notifies or renders).
All five standard checks (`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo clippy -p omnia-bench --all-targets --features scene-counts,readback -- -D warnings`,
`cargo test --workspace`, and `cargo deny check`) pass on a Windows development machine
(certifies no reference budget).

2026-11-17 U1.6 done — Implemented the workspace Toolbar in `crates/omnia-app/src/chrome/toolbar.rs` (new
`chrome` module, wired in `lib.rs`; UD3, UD19, UK3). File/edit/zoom groups and the run controls are
generated from the U1.4 registry via `toolbar_actions` (labels, icons, shortcut tooltips and
enabled/checked all come from `ACTIONS`; clicks dispatch the registry action, disabled buttons
dispatch nothing per UINV5). The place strip shows the twelve mockup category buttons, all disabled
until U4.2, with nearest vendored Tabler glyphs (real symbol glyphs/popovers are U4.2/T2.3); the
"In use" picker is present at 158 px, disabled until U4.2, its count supplied through
`Toolbar::set_in_use_count` (notifies only on change). The analysis picker shows greyed "Transient"
and opens a popover with one disabled entry; Run (60x28 primary), Pause and Stop are disabled with
the UD19 "Simulation is not available in this build" tooltip while `SimState::Unavailable`, and the
74x24 time pill shows the UD19 placeholder with no dot. `Toolbar` observes the per-window
`CommandState` entity and re-renders only when its `ShownState` projection (enabled/checked of every
toolbar action, analysis enabled, sim state) changes. Headless tests flip every `CommandState` field
and assert exactly one render for shown values and zero for `tab_count`, dock toggles, `theme`,
`canvas_edits.delete` and `canvas_edits.place` (plus a same-value notify), `debug_bounds` asserts UK3
sizes/gaps/order at 1440x900, and click tests cover dispatch, disabled no-ops and analysis popover
Esc close. All five standard checks pass on a Windows development machine (certifies no reference
budget); `cargo run -p omnia-app -- --frames 60` exits 0. Deviations: the toolbar is not yet embedded
in the window (workspace layout is U1.7, as with U1.3/U1.4); U4.3 owns overflow at small sizes.
2026-11-22 U2.1 done — Status catch-up only; no code change. PR #70 (`t3/history-cursor-counts`) was merged on 2026-11-18 (merge `90383fa`) after its review on `t3/review-undo-history`, but the roadmap still read `doing`. The five `Doc` methods (`history_cursor`, `can_undo`, `can_redo`, `part_count`, `net_count`), the cursor property test `u2_1_cursor_identity_over_random_command_undo_redo`, and `utc_saved_point_at_doc_level` / `utc_branch_at_doc_level` are on `main`, and CI is green at `e09169b`. U2.2 can now use `HistoryMark::Cursor` from the start.

2026-11-22 U3.1 done — Added the single-line `TextField` (UD16, UG13) with a themed
placeholder and focus ring (UINV1), horizontal caret scrolling, keyboard and mouse selection,
clipboard editing, and GPUI IME composition with UTF-16 ranges. Line separators become spaces.
`TextFieldEvent::Changed` reports committed edits. Enter and Escape emit `Submit` and `Cancel`.
Updated UD3 to record that the focused field handles its editing keystrokes before keymap dispatch.
This keeps the shared bindings in the registry (UINV4). UTC-type-r verifies that lower- and
uppercase input reach the field while Rotate and document Undo/Redo remain inactive.
Attributed the GPUI input example in `THIRD_PARTY.md`.
Headless tests cover typing, navigation, selection, clipboard, deletion and IME composition.
The allocation test observed zero allocations in warmed Omnia paint code for both the placeholder
and populated text, with a caret and a selection. Formatting, workspace Clippy, bench Clippy with
`scene-counts,readback`, app tests, workspace tests and `cargo deny check` all passed on a Windows
development machine.
2026-11-23 U4.2 done — Part picker (UD2, UD4, UK3, UINV5). New `crates/omnia-app/src/parts/picker.rs` (`parts/mod.rs`, a `lib.rs` mod line, and an `omnia-symbols` path dependency in `crates/omnia-app/Cargo.toml`). `PartPicker` is a cached view holding the twelve place-strip buttons and the "In use · N parts" picker. Each non-empty category opens a popover of its built-in symbols from T2.3's library, and empty categories are disabled buttons. The "In use" list is `in_use_entries(doc)`: symbol, display name and count, ordered by name then lowest id. Its counts sum to `Doc::part_count`, and it is disabled while the document is empty. Choosing a row dispatches `PlacePart { symbol }`, gated by `place_part_enabled` (`CanvasEdits::place`, off until U5.2). With the flag off, every row is disabled and a direct `choose` re-checks `enabled` first, so nothing is dispatched (UINV5). Chosen, not in the plan: the symbol-to-category table `SYMBOL_CATALOG` (resistor, potentiometer and capacitor under Basic; diode under Diodes; op-amp under Analog; ground under Sources), with a test that keeps it complete. Place > Part is not built yet (U1.5 owns the menu bar); `part_menu()` and `in_use_entries()` are the data it must read so the menu and the popovers stay identical. Outside the owned files: `crates/omnia-app/src/chrome/toolbar.rs` (U1.6, done; U4.3 not started) now embeds `PartPicker` with `.cached(..)` at `PART_PICKER_WIDTH` in place of its disabled strip and "In use" button. `in_use_label` and the in-use count moved to the picker, and its tests were updated to match. Headless tests: every category and the "In use" picker open by mouse and by keyboard (Enter and Space; Esc returns focus to the button); a second press closes without reopening; the rows match `part_menu` and the document; flag off dispatches nothing, flag on dispatches once and closes; the picker renders on a place-flag flip only while a popover is open; hover across an open popover and the strip never re-renders a cached sibling. GPUI fact for the owner (pinned revision): `Window::focus` calls `refresh()`, and a refreshing window re-renders every cached view (`view.rs`, `!window.refreshing`). GPUI also focuses a focusable element on mouse-down. So any click on a focusable chrome control (the existing toolbar buttons too) and any popover that takes focus for Esc and Tab re-renders `SheetView`. This conflicts with UINV2 ("menu open"), UTC-instrument ("sheet paint count 0 throughout") and UTC-menu-keys as written. It needs an owner decision, recorded as a new UG fact, before U1.5, U4.1 and U4.5. Visuals use plain text rows (no symbol thumbnails) until the U3.7/U4.7 goldens. All five standard checks pass on a Windows development machine (certifies nothing).
2026-11-23 U4.2 review fixes — Independent review (GPT-6.2 Sol, PR #96). Fixed: (1) a second press that closes a popover through click-outside now leaves focus on its button, so Enter reopens it (before, focus stayed on the removed panel); (2) focus leaving an open popover closes it (`on_focus_out`, notify deferred past the draw), so Tab past disabled rows no longer strands an open popover out of Esc's reach; (3) the module comment no longer claims open and close spare the sheet, and states the GPUI focus-refresh fact instead. Regression tests for (1) and (2). Pending integrations recorded as notes on U1.5 (Place > Part reads `part_menu`/`in_use_entries`) and U2.2 (feed `follow_document` from the active document). Not changed: the same-tick suppression reset (a separate press before the executor ticks is swallowed; real input always crosses a tick, and the toolbar's analysis picker uses the same pattern); the PR attribution footer.
2026-11-23 U0.1 done — Owner review (UH0). The owner delegated every pending sign-off to the coordinating agent on 2026-11-23. ui-plan.md section 13 now has an Answer column: UQ1 to UQ13 keep their defaults, dated. Section 14 is approved as written, so U0.2 can start. New UQ14 records the decision U4.2 asked for: UINV2, UB3, UTC-menu-keys and UTC-instrument stay as written; pointer input in the chrome never calls `Window::focus`; open menus and popovers take their keys through the workspace root's key context; a keyboard focus move is not a shell-only change. Review (GPT-6.2 Sol) found instrument clicks and F10 still moved focus; UD2, UD3, UD5, UTC-menu-keys, UTC-instrument and U4.1 now use a workspace key target with `capture_action` routing instead, paint counts unchanged. A second review asked for the routing to be specified and owned: UD3 now binds the overlay keys in `Workspace` and captures deeper actions such as `Sheet`'s Esc; U1.5 owns `key_target.rs` and migrates the part and analysis pickers; U1.7 re-runs UTC-menu-keys on the real workspace; U4.5 notes that only keyboard focus moves. A third review found the overlay bindings would fail the UINV4 validator and that Esc relied on a `Sheet` binding T5.2 has not made: navigation keys are now bound once in `Shell` to `Nav*` actions routed by the `KeyTarget` router, Esc leaves `Sheet` (T5.2 provides `SheetCanvas::cancel`, section 14 row updated), and U1.5 also owns the `Nav*` registry and keymap changes and the pickers' dismissal paths. A fourth review added: the router captures `Nav*` on the root so an open overlay wins over a Tab-focused button; `Nav*` actions live in `default_bindings()`, not `ACTIONS`; U1.5 suppresses mouse-down focus in `widgets/button.rs`; U5.2 focuses the sheet only when focus is elsewhere. A fifth review added: `Div::on_click` refreshes on press and release (new UG16), so chrome buttons activate from their own mouse handlers; a focused `TextField` yields navigation keys while an overlay is the key target; UQ14 records starting a canvas tool from outside the sheet (U5.2) as the one pointer-driven focus move; U1.5's check separates picker dismissal from placement. A sixth review found GPUI refreshes the whole window on every switch between mouse and keyboard input (new UG17), which no chrome design can avoid; that is recorded as UQ15 for the owner, with the invariants unchanged and affected tasks to go `blocked (UQ15)`. Its two nits (capture-phase suppression, click-outside over the sheet) are added to U1.5. If U1.5 cannot build that at the pinned GPUI revision it goes `blocked (UQ14)` and returns to the owner. UG15 records the GPUI fact behind it (from U4.2's log; sections 4, 6, 8 and 12 and U4.1 are outside U0.1's Owns line and is edited here because the U4.2 log asked for the fact to be recorded with the decision). Docs only; nothing measured.
2026-11-23 U0.5 done — Orion reference campaign (owner-approved, run remotely by the coordinating agent; canvas plan 2.1): frozen `run.sh` at `main` `0507b1c`, 10 rotating passes × 4 cases, AC and `performance` governor verified in all 80 snapshots, Baloo suspended. UB10 passes on Orion: `save_10k` median p99 27.314 ms (range 22.617–54.223) < 33.3 ms. `load_10k` median p99 111.4 ms (background executor per D8/UD7). Evidence in `docs/measurements/U0.5/orion-2026-11-23/`. U0.6 needs no `DocSnapshot` request for US4.
