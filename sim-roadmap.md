# Omnia simulation: roadmap for agents

The task list for Stage 3 (circuit simulation). The reference for every `S`-prefixed id used here
(`SB`, `SK`, `SD`, `SINV`, `STC`, `SS`, `SQ`, `HS`) is [sim-plan.md](sim-plan.md). Ids without an
`S` prefix live in [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md) and [ui-plan.md](ui-plan.md).

This roadmap runs beside the canvas, wiring, agent and UI roadmaps. Milestones S1 to S3 are pure
Rust + `ngspice` with **zero GPUI dependency** and can run in parallel with Canvas M4–M7 and UI
U1–U4.

## How to work a task

Follow `AGENTS.md`: pick a `todo` task whose dependencies are all `done` (a `needs-human`
dependency is not `done`), branch `task/<id>-<slug>` from current `main`, set the task to `doing`,
read everything its **Read** line names, edit only what **Owns** lists, run standard checks plus
**Done when** checks, update the status, append one dated log entry at the bottom, get an
independent review, and open one PR per task.

Statuses: `todo`, `doing`, `done`, `blocked (reason)`, `needs-human`.

## Rules

`AGENTS.md` hard rules and canvas rules 1 to 12 apply, plus four simulation rules:

1. **Both engines advance together.** No analysis milestone (S1, S2, S3) closes until both
   `omnia-sim-ngspice` and `omnia-risc` pass the milestone's `STC-*` circuits and differential
   parity (`SB7`, `SINV8`).
2. **Zero GPUI in simulation crates.** `omnia-sim`, `omnia-sim-ngspice`, and `omnia-risc` never
   depend on GPUI or `omnia-app` (`SINV1`).
3. **Zero GPL/LGPL.** Bundled `ngspice` uses strictly its BSD-3-Clause core build (`SD12`,
   `SINV11`). `cargo deny check` stays green.
4. **Keep plans true.** Any change to a constant (`SK`), decision (`SD`), budget (`SB`) or
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
| `sim-risc` | `crates/omnia-risc/` | No |
| `sim-tests` | `tests/sim/`, differential harness, `crates/omnia-bench/benches/sim.rs` | No |
| `doc` / `assets` | `crates/omnia-doc/` (`S1.1`), `crates/omnia-symbols/` (`S1.1`, `S4.2`) | No |
| `sim-ui` | `crates/omnia-app/src/sim/`, `crates/omnia-app/src/instruments/`, `crates/omnia-canvas/src/sheet/` probe/instrument hooks (`S4.2`) | Yes |
| `sim-agent` | `crates/omnia-agent/src/sim.rs` (`S5.1`) | No |

Notice that `sim-ngspice` and `sim-risc` own completely disjoint crates and can always run in
parallel!

## Milestones

| # | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| S0 | `sim-plan.md`, this roadmap, owner sign-off, spikes `SS1` & `SS2` | S0.4 | `HS1`, `SD5`, `SK8` |
| S1 | Part props & symbols, `omnia-sim` IR/ERC/netlist/rings, linear `OP` + `DC` on both `ngspice` and `RIS-C` | S1.7 | `SB1`, `SB2`, `SINV1`–`SINV7`, `STC-erc-01`, `STC-lin-*` |
| S2 | Diode, BJT, MOSFET, OpAmp models + nonlinear `OP`/`DC` on both engines | S2.4 | `SB7` (nonlinear DC), `STC-nl-*` |
| S3 | `AC` sweep & live-streaming `TRAN` on both engines + Wien oscillator parity | S3.5 | `SB3`, `SB4`, `SB7` (full), `STC-ac-*`, `STC-tran-*`, `STC-wien-01`, `STC-err-01` |
| S4 | Live ERC, toolbar Run controls, on-sheet probes, Oscilloscope, Multimeter, Bode Plotter, Results dock | S4.6 | `SB5`, `SB6`, `SB8`, `SINV9`, `HS3` |
| S5 | Agent/MCP simulation tools, BSD-3-Clause `ngspice` bundling, First-boot self-test, final Orion gate | S5.4 | `SINV11`, `HS2`, `HS4`, all `SB1`–`SB8` |

## Where each task fits (cross-roadmap dependencies)

| Simulation task | Waits for (canvas / wiring / UI / MCP) | Can start |
| --- | --- | --- |
| `S0.1`–`S0.3` | nothing | now (`S0.3` after `HS1`) |
| `S0.4` (`SS2`) | `U4.1` or `T7.3` (or standalone GPUI spike in `omnia-bench`) | after `HS1` |
| `S1.1`–`S3.5` (entire solver stack!) | **nothing** beyond `HS1` (zero UI dependency!) | right after `HS1` |
| `S4.1` | `U1.6`, `U1.7`, `U1.8`, `S3.5` | after `U1.8` & `S3.5` |
| `S4.2` | `T4.3`, `T5.3`, `S1.1` | after `T5.3` |
| `S4.3`–`S4.5` | `U4.1`, `U1.7`, `S4.1` | after `U4.1` & `S4.1` |
| `S5.1` | `A3.4`, `S3.5` | after `A3.4` & `S3.5` |
| `S5.3` | `U3.3`, `U3.5`, `S5.2` | after `U3.5` & `S5.2` |

## Dependency overview

```
S0.1 -> S0.2 [HS1] -> S0.3 (SS1), S0.4 (SS2)
HS1  -> S1.1 (doc+symbols) -> S1.2 (IR/value/netlist) -> S1.3 (ERC), S1.4 (engine+ring)
S0.3 + S1.4 -> S1.5 (ngspice OP/DC)  --+--> S1.7 (Gate S1)
S1.4        -> S1.6 (RIS-C OP/DC)    --+
S1.7 -> S2.1 (models) -> S2.2 (ngspice NL), S2.3 (RIS-C NL) -> S2.4 (Gate S2)
S2.4 -> S3.1 (ngspice AC/TRAN)       --+--> S3.4 (Wien + ref) -> S3.5 (Gate S3)
S2.4 -> S3.2 (RIS-C AC), S3.3 (TRAN) --+
S3.5 + U1.8 -> S4.1 (SimController) -> S4.3 (XSC1), S4.4 (XMM1+XBP1), S4.5 (Results) -> S4.6 [HS3]
S1.1 + T5.3 -> S4.2 (sheet probes/rails/instrument blocks) -> S4.6
S3.5 + A3.4 -> S5.1 (agent sim tools)
S0.3 + S3.5 -> S5.2 (ngspice bundling) [HS2] -> S5.3 (First boot + prefs)
S4.6 + S5.1 + S5.3 -> S5.4 (Final Orion Gate) [HS4]
```

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| `HS1` | S0.2 | Reviews `sim-plan.md`, answers `SQ1`–`SQ10` (or accepts defaults), and approves `SD5`–`SD14`. Milestone S1 starts after `HS1`. |
| `HS2` | S5.2 | Verifies the BSD-3-Clause `ngspice` build recipe, binary licence audit, and `THIRD_PARTY.md` entry on Windows and Linux. |
| `HS3` | S4.6 | Opens the Wien oscillator in Omnia, clicks Run on both `ngspice` and `RIS-C`, inspects `XSC1` live waveforms, `XMM1`, `XBP1`, and Results dock, and approves visual/functional feel. |
| `HS4` | S5.4 | Runs the final Orion certification campaign (`SB1`–`SB8` + `B3`/`B4`/`UB3`/`UB4` with live simulation). |

---

## Milestone S0: plan, sign-off and spikes

### S0.1 Simulation plan and roadmap · done
- Lane: docs · Depends: — · Read: `ROADMAP.md`, `canvas-plan.md` D8, `ui-plan.md`, `mcp-plan.md`, `design/build-omnia.mjs`
- Owns: `sim-plan.md`, `sim-roadmap.md`, `AGENTS.md`, `ROADMAP.md`
- Do: write `sim-plan.md` and `sim-roadmap.md` incorporating the 2026-11-23 owner answers (dual engines `ngspice` + `RIS-C` from S1; OP/TRAN/AC/DC; analog core components; Oscilloscope + Multimeter + Bode Plotter + Results panel); register `S`-prefixed ids in `AGENTS.md` and link in `ROADMAP.md`.
- Done when: `sim-plan.md`, `sim-roadmap.md`, `AGENTS.md` and `ROADMAP.md` agree, pass Hard Rule 1 (no commercial companies), and pass independent review.

### S0.2 Owner sign-off and plan alignment · todo → HS1
- Lane: docs · Depends: S0.1 · Read: `sim-plan.md` §13–§14
- Owns: `sim-plan.md`, `sim-roadmap.md`, plus any plan alignments listed in `sim-plan.md` §13
- Do: present `SQ1`–`SQ10` and `SD5`–`SD14` to the owner at `HS1`; record dated answers in `sim-plan.md` and turn `PROPOSED` decisions into `DECIDED`.
- Done when: `HS1` is signed off by the owner and no `PROPOSED` entries remain in `sim-plan.md`.

### S0.3 Spike SS1: `ngspice` worker isolation & BSD-3-Clause build · todo
- Lane: sim-ngspice · Depends: S0.2 · Read: `sim-plan.md` SD5, SD12, SB3, SB4, SINV11, SQ1
- Owns: `docs/measurements/S0.3.md`, spike harness in `crates/omnia-bench/` (or `docs/measurements/S0.3/`)
- Do: build BSD-3-Clause `libngspice` (no GPL readline, no LGPL XSPICE/CIDER/OpenMP) on Windows and Orion Linux; benchmark worker-process IPC vs in-process `libngspice` for startup latency (`SB3`), binary double streaming rate, and kill-on-cancel reliability (`SB4`); audit linked shared libs for zero GPL/LGPL.
- Done when: `docs/measurements/S0.3.md` records numbers and binary symbol audit on Windows and Orion; `SD5` moves from `HYPOTHESIS` to `DECIDED`.

### S0.4 Spike SS2: live oscilloscope ring-buffer overhead · todo
- Lane: bench · Depends: S0.2 · Read: `sim-plan.md` SB5, SB6, SK6, SK8, SINV9; `canvas-plan.md` D7, D8, G6
- Owns: `crates/omnia-bench/src/spike/sim_scope.rs`, `docs/measurements/S0.4.md`
- Do: prototype a 2-channel SPSC `SampleRing` fed at 100 kS/s by a background thread and drained by a cached floating scope view over a `1k` sheet at `30 Hz` and `60 Hz`; measure frame times, sheet paint count, and warm Omnia allocations on Orion.
- Done when: `docs/measurements/S0.4.md` records Orion numbers at `30 Hz` and `60 Hz` and locks `SK8`.

---

## Milestone S1: core IR, ERC, netlist, and linear DC on both engines

### S1.1 Document part properties & v1 analog symbol set · todo
- Lane: doc, assets · Depends: S0.2 · Read: `sim-plan.md` SD3, SD9, SD10, SINV10; `crates/omnia-doc/src/{doc.rs,command.rs,io.rs}`, `crates/omnia-symbols/src/lib.rs`
- Owns: `crates/omnia-doc/src/{doc.rs,command.rs,io.rs}`, `crates/omnia-symbols/`, `xtask/src/symbols.rs`
- Do: add `Part::props: BTreeMap<Arc<str>, Arc<str>>` (empty default) + `Command::SetPartProp` (with inverse and `INV4` property coverage) + RON v3 preserving v2 load (`SINV10`); add `SymbolRole` (`Component`, `Ground`, `PowerRail`, `Instrument`, `Probe`) and compile the remaining v1 analog symbols from `vendor/electronic-symbols/` (inductor, DC/AC/pulse sources, PNP BJT, NMOS/PMOS MOSFET, supply rail tags).
- Done when: `INV4`, `INV12`, and `SINV10` tests pass; all existing `tests/golden/*.ron` v2 fixtures load unchanged and all 12 goldens match with 0 differing pixels.

### S1.2 `omnia-sim` core: values, models, `CircuitIr`, netlist emitter · todo
- Lane: sim-core · Depends: S1.1 · Read: `sim-plan.md` SK1, SK2, SK10, SK12, SD7, SINV1, SINV2, SINV6, SINV12
- Owns: `crates/omnia-sim/Cargo.toml`, `crates/omnia-sim/src/{lib.rs,value.rs,models.rs,ir.rs,netlist.rs}`
- Do: create GPUI-free `omnia-sim` crate; implement `SK1` SI engineering value parser and SPICE formatter; implement symbol-to-device lowering (`CircuitIr::lower`) with canonical deterministic node/device numbering and bidirectional `SourceMap`; emit canonical SPICE netlists for `OP`/`DC`/`AC`/`TRAN`.
- Done when: property tests prove `SINV2` (order-independent lowering & netlist text), `SINV6` (value parser round-trip & `M` vs `Meg` safety), `SINV12` (complete source map), and `SINV1` (no GPUI in dep tree).

### S1.3 Deterministic ERC engine · todo
- Lane: sim-core · Depends: S1.2 · Read: `sim-plan.md` SD8, SB1, SK9, SINV2, SINV3, STC-erc-01
- Owns: `crates/omnia-sim/src/erc.rs`
- Do: implement `run_erc(doc, library) -> Vec<ErcDiagnostic>` covering `ERC001`–`ERC008` (ground, duplicate designators, bad values, shorted voltage sources, unconnected probes, floating pins, bare `M` on resistors, unpowered op-amp) sorted deterministically by severity, code, and item id; add criterion bench for `1k` and `10k` (`SB1`).
- Done when: `STC-erc-01` unit tests pass on every code; `SINV2` determinism and `SINV3` immutability property tests pass; `1k`/`10k` bench runs.

### S1.4 `SimEngine` trait, lock-free `SampleRing`, and `SimResult` · todo
- Lane: sim-core · Depends: S1.2 · Read: `sim-plan.md` §10, SK6, SK7, SD11, SD13, SINV3, SINV4
- Owns: `crates/omnia-sim/src/{engine.rs,ring.rs,results.rs,measure.rs,error.rs}`
- Do: define `SimEngine`, `SimRunHandle`, `SimState`, `Analysis`, `ProbeSpec`, typed `SimError`; implement bounded lock-free SPSC `SampleRing` (`SK6`, zero post-init allocs, drop-counter on overrun); implement `SimResult` storage caps (`SK7`) and waveform measurement routines (`vpp`, `vrms`, `vavg`, `freq`, `duty`, `rise_time`, `bandwidth_3db`, `phase_margin`).
- Done when: `SINV4` allocation-counting & concurrent stress tests pass; measurement unit tests verify analytical sine/square/RC signals.

### S1.5 `omnia-sim-ngspice`: worker IPC and linear `OP`/`DC` · todo
- Lane: sim-ngspice · Depends: S0.3, S1.4 · Read: `sim-plan.md` SD1, SD5, SD12, SD13, SINV1, SINV5, SINV11
- Owns: `crates/omnia-sim-ngspice/`, `crates/omnia-ngspice-worker/`
- Do: implement `omnia-ngspice-worker` dynamically loading BSD-3-Clause `libngspice` and speaking binary framed IPC over stdio; implement `NgspiceEngine: SimEngine` for `OP` and `DC` sweep with `SK2` options, `SourceMap`-mapped error parsing, and `SK11` cancel/kill cleanup.
- Done when: `STC-lin-01` and `STC-lin-02` pass against analytical values to `SK4`; `SINV5` cancel test leaves 0 orphan worker processes; `SINV1` and `cargo deny check` pass.

### S1.6 `omnia-risc`: sparse MNA + LU and linear `OP`/`DC` · todo
- Lane: sim-risc · Depends: S1.4 · Read: `sim-plan.md` SD1, SD6, SK2, SK4, SQ4, SINV1, SINV5, SINV7
- Owns: `crates/omnia-risc/`
- Do: create pure-Rust `omnia-risc` crate implementing `RiscEngine: SimEngine`; build sparse CSC MNA assembler + symbolic ordering + numeric sparse LU with partial pivoting; stamp `R`, `V`, `I`, `VCVS`, `VCCS`, `CCVS`, `CCCS`; implement `OP` and `DC` sweep with cooperative cancellation.
- Done when: `STC-lin-01` and `STC-lin-02` pass against analytical values to `SK4`; `SINV7` KCL/KVL residual check holds on random linear resistor/source networks; `SINV1` passes.

### S1.7 Gate: Milestone S1 (linear DC differential parity & ERC bench) · todo
- Lane: sim-tests · Depends: S1.3, S1.5, S1.6 · Read: `sim-plan.md` SB1, SB2, SB7, SK4, SINV8, STC-lin-01, STC-lin-02
- Owns: `tests/sim/`, `crates/omnia-bench/benches/sim.rs`, `docs/measurements/S1.7.md`
- Do: build the dual-engine differential test harness running both `ngspice` and `RIS-C` on `STC-lin-01`, `STC-lin-02`, and randomized linear resistive networks; benchmark ERC (`SB1`) and lowering (`SB2`) on `1k` and `10k`.
- Done when: `RIS-C` matches `ngspice` within `SK4` (`1e-7`) on every linear test; `SB1`/`SB2` measured on Orion (or labelled non-reference and set `needs-human` until Orion run).

---

## Milestone S2: nonlinear devices (`OP` and `DC`) on both engines

### S2.1 Built-in semiconductor & op-amp model library · todo
- Lane: sim-core · Depends: S1.7 · Read: `sim-plan.md` SD3, SK3, SQ8
- Owns: `crates/omnia-sim/src/library.rs`, `crates/omnia-sim/models/`
- Do: author permissive/public-domain device parameters and subcircuits for `1N4148` (diode), `2N2222`/`2N3904` (NPN), `2N3906` (PNP), `2N7000` (NMOS), `BS250` (PMOS), behavioral `OpAmp` (`SQ8`), `TL072`, and `LM741`; wire `Part::props["model"]` resolution and SPICE `.model`/`.subckt` emission.
- Done when: every built-in model parses cleanly in `omnia-sim` and validates against `ngspice` syntax with zero warnings; deterministic emission verified (`SINV2`).

### S2.2 `omnia-sim-ngspice`: nonlinear `OP`/`DC` & convergence diagnostics · todo
- Lane: sim-ngspice · Depends: S2.1 · Read: `sim-plan.md` SD13, STC-nl-01 to STC-nl-04, STC-err-01
- Owns: `crates/omnia-sim-ngspice/src/`
- Do: wire `.model`/`.subckt` loading, nonlinear `OP`/`DC` sweeps, and stderr diagnostic parsing into typed `SimError::SingularMatrix` / `NoConvergence` mapped via `SourceMap`.
- Done when: `STC-nl-01` through `STC-nl-04` and singular-matrix `STC-err-01` pass on `ngspice`.

### S2.3 `omnia-risc`: Newton-Raphson solver & nonlinear device stamps · todo
- Lane: sim-risc · Depends: S2.1 · Read: `sim-plan.md` SD6, SK2, SK3, SK5, SINV7, STC-nl-01 to STC-nl-04
- Owns: `crates/omnia-risc/src/{nr.rs,devices/}`
- Do: implement damped Newton-Raphson with `pnjlim`/`fetlim` voltage limiting, `GMIN` stepping, and source stepping; implement Diode (Shockley + series resistance + breakdown), BJT (Ebers-Moll), MOSFET (Level 1 Shichman-Hodges), and behavioral OpAmp (smooth tanh rail clamping + finite `A_OL`, `R_in`, `R_out`) stamps; reuse symbolic LU pattern across NR iterations.
- Done when: `STC-nl-01` through `STC-nl-04` converge cleanly and pass `SINV7` KCL/KVL residual checks.

### S2.4 Gate: Milestone S2 (nonlinear differential parity) · todo
- Lane: sim-tests · Depends: S2.2, S2.3 · Read: `sim-plan.md` SB7, SK5, SINV8, STC-nl-01 to STC-nl-04
- Owns: `tests/sim/nonlinear.rs`, `docs/measurements/S2.4.md`
- Do: run `STC-nl-01` (diode sweep), `STC-nl-02` (BJT CE amp), `STC-nl-03` (MOSFET inverter), and `STC-nl-04` (inverting op-amp including rail saturation) on both `ngspice` and `RIS-C` and record max relative/absolute diffs.
- Done when: `RIS-C` matches `ngspice` within `SK5` (`1e-4` rel + `1e-6` abs) across every sweep point on all 4 circuits.

---

## Milestone S3: AC small-signal & Transient streaming on both engines

### S3.1 `omnia-sim-ngspice`: `AC` sweep & live-streaming `TRAN` · todo
- Lane: sim-ngspice · Depends: S2.4 · Read: `sim-plan.md` SD2, SD5, SD11, SB3, SB4, SK6, SK7, SINV4, SINV5
- Owns: `crates/omnia-sim-ngspice/src/`, `crates/omnia-ngspice-worker/src/`
- Do: support `.ac` complex vector extraction (`AcResult`) and `.tran` streaming via `libngspice`'s `SendData` callback over worker binary frames directly into `SampleRingProducer`, with `pause`/`resume` (`bg_halt`/`bg_run`) and instant hard cancel (`SB4`).
- Done when: `STC-ac-01`, `STC-ac-02`, `STC-tran-01`, `STC-tran-02`, and `STC-err-01` cancel pass on `ngspice` with live samples arriving before run completion.

### S3.2 `omnia-risc`: `AC` small-signal complex MNA solver · todo
- Lane: sim-risc · Depends: S2.4 · Read: `sim-plan.md` SD6, SK5, STC-ac-01, STC-ac-02
- Owns: `crates/omnia-risc/src/ac.rs`
- Do: linearise nonlinear devices at DC `OP` (`g_d`, `g_m`, `g_ds`, junction/Miller capacitances), build complex sparse MNA `(G + j ω C) X = I` (`num-complex`), and solve across `Dec`/`Oct`/`Lin` frequency grids.
- Done when: `STC-ac-01` and `STC-ac-02` match analytical curves and `ngspice` within `0.05 dB` magnitude and `0.5°` phase (`SK5`).

### S3.3 `omnia-risc`: `TRAN` adaptive integrator & live `SampleRing` streaming · todo
- Lane: sim-risc · Depends: S2.4 · Read: `sim-plan.md` SD6, SD11, SK2, SK6, SK7, SINV4, SINV5, SINV7
- Owns: `crates/omnia-risc/src/tran.rs`
- Do: implement transient time-stepping from DC `OP` (or `.ic`/`uic`) with Backward Euler startup, Trapezoidal + Gear-2 companion models for `C`, `L`, and nonlinear junction capacitances, LTE adaptive timestep control, exact `PULSE`/`SIN` breakpoint queue, pause/resume/cancel atomics, and live push into `SampleRingProducer`.
- Done when: `STC-tran-01` and `STC-tran-02` match `ngspice` within `SK5`, `SINV7` holds at every accepted timestep, and `SINV5` cancel completes in `< 100 ms`.

### S3.4 Wien bridge oscillator reference verification (`STC-wien-01`) · todo
- Lane: sim-tests · Depends: S3.1, S3.2, S3.3 · Read: `sim-plan.md` STC-wien-01, SB7, SK5; `tests/golden/wien-oscillator.ron`
- Owns: `tests/sim/wien.rs`, `tests/sim/fixtures/wien-oscillator-sim.ron`
- Do: create the simulatable Wien bridge oscillator fixture (derived from `tests/golden/wien-oscillator.ron` with `±12 V` supply rails and `XSC1` probes A/B), run `TRAN` on both `ngspice` and `RIS-C`, and verify clean oscillation startup, steady-state frequency (`≈ 1.59 kHz`, `<= 0.5%` diff), and amplitude (`<= 1%` diff).
- Done when: `STC-wien-01` passes deterministically on both engines in CI.

### S3.5 Gate: Milestone S3 (full analog core parity & Orion solver bench) · todo
- Lane: sim-tests · Depends: S3.4 · Read: `sim-plan.md` SB1–SB4, SB7, SINV1–SINV8
- Owns: `crates/omnia-bench/src/sim.rs`, `docs/measurements/S3.5.md`
- Do: add `bench sim` subcommand measuring `SB1`–`SB4` across both engines; run full `STC-*` differential suite (`lin`, `nl`, `ac`, `tran`, `wien`, `err`) and record Orion numbers.
- Done when: all `STC-*` pass on both engines; `SB1`–`SB4` pass on Orion (or set `needs-human` pending Orion run).

---

## Milestone S4: live ERC, Run controls, and virtual instruments in `omnia-app`

### S4.1 `SimController`, toolbar Run controls, and live status-bar ERC · todo
- Lane: sim-ui · Depends: S3.5, U1.6, U1.7, U1.8 · Read: `sim-plan.md` SD1, SD2, SD8, SD11, SK9, SQ7; `ui-plan.md` UK3, UK6
- Owns: `crates/omnia-app/src/sim/{mod.rs,controller.rs,erc_bar.rs}`
- Do: wire toolbar analysis picker (`Transient`, `AC Sweep`, `DC Sweep`, `DC OP`), Run / Pause / Stop buttons, engine switcher (`ngspice` / `RIS-C preview`), live sim-time pill, and debounced (`150 ms`) background ERC indicator in the status bar with click-to-jump diagnostics; handle schematic edits while running per `SQ7`.
- Done when: simulated-input tests cover Run → Pause → Resume → Stop, ERC error blocking Run, and schematic-edit stale invalidation; `UB3` holds (0 `SheetView` paints on toolbar/sim-time updates).

### S4.2 On-sheet probes, supply rails, and instrument blocks (`XSC1`, `XMM1`, `XBP1`) · todo
- Lane: sim-ui, assets · Depends: S1.1, T4.3, T5.3 · Read: `sim-plan.md` SD10, SQ5; `canvas-plan.md` Q2; `design/build-omnia.mjs:188-235`
- Owns: `crates/omnia-symbols/` (instrument/rail art), `crates/omnia-canvas/src/sheet/instruments.rs`
- Do: render `+12V`/`-12V` supply rail pills (`stage/tag-bg`, `stage/tag-text`), on-sheet `XSC1`/`XMM1`/`XBP1` instrument blocks (`stage/instrument-body`), and channel A/B probe wires/dots (`probe/a`, `probe/b`); wire instruments strip placement into `Tool::Place`.
- Done when: goldens render the Wien oscillator with supply rails and `XSC1` matching `workspace-graphite.png`; `INV6` (no colour literals) and `B10` hold.

### S4.3 Floating Oscilloscope (`XSC1`) · todo
- Lane: sim-ui · Depends: S4.1, U4.1 · Read: `sim-plan.md` SD4, SD11, SB5, SK6, SK8, SINV9; `ui-plan.md` UK10; `design/build-omnia.mjs:236-288`
- Owns: `crates/omnia-app/src/instruments/{mod.rs,scope.rs}`
- Do: build the floating 2-channel Oscilloscope inside `U4.1`'s frame (`576x468`): pre-allocated waveform buffer drained from `SampleRing` at `SK8` cadence, timebase (s/div), vertical scale (V/div) & offset per channel, edge trigger (level, rising/falling, channel A/B), measurement cursors, live frequency/Vpp readout.
- Done when: `SINV9` test proves 0 `SheetView` paints and 0 warm Omnia allocations while `XSC1` streams live samples; Windows golden of `XSC1` exists in both themes.

### S4.4 Floating Multimeter (`XMM1`) and Bode Plotter (`XBP1`) · todo
- Lane: sim-ui · Depends: S4.1, U4.1 · Read: `sim-plan.md` SD4; `ui-plan.md` UK10
- Owns: `crates/omnia-app/src/instruments/{multimeter.rs,bode.rs}`
- Do: build Multimeter (`XMM1`: DC/AC V, DC/AC I, R mode buttons + numeric readout) and Bode Plotter (`XBP1`: log-frequency magnitude dB + phase deg dual plot from `AcResult`, cursors, `-3 dB` & phase-margin markers) inside `U4.1` frames.
- Done when: unit and GPUI test-platform tests verify accurate readouts on `STC-lin-01` and `STC-ac-01`; goldens exist in both themes; `UB3` holds.

### S4.5 Results panel (bottom dock) · todo
- Lane: sim-ui · Depends: S4.1, U1.7 · Read: `sim-plan.md` SD4, SD13; `ui-plan.md` UK11
- Owns: `crates/omnia-app/src/sim/results_panel.rs`
- Do: populate `U1.7`'s bottom Results dock with tabbed Views: multi-trace waveform plot (for `TRAN`/`AC`/`DC`), Operating Point & Measurements table, Diagnostics/ERC tab (click row focuses item on sheet), and CSV export.
- Done when: tests cover plotting `OP`/`TRAN`/`AC`/`DC` results, clicking an ERC diagnostic to jump to the item, and deterministic CSV export.

### S4.6 Gate: Milestone S4 (live Wien oscillator & instruments check) · todo → HS3
- Lane: bench, sim-ui · Depends: S4.2, S4.3, S4.4, S4.5 · Read: `sim-plan.md` SB5, SB6, SB8, SINV9, STC-wien-01
- Owns: `tests/golden/sim/`, `docs/measurements/S4.6.md`
- Do: capture Windows goldens for the full workspace running the Wien oscillator with `XSC1` open in both themes; verify `UB3` (0 sheet paints) and `UB4` (0 idle frames when stopped); stop at `HS3` for the owner's live interactive sign-off on both `ngspice` and `RIS-C`.
- Done when: goldens pass, `HS3` is approved by the owner, and non-reference diagnostics are recorded in `docs/measurements/S4.6.md`.

---

## Milestone S5: agent simulation tools, bundling, and final Orion gate

### S5.1 Agent / MCP simulation tools (`omnia-agent`) · todo
- Lane: sim-agent · Depends: S3.5, A3.4 · Read: `sim-plan.md` SD14, SQ10; `mcp-plan.md` §10–§11
- Owns: `crates/omnia-agent/src/sim.rs`, `docs/agent/sim.md`
- Do: add `sim.erc`, `sim.run` (cancellable `A3` job with progress updates and `engine: ngspice|risc`), `sim.results`, and `sim.measure` to `omnia-agent` catalog and schemas (`AINV11`), exposed automatically over CLI and MCP stdio.
- Done when: end-to-end headless MCP test builds an RC filter or Wien oscillator, runs `sim.erc`, runs `sim.run` on both engines, cancels a run cleanly, and asserts `sim.measure` frequency within `SK5`.

### S5.2 Bundled `ngspice` build & licence audit · todo → HS2
- Lane: sim-ngspice · Depends: S0.3, S3.5 · Read: `sim-plan.md` SD12, SINV11, SQ9
- Owns: `scripts/build-ngspice.*`, `THIRD_PARTY.md`, `.github/workflows/ci.yml` (sim artifact wiring)
- Do: automate reproducible BSD-3-Clause `libngspice` builds for Windows and Linux (checksum-pinned, zero GPL/LGPL libraries linked), wire CI artifact caching so differential tests run on every PR, and update `THIRD_PARTY.md`.
- Done when: clean builds pass on Windows and Linux with zero GPL/LGPL symbols; `HS2` signed off by owner.

### S5.3 First-boot self-test & Preferences simulator settings · todo
- Lane: sim-ui · Depends: S5.2, U3.3, U3.5 · Read: `sim-plan.md` SQ6; `design/build-omnia.mjs:536-537`
- Owns: `crates/omnia-app/src/sim/selftest.rs`, `crates/omnia-app/src/screens/first_boot.rs`, `crates/omnia-app/src/dialogs/preferences.rs`
- Do: wire First-boot simulator rows (`ngspice` bundled self-test showing elapsed time on this machine; `RIS-C` preview option) and Preferences default-engine / tolerance settings.
- Done when: First-boot self-test runs `STC-lin-01` in `< 500 ms` off the UI thread and displays verified status; Preferences round-trip persists cleanly.

### S5.4 Gate: Milestone S5 — final Orion certification · todo → HS4
- Lane: bench · Depends: S4.6, S5.1, S5.3 · Read: `sim-plan.md` SB1–SB8, SINV1–SINV12; `canvas-plan.md` §2.1
- Owns: `docs/measurements/S5.4/`, `docs/measurements/S5.4.md`
- Do: freeze the Milestone S5 Orion campaign script running `SB1`–`SB8` plus full `B3`/`B4`/`UB3`/`UB4` checks with live Wien oscillator streaming into `XSC1` on both `ngspice` and `RIS-C`; run ten interleaved repetitions at `100%` and `125%` on Orion (`HS4`).
- Done when: all `SB1`–`SB8` and `SINV1`–`SINV12` pass on Orion and `HS4` is accepted by the owner.

---

## Log

```
2026-11-23 S0.1 done — Authored `sim-plan.md` and `sim-roadmap.md` against `main` (`b046bbb`)
                       incorporating the 2026-11-23 owner decisions: dual engines (`ngspice`
                       bundled + `RIS-C` pure-Rust SPICE) in parallel from S1 with `ngspice` as
                       differential oracle (`SD1`); full analog core (`OP`, `TRAN`, `AC`, `DC`,
                       `SD2`); passives, sources/rails, diode, BJT, MOSFET, behavioral/macromodel
                       op-amp (`SD3`); Oscilloscope (`XSC1`), Multimeter (`XMM1`), Bode Plotter
                       (`XBP1`), and Results dock (`SD4`). Registered `S`-prefixed ids (`SB`,
                       `SK`, `SD`, `SINV`, `STC`, `SS`, `SQ`, `HS`, `S0.1`…) in `AGENTS.md` and
                       linked Stage 3 in `ROADMAP.md`. Next step: `S0.2` (`HS1`) owner review of
                       `SQ1`–`SQ10` and `SD5`–`SD14`. No code or dependency changes.
```
