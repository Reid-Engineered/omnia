# T1.6 traced diagnostic campaign: results (2026-11-17)

Diagnostic evidence, not a gate verdict (AGENTS.md, Measurements). T1.6 stays blocked.
Prepared by [../README.md](../README.md); the gate-blocker audit section 4.1 asked for it.

## Run

- Attempt `attempt-LBKFmccq`, status `complete`. Source commit
  f8f56b833deecdf980f93ba1a8a868e291698016 (main + PR #56 tty fix). The bench binary's
  SHA-256 was the same before and after (`binary.sha256`, `binary-after.sha256`).
- Orion laptop: Lenovo ThinkPad T470, Intel Core i5-7300U, Intel HD 620 (Mesa ANV, Vulkan),
  Arch Linux 7.2.7, KDE Plasma on Wayland, native eDP-1 1920x1080 at 60 Hz, 100% scale then
  125% (1536x864 logical). AC power, `performance` governor on all cores, power profile
  interface unavailable. Owner at the desk with other apps closed. Full inventory:
  `hardware.txt`; per-run conditions inside the data archive.
- Schedule as frozen in `order.txt`: start, pan, zoom and idle, rotated each of ten passes,
  at 100% and then 125%. 80 invocations, 40 motion runs, all with `OMNIA_BENCH_TRACE=1` and
  `WAYLAND_DEBUG=client`.

## Result

- 40 motion runs, 40 usable traces, **1 missed refresh**, 0 analysis errors, 0
  aggregate/raw disagreements (`analysis.txt`).
- The miss: 125% **zoom** run 6, measured frame 195, present interval 33.13 ms (threshold
  24.997 ms). The two earlier untraced misses were in 125% pan runs.
- Every other gate line in `gate-summary-diagnostic.txt` passes under tracing: frame p99
  medians 0.77 to 1.26 ms, B10 0, B8 0 frames, B1 medians 226 and 233 ms. The summarizer
  exits 1 for this one miss. Tracing overhead means these numbers certify nothing.

## What the trace shows about the miss

Timeline around the miss (CPU clock, from `analysis.txt`):

| Event | Time (ms after frame 194 present end) |
| --- | --- |
| Frame 194 present end | 0 |
| Frame 195 signal (frame callback received) | 31.97 |
| Frame 195 draw | 31.98 to 32.45 |
| Frame 195 present end | 33.13 |

The client log shows the compositor's frame callbacks `wl_callback.done(1168490)` and
`wl_callback.done(1168523)` arriving 33 ms apart, with Omnia's next frame request sent
0.5 ms after the first. So between frames 194 and 195 Omnia had already presented and was
waiting for the compositor. The callback for the next refresh did not arrive; the following
one did. Omnia's own work for frame 195 took 1.2 ms from signal to present end.

Reading, for the owner: in this instance the missed refresh happened while the client was
idle, and the compositor skipped one frame callback. Nothing in the trace points to Omnia's
draw or present time. One instance cannot show whether the earlier untraced misses had the
same cause. Whether a compositor-side skip counts against B3 is an owner decision; this
report does not change B3, its threshold or T1.6's status.

## Files

- `analysis.txt`: per-run analysis with the miss's timeline and Wayland log excerpt.
- `gate-summary-diagnostic.txt`: the gate summarizer run on the traced data.
- `attempt-LBKFmccq-data.tar.xz`: the whole attempt except client logs (JSON reports and
  traces, stdout, conditions, scripts, order, hashes).
- `attempt-LBKFmccq-wayland-logs.tar.xz`: all 80 `WAYLAND_DEBUG=client` logs.
- `hardware.txt`, `order.txt`, `status.txt`, `source-commit.txt`, binary hashes.
