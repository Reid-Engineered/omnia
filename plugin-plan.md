# Omnia plugin system: plan (reference)

This file is the reference for Omnia's plugin and workbench extension system: declarative content
bundles (Level 0 — Bundle), sandboxed WebAssembly Component Model plugins (Level 1 — Wasm),
out-of-process sidecar workbenches with shared framebuffer/GPU surfaces (Level 2 — Sidecar),
namespaced `.omnia` extension data and reversible `Command::PluginPatch` edits, host-rendered
declarative UI and retained 2D viewports, capability permissions, `.oplugin` packaging, and the
authoring SDK. The work itself is in [plugin-roadmap.md](plugin-roadmap.md), which lists tasks
that cite the ids defined here.

It sits beside [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md), [ui-plan.md](ui-plan.md), [sim-plan.md](sim-plan.md),
[parts-plan.md](parts-plan.md) and [context-plan.md](context-plan.md), and re-plans nothing they
decide except where section 14 explicitly lists a sequenced update (notably RON `version: 4` in
`E2.1`, after `P2.3`, `C1.9` and `C1.1`). Every unprefixed id (`B`, `K`, `G`, `D`, `INV`, `Q`, `T`)
is defined in the canvas plan; `W` in the wiring plan; `A` in the agent plan; `U` in the UI plan;
`S` in the simulation plan; `P` in the parts plan; `C` in the circuit-context plan; `F` in
[docs/file-format-proposal.md](docs/file-format-proposal.md). Cross-plan decisions `XD1`–`XD9`
live in `parts-plan.md` section 6. Plugin execution levels are named **Level 0 (Bundle)**,
**Level 1 (Wasm)** and **Level 2 (Sidecar)** so they stay distinct from `context-plan.md`'s
finding tiers (`CK5`).

Written 2026-11-25 against `main` at `a022e6a`, updated 2026-11-25 against `main` at `ad10f58`
after independent review of pull request #135, recording the owner's architectural decisions from
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

WebAssembly (Wasm), WIT, WASI, TOML, RON, ZIP, JSON, JSON-RPC, SPDX, SHA-256, Ed25519, Wayland,
X11, DMA-BUF, Vulkan, `wasmtime`, `wit-bindgen`, `gpui`, `wgpu`, `ngspice` and `RIS-C` are named
here as open standards, graphics/windowing interfaces, open-source crates or first-party engines
already referenced in the repository. `Reid-Engineered/Spectare` is named as a first-party target
workspace plugin. Operating systems appear only as `AGENTS.md` names them (Linux and Windows). No
third-party company or commercial product is named (AGENTS hard rule 1).

## 1. Scope

Omnia already has dedicated mechanisms for component definitions and SPICE models (`.opart`/`.olib`
in `parts-plan.md`) and for external agent/script automation (`omnia-agent` / MCP in `mcp-plan.md`).
The plugin system builds on top of them rather than replacing them.

In scope:

- **Level 0 — Declarative content bundles (`Bundle`).** Zero-code plugins that package
  `.olib` / `.opart` part libraries, `omnia-theme` colour themes, schematic templates, and keymap
  presets into one versioned `.oplugin` archive or folder.
- **Level 1 — Sandboxed Wasm Component Model plugins (`Wasm`, WIT via `wasmtime`).** Portable,
  cross-platform community plugins for:
  - Schematic tools, generators, transforms and parametric layout helpers.
  - Custom electrical/design rule checks (ERC / lint findings) alongside built-in `omnia-circuit`
    and `omnia-sim` checks.
  - Importers, exporters, netlist formatters and bill-of-materials (BOM) generators.
  - Declarative dock panels, floating frames, inspector sections, menu/toolbar/status actions, and
    retained 2D vector overlays or custom 2D workbench viewports.
- **Level 2 — Out-of-process sidecar workbenches (`Sidecar`).** Crash-isolated companion
  executables speaking a versioned local IPC protocol plus a shared framebuffer / GPU surface
  presentation protocol, so heavy native or GPU workbenches (targeting `Reid-Engineered/Spectare`
  as an optional workspace plugin) can run inside Omnia's workspace tabs without coupling to
  Omnia's pinned Rust or GPUI revision.
- **Workbench / workspace shell host.** Mode/tab-hosted workbenches in `omnia-app` that own the
  central viewport and contribute their own toolbar, left/bottom docks, menus and status readouts
  while preserving Omnia's chrome, `omnia-theme` tokens (`INV6`) and document lifecycle.
- **Namespaced document persistence and reversible custom commands.** Opaque reverse-DNS extension
  blocks (`ext`) in `.omnia` (losslessly round-tripped when the plugin is absent), sibling/project
  sidecar files for large workbench payloads, reversible `Command::PluginPatch` inside `omnia-doc`,
  and an optional external undo-participant hook for Level 2 sidecar files.
- **Security, permissions and resource isolation.** Manifest-declared capability permissions with
  explicit user consent on install/upgrade, off-UI-thread Wasm execution with memory/epoch/timeout
  caps, and explicit user trust (warning that the binary runs with the user's full OS rights) plus
  directory-tree SHA-256 pinning (optional Ed25519 signatures) for Level 2 sidecars.
- **Packaging, scopes and developer SDK.** `.oplugin` ZIP archives and unpacked dev folders with
  `plugin.toml`, user-global and workspace-local install scopes, local and Git URL install, dev
  hot-reload, official WIT package, ergonomic Rust SDK (`omnia-plugin-sdk`) and CLI scaffold/pack
  subcommands.

Out of scope in v1:

- **In-app central registry / marketplace UI.** Deferred to a later milestone; `plugin.toml` and
  settings reserve optional metadata fields (`EQ5`) without shipping marketplace hosting or
  discovery UI in v1.
- **In-process native dynamic libraries (`cdylib` / Rust ABI).** Rejected (`ED2`): GPUI is pinned
  to one revision (`Cargo.toml`) and Rust has no stable ABI; in-process native crashes would
  destroy unsaved schematics.
- **Embedded webviews / HTML+Canvas UI.** Rejected (`ED4`): breaks native GPUI performance,
  accessibility (`UG9`), theme consistency (`INV6`) and memory budgets (`UB1`, `UB9`).
- **OS-level process sandboxing for Level 2 sidecars.** Deferred past v1 (`ED7`); Level 2 binaries
  run as trusted native executables with full OS user rights, gated by explicit user consent and
  SHA-256 pinning.
- **Custom simulation solver backends (`SimEngine`) and custom vector symbol artwork.** Deferred
  until `omnia-sim` (`S2.3`) and custom-symbol rendering (`PQ3`) land; the WIT world and capability
  table reserve slots for them (`EQ7`).
- **First-party non-Rust SDK crates.** Any language compiling to Wasm Component Model works through
  the published WIT package; official non-Rust starter templates follow after v1 stabilises (`ED10`).

## 2. Repository facts this plan rests on

Checked against `main` at `ad10f58`:

- **Workspace and toolchain.** Rust `1.98.1` (edition `2024`, resolver `3`), GPUI pinned to rev
  `15d136bf8e72ae43b284183ec46940c009460f5c` in root `Cargo.toml`.
- **Existing crates in `crates/`.** `omnia-doc`, `omnia-circuit`, `omnia-symbols`, `omnia-theme`,
  `omnia-canvas`, `omnia-app`, `omnia-agent`, `omnia-cli`, `omnia-bench`, plus `xtask` and
  `tests/golden`. Planned (not yet in `crates/`): `omnia-parts` (`parts-plan.md`) and `omnia-sim*`
  (`sim-plan.md`).
- **Strict lane boundaries (`AGENTS.md`, `INV12`, `AINV8`, `PINV1`).** `omnia-doc`,
  `omnia-circuit`, `omnia-agent`, `omnia-cli`, and planned `omnia-parts`/`omnia-sim*` have zero
  GPUI dependency. `omnia-theme` owns every colour (`INV6`, `UINV1`).
- **Document edit and file conventions.** Every schematic mutation goes through `Doc::apply`
  (`crates/omnia-doc/src/command.rs`), returning an atomic `Change` and self-contained inverse
  `UndoOp` (`INV1`, `INV4`). File-ownership order for `omnia-doc` (`XD7`) is `P2.1`–`P2.3` (RON v3
  + `EmbeddedPartDef`), then `C1.9` (lineage token), then `C1.1` (net delta in `Change`).
  `docs/file-format-proposal.md` (`FH0`) proposes a binary `.omnia` format with named sections.
- **Archive & hashing crates in `Cargo.lock`.** `toml` (`1`), `zip` (`9.6.0`), `sha2`, `serde`,
  and `ron` are already in `Cargo.lock` (currently pulled transitively or by existing crates) and
  pass `cargo deny check` (`deny.toml` allow-list: `MIT`, `Apache-2.0`, `BSD-2-Clause`,
  `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `MPL-2.0`).
- **GPUI external surface constraint (`G11`, `Q7`, `canvas-plan.md` sections 4 and 11).**
  `paint_surface` in the pinned GPUI revision is unavailable on Linux and Windows; `G11` states
  that direct native GPU surface embedding on Windows needs a patched GPUI fork (Plan C), and `Q7`
  (whether a patched GPUI fork is acceptable) has **no default: stop and ask**. Changing the GPUI
  pin is also its own task under `AGENTS.md`. Therefore the default Level 2 surface path is a
  triple-buffered shared-memory BGRA8 ring uploaded via GPUI's standard image/texture path (`ED4`,
  `ES2`).
- **GPUI chrome, canvas & frame isolation (`INV7`, `UG15`–`UG17`, `UB3`, `UB5`).** `SheetView`
  draws zero `Path` primitives (`INV7`), using quads/lines/sprites instead; `Window::focus` and
  `Div::on_click` trigger full cached-view refreshes; chrome controls use explicit
  `on_mouse_down`/`on_mouse_up` handlers and keep `SheetView` paint count at 0 during chrome
  interaction. Plugin UI widgets must use `omnia-app`'s `U1.3` primitives, and plugin 2D layers
  must use quad/line/glyph GPU primitives rather than per-frame path tessellation.
- **Gesture gate (`AINV13`, `A4.2`, `mcp-plan.md`).** In app mode, document revision never changes
  while a canvas drag/wire gesture is active; `A4.2` refuses writes with
  `user_gesture_in_progress`. Plugin write requests use the exact same refusal gate (`ED11`).

## 3. Budgets

All budgets are **PROPOSED** until `HE0`, and certified only on the Orion reference laptop (Linux,
Wayland, wgpu/Vulkan, 1920x1080 at 60 Hz, 100% and 125%, ten interleaved repetitions, canvas plan
section 2.1). Every existing canvas (`B1`–`B10`), wiring (`WB*`), UI (`UB1`–`UB10`), agent
(`AB1`–`AB4`), parts (`PB1`–`PB5`), context (`CB*`) and simulation (`SB*`) budget continues to hold
with plugins enabled.

| ID | Budget | Measured by |
| --- | --- | --- |
| EB1 | Cold start overhead. With 0 plugins installed, adding `omnia-plugin` adds ≤ 5 ms to `B1`/`UB1` (still < 400 ms). With 10 Level 0/Level 1 plugins installed (dormant until activated), startup discovery and manifest validation add ≤ 15 ms on the critical path to the first interactive frame (total `start --shell` < 400 ms) and ≤ 25 ms total background worker time. | `bench start --shell --plugins 0\|10` |
| EB2 | Archive and manifest scan. Opening, verifying and indexing one `.oplugin` archive (up to 16 MiB compressed, `EK3`) takes < 20 ms p99 off the UI thread; scanning 20 installed plugin manifests takes < 15 ms p99. | `cargo bench -p omnia-bench --bench plugin` |
| EB3 | Level 1 Wasm invocation. Calling an already-instantiated Wasm tool or ERC pass on a `1k` sheet: host↔guest marshalling + execution round-trip < 15 ms p99 for a reference BOM/ERC plugin; `Command::PluginPatch` apply on `10k` with 64-byte `ext` patch < 1 ms p99 (`Doc::apply` stays within `WB*`/`PB5`). | `cargo bench -p omnia-bench --bench plugin_wasm` |
| EB4 | UI isolation and hot-path allocations. With a Level 1 plugin panel open and a retained 2D overlay active on a `1k` sheet: `allocs_per_frame.ours == 0` during pan/zoom/hover (`UB5`, `INV7`), `B3` (< 16.7 ms p99) holds, and plugin UI hover/updates never repaint `SheetView` (`UB3`, `sheet_paints == 0`). | `bench ui --sheet 1k --script plugin-panel\|plugin-overlay` |
| EB5 | Retained 2D viewport. Panning and zooming a Level 1 plugin 2D workbench viewport holding 10,000 retained quad/line/glyph primitives meets `B3` (< 16.7 ms p99, 0 missed refreshes) with 0 guest Wasm calls during camera-only pan/zoom and 0 warm-frame allocations from our code. | `bench ui --script plugin-2d-pan\|plugin-2d-zoom` |
| EB6 | Level 2 framebuffer/surface presentation (**HYPOTHESIS**, settled by `ES2`). Presenting a 1280x800 60 Hz frame from a Level 2 sidecar workbench via the `HE1`-selected surface path adds ≤ 2.5 ms host compositing cost (p99), holds 60 fps (< 16.7 ms p99, 0 missed refreshes) on Orion with bounded GPU/image-cache memory over 60 s (`EB8`), pointer/key input round-trip ≤ 1 frame, and 0 frames drawn over 10 s when the sidecar submits no damage (`UB4`). | `bench ui --script plugin-sidecar-60hz\|plugin-sidecar-idle` |
| EB7 | Document persistence overhead. Saving and loading a `10k` document carrying 256 KiB of opaque `ext` metadata across 4 plugin namespaces adds ≤ 10% to RON `save`/`load` time over v3 and keeps `UB10` (< 33.3 ms p99 UI-thread serialise) green. | `cargo bench -p omnia-bench --bench plugin_doc` |
| EB8 | Memory overhead. With `wasmtime` engine initialised and 5 Level 1 Wasm plugins loaded (1 active workbench), total RSS stays within `B9` (< 300 MB for one `10k` doc) and `UB9` (< 450 MB for two `10k` docs); each dormant Wasm plugin consumes ≤ 4 MiB RSS; sustained 60 Hz Level 2 frame uploads exhibit 0 monotonic RSS/cache growth over 60 s. | `memory_mb` in `bench ui --sheet 10k --plugins 5` and `ES2` |
| EB9 | Runaway & crash containment. A Wasm plugin hitting an infinite loop, memory exhaustion or trap is terminated within `EK5` (250 ms for interactive hooks, 5 s for commands) with 0 UI-thread frames > 50 ms (`B5`), 0 leaked threads, and `Doc` unchanged (`EINV3`). A Level 2 sidecar `SIGKILL`/crash is detected within 200 ms, leaves Omnia responsive, and preserves all open schematics untouched. | Fault-injection tests `ETC-wasm-trap`, `ETC-wasm-timeout`, `ETC-sidecar-workbench` |

## 4. Constants

All constants are **PROPOSED** until `HE0`.

| ID | Constant | Value |
| --- | --- | --- |
| EK1 | Plugin manifest schema version | `schema = 1` in `plugin.toml`. Unknown versions are refused with a clear diagnostic. |
| EK2 | Canonical `plugin_id` grammar | Reverse-DNS ASCII lowercase: `^[a-z][a-z0-9-]{0,31}(\.[a-z][a-z0-9-]{0,31}){1,5}$`, total length 3..=128 chars (for example `org.reid-engineered.spectare`, `example.omnia.bom-csv`). Reserved prefixes `omnia.*` and `org.omnia.*` are disallowed for non-bundled plugins unless explicitly overridden in dev mode; example/test fixtures use `example.omnia.*`. |
| EK3 | `.oplugin` archive safety limits | Standard ZIP (Deflate or Stored only, mirroring `PK3`). Level 0 / Level 1 max compressed size: 16 MiB; max uncompressed total: 64 MiB; max entry count: 1,024; max single entry uncompressed: 32 MiB; max compression ratio: 100:1; max path depth: 8; relative UTF-8 paths only (no `..`, no leading `/`, no symlinks, no backslashes). Level 2 sidecar archives allow up to 128 MiB compressed / 512 MiB uncompressed for native binaries, unpacked into the content-addressed cache only after explicit user trust (`ED7`). |
| EK4 | Capability vocabulary | Finite enum declared in `[permissions]` of `plugin.toml`: `bundle.parts`, `bundle.themes`, `bundle.templates`, `bundle.keymaps`, `doc.read`, `doc.write`, `doc.ext.write` (scoped automatically to the plugin's own `plugin_id` namespace), `circuit.read`, `erc.contribute`, `sim.read`, `ui.actions`, `ui.panel`, `ui.overlay`, `ui.workbench`, `fs.workspace.read`, `fs.workspace.write`, `fs.export` (host file-picker only), `net.hosts` (explicit HTTPS allow-list), `sidecar.exec` (Level 2 only), `sidecar.surface` (Level 2 only). For Level 1 (Wasm), every capability is strictly sandbox-enforced; for Level 2 (Sidecar), capabilities gate host IPC APIs only while the native process runs with full OS user rights (`ED7`, `EINV8`). |
| EK5 | Level 1 Wasm sandbox resource caps | Per-instance linear memory cap: 64 MiB default (manifest may request up to 256 MiB with user consent). Stack cap: 1 MiB. Table elements cap: 10,000. CPU interruption via `wasmtime` epoch tick (10 ms granularity): interactive UI/overlay/ERC hook deadline 250 ms; user-invoked command/export deadline 5,000 ms (cancellable via progress toast). Max concurrent worker calls per plugin: 1 (serialised per plugin actor, `ED2`). |
| EK6 | `.omnia` `ext` metadata limits | Extension map keyed by validated `EK2` `PluginNamespace` (document-level and optional item-level sub-map keyed by `ItemId`). Max distinct plugin namespaces per document: 32. Max total serialised `ext` bytes per document: 1 MiB (larger workbench artifacts must use sibling/project sidecar files, `ED5`). Per-entry `schema_version: u32` + `ExtPayload` (max 256 KiB per namespace). |
| EK7 | Document RON & binary section versions | RON `version: 4` adds top-level `ext` (document-level and item-level namespaced blocks) after `P2.3`'s `version: 3`. `Doc::load` reads v2, v3 and v4 and refuses v1 and v5+ (`E2.1` updates `parts-plan.md` `PK9` / `PTC-version-refuse`, `omnia-agent/src/files.rs` and `ui-plan.md` `UTC-open-bad` accordingly). Per `EQ3`, `Doc::save` writes v3 when `ext` is empty and v4 when any `ext` block is present. Binary `.omnia` (`docs/file-format-proposal.md`) reserves optional section kind `EXT_DATA` (not marked must-understand; note that an older binary reader that skips unknown sections would drop `EXT_DATA` if it re-saves the file, whereas readers supporting `EXT_DATA` preserve unknown plugin namespaces losslessly). |
| EK8 | Declarative UI widget tree caps | Max nodes per panel/workbench chrome render: 4,096. Max nesting depth: 32. Max text string length per node: 8 KiB. Colours are referenced strictly by `omnia-theme` semantic token enum (zero raw RGB/hex literals from plugins, `INV6`). |
| EK9 | Retained 2D draw-list caps | Max retained layers per viewport/overlay: 32. Max primitives per layer: 50,000 (100,000 total per viewport). Primitive set: rects, axis-aligned/angled line segments, polylines, circles/rings (instanced quads/lines), and glyph/text runs (matching `INV7`: no per-frame GPUI `Path` tessellation). Coordinate space: `f64` world/sheet units (nm on schematic overlays; plugin-defined logical units on standalone 2D workbenches), mapped on the host GPU thread without calling Wasm during pan/zoom (`EB5`). |
| EK10 | Level 2 IPC & surface limits | Local transport: UNIX domain socket (`$XDG_RUNTIME_DIR/omnia/plugin-<pid>-<n>.sock` on Linux, mode `0600`) or pre-connected inherited socketpair, and named pipe (`\\.\pipe\omnia-plugin-<pid>-<n>` on Windows, owner-only DACL). Every connection authenticates with a per-launch CSPRNG 256-bit one-time bearer token passed to the child via an inherited stdin/pipe handshake before any RPC message is accepted. Framing: length-prefixed binary frames (max 4 MiB per control message; bulk framebuffers over shared memory). Handshake timeout: 3,000 ms; heartbeat interval: 2,000 ms; missed-heartbeat disconnect: 6,000 ms. Max framebuffer dimension: 8192x8192, BGRA8/RGBA8, triple-buffered ring. |
| EK11 | Install & discovery directories | Mirrors `UD10` / `PQ4`. User-global: `%APPDATA%\Omnia\plugins\` (Windows) and `$XDG_CONFIG_HOME/omnia/plugins/` (Linux). Workspace-local: `<workspace>/.omnia/plugins/` (never auto-executed without workspace trust approval, `ED7`). Trust & consent store: `<config_dir>/plugin-trust.ron`. Content-addressed Level 2 trusted binary cache: `<cache_dir>/omnia/trusted-bin/<sha256>/`. Compiled Wasm cache (`EQ1`): `<cache_dir>/omnia/wasm-cache/`. Dev links (unpacked folders): listed in `prefs.ron` under `plugin_dev_dirs`. |
| EK12 | Host API & WIT versioning | WIT package `omnia:plugin@0.1.0` (semver). Host advertises a supported major/minor range; plugins declare `api = "^0.1"` in `plugin.toml`. Incompatible API versions are refused at load with an actionable message; minor additions are backwards-compatible. |

## 5. Architecture

```
                     .oplugin (ZIP) / unpacked folder (plugin.toml)
                                          │
                                          ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                     omnia-plugin (GPUI-free core crate)                      │
│  manifest.rs (EK1-EK4) · archive.rs (EK3) · trust.rs (ED7) · registry.rs    │
│  tier0.rs (bundles) · erc.rs · service.rs · ui_ir.rs · scene2d.rs           │
└──────────────┬──────────────────────────┬────────────────────────┬───────────┘
               │                          │                        │
               ▼                          ▼                        ▼
   ┌───────────────────────┐  ┌───────────────────────┐  ┌─────────────────────┐
   │ Level 0 (Bundle)      │  │ omnia-plugin-wasm     │  │ omnia-plugin-ipc    │
   │ (no code execution)   │  │ (GPUI-free, Level 1)  │  │ (GPUI-free, Level 2)│
   │ · omnia-parts (.olib) │  │ · wasmtime Component  │  │ · UDS / Named Pipe  │
   │ · omnia-theme         │  │ · WIT host bindings   │  │ · bearer-token auth │
   │ · templates/keymaps   │  │ · per-plugin actor    │  │ · process supervisor│
   └───────────────────────┘  │ · epoch & mem caps    │  │ · shm/surface ring  │
                              └───────────┬───────────┘  └──────────┬──────────┘
                                          │                         │
                                          └───────────┬─────────────┘
                                                      ▼
         ┌────────────────────────────────────────────────────────────────┐
         │               Omnia Core (GPUI-free lanes)                     │
         │  omnia-doc (DocExt, Command::PluginPatch, reversible undo)     │
         │  omnia-circuit (read views, custom ERC finding merge)          │
         │  omnia-parts / omnia-sim (read-only queries & results stream)  │
         └────────────────────────────────┬───────────────────────────────┘
                                          │
                                          ▼
         ┌────────────────────────────────────────────────────────────────┐
         │               omnia-app / omnia-canvas (GPUI)                  │
         │  · WorkbenchHost (mode/tab switcher, central viewport host)    │
         │  · PluginUiRenderer (EK8 declarative widgets -> omnia-theme)   │
         │  · Retained2dCanvas (EK9 quad/line/glyph host pan/zoom)        │
         │  · ExternalSurfaceView (EK10 shm/surface presenter, Level 2)   │
         │  · Consent dialog, Plugin Manager, Gesture gate (AINV13/A4.2)  │
         └────────────────────────────────────────────────────────────────┘
```

Key architectural boundaries:

- **Zero GPUI in plugin core and runtimes (`EINV1`).** `omnia-plugin`, `omnia-plugin-wasm`,
  `omnia-plugin-ipc`, and `omnia-plugin-sdk` never depend on GPUI or `omnia-app`. This enables
  headless CLI execution (`omnia plugin ...`), headless CI testing of every Level 0/1/2 protocol
  path, and clean reuse by `omnia-cli` and `omnia-bench`.
- **Zero plugin runtime in `omnia-doc` (`EINV2`).** `omnia-doc` gains only pure data types
  (`PluginNamespace`, `ExtPayload`, `ExtEntry`, `ExtTarget`, `ExtOp`, `DocExt`,
  `Command::PluginPatch`) with no dependency on `omnia-plugin*`, `wasmtime`, `toml`, `zip`, `sha2`,
  or GPUI.
- **UI thread never blocks on plugin code (`EINV6`).** Every Wasm invocation and every Level 2 IPC
  exchange runs on background worker threads/executors. GPUI receives immutable snapshots of
  declarative UI trees, retained 2D layers, or completed framebuffer/surface handles over bounded
  channels, preserving `UB3`, `UB4`, `UB5`, `B3` and `B4`.

## 6. Decisions

### ED1 Scope: community bundles, schematic tools/transforms and full workbenches. DECIDED (owner interview 2026-11-25)

Plugins cover three complementary roles without duplicating `.opart`/`.olib` or MCP:

1. **Content bundles (Level 0 — Bundle):** bundled part libraries (`.olib`/`.opart`), colour
   themes, schematic templates, and keymap presets.
2. **Schematic tools & transforms (Level 1 — Wasm):** user-invoked commands, circuit generators,
   custom ERC/lint rules, importers, exporters, and BOM/report generators.
3. **Workbenches / workspaces (Level 1 — Wasm and Level 2 — Sidecar):** optional full modes/tabs
   inside Omnia that provide new engineering workflows on top of or beside a schematic — explicitly
   targeting `Reid-Engineered/Spectare` as a flagship optional workspace plugin.

### ED2 Hybrid three-level execution runtime. DECIDED (owner interview 2026-11-25)

- **Level 0 (Bundle — Declarative):** pure data validated against strict Rust schemas. Zero code
  execution, instant load, safe by default.
- **Level 1 (Wasm — WebAssembly Component Model / WIT via `wasmtime`):** portable single-binary
  `.wasm` components compiled from Rust or any WIT-capable language. Memory-safe, OS-sandboxed,
  cross-platform (Linux + Windows with one artifact), ABI-stable across Rust and GPUI upgrades.
  Each enabled Wasm plugin runs inside its own single-threaded worker actor (serialising at most
  one in-flight invocation per plugin at a time, `EK5`), preventing re-entrancy races and bounding
  thread usage.
- **Level 2 (Sidecar — Out-of-process executable over local IPC):** native companion binaries
  launched and supervised by `omnia-plugin-ipc`. Crash-isolated (a sidecar panic or segfault never
  takes down Omnia or loses unsaved schematics), language- and GPU-toolchain-independent, and
  completely decoupled from Omnia's pinned Rust/GPUI revision.

In-process native dynamic libraries (`cdylib`) are explicitly rejected.

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

### ED4 Rendering contract: declarative themed UI, retained 2D draw-lists and Level 2 surfaces. DECIDED (owner interview 2026-11-25)

Embedded webviews / HTML+Canvas UI are explicitly rejected: they violate native GPUI performance,
AccessKit accessibility (`UG9`), `omnia-theme` token discipline (`INV6`) and memory budgets
(`UB1`, `UB9`). Instead:

1. **Chrome, panels, forms and inspectors (Level 1 & Level 2):** plugins emit an immutable
   declarative UI tree (`UiNode`, `EK8`) composed of Omnia's themed widgets (layout stacks,
   scroll areas, labels, mono readouts, buttons, toggles, text/numeric inputs, selects, tables,
   trees, badges, sparklines/plots). `omnia-app` diffs and renders them natively with GPUI `U1.3`
   widgets, guaranteeing native performance, AccessKit accessibility (`UG9`), keyboard focus
   (`UG8`), `UB3` isolation, and `INV6`.
2. **Custom 2D viewports and schematic overlays (Level 1):** plugins submit retained 2D scene
   layers (`Scene2dLayer`, `EK9`: rects, lines, polylines, circles/rings, glyph runs, grid/axis
   hints, semantic colour tokens, hit-test tags). Omnia's host canvas renders them using the same
   quad/line/glyph GPU pipeline (`INV7`, no per-frame GPUI `Path` tessellation) and handles 60 fps
   pan/zoom, culling and hit-testing without calling Wasm per frame (`EB5`), sending only semantic
   pointer/key events back to the plugin when needed.
3. **Heavy / GPU workbench viewports (Level 2):** in addition to declarative chrome/docks, a
   Level 2 sidecar presents rendered frames into the GPUI workbench viewport over a versioned
   surface protocol (`EK10`). Because `paint_surface` in pinned GPUI is unavailable on Linux and
   Windows (`G11`) and a patched GPUI fork has **no default** (`Q7`: stop and ask), the **default
   path** is a triple-buffered shared-memory BGRA8 ring uploaded to a GPUI texture/image per
   damaged frame. Spike `ES2` benchmarks this path (and probes whether zero-copy native OS GPU
   texture sharing is viable without a GPUI fork); choosing a patched GPUI fork at `HE1` requires
   an explicit owner answer to `Q7` first.

### ED5 Persistence and document portability: namespaced `.omnia` `ext` blocks + sidecar files. DECIDED (owner interview 2026-11-25)

- **Small document/item metadata:** `.omnia` gains optional reverse-DNS namespaced extension blocks
  (`DocExt`, keyed by `PluginNamespace`, `EK6`) at document level and optionally attached to
  `PartId` / `WireId` / `LabelId`. `omnia-doc` stores them as canonical opaque `ExtEntry` values
  (`schema_version: u32` + `ExtPayload`) and round-trips them losslessly even when the owning
  plugin is absent (`EINV4`). When a referenced item is deleted, its attached item-level `ext`
  entries are removed with it atomically and restored on undo (`EINV12`).
- **Large workbench payloads:** heavy workbench data (such as 3D meshes, layout/routing databases,
  or large spectrum/capture artifacts) live in sibling or workspace/project sidecar files managed
  through `fs.workspace.*` capabilities, referenced by relative path/hash from the small `.omnia`
  `ext` block so `.omnia` stays fast and lean (`EB7`, `UB9`, `UB10`).
- **Missing-plugin UX:** opening a `.omnia` document whose `ext` blocks reference an uninstalled
  plugin succeeds normally, preserves all `ext` entries untouched across edits and saves, and shows
  a single dismissible non-blocking toast/banner listing the missing `plugin_id`(s).

### ED6 Hybrid commands and transactional undo. DECIDED (owner interview 2026-11-25)

- **Inside `omnia-doc` (`Command::PluginPatch`):** plugins mutate the schematic and/or their own
  `ext` namespace via one atomic `Command::PluginPatch`:
  ```rust
  Command::PluginPatch {
      namespace: PluginNamespace,
      label: Arc<str>,               // human-readable undo label, e.g. "Generate Ladder"
      schematic_ops: Vec<Command>,   // standard omnia-doc commands (never nested PluginPatch)
      ext_ops: Vec<ExtOp>,           // SetDoc / RemoveDoc / SetItem / RemoveItem
  }
  ```
  `Doc::apply` executes `schematic_ops` through the same transactional batch engine as
  `Command::Batch` (refusing nested `PluginPatch`), then resolves and applies `ext_ops` (supporting
  both `ExtTarget::Existing(ItemId)` and `ExtTarget::Created { op_index }` for items created in
  the same patch), checks `EK6` size caps, computes exact prior `ext` values and schematic
  inverses, and records one self-contained `UndoOp` in `omnia-doc`'s undo stack. If any
  sub-operation fails, nothing changes (`INV4`, `EINV3`). An `ext`-only `PluginPatch` still
  advances `Doc::revision` by 1 (preserving `expected_revision` guards) and sets
  `Change::ext_changed = true` with empty schematic deltas so `SheetView` and `omnia-circuit` skip
  unnecessary repaints/rebuilds (`UB3`). Because the inverse is pure data inside `omnia-doc`,
  **Ctrl+Z / Ctrl+Y always succeed deterministically even if the plugin crashes, unloads, or is
  uninstalled (`EINV5`).**
- **Level 2 sidecar undo participant:** a Level 2 workbench that owns external sidecar files may
  additionally register an `UndoParticipant` channel with the workspace history coordinator in
  `omnia-app`. When a workbench-only edit happens in the sidecar, the workspace records a paired
  history entry carrying an opaque `undo_token` and calls `undo(token)` / `redo(token)` over IPC;
  if the sidecar is dead or unloaded, `omnia-app` surfaces a clear non-destructive toast and leaves
  schematic undo history intact (`EINV5`, `EINV14`).

### ED7 Capability security, user consent and Level 2 trust. DECIDED (owner interview 2026-11-25)

- **Default deny:** every plugin starts with zero capabilities (`EK4`). Capabilities are declared
  statically in `plugin.toml` and inspected before any code loads.
- **Level 0 (Bundle):** needs only `bundle.*` capabilities; no code runs, so enabling a
  user-installed bundle requires only schema/archive validation (`EK3`).
- **Level 1 (Wasm — sandbox-enforced):** on first enable (or whenever an upgrade requests a new
  capability), Omnia shows a concise capability consent dialog listing the requested permissions in
  plain English and records the user's decision in `plugin-trust.ron` (`EK11`). Wasm runs off the
  UI thread with `EK5` memory and epoch caps; WASI grants zero ambient filesystem, environment, or
  socket access, exposing only approved workspace/export handles and `net.hosts` HTTPS allow-listed
  requests.
- **Level 2 (Sidecar — trusted native executable, not OS-sandboxed):** because a Level 2 sidecar
  is a native OS process running with the user's full OS privileges, manifest capabilities control
  *only* which Omnia Host IPC APIs are exposed to it. Before launching a Level 2 sidecar for the
  first time (or after any change), Omnia requires an explicit elevated **"Trust Native
  Executable"** consent dialog that clearly states the binary runs with the user's full OS user
  rights and displays the SHA-256 digest of the entire `bin/<os>-<arch>/` directory tree (including
  any bundled shared libraries). Upon approval, Omnia copies the verified directory into the
  read-only content-addressed cache `<cache_dir>/omnia/trusted-bin/<sha256>/` (`EK11`), pins the
  tree hash in `plugin-trust.ron`, and spawns strictly from that verified copy (preventing
  check-to-launch TOCTOU races). Optional Ed25519 publisher signatures are reserved (`EQ4`).
- **Workspace-local plugins (`<workspace>/.omnia/plugins/`):** discovered as **disabled** until the
  user explicitly trusts that workspace/plugin, preventing drive-by code execution when opening a
  downloaded project folder (`EINV8`).

### ED8 Packaging, install scopes and developer workflow. DECIDED (owner interview 2026-11-25)

- **Identical inner layout for `.oplugin` ZIP and unpacked directories** (mirroring `.opart`/`.olib`
  in `parts-plan.md` `PD2`):
  ```text
  my-plugin.oplugin (or unpacked folder)
    plugin.toml          # manifest: id, version, api, level, permissions, contributions
    LICENSE              # SPDX licence text
    README.md            # optional user-facing summary
    assets/              # optional Level 0: parts/*.olib, themes/*.toml, templates/*.omnia, keymaps/*.toml
    wasm/plugin.wasm     # Level 1 Wasm Component Model binary
    bin/<os>-<arch>/...  # Level 2 sidecar executable + any bundled shared libraries
  ```
- **Install scopes:** user-global and workspace-local (`EK11`). When the same `plugin_id` exists in
  both, workspace-local shadows user-global for that workspace only (with explicit indicator in
  Plugin Manager).
- **v1 install sources:** local `.oplugin` file, local unpacked dev folder (with live file-watch
  hot-reload for Level 0 & Level 1), and Git repository URL (via `omnia plugin install` invoking
  system `git` pinned to a commit/tag, with `--` argument separation and a clear error if `git` is
  absent).
- **In-app registry/marketplace:** deferred past v1; `plugin.toml` reserves optional descriptive
  metadata (`EQ5`).

### ED9 Phased milestone delivery. DECIDED (owner interview 2026-11-25)

Build strictly bottom-up so each milestone ships verifiable value and respects cross-plan
dependencies:

- **Milestone E0:** Plan, owner sign-off (`HE0`), parallel spikes `ES1` (`wasmtime` + `deny.toml`
  + cold-start/RSS in a standalone out-of-workspace crate) and `ES2` (GPUI shared-memory ring vs
  native surface probe on Linux Wayland + Windows), converging at `HE1` (`E0.5`).
- **Milestone E1:** GPUI-free `omnia-plugin` core host, `.oplugin` archive/manifest validator,
  trust store, CLI `omnia plugin` basics, and full **Level 0** content bundles (`.olib`, themes,
  templates, keymaps).
- **Milestone E2:** `omnia-doc` `DocExt` + `Command::PluginPatch` + RON v4 (after `P2.3`, `C1.9`,
  `C1.1`), `omnia-plugin-wasm` runtime, WIT host API (`omnia:plugin@0.1.0`), custom ERC/lint &
  exporters, and `omnia-plugin-sdk` + `omnia plugin new/run`.
- **Milestone E3:** Declarative UI widget renderer (`omnia-theme`), retained 2D viewport &
  schematic overlays, workbench tab/mode host in `omnia-app`, gesture refusal gate (`A4.2`), dev
  hot-reload, and Plugin Manager + consent UI (`HE2`).
- **Milestone E4:** `omnia-plugin-ipc` sidecar supervisor, Level 2 framebuffer/surface presenter,
  input forwarding, external undo participant, `Reid-Engineered/Spectare`-target reference sidecar
  workbench, and Orion reference-machine gate (`HE3`).

### ED10 Authoring SDK and v1 tooling. DECIDED (owner interview 2026-11-25)

Omnia ships in v1:
1. The canonical versioned WIT interface package under `wit/omnia-plugin/` (`omnia:plugin@0.1.0`).
2. An ergonomic, MIT/Apache-2.0 Rust SDK crate `crates/omnia-plugin-sdk/` wrapping `wit-bindgen`
   with builders for `PluginPatch`, `UiNode`, `Scene2dLayer`, ERC findings and typed error helpers,
   plus an optional `sidecar` feature exposing the matching Rust IPC server helper for Level 2
   workbenches (such as `Reid-Engineered/Spectare`).
3. CLI subcommands in `omnia-cli`: `omnia plugin verify|pack|install|list|enable|disable|remove`
   (Milestone E1), followed by `omnia plugin new <name>` and headless
   `omnia plugin run <id> <command> --doc <file>` (Milestone E2).

### ED11 Unified Host API contract and gesture safety. PROPOSED

- Level 1 (WIT) and Level 2 (binary IPC) expose the **same logical service model** (`doc`,
  `circuit`, `parts`, `sim`, `erc`, `ui`, `viewport`, `workspace`, `host`), defined once in
  `omnia-plugin` IR types and projected to WIT (`omnia-plugin-wasm`) and framed RPC
  (`omnia-plugin-ipc`).
- Every write request carries `expected_revision: u64` (matching `mcp-plan.md` `AD4`). If the
  document changed concurrently, the host refuses with `StaleRevision` so the plugin re-reads and
  retries cleanly.
- **Gesture gate (`AINV13`, `A4.2`):** in app mode, while a canvas drag, move or wire gesture is in
  progress, every incoming plugin `PluginPatch` write is **refused immediately** with
  `user_gesture_in_progress` through the same gate `A4.2` builds (never queued across a revision
  advance and never mutating the document mid-gesture, `EINV11`).
- **Untrusted text hygiene (`AD14`):** plugin-supplied display strings (labels, diagnostics, toast
  text) are sanitised for control characters, length-capped (`EK8`), and never concatenated into
  SPICE decks (`PK10`) or shell commands.

### ED12 Determinism and conflict resolution. PROPOSED

- Installed plugins load and execute hooks (ERC passes, exporters, menu registration) in strict
  lexicographical `plugin_id` order — never filesystem enumeration order or hash-map order
  (`EINV9`).
- **Wasm execution determinism (`E2.2`):** `wasmtime` compiles with canonical NaN handling
  (`cranelift_nan_canonicalization(true)`), and WASI wall-clock / monotonic-clock / random
  interfaces are disabled (returning `PermissionDenied` / deterministic stubs) during ERC passes
  and deterministic exporters so repeated runs over the same `Doc` produce byte-identical outputs.
- **Conflict rules:**
  - Duplicate `plugin_id` within the same scope: refused with a diagnostic; workspace-local shadows
    user-global across scopes (`ED8`).
  - Duplicate workbench id, action id or ERC rule code: always namespaced as
    `<plugin_id>:<local_id>`, making cross-plugin collisions impossible by construction.
  - Keymap presets (Level 0): built-in bindings win on conflict unless the user explicitly selects
    a plugin keymap preset in Preferences; conflicting plugin bindings are logged and skipped
    deterministically.
  - Themes (Level 0): registered by `<plugin_id>:<theme_name>`; cannot overwrite built-in themes.

### ED13 Crash, timeout and missing-plugin degradation. PROPOSED

- **Level 1 fault quarantine:** if a Wasm plugin traps, runs out of memory, or exceeds its `EK5`
  deadline, its instance is dropped immediately, pending UI/overlay contributions from that plugin
  are cleared, a non-blocking toast explains the failure, and the plugin transitions to `Faulted`
  state (restarted only on explicit user reload/retry, never in a tight crash loop).
- **Level 2 crash recovery:** if a sidecar process exits unexpectedly or misses heartbeats
  (`EK10`), the workbench viewport displays a themed "Workbench disconnected" recovery state with a
  one-click **Restart** button and diagnostic log tail. All schematics and other tabs remain
  completely unaffected.
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
| EINV6 | No Wasm execution or blocking Level 2 IPC ever runs on the GPUI UI thread; `SheetView` paint count stays 0 during plugin chrome/panel interaction (`UB3`, `EB4`). | Worker-thread assertion + `bench ui` test (`E2.2`, `E3.3`, `E4.2`) |
| EINV7 | Plugin declarative UI and retained 2D draw-lists contain zero raw colour literals; every rendered colour resolves through `omnia-theme` tokens (`INV6`, `UINV1`). | Type-level token enum + theme-switch test (`E3.1`, `E3.2`) |
| EINV8 | No Level 1 (Wasm) plugin executes code, accesses files or opens network requests without manifest declaration (`EK4`) and persisted user consent (`ED7`). Workspace-local plugins start disabled. No Level 2 (Sidecar) binary spawns without explicit "Trust Native Executable" consent and a matching `bin/<os>-<arch>/` directory-tree SHA-256 launched from the immutable trusted cache (`ED7`), and undeclared host IPC APIs are refused. | Security/consent test suite (`E1.2`, `E2.2`, `E4.1`) |
| EINV9 | Plugin discovery, manifest indexing, ERC execution order, Wasm ERC/exporter output (`ED12`), `ext` serialisation order and `.oplugin` packing are 100% deterministic across filesystem/ZIP/hash-map orderings. | Shuffled-order property tests (`E1.1`, `E1.2`, `E2.1`, `E2.3`) |
| EINV10 | Hostile `.oplugin` archives (path traversal, ZIP bombs, symlinks, invalid UTF-8, oversized entries, bad `plugin_id`) are refused before allocating past `EK3`. | Hostile-archive fuzz/unit tests (`E1.1`) |
| EINV11 | In app mode, plugin writes while a canvas gesture is active are refused immediately with `user_gesture_in_progress`, leaving `Doc` and `revision` unchanged (extends `AINV13`, `A4.2`). | App-host gesture test (`E3.4`) |
| EINV12 | Deleting a schematic item removes its attached item-level `ext` entries atomically, and undoing the delete restores them byte-identically. | `omnia-doc` command property test (`E2.1`) |
| EINV13 | Plugin-contributed ERC findings are tagged with `<plugin_id>:<rule_id>` and never suppress or mutate built-in `omnia-circuit` or `omnia-sim` findings. | ERC merge test (`E2.3`) |
| EINV14 | A Level 2 sidecar crash, hang, unauthenticated IPC connection, malformed IPC frame or oversized surface descriptor never panics Omnia, never leaks shared-memory/socket handles, and leaves all open schematics intact (`EB9`, `EK10`). | Fault-injection + fuzz tests (`E4.1`, `E4.2`) |

## 8. Scenarios

Each scenario is an automated test with a concrete expected result.

| ID | Setup & Action | Expected result |
| --- | --- | --- |
| ETC-bundle-load | Install a `.oplugin` Level 0 bundle (`example.omnia.starter-bundle`) containing a 3-part `.olib`, a dark/light theme, a schematic template and a keymap preset; enable it | Parts appear in `omnia-parts::Catalog`, theme switches cleanly without `INV6` violations, template opens in New from Template, keymap preset is selectable; disabling removes all four cleanly (`E1.3`). |
| ETC-hostile-oplugin | Feed `omnia-plugin` a `.oplugin` with `../escape.wasm`, a 150:1 zero-filled ZIP bomb, a symlink entry, an invalid `plugin_id`, and an unknown `schema = 99` | All five are refused with specific `PluginError` variants before allocating past `EK3` (`EINV10`). |
| ETC-consent-gate | Place a Level 1 Wasm plugin in `<workspace>/.omnia/plugins/` requesting `doc.write` and `net.hosts = ["example.org"]`; open workspace | Plugin loads in `Untrusted/Disabled` state; zero Wasm code runs; enabling prompts for `doc.write` + `net.hosts`; upgrading the manifest to add `fs.workspace.write` revokes auto-start until re-approved (`EINV8`). |
| ETC-patch-atomic | Wasm plugin returns a `Command::PluginPatch` placing 3 parts, adding 2 wires, setting doc `ext`, and including 1 invalid out-of-bounds wire | `Doc::apply` refuses the entire patch with `CommandError::OutOfBounds`; `Doc` (parts, wires, `ext`, `revision`, undo stack) is strictly `==` to before (`EINV3`). |
| ETC-patch-offline-undo | Wasm plugin applies a `Command::PluginPatch` placing a part, attaching item `ext` to it via `ExtTarget::Created { op_index: 0 }`, and setting doc `ext`; unload and delete the plugin; press Undo then Redo | Undo removes the part and restores prior `ext` cleanly; Redo re-applies both identically; no plugin callback is invoked (`EINV5`). |
| ETC-ext-missing-plugin | Open a RON v4 `.omnia` containing `ext["org.example.layout"]` on a machine without that plugin; move an unrelated resistor; delete a part carrying item `ext`, undo, then save | Non-blocking missing-plugin banner appears; saved file preserves `ext["org.example.layout"]` byte-identically (`EINV4`, `EINV12`). |
| ETC-wasm-erc | Enable `example.omnia.sample-erc` checking resistor power/label naming on a `1k` sheet | Findings appear tagged `example.omnia.sample-erc:*` in deterministic order alongside built-in findings without hiding any built-in finding (`EINV9`, `EINV13`). |
| ETC-wasm-bom-export | Invoke `example.omnia.bom-csv` via CLI (`omnia plugin run`) and UI menu | Deterministic UTF-8 CSV written via host `fs.export` picker/path; plugin cannot read arbitrary host paths outside approved scope (`EINV8`, `EINV9`). |
| ETC-wasm-trap | Run a Wasm plugin that divides by zero / executes `unreachable` inside a tool call | Worker catches trap, quarantines plugin to `Faulted`, emits toast, `Doc` unchanged, UI never stalls > 50 ms (`EB9`, `EINV3`). |
| ETC-wasm-timeout | Run a Wasm plugin with an infinite loop in both an ERC hook and a user command | Epoch interruption terminates the hook ≤ 250 ms and the command ≤ 5 s; worker thread cleanly recovered; UI stays responsive (`EB9`). |
| ETC-gesture-refuse | Start a canvas part drag (`SheetCanvas` gesture active) while a background Wasm plugin submits a `PluginPatch` | Write is refused immediately with `user_gesture_in_progress`; `Doc` and `revision` do not change during the drag (`AINV13`, `A4.2`, `EINV11`). |
| ETC-ui-panel-theme | Render a Level 1 declarative panel with tables, inputs and buttons; toggle light/dark theme; hover across the panel | All widgets re-skin via `omnia-theme` tokens (`EINV7`); `SheetView` paint count stays 0 (`UB3`, `EB4`); keyboard tab navigation reaches controls (`UG8`). |
| ETC-2d-viewport-pan | Open a Level 1 2D workbench tab with 10,000 retained quad/line/glyph primitives; run 600-frame pan/zoom script | Pan/zoom runs at < 16.7 ms p99 on host GPU with 0 guest Wasm invocations and 0 warm-frame allocations from our code (`EB5`, `INV7`). |
| ETC-dev-hot-reload | Edit and rebuild an unpacked dev Level 1 plugin while Omnia is running | Host detects change, re-validates manifest (refusing silent new permissions), reloads Wasm instance, refreshes panel/overlay without restarting Omnia (`E3.4`). |
| ETC-sidecar-trust | Register a Level 2 sidecar plugin, approve "Trust Native Executable" (pinning the `bin/<os>-<arch>/` tree SHA-256), then flip 1 byte in either the binary or a bundled library and relaunch; also attempt an IPC connection without the one-time bearer token | Modified tree is refused with `HashMismatch` before spawning; unauthenticated IPC connection is rejected immediately (`EINV8`, `EINV14`). |
| ETC-sidecar-workbench | Launch a reference Level 2 sidecar workbench (modeling `Reid-Engineered/Spectare`), switch tabs between Schematic and Workbench, forward mouse/key events, send a sidecar edit + undo, then `SIGKILL` the sidecar | Workbench presents 60 Hz frames (`EB6`), forwards input and paired undo cleanly, and after `SIGKILL` displays the themed recovery banner within 200 ms with the schematic tab and undo stack intact (`EB9`, `EINV14`). |

## 9. Spikes

Two independent spikes in Milestone E0 settle every high-risk technical unknown before production
code is written. Both run in parallel after `HE0` and converge at `HE1` (`E0.5`).

| ID | Question it settles | How |
| --- | --- | --- |
| ES1 | Does `wasmtime` (Component Model enabled, Cranelift only, minimal features) pass `cargo deny check` or require a specific SPDX exception in [deny.toml](deny.toml) (such as `Apache-2.0 WITH LLVM-exception`)? What are its compile time, binary size, cold init (`EB1`), per-instance RSS (`EB8`), and epoch interruption latency (`EB9`)? | Build a standalone out-of-workspace spike package at `docs/measurements/E0.3/spike/` (mirroring `P0.3`, so root `Cargo.toml` and root `cargo deny check` stay untouched before `HE1`). Run `cargo deny check` inside the spike crate to enumerate every transitive licence and advisory; time cold init, cached component load, 1,000 host↔guest calls, epoch abort latency, and 1- vs 5-instance RSS. Record results in `docs/measurements/E0.3.md`. |
| ES2 | Because `paint_surface` in pinned GPUI is unavailable on Linux and Windows (`G11`) and a patched GPUI fork requires an explicit owner `Q7` answer, how well does the default **triple-buffered shared-memory BGRA8 ring** uploaded via GPUI's standard image/texture path perform on Linux Wayland (Orion) and Windows? Measure host compositing cost (`EB6`), sustained 60 s image-cache/RSS stability (`EB8`), pointer/key input round-trip latency, idle 0-frame behaviour (`UB4`), and `SheetView` isolation (`UB3`). Also document whether any zero-copy native OS GPU texture-sharing hook is reachable without patching GPUI, or whether it strictly triggers `Q7`. | Build `bench spike plugin-surface` (`crates/omnia-bench/src/spike/plugin_surface.rs`) with a tiny mock sidecar generating 1280x800 60 Hz animated test frames over UDS/named pipe + shared memory. Measure on Windows and on Orion (Wayland). Record results in `docs/measurements/E0.4.md` and settle `ED4`/`EB6` at `HE1` (`E0.5`). |

## 10. Crates, WIT interfaces and APIs

### New crates

1. **`crates/omnia-plugin/` (GPUI-free):**
   - `manifest.rs` — `PluginManifest`, `PluginId` (`EK2`), `PluginLevel`, `Capability` (`EK4`),
     `ContributionDecl` (bundles, commands, erc, exporters, panels, overlays, workbenches).
   - `archive.rs` — `PluginSource` (`Directory` | `ZipArchive`), strict `EK3` validator and
     deterministic packer (`pack_oplugin`), SHA-256 archive and directory-tree digests.
   - `trust.rs` — `TrustStore` (`plugin-trust.ron`), capability grants, workspace trust, Level 2
     directory-tree SHA-256 pins, immutable cache staging, and optional Ed25519 signature metadata
     (`ED7`).
   - `registry.rs` — `PluginHost`, discovery across user-global/workspace-local/dev dirs (`EK11`),
     deterministic load order (`ED12`), lifecycle state machine (`Discovered` -> `Untrusted` ->
     `Enabled` -> `Active` | `Faulted` | `Disabled`).
   - `tier0.rs` — loaders and validators bridging Level 0 bundles into `omnia-parts`,
     `omnia-theme`, template catalog and keymap presets.
   - `erc.rs` — deterministic ERC pass orchestrator merging `<plugin_id>:<rule_id>` findings
     without suppressing built-in findings (`EINV13`).
   - `ui_ir.rs` — `UiTree`, `UiNode`, `ThemeToken`, `UiEvent` (`EK8`, `EINV7`).
   - `scene2d.rs` — `Scene2dLayer`, `DrawPrim`, `ViewportEvent` (`EK9`).
   - `service.rs` — transport-agnostic `PluginService` bridging `omnia-doc`, `omnia-circuit`,
     `omnia-parts`, and `omnia-sim` read views + `PluginPatch` builder/validator (`ED11`).

2. **`crates/omnia-plugin-wasm/` (GPUI-free):**
   - Owns the `wasmtime` engine (`cranelift_nan_canonicalization(true)`), compiled component cache
     (`EQ1`), per-plugin single-threaded worker actors (`ED2`), epoch ticker thread (`EK5`), WASI
     capability & determinism filter (`EK4`, `ED12`), and generated `wit-bindgen` host bindings for
     `wit/omnia-plugin/`.

3. **`crates/omnia-plugin-ipc/` (GPUI-free):**
   - Owns Level 2 sidecar spawning from the verified read-only cache (`ED7`, `EINV8`), UDS /
     named-pipe framed RPC with 256-bit one-time bearer-token authentication (`EK10`), heartbeat
     supervision, shared-memory / surface frame ring receiver, input event forwarding, and
     `UndoParticipant` protocol (`ED6`).

4. **`crates/omnia-plugin-sdk/` (GPUI-free, publishable in future):**
   - Ergonomic Rust SDK wrapping `wit/omnia-plugin/` for Level 1 Wasm plugin authors, plus an
     optional `sidecar` feature (`src/sidecar.rs`, added in `E4.1`) exposing the matching Rust IPC
     server helper for Level 2 workbenches (such as `Reid-Engineered/Spectare`).

### Canonical WIT package (`wit/omnia-plugin/`)

```wit
package omnia:plugin@0.1.0;

interface types {
    type doc-revision = u64;
    variant item-id { part(u32), wire(u32), label(u32) }
    record pt-nm { x: s32, y: s32 }
    variant ext-payload { text(string), bytes(list<u8>) }
    record ext-entry { schema-version: u32, payload: ext-payload }
    variant ext-target { existing(item-id), created(u32) }
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
    use types.{doc-revision, ext-target, ext-entry};
    record patch {
        expected-revision: doc-revision,
        label: string,
        schematic-ops-json: string,   // matches omnia-agent EditOp schema
        ext-ops: list<ext-op>,
    }
    variant ext-op {
        set-doc(ext-entry),
        remove-doc,
        set-item(tuple<ext-target, ext-entry>),
        remove-item(ext-target),
    }
    apply-patch: func(p: patch) -> result<doc-revision, string>;
}

interface ui { /* declarative UiNode tree, ThemeToken enum, UiEvent */ }
interface scene2d { /* retained Scene2dLayer, DrawPrim, ViewportEvent */ }

world omnia-plugin {
    use types.{finding};
    import doc-read;
    import doc-write;
    import ui;
    import scene2d;
    export run-command: func(command-id: string, args-json: string) -> result<string, string>;
    export run-erc: func() -> result<list<finding>, string>;
    export render-panel: func(panel-id: string) -> result<list<u8>, string>;
    export on-event: func(event-bytes: list<u8>) -> result<_, string>;
}
```

### Additions in `crates/omnia-doc/` (`E2.1`, zero plugin-runtime dependency)

```rust
// ---- omnia-doc: ext.rs (E2.1) ----
pub struct PluginNamespace(Arc<str>);          // validated EK2 reverse-DNS id
pub enum ExtPayload {
    Text(Arc<str>),                            // UTF-8 JSON/TOML/text, capped by EK6
    Bytes(Arc<[u8]>),                          // opaque bytes (base64 in RON v4), capped by EK6
}
pub struct ExtEntry {
    pub schema_version: u32,
    pub payload: ExtPayload,
}
pub enum ExtTarget {
    Existing(ItemId),
    Created { op_index: usize },               // item created by schematic_ops[op_index]
}
pub enum ExtOp {
    SetDoc(ExtEntry),
    RemoveDoc,
    SetItem { target: ExtTarget, entry: ExtEntry },
    RemoveItem { target: ExtTarget },
}
pub struct DocExt { /* BTreeMap<PluginNamespace, ExtEntry> + per-ItemId BTreeMap */ }

// ---- omnia-doc: command.rs (E2.1) ----
// Command gains:
//   PluginPatch {
//       namespace: PluginNamespace,
//       label: Arc<str>,
//       schematic_ops: Vec<Command>,          // validated: no nested PluginPatch
//       ext_ops: Vec<ExtOp>,
//   }
// Change gains:
//   pub ext_changed: bool
// CommandError gains:
//   InvalidPluginNamespace, NestedPluginPatch, InvalidExtTarget, ExtLimitExceeded
//
// ---- omnia-doc: Doc queries (E2.1) ----
//   pub fn doc_ext(&self, ns: &str) -> Option<&ExtEntry>;
//   pub fn doc_ext_namespaces(&self) -> impl Iterator<Item = &str>; // sorted
//   pub fn item_ext(&self, item: ItemId, ns: &str) -> Option<&ExtEntry>;
```

### Additions in `crates/omnia-app/` (`E3.1`–`E3.4`, `E4.2`–`E4.3`)

- `crates/omnia-app/src/plugins/mod.rs` — app-side `PluginController` and `A4.2` gesture gate
  bridge (`AINV13`).
- `crates/omnia-app/src/plugins/ui_renderer.rs` — `UiTree` -> native GPUI `U1.3` widgets (`INV6`).
- `crates/omnia-app/src/plugins/viewport2d.rs` — host-accelerated retained 2D quad/line/glyph
  viewport & schematic overlay element (`EB4`, `EB5`, `INV7`).
- `crates/omnia-app/src/plugins/workbench.rs` — mode/tab switcher & workbench host slot (`ED3`).
- `crates/omnia-app/src/plugins/manager.rs` — Plugin Manager view (`E3.4`).
- `crates/omnia-app/src/plugins/consent.rs` — capability consent & Level 2 "Trust Native
  Executable" dialogs (`E3.4`).
- `crates/omnia-app/src/plugins/reload.rs` — dev-folder hot-reload controller (`E3.4`).
- `crates/omnia-app/src/plugins/surface.rs` — Level 2 framebuffer/surface presenter & input
  forwarder (`ED4`, `EB6`, `E4.2`).
- `crates/omnia-app/src/plugins/undo_bridge.rs` — optional Level 2 external `UndoParticipant`
  workspace history bridge (`ED6`, `E4.3`).

## 11. Dependencies and licensing

All new dependencies must pass `cargo deny check` under [deny.toml](deny.toml) and be verified
before production use.

| Crate / Tool | Use | Licence | Status |
| --- | --- | --- | --- |
| `toml` (`1`), `zip` (`9`), `sha2`, `serde`, `ron` | Manifest parsing, `.oplugin` ZIP archives, SHA-256 digests, trust store & RON v4 | MIT / Apache-2.0 | Already in `Cargo.lock` and approved |
| `wasmtime`, `wasmtime-wasi` | Level 1 Wasm Component Model runtime + WASI sandbox (`omnia-plugin-wasm`) | Apache-2.0 WITH LLVM-exception | Audited in out-of-workspace spike `ES1` (`E0.3`); owner approves `deny.toml` entry at `HE1` (`E0.5`) |
| `wit-bindgen` | Guest bindings in `omnia-plugin-sdk` | Apache-2.0 WITH LLVM-exception | Audited in spike `ES1` (`E0.3`); approved at `HE1` (`E0.5`) |
| `ureq` (or `wasmtime-wasi-http` host client) | Sandboxed HTTPS requests for `net.hosts` allow-list (`EK4`) | MIT OR Apache-2.0 | Audited in `E2.2` before enabling `net.hosts`; zero ambient socket access |
| `ed25519-dalek` (optional) | Optional Level 2 publisher signature verification (`ED7`, `EQ4`) | BSD-3-Clause | Optional feature; audited in `E1.2` |
| `notify` (or 500 ms mtime poll) | Dev hot-reload file watch (`ED8`) | MIT / Apache-2.0 or zero-dep poll | Evaluate in `E3.4` against `deny.toml`; fall back to 500 ms mtime poll if licence conflicts |
| OS IPC (`std::os::unix::net`, `windows-sys` already in `Cargo.lock`) | Level 2 UDS + Windows named pipes, job objects & shared memory (`omnia-plugin-ipc`) | MIT / Apache-2.0 | Audit in `ES2` (`E0.4`) / `E4.1` |
| System `git` CLI (external tool, no Rust crate) | Git URL plugin installs pinned to tag/commit (`ED8`, `E1.4`) | External user tool | Invoked with `--` separation; clear error if `git` is not installed |

No GPL/LGPL crate is permitted. Any SPDX exception (such as `Apache-2.0 WITH LLVM-exception` used
by `wasmtime`) requires explicit owner approval at `HE1` before editing `deny.toml`.

## 12. Risks and mitigations

| Risk | Mitigation | Early test |
| --- | --- | --- |
| GPUI `paint_surface` is unavailable on Linux and Windows (`G11`), and a patched GPUI fork requires an explicit owner `Q7` decision | Default to a triple-buffered shared-memory BGRA8 ring uploaded via GPUI's standard image/texture path (`ED4`); spike `ES2` measures compositing cost, 60 s cache/RSS growth and input latency on Orion and Windows before `HE1` | `ES2` (`E0.4`) |
| `wasmtime` pulls transitive crates outside `deny.toml` or regresses cold start / RSS | Run `ES1` in an out-of-workspace crate (`docs/measurements/E0.3/spike/`), auditing every transitive licence, lazy engine init (`EB1`), and RSS (`EB8`) with minimal Cranelift-only features | `ES1` (`E0.3`) |
| Level 1 2D overlays/viewports regress `INV7` (zero `Path` primitives) or `EB5` at 10k primitives | Constrain `EK9` primitives to Omnia's quad/line/glyph GPU pipeline (no per-frame path tessellation) and enforce an explicit 10k pan/zoom + zero-alloc check in `E3.2`/`E3.5` | `ETC-2d-viewport-pan`, `EB5` |
| Plugin UI panels regress `SheetView` paint isolation (`UB3`) or warm-frame allocations (`UB5`) | Declarative UI renders exclusively through `omnia-app`'s `U1.3` non-focus-stealing chrome widgets (`UG15`–`UG17`), gated by `bench ui` in `E3.5` | `ETC-ui-panel-theme`, `EB4` |
| Plugin writes race with an active canvas drag/wire gesture or concurrent agent edit | Enforce `expected_revision` and refuse writes immediately with `user_gesture_in_progress` through the shared `A4.2` gate (`AINV13`, `ED11`) | `ETC-gesture-refuse`, `EINV11` |
| Malicious or modified workspace/sidecar plugin executes without informed consent | Workspace-local plugins start disabled (`ED7`); Level 1 enforces Wasm capability sandboxing; Level 2 requires explicit "Trust Native Executable" consent warning of full OS rights, hashes the full `bin/<os>-<arch>/` tree, and spawns strictly from the read-only content-addressed cache (`EINV8`) | `ETC-consent-gate`, `ETC-sidecar-trust` |
| Local process hijacks Level 2 socket/pipe name | Require a per-launch CSPRNG 256-bit one-time bearer token passed via inherited stdin/pipe handshake (or inherited socketpair on Linux) before accepting RPC frames (`EK10`) | `ETC-sidecar-trust`, `EINV14` |
| Large workbench state bloats `.omnia` files and breaks `UB10` / `UB9` | Hard `EK6` cap (1 MiB total `ext` per document) forces large workbench datasets into sibling/project sidecar files (`ED5`) | `EB7`, `E2.1` |
| `omnia-doc` file conflicts with in-flight `parts` (`P2.1`–`P2.3`) and `context` (`C1.9`, `C1.1`) tasks | `E2.1` sequences strictly after `P2.3`, `C1.9` and `C1.1` (extending `XD7`), keeping Milestone E0 and Milestone E1 completely independent of `omnia-doc` | `XD7` dependency chain |
| Level 2 sidecar crashes or hangs and leaves zombie processes or corrupted undo history | Job objects (Windows) / `PR_SET_PDEATHSIG` + process-group kill (Linux), heartbeat timeout (`EK10`), and isolated sidecar undo participant (`ED6`) | `ETC-sidecar-workbench`, `EINV14` |

## 13. Open questions

Each open question has a single **DEFAULT**; build to the default until the owner's dated answer
replaces it at `HE0` (or `HE1` for spike-dependent defaults).

| ID | Question | Default |
| --- | --- | --- |
| EQ1 | Should Level 1 Wasm cache locally compiled native artifacts after first load? | **DEFAULT:** Ship portable `.wasm` components inside `.oplugin`; `wasmtime` compiles on first load and caches per-host-version artifacts under `<cache_dir>/omnia/wasm-cache/` (invalidated automatically when Omnia's `wasmtime` version changes). |
| EQ2 | How should `ExtPayload` be encoded in RON `version: 4`? | **DEFAULT:** Tagged enum `ExtPayload::Text(Arc<str>)` (plain UTF-8 string for JSON/TOML plugin metadata, readable in diffs) and `ExtPayload::Bytes(Arc<[u8]>)` (canonical base64 string in RON for binary payloads). |
| EQ3 | Should `Doc::save` write RON `version: 4` unconditionally after `E2.1`, or keep `version: 3` when `ext` is empty? | **DEFAULT:** Write `version: 3` when `ext` is empty (keeping existing v3 fixtures byte-identical) and `version: 4` whenever any `ext` block is present; `Doc::load` reads v2, v3 and v4 and refuses v1 and v5+. |
| EQ4 | Should Ed25519 publisher signatures for Level 2 sidecars be implemented in v1 alongside directory-tree SHA-256 pinning, or deferred to v1.1? | **DEFAULT:** Mandatory directory-tree SHA-256 pinning in v1 (`ED7`); optional Ed25519 signature field reserved in `plugin.toml` and `plugin-trust.ron`, implemented if `ed25519-dalek` adds zero friction in `E1.2`. |
| EQ5 | Should `plugin.toml` reserve optional registry metadata (`homepage`, `repository`, `license`, `categories`) now for a future community index? | **DEFAULT:** Yes — optional descriptive fields in `schema = 1`, with no network registry fetches in v1. |
| EQ6 | How should `omnia-agent` / MCP interact with installed plugins in v1? | **DEFAULT:** Read-only exposure first: plugin ERC findings flow into `omnia_context` findings (`EINV13`); exposing plugin-contributed commands as dynamic MCP tools is deferred past v1 to preserve `mcp-plan.md`'s static tool-schema guarantee (`AD14`). |
| EQ7 | Should custom simulation engines (`SimEngine`) and custom vector symbols (`PQ3`) join the plugin WIT/IPC contract in a follow-up minor version (`0.2.0`)? | **DEFAULT:** Yes — keep `omnia:plugin@0.1.0` focused on bundles, schematic tools/ERC/exporters, UI/2D workbenches, and Level 2 workbenches; add `sim-engine` and `symbol-provider` interfaces in `0.2.0` once `omnia-sim` (`S2.3`) and `PQ3` stabilise. |
| EQ8 | For Level 2 workbenches like `Reid-Engineered/Spectare`, how should multi-file workbench state accompany a `.omnia` sheet before a project container exists? | **DEFAULT:** Start with sibling sidecar files next to the `.omnia` file (resolved relative to the document directory, gated by `fs.workspace.*`), and align with any future project container decided in `ROADMAP.md`'s open file-experience decision. |

## 14. Changes to other files

None of the files below are edited by `E0.1`; each row names the task that makes the change.

| Where | Change | Made by |
| --- | --- | --- |
| `AGENTS.md` | Add `plugin-plan.md` / `plugin-roadmap.md` to the sources-of-truth table, register the `E`-prefixed ids (`EB`, `EK`, `ED`, `EINV`, `ETC`, `ES`, `EQ`, `HE`, tasks `E0.1`…), and add `HE*` to hard rule 4. | `E0.2` (upon `HE0` sign-off) |
| `ROADMAP.md` | Link `plugin-plan.md` and `plugin-roadmap.md` in the intro and section 6 (*Grow from a reliable baseline*, as a candidate post-baseline addition). | `E0.2` |
| `.github/wiki/build_wiki.py` | Register `("plugin-roadmap.md", "Plugin-roadmap", "Plugins")` in `ROADMAPS`, `("plugin-plan.md", "Plugins")` in `PLANS`, and add `plugin` to `DEPENDS_FILLER`. | `E0.2` |
| `docs/file-format-proposal.md` | Reserve optional section kind `EXT_DATA` for namespaced `omnia-doc` extension blocks (`EK7`, noting older binary readers that skip unknown sections do not preserve `EXT_DATA` on re-save). | `E0.2` |
| `deny.toml` | Log any approved `wasmtime` / `wit-bindgen` SPDX licence exception (only after `ES1` audit and owner `HE1` sign-off). | `E0.5` (`HE1`) |
| `parts-plan.md`, `ui-plan.md`, `mcp-plan.md`, `canvas-plan.md` (section 9), `crates/omnia-agent/src/files.rs`, `crates/omnia-app` (`UTC-open-bad` fixture) | Update RON version facts from v3 to v4 (`EK7`, `EQ3`): `Doc::load` accepts v2, v3 and v4; bump unsupported-version fixtures (`PTC-version-refuse`, `UTC-open-bad`, `files.rs`) from `version: 4` to `version: 5`; document `DocExt`, `Command::PluginPatch` and `Change::ext_changed`. | `E2.1` (in the same PR as `omnia-doc` changes, after `P2.3`, `C1.9` and `C1.1`) |
| `ui-plan.md` (sections 1, 5 and 10) | Document the workbench mode/tab host slot, plugin dock/overlay slots, Plugin Manager dialog, and `--safe-mode`. | `E3.3` |
| `mcp-plan.md` | Note that plugin-contributed ERC findings appear under `<plugin_id>:<rule_id>` without suppressing built-in findings (`EINV13`). | `E2.3` |
| `THIRD_PARTY.md` | Record any new third-party dependencies added by `omnia-plugin-wasm` or `omnia-plugin-ipc`. | `E2.2`, `E4.1` |
