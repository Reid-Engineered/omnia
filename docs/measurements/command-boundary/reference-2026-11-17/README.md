# Command-boundary reference campaign (2026-11-17)

Attempt `attempt-1ORwADgV` on the Orion laptop, run under canvas-plan section 2.1 with its
CPU-only rule (owner decision 2026-11-17). Status: `complete`. Owner review of the
conditions: accepted (2026-11-17).

## Conditions

- Lenovo ThinkPad T470, Intel Core i5-7300U (2 cores, 4 threads), Arch Linux 7.2.7, KDE
  Plasma on Wayland. CPU-only runner: no window, no GPU work, so the adapter, viewport,
  refresh and scale rules do not apply.
- AC power, `performance` governor on all four CPUs, power profile interface unavailable.
- Source f4fd588 (main before PRs #54 and #50). Runner SHA-256
  `5523831e0a7a5026cd998ce6664a8fc16545dbcfd029326ba2aaa97eb742a52a`.
- Owner at the desk, no user apps open. Recorded background load: the KDE file indexer
  (`baloo_file_extractor`) at about 8% CPU, plus normal desktop services. The owner
  accepted this on 2026-11-17.
- Twelve frozen cases, ten interleaved rotating passes, 1,000 observations per operation
  per pass. Statistic: the median of the ten run p99s, with the range.

## Results

| Case | Median p99 (ms) | Range of run p99s (ms) | Target |
| --- | --- | --- | --- |
| `doc_10k_move_100` (T5.1 done-when) | **0.200** | 0.126 to 0.246 | < 1 ms: pass |
| `doc_10k_move_100_connected` | 0.495 | 0.363 to 0.575 | < 1 ms: pass |
| `doc_chain_20k_delete` (T2.8 split worst case) | **0.924** | 0.872 to 1.051 | < 1 ms: pass |
| `doc_chain_20k_add` (merge worst case) | 0.761 | 0.730 to 1.356 | < 1 ms: pass |
| `doc_label_bridge_add` | 0.404 | 0.278 to 0.441 | < 1 ms: pass |
| `doc_label_bridge_delete` | 0.530 | 0.279 to 1.280 | < 1 ms: pass |
| `doc_10k_wire_add` | 0.005 | 0.004 to 0.011 | < 1 ms: pass |
| `doc_10k_wire_delete` | 0.001 | 0.001 to 0.002 | < 1 ms: pass |
| `chain_20k_middle/add` (connectivity only) | 0.164 | 0.105 to 0.193 | < 1 ms: pass |
| `chain_20k_middle/remove` (connectivity only) | 0.847 | 0.772 to 0.912 | < 1 ms: pass |
| `label_5k_gnd_vcc_bridge/add` | 0.015 | 0.009 to 0.020 | < 1 ms: pass |
| `label_5k_gnd_vcc_bridge/remove` | 0.404 | 0.249 to 0.532 | < 1 ms: pass |
| `real_10k_leaf/add` | 0.000 | 0.000 to 0.000 | < 1 ms: pass |
| `real_10k_leaf/remove` | 0.000 | 0.000 to 0.000 | < 1 ms: pass |
| `index_10k/query` | 0.011 | 0.010 to 0.015 | no target |

The 20k split has the least headroom: about 8% under the target, with single passes up
to 1.051 ms. The `>= 1 ms` counts in `summary.txt` are diagnostics only.

## Files

- `summary.txt`: per-pass p99, max and `>= 1 ms` counts for every case.
- `raw.tar.gz`: per-observation CSVs, before/after conditions per invocation, order,
  machine inventory and runner hash.
- `status.txt`: the campaign's own status line.
