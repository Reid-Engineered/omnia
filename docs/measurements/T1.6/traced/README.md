# T1.6 traced diagnostic campaign

Preparation for gate-blocker-audit.md section 4.1, under canvas-plan section 2.1
and the owner’s B3/B4 missed-refresh decisions. This is diagnostic evidence with
tracing overhead, never a gate verdict. T1.6 remains blocked. No Orion run has
been performed by this preparation. The untraced `../gate.sh` is unchanged.

At Orion’s desk in the local Arch Linux / KDE Wayland session, prepare a clean
checkout and build `cargo build --release --locked -p omnia-bench` from it. The
runner records checkout identity and executable SHA-256, not an embedded build
attestation; the owner confirms that this binary belongs to this source. Use the
native eDP-1 panel alone, 1920x1080 at approximately 60 Hz, initially 100% scale.
Plug in AC. Close other user apps, including editors, browsers and chat clients;
leave normal desktop services alone. Keep the launching terminal for the prompts,
and leave every fullscreen bench window in the foreground. Do not rebuild or edit
the checkout during the campaign. Preflight requires at least **1 GB free** on
the evidence filesystem. Expect about **1.2 MB per client log**, roughly **100 MB
per campaign** in client logs, plus JSON traces and condition inventories.

Run this **one command from the repository root**:

```sh
bash docs/measurements/T1.6/traced/run.sh
```

Preflight checks Linux, local Wayland, an interactive terminal, the release
executable, required tools, Vulkan availability, AC, readable CPU governors,
panel mode/refresh/scale and process inventory before creating evidence. Governor
and power profile are recorded, never changed; unavailable profile interfaces are
explicit. Required tools include `python3`, `sha256sum`, `vulkaninfo`,
`kscreen-doctor`, `kde-inhibit`, `timeout`, `git` and ordinary shell utilities.
Confirm `RUN` only when ready. Sleep inhibition lasts for this command.

Allow roughly **15–20 minutes plus the scale-switch pause** (tracing can add
overhead). The fixed order is start/pan/zoom/idle rotated each of ten passes,
first at 100%, then at 125%: 80 total invocations, 40 motion runs. Arguments match
the gate: empty sheet, start ten samples, motion warmup 60 / measured frames 600,
idle ten seconds. Every invocation has `OMNIA_BENCH_TRACE=1` and
`WAYLAND_DEBUG=client`. Only `run` supports `diagnostic_trace`; start and idle
still preserve their JSON and client log.
`WAYLAND_DEBUG` writes synchronously to stderr and adds diagnostic overhead,
most visibly to B1 start and B8 idle; these results cannot certify those budgets.

At the pause, use display settings to set eDP-1 to **125%**, close settings,
return to the terminal and press Enter. The script verifies 1536x864 logical
viewport conditions before continuing; a wrong scale aborts rather than timing
the wrong leg. It changes no display settings itself. After completion or abort,
restore 100% yourself if desired; the panel stays at its current scale.

Results go to a unique `traced/attempt-XXXXXXXX/`: frozen order, script copies,
source commit, binary hashes, hardware inventory, global and per-run before/after
conditions (AC, governor/profile, full display state, process inventory), exit
codes, JSON, stdout and per-run `.wayland.log`, and per-motion analysis. Scheduled
misses do **not** stop the campaign: retain the first miss and subsequent results.
A crash, 120-second per-invocation timeout (15-second termination grace), interrupt,
lost AC, invalid environment/report or changed binary/source stops early. The
EXIT trap preserves an `incomplete` status and final conditions. Do not resume,
discard data, swap cases or launch another campaign to obtain a clean result.
An existing `attempt-*` triggers a one-line notice before readiness confirmation;
do not start another campaign without owner direction. Analysis failures are
recorded as `analysis-error` in `analysis-errors.txt` and do not abort the schedule.
Foreground timeout handling lets Ctrl-C reach the bench promptly.

After all 80 invocations, the copied gate summarizer runs against the captured
directories; its output and exit status are explicitly diagnostic. It is never
run on an incomplete dataset. Analysis prints `not reproduced in 40 motion runs`
only when `status.txt` reads `complete`, exactly 40 usable scheduled motion traces
exist, and none has a raw miss or aggregate/raw disagreement. A partial directory
or individual JSON always prints an explicit incomplete-dataset notice. Nonzero
aggregate misses with zero raw misses are flagged as possible bucket-top overcount
(audit 2.3.5), and that run is not counted as clean. Completion describes data
collection; recorded analysis errors still require inspection.

Reanalyze on any machine with standard-library Python:

```sh
python3 docs/measurements/T1.6/traced/analyze.py docs/measurements/T1.6/traced/attempt-XXXXXXXX
python3 docs/measurements/T1.6/traced/test_analyze.py
```

The analyzer locates measured indices after warmup, follows pending-draw replacement,
checks trace completeness and raw present-end intervals, and sorts actual timestamps
around every miss (signal and dirty may occur in either order). It prints platform
present duration and flags index 0 for inspection, without attributing a cause.
Missing traces in old s100/s125 data are reported without a false clean verdict.
The committed fixture is invented test data, not a measurement.

Trace creation and client logs have separate clock origins. Correlation requires
a unique contiguous sequence of surface commits that fits **all** platform-present
brackets; it reports the resulting offset bounds and nearby frame callbacks and
presentation feedback, if present. Ambiguous/unmatched logs print callback traffic
on its own clock and explicitly say correlation is unresolved. This conservative
association is not proof of callback origin, scan-out, compositor delay or client
descheduling. Scheduler/compositor evidence may still be required (audit 2.3).
Both numeric millisecond and HH:MM:SS client timestamps are accepted, including
midnight rollover; archived gzip client logs can be correlated directly.

Send back the **whole attempt**, even if incomplete, including all logs and status:

```sh
tar -czf t16-traced-attempt.tar.gz -C docs/measurements/T1.6/traced attempt-XXXXXXXX
```

Process arguments may contain private local information; review the archive before
sharing while preserving the original evidence on Orion.
