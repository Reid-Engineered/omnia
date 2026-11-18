# Sheet benchmark baselines

The four JSON reports are the T2.6 hosted CI baselines. The fixed cases use the seeded
`1k` and `10k` sheets, pan and zoom, a 1280×800 logical viewport at the test
platform's fixed 200% scale, 600 warm-up frames and 600 measured frames. The headless GPUI
platform builds the same scene that the live sheet renderer submits. Reports contain peak
and sweep-wide primitive counts by kind, a sweep-wide total, and Omnia allocations per frame
(B10, Q5). Batches and paint operations are reported as peaks only.

To reproduce one case on the Windows development machine, run these commands from the
repository root and substitute the sheet and script for the other cases.

```powershell
cargo build --locked --release -p omnia-bench --features headless
target/release/omnia-bench.exe run --headless --sheet 1k --script pan --warmup 600 --frames 600 --json 1k-pan.json
target/release/omnia-bench.exe compare bench-baseline/1k-pan.json 1k-pan.json --max-regression 10 --metrics allocs_per_frame.ours,primitives
```

Pull requests use the baseline files from their base commit after T2.6 lands.

## Orion frame timing

On Orion, connect AC power and close other apps. Build both release bench executables
before the campaign. Both revisions must support native pan and zoom on real sheets.
From the candidate repository root, pass a new output directory to the runner.

```sh
kde-inhibit --power --screenSaver python3 bench-baseline/reference.py /path/to/baseline/omnia-bench \
  /path/to/candidate/omnia-bench docs/measurements/T2.6-orion
```

The runner interleaves ten rotating repetitions per case and scale. It restores the panel
scale and records raw reports, binary hashes, case order and before/after conditions
under the output directory. Its `baseline/` and `candidate/` directories each contain
`s100/` (100%) and `s125/` (125%) with `<sheet>-<script>-<repetition>.json` reports.

Run the **Bench regression gate** workflow manually with the two campaign directories
in the selected ref, or run the comparator on Orion from the repository root.

```sh
python3 bench-baseline/compare_orion.py docs/measurements/T2.6-orion/baseline docs/measurements/T2.6-orion/candidate
```

The comparator validates the run reports and the paired condition snapshots, then prints
each case's median of run p99s and range. It fails for a regression greater than 10% or an
incomplete or mismatched campaign. Absolute budget certification is the T2.7 gate.
