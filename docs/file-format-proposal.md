# Proposal: a binary `.omnia` sheet format

Status: **draft for the owner**. Nothing here is decided. If accepted, it becomes a plan and
roadmap pair (proposed id prefix `F`: `FB` budgets, `FK` constants, `FD` decisions, `FINV`
invariants, `FQ` open questions, tasks `F0.1`…), registered in `AGENTS.md`.

Goal, from the owner (2026-11-17): **speed and reliability.** A file opens and saves fast, and
no crash, power cut or full disk ever costs the user a saved file.

## 1. Where we are

- `.omnia` is exactly what `Doc::save` writes (UD8): RON version 2, from private records in
  `crates/omnia-doc/src/io.rs`. One file holds one sheet (UQ3). The format belongs to
  `omnia-doc`; the shell neither wraps nor edits it.
- Saves already go temp-then-rename (UD7). Autosave writes recovery copies, never the user's
  file (UD9).
- Baseline, measured 2026-11-17 on the Windows development laptop (i7-14650HX; certifies
  nothing), from `docs/measurements/U0.5.md`:

  | Sheet | RON size | `save` p50 | `load` p50 |
  | --- | --- | --- | --- |
  | `1k` | 0.48 MiB | 1.3 ms | 4.8 ms |
  | `10k` | 4.84 MiB | 13.7 ms | 50.7 ms |

  `load` includes validation, arena inserts, the spatial-index build and the connectivity
  rebuild. **How much of the 50.7 ms is parsing is not yet known.** If the rebuild dominates,
  a binary format alone will not make opens much faster (spike FS1 answers this first).

## 2. Proposed budgets (FB)

Proposals only; reference-machine numbers decide them (hard rule 3).

| Id | Budget | Measured by |
| --- | --- | --- |
| FB1 | Decode only (bytes to records, no rebuild), `10k`: < 10 ms on Orion | `bench file --sheet 10k --stage decode` |
| FB2 | Full `Doc` load, `10k`: at least 2x faster than RON v2 on the same machine | `bench file --sheet 10k --stage load` |
| FB3 | Encode, `10k`: < 10 ms p99 on Orion, comfortably inside UB10's 33.3 ms | `bench file --sheet 10k --stage encode` |
| FB4 | File size, `10k`: < 1.5 MiB | same run |
| FB5 | Reliability: no saved state is ever lost or half-written (FINV1 to FINV4) | property, fault-injection and fuzz tests |

## 3. Layout (FD1)

All integers little-endian. Coordinates stay `i32` nm (K1), so nothing is rounded.

```
Header (32 bytes, fixed)
  magic        8   "OMNIA\r\n\x1A" + 0x00   (catches text-mode and truncation damage)
  major        u16 incompatible changes; a reader refuses a higher major
  minor        u16 additive changes; a reader accepts any minor of its major
  flags        u32 reserved, must be 0
  section_cnt  u32
  table_crc    u32 CRC-32C of the section table
  header_crc   u32 CRC-32C of the bytes above
  reserved     4   zero

Section table (section_cnt × 24 bytes)
  kind   u32   e.g. META, STRINGS, PARTS, WIRES, LABELS, SYMBOLS
  flags  u32   bit 0 compressed, bit 1 must-understand
  offset u64
  len    u32   stored length
  crc    u32   CRC-32C of the stored bytes

Sections, in kind order, then a trailer:
  "END!" u32 + total file length u64 + CRC-32C of everything before it
```

- **Columns, not rows.** `PARTS` stores each field as its own array (all `at.x`, then all
  `at.y`, orientations, symbol ids, string indices). Decoding is a few bulk copies, and the
  arrays compress well. Empty arena slots are a presence bitmap, so saves keep reserved slots
  and `revision` as RON v2 does.
- **`STRINGS`**: designators and values interned once, referenced by `u32` index.
- **`SYMBOLS`**: the local pins and bounds each used symbol needs (what RON v2 embeds per
  part), stored once per symbol rather than once per part.
- **Unknown sections**: skipped on load. If marked must-understand, the file is refused with a
  clear message instead.

## 4. Reliability (FD2)

| Id | Invariant |
| --- | --- |
| FINV1 | A save either leaves the old file intact or the complete new file in place, at every possible crash point. Write `<name>.omnia.tmp`, flush and sync it, rename over the target, sync the directory (Linux). |
| FINV2 | Every byte is covered by a checksum. A load verifies header, table, every section and trailer before building anything; any mismatch refuses the file with a message naming the damaged section. |
| FINV3 | `load(save(doc)) == doc`, and the same document always gives the same bytes (determinism rule). |
| FINV4 | No input bytes make the loader panic, allocate without bound, or return a partly built `Doc`. Lengths are checked against the file size before any allocation (AK2's size cap applies). |

**Crash recovery between saves** keeps UD9's recovery copies, written in this format. A
command journal (append each applied command to a sidecar, replay on recovery) would cut the
loss window from 60 s to one command. It is **not** proposed for the first version: it needs
a stable serialised form for every command, and recovery copies already give the reliability
the owner asked for. It is FQ3.

## 5. Tooling and tests

- **Text twin.** `cargo xtask sheet dump <file>` writes canonical RON (or JSON) and
  `sheet pack` reads it back. That restores diffs, debugging and hand-made fixtures. RON v2
  stays as the agents' test format (AQ12) and as this dump format.
- **Tests.** Round-trip and byte-determinism property tests over T2.4's generator, rebuilt in
  shuffled order. A truncation test that cuts a valid file at every byte offset and flips
  every bit of a small file: each load must refuse cleanly (FINV2, FINV4). A crash-point test
  that fails the writer after every step of FINV1 and checks the target is old-or-new.
- **Fuzzing.** A fuzz target for `Doc::load`, run on Linux; its corpus kept in the repo.

## 6. Dependencies

Hand-written encoding: no serialisation crate. CRC-32C can be a 40-line table
implementation. Compression is optional and decided by FS1: if the uncompressed columns
already meet FB4, no compression crate is added. If one is needed, its licence is logged
before it is added (dependency rule).

## 7. Spike first (FS1)

Before any format work, on the `1k` and `10k` sheets, under canvas plan section 2.1:

1. Split today's `Doc::load` into parse, validate-and-insert, index build and connectivity
   rebuild, and time each.
2. Prototype FD1's `PARTS`, `WIRES`, `LABELS` and `STRINGS` encode and decode, with and
   without compression, and time them and record the sizes.

Decides: whether FB1 to FB4 are realistic, whether compression is needed, and whether the
rebuild needs its own work (for example storing the spatial index or connectivity) for opens
to feel fast. Windows numbers are recorded and labelled; Orion decides.

## 8. Open questions (FQ)

| Id | Question | Default until answered |
| --- | --- | --- |
| FQ1 | Binary, or keep a text format? | Binary, per the owner's interest, subject to FS1. |
| FQ2 | Store derived data (spatial index, connectivity) to skip the rebuild? | No. Rebuild on load; derived caches stay out of files (canvas plan). Revisit only if FS1 shows the rebuild dominates and FB2 fails. |
| FQ3 | Command journal for recovery between saves? | No; UD9 recovery copies only. |
| FQ4 | Compression? | None unless FS1 shows FB4 needs it. |
| FQ5 | Compatibility promise? | None before stage 5 (UD8). The major/minor scheme is in place from day one so a promise can be made later. |
| FQ6 | Embed full symbol art so files open without the library? | No; embed pins and bounds only, as RON v2 does. |

## 9. Proposed tasks

| Task | Summary | Depends on |
| --- | --- | --- |
| F0.1 | Spike FS1: load-stage split and prototype codec timings | U0.5's bench |
| FH0 | Owner reviews FS1 and FQ1 to FQ6 (human checkpoint) | F0.1 |
| F1.1 | Container: header, table, trailer, CRC-32C; FINV2, FINV4 tests | FH0 |
| F1.2 | Sheet sections: columns, strings, symbols; FINV3 tests | F1.1 |
| F1.3 | `Doc::save` / `Doc::load` switch to the binary format; RON kept as dump and agent format | F1.2 |
| F1.4 | FINV1 crash-point tests on the shell's save path | F1.3, U-lane save task |
| F1.5 | `xtask sheet dump` / `pack`; fuzz target | F1.2 |
| F1.6 | FB1 to FB4 gate on Orion | F1.3 |

Files touched: `crates/omnia-doc/src/io.rs` (or a new `format/` module beside it),
`omnia-bench` for FS1 and FB runs, `xtask`, and UD8 in `ui-plan.md` once FQ1 is answered.
