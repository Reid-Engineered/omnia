# Omnia plugin system: roadmap for agents

The task list for Omnia's plugin and workbench extension system (Tier 0 declarative bundles,
Tier 1 sandboxed Wasm Component Model plugins, Tier 2 out-of-process sidecar workbenches
targeting `Reid-Engineered/Spectare`, declarative themed UI, retained 2D viewports, and
`omnia-doc` extension patches). The reference for every `E`-prefixed id used here (`EB`, `EK`,
`ED`, `EINV`, `ETC`, `ES`, `EQ`, `HE`) is [plugin-plan.md](plugin-plan.md). Unprefixed ids are in
[canvas-plan.md](canvas-plan.md); `W`, `A`, `U`, `S`, `P` and `C` ids are in
[wiring-plan.md](wiring-plan.md), [mcp-plan.md](mcp-plan.md), [ui-plan.md](ui-plan.md),
[sim-plan.md](sim-plan.md), [parts-plan.md](parts-plan.md) and
[context-plan.md](context-plan.md).

This roadmap runs beside the existing roadmaps. Milestone E0 (spikes) and Milestone E1 (core host
+ Tier 0 bundles) have zero `omnia-doc` or GPUI coupling and can progress independently once `HE0`
is signed. Milestone E2 sequences its `omnia-doc` changes (`E2.1`) strictly after `C1.1` (extending
`XD7`). Milestones E3 and E4 integrate into the `omnia-app` shell once `U1.7` and `U2.4` land.

## How to work a task

Follow `AGENTS.md`: pick a `todo` task whose dependencies are all `done` (a `needs-human`
dependency is not `done`), branch from current `main` as `task/<id>-<slug>`, set the task to
`doing`, read everything its **Read** line names, edit only what **Owns** lists, run the standard
checks and **Done when** checks, update the status, append one dated log entry at the bottom, get
an independent review, and open one pull request per task.

Statuses: `todo`, `doing`, `done`, `blocked (reason)`, `needs-human`.

## Rules

`AGENTS.md` hard rules and canvas roadmap rules 1 to 12 apply, with branch names
`task/<id>-<slug>`, for example `task/E1.1-manifest-archive`. Six more:

1. **Zero GPUI in `omnia-plugin*`; zero plugin runtime in `omnia-doc`.** Verify `EINV1` and
   `EINV2` on every `Cargo.toml` edit.
2. **Untrusted plugins and archives are hostile by default.** Enforce `EK3` archive limits before
   allocation (`EINV10`), default-deny all `EK4` capabilities, never auto-run workspace-local
   plugins, and verify Tier 2 SHA-256 pins before spawn (`EINV8`).
3. **Never block the UI thread or break `INV6`/`UB3`/`UB5`.** All Wasm execution and Tier 2 IPC run
   off the UI thread (`EINV6`), and all plugin UI/2D colours resolve through `omnia-theme` tokens
   (`EINV7`).
4. **Deterministic order everywhere.** Never let filesystem walk order, ZIP entry order or
   hash-map iteration order affect load order, ERC findings, `ext` serialisation or `.oplugin`
   packing (`EINV9`, `ED12`).
5. **No licence or budget shortcuts.** Any new crate (especially `wasmtime` / `wit-bindgen`) passes
   `cargo deny check` with owner approval at `HE1`, and budgets are certified only on the Orion
   reference machine (`AGENTS.md` hard rules 2 and 3).
6. **Keep plans true.** Any change to a constant (`EK`), decision (`ED`), budget (`EB`), WIT
   interface or public signature updates [plugin-plan.md](plugin-plan.md) in the same pull request.

## Standard checks

As in `AGENTS.md`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p omnia-bench --all-targets --features scene-counts,readback -- -D warnings
cargo test --workspace
cargo deny check
```

## Lanes

| Lane | Owns | Needs GPUI |
| --- | --- | --- |
| `plugin-core` | `crates/omnia-plugin/`, `wit/omnia-plugin/` | No |
| `plugin-wasm` | `crates/omnia-plugin-wasm/` | No |
| `plugin-ipc` | `crates/omnia-plugin-ipc/` | No |
| `plugin-sdk` | `crates/omnia-plugin-sdk/`, `examples/plugins/` | No |
| `doc` | `crates/omnia-doc/src/{ext.rs,doc.rs,command.rs,undo.rs,io.rs}` (`E2.1` only, after `C1.1`) | No |
| `plugin-ui` | `crates/omnia-app/src/plugins/`, `crates/omnia-canvas/src/overlay.rs` plugin layer hook (`E3.2`) | Yes |
| `cli` | `crates/omnia-cli/src/plugin.rs` | No |
| `bench` | `crates/omnia-bench/src/spike/plugin_*.rs`, `crates/omnia-bench/benches/plugin*.rs`, `docs/measurements/E*.md` | Mixed (`ES2` / UI gates: Yes) |

File ownership order for `omnia-doc` (`doc.rs`, `command.rs`, `undo.rs`, `io.rs`) extends `XD7`:
`P2.1`–`P2.3` first, then `C1.9`, then `C1.1`, then `E2.1`.

## Milestones

| Milestone | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| Milestone E0 | `plugin-plan.md`, this roadmap, owner sign-off (`HE0`), spikes `ES1` (`wasmtime` + `deny.toml`) and `ES2` (GPUI shared GPU surface on Wayland + Windows) | E0.2 (`HE0`), then E0.4 (`HE1`) | `HE0`, `HE1`, `ES1`, `ES2` |
| Milestone E1 | GPUI-free `omnia-plugin` core host, `.oplugin` archive/manifest validator, trust store, CLI management, Tier 0 content bundles (`.olib`/themes/templates/keymaps) | E1.4 | `EB2`, `EINV1`, `EINV8`–`EINV10`, `ETC-tier0-bundle`, `ETC-hostile-oplugin` |
| Milestone E2 | `omnia-doc` `ext` + `Command::PluginPatch` + RON v4, `omnia-plugin-wasm` runtime, `omnia:plugin@0.1.0` WIT bindings, ERC/exporter hooks, `omnia-plugin-sdk`, CLI `new`/`pack`/`run` | E2.5 | `EB3`, `EB7`, `EINV2`–`EINV5`, `EINV12`, `EINV13`, `ETC-patch-*`, `ETC-wasm-*` |
| Milestone E3 | Declarative `omnia-theme` UI renderer, retained 2D viewport & schematic overlays, mode/tab workbench host, gesture gate, consent & Plugin Manager UI, dev hot-reload | E3.5 | `EB4`, `EB5`, `EINV6`, `EINV7`, `EINV11`, `HE2`, `ETC-ui-*`, `ETC-2d-*`, `ETC-dev-*` |
| Milestone E4 | `omnia-plugin-ipc` sidecar supervisor, Tier 2 GPU/shm surface presenter, input forwarding, external undo participant, Spectare reference integration, Orion certification gate | E4.5 | `EB1`, `EB6`, `EB8`, `EB9`, `EINV14`, `HE3`, `ETC-sidecar-*` |

## Cross-roadmap dependencies

| Plugin task | Waits for (other roadmaps) | Can start |
| --- | --- | --- |
| `E0.1`–`E0.4` | nothing beyond `E0.2` (`HE0`) | now (`E0.3`/`E0.4` after `HE0`) |
| `E1.1`, `E1.2` | `E0.2` (`HE0`) | after `HE0` |
| `E1.3` (Tier 0 bundles) | `P1.5` (`omnia-parts::Catalog`), `U3.2` (theme switching) | after `P1.5` & `U3.2` |
| `E2.1` (`omnia-doc` `ext` + `PluginPatch`) | `P2.3`, `C1.9`, `C1.1` (`XD7` file order) | after `C1.1` |
| `E2.2` (`omnia-plugin-wasm`) | `E0.4` (`HE1`), `E1.2`, `E2.1` | after `HE1` & `E2.1` |
| `E2.3` (ERC + exporters + `omnia-circuit` bridge) | `E2.2`, `C1.6` (findings), `S1.3` (ERC codes) | after `E2.2`, `C1.6` & `S1.3` |
| `E3.3` (Workbench host & docks in `omnia-app`) | `E3.1`, `E3.2`, `U1.7` (workspace layout/tabs/docks), `U2.4` (dialogs/toasts) | after `U1.7`, `U2.4`, `E3.1` & `E3.2` |
| `E3.4` (Consent, Plugin Manager, gesture gate, hot-reload) | `E3.3`, `A4.2` (shared app gesture gate) | after `E3.3` & `A4.2` |
| `E4.1`–`E4.3` (Tier 2 sidecar + GPU surface) | `E0.4` (`HE1`), `E3.3`, `E3.4` | after `HE1` & `E3.4` |

## Dependency overview

```
E0.1 -> E0.2 [HE0] -> E0.3 (ES1), E0.4 (ES2) [HE1]
E0.2 -> E1.1 (manifest/archive) -> E1.2 (trust/registry) -> E1.3 (+ P1.5, U3.2: Tier 0) -> E1.4 (CLI + Gate E1)
E0.4 + P2.3 + C1.9 + C1.1 -> E2.1 (omnia-doc ext + PluginPatch + RON v4)
E1.2 + E2.1 + E0.4 -> E2.2 (omnia-plugin-wasm + WIT host)
E2.2 + C1.6 + S1.3 -> E2.3 (ERC + exporters + circuit read views)
E2.2 -> E2.4 (omnia-plugin-sdk + CLI new/pack/run + examples)
E1.4 + E2.3 + E2.4 -> E2.5 (Gate E2 + plugin_wasm/plugin_doc benches)
E2.2 -> E3.1 (declarative UI IR + GPUI widget renderer) --+
E2.2 -> E3.2 (retained 2D draw-list + overlay/viewport) --+--> (+ U1.7, U2.4) E3.3 (workbench host)
E3.3 + A4.2 -> E3.4 (consent, Plugin Manager, gesture gate, hot-reload, --safe-mode) -> E3.5 (Gate E3) [HE2]
E0.4 + E1.2 -> E4.1 (omnia-plugin-ipc supervisor + framed RPC + shm/handle transport)
E4.1 + E3.3 -> E4.2 (GPU/shm surface presenter + input forwarding + sidecar crash recovery)
E4.2 + E3.4 -> E4.3 (external undo participant + Spectare-target reference sidecar workbench)
E4.3 -> E4.4 (Tier 2 benches + fault-injection suite) -> E4.5 (Final Orion Gate) [HE3]
```

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| `HE0` | E0.2 | Reviews `plugin-plan.md` (`EB1`–`EB9`, `EK1`–`EK12`, `ED11`–`ED13`, `EQ1`–`EQ8`), confirms or amends proposed items, and signs off to start `ES1`/`ES2` and Milestone E1. |
| `HE1` | E0.4 | Reviews `ES1` (`wasmtime` licence/size/cold-start/RSS) and `ES2` (GPUI Wayland + Windows surface presentation) measurements, approves any `deny.toml` entry for `wasmtime`, and selects the primary Tier 2 surface path before Milestone E2/E4 begin. |
| `HE2` | E3.5 | Interactively tests a Tier 0 bundle, a Tier 1 Wasm tool/ERC plugin, a declarative themed panel, a 2D workbench tab, consent prompts and dev hot-reload in `omnia-app`, and approves visual/UX feel. |
| `HE3` | E4.5 | Runs the final Orion reference-machine campaign (`EB1`–`EB9` plus `B1`/`B3`/`B4`/`B9`/`UB1`/`UB3`/`UB4`/`UB5`/`UB10` regressions) and verifies a Tier 2 GPU workbench (targeting `Reid-Engineered/Spectare`) end-to-end. |

---

## Milestone E0: plan, decisions and spikes

### E0.1 Plugin system plan and roadmap · done
- Lane: plugin-core · Depends: none · Read: `AGENTS.md`, `ROADMAP.md`, `canvas-plan.md`, `ui-plan.md`, `mcp-plan.md`, `parts-plan.md`, `sim-plan.md`, `context-plan.md`, `docs/file-format-proposal.md`, `deny.toml`, `.github/wiki/build_wiki.py`
- Owns: `plugin-plan.md`, `plugin-roadmap.md`
- Do: write both files recording owner interview decisions `ED1`–`ED10`, proposed decisions `ED11`–`ED13`, budgets `EB1`–`EB9`, constants `EK1`–`EK12`, invariants `EINV1`–`EINV14`, scenarios `ETC-*`, spikes `ES1`–`ES2`, and open questions `EQ1`–`EQ8`. Scan both files for third-party company or commercial product names (`AGENTS.md` hard rule 1) and verify wiki parser compatibility.
- Done when: every `E`-prefixed id is defined once and resolves cleanly; no statement contradicts existing plans; `AGENTS.md` rule-1 scan passes; `.github/wiki` unit tests pass and `build_wiki.parse_roadmap` parses `plugin-roadmap.md` cleanly.

### E0.2 Owner sign-off and repository registration · needs-human → HE0
- Lane: plugin-core · Depends: E0.1 · Read: `plugin-plan.md` sections 3, 4, 6, 13 and 14, `AGENTS.md`, `ROADMAP.md`, `.github/wiki/build_wiki.py`, `docs/file-format-proposal.md`
- Owns: `plugin-plan.md`, `plugin-roadmap.md`, `AGENTS.md`, `ROADMAP.md`, `.github/wiki/build_wiki.py`, `.github/wiki/test_build_wiki.py`, `docs/file-format-proposal.md`
- Do: wait for owner review (`HE0`) of `EB1`–`EB9`, `EK1`–`EK12`, `ED11`–`ED13` and `EQ1`–`EQ8`; flip accepted `PROPOSED` entries to `DECIDED` (dated); apply section 14 registrations to `AGENTS.md`, `ROADMAP.md`, `.github/wiki/build_wiki.py` (`ROADMAPS`, `PLANS`, `DEPENDS_FILLER`), and `docs/file-format-proposal.md` (`EXT_DATA`).
- Done when: owner signs off `HE0`; no `PROPOSED` entry remains in `plugin-plan.md`; `python3 -m unittest discover -s .github/wiki -v` passes with `plugin-roadmap.md` registered; `AGENTS.md` rule-1 scan passes across all touched files.

### E0.3 Spike ES1: `wasmtime` Component Model, licence audit and startup/RSS budget · todo
- Lane: bench · Depends: E0.2 · Read: `plugin-plan.md` sections 3 (`EB1`, `EB3`, `EB8`, `EB9`), 9 (`ES1`) and 11, `deny.toml`
- Owns: `crates/omnia-bench/src/spike/plugin_wasm.rs`, `docs/measurements/E0.3.md`, `docs/measurements/E0.3/`
- Do: prototype a minimal `wasmtime` Component Model host (Cranelift only, minimal features) running a tiny WIT component; run `cargo deny check` to enumerate every transitive licence and advisory; measure cold engine init, cached component load, 1,000 host↔guest calls, epoch abort latency for an infinite loop, and RSS with 1 and 5 instances. Record hardware, raw numbers and exact findings in `docs/measurements/E0.3.md`.
- Done when: `docs/measurements/E0.3.md` settles `ES1` with concrete licence, startup, call-latency, abort-latency and RSS evidence ready for `HE1`.

### E0.4 Spike ES2: GPUI Tier 2 shared GPU surface vs shm ring on Wayland and Windows · todo → HE1
- Lane: bench · Depends: E0.2, E0.3 · Read: `plugin-plan.md` sections 2 (`G11`), 3 (`EB6`), 4 (`EK10`) and 9 (`ES2`), `canvas-plan.md` section 4 (`G11`)
- Owns: `crates/omnia-bench/src/spike/plugin_surface.rs`, `docs/measurements/E0.4.md`, `docs/measurements/E0.4/`, `plugin-plan.md` (`ED4`/`EB6` status update after `HE1`), `deny.toml` (only if owner approves `ES1` licence exception at `HE1`)
- Do: build a standalone `bench spike plugin-surface` with a mock out-of-process sidecar sending 1280x800 60 Hz animated frames over UDS/named pipe; evaluate native GPU texture import vs triple-buffered shared-memory BGRA8 ring uploaded to a GPUI texture/image on Windows and Linux Wayland (Orion); measure host compositing cost, frame pacing, idle CPU (`UB4`) and `SheetView` isolation (`UB3`); pause at `HE1` for owner selection of primary/fallback surface path and `ES1` `deny.toml` approval.
- Done when: `docs/measurements/E0.4.md` has Windows and Orion numbers; owner accepts `HE1`, turning `EB6` from `HYPOTHESIS` to `DECIDED` and approving any `ES1` `deny.toml` entry.

---

## Milestone E1: core host and Tier 0 content bundles

### E1.1 `omnia-plugin` crate: manifest schema, `.oplugin` archive loader and packer · todo
- Lane: plugin-core · Depends: E0.2 · Read: `plugin-plan.md` sections 4 (`EK1`–`EK4`), 6 (`ED8`, `ED12`), 7 (`EINV1`, `EINV9`, `EINV10`), 8 (`ETC-hostile-oplugin`), `parts-plan.md` (`PD2`, `PK3`)
- Owns: `Cargo.toml` (workspace member), `crates/omnia-plugin/Cargo.toml`, `crates/omnia-plugin/src/{lib.rs,manifest.rs,archive.rs,error.rs}`, `crates/omnia-plugin/tests/archive.rs`
- Do: create GPUI-free `omnia-plugin`; implement `PluginManifest` (`schema = 1`), `PluginId` (`EK2`), `Capability` (`EK4`), `PluginSource` (`Directory` | `ZipArchive`) enforcing all `EK3` limits before allocation, canonical SHA-256 digests, and deterministic `pack_oplugin`.
- Done when: `EINV1`, `EINV9` (archive/manifest determinism), `EINV10` and `ETC-hostile-oplugin` pass; all standard checks pass.

### E1.2 Trust store, capability consent state, discovery and lifecycle registry · todo
- Lane: plugin-core · Depends: E1.1 · Read: `plugin-plan.md` sections 4 (`EK4`, `EK11`, `EK12`), 6 (`ED7`, `ED8`, `ED12`, `ED13`), 7 (`EINV8`, `EINV9`), 8 (`ETC-consent-gate`, `ETC-sidecar-trust`)
- Owns: `crates/omnia-plugin/src/{trust.rs,registry.rs}`, `crates/omnia-plugin/tests/trust_registry.rs`
- Do: implement `TrustStore` (`plugin-trust.ron`) tracking granted capabilities, workspace-local trust, Tier 2 SHA-256 pins (and optional Ed25519 signature metadata per `EQ4`); implement deterministic discovery across user-global, workspace-local and dev dirs with workspace shadowing; enforce API version checks (`EK12`) and capability-upgrade re-consent.
- Done when: `EINV8`, `EINV9` (shuffled discovery order), `ETC-consent-gate` (core/headless half) and `ETC-sidecar-trust` (hash-pin refusal) pass.

### E1.3 Tier 0 declarative content bundles (`.olib`, themes, templates, keymaps) · todo
- Lane: plugin-core · Depends: E1.2, P1.5, U3.2 · Read: `plugin-plan.md` sections 1, 6 (`ED1`, `ED8`, `ED12`), 8 (`ETC-tier0-bundle`), `parts-plan.md` section 10, `ui-plan.md` (`U3.2`)
- Owns: `crates/omnia-plugin/src/tier0.rs`, `crates/omnia-plugin/tests/tier0.rs`, `examples/plugins/starter-bundle/`
- Do: implement validated Tier 0 loaders mounting bundled `.olib`/`.opart` packs into `omnia-parts::Catalog`, registering namespaced `omnia-theme` themes (token-validated, zero invalid colours), exposing schematic templates, and registering opt-in keymap presets with deterministic conflict resolution (`ED12`). Add a reference `starter-bundle` fixture.
- Done when: `ETC-tier0-bundle` passes cleanly for both unpacked dir and packed `.oplugin`, including enable/disable teardown and deterministic conflict handling.

### E1.4 CLI plugin management and Milestone E1 gate · todo
- Lane: cli, bench · Depends: E1.3 · Read: `plugin-plan.md` sections 3 (`EB2`), 6 (`ED8`, `ED10`), 10 (`omnia-cli`)
- Owns: `crates/omnia-cli/src/plugin.rs`, `crates/omnia-cli/src/main.rs` (subcommand wiring), `crates/omnia-bench/benches/plugin.rs`, `crates/omnia-bench/Cargo.toml`, `docs/measurements/E1.4.md`
- Do: add `omnia plugin verify|pack|install|list|enable|disable|remove` (supporting local `.oplugin`, unpacked dev folders, and Git URL install pinned to commit/tag); add the `EB2` archive/manifest scan benchmark and record non-reference numbers.
- Done when: CLI commands work end-to-end with exit-code and transcript tests; `EB2` bench runs cleanly and numbers are recorded in `docs/measurements/E1.4.md`; all Milestone E1 invariants (`EINV1`, `EINV8`–`EINV10`) pass.

---

## Milestone E2: `omnia-doc` extension patches, Tier 1 Wasm runtime and SDK

### E2.1 `omnia-doc` namespaced `ext`, `Command::PluginPatch` and RON `version: 4` · todo
- Lane: doc · Depends: E0.4, P2.3, C1.9, C1.1 · Read: `plugin-plan.md` sections 4 (`EK2`, `EK6`, `EK7`), 6 (`ED5`, `ED6`), 7 (`EINV2`, `EINV3`, `EINV4`, `EINV5`, `EINV12`), 8 (`ETC-patch-atomic`, `ETC-patch-offline-undo`, `ETC-ext-missing-plugin`), `canvas-plan.md` (`INV4`), `parts-plan.md` (`PD8`, `XD7`)
- Owns: `crates/omnia-doc/src/{ext.rs,doc.rs,command.rs,undo.rs,io.rs}`, `crates/omnia-doc/tests/plugin_ext.rs`, `canvas-plan.md` (section 9 update), `mcp-plan.md` (RON v4 note), mechanical exhaustive-match updates in workspace crates for `Command::PluginPatch`
- Do: add `PluginNamespace`, `ExtEntry`, `ExtOp`, document-level and item-level `ext` storage in `Doc` (pruned atomically with item deletion and restored on undo), transactional reversible `Command::PluginPatch` (forbidding nested `PluginPatch`, enforcing `EK6` caps), and RON `version: 4` serialisation with v2/v3 load compatibility (`EQ3`). Keep `omnia-doc` strictly free of plugin-runtime dependencies (`EINV2`).
- Done when: `EINV2`, `EINV3`, `EINV4`, `EINV5`, `EINV12`, `ETC-patch-atomic`, `ETC-patch-offline-undo` and `ETC-ext-missing-plugin` pass; existing `tests/golden/*.ron` and v2/v3 fixtures stay unchanged and green; all standard checks pass.

### E2.2 `omnia-plugin-wasm` runtime and `omnia:plugin@0.1.0` WIT host bindings · todo
- Lane: plugin-wasm, plugin-core · Depends: E0.4, E1.2, E2.1 · Read: `plugin-plan.md` sections 3 (`EB3`, `EB8`, `EB9`), 4 (`EK4`, `EK5`, `EK12`), 6 (`ED2`, `ED7`, `ED11`, `ED13`), 7 (`EINV1`, `EINV3`, `EINV6`, `EINV8`), 8 (`ETC-wasm-trap`, `ETC-wasm-timeout`), 10 (`wit/omnia-plugin/`)
- Owns: `Cargo.toml` (workspace member), `wit/omnia-plugin/`, `crates/omnia-plugin/src/service.rs`, `crates/omnia-plugin-wasm/`, `THIRD_PARTY.md`
- Do: define the canonical `wit/omnia-plugin/` package (`omnia:plugin@0.1.0`); implement `PluginService` in `omnia-plugin`; implement GPUI-free `omnia-plugin-wasm` using `wasmtime` Component Model with compiled-artifact caching (`EQ1`), per-plugin off-UI worker actor, memory/stack/table caps, 10 ms epoch interruption (`EK5`), WASI capability filtering (`EK4`), and fault quarantine (`ED13`).
- Done when: `EINV1`, `EINV6` (worker isolation), `EINV8` (WASI sandbox), `ETC-wasm-trap` and `ETC-wasm-timeout` pass; `cargo deny check` passes.

### E2.3 Custom ERC findings, exporters and `omnia-circuit` read views · todo
- Lane: plugin-core, plugin-wasm · Depends: E2.2, C1.6, S1.3 · Read: `plugin-plan.md` sections 6 (`ED11`, `ED12`), 7 (`EINV9`, `EINV13`), 8 (`ETC-wasm-erc`, `ETC-wasm-bom-export`), `context-plan.md` (`C1.6`), `sim-plan.md` (`S1.3`)
- Owns: `crates/omnia-plugin/src/erc.rs`, `crates/omnia-plugin-wasm/src/host_circuit.rs`, `crates/omnia-plugin-wasm/tests/erc_export.rs`, `mcp-plan.md` (plugin findings note)
- Do: wire `omnia-circuit` read views (`outline`, `net`, `findings`, `bindings`) into `doc-read`; implement deterministic ERC pass orchestration across enabled `erc.contribute` plugins tagging every finding `<plugin_id>:<rule_id>` without suppressing built-in findings (`EINV13`); wire host-mediated `fs.export` output for BOM/netlist exporters.
- Done when: `EINV9`, `EINV13`, `ETC-wasm-erc` and `ETC-wasm-bom-export` pass.

### E2.4 `omnia-plugin-sdk`, CLI `new`/`run` and reference Wasm example plugins · todo
- Lane: plugin-sdk, cli · Depends: E2.2 · Read: `plugin-plan.md` sections 6 (`ED10`), 10 (`omnia-plugin-sdk`)
- Owns: `Cargo.toml` (workspace member), `crates/omnia-plugin-sdk/`, `crates/omnia-cli/src/plugin.rs` (`new` and `run` subcommands), `examples/plugins/{sample-erc,bom-csv,ladder-gen}/`
- Do: build ergonomic Rust SDK crate `omnia-plugin-sdk` wrapping `wit/omnia-plugin/` with `PluginPatch`, `UiNode` and `Scene2dLayer` builders; add `omnia plugin new <name>` and headless `omnia plugin run <id> <cmd> --doc <file>`; author three reference Wasm plugins (`sample-erc`, `bom-csv`, `ladder-gen`) compiled and exercised in tests.
- Done when: `omnia plugin new` scaffolds a valid project; all three example plugins compile to Wasm components, pack to `.oplugin`, and run cleanly via `omnia plugin run` (`EINV1` holds).

### E2.5 Milestone E2 gate and `EB3`/`EB7` benchmarks · todo
- Lane: bench · Depends: E1.4, E2.3, E2.4 · Read: `plugin-plan.md` sections 3 (`EB3`, `EB7`), 8 (`ETC-patch-*`, `ETC-wasm-*`)
- Owns: `crates/omnia-bench/benches/{plugin_wasm.rs,plugin_doc.rs}`, `crates/omnia-bench/Cargo.toml`, `docs/measurements/E2.5.md`
- Do: benchmark Wasm BOM/ERC invocation on `1k` and `Command::PluginPatch` on `10k` (`EB3`), plus `10k` RON v4 save/load with 256 KiB opaque `ext` across 4 namespaces vs v3 baseline (`EB7`); verify every Milestone E2 scenario and invariant passes; record non-reference numbers.
- Done when: both benches run cleanly and are recorded in `docs/measurements/E2.5.md`; all Milestone E2 tests and `EINV1`–`EINV5`, `EINV8`–`EINV10`, `EINV12`, `EINV13` pass.

---

## Milestone E3: declarative UI, retained 2D viewports and workbench host

### E3.1 Declarative UI IR and GPUI `omnia-theme` widget renderer · todo
- Lane: plugin-core, plugin-ui · Depends: E2.2, U1.3 · Read: `plugin-plan.md` sections 4 (`EK8`), 6 (`ED4`), 7 (`EINV6`, `EINV7`), 8 (`ETC-ui-panel-theme`), `ui-plan.md` (`U1.3`, `UG8`, `UG15`–`UG17`, `UINV1`)
- Owns: `crates/omnia-plugin/src/ui_ir.rs`, `crates/omnia-app/src/plugins/ui_renderer.rs`, `crates/omnia-app/tests/plugin_ui.rs`
- Do: define bounded `UiTree`/`UiNode`/`ThemeToken` IR (`EK8`) in `omnia-plugin`; implement the GPUI renderer in `omnia-app` mapping nodes onto `U1.3` non-focus-stealing chrome widgets and `omnia-theme` tokens with zero colour literals (`EINV7`) and full AccessKit roles (`UG9`); dispatch `UiEvent`s asynchronously to plugin workers (`EINV6`).
- Done when: `EINV7` and `ETC-ui-panel-theme` (including light/dark switch and `SheetView` paint isolation) pass in headless GPUI tests.

### E3.2 Retained 2D draw-list viewport and schematic overlay layer · todo
- Lane: plugin-core, plugin-ui · Depends: E2.2, T4.3 · Read: `plugin-plan.md` sections 3 (`EB4`, `EB5`), 4 (`EK9`), 6 (`ED3`, `ED4`), 7 (`EINV6`, `EINV7`), 8 (`ETC-2d-viewport-pan`)
- Owns: `crates/omnia-plugin/src/scene2d.rs`, `crates/omnia-app/src/plugins/viewport2d.rs`, `crates/omnia-canvas/src/overlay.rs` (read-only plugin overlay layer hook), `crates/omnia-app/tests/plugin_viewport2d.rs`
- Do: define `Scene2dLayer`/`DrawPrim` (`EK9`) in `omnia-plugin`; build `Retained2dCanvas` in `omnia-app` with host-side pan/zoom/culling/hit-test (0 Wasm calls during camera movement, 0 warm-frame allocations) and wire read-only schematic overlays into `omnia-canvas`.
- Done when: `ETC-2d-viewport-pan` passes with 0 guest calls during 600-frame pan/zoom and 0 warm-frame allocations from our code (`EB4`, `EB5`).

### E3.3 Workbench mode/tab host and shell contribution slots in `omnia-app` · todo
- Lane: plugin-ui · Depends: E3.1, E3.2, U1.7, U2.4 · Read: `plugin-plan.md` sections 6 (`ED3`), 10 (`omnia-app/src/plugins/`), 14 (`ui-plan.md`), `ui-plan.md` (`U1.7`, `U2.4`, `UK2`–`UK6`, `UK11`)
- Owns: `crates/omnia-app/src/plugins/{mod.rs,workbench.rs}`, `crates/omnia-app/src/workspace.rs` (workbench slot integration), `ui-plan.md` (plugin shell slot docs)
- Do: integrate the mode/tab workbench host into `omnia-app`'s workspace shell so plugins can own the central viewport and contribute scoped toolbar groups, docks (`UK11`), menus, floating frames (`UK10`) and status items; display the non-blocking missing-plugin banner when opening `.omnia` files with uninstalled `ext` namespaces (`ED5`).
- Done when: switching between Schematic and a sample Tier 1 2D workbench tab preserves schematic camera, selection and undo state (`UINV8`), respects `UK11` dock collapse rules, and leaves `SheetView` paint count at 0 during chrome hover (`UB3`).

### E3.4 Consent UX, Plugin Manager, gesture gate, dev hot-reload and `--safe-mode` · todo
- Lane: plugin-ui · Depends: E3.3, A4.2 · Read: `plugin-plan.md` sections 6 (`ED7`, `ED8`, `ED11`, `ED13`), 7 (`EINV8`, `EINV11`), 8 (`ETC-consent-gate`, `ETC-gesture-defer`, `ETC-dev-hot-reload`)
- Owns: `crates/omnia-app/src/plugins/{manager.rs,consent.rs,reload.rs}`, `crates/omnia-app/src/main.rs` (`--safe-mode` flag), `crates/omnia-app/tests/plugin_lifecycle.rs`
- Do: build the capability consent dialog (including Tier 2 native binary SHA-256 trust prompt) and Plugin Manager view (list, install, enable/disable, inspect permissions, reload faulted plugins); connect plugin write requests to the shared app gesture gate (`AINV13`, `EINV11`); implement dev-folder hot-reload (`ED8`, refusing silent permission escalations); add `--safe-mode`.
- Done when: `ETC-consent-gate` (full UI flow), `ETC-gesture-defer`, `ETC-dev-hot-reload`, `--safe-mode` and `EINV11` pass.

### E3.5 Milestone E3 gate and interactive UI review · todo → HE2
- Lane: bench, plugin-ui · Depends: E2.5, E3.4 · Read: `plugin-plan.md` sections 3 (`EB4`, `EB5`), 8 (`ETC-ui-*`, `ETC-2d-*`)
- Owns: `crates/omnia-bench/src/ui.rs` (`plugin-panel`, `plugin-overlay`, `plugin-2d-pan`, `plugin-2d-zoom` scripts), `docs/measurements/E3.5.md`
- Do: run the `EB4` and `EB5` shell bench scripts (recording non-reference numbers) and prepare a runnable demo workspace containing `starter-bundle`, `sample-erc`, `bom-csv`, and a sample Tier 1 2D workbench; pause at `HE2` for owner interactive review.
- Done when: non-reference `EB4`/`EB5` numbers are logged in `docs/measurements/E3.5.md` with 0 warm allocations and 0 `SheetView` chrome paints; owner accepts `HE2`.

---

## Milestone E4: Tier 2 sidecar workbenches, GPU surface and Orion gate

### E4.1 `omnia-plugin-ipc` sidecar supervisor, framed RPC and frame-ring transport · todo
- Lane: plugin-ipc, plugin-sdk · Depends: E0.4, E1.2 · Read: `plugin-plan.md` sections 4 (`EK10`), 6 (`ED2`, `ED7`, `ED11`, `ED13`), 7 (`EINV1`, `EINV8`, `EINV14`), 8 (`ETC-sidecar-trust`), `docs/measurements/E0.4.md`
- Owns: `Cargo.toml` (workspace member), `crates/omnia-plugin-ipc/`, `crates/omnia-plugin-sdk/src/sidecar.rs`, `THIRD_PARTY.md`
- Do: build GPUI-free `omnia-plugin-ipc` implementing SHA-256 pre-launch verification (`EINV8`), child process supervision (Windows job objects / Linux `PR_SET_PDEATHSIG` + process-group cleanup), UDS / Windows named-pipe framed RPC matching `omnia:plugin@0.1.0`, heartbeat supervision (`EK10`), and the surface transport chosen at `HE1` (GPU texture handle + shared-memory BGRA8 ring). Expose the Rust sidecar server helper in `omnia-plugin-sdk`.
- Done when: `EINV1`, `EINV8` (`ETC-sidecar-trust`), `EINV14` (malformed frames, hang, `SIGKILL`, zero handle leaks) pass on Windows and Linux CI.

### E4.2 GPU/shm surface presenter, input forwarding and sidecar crash recovery in `omnia-app` · todo
- Lane: plugin-ui · Depends: E4.1, E3.3 · Read: `plugin-plan.md` sections 3 (`EB6`, `EB9`), 6 (`ED3`, `ED4`, `ED13`), 7 (`EINV6`, `EINV14`), `docs/measurements/E0.4.md`
- Owns: `crates/omnia-app/src/plugins/surface.rs`, `crates/omnia-app/tests/plugin_sidecar_ui.rs`
- Do: implement `ExternalSurfaceView` inside `WorkbenchHost` presenting Tier 2 frames using the `HE1`-approved GPUI path (zero idle redraws when no damage arrives, `UB4`), forwarding pointer/scroll/key/resize/scale-factor events cleanly, and rendering the themed "Workbench disconnected / Restart" recovery state within 200 ms of a sidecar crash (`EB9`).
- Done when: resize, scale-factor change (`UB8`), idle 0-frame check (`UB4`) and crash-recovery banner tests pass in headless GPUI tests without affecting open schematic tabs.

### E4.3 External undo participant and `Spectare`-target reference Tier 2 workbench · todo
- Lane: plugin-ui, plugin-sdk · Depends: E4.2, E3.4 · Read: `plugin-plan.md` sections 1, 6 (`ED1`, `ED5`, `ED6`), 8 (`ETC-sidecar-workbench`)
- Owns: `crates/omnia-app/src/plugins/undo_bridge.rs`, `examples/plugins/mock-workbench-sidecar/`, `crates/omnia-app/tests/plugin_sidecar_e2e.rs`
- Do: implement the optional `UndoParticipant` workspace history bridge for Tier 2 sidecar files (`ED6`, degrading gracefully if the sidecar dies mid-session); build `mock-workbench-sidecar` exercising the full Tier 2 contract designed for `Reid-Engineered/Spectare` (live schematic/circuit subscription, declarative docks + toolbar, 60 Hz GPU/shm viewport, sidecar state persistence, paired undo, and graceful restart).
- Done when: `ETC-sidecar-workbench` passes end-to-end, including `SIGKILL` recovery with schematic state and undo history preserved (`EINV14`).

### E4.4 Tier 2 benchmarks and fault-injection hardening · todo
- Lane: bench · Depends: E4.3 · Read: `plugin-plan.md` sections 3 (`EB1`, `EB6`, `EB8`, `EB9`), 8 (`ETC-sidecar-*`)
- Owns: `crates/omnia-bench/src/ui.rs` (`plugin-sidecar-60hz`, `plugin-sidecar-idle` scripts), `crates/omnia-bench/src/start.rs` (`--plugins` option), `docs/measurements/E4.4.md`
- Do: add the `--plugins` cold-start flag (`EB1`), 5-plugin RSS benchmark (`EB8`), and Tier 2 60 Hz + idle presentation bench scripts (`EB6`); run fuzz/fault-injection over IPC frames and surface descriptors; record non-reference numbers in `docs/measurements/E4.4.md`.
- Done when: all new bench commands and fault-injection suites run cleanly and are documented in `docs/measurements/E4.4.md`.

### E4.5 Final reference-machine gate on Orion (`EB1`–`EB9`) · todo → HE3
- Lane: bench · Depends: E3.5, E4.4 · Read: `plugin-plan.md` section 3 (`EB1`–`EB9`), `canvas-plan.md` section 2.1, `ui-plan.md` (`UB1`–`UB10`)
- Owns: `docs/measurements/E4.5.md`, `docs/measurements/E4.5/` (campaign script, order file, raw data, condition snapshots)
- Do: prepare a one-command Orion reference campaign running `EB1`–`EB9` plus regression checks for `B1`, `B3`, `B4`, `B9`, `UB1`, `UB3`, `UB4`, `UB5` and `UB10` with plugins loaded and a Tier 2 workbench active; set to `needs-human` for the owner's Orion certification run (`HE3`).
- Done when: Orion results under canvas plan section 2.1 are recorded with raw data and hardware snapshots; all `EB1`–`EB9` budgets and core regressions hold; owner verifies a Tier 2 workbench (targeting `Reid-Engineered/Spectare`) and accepts `HE3`.

---

## Log

- **2026-11-25 · E0.1 · done:** Drafted [plugin-plan.md](plugin-plan.md) and [plugin-roadmap.md](plugin-roadmap.md) against `main` (`a022e6a`), recording owner architectural decisions `ED1`–`ED10` from the 2026-11-25 design session (three-tier runtime: Tier 0 bundles, Tier 1 Wasm Component Model/WIT, Tier 2 out-of-process sidecar workbenches targeting `Reid-Engineered/Spectare`; mode/tab workbench host; declarative themed UI + retained 2D draw-lists + Tier 2 shared GPU surface; namespaced `.omnia` `ext` + hybrid `Command::PluginPatch` & sidecar undo; capability consent + SHA-256 pinning; `.oplugin` ZIP/folder packaging; `omnia-plugin-sdk` + WIT + CLI). Proposed `EB1`–`EB9`, `EK1`–`EK12`, `ED11`–`ED13`, `EINV1`–`EINV14`, `ETC-*`, spikes `ES1`–`ES2`, `EQ1`–`EQ8`, and checkpoints `HE0`–`HE3`. Scanned both files for third-party company/commercial product names (`AGENTS.md` rule 1): clean. Docs only; nothing measured; no dependency added.
