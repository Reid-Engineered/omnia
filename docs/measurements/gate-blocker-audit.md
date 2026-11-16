# Gate-blocker audit: T1.6 (B3) and T2.8 remaining acceptance evidence

2026-11-16. Offline, source-grounded audit of the two remaining reference gates against
the written plan and roadmap at main `950961d` (PR #27 merged). This document changes no
gate, threshold, plan or roadmap text; it records what the written requirements actually
say, what the existing evidence does and does not establish, and the smallest
fixed-schedule experiment that would close each gap. GPUI citations are against the pinned
revision `15d136bf8e72ae43b284183ec46940c009460f5c`
(`~/.cargo/git/checkouts/zed-a70e2ad075855582/15d136b`). Historical file:line references
below identify that audited snapshot. Review corrections also account for T5.1,
merged at `e81aab6`: public `Doc::apply`, undo and redo are now available for
whole-command measurements. No protocol exception is adopted by this audit.

## 1. What the gates actually say

- B3 (canvas-plan.md:84): pan/zoom on 1k, p99 frame time < 16.7 ms **and zero missed
  refreshes: no present interval longer than 1.5 refresh intervals**, judged per run from
  `missed_refreshes` (owner decision 2026-11-16, canvas-plan.md:105-108). T1.6's done-when
  (canvas-roadmap.md:268-269) requires all four budgets to pass on the reference machine;
  `docs/measurements/T1.6/summarize.py:85-100,135` exits 1 if any run has a nonzero miss.
  The all-run zero-miss rule is unchanged and is not relaxed here.
- D8 (canvas-plan.md:284-285) is the **only** "1 ms" text in the plan: "Incremental
  connectivity runs inline with the command; target under 1 ms per command at 10,000
  parts." Note "target" per *command*, not a hard B-budget.
- T2.8 done-when (canvas-roadmap.md:322-325): `load(save(doc)) == doc`; INV10 property +
  internal consistency pass after every step; `Doc::query` allocates nothing with capacity;
  "benches record whether one add and one remove stay under 1 ms on `10k` and in the worst
  cases." T4.1's done-when (canvas-roadmap.md:410-411) is the same 1 ms on `10k` for one add
  and one remove. T5.1's done-when (canvas-roadmap.md:472-474) adds "one `Move` of 100 items
  on `10k` applies in under 1 ms" — whole-command timing inside `Doc::apply` (INV4,
  canvas-plan.md:304; INV10, canvas-plan.md:310).
- Section 2.1 (canvas-plan.md:60-79), made binding by canvas-plan.md:55 ("Every timed run
  follows the reference protocol in section 2.1"): Adapter :65-67, Viewport :68-71,
  Refresh :71, **Power and noise** :72-73 ("On AC, the bench window in the foreground, other
  apps closed. Each result records the power source, CPU governor and power profile."),
  **Repetitions** :74-75 ("Ten repetitions per case, interleaved (case order rotates each
  pass). The gate uses the median of run p99s and reports the range."), Windows :76-79.
  canvas-plan.md:93: a number from any other machine "never marks a budget as met".
- Roadmap rule 4 (canvas-roadmap.md:27-30): "Only the reference machine (Q4: the Orion
  laptop on Linux, under plan section 2.1) can mark a budget met. On any other machine, or
  on it outside the section 2.1 protocol, a gate task ends as `needs-human`, with the
  numbers recorded."

## 2. T1.6 / B3: the 49.7746 ms interval

### 2.1 Failing evidence (unchanged)

Clean Orion rerun t16-gate2 (roadmap log :1284-1306; docs/measurements/T1.6.md): 39/40
motion runs have zero misses; `docs/measurements/T1.6/s125/pan-1.json` has
`missed_refreshes=1`, `max_present_interval_ms=49.7746`,
`present_intervals_over_2_5_refreshes=1`, `late_frames=0`, `work_missed_refreshes=0`,
`frame_ms` p99 1.9446 / max 3.0802, `dirty_to_present_ms` max 5.3371, no warnings, and **no
`diagnostic_trace`** (untraced run). At 60.008 Hz, 1.5 refreshes = 24.997 ms
(T1.6.md "Budget verdict"). The eight diagnostic runs (six traced, two untraced; T1.6-debug.md) all had zero
misses; largest raw traced interval 22.936100 ms at measured index 364 (warmup 600).

### 2.2 Pinned timing semantics (verified against source)

- Present interval is **present_end to present_end**, recorded only when a newly drawn
  frame was presented, the previous present was an active animation frame, and the window
  is active: profiler.rs:1514-1521 (`record_present_at`, profiler.rs:1482-1559);
  `last_present_at` updates only on presented frames (profiler.rs:1558);
  `animating_at_last_present = next_frame_scheduled && window_active` (profiler.rs:1552).
- Work starts at the **later of signal and dirty** and ends at **present start**:
  profiler.rs:891-908 (`work_start`), profiler.rs:1131 (`submitted_at =
  presentation.present_start`). Waiting for the platform frame signal is excluded from
  `work` however late it fires.
- `present()` brackets the whole platform draw call: window.rs:3964-3980
  (`present_start = Instant::now()` :3968, `platform_window.draw(...)` :3970,
  `record_present` :3972). On Wayland/wgpu that call includes surface acquire
  (wgpu_renderer.rs:1087 `draw`, :1130 `surface.get_current_texture()`) and
  `queue.present` (wgpu_renderer.rs:1165). Present mode is Mailbox preferred
  (wayland/window.rs:587-588); frames are callback-driven (wayland/window.rs:1019
  `frame_callback_fired`, :2050 `draw`) with a local retry timer at
  FRAME_RETRY_INTERVAL = 16.667 ms (wayland/client.rs:200, :645).
- Next-frame callbacks run before the draw decision in the frame handler:
  window.rs:2040-2056.
- Journal validity filtering applies to the pending frame, dirty timestamp and previous
  present timestamp (profiler.rs:1504-1521; journal.rs `frame_sample_is_valid`). The
  aggregate counts and absence of warnings do not reconstruct every sample's validity
  history or establish that the longest interval has a usable dirty timestamp.

### 2.3 Findings (evidence-supported; no causal claim)

1. **No cause is established for the interval.** Work max 3.0802 ms and
   dirty-to-present max 5.3371 ms are separate aggregate summaries, not matched
   timestamps for the frame ending the 49.7746 ms interval. If that frame has a
   valid dirty sample, its dirty-to-present span would be bounded by the latter
   maximum, placing roughly 44.4 ms before dirtying. The report does not establish
   that condition. Work excludes waiting for the signal and the platform present
   call (profiler.rs:891-908,1131), so low work alone cannot locate the delay.
   A matched failing trace is needed; no scheduling, compositor or rendering cause
   is inferred from these summaries.
2. **Discriminant gap 1 — the trace has no frame-request source.** The diagnostic trace
   records signal/dirty/draw/present timestamps and the raw interval
   (crates/omnia-bench/src/trace.rs:34-47) but not `FrameRequestSource`
   (native callback vs the 16.667 ms local retry timer, wayland/client.rs:200), so even a
   failing traced run cannot by itself separate "compositor callback late" from "client
   event loop descheduled / retry timer drove the frame". WAYLAND_DEBUG=client
   adds callback correlation, but its client-side timestamps alone may still be
   insufficient to distinguish server delay from client descheduling. Correlated
   scheduler/compositor evidence may be required.
3. **Discriminant gap 2 — the failing run is untraced, so the miss index is unknown.**
   pan-1.json carries no `diagnostic_trace`; only a failing traced run can locate the
   miss. All six traced diagnostic runs were clean (T1.6-debug.md), so no failing trace exists.
4. **Warmup-baseline boundary (bench-side, bounded, unproven).** The baseline snapshot is
   taken in an `on_next_frame` callback between the last warmup frame and the first
   measured frame (harness.rs:172-177 comment, :185-186 `frame + 1 == warmup`,
   :246-256). GPUI runs next-frame callbacks before dirtying/drawing (window.rs:2040-2056),
   so the baseline cost is excluded from the first measured frame's work/draw/
   dirty-to-present — but the present interval from the last warmup present to the first
   measured present **includes** that callback. A slow baseline callback would inflate
   exactly measured index 0. Never observed (no index-0 miss in any traced run); testable
   only via a failing trace. This is documented in T1.6-debug.md as intentional existing
   behavior, not a demonstrated bug.
5. **Minor measurement imprecision (over-count only).** `count_over_refreshes`
   (crates/omnia-bench/src/stats.rs:72-82) counts a sample when the **top of its histogram
   bucket** exceeds the limit (`value_iterated_to() > limit`, :80), so intervals within
   0.1% below 24.997 ms can be counted as misses; it never under-counts (comment :72-75).
   It cannot explain 49.7746 ms; `missed_refreshes` is an upper bound, not exact.
   Reported at harness.rs:344, :397.
6. **Configured policy caps are disabled in the bench.** The bench sets
   `inactive_frame_interval = low_power_frame_interval = thermal_frame_interval = None`
   (harness.rs:93-96, overriding the platform defaults at platform.rs:2850-2852); on Orion
   `power_mode` stays `Unknown` (default, platform.rs:552-553; no power-profiles daemon),
   so `minimum_frame_interval` returns `None` (window.rs:1374-1396) and `should_defer` is
   false for that configuration (window.rs:1351-1372, called at :1999). This excludes
   the named configured caps as an explanation, not every runtime source of pacing delay.
7. **Sample-level validity remains unobserved.** The failing run records 600 draws,
   zero unmeasured work frames and no warnings. Those aggregates are useful checks,
   but do not replace the matched event/dirty-timestamp evidence described above.

**Conclusion for T1.6:** the gate stays blocked under the unchanged all-run zero-miss
rule; no render/pacing fix is justified by current evidence; the discriminants that would
answer the unknowns are a failing traced run plus Wayland callback traffic, and the
index-0 baseline hypothesis. No subset rerun or aggregation change is accepted.

## 3. T2.8: remaining acceptance gap

### 3.1 What was measured vs what the text targets

- All Orion T2.8 numbers (T2.8-reference.md, T2.8-optimization.md) drove `Connectivity`
  directly: `conn.add_wire` / `conn.remove_wire`
  (docs/measurements/T2.8-reference/runner/main.rs:14,17; the in-repo
  crates/omnia-bench/benches/connectivity.rs:14,17 likewise). `Doc::remove_wire`
  additionally does `index.remove` (crates/omnia-doc/src/doc.rs:330-333;
  `insert_wire` :286-296). So the measured quantity is the connectivity component only;
  the **Doc-level command** that D8's "per command" wording targets (index + connectivity,
  and after T5.1 validation/inverse/undo/dirty-rect inside `apply`) is unmeasured on Orion.
- Numbers: PR26 20k middle remove median run p99 **3.025587 ms** (range 2.858351-3.173120,
  all 10,000 removals >= 1 ms; T2.8-reference.md). PR27 (merged, f551e3c): **0.868915 ms**
  (range 0.736231-0.928822), all ten run p99s < 1 ms, but 26/10,000 individual removals
  >= 1 ms, max 1.553957 ms (T2.8-optimization.md; summary.txt:74). The gate statistic is
  the **median of ten run p99s** (canvas-plan.md:74-75), strict "under"; the `>=1 ms`
  observation counts are diagnostics, not an all-samples gate (T2.8-reference.md
  "Acceptance semantics"). So the numeric target passes **at the connectivity level**;
  the remaining gaps are protocol completeness and Doc-level measurement.

### 3.2 Does section 2.1 require GPU/foreground/two-scale conditions for CPU-only runs?

Exact text: canvas-plan.md:55 makes section 2.1 binding on "every timed run".
The bullets include Adapter/Viewport/Refresh (:65-71), foreground bench window and
other apps closed (:72-73), and ten rotating repetitions (:74-75). There is no
explicit CPU-only exception. Some window/GPU requirements do not map naturally to
a standalone CPU runner, but that ambiguity does not authorize this audit to
waive them or certify an alternative subset of conditions.

A CPU-specific protocol could reasonably retain reference hardware, AC,
governor/profile and process records, other user apps closed, and the same fixed
repetitions/statistic. That is a proposed clarification, not the current accepted
protocol. Until clarified or satisfied, the existing CPU-only results remain
limited evidence. Konsole was an open user app; its presence is a concrete noise
limitation. KDE/Akonadi services are recorded background processes, not automatically
additional user apps that must be terminated. This audit requires neither shutting
down the desktop session nor separate approval for every background service.

### 3.3 Is `needs-human` justified?

Yes. The existing runs do not establish complete section 2.1 compliance and retain
an open user app. Under roadmap rule 4 (canvas-roadmap.md:27-30), the gate task ends
`needs-human` with numbers recorded — which is exactly what T2.8-optimization.md does.
Independently, the measured quantity (connectivity-only) is not the Doc command D8/T5.1
target, so even a protocol-perfect connectivity run would not close D8's "per command".

### 3.4 Precise evidence required to close T2.8

1. Meet the accepted reference protocol on Orion, or first record an explicit
   CPU-only clarification. Record AC, governors/profile and the process inventory;
   close other user apps. Normal desktop services must not be silently reclassified
   as user apps or terminated merely to satisfy this audit.
2. Use ten rotating interleaved passes per case, with 1,000 observations per operation
   per pass and the existing timeout/grace. Retain the Connectivity-only cases for
   comparison, and add public `Doc::apply` cases for effective add/delete and Move
   of 100 items on the actual 10k workload. T5.1 has now landed. Time the command
   boundary, including validation, index/connectivity maintenance and inverse/history
   work. Use undo/redo or setup outside the measured span to restore the starting
   workload; document exactly which operation is timed. The historical
   `Doc::insert_wire`/`remove_wire` helpers are crate-private, so an external runner
   must not bypass the command boundary or change visibility just for this measurement.
3. Evaluate the median of all ten run p99s against the unchanged strict 1 ms target
   for the inline edit cases; report the range and diagnostic `>=1 ms` counts. Keep
   workload and case definitions explicit: T2.8 asks to record worst-case behavior,
   while D8 targets per-command latency and T5.1 requires Move-of-100 latency.
   Do not introduce a new all-samples gate or claim that one Connectivity-only
   statistic satisfies all these acceptance conditions.
4. Functional items via `cargo test --workspace`: `load(save(doc)) == doc`, INV10 property
   + internal consistency after every step, `Doc::query` alloc-free with capacity
   (roadmap:322-325).
5. Artifacts: raw CSVs, order.txt, before/after condition snapshots (AC, governors,
   loadavg, process inventory), source and binary SHA-256, the fixed run.sh, and
   summarize.py output.

### 3.5 Housekeeping observation (not changed here)

The T2.8 heading (canvas-roadmap.md:310) still reads "blocked (20k split exceeds 1 ms on
Orion; full protocol confirmation pending)". PR27 (f551e3c) did not touch
canvas-roadmap.md (last roadmap commit de4e45c); the heading predates the 0.868915 ms
numeric pass and is stale relative to T2.8-optimization.md, which is the current authority
(needs-human). T5.1 has since merged, but this audit still leaves roadmap status
changes to a separate correction; no gate is declared complete here.

## 4. Smallest fixed-schedule acceptance experiments

### 4.1 T1.6 (needs Orion at the desk, apps closed; ready for when the laptop is available)

- Schedule: the unchanged `docs/measurements/T1.6/gate.sh` campaign — 4 cases x 10
  interleaved rotating repetitions at 100% then 125%, warmup 60, frames 600 — but with
  **every run** launched with `OMNIA_BENCH_TRACE=1` and `WAYLAND_DEBUG=client`, capturing
  a per-run Wayland log. No code change; the trace already exists (trace.rs).
- Stop conditions: finish the fixed campaign, retaining the first failing trace and
  all subsequent results. If all 40 motion runs are clean, stop and report "not
  reproduced in 40 motion runs" (80 total case invocations across both scales).
  A timeout, crash or unsafe condition may stop it early; label that diagnostic
  dataset incomplete. No automatic second campaign or until-green retries. Even a
  complete traced campaign is diagnostic evidence, not an untraced gate verdict.
- Artifacts: per-run JSON with `diagnostic_trace`, per-run WAYLAND_DEBUG log, before/after
  condition snapshots (AC, governors, scale, process inventory), binary SHA-256 and source
  commit and per-run trace analysis. Run the gate summarizer only on complete datasets.
- Analysis on a captured miss: locate the measured index and order the actual
  signal/dirty/draw/present timestamps; signal and dirty need not occur in a fixed
  order. Correlate Wayland callback traffic and inspect platform-present duration
  and the index-0 baseline hypothesis. Client logs alone may leave scheduler versus
  compositor attribution unresolved; collect correlated evidence before claiming a cause.

### 4.2 T2.8 (CPU-only; only Orion certifies, diagnostics anywhere)

- Schedule: the same fixed ten-pass rotating structure, 1,000 observations per
  operation and existing timeout/grace, with case selection extended to the public
  command-boundary measurements in section 3.4. Freeze the case list and source/binary
  before the first timed run; document setup/undo outside each measured operation.
- Stop conditions: complete all ten scheduled repetitions per case before calculating
  the acceptance median of ten run p99s. An interim p99 or interim median does not
  establish the final verdict. A timeout, invalid sample, crash or unsafe condition
  may abort the campaign; retain all completed data and report it as incomplete or
  invalid, not a completed ten-run timing pass/fail. Do not retry until green, discard
  observations, or replace cases after seeing results.
- Artifacts: raw CSVs, order.txt, before/after snapshots (AC, governors, loadavg, process
  inventory), source/binary hashes, run.sh, summarize.py output; functional checks via
  `cargo test --workspace`.

## 5. Objections to previous assumptions, risks, next tasks

- Objection: "the 49.77 ms miss was caused by the compositor or another process" — too
  strong; T1.6-debug.md already retracts it. The interval includes signal wait and the
  platform present call by construction (profiler.rs:891-908, 1131, 1514-1521); no
  evidence separates them yet (findings 2.2.1-2.2.3).
- Objection: declaring CPU-only tests exempt from window/two-scale requirements
  without an accepted clarification. Section 2.1 says "every timed run"; record
  the ambiguity instead of silently changing the protocol (section 3.2).
- Objection: reading T2.8's 1 ms as satisfied by connectivity-only numbers — D8 says "per
  command" (plan:284-285) and T5.1's done-when times the whole `apply`; Doc-level timing is
  still missing (section 3.1).
- Risk: `missed_refreshes` is an upper bound (bucket-top counting, stats.rs:72-82); a
  future borderline miss near 24.997 ms should be re-checked against the raw trace before
  any conclusion.
- Risk: traced runs carry diagnostic overhead (T1.6-debug.md), so a traced campaign is
  evidence-gathering, not itself a gate pass; the gate rerun must stay untraced per
  gate.sh, with tracing used only in the bounded diagnostic campaign above.
- Next tasks: (1) prepare public command-boundary cases using the now-merged T5.1;
  (2) resolve CPU-only protocol scope before claiming acceptance; (3) run the bounded
  diagnostic and fixed acceptance schedules when Orion is available; (4) refresh the
  stale T2.8 heading separately, without treating the numeric improvement as a full pass.
