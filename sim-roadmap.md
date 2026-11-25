# Omnia simulation: roadmap for agents

The task list for Stage 3 (circuit simulation). The reference for every `S`-prefixed id used here
(`SB`, `SK`, `SD`, `SINV`, `STC`, `SS`, `SQ`, `HS`) is [sim-plan.md](sim-plan.md). Ids without an
`S` prefix live in [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md) and [ui-plan.md](ui-plan.md); `P`-prefixed ids live in the parts
plan (`parts-plan.md`, pull request #85) and `C`-prefixed ids in the circuit context plan
(`context-plan.md`, pull request #100). Cross-plan decisions `XD1`–`XD9` are summarised in
`sim-plan.md` section 6. Milestones are written `Milestone S0`–
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
| `sim-tests` | `tests/sim/` (`omnia-sim-tests`), `crates/omnia-bench/benches/sim.rs`, `crates/omnia-bench/src/sim.rs`, new modelled `StressSheet` variants in `crates/omnia-bench/src/generate.rs` (`S2.3`; existing variants never change) | No |
| `assets` | `crates/omnia-symbols/` (`S1.1`, `S3.2`, after parts `P1.4`, `XD5`); `library/` additions (`S1.1` analog and rail definitions, `S3.2` instrument definitions, `XD3`). No simulation task owns an `omnia-doc` file (`XD1`). | No |
| `sim-ui` | `crates/omnia-app/src/sim/`, `crates/omnia-app/src/instruments/`, `crates/omnia-canvas/src/sheet/` probe/instrument hooks (`S3.2`) | Yes |
| `sim-agent` | `crates/omnia-agent/src/sim.rs` (`S4.1`) | No |
| `bench` | `crates/omnia-bench/src/spike/sim_scope.rs` (`S0.4`), `crates/omnia-bench/src/ui.rs` sim scripts (`S4.4`), `docs/measurements/S*.md` | Yes |

Notice that `sim-risc` and `sim-ngspice` own completely disjoint crates and run in parallel!

## Milestones

| Milestone | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| Milestone S0 | `sim-plan.md`, this roadmap, owner sign-off (`S0.2`), spikes `SS1` (`S0.3`) & `SS2` (`S0.4`) | S0.2 (`HS1`), then S0.3 / S0.4 | `HS1`, `SD5`, `SK8` |
| Milestone S1 | v1 analog symbols & symbol class, `omnia-sim` bindings/lowering/ERC/netlist/rings, contributed library definitions, full `OP`/`DC`/`AC`/`TRAN` adapters for both `RIS-C` and `ngspice` | S1.5 & S1.6 | `SINV1`–`SINV7`, `SINV12`, `STC-erc-01`/`STC-erc-03` |
| Milestone S2 | Full dual-engine differential parity (`lin`, `nl`, `ac`, `tran`, `err`), modelled Wien bridge oscillator fixture (`723.4 Hz`), and Orion solver bench | S2.3 | `SB1`–`SB4`, `SB7`, `SINV8`, `SINV10`, S2.1 and S2.2 reference circuits, `STC-erc-01`/`STC-erc-03` |
| Milestone S3 | Live ERC, toolbar Run controls, on-sheet probes/rails/instrument blocks, Oscilloscope (`XSC1`), Multimeter (`XMM1`), Bode Plotter (`XBP1`), Results dock | S3.6 | `SINV9`, `HS3` (plus non-reference `SB5`/`SB6`/`SB8` checks; Orion certification in `S4.4`) |
| Milestone S4 | Agent/MCP simulation tools, BSD-3-Clause `ngspice` bundling, First-boot self-test, final Orion gate | S4.4 | `SINV11`, `HS2`, `HS4`, all `SB1`–`SB8` on Orion |

## Where each task fits (cross-roadmap dependencies)

| Simulation task | Waits for (canvas / wiring / UI / MCP) | Can start |
| --- | --- | --- |
| `S0.1`–`S0.3` | nothing (`S0.3` waits on `S0.2`/`HS1`) | now (`S0.3` after `HS1`, signed 2026-11-23) |
| `S0.4` (`SS2`) | standalone GPUI spike in `omnia-bench` (after `S0.2`/`HS1`) | now |
| `S1.1` | parts `P2.3` (document schema, `XD1`, `XD7`), parts `P1.4` (`pins.toml` order, `XD5`) and parts `P2.4` (shared placement helper, `XD3`) | after `P2.4` & `P1.4` |
| `S1.2` | `S1.1` (including the parts `PD5` supply contract); context `C1.9` (`Circuit::bindings`, `XD4`), which itself waits for context `C1.2` and S1.1 | after `S1.1` & `C1.9` |
| `S1.3`–`S2.2` (rest of the dual-engine simulation stack) | nothing beyond `S1.2`'s inputs (zero UI dependency) | after `S1.2` |
| `S2.3` | `S2.2`; context `C1.3` (incremental update, for `SB1`/`SB2` incremental evidence) | after `S2.2` & `C1.3` |
| `S3.1` | `U1.6`, `U1.7`, `U1.8`, `S2.3` | after `U1.8` & `S2.3` |
| `S3.2` | `T4.3`, `T5.3`, `S1.1`, `S1.3` (`ERC005` joins `erc.rs`) | after `T5.3` & `S1.3` |
| `S3.3` | `S0.4` (`SK8`), `S3.1`, `U4.1` | after `S0.4`, `U4.1`, & `S3.1` |
| `S3.4` | `S3.1`, `U4.1` | after `U4.1` & `S3.1` |
| `S3.5` | `S3.1`, `U1.7` | after `U1.7` & `S3.1` |
| `S4.1` | `A3.4`, `S2.3` | after `A3.4` & `S2.3` |
| `S4.3` | `U3.3`, `U3.5`, `S4.2` | after `U3.5` & `S4.2` |

## Dependency overview

```
S0.1 -> S0.2 [HS1] -> S0.3 (SS1), S0.4 (SS2)
HS1 + parts P2.3 + parts P1.4 + parts P2.4 -> S1.1 (symbols, symbol class, library definitions)
S1.1 -> context C1.9 (bindings, XD4); S1.1 + C1.9 -> S1.2 (lowering/netlist) -> S1.3 (ERC), S1.4 (engine+ring)
S1.4        -> S1.5 (omnia-sim-risc: OP/DC/AC/TRAN)    --+
S0.3 + S1.4 -> S1.6 (omnia-sim-ngspice: OP/DC/AC/TRAN) --+--> (+ S1.3) -> S2.1 (diff parity) -> S2.2 (Wien) -> (+ context C1.3) S2.3 (Gate S2)
S2.3 + U1.6 + U1.7 + U1.8 -> S3.1 (SimController)
S3.1 + U4.1 + S0.4 -> S3.3 (XSC1)    --+
S3.1 + U4.1        -> S3.4 (XMM1+XBP1)-+--> (+ S3.2) -> S3.6 [HS3]
S3.1 + U1.7        -> S3.5 (Results) --+
S1.1 + S1.3 + T4.3 + T5.3 -> S3.2 (sheet probes/rails/instrument blocks, STC-erc-02)
S2.3 + A3.4 -> S4.1 (agent sim tools)
S0.3 + S2.3 -> S4.2 (ngspice bundling) [HS2] -> S4.3 (+ U3.3 + U3.5: First boot + prefs)
S3.6 + S4.1 + S4.3 -> S4.4 (Final Orion Gate) [HS4]
```

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| `HS1` | S0.2 | Reviews `sim-plan.md`, answers `SQ1`–`SQ10` (including `SQ4` on `RIS-C` repo visibility/`deny.toml`), and approves `SD6`–`SD14`. Milestone S1 starts after `HS1`. **Signed 2026-11-23** by the coordinating agent under the owner's delegation of that date (answers in `sim-plan.md` section 14). |
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

### S0.2 Owner sign-off and plan alignment · done (HS1 signed 2026-11-23, owner-delegated)
- Lane: docs · Depends: S0.1 · Read: `sim-plan.md` §13–§14; cross-plan decisions `XD1`–`XD9`; `parts-plan.md` / `parts-roadmap.md` (#85, `1d162f9`); `context-plan.md` / `context-roadmap.md` (#100, `eee05c4`)
- Owns: `sim-plan.md`, `sim-roadmap.md`
- Do: present `SQ1`–`SQ10` and `SD6`–`SD14` to the owner at `HS1`; record dated answers in `sim-plan.md` and turn all `PROPOSED` decisions into `DECIDED` (only `SD5` stays `HYPOTHESIS` until `SS1`); apply every cross-plan decision `XD1`–`XD9` that touches the simulation plan and roadmap.
- Done when: `HS1` is signed off by the owner and no `PROPOSED` entries remain in `sim-plan.md`.

### S0.3 Spike SS1: `ngspice` worker isolation & BSD-3-Clause build · todo
- Lane: sim-ngspice · Depends: S0.2 · Read: `sim-plan.md` SD5, SD12, SB3, SB4, SINV11, SQ1
- Owns: `docs/measurements/S0.3.md`, spike harness in `docs/measurements/S0.3/`
- Do: build BSD-3-Clause `libngspice` (no GPL/LGPL libraries bundled or statically linked; system C/compiler runtimes excepted) on Windows and Orion Linux; benchmark worker-process IPC vs in-process `libngspice` for startup latency (`SB3`), binary double streaming rate, and kill-on-cancel reliability (`SB4`); audit linked shared libs and record exact configure flags.
- Done when: `docs/measurements/S0.3.md` records numbers and binary library audit on both Windows and Orion and flips `SD5` from `HYPOTHESIS` to `DECIDED`; if Orion measurements have not run yet, record Windows diagnostics and set `needs-human`.

### S0.4 Spike SS2: live oscilloscope ring-buffer overhead · needs-human
- Lane: bench · Depends: S0.2 · Read: `sim-plan.md` SB5, SB6, SK6, SK8, SINV9; `canvas-plan.md` D7, D8, G6
- Owns: `crates/omnia-bench/src/spike/sim_scope.rs`, `docs/measurements/S0.4.md`
- Do: prototype a 2-channel SPSC `SampleRing` fed at 100 kS/s by a background thread and drained by a cached floating scope view over a `1k` sheet at `30 Hz` and `60 Hz`; measure frame times, sheet paint count, and warm Omnia allocations on Orion.
- Done when: `docs/measurements/S0.4.md` records Orion numbers at `30 Hz` and `60 Hz` and locks `SK8`; if Orion measurements have not run yet, record Windows diagnostics and set `needs-human`.

---

## Milestone S1: core IR, ERC, netlist, models, and dual `SimEngine` backends

### S1.1 v1 analog symbol set and symbol class · todo
- Lane: assets · Depends: S0.2, parts P2.3 (`XD1`, `XD7`), parts P1.4 (`pins.toml` order, `XD5`), parts P2.4 (shared placement helper, `XD3`) · Read: `sim-plan.md` SD3, SD10, XD3, XD5; `parts-plan.md` PK6, PK11, PK12, PD1, PD5, PD7, PD11; `crates/omnia-symbols/{pins.toml,src/lib.rs}`, `xtask/src/symbols.rs`
- Owns: `crates/omnia-symbols/` (`pins.toml`, regenerated `src/generated.rs`, symbol class in `src/lib.rs`), `xtask/src/symbols.rs` (only if needed to keep every existing `SymbolId` unchanged); new `library/parts/**` and `library/models/**` files for the extra `SD3` definitions (rail definitions included), the regenerated `crates/omnia-parts/src/generated.rs`, and their `THIRD_PARTY.md` rows. No `omnia-doc` file and no document schema work (`XD1`).
- Do: add the symbol class (`SymbolClass`: `Component`, `Ground`, `PowerRail`, `Instrument`, `Probe`), a per-symbol type kept separate from parts `PD5` per-pin roles (`XD5`); compile the v1 analog symbols that parts `P1.4` did not add from `vendor/electronic-symbols/` (inductor, remaining independent DC/AC/pulse sources, VCVS/VCCS dependent sources, NPN & PNP BJT, NMOS & PMOS MOSFET, supply rail tags), appending them so every existing `SymbolId` stays unchanged; contribute their part definitions to the shared `library/` under generic ids and `PK11` licences (`XD3`, no second deck tree): BJT and MOSFET `.model`s, inductor and source primitives, op-amp macromodels built only from `R`/`C`/`E`/`G`/`D` and using parts `PD5`'s `SupplyDecl` (`SD3`), and `rail` definitions (`dc` parameter, no `primary_param`, class `PowerRail`, `SD10`).
- Done when: every existing `SymbolId` is unchanged and all 12 goldens match with 0 differing pixels; every compiled symbol has a class; placing a generic op-amp with `Catalog::place_symbol(SymbolId, …)` (parts `P2.4`) embeds its `[symbol] default = true` definition with the `SQ8` binding, and reading its `SupplyDecl` verifies the ordered `+15 V`/`−15 V` fallback (`XD3`); `INV12` and `cargo xtask parts --check` pass.

### S1.2 `omnia-sim` core: bindings, `CircuitIr`, netlist emitter · todo
- Lane: sim-core · Depends: S1.1, context C1.9 (`Circuit::bindings`: state-matched pin-to-net bindings and declared ground evidence, `XD4`) · Read: `context-plan.md` CK1, CK4, CD18, CINV14, CTC-bind-01, section 12 (`Bindings`); `sim-plan.md` SK1, SK2, SK3, SK10, SK12, SD3, SD7, SD10, SQ8, SINV1, SINV2, SINV6, SINV12, XD1–XD5; `parts-plan.md` PK4, PK6, PK12, PD3, PD5, PD6
- Owns: `crates/omnia-sim/Cargo.toml`, `crates/omnia-sim/src/{lib.rs,binding.rs,ir.rs,netlist.rs}` (lowering only; library definitions are S1.1's); declares `Analysis` and `ProbeSpec`, which `lower` takes
- Do: create GPUI-free `omnia-sim` (no `omnia-parts` runtime dependency); read embedded definitions and typed parameters through `Doc::part_def` / `Doc::resolved_param` (no value parser: values are `PK4` `FiniteF64`s) and choose the model only from the embedded binding (`Primitive`, `Model`, `Subckt`, `model` enum), never from `Part::value` (`XD3`); implement insertion-order-independent `CircuitIr::lower(doc, bindings, analysis, probes)` (`SD7`) that checks and retains the `(lineage, revision)` pair, collapses ground nets, joins rails by rail name (`SD10`), expands parts (potentiometer split, VCVS/VCCS) in embedded terminal order, reads `SupplyDecl` to apply the `SD3` supply contract, and builds the many-to-many `SourceMap`; emit deterministic SPICE netlists for `OP`/`DC`/`AC`/`TRAN` with plain numbers only.
- Done when: property tests prove `SINV2` (any arena insertion order gives identical numbering & netlist text), `SINV6` (emitter writes plain numbers that round-trip bit for bit; `10M` reaches the netlist as `1e7`), `SINV12` (complete many-to-many source map), and `SINV1` (no GPUI in dep tree); lowering records the verified pair and refuses a mismatch in either lineage or revision (`CD18`). Test a stale revision in one lineage and two forks with different edits at the same revision, passing the first fork's valid bindings to `lower` with the second fork's document and asserting stale refusal. Compile-time checks establish that solver `NodeId` has no conversion in either direction with context `CircuitKey` or MCP `NetKey` (`CK1`, `XD4`). Unit tests cover both supplies connected, implicit fallback, one supply connected, and neither connected without a fallback.

### S1.3 Deterministic ERC engine · todo
- Lane: sim-core · Depends: S1.2 · Read: `sim-plan.md` SD3 (supply contract), SD8, SD10, SB1, SK9, SK10, SQ3, SQ8, SINV2, SINV3, STC-erc-01, STC-erc-03, XD6; `context-plan.md` CD18; `context-plan.md` CK4 and section 9.4 (case folding, blank designators, ERC crosswalk)
- Owns: `crates/omnia-sim/src/erc.rs`
- Do: implement `run_erc(doc, bindings) -> Result<Vec<ErcDiagnostic>, StaleBindings>` reading `Circuit::bindings` (`C1.9`), first checking `(bindings.lineage(), bindings.revision()) == (doc.lineage(), doc.revision())` (`CD18`) and refusing a mismatch, and `validate_probes(doc, bindings, probes) -> Result<Vec<ErcDiagnostic>, StaleBindings>` with the same check, which reports headless `ERC005` for a `ProbeSpec` naming a missing net by probe name and net (unwired instrument ports join `run_erc` in S3.2). `run_erc` covers the codes of `SD8`: `ERC001` ground with the non-`Ground`-part condition and the `GND` hint, `ERC002` duplicate designators per context section 9.4 (ASCII case folding; blank designators never duplicate), `ERC003` the shared parse-failure case, `ERC004` shorted voltage sources, `ERC006` floating pins with `no_connect` exempt, `ERC008` using `SupplyDecl` and pin roles from the embedded definition and net membership from `C1.9`, `ERC009` no simulation model, `ERC010` conflicting rail voltage; use the shared predicates of the `XD6` crosswalk; never emit the retired `ERC007`; sort deterministically by severity, code, geometry, and item id. The Error/Warning policy is the Run gate.
- Done when: `STC-erc-01` passes for `ERC001`–`ERC004`, `ERC006`, `ERC008` (both severities and the partial-supply Error), `ERC009` and `ERC010`, and `STC-erc-03` (headless-probe `ERC005`); an empty sheet raises nothing; a same-revision divergent-fork `bindings` is refused. Assert that the v3 fixture with `resistance = 0` is refused at load (`PD8`, `PD10`); test any defensive typed `ERC003` check with an internal fixture. `SINV2` determinism and `SINV3` immutability property tests pass.

### S1.4 `SimEngine` trait, lock-free `SampleRing`, and `SimResult` · todo
- Lane: sim-core · Depends: S1.2 · Read: `sim-plan.md` §10, SK6, SK7, SD11, SD13, SINV3, SINV4
- Owns: `crates/omnia-sim/src/{engine.rs,ring.rs,results.rs,measure.rs,error.rs}`
- Do: define `SimEngine`, `SimRunHandle`, `SimState`, typed `SimError` (using S1.2's `Analysis` and `ProbeSpec`); implement bounded lock-free SPSC `SampleRing` (`SK6`, zero post-init allocs, drop-counter on overrun); implement `SimResult` storage caps (`SK7`) and waveform measurement routines (`vpp`, `vrms`, `vavg`, `freq`, `duty`, `rise_time`, `bandwidth_3db`, `phase_margin`).
- Done when: `SINV4` allocation-counting & concurrent stress tests pass; measurement unit tests verify analytical sine/square/RC signals.

### S1.5 `omnia-sim-risc`: `SimEngine` adapter over pinned `risc-c` (`OP`/`DC`/`AC`/`TRAN`) · todo
- Lane: sim-risc · Depends: S1.4 · Read: `sim-plan.md` SD1, SD2, SD6, SD13, SQ4, SINV1, SINV5, SINV7, SINV11; `Reid-Engineered/RIS-C` `docs/AGENT_INTEGRATION.md`
- Owns: `Cargo.toml` (pinned `risc-c` git dep), `deny.toml` (`allow-git` for `RIS-C`), `crates/omnia-sim-risc/`
- Do: add `https://github.com/Reid-Engineered/RIS-C` to `deny.toml` `allow-git` pinned to a reviewed SHA (`SQ4`, decided at `HS1`; `RIS-C` is public, so CI needs no credentials), and create GPUI-free `omnia-sim-risc` wrapping `risc-c` (pinned commit SHA) behind `RiscEngine: SimEngine`; wire `OP`, single-source `DC`, `AC`, and live-streaming `TRAN` (`transient_with` pushing into `SampleRingProducer`, pause gate, `ControlFlow::Break(())` cancellation within `SK11`); map `risc_c::RunError` through `SourceMap` to typed `SimError`; enforce `SINV7` (missing signal name returns error, never `0.0`).
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
- Lane: sim-tests · Depends: S2.1 · Read: `sim-plan.md` STC-wien-01, SB7, SK5, SINV10, XD1, XD3; `parts-plan.md` PD11, P2.4; `tests/golden/wien-oscillator.ron`
- Owns: `tests/sim/tests/wien.rs`, `tests/sim/fixtures/wien-oscillator-sim.ron`
- Do: build a separately modelled Wien bridge oscillator fixture from the topology of the unmodified v2 golden `tests/golden/wien-oscillator.ron` (`XD1`): a RON v3 sheet whose parts carry embedded library definitions and typed parameters (`R1`/`R2` `resistance = 10k`, `C1`/`C2` `capacitance = 22n`, `R4` `resistance = 22k`, pot `R3` `resistance = 20k` and `wiper = 0.5`, `D1`/`D2` bound to the signal-diode definition, plus `±12 V` `rail` parts). Build through public commands and select `U1`'s dual-JFET op-amp macromodel by source id with `Catalog::place_command(source_id, …)`. Observe the output and `N+` nets through named headless `ProbeSpec`s. Run `TRAN` on both `ngspice` and `RIS-C` and verify clean oscillation startup, steady-state frequency (`≈ 723.4 Hz`, `<= 0.5%` diff), and amplitude (`<= 1%` diff).
- Done when: `STC-wien-01` passes deterministically on both engines in CI; `SINV10` holds (the v2 golden's bytes on disk are unchanged and it still loads).

### S2.3 Gate: Milestone S2 (dual-engine parity & Orion solver bench) · todo
- Lane: sim-tests · Depends: S2.2, context C1.3 (incremental update, for `SB1`/`SB2` incremental evidence) · Read: `sim-plan.md` SB1–SB4, SB7, SINV1–SINV8; `parts-plan.md` PD11 (stress sheets stay fixed); `context-plan.md` C1.7 (bindings read timings)
- Owns: `crates/omnia-bench/benches/sim.rs`, `crates/omnia-bench/src/sim.rs`, `crates/omnia-bench/src/main.rs` (`bench sim` dispatch), new `StressSheet` variants `Modelled1k` and `Modelled10k` in `crates/omnia-bench/src/generate.rs`, the corresponding match arms in `crates/omnia-bench/src/scene.rs` (`Sheet::name`), `docs/measurements/S2.3.md`
- Do: add the named modelled stress sheets `Modelled1k` and `Modelled10k` (every part bound to a built-in library definition through public APIs, so `SB2` measures device lowering; every existing `StressSheet` variant stays item-for-item identical for every seed, parts `PD11`); add criterion ERC/lowering bench (`SB1`, `SB2` on the modelled `1k`/`10k`, each in cold, incremental (after one edit, through context `C1.3`) and cached cases, `XD9`) and `bench sim` subcommand (`--circuit wien --engine ngspice|risc` and `--script cancel` for `SB3`, `SB4`; `SB3`'s span includes snapshot and `omnia-circuit` bindings acquisition, `XD9`); run the S2.1 and S2.2 reference circuits and the headless `STC-erc-01`/`STC-erc-03` checks, and record Orion numbers.
- Done when: the S2.1 and S2.2 reference circuit tests pass on both engines and the headless `STC-erc-01` and `STC-erc-03` checks pass. S3.2 owns acceptance of `STC-erc-02`. `SB1`–`SB4` pass on Orion (otherwise record non-reference numbers and set `needs-human`).

---

## Milestone S3: live ERC, Run controls, and virtual instruments in `omnia-app`

### S3.1 `SimController`, toolbar Run controls, and live status-bar ERC · todo
- Lane: sim-ui · Depends: S2.3, U1.6, U1.7, U1.8 · Read: `sim-plan.md` SD1, SD2, SD8, SD11, SK9, SQ7; `ui-plan.md` UK3, UK6
- Owns: `crates/omnia-app/src/sim/{mod.rs,controller.rs,erc_bar.rs}`
- Do: wire toolbar analysis picker (`Transient`, `AC Sweep`, `DC Sweep`, `DC OP`), Run / Pause / Stop buttons, engine switcher (`ngspice` / `RIS-C preview`), live sim-time pill, and debounced (`150 ms`) background ERC indicator in the status bar with click-to-jump diagnostics; handle schematic edits while running per `SQ7`.
- Done when: simulated-input tests cover Run → Pause → Resume → Stop, ERC error blocking Run, and stale invalidation (`SD11`, `XD7`), including replacing the live document with another at the same revision, a `SetParam`-only edit, and an `UpdatePartDef` with unchanged topology and text; `UB3` holds (0 `SheetView` paints on toolbar/sim-time updates).

### S3.2 On-sheet probes, supply rails, and instrument blocks (`XSC1`, `XMM1`, `XBP1`) · todo
- Lane: sim-ui, assets · Depends: S1.1, S1.3, T4.3, T5.3 · Read: `sim-plan.md` SD8 (`ERC005`), SD10, SQ5, STC-erc-02, XD5; `canvas-plan.md` Q2; `design/build-omnia.mjs:188-235`
- Owns: `crates/omnia-symbols/` (instrument art; the third `pins.toml` editor after parts `P1.4` and S1.1, `XD5`), instrument part definitions under `library/` (`XD3`), regenerated `crates/omnia-parts/src/generated.rs`, `crates/omnia-sim/src/erc.rs` (`ERC005` only, after S1.3), `crates/omnia-canvas/src/sheet/instruments.rs`, `tests/golden/sim/` (rail & instrument sheet goldens; leaves T3.3's `tests/golden/wien-oscillator-*.png` unchanged)
- Do: render `+12V`/`-12V` supply rail pills (`stage/tag-bg`, `stage/tag-text`), on-sheet `XSC1`/`XMM1`/`XBP1` instrument blocks (`stage/instrument-body`), and channel A/B probe wires/dots (`probe/a`, `probe/b`); add the instrument part definitions (input impedance per `SQ5`) and regenerate the parts catalog. In `erc.rs`, add `ERC005` for unwired instrument ports to document ERC, attributed to the instrument `PartId` and port (S1.3 already validates headless `ProbeSpec`s). Wire instruments strip placement into `Tool::Place`.
- Done when: new goldens in `tests/golden/sim/` render the Wien oscillator with supply rails and `XSC1` matching `workspace-graphite.png` while T3.3's existing goldens remain unchanged; `STC-erc-02` blocks Run with the specified `ERC005` attribution; `cargo xtask parts --check`, `INV6` (no colour literals) and `B10` pass.

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
- Do: update `mcp-plan.md` (§1 simulation bullet, `AD7` worker policy, `AD8`/`AD12` `sim.*`-scoped finite canonical JSON numbers, `XD8`); add `sim.erc` (tier 0, document ERC), `sim.run` (tier 0 + bounded worker process execution as an `A3` cancellable job with progress updates and `engine: ngspice|risc`), `sim.results` (tier 0), and `sim.measure` (tier 0) to `omnia-agent` catalog and schemas (`AINV11`), exposed automatically over CLI and MCP stdio. `sim.run` uses Run preflight for both document ERC and request probe validation (`SD8`, `SD14`). Circuit views stay with `omnia_context`.
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
- Do: add `bench ui --sheet 1k --script sim-live` and `bench idle --shell --sim-idle`; freeze the Milestone S4 Orion campaign script running `SB1`–`SB8` (with `SB1`/`SB2` cold, incremental and cached cases and `SB3` including snapshot and bindings acquisition, `XD9`) plus full `B3`/`B4`/`UB3`/`UB4` checks with live Wien oscillator streaming into `XSC1` on both `ngspice` and `RIS-C`; run ten interleaved repetitions at `100%` and `125%` on Orion (`HS4`).
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
                       all independent review findings (F1–F17) from an independent model review and
                       rewrote the roadmap around the existing `RIS-C` crate (23 tasks across
                       Milestones S0–S4 instead of re-implementing a SPICE solver in-repo).
                       Next step: `S0.2` (`HS1`) owner review of `SQ1`–`SQ10` and `SD6`–`SD14`.
                       No code or dependency changes.
2026-11-23 S0.2 done — HS1 signed 2026-11-23 by the coordinating agent under the owner's
                       delegation of that date, against `main` `0e8cb21`, parts plan #85
                       (`4bee8d5`) and context plan #100 (`6a50b28`). Answers: `SQ1`, `SQ3`,
                       `SQ5`, `SQ6`, `SQ7`, `SQ9`, `SQ10` default kept; `SQ2` replaced by `XD2`
                       (user-facing `M` = mega, parts `PK4`/`PQ2`; SPICE syntax only inside
                       decks; `ERC007` retired); `SQ8` default behaviour kept, delivered by the
                       generic op-amp's part definition binding at placement (`XD3`); `SQ4`
                       default kept (allow-git pinned SHA) with an open owner action: before
                       S1.5 merges, make `RIS-C` public or provide a read-only CI token (added
                       to human checkpoints). `SD6`, `SD7`, `SD8`, `SD10`, `SD12`, `SD13`,
                       `SD14` DECIDED and amended per cross-plan decisions XD1–XD9
                       (coordinator, owner-delegated, 2026-11-23); `SD9` rewritten per `XD1`
                       (props, `part_props`, `SetPartProp` and the sim RON bump withdrawn; sim
                       keys are typed parameters, parts `PK12`) and DECIDED; `SD5` stays
                       HYPOTHESIS until `SS1`; `SD11` keeps DECIDED with the `XD7` stale rule.
                       Applied XD1–XD9 to sim-plan §2, §4 (`SK1`, `SK10`), §5, `SD3`, `SD7`
                       (lowering in `omnia-sim` from `Circuit::bindings`; many-to-many
                       source maps), `SK10` ground evidence matching context `CK4`, `SD8`
                       (crosswalk table matching context §9.4, `ERC002` citing its ASCII case
                       folding and blank designators, `ERC008` Error/Warning split),
                       `SD10`, `SD13`, `SD14`, a new XD summary in §6, `SINV2`, `SINV6`,
                       `SINV10`, `SINV12`, `STC-erc-01`, `STC-wien-01`, §10, §11, §12, §13
                       (doc row replaced by parts `P2.1`–`P2.3`) and §14 (Answer column), and
                       budgets per `XD9` (thresholds unchanged). Roadmap: S1.1 now waits for
                       parts `P2.3` and `P1.4` and owns only symbols and the symbol class;
                       S1.2 waits for context `C1.9` (`Circuit::bindings`, `XD4`),
                       consumes embedded definitions and contributes to the shared `library/`;
                       S1.3 implements the crosswalk; S2.2 builds a separately modelled Wien
                       fixture from the unmodified v2 golden. New rules not spelled out by
                       XD1–XD9, for review: `ERC003` covers a connected `Component` part whose
                       binding is `None` (every migrated v2 part); rails and instruments are
                       lowered by symbol class with a typed rail-voltage parameter (proposed
                       `dc`). Requests logged for other plans (sim-plan §13): parts — `PK4` unit
                       tokens and whitespace, a rail-voltage parameter and one-pin rail binding,
                       and the `XD7` `omnia-doc` order now skipping S1.1; context — nothing
                       further (its §15 sim items are applied here). Rule-1
                       scan of both files: no company named; standard component numbers kept
                       only as compatibility notes, with generic library and fixture ids per
                       parts `PD11`. Docs only; nothing measured; no dependency added.
                       Review of #104 (head `7d7e8ff`), coordinator decisions, against parts
                       #85 `66f9e49`: (1) S2.2's Wien fixture uses headless probe
                       specifications; `ERC005` moves to new `STC-erc-02`, scheduled with
                       S3.2 (which now also owns `ERC005` in `erc.rs`). (2) `ERC003` is the
                       shared parse-failure case only; new simulation-only `ERC009` "no
                       simulation model" for a connected component with binding `None`;
                       `ERC001` uses context `ground.missing`'s non-`Ground`-part condition.
                       (3) `SD3` supply contract: bindings declare `supply_terminals` and
                       `implicit_supply`, read by both lowering and ERC; partial supplies are
                       an `ERC008` Error; requested for parts `EmbeddedSimBinding` (§13). (4)
                       Rails use parts' `rail` primitive and `dc`; the rail name is the
                       display text of a rail part (no `primary_param`); new `ERC010`
                       "conflicting rail voltage"; S1.1 owns the rail symbols and all
                       sim-contributed library definitions, S1.2 owns lowering only (read as
                       covering every extra `SD3` definition, not only rails). (5) The shared
                       placement helper (parts `P2.4`) embeds a symbol's catalog definition;
                       S1.1 depends on `P2.4` and verifies the generic op-amp's default
                       binding; requests logged for parts (helper default) and the UI roadmap
                       (U4.2, U5.2 placement through the helper). (6) S2.3 depends on context
                       `C1.3` and adds named `Modelled1k`/`Modelled10k` stress sheets;
                       existing sheets never change. Nit: the S0.1 entry no longer names a
                       commercial review tool (hard rule 1). §13 parts items answered at
                       `66f9e49` are marked answered; context asked to add `ERC009`/`ERC010`
                       to its crosswalk. Docs only.
                       Owner update 2026-11-23: `SQ4` resolved — the owner made
                       `Reid-Engineered/RIS-C` public, so no CI token is needed; the pending
                       owner action is removed from the human checkpoints and from S1.5's gate
                       and ownership (the `allow-git` entry pinned to a reviewed SHA stays).
                       The owner approved this round's blocking fixes.
2026-11-23  S0.2      Re-review of #104 at `6d1a0d6` (blocking fixes approved by the owner).
                       Cross-plan decisions XD1–XD9 (coordinator, owner-delegated, 2026-11-23):
                       align `SD3` and S1.1–S1.3 with parts `PD5` `SupplyDecl` and use `C1.9`
                       net membership for `ERC008`. S1.1 verifies the default binding with
                       `Catalog::place_symbol` (also the picker and agent handoff); S2.2
                       selects its dual-JFET definition by source id with `Catalog::place_command`.
                       `SD7` and S1.2 reject a mismatched lineage or revision, test divergent
                       forks at equal revisions (`CD18`), retain the pair for `SD11` stale checks,
                       and verify `NodeId` cannot convert to or from `CircuitKey` or `NetKey`.
                       `STC-erc-01` checks zero resistance at v3 load. S2.3 gates the reference
                       circuits and headless ERC and owns the `SceneChoice::name` match arms.
                       S3.2 owns `STC-erc-02` and catalog regeneration. Run-request validation
                       reports headless `ERC005` by probe name and requested net. §13 records
                       the answers in parts #85 `136a2dd` and context #100 `674a7d3`; `SQ4` now
                       asks how to fetch the dependency. Documentation only.
2026-11-23 S0.2 review fixes — independent review of 63a2cb3 and 0cb7f3d (coordinator fixes,
                       owner-delegated). Plan heads refreshed to parts `1d162f9` and context
                       `eee05c4`; §13 placement row matches parts §14 (canvas `T5.3` provider hook,
                       UI `U5.2` installs `Catalog::place_symbol`); `UTC-open-bad` and File > Open
                       `check_doc` go to whichever of `P2.3` and `U2.3` merges second; S2.3 owns
                       `Sheet::name` (the earlier entry's `SceneChoice::name` does not exist);
                       headless-probe `ERC005` moves to S1.3 as `STC-erc-03` (`validate_probes`),
                       so S4.1's Run preflight needs no S3.2, which keeps instrument-port `ERC005`
                       (`STC-erc-02`); S1.2 declares `Analysis` and `ProbeSpec`; `run_erc` and
                       `validate_probes` return `Result` and refuse stale `(lineage, revision)`
                       bindings (`CD18`). Documentation only.
2026-11-25 S0.4 SS2 needs-human — Windows diagnostics: median run p99 0.840 ms
                       at 30 Hz and 0.814 ms at 60 Hz, zero sheet paints, warm Omnia
                       allocations, dropped pairs or missed refreshes. Orion pending
                       (SB5, SB6, SK6, SK8, SINV9); SK8 stays at 30 Hz. Standard
                       checks and focused spike tests pass. Evidence and owner command
                       in docs/measurements/S0.4.md.
```
