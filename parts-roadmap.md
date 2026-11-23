# Component definitions and simulation models: roadmap for agents

The task list for Omnia's component definitions, simulation model bindings, `.opart`/`.olib`
containers and embedded document definitions. The reference for every `P`-prefixed id used here
(`PB`, `PK`, `PD`, `PINV`, `PTC`, `PS`, `PQ`, `HP`) is [parts-plan.md](parts-plan.md). Unprefixed
ids are in [canvas-plan.md](canvas-plan.md); `W`, `A`, `U` and `S` ids are in
[wiring-plan.md](wiring-plan.md), [mcp-plan.md](mcp-plan.md), [ui-plan.md](ui-plan.md) and
[sim-plan.md](sim-plan.md). Cross-plan decisions `XD1`–`XD9` are copied into `parts-plan.md`
section 6.

This roadmap runs beside the canvas, wiring, agent, UI and simulation roadmaps. Milestone P1
touches the new `parts` lane, `xtask` and two new leaf files in `omnia-doc`; Milestone P2
coordinates with the `doc` lane and, for one mechanical step, every crate that builds a
`PlacePart` command.

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
14. **Untrusted archives, documents and commands are hostile by default.** Enforce `PK3` before
    allocating buffers and run the one `PD10` validator before admitting any definition
    (`PINV8`, `PINV9`).
15. **Deterministic outputs everywhere.** Never let filesystem walk order, ZIP entry order,
    timestamps or hash-map iteration order affect `Catalog`, `PK5` hashes, `generated.rs`,
    definition ids or RON v3 output (`PINV2`, `PD8`).
16. **Approved model licences only in `library/models/`.** Every bundled model file carries
    explicit licence metadata from `PK11` and is listed in `THIRD_PARTY.md`; proprietary models
    are never committed. Commercial part numbers are not used as fixture ids (AGENTS rule 1).
17. **Keep the plans true.** Any change to a constant, decision, schema field or public
    signature updates [parts-plan.md](parts-plan.md) in the same pull request, and any file in
    its section 14 that the change affects.

## Standard checks

As in `AGENTS.md`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p omnia-bench --all-targets --features scene-counts,readback -- -D warnings
cargo test --workspace
cargo deny check
```

Plus, once `P1.4` lands (and in CI from then on):

```sh
cargo xtask parts --check
```

## Lanes

| Lane | Owns | Needs GPUI |
| --- | --- | --- |
| `parts` (new) | `crates/omnia-parts/`, `library/`, `xtask/src/parts.rs` | No |
| `doc` | `crates/omnia-doc/` files named by a P task | No |
| `assets` | `crates/omnia-symbols/pins.toml`, `THIRD_PARTY.md` | No |
| `bench` | `crates/omnia-bench/benches/{parts.rs,parts_doc.rs}`, `docs/measurements/P*.md` | No |

File ownership order for `omnia-doc` `doc.rs`, `command.rs`, `undo.rs` and `io.rs` (`XD7`):
`P2.1`–`P2.3` first, then simulation `S1.1`, then context `C1.1`. Before starting a P2 task,
check `canvas-roadmap.md`, `wiring-roadmap.md`, `mcp-roadmap.md`, `ui-roadmap.md` and
`sim-roadmap.md` for an open task owning those files. `pins.toml` and the symbol compiler have
one editing task at a time: `P1.4`, then `S1.1`, then `S3.2` (`XD5`).

## Milestones

| # | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| P0 | Plan, roadmap, owner sign-off (`HP0`), SPICE/archive spike (`PS1`) | P0.3 | `HP0`, `PS1` |
| P1 | `omnia-parts` crate, numeric grammar and SPICE screen in `omnia-doc`, `.opart`/`.olib` loader & packer, starter `library/`, `xtask parts` in CI, static `Catalog::builtin()`, mounted libraries | P1.5 | `PB1`–`PB3`, `PINV1`–`PINV3`, `PINV8`–`PINV10` |
| P2 | Embedded definition table, `PlacePart` definitions, `SetParam` & `UpdatePartDef`, `Change::modified`, RON v3 + v2 migration, library diff, document benches | P2.5 | `PB4`, `PB5`, `PINV4`–`PINV7`, `PINV11` |
| P3 | Reference-machine gate on Orion (`PB1`–`PB5`, `B1`, `UB10`, `INV2`) | P3.1 | `HP1` |

## Dependency overview

```
P0.1 -> P0.2 [HP0] -> P0.3 (PS1)
P0.3 -> P1.1 -> P1.2 -> P1.3
P1.2 -> P2.1 -> P2.2 -> P2.3
P1.3 + P2.1 -> P1.4 -> P1.5
P1.5 + P2.3 -> P2.4
P1.4 + P2.3 -> P2.5
P1.5 + P2.4 + P2.5 -> P3.1 [HP1]
P2.3 -> sim S1.1 (XD7)
```

## Human checkpoints

| ID | When | What the owner does |
| --- | --- | --- |
| HP0 | P0.2 | Reviews `PD4`–`PD11` (turning PROPOSED to DECIDED or amending them) and answers or keeps defaults for `PQ1`–`PQ8`. Milestone P1 and P2 do not start before `HP0`. Accepted 2026-11-20; the amendments of 2026-11-23 were made under the owner's delegation (see the log). |
| HP1 | P3.1 | Runs the `PB1`–`PB5` measurement script on the Orion reference laptop (canvas plan section 2.1) and accepts or blocks the gate. |

---

## Milestone P0: plan, decisions and spike

### P0.1 Parts plan and roadmap · done
- Lane: parts · Depends: none · Read: `AGENTS.md`, `ROADMAP.md`, `canvas-plan.md`, `ui-plan.md`, `mcp-plan.md`, `crates/omnia-doc/src/{doc.rs,command.rs,io.rs}`, `crates/omnia-symbols/src/lib.rs`
- Owns: `parts-plan.md`, `parts-roadmap.md`
- Do: write both files recording owner decisions `PD1`–`PD3`, proposed architecture `PD4`–`PD11`, budgets `PB1`–`PB5`, constants `PK1`–`PK10`, invariants `PINV1`–`PINV10`, scenarios `PTC-*`, spike `PS1` and open questions `PQ1`–`PQ8`. Scan both files for names of other companies and commercial products (AGENTS rule 1).
- Done when: every `P`-prefixed id is defined once and every reference resolves; no statement contradicts existing plans; the rule-1 scan passes and is logged.

### P0.2 Owner decisions and cross-plan alignment · done (HP0 accepted 2026-11-20; amended 2026-11-23)
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

### P1.1 Crate scaffold, TOML schema and the shared numeric grammar · todo
- Lane: parts, doc · Depends: P0.3 · Read: `parts-plan.md` sections 4 (`PK1`, `PK2`, `PK4`, `PK6`, `PK7`, `PK12`), 6 (`PD4`–`PD6`, `XD2`), 10 and 11
- Owns: `crates/omnia-parts/{Cargo.toml,src/lib.rs,src/schema.rs}`; `crates/omnia-doc/src/units.rs` (new) and its `mod` line in `crates/omnia-doc/src/lib.rs`; workspace `Cargo.toml` (if needed — `members = ["crates/*", "xtask"]` already includes it)
- Do: create `omnia-parts` (depending only on `omnia-doc`, `omnia-symbols`, `serde`, `toml`, `zip`, `sha2`); implement `part.toml` and `manifest.toml` DTOs with `#[serde(deny_unknown_fields)]` and `PK1`/`PK2`/`PK7` validation, including the single `no_connect` role encoding (`PD4`). In `omnia-doc` `units.rs`, implement `FiniteF64` and the `PK4` parser (`XD2`); `omnia-parts` uses it and adds no parser of its own.
- Done when: `PINV1` holds; unit and property tests verify `PK2` id rules, `PK4`/`PQ2` suffix parsing (`10M` vs `10m` vs `10meg`), refusal of non-finite values, `-0.0` normalisation and `FiniteF64` equality; standard checks pass.

### P1.2 SPICE extractor, local closure and safety screen · todo
- Lane: parts, doc · Depends: P1.1 · Read: `parts-plan.md` sections 4 (`PK10`), 6 (`PD3`, `PD4`, `PD10`), 7 (`PINV3`, `PINV9`), `docs/measurements/P0.3.md`
- Owns: `crates/omnia-parts/src/spice.rs`; `crates/omnia-doc/src/spice_screen.rs` (new) and its `mod` line in `crates/omnia-doc/src/lib.rs`
- Do: implement `.model` and `.subckt` extraction with transitive local dependency closure within a `.lib` / archive and the `PD3` preservation rule (LF and byte-order mark only); in `omnia-doc`, implement `screen_embedded` (`PK10` embedded rules) and `subckt_ports`; `spice.rs` adds archive-relative `.include`/`.lib` resolution and flattening, then calls `screen_embedded` on the result.
- Done when: unit tests cover continuations, nested subcircuits, `PARAMS:`, transitive closure, preservation of comments and case, port counts and every `PK10` forbidden directive in both archive and embedded form (`PINV9`).

### P1.3 Container loader, packer and canonical content hash · todo
- Lane: parts · Depends: P1.2 · Read: `parts-plan.md` sections 4 (`PK3`, `PK5`, `PK8`), 6 (`PD2`, `PD9`, `PD10`), 7 (`PINV2`, `PINV8`), 8 (`PTC-opart-roundtrip`, `PTC-hostile-opart`)
- Owns: `crates/omnia-parts/src/{container.rs,hash.rs}`
- Do: implement `LibrarySource` for unpacked directories and `.opart`/`.olib` ZIP archives with streaming `PK3` size/entry/ratio enforcement, path-traversal/symlink rejection and `CustomSymbolUnsupported` for `symbol.svg`/`pins.toml` (`PD2`); implement the `PK5` canonical encoding and SHA-256 hash; implement `pack_opart` and `pack_olib`.
- Done when: `PINV2` (order- and timestamp-independent hash property test), `PINV8`, `PTC-opart-roundtrip` (at the container level) and `PTC-hostile-opart` pass.

### P1.4 Starter library, `xtask parts` compiler, CI step and static built-ins · todo
- Lane: parts, assets · Depends: P1.3, P2.1 · Read: `parts-plan.md` sections 4 (`PK11`, `PK12`), 6 (`PD1`, `PD5`, `PD7`, `PD10`, `PD11`, `XD3`, `XD5`), 7 (`PINV3`, `PINV10`, `PINV11`), 8 (`PTC-builtin-load`, `PTC-xtask-mismatch`)
- Owns: `library/`, `xtask/src/parts.rs`, `xtask/src/main.rs` (command registration), `xtask/Cargo.toml` (workspace-path dependencies only), `crates/omnia-parts/src/{generated.rs,catalog.rs}`, `crates/omnia-symbols/pins.toml` and its regenerated `crates/omnia-symbols/src/generated.rs` (only the source symbols the starter parts need), `xtask/src/symbols.rs` (only if needed to keep every existing `SymbolId` unchanged), `THIRD_PARTY.md`, `.github/workflows/ci.yml` (one step running `cargo xtask parts --check`)
- Do: add the `PD11` starter parts (`diode-signal`, `opamp-generic` with its `CC0-1.0` behavioural macromodel per `XD3`, and the primitives) under `library/` with `PK12` parameter names; implement `cargo xtask parts [--check]` (running the `omnia-doc` validator and the `PK11` licence check) and `cargo xtask part pack`; compile built-ins into `generated.rs` and expose `Catalog::builtin()`; add the CI step.
- Done when: `cargo xtask parts --check` passes locally and the CI step runs it; `PINV3`, `PINV10`, `PINV11`, `PTC-builtin-load` and `PTC-xtask-mismatch` pass; every existing `SymbolId` and all goldens are unchanged; `THIRD_PARTY.md` lists every bundled model licence.

### P1.5 Mounted user libraries, resolve, search and P1 benchmarks · todo
- Lane: parts, bench · Depends: P1.4 · Read: `parts-plan.md` sections 3 (`PB1`–`PB3`), 6 (`PD2`, `PD9`), 8 (`PTC-hash-provenance`), 10 (`Catalog`)
- Owns: `crates/omnia-parts/src/catalog.rs`, `crates/omnia-bench/benches/parts.rs`, its `[[bench]]` entry and the `omnia-parts` dependency in `crates/omnia-bench/Cargo.toml`, `docs/measurements/P1.5.md`
- Do: implement `mount_opart`, `mount_olib`, `mount_dir`, metadata-only indexing, `resolve` with lazy `PK5` verification (`PD9`) and deterministic `Catalog::search`; add criterion benches for `PB1` (builtin init), `PB2` (`.opart` load and resolve) and `PB3` (500-part `.olib` index). Record non-reference timings in `docs/measurements/P1.5.md`.
- Done when: deterministic search order tests pass across shuffled inputs (`PINV2`); `PTC-hash-provenance` passes; benches compile and run cleanly; non-reference numbers are recorded and labelled.

---

## Milestone P2: `omnia-doc` embedding, commands and RON v3

### P2.1 Definition table, validator and stored part fields in `omnia-doc` · todo
- Lane: doc · Depends: P1.2 · Read: `parts-plan.md` sections 6 (`PD3`, `PD5`, `PD6`, `PD8`, `PD10`, `XD1`, `XD7`), 7 (`PINV1`, `PINV5`, `PINV6`, `PINV11`), 10 (`omnia-doc` additions); `crates/omnia-doc/src/{doc.rs,io.rs}`
- Owns: `crates/omnia-doc/src/{part_def.rs,doc.rs,lib.rs}`; in `crates/omnia-doc/src/io.rs`, only the `PartialEq`/`content_eq` impls and the v2 loader's interning of unmodelled definitions
- Do: add `PartDefId`, `ParamValue`, `Params`, `PinRole`, `EmbeddedPin`, `EmbeddedParamSpec`, `EmbeddedSimBinding`, `EmbeddedPartDef` and `validate` (`PD10`, calling `spice_screen`); add the definition arena with the `PD8` id policy and interning; add `def` and `params` to crate-private `StoredPart` (not to `Part`); every existing insertion path interns the `PD8` unmodelled definition; add the read queries (`part_def`, `part_defs`, `part_def_id`, `part_params`, `resolved_param`); extend `PartialEq` and `content_eq` as `PD8` defines. Keep `omnia-doc` free of `omnia-parts`, `toml`, `zip` and `sha2`.
- Done when: `PINV1`, `PINV5`, `PINV6` and `PINV11` tests pass, plus validator unit tests for every `PD10` rule; no file outside **Owns** changes and the whole workspace builds and tests green (the public `Part` and `Command` are unchanged at this step).

### P2.2 `PlacePart` definitions, `SetParam`, `UpdatePartDef`, `Change::modified` and value sync · todo
- Lane: doc (plus mechanical updates in other crates) · Depends: P2.1 · Read: `parts-plan.md` sections 6 (`PD6`, `PD8`, `XD7`), 7 (`PINV7`, `PINV11`), 8 (`PTC-place-dedup`, `PTC-param-sync`, `PTC-param-refuse`, `PTC-param-silent`, `PTC-batch-intern-refused`, `PTC-pin-order`, `PTC-hostile-embedded`), `canvas-plan.md` `INV4`
- Owns: `crates/omnia-doc/src/{command.rs,undo.rs}`. Mechanical updates only (adding `def: PlaceDef::Unmodelled` to each `PlacePart`, the `modified` field to each `Change` literal, and arms for the new variants in each exhaustive `match`; no behaviour change): `crates/omnia-symbols/src/lib.rs` (`place_command`), `crates/omnia-agent/src/{ops.rs,service.rs,schema/tests.rs}`, `crates/omnia-canvas/src/{sheet.rs,sheet/text.rs}`, `crates/omnia-canvas/tests/sheet_text_render.rs`, the test modules in `crates/omnia-doc/src/{doc.rs,route.rs,run.rs}`, `crates/omnia-doc/tests/index_alloc.rs`, and any other construction site the compiler reports, listed in the log entry
- Do: add `PlaceDef` to `Command::PlacePart` (`Unmodelled`, `Existing`, `Embed` with validation and interning); add transactional, undoable `Command::SetParam` and `Command::UpdatePartDef` with exact rollback of interning on refusal; add `Change::modified` (`XD7`) for apply, undo and redo; sync `primary_param` with `Part::value` (`PD6`); add the new `CommandError` variants.
- Done when: `PINV7`, `PINV11`, `PTC-place-dedup`, `PTC-param-sync`, `PTC-param-refuse`, `PTC-param-silent`, `PTC-batch-intern-refused`, `PTC-pin-order`, the command half of `PTC-hostile-embedded` and property tests (`INV4`, `INV10` with the new commands in the generator) pass; the U4.2 picker and the agent's `place_part` behave as before.

### P2.3 RON `version: 3`, v2 migration and file-host behaviour · todo
- Lane: doc, agent · Depends: P2.2 · Read: `parts-plan.md` sections 4 (`PK9`), 6 (`PD3`, `PD8`, `PD10`, `XD1`), 7 (`PINV4`), 8 (`PTC-offline-omnia`, `PTC-def-lifecycle`, `PTC-hostile-embedded`, `PTC-v2-compat`, `PTC-version-refuse`), 13 (`PQ6`); `crates/omnia-agent/src/{files.rs,catalog.rs}`
- Owns: `crates/omnia-doc/src/io.rs`; `crates/omnia-agent/src/{files.rs,catalog.rs}`; `ui-plan.md` (`UTC-open-bad` and the two "RON version 2" facts); `mcp-plan.md` (`AQ12` and the "RON v2" mentions)
- Do: write RON `version: 3` with the definition table indexed by id (`None` for retained and empty slots, no renumbering, arena length kept) and per-part `def` + `params`; load v3 through the `PD10` validator; load v2 by the strict `PD8` migration; refuse every other version. In `omnia-agent`, open v2 and v3, save v3, and update the error text and the RON version strings; move the unsupported-version fixtures from `version:3` to `version:4` in `io.rs` and `files.rs`; update `ui-plan.md` and `mcp-plan.md` to match.
- Done when: `PINV4` (including the byte-identity property), `PTC-offline-omnia` (with a hand-built embedded definition), `PTC-def-lifecycle`, the load half of `PTC-hostile-embedded`, `PTC-v2-compat` and `PTC-version-refuse` pass; every `tests/golden/*.ron` file is unchanged on disk and all goldens match with 0 differing pixels; all stress-sheet and agent file tests stay green.

### P2.4 `omnia-parts` document bridge and library diff · todo
- Lane: parts · Depends: P1.5, P2.3 · Read: `parts-plan.md` sections 6 (`PD9`), 8 (`PTC-library-update`, `PTC-offline-omnia`), 10 (`embed.rs`, `Catalog`)
- Owns: `crates/omnia-parts/src/embed.rs`, `crates/omnia-parts/src/catalog.rs`
- Do: implement `Catalog::to_embedded`, `Catalog::place_command` (`PlaceDef::Embed`, value from the primary parameter's default) and `Catalog::diff_embedded(&Doc) -> Vec<DefUpdateAvailable>` with the `PD9` hash rules.
- Done when: `PTC-library-update` passes end-to-end (detect update -> `Command::UpdatePartDef` -> `undo` restores the prior record and id) and `PTC-offline-omnia` passes with a mounted `.opart` part.

### P2.5 Document benches for `PB4` and `PB5` · todo
- Lane: bench · Depends: P1.4, P2.3 · Read: `parts-plan.md` section 3 (`PB4`, `PB5`), canvas plan section 2.1, `crates/omnia-bench/benches/{doc_io.rs,command.rs}`
- Owns: `crates/omnia-bench/benches/parts_doc.rs`, its `[[bench]]` entry in `crates/omnia-bench/Cargo.toml`, `docs/measurements/P2.5.md`
- Do: build a `10k` sheet with 50 unique embedded definitions (primitives, `.model` and `.subckt` from `Catalog::builtin()`) through public APIs only; bench `save`/`load` against the unmodelled `10k` baseline (`PB4`) and `apply(SetParam)` (`PB5`), with the `INV2` check. Record non-reference numbers.
- Done when: the bench runs cleanly; non-reference numbers are recorded and labelled.

---

## Milestone P3: verification and Orion gate

### P3.1 Reference-machine gate (`PB1`–`PB5`) · todo → HP1
- Lane: bench · Depends: P1.5, P2.4, P2.5 · Read: `parts-plan.md` section 3 (`PB1`–`PB5`), `canvas-plan.md` section 2.1, `ui-plan.md` `UB10`
- Owns: `docs/measurements/P3.1.md`, `docs/measurements/P3.1/` (campaign script, order file, raw data, condition snapshots)
- Do: prepare a one-command reference campaign script running the `parts.rs` and `parts_doc.rs` benches for `PB1`–`PB5` plus `B1`, `UB10` and `INV2` regression checks; set to `needs-human` for the owner's Orion run (`HP1`).
- Done when: Orion results under canvas plan section 2.1 are recorded with raw data and hardware snapshots, all `PB1`–`PB5` budgets hold, and the owner accepts `HP1`.

---

## Log

- **2026-11-20 · P0.1 · done:** Wrote [parts-plan.md](parts-plan.md) and [parts-roadmap.md](parts-roadmap.md) against `main` (`07e96c6`). Recorded owner decisions `PD1`–`PD3` (TOML + shared `models/` tree, `.opart`/`.olib` ZIP containers, deduplicated `.omnia` embedding with lean primitives). Scanned both files for third-party company or commercial product names (AGENTS rule 1): none found. Docs only; nothing measured; no dependency added.
- **2026-11-20 · P0.2 · done (`HP0`):** Owner signed off on `PD4`–`PD11` (all `DECIDED`) and accepted defaults for `PQ1`–`PQ8`. Applied `parts-plan.md` section 14 cross-links to `AGENTS.md`, `ROADMAP.md`, `canvas-plan.md`, `ui-plan.md`, `mcp-plan.md` and `docs/file-format-proposal.md`. Rule-1 scan passed across all touched files. Docs only; nothing measured; no dependency added.
- **2026-11-23 · P0.1/P0.2 · done (amended, owner-delegated):** Updated pull request #85 onto `main` (`9d4b844`). Resolved `AGENTS.md` (simulation and parts registrations both kept; `HP*` added to hard rule 4) and `ROADMAP.md` (UI, simulation and parts links; Stage 3 cites parts for definitions and bindings and simulation for lowering; the stale "engine and execution plan still needed" sentence dropped). Fixed the independent review of #85: (1) `PD8` amended — `Part` unchanged with `def`/`params` on `StoredPart`, never-renumbered definition ids, interning against referenced records only, exact rollback on refusal, persistent vs live equality, `PINV4`/`PINV7` restated, finite reals keep `Eq` (`PD6`), new `PTC-def-lifecycle` and `PTC-batch-intern-refused`; (2) buildable migration — `P2.1` changes nothing public, `P2.2` owns every `PlacePart`/`Change`/`match` site, `P2.3` owns the v3 writer, v2 reader, agent file behaviour and version fixtures, goldens and `SymbolId`s stay unchanged; (3) one `PD10` validator on v3 load, `PlacePart` and `UpdatePartDef`, explicit stored-index → pin number → terminal mapping (`PD5`, `PINV11`, `PTC-pin-order`), `PD2` refuses custom symbols per `PQ3`, `PD3` separates deck preservation from hashing, `PD9` defines verified hashes and provenance; (4) generic op-amp and diode fixtures replace commercial part numbers; (5) `P0.3.md`, `UpdatePartDef`, `mount_opart`, `part_defs()`, `place_command`, one no-connect encoding, metadata-only `PB3` indexing, `PB4`/`PB5` bench in new task `P2.5`, CI step owned by `P1.4`, model licence policy `PK11` separate from `deny.toml`, and section 14 matched to the edits actually made. Aligned with cross-plan decisions XD1–XD9 (coordinator, owner-delegated, 2026-11-23), copied into `parts-plan.md` section 6: `PK4` parser in `omnia-doc` `units.rs` (`XD2`), `PK12` shared parameter names (`XD1`), generic op-amp default binding (`XD3`), "definitions and bindings" in place of netlist wording (`XD4`), pin role vs symbol class and `pins.toml` editing order (`XD5`), `Change::modified` and the `omnia-doc` file order (`XD7`). Rule-1 scan of every touched file: no company named; one brief compatibility nod to a diode class in `PD11`. Docs only; nothing measured; no dependency added.
