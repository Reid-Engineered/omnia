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
`P2.1`–`P2.3` first, then context `C1.9` (lineage token in `doc.rs`), then context `C1.1` (simulation `S1.1` no longer owns any `omnia-doc` file after sim `S0.2`, pull request #104). Before starting a P2 task,
check `canvas-roadmap.md`, `wiring-roadmap.md`, `mcp-roadmap.md`, `ui-roadmap.md` and
`sim-roadmap.md` for an open task owning those files. `pins.toml` and the symbol compiler have
one editing task at a time: `P1.4`, then `S1.1`, then `S3.2` (`XD5`).

## Milestones

| # | Delivers | Gate task | Gated |
| --- | --- | --- | --- |
| P0 | Plan, roadmap, owner sign-off (`HP0`), SPICE/archive spike (`PS1`) | P0.3 | `HP0`, `PS1` |
| P1 | `omnia-parts` crate, numeric grammar and SPICE screen in `omnia-doc`, `.opart`/`.olib` loader & packer, starter `library/`, `xtask parts` in CI, static `Catalog::builtin()`, mounted libraries | P1.5 | `PB1`–`PB3`, `PINV1`–`PINV3`, `PINV8`–`PINV10` |
| P2 | Embedded definition table, `PlacePart` definitions, `SetParam` & `UpdatePartDef`, `Change::modified`, RON v3 + v2 migration, library diff, document benches | P2.5 | `PB4`, `PB5`, `PINV4`–`PINV7`, `PINV11`, `PINV12` |
| P3 | Reference-machine gate on Orion (`PB1`–`PB5`, `B1`, `B9`, `UB10`, `AB2`, `INV7`) | P3.1 | `HP1` |

## Dependency overview

```
P0.1 -> P0.2 [HP0] -> P0.3 (PS1)
P0.3 -> P1.1 -> P1.2 -> P1.3
P1.2 -> P2.1 -> P2.2 -> P2.3
P1.3 + P2.1 -> P1.4 -> P1.5
P1.4 -> P2.3 (SymbolLibrary::check_doc)
P1.5 + P2.3 -> P2.4
P1.5 + P2.3 + P2.4 -> P2.5
P1.5 + P2.4 + P2.5 + UI U2.8 + agent A1.10 -> P3.1 [HP1]
P1.4 -> sim S1.1 (XD5: pins.toml and the symbol compiler)
P2.3 -> context C1.9 -> context C1.1 (XD7: omnia-doc files)
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

### P0.3 Spike PS1: SPICE extractor and `.olib` index timing · done
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
- Done when: `PINV1` holds; unit and property tests verify `PK2` id rules, `PK4`/`PQ2` suffix parsing (`10M` vs `10m` vs `10meg`), unit tokens and the one-space rule (`10 kΩ`, `4.7uF`, `1F` vs `10fF`, `Ω` vs `ohm`, unit mismatch, `50%` on a `ratio` parameter, `4k7` refused), refusal of non-finite values, `-0.0` normalisation and `FiniteF64` equality; standard checks pass.

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
- Owns: `library/`, `xtask/src/parts.rs`, `xtask/src/main.rs` (command registration), `xtask/Cargo.toml` (workspace-path dependencies only), `crates/omnia-parts/src/{generated.rs,catalog.rs}`, the `SYMBOL_CATALOG` rows for new symbols in `crates/omnia-app/src/parts/picker.rs` (its completeness test covers every built-in symbol), `crates/omnia-symbols/pins.toml` and its regenerated `crates/omnia-symbols/src/generated.rs` (only the source symbols the starter parts need), `xtask/src/symbols.rs` (only if needed to keep every existing `SymbolId` unchanged), `THIRD_PARTY.md`, `.github/workflows/ci.yml` (one step running `cargo xtask parts --check`), `crates/omnia-bench/src/generate.rs` (only the fixed symbol list and its fingerprint test), `crates/omnia-symbols/src/lib.rs` (only `try_get` and `check_doc`, `PD8`)
- Do: add the `PD11` starter parts (`diode-signal`, `opamp-generic` with its `CC0-1.0` behavioural macromodel per `XD3`, and the primitives) under `library/` with `PK12` parameter names; implement `cargo xtask parts [--check]` (running the `omnia-doc` validator and the `PK11` licence check) and `cargo xtask part pack`; compile built-ins into `generated.rs` and expose `Catalog::builtin()`; parse and validate `[sim.supply]` (`PD5`) and `[symbol] default` (at most one per symbol, `PD11`), with `opamp-generic` declaring its supply pins and ±15 V fallback; add `SymbolLibrary::try_get` and `check_doc`; add the CI step. First, before touching `pins.toml`, pin the stress-sheet generator to an explicit fixed list of today's six `SymbolId`s in today's order instead of every built-in symbol (`PD11`), and add a test that fingerprints every `StressSheet::ALL` sheet at the seeds the benches use: a 64-bit FNV-1a over the generated items (each part's symbol id, position, orientation, stored world pin coordinates, local bounds, designator and value; each wire; each label), independent of the file format, with the expected values computed on `main` before the change and committed in the test. New symbols do not enter existing sheets; a sheet that should use them is a new named variant.
- Done when: the stress-sheet fingerprint test passes after the new symbols are added, so every existing stress sheet is item-for-item identical to `main`; `cargo xtask parts --check` passes locally and the CI step runs it; `PINV3`, `PINV10`, `PINV11`, `PTC-builtin-load`, `PTC-xtask-mismatch` and the `check_doc` half of `PTC-unknown-symbol` pass; every existing `SymbolId` and all goldens are unchanged; `THIRD_PARTY.md` lists every bundled model licence.

### P1.5 Mounted user libraries, resolve, search and P1 benchmarks · todo
- Lane: parts, bench · Depends: P1.4 · Read: `parts-plan.md` sections 3 (`PB1`–`PB3`), 6 (`PD2`, `PD9`), 8 (`PTC-hash-provenance`), 10 (`Catalog`)
- Owns: `crates/omnia-parts/src/catalog.rs`, `crates/omnia-bench/benches/parts.rs`, its `[[bench]]` entry and the `omnia-parts` dependency in `crates/omnia-bench/Cargo.toml` (under `[dependencies]`, since `P2.5` uses it from the library's `src/generate.rs`), `docs/measurements/P1.5.md`
- Do: implement `mount_opart`, `mount_olib`, `mount_dir`, metadata-only indexing, `resolve` with lazy `PK5` verification (`PD9`) and deterministic `Catalog::search`; add criterion benches for `PB1` (builtin init), `PB2` (`.opart` load and resolve) and `PB3` (500-part `.olib` index). Record non-reference timings in `docs/measurements/P1.5.md`.
- Done when: deterministic search order tests pass across shuffled inputs (`PINV2`); `PTC-hash-provenance` passes; benches compile and run cleanly; non-reference numbers are recorded and labelled.

---

## Milestone P2: `omnia-doc` embedding, commands and RON v3

### P2.1 Definition table, validator and stored part fields in `omnia-doc` · todo
- Lane: doc · Depends: P1.2 · Read: `parts-plan.md` sections 6 (`PD3`, `PD5`, `PD6`, `PD8`, `PD10`, `XD1`, `XD7`), 7 (`PINV1`, `PINV5`, `PINV6`, `PINV11`, `PINV12`), 10 (`omnia-doc` additions); `crates/omnia-doc/src/{doc.rs,io.rs}`
- Owns: `crates/omnia-doc/src/{part_def.rs,doc.rs,lib.rs}`; in `crates/omnia-doc/src/command.rs`, only the definition-arena rollback in `apply`/`apply_batch` and the test snapshots described below; in `crates/omnia-doc/src/io.rs`, only the `PartialEq`/`content_eq` impls, the v2 loader's interning of unmodelled definitions and the temporary save refusal (`PD8`, no lossy save)
- Do: add `PartDefId`, `ParamValue`, `Params`, `PinRole`, `EmbeddedPin`, `EmbeddedParamSpec`, `EmbeddedSimBinding`, `EmbeddedPartDef`, `validate` (`PD10`, calling `spice_screen`) and `validate_instance` (`PD8`); make `save` refuse a document holding a non-unmodelled referenced definition or a non-empty `params` (`PD8`); add the definition arena with the `PD8` id policy and interning; add `def` and `params` to crate-private `StoredPart` (not to `Part`); every existing insertion path interns the `PD8` unmodelled definition; add the read queries (`part_def`, `part_defs`, `part_def_id`, `part_params`, `resolved_param`); extend `PartialEq` and `content_eq` as `PD8` defines; make refusal in `apply` and `apply_batch` truncate the definition arena to its prior length; add a test-only in-memory snapshot (`#[cfg(test)]` clone of all persistent state plus retained definition records, without history) and switch the `INV4` transaction property and the other `command.rs` tests that use a save/load round trip as an equality snapshot to it, keeping v2 byte-compatibility and the save refusal as separate tests. Keep `omnia-doc` free of `omnia-parts`, `toml`, `zip` and `sha2`.
- Done when: `PINV1`, `PINV5`, `PINV6` and `PINV11` tests pass, plus unit tests for every `PD10` rule, every `validate_instance` rule (including two pins swapped at equal count, `raw`/`value` disagreement and `Part::value` sync) and the save refusal, through crate-private inserts; a refused batch whose first step interns an unmodelled definition leaves the arena length and records unchanged, and, asserted separately because the snapshot excludes history, leaves `history_cursor`, `can_undo`, `can_redo` and the redo stack unchanged (`PINV7`); `validate` checks every `SupplyDecl` rule (`PD5`); the `INV4` property passes on in-memory snapshots, and a separate test still proves v2 save/load byte-compatibility for unmodelled documents; no file outside **Owns** changes and the whole workspace builds and tests green (the public `Part` and `Command` are unchanged at this step).

### P2.2 `PlacePart` definitions, `SetParam`, `UpdatePartDef`, `Change::modified` and value sync · todo
- Lane: doc (plus mechanical updates in other crates) · Depends: P2.1 · Read: `parts-plan.md` sections 6 (`PD6`, `PD8`, `XD7`), 7 (`PINV7`, `PINV11`), 8 (`PTC-place-dedup`, `PTC-param-sync`, `PTC-param-refuse`, `PTC-param-silent`, `PTC-batch-intern-refused`, `PTC-pin-order`, `PTC-instance-refuse`, `PTC-no-lossy-save`, `PTC-hostile-embedded`), `canvas-plan.md` `INV4`
- Owns: `crates/omnia-doc/src/{command.rs,undo.rs}`. Mechanical updates only (adding `def: PlaceDef::Unmodelled` and `params: Params::new()` to each `PlacePart`, the `modified` field to each `Change` literal, and arms for the new variants in each exhaustive `match`; no behaviour change): `crates/omnia-symbols/src/lib.rs` (`place_command`), `crates/omnia-agent/src/{ops.rs,service.rs,schema/tests.rs}`, `crates/omnia-canvas/src/{sheet.rs,sheet/text.rs}`, `crates/omnia-canvas/tests/sheet_text_render.rs`, the test modules in `crates/omnia-doc/src/{doc.rs,route.rs,run.rs}`, `crates/omnia-doc/tests/index_alloc.rs`, and any other construction site the compiler reports, listed in the log entry
- Do: add `PlaceDef` (`Unmodelled`, `Existing`, `Embed` with validation and interning) and initial override `params: Params` to `Command::PlacePart`, storing `params` on the new `StoredPart` and calling `validate_instance` (with `Part::value` and `params`) in `PlacePart`, `SetParam` and `UpdatePartDef`; add transactional, undoable `Command::SetParam` and `Command::UpdatePartDef` with exact rollback of interning on refusal; add `Change::modified` (`XD7`) for apply, undo and redo; refresh `Part::value` on `UpdatePartDef` from `to`'s resolved primary in every case (`PD8`); sync `primary_param` with `Part::value` (`PD6`); add the new `CommandError` variants.
- Done when: a `PlacePart` with a valid initial override stores it and shows its text, and one with an invalid override or a `Part::value` that disagrees with it is refused with the document unchanged; `PINV7`, `PINV11`, `PTC-place-dedup`, `PTC-param-sync`, `PTC-param-refuse`, `PTC-param-silent`, `PTC-batch-intern-refused`, `PTC-pin-order`, the command halves of `PTC-instance-refuse` and `PTC-hostile-embedded`, `PTC-update-value`, the refusal half of `PTC-no-lossy-save` and property tests (`INV4`, `INV10`, `PINV12` with the new commands in the generator) pass; `save` still writes byte-identical v2 for unmodelled documents; the U4.2 picker and the agent's `place_part` behave as before.

### P2.3 RON `version: 3`, v2 migration and file-host behaviour · todo
- Lane: doc, agent (and shell if `U2.3` has already merged) · Depends: P2.2, P1.4 (`SymbolLibrary::check_doc`) · Read: `parts-plan.md` sections 4 (`PK9`), 6 (`PD3`, `PD8`, `PD10`, `XD1`), 7 (`PINV4`), 8 (`PTC-offline-omnia`, `PTC-def-lifecycle`, `PTC-instance-refuse`, `PTC-no-lossy-save`, `PTC-hostile-embedded`, `PTC-unknown-symbol`, `PTC-v2-compat`, `PTC-version-refuse`), 13 (`PQ6`); `crates/omnia-agent/src/{files.rs,catalog.rs}`
- Owns: `crates/omnia-doc/src/io.rs`; `crates/omnia-agent/src/{files.rs,catalog.rs}`; `ui-plan.md` (RON version facts); `mcp-plan.md` (`AQ12` and the "RON v2" mentions). If `U2.3` has already merged, also own the File > Open check in `crates/omnia-app/src/{files.rs,workspace.rs}` and the `UTC-open-bad` fixture in `crates/omnia-app/src/files.rs` or the `crates/omnia-app/tests/shell_*.rs` file where `U2.3` puts it, plus the `UTC-open-bad` scenario in `ui-plan.md`.
- Do: write RON `version: 3` with the definition table indexed by id (`None` for retained and empty slots, no renumbering, arena length kept) and per-part `def` + `params`; load v3 through `validate` and `validate_instance`, with part geometry taken from the definition; load v2 by the strict `PD8` migration; refuse every other version; remove the temporary save refusal. In `omnia-agent`, open v2 and v3 and refuse a file that fails `SymbolLibrary::check_doc`, save v3, and update the error text and the RON version strings; move the unsupported-version fixtures from `version:3` to `version:4` in `io.rs` and `files.rs`; update `ui-plan.md` and `mcp-plan.md` to match.
  Whichever of `P2.3` and `U2.3` merges second calls `SymbolLibrary::check_doc` immediately after `Doc::load` in File > Open and bumps the rejected version from `3` to `4` in both `UTC-open-bad` and its app fixture. That task runs `UTC-open-bad` and the app half of `PTC-unknown-symbol`.
- Done when: `PINV4` (including the byte-identity property), `PTC-offline-omnia` (with a hand-built embedded definition), `PTC-def-lifecycle` (both branches), the load halves of `PTC-instance-refuse` and `PTC-hostile-embedded`, the v3 half of `PTC-no-lossy-save`, `PINV12` on load, the agent file-host check in `PTC-unknown-symbol`, `PTC-v2-compat` and `PTC-version-refuse` pass. If `P2.3` merges second, `UTC-open-bad` and the app check in `PTC-unknown-symbol` also pass. Every `tests/golden/*.ron` file is unchanged on disk and all goldens match with 0 differing pixels. All stress-sheet and agent file tests stay green.

### P2.4 `omnia-parts` document bridge and library diff · todo
- Lane: parts · Depends: P1.5, P2.3 · Read: `parts-plan.md` sections 6 (`PD9`), 8 (`PTC-library-update`, `PTC-offline-omnia`), 10 (`embed.rs`, `Catalog`)
- Owns: `crates/omnia-parts/src/embed.rs`, `crates/omnia-parts/src/catalog.rs`
- Do: implement `Catalog::to_embedded`, `Catalog::place_command` (`PlaceDef::Embed`, value from the resolved primary), `Catalog::place_symbol` (the symbol's default definition if any, else `PlaceDef::Unmodelled`, `PD11`; an optional `value` becomes the primary-parameter override in `PlacePart::params`) and `Catalog::diff_embedded(&Doc) -> Vec<DefUpdateAvailable>` with the `PD9` hash rules, using `SymbolLibrary::try_get` so an unknown symbol is reported, never indexed.
- Done when: `PTC-library-update` passes end-to-end (detect update -> `Command::UpdatePartDef` -> `undo` restores the prior record and id), the `diff_embedded` half of `PTC-unknown-symbol` passes, `place_symbol` on the op-amp symbol embeds `opamp-generic` with its supply declaration and on a symbol without a default gives `PlaceDef::Unmodelled`, `place_symbol` on the resistor with `4.7k` commits one `PlacePart` whose part has a 4,700 Ω override and `Part::value` `4.7k` (empty and `10k` give no override; `opamp-generic` with a value is refused; a refused value leaves the document unchanged), and `PTC-offline-omnia` passes with a mounted `.opart` part.

### P2.5 Document benches for `PB4` and `PB5` and the embedded stress sheet · todo
- Lane: bench · Depends: P1.5 (which adds the `omnia-parts` dependency to `crates/omnia-bench/Cargo.toml`), P2.3, P2.4 (catalog embedding) · Read: `parts-plan.md` sections 3 (`PB4`, `PB5`) and 6 (`PD11`), canvas plan section 2.1, `crates/omnia-bench/benches/{doc_io.rs,command.rs}`, `crates/omnia-bench/src/{generate.rs,scene.rs}`
- Owns: `crates/omnia-bench/benches/parts_doc.rs`, its `[[bench]]` entry in `crates/omnia-bench/Cargo.toml`, `crates/omnia-bench/src/generate.rs` (new `StressSheet::Embedded10k` and its generator, which takes the `seed` argument of `generate` like every sheet), `crates/omnia-bench/src/main.rs` (the `USAGE` sheet list), `crates/omnia-bench/src/scene.rs` (sheet parser and exhaustive match for the new variant), `docs/measurements/P2.5.md`
- Do: build the fixture deterministically through public APIs only. (1) Let `B` be the `Catalog::builtin()` definitions whose symbol occurs in the generated `10k` stress sheet (the benches use seed 42; `generate(StressSheet::Embedded10k, seed)` threads `seed` through), in `source_id` order; with the `P1.4` starter set this includes `diode-signal` (`.model`) and `opamp-generic` (`.subckt`) alongside primitives. (2) For `i` in `0..50`, variant `V[i]` is a clone of the embedded record of `B[i mod |B|]` with `source_id = "bench:<base id>-<i:02>"`, `description` suffixed with ` #<i:02>` and `content_hash = None`; nothing else changes, so all 50 records are distinct and each passes `validate`. (3) Walk the generated sheet's parts in id order and apply `PlacePart { def: Embed(V[j]) }`, where `V[j]` is the next variant, round robin, among those whose symbol matches the part; each placed part keeps the generator's designator, and its `Part::value` is the `raw` text of its definition's resolved primary parameter (empty when it has none), since the generator's seeded values would fail `validate_instance`; then add the sheet's wires and labels. Assert that `part_defs().count() == 50`. The unmodelled baseline is the same walk with `PlaceDef::Unmodelled` and the same value text, so the comparison isolates embedding. Bench `save`/`load` of both (`PB4`) and `apply(SetParam)` on a fixed sequence of resistor parts (`PB5`), with the `INV7` check. Record non-reference numbers.
  Share the embedded fixture generator with `StressSheet::Embedded10k` and expose it as `bench run --sheet embedded-10k` for `B9`. Return both the embedded fixture and its matching unmodelled baseline with empty undo history (a `save`/`load` round trip, which keeps `revision` and satisfies `generate`'s `doc == Doc::load(save(doc))` check) so the measurements represent an opened sheet. Keep the existing stress sheets fixed (`PD11`).
- Done when: the fixture assertion holds and the fixture saves byte-identical across two runs; `bench run --sheet embedded-10k` selects the workload with 10,000 parts, 20,000 wires and 50 definitions; the fingerprints for existing sheets still match; the bench runs cleanly; non-reference numbers are recorded and labelled.

---

## Milestone P3: verification and Orion gate

### P3.1 Reference-machine gate (`PB1`–`PB5`) · todo → HP1
- Lane: bench · Depends: P1.5, P2.4, P2.5, UI U2.8 (`UB10` harness), agent A1.10 (`AB2` harness) · Read: `parts-plan.md` section 3 (`PB1`–`PB5`), `canvas-plan.md` section 2.1, `ui-plan.md` `UB10`, `mcp-plan.md` `AB2`
- Owns: `docs/measurements/P3.1.md`, `docs/measurements/P3.1/` (campaign script, order file, raw data, condition snapshots)
- Do: prepare a one-command reference campaign script running the `parts.rs` and `parts_doc.rs` benches for `PB1`–`PB5` plus regression checks for `B1` (cold start), `B9` (memory on the `Embedded10k` workload, using `--sheet embedded-10k`), `UB10` (save), `AB2` (preview of a `Move` on `10k`, fork included, with the larger `StoredPart`) and `INV7` (warm paint allocation-free); set to `needs-human` for the owner's Orion run (`HP1`).
- Done when: Orion results under canvas plan section 2.1 are recorded with raw data and hardware snapshots; all `PB1`–`PB5` budgets hold; the regression checks the campaign runs also pass: `B1`, `B9`, `UB10`, `AB2` (as specified: `preview` of a 100-part `Move` on `10k`, fork included, p99 < 25 ms, with peak extra memory recorded against `B9`) and `INV7`; and the owner accepts `HP1`.

---

## Log

- **2026-11-20 · P0.1 · done:** Wrote [parts-plan.md](parts-plan.md) and [parts-roadmap.md](parts-roadmap.md) against `main` (`07e96c6`). Recorded owner decisions `PD1`–`PD3` (TOML + shared `models/` tree, `.opart`/`.olib` ZIP containers, deduplicated `.omnia` embedding with lean primitives). Scanned both files for third-party company or commercial product names (AGENTS rule 1): none found. Docs only; nothing measured; no dependency added.
- **2026-11-20 · P0.2 · done (`HP0`):** Owner signed off on `PD4`–`PD11` (all `DECIDED`) and accepted defaults for `PQ1`–`PQ8`. Applied `parts-plan.md` section 14 cross-links to `AGENTS.md`, `ROADMAP.md`, `canvas-plan.md`, `ui-plan.md`, `mcp-plan.md` and `docs/file-format-proposal.md`. Rule-1 scan passed across all touched files. Docs only; nothing measured; no dependency added.
- **2026-11-23 · P0.1/P0.2 · done (amended, owner-delegated):** Updated pull request #85 onto `main` (`9d4b844`). Resolved `AGENTS.md` (simulation and parts registrations both kept; `HP*` added to hard rule 4) and `ROADMAP.md` (UI, simulation and parts links; Stage 3 cites parts for definitions and bindings and simulation for lowering; the stale "engine and execution plan still needed" sentence dropped). Fixed the independent review of #85: (1) `PD8` amended — `Part` unchanged with `def`/`params` on `StoredPart`, never-renumbered definition ids, interning against referenced records only, exact rollback on refusal, persistent vs live equality, `PINV4`/`PINV7` restated, finite reals keep `Eq` (`PD6`), new `PTC-def-lifecycle` and `PTC-batch-intern-refused`; (2) buildable migration — `P2.1` changes nothing public, `P2.2` owns every `PlacePart`/`Change`/`match` site, `P2.3` owns the v3 writer, v2 reader, agent file behaviour and version fixtures, goldens and `SymbolId`s stay unchanged; (3) one `PD10` validator on v3 load, `PlacePart` and `UpdatePartDef`, explicit stored-index → pin number → terminal mapping (`PD5`, `PINV11`, `PTC-pin-order`), `PD2` refuses custom symbols per `PQ3`, `PD3` separates deck preservation from hashing, `PD9` defines verified hashes and provenance; (4) generic op-amp and diode fixtures replace commercial part numbers; (5) `P0.3.md`, `UpdatePartDef`, `mount_opart`, `part_defs()`, `place_command`, one no-connect encoding, metadata-only `PB3` indexing, `PB4`/`PB5` bench in new task `P2.5`, CI step owned by `P1.4`, model licence policy `PK11` separate from `deny.toml`, and section 14 matched to the edits actually made. Aligned with cross-plan decisions XD1–XD9 (coordinator, owner-delegated, 2026-11-23), copied into `parts-plan.md` section 6: `PK4` parser in `omnia-doc` `units.rs` (`XD2`), `PK12` shared parameter names (`XD1`), generic op-amp default binding (`XD3`), "definitions and bindings" in place of netlist wording (`XD4`), pin role vs symbol class and `pins.toml` editing order (`XD5`), `Change::modified` and the `omnia-doc` file order (`XD7`). Rule-1 scan of every touched file: no company named; one brief compatibility nod to a diode class in `PD11`. Docs only; nothing measured; no dependency added.
- **2026-11-23 · P0.1/P0.2 · done (re-review of #85, owner-delegated):** Fixed the re-review of head `4bee8d5`. (1) No lossy save between `P2.2` and `P2.3`: `save` refuses a document with non-unmodelled definitions or overrides until the v3 writer lands (`PD8`, `PTC-no-lossy-save`; refusal added by `P2.1`, removed by `P2.3`). (2) `PTC-def-lifecycle` now branches from the post-delete state, so allocation across save/reload and undo are checked separately. (3) One instance validator, `validate_instance`, on v3 load, `PlacePart`, `SetParam` and `UpdatePartDef`: override names, kinds, bounds and canonical form; local pins equal to the definition's in order; part geometry comes from the definition; `SymbolMismatch` in `diff_embedded` (`PD8`, `PD9`, `PINV12`, `PTC-instance-refuse`). (4) `P2.5` depends on `P1.5` (owner of the bench crate's `omnia-parts` dependency) and builds 50 distinct records deterministically from the built-ins whose symbols occur on the stress sheet. Nits: `P1.4 -> S1.1` added to the dependency overview; `mcp-plan.md` marks `list_parts` as a read operation. Follow-up: the stress sheets back certified Orion budgets (`T2.7`, `T3.4` and others), so a log note is not enough: `P1.4` now owns `crates/omnia-bench/src/generate.rs` to pin the generator to today's six `SymbolId`s before adding symbols, with a format-independent fingerprint test of every existing sheet as a done-when check; new symbols go into new named sheets (`PD11`). Rule-1 scan of the changed text: clean. Docs only; nothing measured; no dependency added.
- **2026-11-23 · P0.1/P0.2 · done (third review of #85 and sim S0.2 items, owner-delegated):** Fixed the review of head `acc1eb7`. (1) `P2.1` owns the definition-arena rollback in `apply`/`apply_batch` (an existing `PlacePart` already interns an unmodelled definition) and switches the `INV4` transaction property and other save/load equality snapshots in `command.rs` tests to in-memory snapshots, keeping v2 byte-compatibility and the save refusal as separate tests (`PD8`). (2) Every real must satisfy `parse_value(raw, unit) == value` in `validate` and `validate_instance`; `Part::value` must match the resolved primary parameter; `UpdatePartDef` refreshes `Part::value` from the new default when there is no override (`PD6`, `PD8`, `PTC-instance-refuse` (e)/(f), `PTC-update-value`). (3) Unknown symbol ids: `SymbolLibrary::try_get`/`check_doc` (`P1.4`), called by the agent file host (`P2.3`) and File > Open, and `diff_embedded` reports `UnknownSymbol` (`PTC-unknown-symbol`). (4) `INV2` was wrong: the warm-paint allocation invariant is `INV7`; `P3.1` adds `AB2` and `B9`. Nits: the stress-sheet fingerprint covers stored pin coordinates and local bounds; `PTC-def-lifecycle` branch B replays place and delete. From the sim `S0.2` alignment (pull request #104): `PK4` defines unit tokens, whitespace, `F` farad vs `f` femto, `Ω`/`ohm` and `%`; `PK6` adds a one-terminal `rail` primitive with `PK12` `dc` as its voltage; `XD7` order is now `P2.1`–`P2.3`, then `C1.1`, since `S1.1` no longer owns `omnia-doc` files. Rule-1 scan of the changed text: clean. Docs only; nothing measured; no dependency added.
- **2026-11-23 · P0.1/P0.2 · done (fourth review of #85 and cross-plan requests, owner-delegated):** Fixed the review of head `66f9e49`. (1) `P2.3` depends on `P1.4` for `SymbolLibrary::check_doc`; the File > Open handoff to `U2.3` (whichever ships second wires it) is recorded in section 14 and `ui-plan.md`. (2) `UpdatePartDef` sets `Part::value` from `to`'s resolved primary (override, else default) in every case, including a primary that moves to an already-overridden parameter (`PD8`, `PTC-update-value`). (3) `P2.5` sets each fixture part's value from its definition's resolved primary and uses the same text in the unmodelled baseline. (4) No change: rail definitions stay with `S1.1`. (5) `P3.1` done-when requires its regression checks (`B1`, `B9`, `UB10`, `AB2` as specified, `INV7`) to pass. Nit: `P2.1`'s refusal test asserts history separately. Cross-plan: (A) `[sim.supply]` / `SupplyDecl` declares supply terminals and an optional implicit-supply fallback, read by sim lowering and `ERC008` (`PD5`); (B) `Catalog::place_symbol` embeds a symbol's default definition when one exists, and the U4.2/U5.2 placement and the agent's `place_part` go through it (`PD11`, section 14); (C) the `omnia-doc` file order is `P2.1`–`P2.3`, then `C1.9`, then `C1.1` (`XD7`). Rule-1 scan of the changed text: clean. Docs only; nothing measured; no dependency added.
- **2026-11-23 · P0.1/P0.2 · done (review of #85, owner-delegated):** Updated from main (0507b1c) and applied the review of head 136a2dd under cross-plan decisions XD1–XD9 (coordinator, owner-delegated, 2026-11-23). P2.3 and U2.3 now assign the File > Open symbol check and UTC-open-bad version bump to the task that merges second (PD8, XD1). T5.3 owns the placement-provider hook. U5.2 installs the catalog provider and tests the committed op-amp binding. The agent adoption task places and applies a supplied primary value in one transaction, with a 4.7k resistor and refusal tests (PD11, PK4, XD2, XD3). P2.5 owns Embedded10k and the scene selector for the B9 regression check. P3.1 waits for the UB10 and AB2 harnesses (U2.8 and A1.10, XD9). Rule-1 scan passed. Documentation only. No budgets measured.
- **2026-11-23 · P0.1/P0.2 · independent review of `1d162f9` (coordinator fixes, owner-delegated):** (1) `place_part` with a value cannot be a place-then-`SetParam` batch (`apply_batch` is private and the new `PartId` is unknown before `apply`): `PlacePart` now carries its initial override `params`, checked by `validate_instance`, and `Catalog::place_symbol` takes `value: Option<&str>` (empty = default, equal = no override, no primary parameter = refused, unmodelled = legacy text); the agent row and `mcp-plan.md` follow. (2) The handoffs are now in the roadmaps that own them: UI `U2.3` (conditional `check_doc`, `PTC-unknown-symbol` app test, `UTC-open-bad` 3→4), UI `U5.2` (depends on `P2.4`, installs the provider, op-amp default-binding test) and canvas `T5.3` (provider hook on `SheetCanvas` in `input.rs`). Nits: `Embedded10k` takes `seed` and has `revision == 0`; `P2.5` owns the `USAGE` line; `P1.5` adds `omnia-parts` under `[dependencies]`; the picker row names `U5.2`.
- **2026-11-23 · P0.1/P0.2 · done (second independent review of the `1d162f9` round; coordinator fixes, owner-delegated):** `P2.5`'s baseline keeps its `revision` (only the undo history is cleared, by a `save`/`load` round trip; no public API resets `revision`); `P2.2` adds `params: Params` to `PlacePart`, stores it, validates it and tests a valid and a refused initial override, and its mechanical updates add `params: Params::new()`; `canvas-plan.md` section 9 and the §5 diagram show `params`; `P1.4` owns new picker catalog rows; §14 rows recorded in the owning roadmaps are no longer marked as requests. Rule-1 scan clean; documentation only.
- **2026-11-25 · P0.3 · done (`PS1`):** Prototyped the line-oriented `.model`/`.subckt` extractor and a metadata-only `.olib`/folder indexer as a throwaway Rust package outside the workspace (`docs/measurements/P0.3/spike/`, crates already approved in plan section 11 at the workspace lock's versions; no product dependency added). Extraction: 20 spike decks written for this task (`CC0-1.0`, generic values, no commercial part numbers) give 24/24 extractions as specified and 15/15 hostile fixtures refused with the expected error; every output passes the embedded `PK10` screen and re-extracts to itself. One fixture expectation was a hand miscount (`NPN_GENERIC` 7 → 6 lines) and was corrected in the open. Twelve grammar points recorded for `P1.2` in `docs/measurements/P0.3.md`, chiefly top-level `.param`/`.func` closure, scoped resolution (local model shadowing a top-level one), positional model/subcircuit reference checks and `.lib <file> <section>` flattening; three archive notes for `P1.3`. `PB3` (Windows development machine, battery, Silent scheme, i7-14650HX capped at 2.2 GHz; certifies nothing): 500-part `.olib` median run p99 19.82 ms (range 17.57–40.76), in-memory 15.79 ms, unpacked folder 60.37 ms (range 50.59–102.79; per-file open cost with real-time scanning dominates; first call 2.29 s), 10 rotating repetitions × 200 calls, raw CSV and conditions kept. Verdict: extractor yes; Orion index time not measured here (`run.sh` makes it one command); `PB3` is certified on Orion by `P3.1` (`HP1`), and the Windows folder result is a risk for `P1.5`. No plan constant or decision changed. Rule-1 scan of every new file: clean (hardware named as in earlier measurements; process names hashed).
- **2026-11-25 · P0.3 · done (review of #133 addressed):** The independent review asked for changes, which are made. (1) `spice.rs` now skips reference names by position, not value, so an element or definition sharing a name with its top-level model, subcircuit or `.param` keeps the dependency; four `same_name_*`/`param_*` tests are added. (2) Every include path is checked under `PK10`, including those in unselected `.lib` sections; hostile fixture `h16` and a test are added. After both fixes the 24 extractions are byte-identical to the campaign and 16/16 hostile fixtures are refused (`results/runs/review-fix-extract/`). (3) `run.ps1`, `run.sh` and `extract` write each run to a fresh `results/runs/` directory and never overwrite the committed campaign. (4) `P0.3.md` gains gaps 13–14 for `P1.2` and `P1.3` notes: the 8 MiB cap is missing on the reader path, and `PK7` is only partly applied (no parameter-string or inline-SPICE caps). It also clarifies that the index calls do no content hashing, gives the process count as 428 → 437, and records binary provenance (Windows release builds here are not byte-reproducible). The timing code and numbers are unchanged. The PR footer naming the authoring tool is kept, under the owner's 2026-11-23 attribution exception to rule 1. Merged `origin/main` (`a022e6a`).
- **2026-11-25 · P0.3 · done (re-check of #133 addressed):** Two findings fixed. (1) `.param` and `.subckt` header assignments are now found only at nesting depth 0 and never in `==`/`!=`/`<=`/`>=`, so `GAIN={ LIMIT>=1 }` keeps `LIMIT` as a dependency and defines no bogus parameter; two tests are added. (2) The run scripts and `extract` claim their output directories atomically (`mkdir` without `-p`, `New-Item` without `-Force`, `fs::create_dir`), so concurrent or interrupted runs cannot collide. There are 14/14 prototype tests, the 24 extractions are byte-identical to the campaign, and 16/16 hostile fixtures are refused. Timing code and numbers are unchanged.
