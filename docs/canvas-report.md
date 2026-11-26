# Omnia Canvas: Final Budget and Invariant Report (T7.5 → H4)

**Status: `done` (`H4` approved by owner 2026-11-26). 9 of 10 budgets (`B1`–`B9`) PASS at both 100% and 125% scaling on the Orion reference machine; `B10` PASSES (`0` allocs/frame) on all pan, zoom, and wire-drag scripts and MISSES (`45` max Omnia allocations/frame) on `1k-drag-part` and `10k-drag-part` following PR #155 (`W2.3`). Every invariant (`INV1`–`INV13`) and all 84 golden images pass.**

Per `AGENTS.md` Hard Rule 3 and `canvas-plan.md` §2.1, only the Orion reference machine (`Q4`) certifies performance budgets. Windows measurements and golden renders (`render_to_image`, unavailable on Linux) are labelled as non-reference.

## 1. Budget Summary (`B1`–`B10`, Orion Reference Machine)

All values below come from a single frozen 230-invocation Orion campaign at `1b9c127906e4c547f0f54f2d08210625d81abfc2` (whose Rust crate sources are identical to `origin/main` `3f8d257`), ten interleaved repetitions per case at `100%` (`1920×1080` logical) and `125%` (`1536×864` logical) on native `eDP-1` `1920×1080 @ 60.008 Hz` with `OMNIA_BENCH_TRACE=1` on all motion runs. Brackets give `[min, max]` across the ten repetitions; values are `100%` / `125%`.

| Budget | Target (`canvas-plan.md` §2.1) | Measured on Orion (`100%` / `125%`) | Result |
| --- | --- | --- | :---: |
| **B1** | Cold start to interactive empty sheet `< 400 ms` (`bench start`) | `191.783 [183.927, 208.069]` / `190.524 [183.260, 193.465] ms` (worst sample `235.637` / `216.525 ms`) | **PASS** |
| **B2** | Opening `1k` sheet adds `< 300 ms` (`start-1k - start-empty`) | `40.584 [17.117, 49.417]` / `47.038 [39.127, 53.523] ms` | **PASS** |
| **B3** | `1k` pan & zoom (and `1k`/`dense` interactive drags): p99 `< 16.7 ms`, `0` counted missed refreshes (`1.5R`) | `1k-pan` `1.567` / `1.083 ms`; `1k-zoom` `2.767` / `2.184 ms`; `1k-drag-part` `0.814` / `0.611 ms`; `1k-drag-wire` `0.832` / `0.615 ms`; `dense-drag-wire` `0.952` / `0.707 ms`; `10/10` attribution (`0` over-limit) on every case | **PASS** |
| **B4** | `10k` pan & zoom (and `10k-drag-part`): p99 `< 33.3 ms`, `0` counted intervals `> 2.5R` | `10k-pan` `2.008` / `1.457 ms`; `10k-zoom` `5.108` / `5.013 ms`; `10k-drag-part` `0.828` / `0.614 ms`; `10/10` attribution (`0` over-limit) on every case | **PASS** |
| **B5** | No long stalls: max frame `<= 33.3 ms` (B3) and `<= 50.0 ms` (B4) | Worst B3 frame `5.284` / `4.399 ms` (`1k-pan`); worst B4 frame `5.472` / `5.693 ms` (`10k-zoom`) | **PASS** |
| **B6** | Dragged part or wire drawn in next frame: dirty-to-present p99 `<= 16.664 ms` (1 refresh) | `1k-drag-part` `1.528` / `1.190 ms`; `10k-drag-part` `1.540` / `1.194 ms`; `1k-drag-wire` `1.542` / `1.193 ms`; `dense-drag-wire` `1.761` / `1.329 ms` | **PASS** |
| **B7** | Hit test under cursor `< 0.2 ms` on `10k` (`bench hit --sheet 10k`, CPU-only) | `0.001 [0.001, 0.001] ms` (`10/10` runs during `s100`) | **PASS** |
| **B8** | Idle: `0%` CPU and `0` frames drawn over `10 s` (`bench idle --seconds 10`, gated on `0` frames) | `0` frames, `0` renders in all `20/20` runs (`CPU % 0.000 [0.000, 0.100]` at both scales) | **PASS** |
| **B9** | Private memory `< 300 MB` with `10k` sheet open | `10k-pan` `151.082` / `149.881 MB`; `10k-zoom` `173.102` / `171.152 MB`; `10k-drag-part` `151.832` / `150.492 MB` (peak across all runs `174.949 MB`) | **PASS** |
| **B10** | Zero warm Omnia heap allocations per frame (`allocs_per_frame.ours == 0`, `Q5`) | `0` on `1k-pan`, `1k-zoom`, `10k-pan`, `10k-zoom`, `1k-drag-wire`, `dense-drag-wire`; **`45` max & p99 Omnia allocs/frame (means `34.77` / `41.62`) on `1k-drag-part` and `10k-drag-part`** at both scales | **MISS** |

### B10 Miss Diagnosis and H4 Decision Item

`T7.5`'s frozen Orion gate returned `fail` (exit `1`) solely because `1k-drag-part` and `10k-drag-part` recorded **`45` max & p99 Omnia allocations/frame** (`allocs_per_frame.ours`, means `34.77` and `41.62`, at both `100%` and `125%`; Windows `--features headless` `headless_drag_part_inv5_inv11_b10_on_100` likewise failed with `48` on `Parts100`).

- **Cause:** PR #155 (`W2.3`, merged at `5331a78` between `T6.4` and `T7.5`) wired live `omnia_doc::follow::plan_follow` into `ToolMachine::pointer_move` (`move_.rs:661`) during `drag-part` so attached wires preview follow-routing on every grid step. `OverlayView::on_mouse_move` (`overlay.rs:1547`) wraps `ToolMachine::pointer_move` inside `OverlayPaintScope::omnia`, so `plan_follow`'s per-move allocations count against `allocs_per_frame.ours`.
- **Existing plan language (`wiring-plan.md` §9, lines 610–612):** *"`plan_follow` takes its output buffers from the caller so the move tool can reuse them across a drag. It is not in the paint path, so B10 does not bind it, but it should allocate nothing once its buffers are warm."*
- **Paint path remains zero-alloc:** `SheetView` and `OverlayView` paint paths still allocate `0` once warm — confirmed on Orion by `0` allocs/frame on all pan, zoom, and `drag-wire` runs (and `T7.3` `--instrument`), and by `w2_3_b10_warm_follow_paint_makes_zero_omnia_allocations`, `w2_3_b10_warm_rigid_label_follow_paint_makes_zero_omnia_allocations`, `sheet_alloc`, and `sheet_text_alloc`.
- **No frame-time regression:** Even with live `plan_follow` running on `10k-drag-part`, Orion measured `0.828` / `0.614 ms` frame p99 and `1.540` / `1.194 ms` dirty-to-present p99 (over 10× below the `16.664 ms` `B6` limit).
- **For `H4` owner decision:** Decide whether to (1) eliminate warm heap allocations in `plan_follow` (`W2.4` / follow-up) to satisfy `wiring-plan.md`'s *"should allocate nothing once its buffers are warm"* and restore `ours == 0` under `OverlayPaintScope::omnia`, or (2) narrow `OverlayPaintScope::omnia` on `drag-part` to match `wiring-plan.md`'s *"not in the paint path, so B10 does not bind it"* (keeping `plan_follow` out of `B10` while tracking its allocations separately).

## 2. Detailed Orion Per-Case Results (`1b9c127`)

### `100%` scaling (`s100`, `1920×1080` logical)

| Case | Frame p99 `[min, max]` (ms) | Worst frame (ms) | `d2p` p99 `[min, max]` (ms) | Attribution | Memory `[min, max]` (MB) | `ours` max | `INV5` / `INV11` / `D7` |
| --- | ---: | ---: | ---: | :---: | ---: | ---: | --- |
| `1k-pan` | `1.567 [1.516, 1.621]` | `5.284` | — | `10/10` (`1.5R`) | — | `0` | — |
| `1k-zoom` | `2.767 [2.720, 2.884]` | `3.840` | — | `10/10` (`1.5R`) | — | `0` | — |
| `10k-pan` | `2.008 [1.979, 2.045]` | `3.101` | — | `10/10` (`2.5R`) | `151.082 [150.824, 152.894]` | `0` | — |
| `10k-zoom` | `5.108 [5.075, 5.161]` | `5.472` | — | `10/10` (`2.5R`) | `173.102 [170.488, 174.949]` | `0` | — |
| `1k-drag-part` | `0.814 [0.809, 0.867]` | `2.867` | `1.528 [1.504, 1.594]` | `10/10` (`1.5R`) | — | **`45`** | `rev=0`, `sheet=0`, `press=0`, `lift=[1]` |
| `10k-drag-part` | `0.828 [0.822, 0.844]` | `1.205` | `1.540 [1.505, 1.646]` | `10/10` (`2.5R`) | `151.832 [151.484, 153.844]` | **`45`** | `rev=0`, `sheet=0`, `press=0`, `lift=[1]` |
| `1k-drag-wire` | `0.832 [0.825, 0.848]` | `2.972` | `1.542 [1.511, 1.643]` | `10/10` (`1.5R`) | — | `0` | `rev=0`, `sheet=0`, `press=0`, `warmup=0` |
| `dense-drag-wire` | `0.952 [0.943, 1.026]` | `3.074` | `1.761 [1.719, 1.809]` | `10/10` (`1.5R`) | — | `0` | `rev=0`, `sheet=0`, `press=0`, `warmup=0` |

### `125%` scaling (`s125`, `1536×864` logical)

| Case | Frame p99 `[min, max]` (ms) | Worst frame (ms) | `d2p` p99 `[min, max]` (ms) | Attribution | Memory `[min, max]` (MB) | `ours` max | `INV5` / `INV11` / `D7` |
| --- | ---: | ---: | ---: | :---: | ---: | ---: | --- |
| `1k-pan` | `1.083 [1.067, 1.166]` | `4.399` | — | `10/10` (`1.5R`) | — | `0` | — |
| `1k-zoom` | `2.184 [2.124, 2.255]` | `3.275` | — | `10/10` (`1.5R`) | — | `0` | — |
| `10k-pan` | `1.457 [1.425, 1.546]` | `2.779` | — | `10/10` (`2.5R`) | `149.881 [149.469, 151.539]` | `0` | — |
| `10k-zoom` | `5.013 [4.977, 5.149]` | `5.693` | — | `10/10` (`2.5R`) | `171.152 [170.676, 173.035]` | `0` | — |
| `1k-drag-part` | `0.611 [0.592, 0.675]` | `2.751` | `1.190 [1.167, 1.263]` | `10/10` (`1.5R`) | — | **`45`** | `rev=0`, `sheet=0`, `press=0`, `lift=[1]` |
| `10k-drag-part` | `0.614 [0.592, 0.670]` | `0.840` | `1.194 [1.173, 1.283]` | `10/10` (`2.5R`) | `150.492 [150.106, 152.004]` | **`45`** | `rev=0`, `sheet=0`, `press=0`, `lift=[1]` |
| `1k-drag-wire` | `0.615 [0.598, 0.667]` | `3.041` | `1.193 [1.179, 1.239]` | `10/10` (`1.5R`) | — | `0` | `rev=0`, `sheet=0`, `press=0`, `warmup=0` |
| `dense-drag-wire` | `0.707 [0.700, 0.761]` | `1.884` | `1.329 [1.307, 1.407]` | `10/10` (`1.5R`) | — | `0` | `rev=0`, `sheet=0`, `press=0`, `warmup=0` |

### Related Earlier Orion Certifications (Cross-Reference)

For completeness alongside the `T7.5` `B1`–`B10` campaign above, specialty milestone scenes certified separately on Orion:
- **`T7.3` (`10k-pan --instrument`, 60 Hz & 30 Hz, `99e122c`, 2026-11-26):** `10k-pan-inst60` p99 `2.243` / `1.717 ms` (`< 16.0 ms`), `10/10` attribution, `0` Omnia allocs/frame ([`docs/measurements/T7.3.md`](measurements/T7.3.md)).
- **`T4.6` (`10k-hover`, `d39778b`, 2026-11-24):** p99 `< 16 ms`, `0` sheet paints, `10/10` attribution ([`docs/measurements/T4.6.md`](measurements/T4.6.md)).
- **`T3.4` (`text-pan` / `text-zoom`, 2026-11-20):** p99 `< 16.7 ms` (`B3`), `0` Omnia allocs/frame ([`docs/measurements/T3.4.md`](measurements/T3.4.md)).

## 3. Invariant Verification (`INV1`–`INV13`)

Verified on Windows (`3f8d257`, non-reference host: `cargo test --workspace` **820 passed, 0 failed, 3 ignored** across 40 unit/integration/doc-test suites plus all custom-harness render suites; `cargo fmt --all --check`, both Clippy commands, and `cargo deny check` passed):

| ID | Rule (`canvas-plan.md` §7) | Verified by | Result |
| --- | --- | --- | :---: |
| **INV1** | Axis-aligned, non-degenerate wires | `omnia-doc` `doc::tests::inv1_checked_at_insert` | **PASS** |
| **INV2** | No wire end on interior; crossing interiors stay separate; label points preserved | `omnia-doc` `route::tests::inv2_*`, `clip::tests::t7_1_*`, `command::tests::t7_2_*` | **PASS** |
| **INV3** | Junction dots computed (≥ 3 ends/pins), never stored | `omnia-doc` `doc::tests::inv3_matches_brute_force_after_each_edit` | **PASS** |
| **INV4** | Mutations only via `apply`/`undo`/`redo`; `content_eq` with exact live ids | `omnia-doc` `command::tests::inv4_and_inv10_*`, `t5_5_*`, `t7_1_*`, `t7_2_*`, `tc_del_*` | **PASS** |
| **INV5** | Revision unchanged mid-gesture; commit is one undo step; Esc leaves revision unchanged | `omnia-canvas` `tool::tests::inv5_*`, `tool::label::tests::*`, plus Orion `drag-part` & `drag-wire` (`revision_changes == 0`) | **PASS** |
| **INV6** | No colour literals in `omnia-canvas` or `omnia-app` (`UINV1`) | `omnia-theme` `tests/inv6.rs` (4/4), `omnia-app` `theme::tests::uinv1_*` | **PASS** |
| **INV7** | `SheetView` paint uses no `paint_path`/`canvas()` and allocates nothing warm | `omnia-canvas` `sheet::tests::inv7_*`, `tests/sheet_alloc.rs` (2/2), `tests/sheet_text_alloc.rs` (2/2) | **PASS** |
| **INV8** | No `f32` world coordinates; integer camera subtraction before `f64` | `omnia-canvas` `camera::tests::*` (included in 227/227 `omnia-canvas` unit tests) | **PASS** |
| **INV9** | Idempotent grid snap and radius-wins priority snap (`WD5`, `WINV7`) | `omnia-doc` `geom::tests::snap_to_inv9_*`, `hit::tests::*inv9`, `snap::tests::inv9_*`, `winv7_*` | **PASS** |
| **INV10** | Incremental connectivity equals from-scratch rebuild after any sequence | `omnia-doc` `connectivity::tests::inv10_*`, `command::tests::*inv10*`, `route::tests::*inv10*` | **PASS** |
| **INV11** | Hover, selection, net highlight, drag moves, and stub animation do not notify `SheetView` | `omnia-canvas` `overlay::tests::inv11_*`, `instrument_host::tests::inv11_*`, `tool::move_::tests::w2_3_winv11_*`, plus Orion `drag-part`/`drag-wire` (`sheet_paints == 0`) | **PASS** |
| **INV12** | `omnia-doc` and `omnia-symbols` do not depend on GPUI | `xtask` `tests::inv12_document_and_symbols_are_gpui_independent` | **PASS** |
| **INV13** | GPUI revision pinned and unchanged outside upgrade task | Root `Cargo.toml` verified pinned to `15d136bf8e72ae43b284183ec46940c009460f5c` | **PASS** |

*Note on `--features headless` (`omnia-bench`):* `cargo test -p omnia-bench --features headless -- --test-threads=1` passed `headless_drag_wire_inv5_inv11_b10_on_100_and_dense` and `headless_pan_with_instrument_60hz_and_30hz_allocates_zero_and_paints_on_cadence`, and reproduced the `W2.3` `B10` miss in `headless_drag_part_inv5_inv11_b10_on_100` (`ours.max == 48.0`, expected `0.0`).

## 4. Golden Images (Windows Development Machine, Non-Reference)

Linux GPUI has no `render_to_image` (`AGENTS.md`), so golden-image verification ran on the Windows development laptop (`ASUS TUF Gaming F16`, Intel Core i7-14650HX, Intel UHD Graphics driver `32.0.101.7088`, Windows 11 Home `10.0.26200`, DirectX 11):

- **`cargo test -p omnia-golden`:** **PASS** — 6/6 harness unit tests + **84/84 golden images matched with `0` differing pixels** (largest channel difference `0`, limit `<= 0.1%` differing by `> 2/255`):
  - `tests/golden/tests/overlay.rs`: **72/72** (`12` overlay states × `{graphite, gray}` × `{100%, 125%, 150%}`), including `overlay-follow-preview` and `overlay-follow-refused`.
  - `tests/golden/tests/sheet.rs`: **12/12** (`inverting-amp` and `wien-oscillator` × `{graphite, gray}` × `{100%, 125%, 150%}`).
- **Custom-harness render tests (`cargo test --workspace`):** `omnia-app` `tests/fonts.rs` and `tests/icons.rs`, and `omnia-canvas` `tests/scale_factor.rs`, `tests/sheet_render.rs`, and `tests/sheet_text_render.rs` all passed.

## 5. Orion Campaign Provenance, Conditions, and Evidence

- **Provenance:** clean checkout `origin/task/T7.5-final-report` at `1b9c127906e4c547f0f54f2d08210625d81abfc2` (only `docs/measurements/T7.5/{gate.py,test_gate.py,owner-run.sh}` and `canvas-roadmap.md` added on top of `origin/main` `3f8d257`; Rust workspace sources are byte-for-byte identical to `3f8d257`). Release binary `/home/marcus/omnia/target/release/omnia-bench` SHA-256 `3a0cfdb35a784c4878833b68c77fc520186680a4fb3e69ac36bde058252cedc1`, identical before and after.
- **Hardware:** Orion laptop, Core i5-7300U (2 cores / 4 threads and 8 GB RAM per `Q4`), HD Graphics 620 hardware Vulkan (Mesa ANV), Linux `7.2.7-arch1-1`, KDE Wayland. Native `eDP-1` panel at `1920×1080`, `60.008 Hz`; logical viewport `1920×1080` at 100% and `1536×864` at 125%. AC power and `performance` on all four CPUs throughout; power-profile service unavailable (`null`), no platform profile exposed (`{}`).
- **Timing:** `2026-11-26T20:06:20Z–21:07:36Z` (`15:06:20–16:07:36 EST`), 1h 01m 16s timed (`20:04:35Z–21:07:41Z`, 1h 03m 06s wrapper wall time including 22.99 s incremental build and 60 s settle). With the owner's instruction to run remotely, `owner-run.sh --remote` ran via `systemd-run --user`. `tailscaled` and remote SSH access stayed available (`conditions.txt`: `mode: remote: tailscaled and the starting SSH link kept running`), with no SSH polling during the timed window; T3 Code and all coding agents on Orion were closed.
- **Conditions:** all 460 before/after records in `runs.jsonl` show DPMS on, AC connected, and four `performance` governors; no T3 Code, coding agent, `cargo`, `rustc`, Baloo content extractor, or backlight helper appears (`kioworker` appeared in the first 17 of the 460 records — two processes each, `s100` repetition 1 `start-empty` before through `1k-drag-wire` before — and in zero of the remaining 443 records, as a standard KDE I/O worker). Baloo was left running, passed the pre-run idle check, and reported zero queued files afterwards (indexed count `84,377` → `84,378`, +1 file; `baloo_file` used `0.01` CPU seconds). `SimulateUserActivity` exited 0; `HasInhibit` and both `HasInhibition(1)` / `(4)` were `true`. Brightness/dimming stayed `30% / 100%` during the inhibitor check and afterwards. The root helper logged governor restoration at `21:07:41Z`, and [post-run-check.txt](measurements/T7.5/orion-2026-11-26/post-run-check.txt) at `21:14:08Z` confirmed clean checkout `1b9c127` and all four governors back on `powersave`.
- **Other-process CPU:** processes with matching PID and command between `cpu-before.json` (`20:06:09Z`) and `cpu-after.json` (`21:07:41Z`) consumed `308.00` CPU seconds (100 ticks/s): compositor `215.58 s` (includes serving the benchmark itself), desktop shell `66.52 s`, remote transport (`tailscaled`) `2.57 s`, `baloo_file` `0.01 s`. This is other-process CPU disclosure, not a gate input.
- **Replay:** extracting `raw-reports.tar.xz` into a temporary copy of the campaign directory and running `python -B docs/measurements/T7.5/gate.py summarize` exited `1` (`fail`, matching `gate-exit.txt` and `status.txt`) and reproduced `summary.txt` byte-for-byte.
- **Follow-ups outside this task:**
  - `cargo test -p omnia-bench --features headless --lib headless_drag_part_inv5_inv11_b10_on_100` has been failing on `main` since PR #155 (`W2.3`, `48` vs `0` allocs) because `.github/workflows/bench.yml` builds with `--features headless` without executing its unit tests; tracked for `W2.3`/`W2.4` and the `H4` decision above.
  - The existing Linux-only unused `window` compiler warning at `crates/omnia-bench/src/spike/shell_cache.rs:1657` remains visible in `owner-run.log`.
  - Raw `hit-10k` JSON files contain a hard-coded `reference_machine: false` field in `crates/omnia-bench/src/hit.rs`; Orion certification is established by the gate's `hardware.json`, `binary-before`/`after`, and `runs.jsonl` condition checks rather than that static field.

**Evidence archive:**
[summary](measurements/T7.5/orion-2026-11-26/summary.txt),
[status](measurements/T7.5/orion-2026-11-26/status.txt),
[order](measurements/T7.5/orion-2026-11-26/order.json),
[condition journal](measurements/T7.5/orion-2026-11-26/runs.jsonl),
[hardware](measurements/T7.5/orion-2026-11-26/hardware.json),
[binary before](measurements/T7.5/orion-2026-11-26/binary-before.json) /
[after](measurements/T7.5/orion-2026-11-26/binary-after.json),
[source](measurements/T7.5/orion-2026-11-26/source.json),
[wrapper conditions](measurements/T7.5/orion-2026-11-26/conditions.txt),
[inhibition](measurements/T7.5/orion-2026-11-26/inhibit.txt),
[wrapper log](measurements/T7.5/orion-2026-11-26/owner-run.log),
[helper log](measurements/T7.5/orion-2026-11-26/helper.log),
[gate exit](measurements/T7.5/orion-2026-11-26/gate-exit.txt),
[post-run check](measurements/T7.5/orion-2026-11-26/post-run-check.txt),
[CPU before](measurements/T7.5/orion-2026-11-26/cpu-before.json) /
[after](measurements/T7.5/orion-2026-11-26/cpu-after.json),
and [raw reports and logs](measurements/T7.5/orion-2026-11-26/raw-reports.tar.xz)
(SHA-256 `658637995113d92f28c778400f463742b10d4571700e9c196b8302a769ec9292`).
Run scripts: [owner-run.sh](measurements/T7.5/owner-run.sh), [gate.py](measurements/T7.5/gate.py), and [test_gate.py](measurements/T7.5/test_gate.py) (6/6 passed).
