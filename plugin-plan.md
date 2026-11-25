# Omnia plugin system: plan (reference)

This file is the reference for Omnia's plugin and workbench extension system: declarative content
bundles (Tier 0), sandboxed WebAssembly Component Model plugins (Tier 1), out-of-process sidecar
workbenches with shared GPU surfaces (Tier 2), namespaced `.omnia` extension data and reversible
`Command::PluginPatch` edits, host-rendered declarative UI and retained 2D viewports, capability
permissions, `.oplugin` packaging, and the authoring SDK. The work itself is in
[plugin-roadmap.md](plugin-roadmap.md), which lists tasks that cite the ids defined here.

It sits beside [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md), [ui-plan.md](ui-plan.md), [sim-plan.md](sim-plan.md),
[parts-plan.md](parts-plan.md) and [context-plan.md](context-plan.md), and re-plans nothing they
decide. Every unprefixed id (`B`, `K`, `G`, `D`, `INV`, `Q`, `T`) is defined in the canvas plan;
`W` in the wiring plan; `A` in the agent plan; `U` in the UI plan; `S` in the simulation plan; `P`
in the parts plan; `C` in the circuit-context plan; `F` in
[docs/file-format-proposal.md](docs/file-format-proposal.md). Cross-plan decisions `XD1`–`XD9`
live in `parts-plan.md` section 6. Section 14 lists the changes this plan makes to other files and
which task makes each one.

Written 2026-11-25 against `main` at `a022e6a`, recording the owner's architectural decisions from
the 2026-11-25 design session (`ED1`–`ED10`). No `omnia-plugin*` code exists yet.

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `EB` | Performance, latency, memory or containment budget, a hard requirement | 3 |
| `EK` | Constant: a limit, grammar or convention every plugin crate agrees on | 4 |
| `ED` | Architectural decision | 6 |
| `EINV` | Invariant: a rule that must hold in code, each with a test | 7 |
| `ETC` | Scenario with its expected result | 8 |
| `ES` | Spike: a measurement or prototype that settles an unknown before build | 9 |
| `EQ` | Open question for the owner, each with a default | 13 |
| `HE` | Human checkpoint | [plugin-roadmap.md](plugin-roadmap.md) |
| `E0.1`, `E1.1`… | Task (milestones are written `Milestone E0`–`Milestone E4`) | [plugin-roadmap.md](plugin-roadmap.md) |

Status words follow the repository conventions:

- **DECIDED**: settled by the owner (dated where decided in the 2026-11-25 design interview or at
  `HE0`).
- **PROPOSED**: written by the plan author. Becomes DECIDED, or is amended, at `HE0`. Build
  nothing from a PROPOSED entry before `HE0`.
- **HYPOTHESIS**: build only after the named `ES` spike confirms it.
- **DEFAULT**: open question with a default; build to the default until the owner's dated answer
  replaces it.

WebAssembly (Wasm), WIT, WASI, TOML, RON, ZIP, JSON-RPC, SPDX, SHA-256, Ed25519, Wayland, X11,
DMA-BUF, Vulkan, DXGI, Direct3D 12, Metal, `wasmtime`, `wit-bindgen`, `gpui`, `wgpu`, `ngspice`
and `RIS-C` are named here as open standards, OS/graphics interfaces, open-source crates or
first-party engines already referenced in the repository. `Reid-Engineered/Spectare` is named as
a first-party target workspace plugin. Operating systems appear only as `AGENTS.md` names them. No
other company or commercial product is named (AGENTS hard rule 1).

## 1. Scope

Omnia already has dedicated mechanisms for component definitions and SPICE models (`.opart`/`.olib`
in `parts-plan.md`) and for external agent/script automation (`omnia-agent` / MCP in `mcp-plan.md`).
The plugin system builds on top of them rather than replacing them.

In scope:

- **Tier 0 — Declarative content bundles.** Zero-code plugins that package `.olib` / `.opart` part
  libraries, `omnia-theme` colour themes, schematic templates, and keymap presets into one versioned
  `.oplugin` archive or folder.
- **Tier 1 — Sandboxed Wasm Component Model plugins (WIT via `wasmtime`).** Portable, cross-platform
  community plugins for:
  - Schematic tools, generators, transforms and parametric layout helpers.
  - Custom electrical/design rule checks (ERC / lint findings) alongside built-in `omnia-circuit`
    and `omnia-sim` checks.
  - Importers, exporters, netlist formatters and bill-of-materials (BOM) generators.
  - Declarative dock panels, floating frames, inspector sections, menu/toolbar/status actions, and
    retained 2D vector overlays or custom 2D workbench viewports.
- **Tier 2 — Out-of-process sidecar workbenches.** Crash-isolated companion executables speaking a
  versioned local IPC protocol plus a shared GPU surface / framebuffer presentation protocol, so
  heavy native or GPU workbenches (targeting `Reid-Engineered/Spectare` as an optional workspace
  plugin) can run inside Omnia's workspace tabs without coupling to Omnia's pinned Rust or GPUI
  revision.
- **Workbench / workspace shell host.** Mode/tab-hosted workbenches in `omnia-app` that own the
  central viewport and contribute their own toolbar, left/bottom docks, menus and status readouts
  while preserving Omnia's chrome, `omnia-theme` tokens (`INV6`) and document lifecycle.
- **Namespaced document persistence and reversible custom commands.** Opaque reverse-DNS extension
  blocks (`ext`) in `.omnia` (losslessly round-tripped when the plugin is absent), sibling/project
  sidecar files for large workbench payloads, reversible `Command::PluginPatch` inside `omnia-doc`,
  and an optional external undo-participant hook for Tier 2 sidecar files.
- **Security, permissions and resource isolation.** Manifest-declared capability permissions with
  explicit user consent on install/upgrade, off-UI-thread Wasm execution with memory/fuel/timeout
  caps, and explicit user trust + SHA-256 pinning (optional Ed25519 signatures) for Tier 2 sidecars.
- **Packaging, scopes and developer SDK.** `.oplugin` ZIP archives and unpacked dev folders with
  `plugin.toml`, user-global and workspace-local install scopes, local and Git URL install, dev
  hot-reload, official WIT package, ergonomic Rust SDK (`omnia-plugin-sdk`) and CLI scaffold/pack
  subcommands.

Out of scope in v1:

- **In-app central registry / marketplace UI.** Deferred to a later milestone; `plugin.toml` and
  settings reserve a clean registry index URL field (`EQ5`) without shipping marketplace hosting or
  discovery UI in v1.
- **In-process native dynamic libraries (`cdylib` / Rust ABI).** Rejected (`ED2`): GPUI is pinned
  to one Git commit (`Cargo.toml`) and Rust has no stable ABI; in-process native crashes would
  destroy unsaved schematics.
- **Embedded webviews / HTML+Canvas UI.** Rejected (`ED4`): breaks native GPUI performance,
  accessibility (`UG9`), theme consistency (`INV6`) and memory budgets (`UB1`, `UB9`).
- **Custom simulation solver backends (`SimEngine`) and custom vector symbol artwork.** Deferred
  until `omnia-sim` (`S2.3`) and custom-symbol rendering (`PQ3`) land; the WIT world and capability
  table reserve slots for them (`EQ7`).
- **First-party TypeScript/JavaScript or Python SDK crates.** Any language compiling to Wasm
  Component Model works through the published WIT package; official non-Rust starter templates
  follow after v1 stabilizes (`ED10`).

## 2. Repository facts this plan rests on

Checked against `main` at `a022e6a`:

- **Workspace and toolchain.** Rust `1.98.1` (edition `2024`, resolver `3`), GPUI pinned to
  rev `15d136bf8e72ae43b284183ec46940c009460f5c` in root `Cargo.toml`.
- **Existing crates.** `omnia-doc`, `omnia-circuit`, `omnia-symbols`, `omnia-theme`,
  `omnia-canvas`, `omnia-app`, `omnia-agent`, `omnia-cli`, `omnia-bench`, plus `xtask` and
  `tests/golden`.
- **Strict lane boundaries (`AGENTS.md`, `INV12`, `AINV8`, `PINV1`).** `omnia-doc`,
  `omnia-circuit`, `omnia-agent`, `omnia-cli` and `omnia-parts` have zero GPUI dependency.
  `omnia-theme` owns every colour (`INV6`, `UINV1`).
- **Document edit and file conventions.** Every schematic mutation goes through `Doc::apply`
  (`crates/omnia-doc/src/command.rs`), returning an atomic `Change` and self-contained inverse
  `UndoOp` (`INV1`, `INV4`). File-ownership order for `omnia-doc` (`XD7`) is `P2.1`–`P2.3` (RON v3
  + `EmbeddedPartDef`), then `C1.9` (lineage token), then `C1.1` (net delta in `Change`).
  `docs/file-format-proposal.md` (`FH0`) proposes a binary `.omnia` format with named sections.
- **Archive & hashing crates already approved.** `toml = "1"`, `zip = "9"`
  (`deflate-flate2-zlib-rs`), `sha2`, `serde`, and `ron` are already in `Cargo.lock` and pass
  `cargo deny check` (`deny.toml` allow-list: `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`,
  `ISC`, `Zlib`, `Unicode-3.0`, `MPL-2.0`).
- **GPUI rendering constraint (`G11`, `canvas-plan.md` section 4).** `paint_surface` in the pinned
  GPUI revision is compiled for macOS and iOS only. Linux (Wayland/X11) and Windows have no
  built-in external GPU surface paint primitive in GPUI, making spike `ES2` mandatory before
  Milestone E4.
- **GPUI chrome & frame isolation (`UG15`–`UG17`, `UB3`, `UB5`).** `Window::focus` and
  `Div::on_click` trigger full cached-view refreshes; chrome controls use explicit
  `on_mouse_down`/`on_mouse_up` handlers and keep `SheetView` paint count at 0 during chrome
  interaction. Plugin chrome and panel widgets must follow the exact same `omnia-app` widget
  primitives (`U1.3`) so they never regress `UB3` or `UB5`.
- **Gesture gate (`AINV13`, `mcp-plan.md`).** In app mode, document revision never changes while a
  canvas drag/wire gesture is active; plugin write requests obey the same gate (`ED11`).

## 3. Budgets

All budgets are **PROPOSED** until `HE0`, and certified only on the Orion reference laptop (Linux,
Wayland, wgpu/Vulkan, 1920x1080 at 60 Hz, 100% and 125%, ten interleaved repetitions, canvas plan
section 2.1). Every existing canvas (`B1`–`B10`), wiring (`WB*`), UI (`UB1`–`UB10`), agent
(`AB1`–`AB4`), parts (`PB1`–`PB5`), context (`CB*`) and simulation (`SB*`) budget continues to hold
with plugins enabled.

| ID | Budget | Measured by |
| --- | --- | --- |
| EB1 | Cold start overhead. With 0 plugins installed, adding `omnia-plugin` adds ≤ 5 ms to `B1`/`UB1` (still < 400 ms). With 10 Tier 0/Tier 1 plugins installed (dormant until activated), startup discovery and manifest validation add ≤ 25 ms off-path or ≤ 15 ms on the critical path, keeping `start --shell` < 400 ms. | `bench start --shell --plugins 0\|10` |
| EB2 | Archive and manifest scan. Opening, verifying and indexing one `.oplugin` archive (up to 16 MiB compressed, `EK3`) takes < 20 ms p99 off the UI thread; scanning 20 installed plugin manifests takes < 15 ms p99. | `cargo bench -p omnia-bench --bench plugin` |
| EB3 | Tier 1 Wasm invocation. Calling an already-instantiated Wasm tool or ERC pass on a `1k` sheet: host↔guest marshalling + execution round-trip < 15 ms p99 for a reference BOM/ERC plugin; `Command::PluginPatch` apply on `10k` with 64-byte ext patch < 1 ms p99 (`Doc::apply` stays within `WB*`/`PB5`). | `cargo bench -p omnia-bench --bench plugin_wasm` |
| EB4 | UI isolation and hot-path allocations. With a Tier 1 plugin panel open and a retained 2D overlay active on a `1k` sheet: `allocs_per_frame.ours == 0` during pan/zoom/hover (`UB5`, `INV7`), `B3` (< 16.7 ms p99) holds, and plugin UI hover/updates never repaint `SheetView` (`UB3`, `sheet_paints == 0`). | `bench ui --sheet 1k --script plugin-panel\|plugin-overlay` |
| EB5 | Retained 2D viewport. Panning and zooming a Tier 1 plugin 2D workbench viewport holding 10,000 retained draw primitives meets `B3` (< 16.7 ms p99, 0 missed refreshes) with 0 guest Wasm calls during camera-only pan/zoom. | `bench ui --script plugin-2d-pan\|plugin-2d-zoom` |
| EB6 | Tier 2 GPU surface presentation (**HYPOTHESIS**, refined by `ES2`). Presenting a 1280x800 60 Hz frame from a Tier 2 sidecar workbench adds ≤ 2.5 ms host compositing cost (p99), holds 60 fps (< 16.7 ms p99, 0 missed refreshes) on Orion, and keeps idle CPU at 0 frames drawn over 10 s when the sidecar submits no damage (`UB4`). | `bench ui --script plugin-sidecar-60hz\|plugin-sidecar-idle` |
| EB7 | Document persistence overhead. Saving and loading a `10k` document carrying 256 KiB of opaque `ext` metadata across 4 plugin namespaces adds ≤ 10% to RON v4 `save`/`load` time and keeps `UB10` (< 33.3 ms p99 UI-thread serialise) green. | `cargo bench -p omnia-bench --bench plugin_doc` |
| EB8 | Memory overhead. With `wasmtime` engine initialised and 5 Tier 1 Wasm plugins loaded (1 active workbench), total RSS stays within `B9` (< 300 MB for one `10k` doc) and `UB9` (< 450 MB for two `10k` docs); each dormant Wasm plugin consumes ≤ 4 MiB RSS. | `memory_mb` in `bench ui --sheet 10k --plugins 5` |
| EB9 | Runaway & crash containment. A Wasm plugin hitting an infinite loop, memory exhaustion or trap is terminated within `EK5` (default 250 ms for interactive hooks, 5 s for commands) with 0 UI-thread frames > 50 ms (`B5`), 0 leaked threads, and `Doc` unchanged (`EINV3`). A Tier 2 sidecar `SIGKILL`/crash is detected within 200 ms, leaves Omnia responsive, and preserves all open schematics untouched. | Fault-injection tests `ETC-wasm-trap`, `ETC-wasm-timeout`, `ETC-sidecar-workbench` |

## 4. Constants

All constants are **PROPOSED** until `HE0`.

| ID | Constant | Value |
| --- | --- | --- |
| EK1 | Plugin manifest schema version | `schema = 1` in `plugin.toml`. Unknown versions are refused with a clear diagnostic. |
| EK2 | Canonical `plugin_id` grammar | Reverse-DNS ASCII lowercase: `^[a-z][a-z0-9-]{0,31}(\.[a-z][a-z0-9-]{0,31}){1,5}$`, total length 3..=128 chars (for example `org.reid-engineered.spectare`, `dev.omnia.bom-csv`). Reserved prefixes `dev.omnia.*` and `org.omnia.*` are disallowed for non-bundled plugins unless explicitly overridden in dev mode. |
| EK3 | `.oplugin` archive safety limits | Standard ZIP (Deflate or Stored only, mirroring `PK3`). Tier 0 / Tier 1 max compressed size: 16 MiB; max uncompressed total: 64 MiB; max entry count: 1,024; max single entry uncompressed: 32 MiB; max compression ratio: 100:1; max path depth: 8; relative UTF-8 paths only (no `..`, no leading `/`, no symlinks, no backslashes). Tier 2 sidecar archives allow up to 128 MiB compressed / 512 MiB uncompressed for native binaries, verified only after explicit user trust (`ED7`). |
| EK4 | Capability vocabulary | Finite enum declared in `[permissions]` of `plugin.toml`: `bundle.parts`, `bundle.themes`, `bundle.templates`, `bundle.keymaps`, `doc.read`, `doc.write`, `doc.ext.write` (scoped automatically to the plugin's own `plugin_id` namespace), `circuit.read`, `erc.contribute`, `sim.read`, `ui.actions`, `ui.panel`, `ui.overlay`, `ui.workbench`, `fs.workspace.read`, `fs.workspace.write`, `fs.export` (host file-picker only), `net.hosts` (explicit allow-list of hostnames, HTTPS only), `sidecar.exec` (Tier 2 only), `sidecar.gpu_surface` (Tier 2 only). Undeclared capabilities are always denied. |
| EK5 | Tier 1 Wasm sandbox resource caps | Per-instance linear memory cap: 64 MiB default (manifest may request up to 256 MiB with user consent). Stack cap: 1 MiB. Table elements cap: 10,000. CPU interruption via `wasmtime` epoch tick (10 ms granularity): interactive UI/overlay/ERC hook deadline 250 ms; user-invoked command/export deadline 5,000 ms (cancellable via progress toast). Max concurrent worker calls per plugin: 1 (serialised per plugin actor, `ED2`). |
| EK6 | `.omnia` `ext` metadata limits | Extension map keyed by validated `EK2` `plugin_id` (plus optional per-item sub-map keyed by `ItemId`). Max distinct plugin namespaces per document: 32. Max total serialised `ext` bytes per document: 1 MiB (larger workbench artifacts must use sibling/project sidecar files, `ED5`). Per-entry schema version `u32` + canonical payload (RON/UTF-8 JSON or opaque bytes, max 256 KiB per namespace). |
| EK7 | Document RON & binary section versions | RON `version: 4` adds top-level `ext` (document-level and item-level namespaced blocks) after `P2.3`'s `version: 3`. `Doc::load` reads v2, v3 and v4; `Doc::save` writes v4 only when `ext` is non-empty (or always v4 once `E2.1` lands, preserving byte-identical v2/v3 compatibility fixtures; see `EQ3`). Binary `.omnia` (`docs/file-format-proposal.md`) reserves optional section kind `EXT_DATA` (not marked must-understand, so readers without plugins skip or preserve it safely). |
| EK8 | Declarative UI widget tree caps | Max nodes per panel/workbench chrome render: 4,096. Max nesting depth: 32. Max text string length per node: 8 KiB. Colours are referenced strictly by `omnia-theme` semantic token enum (zero raw RGB/hex literals from plugins, `INV6`). |
| EK9 | Retained 2D draw-list caps | Max retained layers per viewport/overlay: 32. Max primitives per layer: 50,000 (100,000 total per viewport). Max path vertices per primitive: 4,096. Coordinate space: `f64` world/sheet units (nm on schematic overlays; plugin-defined logical units on standalone 2D workbenches), mapped on the host GPU thread without calling Wasm during pan/zoom (`EB5`). |
| EK10 | Tier 2 IPC & GPU surface limits | Local transport: UNIX domain socket (`$XDG_RUNTIME_DIR/omnia/plugin-<pid>-<n>.sock` on Linux, mode `0600`) and named pipe (`\\.\pipe\omnia-plugin-<pid>-<n>` on Windows, owner-only DACL). Framing: length-prefixed binary frames (max 4 MiB per control message; bulk buffers over shared memory). Handshake timeout: 3,000 ms; heartbeat interval: 2,000 ms; missed-heartbeat kill/disconnect: 6,000 ms. Max framebuffer dimension: 8192x8192, BGRA8/RGBA8, triple-buffered ring. |
| EK11 | Install & discovery directories | Mirrors `UD10` / `PQ4`. User-global: `%APPDATA%\Omnia\plugins\` (Windows) and `$XDG_CONFIG_HOME/omnia/plugins/` (Linux). Workspace-local: `<workspace>/.omnia/plugins/` (never auto-executed without workspace trust approval, `ED7`). Trust & consent store: `<config_dir>/plugin-trust.ron`. Dev links (unpacked folders): listed in `prefs.ron` under `plugin_dev_dirs`. |
| EK12 | Host API & WIT versioning | WIT package `omnia:plugin@0.1.0` (semver). Host advertises a supported major/minor range; plugins declare `api = "^0.1"` in `plugin.toml`. Incompatible API versions are refused at load with an actionable message; minor additions are backwards-compatible. |

## 5. Architecture

```
                     .oplugin (ZIP) / unpacked folder (plugin.toml)
                                          │
                                          ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                     omnia-plugin (GPUI-free core crate)                      │
│  manifest.rs (EK1-EK4) · archive.rs (EK3) · trust.rs (ED7) · registry.rs    │
│  tier0.rs (parts/themes/templates/keymaps) · patch.rs · ui_ir.rs · scene2d  │
└──────────────┬──────────────────────────┬────────────────────────┬───────────┘
               │                          │                        │
               ▼                          ▼                        ▼
   ┌───────────────────────┐  ┌───────────────────────┐  ┌─────────────────────┐
   │  Tier 0 consumers     │  │ omnia-plugin-wasm     │  │ omnia-plugin-ipc    │
   │  (no code execution)  │  │ (GPUI-free)           │  │ (GPUI-free)         │
   │  · omnia-parts (.olib)│  │ · wasmtime Component  │  │ · UDS / Named Pipe  │
   │  · omnia-theme        │  │ · WIT host bindings   │  │ · process supervisor│
   │  · templates/keymaps  │  │ · off-UI worker pool  │  │ · shm / GPU surface │
   └───────────────────────┘  │ · fuel/epoch & mem cap│  │   frame receiver    │
                              └───────────┬───────────┘  └──────────┬──────────┘
                                          │                         │
                                          └───────────┬─────────────┘
                                                      ▼
         ┌────────────────────────────────────────────────────────────────┐
         │               Omnia Core (GPUI-free lanes)                     │
         │  omnia-doc (ext blocks, Command::PluginPatch, reversible undo) │
         │  omnia-circuit (read views, custom ERC finding merge)          │
         │  omnia-parts / omnia-sim (read-only queries & results stream)  │
         └────────────────────────────────┬───────────────────────────────┘
                                          │
                                          ▼
         ┌────────────────────────────────────────────────────────────────┐
         │               omnia-app / omnia-canvas (GPUI)                  │
         │  · WorkbenchHost (mode/tab switcher, central viewport host)    │
         │  · PluginUiRenderer (EK8 declarative widgets -> omnia-theme)   │
         │  · Retained2dCanvas (EK9 host pan/zoom/hit-test for Tier 1)    │
         │  · ExternalSurfaceView (EK10 GPU/shm frame presenter, Tier 2)  │
         │  · Consent dialog, Plugin Manager, Gesture gate (AINV13)       │
         └────────────────────────────────────────────────────────────────┘
```

Key architectural boundaries:

- **Zero GPUI in plugin core and runtimes (`EINV1`).** `omnia-plugin`, `omnia-plugin-wasm`,
  `omnia-plugin-ipc`, and `omnia-plugin-sdk` never depend on GPUI or `omnia-app`. This enables
  headless CLI execution (`omnia plugin ...`), headless CI testing of every Tier 0/1/2 protocol
  path, and clean reuse by `omnia-cli` and `omnia-bench`.
- **Zero plugin runtime in `omnia-doc` (`EINV2`).** `omnia-doc` only gains pure data types
  (`ExtKey`, `ExtPayload`, `DocExt`, `Command::PluginPatch`) with no dependency on `omnia-plugin`,
  `wasmtime`, `toml`, `zip`, or GPUI.
- **UI thread never blocks on plugin code (`EINV6`).** Every Wasm invocation and every Tier 2 IPC
  exchange runs on background worker threads/executors. GPUI receives immutable snapshots of
  declarative UI trees, retained 2D layers, or completed GPU/shm frame handles over bounded
  channels, preserving `UB3`, `UB4`, `UB5`, `B3` and `B4`.

## 6. Decisions

### ED1 Scope: community bundles, schematic tools/transforms and full workbenches. DECIDED (owner interview 2026-11-25)

Plugins cover three complementary roles without duplicating `.opart`/`.olib` or MCP:

1. **Content bundles (Tier 0):** bundled part libraries (`.olib`/`.opart`), colour themes,
   schematic templates, and keymap presets.
2. **Schematic tools & transforms (Tier 1):** user-invoked commands, circuit generators, custom
   ERC/lint rules, importers, exporters, and BOM/report generators.
3. **Workbenches / workspaces (Tier 1 and Tier 2):** optional full modes/tabs inside Omnia that
   provide new engineering workflows on top of or beside a schematic — explicitly targeting
   `Reid-Engineered/Spectare` as a flagship optional workspace plugin.

### ED2 Hybrid three-tier execution runtime. DECIDED (owner interview 2026-11-25)

- **Tier 0 (Declarative):** pure data validated against strict Rust schemas. Zero code execution,
  instant load, safe by default.
- **Tier 1 (WebAssembly Component Model / WIT via `wasmtime`):** portable single-binary
  `.wasm` components compiled from Rust or any WIT-capable language. Memory-safe, OS-sandboxed,
  cross-platform (Linux + Windows with one artifact), ABI-stable across Rust and GPUI upgrades.
- **Tier 2 (Out-of-process sidecar executable over local IPC):** native companion binaries
  launched and supervised by `omnia-plugin-ipc`. Crash-isolated (a sidecar panic or segfault never
  takes down Omnia or loses unsaved schematics), language- and GPU-toolchain-independent, and
  completely decoupled from Omnia's pinned Rust/GPUI revision.

In-process native dynamic libraries (`cdylib`) are explicitly out of scope.

### ED3 Mode/tab-hosted workbench integration in the UI shell. DECIDED (owner interview 2026-11-25)

A plugin may register:

- **Workbenches:** named workspace modes selectable in the workspace/tab switcher (for example
  `Schematic` | `Spectare` | `<Plugin Workbench>`), owning the central viewport area (`UK4`) plus
  workbench-scoped toolbar groups, left/bottom docks (`UK11`), menu entries and status-bar items,
  while sharing Omnia's window chrome (`UK2`), tab row (`UK5`), command/action registry (`U1.4`),
  theme (`omnia-theme`), and file/project lifecycle.
- **Schematic-side contributions:** dock panels, floating frames (`UK10`), schematic canvas overlays
  (rendered on a dedicated non-mutating overlay layer above `SheetView`), inspector sections,
  menu/toolbar actions, and ERC diagnostic providers.

All host-drawn plugin chrome uses `omnia-theme` tokens exclusively (`INV6`, `UINV1`).

### ED4 Rendering contract: declarative themed UI, retained 2D draw-lists and Tier 2 GPU surfaces. DECIDED (owner interview 2026-11-25)

1. **Chrome, panels, forms and inspectors (Tier 1 & Tier 2):** plugins emit an immutable
   declarative UI tree (`UiNode`, `EK8`) composed of Omnia's themed widgets (layout stacks,
   scroll areas, labels, mono readouts, buttons, toggles, text/numeric inputs, selects, tables,
   trees, badges, sparklines/plots). `omnia-app` diffs and renders them natively with GPUI widgets
   (`U1.3`), guaranteeing native performance, AccessKit accessibility (`UG9`), keyboard focus
   (`UG8`), and `INV6`.
2. **Custom 2D viewports and schematic overlays (Tier 1):** plugins submit retained 2D scene
   layers (`Scene2dLayer`, `EK9`: polylines, polygons, rects, circles/arcs, text runs, grid/axis
   hints, semantic colour tokens, hit-test tags). Omnia's host canvas performs 60 fps pan/zoom,
   culling and hit-testing without calling Wasm per frame (`EB5`), sending only semantic pointer/key
   events back to the plugin when needed.
3. **Heavy / GPU workbench viewports (Tier 2):** in addition to declarative chrome/docks, a Tier 2
   sidecar presents rendered frames into the GPUI workbench viewport over a versioned surface
   protocol (`EK10`): native GPU texture sharing (DMA-BUF / Vulkan on Linux Wayland; DXGI shared
   handle / D3D12 on Windows) where spike `ES2` proves it viable in pinned GPUI, with a zero-panic
   shared-memory BGRA8 ring-buffer fallback (`ES2` settles the primary and fallback paths).

### ED5 Persistence and document portability: namespaced `.omnia` `ext` blocks + sidecar files. DECIDED (owner interview 2026-11-25)

- **Small document/item metadata:** `.omnia` gains optional reverse-DNS namespaced extension blocks
  (`ext[plugin_id]`, `EK6`) at document level and optionally attached to `PartId` / `WireId` /
  `LabelId`. `omnia-doc` stores them as canonical opaque payloads (`schema_version: u32` +
  validated bytes/RON) and round-trips them losslessly even when the owning plugin is absent
  (`EINV4`). When a referenced item is deleted, its attached item-level `ext` entries are removed
  with it and restored on undo.
- **Large workbench payloads:** heavy workbench data (such as 3D meshes, layout/routing databases,
  or large spectrum/capture artifacts) live in sibling or workspace/project sidecar files managed
  through `fs.workspace.*` capabilities, referenced by relative path/hash from the small `.omnia`
  `ext` block so `.omnia` stays fast and lean (`EB7`, `UB9`, `UB10`).
- **Missing-plugin UX:** opening a `.omnia` document whose `ext` blocks reference an uninstalled
  plugin succeeds normally, preserves all `ext` bytes untouched across edits and saves, and shows
  a single dismissible non-blocking toast/banner listing the missing `plugin_id`(s).

### ED6 Hybrid commands and transactional undo. DECIDED (owner interview 2026-11-25)

- **Inside `omnia-doc` (`Command::PluginPatch`):** plugins mutate the schematic and/or their own
  `ext` namespace via one atomic `Command::PluginPatch`:
  ```rust
  Command::PluginPatch {
      plugin_id: Arc<str>,
      label: Arc<str>,               // human-readable undo label, e.g. "Generate Ladder"
      schematic_ops: Vec<Command>,   // standard omnia-doc commands (never nested PluginPatch)
      ext_ops: Vec<ExtOp>,           // SetDocExt / RemoveDocExt / SetItemExt / RemoveItemExt
  }
  ```
  `Doc::apply` validates `plugin_id` (`EK2`), size caps (`EK6`), every `schematic_ops` command, and
  item existence; computes the exact prior `ext` values and schematic inverses; and records one
  self-contained `UndoOp` in `omnia-doc`'s undo stack. If any sub-operation fails, nothing changes
  (`INV4`, `EINV3`). Because the inverse is pure data inside `omnia-doc`, **Ctrl+Z / Ctrl+Y always
  succeed deterministically even if the plugin crashes, unloads, or is uninstalled.**
- **Tier 2 sidecar undo participant:** a Tier 2 workbench that owns external sidecar files may
  additionally register an `UndoParticipant` channel with the workspace history coordinator in
  `omnia-app`. When a workbench-only edit happens in the sidecar, the workspace records a paired
  history entry carrying an opaque `undo_token` and calls `undo(token)` / `redo(token)` over IPC;
  if the sidecar is dead or unloaded, `omnia-app` surfaces a clear non-destructive toast and leaves
  schematic undo history intact (`EINV5`).

### ED7 Capability security, user consent and Tier 2 trust. DECIDED (owner interview 2026-11-25)

- **Default deny:** every plugin starts with zero capabilities (`EK4`). Capabilities are declared
  statically in `plugin.toml` and inspected before any code loads.
- **Tier 0 bundles:** need only `bundle.*` capabilities; no code runs, so enabling a user-installed
  bundle requires only schema/archive validation (`EK3`).
- **Tier 1 Wasm:** on first enable (or whenever an upgrade requests a new capability), Omnia shows
  a concise capability consent dialog listing the requested permissions in plain English and records
  the user's decision in `plugin-trust.ron` (`EK11`). Wasm runs off the UI thread with `EK5` memory
  and epoch caps and WASI capabilities restricted strictly to approved directories/hosts (never raw
  ambient filesystem, environment variables, or arbitrary sockets).
- **Tier 2 sidecars (`sidecar.exec`):** require an explicit elevated **"Trust Native Executable"**
  consent step displaying the binary path and SHA-256 digest. Omnia pins the SHA-256 hash in
  `plugin-trust.ron` and refuses to launch if the binary changes on disk until the user re-approves
  (or matches a user-trusted Ed25519 publisher key, optional in v1).
- **Workspace-local plugins (`<workspace>/.omnia/plugins/`):** discovered as **disabled** until the
  user explicitly trusts that workspace/plugin, preventing drive-by code execution when opening a
  downloaded project folder (`EINV8`).

### ED8 Packaging, install scopes and developer workflow. DECIDED (owner interview 2026-11-25)

- **Identical inner layout for `.oplugin` ZIP and unpacked directories** (mirroring `.opart`/`.olib`
  in `parts-plan.md` `PD2`):
  ```text
  my-plugin.oplugin (or unpacked folder)
    plugin.toml          # manifest: id, version, api, tier, permissions, contributions
    LICENSE              # SPDX licence text
    README.md            # optional user-facing summary
    assets/              # optional Tier 0: parts/*.olib, themes/*.toml, templates/*.omnia, keymaps/*.toml
    wasm/plugin.wasm     # Tier 1 Wasm Component Model binary
    bin/<os>-<arch>/...  # Tier 2 sidecar executable(s) (per target triple)
  ```
- **Install scopes:** user-global and workspace-local (`EK11`). When the same `plugin_id` exists in
  both, workspace-local shadows user-global for that workspace only (with explicit indicator in
  Plugin Manager).
- **v1 install sources:** local `.oplugin` file, local unpacked dev folder (with live file-watch
  hot-reload for Tier 0 & Tier 1), and Git repository URL (via `omnia plugin install <path|git-url>`
  or Plugin Manager UI, pinned to a tag/commit).
- **In-app registry/marketplace:** deferred past v1; `prefs.ron` and manifest metadata leave a
  reserved `registry_url` hook (`EQ5`).

### ED9 Phased milestone delivery. DECIDED (owner interview 2026-11-25)

Build strictly bottom-up so each milestone ships verifiable value and respects cross-plan
dependencies:

- **Milestone E0:** Plan, owner sign-off (`HE0`), and spikes `ES1` (`wasmtime` + `deny.toml` +
  cold-start/RSS) and `ES2` (GPUI shared GPU surface on Linux Wayland + Windows), gated at `HE1`.
- **Milestone E1:** GPUI-free `omnia-plugin` core host, `.oplugin` archive/manifest validator,
  trust store, CLI `omnia plugin` basics, and full **Tier 0** content bundles (`.olib`, themes,
  templates, keymaps).
- **Milestone E2:** `omnia-doc` `ext` + `Command::PluginPatch` + RON v4 (after `C1.1`),
  `omnia-plugin-wasm` runtime, WIT host API (`omnia:plugin@0.1.0`), custom ERC/lint & exporters,
  and `omnia-plugin-sdk` + `omnia plugin new/build/pack`.
- **Milestone E3:** Declarative UI widget renderer (`omnia-theme`), retained 2D viewport &
  schematic overlays, workbench tab/mode host in `omnia-app`, dev hot-reload, and Plugin Manager +
  consent UI (`HE2`).
- **Milestone E4:** `omnia-plugin-ipc` sidecar supervisor, Tier 2 shared GPU surface / shm frame
  presenter, input forwarding, external undo participant, `Reid-Engineered/Spectare` integration
  acceptance check, and Orion reference-machine gate (`HE3`).

### ED10 Authoring SDK and v1 tooling. DECIDED (owner interview 2026-11-25)

Omnia ships in v1:
1. The canonical versioned WIT interface package under `wit/omnia-plugin/` (`omnia:plugin@0.1.0`).
2. An ergonomic, MIT/Apache-2.0 Rust SDK crate `crates/omnia-plugin-sdk/` wrapping `wit-bindgen`
   with builders for `PluginPatch`, `UiNode`, `Scene2dLayer`, ERC findings and typed error helpers,
   plus a companion Tier 2 IPC Rust client helper (`omnia-plugin-ipc-client` or feature flag).
3. CLI subcommands in `omnia-cli`: `omnia plugin new <name>`, `omnia plugin pack <dir> <out.oplugin>`,
   `omnia plugin verify <path>`, `omnia plugin install/list/enable/disable/remove`, and
   `omnia plugin run <id> <command> --doc <file>` (headless testing).

### ED11 Unified Host API contract and gesture safety. PROPOSED

- Tier 1 (WIT) and Tier 2 (binary IPC) expose the **same logical service model** (`doc`, `circuit`,
  `parts`, `sim`, `erc`, `ui`, `viewport`, `workspace`, `host`), defined once in `omnia-plugin`
  IR types and projected to WIT (`omnia-plugin-wasm`) and framed RPC (`omnia-plugin-ipc`).
- Every write request carries `expected_revision: u64` (matching `mcp-plan.md` `AD4`). If the
  document changed concurrently, the host refuses with `StaleRevision` so the plugin re-reads and
  retries cleanly.
- **Gesture gate (`AINV13`):** in app mode, while a canvas drag, move or wire gesture is in
  progress, incoming plugin `PluginPatch` writes are queued (bounded queue, max 8) or rejected with
  `BusyInGesture` (for interactive calls), never mutating the document mid-gesture.
- **Untrusted text hygiene (`AD14`):** plugin-supplied display strings (labels, diagnostics, toast
  text) are sanitised for control characters, length-capped (`EK8`), and never concatenated into
  SPICE decks (`PK10`) or shell commands.

### ED12 Determinism and conflict resolution. PROPOSED

- Installed plugins load and execute hooks (ERC passes, exporters, menu registration) in strict
  lexicographical `plugin_id` order — never filesystem enumeration order or hash-map order
  (`EINV9`).
- **Conflict rules:**
  - Duplicate `plugin_id` within the same scope: refused with a diagnostic; workspace-local shadows
    user-global across scopes (`ED8`).
  - Duplicate workbench id, action id or ERC rule code: always namespaced as
    `<plugin_id>:<local_id>`, making cross-plugin collisions impossible by construction.
  - Keymap presets (Tier 0): built-in bindings win on conflict unless the user explicitly selects a
    plugin keymap preset in Preferences; conflicting plugin bindings are logged and skipped
    deterministically.
  - Themes (Tier 0): registered by `<plugin_id>:<theme_name>`; cannot overwrite built-in themes.

### ED13 Crash, timeout and missing-plugin degradation. PROPOSED

- **Tier 1 fault quarantine:** if a Wasm plugin traps, runs out of memory, or exceeds its `EK5`
  deadline, its instance is dropped immediately, pending UI/overlay contributions from that plugin
  are cleared, a non-blocking toast explains the failure, and the plugin transitions to `Faulted`
  state (auto-restarted only on explicit user reload/retry, never in a tight crash loop).
- **Tier 2 crash recovery:** if a sidecar process exits unexpectedly or misses heartbeats (`EK10`),
  the workbench viewport displays a themed "Workbench disconnected" recovery state with a one-click
  **Restart** button and diagnostic log tail. All schematics and other tabs remain completely
  unaffected.
- **Safe mode:** `omnia-app --safe-mode` (and a Help menu restart action) launches with all
  non-bundled plugins disabled so a broken plugin can never lock a user out of Omnia.

## 7. Invariants

Every invariant has a named automated test (unit, property, or integration) citing its `EINV` id.

| ID | Invariant | Enforced by / Task |
| --- | --- | --- |
| EINV1 | `omnia-plugin`, `omnia-plugin-wasm`, `omnia-plugin-ipc` and `omnia-plugin-sdk` never depend on `gpui`, `gpui_platform`, `omnia-canvas` or `omnia-app` (extends `INV12`, `AINV8`, `PINV1`). | Dependency-graph test in `omnia-plugin` (`E1.1`, `E2.2`, `E4.1`) |
| EINV2 | `omnia-doc` never depends on `omnia-plugin*`, `wasmtime`, `toml`, `zip` or `sha2`. | `Cargo.toml` / dependency test (`E2.1`) |
| EINV3 | A refused, trapped or timed-out `Command::PluginPatch` leaves `Doc` `==` (including `revision`, `ext` blocks and undo history; extends `INV4`, `AINV4`). | Property + fault tests (`E2.1`, `E2.2`) |
| EINV4 | Opaque `ext` blocks in `.omnia` round-trip byte-identically through `Doc::load` -> edit unrelated items -> `Doc::save` even when the owning plugin is absent (`ED5`). | Round-trip property test (`E2.1`) |
| EINV5 | Every applied `Command::PluginPatch` undoes and redoes cleanly inside `omnia-doc` with zero plugin callbacks, even after the plugin is unloaded or killed (`ED6`). | Unit + property tests (`E2.1`, `E2.3`) |
| EINV6 | No Wasm execution or blocking Tier 2 IPC ever runs on the GPUI UI thread; `SheetView` paint count stays 0 during plugin chrome/panel interaction (`UB3`, `EB4`). | Worker-thread assertion + `bench ui` test (`E2.2`, `E3.3`, `E4.2`) |
| EINV7 | Plugin declarative UI and retained 2D draw-lists contain zero raw colour literals; every rendered colour resolves through `omnia-theme` tokens (`INV6`, `UINV1`). | Type-level token enum + theme-switch test (`E3.1`, `E3.2`) |
| EINV8 | No Tier 1 or Tier 2 plugin executes code, accesses files, or opens network connections without manifest declaration (`EK4`) and persisted user consent (`ED7`). Workspace-local plugins start disabled; Tier 2 binaries fail to launch if their SHA-256 differs from the trusted pin. | Security/consent test suite (`E1.2`, `E2.2`, `E4.1`) |
| EINV9 | Plugin discovery, manifest indexing, ERC execution order, `ext` serialisation order and `.oplugin` packing are 100% deterministic across filesystem/ZIP/hash-map orderings. | Shuffled-order property tests (`E1.1`, `E1.2`, `E2.1`) |
| EINV10 | Hostile `.oplugin` archives (path traversal, ZIP bombs, symlinks, invalid UTF-8, oversized entries, bad `plugin_id`) are refused before allocating past `EK3`. | Hostile-archive fuzz/unit tests (`E1.1`) |
| EINV11 | In app mode, plugin writes never mutate `Doc` while a canvas gesture is active (extends `AINV13`). | App-host gesture test (`E3.4`) |
| EINV12 | Deleting a schematic item removes its attached item-level `ext` entries atomically, and undoing the delete restores them byte-identically. | `omnia-doc` command property test (`E2.1`) |
| EINV13 | Plugin-contributed ERC findings are tagged with `<plugin_id>:<rule_id>` and never suppress or mutate built-in `omnia-circuit` or `omnia-sim` findings. | ERC merge test (`E2.3`) |
| EINV14 | A Tier 2 sidecar crash, hang, malformed IPC frame or oversized surface descriptor never panics Omnia, never leaks shared-memory/socket handles, and leaves all open schematics intact (`EB9`). | Fault-injection + fuzz tests (`E4.1`, `E4.2`) |

## 8. Scenarios

Each scenario is an automated test with a concrete expected result.

| ID | Setup & Action | Expected result |
| --- | --- | --- |
| ETC-tier0-bundle | Install a `.oplugin` Tier 0 bundle containing a 3-part `.olib`, a dark/light theme, a schematic template and a keymap preset; enable it | Parts appear in `omnia-parts::Catalog`, theme switches cleanly without `INV6` violations, template opens in New from Template, keymap preset is selectable; disabling removes all four cleanly (`E1.3`). |
| ETC-hostile-oplugin | Feed `omnia-plugin` a `.oplugin` with `../escape.wasm`, a 150:1 zero-filled ZIP bomb, a symlink entry, an invalid `plugin_id`, and an unknown `schema = 99` | All five are refused with specific `PluginError` variants before allocating past `EK3` (`EINV10`). |
| ETC-consent-gate | Place a Tier 1 Wasm plugin in `<workspace>/.omnia/plugins/` requesting `doc.write` and `net.hosts = ["example.org"]`; open workspace | Plugin loads in `Untrusted/Disabled` state; zero Wasm code runs; enabling prompts for `doc.write` + `net.hosts`; upgrading the manifest to add `fs.workspace.write` revokes auto-start until re-approved (`EINV8`). |
| ETC-patch-atomic | Wasm plugin returns a `Command::PluginPatch` placing 3 parts, adding 2 wires, setting doc `ext`, and including 1 invalid out-of-bounds wire | `Doc::apply` refuses the entire patch with `CommandError::OutOfBounds`; `Doc` (parts, wires, `ext`, `revision`, undo stack) is strictly `==` to before (`EINV3`). |
| ETC-patch-offline-undo | Wasm plugin applies a `Command::PluginPatch` (parts + item `ext` + doc `ext`); unload and delete the plugin; press Undo then Redo | Undo removes parts and restores prior `ext` cleanly; Redo re-applies both identically; no plugin callback is invoked (`EINV5`). |
| ETC-ext-missing-plugin | Open a RON v4 `.omnia` containing `ext["org.example.layout"]` on a machine without that plugin; move an unrelated resistor; delete a part carrying item `ext`, undo, then save | Non-blocking missing-plugin banner appears; saved file preserves `ext["org.example.layout"]` byte-identically (`EINV4`, `EINV12`). |
| ETC-wasm-erc | Enable a reference Wasm ERC plugin checking resistor power/label naming on a `1k` sheet | Findings appear tagged `dev.omnia.sample-erc:*` in deterministic order alongside built-in findings without hiding any built-in finding (`EINV9`, `EINV13`). |
| ETC-wasm-bom-export | Invoke reference Wasm CSV BOM exporter via CLI (`omnia plugin run`) and UI menu | Deterministic UTF-8 CSV written via host `fs.export` picker/path; plugin cannot read arbitrary host paths outside approved scope (`EINV8`). |
| ETC-wasm-trap | Run a Wasm plugin that divides by zero / executes `unreachable` inside a tool call | Worker catches trap, quarantines plugin to `Faulted`, emits toast, `Doc` unchanged, UI never stalls > 50 ms (`EB9`, `EINV3`). |
| ETC-wasm-timeout | Run a Wasm plugin with an infinite loop in both an ERC hook and a user command | Epoch interruption terminates the hook ≤ 250 ms and the command ≤ 5 s; worker thread cleanly recovered; UI stays responsive (`EB9`). |
| ETC-gesture-defer | Start a canvas part drag (`SheetCanvas` gesture active) while a background Wasm plugin submits a `PluginPatch` | Revision does not change during the drag; patch is deferred until gesture release (or rejected with `BusyInGesture` per `ED11`), preserving `AINV13` (`EINV11`). |
| ETC-ui-panel-theme | Render a Tier 1 declarative panel with tables, inputs and buttons; toggle light/dark theme; hover across the panel | All widgets re-skin via `omnia-theme` tokens (`EINV7`); `SheetView` paint count stays 0 (`UB3`, `EB4`); keyboard tab navigation reaches controls (`UG8`). |
| ETC-2d-viewport-pan | Open a Tier 1 2D workbench tab with 10,000 retained primitives; run 600-frame pan/zoom script | Pan/zoom runs at < 16.7 ms p99 on host GPU with 0 guest Wasm invocations during camera motion (`EB5`). |
| ETC-dev-hot-reload | Edit and rebuild an unpacked dev Tier 1 plugin while Omnia is running | Host detects change, re-validates manifest (refusing silent new permissions), reloads Wasm instance, refreshes panel/overlay without restarting Omnia (`E3.4`). |
| ETC-sidecar-trust | Register a Tier 2 sidecar plugin, approve trust (pinning SHA-256), then flip 1 byte in the sidecar binary and relaunch | Host refuses launch with `HashMismatch` and prompts user to re-verify; binary is never spawned (`EINV8`). |
| ETC-sidecar-workbench | Launch a reference Tier 2 GPU sidecar workbench (modeling `Reid-Engineered/Spectare`), switch tabs between Schematic and Workbench, forward mouse/key events, send a sidecar edit + undo, then `SIGKILL` the sidecar | Workbench presents 60 Hz frames (`EB6`), forwards input and paired undo cleanly, and after `SIGKILL` displays the themed recovery banner within 200 ms with the schematic tab and undo stack intact (`EB9`, `EINV14`). |

## 9. Spikes

Two targeted spikes in Milestone E0 settle every high-risk technical unknown before production code
is written.

| ID | Question it settles | How |
| --- | --- | --- |
| ES1 | Does `wasmtime` (Component Model enabled, minimal feature set) pass `cargo deny check` cleanly under [deny.toml](deny.toml) (no disallowed licences or unapproved advisories)? What are its actual impact on compile time, binary size, cold start (`EB1`), per-instance RSS (`EB8`), and epoch interruption latency (`EB9`)? | Build an isolated prototype in `crates/omnia-bench/src/spike/plugin_wasm.rs` (or a spike branch/bench) compiling a minimal WIT component, measuring cold init, instantiation, 1,000 host↔guest calls, epoch abort latency, RSS with 5 instances, and `cargo deny check` output. Record results in `docs/measurements/E0.3.md`. |
| ES2 | Because GPUI's `paint_surface` is macOS/iOS-only (`G11`), how can a Tier 2 out-of-process sidecar present 60 Hz frames into a GPUI viewport on **Linux Wayland** (Orion) and **Windows**? Compare (a) native GPU texture import (DMA-BUF / DXGI shared handle via GPUI's underlying `wgpu`/DX12/Blade surface) vs (b) a triple-buffered shared-memory BGRA8 ring uploaded to a GPUI `RenderImage` / GPU texture per damaged frame, measuring host compositing latency (`EB6`), zero-idle cost (`UB4`), and `SheetView` isolation (`UB3`). | Build a standalone spike `bench spike plugin-surface` with a tiny mock sidecar generating 1280x800 60 Hz animated test frames over UDS/named pipe + shared memory (and native handle probe where GPUI exposes backend hooks). Measure on Windows and on Orion (Wayland). Record results in `docs/measurements/E0.4.md` and settle `ED4`/`EB6` at `HE1`. |

## 10. Crates, WIT interfaces and APIs

### New crates

1. **`crates/omnia-plugin/` (GPUI-free):**
   - `manifest.rs` — `PluginManifest`, `PluginId` (`EK2`), `PluginTier`, `Capability` (`EK4`),
     `ContributionDecl` (bundles, commands, erc, exporters, panels, overlays, workbenches).
   - `archive.rs` — `PluginSource` (`Directory` | `ZipArchive`), strict `EK3` validator and
     deterministic packer (`pack_oplugin`), SHA-256 content & binary digests.
   - `trust.rs` — `TrustStore` (`plugin-trust.ron`), capability grants, workspace trust, Tier 2
     SHA-256 pins and optional Ed25519 signature verification (`ED7`).
   - `registry.rs` — `PluginHost`, discovery across user-global/workspace-local/dev dirs (`EK11`),
     deterministic load order (`ED12`), lifecycle state machine (`Discovered` -> `Untrusted` ->
     `Enabled` -> `Active` | `Faulted` | `Disabled`).
   - `tier0.rs` — loaders and validators bridging Tier 0 bundles into `omnia-parts`, `omnia-theme`,
     template catalog and keymap presets.
   - `ui_ir.rs` — `UiTree`, `UiNode`, `ThemeToken`, `UiEvent` (`EK8`, `EINV7`).
   - `scene2d.rs` — `Scene2dLayer`, `DrawPrim`, `ViewportEvent` (`EK9`).
   - `service.rs` — transport-agnostic `PluginService` bridging `omnia-doc`, `omnia-circuit`,
     `omnia-parts`, and `omnia-sim` read views + `PluginPatch` builder/validator (`ED11`).

2. **`crates/omnia-plugin-wasm/` (GPUI-free):**
   - Owns the `wasmtime` engine, compiled component cache, per-plugin worker actors, epoch ticker
     thread (`EK5`), WASI capability filter (`EK4`), and generated `wit-bindgen` host bindings for
     `wit/omnia-plugin/`.

3. **`crates/omnia-plugin-ipc/` (GPUI-free):**
   - Owns Tier 2 sidecar spawning, SHA-256 pre-launch verification (`EINV8`), UDS/named-pipe framed
     RPC transport (`EK10`), heartbeat supervision, shared-memory / GPU surface frame ring receiver,
     input event forwarding, and `UndoParticipant` protocol (`ED6`).

4. **`crates/omnia-plugin-sdk/` (GPUI-free, publishable in future):**
   - Ergonomic Rust SDK wrapping `wit/omnia-plugin/` for Tier 1 Wasm plugin authors, plus an
     optional `sidecar` feature exposing the matching Rust IPC server helper for Tier 2 workbenches
     (such as `Reid-Engineered/Spectare`).

### Canonical WIT package (`wit/omnia-plugin/`)

```wit
package omnia:plugin@0.1.0;

interface types {
    type doc-revision = u64;
    type item-id = u64;
    record pt-nm { x: s32, y: s32 }
    record ext-entry { schema-version: u32, payload: list<u8> }
    record finding { rule-id: string, severity: severity, message: string, items: list<item-id> }
    enum severity { info, warning, error }
}

interface doc-read {
    use types.{doc-revision, item-id, ext-entry};
    revision: func() -> doc-revision;
    inspect-json: func(filter-json: string) -> result<string, string>;
    circuit-context: func(view-json: string) -> result<string, string>;
    get-doc-ext: func() -> option<ext-entry>;
    get-item-ext: func(item: item-id) -> option<ext-entry>;
}

interface doc-write {
    use types.{doc-revision, item-id, ext-entry};
    record patch {
        expected-revision: doc-revision,
        label: string,
        schematic-ops-json: string,   // matches omnia-agent EditOp schema
        ext-ops: list<ext-op>,
    }
    variant ext-op {
        set-doc(ext-entry),
        remove-doc,
        set-item(tuple<item-id, ext-entry>),
        remove-item(item-id),
    }
    apply-patch: func(p: patch) -> result<doc-revision, string>;
}

interface ui { /* declarative UiNode tree, ThemeToken enum, UiEvent */ }
interface scene2d { /* retained Scene2dLayer, DrawPrim, ViewportEvent */ }

world omnia-plugin {
    import doc-read;
    import doc-write;
    import ui;
    import scene2d;
    export run-command: func(command-id: string, args-json: string) -> result<string, string>;
    export run-erc: func() -> result<list<types.finding>, string>;
    export render-panel: func(panel-id: string) -> result<list<u8>, string>;
    export on-event: func(event-bytes: list<u8>) -> result<_, string>;
}
```

### Additions in `crates/omnia-doc/` (`E2.1`, zero plugin-runtime dependency)

```rust
// ---- omnia-doc: ext.rs (E2.1) ----
pub struct PluginNamespace(Arc<str>);          // validated EK2 reverse-DNS id
pub struct ExtEntry {
    pub schema_version: u32,
    pub payload: Arc<[u8]>,                    // opaque canonical bytes, capped by EK6
}
pub enum ExtOp {
    SetDoc(ExtEntry),
    RemoveDoc,
    SetItem { item: ItemId, entry: ExtEntry },
    RemoveItem { item: ItemId },
}

// ---- omnia-doc: command.rs (E2.1) ----
// Command gains:
//   PluginPatch {
//       namespace: PluginNamespace,
//       label: Arc<str>,
//       schematic_ops: Vec<Command>,          // validated: no nested PluginPatch
//       ext_ops: Vec<ExtOp>,
//   }
// CommandError gains:
//   InvalidPluginNamespace, NestedPluginPatch, ExtLimitExceeded
//
// ---- omnia-doc: Doc queries (E2.1) ----
//   pub fn doc_ext(&self, ns: &str) -> Option<&ExtEntry>;
//   pub fn doc_ext_namespaces(&self) -> impl Iterator<Item = &str>; // sorted
//   pub fn item_ext(&self, item: ItemId, ns: &str) -> Option<&ExtEntry>;
```

### Additions in `crates/omnia-app/` (`E3.1`–`E3.4`, `E4.2`–`E4.3`)

- `crates/omnia-app/src/plugins/mod.rs` — app-side `PluginController`, gesture gate bridge
  (`AINV13`), consent dialogs, Plugin Manager view, `--safe-mode` flag.
- `crates/omnia-app/src/plugins/ui_renderer.rs` — `UiTree` -> native GPUI `U1.3` widgets (`INV6`).
- `crates/omnia-app/src/plugins/viewport2d.rs` — host-accelerated retained 2D viewport & schematic
  overlay element (`EB4`, `EB5`).
- `crates/omnia-app/src/plugins/workbench.rs` — mode/tab switcher & workbench host slot (`ED3`).
- `crates/omnia-app/src/plugins/surface.rs` — Tier 2 GPU/shm frame presenter & input forwarder
  (`ED4`, `EB6`).

## 11. Dependencies and licensing

All new dependencies must pass `cargo deny check` under [deny.toml](deny.toml) and be verified in
`ES1` before Milestone E2 starts.

| Crate | Use | Licence | Status |
| --- | --- | --- | --- |
| `toml` (`1`), `zip` (`9`), `sha2`, `serde`, `ron` | Manifest parsing, `.oplugin` ZIP archives, SHA-256 digests, trust store & RON v4 | MIT / Apache-2.0 | Already in `Cargo.lock` |
| `wasmtime`, `wasmtime-wasi` | Tier 1 Wasm Component Model runtime + WASI sandbox (`omnia-plugin-wasm`) | Apache-2.0 WITH LLVM-exception | New — audited in spike `ES1` (check every transitive crate against `deny.toml`; log LLVM-exception SPDX handling if needed) |
| `wit-bindgen` | Guest bindings in `omnia-plugin-sdk` | Apache-2.0 WITH LLVM-exception | New — audited in spike `ES1` |
| `ed25519-dalek` (optional) | Optional Tier 2 publisher signature verification (`ED7`, `EQ4`) | BSD-3-Clause | New — optional feature, audited in `E1.2` |
| `notify` (or lightweight polling) | Dev hot-reload file watch (`ED8`) | CC0-1.0 / Artistic-2.0 or MIT/Apache-2.0 alternative | Evaluate in `E3.4` against `deny.toml` (fall back to clean 500 ms mtime poll if licence conflicts) |
| OS IPC (`std::os::unix::net`, `windows-sys` / `interprocess`) | Tier 2 UDS + Windows named pipes & shared memory (`omnia-plugin-ipc`) | MIT / Apache-2.0 | Audit in `ES2` / `E4.1` (prefer `std` + already-present Windows/Unix crates in `Cargo.lock`) |

No GPL/LGPL crate is permitted. Any SPDX exception (such as `Apache-2.0 WITH LLVM-exception` used
by `wasmtime`) must be explicitly reviewed and approved by the owner at `HE1` before updating
`deny.toml`.

## 12. Risks and mitigations

| Risk | Mitigation | Early test |
| --- | --- | --- |
| GPUI `paint_surface` is macOS-only (`G11`), blocking zero-copy GPU texture sharing on Wayland/Windows | Spike `ES2` explicitly benchmarks both native texture import hooks and a triple-buffered shared-memory BGRA8 ring uploaded via GPUI's image/texture path on Orion and Windows before Milestone E4 starts | `ES2` (`E0.4`) |
| `wasmtime` pulls transitive crates outside `deny.toml` or increases cold start / binary size too much | Spike `ES1` checks `cargo deny`, cold-start lazy engine initialisation (`EB1`), and RSS (`EB8`) with minimal features (`cranelift` only, no unused proposals) | `ES1` (`E0.3`) |
| Plugin UI panels regress `SheetView` paint isolation (`UB3`) or warm-frame allocations (`UB5`) | Declarative UI renders exclusively through `omnia-app`'s `U1.3` non-focus-stealing chrome widgets (`UG15`–`UG17`), with an explicit `bench ui` gate in `E3.5` | `ETC-ui-panel-theme`, `EB4` |
| Plugin writes race with an active canvas drag/wire gesture or concurrent agent edit | Enforce `expected_revision` and the app-mode gesture gate (`AINV13`, `ED11`) inside one shared mutation coordinator | `ETC-gesture-defer`, `EINV11` |
| Malicious or buggy workspace plugin executes automatically on folder open | Workspace-local plugins start disabled (`ED7`); Tier 1 needs capability consent; Tier 2 needs explicit trust + SHA-256 pin (`EINV8`) | `ETC-consent-gate`, `ETC-sidecar-trust` |
| Large workbench state bloats `.omnia` files and breaks `UB10` / `UB9` | Hard `EK6` cap (1 MiB total `ext` per document) forces large workbench datasets into sibling/project sidecar files (`ED5`) | `EB7`, `E2.1` |
| `omnia-doc` file conflicts with in-flight `parts` (`P2.1`–`P2.3`) and `context` (`C1.9`, `C1.1`) tasks | `E2.1` explicitly sequences after `C1.1` (extending `XD7`), keeping Milestone E0 and Milestone E1 completely independent of `omnia-doc` | `XD7` dependency chain |
| Tier 2 sidecar crashes or hangs and leaves zombie processes or corrupted undo history | Job objects (Windows) / `PR_SET_PDEATHSIG` + process-group kill (Linux), heartbeat timeout (`EK10`), and isolated sidecar undo participant (`ED6`) | `ETC-sidecar-workbench`, `EINV14` |

## 13. Open questions

Each open question has a **DEFAULT**; build to the default until the owner's dated answer replaces
it at `HE0` (or `HE1` for spike-dependent defaults).

| ID | Question | Default |
| --- | --- | --- |
| EQ1 | Should Tier 1 Wasm support both precompiled/cached native artifacts (faster startup) and portable `.wasm` components, or compile `.wasm` on first load and cache locally under `<cache_dir>/omnia/wasm-cache/`? | **DEFAULT:** Ship portable `.wasm` components inside `.oplugin`; `wasmtime` compiles on first load and caches per-host-version compiled artifacts under `<cache_dir>/omnia/wasm-cache/` (invalidated automatically when Omnia's `wasmtime` version changes). |
| EQ2 | What exact encoding should opaque `ext` payloads use inside RON `version: 4`? | **DEFAULT:** Base64-encoded canonical bytes (or structured RON `ExtPayload::Text(String)` / `ExtPayload::Bytes(Vec<u8>)` with deterministic encoding) so hand-editable JSON/TOML plugin metadata stays readable in diffs while binary payloads round-trip losslessly. |
| EQ3 | Should `Doc::save` write RON `version: 4` unconditionally after `E2.1`, or stay on `version: 3` whenever `ext` is empty? | **DEFAULT:** Write `version: 3` when `ext` is empty (keeping existing v3 fixtures byte-identical) and `version: 4` whenever any `ext` block is present; `Doc::load` reads v2, v3 and v4. |
| EQ4 | Should Ed25519 publisher signatures for Tier 2 sidecars be implemented in v1 alongside SHA-256 pinning, or deferred to v1.1? | **DEFAULT:** Mandatory SHA-256 pinning in v1 (`ED7`); optional Ed25519 signature verification field reserved in `plugin.toml` and `plugin-trust.ron`, implemented if `ed25519-dalek` adds zero friction in `E1.2`. |
| EQ5 | Should `plugin.toml` reserve optional registry metadata (`homepage`, `repository`, `registry_id`) now for a future community index? | **DEFAULT:** Yes — optional descriptive fields (`homepage`, `repository`, `license`, `categories`) in `schema = 1`, with no network registry fetches in v1. |
| EQ6 | How should `omnia-agent` / MCP interact with installed plugins (for example invoking a plugin command or reading plugin ERC findings)? | **DEFAULT:** Read-only exposure first: plugin ERC findings flow into `omnia_context` findings (`EINV13`); exposing plugin-contributed commands as dynamic MCP tools is deferred past v1 to preserve `mcp-plan.md`'s static tool-schema guarantee (`AD14`). |
| EQ7 | Should custom simulation engines (`SimEngine`) and custom vector symbols (`PQ3`) join the plugin WIT/IPC contract in a follow-up minor version (`0.2.0`)? | **DEFAULT:** Yes — keep `omnia:plugin@0.1.0` focused on bundles, schematic tools/ERC/exporters, UI/2D workbenches, and Tier 2 GPU workbenches; add `sim-engine` and `symbol-provider` interfaces in `0.2.0` once `omnia-sim` (`S2.3`) and `PQ3` stabilise. |
| EQ8 | For Tier 2 workbenches like `Reid-Engineered/Spectare`, should a project folder convention (`.omnia-project.toml` or workspace root folder) be introduced when multiple sidecar files accompany one or more `.omnia` sheets? | **DEFAULT:** Start with sibling sidecar files next to the `.omnia` file (resolved relative to the document directory, gated by `fs.workspace.*`), and align with any future project container decided in `ROADMAP.md`'s open file-experience decision. |

## 14. Changes to other files

None of the files below are edited by `E0.1`; each row names the task that makes the change.

| Where | Change | Made by |
| --- | --- | --- |
| `AGENTS.md` | Add `plugin-plan.md` / `plugin-roadmap.md` to the sources-of-truth table, register the `E`-prefixed ids (`EB`, `EK`, `ED`, `EINV`, `ETC`, `ES`, `EQ`, `HE`, tasks `E0.1`…), and add `HE*` to hard rule 4. | `E0.2` (upon `HE0` sign-off) |
| `ROADMAP.md` | Link `plugin-plan.md` and `plugin-roadmap.md` in the intro and Stage 6 (extensibility / workbenches). | `E0.2` |
| `.github/wiki/build_wiki.py` | Register `("plugin-roadmap.md", "Plugin-roadmap", "Plugins")` in `ROADMAPS`, `("plugin-plan.md", "Plugins")` in `PLANS`, and add `plugin` to `DEPENDS_FILLER`. | `E0.2` |
| `docs/file-format-proposal.md` | Reserve optional section kind `EXT_DATA` for namespaced `omnia-doc` extension blocks (`EK7`). | `E0.2` |
| `deny.toml` | Log any approved `wasmtime` / `wit-bindgen` SPDX licence exception (only after `ES1` audit and owner `HE1` sign-off). | `E0.3` / `E0.4` (`HE1`) |
| `canvas-plan.md` section 9 | Document `ext` queries, `Command::PluginPatch`, and RON `version: 4` beside `P2.1`–`P2.3` notes. | `E2.1` (in the same PR as `omnia-doc` changes) |
| `ui-plan.md` sections 1, 5 and 10 | Document the workbench mode/tab host slot, plugin dock/overlay slots, Plugin Manager dialog, and `--safe-mode`. | `E3.3` |
| `mcp-plan.md` | Note that plugin-contributed ERC findings appear under `<plugin_id>:<rule_id>` and that `omnia-agent` file host opens/saves RON v4 preserving `ext`. | `E2.1` / `E2.3` |
| `THIRD_PARTY.md` | Record any new third-party dependencies added by `omnia-plugin-wasm` or `omnia-plugin-ipc`. | `E2.2`, `E4.1` |
