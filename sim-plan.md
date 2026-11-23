# Omnia simulation: plan (reference)

This file is the reference for circuit simulation in Omnia: electrical rules checking (ERC),
component-to-model binding, circuit IR and SPICE netlist generation, dual simulation backends
(`ngspice` and `RIS-C`), live sample streaming, virtual instruments, and agent simulation tools.
The work itself is in [sim-roadmap.md](sim-roadmap.md), which lists tasks that cite the ids
defined here.

It extends [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md) and [ui-plan.md](ui-plan.md) and re-plans nothing they decide. Every
`B`, `K`, `G`, `D`, `INV`, `Q`, `W`-prefixed, `A`-prefixed and `U`-prefixed id is defined in
those files and still holds. Section 13 lists the small changes this plan requests from other
plans and crates; task S0.2 applies the plan-doc items after HS1.

Written 2026-11-23 against Omnia `main` (`b046bbb`) and `Reid-Engineered/RIS-C` (`24710eaf`).
No Omnia simulation integration code exists yet.

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `SB` | Performance, latency or accuracy budget, a hard requirement | 3 |
| `SK` | Constant every simulation crate must agree on | 4 |
| `SD` | Simulation architectural decision | 6 |
| `SINV` | Invariant: a rule that must hold in code, each with a test | 7 |
| `STC` | Reference circuit or error scenario with its expected result | 8 |
| `SS` | Spike: a measurement or prototype that settles an open technical fact | 9 |
| `SQ` | Open question for the owner, each with a default | 14 |
| `HS` | Human checkpoint | [sim-roadmap.md](sim-roadmap.md) |
| `S0.1`, `S1.1`… | Task (milestones are always written `Milestone S0`–`Milestone S4`, distinct from canvas spikes `S1`–`S8`) | [sim-roadmap.md](sim-roadmap.md) |

Status words follow the repository conventions: **DECIDED** (owner confirmed), **DEFAULT**
(build to this until an owner decision replaces it), **HYPOTHESIS** (build only after the named
`SS` spike confirms it), and **PROPOSED** (becomes DECIDED or is amended at HS1).

`SPICE`, `ngspice`, `RIS-C` (`Reid-Engineered/RIS-C`), and standard JEDEC component numbers
(`1N4148`, `2N2222`, `2N3904`, `2N7000`, `LM741`, `TL072`) are named here as open standards,
first-party/open-source engines, and generic device types. No commercial company or proprietary
product is named.

## 1. Scope

Owner decisions recorded 2026-11-23:

- **Engines (SD1):** Both `ngspice` (bundled) and `RIS-C` (the existing first-party pure-Rust
  SPICE crate `risc-c` at `Reid-Engineered/RIS-C`) integrate in parallel from Milestone S1 behind
  one shared `SimEngine` trait, with `ngspice` serving as the differential accuracy oracle on
  every reference circuit.
- **Analyses (SD2):** Full analog core in v1 — DC operating point (`OP`), transient (`TRAN`),
  AC small-signal sweep (`AC`), and single-source DC sweep (`DC`).
- **Components and models (SD3):** Passives (resistor, potentiometer, capacitor, inductor),
  independent & linear dependent sources and supply rails (DC, AC, sine, pulse, VCVS/VCCS,
  `+V`/`-V` rails, ground), diode, BJT, MOSFET, and behavioral plus macromodel op-amps —
  covering the Wien bridge oscillator and standard analog circuits.
- **Instruments and views (SD4):** Floating **Oscilloscope** (`XSC1`, live ring-buffer traces),
  **Multimeter** (`XMM1`), **Bode Plotter** (`XBP1`), and the bottom-dock **Results** panel
  (plots, measurements, diagnostics, CSV export).

Out of scope for this plan (candidates for post-v1 milestones):

- Digital / mixed-mode simulation (logic gates, MCU co-simulation, XSPICE codemodels).
- Nested two-source DC sweeps, parametric, temperature, noise, sensitivity, Fourier/distortion,
  and Monte Carlo sweeps.
- Additional strip instruments beyond the four above (function generator, wattmeter, 4-channel
  scope, frequency counter, word generator, logic analyzer, logic converter, IV analyzer,
  distortion analyzer — their strip icons stay reserved per `ui-plan.md`).
- Live edit-while-running re-simulation without stopping (SQ7 default: stop and mark stale).
- External netlist import (SPICE `.cir`/`.asc` import) and vendor encrypted models.
- Sub-circuit hierarchy / multi-sheet designs (out of scope per `ui-plan.md` UQ3).

## 2. Repository facts this plan rests on

Checked at Omnia `b046bbb` and `Reid-Engineered/RIS-C` `24710eaf`.

| Fact | Where | Consequence |
| --- | --- | --- |
| `Reid-Engineered/RIS-C` (`risc-c`, BSD-3-Clause, Edition 2024, Rust 1.91+) already implements classic analog SPICE (`OP`, single-source `DC`, `AC lin\|dec\|oct`, adaptive `TRAN` with BE/Trap/BDF2, `R`/`C`/`L`/`K`/`S`/`W`/`V`/`I`/`E`/`G`/`F`/`H`/`D`/`Q`/`M`, `.model`, `.subckt`, `.ic`, `.nodeset`, `.temp`), validated against `ngspice 47` on Linux/macOS/Windows, with `CompiledCircuit`, concurrent `Simulator`s, and `transient_with` streaming + `ControlFlow::Break(())` cancellation | `Reid-Engineered/RIS-C` `docs/AGENT_INTEGRATION.md`, `docs/COMPATIBILITY.md`, `docs/STATUS.md` | Omnia does **not** re-implement a SPICE solver in-repo. `omnia-sim-risc` wraps the pinned `risc-c` Git crate behind `SimEngine`, obeying `RIS-C` contract rule 8 (integrate via public API, never copy equations). |
| `RIS-C` does not support runtime behavioral expressions, `.control`, `.measure`, or nested two-source `.dc` | `RIS-C` `docs/COMPATIBILITY.md` | Op-amp macromodels are written as standard SPICE `.subckt`s using `R`/`C`/`E`/`G`/`D` (identical on both `RIS-C` and `ngspice`), all measurements run in `omnia-sim`, and v1 `Analysis::Dc` is single-source. |
| `omnia-doc` has no UI framework dependency (`INV12`) and owns `Doc`, `Connectivity` (`NetId`, `net_of`, `net_wires`), `Doc::fork()`, and `Doc::revision()` | `crates/omnia-doc/src/` | Simulation crates take a `Doc` snapshot (`Doc::fork()`) and stay 100% GPUI-free (`SINV1`). |
| `Part` currently stores only `symbol`, `at`, `orient`, `designator: Arc<str>`, and `value: Arc<str>` | `crates/omnia-doc/src/doc.rs:53` | Value covers `10 kΩ`, `22 nF`, `50%`, `TL072`, `12V`, etc.; optional overrides (explicit SPICE model, potentiometer total resistance, source waveform parameters, initial conditions) need an additive `props` map on `Part` (S1.1, RON v3 backward-compatible with v2). |
| `omnia-symbols` currently has 6 compiled symbols: `Capacitor-IEEE-NonPolarized`, `Diode-COM-Standard`, `Ground-COM-General`, `IC-COM-OpAmp` (5 pins), `Resistor-IEEE-Potentiometer` (3 pins), and `Resistor-IEEE-Standard` | `crates/omnia-symbols/pins.toml`, `generated.rs` | S1.1 adds the missing v1 analog symbols from `vendor/electronic-symbols/` (inductor, independent DC/AC/pulse sources, VCVS/VCCS dependent sources, NPN & PNP BJT, NMOS & PMOS MOSFET, supply-rail tags) plus on-sheet instrument/probe glyphs in S3.2. |
| `canvas-plan.md` D8 specifies background ERC/netlist on a `DocSnapshot` debounced at `K13` (`150 ms`) and single-producer ring buffers per channel drained by floating cached instrument views | `canvas-plan.md:366-378` | `omnia-sim` implements that exact SPSC ring-buffer contract (`SampleRing`) and debounced ERC runner. |
| `ui-plan.md` reserves toolbar analysis picker, Run/Pause/Stop controls, sim-time pill, status-bar ERC indicator, instruments strip, floating instrument frames (`U4.1`), and Results dock (`U1.7`, `U5.5`) | `ui-plan.md` UK3, UK4, UK6, UK10, UK11 | Milestone S3 wires real simulation state directly into those reserved slots with zero shell redesign. |
| `mcp-plan.md` §11 job model (`A3`) is designed for long-running cancellable simulation jobs; `AD7` sets tier 0 = read, tier 1 = undoable doc edit | `mcp-plan.md:53-54`, `AD7` | S4.1 exposes `sim.erc`, `sim.results`, and `sim.measure` as tier 0 reads, and `sim.run` as tier 0 read + bounded worker-process execution over `A3`'s job lifecycle. |
| `deny.toml` allows `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `MPL-2.0` (no GPL/LGPL) and restricts `allow-git` | `deny.toml:10-38` | Bundled `ngspice` builds strictly from its BSD-3-Clause core with zero GPL/LGPL bundled or statically linked (system C/compiler runtimes excepted). Adding `https://github.com/Reid-Engineered/RIS-C` to `deny.toml` `allow-git` (and handling CI access while `RIS-C` is private) is decided at `HS1` (`SQ4`). |
| `tests/golden/wien-oscillator.ron` models the mockup's Wien bridge oscillator (`R1`/`R2 = 10 kΩ`, `C1`/`C2 = 22 nF`, `R4 = 22 kΩ`, pot `R3 = 50%`, `D1`/`D2 = 1N4148`, `U1 = TL072`, two empty-designator grounds; `f_0 ≈ 723.4 Hz`) | `tests/golden/wien-oscillator.ron` | Used directly (adding `±12 V` supply rails, `R3` total resistance prop, and `XSC1` probes in a sim fixture) as the headline `STC-wien-01` reference circuit for both engines. |

## 3. Budgets

Measured on the Orion reference machine under canvas plan §2.1 (`1920x1080 @ 60 Hz`, `100%` and
`125%`, ten interleaved repetitions) unless marked deterministic. All canvas (`B1`–`B10`) and UI
(`UB1`–`UB10`) budgets still hold while simulation is idle or running; these are added.

| ID | Budget | Measured by |
| --- | --- | --- |
| `SB1` | **ERC latency:** full ERC on `1k` sheet < 5 ms p99; on `10k` sheet < 40 ms p99 (background thread; 0 ms UI stall). | `cargo bench -p omnia-bench --bench sim` (Orion) |
| `SB2` | **Netlist lowering:** `Doc` → `CircuitIr` + SPICE text on `1k` sheet < 5 ms p99; on `10k` sheet < 40 ms p99. | same bench |
| `SB3` | **Cold run latency (Wien oscillator):** from Run click to first sample in `SampleRing` < 150 ms p99 on `ngspice` and < 50 ms p99 on `RIS-C`. | `bench sim --circuit wien --engine ngspice\|risc` (Orion) |
| `SB4` | **Cancellation latency:** Stop clicked during a heavy transient run returns to `Idle` and frees worker CPU within < 100 ms p99 (no orphan worker processes). | `bench sim --script cancel` (Orion) |
| `SB5` | **UI isolation during live simulation:** with `1k` sheet open, transient streaming into `XSC1` oscilloscope at `SK8` cadence (`30 Hz`, or `60 Hz` if `SS2` passes), `B3` (< 16.7 ms p99, 0 missed refreshes), `B5` (≤ 33.3 ms), `UB3` (`SheetView` paint count = 0 while only `XSC1` updates), and `B10` (`allocs_per_frame.ours == 0` once `XSC1` buffers are warm) all hold. | `bench ui --sheet 1k --script sim-live` (Orion) |
| `SB6` | **Memory:** B9 (< 300 MB) holds on `10k` with simulation idle, and `< 300 MB` holds on `1k` with active live simulation + `XSC1` + Results dock; ring buffers and stored results are strictly bounded by `SK6` and `SK7`. | `memory_mb` in `SB5` and `B9` runs (Orion) |
| `SB7` | **Differential accuracy (`RIS-C` vs `ngspice`):** under the differential test tolerances (`SK2`), linear `OP`/`DC` relative error ≤ `SK4` (`1e-7`); nonlinear `OP`/`DC` ≤ `SK5` (`1e-4` rel + `1e-6` V/A abs); `AC` magnitude ≤ `0.05 dB` and phase ≤ `0.5°`; `TRAN` steady-state oscillation frequency ≤ `0.5%` and peak-to-peak amplitude ≤ `1%` on `STC-wien-01`. | deterministic CI test suite (`cargo test -p omnia-sim-tests`, created in S2.1) |
| `SB8` | **Idle zero cost:** when simulation is `Idle` (or `Paused` with static traces), `UB4` still holds (0 frames drawn, 0 worker CPU over 10 s). | `bench idle --shell --sim-idle --seconds 10` (Orion) |

## 4. Constants

| ID | Constant | Value |
| --- | --- | --- |
| `SK1` | SI engineering value parser | Accepts optional whitespace between number, prefix, and unit (e.g. `10 kΩ`, `22 nF`, `4.7u`, `1Meg`, `100uH`, `-12V`, `50%` for potentiometer wiper ratio). Prefixes: `f` (`1e-15`), `p` (`1e-12`), `n` (`1e-9`), `u`/`µ` (`1e-6`), `m` (`1e-3`), `k`/`K` (`1e3`), `Meg`/`MEG` (`1e6`), `G` (`1e9`), `T` (`1e12`). Unit tokens (`Ohm`/`Ω`, `F`, `H`, `V`, `A`, `Hz`, `s`) are parsed *with* the prefix so uppercase `1 F` or `1F` on a capacitor is `1 Farad` (while `10fF` or `10f` is `10 fF`). Bare `M` is `1e-3` (SPICE convention, matching `RIS-C` contract 2) and triggers `ERC007` whenever attached to a resistor (SQ2). |
| `SK2` | SPICE solver tolerances | Interactive default options (passed identically to both engines via `.options` / `RIS-C` options): `RELTOL = 1e-3`, `ABSTOL = 1e-12 A`, `VNTOL = 1e-6 V`, `ITL1 = 100`. Differential oracle tests (`SB7`) tighten both engines to `RELTOL = 1e-6`, `ABSTOL = 1e-13 A`, `VNTOL = 1e-7 V` so stopping tolerance is 100× tighter than `SK5`. |
| `SK3` | Default temperature | `TNOM = 27.0 °C` (`300.15 K`), thermal voltage `V_T = k T / q ≈ 25.86492 mV` (`25.865 mV`). |
| `SK4` | Linear differential tolerance | `|a - b| <= 1e-7 * max(|a|, |b|) + 1e-12` (exact MNA match modulo floating-point ordering). |
| `SK5` | Nonlinear differential tolerance | `|a - b| <= 1e-4 * max(|a|, |b|) + 1e-6` for voltages (V) and currents (A); AC `<= 0.05 dB` magnitude, `<= 0.5°` phase; Wien transient `<= 0.5%` frequency, `<= 1.0%` Vpp. |
| `SK6` | Ring-buffer capacity | `SampleRing` holds `65,536` `(t: f64, y: f64)` samples per channel (`1 MiB` per channel, power-of-two lock-free SPSC ring). Producer overwrites oldest or drops with a monotonic `dropped_count` counter if UI stalls — never blocks the solver thread or grows heap. |
| `SK7` | Stored run result cap | At most `2,000,000` sample points per trace (`32 MiB` per trace) and `128 MiB` total stored result memory per run (oldest points decimated or run halts cleanly with typed `ResultCapReached` diagnostic). |
| `SK8` | Instrument redraw cadence | Default `30 Hz` (`33.3 ms` timer) for live oscilloscope updates, promotable to `60 Hz` only if `SS2` (S0.4) proves `B3` holds on Orion (canvas plan §11 risk row). |
| `SK9` | ERC debounce | `K13 = 150 ms` after the last `Doc::revision()` change (D8). |
| `SK10` | Reference ground node | SPICE node `"0"` (`NodeId(0)`). Any net carrying a `Ground` symbol (or net label `"0"` per SQ3) maps to `NodeId(0)`. |
| `SK11` | Engine timeout & cancel grace | Cooperative cancel grace `50 ms`, hard process kill (for `ngspice` worker) or `ControlFlow::Break(())` abort (for `RIS-C`) within `100 ms` (`SB4`). |
| `SK12` | Circuit limits | Max `65,535` nodes, `65,535` devices, `64` live probe channels per run (guarding both engines and IPC framing). |

## 5. Architecture

```
   omnia-app (GPUI UI thread)                       omnia-agent / omnia-cli (headless)
     SimController                                    sim.erc / sim.run (A3 job)
     Toolbar (Run/Pause/Stop, Analysis, Engine)       sim.results / sim.measure
     StatusBar (live ERC badge)                                  |
     XSC1 Oscilloscope / XMM1 / XBP1 / Results                   |
                |                                                |
     Doc::fork() + revision (D8)     SampleRing (SPSC, lock-free)|
                |                          ^                     |
   +------------v--------------------------+---------------------v------------+
   | omnia-sim (pure Rust, no GPUI, INV12)                                    |
   |   value.rs     SI engineering value parser (SK1)                         |
   |   models.rs    symbol -> typed Device + built-in .model/.subckt library  |
   |   erc.rs       deterministic ERC on Doc snapshot (SB1)                   |
   |   ir.rs        canonical CircuitIr + NodeId<->NetId source map (SB2)     |
   |   netlist.rs   deterministic SPICE netlist emitter (RIS-C & ngspice)     |
   |   engine.rs    SimEngine trait, SimRunHandle, SampleRing, SimState       |
   |   results.rs   OpResult, TranResult, AcResult, DcResult, measurements    |
   +-------------------+-----------------------------------+------------------+
                       |                                   |
                       | SimEngine                         | SimEngine
                       v                                   v
   +-----------------------------------+   +----------------------------------+
   | omnia-sim-ngspice                 |   | omnia-sim-risc                   |
   |   worker IPC + libngspice loader  |   |   thin adapter over pinned       |
   |   omnia-ngspice-worker binary     |   |   `risc-c` (Reid-Engineered/     |
   |   BSD-3-Clause libngspice (.dll/.so)| |   RIS-C @ 24710eaf, BSD-3-Clause)|
   |   crash-isolated, kill-on-cancel  |   |   CircuitBuilder / netlist ->    |
   |   binary double streaming         |   |   transient_with -> SampleRing   |
   +-----------------------------------+   +----------------------------------+
```

**Lane boundaries:**
- `omnia-sim`, `omnia-sim-ngspice`, and `omnia-sim-risc` have **zero GPUI dependency** (`SINV1`,
  extending `INV12`). All unit, property, and differential tests run headlessly in standard CI.
- `omnia-doc` has **zero simulator dependency**. It stores schematic items and opaque key/value
  part properties (`Part::props`); all SPICE interpretation lives in `omnia-sim`.
- `omnia-canvas` draws on-sheet probe badges, rail tags, and instrument blocks using theme
  tokens (`probe/a`, `probe/b`, `stage/instrument-body`, `stage/tag-bg`, `stage/tag-text`); it
  never imports a simulator crate.

## 6. Decisions

### SD1 Dual engines behind `SimEngine` from Milestone S1. DECIDED (2026-11-23)

- Both `omnia-sim-ngspice` (`ngspice`, bundled) and `omnia-sim-risc` (adapter over the pinned
  first-party crate `risc-c` from `Reid-Engineered/RIS-C`) implement one `SimEngine` trait and
  integrate in parallel from Milestone S1.
- The user can switch engines per sheet/session (`ngspice` default/stable, `RIS-C` preview,
  matching `design/build-omnia.mjs:536-537`).
- Every reference circuit (`STC-*`) runs on **both** engines in CI; `ngspice` acts as the
  differential oracle verifying end-to-end Omnia-to-`RIS-C` lowering and numerical agreement
  (`SB7`).

### SD2 v1 analysis set: OP, TRAN, AC, DC. DECIDED (2026-11-23)

- `Analysis::Op` — DC operating point (all node voltages and source branch currents).
- `Analysis::Tran { step_s, stop_s, start_s, max_step_s, uic }` — time-domain transient
  simulation with live sample streaming into `SampleRing`.
- `Analysis::Ac { sweep: Dec | Oct | Lin, points, f_start_hz, f_stop_hz }` — AC small-signal
  linearised frequency sweep returning complex phasors `(re, im)` (displayed as magnitude dB +
  phase degrees).
- `Analysis::Dc { source, start, stop, step }` — single-source DC transfer sweep (matches
  `RIS-C` 0.1's `.dc` contract; nested two-source sweep deferred post-v1).

### SD3 v1 component and model set. DECIDED (2026-11-23)

- **Passives:** Resistor (`R`), Potentiometer (`R_pot`, 3-terminal wiper ratio `0%..100%` +
  total resistance in `props["resistance"]` defaulting to `20 kΩ`, lowered to two series
  resistors `(1 - α)R` and `αR` meeting at pin 3), Capacitor (`C`, optional `ic`), Inductor
  (`L`, optional `ic`).
- **Sources & power:** Independent DC/AC/SIN/PULSE voltage (`V`) and current (`I`) sources;
  linear dependent sources VCVS (`E`) and VCCS (`G`) (used both directly and inside macromodels);
  named supply rail tags (`+12V`, `-12V`, `+5V`, `+3.3V`, etc., lowered to DC voltage sources
  referenced to ground `0`); Ground (`0`).
- **Semiconductors:** Diode (`D`), NPN/PNP BJT (`Q`), NMOS/PMOS MOSFET (`M`) — using the classic
  SPICE device parameter subset supported identically by both `ngspice` and `RIS-C` (`docs/COMPATIBILITY.md`).
- **Op-amps:** Behavioral op-amp `.subckt` macromodel (`A_OL`, `GBW` dominant pole, `R_in`,
  `R_out`, diode rail-clamped output stage built strictly from `R`/`C`/`E`/`G`/`D` — zero
  behavioral expressions so it runs identically on both `RIS-C` and `ngspice`) plus built-in
  standard macromodels (`TL072`, `LM741`) authored from public datasheet parameters.
- **Model resolution precedence:** `part.props["model"]` (explicit override) → `part.value`
  matching a built-in model name (e.g. `U1` with `value: "TL072"` or `D1` with `value: "1N4148"`)
  → symbol default model (`SQ8` for generic op-amps).

### SD4 v1 instruments and result views. DECIDED (2026-11-23)

- **Oscilloscope (`XSC1`):** 2-channel (`A`, `B`, colours `probe/a`, `probe/b`) + trigger
  floating view inside `U4.1`'s frame (`576x468` per `UK10`). Drains `SampleRing` on a fixed
  cadence (`SK8`), never blocks the solver, never repaints `SheetView` (`SB5`, `UB3`).
- **Multimeter (`XMM1`):** 2-terminal floating readout for DC V, AC Vrms, DC/AC I, or R.
- **Bode Plotter (`XBP1`):** 2-port (`IN`, `OUT`) floating magnitude (dB) + phase (deg)
  log-frequency plot driven by `AcResult` with cursors and `-3 dB` / phase-margin readouts.
- **Results Panel:** Bottom dock (`U1.7`/`U5.5`) with tabbed Plots, Measurements table,
  Diagnostics/ERC list (click to focus item on sheet), and CSV export.

### SD5 `ngspice` process isolation via helper worker. HYPOTHESIS (settled by SS1)

- `libngspice` keeps process-global static C state: only one simulation can run per process,
  C-level `abort()`/`SIGSEGV` kills the host process, and `bg_halt` cannot guarantee instant
  cancellation inside a stuck matrix factorisation.
- **Default architecture:** `omnia-sim-ngspice` spawns a small bundled helper executable
  (`omnia-ngspice-worker`) that dynamically loads `libngspice` (`ngspice.dll` / `libngspice.so`)
  and streams binary `f64` sample frames over stdout/pipe. Stop sends cooperative `bg_halt`,
  then kills the worker cleanly after `SK11` (`50 ms`), guaranteeing zero crashes, zero leaked
  state, and multi-tab concurrency (`SB4`).
- Spike `SS1` (S0.3) benchmarks startup latency (`SB3`) and streaming throughput of the worker
  against in-process `libngspice` on both Windows and Orion before S1.6 locks it in.

### SD6 `RIS-C` integration boundary (`omnia-sim-risc`). PROPOSED (confirmed at HS1)

- Pin `risc-c` by exact Git commit SHA in workspace `Cargo.toml` (starting at `24710eaf`,
  upgraded explicitly like GPUI; `Cargo.lock` committed) and never copy internal solver code into
  Omnia (`RIS-C` `AGENT_INTEGRATION.md` rule 8).
- `omnia-sim-risc` compiles `CircuitIr` via `risc_c::netlist::parse_str` (with `SK2` options
  applied) or `CircuitBuilder` into a shareable `CompiledCircuit`, creates a fresh `Simulator`
  per run on a background worker thread, streams transient samples via `transient_with` directly
  into `SampleRingProducer`, supports Pause via a worker condition variable and Stop via
  `ControlFlow::Break(())` (`Error::Cancelled`), and maps `risc_c::RunError` through `SourceMap`
  into typed `SimError`.
- Never silently substitute `0.0` for missing signals (`sample.get(&circuit, name)` error
  propagates explicitly per `RIS-C` contract 4).

### SD7 Deterministic circuit lowering and source mapping. PROPOSED (confirmed at HS1)

- `CircuitIr::lower(doc, library, analysis, probes)` produces bit-for-bit identical node ids,
  device order, and SPICE netlist text regardless of arena insertion order or hash-map seeding
  (`SINV2`, matching `AGENTS.md` determinism rules: geometry and label/designator before id):
  - Ground nets collapse to `NodeId(0)`.
  - Non-ground nets sort deterministically by `(explicit_label_name, min_pin_key)` where
    `pin_key = (part.designator, pin_number, world_y, world_x, part_id)`.
  - Devices sort by `(spice_prefix, part.designator, world_y, world_x, part_id)`.
- `SourceMap` records bidirectional links `NodeId <-> NetId`, `DeviceIdx <-> PartId`, so every
  ERC error, convergence failure, and waveform channel highlights the exact schematic net or part.

### SD8 Electrical Rules Check (ERC) ruleset. PROPOSED (confirmed at HS1)

- Severity levels: **`Error`** (blocks simulation start), **`Warning`** (allows run, shown in
  status bar & Diagnostics), **`Info`**.
- v1 checks:
  - `ERC001` (Error): No ground reference (`0`) in circuit.
  - `ERC002` (Error): Duplicate non-empty part designator (case-insensitive per SPICE; empty
    designators on `Ground` and `PowerRail` parts are exempt).
  - `ERC003` (Error): Unparseable or out-of-range component value (e.g. `R = 0`, `C <= 0`, NaN,
    potentiometer ratio outside `0%..100%`).
  - `ERC004` (Error): Voltage source / supply rail short circuit (zero-resistance loop of ideal
    voltage sources or shorted to ground).
  - `ERC005` (Error): Unconnected instrument port or probe referencing a missing net.
  - `ERC006` (Warning): Floating pin or single-pin net (unconnected terminal).
  - `ERC007` (Warning): Bare `M` suffix on a resistor (SPICE interprets `1M` as `1 mΩ`, not
    `1 MΩ`; suggests `1Meg` or `1m`).
  - `ERC008` (Warning): Op-amp power rail pin unconnected when selected macromodel requires
    supplies.

### SD9 Value & parameter storage in `omnia-doc`. PROPOSED (confirmed at HS1)

- `Part` keeps `designator: Arc<str>` and `value: Arc<str>` unchanged so existing canvas/doc/MCP
  code stays untouched.
- Add `pub props: BTreeMap<Arc<str>, Arc<str>>` (empty by default, zero overhead when empty;
  sorted for determinism) to `Part`, edited through a new `Command::SetPartProp` (full undo/redo
  per `INV4`). Used for optional SPICE model override (`model = "TL072"`), potentiometer total
  resistance (`resistance = "20k"`), source waveform specs (`waveform = "SIN(0 1 1k)"`,
  `ac_mag = "1"`), and initial conditions (`ic = "0.1"`).
- RON serialisation (`crates/omnia-doc/src/io.rs` `Sheet`) bumps to v3 with seamless
  backward-compatible load of existing v2 `.ron` files (`props` defaults to empty; unsupported
  version rejection tests and `ui-plan.md` `UTC-open-bad` bump their rejected example to
  `version: 4`).

### SD10 Supply rails and on-sheet probes/instruments. PROPOSED (confirmed at HS1)

- Supply rail symbols (`+12V`, `-12V`, `+5V`, `+3.3V`, custom voltage label) are ordinary `Part`
  entries with a `PowerRail` symbol role: ERC and `CircuitIr` connect every rail of the same
  voltage name into a shared global net driven by one ideal DC voltage source referenced to
  ground `0` (so `wien-oscillator.ron` needs only rail tags at its op-amp supply pins, matching
  `design/build-omnia.mjs:188`).
- On-sheet instruments (`XSC1`, `XMM1`, `XBP1`) are `Part` entries with an `Instrument` symbol
  role whose pins wire onto the sheet like any other component (`XSC1` channels A/B wired in
  `design/build-omnia.mjs:229-234`); `CircuitIr` treats instrument voltage inputs as high-Z
  probes (`10 MΩ` default or ideal open, configurable per `SQ5`) and current inputs as zero-volt
  ammeters (`V = 0` branch current sense).

### SD11 Live streaming & UI threading. DECIDED (extends canvas-plan.md D8; live-edit policy DEFAULT per SQ7)

- Simulation never runs on the GPUI UI thread (`SINV1`, `D8`).
- Live `TRAN` streams `(t, v_0, v_1, ...)` into lock-free SPSC `SampleRing`s (one per probe
  channel, `SK6`).
- The floating `XSC1` oscilloscope view drains available samples into its pre-allocated display
  buffer on its `SK8` cadence (`30 Hz` default) and notifies **only** itself — `SheetView` stays
  `.cached()` with 0 repaints (`SB5`, `UB3`, `INV11`).
- **Live-edit policy (`DEFAULT`, `SQ7`):** Editing the schematic while a simulation is `Running`
  stops the live run cleanly and marks results stale with a banner; never mutates a live
  `CircuitIr` mid-solve.

### SD12 Bundled `ngspice` licensing and build hygiene. PROPOSED (policy confirmed at HS1; exact flags verified at HS2)

- `ngspice` core is **BSD-3-Clause** (compatible with `deny.toml`).
- Bundled builds must contain **zero GPL or LGPL code bundled or statically linked** (standard OS
  C and compiler runtimes such as `glibc`, `libgcc`, and `libstdc++` excepted). Optional
  GPL/LGPL-dependent features (such as `readline`, `FFTW`, `OpenMP`, and non-BSD solvers or GUI
  toolkits) are disabled; exact configure flags and binary library audits are settled in `SS1`
  (S0.3) and signed off at `HS2` (S4.2).
- `THIRD_PARTY.md` records the exact `ngspice` version, source URL, build flags, and BSD-3-Clause
  licence text, plus the pinned `RIS-C` attribution.

### SD13 Structured errors and convergence diagnostics. PROPOSED (confirmed at HS1)

- Raw SPICE stderr is never dumped raw as the only user message. `omnia-sim` parses failures from
  both `ngspice` and `risc_c::RunError` into typed `SimError` variants (`ErcFailed`,
  `SingularMatrix { nodes }`, `NoConvergence { phase, step_t, nodes }`, `TimestepTooSmall { t }`,
  `WorkerCrashed`, `Cancelled`, `ResultCapReached`, `EngineUnavailable`), mapped through
  `SourceMap` to net names and `PartId`s with actionable fix hints (e.g., "Check for floating
  node on net `N3` or add a DC path to ground").

### SD14 Agent / MCP integration. PROPOSED (confirmed at HS1)

- Adds 4 tools to `omnia-agent` (after `A3.4`), classified per `mcp-plan.md` `AD7`:
  - `sim.erc` (**tier 0 read**): runs ERC on an open document, returns typed `ErcDiagnostic` list.
  - `sim.run` (**tier 0 read + bounded worker process execution**): launches `OP`/`TRAN`/`AC`/`DC`
    on `ngspice` or `RIS-C` via an `A3` cancellable job with progress notifications; mutates no
    document and runs under `SK11`/`SK12` resource limits (spawns `omnia-ngspice-worker` when
    `engine = "ngspice"`, declared explicitly in the `AD7` policy table in `S4.1`).
  - `sim.results` (**tier 0 read**): paged/decimated waveform slice or operating-point table for
    a completed `sim_run_id`.
  - `sim.measure` (**tier 0 read**): computes `vpp`, `vrms`, `vavg`, `vmin`, `vmax`, `freq`,
    `duty`, `rise_time`, `fall_time`, `bandwidth_3db`, `phase_margin` on any probed net/branch.

## 7. Invariants

Each has a unit or property test; the roadmap task that adds it is in brackets.

| ID | Rule |
| --- | --- |
| `SINV1` | `omnia-sim`, `omnia-sim-ngspice`, and `omnia-sim-risc` have zero dependency on GPUI or `omnia-app` (extending `INV12`). [S1.2, S1.5, S1.6] |
| `SINV2` | **Determinism:** rebuilding the same circuit with parts/wires/labels inserted in any order yields identical `ErcDiagnostic`s, identical `CircuitIr` node/device numbering, and identical SPICE netlist text. [S1.2, S1.3] |
| `SINV3` | **Document immutability:** running ERC or any simulation never mutates `Doc` or changes `Doc::revision()`. [S1.3, S1.4] |
| `SINV4` | **Bounded streaming memory:** `SampleRing` never allocates after construction and never blocks the solver thread if the UI stops draining. [S1.4] |
| `SINV5` | **Clean cancellation:** cancelling a running simulation on either `ngspice` or `RIS-C` terminates worker execution within `SK11` and leaves zero orphan child processes or leaked threads. [S1.5, S1.6] |
| `SINV6` | **Value round-trip & SPICE suffix safety:** every valid `SK1` value (including whitespace, units, and potentiometer `%`) parses deterministically, rejects NaN/Inf/negative R/C/L, and emits unambiguous SPICE numeric strings (never emitting bare `M` for mega). [S1.2] |
| `SINV7` | **No silent signal fallback:** unknown or unallocated probe signal names always return a typed error on both `RIS-C` and `ngspice` (never silently returning `0.0`; `RIS-C` contract 4). [S1.5, S1.6] |
| `SINV8` | **Differential parity:** every `STC-*` reference circuit produces matching results on `RIS-C` and `ngspice` within `SB7` (`SK4`/`SK5`). [S2.1, S2.2] |
| `SINV9` | **Sheet isolation:** live oscilloscope updates at `SK8` cadence trigger 0 `SheetView` paints (`INV11`, `UB3`) and 0 Omnia heap allocations per warm paint frame (`B10`). [S3.3, S3.6] |
| `SINV10` | **RON backward compatibility:** loading any existing v2 `.ron` fixture (including `inverting-amp.ron` and `wien-oscillator.ron`) succeeds identically after adding `Part::props` (v3). [S1.1] |
| `SINV11` | **Licence purity:** bundled `ngspice` build and all Rust simulation crates (including `risc-c` transitive deps) bundle/statically link zero GPL/LGPL code (system C/compiler runtimes excepted); `cargo deny check` stays green. [S0.3, S1.5, S1.6, S4.2] |
| `SINV12` | **Source-map completeness:** every non-ground `NodeId` and every `DeviceIdx` in `CircuitIr` maps back to a live `NetId` or `PartId` in the source `Doc` snapshot. [S1.2] |

## 8. Reference circuits and scenarios (`STC`)

Every circuit below lives as a committed fixture in `tests/sim/` (crate `omnia-sim-tests`,
created in S2.1) and runs on **both** `ngspice` and `RIS-C` against analytical solutions and
each other under `SK2`'s differential tolerances.

| ID | Circuit / Scenario | Analysis | Expected result |
| --- | --- | --- | --- |
| `STC-erc-01` | Missing ground, duplicate `R1` (plus two empty-designator grounds that must pass), `R = 0`, shorted DC sources, unconnected `XSC1` probe, floating pin, `1M` resistor, unpowered op-amp | ERC | Exact `ERC001`–`ERC008` codes mapped to their `PartId`/`NetId`; `Error` blocks run, `Warning` permits run. [S1.3] |
| `STC-lin-01` | Resistive divider + Wheatstone bridge (`10 V`, `1k`/`2k`/`3k`/`4k`) | `OP`, `DC` | Matches closed-form nodal voltages & currents to `SK4` (`1e-7`) on both engines. [S1.5, S1.6, S2.1] |
| `STC-lin-02` | Dependent-source active network (VCVS `E` + VCCS `G` + resistors) | `OP` | Matches analytical gain and input/output currents to `SK4` on both engines. [S2.1] |
| `STC-nl-01` | Half-wave `1N4148` diode + `1k` load (`DC` sweep `-2 V..+5 V`) | `OP`, `DC` | Knee voltage (~0.65–0.72 V) and current curve match `ngspice` within `SK5`. [S2.1] |
| `STC-nl-02` | Common-emitter `2N2222` NPN amplifier biased from `12 V` | `OP`, `DC` | `V_B`, `V_C`, `V_E`, `I_C` match `ngspice` within `SK5`. [S2.1] |
| `STC-nl-03` | `2N7000` NMOS common-source inverter (`V_GS` sweep `0..5 V`) | `OP`, `DC` | Threshold transition and saturation current match `ngspice` within `SK5`. [S2.1] |
| `STC-nl-04` | Standalone inverting op-amp fixture (`R1 = 10 kΩ`, `R2 = 22 kΩ`, `V_in = 1 V`, `±12 V` rails) | `OP`, `DC` | `V_out ≈ -2.200 V` in linear region, saturates cleanly near `±12 V` rails; both engines agree within `SK5`. [S2.1] |
| `STC-ac-01` | First-order RC low-pass (`10 kΩ`, `10 nF`) & RLC bandpass | `AC` (`dec 50 10 1Meg`) | `-3.01 dB` and `-45°` at `f_c = 1/(2πRC) ≈ 1591.5 Hz`; both engines agree within `0.05 dB` / `0.5°`. [S2.1] |
| `STC-ac-02` | Inverting op-amp closed-loop frequency response (`R1 = 10 kΩ`, `R2 = 22 kΩ`, `TL072` / behavioral GBW) | `AC` | Flat passband `+6.85 dB`, `-20 dB/dec` roll-off at `GBW / (1 + R2/R1)`; both engines agree within `SK5`. [S2.1] |
| `STC-tran-01` | RC step response (`PULSE 0 5 1m 1u 1u 10m 20m`, `10 kΩ`, `100 nF`) | `TRAN` (`0..15 ms`) | `v(t) = 5(1 - e^{-(t-1ms)/1ms})` within `SK5` on both engines. [S2.1] |
| `STC-tran-02` | Full-wave diode bridge rectifier with `100 uF` filter cap + `1 kΩ` load (`60 Hz` sine) | `TRAN` (`0..50 ms`) | Peak voltage and ripple match `ngspice` within `SK5`. [S2.1] |
| `STC-wien-01` | **Headline mockup circuit (derived from `tests/golden/wien-oscillator.ron`):** Wien bridge oscillator (`R1`/`R2 = 10 kΩ`, `C1`/`C2 = 22 nF`, `R4 = 22 kΩ`, pot `R3 = 50%`, `D1`/`D2 = 1N4148`, `U1 = TL072`, `±12 V` rails, `XSC1` on output & `N+`) | `TRAN` (`0..40 ms`) | Self-starts cleanly (via startup transient or small `.ic`), stabilises near `f_0 = 1/(2π · 10k · 22n) ≈ 723.4 Hz`; `RIS-C` and `ngspice` agree within `0.5%` frequency and `1%` Vpp. [S2.2, S3.6] |
| `STC-err-01` | Deliberately singular / non-convergent circuit & mid-run Stop | `OP`, `TRAN` | Typed `SimError` naming the offending net/part; Stop returns to `Idle` within `100 ms` with zero orphan processes (`SB4`, `SINV5`). [S2.1, S2.3] |

## 9. Spikes (`SS`)

Run in Milestone S0 before locking `SD5` and `SK8`. Results recorded in `docs/measurements/S0.*.md`.

| ID | What it tests | Answers |
| --- | --- | --- |
| `SS1` | Prototype `omnia-ngspice-worker` (BSD-3-Clause `libngspice` build) vs in-process `libngspice` on Windows and Orion Linux | Exact BSD-3-Clause build flags (confirming zero bundled/static GPL/LGPL), startup latency (`SB3`), binary streaming overhead, kill-on-cancel reliability (`SB4`), and cross-platform packaging layout. |
| `SS2` | Live 2-channel `SampleRing` producer at `100 kS/s` drained by a cached floating `XSC1` GPUI view at `30 Hz` and `60 Hz` over a `1k` sheet on Orion | Whether `XSC1` meets `SB5` (`B3` < 16.7 ms, 0 missed refreshes, 0 sheet repaints, 0 warm Omnia allocs) at `30 Hz` and whether `60 Hz` is safe (`SK8`). |

## 10. Core data model and API sketch (`omnia-sim`)

```rust
pub enum Analysis {
    Op,
    Tran { step_s: f64, stop_s: f64, start_s: f64, max_step_s: Option<f64>, uic: bool },
    Ac { sweep: AcSweep, points: u32, f_start_hz: f64, f_stop_hz: f64 },
    Dc { source: PartId, start: f64, stop: f64, step: f64 },
}

pub enum EngineKind {
    Ngspice,
    RisC,
}

pub enum SimState {
    Idle,
    Running { sim_time_s: f64, progress: Option<f32> },
    Paused { sim_time_s: f64 },
    Completed,
    Failed(SimError),
}

pub trait SimEngine: Send + Sync {
    fn kind(&self) -> EngineKind;
    fn start(
        &self,
        ir: Arc<CircuitIr>,
        analysis: Analysis,
        probes: Vec<ProbeSpec>,
        rings: Vec<SampleRingProducer>,
    ) -> Result<Box<dyn SimRunHandle>, SimError>;
}

pub trait SimRunHandle: Send {
    fn state(&self) -> SimState;
    fn pause(&self) -> Result<(), SimError>;
    fn resume(&self) -> Result<(), SimError>;
    fn cancel(&self);
    fn wait(self: Box<Self>) -> Result<SimResult, SimError>;
}
```

## 11. Dependencies and licensing

Every new crate must pass `cargo deny check` (`MIT`, `Apache-2.0`, `BSD-2-Clause`,
`BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `MPL-2.0` — zero GPL/LGPL).

| Dependency | Crate | Purpose | Licence |
| --- | --- | --- | --- |
| `risc-c` (`git = "https://github.com/Reid-Engineered/RIS-C"`, pinned `--rev 24710eaf…`) | `omnia-sim-risc` | First-party pure-Rust SPICE engine (`OP`, `DC`, `AC`, `TRAN`, streaming + cancel) | BSD-3-Clause (transitive deps `faer`, `peroxide`, `num-complex`, `thiserror` verified by `cargo deny` in S1.5) |
| `libngspice` (bundled dynamic library, BSD-3-Clause core build) | `omnia-sim-ngspice` | Reference SPICE backend & differential oracle | BSD-3-Clause (verified at `SS1` / `HS2`) |
| `libloading` | `omnia-sim-ngspice` | Runtime dynamic loading of `ngspice.dll` / `libngspice.so` | ISC |

No C/C++ code is compiled into `omnia-doc`, `omnia-sim`, `omnia-sim-risc`, `omnia-canvas`, or
`omnia-app` directly.

## 12. Failure modes and mitigations

| Risk | Mitigation | Where checked |
| --- | --- | --- |
| `libngspice` C crash or stuck solver hangs Omnia | Worker-process isolation (`SD5`) + `SK11` kill-on-cancel | `SS1`, `S1.6`, `STC-err-01` |
| GPL/LGPL accidentally linked into `ngspice` build | Explicit BSD-3-Clause configure flags + linked-library audit script | `SS1`, `S4.2`, `HS2` |
| Private `RIS-C` Git dependency blocks public CI or external contributors | Resolved at `HS1` (`SQ4`: make `RIS-C` public, configure CI read token, or publish crate) + `allow-git` entry in `deny.toml` | `S0.2`, `S1.5` |
| `RIS-C` and `ngspice` diverge on a lowered schematic or macromodel | Differential test suite runs on every milestone PR with tightened `SK2` tolerances (`1e-6`) against `SK4`/`SK5` gates | `S2.1`, `S2.2`, `S2.3` |
| Wien bridge oscillator fails to self-start in noiseless SPICE | Built-in startup transient / `.ic` support and verified gain-stabilised fixture (`723.4 Hz`) | `S2.2`, `STC-wien-01` |
| Live oscilloscope breaks `B3` / `B10` / `UB3` on Orion | Cached `SheetView`, lock-free `SampleRing`, pre-allocated trace buffers, `30 Hz` throttle | `SS2`, `S3.3`, `S4.4` |
| `1M` resistor parsed as `1 mΩ` surprises user | SPICE-compliant parsing (`SK1`) + prominent `ERC007` warning with one-click fix suggestion | `S1.2`, `S1.3`, `STC-erc-01` |

## 13. Changes requested in other plans and crates

Applied in task S0.1 (where noted), S0.2 (plan alignments), and S1.1 / S1.5 / S3.x / S4.1 (code):

| Target | Change |
| --- | --- |
| `AGENTS.md` | Add `sim-plan.md` and `sim-roadmap.md` to Sources of truth and register `S`-prefixed ids (`SB`, `SK`, `SD`, `SINV`, `STC`, `SS`, `SQ`, `HS`, `S0.1`…; note `Milestone S0`–`S4` vs canvas spikes `S1`–`S8`). (Applied in S0.1.) |
| `ROADMAP.md` | Link `sim-plan.md` and `sim-roadmap.md` in intro and Stage 3; update open product decisions with 2026-11-23 owner answers. (Applied in S0.1.) |
| `deny.toml` (`S0.2` / `S1.5`) | Add `https://github.com/Reid-Engineered/RIS-C` to `sources.allow-git` after `HS1` approval (`SQ4`). |
| `crates/omnia-doc` (`S1.1`) | Add `Part::props: BTreeMap<Arc<str>, Arc<str>>` (empty default) + `Command::SetPartProp` + RON v3 (`io.rs` `Sheet` record, backward-compatible v2 read per `SINV10`, unsupported-version test bumped to `version: 4`). |
| `ui-plan.md` (`S0.2` / `S1.1`) | Bump `UTC-open-bad`'s rejected-version example from `version: 3` to `version: 4` when RON v3 lands in `S1.1` (`UD8`). |
| `crates/omnia-symbols` (`S1.1`, `S3.2`) | Add symbol role metadata (`Component`, `Ground`, `PowerRail`, `Instrument`, `Probe`) and compile the remaining v1 analog symbols + on-sheet `XSC1`/`XMM1`/`XBP1` blocks. |
| `ui-roadmap.md` `U5.5` / `canvas-roadmap.md` `T7.3` | `S3.3`–`S3.5` supply the real oscilloscope, multimeter, Bode plotter, and Results panel contents inside `U4.1`'s floating frames and `U1.7`'s bottom dock. |
| `mcp-plan.md` (`S4.1`) | Register `sim.erc`, `sim.run` (tier 0 + bounded worker spawn under `AD7`), `sim.results`, and `sim.measure` in the `omnia-agent` catalog once `A3.4` is done. |

## 14. Open questions (`SQ`)

Every question has a default; build to the default until the owner's dated answer at `HS1`
replaces it (Hard Rule 5).

| ID | Question | Default |
| --- | --- | --- |
| `SQ1` | Should `omnia-sim-ngspice` always use the isolated `omnia-ngspice-worker` helper process (`SD5`), or allow in-process `libngspice`? | Always use `omnia-ngspice-worker` unless `SS1` shows worker launch exceeds `SB3` (`150 ms`) on Orion. |
| `SQ2` | How should bare `1M` on a resistor value be handled (SPICE `1 mΩ` vs human `1 MΩ`)? | Follow SPICE (`1M = 1e-3`, `1Meg = 1e6`) for netlist compatibility and emit `ERC007` warning whenever bare `M` is used on a resistor. |
| `SQ3` | Should a net label `"GND"` without a `Ground` symbol count as node `0`? | No — require an explicit `Ground` symbol (or net label `"0"`), and emit a helpful `ERC001` hint if `"GND"` exists without a `Ground` symbol. |
| `SQ4` | `Reid-Engineered/RIS-C` is currently a private GitHub repository while `omnia` is public. How should Omnia's CI and contributors fetch `risc-c`? | Add `https://github.com/Reid-Engineered/RIS-C` to `deny.toml` `allow-git` pinned to a reviewed SHA (`24710eaf`), and either make `RIS-C` public or supply a read-only CI token before `S1.5` merges. |
| `SQ5` | Default input impedance of on-sheet `XSC1` oscilloscope and `XMM1` voltmeter probes? | `10 MΩ` to ground (realistic bench probe; prevents floating-node singularities when attached to capacitive nodes), overridable to ideal open (`∞`) per instrument. |
| `SQ6` | Should `RIS-C` ship enabled in the engine picker in v1 or behind a Preferences toggle? | Visible in the First-boot / Preferences / toolbar engine selector labelled `preview`, with `ngspice` as default (matching `design/build-omnia.mjs:536-537`). |
| `SQ7` | What happens if the user edits the schematic while a simulation is `Running`? | Stop the live run cleanly, mark displayed results/oscilloscope traces with a "Stale — schematic changed" badge, and require clicking Run again (live edit-while-running deferred to post-v1). |
| `SQ8` | Default op-amp model when placing a generic `IC-COM-OpAmp` with neither `props["model"]` nor a known `value` model name? | Built-in behavioral op-amp `.subckt` (`A_OL = 100k`, `GBW = 3 MHz`, `R_in = 1 MΩ`, `R_out = 75 Ω`, diode rail clamp to connected supplies or `±15 V` if supply pins omitted). |
| `SQ9` | In CI (`windows-latest`), how is `libngspice` provided for `omnia-sim-ngspice` and differential tests? | Pre-built BSD-3-Clause `ngspice` artifact cached/fetched in CI (checksum-pinned, verified at `HS2`); `omnia-sim-risc` tests always run with zero external binaries. |
| `SQ10` | Should `sim.run` over MCP (`S4.1`) allow selecting `engine: "ngspice" \| "risc"`? | Yes — optional `engine` field defaulting to `"ngspice"`, letting agents cross-check circuits on both engines. |
