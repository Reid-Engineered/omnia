# Omnia simulation: roadmap for agents

The task list for Stage 3 (circuit simulation). The reference for every `S`-prefixed id used here
(`SB`, `SK`, `SD`, `SINV`, `STC`, `SS`, `SQ`, `HS`) is [sim-plan.md](sim-plan.md). Ids without an
`S` prefix live in [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md) and [ui-plan.md](ui-plan.md). Milestones are written `Milestone S0`–
`Milestone S4` (and tasks `S0.1`–`S4.4`) to stay distinct from canvas Day-1 spikes `S1`–`S8`.

Because Omnia's pure-Rust SPICE engine already exists in `Reid-Engineered/RIS-C` (`risc-c`,
BSD-3-Clause, `OP`/`DC`/`AC`/`TRAN` validated against `ngspice 47`), Milestones S1 and S2 wire
**both** `RIS-C` and `ngspice` behind `SimEngine` directly with zero GPUI dependency, running in
parallel with Canvas M4–M7 and UI U1–U4.

## How to work a task

Follow `AGENTS.md`: pick a `todo` task whose dependencies are all `done` (a `needs-human`
dependency is not `done`), branch `task/<id>-<slug>` from current `main`, set the task to `doing`,
read everything its **Read** line names, edit only what **Owns** lists, run standard checks plus
**Done when** checks, update the status, append one dated log entry at the bottom, get an
independent review, and open one PR per task.

Statuses: `todo`, `doing`, `done`, `blocked (reason)`, `needs-human`.

## Rules

`AGENTS.md` hard rules and canvas rules 1 to 12 apply, plus five simulation rules:

1. **Both engines advance together.** Milestone S2 does not close until both `omnia-sim-ngspice`
   and `omnia-sim-risc` pass every `STC-*` circuit and differential parity (`SB7`, `SINV8`).
2. **Integrate `RIS-C` through its public API.** Keep `risc-c` pinned in `Cargo.toml`/`Cargo.lock`
   and never copy internal solver equations into Omnia (`SD6`, `RIS-C` contract rule 8).
3. **Zero GPUI in simulation crates.** `omnia-sim`, `omnia-sim-ngspice`, and `omnia-sim-risc`
   never depend on GPUI or `omnia-app` (`SINV1`).
4. **Zero bundled or static GPL/LGPL.** Bundled `ngspice` uses strictly its BSD-3-Clause core
   build (system C/compiler runtimes excepted; `SD12`, `SINV11`). `cargo deny check` stays green.
5. **Keep plans true.** Any change to a constant (`SK`), decision (`SD`), budget (`SB`) or
   signature updates `sim-plan.md` in the same pull request.

## Standard checks

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
| `sim-core` | `crates/omnia-sim/` | No |
| `sim-ngspice` | `crates/omnia-sim-ngspice/`, `crates/omnia-ngspice-worker/` | No |
| `sim-risc` | `crates/omnia-sim-risc/` (adapter over pinned `risc-c`) | No |
| `sim-tests` | `tests/sim/` (`omnia-sim-tests`), `crates/omnia-bench/benches/sim.rs`, `crates/omnia-bench/src/sim.rs` | No |
| `doc` / `assets` | `crates/omnia-doc/` (`S1.1`), `crates/omnia-symbols/` (`S1.1`, `S3.2`) | No |
| `sim-ui` | `crates/omnia-app/src/sim/`, `crates/omnia-app/src/instruments/`, `crates/omnia-canvas/src/sheet/` probe/instrument hooks (`S3.2`) | Yes |
| `sim-agent` | `crates/omnia-agent/src/sim.rs` (`S4.1`) | No |
| `bench` | `crates/omnia-bench/src/spike/sim_scope.rs` (`S0.4`), `crates/omnia-bench/src/ui.rs` sim scripts (`S4.4`), `docs/measurements/S*.md` | Yes |

Notice that `sim-risc` and `sim-ngspice` own completely disjoint crates and run in parallel!

## Milestones

| Milestone | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| Milestone S0 | `sim-plan.md`, this roadmap, owner sign-off (`S0.2`), spikes `SS1` (`S0.3`) & `SS2` (`S0.4`) | S0.2 (`HS1`), then S0.3 / S0.4 | `HS1`, `SD5`, `SK8` |
| Milestone S1 | Part props & v1 analog symbols, `omnia-sim` IR/ERC/models/netlist/rings, full `OP`/`DC`/`AC`/`TRAN` adapters for both `RIS-C` and `ngspice` | S1.5 & S1.6 | `SINV1`–`SINV7`, `SINV10`, `SINV12`, `STC-erc-01` |
| Milestone S2 | Full dual-engine differential parity (`lin`, `nl`, `ac`, `tran`, `err`), Wien bridge oscillator (`723.4 Hz`), and Orion solver bench | S2.3 | `SB1`–`SB4`, `SB7`, `SINV8`, all `STC-*` |
| Milestone S3 | Live ERC, toolbar Run controls, on-sheet probes/rails/instrument blocks, Oscilloscope (`XSC1`), Multimeter (`XMM1`), Bode Plotter (`XBP1`), Results dock | S3.6 | `SINV9`, `HS3` (plus non-reference `SB5`/`SB6`/`SB8` checks; Orion certification in `S4.4`) |
| Milestone S4 | Agent/MCP simulation tools, BSD-3-Clause `ngspice` bundling, First-boot self-test, final Orion gate | S4.4 | `SINV11`, `HS2`, `HS4`, all `SB1`–`SB8` on Orion |

## Where each task fits (cross-roadmap dependencies)

| Simulation task | Waits for (canvas / wiring / UI / MCP) | Can start |
| --- | --- | --- |
| `S0.1`–`S0.3` | nothing (`S0.3` waits on `S0.2`/`HS1`) | now (`S0.3` after `HS1`) |
| `S0.4` (`SS2`) | standalone GPUI spike in `omnia-bench` (after `S0.2`/`HS1`) | after `HS1` |
| `S1.1`–`S2.3` (entire dual-engine simulation stack!) | **nothing** beyond `HS1` (zero UI dependency!) | right after `HS1` |
| `S3.1` | `U1.6`, `U1.7`, `U1.8`, `S2.3` | after `U1.8` & `S2.3` |
| `S3.2` | `T4.3`, `T5.3`, `S1.1` | after `T5.3` |
| `S3.3` | `S0.4` (`SK8`), `S3.1`, `U4.1` | after `S0.4`, `U4.1`, & `S3.1` |
| `S3.4` | `S3.1`, `U4.1` | after `U4.1` & `S3.1` |
| `S3.5` | `S3.1`, `U1.7` | after `U1.7` & `S3.1` |
| `S4.1` | `A3.4`, `S2.3` | after `A3.4` & `S2.3` |
| `S4.3` | `U3.3`, `U3.5`, `S4.2` | after `U3.5` & `S4.2` |

## Dependency overview

```
S0.1 -> S0.2 [HS1] -> S0.3 (SS1), S0.4 (SS2)
HS1  -> S1.1 (doc+symbols) -> S1.2 (IR/values/models/netlist) -> S1.3 (ERC), S1.4 (engine+ring)
S1.4        -> S1.5 (omnia-sim-risc: OP/DC/AC/TRAN)    --+
S0.3 + S1.4 -> S1.6 (omnia-sim-ngspice: OP/DC/AC/TRAN) --+--> (+ S1.3) -> S2.1 (diff parity) -> S2.2 (Wien) -> S2.3 (Gate S2)
S2.3 + U1.6 + U1.7 + U1.8 -> S3.1 (SimController)
S3.1 + U4.1 + S0.4 -> S3.3 (XSC1)    --+
S3.1 + U4.1        -> S3.4 (XMM1+XBP1)-+--> (+ S3.2) -> S3.6 [HS3]
S3.1 + U1.7        -> S3.5 (Results) --+
S1.1 + T4.3 + T5.3 -> S3.2 (sheet probes/rails/instrument blocks)
S2.3 + A3.4 -> S4.1 (agent sim tools)
S0.3 + S2.3 -> S4.2 (ngspice bundling) [HS2] -> S4.3 (+ U3.3 + U3.5: First boot + prefs)
S3.6 + S4.1 + S4.3 -> S4.4 (Final Orion Gate) [HS4]
```

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| `HS1` | S0.2 | Reviews `sim-plan.md`, answers `SQ1`–`SQ10` (including `SQ4` on `RIS-C` repo visibility/`deny.toml`), and approves `SD6`–`SD14`. Milestone S1 starts after `HS1`. |
| `HS2` | S4.2 | Verifies the BSD-3-Clause `ngspice` build recipe, binary licence audit, and `THIRD_PARTY.md` entry on Windows and Linux. |
| `HS3` | S3.6 | Opens the Wien oscillator in Omnia, clicks Run on both `ngspice` and `RIS-C`, inspects `XSC1` live waveforms, `XMM1`, `XBP1`, and Results dock, and approves visual/functional feel. |
| `HS4` | S4.4 | Runs the final Orion certification campaign (`SB1`–`SB8` + `B3`/`B4`/`UB3`/`UB4` with live simulation). |

---

## Milestone S0: plan, sign-off and spikes

### S0.1 Simulation plan and roadmap · done
- Lane: docs · Depends: — · Read: `ROADMAP.md`, `canvas-plan.md` D8, `ui-plan.md`, `mcp-plan.md`, `design/build-omnia.mjs`, `Reid-Engineered/RIS-C` `docs/{AGENT_INTEGRATION,COMPATIBILITY,STATUS}.md`
- Owns: `sim-plan.md`, `sim-roadmap.md`, `AGENTS.md`, `ROADMAP.md`
- Do: write `sim-plan.md` and `sim-roadmap.md` incorporating the 2026-11-23 owner decisions and integrating the existing first-party `Reid-Engineered/RIS-C` (`risc-c`) crate alongside bundled `ngspice`; register `S`-prefixed ids in `AGENTS.md` and link Stage 3 in `ROADMAP.md`.
- Done when: `sim-plan.md`, `sim-roadmap.md`, `AGENTS.md` and `ROADMAP.md` agree, pass Hard Rule 1 (no commercial companies), and pass independent review.

### S0.2 Owner sign-off and plan alignment · todo → HS1
- Lane: docs · Depends: S0.1 · Read: `sim-plan.md` §13–§14
- Owns: `sim-plan.md`, `sim-roadmap.md`
- Do: present `SQ1`–`SQ10` and `SD6`–`SD14` to the owner at `HS1`; record dated answers in `sim-plan.md` and turn all `PROPOSED` decisions into `DECIDED` (only `SD5` stays `HYPOTHESIS` until `SS1`).
- Done when: `HS1` is signed off by the owner and no `PROPOSED` entries remain in `sim-plan.md`.

### S0.3 Spike SS1: `ngspice` worker isolation & BSD-3-Clause build · todo
- Lane: sim-ngspice · Depends: S0.2 · Read: `sim-plan.md` SD5, SD12, SB3, SB4, SINV11, SQ1
- Owns: `docs/measurements/S0.3.md`, spike harness in `docs/measurements/S0.3/`
- Do: build BSD-3-Clause `libngspice` (no GPL/LGPL libraries bundled or statically linked; system C/compiler runtimes excepted) on Windows and Orion Linux; benchmark worker-process IPC vs in-process `libngspice` for startup latency (`SB3`), binary double streaming rate, and kill-on-cancel reliability (`SB4`); audit linked shared libs and record exact configure flags.
- Done when: `docs/measurements/S0.3.md` records numbers and binary library audit on both Windows and Orion and flips `SD5` from `HYPOTHESIS` to `DECIDED`; if Orion measurements have not run yet, record Windows diagnostics and set `needs-human`.

### S0.4 Spike SS2: live oscilloscope ring-buffer overhead · todo
- Lane: bench · Depends: S0.2 · Read: `sim-plan.md` SB5, SB6, SK6, SK8, SINV9; `canvas-plan.md` D7, D8, G6
- Owns: `crates/omnia-bench/src/spike/sim_scope.rs`, `docs/measurements/S0.4.md`
- Do: prototype a 2-channel SPSC `SampleRing` fed at 100 kS/s by a background thread and drained by a cached floating scope view over a `1k` sheet at `30 Hz` and `60 Hz`; measure frame times, sheet paint count, and warm Omnia allocations on Orion.
- Done when: `docs/measurements/S0.4.md` records Orion numbers at `30 Hz` and `60 Hz` and locks `SK8`; if Orion measurements have not run yet, record Windows diagnostics and set `needs-human`.

---

## Milestone S1: core IR, ERC, netlist, models, and dual `SimEngine` backends

### S1.1 Document part properties & v1 analog symbol set · todo
- Lane: doc, assets · Depends: S0.2 · Read: `sim-plan.md` SD3, SD9, SD10, SINV10; `crates/omnia-doc/src/{doc.rs,command.rs,io.rs}`, `crates/omnia-symbols/{pins.toml,src/lib.rs}`, `crates/omnia-agent/src/{files.rs,catalog.rs}`
- Owns: `crates/omnia-doc/src/{doc.rs,command.rs,io.rs}`, `crates/omnia-symbols/`, `xtask/src/symbols.rs`, `crates/omnia-agent/src/{files.rs,catalog.rs}`, `ui-plan.md` (`UTC-open-bad`), `mcp-plan.md` (`AQ12`)
- Do: store `props: BTreeMap<Arc<str>, Arc<str>>` on crate-private `StoredPart` (keeping `Part`'s 5 public fields unchanged so no `Part { .. }` literals break), add `Doc::part_props(id)` and `Command::SetPartProp` (with inverse and `INV4` property coverage), bump RON serialisation to v3 (`io.rs` `Sheet` preserving v2 load per `SINV10`), bump rejected-version tests/examples from `version: 3` to `version: 4` in `io.rs`, `omnia-agent/src/files.rs`, and `ui-plan.md` (`UTC-open-bad`), and update RON v2 strings in `omnia-agent/src/{files.rs,catalog.rs}` and `mcp-plan.md` (`AQ12`); add `SymbolRole` (`Component`, `Ground`, `PowerRail`, `Instrument`, `Probe`) and compile the missing v1 analog symbols from `vendor/electronic-symbols/` (inductor, independent DC/AC/pulse sources, VCVS/VCCS dependent sources, NPN & PNP BJT, NMOS & PMOS MOSFET, supply rail tags).
- Done when: `INV4`, `INV12`, and `SINV10` tests pass; all existing `tests/golden/*.ron` v2 fixtures load unchanged and all 12 goldens match with 0 differing pixels.

### S1.2 `omnia-sim` core: values, models, `CircuitIr`, netlist emitter · todo
- Lane: sim-core · Depends: S1.1 · Read: `sim-plan.md` SK1, SK2, SK3, SK10, SK12, SD3, SD7, SQ8, SINV1, SINV2, SINV6, SINV12
- Owns: `crates/omnia-sim/Cargo.toml`, `crates/omnia-sim/models/`, `crates/omnia-sim/src/{lib.rs,value.rs,models.rs,library.rs,ir.rs,netlist.rs}`
- Do: create GPUI-free `omnia-sim` crate; implement `SK1` SI engineering value parser (whitespace, units, `1F` vs `10fF`, potentiometer `%`, bare `M`) and SPICE formatter; author the built-in permissive/public-domain `.model`/`.subckt` library (`1N4148`, `2N2222`/`2N3904`, `2N3906`, `2N7000`, `BS250`, behavioral `OpAmp`, `TL072`, `LM741` using only `R`/`C`/`E`/`G`/`D` so both `RIS-C` and `ngspice` run them identically) with precedence `props["model"]` → `part.value` → `SQ8` default; implement insertion-order-independent `CircuitIr::lower` (`SD7`, including potentiometer split and VCVS/VCCS) with bidirectional `SourceMap` and deterministic SPICE netlist emission for `OP`/`DC`/`AC`/`TRAN`.
- Done when: property tests prove `SINV2` (any arena insertion order gives identical numbering & netlist text), `SINV6` (value parser round-trip & `M`/`F` safety), `SINV12` (complete source map), and `SINV1` (no GPUI in dep tree).

### S1.3 Deterministic ERC engine · todo
- Lane: sim-core · Depends: S1.2 · Read: `sim-plan.md` SD8, SB1, SK9, SINV2, SINV3, STC-erc-01
- Owns: `crates/omnia-sim/src/erc.rs`
- Do: implement `run_erc(doc, library) -> Vec<ErcDiagnostic>` covering `ERC001`–`ERC008` (ground, duplicate non-empty designators with `Ground`/`PowerRail` empty exemption, bad values, shorted voltage sources, unconnected probes, floating pins, bare `M` on resistors, unpowered op-amp) sorted deterministically by severity, code, geometry, and item id.
- Done when: `STC-erc-01` unit tests pass on all 8 codes (`ERC001`–`ERC008`); `SINV2` determinism and `SINV3` immutability property tests pass.

### S1.4 `SimEngine` trait, lock-free `SampleRing`, and `SimResult` · todo
- Lane: sim-core · Depends: S1.2 · Read: `sim-plan.md` §10, SK6, SK7, SD11, SD13, SINV3, SINV4
- Owns: `crates/omnia-sim/src/{engine.rs,ring.rs,results.rs,measure.rs,error.rs}`
- Do: define `SimEngine`, `SimRunHandle`, `SimState`, `Analysis`, `ProbeSpec`, typed `SimError`; implement bounded lock-free SPSC `SampleRing` (`SK6`, zero post-init allocs, drop-counter on overrun); implement `SimResult` storage caps (`SK7`) and waveform measurement routines (`vpp`, `vrms`, `vavg`, `freq`, `duty`, `rise_time`, `bandwidth_3db`, `phase_margin`).
- Done when: `SINV4` allocation-counting & concurrent stress tests pass; measurement unit tests verify analytical sine/square/RC signals.

### S1.5 `omnia-sim-risc`: `SimEngine` adapter over pinned `risc-c` (`OP`/`DC`/`AC`/`TRAN`) · todo
- Lane: sim-risc · Depends: S1.4 · Read: `sim-plan.md` SD1, SD2, SD6, SD13, SQ4, SINV1, SINV5, SINV7, SINV11; `Reid-Engineered/RIS-C` `docs/AGENT_INTEGRATION.md`
- Owns: `Cargo.toml` (pinned `risc-c` git dep), `deny.toml` (`allow-git` for `RIS-C`), `.github/workflows/ci.yml` (`RIS-C` fetch credentials if still private per `SQ4`), `crates/omnia-sim-risc/`
- Do: add `https://github.com/Reid-Engineered/RIS-C` to `deny.toml` `allow-git` (per `HS1`/`SQ4`), wire CI fetch access if private, and create GPUI-free `omnia-sim-risc` wrapping `risc-c` (pinned commit SHA) behind `RiscEngine: SimEngine`; wire `OP`, single-source `DC`, `AC`, and live-streaming `TRAN` (`transient_with` pushing into `SampleRingProducer`, pause gate, `ControlFlow::Break(())` cancellation within `SK11`); map `risc_c::RunError` through `SourceMap` to typed `SimError`; enforce `SINV7` (missing signal name returns error, never `0.0`).
- Done when: `STC-lin-01` (`OP`/`DC`), `STC-ac-01` (`AC`), `STC-tran-01` (`TRAN`), and `SINV5` cancel test pass on `RIS-C`; `SINV1`, `SINV7`, and `cargo deny check` pass in CI.

### S1.6 `omnia-sim-ngspice`: worker IPC, `libngspice` backend, and CI artifact wiring (`OP`/`DC`/`AC`/`TRAN`) · todo
- Lane: sim-ngspice · Depends: S0.3, S1.4 · Read: `sim-plan.md` SD1, SD2, SD5, SD12, SD13, SQ9, SINV1, SINV5, SINV7, SINV11
- Owns: `crates/omnia-sim-ngspice/`, `crates/omnia-ngspice-worker/`, `scripts/build-ngspice.*`, `.github/workflows/ci.yml` (cached BSD-3-Clause `libngspice` CI artifact)
- Do: implement `omnia-ngspice-worker` dynamically loading BSD-3-Clause `libngspice` and speaking binary framed IPC over stdio; implement `NgspiceEngine: SimEngine` for `OP`, `DC`, `AC`, and live-streaming `TRAN` (via `SendData` callback into `SampleRingProducer`), with `SK2` options, `pause`/`resume` (`bg_halt`/`bg_run`), `SourceMap`-mapped error parsing, and `SK11` cancel/kill cleanup; automate the checksum-pinned BSD-3-Clause `libngspice` CI artifact (`SQ9`, using `S0.3`'s verified configure flags) so `S1.6`, `S2.1`, and `S2.2` run real `ngspice` tests in CI.
- Done when: `STC-lin-01`, `STC-ac-01`, `STC-tran-01` pass on `ngspice` in CI; `SINV5` cancel test leaves 0 orphan worker processes; `SINV1`, `SINV7`, and `cargo deny check` pass.

---

## Milestone S2: dual-engine differential parity, Wien oscillator, and solver gate

### S2.1 Full dual-engine differential parity suite (`omnia-sim-tests`) · todo
- Lane: sim-tests · Depends: S1.3, S1.5, S1.6 · Read: `sim-plan.md` SB7, SK2, SK4, SK5, SINV8, STC-lin-* to STC-err-01
- Owns: `Cargo.toml` (add `tests/sim` workspace member), `tests/sim/Cargo.toml` (`omnia-sim-tests`), `tests/sim/`
- Do: create `omnia-sim-tests` running both `omnia-sim-ngspice` and `omnia-sim-risc` under tightened `SK2` tolerances on `STC-lin-01`, `STC-lin-02`, `STC-nl-01`–`STC-nl-04` (diode, BJT, MOSFET, op-amp saturation), `STC-ac-01`, `STC-ac-02`, `STC-tran-01`, `STC-tran-02`, and `STC-err-01` (singular/non-convergent + mid-run Stop).
- Done when: `cargo test -p omnia-sim-tests` passes with `RIS-C` matching `ngspice` within `SK4` (linear) and `SK5` (nonlinear/AC/TRAN) on every circuit.

### S2.2 Wien bridge oscillator reference verification (`STC-wien-01`) · todo
- Lane: sim-tests · Depends: S2.1 · Read: `sim-plan.md` STC-wien-01, SB7, SK5; `tests/golden/wien-oscillator.ron`
- Owns: `tests/sim/tests/wien.rs`, `tests/sim/fixtures/wien-oscillator-sim.ron`
- Do: create the simulatable Wien bridge oscillator fixture derived from `tests/golden/wien-oscillator.ron` (`R1`/`R2 = 10 kΩ`, `C1`/`C2 = 22 nF`, `R4 = 22 kΩ`, pot `R3 = 50%` with `resistance = "20k"`, `D1`/`D2 = 1N4148`, `U1 = TL072`, adding `±12 V` supply rails and `XSC1` probes A/B), run `TRAN` on both `ngspice` and `RIS-C`, and verify clean oscillation startup, steady-state frequency (`≈ 723.4 Hz`, `<= 0.5%` diff), and amplitude (`<= 1%` diff).
- Done when: `STC-wien-01` passes deterministically on both engines in CI.

### S2.3 Gate: Milestone S2 (dual-engine parity & Orion solver bench) · todo
- Lane: sim-tests · Depends: S2.2 · Read: `sim-plan.md` SB1–SB4, SB7, SINV1–SINV8
- Owns: `crates/omnia-bench/benches/sim.rs`, `crates/omnia-bench/src/sim.rs`, `crates/omnia-bench/src/main.rs` (`bench sim` dispatch), `docs/measurements/S2.3.md`
- Do: add criterion ERC/lowering bench (`SB1`, `SB2` on `1k`/`10k`) and `bench sim` subcommand (`--circuit wien --engine ngspice|risc` and `--script cancel` for `SB3`, `SB4`); run full `STC-*` suite and record Orion numbers.
- Done when: all `STC-*` pass on both engines; `SB1`–`SB4` pass on Orion (otherwise record non-reference numbers and set `needs-human`).

---

## Milestone S3: live ERC, Run controls, and virtual instruments in `omnia-app`

### S3.1 `SimController`, toolbar Run controls, and live status-bar ERC · todo
- Lane: sim-ui · Depends: S2.3, U1.6, U1.7, U1.8 · Read: `sim-plan.md` SD1, SD2, SD8, SD11, SK9, SQ7; `ui-plan.md` UK3, UK6
- Owns: `crates/omnia-app/src/sim/{mod.rs,controller.rs,erc_bar.rs}`
- Do: wire toolbar analysis picker (`Transient`, `AC Sweep`, `DC Sweep`, `DC OP`), Run / Pause / Stop buttons, engine switcher (`ngspice` / `RIS-C preview`), live sim-time pill, and debounced (`150 ms`) background ERC indicator in the status bar with click-to-jump diagnostics; handle schematic edits while running per `SQ7`.
- Done when: simulated-input tests cover Run → Pause → Resume → Stop, ERC error blocking Run, and schematic-edit stale invalidation; `UB3` holds (0 `SheetView` paints on toolbar/sim-time updates).

### S3.2 On-sheet probes, supply rails, and instrument blocks (`XSC1`, `XMM1`, `XBP1`) · todo
- Lane: sim-ui, assets · Depends: S1.1, T4.3, T5.3 · Read: `sim-plan.md` SD10, SQ5; `canvas-plan.md` Q2; `design/build-omnia.mjs:188-235`
- Owns: `crates/omnia-symbols/` (instrument/rail art), `crates/omnia-canvas/src/sheet/instruments.rs`, `tests/golden/sim/` (rail & instrument sheet goldens; leaves T3.3's `tests/golden/wien-oscillator-*.png` unchanged)
- Do: render `+12V`/`-12V` supply rail pills (`stage/tag-bg`, `stage/tag-text`), on-sheet `XSC1`/`XMM1`/`XBP1` instrument blocks (`stage/instrument-body`), and channel A/B probe wires/dots (`probe/a`, `probe/b`); wire instruments strip placement into `Tool::Place`.
- Done when: new goldens in `tests/golden/sim/` render the Wien oscillator with supply rails and `XSC1` matching `workspace-graphite.png` while T3.3's existing goldens remain unchanged; `INV6` (no colour literals) and `B10` hold.

### S3.3 Floating Oscilloscope (`XSC1`) · todo
- Lane: sim-ui · Depends: S0.4, S3.1, U4.1 · Read: `sim-plan.md` SD4, SD11, SB5, SK6, SK8, SINV9; `ui-plan.md` UK10; `design/build-omnia.mjs:236-288`
- Owns: `crates/omnia-app/src/instruments/{mod.rs,scope.rs}`
- Do: build the floating 2-channel Oscilloscope inside `U4.1`'s frame (`576x468`): pre-allocated waveform buffer drained from `SampleRing` at `SK8` cadence (locked by `S0.4`), timebase (s/div), vertical scale (V/div) & offset per channel, edge trigger (level, rising/falling, channel A/B), measurement cursors, live frequency/Vpp readout.
- Done when: `SINV9` test proves 0 `SheetView` paints and 0 warm Omnia allocations while `XSC1` streams live samples; Windows golden of `XSC1` exists in both themes.

### S3.4 Floating Multimeter (`XMM1`) and Bode Plotter (`XBP1`) · todo
- Lane: sim-ui · Depends: S3.1, U4.1 · Read: `sim-plan.md` SD4; `ui-plan.md` UK10
- Owns: `crates/omnia-app/src/instruments/{multimeter.rs,bode.rs}`
- Do: build Multimeter (`XMM1`: DC/AC V, DC/AC I, R mode buttons + numeric readout) and Bode Plotter (`XBP1`: log-frequency magnitude dB + phase deg dual plot from `AcResult`, cursors, `-3 dB` & phase-margin markers) inside `U4.1` frames.
- Done when: unit and GPUI test-platform tests verify accurate readouts on `STC-lin-01` and `STC-ac-01`; goldens exist in both themes; `UB3` holds.

### S3.5 Results panel (bottom dock) · todo
- Lane: sim-ui · Depends: S3.1, U1.7 · Read: `sim-plan.md` SD4, SD13; `ui-plan.md` UK11
- Owns: `crates/omnia-app/src/sim/results_panel.rs`
- Do: populate `U1.7`'s bottom Results dock with tabbed Views: multi-trace waveform plot (for `TRAN`/`AC`/`DC`), Operating Point & Measurements table, Diagnostics/ERC tab (click row focuses item on sheet), and CSV export.
- Done when: tests cover plotting `OP`/`TRAN`/`AC`/`DC` results, clicking an ERC diagnostic to jump to the item, and deterministic CSV export.

### S3.6 Gate: Milestone S3 (live Wien oscillator & instruments check) · todo → HS3
- Lane: bench, sim-ui · Depends: S3.2, S3.3, S3.4, S3.5 · Read: `sim-plan.md` SINV9, STC-wien-01
- Owns: `tests/golden/sim/`, `docs/measurements/S3.6.md`
- Do: capture Windows goldens for the full workspace running the Wien oscillator with `XSC1` open in both themes; verify `SINV9` / `UB3` (0 sheet paints) and `UB4` (0 idle frames when stopped) locally; stop at `HS3` for the owner's live interactive sign-off on both `ngspice` and `RIS-C`. (Orion certification of `SB5`/`SB6`/`SB8` runs in `S4.4`.)
- Done when: goldens pass, `HS3` is approved by the owner, and non-reference diagnostics are recorded in `docs/measurements/S3.6.md`.

---

## Milestone S4: agent simulation tools, bundling, and final Orion gate

### S4.1 Agent / MCP simulation tools (`omnia-agent`) · todo
- Lane: sim-agent · Depends: S2.3, A3.4 · Read: `sim-plan.md` SD14, SQ10; `mcp-plan.md` §1, AD7, AD8, AD12, AINV5, §10–§11
- Owns: `crates/omnia-agent/src/sim.rs`, `docs/agent/sim.md`, `mcp-plan.md` (§1 out-of-scope update, catalog, `AD7` note for `sim.run` bounded worker execution, and `sim.*`-scoped finite-number amendment to `AD8`/`AD12`)
- Do: update `mcp-plan.md` (§1 simulation bullet, `AD7` worker policy, `AD8`/`AD12` `sim.*`-scoped finite canonical JSON numbers); add `sim.erc` (tier 0), `sim.run` (tier 0 + bounded worker process execution as an `A3` cancellable job with progress updates and `engine: ngspice|risc`), `sim.results` (tier 0), and `sim.measure` (tier 0) to `omnia-agent` catalog and schemas (`AINV11`), exposed automatically over CLI and MCP stdio.
- Done when: end-to-end headless MCP test builds an RC filter or Wien oscillator, runs `sim.erc`, runs `sim.run` on both engines, cancels a run cleanly, verifies `AINV5` canonical JSON and `NaN`/`Inf` rejection, and asserts `sim.measure` frequency within `SK5`.

### S4.2 Bundled `ngspice` distribution packaging & final licence audit · todo → HS2
- Lane: sim-ngspice · Depends: S0.3, S2.3 · Read: `sim-plan.md` SD12, SINV11, SQ9
- Owns: `scripts/build-ngspice.*` (release bundle packaging), `THIRD_PARTY.md`
- Do: finalise reproducible BSD-3-Clause `libngspice` + `omnia-ngspice-worker` release bundles for Windows and Linux (building on `S1.6`'s CI build script; checksum-pinned, zero GPL/LGPL libraries bundled or statically linked, system C/compiler runtimes excepted) and record full `ngspice` + `RIS-C` attribution in `THIRD_PARTY.md`.
- Done when: release bundles pass on Windows and Linux and pass the linked-library audit; `HS2` signed off by owner.

### S4.3 First-boot self-test & Preferences simulator settings · todo
- Lane: sim-ui · Depends: S4.2, U3.3, U3.5 · Read: `sim-plan.md` SQ6; `design/build-omnia.mjs:536-537`
- Owns: `crates/omnia-app/src/sim/selftest.rs`, `crates/omnia-app/src/screens/first_boot.rs`, `crates/omnia-app/src/dialogs/preferences.rs`
- Do: wire First-boot simulator rows (`ngspice` bundled self-test showing elapsed time on this machine; `RIS-C` preview option) and Preferences default-engine / tolerance settings.
- Done when: First-boot self-test runs `STC-lin-01` in `< 500 ms` off the UI thread and displays verified status; Preferences round-trip persists cleanly.

### S4.4 Gate: Milestone S4 — final Orion certification · todo → HS4
- Lane: bench · Depends: S3.6, S4.1, S4.3 · Read: `sim-plan.md` SB1–SB8, SINV1–SINV12; `canvas-plan.md` §2.1
- Owns: `crates/omnia-bench/src/ui.rs` (`sim-live` bench script), `crates/omnia-bench/src/main.rs` (`--sim-idle` flag on `cmd_idle`), `docs/measurements/S4.4/`, `docs/measurements/S4.4.md`
- Do: add `bench ui --sheet 1k --script sim-live` and `bench idle --shell --sim-idle`; freeze the Milestone S4 Orion campaign script running `SB1`–`SB8` plus full `B3`/`B4`/`UB3`/`UB4` checks with live Wien oscillator streaming into `XSC1` on both `ngspice` and `RIS-C`; run ten interleaved repetitions at `100%` and `125%` on Orion (`HS4`).
- Done when: all `SB1`–`SB8` and `SINV1`–`SINV12` pass on Orion and `HS4` is accepted by the owner.

---

## Log

```
2026-11-23 S0.1 done — Authored `sim-plan.md` and `sim-roadmap.md` against Omnia `main`
                       (`b046bbb`) and `Reid-Engineered/RIS-C` (`24710eaf`, BSD-3-Clause),
                       incorporating the 2026-11-23 owner decisions: dual engines (`ngspice`
                       bundled + pinned `risc-c` adapter `omnia-sim-risc`) in parallel from
                       Milestone S1 with `ngspice` as differential oracle (`SD1`); full analog
                       core (`OP`, `TRAN`, `AC`, single-source `DC`, `SD2`); passives
                       (including potentiometer), sources and VCVS/VCCS, diode, BJT, MOSFET,
                       behavioral/macromodel op-amp `.subckt`s (`SD3`); Oscilloscope (`XSC1`),
                       Multimeter (`XMM1`), Bode Plotter (`XBP1`), and Results dock (`SD4`).
                       Registered `S`-prefixed ids (`SB`, `SK`, `SD`, `SINV`, `STC`, `SS`,
                       `SQ`, `HS`, `S0.1`…) in `AGENTS.md` (also bringing Hard Rule 4 up to
                       date with `HA*` and `UH*`) and linked Stage 3 in `ROADMAP.md`. Addressed
                       all independent review findings (F1–F17) from Claude Sonnet 5.5 and
                       rewrote the roadmap around the existing `RIS-C` crate (23 tasks across
                       Milestones S0–S4 instead of re-implementing a SPICE solver in-repo).
                       Next step: `S0.2` (`HS1`) owner review of `SQ1`–`SQ10` and `SD6`–`SD14`.
                       No code or dependency changes.
```
