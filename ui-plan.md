# Omnia application UI: plan (reference)

This file is the reference for everything around the sheet: the window and its chrome, menus,
toolbars, panels, tabs, dialogs, the Home and First boot screens, the action and keyboard
system, the file lifecycle, preferences and theme switching. The work itself is in
[ui-roadmap.md](ui-roadmap.md), which lists tasks that cite the ids defined here.

It sits beside [canvas-plan.md](canvas-plan.md) and [wiring-plan.md](wiring-plan.md) and
re-plans nothing they decide. Every `B`, `K`, `G`, `D`, `INV`, `Q` id is defined in the canvas
plan; every `W`-prefixed id in the wiring plan. Where this plan needs a change in either, section
14 lists it; roadmap task U0.2 applies the list.

Written 2026-11-16. GPUI facts in section 4 were read from source at the pinned revision
`15d136bf8e72ae43b284183ec46940c009460f5c` on that date. No UI code beyond T1.5's status bar
exists yet.

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `UB` | Performance or quality budget, a hard requirement | 2 |
| `UK` | Constant: a number the shell must agree on | 3 |
| `UG` | Fact about GPUI, read from source at the pinned revision | 4 |
| `UD` | Decision | 6 |
| `UINV` | Invariant: a rule that must hold in code, each with a test | 7 |
| `UTC` | Scenario with its expected result | 8 |
| `US` | Spike: a measurement that settles something reading could not | 9 |
| `UQ` | Open question for the owner, each with a default | 13 |

Status words are the canvas plan's: **DECIDED**, **HYPOTHESIS** (build only after the named
spike confirms it) and **DEFAULT** (the owner has not answered; build the default and do not
ask again).

## 1. Scope

In scope:

- **Workspace shell.** Window chrome and layout, the menu bar, the toolbar, the instruments
  strip, sheet tabs, and the Toolbox, Spreadsheet and Results panels as layout and toggles.
  Their contents are stubs except where a stage 2 workflow needs them: the part picker that
  starts placement. Floating instrument windows: frame, drag, stacking and focus only.
- **Other screens.** The status bar beyond T1.5, the Home screen (recent files, the sidebar),
  First boot.
- **Action system.** One registry behind menus, toolbar, shortcuts and tooltips, consistent with
  T5.2's keys, with an enabled state for every action.
- **File lifecycle.** New, open, save, save as, recent files, unsaved-changes prompts on close
  and quit, autosave, crash recovery, file errors, the `.omnia` extension.
- **Multiple documents.** Tabs, one `Doc` per tab, switching without losing camera, selection or
  undo history.
- **Theme switching** between Graphite and Gray at runtime, and following the system.
- **Preferences.** A minimal dialog and file, including WD12's wiring preferences.
- **Accessibility basics.** Keyboard reachability, focus order, contrast in both themes,
  screen-reader roles where GPUI provides them.

Out of scope: canvas behaviour (the shell hosts `SheetCanvas` and calls its public methods);
simulation internals, models, netlists, instrument signal processing and result plots (the
shell reserves their places and the states idle, running, paused, failed); a component library
browser and property inspector beyond what placement needs (the data model is in
[parts-plan.md](parts-plan.md); Part Libraries and inspector UI will consume
`omnia-parts::Catalog` and `Doc::part_defs`; placement goes through `Catalog::place_symbol`,
wired by whichever of parts `P2.4` and U5.2 ships second, and File > Open calls
`SymbolLibrary::check_doc`, wired by whichever of parts `P2.3` and U2.3 ships second); printing; packaging, installers and updates; multiple
top-level workspace windows; multi-sheet designs and project containers (UQ3).

Repository facts this plan builds on:

- `crates/omnia-app/` holds `main.rs` (`--frames N`), `lib.rs` (font loading, a `Workspace` that
  embeds `SheetCanvas` and `StatusBar` as cached views) and `status_bar.rs` (T1.5). `lib.rs`
  still has a colour literal (`STAGE_PAPER`, `rgb(..)`) from T0.1; U1.1 removes it.
- `omnia-theme` exposes `Theme` with 35 tokens (22 from `design/omnia.figcraft`, 13 from
  `canvas-tokens.toml`), `GRAPHITE` and `GRAY`.
- `omnia-doc` has `Doc::apply`, `undo`, `redo`, `revision`, `save` and `load` (RON version 2).
  `revision` advances on undo and redo too, so it cannot say "back at the save point" (UD6).
- The mockups are `design/mockups/{workspace,home,first-boot}-{graphite,gray}.png`, drawn by
  `design/build-omnia.mjs`, which is the source of the numbers in section 3.

## 2. Budgets

Measured on the reference machine under canvas plan section 2.1: the Orion laptop, 1920x1080 at
60 Hz, 100% and 125%, ten interleaved repetitions, the gate reads the median of run p99s and
reports the range. Numbers from any other machine are recorded and labelled and never mark a
budget met. Every canvas budget (B1 to B10) still holds with the full shell around the sheet;
these are added.

`bench` means `cargo run --release -p omnia-bench --`. `--shell` builds the full workspace
(chrome, toolbar, tabs, strip, status bar, one untitled document) around the sheet instead of
the bare sheet. `bench ui` is the new shell script runner (U1.10).

| ID | Budget | Measured by |
| --- | --- | --- |
| UB1 | Cold start. With the full shell, B1 still holds (< 400 ms to an interactive empty sheet), and the shell's own share is ≤ 80 ms: `start --shell` median minus `start` median. Home with 20 recent entries is interactive in < 400 ms. | `bench start --shell`, `bench start`, `bench start --home --recent 20` |
| UB2 | Responsiveness. Opening a menu, a popover or a dialog, toggling a panel, and switching between two `1k` tabs are drawn in the next frame: dirty-to-present p99 ≤ one refresh interval, and frame p99 within B3 while the `1k` sheet is open. | `bench ui --sheet 1k --script menus\|popovers\|dialogs\|panels\|tabs` |
| UB3 | Isolation. Hover over chrome, menus, toolbar state changes, status updates, toasts, tab-strip hover and instrument-frame moves leave `SheetView`'s paint count at 0 (INV11, UINV2). A panel or splitter resize paints the sheet at most once per frame. | `bench ui --script chrome-hover\|instrument-drag\|resize`: `sheet_paints` |
| UB4 | Idle. 0 frames drawn and 0 view renders over 10 s with the shell, two open documents (one modified), the simulation-time readout showing a fixed value, and no toast. | `bench idle --shell --docs 2 --modified --seconds 10` |
| UB5 | Allocations. No heap allocation from our code in any frame once warm, shell included: `allocs_per_frame.ours == 0` in pan, zoom and hover runs with the full shell, while the cursor, zoom and grid readouts change every frame (Q5 scope: our code only). | `bench run --shell --sheet 1k --script pan\|zoom\|hover` |
| UB6 | Tab switching between two open `10k` sheets: input to the switched sheet presented < 50 ms (p99). After any number of switches each document's camera, selection, undo and redo depth and history cursor are unchanged (UINV8). | `bench ui --sheet 10k --docs 2 --script tabs` |
| UB7 | Theme switching costs under one frame of jank: no present interval longer than 2 refresh intervals in the 10 frames around a switch with `1k` open; with `10k` open, B4's limit (2.5) holds. No colour literal anywhere in the shell (UINV1). | `bench ui --sheet 1k\|10k --script theme` |
| UB8 | Scale factors. The shell is laid out and drawn correctly at 100%, 125% and 150% and after a live change between them (T7.4 for the sheet). | Goldens at 1.0, 1.25, 1.5 (Windows); scale-change test; reference runs at 100% and 125% (section 2.1 has no 150% on Orion) |
| UB9 | Memory. B9 (< 300 MB) holds with the shell and one `10k` document; two `10k` documents open stay < 450 MB (chosen). | `memory_mb` in the UB6 run |
| UB10 | File work never stalls the UI. Opening a `10k` file: no UI-thread frame over 50 ms (B5) while it loads. Saving or autosaving a `10k` document: the UI-thread part (serialising) < 33.3 ms p99. Autosave never runs during a gesture. | `bench ui --sheet 10k --script open\|save\|autosave`: `max_frame_ms`, `ui_stall_ms` |

UB2 "drawn in the next frame" uses GPUI's own dirty-to-present histogram (G13): the frame that
handles the input presents within one refresh. UB6 is an end-to-end input-to-present time
because the incoming sheet has no cached frame and must paint once (D7).

## 3. Constants

All from `design/build-omnia.mjs` unless marked "chosen". Logical pixels.

| ID | Constant | Value |
| --- | --- | --- |
| UK1 | Window | Mockup frame 1440x900. Default size 1440x900 clamped to 90% of the display (chosen). Minimum 960x600 (chosen). Backdrop `bg/backdrop`. |
| UK2 | Title and menu bar | One row, 36 high, `bg/backdrop`. Logo tile 22x22 at (12, 7), radius 6. Menu labels Inter 13 from x = 48, each label's width plus 20 apart: File, Edit, View, Place, Simulate, Tools, Reports, Window, Help. Title centred, Inter 12.5 `text/secondary`. Window controls minus, square, x at 16 px, 30 apart, from the right edge. |
| UK3 | Toolbar | 50 high, `bg/panel`. Groups 34 high, `bg/inset`, radius 9, 2 px padding; buttons 30x30, radius 7, active `bg/raised`; icons 18 px, stroke 1.35. 8 between groups, 14 before the place strip with a 1x22 `rule` divider. Order: file (new, open, save, print), edit (undo, redo, cut, copy, paste), zoom (in, out, area, fit, full screen), place strip (12 category buttons), "In use · N parts" picker (158 wide, `rule` stroke), then right-aligned: analysis picker (118 wide), run controls (136 wide: Run 60x28 `button/primary`, pause, stop), simulation-time pill (74x24, radius 12, `rule` stroke). |
| UK4 | Sheet area and strip | Sheet at x = 8, y = 92 (6 below the toolbar), radius 10, 6 from the strip. Instruments strip 52 wide, 8 from the right edge, `bg/panel`, radius 10; items 36x36, radius 8, every 44; icons 20; a "placed" dot 6 px `text/primary`; divider then the probe item. |
| UK5 | Tab row | 32 high at 8 below the sheet. Tab width = label width + 32, + 12 with the modified dot; 6 apart; radius 8. Active: `bg/panel`, Inter SemiBold 12.5 `text/primary`. Inactive: `rule` stroke, Inter 12.5 `text/secondary`. Modified dot 6 px `text/secondary`. "+" 32x32. Panel toggles right-aligned, 32 high, `rule` stroke, label Inter 12, shortcut Source Code Pro 10.5. |
| UK6 | Status bar | 40 high (T1.5's `STATUS_BAR_HEIGHT`), `bg/panel`, 1 px `rule` on top. Left: message Inter 12 `text/secondary` at x = 14; context hint Inter 12 at x = 220. ERC indicator at x = 760. Right-aligned readouts Source Code Pro 11.5 `text/secondary`, 5 spaces apart. |
| UK7 | Type | UI text Inter 400/500/600 at 11 to 14 px; screen titles Inter SemiBold 22 (Home) and 30 (First boot). Values, shortcuts, readouts and counts Source Code Pro 10 to 11.5. No other faces. |
| UK8 | Home | Sidebar 288 wide at (12, 44), radius 14, `bg/panel`; main panel from x = 312 to 12 from the right, radius 14. Cards 3 per row, 236 high, 16 apart, `bg/card`, radius 12, thumbnail 152 high on `stage/paper`. Template tiles 6 per row, 110 high, `bg/inset`. |
| UK9 | First boot | Frameless except the window controls. Hero panel 588 wide at (12, 12), radius 18. Setup column at x = 640, 760 wide. Footer buttons "Open a sample" and "Start designing" (primary). |
| UK10 | Floating instrument frame | `bg/card`, 1 px `rule`, radius 12, drop shadow offset (0, 14) blur 36 in `shadow`. Header 38 high: designator Source Code Pro 11, title Inter SemiBold 13, close button. Default size per instrument; the oscilloscope is 576x468. |
| UK11 | Docks (chosen) | Toolbox: left of the sheet, default 260 wide, 200 to 480. Bottom dock (Spreadsheet, Results): default 240 high, 120 to 60% of the body. Sheet area never below 320x240: a dock that would push it below collapses. Splitter hit area 6 px, drawn as 1 px `rule`. |
| UK12 | Timing (chosen) | Autosave 60 s after the first unsaved edit, deferred until 2 s without input and never during a gesture. Toast lifetime 8 s. Status message until the next input or 5 s. Tooltip delay 500 ms. Panels, menus and dialogs appear without animation. |
| UK13 | Lists (chosen) | Recent files kept: 20. Home shows 6 cards; the list view shows all 20. Window menu lists every open tab. |
| UK14 | Files | Extension `.omnia`. Untitled documents "Untitled 1", "Untitled 2", … per session. Preference file `prefs.ron`; state files `recent.ron`, `window.ron`; recovery under `recovery/`. Locations in UD10. |

## 4. GPUI facts

Read from source at the pinned revision, paths relative to the GPUI checkout's `crates/`
directory (canvas plan section 4 says where it is). Before relying on an API, open it again; if a
fact no longer matches, stop and log it.

| ID | Fact | Where | Consequence |
| --- | --- | --- | --- |
| UG1 | `set_menus` on Windows and Linux stores the menu list and draws nothing; `get_menus` returns it. There is no native menu bar on either platform at this revision. | `gpui_windows/src/platform.rs` `set_menus`; `gpui_linux/src/linux/platform.rs` `set_menus` | Menus are drawn by us (UD2). |
| UG2 | `WindowKind::AnchoredPopup` is rejected with `PopupNotSupportedError` on Windows and X11; only Wayland implements it. The comment says callers fall back to in-window popovers. | `gpui_windows/src/window.rs` `new`; `gpui_linux/src/linux/x11/window.rs` | Menus, popovers and tooltips are in-window layers (UD2, UD5). |
| UG3 | `Window::prompt` uses a native dialog on Windows; Wayland and X11 return `None`, so GPUI renders its fallback in-window prompt. `on_window_should_close` takes a synchronous `bool`. `on_app_quit` "cannot cancel the quit". | `window.rs` `prompt`, `on_window_should_close`; `app.rs` `on_app_quit`; `gpui_linux/.../wayland/window.rs` `prompt` | Our own modal dialogs on every platform; close is refused and re-issued after the dialog; quit is our action (UD7). |
| UG4 | `prompt_for_paths(PathPromptOptions { files, directories, multiple, prompt })` and `prompt_for_new_path(dir, suggested_name)` use the native picker on Windows and the desktop file-chooser portal on Linux, which errors when no portal is installed. There is no file-type filter. | `gpui/src/platform.rs` `PathPromptOptions`; `gpui_windows/src/platform.rs`; `gpui_linux/src/linux/platform.rs` | Native pickers; we check the extension ourselves; a missing portal is a file error dialog (UD11). |
| UG5 | Custom title bars: `TitlebarOptions { appears_transparent }` (Windows), `WindowOptions::window_decorations` (`Client` or `Server`, Linux; "the platform may ignore requests"), `window_min_size`. `Div::window_control_area(Drag \| Close \| Max \| Min)` becomes native hit-test codes on Windows. On Wayland the hit-test callback is a no-op, so dragging and buttons use `start_window_move`, `start_window_resize(ResizeEdge)`, `set_client_inset`, `minimize_window`, `zoom_window`, `remove_window`; `window_decorations()` reports what was granted. | `gpui/src/platform.rs`; `gpui/src/elements/div.rs` `window_control_area`; `gpui_windows/src/events.rs`; `gpui_linux/.../wayland/window.rs` | UD1. KDE on Wayland behaviour is spike US1. |
| UG6 | Actions and keys: `actions!`, `KeyBinding::new(keys, action, Some(context))`, `App::bind_keys`, `.key_context(..)`, `.on_action(..)`, `Window::bindings_for_action`, `Keystroke::unparse(os)`, `App::is_action_available`, `Window::dispatch_action`, `Window::context_stack`. Deeper key contexts win. | `gpui/docs/key_dispatch.md`; `keymap/binding.rs`; `window.rs`; `app.rs`; `platform/keystroke.rs` | UD3. |
| UG7 | Overlays: `deferred(child).priority(n)` paints after its siblings; `anchored()` with `attach`, `position`, `snap_to_window`. `occlude_mouse` and `block_mouse_except_scroll` stop events reaching what is below. | `elements/deferred.rs`, `elements/anchored.rs`, `elements/div.rs` | Menus, popovers, dialogs, toasts. |
| UG8 | Focus: `FocusHandle::tab_index`, `tab_stop`, `Window::focus_next` and `focus_prev`. | `window.rs`; `examples/tab_stop.rs` | UINV12. |
| UG9 | Accessibility: GPUI exposes elements that have an `id` and a `role(Role::..)` to assistive technology through AccessKit; the Windows and Unix adapters are in `Cargo.lock`. | `gpui/src/_accessibility.rs`; `examples/a11y.rs` | Roles on chrome controls (UD20). |
| UG10 | File drops arrive as `PlatformInput::FileDrop` on Windows, Wayland and X11 and reach `.on_drop::<ExternalPaths>()`. | `gpui_windows/src/window.rs`; `gpui_linux/.../wayland/client.rs` | Drop to open (UD7). |
| UG11 | Test contexts: `simulate_keystrokes`, `simulate_input`, `simulate_click`, `simulate_mouse_move`, `simulate_resize`, `dispatch_action`, `debug_bounds(selector)`, `simulate_new_path_selection`, `did_prompt_for_new_path`, `simulate_prompt_answer`, `has_pending_prompt`. They run on GPUI's test platform with no OS window. | `gpui/src/app/test_context.rs` | Headless shell tests in CI (UD21). |
| UG12 | `set_window_title`, `toggle_fullscreen`, `is_fullscreen`, `window_bounds`, `appearance()`, `observe_window_appearance`, `scale_factor()`. | `window.rs` | Title, Full screen, Match system, window restore. |
| UG13 | GPUI has no text-field element. `examples/input.rs` builds one from `EntityInputHandler` and `ElementInputHandler`. | `gpui/examples/input.rs` | UD16. |
| UG14 | `Application::with_assets(impl AssetSource)` serves bytes to `svg()`; SVGs rasterise once per size into the atlas (G4). | `app.rs` `with_assets`; `assets.rs` | Icons (UD17). |
| UG15 | `Window::focus` calls `refresh()`, and a refreshing window re-renders every cached view (`!window.refreshing` in the cache check). A mouse-down on a focusable element focuses it. | `window.rs` `focus`; `view.rs`; `elements/div.rs` (mouse-down focus, which a handler can suppress) | Pointer input in the chrome must not move focus (UQ14). Found by U4.2. |
| UG16 | `Div::on_click` records the press with `window.refresh()` on mouse-down and refreshes again when the release over the hitbox completes the click. | `elements/div.rs` (click listeners, mouse-down and mouse-up handlers) | Chrome controls activate from their own `on_mouse_down`/`on_mouse_up` handlers with pressed state in the owning view, never `on_click` (UQ14). Found by the UH0 review. |
| UG17 | `Window::dispatch_event` tracks input modality (keyboard, mouse, touch) and calls `refresh()` whenever it changes, before any handler runs. A refreshing window re-renders every cached view (UG15), so the first key after mouse input, or the first mouse move or press after a key, repaints `SheetView` once. Headless `simulate_keystrokes` uses the same dispatcher. | `window.rs` `dispatch_event`; `view.rs` cache check | No chrome code can avoid it; UQ15. Found by the UH0 review. |

Not settled by reading, so measured in section 9: whether a `deferred` child of a `.cached()` view
replays and occludes correctly (US2); what our render code allocates for a view whose only child
is a custom element (US3); whether KDE on Wayland grants client decorations and how resize edges
behave (US1); `Doc::save` and `Doc::load` time at `10k` on Orion (US4); whether `render_to_image`
captures deferred layers and the title bar (US5).

## 5. Architecture

```
 OS window (one per app) ── client-drawn chrome (UD1)
 │
 Shell (window root entity) ── screen: FirstBoot | Home | Workspace          app globals:
 │   notified only by: screen switch, modal open/close, theme              Prefs, ActiveTheme,
 │                                                                          RecentFiles
 ├── TitleBar ·cached·        title, window controls        ← Workspace (title/dirty change)
 │    └── MenuBar ·cached·    drawn menus (UD2)             ← its own open/hover state
 ├── Toolbar ·cached·         icon groups, pickers, sim     ← CommandState flips only
 ├── body row
 │   ├── Dock(Toolbox) ·cached·, optional
 │   ├── SheetArea
 │   │    ├── SheetCanvas (active tab) ·cached·  ── SheetView ·cached·, OverlayView  (canvas plan D7)
 │   │    ├── InstrumentLayer ·cached·  floating frames, z-order (UD5)
 │   │    └── ToastLayer ·cached·
 │   └── InstrumentsStrip ·cached·
 ├── Dock(Spreadsheet | Results) ·cached·, optional
 ├── TabStrip ·cached·        tabs, modified dots, panel toggles
 ├── StatusBar ·cached·       message, hint, ERC, Readout elements (UD18)
 └── ModalLayer (deferred)    dialogs; blocks input beneath (UINV13)

 Workspace ── Vec<Entity<Document>> ── Document { Entity<Doc>, Entity<SheetCanvas>, path,
                                                  saved cursor, autosave state }
 CommandState (per window) ◄── CanvasEvent (status, selection, history)  ◄── SheetCanvas
        │                  ◄── Document events (dirty, path)
        └──► MenuBar (read at open), Toolbar (observes, notified on flips), StatusBar

 Actions: keystroke / menu item / toolbar click ──► GPUI action ──► handler on Workspace
          ──► Document / SheetCanvas public method ──► Doc::apply|undo|redo (canvas) ──► Change

 Files:   UI thread: Doc::save → bytes ─► background: write temp, rename (UD7)
          background: read, Doc::load ─► UI thread: Entity<Doc> + SheetCanvas (D8)
```

Data flows one way, as in the canvas plan. Input reaches a GPUI action; the action's handler on
`Workspace` calls a public method of the active `Document` or `SheetCanvas`; the canvas applies
the command and emits events; `CommandState` folds those events into booleans; the chrome views
that show those booleans are notified only when a boolean flips.

Invalidation is D7 extended to the shell. Every chrome view is its own entity embedded with
`.cached(..)`. A view is notified only by the state it shows. The window root (`Shell`, and
`Workspace` inside it) re-renders only for a layout change: switching screen or tab, toggling or
resizing a dock, opening or closing a modal, switching theme. Because every child is cached, a
root re-render replays the sheet's previous primitives (G6) and does not run `SheetView`'s paint
code. Hover, menu, toolbar, status, toast and instrument-frame changes notify only their own view
(UINV2). The only shell actions that repaint the sheet are the ones D7 already allows: a theme
change, a scale-factor change, and a viewport change from a dock resize, at most once per frame.
Independently of the shell, GPUI itself refreshes the window, and so repaints the sheet once,
when input switches between mouse and keyboard (UG17); whether that repaint is accepted is UQ15,
and until it is answered the counts in UINV2, UB3 and the UTC rows stay as written.

## 6. Decisions

### UD1 Window chrome: one client-drawn bar with title, menus and controls. DECIDED (Linux part HYPOTHESIS until US1)

- The mockup draws the title, the menu bar and the window controls in one 36 px row (UK2). The
  shell draws it on both platforms; there is no system title bar above it.
- Windows: `TitlebarOptions { appears_transparent: true }`. The bar's empty area is
  `window_control_area(Drag)` and the three buttons are `Min`, `Max`, `Close`, so the system
  handles dragging, double-click maximise and snap layouts (UG5).
- Linux: request `WindowDecorations::Client`. Drag with `start_window_move`, buttons call
  `minimize_window`, `zoom_window` and the close path in UD7, and the window edge gets resize
  handles through `set_client_inset` and `start_window_resize`. If the compositor answers
  `Decorations::Server`, the shell hides its three buttons and keeps the row as a menu bar under
  the system title bar.
- The window title (`set_window_title`) mirrors the drawn title so the task switcher shows it:
  `<file name> — Omnia`, with the modified dot after the file name when modified, `Omnia` on Home.
- First boot uses the same window with only the controls drawn (UK9).

### UD2 Menus: drawn in the window, shortcuts from the live keymap. DECIDED

- There is no native menu bar to use (UG1), and native popups are unavailable on Windows (UG2).
  `MenuBar` draws the nine top-level menus and opens each as an `anchored` popover inside a
  `deferred` layer, positioned under its label and kept inside the window.
- Menu contents come from the action registry (UD3): each item is an action, a separator, or a
  submenu. Enabled and checked states are read from `CommandState` when the menu opens and while
  it is open; a disabled item is drawn in the disabled colour (`text/muted`, or its UQ1
  substitute), is skipped by arrow keys, and does nothing.
- Shortcut text is the action's last binding in the current context
  (`Window::bindings_for_action`), formatted with `Keystroke::unparse` for the running OS:
  `Ctrl+S`, `Ctrl+Shift+S`, `F11`. The mockup's command-key glyphs are not used on Windows or
  Linux (UQ11).
- Keyboard: F10 makes the menu bar the key target (UQ14) and highlights the first menu without
  moving GPUI focus; Alt+underlined letter opens a menu the same way; arrows move, Enter
  activates, Esc closes one level and returns the key target to where it was. There is no bare-Alt
  activation: Alt held at commit is the wiring follow modifier (WK3), and releasing it after a
  drag must not open a menu.
- Opening a menu, moving between items and hovering notify `MenuBar` only. US2 confirms that a
  deferred popover inside a cached view replays and occludes correctly; if it does not, the
  popover moves to the `ModalLayer`'s deferred slot, which is notified instead.
- Menu contents for stage 2, by menu: File (New, Open, Open Recent ▸, Save, Save As, Close
  Tab, Home, Preferences, Quit); Edit (Undo, Redo, Cut, Copy, Paste, Delete, Select All, Rotate,
  Mirror Horizontal, Mirror Vertical); View (Zoom In, Zoom Out, Zoom Area, Fit, Full Screen,
  Toolbox, Spreadsheet, Results, Theme ▸); Place (Part ▸ by category, Wire, Net Label); Simulate
  (analysis items, Run, Pause, Stop: all disabled, UD19); Tools, Reports (empty except a disabled
  "Not available yet" item); Window (Next Tab, Previous Tab, open tabs, open instruments); Help
  (Keyboard Shortcuts, About Omnia).

### UD3 One action registry; contexts keep canvas keys and shell keys apart. DECIDED

- Every user command is a GPUI action. Shell actions are declared in `omnia-app`
  (`actions.rs`); canvas tool actions are declared by T5.2 in `omnia-canvas` (`keymap.rs`) and
  re-used, never redeclared.
- One static table, `ACTIONS: &[ActionSpec]`, describes every action once: id, label, icon,
  menu placement, default bindings with their key context, and two functions of `CommandState`,
  `enabled` and `checked`. Menus, toolbar buttons, tooltips ("Save (Ctrl+S)") and the Help >
  Keyboard Shortcuts list are all generated from it. No other code builds a menu item or a
  binding.
- Key contexts, outermost first: `Shell` (window root), `Workspace`, then exactly one focused
  context of `Sheet` (the canvas has focus), `Panel`, `Dialog`, `Home`, `FirstBoot`, or a chrome
  control reached by Tab (UINV12); `TextField` nests inside `Dialog`, `Home` or `Panel`. Menus,
  popovers and instrument frames are not focused contexts: they are key targets held by the
  workspace (UQ14).
- Who binds what:
  - `Shell` and `Workspace` bind every modifier shortcut: Ctrl+N, O, S, Shift+S, W, Q, Z, Y,
    Shift+Z, X, C, V, A, comma, =, -, 0, 1, 2, 3, Tab, Shift+Tab, F1, F10, F11, and Delete.
  - `Sheet` binds the single-key canvas tools from T5.2 (R, X, Y, W, and Space as T1.5 and
    T6.2 use it), so typing in a text field can never rotate a part. Esc is not bound in `Sheet`
    (UQ14): the shell's `NavCancel` handler calls `SheetCanvas::cancel` (T5.2) when the sheet is
    the key target.
  - The focused `TextField` handles its editing keystrokes before keymap dispatch (Ctrl+A,
    Ctrl+C, Ctrl+V, Ctrl+X, arrows, Backspace, Delete). It consumes document undo and redo
    keystrokes while focused. Text insertion goes through GPUI's input handler.
  - Navigation keys are bound once, in `Shell`, so no two active contexts share one (UINV4)
    and they resolve on every context stack, with or without `Workspace`: Up, Down, Left, Right,
    Enter, Esc, Tab and Shift+Tab map to `NavUp`, `NavDown`, `NavLeft`, `NavRight`,
    `NavConfirm`, `NavCancel`, `NavNext` and `NavPrev`. These are internal actions, not user
    commands: they are not in `ACTIONS` (so UINV3's menu-entry rule does not apply) and
    `default_bindings()` adds them beside `FocusMenuBar`, where the UINV4 validator covers them.
    No other context binds these keys;
    `Dialog`, a Tab-focused chrome control and the `Home` menu handle the `Nav*` actions with
    `on_action`, which runs before `Shell`'s handler because they are deeper on the dispatch path.
    Space on a Tab-focused chrome control is handled by that control's own key-down listener, not
    by a binding.
  - Key target routing (UQ14). Chrome buttons and popover rows suppress GPUI's automatic
    mouse-down focus and activate from their own mouse-down and mouse-up handlers, not
    `on_click`, which refreshes the window on press and release (UG16); the shared `widgets/`
    button does both once for all of them. Menus, popovers
    and instrument frames never take GPUI focus from a pointer, from F10 or Alt+letter, or when they close; the `KeyTarget` router (`Sheet`, `Menu`,
    `Popover(id)`, `Instrument(id)`) records which one receives keys. The router registers
    `capture_action` for every `Nav*` action on the root element; the capture phase runs root
    first, before any focused control's `on_action`. While a menu, popover or instrument is the
    key target, the router hands the action to it and stops propagation, so a Tab-focused toolbar
    button cannot swallow Enter meant for an open menu. Opening a dialog closes any such overlay
    first, so a focused dialog always receives its keys. A focused `TextField` intercepts its
    editing keys before bindings resolve; while the key target is not `Sheet` it yields Enter,
    Esc, Up, Down, Left, Right and Tab (it reads the router's target), so they resolve to `Nav*`
    and reach the open overlay. With the target `Sheet`, the capture
    handler lets the action propagate; if nothing deeper handles it, the root's bubble handler
    acts: `NavCancel` calls
    `SheetCanvas::cancel` (a no-op until T5.2 provides it), `NavNext` and `NavPrev` move focus per
    UINV12, and the rest do nothing. Esc therefore closes an overlay whether the sheet or a
    Tab-focused toolbar control has focus, and needs no canvas binding.
- Undo, Redo, Cut, Copy, Paste, Delete and Select All are shell actions. Their handlers call the
  active canvas's public methods (`undo`, `redo`, `cut`, `copy`, `paste`, `delete_selection`,
  `select_all`; section 14 adds these to T5.2, T5.3 and T7.1). So the shortcut works wherever
  focus is in the workspace, and there is one handler per keystroke. T5.2 no longer binds
  Ctrl+Z, Ctrl+Y or Del itself (section 14).
- Enabled state: `CommandState` is a per-window entity holding plain booleans and enums (has a
  document, can undo, can redo, has selection, clipboard has a clip, document modified,
  untitled, simulation state, which docks are open). It is updated from `CanvasEvent`s and
  `Document` events. A handler re-checks `enabled` first and returns without effect when false
  (UINV5). The toolbar observes `CommandState` and is notified only when a value it shows
  changes.
- A keymap test builds every binding and fails if two bindings share a keystroke in contexts that
  can be active together (UINV4). Stage 3 bindings (for example the mockup's "A adjusts R3" and
  "Space pauses" hints) must pass the same test.
- No command palette in this plan (UQ2). The registry makes one cheap to add later.

### UD4 Ownership: entities per screen, per document and per chrome region. DECIDED

| Entity | Owns | Notified by |
| --- | --- | --- |
| `Shell` | current screen, `ModalLayer` | screen switch, modal open/close, theme |
| `Workspace` | documents, active index, chrome entities, dock layout | tab switch, dock toggle or resize |
| `Document` | `Entity<Doc>`, `Entity<SheetCanvas>`, path, saved cursor, load/save/autosave state | its own lifecycle; emits `DocumentEvent` |
| `TitleBar`, `MenuBar` | title text; open menu, highlighted item | title or modified change; its own input |
| `Toolbar` | picker open state | `CommandState` flips it shows; its own input |
| `TabStrip` | scroll offset, hover | document list, modified flags, active index, dock toggles |
| `StatusBar` | message, hint, ERC state; `Readout` state cells | message/hint/ERC change, active tab change |
| `InstrumentLayer` | frames: id, rect, z-order, focus | open, close, drag, raise |
| `ToastLayer` | queue of at most 3 toasts | push, dismiss, expiry |
| `HomeScreen`, `FirstBootScreen`, dialogs | their own state | their own input |

- App globals: `Prefs` (UD14), `ActiveTheme` (UD13), `RecentFiles` (UD10).
- The status bar follows the active tab: on a tab switch `Workspace` calls
  `StatusBar::follow(&canvas)`, which replaces its `CanvasEvent` subscription (T1.5's view keeps
  its D7 rule: it never reads the canvas while rendering).
- Inactive documents keep their `SheetCanvas` entity alive and out of the element tree. Their
  camera, selection, tool state and overlay live in that entity, and their history lives in
  their `Doc`, so a switch only changes which cached view is the sheet area's child (UD6, UB6).

### UD5 Floating windows and dialogs are layers inside the main window. DECIDED

- Instruments: each floating frame is a cached view inside `InstrumentLayer`, which sits over
  the sheet area (UK10). Reasons: Wayland clients cannot position top-level windows, and
  instrument frames sit at sheet-relative places; one window means one frame per refresh and
  one focus model; and tests run on the test platform.
- Drag moves the frame by updating its rect in `InstrumentLayer` state, which notifies only the
  layer; the sheet replays (G6), and T-replay already showed replay is cheaper than repaint. A
  click raises a frame to the top and makes it the key target (`Instrument(id)`, UQ14) without
  moving GPUI focus, so the sheet is not refreshed; Esc returns the key target to the sheet. Tab
  and F6 may move GPUI focus into a frame (UINV12); that keyboard move is not a shell-only
  change, and Esc from a frame that holds real focus also returns GPUI focus to the sheet. Frames are clamped so their header stays inside the sheet area, and they are clamped
  again after a window or dock resize. The strip button for an open instrument shows UK4's dot
  and raises it.
- Cost: an instrument that animates notifies only its frame; the frame's own paint plus sheet
  replay is what T7.3 gates. The frame is built here; T7.3 supplies the animated stub content and
  keeps its gate (section 14).
- Dialogs are `ModalLayer` views: a scrim in `shadow`, a centred card, `Dialog` key context,
  mouse occluded beneath (UINV13). `Window::prompt` is not used (UG3). The only OS windows besides
  the main one are the native file pickers (UG4).

### UD6 Documents: one file, one `Doc`, one tab; modified means "not at the save point". DECIDED

- A document is one `.omnia` file holding one sheet (UQ3), loaded into one `Doc`, shown in one
  tab with its own `SheetCanvas`.
- Dirty tracking uses a history cursor that `omnia-doc` adds (section 14): an opaque `u64` naming
  the current state in the undo history. An effective `apply` moves to a fresh cursor value (its
  new revision), `undo` moves back to the previous state's value, and `redo` forward again.
  Validated no-ops change nothing (D2).
- `Document` stores `saved_cursor` when a save completes. Modified ⇔
  `doc.history_cursor() != saved_cursor`. Consequences:
  - Undo back to the save point makes the document unmodified again; redo away makes it
    modified.
  - An edit after undoing past the save point starts a new branch. The save point's state is
    then unreachable, so the document stays modified even if the content happens to match. There
    is no content comparison.
  - A new untitled document starts unmodified and closes without a prompt until its first edit.
- Until the cursor API lands (U2.1, waiting for T5.1), modified is `doc.revision() !=
  saved_revision`. This is conservative: undo back to the save point still shows modified.
- The tab's dot, the title's dot and `CommandState::modified` change in the same frame as the
  edit (UINV6).

### UD7 File lifecycle. DECIDED

- **New** (Ctrl+N, toolbar, "+" tab): opens "Untitled N" with an empty `Doc`, camera at the
  origin at 100%.
- **Open** (Ctrl+O): native picker (UG4), several files allowed. A path that is already open
  activates its tab. Each file gets a tab at once in a "loading" state; reading and `Doc::load`
  run on the background executor (D8), and the `Doc` entity and canvas are created on the UI
  thread when it finishes. A failure closes the placeholder tab and shows the error dialog
  (UD11). Files dropped on the window (UG10) and paths on the command line open the same way.
- **Save** (Ctrl+S): an untitled document goes to Save As. Otherwise `Doc::save` writes to a
  `Vec<u8>` on the UI thread (UB10, US4), and the background executor writes
  `<name>.omnia.tmp` in the same directory, flushes it, and renames it over the target. The
  save point is set only after the rename succeeds, to the cursor captured when serialising, so
  edits made during the write leave the document modified. A failure leaves the old file
  untouched (UINV7).
- **Save As** (Ctrl+Shift+S): native save picker with the current name. `.omnia` is appended
  when missing. Saving to a path open in another tab is refused with a dialog.
- **Close tab** (Ctrl+W, middle-click, the hover "×" that replaces the dot): if modified, the
  unsaved-changes dialog asks "Save", "Don't Save" or "Cancel".
- **Close window and Quit** (Ctrl+Q, File > Quit, window close button, Alt+F4): if no document
  is modified, the window closes. Otherwise `on_window_should_close` returns `false`, and the
  shell shows one dialog listing every modified document with "Save All", "Don't Save" and
  "Cancel". Save All saves each in turn, asking Save As for untitled ones; cancelling any picker
  or any failed save stops the quit with nothing discarded. When all are resolved, the shell
  writes its state files, deletes recovery files, and calls `remove_window`; the last window
  closing quits (as today in `lib.rs`).
- **Recent files**: every successful open and save moves the path to the front of
  `RecentFiles` (UK13) with the time and the document's camera. Reopening restores that camera.
  A recent entry whose file is missing is shown greyed on Home; opening it shows the error
  dialog and offers to remove it.

### UD8 The `.omnia` file is the document's own serialisation. DECIDED

- Extension `.omnia`, as the mockup shows. Contents: exactly what `Doc::save` writes (RON
  version 2 today). The format and its versioning belong to `omnia-doc`; the shell neither wraps
  nor edits it.
- No compatibility promise before stage 5. A file with an unknown version, or one `Doc::load`
  rejects, opens nothing and shows the error dialog with the loader's message.
- View state (camera) is not stored in the file, so opening a file never makes it modified and
  two people opening it are not handed a camera. It is kept per path in `recent.ron`.

### UD9 Autosave writes recovery copies; recovery is offered at the next launch. DECIDED

- Autosave never writes the user's file. When a document has unsaved changes, UK12's timer
  serialises it (on the UI thread, outside gestures) and the background executor writes
  `recovery/<id>.omnia` plus `recovery/<id>.ron` (original path or "untitled", display name,
  time). `<id>` is the process id, launch time and a counter, so no new dependency is needed.
- Saving, discarding at close, or closing an unmodified document deletes that document's
  recovery files. A clean quit leaves none.
- At launch, any recovery file left over is shown on Home as a "Recovered" card with Restore and
  Discard; when the app opens straight into the workspace, a dialog offers the same choice
  first. Restore opens the copy as a modified document bound to its original path (or untitled),
  so the next Save writes it there.
- The timer exists only while some document has unsaved, un-autosaved changes (UINV9). A failed
  autosave shows one toast and retries at the next interval.
- Preference "Autosave recovery copies" (UD14), on by default (UQ5).

### UD10 Where state lives. DECIDED

- Preferences: `prefs.ron` in the per-user configuration directory: `%APPDATA%\Omnia\` on
  Windows; `$XDG_CONFIG_HOME/omnia/` (default `~/.config/omnia/`) on Linux.
- State: `recent.ron`, `window.ron` (size, maximised, last screen) and `recovery/` in the per-user
  state directory: `%LOCALAPPDATA%\Omnia\` on Windows; `$XDG_STATE_HOME/omnia/` (default
  `~/.local/state/omnia/`) on Linux. Wayland ignores a restored window position; size and
  maximised state are restored.
- Paths come from these environment variables directly; no directory crate is added. Every write
  is temp-then-rename. Files are written in a stable field order so that `load(save(x)) == x`
  byte for byte (UINV10).
- A missing `prefs.ron` means first run: First boot is shown. An unreadable one is replaced by
  defaults in memory, reported in a toast, and rewritten only when a preference next changes.

### UD11 Error reporting: dialog, status message or toast. DECIDED

| Kind | Used for | Behaviour |
| --- | --- | --- |
| Dialog | Anything that needs a decision, or that defeats what the user just asked for: unsaved changes; open failed (unreadable, invalid, unknown version, missing); save failed (Retry, Save As, Cancel); no file picker available; recovery offer at launch; Save As onto an open path | Modal (UINV13). Title says what failed; body names the file and the reason in plain words, then the system message in Source Code Pro. Default button is the safe one. |
| Status message | Feedback on the action just taken that needs no decision: "Saved wien-oscillator.omnia"; refusals from the canvas ("Move refused: no route for 2 wires", WD11 above WK7); "Nothing to paste" | Replaces the left status text until the next input or 5 s (UK12). Not announced as an error. |
| Toast | Background events not tied to the current action: autosave failed; preferences file unreadable, defaults in use; a recent file removed because it is missing | Top-right of the sheet area, `bg/card`, at most 3, dismiss button, 8 s life (UK12); its timer exists only while a toast is shown. |

### UD12 Layout: docks, splitters, and a defined order of giving way. DECIDED

- The Toolbox dock is left of the sheet. Spreadsheet and Results share the bottom dock and sit
  side by side when both are open. Sizes, limits and the sheet's minimum are UK11. Ctrl+1, 2 and
  3 toggle them, as the mockup's toggles show.
- Toggling is instant (UK12): no animated size changes, because an animated dock would resize
  the sheet every frame. A splitter drag changes the sheet's viewport and so repaints the sheet,
  at most once per frame (UB3).
- Below the mockup's width, the toolbar gives way right to left in this order: the place strip
  collapses into one "Place" popover button, then the "In use" picker into it, then the zoom
  group into an overflow menu. The analysis and run controls stay. Tabs scroll horizontally with
  chevrons once they overflow. The instruments strip scrolls vertically. The status bar drops,
  in order, the context hint, the counts, then the simulation time. Docks collapse before the
  sheet area goes below 320x240.
- Home: card columns 3, 2, 1 as the main panel narrows; the sidebar stays.
- Minimum window size is 960x600 logical pixels (UK1) at every scale factor, so at 150% the
  window is at least 1440x900 device pixels. The give-way order above is defined down to that
  size; nothing smaller is supported.

### UD13 Theme switching at runtime. DECIDED

- `ActiveTheme` is a global holding `&'static Theme` and the preference (`Graphite`, `Gray`,
  `System`). Every shell view reads colours from it at render through one helper and observes
  the global, so a switch notifies each shell view once. Each open document's `SheetCanvas` gets
  `set_theme` (canvas plan section 9), which repaints that sheet once (D7); inactive documents
  get it when they become active.
- `System` follows `Window::appearance()` and `observe_window_appearance`: dark appearances map
  to Graphite, light to Gray.
- No colour literal in `omnia-app` (UINV1). The mockup's few colours that are not tokens are added
  to the overlay token file (UQ6).

### UD14 Preferences: a small dialog and one file. DECIDED

- Preferences (Ctrl+,): Appearance (Graphite, Gray, Match system); Schematic symbols (IEEE,
  IEC; stored for new sheets, UQ12); Wiring (the four WK10 preferences: follow attached wires on
  move, follow pin threshold, rubber-band mode, delete wires with part); Files (autosave recovery
  copies). Changes apply at once and are written to `prefs.ron`.
- WD12's wiring preferences are added here, with persistence. `omnia-doc`'s `WiringPrefs` is
  unchanged: the shell keeps a plain mirror struct for the file and builds `WiringPrefs` through
  its checked constructor, so an invalid threshold in the file is rejected and reported, never
  used. `Workspace` passes the result to every `SheetCanvas` (`set_wiring_prefs`, section 14).
  Commands carry the `FollowPolicy` they were built with (WD12), so a change never alters what
  redo does.
- Not added: keymap editing, per-document preferences, font size.

### UD15 Home and First boot. DECIDED

- The window opens on First boot when there is no `prefs.ron`; on the workspace when files were
  passed on the command line; otherwise on Home. File > Home (Ctrl+Shift+H) shows Home with the
  documents still open; opening or creating a document returns to the workspace. Home is also
  shown when the last tab closes.
- Home sidebar: the wordmark, Search (filters recent files by name), New design (Ctrl+N), Home
  and Recent (working), Templates, Part libraries, Simulation results (shown disabled with a
  "Not available yet" tooltip, UQ7), Projects (the parent folders of recent files, with counts;
  clicking one filters), and the Instruments blurb. The account footer is omitted.
- Home main panel: "Recent designs" with the Grid/List switch and cards (UK8) for the newest six
  recent files, recovery cards first. A card shows the file name, folder and relative time, and a
  thumbnail: until U4.6 a plain `stage/paper` tile with the part count; from U4.6 a preview drawn
  as LOD boxes and wire quads from a capped summary stored next to the recent entry at save time.
  "Start from a template" lists the bundled samples (none until a fixture is published, then
  the T3.3 Wien oscillator); the row is hidden when there are none.
- First boot: Appearance (working, previews in each theme) and Schematic symbols (stored) as in
  the mockup. The mockup's third and fourth sections name third-party products and features that
  do not exist yet; they are drawn as reserved rows listing only what this build has: the
  built-in symbol set, and "Simulator: not available in this build" (UQ9). Enter accepts the
  shown defaults and writes `prefs.ron`. "Open a sample" is disabled until a sample ships.

### UD16 Text input: one small text field. DECIDED (HYPOTHESIS for allocation and IME until U3.1)

- GPUI has no text field (UG13). `widgets/text_field.rs` is a single-line field adapted from
  `examples/input.rs` (Apache-2.0, attributed in `THIRD_PARTY.md`): insert, delete, selection,
  clipboard, IME through `EntityInputHandler`. Used by Home search, the part picker filter and
  the threshold preference. Net-label text entry stays T7.2's.

### UD17 Icons and fonts. DECIDED

- Tabler icons (MIT) are embedded with `include_bytes!` behind an `AssetSource` (UG14) and drawn
  with `svg()` tinted by a token. Icons not yet vendored are added from the same release, 3.19.0:
  `chevron-right`, `chevron-left`, `check`, `alert-triangle`, `info-circle`, `settings`,
  `keyboard`, `file`, `trash`, `restore`, `layout-sidebar`, `layout-bottombar`, `table`.
- Inter and Source Code Pro are loaded once at start (T0.2); no other font.

### UD18 Status bar beyond T1.5. DECIDED

- Slots left to right (UK6): message ("Ready · F1 for help", or a UD11 status message); context
  hint from the active tool; ERC indicator (a `Mark`-style dot and text; "ERC not run" until ERC
  exists); then the right-aligned readouts: simulation time (`Tran: —` while idle), cursor
  position (kept from T1.5, UQ8), grid, zoom, and "N parts · M nets".
- Readouts that change during a gesture (cursor, zoom, grid, simulation time, counts) are drawn
  by `Readout`, a custom element that paints each character from a cached shaped glyph line in
  Source Code Pro. The face is monospaced, so each character is at a fixed advance and nothing is
  reshaped or allocated per frame (UB5). The readouts live in a small `Readouts` view notified by
  `CanvasEvent::Status`; the rest of the status bar re-renders only when a message, hint, ERC
  state or the active tab changes. US3 confirms the zero-allocation claim; if our render path
  allocates, the readout state moves into a shared cell painted by the element without a render.
- Counts need `Doc::part_count` and `Doc::net_count` (section 14).

### UD19 Simulation placeholders. DECIDED

- `CommandState` carries `SimState { Unavailable, Idle, Running, Paused, Failed }`. This build is
  always `Unavailable`: the analysis picker shows "Transient" greyed and opens a list with one
  disabled entry; Run, Pause and Stop are disabled with the tooltip "Simulation is not available
  in this build"; the time pill shows "—" with no dot; the status bar shows `Tran: —`.
- The states exist so the stage 3 plan can drive them; nothing else about simulation is built.

### UD20 Accessibility basics. DECIDED

- Every action is reachable from the keyboard: through a menu (F10, Alt+letter) or a binding
  (UINV3). Toolbar buttons and tabs are also `tab_stop`s with a defined order (UINV12).
- Focus is always visible: a 2 px ring in a token (UQ6) on the focused control.
- Chrome controls carry `id` and `role` so assistive technology sees buttons, menus, menu items,
  tabs, dialogs and status text with their labels (UG9). The sheet's contents are not exposed in
  this plan.
- Contrast: every text-on-background pair the shell uses meets 4.5:1, and every icon or
  indicator that carries meaning on its own meets 3:1, in both themes (UINV11). Measured from
  the generated tokens today: `text/primary` passes everywhere (9.65 to 17.57); `text/secondary`
  passes in Graphite (5.56 to 7.65) and in Gray on panel, card and raised (4.95 to 6.23) but not
  on Gray backdrop (4.25) or inset (4.43); `text/muted` fails in both themes (2.32 to 3.88).
  UQ1 decides; until then the shell substitutes the next stronger token where a pair fails.
- Disabled items are not shown by colour alone: they are skipped by keyboard navigation and say
  "unavailable" to assistive technology.

### UD21 Testing. DECIDED

- **Logic tests**, any machine: registry completeness, keymap conflicts, dirty tracking, file
  write and recovery, preference round trips, layout arithmetic for UD12.
- **Simulated-input tests**, any machine including headless CI: `#[gpui::test]` with
  `VisualTestContext` (UG11). Keystrokes and clicks drive actions and dialogs;
  `simulate_new_path_selection` answers pickers; `debug_bounds` checks layout against UK
  constants; render counters on every view check UINV2. These are the `UTC` tests.
- **Golden images**, Windows development machine (Linux has no `render_to_image`, T0.8): each
  screen (workspace, Home, First boot, each dialog, an open menu) in both themes at 1.0, 1.25 and
  1.5, compared with committed PNGs using T3.2's rule (at most 0.1% of pixels differ by more
  than 2/255). U1.9 builds this harness so T3.2 can extend it (section 14). Window tests skip
  themselves when `OMNIA_HEADLESS_CI` is set, as `crates/omnia-app/tests/fonts.rs` does today.
- **Visual sign-off** against the mockups at human checkpoints UH2 and UH3: a side-by-side
  image and a list of every visible difference, as T3.3 does for the sheet.
- **Budgets** with `bench ui`, `bench start --shell`, `bench idle --shell` on Orion (section 2).

## 7. Invariants

Each has a test; the task that adds it is in brackets.

| ID | Rule |
| --- | --- |
| UINV1 | `omnia-app` contains no colour literal: no hex colour and no call to `rgb`, `rgba` or `hsla` in `crates/omnia-app/src` (INV6 extended to the shell). [U1.1] |
| UINV2 | A shell-only change does not notify `SheetView`: menu open, hover and close; toolbar and tab-strip hover; `CommandState` flips; status message, hint and readout updates; toasts; instrument frame open, drag, raise and close. A dock resize paints the sheet at most once per frame. [U1.7, U4.1] |
| UINV3 | Every registry action appears in a menu, and every action reachable by mouse has a menu path or a binding. [U1.4] |
| UINV4 | No two bindings share a keystroke in key contexts that can be active together. A letter, digit or Space without a modifier is bound only in `Sheet`, `Menu`, `Dialog` and `TextField`. [U1.4] |
| UINV5 | Invoking a disabled action changes nothing: document revision, files on disk, documents open and `CommandState` are unchanged. Menus and toolbar show exactly `enabled(state)`. [U1.4, U2.5] |
| UINV6 | A document is shown modified exactly when its history cursor differs from its saved cursor; the tab dot, title and `CommandState` agree in the same frame. [U2.1, U2.2] |
| UINV7 | No path loses work silently: closing or quitting with a modified document always passes through the unsaved-changes dialog; Cancel changes nothing; a failed save neither marks the document saved nor alters the existing file. [U2.3, U2.4] |
| UINV8 | Switching tabs, any number of times, leaves each document's camera, selection, tool, undo and redo depth and history cursor unchanged. [U2.2] |
| UINV9 | No repeating timer or animation-frame request exists while nothing changes. The autosave timer exists only while a document has unsaved, un-autosaved changes; the toast timer only while a toast is shown. [U2.7, U1.10] |
| UINV10 | Shell files are deterministic: `prefs.ron`, `recent.ron` and `window.ron` round-trip byte for byte, and the same state writes the same bytes. [U2.6] |
| UINV11 | Contrast in both themes: each text-on-background pair the shell uses is ≥ 4.5:1 and each meaningful icon or indicator is ≥ 3:1, computed from `omnia-theme` tokens over the list of pairs the shell declares. [U3.6] |
| UINV12 | Focus order is defined and visible: Tab and Shift+Tab cycle menu bar, toolbar, sheet, instrument frames, strip, tabs, toggles, status bar; Esc closes the topmost layer (menu, popover, dialog, instrument focus) and returns focus to the element focused before it. [U4.5] |
| UINV13 | While a dialog is open, no action outside its context runs and no mouse event reaches what is beneath. [U2.4] |
| UINV14 | The shell edits documents only through `SheetCanvas` methods, or `Doc::undo` and `Doc::redo` before U5.1: `omnia-app` outside its tests never calls `Doc::apply` (INV4 holds unchanged). [U2.5] |

## 8. Scenarios

Each scenario is a test with this expected result. "Headless" means a simulated-input test that
runs in CI; "golden" means a Windows golden; "bench" means a reference-machine run.

| ID | Steps | Expected |
| --- | --- | --- |
| UTC-new | Ctrl+N twice | Tabs "Untitled 1", "Untitled 2", second active, neither modified; title "Untitled 2 — Omnia". Headless. |
| UTC-open | Ctrl+O, pick `a.omnia` (100 parts) | A loading tab appears in the same frame; then the sheet shows, status counts read "100 parts · …", `a.omnia` heads Recent. Headless. |
| UTC-open-twice | Open `a.omnia`, open it again | One tab; it becomes active. Headless. |
| UTC-open-bad | Open a file with `version:3`; then a truncated file; then a missing recent entry | Each: no tab remains; an error dialog names the file and the reason; for the missing entry the dialog offers "Remove from Recent". Headless. |
| UTC-open-drop | Drop two `.omnia` files and one `.txt` on the window | Two tabs open; one dialog lists the `.txt` as not an Omnia file. Headless. |
| UTC-save-untitled | Edit an untitled document, Ctrl+S, pick `b` | File `b.omnia` written; tab "b", not modified; status "Saved b.omnia". Headless. |
| UTC-save-cancel | Edit an untitled document, Ctrl+S, cancel the picker | Nothing written; still modified; no dialog. Headless. |
| UTC-save-fail | Save to a read-only directory, and over a read-only existing file | Error dialog with Retry, Save As, Cancel; still modified; the existing file's bytes unchanged; no temp file left. Headless. |
| UTC-saved-point | Open, apply A, save, apply B, undo | Not modified. Undo again: modified. Redo: not modified. Headless (after U2.1). |
| UTC-branch | Open, apply A, save, undo, apply B, undo | Modified (the save point's branch was discarded). Headless (after U2.1). |
| UTC-close-dirty | Modify, Ctrl+W, then Cancel / Don't Save / Save | Cancel: tab stays, modified. Don't Save: tab closes, file unchanged. Save: file written, tab closes. Headless. |
| UTC-quit-dirty | Two modified tabs (one untitled), Ctrl+Q, Save All, cancel the picker | Quit stops; both tabs open; the titled one saved, the untitled still modified. Headless. |
| UTC-close-window | One modified tab, window close request | `on_window_should_close` returns false; the dialog shows; Don't Save closes the window and quits. Headless. |
| UTC-recovery | Modify, wait for autosave (timer driven), drop the app without quitting, relaunch | Home shows a Recovered card; Restore opens it modified with the edit present and bound to the original path; Discard deletes both recovery files. Headless with the test clock. |
| UTC-recent | Open 25 different files | `recent.ron` holds 20, newest first; reopening the third restores its camera. Headless. |
| UTC-tabs | Two documents: pan, zoom and select in A; switch to B and back ten times | A's camera, selection, undo depth and cursor equal their values before. Headless; UB6 bench. |
| UTC-menu-keys | F10, Right ×2, Down, Enter on View > Zoom In | View opened by keyboard; Zoom In ran once; GPUI focus stays on the sheet throughout; `SheetView` paints once (the zoom) and `MenuBar` renders for each move. Headless. |
| UTC-menu-mouse | Hover across all nine menu labels with a menu open, then Esc | Each opens in the next frame; sheet paint count 0. Headless; UB2/UB3 bench. |
| UTC-disabled | With no selection, invoke Delete, Cut, Copy by key, menu and toolbar | Nothing changes (UINV5); items shown disabled. Headless. |
| UTC-type-r | Open the threshold field in Preferences, type "R" and "Ctrl+Z" | The field receives the text; no rotate, no document undo. Headless. |
| UTC-dock | Ctrl+1, Ctrl+2, Ctrl+3, drag the bottom splitter 100 px over 20 frames | Each toggle drawn next frame; the drag paints the sheet ≤ once per frame; sheet never below 320x240. Headless; bench. |
| UTC-small | Resize to 960x600 at 1.0 and 1.5 | Toolbar collapses in UD12's order; tabs scroll; nothing overlaps (`debug_bounds`). Headless; golden. |
| UTC-theme | Switch Graphite → Gray → Match system with the system dark | Every view re-renders once per switch; each open sheet repaints once; the golden equals a fresh start in that theme. Headless; golden; UB7 bench. |
| UTC-scale | Shell at 1.25, change to 1.5 | Image equals a fresh window at 1.5. Golden (with T7.4 for the sheet). |
| UTC-instrument | Open two instruments, drag one 200 px over 30 frames, click the other, Esc | Clicked frame on top and the key target; Esc makes the sheet the key target; GPUI focus stays on the sheet; sheet paint count 0 throughout. Headless; bench. |
| UTC-toast | Make `prefs.ron` unreadable, launch | Defaults in use; one toast; it expires after 8 s; afterwards no timer exists. Headless. |
| UTC-first-boot | No `prefs.ron`, launch, press Enter | First boot shows; Enter writes `prefs.ron` with defaults and opens Home. Headless. |
| UTC-wiring-prefs | Set follow threshold to 3 in Preferences, restart | The value is in `prefs.ron`; each canvas reports 3; a threshold of 0 in the file is rejected with a toast and the default used. Headless. |
| UTC-idle | Shell, two documents (one modified), fixed readout | 0 frames and 0 renders over 10 s. Bench (UB4). |

## 9. Spikes

Each is small, runs before the work it de-risks, and writes `docs/measurements/U0.x.md`. Spikes
on Orion follow section 2.1 when they time something.

| ID | Question | Method | Decides |
| --- | --- | --- | --- |
| US1 | Client-drawn chrome. Windows: do drag, double-click, snap layouts and the three buttons work through `window_control_area`? Orion (KDE, Wayland): is `Client` granted; do `start_window_move` and `start_window_resize` work; what inset and shadow are needed? | A bare window with the 36 px bar and resize edges on both machines; the owner tries each gesture once. | UD1's Linux half; if `Client` is refused, the fallback in UD1. |
| US2 | Does a `deferred` popover inside a `.cached()` view replay and occlude correctly, and does opening it leave the sheet's paint count at 0? | `MenuBar` prototype over the `empty` sheet with paint counters; hover across labels for 300 frames. | Where popovers live (UD2). |
| US3 | What does our code allocate when a cached view with one custom child element re-renders, and does `Readout` paint without allocation once warm? | Counting allocator scope around render and paint of the status bar during the `pan` script. | UD18's design; UB5 feasibility. |
| US4 | `Doc::save` into memory and `Doc::load` at `1k` and `10k` on Orion; size of the file. | Criterion bench in `omnia-bench` over T2.4's generator. | Whether UB10 holds with UI-thread serialisation, or `omnia-doc` needs a snapshot (D8's `DocSnapshot`). |
| US5 | Does `render_to_image` capture deferred layers and the client title bar on Windows? | Golden of a window with an open menu. | Whether shell goldens cover menus and dialogs or need a separate in-tree render. |

## 10. Crates and APIs

All shell code is in `crates/omnia-app`. No new crate: the shell has one consumer, and the canvas
must not depend on it.

```
crates/omnia-app/src/
  main.rs              args: [--frames N] [--home] [paths…]                          (U2.3)
  lib.rs               open_app, fonts, assets, globals                              (U1.1)
  theme.rs             ActiveTheme, ThemeChoice, color(), observe system appearance  (U1.1, U3.2)
  assets.rs            AssetSource over embedded Tabler icons                        (U1.2)
  actions.rs           actions!, ActionSpec, ACTIONS, CommandState                   (U1.4)
  keymap.rs            default bindings (shell + canvas), conflict test              (U1.4)
  shell.rs             Shell root, Screen, ModalLayer                                (U1.7)
  workspace.rs         Workspace, layout, docks, action handlers                     (U1.7, U2.2)
  document.rs          Document, DocumentEvent, dirty tracking                       (U2.2)
  files.rs             open/save tasks, atomic write, FileError                      (U2.3)
  persist.rs           directories, Prefs, RecentFiles, WindowState                  (U2.6)
  recovery.rs          autosave timer, recovery files                                (U2.7)
  chrome/title_bar.rs  TitleBar, window controls                                     (U1.5)
  chrome/menu_bar.rs   MenuBar, menu popover, keyboard navigation                    (U1.5)
  chrome/toolbar.rs    Toolbar, overflow                                             (U1.6, U4.3)
  chrome/tab_strip.rs  TabStrip, panel toggles                                       (U1.7)
  status_bar.rs        StatusBar (T1.5's file, extended), Readouts                   (U1.8)
  readout.rs           Readout element                                               (U1.8)
  dialogs/             unsaved.rs, error.rs, prefs.rs, shortcuts.rs, about.rs        (U2.4, U3.3)
  toast.rs             ToastLayer                                                    (U2.4)
  instruments/         strip.rs, layer.rs (InstrumentLayer, FloatingFrame)           (U1.7, U4.1)
  panels/              dock.rs, toolbox.rs, spreadsheet.rs, results.rs (stubs)       (U1.7)
  parts/picker.rs      place-strip popovers, "In use" picker                         (U4.2)
  home/                mod.rs, sidebar.rs, cards.rs, preview.rs                      (U3.4, U4.6)
  first_boot.rs        FirstBootScreen                                               (U3.5)
  widgets/             button.rs, icon.rs, popover.rs, splitter.rs, tooltip.rs,
                       text_field.rs                                                 (U1.3, U3.1)
crates/omnia-app/tests/  shell_*.rs: UTC tests (headless), golden_shell.rs (Windows)
crates/omnia-bench/src/ui.rs   `bench ui`, `--shell` for start, idle and run          (U1.10)
xtask/src/golden.rs, tests/golden/shell/   golden harness and shell PNGs              (U1.9)
```

Signatures are a sketch: names and shapes to build toward, not a frozen interface.

```rust
// ---- theme.rs ----
pub enum ThemeChoice { Graphite, Gray, System }
pub struct ActiveTheme { pub theme: &'static omnia_theme::Theme, pub choice: ThemeChoice }
impl gpui::Global for ActiveTheme {}
pub fn color(c: omnia_theme::Rgba) -> gpui::Hsla;          // the only token-to-GPUI conversion
pub fn theme(cx: &gpui::App) -> &'static omnia_theme::Theme;

// ---- actions.rs ----
gpui::actions!(omnia, [NewDocument, Open, Save, SaveAs, CloseTab, Quit, ShowHome, Preferences,
    Undo, Redo, Cut, Copy, Paste, Delete, SelectAll, ZoomIn, ZoomOut, ZoomArea, ZoomFit,
    ToggleFullScreen, ToggleToolbox, ToggleSpreadsheet, ToggleResults, NextTab, PreviousTab,
    RunSimulation, PauseSimulation, StopSimulation, ShowShortcuts, About, FocusMenuBar]);
#[derive(Clone, PartialEq, gpui::Action)] pub struct OpenRecent { pub index: usize }
#[derive(Clone, PartialEq, gpui::Action)] pub struct SetTheme { pub choice: ThemeChoice }
#[derive(Clone, PartialEq, gpui::Action)] pub struct ToggleInstrument { pub kind: InstrumentKind }
#[derive(Clone, PartialEq, gpui::Action)] pub struct PlacePart { pub symbol: omnia_symbols::SymbolId }

pub enum KeyContext { Shell, Workspace, Sheet, Panel, Instrument, Menu, Dialog, Home, FirstBoot, TextField }
pub struct Binding { pub keys: &'static str, pub context: KeyContext }
pub enum MenuPlace { None, At { menu: TopMenu, group: u8, order: u8 } }
pub struct ActionSpec {
    pub id: &'static str,                                   // stable, e.g. "file.save"
    pub label: &'static str,
    pub icon: Option<&'static str>,                         // Tabler name
    pub menu: MenuPlace,
    pub bindings: &'static [Binding],
    pub build: fn() -> Box<dyn gpui::Action>,
    pub enabled: fn(&CommandState) -> bool,
    pub checked: fn(&CommandState) -> bool,
}
pub static ACTIONS: &[ActionSpec];

#[derive(Clone, Copy, PartialEq, Default)]
pub struct CommandState {
    pub screen: ScreenKind, pub has_document: bool, pub loading: bool,
    pub modified: bool, pub untitled: bool, pub can_undo: bool, pub can_redo: bool,
    pub has_selection: bool, pub has_clip: bool, pub canvas_edits: CanvasEdits,
    pub toolbox: bool, pub spreadsheet: bool, pub results: bool, pub full_screen: bool,
    pub sim: SimState, pub theme: ThemeChoice,
}
pub struct CanvasEdits { pub undo: bool, pub delete: bool, pub clipboard: bool, pub place: bool }
                                                    // which canvas methods exist yet (U5.x flips)
pub enum SimState { Unavailable, Idle, Running, Paused, Failed }

// ---- keymap.rs ----
pub fn default_bindings() -> Vec<gpui::KeyBinding>;        // shell + omnia_canvas::keymap::bindings()
pub fn conflicts(bindings: &[gpui::KeyBinding]) -> Vec<(String, String, String)>; // UINV4

// ---- document.rs ----
pub struct Document { /* doc: Entity<Doc>, canvas: Entity<SheetCanvas>, path: Option<PathBuf>,
                         name: SharedString, saved: HistoryMark, state: DocState, recovery: Option<RecoveryId> */ }
pub enum DocState { Loading, Ready, Saving, Failed }
pub enum HistoryMark { Cursor(u64), Revision(u64) }        // Revision until U2.1 (UD6)
pub enum DocumentEvent { Modified(bool), Renamed, Saved, Loaded, Failed(FileError) }
impl Document {
    pub fn untitled(n: u32, theme: &'static Theme, window: &mut Window, cx: &mut Context<Self>) -> Self;
    pub fn open(path: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self;  // background load
    pub fn is_modified(&self, cx: &App) -> bool;
    pub fn save(&mut self, cx: &mut Context<Self>) -> Task<Result<(), FileError>>;
    pub fn save_as(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Task<Result<(), FileError>>;
    pub fn canvas(&self) -> &Entity<omnia_canvas::SheetCanvas>;
    pub fn doc(&self) -> &Entity<omnia_doc::Doc>;
}
impl EventEmitter<DocumentEvent> for Document {}

// ---- files.rs ----
pub enum FileError { Read(io::Error), Invalid(String), NotOmnia, Write(io::Error),
                     PickerUnavailable(String), AlreadyOpen(PathBuf) }
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()>;   // temp in same dir, flush, rename
pub fn ensure_extension(path: PathBuf) -> PathBuf;                  // ".omnia"

// ---- persist.rs ----
pub struct Dirs { pub config: PathBuf, pub state: PathBuf }
impl Dirs { pub fn from_env() -> Option<Self>; }                     // UD10
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Prefs { pub theme: ThemeChoice, pub symbols: SymbolStyle, pub autosave: bool,
                   pub wiring: WiringPrefsFile }
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct WiringPrefsFile { pub follow_on_move: bool, pub follow_pin_threshold: u16,
                             pub select_mode: SelectModeFile, pub delete_wires_with_part: bool }
impl WiringPrefsFile { pub fn to_prefs(&self) -> Result<omnia_doc::WiringPrefs, omnia_doc::PrefsError>; }
pub struct RecentFiles { /* Vec<RecentEntry>, cap UK13 */ }
pub struct RecentEntry { pub path: PathBuf, pub opened: SystemTime, pub center: (i32, i32),
                         pub zoom: f64, pub parts: u32 }

// ---- workspace.rs ----
pub struct Workspace { /* documents: Vec<Entity<Document>>, active: Option<usize>, chrome entities,
                          docks: DockLayout, command_state: Entity<CommandState> */ }
impl Workspace {
    pub fn open_paths(&mut self, paths: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>);
    pub fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>);
    pub fn close(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> Task<bool>;
    pub fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<bool>;
}

// ---- instruments/layer.rs ----
pub enum InstrumentKind { Multimeter, FunctionGenerator, Wattmeter, Oscilloscope, FourChannelScope,
    BodePlotter, FrequencyCounter, WordGenerator, LogicAnalyzer, LogicConverter, IvAnalyzer,
    DistortionAnalyzer, SpectrumAnalyzer, NetworkAnalyzer, Probe }
pub struct InstrumentLayer { /* frames: Vec<Frame> in z-order */ }
pub struct Frame { pub kind: InstrumentKind, pub rect: Bounds<Pixels>, pub content: AnyView }
impl InstrumentLayer {
    pub fn open(&mut self, kind: InstrumentKind, content: AnyView, cx: &mut Context<Self>);
    pub fn raise(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>);
    pub fn clamp_to(&mut self, area: Bounds<Pixels>, cx: &mut Context<Self>);
}

// ---- readout.rs ----
pub struct ReadoutCache { /* one ShapedLine per character in "0123456789.,-%: —·" per size */ }
pub struct Readout { /* text: ArrayString-like fixed buffer, cache: Rc<ReadoutCache>, color: Hsla */ }
impl gpui::Element for Readout { /* layout: char count × advance; paint: cached glyph lines */ }
```

Additions needed outside `omnia-app` (section 14 lists the owning tasks):

```rust
// omnia-doc
impl Doc {
    pub fn history_cursor(&self) -> u64;      // UD6: current state's id; apply → new revision; undo/redo move
    pub fn can_undo(&self) -> bool;
    pub fn can_redo(&self) -> bool;
    pub fn part_count(&self) -> usize;
    pub fn net_count(&self) -> usize;         // Connectivity::net_count
}
// omnia-canvas
pub enum CanvasEvent { Status(CanvasStatus), SelectionChanged,
                       History { can_undo: bool, can_redo: bool, cursor: u64 } } // History: new
impl SheetCanvas {
    pub fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>);            // T5.2
    pub fn redo(&mut self, window: &mut Window, cx: &mut Context<Self>);            // T5.2
    pub fn delete_selection(&mut self, window: &mut Window, cx: &mut Context<Self>); // T5.3
    pub fn select_all(&mut self, cx: &mut Context<Self>);                           // T4.3
    pub fn cut(&mut self, cx: &mut Context<Self>);                                  // T7.1
    pub fn copy(&self, cx: &mut Context<Self>);                                     // T7.1
    pub fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>);           // T7.1
    pub fn zoom_step(&mut self, steps: i32, window: &mut Window, cx: &mut Context<Self>); // T4.5: K4 steps about the viewport centre
    pub fn set_wiring_prefs(&mut self, prefs: WiringPrefs, cx: &mut Context<Self>); // WD12
    pub fn focus_handle(&self) -> &FocusHandle;                                     // UINV12
}
pub enum Tool { Select, Wire, Place(SymbolId), Label, ZoomArea }                    // ZoomArea: T4.5
```

## 11. Dependencies

No new crate. Everything the shell needs is GPUI plus crates already in canvas plan section 10.

| Crate or asset | Use | Licence |
| --- | --- | --- |
| `gpui`, `gpui_platform` (pinned) | Everything drawn, actions, pickers, tests | Apache-2.0 |
| `serde`, `ron` (already in the workspace) | `prefs.ron`, `recent.ron`, `window.ron`, recovery metadata | MIT or Apache-2.0 |
| `omnia-doc`, `omnia-canvas`, `omnia-theme`, `omnia-symbols` | Documents, sheet, tokens, part list | This repository |
| Tabler Icons 3.19.0, outline (the 13 icons in UD17 added to `vendor/tabler-icons/outline/`) | Icons | MIT |
| Inter, Source Code Pro (already vendored) | UI and readouts | SIL OFL 1.1 |
| Code adapted from GPUI's `examples/input.rs` | `TextField` (UD16) | Apache-2.0; attribution added to `THIRD_PARTY.md` |

Directory lookup reads environment variables (UD10) instead of adding a directory crate. Temp
names and recovery ids use the process id, time and a counter instead of a UUID crate.

## 12. Risks

| Risk | Mitigation | Early test |
| --- | --- | --- |
| KDE on Wayland refuses client decorations, or client resize edges feel wrong | UD1 fallback: hide our buttons and keep the menu row under the system title bar | US1 |
| A popover inside a cached view does not replay or occlude correctly | Move popovers into `ModalLayer`'s deferred slot | US2 |
| Shell renders allocate during pan because readouts change each frame | `Readout` element; render-free fallback through a shared cell | US3 |
| Serialising a `10k` document on the UI thread exceeds UB10 | Background serialisation from a `DocSnapshot` (D8) added by the doc lane | US4 |
| `render_to_image` misses deferred layers or chrome | Golden only static screens; check menus by `debug_bounds` and render counters | US5 |
| Two `10k` sheets exceed memory (UB9) | Measure in U5.6; if over, release an inactive canvas's caches on switch away (camera, selection kept) | UB9 run |
| A tab switch to a `10k` sheet misses 50 ms because the incoming sheet paints cold | Keep each canvas's atlas entries warm (shared atlas); if still over, present the sheet at its previous LOD first | UB6 run in U5.6 |
| Theme tokens fail contrast (measured: `text/muted`, Gray `text/secondary` on two backgrounds) | UQ1 default substitutions; owner adjusts tokens in `omnia.figcraft` | U3.6 contrast test |
| Shortcut conflicts appear as canvas and stage 3 add keys | UINV4 test over the whole keymap, run in CI | U1.4 |
| The file-chooser portal is missing on a Linux desktop | `FileError::PickerUnavailable` dialog naming the missing service; files can still be opened by drop or command line | US1 machine check |
| Users lose work to a crash between saves | UD9 recovery copies every 60 s of unsaved work | UTC-recovery |
| GPUI churn changes action, focus or overlay APIs | Facts UG1 to UG17 re-checked at each upgrade (canvas plan section 10, step 2) | Upgrade routine |
| Shell work starts before canvas editing exists, then has to change | Shell calls only the canvas methods in section 10; `CanvasEdits` flags enable menu items as each canvas task lands (U5.x) | U2.5 tests with flags off and on |

## 13. Open questions

Each has a default. Build to the default; the owner's dated answer replaces it here.

UH0 was signed on 2026-11-23. The owner delegated the sign-off to the coordinating agent, which
kept every default and approved section 14 as written.

| ID | Question | Default until answered | Answer |
| --- | --- | --- | --- |
| UQ1 | `text/muted` (2.32 to 3.88:1) and Gray `text/secondary` on backdrop (4.25) and inset (4.43) are below 4.5:1. Change the tokens, or change where they are used? | The shell uses the next stronger token wherever a pair it draws would fail (muted → secondary, secondary → primary); UH2 lists every substitution next to the mockup. | Default kept (UH0, 2026-11-23). |
| UQ2 | Command palette? | No. Menus, F10 and Alt+letter cover keyboard reach. | Default kept (UH0, 2026-11-23). |
| UQ3 | Standalone files or a project container (ROADMAP open decision)? | Standalone `.omnia` files, one sheet each. Home's "Projects" are folders. | Default kept (UH0, 2026-11-23). |
| UQ4 | Tab label: file name or a design title stored in the file? | File name without extension. | Default kept (UH0, 2026-11-23). |
| UQ5 | Autosave recovery copies on by default, every 60 s? | Yes. | Default kept (UH0, 2026-11-23). |
| UQ6 | Shell colours not in the `Theme` collection: logo tile, mark, oscilloscope screen, "running" pill, focus ring. | Add to the overlay token file, same value in both themes: `brand/tile` `#F2F2F2`, `brand/mark` `#111112`, `instrument/screen` `#0E0F11`, `focus/ring` = `stage/select`; the pill uses `stage/tag-bg`, `stage/tag-text`, `accent/mint`. A token that appears in `omnia.figcraft` wins. | Default kept (UH0, 2026-11-23). |
| UQ7 | Home sidebar items for features that do not exist (Templates, Part libraries, Simulation results, account footer)? | Shown disabled with "Not available yet"; the footer omitted. | Default kept (UH0, 2026-11-23). |
| UQ8 | Keep T1.5's cursor readout, which the mockup's status bar does not show? | Keep it, before grid. | Default kept (UH0, 2026-11-23). |
| UQ9 | First boot sections 3 and 4 (libraries, simulator) before those features exist? | Reserved rows listing only what this build has. | Default kept (UH0, 2026-11-23). |
| UQ10 | Guard against two instances editing the same file? | No lock; last save wins. | Default kept (UH0, 2026-11-23). |
| UQ11 | The mockup shows command-key glyphs; the targets are Windows and Linux. | Show "Ctrl+N" style text. | Default kept (UH0, 2026-11-23). |
| UQ12 | IEC symbol preference before an IEC symbol set is compiled? | Stored only; the sheet keeps the built-in set. | Default kept (UH0, 2026-11-23). |
| UQ13 | Print (toolbar, File)? | Shown disabled; no print plan exists. | Default kept (UH0, 2026-11-23). |
| UQ14 | At the pinned revision `Window::focus` calls `refresh()`, which re-renders every cached view, and a mouse-down focuses a focusable element (UG15). Chrome clicks and popovers that take focus would therefore repaint `SheetView`, against UINV2, UB3, UTC-menu-keys and UTC-instrument. Accept the repaint and reword those, or keep pointer input in the chrome from moving focus? | None (raised by U4.2 after UH0's draft). | 2026-11-23 (UH0): keep UINV2, UB3 and their paint counts. Neither a pointer action in the chrome nor F10/Alt+letter calls `Window::focus`; mouse-down focus is suppressed on chrome controls (UG15). The workspace root holds a key target (`Sheet`, `Menu`, `Instrument(id)`); menus, popovers and instrument frames become the key target, not the focused element, and receive arrows, Enter, Esc and Tab through `Nav*` actions bound once in `Shell` and routed by the root's `KeyTarget` router (Esc is no longer a `Sheet` binding; T5.2 provides `SheetCanvas::cancel` instead) (UD2, UD3, UD5; UTC-menu-keys and UTC-instrument reworded to say GPUI focus stays on the sheet, paint counts unchanged). Chrome controls activate from their own mouse-down and mouse-up handlers, never `Div::on_click` (UG16). A focused text field yields navigation keys while an overlay is the key target. GPUI focus moves only for keyboard traversal (Tab, F6, UINV12), for dialogs and text fields, which need the input handler, and when a command starts a canvas tool while focus is outside the sheet (U5.2's `PlacePart`: the tool needs `Sheet`'s bindings; at most one sheet repaint). None of these is a shell-only change under UINV2. If U1.5 finds this cannot be built at the pinned revision, it is set to `blocked (UQ14)` and returns to the owner; the invariants are not reworded to fit. |
| UQ15 | GPUI refreshes the whole window when input switches between mouse and keyboard (UG17), so a click then Esc (UTC-instrument), mouse input then F10 (UTC-menu-keys), or Tab then a pointer move repaints `SheetView` once, whatever the chrome does. Accept one sheet repaint per input-modality switch and say so in UINV2, UB3, UTC-menu-keys and UTC-instrument, or patch the pinned GPUI so a modality change refreshes only views that style on it? | Neither: UINV2, UB3 and the UTC rows stay as written. A task whose acceptance check fails only because of the UG17 refresh is set to `blocked (UQ15)`, records the counts with and without the modality-switch frame, and returns to the owner. | Open: for the owner (raised 2026-11-23 by the UH0 review; the delegated sign-off does not reword invariants). |

## 14. Changes to the canvas and wiring plans

Applied by roadmap task U0.2 after the owner reads this plan (UH0). Until then the canvas and
wiring files stay authoritative for their ids.

| Where | Change |
| --- | --- |
| canvas-plan.md section 1 | Out of scope "the libraries UI" stays; add "the application shell: see ui-plan.md". |
| canvas-plan.md section 9, crate list | `omnia-app` line: "window shell; see ui-plan.md section 10". |
| canvas-plan.md section 9, `Doc` | Add `history_cursor`, `can_undo`, `can_redo`, `part_count`, `net_count` (UD6, UD18). |
| canvas-plan.md section 9, `SheetCanvas` | Add `undo`, `redo`, `delete_selection`, `select_all`, `cut`, `copy`, `paste`, `zoom_step`, `set_wiring_prefs`, `focus_handle`; `Tool::ZoomArea`; `CanvasEvent::History`. |
| canvas-plan.md INV6 | "and `omnia-app` (ui-plan.md UINV1)". |
| canvas-plan.md INV11 | "A hover, selection or drag-move change, or any shell-only change (ui-plan.md UINV2), does not notify `SheetView`." |
| canvas-plan.md Q2 | Note UQ6's shell overlay tokens in the same file. |
| canvas-roadmap.md Lanes | New lane `shell` owns `crates/omnia-app/` (ui-roadmap.md); `canvas` keeps `crates/omnia-canvas/`. |
| canvas-roadmap.md T3.2 | The harness (`xtask/src/golden.rs`, the comparison rule, `tests/golden/`) is created by ui-roadmap.md U1.9; T3.2 adds sheet fixtures and its own done-when to it. |
| canvas-roadmap.md T4.3 | Also provide `select_all` (Ctrl+A is bound by the shell, UD3). |
| canvas-roadmap.md T4.5 | Also provide `zoom_step` and `Tool::ZoomArea` (drag a rectangle, zoom to it with the same animation), used by View and the toolbar. |
| canvas-roadmap.md T5.1 | Follow-up for doc lane task U2.1: `history_cursor`, `can_undo`, `can_redo`. |
| canvas-roadmap.md T5.2 | Bind R, X, Y, W (and T1.5/T6.2's Space) in key context `Sheet`; provide `SheetCanvas::cancel`, which the shell's Esc (`NavCancel`, UD3, UQ14) calls. Ctrl+Z, Ctrl+Y and Del are bound by the shell (UD3), whose handlers call `SheetCanvas::undo`, `redo` and (T5.3) `delete_selection`; T5.2 provides `undo`, `redo` and `CanvasEvent::History`. The keymap conflict test (UINV4) covers T5.2's bindings. |
| canvas-roadmap.md T5.3 | Provide `delete_selection`. |
| canvas-roadmap.md T7.1 | Ctrl+C, Ctrl+X, Ctrl+V are bound by the shell; T7.1 provides `cut`, `copy`, `paste`. |
| canvas-roadmap.md T7.3 | The floating frame, drag, stacking and focus come from ui-roadmap.md U4.1. T7.3 supplies the animated stub content inside that frame, keeps `bench run --script pan --instrument` and its done-when; `instrument_host.rs` holds the content only. |
| wiring-roadmap.md W2.3 | Also provide `SheetCanvas::set_wiring_prefs` (the move tool is the first consumer of `WiringPrefs`); ui-roadmap.md U5.4 calls it. |
| wiring-plan.md section 1 | Out of scope "a preferences window, persistence of preferences": add "provided by ui-plan.md UD14". |
| wiring-plan.md WD12 | Add: "Persistence and a settings surface are provided by ui-plan.md UD14. `WiringPrefs` is unchanged; the shell builds it through the checked constructor and applies it with `SheetCanvas::set_wiring_prefs`." |
| ROADMAP.md | Link ui-plan.md and ui-roadmap.md under stages 2 and 4; record UQ3 and UQ5 defaults against the open decisions on files and autosave. |
