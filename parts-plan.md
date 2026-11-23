# Component definitions and simulation models: plan (reference)

This file is the reference for Omnia's component data, part libraries, custom part containers
(`.opart` / `.olib`), simulation model bindings, and how placed parts and their definitions are
embedded in `.omnia` documents. The work itself is in [parts-roadmap.md](parts-roadmap.md),
which lists tasks that cite the ids defined here.

It sits beside [canvas-plan.md](canvas-plan.md), [wiring-plan.md](wiring-plan.md),
[mcp-plan.md](mcp-plan.md), [ui-plan.md](ui-plan.md) and [sim-plan.md](sim-plan.md), and
re-plans nothing they decide except where the cross-plan decisions in section 6 (`XD1`–`XD9`)
say so. Every unprefixed id (`B`, `K`, `G`, `D`, `INV`, `Q`, `T`) is defined in the canvas
plan; `W`-prefixed ids in the wiring plan; `A`-prefixed ids in the agent plan; `U`-prefixed ids
in the UI plan; `S`-prefixed ids in the simulation plan. `C`-prefixed ids (`CK1`, `CD13`,
`CD14`, `CQ2`, `CQ10`, task `C1.1`, `C3.1`) belong to the proposed context plan (pull request
#100) and are cited only as `XD1`–`XD9` cite them. Section 14 lists the changes this plan makes
to other files and which task makes each one.

Written 2026-11-20 against `main` at `07e96c6`; owner sign-off at `HP0` 2026-11-20. Updated
2026-11-23 against `main` at `9d4b844` after the independent review of pull request #85 and the
cross-plan decisions `XD1`–`XD9`. No `omnia-parts` code exists yet.

## How to read this file

| Prefix | Meaning | Section |
| --- | --- | --- |
| `PB` | Performance or quality budget, a hard requirement | 3 |
| `PK` | Constant: a limit or convention every part crate must agree on | 4 |
| `PD` | Decision | 6 |
| `XD` | Cross-plan decision shared with the simulation and context plans | 6 |
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

An entry marked "(amended 2026-11-23, owner-delegated, review of #85)" was changed after `HP0`
by the coordinating agent under the owner's delegation of 2026-11-23, to fix a finding of the
independent review of pull request #85 or to agree with `XD1`–`XD9`. The amendment text says what
changed.

SPICE, `ngspice`, `RIS-C`, IEEE, IEC, TOML, RON, ZIP, SPDX and SHA-256 are named here as open
standards, formats or engines already referenced in [ROADMAP.md](ROADMAP.md). No other company
is named, and no commercial part number appears except one brief compatibility nod in `PD11`
(AGENTS rule 1).

## 1. Scope

In scope:

- **Component definitions (`part.toml`).** Structured metadata (id, name, category, description,
  tags, datasheet URL, licence), symbol binding, the pin table (stored pin index, definition pin
  number, name, electrical role), typed parameter schemas with units and defaults, and the
  simulation binding with its ordered terminal list.
- **Simulation model storage.** Untouched SPICE `.lib` / `.mod` files in a shared `models/` tree
  for the built-in library, plus native primitive bindings that require no SPICE deck.
- **Portable custom containers.** `.opart` (single part) and `.olib` (multi-part library pack)
  as standard ZIP archives using the exact same inner layout and loader as the repository's
  unpacked library.
- **Build-time compilation (`cargo xtask parts`).** Validates every built-in part against
  `omnia-symbols` and `library/models/`, checks model licences (`PK11`) and SPICE port counts,
  and compiles the built-in catalog into static Rust tables inside `omnia-parts`.
- **Document embedding (`omnia-doc`).** A deduplicated part-definition table inside `Doc` and
  `.omnia` files, per-part parameter overrides, provenance (`source_id` + optional SHA-256
  hash), explicit library-update commands, and strict migration of RON v2 test sheets.
- **Definitions-and-bindings read API.** For each placed part: its resolved definition, resolved
  parameter values, pin roles and SPICE terminal order. Pin-to-net bindings come from
  `omnia-circuit` and lowering to solver input belongs to `omnia-sim` (`XD4`).
- **Archive and model safety.** Strict ZIP limits, path validation, and SPICE directive
  screening on every path that admits a definition: library import, document load and commands
  (`PD10`).

Out of scope:

- Simulation execution, lowering (`CircuitIr`, node ordering, device expansion, SPICE text),
  ERC, solver architecture, engines, analyses, waveforms and instruments (the
  [simulation plan](sim-plan.md), `XD4`).
- Full library browser, parametric search and property inspector UI (extends `ui-plan.md` once
  the data model lands; this plan provides the query and edit APIs).
- PCB footprints, 3D packages, BOM pricing, supply-chain links and hierarchical sub-sheets.
- Custom symbol artwork in `.opart` files (`PQ3`).
- Switching `.omnia` from RON to a binary container (tracked separately in
  `docs/file-format-proposal.md`; this plan defines the logical records and RON v3, which map
  directly onto binary sections if that proposal is accepted; `XD8` keeps `FH0` separate).

## 2. Repository facts this plan rests on

Checked at `9d4b844` (workspace root).

| Fact | Where | Consequence |
| --- | --- | --- |
| `Part` holds only `symbol: SymbolId`, `at`, `orient`, `designator: Arc<str>`, `value: Arc<str>`; crate-private `StoredPart` adds world `pins` and `local_bounds`. There are about 100 `Part { .. }` literals across `omnia-doc`, `omnia-symbols`, `omnia-canvas`, `omnia-agent` and `omnia-bench` | `crates/omnia-doc/src/doc.rs` | Adding public fields to `Part` breaks every literal at once. `PD8` stores the definition reference and overrides on `StoredPart` instead, so `Part` literals stay valid. |
| `Command::PlacePart { part, local_pins, local_bounds }` is built in `omnia-symbols` (`SymbolLibrary::place_command`), `omnia-agent` (ops, service, tests), `omnia-canvas` (sheet, text, tests) and `omnia-doc` tests | `crates/omnia-symbols/src/lib.rs`, those crates | The task that changes `PlacePart` also owns the mechanical update of every construction site (`P2.2`). |
| `Doc::save` / `Doc::load` write and accept only RON `version: 2` (`deny_unknown_fields`); `PartialEq for Doc` compares revision and every reserved slot; `content_eq` compares live items and exact live IDs and ignores revision; undo advances revision | `crates/omnia-doc/src/io.rs` | Persistent equality and undo equality are different relations. `PD8` defines both for definitions. |
| `Change { revision, dirty, added, removed, nets_changed }`; `dirty: None` means a validated no-op | `crates/omnia-doc/src/command.rs` | A parameter edit that changes no geometry or text still needs a visible change (`XD7`). |
| `omnia-agent` refuses any version other than 2 ("not a valid version 2 test sheet") and tests a `version:3` file as unsupported; `ui-plan.md` `UTC-open-bad` and `mcp-plan.md` `AQ12` say the same | `crates/omnia-agent/src/files.rs`, `ui-plan.md`, `mcp-plan.md` | `P2.3` moves the doc and agent fixtures to `version:4` (`XD1`). Whichever of `P2.3` and `U2.3` ships second moves `UTC-open-bad` and its app fixture to `version:4`. |
| `omnia-symbols` compiles `pins.toml` + vendored SVGs via `cargo xtask symbols` into `generated.rs`; 6 symbols are defined; `SymbolId` is the symbol's index in the compiled order | `crates/omnia-symbols/src/lib.rs`, `pins.toml`, `xtask/src/symbols.rs` | v2 files store raw `SymbolId`s, so adding symbols must not renumber existing ones (`P1.4`). |
| `omnia-doc` has no UI dependency; `omnia-symbols` depends on `omnia-doc` | `crates/omnia-doc/Cargo.toml`, `crates/omnia-symbols/Cargo.toml` | `omnia-parts` depends on `omnia-doc` and `omnia-symbols` (no GPUI); `omnia-doc` stays independent of `omnia-parts` and `omnia-symbols`. Anything both need (value grammar, definition validator, SPICE screen) lives in `omnia-doc` (`XD2`, `PD10`). |
| `toml = "1"`, `zip = "9"` (`deflate-flate2-zlib-rs`) and `sha2` are already in `Cargo.lock` and pass `cargo deny check` | `xtask/Cargo.toml`, `Cargo.lock`, `deny.toml` | No new third-party dependency is needed. `deny.toml` governs Rust crates only; bundled model files have their own licence policy (`PK11`). |
| The U4.2 part picker places built-in symbols through `PlacePart` (enabled by `CanvasEdits::place`); Home shows "Part libraries" disabled (`UD15`); simulation controls are `SimState::Unavailable` (`UD19`) | `crates/omnia-app/src/parts/picker.rs`, `ui-plan.md` | The picker keeps placing unmodelled definitions after `P2.2` (`PlaceDef::Unmodelled`); UI `U5.2` switches it to catalog parts through the `Catalog::place_symbol` provider (§14). |
| `.github/workflows/ci.yml` runs the standard checks only | `.github/workflows/ci.yml` | `P1.4` owns the step that adds `cargo xtask parts --check`. |
| `crates/omnia-bench/benches/doc_io.rs` and `command.rs` measure `UB10` save/load and command apply | `crates/omnia-bench/` | `PB4`/`PB5` get their own bench target, owned by `P2.5`. |

## 3. Budgets

Measured on the reference machine under canvas plan section 2.1 (Orion laptop, 10 interleaved
repetitions, median of run p99s). All canvas (`B1`–`B10`), wiring (`WB1`–`WB7`), agent
(`AB1`–`AB3`), UI (`UB1`–`UB10`) and simulation (`SB1`–`SB3`) budgets still hold with their
thresholds (`XD9`); these are added. `PB1`–`PB5` are chosen targets, to be verified on Orion.

| ID | Budget | Measured by |
| --- | --- | --- |
| PB1 | Built-in catalog init (up to 500 compiled parts): < 1 ms, 0 filesystem reads; `B1` (< 400 ms cold start) holds | `crates/omnia-bench/benches/parts.rs` (`P1.5`) |
| PB2 | Open, verify and resolve one `.opart` archive (up to 1 MiB uncompressed), including model extraction and the `PK5` hash: p99 < 5 ms | same bench |
| PB3 | Index a user `.olib` or folder containing 500 parts: p99 < 50 ms off the UI thread. Indexing reads manifests and `part.toml` metadata only; model files are not read and no `PK5` hash is computed until a part is first resolved (`PD9`), which `PB2` covers (amended 2026-11-23, owner-delegated, review of #85) | same bench |
| PB4 | `Doc::save` and `Doc::load` on `Embedded10k` with 50 unique embedded definitions (mix of primitives, `.model` and `.subckt`): `save` stays within `UB10` (< 33.3 ms UI-thread serialise); `load` adds < 5% over the matching unmodelled baseline | `crates/omnia-bench/benches/parts_doc.rs` (`P2.5`) |
| PB5 | `Doc::apply(Command::SetParam { .. })` on `Embedded10k`: p99 < 0.05 ms; warm canvas paint stays allocation-free (`INV7`) | `crates/omnia-bench/benches/parts_doc.rs` (`P2.5`) + `INV7` check |

## 4. Constants

| ID | Constant | Value |
| --- | --- | --- |
| PK1 | Part manifest schema version | `schema = 1`. Unknown versions are refused with a clear error. |
| PK2 | Canonical part id | ASCII `^[a-z0-9][a-z0-9._-]{0,63}$` (1 to 64 chars, lowercase). Pack-qualified ids use `<pack>/<id>`. |
| PK3 | Archive safety limits (`.opart` / `.olib`) | Max compressed file: 8 MiB (`AK2`). Max uncompressed total: 32 MiB. Max single entry: 2 MiB. Max entries: 1,024. Max expansion ratio: 100:1. |
| PK4 | Numeric grammar for user-facing values (`XD2`) | `value := number [" "] [prefix] [unit]` after trimming leading and trailing whitespace. `number` is a decimal with optional sign, fraction and exponent (`4.7`, `.5`, `-1e-3`). At most one ASCII space may separate the number from what follows (`10 kΩ`, `10k`); any other inner whitespace is refused. `prefix` is case-sensitive: `f`, `p`, `n`, `u`/`µ` (U+00B5)/`μ` (U+03BC), `m` (milli), `k`/`K`, `M` (mega), `G`, `T`, or `meg`/`MEG` (= `1e6`) (`PQ2`). `unit`, if present, must name the parameter's declared unit: `Ω`/`ohm` (ohm), `F` (farad), `H` (henry), `V` (volt), `A` (ampere), `Hz` (hertz), `s` (second); a mismatched or unknown unit is refused. A prefix letter is read as a prefix only when a unit or nothing follows it, so `1F` is one farad, `10fF` ten femtofarads and `10f` ten femto-units; `F` is never femto. `%` is accepted only for a parameter whose unit is `ratio` and multiplies by `0.01` (`50%` = `0.5`). Embedded-digit forms such as `4k7` are refused in v1. Native SPICE number syntax applies only inside model decks. Values handed to lowering are plain finite `f64`s; the emitter writes unambiguous numbers. One parser, in `omnia-doc` (`units.rs`), serves the parts loader, `Doc::apply` and the simulator. |
| PK5 | Content hash | Lowercase hex SHA-256 (64 chars) over the canonical encoding of a definition: every field except `source_id` and the hash itself, in a fixed field order, each length-prefixed; text fields (including SPICE text) hashed exactly as stored (`PD3`). Independent of ZIP timestamps, file order, TOML key order or OS line endings (line endings are normalised once, at extraction). Computed by `omnia-parts` only. |
| PK6 | Built-in primitive kinds | `resistor`, `capacitor`, `inductor`, `potentiometer`, `vsource`, `isource`, `vcvs`, `cccs`, `vccs`, `ccvs`, `ground`, `switch`, and `rail` (added 2026-11-23, sim S0.2): a one-terminal supply rail whose `dc` parameter is the rail voltage; simulation lowers it as a DC source from the pin's net to node 0. |
| PK7 | Field size caps | At most 64 parameters per part definition; parameter string max 256 bytes; description max 2,048 bytes; at most 64 tags (32 bytes each); at most 256 pins per part; embedded SPICE text max 2 MiB (`PK3` single entry). |
| PK8 | Container extensions | `.opart` (single-part ZIP archive), `.olib` (multi-part pack ZIP archive), `.omnia` (schematic document). |
| PK9 | Document RON version | `version: 3` adds the `part_defs` table and per-part `def` + `params`. `version: 2` loads by strict migration (`PD8`); every other version is refused. |
| PK10 | Disallowed SPICE content in embedded and imported models | `.control`, `.endc`, `shell`, `system`, `osdi`, `pre_osdi`, `codemodel`, `.csdf`. In archives, any `.include` / `.lib` whose path contains `..`, a drive prefix, a leading `/` or `\`, or names a file outside the archive. In embedded text (documents and commands), any `.include` or `.lib` at all: embedded decks are already flattened. |
| PK11 | Model licence policy (added 2026-11-23, owner-delegated, review of #85) | Bundled files under `library/models/` carry an SPDX id from this list: `MIT`, `Apache-2.0`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `CC0-1.0`, or `LicenseRef-public-domain`. This is a separate policy from `deny.toml` (which covers Rust crates); `cargo xtask parts --check` enforces it from a constant table in `xtask/src/parts.rs` and checks that `THIRD_PARTY.md` lists each file (`PQ7`). |
| PK12 | Parameter names shared with simulation (added 2026-11-23, `XD1`) | `resistance`, `capacitance`, `inductance`, `wiper` (potentiometer ratio, `0..1`), `ic` (initial condition), `waveform` (enum `dc`, `sin`, `pulse`) with its real fields (`dc`, `offset`, `ampl`, `freq`, `delay`, `v1`, `v2`, `rise`, `fall`, `width`, `period`), `ac_mag`, `ac_phase`, `dc` also as the voltage of a `rail` part (real, unit `V`), and `model` (enum over model entries embedded in the same definition). Definitions use these names when they mean these quantities. |

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
  |   spice.rs      .model/.subckt extractor + local closure          |
  |   container.rs  LibrarySource trait: Directory | ZipArchive (PK3) |
  |   hash.rs       canonical SHA-256 content hash (PK5)              |
  |   catalog.rs    static built-ins + mounted user packs + queries   |
  |   embed.rs      PartDef -> omnia_doc::EmbeddedPartDef, place cmd  |
  +-----------+-------------------------------------------------------+
              | EmbeddedPartDef + PlaceDef
              v
  +----------------------------------------+
  | omnia-doc (no GPUI)                    |
  |   units.rs     PK4 numeric grammar     |      definitions and bindings
  |   spice_screen.rs  PK10 screen         |      +----------------------------+
  |   part_def.rs  EmbeddedPartDef,        |      | omnia-circuit (context)    |
  |                validate(), ParamValue  | ---> |   pin-to-net bindings,     |
  |   StoredPart { def, params }           |      |   ground evidence (XD4)    |
  |   Command::PlacePart {..,def,params}   |      +-------------+--------------+
  |   Command::SetParam                    |                    v
  |   Command::UpdatePartDef               |      +----------------------------+
  |   RON v3 save / v2 migration           | ---> | omnia-sim: lowering        |
  +----------------------------------------+      |   CircuitIr, SPICE text,   |
              |                                   |   ngspice / RIS-C (XD4)    |
              v                                   +----------------------------+
  omnia-canvas (reads Part::designator & Part::value unchanged)
  omnia-app    (part picker, inspector, update-from-library banner)
  omnia-agent  (typed placement & param edits through the preview/apply path, XD8)
```

Lane separation (AGENTS.md) stays strict:
- `omnia-doc` never depends on `omnia-parts`, `omnia-symbols`, `toml`, `zip`, `sha2` or GPUI. It
  stores plain, self-contained `EmbeddedPartDef` records and owns the one validator every
  ingress path uses (`PD10`).
- `omnia-parts` depends on `omnia-doc` and `omnia-symbols`, never on GPUI.
- `omnia-canvas` keeps reading `Part::designator` and `Part::value` on the hot render path; it
  never inspects the definition table or parses TOML/SPICE while drawing.

## 6. Decisions

### PD1 Built-in library layout: one TOML per part + shared `models/` tree. DECIDED (owner, 2026-11-20)

- The repository stores built-in parts unpacked under `library/`:
  - `library/parts/<category>/<id>.toml`: one file per concrete part (e.g.
    `diodes/diode-signal.toml`, `opamps/opamp-generic.toml`) and one file per generic
    parametric primitive (e.g. `passives/resistor.toml`, `passives/capacitor.toml` — never one
    file per resistance value).
  - `library/models/<family>/<name>.lib`: standard SPICE `.model` / `.subckt` files kept
    verbatim with their original headers and licence notices.
- Multiple part `.toml` files may reference different entries in the same `.lib` file (for
  example a family deck covering several related devices), avoiding duplicated SPICE text and
  keeping third-party licences cleanly separated from Omnia's metadata.
- (amended 2026-11-23, owner-delegated, review of #85) Example file names use generic fixtures
  instead of commercial part numbers (AGENTS rule 1). This is the only model tree: the simulator
  adds its extra `SD3` definitions here rather than keeping a second deck tree (`XD3`).

### PD2 Portable containers: `.opart` and `.olib` ZIP archives with identical inner layout. DECIDED (owner, 2026-11-20)

- A single custom or shared part is packaged as `<name>.opart`, a standard ZIP archive (Deflate
  or Stored) containing the exact same files an unpacked part uses:

  ```text
  opamp-generic.opart (ZIP)
    part.toml        # required; schema = 1 (PK1)
    model.lib        # optional; required if [sim] references a local file
    LICENSE          # optional; preserved and shown in part info
  ```

- A multi-part library pack is `<name>.olib`, a ZIP archive with `manifest.toml` at its root
  plus `parts/<category>/<id>.toml` and `models/...` — mirroring `library/` directly.
- One internal `LibrarySource` trait reads both unpacked directories and ZIP archives, so built-in
  parts, local user folders, `.opart` files and `.olib` packs go through the exact same parser,
  validator and hasher.
- (amended 2026-11-23, owner-delegated, review of #85) In v1 a part's `[symbol]` must name a
  built-in symbol (`PQ3`). The names `symbol.svg` and `pins.toml` are reserved inside archives
  for the custom-symbol follow-up; an archive that contains either is refused with
  `LibraryError::CustomSymbolUnsupported` rather than silently ignored.

### PD3 Document embedding policy: deduplicated embed-all, lean primitives. DECIDED (owner, 2026-11-20)

- Every unique part definition used by at least one `Part` on a sheet is embedded once in a
  sheet-level definition table inside `Doc` and serialised into `.omnia`.
- Each embedded entry records:
  - Its local `PartDefId(u32)` within the document.
  - Provenance: `source_id: Arc<str>` (e.g. `"omnia:opamp-generic"` or
    `"custom:my-opamp"`) and `content_hash: Option<[u8; 32]>` (`PK5`; `None` for definitions
    that never came from a library, such as migrated v2 parts).
  - Metadata needed offline (name, description, category), the pin table, parameter schema
    (names, types, units, defaults, primary parameter), and simulation binding.
- **Lean primitives:** when the simulation binding is a built-in primitive (`PK6`, such as an
  ideal resistor, capacitor, voltage source or ground), **no SPICE deck is embedded**; the
  simulator lowers the part from its binding kind, terminal order and resolved parameters
  (`XD4`).
- **Model/subcircuit parts:** when the binding is `model` or `subckt`, the resolved SPICE
  definition (`.model` card or `.subckt`…`.ends` block, plus any transitive local
  `.model`/`.subckt` dependencies inside the same `.lib`) is embedded once.
- **Source-deck preservation** (amended 2026-11-23, owner-delegated, review of #85): the embedded
  block is the extracted lines of the source deck in source order, byte for byte, with one
  normalisation applied at extraction: line endings become LF and a leading UTF-8 byte-order
  mark is dropped. Comments, case, whitespace and continuation lines inside the block are kept.
  `.include`/`.lib` directives that resolve inside an archive are replaced by the lines they
  name (flattening, `PD10`); nothing else is rewritten. The `PK5` hash is computed over this
  stored text and adds no normalisation of its own, so two decks that differ only in line
  endings hash equal and two that differ in a comment do not.
- Consequence: any `.omnia` file opens, edits and simulates identically on another machine with
  no external library installed.

### PD4 `part.toml` schema. DECIDED (owner, 2026-11-20)

Two concrete examples define the full v1 surface (amended 2026-11-23, owner-delegated, review of
#85: generic op-amp fixture instead of a commercial part, `PK12` parameter names, one
no-connect encoding):

**Generic primitive (`library/parts/passives/resistor.toml`):**

```toml
schema = 1
id = "resistor"
name = "Resistor"
category = "Passives"
description = "Ideal linear two-terminal resistor"
tags = ["passive", "resistor", "linear"]
designator_prefix = "R"
primary_param = "resistance"

[symbol]
builtin = "Resistor-IEEE-Standard"
default = true

[[pins]]
number = "1"
name = "a"
role = "passive"

[[pins]]
number = "2"
name = "b"
role = "passive"

[params.resistance]
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

**Concrete subcircuit part (`library/parts/opamps/opamp-generic.toml`):**

```toml
schema = 1
id = "opamp-generic"
name = "Op-amp (generic)"
category = "Op-Amps"
description = "Behavioural voltage-feedback operational amplifier macromodel"
tags = ["opamp", "analog"]
designator_prefix = "U"

[symbol]
builtin = "IC-COM-OpAmp"
default = true             # default definition for this symbol (PD11)

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
file = "models/opamps/opamp-generic.lib"
entry = "OPAMP_GENERIC"
# SPICE terminal order, matching `.subckt OPAMP_GENERIC inp inn vcc vee out`
pins = ["3", "2", "8", "4", "1"]

[sim.supply]
pins = ["8", "4"]          # positive, then negative supply terminal
implicit = ["15", "-15"]   # optional implicit-supply fallback voltages (V)

[license]
spdx = "CC0-1.0"
source = "models/opamps/opamp-generic.lib"
```

Pin numbers are the ones the referenced symbol declares in `pins.toml` (`PD5`).

Rules enforced by the schema parser (`#[serde(deny_unknown_fields)]`):
- `[sim].kind` is one of `"primitive"`, `"model"`, `"subckt"`, or `"none"` (mechanical or
  annotation-only parts that the simulator skips).
- When `kind = "model"`, `[sim]` specifies `spice_prefix` (e.g. `"D"`, `"Q"`, `"M"`), `file`,
  `entry` (the `.model` name), and `pins` in SPICE terminal order.
- `[sim].pins` lists, in SPICE terminal order, every declared pin whose role is not
  `no_connect`, each exactly once; pins with role `no_connect` never appear in it. There is no
  separate `no_connect = true` flag (amended 2026-11-23: one encoding, the role).
- For single-part `.opart` archives, `file = "model.lib"` resolves relative to the archive root;
  an inline `spice = """..."""` field is also permitted only when `file` is omitted (PQ5).

### PD5 Pin mapping and electrical roles. DECIDED (owner, 2026-11-20)

- `[[pins]]` is the single bridge between visual artwork (`omnia-symbols` `Pin::number`) and
  electrical meaning.
- Every pin declared in `part.toml` must match a `Pin::number` in the referenced symbol; missing
  or extra pin numbers fail validation.
- Electrical roles (`passive`, `input`, `output`, `bidir`, `power`, `ground`, `no_connect`) are
  stored in the embedded definition so ERC and context checks can use them without touching
  external libraries.
- **Explicit terminal mapping** (amended 2026-11-23, owner-delegated, review of #85). Three
  orders exist and the embedded definition records the mapping between them, so a pin count
  alone never stands in for it:
  1. *Stored pin index* `i`: the position of the pin in the symbol's compiled pin list, which is
     the order of `local_pins` and of `Doc::pins(part)`.
  2. *Definition pin number*: `EmbeddedPartDef::pins[i].number`, the `Pin::number` at index `i`.
     The embedded pin table is stored in stored-pin-index order whatever order `part.toml`
     lists them in.
  3. *SPICE terminal order*: `EmbeddedSimBinding::terminals`, a list of stored pin indices, one
     per terminal of the primitive or `.subckt`/`.model` in order.
  Validation (`PD10`) checks that `pins.len() == local_pins.len()` (or `pins` is empty, which
  only an unmodelled definition with `sim: None` may have; see `PD8`), pin numbers are unique,
  `terminals` holds every non-`no_connect` index exactly once, and `terminals.len()` equals the
  primitive arity or the port count parsed from the embedded deck. A definition that lists the
  same number of pins in a different order maps to different terminals, and tests prove it
  (`PTC-pin-order`).
- **Supply terminals** (added 2026-11-23, cross-plan request from sim #104 and context #100). A
  `model` or `subckt` binding may declare `[sim.supply]`: `pins`, the positive and negative
  supply pins (two distinct pins with role `power`, both in the terminal list), and optionally
  `implicit`, the positive and negative voltages of an implicit-supply fallback (`PK4` values in
  volts, positive above negative). The embedded binding stores it as
  `supply: Option<SupplyDecl>`. This is the one declaration that simulation lowering (`S1.2`) and
  the model-required supply check `ERC008` (`S1.3`) read; what happens when only one of the two
  supply pins is connected (an error, with no partial fallback) is the simulation plan's rule.
  `opamp-generic` declares pins `8`/`4` and the `SQ8` fallback of ±15 V.
- **Pin role vs symbol class** (`XD5`). Per-pin electrical roles here and the simulation plan's
  per-symbol class (`Component`, `Ground`, `PowerRail`, `Instrument`, `Probe`, task `S1.1`) are
  two separate types. A placed part's embedded definition is authoritative for its pin roles
  and terminal order.

### PD6 Typed parameters and `Part::value` synchronisation. DECIDED (owner, 2026-11-20)

- Supported parameter kinds: `real` (with optional `unit`, `min`, `max`), `int` (`i64`), `bool`,
  `enum` (closed list of string variants), `text` (bounded by `PK7`).
- **Finite reals with defined equality** (amended 2026-11-23, owner-delegated, review of #85):
  a real is stored as `ParamValue::Real { raw: Arc<str>, value: FiniteF64 }`. `raw` is the user's
  text as entered; `value` is the `PK4` parse. `FiniteF64` can only be built from a finite
  `f64` (NaN and infinities are refused with `CommandError::InvalidParam`) and stores `-0.0` as
  `0.0`; its `Eq` compares bit patterns. Two reals are equal when both `raw` and `value` are
  equal. This keeps `Eq` on `Part`, `StoredPart`, `Command` and `EmbeddedPartDef`. Because
  `raw` and `value` are stored separately, every real (default or override) must satisfy
  `parse_value(raw, unit) == value` under `PK4`; `validate` checks defaults and `validate_instance`
  checks overrides, so a file or command with `raw = "10M"` and `value = 0.01` is refused
  (amended 2026-11-23, third review of #85). Enum
  variants are stored as `ParamValue::Enum(Arc<str>)`.
- Parameters are a deterministic sorted map (`BTreeMap<Arc<str>, ParamValue>`) per part, storing
  **only overrides** that differ from the definition's defaults. Unmodified parts store an empty
  map (minimal memory and file size). Setting a parameter to its default removes the override.
- **Canvas value sync:** if the definition declares `primary_param` (e.g. `"resistance"` on a
  resistor), `Command::SetParam` on `primary_param` updates `Part::value` in the same
  transaction; and `Command::SetText { field: TextField::Value, .. }` parses and updates
  `primary_param` if valid (refusing with `CommandError::InvalidParam` if out of bounds or
  unparseable, or falling back to display text for unmodelled parts). `omnia-canvas` keeps
  rendering `Part::value` with zero changes.
- Typed parameter validation is the shared predicate behind simulation `ERC003` and the context
  plan's `value.unparsed` for legacy text (`XD6`).

### PD7 Build-time compilation of built-ins (`cargo xtask parts`). DECIDED (owner, 2026-11-20)

- `cargo xtask parts` reads `library/parts/**/*.toml` and `library/models/**/*` in deterministic
  lexicographic path order, validates every definition against `omnia-symbols`, `spice.rs` and
  the `omnia-doc` validator (`PD10`), extracts the minimal self-contained SPICE block for each
  `model`/`subckt`, computes its `PK5` SHA-256 hash, checks `PK11`, and generates
  `crates/omnia-parts/src/generated.rs`.
- A CI step (`cargo xtask parts --check`, added to `.github/workflows/ci.yml` by `P1.4`) fails if
  `generated.rs` is stale, any part fails validation, or a model licence is outside `PK11`.
- At runtime, built-in parts are `&'static` data: zero filesystem access and zero TOML parsing
  at startup (`PB1`).

### PD8 `omnia-doc` integration and RON v2 migration. DECIDED (owner, 2026-11-20; amended 2026-11-23, owner-delegated, review of #85)

The 2026-11-20 text put `pub def` and `pub params` on `Part`, retained unused definitions for
undo while pruning them on save, and left definition-ID remapping open. That cannot satisfy both
`load(save(doc)) == doc` and exact undo, and it breaks every `Part` literal in one step. The
amended decision:

- **Storage.** `Part` keeps its five public fields. Crate-private `StoredPart` gains
  `def: PartDefId` and `params: Params` (the `PD6` override map). They are read through
  `Doc::part_def_id(PartId)`, `Doc::part_params(PartId)` and
  `Doc::resolved_param(PartId, &str)`; they change only through commands. This is what `XD1`
  calls `Part::def`.
- **Definition table and ID policy.** `Doc` holds a crate-private arena of `EmbeddedPartDef`
  records indexed by `PartDefId(u32)`. Ids are allocated at the arena end in apply order and are
  never reused or renumbered, in memory or on save. A record is *referenced* while a live part
  uses it and *retained* otherwise; retained records stay in memory so undo and redo restore
  the exact id and record without the library.
- **Interning.** A placement or update that brings a definition reuses the id of a referenced
  record that is equal to it (full structural equality; `content_hash` is only a prefilter);
  otherwise it allocates a new id. Retained records are never matched, so interning gives the
  same id in a document and in `load(save(document))`.
- **Commands** (all transactional and undoable through `Doc::apply`, `INV4`):
  - `PlacePart { part, local_pins, local_bounds, def: PlaceDef, params: Params }` where `PlaceDef` is
    `Unmodelled` (synthesise the migration definition below), `Existing(PartDefId)` (a
    referenced id; a retained id is refused, because a reloaded document no longer has it) or
    `Embed(Arc<EmbeddedPartDef>)` (validate, then intern). `params` holds the new part's initial
    override-only parameters (empty for a default placement), so a placement with a chosen value
    is one command and needs no `PartId` in advance (amended 2026-11-23, owner-delegated, review
    of #85). The instance validator below must accept the part, its `Part::value` and `params`
    against the definition, or the command is refused.
  - `SetParam { part: PartId, name: Arc<str>, value: Option<ParamValue> }` (`None` resets to the
    definition's default).
  - `UpdatePartDef { from: PartDefId, to: Arc<EmbeddedPartDef> }` validates `to`, interns it and
    moves every part referencing `from` onto it. It is refused if any of those parts' pin
    geometry or overrides would become invalid. `from` becomes retained.
- **Instance validation** (amended again 2026-11-23, re-review of #85). One function,
  `omnia_doc::part_def::validate_instance(def, symbol, local_pins, local_bounds, value, params)`,
  checks one part against its definition, and every path that creates or changes a part's
  definition or overrides calls it: v3 load (each part record), `PlacePart`, `SetParam` (on the
  resulting override map) and `UpdatePartDef` (each moved part against `to`). It checks:
  - `symbol == def.symbol`;
  - `local_pins == def.local_pins` element by element, in order, and
    `local_bounds == def.local_bounds`. Equal count is not enough: the same pins in another
    order are refused, because the terminal mapping (`PD5`) indexes `def.local_pins`;
  - every override name is declared in `def.params`; its `ParamValue` variant matches the
    declared kind; reals and ints lie within `min`/`max`; enum values are declared variants;
    text is within `PK7`; every real satisfies `parse_value(raw, unit) == value` (`PD6`); and no
    override equals its default (overrides are stored canonically, so a file holding a
    default-valued override is refused rather than normalised);
  - if `def.primary_param` is set, `Part::value` equals the `raw` text of the resolved primary
    parameter (override, else default), which is what `PD6` sync maintains.
- **`UpdatePartDef` refreshes display values** (amended 2026-11-23, fourth review of #85). For
  each moved part, overrides are kept and validated against `to`. If `to` has a `primary_param`,
  `Part::value` becomes, in the same transaction and in every case, the `raw` text of `to`'s
  resolved primary parameter for that part: its override if it has one, else `to`'s default.
  So a default changing from `10k` to `22k` shows `22k` on a part without an override, and a
  primary that moves from parameter `a` to parameter `b` shows the part's existing `b` override.
  If `to` has no `primary_param`, `Part::value` is unchanged. Undo restores the old text with the
  old definition (`PTC-update-value`).
- **Symbol ids are checked where symbols are known.** `omnia-doc` cannot see the symbol library,
  so a v3 (or v2) file can name a `SymbolId` that does not exist. `omnia-symbols` provides
  `SymbolLibrary::try_get(id) -> Option<&SymbolDef>` and
  `SymbolLibrary::check_doc(&Doc) -> Result<(), UnknownSymbol>`, which checks every live part's
  symbol and every referenced definition's symbol. Every symbol-aware open path calls
  `check_doc` right after `Doc::load` and refuses the file on error: the agent file host
  (`P2.3`) and the app's File > Open (whichever of `P2.3` and `U2.3` ships second wires the
  check and bumps the `UTC-open-bad` fixture to `version:4`). Canvas paint already skips
  out-of-range ids. `diff_embedded` uses `try_get` and reports `UnknownSymbol`
  (`PTC-unknown-symbol`).
- **Pin geometry comes from the definition.** A stored part's local pins and art bounds are its
  definition's. The v3 part record stores no geometry of its own; world pins are rebuilt from
  `def.local_pins`, `at` and `orient` on load. The terminal mapping is therefore consistent by
  construction once `validate` (definition) and `validate_instance` (part) pass. `omnia-doc`
  cannot see symbol pin numbers, so a definition's geometry is authoritative for its parts, as
  per-part geometry is in v2; `Catalog::diff_embedded` reports `SymbolMismatch` when an embedded
  definition's geometry differs from the built-in symbol it names (`PD9`).
- **Refusal rolls back exactly.** A refused command or batch, including one that interned a new
  definition before a later step failed, leaves the arena length, every record, every part, the
  revision and the history as they were. The rollback in `apply` and `apply_batch` truncates the
  definition arena to its length before the command, alongside the item arenas, from the task
  that adds the arena (`P2.1`): even then, an existing `PlacePart` interns an unmodelled
  definition, so a refused batch can already allocate one. Transaction tests compare in-memory
  state (including retained records), not a save/load round trip, because a save cannot
  represent retained records or, before `P2.3`, modelled definitions.
- **Change visibility** (`XD7`). `Change` gains `modified: Vec<PartId>`: the parts whose
  `params` or `def` changed. `SetParam` lists its part and `UpdatePartDef` lists every instance
  it moved, even when geometry, text and topology are unchanged; the revision advances and
  `dirty` covers those parts' bounds. Undo and redo report the same parts. Context invalidation,
  `since` deltas and the simulator's stale-result handling (`SD11`) consume this.
- **Persistent equality.** `PartialEq for Doc` compares the persistent state: revision, every
  item slot (including `def` and `params` of each stored part), the definition arena length, and
  the records of referenced definitions. Retained records are history state: they are neither
  saved nor compared, like `History`.
- **Live equality.** `Doc::content_eq` additionally compares, for each live part, its `def` id,
  the record that id resolves to, and its `params`. Undo restores this live state exactly
  (`PINV7`); revision is not part of it.
- **Save.** `Doc::save` writes `version: 3` with `part_defs: Vec<Option<EmbeddedPartDef>>`
  indexed by id: referenced records at their ids, `None` for retained or never-filled slots,
  trailing `None`s kept so the arena length survives. Nothing is remapped, so ids in a saved
  file equal ids in memory (`PQ6`).
- **Load.** `version: 3` runs every record through the `PD10` validator and every part record
  through `validate_instance`, and refuses the file if a part names a missing or `None` id or
  either check fails.
- **v2 migration** (`XD1`). `version: 2` files load by strict migration: each unique
  `(symbol, local_pins, local_bounds)` tuple, in order of first appearance by part slot,
  becomes an unmodelled definition with `source_id = "legacy:<symbol id>"`,
  `content_hash = None`, the tuple's geometry, an empty pin table (`omnia-doc` cannot see
  symbol pin numbers, and inventing roles would be false; context derives `unspecified`,
  `XD5`), no parameters and `sim: None`; each part references its tuple's id with empty
  `params`. The same synthesis serves `PlaceDef::Unmodelled`, so a sheet built by placement and
  the same sheet loaded from v2 are equal. v2 is read only; `save` always writes v3. Any other
  version is refused.
- **No lossy save between tasks** (amended again 2026-11-23, re-review of #85). The commands
  land in `P2.2` and the v3 writer in `P2.3`. In between, `Doc::save` still writes v2, which
  cannot hold definitions or overrides, so from `P2.1` until `P2.3` `save` refuses with
  `io::ErrorKind::InvalidData` ("document holds part definitions or parameters that this build
  cannot save") when any referenced definition is not an unmodelled one or any part has a
  non-empty `params`. A document of unmodelled parts saves exactly as before. `P2.3` removes the
  refusal when it lands the v3 writer (`PTC-no-lossy-save`).

### PD9 Provenance, hash verification and explicit library updates. DECIDED (owner, 2026-11-20)

- Opening a `.omnia` file **never** silently overwrites its embedded definitions from the local
  library.
- `omnia-parts` provides `Catalog::diff_embedded(&Doc) -> Vec<DefUpdateAvailable>`, comparing
  each referenced embedded `(source_id, content_hash)` against the currently mounted libraries.
- Applying an update is an explicit user (or agent) action issuing `Command::UpdatePartDef`, so
  it is atomic, inspectable and fully undoable.
- **When a hash is verified** (amended 2026-11-23, owner-delegated, review of #85). A catalog
  entry indexed from a user folder or `.olib` starts *unverified*: indexing reads metadata only
  (`PB3`), and a hash declared in a pack manifest is only a claim. The first `Catalog::resolve`
  reads and extracts the model, builds the canonical definition and computes `PK5`; the entry
  is then *verified*. A declared hash that disagrees is refused (`LibraryError::HashMismatch`)
  and the part is unusable. Built-ins are verified at build time by `xtask parts`. Only
  resolved, verified definitions are embedded (`to_embedded` resolves first), so every
  `Some(content_hash)` written into a document was computed by `omnia-parts`.
- **What a hash match means.** Equal `PK5` hashes mean byte-identical canonical content, whatever
  the provenance. `diff_embedded` pairs records with library entries by `source_id` only; an
  equal hash under a different `source_id` is reported as "same content, different source" and
  never offered as an update. A document record whose `content_hash` no longer matches its own
  content when recomputed by `omnia-parts` is reported as `LocallyModified`, and one whose
  local pins or bounds differ from the built-in symbol it names as `SymbolMismatch`, or whose
  symbol id is not in the library as `UnknownSymbol` (`PD8`). In
  `Doc`, records with different `source_id`s are different records even if their content is
  equal.

### PD10 One validation contract for every ingress. DECIDED (owner, 2026-11-20; amended 2026-11-23, owner-delegated, review of #85)

- `.opart` and `.olib` files are treated as untrusted input:
  - Enforce `PK3` byte, entry-count and decompression-ratio caps while streaming entries, before
    allocating output buffers.
  - Reject any archive entry path with `..`, absolute root, Windows drive prefix, NUL byte,
    duplicate canonical path, or symlink.
  - Reject any non-UTF-8 `part.toml` or SPICE file.
  - `.include` or `.lib` directives must resolve strictly inside the same archive and are
    flattened during extraction (`PD3`).
- **One validator.** `omnia_doc::part_def::validate(&EmbeddedPartDef) -> Result<(), DefError>`
  is the single check for a self-contained definition: `PK7` caps, the `PD5` pin table and
  terminal mapping, parameter specs (each default valid under its own kind and bounds,
  `primary_param` present), binding arity, `PK12` enum variants naming embedded entries, and
  the `PK10` screen of embedded SPICE text (`spice_screen.rs`, which accepts no `.include` or
  `.lib` at all). It runs on every path that admits a definition into a `Doc`: v3 load,
  `PlacePart` with `PlaceDef::Embed`, and `UpdatePartDef`. `omnia-parts` runs it on every
  definition it builds (built-in compile, `.opart`, `.olib`, folder), after its own TOML and
  archive checks. It lives in `omnia-doc` so the dependency boundary holds (`PINV1`).
- A document or command that carries a definition is therefore as hostile as an archive: a
  hand-edited `.omnia` with `.control` in an embedded deck is refused at load, and the same text
  in a `PlacePart` is refused by `apply`.

### PD11 Starter library scope. DECIDED (owner, 2026-11-20; amended 2026-11-23, owner-delegated, review of #85)

- The first built-in set covers the 6 symbols already in `crates/omnia-symbols/pins.toml` plus
  the minimal sources needed for the reference circuits (voltage divider, RC low-pass filter,
  diode clipper, op-amp stages):
  - Primitives: `resistor`, `potentiometer`, `capacitor`, `ground`, `vsource-dc`, `vsource-ac`.
  - Modelled parts: `diode-signal` (`.model`; a generic small-signal silicon diode, parameter
    values compatible with the common 1N4148 class) and `opamp-generic` (`.subckt`). The
    generic op-amp's default binding is the simulation plan's `SQ8` behavioural macromodel,
    built only from `R`/`C`/`E`/`G`/`D` and authored for Omnia under `CC0-1.0` (`XD3`).
- Commercial part numbers are not used as fixture ids, file names or test names (AGENTS rule 1).
  The simulation plan's extra `SD3` definitions join this library through its own tasks
  (`XD3`).
- `pins.toml` and the symbol compiler have one editing task at a time (`XD5`): `P1.4` adds only
  the source symbols the starter parts need, then `S1.1` adds the remaining analog symbols, then
  `S3.2` adds instrument artwork. Every addition keeps every existing `SymbolId` unchanged,
  because v2 files and goldens store raw ids.
- The existing v2 goldens stay unchanged. The simulation plan builds its own modelled Wien
  oscillator fixture (`S2.2`, `XD1`).
- **Default definition per symbol** (added 2026-11-23, cross-plan request, `XD3`). A built-in
  part may mark itself `[symbol] default = true`; `xtask parts --check` refuses two defaults for
  one symbol. `Catalog::place_symbol` is the shared placement helper: it embeds the default
  definition for the symbol whenever one exists (`PlaceDef::Embed`, value from the resolved
  primary), and falls back to `PlaceDef::Unmodelled` only when none exists. So a generic op-amp
  placed from the U4.2 picker gets its default binding when canvas `T5.3` commits placement
  through the shell's `U5.2` provider. The agent's `place_part` also uses this helper
  (section 14). The starter defaults are `resistor`,
  `potentiometer`, `capacitor`, `ground`, `diode-signal` and `opamp-generic`.
- Supply-rail part definitions (`rail` primitive, `dc` parameter, symbol class `PowerRail`) are
  added with their rail symbols by `S1.1`, through this library (`XD3`, `XD5`).
- **Stress sheets stay fixed** (added 2026-11-23, re-review of #85). The `omnia-bench` stress
  sheets (`100`, `1k`, `10k`, dense wiring, text heavy) back certified Orion budgets (for example
  `T2.7`, `T3.4`), and today the generator draws from every built-in symbol. Before any symbol is
  added, `P1.4` pins the generator to an explicit, fixed list of today's six `SymbolId`s, in
  today's order, so every existing sheet stays item-for-item identical (including stored pin
  coordinates and local bounds) for every seed. Use a new named `StressSheet` variant for a
  workload with new symbols or embedded definitions. `P2.5` adds `Embedded10k` (`embedded-10k`)
  with 50 unique definitions for `PB4`, `PB5` and the `P3.1` `B9` regression check. The fixed
  symbol list also protects the existing sheets when `S1.1` and `S3.2` add symbols.

### XD cross-plan decisions (coordinator, owner-delegated, 2026-11-23)

The owner delegated pending sign-offs to the coordinating agent on 2026-11-23. After an
independent review of this plan against `sim-plan.md` and the proposed context plan (#100), the
coordinator decided `XD1`–`XD9`. They bind this plan; the text that touches it is copied here.
The simulation and context plans record their own amendments in their own tasks.

- **XD1 One document schema.** Parts PK9/PD6/PD8 and P2.1–P2.3 own `Part::def`, typed
  non-default `params`, `Command::SetParam`, `Command::UpdatePartDef` and the single RON v3.
  Sim SD9's `StoredPart::props`, `Doc::part_props`, `SetPartProp` and its own version bump are
  withdrawn; sim keys (resistance, `ic`, source waveform, `ac_mag`, model override) become typed
  parameters declared in part definitions. P2.3 owns the strict v2 → v3 migration (legacy parts
  become unmodelled definitions), the v3 writer, and the unsupported-version fixture bumps across
  `omnia-doc` and `omnia-agent`. The v2 goldens stay unchanged; S2.2 builds a separately
  modelled Wien simulation fixture.
  *In this plan:* `PD8`, `PK9`, `PK12`, `P2.1`–`P2.3`.
- **XD2 One numeric grammar.** Parts PK4/PQ2 win: in user-facing values `M` is mega and `m` is
  milli. Native SPICE syntax applies only inside actual model decks; the emitter writes
  unambiguous numbers. Sim SK1, SQ2, SINV6, STC-erc-01 and S1.2 are amended; ERC007 is retired.
  The shared parser lives below both the parts loader and `Doc::apply` (in `omnia-doc` or a
  leaf crate it depends on), so `omnia-doc` never depends on `omnia-parts`.
  *In this plan:* `PK4`; the parser is `crates/omnia-doc/src/units.rs`, created by `P1.1`.
- **XD3 One model source.** Parts PD1/PD3/PD7/PD9/PD11 own the catalog and embedded model
  bindings. A generic op-amp gets its default binding (sim SQ8's behavioural model) at
  placement, through its part definition. S1.2 consumes embedded records and contributes any
  extra SD3 definitions to the shared built-in library; no second deck tree, no model chosen
  from `Part::value`. SD3's component coverage stands.
  *In this plan:* `PD1`, `PD11`.
- **XD4 Bindings, ground and lowering.** `omnia-doc` owns connectivity. `omnia-circuit`
  (context plan) exposes revision-matched pin-to-net bindings and declared ground evidence
  through a small read-only interface task that precedes S1.2 (split from C3.1 if needed).
  `omnia-sim` owns lowering: `CircuitIr`, node ordering, device expansion, source maps (many
  nets per solver node, many devices per part), diagnostics mapping and SPICE text. CK1 net
  keys, MCP `NetKey` and solver `NodeId` stay distinct types. Ground: sim SQ3's default stands
  (a `Ground` symbol or a net labelled `0` is node 0; a `GND` label alone is not, and gets a
  hint). Context CD13's name/topology heuristics feed context findings only, never solver
  connectivity. Parts' netlist/"cards" wording becomes "definitions and bindings"; lowering is
  sim's.
  *In this plan:* sections 1 and 5, `PD3`.
- **XD5 Symbol class vs pin role.** Two separate types: sim S1.1's symbol class (`Component`,
  `Ground`, `PowerRail`, `Instrument`, `Probe`) and parts PD5's per-pin electrical roles (plus
  context's derived `unspecified`). A placed part's embedded definition is authoritative for
  pin roles and SPICE terminal order. Context CQ2 stays at its default (`power.undriven` off
  until PD5 is extended). `pins.toml` and the symbol compiler have one editing task at a time:
  P1.4 (starter parts), then S1.1 (remaining analog symbols), then S3.2 (instrument artwork).
  *In this plan:* `PD5`, `PD11`, `P1.4`.
- **XD6 ERC crosswalk.** Shared predicates, separate severity per surface. ERC003 ↔ typed param
  validation and legacy `value.unparsed`; ERC008 is a model-required supply check on shared pin
  facts. (The rest of `XD6` concerns the simulation and context plans only.)
  *In this plan:* `PD6`.
- **XD7 File ownership order.** `omnia-doc` `doc.rs`/`command.rs`/`undo.rs`/`io.rs`: parts
  P2.1–P2.3 first, then sim S1.1 (which also lists `undo.rs`), then context C1.1 per CQ10 once no
  open task owns those files. `SetParam` and every instance an `UpdatePartDef` touches appear in
  `Change`, undo/redo, context invalidation and `since`, and SD11 stale-result handling, even
  when topology and display text are unchanged.
  *Update 2026-11-23 (sim `S0.2`, pull request #104; context #100):* `S1.1` no longer owns any
  `omnia-doc` file, and context `C1.9` adds a lineage token to `doc.rs`, so the order for
  `doc.rs`/`command.rs`/`undo.rs`/`io.rs` is parts `P2.1`–`P2.3`, then context `C1.9`, then
  context `C1.1`.
  *In this plan:* `PD8` (change visibility), parts-roadmap Lanes.
- **XD8 Crates and agent surface.** `omnia-parts` (catalog, import), `omnia-circuit` (derived
  circuit facts), `omnia-sim` + adapters (execution, results); all headless, dependencies
  one-way. Parts' `set_param`/`update_part_def` use the existing preview/apply edit path and
  appear in CD14 deltas. FH0's binary format stays separate from the RON migration.
  *In this plan:* section 5, section 14 (`mcp-plan.md`).
- **XD9 Ids and budgets.** Every budget keeps its threshold (PB5, CB1, SB1–SB3, B9, UB10, AB2).
  *In this plan:* section 3.

## 7. Invariants

Each invariant has an automated test; the owning task is named in `parts-roadmap.md`.

| ID | Rule |
| --- | --- |
| PINV1 | `omnia-parts` has no GPUI dependency, and `omnia-doc` has no dependency on `omnia-parts`, `omnia-symbols`, `toml`, `zip` or `sha2`. Checked by `cargo tree` / workspace test. |
| PINV2 | **Determinism.** Loading the same library from files enumerated in any order (or unpacked vs `.olib` ZIP with different entry order/timestamps) produces identical `Catalog` order, identical `content_hash` (`PK5`) and byte-identical `xtask parts` output. |
| PINV3 | **Pin consistency.** Every built-in and loaded part's `[[pins]]` matches its symbol's `Pin::number` set exactly, and its terminal list matches the `.subckt` port count or primitive arity. |
| PINV4 | **Self-contained `.omnia`** (amended 2026-11-23). For any `doc`, `load(save(doc)) == doc` under the persistent equality of `PD8`, and `save(load(save(doc)))` is byte-identical to `save(doc)`. With no library mounted, every referenced definition, parameter override, pin table and SPICE block survives byte for byte, and definition ids are unchanged. |
| PINV5 | **Deduplication.** Placing `N` instances of the same part definition creates exactly 1 referenced record in the definition table. |
| PINV6 | **Lean primitives.** An `EmbeddedPartDef` whose binding is `Primitive(_)` or `None` stores zero SPICE deck bytes. |
| PINV7 | **Transactional edits (`INV4`)** (amended 2026-11-23). A refused `PlacePart`, `SetParam`, `UpdatePartDef` or batch containing one leaves the `Doc` equal under `PartialEq`, with the same definition-arena length, retained records and history. After an accepted command `c`: `undo` restores `content_eq` with the state before `c` (exact live item ids, the same `def` id per part resolving to an equal record, equal `params`) and `redo` restores `content_eq` with the state after `c`; revision advances on each step. Every such step reports the touched parts in `Change::modified` (`XD7`). |
| PINV8 | **Archive safety (`PK3`).** No malformed, truncated, path-traversal, symlink or ZIP-bomb `.opart` / `.olib` file causes a panic or allocation beyond `PK3` caps; every such input returns a typed `LibraryError`. |
| PINV9 | **SPICE hygiene (`PK10`).** No definition containing `.control`, shell escapes or an unresolved `.include`/`.lib` is admitted through any ingress (archive, folder, v3 load, `PlacePart`, `UpdatePartDef`); every embedded SPICE block has zero external file references. |
| PINV10 | **Built-in zero-I/O start (`PB1`).** Constructing `Catalog::builtin()` executes zero filesystem calls and parses zero TOML at runtime. |
| PINV11 | **Terminal mapping** (added 2026-11-23). For every admitted definition, `terminals` maps each SPICE terminal to exactly one stored pin index and every non-`no_connect` pin to exactly one terminal (`PD5`). |
| PINV12 | **Instance validity** (added 2026-11-23, re-review of #85). Every stored part passes `validate_instance` against its definition after every accepted command and every load; no path admits an override that is undeclared, of the wrong kind, out of bounds or equal to its default, or local pins that differ from the definition's in value or order. |

## 8. Scenarios

Each scenario is an automated test with this expected result.

| ID | Steps | Expected |
| --- | --- | --- |
| PTC-builtin-load | Construct `Catalog::builtin()` and query `resistor`, `diode-signal`, `opamp-generic` | All present, valid `SymbolId`, pin counts match `omnia-symbols`, hashes match `PK5`, zero filesystem reads. `opamp-generic` carries `SupplyDecl { pos: 8's index, neg: 4's index, implicit: ±15 V }`; each starter default is the only default for its symbol. |
| PTC-place-dedup | Place 50 `resistor` parts and 10 `opamp-generic` parts via `Doc::apply` | `doc.part_defs().count() == 2`; the resistor record carries no SPICE text (`PINV6`); the op-amp record embeds its resolved `.subckt` once (`PINV5`). |
| PTC-param-sync | Place `resistor` (default `10k`), apply `SetParam("resistance", "4.7k")`, then `SetText(Value, "2.2k")`, then `undo` twice | `Part::value` reads `"4.7k"`, then `"2.2k"`, then `"4.7k"`, then `"10k"`; the override map is empty after the second undo; each step lists the part in `Change::modified`. |
| PTC-param-refuse | Apply `SetParam("resistance", "-5")` (below `min = 1e-6`), `SetParam("bogus", "1")` and a real whose parse is not finite (`1e999`) | All refused with `CommandError::InvalidParam`; `Doc` unchanged under `PartialEq` (`PINV7`). |
| PTC-param-silent | Apply `SetParam("tc1", "1e-3")` on a resistor (not the primary parameter) | Accepted; `Part::value` unchanged; revision advances; `Change::modified == [part]`; undo reports the same part (`XD7`). |
| PTC-opart-roundtrip | Pack the `opamp-generic` directory into an `.opart`, mount it with `Catalog::mount_opart`, resolve, compare against the unpacked load | Identical definition, identical `content_hash` (`PINV2`), identical resolved SPICE block. |
| PTC-offline-omnia | Place a custom `.opart` part, save, drop `Catalog`, load | Loads cleanly; `load(save(doc)) == doc`; all metadata, pin roles, param overrides and SPICE `.subckt` intact; ids unchanged (`PINV4`). |
| PTC-def-lifecycle | Place a modelled part (definition id 1) and delete it: call this state `D`. Branch A: save `D`, load it as `L`, then place the same definition in both `D` and `L`. Branch B: rebuild `D` on a fresh document by replaying the same place and delete (a fork of `D` has no history to undo), then undo the delete and place the same definition again | A: the saved file has `None` at id 1 and the same arena length; `L == D` under `PartialEq`; both placements allocate the same new id 2, since neither document has a referenced record at 1 (`PD8`, `PINV4`). B: undo restores the part with `def == 1` and an equal record (`PINV7`); the new placement reuses id 1 because it is referenced again. |
| PTC-instance-refuse | Load v3 files whose part records hold (a) an override below its `min`, (b) an undeclared override name, (c) an override of the wrong kind, (d) an override equal to its default; (e) a real override with `raw = "10M"` and `value = 0.01`, (f) a resistor whose `Part::value` differs from its resolved `resistance` text; apply `PlacePart { def: Existing(id) }` whose `local_pins` are the definition's pins with two swapped (same count) | Each file refused at load with an error naming the part and parameter; the command refused with `InvalidInstance`; `Doc` unchanged (`PD8`, `PINV12`). |
| PTC-update-value | Place two resistors (default `10k`), give the second an override `4.7k`, then `UpdatePartDef` to a definition whose default is `22k`; undo. Then, with a definition pair whose primary moves from parameter `a` to parameter `b`, update a part that already overrides `b` | After the first update the parts show `22k` and `4.7k`; both are in `Change::modified`; undo restores `10k` and `4.7k`. After the second update the part shows its `b` override's `raw` text, and undo restores its old `a` text (`PD8`). |
| PTC-unknown-symbol | Open a v3 file whose part names `SymbolId(999)` through the agent file host and app File > Open; call `SymbolLibrary::check_doc` and `Catalog::diff_embedded` on the `Doc` returned by `Doc::load` | Both hosts refuse the file with a clear error and the app leaves no tab open; `check_doc` returns `UnknownSymbol`; `diff_embedded` reports `UnknownSymbol` and does not panic (`PD8`, `PD9`). |
| PTC-no-lossy-save | Between `P2.2` and `P2.3`: place a part with `PlaceDef::Embed`, or apply `SetParam`, then call `save`; and save a document of unmodelled parts | The first two saves are refused with `InvalidData` and write nothing usable as a v2 file; the unmodelled document saves byte-identical to before. From `P2.3` the same documents save as v3 and `PINV4` holds (`PD8`). |
| PTC-batch-intern-refused | Apply a `Batch` of `PlacePart { def: Embed(new) }` followed by an invalid `SetParam` | Refused; the definition arena length, every record, every part, revision and history are unchanged (`PINV7`). |
| PTC-pin-order | Build two definitions for the same 3-pin symbol, one listing `[[pins]]` in symbol order and one in another order, and a third whose `[sim].pins` swaps two terminals; place each and read terminal-to-pin mappings | The first two give identical embedded pin tables and terminals; the third maps the swapped terminals to the swapped pin indices; a terminal list with the right count but a repeated pin is refused (`PINV11`). |
| PTC-library-update | Load a document whose embedded `opamp-generic` has an older hash; mount a newer one; run `diff_embedded` + `UpdatePartDef` + `undo` | `diff_embedded` reports 1 update; `UpdatePartDef` updates hash and SPICE text on all instances and lists them in `Change::modified`; `undo` restores the old record and id. |
| PTC-hash-provenance | Mount two packs with equal content under different `source_id`s, and one pack whose manifest declares a wrong hash | The equal pair is reported as "same content, different source", never as an update; the wrong declared hash is refused with `LibraryError::HashMismatch` on first resolve (`PD9`). |
| PTC-hostile-opart | Feed `mount_opart` a ZIP with `../escape.lib`, a 150:1 zero-filled bomb, a `.lib` containing `.control / shell rm -rf`, and a `symbol.svg` | All four refused with specific `LibraryError` variants before allocating past `PK3` (`PINV8`, `PINV9`, `PD2`). |
| PTC-hostile-embedded | Load a v3 file whose embedded deck contains `.control`; apply `PlacePart { def: Embed(..) }` and `UpdatePartDef` with a deck containing `.include x.lib` | Load refused with a clear error; both commands refused; `Doc` unchanged (`PD10`, `PINV9`). |
| PTC-v2-compat | Load the existing RON v2 stress sheet (`1k`) and every `tests/golden/*.ron` through `Doc::load` | Succeeds; synthesises unmodelled definitions; all parts, wires, labels, nets and `SymbolId`s match the v2 load; goldens render with 0 differing pixels; the files on disk are unchanged. |
| PTC-version-refuse | Load `version: 4` and `version: 1` files through `Doc::load` and the agent file host | Both refused; the agent reports `unsupported_file_version` (`P2.3`). |
| PTC-xtask-mismatch | Run `xtask parts --check` on a fixture where `[sim].pins` has 4 entries for a 5-port `.subckt`, and one with a model licence outside `PK11` | Fails with a diagnostic naming the part file, `.subckt` name and expected vs actual port count (`PINV3`), and the file and licence id. |

## 9. Spikes

One small spike before building the parser and archive pipeline:

| ID | Question it settles | How |
| --- | --- | --- |
| PS1 | Does a lightweight line-oriented SPICE extractor reliably handle real `.model` / `.subckt` decks (line continuations `+`, nested `.subckt`, inline comments `*` / `;`, `PARAMS:` defaults, transitive local references) without pulling in a full SPICE parser? And what is actual `.olib` (500 parts) index time on Orion (`PB3`)? | Prototype `spice.rs` extractor on 20 representative openly licensed SPICE decks (diodes, BJTs, MOSFETs, op-amps with `PARAMS:` and nested subcircuits) and time a 500-part `.olib` metadata index scan. Record results in `docs/measurements/P0.3.md`. |

## 10. Crates and APIs

New crate `crates/omnia-parts/` (no GPUI):

```rust
// ---- omnia-parts ----
pub struct PartIdSlug(Arc<str>);           // validated PK2 id
pub struct ContentHash(pub [u8; 32]);      // PK5 SHA-256

pub struct PartDef {                       // resolved, verified library definition
    pub source_id: Arc<str>,
    pub content_hash: ContentHash,
    pub tags: Vec<Arc<str>>,
    pub datasheet: Option<Arc<str>>,
    pub license: Option<LicenseInfo>,
    pub embedded: Arc<omnia_doc::EmbeddedPartDef>, // what a document stores (PD3)
}

pub struct Catalog { /* static built-ins + mounted user folders / .opart / .olib */ }
impl Catalog {
    pub fn builtin() -> Self;                                      // PB1, PINV10
    pub fn mount_opart(&mut self, path: &Path) -> Result<PartIdSlug, LibraryError>;
    pub fn mount_olib(&mut self, path: &Path) -> Result<PackSummary, LibraryError>;
    pub fn mount_dir(&mut self, path: &Path) -> Result<PackSummary, LibraryError>;
    pub fn search(&self, query: &PartQuery) -> Vec<PartSummary>;   // metadata only; deterministic order
    pub fn resolve(&self, source_id: &str) -> Result<Arc<PartDef>, LibraryError>; // reads model, verifies PK5 (PD9)
    pub fn to_embedded(&self, source_id: &str) -> Result<Arc<omnia_doc::EmbeddedPartDef>, LibraryError>;
    pub fn place_command(&self, source_id: &str, at: Pt, orient: Orient,
                         designator: Arc<str>) -> Result<omnia_doc::Command, LibraryError>; // PlaceDef::Embed; value from resolved primary
    pub fn place_symbol(&self, symbol: SymbolId, at: Pt, orient: Orient, designator: Arc<str>,
                        value: Option<&str>) -> Result<omnia_doc::Command, LibraryError>;
    // Default definition if any, else Unmodelled (PD11). `value`: None or empty -> the default
    // (no override); otherwise parsed under PK4 as the default definition's `primary_param`; equal
    // to the default -> no override (PD6); unparseable or out of range -> InvalidValue; a non-empty
    // value for a definition with no `primary_param` (e.g. ground, opamp-generic) -> NoPrimaryParam;
    // with no default definition (Unmodelled) the text is kept as legacy display text.
    pub fn diff_embedded(&self, doc: &omnia_doc::Doc) -> Vec<DefUpdateAvailable>;
}

pub enum DefUpdateAvailable {
    Newer { def: PartDefId, to: Arc<PartDef> },
    SameContentDifferentSource { def: PartDefId, library_source_id: Arc<str> },
    LocallyModified { def: PartDefId },
    SymbolMismatch { def: PartDefId },
    UnknownSymbol { def: PartDefId },
}

// Packing helpers for export and tests
pub fn pack_opart(dir: &Path, out_path: &Path) -> Result<ContentHash, LibraryError>;
pub fn pack_olib(dir: &Path, out_path: &Path) -> Result<PackSummary, LibraryError>;
```

Additions in `crates/omnia-doc/` (no `omnia-parts` dependency):

```rust
// ---- omnia-doc: units.rs (P1.1, XD2) ----
pub struct FiniteF64(f64);                 // finite, -0.0 stored as 0.0, Eq by bits
pub fn parse_value(text: &str, unit: Option<&str>) -> Result<FiniteF64, ValueError>; // PK4 grammar

// ---- omnia-doc: spice_screen.rs (P1.2) ----
pub fn screen_embedded(spice: &str) -> Result<(), ScreenError>;     // PK10, embedded rules
pub fn subckt_ports(spice: &str, entry: &str) -> Option<usize>;      // for terminal counts

// ---- omnia-doc: part_def.rs (P2.1) ----
pub struct PartDefId(pub u32);

pub enum ParamValue {
    Real { raw: Arc<str>, value: FiniteF64 },  // PD6
    Int(i64),
    Bool(bool),
    Enum(Arc<str>),
    Text(Arc<str>),
}
pub type Params = BTreeMap<Arc<str>, ParamValue>; // overrides only

pub enum PinRole { Passive, Input, Output, Bidir, Power, Ground, NoConnect }
pub struct EmbeddedPin { pub number: Arc<str>, pub name: Arc<str>, pub role: PinRole }

pub enum ParamKind {
    Real { unit: Option<Arc<str>>, min: Option<FiniteF64>, max: Option<FiniteF64> },
    Int { min: Option<i64>, max: Option<i64> },
    Bool,
    Enum(Vec<Arc<str>>),
    Text,
}
pub struct EmbeddedParamSpec {
    pub name: Arc<str>, pub label: Arc<str>, pub kind: ParamKind,
    pub default: ParamValue, pub optional: bool,
}

pub enum PrimitiveKind {
    Resistor, Capacitor, Inductor, Potentiometer,
    VSource, ISource, Vcvs, Cccs, Vccs, Ccvs, Ground, Switch, Rail,
}

pub enum EmbeddedSimBinding {
    None,
    Primitive { kind: PrimitiveKind, terminals: Vec<u16> },               // stored pin indices
    Model { spice_prefix: char, entry: Arc<str>, terminals: Vec<u16>, spice: Arc<str>,
            supply: Option<SupplyDecl> },
    Subckt { entry: Arc<str>, terminals: Vec<u16>, spice: Arc<str>, supply: Option<SupplyDecl> },
}
pub struct SupplyDecl {                    // PD5: read by sim lowering and ERC008
    pub pos: u16, pub neg: u16,            // stored pin indices, both in `terminals`
    pub implicit: Option<(FiniteF64, FiniteF64)>, // fallback volts (pos, neg), pos > neg
}

pub struct EmbeddedPartDef {               // Eq; validated by validate() on every ingress (PD10)
    pub source_id: Arc<str>,
    pub content_hash: Option<[u8; 32]>,    // None: no library provenance (v2 migration, Unmodelled)
    pub name: Arc<str>,
    pub category: Arc<str>,
    pub description: Arc<str>,
    pub designator_prefix: Arc<str>,
    pub symbol: SymbolId,                  // built-in only in v1 (PQ3)
    pub local_pins: Vec<Pt>,
    pub local_bounds: Rect,
    pub pins: Vec<EmbeddedPin>,            // stored-pin-index order (PD5); empty only for unmodelled defs
    pub params: Vec<EmbeddedParamSpec>,    // sorted by name
    pub primary_param: Option<Arc<str>>,
    pub sim: EmbeddedSimBinding,
}
pub fn validate(def: &EmbeddedPartDef) -> Result<(), DefError>;
pub fn validate_instance(def: &EmbeddedPartDef, symbol: SymbolId, local_pins: &[Pt],
                         local_bounds: Rect, value: &str, params: &Params)
                         -> Result<(), InstanceError>; // PD8

// ---- omnia-doc: command.rs (P2.2) ----
pub enum PlaceDef { Unmodelled, Existing(PartDefId), Embed(Arc<EmbeddedPartDef>) }
//   PlacePart { part: Part, local_pins: Vec<Pt>, local_bounds: Rect, def: PlaceDef, params: Params }
//   SetParam { part: PartId, name: Arc<str>, value: Option<ParamValue> }
//   UpdatePartDef { from: PartDefId, to: Arc<EmbeddedPartDef> }
//   Change gains `modified: Vec<PartId>` (XD7)
//   CommandError gains InvalidParam { part, name }, InvalidDef(DefError),
//   InvalidInstance(InstanceError), MissingDef(PartDefId)

// ---- omnia-doc: Doc queries (P2.1) ----
//   pub fn part_def(&self, id: PartDefId) -> Option<&EmbeddedPartDef>;          // referenced or retained
//   pub fn part_defs(&self) -> impl Iterator<Item = (PartDefId, &EmbeddedPartDef)>; // referenced only, id order
//   pub fn part_def_id(&self, part: PartId) -> Option<PartDefId>;
//   pub fn part_params(&self, part: PartId) -> Option<&Params>;
//   pub fn resolved_param(&self, part: PartId, name: &str) -> Option<&ParamValue>; // override, else default
```

`Part` itself is unchanged (`PD8`).

Additions in `crates/omnia-symbols/` (`P1.4`):

```rust
impl SymbolLibrary {
    pub fn try_get(&self, id: SymbolId) -> Option<&SymbolDef>;
    pub fn check_doc(&self, doc: &omnia_doc::Doc) -> Result<(), UnknownSymbol>; // PD8
}
```

`xtask` additions (`xtask/src/parts.rs`):
- `cargo xtask parts` — validates `library/` and regenerates `crates/omnia-parts/src/generated.rs`.
- `cargo xtask parts --check` — CI mode; fails if `generated.rs` differs, any part fails
  validation, or a model licence is outside `PK11`.
- `cargo xtask part pack <dir> <out.opart>` — packs and validates a `.opart` archive.

## 11. Dependencies

No new crates beyond what is already in `Cargo.lock` and approved in [deny.toml](deny.toml).

| Crate | Use | Licence | Status in repo |
| --- | --- | --- | --- |
| `toml` (`1`) | Parse `part.toml` and `manifest.toml` in `omnia-parts` and `xtask` | MIT OR Apache-2.0 | Already in `xtask/Cargo.toml` and `Cargo.lock` |
| `zip` (`9`, `deflate-flate2-zlib-rs` only) | Read and write `.opart` / `.olib` ZIP archives | MIT | Already in `xtask/Cargo.toml` and `Cargo.lock` |
| `sha2` | Deterministic `PK5` content hashes (in `omnia-parts` only) | MIT OR Apache-2.0 | Already in `Cargo.lock` |
| `serde`, `ron` | `EmbeddedPartDef` RON v3 serialisation in `omnia-doc` | MIT OR Apache-2.0 | Already in `omnia-doc/Cargo.toml` |
| `omnia-doc`, `omnia-symbols` | Document types, validator, geometry and symbol pin definitions | This repository | Existing workspace crates |

Every crate stays within the `deny.toml` allow-list. No regex crate is added; `PK2` validation,
`PK4` parsing and `PK10` screening are small hand-written routines. Model files are not crates
and follow `PK11`.

## 12. Risks

| Risk | Mitigation | Early test |
| --- | --- | --- |
| SPICE `M` = milli vs engineer `M` = mega (`10M` resistor parsed as `10 mΩ`) | One `PK4` grammar for all user values (`XD2`); lowering receives plain numbers; native SPICE syntax only inside decks | Unit tests in `omnia-doc` `units.rs` |
| Multi-unit ICs (e.g. a dual op-amp sharing supply pins between units) do not fit a 1-symbol-per-part model | Tracked in `PQ1`; v1 models single-unit symbols cleanly and reserves optional `[[units]]` in `schema = 1` | `PQ1` |
| Third-party SPICE models carry restrictive or non-redistributable licences | `PK11` list, `[license]` metadata and `THIRD_PARTY.md`, enforced by `xtask parts --check` in CI | `PTC-xtask-mismatch` |
| Transitive `.subckt` dependencies inside a `.lib` (subcircuit A calls subcircuit B + `.model` C) | `spice.rs` performs a local dependency closure within the `.lib` when extracting the embedded block | `PS1` spike + `PTC-opart-roundtrip` |
| Untrusted `.opart`, `.omnia` or agent command contains host-executing `.control` / `shell` directives or ZIP bombs | `PK3` streaming caps + one `PD10` validator on every ingress | `PTC-hostile-opart`, `PTC-hostile-embedded` |
| Equal pin counts hide a wrong terminal order | Explicit stored-index → number → terminal mapping (`PD5`); geometry taken from the definition and checked in order by `validate_instance` (`PD8`) | `PTC-pin-order`, `PTC-instance-refuse` |
| A save between `P2.2` and `P2.3` drops definitions or overrides | `save` refuses such documents until the v3 writer lands (`PD8`) | `PTC-no-lossy-save` |
| Adding symbols silently changes the stress sheets behind certified budgets | `P1.4` pins the generator to today's symbol list before adding symbols; new symbols go into new named sheets (`PD11`) | Stress-sheet fingerprint test (`P1.4`) |
| Workspace-wide breakage from `Part`/`Command` changes | `Part` unchanged; the task that changes `PlacePart`, `Command` or `Change` owns every construction site and exhaustive match (`P2.2`) | Standard checks on `P2.1`, `P2.2` |
| `StoredPart` growth or map cloning slows `Doc::fork` (`AB2`) or `10k` load (`PB4`, `UB10`) | Store only non-default overrides (empty on default parts); deduplicate definitions; measure | `PB4` bench + `AB2` check |
| Divergence between RON v3 and the proposed binary `.omnia` format | The definition table maps 1:1 to a single `PART_DEFS` binary section (`PQ8`); `FH0` stays separate (`XD8`) | `FH0` alignment |

## 13. Open questions

Each has a default. All `PQ1`–`PQ8` defaults were accepted by the owner at `HP0` (2026-11-20).

| ID | Question | Answer |
| --- | --- | --- |
| PQ1 | Multi-unit parts (e.g. dual/quad op-amps sharing supply pins between units) in v1, or single-unit first? | **Answered 2026-11-20:** default accepted — single-unit parts in v1 (e.g. one op-amp symbol with its 5 pins); add multi-unit `[[units]]` in a minor schema extension once Stage 3 simulation works. |
| PQ2 | In user-entered parameter values, should uppercase `M` mean `1e6` (SI mega, e.g. `10M` = 10 MΩ) or `1e-3` (legacy SPICE milli)? | **Answered 2026-11-20:** default accepted — case-sensitive SI: `M` = `1e6`, `m` = `1e-3`, and `meg`/`MEG` also accepted as `1e6`. Values reach lowering as plain numbers, so engines never misread them. Confirmed across plans by `XD2` (2026-11-23). |
| PQ3 | Should `.omnia` embed custom `symbol.svg` vector art from `.opart` files in v1, or only built-in `SymbolId` references + geometry? | **Answered 2026-11-20:** default accepted — built-in `SymbolId` references in v1 (matching `FQ6`); `.opart` `symbol.svg` support lands in a follow-up once custom-symbol rendering is wired into `omnia-canvas`. Until then archives containing `symbol.svg` or `pins.toml` are refused (`PD2`, amended 2026-11-23). |
| PQ4 | Where do user-installed `.opart` / `.olib` libraries live on disk? | **Answered 2026-11-20:** default accepted — under the `UD10` config directory: `%APPDATA%\Omnia\libraries\` on Windows and `$XDG_CONFIG_HOME/omnia/libraries/` on Linux, plus user-added folders in `prefs.ron`. |
| PQ5 | Allow an inline `spice = """..."""` field inside `part.toml` for tiny one-off custom parts, or always require a separate `model.lib`? | **Answered 2026-11-20:** default accepted — allow inline `spice` in `.opart` / user parts when `file` is omitted; built-in repo parts must always use `library/models/` so third-party models stay cleanly separated. |
| PQ6 | When should unreferenced definitions be pruned from the document? | **Answered 2026-11-20:** default accepted — keep them in memory while the document is open (so `undo` of a delete never needs the external library); prune unreferenced definitions on `Doc::save` so saved files carry only live definitions. (amended 2026-11-23, owner-delegated, review of #85): pruning writes `None` at the pruned id and never renumbers, so ids in memory and on disk agree (`PD8`). |
| PQ7 | Which licences are accepted for bundled SPICE models in `library/models/`? | **Answered 2026-11-20:** default accepted — permissive licences only; proprietary vendor `.lib` files are never bundled; users may import them locally as `.opart` / `.olib`. (amended 2026-11-23, owner-delegated, review of #85): the list is its own model policy, `PK11`, not the `deny.toml` crate allow-list (which has no `CC0-1.0` and does include crate-only licences); `P1.4` enforces it in `xtask parts --check`. |
| PQ8 | Coordinate the definition table with `docs/file-format-proposal.md` (binary `.omnia`) or land RON v3 first? | **Answered 2026-11-20:** default accepted — land RON v3 in `omnia-doc` first (keeps `Doc` independent of `FH0`); if the binary format proposal is accepted, add a `PART_DEFS` section storing the exact same `EmbeddedPartDef` records. |

## 14. Changes to other files

| Where | Change | Made by |
| --- | --- | --- |
| `AGENTS.md` | Add `parts-plan.md` / `parts-roadmap.md` to the sources-of-truth table, register the `P`-prefixed ids, add `HP*` to hard rule 4. | P0.2 (applied; merged with the simulation registration 2026-11-23) |
| `ROADMAP.md` | Link `parts-plan.md` in the intro and Stage 2; Stage 3 cites the parts plan for definitions and bindings and the simulation plan for lowering. | P0.2 (applied; updated 2026-11-23) |
| `canvas-plan.md` section 9 | Comments: `Part` unchanged with `def`/`params` on `StoredPart`; `PlacePart` gains `def: PlaceDef` and initial override `params: Params`; `SetParam`, `UpdatePartDef`; `Change::modified`; new `Doc` queries and equality; RON v3 with v2 migration. | P0.2 (applied; updated 2026-11-23). `P2.1`–`P2.3` turn the comments into signatures when they land. |
| `ui-plan.md` sections 1 and 8 | Part Libraries and the inspector will consume `omnia-parts::Catalog` and the `Doc` definition queries. File > Open calls `SymbolLibrary::check_doc` after `Doc::load` (`PD8`). Whichever of `P2.3` and `U2.3` ships second wires the check and bumps `UTC-open-bad` and its app fixture from rejected `version:3` to `version:4`. | P0.2 (handoff documented 2026-11-23); P2.3 or U2.3 (implementation) |
| `omnia-canvas` placement | Add a shell-supplied placement provider hook (for example `SheetCanvas::set_place_provider`). When `Tool::Place(symbol)` commits, use the provider's returned `Command` for the symbol and snapped point, carrying the chosen orientation and designator. | Canvas T5.3 |
| `omnia-app` placement | Add the workspace `omnia-parts` dependency and install the provider that calls `Catalog::place_symbol` (`PD11`, `XD3`). Test that placing a generic op-amp through the picker and committing it on the canvas stores the `opamp-generic` definition with its default binding. | UI U5.2, after T5.3 and P2.4 |
| `omnia-agent` `place_part` | Build placement through `Catalog::place_symbol` (`XD3`). Pass the operation's `value` to `place_symbol`, which builds one `PlacePart` carrying the primary-parameter override in its `params` (empty value means the default; equal-to-default means no override; a value on a definition without a primary parameter is refused), so the placement is a single atomic command and needs no new `PartId` in advance. Several `place_part` operations in one request become one `Command::Batch`. Test preview/apply of a resistor with value `4.7k`, verifying a resistance override of 4,700 Ω and displayed text `4.7k`. Test that a refused value leaves the document unchanged. | The agent-lane task that adopts `omnia-parts`, after `P2.4` (handoff) |
| `mcp-plan.md` section 9.2 | Note future minor schema additions (`set_param`, `list_parts`, `update_part_def`) on the existing preview/apply path (`XD8`). | P0.2 (applied; updated 2026-11-23) |
| `docs/file-format-proposal.md` | Reserve section kind `PART_DEFS` for the definition table and per-part `def` + `params`. | P0.2 (applied) |
| `ui-plan.md` ("RON version 2" facts) and `mcp-plan.md` (`AQ12` and other "RON v2" mentions) | Record that documents are RON v3 with v2 read by migration. | P2.3, in the same pull request as the code (`XD1`) |
| `sim-plan.md`, `sim-roadmap.md`, context plan | Amendments required by `XD1`–`XD9` (withdraw `SD9` props, `SK1` grammar, model precedence; `S1.1` no longer owns `omnia-doc` files, per #104). | Not this plan: their own tasks. |
