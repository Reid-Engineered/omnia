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
plans and crates; task S0.2 applies them after HS1.

Written 2026-11-23 against `main` at `b046bbb`. No simulation code exists yet.

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
| `S0.1`, `S1.1`… | Task | [sim-roadmap.md](sim-roadmap.md) |

Status words follow the repository conventions: **DECIDED** (owner confirmed), **DEFAULT**
(build to this until an owner decision replaces it), **HYPOTHESIS** (build only after the named
`SS` spike confirms it), and **PROPOSED** (becomes DECIDED or is amended at HS1).

`SPICE`, `ngspice`, and standard JEDEC component numbers (`1N4148`, `2N2222`, `2N3904`,
`2N7000`, `LM741`, `TL072`) are named here as open standards, open-source software, and generic
device types. No commercial company or proprietary product is named.

## 1. Scope

Owner decisions recorded 2026-11-23:

- **Engines (SD1):** Both `ngspice` (bundled) and `RIS-C` (Omnia's pure-Rust SPICE engine)
  build in parallel from Milestone S1 behind one shared `SimEngine` trait, with `ngspice`
  serving as the differential accuracy oracle for `RIS-C` on every reference circuit.
- **Analyses (SD2):** Full analog core in v1 — DC operating point (`OP`), transient (`TRAN`),
  AC small-signal sweep (`AC`), and DC sweep (`DC`).
- **Components and models (SD3):** Passives (resistor, capacitor, inductor), sources and supply
  rails (DC, AC, sine, pulse, `+V`/`-V` rails, ground), diode, BJT, MOSFET, and behavioral plus
  macromodel op-amps — covering the Wien bridge oscillator and standard analog circuits.
- **Instruments and views (SD4):** Floating **Oscilloscope** (`XSC1`, live ring-buffer traces),
  **Multimeter** (`XMM1`), **Bode Plotter** (`XBP1`), and the bottom-dock **Results** panel
  (plots, measurements, diagnostics, CSV export).

Out of scope for this plan (candidates for post-v1 milestones):

- Digital / mixed-mode simulation (logic gates, MCU co-simulation, XSPICE codemodels).
- Parametric, temperature, noise, sensitivity, Fourier/distortion, and Monte Carlo sweeps.
- Additional strip instruments beyond the four above (function generator, wattmeter, 4-channel
  scope, frequency counter, word generator, logic analyzer, logic converter, IV analyzer,
  distortion analyzer — their strip icons stay reserved per `ui-plan.md`).
- Live edit-while-running re-simulation without stopping (SQ7 default: explicit Stop/Re-run or
  restart-on-edit toggle deferred).
- External netlist import (SPICE `.cir`/`.asc` import) and vendor encrypted models.
- Sub-circuit hierarchy / multi-sheet designs (out of scope per `ui-plan.md` UQ3).

## 2. Repository facts this plan rests on

Checked at `b046bbb`.

| Fact | Where | Consequence |
| --- | --- | --- |
| `omnia-doc` has no UI framework dependency (`INV12`) and owns `Doc`, `Connectivity` (`NetId`, `net_of`, `net_wires`), `Doc::fork()`, and `Doc::revision()` | `crates/omnia-doc/src/` | Simulation crates take a `Doc` snapshot (`Doc::fork()`) and stay 100% GPUI-free (`SINV1`). |
| `Part` currently stores only `symbol`, `at`, `orient`, `designator: Arc<str>`, and `value: Arc<str>` | `crates/omnia-doc/src/doc.rs:53` | Value covers `10k`, `100nF`, `12V`, etc.; optional simulation overrides (model name, source waveform parameters, initial conditions) need a small additive `props` map on `Part` (S1.1, RON v3 backward-compatible with v2). |
| `omnia-symbols` currently has 6 compiled symbols: resistor, capacitor, diode, ground, op-amp (5 pins), NPN BJT | `crates/omnia-symbols/src/generated.rs` | S1.1 adds the missing v1 analog symbols from `vendor/electronic-symbols/` (inductor, DC/AC/pulse sources, PNP BJT, NMOS/PMOS MOSFET, supply-rail tags) plus on-sheet instrument/probe glyphs. |
| `canvas-plan.md` D8 specifies background ERC/netlist on a `DocSnapshot` debounced at `K13` (`150 ms`) and single-producer ring buffers per channel drained by floating cached instrument views | `canvas-plan.md:366-378` | `omnia-sim` implements that exact SPSC ring-buffer contract (`SampleRing`) and debounced ERC runner. |
| `ui-plan.md` reserves toolbar analysis picker, Run/Pause/Stop controls, sim-time pill, status-bar ERC indicator, instruments strip, floating instrument frames (`U4.1`), and Results dock (`U1.7`, `U5.5`) | `ui-plan.md` UK3, UK4, UK6, UK10, UK11 | S4 wires real simulation state directly into those reserved slots with zero shell redesign. |
| `mcp-plan.md` §11 job model (`A3`) is explicitly designed for long-running cancellable simulation jobs | `mcp-plan.md:53-54` | S5.1 exposes `sim.erc`, `sim.run`, `sim.results`, and `sim.measure` over `A3`'s job lifecycle. |
| `deny.toml` allows `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `MPL-2.0` — no GPL or LGPL | `deny.toml:10-22` | Bundled `ngspice` must be built strictly from its BSD-3-Clause core (no GPL readline, no LGPL XSPICE/CUSPICE), and `omnia-risc` uses only permissive Rust crates (BSD/MIT/Apache-2.0). |
| `tests/golden/wien-oscillator.ron` already models the mockup's Wien bridge oscillator (11 parts, 31 wires, 9 nets) | `tests/golden/wien-oscillator.ron` | Used directly (with its `+12V`/`-12V` supply rails added in a sim fixture) as the headline `STC-wien-01` reference circuit for both engines. |

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
| `SB5` | **UI isolation during live simulation:** with `1k` sheet open, transient streaming into `XSC1` oscilloscope at 30 Hz (or 60 Hz if S0.4 passes), `B3` (< 16.7 ms p99, 0 missed refreshes), `B5` (≤ 33.3 ms), `UB3` (`SheetView` paint count = 0 while only `XSC1` updates), and `B10` (`allocs_per_frame.ours == 0` once `XSC1` buffers are warm) all hold. | `bench run --shell --sheet 1k --script sim-live` (Orion) |
| `SB6` | **Memory:** B9 (< 300 MB) holds on `1k` with active simulation + `XSC1` + Results dock; ring buffers and stored results are strictly bounded by `SK6` and `SK7`. | `memory_mb` in `SB5` run (Orion) |
| `SB7` | **Differential accuracy (`RIS-C` vs `ngspice`):** linear `OP`/`DC` relative error ≤ `SK4` (`1e-7`); nonlinear `OP`/`DC` ≤ `SK5` (`1e-4` rel + `1e-6` V/A abs); `AC` magnitude ≤ `0.05 dB` and phase ≤ `0.5°`; `TRAN` steady-state oscillation frequency ≤ `0.5%` and peak-to-peak amplitude ≤ `1%` on `STC-wien-01`. | deterministic CI test suite (`cargo test -p omnia-sim-tests`) |
| `SB8` | **Idle zero cost:** when simulation is `Idle` (or `Paused` with static traces), `UB4` still holds (0 frames drawn, 0 worker CPU over 10 s). | `bench idle --shell --sim-idle --seconds 10` (Orion) |

## 4. Constants

| ID | Constant | Value |
| --- | --- | --- |
| `SK1` | SI engineering suffixes | Standard SPICE prefixes: `f` (`1e-15`), `p` (`1e-12`), `n` (`1e-9`), `u`/`µ` (`1e-6`), `m` (`1e-3`), `k`/`K` (`1e3`), `Meg`/`MEG` (`1e6`), `G` (`1e9`), `T` (`1e12`). Trailing unit tokens (`Ohm`/`Ω`, `F`, `H`, `V`, `A`, `Hz`, `s`) are stripped deterministically. Bare `M` is `1e-3` (SPICE convention) with an ERC hint warning when attached to a resistor > 1 Ω (SQ2). |
| `SK2` | Default SPICE tolerances | `RELTOL = 1e-3`, `ABSTOL = 1e-12 A`, `VNTOL = 1e-6 V`, `CHGTOL = 1e-14 C`, `GMIN = 1e-12 S`, `ITL1 = 100` (DC OP), `ITL2 = 50` (DC sweep), `ITL4 = 10` (transient per-step NR). Identical defaults passed to both `ngspice` and `RIS-C`. |
| `SK3` | Default temperature | `TNOM = 27.0 °C` (`300.15 K`), thermal voltage `V_T = k T / q ≈ 25.85199 mV`. |
| `SK4` | Linear differential tolerance | `|a - b| <= 1e-7 * max(|a|, |b|) + 1e-12` (exact MNA match modulo floating-point ordering). |
| `SK5` | Nonlinear differential tolerance | `|a - b| <= 1e-4 * max(|a|, |b|) + 1e-6` for voltages (V) and currents (A); AC `<= 0.05 dB` magnitude, `<= 0.5°` phase; Wien transient `<= 0.5%` frequency, `<= 1.0%` Vpp. |
| `SK6` | Ring-buffer capacity | `SampleRing` holds `65,536` `(t: f64, y: f64)` samples per channel (`1 MiB` per channel, power-of-two lock-free SPSC ring). Producer overwrites oldest or drops with a monotonic `dropped_count` counter if UI stalls — never blocks the solver thread or grows heap. |
| `SK7` | Stored run result cap | At most `2,000,000` sample points per trace (`32 MiB` per trace) and `128 MiB` total stored result memory per run (configurable upper guard; oldest points decimated or run halts cleanly with typed `ResultCapReached` diagnostic). |
| `SK8` | Instrument redraw cadence | Default `30 Hz` (`33.3 ms` timer) for live oscilloscope updates, promotable to `60 Hz` only if `SS2` proves `B3` holds on Orion (canvas plan §11 risk row). |
| `SK9` | ERC debounce | `K13 = 150 ms` after the last `Doc::revision()` change (D8). |
| `SK10` | Reference ground node | SPICE node `"0"` (`NodeId(0)`). Any net carrying a `Ground` symbol (or net label `"0"` / `"GND"` when explicitly configured per SQ3) maps to `NodeId(0)`. |
| `SK11` | Engine timeout & cancel grace | Cooperative cancel grace `50 ms`, hard process kill (for `ngspice` worker) or thread flag abort (for `RIS-C`) within `100 ms` (`SB4`). |
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
   |   models.rs    symbol -> typed Device + built-in model/subckt library    |
   |   erc.rs       deterministic ERC on Doc snapshot (SB1)                   |
   |   ir.rs        canonical CircuitIr + NodeId<->NetId source map (SB2)     |
   |   netlist.rs   deterministic SPICE netlist emitter                       |
   |   engine.rs    SimEngine trait, SimRunHandle, SampleRing, SimState       |
   |   results.rs   OpResult, TranResult, AcResult, DcResult, measurements    |
   +-------------------+-----------------------------------+------------------+
                       |                                   |
                       | SimEngine                         | SimEngine
                       v                                   v
   +-----------------------------------+   +----------------------------------+
   | omnia-sim-ngspice                 |   | omnia-risc (Omnia Rust SPICE)    |
   |   worker IPC + libngspice loader  |   |   mna.rs      sparse MNA + LU    |
   |   omnia-ngspice-worker binary     |   |   dc.rs       OP + DC + NR/GMIN  |
   |   BSD-3-Clause libngspice (.dll/.so)| |   ac.rs       complex AC phasor  |
   |   crash-isolated, kill-on-cancel  |   |   tran.rs     BE/Trap/Gear2+LTE  |
   |   binary double streaming         |   |   devices/    R,C,L,V,I,D,Q,M,U  |
   +-----------------------------------+   +----------------------------------+
```

**Lane boundaries:**
- `omnia-sim`, `omnia-sim-ngspice`, and `omnia-risc` have **zero GPUI dependency** (`SINV1`,
  extending `INV12`). All unit, property, and differential tests run headlessly in standard CI.
- `omnia-doc` has **zero simulator dependency**. It stores schematic items and opaque key/value
  part properties (`Part::props`); all SPICE interpretation lives in `omnia-sim`.
- `omnia-canvas` draws on-sheet probe badges, rail tags, and instrument blocks using theme
  tokens (`probe/a`, `probe/b`, `stage/instrument-body`, `stage/tag-bg`, `stage/tag-text`); it
  never imports a simulator crate.

## 6. Decisions

### SD1 Dual engines behind `SimEngine` from S1. DECIDED (2026-11-23)

- Both `omnia-sim-ngspice` (`ngspice`, bundled) and `omnia-risc` (`RIS-C`, Omnia's pure-Rust
  SPICE engine) implement one `SimEngine` trait and develop in parallel from Milestone S1.
- The user can switch engines per sheet/session (`ngspice` default/stable, `RIS-C` preview,
  matching `design/build-omnia.mjs:536-537`).
- Every reference circuit (`STC-*`) runs on **both** engines in CI; `ngspice` acts as the
  differential oracle verifying `RIS-C` numerical accuracy (`SB7`).

### SD2 v1 analysis set: OP, TRAN, AC, DC. DECIDED (2026-11-23)

- `Analysis::Op` — DC operating point (all node voltages and source branch currents).
- `Analysis::Tran { step, stop, start, max_step, uic }` — time-domain transient simulation with
  live sample streaming into `SampleRing`.
- `Analysis::Ac { sweep: Dec | Oct | Lin, points, f_start, f_stop }` — AC small-signal
  linearised frequency sweep returning complex phasors `(re, im)` (displayed as magnitude dB +
  phase degrees).
- `Analysis::Dc { source, start, stop, step, second_source: Option<...> }` — primary (and
  optional nested secondary) DC transfer sweep.

### SD3 v1 component and model set. DECIDED (2026-11-23)

- **Passives:** Resistor (`R`), Capacitor (`C`, with optional `ic`), Inductor (`L`, with
  optional `ic`).
- **Sources & power:** Independent DC/AC/SIN/PULSE voltage (`V`) and current (`I`) sources;
  named supply rail tags (`+12V`, `-12V`, `+5V`, `+3.3V`, etc., lowered to DC voltage sources
  referenced to ground `0`); Ground (`0`).
- **Semiconductors:** Diode (`D`: Shockley exponential + junction capacitance + breakdown),
  NPN/PNP BJT (`Q`: Ebers-Moll / Gummel-Poon DC + junction capacitances), NMOS/PMOS MOSFET
  (`M`: Level 1 Shichman-Hodges + gate/junction capacitances).
- **Op-amps:** Behavioral op-amp macromodel (`A_OL`, `GBW` dominant pole, `R_in`, `R_out`,
  supply-rail output saturation clamp) plus built-in standard macromodels (`TL072`, `LM741`)
  authored from public datasheet parameters (no proprietary vendor subcircuits).

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
  against in-process `libngspice` on both Windows and Orion before S1.5 locks it in.

### SD6 `RIS-C` solver architecture. PROPOSED (confirmed at HS1)

- **Linear algebra:** Sparse Modified Nodal Analysis (MNA) assembled in CSC/CSR format with
  symbolic ordering (AMD / minimum-degree) and numeric sparse LU with partial pivoting, reused
  across Newton-Raphson iterations and transient timesteps (`SINV7` — no per-iteration symbolic
  re-factorisation when sparsity pattern is fixed).
- **Nonlinear DC (`OP`/`DC`):** Damped Newton-Raphson with SPICE-standard junction voltage
  limiting (`pnjlim`, `fetlim`), automatic `GMIN` stepping (`1e-3 -> 1e-12`) and source stepping
  (`0% -> 100%`) fallback on non-convergence.
- **AC small-signal:** Linearise every nonlinear device at its DC `OP` conductance/capacitance
  stamp, solve complex sparse MNA `(G + j ω C) X = I` at each sweep frequency.
- **Transient (`TRAN`):** Start from DC `OP` (or `.ic` if `uic`); first step uses Backward
  Euler to avoid trapezoidal ringing, then Trapezoidal (with Gear-2 fallback on stiff/ringing
  steps); adaptive timestep via Local Truncation Error (LTE) estimate and exact breakpoint table
  for `PULSE`/`SIN` corners; emits samples into `SampleRing` without waiting for run completion.

### SD7 Deterministic circuit lowering and source mapping. PROPOSED (confirmed at HS1)

- `CircuitIr::lower(doc, library, analysis, probes)` produces bit-for-bit identical node ids,
  device order, and SPICE netlist text regardless of arena insertion order or hash-map seeding
  (`SINV2`):
  - Ground nets collapse to `NodeId(0)`.
  - Non-ground nets sort deterministically by `(explicit_label_name, lowest_pin_part_id, pin_idx)`.
  - Devices sort by `(spice_prefix, designator, part_id)`.
- `SourceMap` records bidirectional links `NodeId <-> NetId`, `DeviceIdx <-> PartId`, so every
  ERC error, convergence failure, and waveform channel highlights the exact schematic net or part.

### SD8 Electrical Rules Check (ERC) ruleset. PROPOSED (confirmed at HS1)

- Severity levels: **`Error`** (blocks simulation start unless explicitly overridden only for
  warnings), **`Warning`** (allows run, shown in status bar & Diagnostics), **`Info`**.
- v1 checks:
  - `ERC001` (Error): No ground reference (`0`) in circuit.
  - `ERC002` (Error): Duplicate part designator (case-insensitive per SPICE).
  - `ERC003` (Error): Unparseable or out-of-range component value (e.g. `R = 0`, `C <= 0`, NaN).
  - `ERC004` (Error): Voltage source / supply rail short circuit (zero-resistance loop of ideal
    voltage sources or shorted to ground).
  - `ERC005` (Error): Unconnected instrument port or probe referencing a missing net.
  - `ERC006` (Warning): Floating pin or single-pin net (unconnected terminal).
  - `ERC007` (Warning): Ambiguous `M` suffix on resistor (SPICE interprets `1M` as `1 mΩ`, not
    `1 MΩ`; suggests `1Meg` or `1m`).
  - `ERC008` (Warning): Op-amp power rail pin unconnected when macromodel requires supplies.

### SD9 Value & parameter storage in `omnia-doc`. PROPOSED (confirmed at HS1)

- `Part` keeps `designator: Arc<str>` and `value: Arc<str>` unchanged so existing canvas/doc/MCP
  code stays untouched.
- Add `pub props: BTreeMap<Arc<str>, Arc<str>>` (empty by default, zero overhead when empty;
  sorted for determinism) to `Part`, edited through a new `Command::SetPartProp` (full undo/redo
  per `INV4`). Used for optional SPICE model override (`model = "TL072"`), source waveform specs
  (`waveform = "SIN(0 1 1k)"`, `ac_mag = "1"`), and initial conditions (`ic = "0.1"`).
- RON serialisation bumps to v3 with seamless backward-compatible load of existing v2 `.ron`
  files (`props` defaults to empty).

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

### SD11 Live streaming & UI threading. DECIDED (extends canvas-plan.md D8)

- Simulation never runs on the GPUI UI thread (`SINV1`, `D8`).
- Live `TRAN` streams `(t, v_0, v_1, ...)` into lock-free SPSC `SampleRing`s (one per probe
  channel, `SK6`).
- The floating `XSC1` oscilloscope view drains available samples into its pre-allocated display
  buffer on its `SK8` cadence (`30 Hz` default) and notifies **only** itself — `SheetView` stays
  `.cached()` with 0 repaints (`SB5`, `UB3`, `INV11`).
- Editing the schematic while a simulation is `Running` stops or invalidates the live run cleanly
  per `SQ7` (default: marks results stale with a banner and stops live streaming; never mutates a
  live `CircuitIr` mid-solve).

### SD12 Bundled `ngspice` licensing and build hygiene. PROPOSED (confirmed at HS1 & HS2)

- `ngspice` core is **BSD-3-Clause** (compatible with `deny.toml`).
- Bundled builds MUST compile with `--with-ngshared --disable-xspice --disable-cider
  --disable-openmp --without-x --without-readline` (exact flags verified in `SS1`/`HS2`) so zero
  GPL/LGPL code enters the binary.
- `THIRD_PARTY.md` records the exact `ngspice` version, source URL, build flags, and BSD-3-Clause
  licence text.

### SD13 Structured errors and convergence diagnostics. PROPOSED (confirmed at HS1)

- Raw SPICE stderr is never dumped raw as the only user message. `omnia-sim` parses engine
  failures into typed `SimError` variants (`ErcFailed`, `SingularMatrix { nodes }`,
  `NoConvergence { phase, step_t, nodes }`, `TimestepTooSmall { t }`, `WorkerCrashed`,
  `Cancelled`, `ResultCapReached`, `EngineUnavailable`), mapped through `SourceMap` to net names
  and `PartId`s with actionable fix hints (e.g., "Check for floating node on net `N3` or add a
  DC path to ground").

### SD14 Agent / MCP integration. PROPOSED (confirmed at HS1)

- Adds 4 read/job tools to `omnia-agent` (after `A3.4`):
  - `sim.erc` (tier 1 read): runs ERC on an open document, returns typed `ErcDiagnostic` list.
  - `sim.run` (tier 1 read / A3 job): launches `OP`/`TRAN`/`AC`/`DC` on `ngspice` or `RIS-C`
    with progress notifications and cancellation; never mutates the document.
  - `sim.results` (tier 1 read): paged/decimated waveform slice or operating-point table for a
    completed `sim_run_id`.
  - `sim.measure` (tier 1 read): computes `vpp`, `vrms`, `vavg`, `vmin`, `vmax`, `freq`, `duty`,
    `rise_time`, `fall_time`, `bandwidth_3db`, `phase_margin` on any probed net/branch.

## 7. Invariants

Each has a unit or property test; the roadmap task that adds it is in brackets.

| ID | Rule |
| --- | --- |
| `SINV1` | `omnia-sim`, `omnia-sim-ngspice`, and `omnia-risc` have zero dependency on GPUI or `omnia-app` (extending `INV12`). [S1.2, S1.5, S1.6] |
| `SINV2` | **Determinism:** rebuilding the same circuit with parts/wires/labels inserted in any order yields identical `ErcDiagnostic`s, identical `CircuitIr` node/device numbering, and identical SPICE netlist text. [S1.2, S1.3] |
| `SINV3` | **Document immutability:** running ERC or any simulation never mutates `Doc` or changes `Doc::revision()`. [S1.3, S1.4] |
| `SINV4` | **Bounded streaming memory:** `SampleRing` never allocates after construction and never blocks the solver thread if the UI stops draining. [S1.4] |
| `SINV5` | **Clean cancellation:** cancelling a running simulation on either `ngspice` or `RIS-C` terminates worker execution within `SK11` and leaves zero orphan child processes or leaked threads. [S1.5, S1.6, S3.1, S3.3] |
| `SINV6` | **Value round-trip & SPICE suffix safety:** every valid `SK1` value parses deterministically, rejects NaN/Inf/negative R/C/L, and emits unambiguous SPICE numeric strings (never emitting bare `M` for mega). [S1.2] |
| `SINV7` | **RIS-C KCL/KVL conservation:** at every converged `OP`, `DC`, `AC`, and `TRAN` step, the residual norm `‖F(x)‖∞` satisfies `SK2` (`ABSTOL` + `RELTOL * max_branch`) — Kirchhoff's laws hold at every node. [S1.6, S2.3, S3.2, S3.3] |
| `SINV8` | **Differential parity:** every `STC-*` reference circuit produces matching results on `RIS-C` and `ngspice` within `SB7` (`SK4`/`SK5`). [S1.7, S2.4, S3.4] |
| `SINV9` | **Sheet isolation:** live oscilloscope updates at 30 Hz (or 60 Hz) trigger 0 `SheetView` paints (`INV11`, `UB3`) and 0 Omnia heap allocations per warm paint frame (`B10`). [S4.3, S4.6] |
| `SINV10` | **RON backward compatibility:** loading any existing v2 `.ron` fixture (including `inverting-amp.ron` and `wien-oscillator.ron`) succeeds identically after adding `Part::props` (v3). [S1.1] |
| `SINV11` | **Licence purity:** bundled `ngspice` build and all Rust simulation crates contain zero GPL/LGPL code; `cargo deny check` stays green. [S0.3, S1.5, S1.6, S5.2] |
| `SINV12` | **Source-map completeness:** every non-ground `NodeId` and every `DeviceIdx` in `CircuitIr` maps back to a live `NetId` or `PartId` in the source `Doc` snapshot. [S1.2] |

## 8. Reference circuits and scenarios (`STC`)

Every circuit below lives as a committed fixture in `tests/sim/` and runs on **both** `ngspice`
and `RIS-C` against analytical solutions and each other.

| ID | Circuit / Scenario | Analysis | Expected result |
| --- | --- | --- | --- |
| `STC-erc-01` | Missing ground, duplicate `R1`, `R = 0`, shorted DC sources, floating pin, `1M` resistor | ERC | Exact `ERC001`–`ERC007` codes mapped to their `PartId`/`NetId`; `Error` blocks run, `Warning` permits run. [S1.3] |
| `STC-lin-01` | Resistive divider + Wheatstone bridge (`10 V`, `1k`/`2k`/`3k`/`4k`) | `OP`, `DC` | Matches closed-form nodal voltages & currents to `SK4` (`1e-7`) on both engines. [S1.7] |
| `STC-lin-02` | Dependent-source active network (VCVS + VCCS) | `OP` | Matches analytical gain and input/output currents to `SK4` on both engines. [S1.7] |
| `STC-nl-01` | Half-wave `1N4148` diode + `1k` load (`DC` sweep `-2 V..+5 V`) | `OP`, `DC` | Knee voltage (~0.65–0.72 V) and current curve match `ngspice` within `SK5`. [S2.4] |
| `STC-nl-02` | Common-emitter `2N2222` NPN amplifier biased from `12 V` | `OP`, `DC` | `V_B`, `V_C`, `V_E`, `I_C` match `ngspice` within `SK5`. [S2.4] |
| `STC-nl-03` | `2N7000` NMOS common-source inverter (`V_GS` sweep `0..5 V`) | `OP`, `DC` | Threshold transition and saturation current match `ngspice` within `SK5`. [S2.4] |
| `STC-nl-04` | Inverting op-amp (`R1 = 10k`, `R2 = 22k`, `V_in = 1 V`, `±12 V` rails, matching `inverting-amp.ron`) | `OP`, `DC` | `V_out ≈ -2.200 V` in linear region, saturates cleanly near `±12 V` rails; both engines agree within `SK5`. [S2.4] |
| `STC-ac-01` | First-order RC low-pass (`10k`, `10nF`) & RLC bandpass | `AC` (`dec 50 10 1Meg`) | `-3.01 dB` and `-45°` at `f_c = 1/(2πRC) ≈ 1591.5 Hz`; both engines agree within `0.05 dB` / `0.5°`. [S3.5] |
| `STC-ac-02` | Inverting op-amp closed-loop frequency response (`TL072` / behavioral GBW) | `AC` | Flat passband `+6.85 dB`, `-20 dB/dec` roll-off at `GBW / (1 + R2/R1)`; both engines agree within `SK5`. [S3.5] |
| `STC-tran-01` | RC step response (`PULSE 0 5 1m 1u 1u 10m 20m`, `10k`, `100nF`) | `TRAN` (`0..15 ms`) | `v(t) = 5(1 - e^{-(t-1ms)/1ms})` within `SK5` on both engines. [S3.5] |
| `STC-tran-02` | Full-wave diode bridge rectifier with `100uF` filter cap + `1k` load (`60 Hz` sine) | `TRAN` (`0..50 ms`) | Peak voltage and ripple match `ngspice` within `SK5`. [S3.5] |
| `STC-wien-01` | **Headline mockup circuit:** Wien bridge oscillator (`R = 10k`, `C = 10nF`, gain network + diode amplitude stabilisation, `±12 V` rails, `XSC1` on output & `N+`) | `TRAN` (`0..20 ms`) | Self-starts cleanly without manual kick (or small `.ic`), stabilises near `f_0 = 1/(2πRC) ≈ 1.59 kHz`; `RIS-C` and `ngspice` agree within `0.5%` frequency and `1%` Vpp. [S3.4, S4.6] |
| `STC-err-01` | Deliberately singular / non-convergent circuit & mid-run Stop | `OP`, `TRAN` | Typed `SimError` naming the offending net/part; Stop returns to `Idle` within `100 ms` with zero orphan processes (`SB4`, `SINV5`). [S3.5] |

## 9. Spikes (`SS`)

Run in Milestone S0 before locking `SD5` and `SK8`. Results recorded in `docs/measurements/S0.*.md`.

| ID | What it tests | Answers |
| --- | --- | --- |
| `SS1` | Prototype `omnia-ngspice-worker` (BSD-3-Clause `libngspice` build) vs in-process `libngspice` on Windows and Orion Linux | Exact BSD-3-Clause build flags (confirming zero GPL/LGPL), startup latency (`SB3`), binary streaming overhead, kill-on-cancel reliability (`SB4`), and cross-platform packaging layout. |
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
| `libngspice` (bundled dynamic library, built without XSPICE/CIDER/readline/OpenMP) | `omnia-sim-ngspice` | Reference SPICE backend & differential oracle | BSD-3-Clause (verified at `SS1` / `HS2`) |
| `libloading` | `omnia-sim-ngspice` | Runtime dynamic loading of `ngspice.dll` / `libngspice.so` | ISC |
| `num-complex` | `omnia-risc` | Complex phasor arithmetic for AC MNA | MIT OR Apache-2.0 |
| `faer` (or hand-written sparse CSC LU; decided in S1.6) | `omnia-risc` | Sparse LU / linear solve (if external crate used) | MIT |

No C/C++ code is compiled into `omnia-doc`, `omnia-sim`, `omnia-risc`, `omnia-canvas`, or
`omnia-app` directly.

## 12. Failure modes and mitigations

| Risk | Mitigation | Where checked |
| --- | --- | --- |
| `libngspice` C crash or stuck solver hangs Omnia | Worker-process isolation (`SD5`) + `SK11` kill-on-cancel | `SS1`, `S1.5`, `STC-err-01` |
| GPL/LGPL accidentally enabled in `ngspice` build | Explicit BSD-3-Clause configure flags + symbol/licence audit script | `SS1`, `S5.2`, `HS2` |
| `RIS-C` diverges numerically from `ngspice` on nonlinear circuits | Differential test suite runs on every milestone PR with exact `SK4`/`SK5` gates | `S1.7`, `S2.4`, `S3.4`, `S3.5` |
| Wien bridge oscillator fails to self-start in noiseless SPICE | Built-in initial kick / `.ic` support and verified gain-stabilised fixture | `S3.4`, `STC-wien-01` |
| Live oscilloscope breaks `B3` / `B10` / `UB3` on Orion | Cached `SheetView`, lock-free `SampleRing`, pre-allocated trace buffers, `30 Hz` throttle | `SS2`, `S4.3`, `S5.4` |
| `1M` resistor parsed as `1 mΩ` surprises user | SPICE-compliant parsing (`SK1`) + prominent `ERC007` warning with one-click fix suggestion | `S1.2`, `S1.3`, `STC-erc-01` |

## 13. Changes requested in other plans and crates

Applied in task S0.2 (for plan docs) and S1.1 / S4.x (for code):

| Target | Change |
| --- | --- |
| `AGENTS.md` | Add `sim-plan.md` and `sim-roadmap.md` to Sources of truth and register `S`-prefixed ids (`SB`, `SK`, `SD`, `SINV`, `STC`, `SS`, `SQ`, `HS`, `S0.1`…). (Applied in S0.1.) |
| `ROADMAP.md` | Link `sim-plan.md` and `sim-roadmap.md` in intro and Stage 3; update open product decisions with 2026-11-23 owner answers. (Applied in S0.1.) |
| `crates/omnia-doc` (`S1.1`) | Add `Part::props: BTreeMap<Arc<str>, Arc<str>>` (empty default) + `Command::SetPartProp` + RON v3 with backward-compatible v2 read (`SINV10`). |
| `crates/omnia-symbols` (`S1.1`, `S4.2`) | Add symbol role metadata (`Component`, `Ground`, `PowerRail`, `Instrument`, `Probe`) and compile the remaining v1 analog symbols + on-sheet `XSC1`/`XMM1`/`XBP1` blocks. |
| `ui-roadmap.md` `U5.5` / `canvas-roadmap.md` `T7.3` | `S4.3`–`S4.5` supply the real oscilloscope, multimeter, Bode plotter, and Results panel contents inside `U4.1`'s floating frames and `U1.7`'s bottom dock. |
| `mcp-plan.md` (`S5.1`) | Register `sim.erc`, `sim.run`, `sim.results`, and `sim.measure` in the `omnia-agent` catalog once `A3.4` is done. |

## 14. Open questions (`SQ`)

Every question has a default; build to the default until the owner's dated answer at `HS1`
replaces it (Hard Rule 5).

| ID | Question | Default |
| --- | --- | --- |
| `SQ1` | Should `omnia-sim-ngspice` always use the isolated `omnia-ngspice-worker` helper process (`SD5`), or allow in-process `libngspice`? | Always use `omnia-ngspice-worker` unless `SS1` shows worker launch exceeds `SB3` (`150 ms`) on Orion. |
| `SQ2` | How should bare `1M` on a resistor value be handled (SPICE `1 mΩ` vs human `1 MΩ`)? | Follow SPICE (`1M = 1e-3`, `1Meg = 1e6`) for netlist compatibility and emit `ERC007` warning whenever bare `M` is used on a resistor. |
| `SQ3` | Should a net label `"GND"` without a `Ground` symbol count as node `0`? | No — require an explicit `Ground` symbol (or net label `"0"`), and emit a helpful `ERC001` hint if `"GND"` exists without a `Ground` symbol. |
| `SQ4` | For `omnia-risc` sparse linear algebra in S1.6: hand-written pure-Rust sparse CSC LU (zero external math crates) or `faer` (MIT)? | Hand-written pure-Rust sparse CSC LU with Markowitz/AMD ordering for S1–S3 (zero extra deps, small matrices `< 10k` nodes), benchmarked against `ngspice` at S3.5. |
| `SQ5` | Default input impedance of on-sheet `XSC1` oscilloscope and `XMM1` voltmeter probes? | `10 MΩ` to ground (realistic bench probe; prevents floating-node singularities when attached to capacitive nodes), overridable to ideal open (`∞`) per instrument. |
| `SQ6` | Should `RIS-C` ship enabled in the engine picker in v1 or behind a Preferences toggle? | Visible in the First-boot / Preferences / toolbar engine selector labelled `preview`, with `ngspice` as default (matching `design/build-omnia.mjs:536-537`). |
| `SQ7` | What happens if the user edits the schematic while a simulation is `Running`? | Stop the live run cleanly, mark displayed results/oscilloscope traces with a "Stale — schematic changed" badge, and require clicking Run again (live edit-while-running deferred to post-v1). |
| `SQ8` | Default op-amp model when placing a generic `IC-COM-OpAmp` with no `model` prop set? | Built-in behavioral op-amp (`A_OL = 100k`, `GBW = 3 MHz`, `R_in = 1 MΩ`, `R_out = 75 Ω`, rail clamp to connected supplies or `±15 V` if supply pins omitted). |
| `SQ9` | In CI (`windows-latest`), how is `libngspice` provided for `omnia-sim-ngspice` and differential tests? | Pre-built BSD-3-Clause `ngspice` artifact cached/fetched in CI (checksum-pinned, verified at `HS2`); `omnia-risc` tests always run with zero external binaries. |
| `SQ10` | Should `sim.run` over MCP (`S5.1`) allow selecting `engine: "ngspice" | "risc"`? | Yes — optional `engine` field defaulting to `"ngspice"`, letting agents cross-check circuits on both engines. |
