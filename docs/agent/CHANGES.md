# Omnia Agent Surface Changelog

Schema version (`AK7`): **`omnia.agent/1`**. Adding an optional field or a new operation is a
minor change; removing or changing an existing field or operation is a major change and requires
a new schema version string.

## `omnia.agent/1` — Milestone A1 (headless read, preview, apply, undo, open, save)

- **2026-11-25 (`A1.9`):** Published the agent contract ([omnia-agent.md](omnia-agent.md)),
  verification evidence register ([evidence.md](evidence.md)), and this changelog
  ([CHANGES.md](CHANGES.md)). Added automated contract-vs-catalog drift verification in
  `crates/omnia-cli/tests/contract.rs` checking all 14 operations/tools, 7 edit operations,
  33 error codes, counts, units, limits, committed schemas, transcript fixture references, and
  evidence links.
- **2026-11-25 (`A1.8`):** Added `omnia mcp [--root DIR]...` over newline-delimited stdio in
  `crates/omnia-cli/src/mcp.rs` with dual-era support (`2026-07-28` stateless; `2025-11-25` and
  `2025-06-18` handshake), one-era-per-process selection, `tools/list`, `tools/call` with
  `structuredContent` and strict raw-argument decoding, all A1 `omnia://` resources (including
  `omnia://schema/symbols` and `omnia://schema/symbol`), subscriptions, logging, all three static
  prompts, and 6 exact CI transcript fixture pairs under `crates/omnia-cli/tests/transcripts/`.
  Tasks are never advertised (`AQ15`).
- **2026-11-23 (`A1.7`):** Added `crates/omnia-cli` (binary `omnia`) with `capabilities`,
  `schema [NAME]`, `call [--root DIR]... [--] REQUEST`, `serve [--root DIR]...`, and `help`,
  sharing one response encoding with `AgentService` (`AD1`). Added `omnia-cli` to the `INV12` /
  `AINV8` GPUI-independence check in `xtask/src/main.rs`.
- **2026-11-23 (`A1.6`):** Added root-jailed file access (`files.rs`, `AINV12`), staged atomic
  saves, RON v2 `open`/`save`/`close`, tier approval policy (`AD7`, `AQ4`), and FxHash effect
  digests (`approval.rs`, `AINV9`, `AQ10`).
- **2026-11-22 (`A1.5`):** Added `AgentService`, `DocHost` / `HeadlessHost`, 1:1 operation-to-
  `Command` mapping (`ops.rs`, `AD2`), fork preview with `preview_id` (`preview.rs`, `AD3`),
  dirty-bounded diff (`diff.rs`), optimistic revision checking (`AD4`, `AINV6`), per-session
  idempotency cache and `request_status` (`AK5`, `AINV7`), and revision-checked `undo`/`redo`.
- **2026-11-22 (`A1.4`):** Added deterministic read projections (`read.rs`): summary, keyset-
  paged items (`AK4`), `envelope`/`within`/`touching` queries, canonical net grouping (`AD8`),
  and `SymbolInfo` catalog reads (`AINV5`).
- **2026-11-21 (`A1.3`):** Created `crates/omnia-agent` with `omnia.agent/1` DTOs, strict
  schema-backed request/response decoders for both envelope and MCP argument forms, 33 structured
  error codes with exhaustive `CommandError` mapping (`AINV10`), capability catalog dispatch
  table (`Catalog::current()`, `AINV11`), and 44 committed JSON Schema 2020-12 files under
  `crates/omnia-agent/schema/`.
- **2026-11-20 (`A1.2`):** Added `SymbolLibrary::place_command` in `crates/omnia-symbols/src/lib.rs`
  (`AD11`).
- **2026-11-20 (`A1.1`):** Added `Doc::fork()` in `crates/omnia-doc/src/doc.rs` (`AD3`) and
  recorded a non-reference Windows development baseline in `docs/measurements/A1.1.md`.
- **2026-11-20 (`A0.2`, `HA1`, `AQ15`):** Ratified `AD1`–`AD14`, pinned `AK1` to `2026-07-28`,
  `2025-11-25` and `2025-06-18`, and settled `AQ1`–`AQ15`.
