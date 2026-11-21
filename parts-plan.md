# Component definitions and simulation models: plan (reference)

This file is the reference for Omnia's component data, part libraries, custom part containers
(`.opart` / `.olib`), simulation model bindings, and how placed parts and their definitions are
embedded in `.omnia` documents. The work itself is in [parts-roadmap.md](parts-roadmap.md),
which lists tasks that cite the ids defined here.

It sits beside [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md) and [ui-plan.md](ui-plan.md), and re-plans nothing they decide.
Every unprefixed id (`B`, `K`, `G`, `D`, `INV`, `Q`, `T`) is defined in the canvas plan;
`W`-prefixed ids in the wiring plan; `A`-prefixed ids in the agent plan; `U`-prefixed ids in
the UI plan. Section 14 lists the changes this plan makes to those files; task P0.2 applied
them after owner sign-off at `HP0` (2026-11-20).

Written 2026-11-20, against `main` at `07e96c6`. No `omnia-parts` code exists yet.

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `PB` | Performance or quality budget, a hard requirement | 3 |
| `PK` | Constant: a limit or convention every part crate must agree on | 4 |
| `PD` | Decision | 6 |
| `PINV` | Invariant: a rule that must hold in code, each with a test | 7 |
| `PTC` | Scenario with its expected result | 8 |
| `PS` | Spike: a measurement or prototype that settles an unknown before build | 9 |
| `PQ` | Open question for the owner, each with a default | 13 |
| `HP` | Human checkpoint | [parts-roadmap.md](parts-roadmap.md) |
| `P0.1`, `P1.1`… | Task | [parts-roadmap.md](parts-roadmap.md) |

Status words follow the other plans:

- **DECIDED**: settled by the owner (dated where decided in review).
- **PROPOSED**: written by the plan author. Becomes DECIDED, or is amended, at `HP0`. Build
  nothing from a PROPOSED entry before `HP0`.
- **HYPOTHESIS**: build only after the named spike confirms it.
- **DEFAULT**: open question with a default; build to the default until the owner's dated answer
  replaces it.

SPICE, `ngspice`, `RIS-C`, IEEE, IEC, TOML, RON, ZIP and SHA-256 are named here as open
standards, formats or engines already referenced in [ROADMAP.md](ROADMAP.md). No other company
or commercial product is named (AGENTS rule 1).

## 1. Scope

In scope:

- **Component definitions (`part.toml`).** Structured metadata (id, name, category, description,
  tags, datasheet URL, licence), symbol binding, pin-to-node mapping, electrical pin roles,
  typed parameter schemas with units and defaults, and simulation bindings.
- **Simulation model storage.** Untouched SPICE `.lib` / `.mod` files in a shared `models/` tree
  for the built-in library, plus native primitive bindings that require no SPICE deck.
- **Portable custom containers.** `.opart` (single part) and `.olib` (multi-part library pack)
  as standard ZIP archives using the exact same inner layout and loader as the repository's
  unpacked library.
- **Build-time compilation (`cargo xtask parts`).** Validates every built-in part against
  `omnia-symbols` and `library/models/`, checks licences and SPICE port counts, and compiles
  the built-in catalog into static Rust tables inside `omnia-parts`.
- **Document embedding (`omnia-doc`).** A deduplicated `PartDefTable` inside `Doc` and `.omnia`
  files, per-part parameter overrides, provenance (`source_id` + SHA-256 hash), explicit
  library-update commands, and backward loading of RON v2 test sheets.
- **Netlist-ready read API.** Deterministic extraction of a placed part's resolved SPICE card,
  model/subcircuit text, and pin-to-net bindings for Stage 3 simulation engines.
- **Archive and model safety.** Strict ZIP limits, path validation, and SPICE directive
  screening when importing untrusted `.opart` / `.olib` files.

Out of scope:

- Simulation execution, solver architecture, engine choice (`ngspice` vs `RIS-C`), analyses,
  convergence diagnostics, waveforms and instruments (Stage 3 simulation plan).
- Full library browser, parametric search and property inspector UI (extends `ui-plan.md` once
  the data model lands; this plan provides the query and edit APIs).
- PCB footprints, 3D packages, BOM pricing, supply-chain links and hierarchical sub-sheets.
- Switching `.omnia` from RON to a binary container (tracked separately in
  `docs/file-format-proposal.md`; this plan defines the logical records and RON v3, which map
  directly onto binary sections if that proposal is accepted).

## 2. Repository facts this plan rests on

Checked at `07e96c6` (workspace root).

| Fact | Where | Consequence |
| --- | --- | --- |
| `Part` holds only `symbol: SymbolId`, `at`, `orient`, `designator: Arc<str>`, `value: Arc<str>` | `crates/omnia-doc/src/doc.rs` | No structured parameters, model refs or part-definition ids exist in `Doc` yet. |
| `Doc::save` / `Doc::load` write RON `version: 2`, embedding `local_pins` and `local_bounds` per part with `#[serde(deny_unknown_fields)]` | `crates/omnia-doc/src/io.rs` | Embedding geometry in `.omnia` is already the pattern; bumping to `version: 3` keeps strict validation while letting v2 fixtures keep loading. |
| `Doc::apply`, `undo` and `redo` are the only public mutators; `apply` is transactional | `crates/omnia-doc/src/command.rs` | Parameter edits and part-definition updates must be `Command` variants so INV4 and undo/redo hold unchanged. |
| `omnia-symbols` compiles `pins.toml` + 116 vendored SVGs via `cargo xtask symbols` into `generated.rs`; 6 symbols are currently defined in `pins.toml` | `crates/omnia-symbols/src/lib.rs`, `pins.toml` | `omnia-parts` can mirror the same `xtask` codegen pattern and stay GPUI-free. |
| `omnia-doc` has no UI dependency; `omnia-symbols` depends on `omnia-doc` | `crates/omnia-doc/Cargo.toml`, `crates/omnia-symbols/Cargo.toml` | `omnia-parts` depends on `omnia-doc` and `omnia-symbols` (no GPUI); `omnia-doc` stays independent of `omnia-parts` and `omnia-symbols`. |
| `toml = "1"`, `zip = "9"` (`deflate-flate2-zlib-rs`) and `sha2` are already in `Cargo.lock` and pass `cargo deny check` | `xtask/Cargo.toml`, `Cargo.lock`, `deny.toml` | Zero new third-party dependencies are required for TOML parsing, `.opart`/`.olib` archives or content hashing. |
| `ui-plan.md` UD19 reserves `SimState` and disables Part Libraries until a library plan exists | `ui-plan.md` | UI integration can plug directly into existing shell slots once `omnia-parts` exists. |

## 3. Budgets

Measured on the reference machine under canvas plan section 2.1 (Orion laptop, 10 interleaved
repetitions, median of run p99s). All canvas (`B1`–`B10`), wiring (`WB1`–`WB7`), agent
(`AB1`–`AB3`) and UI (`UB1`–`UB10`) budgets still hold; these are added. `PB1`–`PB5` are
chosen targets, to be verified on Orion.

| ID | Budget | Measured by |
| --- | --- | --- |
| PB1 | Built-in catalog init (up to 500 compiled parts): < 1 ms, 0 filesystem reads; `B1` (< 400 ms cold start) holds | `crates/omnia-bench/benches/parts.rs` |
| PB2 | Open, verify and load one `.opart` archive (up to 1 MiB uncompressed): p99 < 5 ms | same bench |
| PB3 | Index a user `.olib` or folder containing 500 parts (metadata + hash, lazy model read): p99 < 50 ms off the UI thread | same bench |
| PB4 | `Doc::save` and `Doc::load` on a `10k` sheet with 50 unique embedded `PartDef`s (mix of primitives, `.model` and `.subckt`): `save` stays within `UB10` (< 33.3 ms UI-thread serialise); `load` adds < 5% over the unmodelled `10k` baseline | `bench file --sheet 10k --embedded-defs 50` |
| PB5 | `Doc::apply(Command::SetParam { .. })` on `10k`: p99 < 0.05 ms; canvas pan/zoom stays zero-allocation (`INV2`) | criterion bench in `omnia-doc` + `INV2` check |

## 4. Constants

| ID | Constant | Value |
| --- | --- | --- |
| PK1 | Part manifest schema version | `schema = 1`. Unknown versions are refused with a clear error. |
| PK2 | Canonical part id | ASCII `^[a-z0-9][a-z0-9._-]{0,63}$` (1 to 64 chars, lowercase). Pack-qualified ids use `<pack>/<id>`. |
| PK3 | Archive safety limits (`.opart` / `.olib`) | Max compressed file: 8 MiB (`AK2`). Max uncompressed total: 32 MiB. Max single entry: 2 MiB. Max entries: 1,024. Max expansion ratio: 100:1. |
| PK4 | Engineering value suffixes | Case-sensitive SI by default for user-facing parameters (`f`, `p`, `n`, `u`/`µ`, `m`, `k`/`K`, `M`, `G`, `T`) plus explicit SPICE `meg`/`MEG` (= `1e6`). See `PQ2` for the `M` vs `m` policy; netlist emission always writes unambiguous SPICE notation (`1e6`, `1e-3` or `meg`). |
| PK5 | Content hash | Lowercase hex SHA-256 (64 chars) over canonicalised part definition bytes (sorted fields, LF line endings, resolved model text if non-primitive, custom symbol bytes if embedded). Independent of ZIP timestamps, file order or OS line endings. |
| PK6 | Built-in primitive kinds | `resistor`, `capacitor`, `inductor`, `potentiometer`, `vsource`, `isource`, `vcvs`, `cccs`, `vccs`, `ccvs`, `ground`, `switch`. |
| PK7 | Field size caps | At most 64 parameters per part definition; parameter string max 256 bytes; description max 2,048 bytes; at most 64 tags (32 bytes each); at most 256 pins per part. |
| PK8 | Container extensions | `.opart` (single-part ZIP archive), `.olib` (multi-part pack ZIP archive), `.omnia` (schematic document). |
| PK9 | Document RON version | `version: 3` adds the deduplicated `part_defs` table and per-part `def` + `params` overrides. `version: 2` test sheets still load (PD8). |
| PK10 | Disallowed SPICE directives in untrusted models | `.control`, `.endc`, `shell`, `system`, `osdi`, `pre_osdi`, `codemodel`, `.csdf`, and any `.include` / `.lib` path containing `..`, a drive prefix, leading `/` or `\` or referencing a file outside the archive. |

## 5. Architecture

```
  Repo built-ins (git-tracked, unpacked)        User / shared libraries
  library/                                      *.opart (1 part ZIP)
    parts/<category>/<id>.toml                  *.olib  (pack ZIP)
    models/<family>/<file>.lib                  user folder (unpacked)
              |                                           |
     [cargo xtask parts]                                  |
     validate pins, models, licences                      |
     emit static catalog + hashes                         |
              |                                           v
  +-----------v-------------------------------------------v-----------+
  | omnia-parts (library; no GPUI)                                    |
  |   schema.rs     part.toml + pack manifest DTOs (PK1, PK2, PK7)    |
  |   units.rs      engineering value parser & SPICE formatter (PK4)  |
  |   spice.rs      .model/.subckt extractor & PK10 safety scanner    |
  |   container.rs  LibrarySource trait: Directory | ZipArchive (PK3) |
  |   hash.rs       canonical SHA-256 content hash (PK5)              |
  |   catalog.rs    static built-ins + mounted user packs + queries   |
  |   embed.rs      PartDef -> omnia_doc::EmbeddedPartDef conversion  |
  +-----------+-------------------------------------------+-----------+
              |                                           |
              | EmbeddedPartDef + initial params          | netlist cards
              v                                           v
  +-------------------------+                 +-----------------------+
  | omnia-doc (no GPUI)     |                 | Stage 3: omnia-sim    |
  |   PartDefTable (dedup)  | --------------> |   ERC + netlister     |
  |   Part { def, params }  |  reads Doc +    |   ngspice / RIS-C     |
  |   Command::SetParam     |  PartDefTable   +-----------------------+
  |   Command::UpdateDef    |
  |   RON v3 save / load    |
  +-------------------------+
              |
              v
  omnia-canvas (reads Part::designator & Part::value unchanged)
  omnia-app    (part picker, inspector, update-from-library banner)
  omnia-agent  (typed part placement & param edits via omnia-doc)
```

Lane separation (AGENTS.md) stays strict:
- `omnia-doc` never depends on `omnia-parts`, `omnia-symbols`, `toml`, `zip` or GPUI. It stores
  plain, self-contained `EmbeddedPartDef` records inside the document.
- `omnia-parts` depends on `omnia-doc` and `omnia-symbols`, never on GPUI.
- `omnia-canvas` keeps reading `Part::designator` and `Part::value` on the hot render path; it
  never inspects `PartDefTable` or parses TOML/SPICE while drawing.

## 6. Decisions

### PD1 Built-in library layout: one TOML per part + shared `models/` tree. DECIDED (owner, 2026-11-20)

- The repository stores built-in parts unpacked under `library/`:
  - `library/parts/<category>/<id>.toml`: one file per concrete part (e.g. `diodes/1n4148.toml`,
    `opamps/tl072.toml`) and one file per generic parametric primitive (e.g.
    `passives/resistor.toml`, `passives/capacitor.toml` — never one file per resistance value).
  - `library/models/<family>/<name>.lib`: standard SPICE `.model` / `.subckt` files kept
    verbatim with their original headers and licence notices.
- Multiple part `.toml` files may reference different entries in the same `.lib` file (for
  example a family deck covering several related devices), avoiding duplicated SPICE text and
  keeping third-party licences cleanly separated from Omnia's metadata.

### PD2 Portable containers: `.opart` and `.olib` ZIP archives with identical inner layout. DECIDED (owner, 2026-11-20)

- A single custom or shared part is packaged as `<name>.opart`, a standard ZIP archive (Deflate
  or Stored) containing the exact same files an unpacked part uses:

  ```text
  tl072.opart (ZIP)
    part.toml        # required; schema = 1 (PK1)
    model.lib        # optional; required if [sim] references a local file
    symbol.svg       # optional; custom symbol artwork if not using a built-in SymbolId
    pins.toml        # required iff symbol.svg is present (same format as omnia-symbols)
    LICENSE          # optional; preserved and shown in part info
  ```

- A multi-part library pack is `<name>.olib`, a ZIP archive with `manifest.toml` at its root
  plus `parts/<category>/<id>.toml` and `models/...` — mirroring `library/` directly.
- One internal `LibrarySource` trait reads both unpacked directories and ZIP archives, so built-in
  parts, local user folders, `.opart` files and `.olib` packs go through the exact same parser,
  validator and hasher.

### PD3 Document embedding policy: deduplicated embed-all, lean primitives. DECIDED (owner, 2026-11-20)

- Every unique part definition used by at least one `Part` on a sheet is embedded once in a
  sheet-level `PartDefTable` inside `Doc` and serialised into `.omnia`.
- Each embedded entry records:
  - Its local `PartDefId(u32)` within the document.
  - Provenance: `source_id: Arc<str>` (e.g. `"omnia:tl072"` or `"custom:tl072"`) and
    `content_hash: [u8; 32]` (`PK5`).
  - Metadata needed offline (name, description, category), pin-to-node map, parameter schema
    (names, types, units, defaults, primary parameter), and simulation binding.
- **Lean primitives:** when the simulation binding is a built-in primitive (`PK6`, such as an
  ideal resistor, capacitor, voltage source or ground), **no SPICE deck is embedded**; the
  netlister emits the engine's native element card from the binding kind, pins and parameters.
- **Model/subcircuit parts:** when the binding is `model` or `subckt`, the **exact resolved
  SPICE definition** (`.model` card or `.subckt`…`.ends` block, plus any transitive local
  `.model`/`.subckt` dependencies inside the same `.lib`) is embedded verbatim once in the
  `PartDefTable`.
- Consequence: any `.omnia` file opens, edits and simulates identically on another machine with
  no external library installed.

### PD4 `part.toml` schema. DECIDED (owner, 2026-11-20)

Two concrete examples define the full v1 surface:

**Generic primitive (`library/parts/passives/resistor.toml`):**

```toml
schema = 1
id = "resistor"
name = "Resistor"
category = "Passives"
description = "Ideal linear two-terminal resistor"
tags = ["passive", "resistor", "linear"]
designator_prefix = "R"
primary_param = "R"

[symbol]
builtin = "Resistor-IEEE-Standard"

[[pins]]
number = "1"
name = "a"
role = "passive"

[[pins]]
number = "2"
name = "b"
role = "passive"

[params.R]
kind = "real"
unit = "ohm"
default = "10k"
min = "1e-6"
label = "Resistance"

[params.tc1]
kind = "real"
unit = "1/K"
default = "0"
label = "Linear tempco"
optional = true

[sim]
kind = "primitive"
primitive = "resistor"
pins = ["1", "2"]
```

**Concrete subcircuit part (`library/parts/opamps/tl072.toml`):**

```toml
schema = 1
id = "tl072"
name = "TL072"
category = "Op-Amps"
description = "Low-noise JFET-input operational amplifier"
tags = ["opamp", "jfet", "analog"]
designator_prefix = "U"
datasheet = "https://example.invalid/tl072-datasheet"

[symbol]
builtin = "IC-COM-OpAmp"

[[pins]]
number = "3"
name = "in+"
role = "input"

[[pins]]
number = "2"
name = "in-"
role = "input"

[[pins]]
number = "8"
name = "v+"
role = "power"

[[pins]]
number = "4"
name = "v-"
role = "power"

[[pins]]
number = "1"
name = "out"
role = "output"

[sim]
kind = "subckt"
file = "models/opamps/tl07x.lib"
entry = "TL07X"
# Ordered to match `.subckt TL07X in+ in- v+ v- out`
pins = ["3", "2", "8", "4", "1"]

[license]
spdx = "BSD-3-Clause"
source = "models/opamps/tl07x.lib"
```

Rules enforced by the schema parser (`#[serde(deny_unknown_fields)]`):
- `[sim].kind` is one of `"primitive"`, `"model"`, `"subckt"`, or `"none"` (mechanical or
  annotation-only parts that emit no netlist card).
- When `kind = "model"`, `[sim]` specifies `spice_prefix` (e.g. `"D"`, `"Q"`, `"M"`), `file`,
  `entry` (the `.model` name), and `pins` in SPICE terminal order.
- `[sim].pins` must list a permutation of the declared `[[pins]]` numbers (or a valid subset for
  no-connect pins marked `no_connect = true`).
- For single-part `.opart` archives, `file = "model.lib"` resolves relative to the archive root;
  an inline `spice = """..."""` field is also permitted only when `file` is omitted (PQ5).

### PD5 Pin mapping and electrical roles. DECIDED (owner, 2026-11-20)

- `[[pins]]` is the single bridge between visual artwork (`omnia-symbols` `Pin::number`) and
  electrical meaning.
- Every pin declared in `part.toml` must match a `Pin::number` in the referenced symbol; missing
  or extra pin numbers fail validation.
- Electrical roles (`passive`, `input`, `output`, `bidir`, `power`, `ground`, `no_connect`) are
  stored in the embedded definition so Stage 3 ERC can check floating inputs, shorted outputs and
  missing ground without touching external libraries.

### PD6 Typed parameters and `Part::value` synchronisation. DECIDED (owner, 2026-11-20)

- Supported parameter kinds: `real` (parsed `f64` + original engineering text or canonical
  formatted string, with optional `unit`, `min`, `max`), `int` (`i64`), `bool`, `enum` (closed
  list of string variants), `text` (bounded by `PK7`).
- Parameters live in a deterministic sorted map (`BTreeMap<Arc<str>, ParamValue>`) on `Part`,
  storing **only overrides** that differ from the `EmbeddedPartDef` defaults. Unmodified parts
  store an empty map (minimal memory and file size).
- **Canvas value sync:** if the definition declares `primary_param` (e.g. `"R"` on a resistor),
  `Command::SetParam` on `primary_param` updates `Part::value` in the same transaction; and
  `Command::SetText { field: TextField::Value, .. }` parses and updates `primary_param` if valid
  (refusing with `CommandError::InvalidParam` if out of bounds or unparseable, or falling back
  to display text for unmodelled v2 parts). `omnia-canvas` keeps rendering `Part::value` with
  zero changes.

### PD7 Build-time compilation of built-ins (`cargo xtask parts`). DECIDED (owner, 2026-11-20)

- `cargo xtask parts` reads `library/parts/**/*.toml` and `library/models/**/*` in deterministic
  lexicographic path order, validates every definition against `omnia-symbols` and `spice.rs`,
  extracts the minimal self-contained SPICE block for each `model`/`subckt`, computes its `PK5`
  SHA-256 hash, and generates `crates/omnia-parts/src/generated.rs`.
- A CI check (`cargo xtask parts --check`) fails if `generated.rs` is stale or any part fails
  validation, mirroring `cargo xtask symbols`.
- At runtime, built-in parts are `&'static` data: zero filesystem access and zero TOML parsing
  at startup (`PB1`).

### PD8 `omnia-doc` integration and RON v2 compatibility. DECIDED (owner, 2026-11-20)

- `Doc` gains a private `part_defs: Arena<EmbeddedPartDef>` (referenced by `PartDefId(u32)`),
  deduplicated by `content_hash` on insertion.
- `Part` gains `pub def: PartDefId` and `pub params: BTreeMap<Arc<str>, ParamValue>` (or an
  immutable `Arc` wrapper if clone cost matters in `Doc::fork`).
- New `Command` variants (all transactional and undoable through `Doc::apply`, `INV4`):
  - `PlacePart` carries an `EmbeddedPartDef` (or existing `PartDefId`); if the hash already
    exists in `part_defs`, `Doc` reuses that slot instead of duplicating it.
  - `SetParam { part: PartId, name: Arc<str>, value: Option<ParamValue> }` (`None` resets to the
    definition's default).
  - `UpdatePartDef { from: PartDefId, to: EmbeddedPartDef }` atomically replaces a definition on
    all parts referencing `from` (validating pin count and parameter compatibility first; refused
    if any placed part's pin geometry or overrides become invalid).
- **Garbage collection:** unused `PartDef` slots stay intact during an editing session so `undo`
  of a deleted part restores without re-fetching from the library; `Doc::save` writes only
  definitions referenced by live parts (remapping deterministically on save, or preserving slots
  per PQ6).
- **RON v2 compatibility:** `Doc::load` still accepts `version: 2` files (all existing stress
  sheets and agent test fixtures) by synthesising one unmodelled `EmbeddedPartDef` per unique
  `(symbol, local_pins, local_bounds)` tuple (`sim: SimBinding::None`). `Doc::save` writes
  `version: 3` (with `save_v2` retained only if tests need it; existing v2 tests keep passing).

### PD9 Provenance and explicit library updates. DECIDED (owner, 2026-11-20)

- Opening a `.omnia` file **never** silently overwrites its embedded definitions from the local
  library.
- `omnia-parts` provides `Catalog::diff_embedded(&Doc) -> Vec<DefUpdateAvailable>`, comparing
  each embedded `(source_id, content_hash)` against the currently mounted libraries.
- Applying an update is an explicit user (or agent) action issuing `Command::UpdatePartDef`, so
  it is atomic, inspectable and fully undoable.

### PD10 Archive and SPICE security. DECIDED (owner, 2026-11-20)

- `.opart` and `.olib` files are treated as untrusted input:
  - Enforce `PK3` byte, entry-count and decompression-ratio caps while streaming entries, before
    allocating output buffers.
  - Reject any archive entry path with `..`, absolute root, Windows drive prefix, NUL byte,
    duplicate canonical path, or symlink.
  - Reject any non-UTF-8 `part.toml` or SPICE file.
- SPICE model text extracted from external `.opart` / `.olib` archives is screened against
  `PK10` before admission into a `Catalog` or `Doc`. Any `.include` or `.lib` directive must
  resolve strictly inside the same archive and is flattened during extraction so the embedded
  SPICE text in `.omnia` has zero external filesystem dependencies.

### PD11 Starter library scope. DECIDED (owner, 2026-11-20)

- The first built-in set covers the 6 symbols already in `crates/omnia-symbols/pins.toml` plus
  minimal sources needed for Stage 3 reference circuits (voltage divider, RC low-pass filter,
  diode clipper, Wien-bridge oscillator):
  - Primitives: `resistor`, `potentiometer`, `capacitor`, `ground`, `vsource-dc`, `vsource-ac`,
    `opamp-ideal` (ideal VCVS macro or primitive).
  - Modelled parts: `1n4148` (`.model`), `tl072` (`.subckt`, using an openly licensed model
    verified under `deny.toml` / `PQ7`).
- Additional symbols (`vsource`, `isource`, `inductor`, `bjt`, `mosfet`) are added to
  `pins.toml` in lockstep with their `part.toml` definitions.

## 7. Invariants

Each invariant has an automated test; the owning task id will be added in `parts-roadmap.md`.

| ID | Rule |
| --- | --- |
| PINV1 | `omnia-parts` has no GPUI dependency, and `omnia-doc` has no dependency on `omnia-parts`, `toml` or `zip`. Checked by `cargo tree` / workspace test. |
| PINV2 | **Determinism.** Loading the same library from files enumerated in any order (or unpacked vs `.olib` ZIP with different entry order/timestamps) produces identical `Catalog` order, identical `content_hash` (`PK5`) and byte-identical `xtask parts` output. |
| PINV3 | **Pin consistency.** Every built-in and loaded part's `[[pins]]` matches its symbol's `Pin::number` set exactly, and `[sim].pins` matches the `.subckt` port count or primitive arity. |
| PINV4 | **Self-contained `.omnia`.** Saving a `Doc` containing custom `.opart` parts and loading it in a process with no libraries mounted preserves every `PartDef`, parameter override, pin map and resolved SPICE block byte-for-byte (`load(save(doc)) == doc`). |
| PINV5 | **Deduplication.** Placing `N` instances of the same part definition creates exactly 1 `EmbeddedPartDef` entry in `Doc::part_defs` (matched by `content_hash`). |
| PINV6 | **Lean primitives.** An `EmbeddedPartDef` whose `SimBinding` is `Primitive(_)` or `None` stores zero SPICE deck bytes. |
| PINV7 | **Transactional edits (`INV4`).** Every `SetParam` and `UpdatePartDef` runs through `Doc::apply`; a refused edit (out-of-range param, unknown param name, pin mismatch) leaves `Doc`, `revision` and `part_defs` unchanged; `undo(apply(c)) == before` and `redo(undo(apply(c))) == after`. |
| PINV8 | **Archive safety (`PK3`).** No malformed, truncated, path-traversal, symlink or ZIP-bomb `.opart` / `.olib` file causes a panic or allocation beyond `PK3` caps; every such input returns a typed `LibraryError`. |
| PINV9 | **SPICE hygiene (`PK10`).** No untrusted `.opart` / `.olib` containing `.control`, shell escapes or out-of-archive `.include`/`.lib` is accepted; every embedded SPICE block has zero unresolved external file references. |
| PINV10 | **Built-in zero-I/O start (`PB1`).** Constructing `Catalog::builtin()` executes zero filesystem calls and parses zero TOML at runtime. |

## 8. Scenarios

Each scenario is an automated test with this expected result.

| ID | Steps | Expected |
| --- | --- | --- |
| PTC-builtin-load | Construct `Catalog::builtin()` and query `resistor`, `1n4148`, `tl072` | All present, valid `SymbolId`, pin counts match `omnia-symbols`, hashes match `PK5`, 0 allocations from filesystem I/O. |
| PTC-place-dedup | Place 50 `resistor` parts and 10 `tl072` parts via `Doc::apply` | `doc.part_defs_len() == 2`; resistor def carries no SPICE text (`PINV6`); `tl072` def embeds its resolved `.subckt` once (`PINV5`). |
| PTC-param-sync | Place `resistor` (default `10k`), apply `SetParam("R", "4.7k")`, then `SetText(Value, "2.2k")`, then `undo` twice | `Part::value` reads `"4.7k"`, then `"2.2k"`, then `"4.7k"`, then `"10k"`; `params` map is empty after second undo. |
| PTC-param-refuse | Apply `SetParam("R", "-5")` (below `min = 1e-6`) and `SetParam("bogus", "1")` | Both refused with `CommandError::InvalidParam`; `Doc` and `revision` unchanged (`PINV7`). |
| PTC-opart-roundtrip | Pack `tl072` directory into `tl072.opart`, load via `Catalog::load_opart`, compare against unpacked load | Identical `PartDef`, identical `content_hash` (`PINV2`), identical resolved SPICE block. |
| PTC-offline-omnia | Place custom `.opart` part, save `circuit.omnia`, drop `Catalog`, load `circuit.omnia` | Loads cleanly; all metadata, pin roles, param overrides and SPICE `.subckt` intact (`PINV4`). |
| PTC-library-update | Load `circuit.omnia` whose embedded `tl072` has an older hash; mount newer `tl072`; run `diff_embedded` + `UpdatePartDef` + `undo` | `diff_embedded` reports 1 update; `UpdatePartDef` updates hash and SPICE text on all instances; `undo` restores old hash and text. |
| PTC-hostile-opart | Feed `load_opart` a ZIP with `../escape.lib`, a 150:1 zero-filled bomb, and a `.lib` containing `.control / shell rm -rf` | All three refused with specific `LibraryError` variants before allocating past `PK3` (`PINV8`, `PINV9`). |
| PTC-v2-compat | Load existing RON v2 stress sheet (`1k`) through `Doc::load` | Succeeds; synthesises unmodelled `EmbeddedPartDef`s; all parts, wires, labels and nets match v2 load. |
| PTC-xtask-mismatch | Run `xtask parts --check` on a fixture where `[sim].pins` has 4 entries for a 5-port `.subckt` | Fails with a diagnostic naming the part file, `.subckt` name and expected vs actual port count (`PINV3`). |

## 9. Spikes

One small spike before building the parser and archive pipeline:

| ID | Question it settles | How |
| --- | --- | --- |
| PS1 | Does a lightweight line-oriented SPICE extractor reliably handle real `.model` / `.subckt` decks (line continuations `+`, nested `.subckt`, inline comments `*` / `;`, `PARAMS:` defaults, transitive local references) without pulling in a full SPICE parser? And what is actual `.olib` (500 parts) index time on Orion (`PB3`)? | Prototype `spice.rs` extractor on 20 representative public/open SPICE decks (diodes, BJTs, MOSFETs, op-amps with `PARAMS:` and nested subcircuits) and time a 500-part `.olib` index scan. Record results in `docs/measurements/P0.2.md`. |

## 10. Crates and APIs

New crate `crates/omnia-parts/` (no GPUI):

```rust
// ---- omnia-parts ----
pub struct PartIdSlug(Arc<str>);           // validated PK2 id
pub struct ContentHash(pub [u8; 32]);      // PK5 SHA-256

pub enum PinRole { Passive, Input, Output, Bidir, Power, Ground, NoConnect }

pub struct PinDef {
    pub number: Arc<str>,                  // matches omnia_symbols::Pin::number
    pub name: Arc<str>,
    pub role: PinRole,
}

pub enum ParamKind {
    Real { unit: Option<Arc<str>>, min: Option<f64>, max: Option<f64> },
    Int { min: Option<i64>, max: Option<i64> },
    Bool,
    Enum(Vec<Arc<str>>),
    Text,
}

pub struct ParamSpec {
    pub name: Arc<str>,
    pub label: Arc<str>,
    pub kind: ParamKind,
    pub default: ParamValue,
    pub optional: bool,
}

pub enum PrimitiveKind {
    Resistor, Capacitor, Inductor, Potentiometer,
    VSource, ISource, Vcvs, Cccs, Vccs, Ccvs, Ground, Switch,
}

pub enum SimBinding {
    None,
    Primitive { kind: PrimitiveKind, pins: Vec<Arc<str>> },
    Model { spice_prefix: char, entry: Arc<str>, pins: Vec<Arc<str>>, spice: Arc<str> },
    Subckt { entry: Arc<str>, pins: Vec<Arc<str>>, spice: Arc<str> },
}

pub struct PartDef {
    pub source_id: Arc<str>,
    pub content_hash: ContentHash,
    pub name: Arc<str>,
    pub category: Arc<str>,
    pub description: Arc<str>,
    pub tags: Vec<Arc<str>>,
    pub designator_prefix: Arc<str>,
    pub datasheet: Option<Arc<str>>,
    pub license: Option<LicenseInfo>,
    pub symbol: SymbolRef,                 // Builtin(SymbolId) | Custom(CustomSymbol)
    pub pins: Vec<PinDef>,
    pub params: Vec<ParamSpec>,            // sorted by name
    pub primary_param: Option<Arc<str>>,
    pub sim: SimBinding,
}

pub struct Catalog { /* static built-ins + mounted user folders / .opart / .olib */ }
impl Catalog {
    pub fn builtin() -> Self;                                      // PB1, PINV10
    pub fn mount_opart(&mut self, path: &Path) -> Result<PartIdSlug, LibraryError>;
    pub fn mount_olib(&mut self, path: &Path) -> Result<PackSummary, LibraryError>;
    pub fn mount_dir(&mut self, path: &Path) -> Result<PackSummary, LibraryError>;
    pub fn get(&self, source_id: &str) -> Option<&PartDef>;
    pub fn search(&self, query: &PartQuery) -> Vec<&PartDef>;      // deterministic order
    pub fn to_embedded(&self, def: &PartDef) -> omnia_doc::EmbeddedPartDef;
    pub fn diff_embedded(&self, doc: &omnia_doc::Doc) -> Vec<DefUpdateAvailable>;
}

// Packing helper for export & tests
pub fn pack_opart(dir: &Path, out_path: &Path) -> Result<ContentHash, LibraryError>;
pub fn pack_olib(dir: &Path, out_path: &Path) -> Result<PackSummary, LibraryError>;
```

Additions in `crates/omnia-doc/` (owned by the `doc` lane, no `omnia-parts` dependency):

```rust
// ---- omnia-doc ----
pub struct PartDefId(pub u32);

pub enum ParamValue {
    Real { raw: Arc<str>, si: f64 },       // preserves user notation + validated f64
    Int(i64),
    Bool(bool),
    Text(Arc<str>),
}

pub struct EmbeddedPartDef {
    pub source_id: Arc<str>,
    pub content_hash: [u8; 32],
    pub name: Arc<str>,
    pub category: Arc<str>,
    pub description: Arc<str>,
    pub designator_prefix: Arc<str>,
    pub symbol: SymbolId,                  // PQ3: custom art optional later
    pub local_pins: Vec<Pt>,
    pub local_bounds: Rect,
    pub pin_meta: Vec<EmbeddedPin>,        // number, name, role
    pub params: Vec<EmbeddedParamSpec>,    // sorted by name
    pub primary_param: Option<Arc<str>>,
    pub sim: EmbeddedSimBinding,           // None | Primitive | Model(spice) | Subckt(spice)
}

pub struct Part {
    pub symbol: SymbolId,
    pub def: PartDefId,
    pub at: Pt,
    pub orient: Orient,
    pub designator: Arc<str>,
    pub value: Arc<str>,
    pub params: BTreeMap<Arc<str>, ParamValue>, // overrides only
}

// New Command variants:
//   SetParam { part: PartId, name: Arc<str>, value: Option<ParamValue> }
//   UpdatePartDef { from: PartDefId, to: EmbeddedPartDef }
//
// New Doc queries:
//   pub fn part_def(&self, id: PartDefId) -> Option<&EmbeddedPartDef>;
//   pub fn part_defs(&self) -> impl Iterator<Item = (PartDefId, &EmbeddedPartDef)>;
//   pub fn resolved_param<'a>(&'a self, part: PartId, name: &str) -> Option<&'a ParamValue>;
```

`xtask` additions (`xtask/src/parts.rs`):
- `cargo xtask parts` — validates `library/` and regenerates `crates/omnia-parts/src/generated.rs`.
- `cargo xtask parts --check` — CI mode; fails if `generated.rs` differs or any part fails validation.
- `cargo xtask part pack <dir> <out.opart>` — packs and validates a `.opart` archive.

## 11. Dependencies

No new crates beyond what is already in `Cargo.lock` and approved in [deny.toml](deny.toml).

| Crate | Use | Licence | Status in repo |
| --- | --- | --- | --- |
| `toml` (`1`) | Parse `part.toml` and `manifest.toml` in `omnia-parts` and `xtask` | MIT OR Apache-2.0 | Already in `xtask/Cargo.toml` and `Cargo.lock` |
| `zip` (`9`, `deflate-flate2-zlib-rs` only) | Read and write `.opart` / `.olib` ZIP archives | MIT | Already in `xtask/Cargo.toml` and `Cargo.lock` |
| `sha2` | Deterministic `PK5` content hashes | MIT OR Apache-2.0 | Already in `Cargo.lock` |
| `serde`, `ron` | `EmbeddedPartDef` RON v3 serialisation in `omnia-doc` | MIT OR Apache-2.0 | Already in `omnia-doc/Cargo.toml` |
| `omnia-doc`, `omnia-symbols` | Document types, geometry and symbol pin definitions | This repository | Existing workspace crates |

Every crate stays within the `deny.toml` allow-list. No regex crate is added; `PK2` validation
and `PK4` engineering-unit parsing are small hand-written routines.

## 12. Risks

| Risk | Mitigation | Early test |
| --- | --- | --- |
| SPICE `M` = milli vs engineer `M` = mega (`10M` resistor parsed as `10 mΩ`) | Explicit `PK4` SI-first rule for user parameters + mandatory canonical SPICE formatting (`1e6` / `meg`) at netlist time; owner confirms in `PQ2` | Unit tests in `units.rs` |
| Multi-unit ICs (e.g. dual op-amp `U1A` / `U1B` sharing power pins `4` and `8`) do not fit a 1-symbol-per-part model | Tracked in `PQ1`; v1 models single-unit symbols cleanly and reserves optional `[[units]]` in `schema = 1` without blocking Stage 3 | `PQ1` review at `HP0` |
| Third-party SPICE models carry restrictive or non-redistributable licences | Every built-in `library/models/` file must declare an approved licence in `[license]` and `THIRD_PARTY.md`; `xtask parts --check` fails on missing licence metadata (`PQ7`) | `xtask parts --check` in CI |
| Transitive `.subckt` dependencies inside a vendor `.lib` (subcircuit A calls subcircuit B + `.model` C) | `spice.rs` performs a local dependency closure within the `.lib` when extracting the embedded block | `PS1` spike + `PTC-opart-roundtrip` |
| Untrusted `.opart` contains host-executing `.control` / `shell` directives or ZIP bombs | `PK3` streaming archive caps + `PK10` directive screening on load | `PTC-hostile-opart` (`PINV8`, `PINV9`) |
| `Part` struct growth or `BTreeMap` cloning slows `Doc::fork` (`AB2`) or `10k` load (`PB4`, `UB10`) | Store only non-default overrides (empty on default parts); deduplicate `PartDefTable`; measure on `10k` in `PB4` and `AB2` | `PB4` bench + `AB2` check |
| Divergence between RON v3 and the proposed binary `.omnia` format (`docs/file-format-proposal.md`) | `PartDefTable` maps 1:1 to a single `PART_DEFS` binary section alongside `SYMBOLS` and `STRINGS` (`PQ8`) | `HP0` + `FH0` alignment |

## 13. Open questions

Each has a default. All `PQ1`–`PQ8` defaults were accepted by the owner at `HP0` (2026-11-20).

| ID | Question | Answer |
| --- | --- | --- |
| PQ1 | Multi-unit parts (e.g. dual/quad op-amps `U1A`/`U1B` sharing supply pins) in v1, or single-unit first? | **Answered 2026-11-20:** default accepted — single-unit parts in v1 (e.g. one op-amp symbol with its 5 pins); add multi-unit `[[units]]` in a minor schema extension once Stage 3 simulation works. |
| PQ2 | In user-entered parameter values, should uppercase `M` mean `1e6` (SI mega, e.g. `10M` = 10 MΩ) or `1e-3` (legacy SPICE milli)? | **Answered 2026-11-20:** default accepted — case-sensitive SI: `M` = `1e6`, `m` = `1e-3`, and `meg`/`MEG` also accepted as `1e6`. Netlist emission always writes unambiguous scientific notation or `meg` so SPICE engines never misread it. |
| PQ3 | Should `.omnia` embed custom `symbol.svg` vector art from `.opart` files in v1, or only built-in `SymbolId` references + geometry? | **Answered 2026-11-20:** default accepted — built-in `SymbolId` references in v1 (matching `FQ6`); `.opart` `symbol.svg` support lands in a follow-up once custom-symbol rendering is wired into `omnia-canvas`. |
| PQ4 | Where do user-installed `.opart` / `.olib` libraries live on disk? | **Answered 2026-11-20:** default accepted — under the `UD10` config directory: `%APPDATA%\Omnia\libraries\` on Windows and `$XDG_CONFIG_HOME/omnia/libraries/` on Linux, plus user-added folders in `prefs.ron`. |
| PQ5 | Allow an inline `spice = """..."""` field inside `part.toml` for tiny one-off custom parts, or always require a separate `model.lib`? | **Answered 2026-11-20:** default accepted — allow inline `spice` in `.opart` / user parts when `file` is omitted; built-in repo parts must always use `library/models/` so third-party models stay cleanly separated. |
| PQ6 | When should unreferenced `PartDef` entries be pruned from `Doc::part_defs`? | **Answered 2026-11-20:** default accepted — keep them in memory while the document is open (so `undo` of a delete never needs the external library); prune unreferenced definitions on `Doc::save` so saved files carry only live definitions. |
| PQ7 | Which licences are accepted for bundled SPICE models in `library/models/`? | **Answered 2026-11-20:** default accepted — same allow-list as `deny.toml` (`MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `CC0-1.0` / public domain). Proprietary vendor `.lib` files are never bundled; users may import them locally as `.opart` / `.olib`. |
| PQ8 | Coordinate `PartDefTable` with `docs/file-format-proposal.md` (binary `.omnia`) or land RON v3 first? | **Answered 2026-11-20:** default accepted — land RON v3 in `omnia-doc` first (keeps `Doc` independent of `FH0`); if the binary format proposal is accepted, add a `PART_DEFS` section storing the exact same `EmbeddedPartDef` records. |

## 14. Changes to other files

Applied by roadmap task P0.2 after owner sign-off at `HP0` (2026-11-20).

| Where | Change |
| --- | --- |
| `AGENTS.md` | Add `parts-plan.md` / `parts-roadmap.md` to the sources-of-truth table and register the `P`-prefixed ids (`PB`, `PK`, `PD`, `PINV`, `PTC`, `PS`, `PQ`, `HP`, `P0.1`…). |
| `ROADMAP.md` | Link `parts-plan.md` under Stages 2 and 3; note the decided `.opart`/`.olib` and `.omnia` embedding policy. |
| `canvas-plan.md` section 9 (`Doc`, `Part`) | Add `def: PartDefId`, `params`, `PartDefTable` queries, `SetParam` and `UpdatePartDef` commands, and RON v3 note. |
| `ui-plan.md` section 1 & UD15/UD19 | Note that Part Libraries and the inspector will consume `omnia-parts::Catalog` and `Doc::part_defs`; keep `ui-plan.md` stage-2 shell scope unchanged. |
| `mcp-plan.md` section 9 & 14 | Note future minor schema additions (`set_param`, `list_parts`, `update_part_def`) once `omnia-doc` lands `SetParam`. |
| `docs/file-format-proposal.md` | If accepted at `FH0`, reserve section kind `PART_DEFS` for `Doc::part_defs` and per-part `def` + `params` columns. |
