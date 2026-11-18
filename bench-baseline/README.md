# T2.6 benchmark gates

The hosted gate runs pan and zoom on the generated `1k` and `10k` sheets using seed 42,
a 1280 × 800 viewport at scale 1, 600 warm-up frames and 600 measured frames.
It repeats the camera sweep after warming the renderer. Reports sum each primitive kind
over the measured sweep and include their total and the per-frame Omnia allocation summary
(Q5). The gate uses `bench compare` with a 10% limit, which allows no increase from zero.

Run the hosted gate locally from the repository root:

```powershell
cargo build --locked -p omnia-bench --features headless
./bench-baseline/hosted.ps1
```

For an intentional rendering change, run the same script with `-Capture` into a fresh
output directory and review the four reports before replacing the committed files.

The initial headless baselines come from the Windows development machine (i7-14650HX,
Windows 11 25H2, 26200, AC power, Silent power scheme). The test platform creates no GPU
renderer and uses the fixed virtual viewport and scale above with no refresh rate.
These counts carry no frame-time or reference-budget verdict.

The four unchanged reruns matched their baselines. Two temporary source changes in
separate builds verified the negative cases on `1k` pan:

- One heap allocation in the sheet's background paint per frame increased the Omnia
  allocation summary from 0 to 1. Primitive counts matched the baseline.
- Emitting 189,499 extra quads across the sweep increased total primitives from 1,722,710
  to 1,912,209 (+11.0%). Omnia allocations remained zero.

Both plants made the hosted comparison command exit 1.

## Orion frame-time comparison

Q4 calls for the owner to run timing comparisons at the desk until a reference runner is
configured. On Orion, close other apps, connect AC power, and prepare two release
`omnia-bench` executables from the baseline and candidate revisions. Both must support
windowed pan and zoom on the generated sheets. Run from the candidate repository root:

```sh
python3 bench-baseline/reference.py /path/to/baseline/omnia-bench \
  /path/to/candidate/omnia-bench docs/measurements/T2.6-orion
```

The output path must be new. The script rotates the order of both builds and all four
cases across ten repetitions at each panel scale, restores the original scale, and
retains raw reports, run order, binary hashes and condition snapshots. It reports the
median of run p99s with the range and gates the relative change in p99 at 10%. The T2.7
budget gate has its own acceptance criteria.

The same command backs the manually dispatched Orion job on `main`. It requires an
`orion`-labelled runner launched from the reference Wayland desktop session and a
baseline commit on `main` that includes real-sheet benchmarks.
