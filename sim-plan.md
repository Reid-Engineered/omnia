# Omnia simulation: plan (reference)

This file is the reference for circuit simulation in Omnia: electrical rules checking (ERC),
component-to-model binding, circuit IR and SPICE netlist generation, dual simulation backends
(`ngspice` and `RIS-C`), live sample streaming, virtual instruments, and agent simulation tools.
The work itself is in [sim-roadmap.md](sim-roadmap.md), which lists tasks that cite the ids
defined here.

It extends [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md) and [ui-plan.md](ui-plan.md) and re-plans nothing they decide. Every
`B`, `K`, `G`, `D`, `INV`, `Q`, `W`-prefixed, `A`-prefixed and `U`-prefixed id is defined in
those files and still holds. It also depends on two plans that are not yet merged: the
component-definitions plan `parts-plan.md` / `parts-roadmap.md` (pull request #85, head
`66f9e49`; `P`-prefixed ids such as `PD5`, `PK4`, `P2.3`) and the circuit context plan
`context-plan.md` / `context-roadmap.md` (pull request #100, head `6a50b28`; `C`-prefixed ids
such as `CK4`, `CD13`, `C1.9`). Where this plan cites them, the cross-plan decisions `XD1`–`XD9`
(section 6) bind both sides. Section 13 lists the changes this plan requests from other plans
and crates.

Written 2026-11-23 against Omnia `main` (`b046bbb`) and `Reid-Engineered/RIS-C` (`24710eaf`).
Signed off at `HS1` on 2026-11-23 (task S0.2, against `main` at `0e8cb21`) by the coordinating
agent under the owner's delegation of that date, applying the cross-plan decisions `XD1`–`XD9`.
No Omnia simulation integration code exists yet.

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `SB` | Performance, latency or accuracy budget, a hard requirement | 3 |
| `SK` | Constant every simulation crate must agree on | 4 |
| `SD` | Simulation architectural decision | 6 |
| `XD` | Cross-plan decision shared with the parts and context plans | 6 |
| `SINV` | Invariant: a rule that must hold in code, each with a test | 7 |
| `STC` | Reference circuit or error scenario with its expected result | 8 |
| `SS` | Spike: a measurement or prototype that settles an open technical fact | 9 |
| `SQ` | Open question for the owner, each with a default | 14 |
| `HS` | Human checkpoint | [sim-roadmap.md](sim-roadmap.md) |
| `S0.1`, `S1.1`… | Task (milestones are always written `Milestone S0`–`Milestone S4`, distinct from canvas spikes `S1`–`S8`) | [sim-roadmap.md](sim-roadmap.md) |

Status words follow the repository conventions: **DECIDED** (owner confirmed, or decided at
`HS1` under the owner's delegation of 2026-11-23), **DEFAULT** (build to this until an owner
decision replaces it), **HYPOTHESIS** (build only after the named `SS` spike confirms it), and
**PROPOSED** (becomes DECIDED or is amended at HS1). Since `HS1` no entry is PROPOSED.

`SPICE`, `ngspice`, `RIS-C` (`Reid-Engineered/RIS-C`), and standard component numbers
(`1N4148`, `2N2222`, `2N3904`, `2N7000`, `LM741`, `TL072`) are named here as open standards,
first-party/open-source engines, and generic device types. No commercial company or proprietary
product is named. Following parts `PD11`, the numbers are compatibility notes only: library
entry ids, file names, fixture ids and test names stay generic (`SD3`).

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
  covering the Wien bridge oscillator and standard analog circuits. Every component reaches the
  simulator as a part definition embedded in the document (parts `PD3`, `XD1`, `XD3`); this
  plan owns lowering those definitions to solver input (`XD4`).
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

Checked at Omnia `b046bbb` and `Reid-Engineered/RIS-C` `24710eaf`; rows amended at `HS1`
(2026-11-23) for `XD1`–`XD9`, against parts plan head `66f9e49` and context plan head
`6a50b28`.

| Fact | Where | Consequence |
| --- | --- | --- |
| `Reid-Engineered/RIS-C` (`risc-c`, BSD-3-Clause, Edition 2024, Rust 1.91+) already implements classic analog SPICE (`OP`, single-source `DC`, `AC lin\|dec\|oct`, adaptive `TRAN` with BE/Trap/BDF2, `R`/`C`/`L`/`K`/`S`/`W`/`V`/`I`/`E`/`G`/`F`/`H`/`D`/`Q`/`M`, `.model`, `.subckt`, `.ic`, `.nodeset`, `.temp`), validated against `ngspice 47` on Linux/macOS/Windows, with `CompiledCircuit`, concurrent `Simulator`s, and `transient_with` streaming + `ControlFlow::Break(())` cancellation | `Reid-Engineered/RIS-C` `docs/AGENT_INTEGRATION.md`, `docs/COMPATIBILITY.md`, `docs/STATUS.md` | Omnia does **not** re-implement a SPICE solver in-repo. `omnia-sim-risc` wraps the pinned `risc-c` Git crate behind `SimEngine`, obeying `RIS-C` contract rule 8 (integrate via public API, never copy equations). |
| `RIS-C` does not support runtime behavioral expressions, `.control`, `.measure`, or nested two-source `.dc` | `RIS-C` `docs/COMPATIBILITY.md` | Op-amp macromodels are written as standard SPICE `.subckt`s using `R`/`C`/`E`/`G`/`D` (identical on both `RIS-C` and `ngspice`), all measurements run in `omnia-sim`, and v1 `Analysis::Dc` is single-source. |
| `omnia-doc` has no UI framework dependency (`INV12`) and owns `Doc`, `Connectivity` (`NetId`, `net_of`, `net_wires`), `Doc::fork()`, and `Doc::revision()` | `crates/omnia-doc/src/` | Simulation crates take a `Doc` snapshot (`Doc::fork()`) and stay 100% GPUI-free (`SINV1`). `omnia-doc` owns connectivity; `omnia-circuit` (context plan) exposes revision-matched pin-to-net bindings and declared ground evidence through the read-only `Circuit::bindings` (context task `C1.9`) that lowering consumes (`XD4`). |
| `Part` currently stores `symbol`, `at`, `orient`, `designator: Arc<str>`, and `value: Arc<str>`, inside crate-private `StoredPart` | `crates/omnia-doc/src/doc.rs:53,93` | Parts `PD8` (tasks `P2.1`–`P2.3`) keeps `Part` unchanged and adds `def: PartDefId` and typed, override-only `params` on `StoredPart`, `Command::SetParam`, `Command::UpdatePartDef`, `Change::modified` and the single RON v3 with strict v2 migration (`XD1`). The simulator reads them through `Doc::part_def`, `Doc::part_def_id` and `Doc::resolved_param` and adds no document schema of its own (`SD9`, `SINV10`). |
| Parts plan (#85, `66f9e49`): one user-value grammar `PK4` in `omnia-doc` `units.rs` (`M` = mega; unit tokens and at most one space defined there); a one-terminal `rail` primitive with a `dc` voltage (`PK6`); shared parameter names `PK12`; embedded definitions carry pin roles and SPICE terminal order (`PD5`) and a `Primitive`/`Model`/`Subckt`/`None` binding (`PD3`); one built-in model tree under `library/` compiled by `cargo xtask parts` (`PD1`, `PD7`) | `parts-plan.md` sections 4, 6, 10 | The simulator has no value parser, no model tree and no model precedence of its own (`SK1`, `SD3`, `XD2`, `XD3`). |
| `omnia-symbols` currently has 6 compiled symbols: `Capacitor-IEEE-NonPolarized`, `Diode-COM-Standard`, `Ground-COM-General`, `IC-COM-OpAmp` (5 pins), `Resistor-IEEE-Potentiometer` (3 pins), and `Resistor-IEEE-Standard` | `crates/omnia-symbols/pins.toml`, `generated.rs` | `pins.toml` has one editing task at a time (`XD5`): parts `P1.4` adds the source symbols its starter parts need, then S1.1 adds the remaining v1 analog symbols from `vendor/electronic-symbols/` (inductor, any independent sources `P1.4` did not add, VCVS/VCCS dependent sources, NPN & PNP BJT, NMOS & PMOS MOSFET, supply-rail tags) and the symbol class, then S3.2 adds instrument/probe artwork. Every existing `SymbolId` stays unchanged. |
| `canvas-plan.md` D8 specifies background ERC/netlist on a `DocSnapshot` debounced at `K13` (`150 ms`) and single-producer ring buffers per channel drained by floating cached instrument views | `canvas-plan.md:366-378` | `omnia-sim` implements that exact SPSC ring-buffer contract (`SampleRing`) and debounced ERC runner. |
| `ui-plan.md` reserves toolbar analysis picker, Run/Pause/Stop controls, sim-time pill, status-bar ERC indicator, instruments strip, floating instrument frames (`U4.1`), and Results dock (`U1.7`, `U5.5`) | `ui-plan.md` UK3, UK4, UK6, UK10, UK11 | Milestone S3 wires real simulation state directly into those reserved slots with zero shell redesign. |
| `mcp-plan.md` §11 job model (`A3`) is designed for long-running cancellable simulation jobs; `AD7` sets tier 0 = read, tier 1 = undoable doc edit; `AD8`/`AD12` forbid floats in schematic DTOs | `mcp-plan.md:53-54`, `AD7`, `AD8`, `AD12` | S4.1 exposes `sim.erc`, `sim.results`, and `sim.measure` as tier 0 reads, `sim.run` as tier 0 read + bounded worker-process execution over `A3`, and adds an explicit `sim.*`-scoped amendment to `AD8`/`AD12` for finite canonical JSON numbers (while schematic geometry remains integer nm). |
| `deny.toml` allows `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `MPL-2.0` (no GPL/LGPL) and restricts `allow-git` | `deny.toml:10-38` | Bundled `ngspice` builds strictly from its BSD-3-Clause core with zero GPL/LGPL bundled or statically linked (system C/compiler runtimes excepted). `SQ4` (`HS1`, 2026-11-23): S1.5 adds `https://github.com/Reid-Engineered/RIS-C` to `deny.toml` `allow-git` pinned to a reviewed SHA; before S1.5 merges the owner must make `RIS-C` public or provide a read-only CI token (open owner action, `sim-roadmap.md` human checkpoints). |
| `tests/golden/wien-oscillator.ron` models the mockup's Wien bridge oscillator (`R1`/`R2 = 10 kΩ`, `C1`/`C2 = 22 nF`, `R4 = 22 kΩ`, pot `R3 = 50%`, `D1`/`D2 = 1N4148`, `U1 = TL072`, two empty-designator grounds; `f_0 ≈ 723.4 Hz`) as RON v2 | `tests/golden/wien-oscillator.ron` | The v2 golden stays byte-identical; after migration its parts are unmodelled (`XD1`). S2.2 builds a separately modelled Wien fixture from its topology (embedded definitions, typed parameters, `±12 V` rails, headless probe specifications) as the headline `STC-wien-01` reference circuit for both engines. |

## 3. Budgets

Measured on the Orion reference machine under canvas plan §2.1 (`1920x1080 @ 60 Hz`, `100%` and
`125%`, ten interleaved repetitions) unless marked deterministic. All canvas (`B1`–`B10`) and UI
(`UB1`–`UB10`) budgets still hold while simulation is idle or running; these are added.

| ID | Budget | Measured by |
| --- | --- | --- |
| `SB1` | **ERC latency:** full ERC on `1k` sheet < 5 ms p99; on `10k` sheet < 40 ms p99 (background thread; 0 ms UI stall), on the modelled stress sheets of `SB2`. Evidence reports cold (fresh snapshot, empty caches), incremental (after one edit) and cached (no change since the last run) cases separately, and each case meets the threshold (`XD9`). | `cargo bench -p omnia-bench --bench sim` (Orion) |
| `SB2` | **Netlist lowering:** embedded definitions + `omnia-circuit` bindings → `CircuitIr` + SPICE text on `1k` sheet < 5 ms p99; on `10k` sheet < 40 ms p99. Measured on the new modelled stress sheets `Modelled1k` and `Modelled10k` (every part bound to a built-in definition, so lowering expands real devices; the existing stress sheets never change, parts `PD11`). Evidence reports cold, incremental (after one edit, through context `C1.3`'s incremental update) and cached cases separately, and each case meets the threshold (`XD9`). | same bench |
| `SB3` | **Cold run latency (Wien oscillator):** from Run click to first sample in `SampleRing` < 150 ms p99 on `ngspice` and < 50 ms p99 on `RIS-C`. The measured span includes acquiring the document snapshot and the revision-matched `Circuit::bindings` read (or a circuit snapshot), ERC preflight and lowering (`XD9`); context `C1.7` records the bindings read cold and after an incremental update as evidence and gates nothing. | `bench sim --circuit wien --engine ngspice\|risc` (Orion) |
| `SB4` | **Cancellation latency:** Stop clicked during a heavy transient run returns to `Idle` and frees worker CPU within < 100 ms p99 (no orphan worker processes). | `bench sim --script cancel` (Orion) |
| `SB5` | **UI isolation during live simulation:** with `1k` sheet open, transient streaming into `XSC1` oscilloscope at `SK8` cadence (`30 Hz`, or `60 Hz` if `SS2` passes), `B3` (< 16.7 ms p99, 0 missed refreshes), `B5` (≤ 33.3 ms), `UB3` (`SheetView` paint count = 0 while only `XSC1` updates), and `B10` (`allocs_per_frame.ours == 0` once `XSC1` buffers are warm) all hold. | `bench ui --sheet 1k --script sim-live` (Orion) |
| `SB6` | **Memory:** B9 (< 300 MB) holds on `10k` with simulation idle, and `< 300 MB` holds on `1k` with active live simulation + `XSC1` + Results dock; ring buffers and stored results are strictly bounded by `SK6` and `SK7`. | `memory_mb` in `SB5` and `B9` runs (Orion) |
| `SB7` | **Differential accuracy (`RIS-C` vs `ngspice`):** under the differential test tolerances (`SK2`), linear `OP`/`DC` relative error ≤ `SK4` (`1e-7`); nonlinear `OP`/`DC` ≤ `SK5` (`1e-4` rel + `1e-6` V/A abs); `AC` magnitude ≤ `0.05 dB` and phase ≤ `0.5°`; `TRAN` steady-state oscillation frequency ≤ `0.5%` and peak-to-peak amplitude ≤ `1%` on `STC-wien-01`. | deterministic CI test suite (`cargo test -p omnia-sim-tests`, created in S2.1) |
| `SB8` | **Idle zero cost:** when simulation is `Idle` (or `Paused` with static traces), `UB4` still holds (0 frames drawn, 0 worker CPU over 10 s). | `bench idle --shell --sim-idle --seconds 10` (Orion) |

## 4. Constants

| ID | Constant | Value |
| --- | --- | --- |
| `SK1` | User-facing value grammar (amended at `HS1` per `XD2`) | Parts `PK4`, one parser in `omnia-doc` `units.rs` (created by parts `P1.1`), shared by the parts loader, `Doc::apply` and the simulator: case-sensitive SI suffixes `f`, `p`, `n`, `u`/`µ`, `m` (`1e-3`), `k`/`K`, `M` (`1e6`), `G`, `T`, plus `meg`/`MEG` (`1e6`). `M` is mega and `m` is milli (parts `PQ2`). `omnia-sim` has no parser of its own: it reads typed parameters whose reals are finite `FiniteF64` values (parts `PD6`). Native SPICE number syntax applies only inside embedded model decks; the netlist emitter writes every number in plain decimal or exponent form with no scale suffix (`SINV6`). Unit tokens and whitespace (`10 kΩ`, `22 nF`: one optional space, the parameter's declared unit) are accepted exactly as `PK4` defines them. |
| `SK2` | SPICE solver tolerances | Interactive default options (passed identically to both engines via `.options` / `RIS-C` options): `RELTOL = 1e-3`, `ABSTOL = 1e-12 A`, `VNTOL = 1e-6 V`, `ITL1 = 100`. Differential oracle tests (`SB7`) tighten both engines to `RELTOL = 1e-6`, `ABSTOL = 1e-13 A`, `VNTOL = 1e-7 V` so stopping tolerance is 100× tighter than `SK5`. |
| `SK3` | Default temperature | `TNOM = 27.0 °C` (`300.15 K`), thermal voltage `V_T = k T / q ≈ 25.86492 mV` (`25.865 mV`). |
| `SK4` | Linear differential tolerance | `|a - b| <= 1e-7 * max(|a|, |b|) + 1e-12` (exact MNA match modulo floating-point ordering). |
| `SK5` | Nonlinear differential tolerance | `|a - b| <= 1e-4 * max(|a|, |b|) + 1e-6` for voltages (V) and currents (A); AC `<= 0.05 dB` magnitude, `<= 0.5°` phase; Wien transient `<= 0.5%` frequency, `<= 1.0%` Vpp. |
| `SK6` | Ring-buffer capacity | `SampleRing` holds `65,536` `(t: f64, y: f64)` samples per channel (`1 MiB` per channel, power-of-two lock-free SPSC ring). Producer overwrites oldest or drops with a monotonic `dropped_count` counter if UI stalls — never blocks the solver thread or grows heap. |
| `SK7` | Stored run result cap | At most `2,000,000` sample points per trace (`32 MiB` per trace) and `128 MiB` total stored result memory per run (oldest points decimated or run halts cleanly with typed `ResultCapReached` diagnostic). |
| `SK8` | Instrument redraw cadence | Default `30 Hz` (`33.3 ms` timer) for live oscilloscope updates, promotable to `60 Hz` only if `SS2` (S0.4) proves `B3` holds on Orion (canvas plan §11 risk row). |
| `SK9` | ERC debounce | `K13 = 150 ms` after the last `Doc::revision()` change (D8). |
| `SK10` | Reference ground node | SPICE node `"0"` (`NodeId(0)`). A net maps to `NodeId(0)` exactly when it carries declared ground evidence as context `CK4` defines it, which is `SQ3`'s default: a part whose symbol class is `Ground`, or a label whose text is exactly `0`. Lowering reads that evidence from `Circuit::bindings` (context `C1.9`, `XD4`). A `ground`-role pin (parts `PD5`) does not set the reference, and a name such as `GND` or `AGND` never does: it gets the `ERC001` hint, shared with context `ground.name_only`. Context `CD13` name/topology heuristics feed context findings only, never solver connectivity. |
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
     embedded part defs + typed params (parts PD8, XD1)          |
     omnia-circuit bindings + ground evidence (context, XD4)     |
                |                          ^                     |
   +------------v--------------------------+---------------------v------------+
   | omnia-sim (pure Rust, no GPUI, INV12)                                    |
   |   binding.rs   embedded definition + resolved params -> typed Device     |
   |   erc.rs       deterministic ERC on Doc snapshot (SB1, XD6)              |
   |   ir.rs        canonical CircuitIr + many-to-many source map (SB2, XD4)  |
   |   netlist.rs   deterministic SPICE emitter, plain numbers (SK1, SINV6)   |
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
- `omnia-doc` has **zero simulator dependency**. It stores schematic items, embedded part
  definitions and typed parameter overrides (parts `PD8`, `XD1`) and screens embedded decks
  (parts `PD10`); all lowering lives in `omnia-sim` (`XD4`).
- Dependencies run one way (`XD8`): `omnia-parts` (catalog, import), `omnia-circuit` (derived
  circuit facts) and `omnia-sim` plus its adapters (execution, results) are all headless.
  `omnia-sim` reads `omnia-doc` and the read-only `Circuit::bindings` (context `C1.9`); it needs
  no `omnia-parts` dependency at run time because every definition it lowers is embedded in the
  `Doc` (`XD3`). Test crates may use `omnia_parts::Catalog::builtin()` to build fixtures.
- Three identities stay distinct types (`XD4`): context `CK1` net keys, the agent plan's
  `NetKey`, and the solver `NodeId`.
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

### SD3 v1 component and model set. DECIDED (2026-11-23; parameters and model resolution amended at HS1 per XD1, XD3)

The component coverage below stands (`XD3`). Every parameter named here is a typed parameter
declared in the part definition, using the parts plan's shared names (`PK12`).

- **Passives:** Resistor (`R`, `resistance`), Potentiometer (`R_pot`, 3-terminal, wiper ratio
  `wiper` in `0..1` and total resistance `resistance`, both with the defaults its definition
  declares; the simulation plan proposes `20 kΩ` and `0.5`; lowered to two series resistors
  `(1 - α)R` and `αR` meeting at pin 3), Capacitor (`C`, `capacitance`, optional `ic`), Inductor
  (`L`, `inductance`, optional `ic`).
- **Sources & power:** Independent DC/AC/SIN/PULSE voltage (`V`) and current (`I`) sources
  (`waveform` enum `dc`/`sin`/`pulse` with its real fields, `ac_mag`, `ac_phase`); linear
  dependent sources VCVS (`E`) and VCCS (`G`) (used both directly and inside macromodels); named
  supply rail tags (`+12V`, `-12V`, `+5V`, `+3.3V`, etc., lowered to DC voltage sources
  referenced to ground `0`, `SD10`); Ground (`0`).
- **Semiconductors:** Diode (`D`), NPN/PNP BJT (`Q`), NMOS/PMOS MOSFET (`M`) — using the classic
  SPICE device parameter subset supported identically by both `ngspice` and `RIS-C` (`docs/COMPATIBILITY.md`).
- **Op-amps:** Behavioral op-amp `.subckt` macromodel (`A_OL`, `GBW` dominant pole, `R_in`,
  `R_out`, diode rail-clamped output stage built strictly from `R`/`C`/`E`/`G`/`D` — zero
  behavioral expressions so it runs identically on both `RIS-C` and `ngspice`) plus built-in
  standard macromodels (`TL072`, `LM741` classes) authored from public datasheet parameters.
- **Model resolution (`XD3`):** the placed part's embedded definition decides. A `Primitive`
  binding (parts `PK6`) is lowered from its kind, its terminal order (parts `PD5`) and the
  resolved parameters; a `Model` or `Subckt` binding is lowered from the deck embedded in the
  definition. A definition that embeds several model entries selects one with its `model` enum
  parameter (`PK12`). Nothing is chosen from `Part::value`. A generic op-amp gets the `SQ8`
  behavioural macromodel as its default binding at placement, through its part definition
  (parts `PD11` `opamp-generic`).
- **Unmodelled parts:** a definition with binding `None` (every migrated v2 part and every
  `PlaceDef::Unmodelled` placement, parts `PD8`) is not lowered. A `Component`-class part of
  that kind with a connected pin is `ERC009` ("no simulation model", `SD8`). `Instrument` and
  `Probe` parts are lowered by class (`SD10`); ground and supply rails are lowered from their
  `ground` and `rail` primitive bindings (parts `PK6`).
- **Default binding at placement (`XD3`).** The shared placement helper (parts `P2.4`
  `Catalog::place_command`) embeds the catalog definition whenever one exists for the symbol
  being placed, so a generic op-amp placed from the picker, the agent or a test carries the
  `SQ8` macromodel binding. Placing through `PlaceDef::Unmodelled` is for migrations and
  symbols with no catalog definition (requests in section 13).
- **Supply contract.** A `Model` or `Subckt` binding that needs supplies declares them in the
  embedded definition, and lowering (S1.2) and ERC (S1.3) read the same declaration:
  - `supply_terminals`: the terminal indices (into the binding's terminal list) that are supply
    terminals, each mapped to a pin whose role is `power` (parts `PD5`);
  - `implicit_supply`: either none, or one finite voltage per supply terminal (the `SQ8`
    generic op-amp declares `+15 V` and `-15 V`).
  Lowering a part: if every supply pin is connected (not alone on its net), its terminals are
  wired to those nets; if none is connected and `implicit_supply` is declared, lowering adds
  internal DC sources at the declared voltages referenced to node `0` (and `ERC008` is a
  Warning); otherwise the part is not lowered and `ERC008` is an Error. There is no partial
  fallback: exactly one of two supply pins connected is an Error. The binding fields belong to
  the parts plan's `EmbeddedSimBinding` and its `PD10` validator (requested in section 13).
- **One model source (`XD3`):** the built-in models live in the parts plan's single
  `library/` tree (parts `PD1`, `PD7`, `PK11` licences). S1.1 contributes the `SD3` definitions
  the parts starter set (`PD11`) lacks to that tree, together with the symbols they bind to
  (rail definitions included, parts `PD11`); S1.2 owns lowering only; there is no second deck
  tree in `omnia-sim`. Following parts `PD11`, library ids, file names, fixture ids and test names are
  generic (for example `diode-signal`, `opamp-generic`); standard component numbers appear only
  as compatibility notes in descriptions.

### SD4 v1 instruments and result views. DECIDED (2026-11-23)

- **Oscilloscope (`XSC1`):** 2-channel (`A`, `B`, colours `probe/a`, `probe/b`) + trigger
  floating view inside `U4.1`'s frame (`576x468` per `UK10`). Drains `SampleRing` on a fixed
  cadence (`SK8`), never blocks the solver, never repaints `SheetView` (`SB5`, `UB3`).
- **Multimeter (`XMM1`):** 2-terminal floating readout for DC V, AC Vrms, DC/AC I, or R.
- **Bode Plotter (`XBP1`):** 2-port (`IN`, `OUT`) floating magnitude (dB) + phase (deg)
  log-frequency plot driven by `AcResult` with cursors and `-3 dB` / phase-margin readouts.
- **Results Panel:** Bottom dock (`U1.7`/`U5.5`) with tabbed Plots, Measurements table,
  Diagnostics/ERC list (click to focus item on sheet), and CSV export.

### SD5 `ngspice` process isolation via helper worker. HYPOTHESIS (settled by SS1; `SQ1` default kept at HS1, 2026-11-23)

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

### SD6 `RIS-C` integration boundary (`omnia-sim-risc`). DECIDED (HS1, 2026-11-23)

- Pin `risc-c` by exact Git commit SHA in workspace `Cargo.toml` (starting at `24710eaf`,
  upgraded explicitly like GPUI; `Cargo.lock` committed) and never copy internal solver code into
  Omnia (`RIS-C` `AGENT_INTEGRATION.md` rule 8). `deny.toml` `allow-git` admits the repository
  (`SQ4`); before S1.5 merges, the owner makes `RIS-C` public or provides a read-only CI token.
- `omnia-sim-risc` compiles `CircuitIr` via `risc_c::netlist::parse_str` (with `SK2` options
  applied) or `CircuitBuilder` into a shareable `CompiledCircuit`, creates a fresh `Simulator`
  per run on a background worker thread, streams transient samples via `transient_with` directly
  into `SampleRingProducer`, supports Pause via a worker condition variable and Stop via
  `ControlFlow::Break(())` (`Error::Cancelled`), and maps `risc_c::RunError` through `SourceMap`
  into typed `SimError`.
- Never silently substitute `0.0` for missing signals (`sample.get(&circuit, name)` error
  propagates explicitly per `RIS-C` contract 4).

### SD7 Deterministic circuit lowering and source mapping. DECIDED (HS1, 2026-11-23; amended per XD4)

- **Ownership (`XD4`).** `omnia-sim` owns lowering: `CircuitIr`, node ordering, device
  expansion, source maps, diagnostics mapping and SPICE text. `omnia-doc` owns connectivity.
  Lowering consumes the revision-matched pin-to-net bindings and declared ground evidence that
  `omnia-circuit` exposes through the read-only `Circuit::bindings` (context task `C1.9`, which
  precedes S1.2; context `CINV14`); it never re-derives connectivity, and a bindings read whose
  revision differs from the `Doc` snapshot's is refused as stale. Bindings carry each net's
  circuit key and current `NetId` side by side; lowering assigns `NodeId`s itself. Device terminals come from the
  embedded definition's terminal order (parts `PD5`), never from pin count or position.
- `CircuitIr::lower(doc, bindings, analysis, probes)` produces bit-for-bit identical node ids,
  device order, and SPICE netlist text regardless of arena insertion order or hash-map seeding
  (`SINV2`, matching `AGENTS.md` determinism rules: geometry and label/designator before id):
  - Ground nets collapse to `NodeId(0)` (`SK10`); supply-rail nets of the same rail name join
    one node (`SD10`).
  - Non-ground nodes sort deterministically by `(explicit_label_name, min_pin_key)`, where the
    label name is the lowest label text by byte order among the node's nets and
    `pin_key = (part.designator, pin_number, world_y, world_x, part_id)`.
  - Devices sort by `(spice_prefix, part.designator, world_y, world_x, part_id, sub_index)`,
    where `sub_index` orders the devices one part expands into.
- **Source maps are many-to-many (`XD4`).** `SourceMap` records `NodeId -> {NetId…}` (one solver
  node may carry several nets: every ground net, every net of one rail name) with the inverse
  `NetId -> NodeId`, and `PartId -> {DeviceIdx…}` (one part may expand into several devices: a
  potentiometer into two resistors, a rail into a shared source) with the inverse
  `DeviceIdx -> PartId`, so every ERC error, convergence failure, and waveform channel highlights
  the exact schematic nets or part.

### SD8 Electrical Rules Check (ERC) ruleset. DECIDED (HS1, 2026-11-23; amended per XD2, XD6, XD9)

- Severity levels: **`Error`** (blocks simulation start), **`Warning`** (allows run, shown in
  status bar & Diagnostics), **`Info`**. This Error/Warning policy is the Run gate; context
  finding severities (context plan section 9.4) are advisory and never gate a run (`XD6`).
- `ERC001`–`ERC008` are registered diagnostic codes (`XD9`); `ERC009` and `ERC010` are added
  at `HS1` as simulation-only codes (coordinator decision on the review of #104). A retired code
  is never reused.
- **Crosswalk (`XD6`).** Where a check has a context counterpart, both evaluate one shared
  predicate over the same pin and net facts; each surface keeps its own severity.

| Code | Severity | Predicate | Context counterpart |
| --- | --- | --- | --- |
| `ERC001` | Error | The sheet has at least one part whose symbol class is not `Ground`, and no net carries declared ground evidence (`SK10`, `SQ3`, context `CK4`, read from `C1.9`; the same condition as context `ground.missing`, so an empty sheet raises nothing). A net whose label folds to a ground-like name (`gnd`, `agnd`, `dgnd`, `pgnd`, `vss`, per context `ground.name_only`) without declared evidence adds a hint to label it `0` or place a ground symbol. | `ground.missing`, `ground.name_only` |
| `ERC002` | Error | Duplicate designator under the context plan's section 9.4 rules: ASCII case folding only (`A`–`Z` to `a`–`z`, no trimming or Unicode normalisation), and a blank designator (empty, or only ASCII spaces and tabs) never joins a duplicate group. Context's `designator.blank` (a blank designator on a part that is not `Ground` or `PowerRail` class) has no ERC code. | `designator.duplicate` |
| `ERC003` | Error | The shared parse-failure case: a typed parameter the binding needs fails validation (parts `PD6`/`PD10` predicate: unparseable under `PK4`, non-finite, outside its declared bounds, such as `resistance <= 0` or `wiper` outside `0..1`; `apply` already refuses such values, so this guards documents built any other way), or a legacy unmodelled part's free text fails `PK4` (the `value.unparsed` predicate). | typed parameter validation; `value.unparsed` for legacy unmodelled parts |
| `ERC004` | Error | Zero-resistance loop of ideal voltage sources or supply rails, or such a source shorted across itself or to ground. | `source_loop`, where the predicates coincide |
| `ERC005` | Error | Unconnected instrument port, or a probe referencing a missing net. Tested with the instrument parts in S3.2 (`STC-erc-02`). | none (simulation only) |
| `ERC006` | Warning | Floating pin or single-pin net; pins whose role is `no_connect` (parts `PD5`) are exempt. | `pin.unconnected`, `net.single_pin` |
| `ERC007` | retired | Was "bare `M` on a resistor". Retired by `XD2`: `M` is mega in every user-facing value, so the ambiguity no longer exists. | none |
| `ERC008` | Error, or Warning when the implicit supply is used | Model-required supply check that this plan owns, over the binding's supply contract (`SD3`) and the shared pin facts (`power`-role pins and their nets, context `C1.9` and `C3.1`). Error: some but not all supply pins are connected, or none is connected and the binding declares no `implicit_supply`. Warning: none is connected and the binding's `implicit_supply` (such as `SQ8`'s `±15 V`) is used. All connected: nothing. | none (context `power.undriven` stays off per `CQ2`) |
| `ERC009` | Error | No simulation model: a `Component`-class part with a connected pin whose definition's binding is `None` (every migrated v2 part, every `PlaceDef::Unmodelled` placement), naming the part with the hint to replace it with a library part. | none (simulation only) |
| `ERC010` | Error | Conflicting rail voltage: two or more `rail` parts with the same rail name (`SD10`) resolve to different `dc` values. Reported before lowering, naming every part in the group. | none (simulation only) |

- The same crosswalk is recorded in context plan section 9.4; where the two texts differ, the
  context plan defines case folding and blank designators and this plan defines ERC severities
  and predicates.

### SD9 Value & parameter storage: the parts plan's typed parameters. DECIDED (HS1, 2026-11-23; rewritten per XD1)

- **Withdrawn (`XD1`).** This plan's `StoredPart::props`, `Doc::part_props`,
  `Command::SetPartProp` and its own RON version bump are withdrawn. No simulation task changes
  the document schema, `Part`, `StoredPart`, `Command` or the RON version.
- **Owner.** Parts `PK9`, `PD6`, `PD8` and tasks `P2.1`–`P2.3` own `Part::def` (stored as
  `StoredPart::def`), typed override-only `params`, `Command::SetParam`,
  `Command::UpdatePartDef`, `Change::modified` and the single RON v3. `P2.3` owns the strict
  v2 → v3 migration (legacy parts become unmodelled definitions), the v3 writer, and the
  unsupported-version fixture bumps in `omnia-doc`, `omnia-agent`, `ui-plan.md` and
  `mcp-plan.md`.
- **Simulation keys are typed parameters.** Resistance, capacitance, inductance, potentiometer
  wiper, `ic`, source `waveform` and its fields, `ac_mag`, `ac_phase` and the `model` selector
  are parameters declared in part definitions under the parts plan's shared names (`PK12`).
  The simulator reads them with `Doc::resolved_param` (override, else the definition default)
  and never parses `Part::value`.
- The v2 goldens stay byte-identical; S2.2 builds a separately modelled Wien fixture (`XD1`).

### SD10 Supply rails and on-sheet probes/instruments. DECIDED (HS1, 2026-11-23; amended per XD4, XD5)

- **Symbol class vs pin role (`XD5`).** S1.1's symbol class (`Component`, `Ground`, `PowerRail`,
  `Instrument`, `Probe`) is a per-symbol type in `omnia-symbols`; parts `PD5`'s per-pin
  electrical roles are a separate type. A placed part's embedded definition is authoritative
  for pin roles and SPICE terminal order. Lowering dispatches `Instrument` and `Probe` parts
  by class, and every other part by its binding (`SD3`): `Ground` parts by the `ground`
  primitive, `PowerRail` parts by the `rail` primitive, `Component` parts by theirs.
- Supply rail symbols (`+12V`, `-12V`, `+5V`, `+3.3V`, custom voltage label) are ordinary `Part`
  entries with the `PowerRail` symbol class and a `rail` primitive binding whose `dc` parameter
  is the rail voltage (parts `PK6`, `PK12`). S1.1 adds the rail symbols and their part
  definitions through the parts library (parts `PD11`, `XD3`, `XD5`). A rail definition declares
  no `primary_param`, so `Part::value` is free display text: it is the **rail name** (for
  example `+12V`), compared under context section 9.4's ASCII case folding. It names a node and
  never selects a model. ERC and `CircuitIr` connect every rail of the same rail name into one
  solver node driven by one ideal DC voltage source of value `dc` referenced to ground `0` (so a
  Wien fixture needs only rail tags at its op-amp supply pins, matching
  `design/build-omnia.mjs:188`). Rails of one name whose `dc` values differ are `ERC010` and are
  not lowered. The source map lists every rail net on that node (`SD7`).
- On-sheet instruments (`XSC1`, `XMM1`, `XBP1`) are `Part` entries with the `Instrument` symbol
  class whose pins wire onto the sheet like any other component (`XSC1` channels A/B wired in
  `design/build-omnia.mjs:229-234`); `CircuitIr` treats instrument voltage inputs as high-Z
  probes (`10 MΩ` default or ideal open, configurable per `SQ5` through typed parameters of the
  instrument's definition, named by S3.2) and current inputs as zero-volt ammeters (`V = 0`
  branch current sense).
- Ground is read from `omnia-circuit`'s declared ground evidence (`SK10`, `XD4`).

### SD11 Live streaming & UI threading. DECIDED (extends canvas-plan.md D8; live-edit policy per SQ7, default kept at HS1 2026-11-23; stale rule per XD7)

- Simulation never runs on the GPUI UI thread (`SINV1`, `D8`).
- Live `TRAN` streams `(t, v_0, v_1, ...)` into lock-free SPSC `SampleRing`s (one per probe
  channel, `SK6`).
- The floating `XSC1` oscilloscope view drains available samples into its pre-allocated display
  buffer on its `SK8` cadence (`30 Hz` default) and notifies **only** itself — `SheetView` stays
  `.cached()` with 0 repaints (`SB5`, `UB3`, `INV11`).
- **Live-edit policy (`DEFAULT`, `SQ7`):** Editing the schematic while a simulation is `Running`
  stops the live run cleanly and marks results stale with a banner; never mutates a live
  `CircuitIr` mid-solve.
- **What counts as an edit (`XD7`).** Results go stale on any `Change` whose revision advances,
  including `Command::SetParam` and every instance an `Command::UpdatePartDef` moves (listed in
  `Change::modified`, parts `PD8`), even when topology and display text are unchanged, and on
  undo and redo of those commands.

### SD12 Bundled `ngspice` licensing and build hygiene. DECIDED (policy, HS1 2026-11-23; exact flags verified at HS2)

- `ngspice` core is **BSD-3-Clause** (compatible with `deny.toml`).
- Bundled builds must contain **zero GPL or LGPL code bundled or statically linked** (standard OS
  C and compiler runtimes such as `glibc`, `libgcc`, and `libstdc++` excepted). Optional
  GPL/LGPL-dependent features (such as `readline`, `FFTW`, `OpenMP`, and non-BSD solvers or GUI
  toolkits) are disabled; exact configure flags and binary library audits are settled in `SS1`
  (S0.3) and signed off at `HS2` (S4.2).
- `THIRD_PARTY.md` records the exact `ngspice` version, source URL, build flags, and BSD-3-Clause
  licence text, plus the pinned `RIS-C` attribution.

### SD13 Structured errors and convergence diagnostics. DECIDED (HS1, 2026-11-23; amended per XD4)

- Raw SPICE stderr is never dumped raw as the only user message. `omnia-sim` parses failures from
  both `ngspice` and `risc_c::RunError` into typed `SimError` variants (`ErcFailed`,
  `SingularMatrix { nodes }`, `NoConvergence { phase, step_t, nodes }`, `TimestepTooSmall { t }`,
  `WorkerCrashed`, `Cancelled`, `ResultCapReached`, `EngineUnavailable`), mapped through
  `SourceMap` to net names and `PartId`s with actionable fix hints (e.g., "Check for floating
  node on net `N3` or add a DC path to ground").
- Because the source map is many-to-many (`SD7`, `XD4`), a diagnostic on a solver node names
  every net mapped to it, and a diagnostic on a device names the part it was expanded from.
  Solver `NodeId`s are never shown to the user or used as a net identity outside `omnia-sim`.

### SD14 Agent / MCP integration. DECIDED (HS1, 2026-11-23; amended per XD8)

- Adds 4 tools to `omnia-agent` (after `A3.4`), classified per `mcp-plan.md` `AD7`:
  - `sim.erc` (**tier 0 read**): the Run preflight; runs ERC on an open document, returns typed
    `ErcDiagnostic` list (`SD8`). Circuit views (nets, pins, findings) are served by the context
    plan's `omnia_context`, not by `sim.*` (`XD8`).
  - `sim.run` (**tier 0 read + bounded worker process execution**): launches `OP`/`TRAN`/`AC`/`DC`
    on `ngspice` or `RIS-C` via an `A3` cancellable job with progress notifications; mutates no
    document and runs under `SK11`/`SK12` resource limits (spawns `omnia-ngspice-worker` when
    `engine = "ngspice"`, declared explicitly in the `AD7` policy table in `S4.1`).
  - `sim.results` (**tier 0 read**): paged/decimated waveform slice or operating-point table for
    a completed `sim_run_id`.
  - `sim.measure` (**tier 0 read**): computes `vpp`, `vrms`, `vavg`, `vmin`, `vmax`, `freq`,
    `duty`, `rise_time`, `fall_time`, `bandwidth_3db`, `phase_margin` on any probed net/branch.
- **Numeric encoding (`AD8`/`AD12` amendment in `S4.1`):** Schematic geometry remains integer
  nanometres (`AD8`/`AK8`). `S4.1` amends `AD8`/`AD12` exclusively for `sim.*` payloads to permit
  finite IEEE-754 JSON numbers (rejecting `NaN`/`Inf` with a typed error, normalising `-0.0` to
  `0.0`, and emitting canonical shortest decimal representation so `AINV5` byte-identical JSON
  still holds). The amendment stays scoped to `sim.*` (`XD8`): document parameters keep the
  parts plan's text-plus-value encoding and every other payload keeps `AD8`/`AD12` unchanged.
- `sim.*` payloads identify nets with the agent plan's `NetKey` (or net names) and parts with
  `PartId`/designator; solver `NodeId`s do not appear (`XD4`). Parameter edits go through the
  parts plan's `set_param`/`update_part_def` on the existing preview/apply path, not through a
  `sim.*` tool (`XD8`).

### XD cross-plan decisions (coordinator, owner-delegated, 2026-11-23)

The owner delegated pending sign-offs to the coordinating agent on 2026-11-23. After an
independent review of the parts plan (#85) against this plan and the context plan (#100), the
coordinator decided `XD1`–`XD9`. They bind this plan; the full text is copied in
`parts-plan.md` section 6. What each one changes here:

- **XD1 One document schema.** `SD9` rewritten (props, `part_props`, `SetPartProp` and the sim
  RON bump withdrawn; typed parameters per parts `PK12`); `SINV10`; S1.1 does no schema work;
  S2.2 builds a separately modelled Wien fixture; section 13.
- **XD2 One numeric grammar.** `SK1` (parts `PK4`, `M` = mega), `SQ2`, `SINV6`, `STC-erc-01`,
  S1.2; `ERC007` retired.
- **XD3 One model source.** `SD3` model resolution from the embedded definition; generic
  op-amp default binding (`SQ8`) at placement through the shared placement helper; S1.1
  contributes extra `SD3` definitions (rails included) to the shared `library/` tree; no second
  deck tree; no model from `Part::value`.
- **XD4 Bindings, ground and lowering.** `SD7`, `SK10`, `SD13`, section 5; lowering consumes
  `Circuit::bindings`; S1.2 waits for context task `C1.9`; ground evidence matches context
  `CK4`; three distinct identity types.
- **XD5 Symbol class vs pin role.** `SD10`; `pins.toml` editing order `P1.4` → S1.1 → S3.2.
- **XD6 ERC crosswalk.** `SD8` table, matching context section 9.4; Run gate vs advisory
  context severities; case folding and blank designators cited from context section 9.4.
- **XD7 File ownership order.** `SD11` stale-result rule. S1.1 no longer owns any `omnia-doc`
  file, so its slot in the `omnia-doc` order is empty (section 13).
- **XD8 Crates and agent surface.** Section 5 dependency direction; `SD14` (`sim.erc` is Run
  preflight, `omnia_context` serves circuit views, finite-number amendment scoped to `sim.*`).
- **XD9 Ids and budgets.** `ERC001`–`ERC008` registered (`SD8`; `ERC009`, `ERC010` added as
  simulation-only codes); thresholds unchanged; `SB3`
  includes snapshot and bindings acquisition; `SB1`/`SB2` evidence separates cold, incremental
  and cached runs.

## 7. Invariants

Each has a unit or property test; the roadmap task that adds it is in brackets.

| ID | Rule |
| --- | --- |
| `SINV1` | `omnia-sim`, `omnia-sim-ngspice`, and `omnia-sim-risc` have zero dependency on GPUI or `omnia-app` (extending `INV12`). [S1.2, S1.5, S1.6] |
| `SINV2` | **Determinism:** rebuilding the same circuit with parts/wires/labels (and their embedded definitions) inserted in any order yields identical `ErcDiagnostic`s, identical `CircuitIr` node/device numbering, and identical SPICE netlist text. [S1.2, S1.3] |
| `SINV3` | **Document immutability:** running ERC or any simulation never mutates `Doc` or changes `Doc::revision()`. [S1.3, S1.4] |
| `SINV4` | **Bounded streaming memory:** `SampleRing` never allocates after construction and never blocks the solver thread if the UI stops draining. [S1.4] |
| `SINV5` | **Clean cancellation:** cancelling a running simulation on either `ngspice` or `RIS-C` terminates worker execution within `SK11` and leaves zero orphan child processes or leaked threads. [S1.5, S1.6] |
| `SINV6` | **SPICE number safety (amended per `XD2`):** the netlist emitter writes every finite parameter value in plain decimal or exponent form with no scale suffix, and parsing the emitted text back gives the same `f64` bit for bit; a value written by the user as `10M` reaches the netlist as `1e7` (never as an engine-dependent `M`). The `PK4` grammar itself, including refusal of non-finite values, is tested by parts `P1.1`. [S1.2] |
| `SINV7` | **No silent signal fallback:** unknown or unallocated probe signal names always return a typed error on both `RIS-C` and `ngspice` (never silently returning `0.0`; `RIS-C` contract 4). [S1.5, S1.6] |
| `SINV8` | **Differential parity:** every `STC-*` reference circuit produces matching results on `RIS-C` and `ngspice` within `SB7` (`SK4`/`SK5`). [S2.1, S2.2] |
| `SINV9` | **Sheet isolation:** live oscilloscope updates at `SK8` cadence trigger 0 `SheetView` paints (`INV11`, `UB3`) and 0 Omnia heap allocations per warm paint frame (`B10`). [S3.3, S3.6] |
| `SINV10` | **No second document schema (rewritten per `XD1`):** no simulation crate or task changes `Part`, `StoredPart`, `Command`, the RON version or any existing `tests/golden/*.ron` file; simulation reads definitions and parameters only through `omnia-doc`'s public queries. v2 load compatibility itself is parts `PTC-v2-compat` (`P2.3`). Tested by S2.2 loading the unchanged `tests/golden/wien-oscillator.ron` beside the modelled fixture and checking the golden's bytes on disk. [S1.1, S2.2] |
| `SINV11` | **Licence purity:** bundled `ngspice` build and all Rust simulation crates (including `risc-c` transitive deps) bundle/statically link zero GPL/LGPL code (system C/compiler runtimes excepted); `cargo deny check` stays green. [S0.3, S1.5, S1.6, S4.2] |
| `SINV12` | **Source-map completeness:** every non-ground `NodeId` maps back to one or more live `NetId`s, every `DeviceIdx` to exactly one live `PartId`, and every net and modelled part in the source `Doc` snapshot to its node or devices (`SD7`, `XD4`). [S1.2] |

## 8. Reference circuits and scenarios (`STC`)

Every circuit below lives as a committed fixture in `tests/sim/` (crate `omnia-sim-tests`,
created in S2.1) and runs on **both** `ngspice` and `RIS-C` against analytical solutions and
each other under `SK2`'s differential tolerances.

| ID | Circuit / Scenario | Analysis | Expected result |
| --- | --- | --- | --- |
| `STC-erc-01` | Missing ground (plus a net labelled `GND` that must raise the `ERC001` hint), an empty sheet that must raise nothing, duplicate `R1` (plus two empty-designator grounds that must pass), `resistance = 0` loaded from a hand-built file, a migrated v2 resistor wired into the circuit with a parseable value, shorted DC sources, floating pin, a `10M` resistor that must pass (`XD2`), an op-amp whose model declares supplies but no `implicit_supply` with both supply pins unconnected, a second one with exactly one supply pin connected, the generic op-amp on its implicit supply, and two `+12V` rails with `dc` `12` and `15` | ERC | Exact `ERC001`–`ERC004`, `ERC006`, `ERC008`, `ERC009` and `ERC010` codes mapped to their `PartId`/`NetId`; the migrated resistor raises `ERC009` and not `ERC003`; `ERC008` is an Error for the first two op-amps and a Warning for the third; no `ERC007` is ever emitted; `Error` blocks run, `Warning` permits run. [S1.3] |
| `STC-erc-02` | Instrument cases, once instrument parts exist: an `XSC1` with channel B unwired, and a run whose probe names a net that is not on the sheet | ERC | `ERC005` on the unwired port and on the missing net, mapped to the instrument `PartId`; Run blocked. [S3.2] |
| `STC-lin-01` | Resistive divider + Wheatstone bridge (`10 V`, `1k`/`2k`/`3k`/`4k`) | `OP`, `DC` | Matches closed-form nodal voltages & currents to `SK4` (`1e-7`) on both engines. [S1.5, S1.6, S2.1] |
| `STC-lin-02` | Dependent-source active network (VCVS `E` + VCCS `G` + resistors) | `OP` | Matches analytical gain and input/output currents to `SK4` on both engines. [S2.1] |
| `STC-nl-01` | Half-wave `1N4148` diode + `1k` load (`DC` sweep `-2 V..+5 V`) | `OP`, `DC` | Knee voltage (~0.65–0.72 V) and current curve match `ngspice` within `SK5`. [S2.1] |
| `STC-nl-02` | Common-emitter `2N2222` NPN amplifier biased from `12 V` | `OP`, `DC` | `V_B`, `V_C`, `V_E`, `I_C` match `ngspice` within `SK5`. [S2.1] |
| `STC-nl-03` | `2N7000` NMOS common-source inverter (`V_GS` sweep `0..5 V`) | `OP`, `DC` | Threshold transition and saturation current match `ngspice` within `SK5`. [S2.1] |
| `STC-nl-04` | Standalone inverting op-amp fixture (`R1 = 10 kΩ`, `R2 = 22 kΩ`, `V_in = 1 V`, `±12 V` rails) | `OP`, `DC` | `V_out ≈ -2.200 V` in linear region, saturates cleanly near `±12 V` rails; both engines agree within `SK5`. [S2.1] |
| `STC-ac-01` | First-order RC low-pass (`10 kΩ`, `10 nF`) & RLC bandpass | `AC` (`dec 50 10 1Meg`) | `-3.01 dB` and `-45°` at `f_c = 1/(2πRC) ≈ 1591.5 Hz`; both engines agree within `0.05 dB` / `0.5°`. [S2.1] |
| `STC-ac-02` | Inverting op-amp closed-loop frequency response (`R1 = 10 kΩ`, `R2 = 22 kΩ`, `TL072` / behavioral GBW) | `AC` | Flat passband `+6.85 dB`, `-20 dB/dec` roll-off at `GBW / (1 + R2/R1)`; both engines agree within `SK5`. [S2.1] |
| `STC-tran-01` | RC step response (`PULSE 0 5 1m 1u 1u 10m 20m`, `10 kΩ`, `100 nF`) | `TRAN` (`0..15 ms`) | Charging step `v(t) = 5(1 - e^{-(t-1ms)/1ms})` over `1..11 ms` and matching exponential decay after `11 ms` within `SK5` on both engines. [S2.1] |
| `STC-tran-02` | Full-wave diode bridge rectifier with `100 uF` filter cap + `1 kΩ` load (`60 Hz` sine) | `TRAN` (`0..50 ms`) | Peak voltage and ripple match `ngspice` within `SK5`. [S2.1] |
| `STC-wien-01` | **Headline mockup circuit (a separately modelled fixture built from the unchanged v2 golden `tests/golden/wien-oscillator.ron`, `XD1`):** Wien bridge oscillator (`R1`/`R2 = 10 kΩ`, `C1`/`C2 = 22 nF`, `R4 = 22 kΩ`, pot `R3` with `wiper = 0.5`, `D1`/`D2` bound to the signal-diode definition (`1N4148` class), `U1` bound to the dual-JFET op-amp macromodel definition (`TL072` class), `±12 V` rails; headless probe specifications on the output and `N+` nets in S2.2, the `XSC1` instrument in S3.6) | `TRAN` (`0..40 ms`) | Self-starts cleanly (via startup transient or small `.ic`), stabilises near `f_0 = 1/(2π · 10k · 22n) ≈ 723.4 Hz`; `RIS-C` and `ngspice` agree within `0.5%` frequency and `1%` Vpp. [S2.2, S3.6] |
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

// Lowering (SD7, XD4). `bindings` comes from `omnia_circuit::Circuit::bindings(&doc)`
// (context C1.9), which refuses a stale circuit; lowering checks the revision again.
pub fn lower(
    doc: &Doc,
    bindings: &omnia_circuit::Bindings<'_>,
    analysis: &Analysis,
    probes: &[ProbeSpec],
) -> Result<(CircuitIr, SourceMap), LowerError>;

pub struct SourceMap {
    node_nets: Vec<SmallVec<[NetId; 1]>>,    // NodeId -> nets (many nets per node)
    net_node: BTreeMap<NetId, NodeId>,
    part_devices: BTreeMap<PartId, Vec<DeviceIdx>>, // PartId -> devices (many per part)
    device_part: Vec<PartId>,
}
```

## 11. Dependencies and licensing

Every new crate must pass `cargo deny check` (`MIT`, `Apache-2.0`, `BSD-2-Clause`,
`BSD-3-Clause`, `ISC`, `Zlib`, `Unicode-3.0`, `MPL-2.0` — zero GPL/LGPL).

| Dependency | Crate | Purpose | Licence |
| --- | --- | --- | --- |
| `risc-c` (`git = "https://github.com/Reid-Engineered/RIS-C"`, pinned `--rev 24710eaf…`) | `omnia-sim-risc` | First-party pure-Rust SPICE engine (`OP`, `DC`, `AC`, `TRAN`, streaming + cancel) | BSD-3-Clause (transitive deps `faer`, `peroxide`, `num-complex`, `thiserror` verified by `cargo deny` in S1.5). Admitted through `deny.toml` `allow-git` at a reviewed SHA (`SQ4`); CI access needs the owner action recorded at `HS1`. |
| `libngspice` (bundled dynamic library, BSD-3-Clause core build) | `omnia-sim-ngspice` | Reference SPICE backend & differential oracle | BSD-3-Clause (verified at `SS1` / `HS2`) |
| `libloading` | `omnia-sim-ngspice` | Runtime dynamic loading of `ngspice.dll` / `libngspice.so` | ISC |
| `omnia-doc`, `omnia-circuit` (workspace) | `omnia-sim` | `Doc` snapshot, embedded definitions and typed parameters (parts `PD8`); read-only pin-to-net bindings and ground evidence (`XD4`) | This repository |
| `smallvec` | `omnia-sim` | `SourceMap` node-to-nets lists | Already in `Cargo.lock` (canvas plan section 10) |

No C/C++ code is compiled into `omnia-doc`, `omnia-sim`, `omnia-sim-risc`, `omnia-canvas`, or
`omnia-app` directly. `omnia-sim` takes no `omnia-parts` dependency (`XD3`, section 5). Bundled
SPICE model files are not crates: the built-in models S1.1 contributes follow the parts plan's
model licence policy (`PK11`) and are listed in `THIRD_PARTY.md`.

## 12. Failure modes and mitigations

| Risk | Mitigation | Where checked |
| --- | --- | --- |
| `libngspice` C crash or stuck solver hangs Omnia | Worker-process isolation (`SD5`) + `SK11` kill-on-cancel | `SS1`, `S1.6`, `STC-err-01` |
| GPL/LGPL accidentally linked into `ngspice` build | Explicit BSD-3-Clause configure flags + linked-library audit script | `SS1`, `S4.2`, `HS2` |
| Private `RIS-C` Git dependency blocks public CI or external contributors | `SQ4` decided at `HS1`: `allow-git` entry pinned to a reviewed SHA; owner makes `RIS-C` public or provides a read-only CI token before S1.5 merges (open owner action) | `S0.2`, `S1.5` |
| `RIS-C` and `ngspice` diverge on a lowered schematic or macromodel | Differential test suite runs on every milestone PR with tightened `SK2` tolerances (`1e-6`) against `SK4`/`SK5` gates | `S2.1`, `S2.2`, `S2.3` |
| Wien bridge oscillator fails to self-start in noiseless SPICE | Built-in startup transient / `.ic` support and verified gain-stabilised fixture (`723.4 Hz`) | `S2.2`, `STC-wien-01` |
| Live oscilloscope breaks `B3` / `B10` / `UB3` on Orion | Cached `SheetView`, lock-free `SampleRing`, pre-allocated trace buffers, `30 Hz` throttle | `SS2`, `S3.3`, `S4.4` |
| `10M` resistor read as `10 mΩ` by an engine | One user-value grammar, `M` = mega (`SK1`, parts `PK4`, `XD2`); the emitter writes plain numbers (`SINV6`); `ERC007` retired | parts `P1.1`, `S1.2`, `STC-erc-01` |
| Parts (#85) or context (#100) plan changes before merge, or its tasks slip | S1.1 waits for parts `P2.3`, S1.2 for context `C1.9`; any change to a cited id is re-checked against `XD1`–`XD9` before those tasks start | `S1.1`, `S1.2` |
| Existing sheets are all unmodelled after v2 migration, so they cannot run | `ERC009` names each unmodelled component with a connected pin; the shared placement helper embeds catalog definitions so new placements are modelled; the modelled Wien fixture (`S2.2`) is built from library definitions | `S1.1`, `S1.3`, `S2.2` |

## 13. Changes requested in other plans and crates

Applied in task S0.1 (where noted), S0.2 (plan alignments at `HS1`), and S1.1 / S1.5 / S3.x /
S4.1 (code). Requests to the parts and context plans are logged here and in the S0.2 log entry;
this plan does not edit those files.

| Target | Change |
| --- | --- |
| `AGENTS.md` | Add `sim-plan.md` and `sim-roadmap.md` to Sources of truth and register `S`-prefixed ids (`SB`, `SK`, `SD`, `SINV`, `STC`, `SS`, `SQ`, `HS`, `S0.1`…; note `Milestone S0`–`S4` vs canvas spikes `S1`–`S8`). (Applied in S0.1.) |
| `ROADMAP.md` | Link `sim-plan.md` and `sim-roadmap.md` in intro and Stage 3; update open product decisions with 2026-11-23 owner answers. (Applied in S0.1.) |
| `deny.toml` & `.github/workflows/ci.yml` (`S1.5`) | Add `https://github.com/Reid-Engineered/RIS-C` to `sources.allow-git` pinned to a reviewed SHA (`SQ4`, decided at `HS1`) and wire CI fetch access with the read-only token if `RIS-C` is still private. |
| `.github/workflows/ci.yml` & `scripts/build-ngspice.*` (`S1.6`) | Wire the cached BSD-3-Clause `libngspice` CI artifact (`SQ9`) so `S1.6`, `S2.1`, and `S2.2` run real `ngspice` differential tests in CI before `S4.2` finalises app packaging. |
| `crates/omnia-doc`, `omnia-agent`, `ui-plan.md`, `mcp-plan.md` | No simulation change (`XD1`). The withdrawn props / `SetPartProp` / RON v3 row is replaced by parts tasks `P2.1` (definition table, typed `params` on `StoredPart`), `P2.2` (`SetParam`, `UpdatePartDef`, `Change::modified`) and `P2.3` (RON v3, v2 migration, unsupported-version fixtures in `io.rs`, `omnia-agent/src/files.rs`, `ui-plan.md` `UTC-open-bad`, `mcp-plan.md` `AQ12`). |
| `crates/omnia-symbols` (`S1.1`, `S3.2`) | After parts `P1.4` (`XD5`): add the symbol class (`Component`, `Ground`, `PowerRail`, `Instrument`, `Probe`), distinct from parts `PD5` pin roles, and compile the remaining v1 analog symbols (S1.1); then on-sheet `XSC1`/`XMM1`/`XBP1` artwork (S3.2). Every existing `SymbolId` unchanged. |
| `library/` and `THIRD_PARTY.md` (`S1.1`, `S3.2`) | Contribute the `SD3` definitions and models the parts starter set lacks (inductor, current and dependent sources, BJT, MOSFET, op-amp macromodels, `rail` definitions for the rail symbols) with their symbols in S1.1, and the instrument definitions in S3.2, under generic ids and `PK11` licences (`XD3`). |
| `ui-roadmap.md` `U5.5` / `canvas-roadmap.md` `T7.3` | `S3.3`–`S3.5` supply the real oscilloscope, multimeter, Bode plotter, and Results panel contents inside `U4.1`'s floating frames and `U1.7`'s bottom dock. |
| `mcp-plan.md` (`S4.1`) | Update §1 out-of-scope simulation bullet (lines 51–54), register `sim.erc`/`sim.run`/`sim.results`/`sim.measure` (tier 0 + bounded worker spawn under `AD7`), and add the `sim.*`-scoped finite-number amendment to `AD8`/`AD12` (`XD8`). |
| Parts plan (#85, `66f9e49`), request | (1) **Supply contract** (`SD3`): add `supply_terminals` and `implicit_supply` (one finite voltage per supply terminal, or none) to the `Model` and `Subckt` variants of `EmbeddedSimBinding`, with `PD10` checks that each supply terminal maps to a `power`-role pin and each voltage is finite; the `opamp-generic` starter part declares `+15 V`/`-15 V`. (2) **Placement default** (`XD3`): `Catalog::place_command` (`P2.4`) or a symbol-keyed helper beside it embeds the catalog definition whenever one exists for the symbol being placed, and falls back to `PlaceDef::Unmodelled` only when none exists. Items requested earlier at `HS1` (`PK4` units and whitespace, the `rail` primitive and `dc`, the `XD7` order) are answered at `66f9e49`. |
| `ui-roadmap.md`, request | The U4.2 part picker and U5.2 placement place parts through the shared placement helper above, not through `PlaceDef::Unmodelled`, so placed parts carry their catalog definitions (`XD3`). |
| Context plan (#100), request | Its head at `6a50b28` records `C1.9` (`Circuit::bindings`), declared ground evidence `CK4` matching `SQ3`, case folding and blank designators and the crosswalk in section 9.4, and `CQ9` answered by `XD4`. Add `ERC009` and `ERC010` to the section 9.4 crosswalk as simulation-only codes with no context finding, and note that `ERC001` uses `ground.missing`'s non-`Ground`-part condition. |

## 14. Open questions (`SQ`)

Every question has a default; build to the default until the owner's dated answer replaces it
(Hard Rule 5). All ten were answered at `HS1` on 2026-11-23 by the coordinating agent under the
owner's delegation of that date.

| ID | Question | Default | Answer |
| --- | --- | --- | --- |
| `SQ1` | Should `omnia-sim-ngspice` always use the isolated `omnia-ngspice-worker` helper process (`SD5`), or allow in-process `libngspice`? | Always use `omnia-ngspice-worker` unless `SS1` shows worker launch exceeds `SB3` (`150 ms`) on Orion. | **Default kept** (HS1, 2026-11-23). `SD5` stays HYPOTHESIS until `SS1`. |
| `SQ2` | How should bare `1M` on a resistor value be handled (SPICE `1 mΩ` vs human `1 MΩ`)? | Follow SPICE (`1M = 1e-3`, `1Meg = 1e6`) for netlist compatibility and emit `ERC007` warning whenever bare `M` is used on a resistor. | **Replaced by `XD2`** (HS1, 2026-11-23): user-facing `M` is mega and `m` is milli (parts `PK4`, `PQ2`); SPICE number syntax applies only inside model decks; the emitter writes plain numbers; `ERC007` retired. |
| `SQ3` | Should a net label `"GND"` without a `Ground` symbol count as node `0`? | No — require an explicit `Ground` symbol (or net label `"0"`), and emit a helpful `ERC001` hint if `"GND"` exists without a `Ground` symbol. | **Default kept** (HS1, 2026-11-23), confirmed by `XD4`. |
| `SQ4` | `Reid-Engineered/RIS-C` is currently a private GitHub repository while `omnia` is public. How should Omnia's CI and contributors fetch `risc-c`? | Add `https://github.com/Reid-Engineered/RIS-C` to `deny.toml` `allow-git` pinned to a reviewed SHA (`24710eaf`), and either make `RIS-C` public or supply a read-only CI token before `S1.5` merges. | **Default kept** (HS1, 2026-11-23). Open owner action: before S1.5 merges, the owner makes `RIS-C` public or provides a read-only CI token (`sim-roadmap.md` human checkpoints). |
| `SQ5` | Default input impedance of on-sheet `XSC1` oscilloscope and `XMM1` voltmeter probes? | `10 MΩ` to ground (realistic bench probe; prevents floating-node singularities when attached to capacitive nodes), overridable to ideal open (`∞`) per instrument. | **Default kept** (HS1, 2026-11-23). |
| `SQ6` | Should `RIS-C` ship enabled in the engine picker in v1 or behind a Preferences toggle? | Visible in the First-boot / Preferences / toolbar engine selector labelled `preview`, with `ngspice` as default (matching `design/build-omnia.mjs:536-537`). | **Default kept** (HS1, 2026-11-23). |
| `SQ7` | What happens if the user edits the schematic while a simulation is `Running`? | Stop the live run cleanly, mark displayed results/oscilloscope traces with a "Stale — schematic changed" badge, and require clicking Run again (live edit-while-running deferred to post-v1). | **Default kept** (HS1, 2026-11-23); parameter-only edits count (`SD11`, `XD7`). |
| `SQ8` | Default op-amp model when placing a generic `IC-COM-OpAmp` with neither `props["model"]` nor a known `value` model name? | Built-in behavioral op-amp `.subckt` (`A_OL = 100k`, `GBW = 3 MHz`, `R_in = 1 MΩ`, `R_out = 75 Ω`, diode rail clamp to connected supplies or `±15 V` if supply pins omitted). | **Default behaviour kept** (HS1, 2026-11-23), delivered through the generic op-amp's part definition binding at placement (parts `PD11` `opamp-generic`, `XD3`), not chosen from `Part::value` or a property. Using the implicit `±15 V` raises `ERC008` as a Warning (`SD8`). |
| `SQ9` | In CI (`windows-latest`), how is `libngspice` provided for `omnia-sim-ngspice` and differential tests? | Pre-built BSD-3-Clause `ngspice` artifact cached/fetched in CI (checksum-pinned, verified at `HS2`); `omnia-sim-risc` tests always run with zero external binaries. | **Default kept** (HS1, 2026-11-23). |
| `SQ10` | Should `sim.run` over MCP (`S4.1`) allow selecting `engine: "ngspice" \| "risc"`? | Yes — optional `engine` field defaulting to `"ngspice"`, letting agents cross-check circuits on both engines. | **Default kept** (HS1, 2026-11-23). |
