# Component definitions and simulation models: roadmap for agents

The task list for Omnia's component definitions, simulation model bindings, `.opart`/`.olib`
containers and embedded document definitions. The reference for every `P`-prefixed id used here
(`PB`, `PK`, `PD`, `PINV`, `PTC`, `PS`, `PQ`, `HP`) is [parts-plan.md](parts-plan.md). Unprefixed
ids are in [canvas-plan.md](canvas-plan.md); `W`, `A` and `U` ids are in
[wiring-plan.md](wiring-plan.md), [mcp-plan.md](mcp-plan.md) and [ui-plan.md](ui-plan.md).

This roadmap runs beside the canvas, wiring, agent and UI roadmaps. Milestone P1 touches only
the new `parts` lane and `xtask`; Milestone P2 coordinates with the `doc` lane.

## How to work a task

The same steps as `AGENTS.md`: pick a `todo` task whose dependencies are all `done` (a
`needs-human` dependency is not `done`), branch from current `main` as `task/<id>-<slug>`, mark
the task `doing`, read what **Read** names, do **Do** inside **Owns**, run the standard checks
and **Done when** checks, set the status, and append one log entry at the end of this file.

Statuses: `todo`, `doing`, `done`, `blocked (reason)`, `needs-human`.

## Rules

`AGENTS.md` and canvas roadmap rules 1 to 12 apply, with branch names `task/<id>-<slug>`, for
example `task/P1.1-schema-units`. Five more:

13. **No GPUI in `omnia-parts`; no library dependency in `omnia-doc`.** Check `PINV1` on every
    change to `Cargo.toml`.
14. **Untrusted archives and models are hostile by default.** Enforce `PK3` before allocating
    buffers and `PK10` before admitting SPICE text (`PINV8`, `PINV9`).
15. **Deterministic outputs everywhere.** Never let filesystem walk order, ZIP entry order,
    timestamps or hash-map iteration order affect `Catalog`, `PK5` hashes, `generated.rs` or
    RON v3 output (`PINV2`).
16. **Approved licences only in `library/models/`.** Every bundled model file carries explicit
    licence metadata matching `PQ7` and `THIRD_PARTY.md`; proprietary models are never committed.
17. **Keep the plans true.** Any change to a constant, decision, schema field or public
    signature updates [parts-plan.md](parts-plan.md) in the same pull request, and any listed
    plan in section 14 if affected.

## Standard checks

As in `AGENTS.md`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p omnia-bench --all-targets --features scene-counts,readback -- -D warnings
cargo test --workspace
cargo deny check
```

Plus, once `P1.4` lands:

```sh
cargo xtask parts --check
```

## Lanes

| Lane | Owns | Needs GPUI |
| --- | --- | --- |
| `parts` (new) | `crates/omnia-parts/`, `library/`, `xtask/src/parts.rs` | No |
| `doc` | `crates/omnia-doc/` files named by a P2 task | No |
| `assets` | `crates/omnia-symbols/pins.toml`, `THIRD_PARTY.md` | No |
| `bench` | `crates/omnia-bench/benches/parts.rs`, `docs/measurements/P*.md` | No |

Milestone P2 touches `crates/omnia-doc/src/{doc.rs,command.rs,io.rs}` and must not run while
another task owns those files (check `canvas-roadmap.md`, `wiring-roadmap.md`, `mcp-roadmap.md`
and `ui-roadmap.md` before starting a P2 task).

## Milestones

| # | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| P0 | Plan, roadmap, owner sign-off (`HP0`), SPICE/archive spike (`PS1`) | P0.3 | `HP0`, `PS1` |
| P1 | `omnia-parts` crate, `.opart`/`.olib` loader & packer, starter `library/`, `xtask parts`, static `Catalog::builtin()` | P1.5 | `PB1`–`PB3`, `PINV1`–`PINV3`, `PINV8`–`PINV10` |
| P2 | `Doc` embedded `PartDefTable`, `SetParam` & `UpdatePartDef` commands, RON v3 + v2 compat, library diff | P2.4 | `PB4`, `PB5`, `PINV4`–`PINV7` |
| P3 | Reference-machine gate on Orion (`PB1`–`PB5`, `B1`, `UB10`, `INV2`) | P3.1 | `HP1` |

## Dependency overview

```
P0.1 -> P0.2 [HP0] -> P0.3 (PS1)
P0.3 -> P1.1 -> P1.2 -> P1.3 -> P1.4 -> P1.5
P1.1 -> P2.1 -> P2.2 -> P2.3 -> P2.4 (also needs P1.5)
P1.5 + P2.4 -> P3.1 [HP1]
```

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| HP0 | P0.2 | Reviews `PD4`–`PD11` (turning PROPOSED to DECIDED or amending them) and answers or keeps defaults for `PQ1`–`PQ8`. Milestone P1 and P2 do not start before `HP0`. |
| HP1 | P3.1 | Runs the `PB1`–`PB5` measurement script on the Orion reference laptop (canvas plan section 2.1) and accepts or blocks the gate. |

---

## Milestone P0: plan, decisions and spike

### P0.1 Parts plan and roadmap · done
- Lane: parts · Depends: none · Read: `AGENTS.md`, `ROADMAP.md`, `canvas-plan.md`, `ui-plan.md`, `mcp-plan.md`, `crates/omnia-doc/src/{doc.rs,command.rs,io.rs}`, `crates/omnia-symbols/src/lib.rs`
- Owns: `parts-plan.md`, `parts-roadmap.md`
- Do: write both files recording owner decisions `PD1`–`PD3`, proposed architecture `PD4`–`PD11`, budgets `PB1`–`PB5`, constants `PK1`–`PK10`, invariants `PINV1`–`PINV10`, scenarios `PTC-*`, spike `PS1` and open questions `PQ1`–`PQ8`. Scan both files for names of other companies and commercial products (AGENTS rule 1).
- Done when: every `P`-prefixed id is defined once and every reference resolves; no statement contradicts existing plans; the rule-1 scan passes and is logged.

### P0.2 Owner decisions and cross-plan alignment · done (HP0 accepted 2026-11-20)
- Lane: any · Depends: P0.1 · Read: `parts-plan.md` sections 6, 13 and 14
- Owns: `parts-plan.md` sections 6 and 13; section 14 edits (`AGENTS.md`, `ROADMAP.md`, `canvas-plan.md`, `ui-plan.md`, `mcp-plan.md`, `docs/file-format-proposal.md`)
- Do: owner reviews `PD4`–`PD11` and `PQ1`–`PQ8`; record dated answers in `parts-plan.md`; apply section 14 cross-links once approved.
- Done when: every `PQ` has a dated answer or kept default; `PD4`–`PD11` are `DECIDED` or amended; section 14 links are applied cleanly.

### P0.3 Spike PS1: SPICE extractor and `.olib` index timing · todo
- Lane: parts · Depends: P0.2 · Read: `parts-plan.md` sections 3, 4 (`PK3`, `PK10`) and 9 (`PS1`)
- Owns: `docs/measurements/P0.3.md` and spike fixture/bench data under `docs/measurements/P0.3/`
- Do: prototype the line-oriented `.model`/`.subckt` extractor on 20 representative openly licensed SPICE decks (continuations `+`, nested `.subckt`, comments `*`/`;`, `PARAMS:`, local dependencies) and measure a 500-part `.olib` metadata-index scan (`PB3`). Record hardware and results.
- Done when: extraction passes on all 20 decks (or records concrete grammar gaps to handle in `P1.2`), `.olib` index timing is recorded and labelled, and `PS1` verdict is logged.

---

## Milestone P1: `omnia-parts`, containers and built-in compiler

### P1.1 Crate scaffold, TOML schema and engineering units · todo
- Lane: parts · Depends: P0.3 · Read: `parts-plan.md` sections 4 (`PK1`, `PK2`, `PK4`, `PK6`, `PK7`), 6 (`PD4`–`PD6`), 10 and 11
- Owns: `crates/omnia-parts/{Cargo.toml,src/lib.rs,src/schema.rs,src/units.rs}`; workspace `Cargo.toml` (if needed — `members = ["crates/*", "xtask"]` already includes it)
- Do: create `omnia-parts` (depending only on `omnia-doc`, `omnia-symbols`, `serde`, `toml`, `zip`, `sha2`); implement `part.toml` and `manifest.toml` DTOs with `#[serde(deny_unknown_fields)]`, `PK1`/`PK2`/`PK7` validation, and `units.rs` (`PK4` / `PQ2` parser + unambiguous SPICE formatter).
- Done when: `PINV1` holds (no GPUI in `omnia-parts`); unit and property tests verify `PK2` id rules, `PK4`/`PQ2` suffix parsing (`10M` vs `10m` vs `10meg`) and SPICE formatting round trips; standard checks pass.

### P1.2 SPICE extractor, local closure and safety scanner · todo
- Lane: parts · Depends: P1.1 · Read: `parts-plan.md` sections 4 (`PK10`), 6 (`PD4`, `PD10`), 7 (`PINV3`, `PINV9`), `docs/measurements/P0.3.md`
- Owns: `crates/omnia-parts/src/spice.rs`
- Do: implement `.model` and `.subckt` extraction with transitive local dependency closure within a `.lib` / archive, port-count parsing for `PINV3`, and `PK10` directive screening rejecting `.control`, `shell`, `system`, `osdi`, `codemodel`, `.csdf` and out-of-archive `.include`/`.lib`.
- Done when: unit tests cover continuations, nested subcircuits, `PARAMS:`, transitive closure, port-count mismatches and every `PK10` forbidden directive (`PINV9`).

### P1.3 Container loader, packer and canonical content hash · todo
- Lane: parts · Depends: P1.2 · Read: `parts-plan.md` sections 4 (`PK3`, `PK5`, `PK8`), 6 (`PD2`, `PD10`), 7 (`PINV2`, `PINV8`), 8 (`PTC-opart-roundtrip`, `PTC-hostile-opart`)
- Owns: `crates/omnia-parts/src/{container.rs,hash.rs}`
- Do: implement `LibrarySource` for unpacked directories and `.opart`/`.olib` ZIP archives with streaming `PK3` size/entry/ratio enforcement and path-traversal/symlink rejection; implement `PK5` canonical SHA-256 hashing; implement `pack_opart` and `pack_olib`.
- Done when: `PINV2` (order- and timestamp-independent hash/catalog property test), `PINV8`, `PTC-opart-roundtrip` and `PTC-hostile-opart` pass.

### P1.4 Starter library, `xtask parts` compiler and static built-ins · todo
- Lane: parts, assets · Depends: P1.3 · Read: `parts-plan.md` sections 6 (`PD1`, `PD7`, `PD11`), 7 (`PINV3`, `PINV10`), 8 (`PTC-builtin-load`, `PTC-xtask-mismatch`)
- Owns: `library/`, `xtask/src/parts.rs`, `xtask/src/main.rs` (command registration), `crates/omnia-parts/src/{generated.rs,catalog.rs}`, `crates/omnia-symbols/pins.toml` (only any missing starter source symbols, regenerated via `cargo xtask symbols`), `THIRD_PARTY.md`
- Do: add the `PD11` starter parts and openly licensed models (`PQ7`) under `library/`; implement `cargo xtask parts [--check]` and `cargo xtask part pack`; compile built-ins into `generated.rs` and expose `Catalog::builtin()`.
- Done when: `cargo xtask parts --check` passes; `PINV3`, `PINV10`, `PTC-builtin-load` and `PTC-xtask-mismatch` pass; `THIRD_PARTY.md` lists every bundled model licence.

### P1.5 Mounted user libraries, search and P1 benchmarks · todo
- Lane: parts, bench · Depends: P1.4 · Read: `parts-plan.md` sections 3 (`PB1`–`PB3`), 6 (`PD2`), 10 (`Catalog`)
- Owns: `crates/omnia-parts/src/catalog.rs`, `crates/omnia-bench/benches/parts.rs`, `docs/measurements/P1.5.md`
- Do: implement `mount_opart`, `mount_olib`, `mount_dir` and deterministic `Catalog::search`; add criterion benches for `PB1` (builtin init), `PB2` (`.opart` load) and `PB3` (500-part `.olib` index). Record non-reference timings in `docs/measurements/P1.5.md`.
- Done when: deterministic search order tests pass across shuffled inputs (`PINV2`); benches compile and run cleanly; non-reference numbers are recorded and labelled.

---

## Milestone P2: `omnia-doc` embedding, commands and RON v3

### P2.1 Embedded `PartDefTable` and `Part` fields in `omnia-doc` · todo
- Lane: doc · Depends: P1.1 · Read: `parts-plan.md` sections 6 (`PD3`, `PD6`, `PD8`), 7 (`PINV1`, `PINV5`, `PINV6`), 10 (`omnia-doc` additions)
- Owns: `crates/omnia-doc/src/{part_def.rs,doc.rs,lib.rs}`
- Do: add `PartDefId`, `ParamValue`, `EmbeddedPartDef`, `EmbeddedPin`, `EmbeddedParamSpec`, `EmbeddedSimBinding` and deduplicated `part_defs` storage on `Doc`; add `def` and `params` (overrides only) to `Part` plus read queries (`part_def`, `part_defs`, `resolved_param`). Keep `omnia-doc` free of `omnia-parts`, `toml` and `zip`.
- Done when: `PINV1`, `PINV5` and `PINV6` tests pass; existing `omnia-doc` tests and `omnia-canvas` / `omnia-app` builds stay green.

### P2.2 `SetParam`, `UpdatePartDef` and `Part::value` sync · todo
- Lane: doc · Depends: P2.1 · Read: `parts-plan.md` sections 6 (`PD6`, `PD8`), 7 (`PINV7`), 8 (`PTC-place-dedup`, `PTC-param-sync`, `PTC-param-refuse`), `canvas-plan.md` `INV4`
- Owns: `crates/omnia-doc/src/{command.rs,undo.rs}`
- Do: update `Command::PlacePart` to accept/deduplicate `EmbeddedPartDef`; add transactional, undoable `Command::SetParam` and `Command::UpdatePartDef`; sync `primary_param` with `Part::value` (`PD6`); add criterion bench for `PB5`.
- Done when: `PINV7`, `PTC-place-dedup`, `PTC-param-sync`, `PTC-param-refuse` and property tests (`undo(apply(c)) == before`, `redo(undo(apply(c))) == after`, `INV10`) pass.

### P2.3 RON `version: 3` save/load and v2 compatibility · todo
- Lane: doc · Depends: P2.2 · Read: `parts-plan.md` sections 4 (`PK9`), 6 (`PD3`, `PD8`), 7 (`PINV4`), 8 (`PTC-offline-omnia`, `PTC-v2-compat`), 13 (`PQ6`)
- Owns: `crates/omnia-doc/src/io.rs`
- Do: implement RON `version: 3` serialising the deduplicated `part_defs` table (pruning unreferenced definitions per `PQ6`) and per-part `def` + `params`; keep `version: 2` loading by synthesising unmodelled `EmbeddedPartDef`s.
- Done when: `PINV4`, `PTC-offline-omnia`, `PTC-v2-compat` and byte-determinism round-trip property tests pass; all existing stress-sheet tests stay green.

### P2.4 `omnia-parts` document bridge and library diff · todo
- Lane: parts · Depends: P1.5, P2.3 · Read: `parts-plan.md` sections 6 (`PD9`), 8 (`PTC-library-update`), 10 (`embed.rs`)
- Owns: `crates/omnia-parts/src/embed.rs`, `crates/omnia-parts/src/catalog.rs`
- Do: implement `Catalog::to_embedded`, `Catalog::place_command` and `Catalog::diff_embedded(&Doc) -> Vec<DefUpdateAvailable>`.
- Done when: `PTC-library-update` passes end-to-end (detect update -> `Command::UpdatePartDef` -> `undo` restores prior hash and SPICE deck).

---

## Milestone P3: verification and Orion gate

### P3.1 Reference-machine gate (`PB1`–`PB5`) · todo → HP1
- Lane: bench · Depends: P1.5, P2.4 · Read: `parts-plan.md` section 3 (`PB1`–`PB5`), `canvas-plan.md` section 2.1, `ui-plan.md` `UB10`
- Owns: `crates/omnia-bench/benches/parts.rs`, `docs/measurements/P3.1.md`, `docs/measurements/P3.1/`
- Do: prepare a one-command reference campaign script covering `PB1`–`PB5` plus `B1`, `UB10` and `INV2` regression checks; set to `needs-human` for the owner's Orion run (`HP1`).
- Done when: Orion results under canvas plan section 2.1 are recorded with raw data and hardware snapshots, all `PB1`–`PB5` budgets hold, and the owner accepts `HP1`.

---

## Log

- **2026-11-20 · P0.1 · done:** Wrote [parts-plan.md](parts-plan.md) and [parts-roadmap.md](parts-roadmap.md) against `main` (`07e96c6`). Recorded owner decisions `PD1`–`PD3` (TOML + shared `models/` tree, `.opart`/`.olib` ZIP containers, deduplicated `.omnia` embedding with lean primitives). Scanned both files for third-party company or commercial product names (AGENTS rule 1): none found. Docs only; nothing measured; no dependency added.
- **2026-11-20 · P0.2 · done (`HP0`):** Owner signed off on `PD4`–`PD11` (all `DECIDED`) and accepted defaults for `PQ1`–`PQ8`. Applied `parts-plan.md` section 14 cross-links to `AGENTS.md`, `ROADMAP.md`, `canvas-plan.md`, `ui-plan.md`, `mcp-plan.md` and `docs/file-format-proposal.md`. Rule-1 scan passed across all touched files. Docs only; nothing measured; no dependency added.
