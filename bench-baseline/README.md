# Sheet benchmark baselines

The four JSON reports are the T2.6 hosted CI baselines. The fixed cases use the seeded
`1k` and `10k` sheets, pan and zoom, a 1280×800 logical viewport at the test
platform's fixed 200% scale, 600 warm-up frames and 600 measured frames. The headless GPUI
platform builds the same scene that the live sheet renderer submits. Reports contain peak
primitives by kind and Omnia allocations per frame (B10, Q5). No timing metrics are reported.

To reproduce one case on the Windows development machine, run these commands from the
repository root and substitute the sheet and script for the other cases.

```powershell
cargo build --locked --release -p omnia-bench --features headless
target/release/omnia-bench.exe run --headless --sheet 1k --script pan --warmup 600 --frames 600 --json 1k-pan.json
target/release/omnia-bench.exe compare bench-baseline/1k-pan.json 1k-pan.json --max-regression 10 --metrics allocs_per_frame.ours,primitives
```

Pull requests use the baseline files from their base commit after T2.6 lands.

## Orion frame timing

Collect both campaigns on Orion under canvas plan section 2.1. Keep the raw reports,
binary identity, case order and condition snapshots with each campaign. In each campaign
directory, place the pan and zoom reports in `s100/` (100%) and `s125/` (125%).
Use `<sheet>-<script>-<repetition>.json` or `.json.gz`, with repetitions 1 through 10.
For example, `s125/10k-zoom-4.json` is the fourth 10k zoom run at 125%.

Run the **Bench regression gate** workflow manually with the two campaign directories
in the selected ref, or run the comparator on Orion from the repository root.

```sh
python3 bench-baseline/compare-orion.py docs/measurements/orion-baseline docs/measurements/orion-candidate
```

The comparator prints each case's median of run p99s and range. It fails for a
regression greater than 10% or an incomplete or mismatched campaign. Absolute budget
certification is the T2.7 gate.
