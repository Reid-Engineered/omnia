# Working in this repository

Standards and operating rules for anyone, human or agent, changing Omnia. They apply to
every task. The roadmaps add task-specific rules; where a roadmap is stricter, it wins.

## Sources of truth

Read these before you change anything they cover. In order of authority:

| File | What it decides |
| --- | --- |
| `canvas-plan.md`, `wiring-plan.md`, `mcp-plan.md`, `ui-plan.md` | Budgets, constants, decisions, invariants, open questions with defaults |
| `canvas-roadmap.md`, `wiring-roadmap.md`, `mcp-roadmap.md`, `ui-roadmap.md` | Tasks, their status, dependencies, owned files, done-when checks, the log |
| `ROADMAP.md` | Product stages above the technical roadmaps |
| `docs/measurements/` | Recorded numbers and gate evidence |

Ids are global: `B`, `K`, `D`, `INV`, `Q`, `T` live in the canvas files; `W`-prefixed ids
(`WB`, `WK`, `WD`, `WINV`, `WQ`, `TC`, `M`) live in the wiring files; `A`-prefixed ids
(`AB`, `AK`, `AD`, `AINV`, `ATC`, `AQ`, `HA`, tasks `A0.1`…) live in the agent-control files
(`mcp-plan.md`, `mcp-roadmap.md`); `U`-prefixed ids (`UB`, `UK`, `UG`, `UD`, `UINV`, `UTC`,
`US`, `UQ`, checkpoints `UH`, tasks `U0.1`…) live in the application UI files (`ui-plan.md`,
`ui-roadmap.md`). Cite ids in commits, pull requests and log entries.

The wiring files name crates `schematic-doc`, `schematic-canvas` and `schematic-bench`.
Those are this repository's `omnia-doc`, `omnia-canvas` and `omnia-bench`.

## Hard rules

1. **No other companies.** Do not name other companies or their products beyond a brief nod
   to a feature. Scan every plan, doc and pull request for this before you commit it.
2. **Never make a check pass by weakening it.** No loosened thresholds, tolerances, budgets,
   property-test ranges, lints or `#[allow]` added to get green. If you cannot fix the
   cause, set the task to `blocked (reason)` and log what you measured.
3. **Only the reference machine certifies a budget.** That is the Orion laptop (Linux,
   Wayland, wgpu/Vulkan), run under canvas plan section 2.1. Numbers from any other machine
   are recorded and labelled, and never mark a budget met.
4. **Stop at human checkpoints** (`H*`, `HW*`). Do not work past one or answer it yourself.
5. **Open questions have defaults.** Build to the default in the plan's open-questions
   section until the owner's dated answer replaces it. Do not pick a different answer.

## How a task runs

1. Pick a `todo` task whose dependencies are all `done`. A dependency that is `needs-human`
   is not `done`.
2. Branch from current `main`: `task/<id>-<slug>`, for example `task/W1.2-runs`. Work that is
   not a roadmap task uses `chore/`, `docs/` or `fix/` prefixes. Never commit to `main`.
3. Set the task to `doing` in its roadmap. Read everything its **Read** line names.
4. Change only the files the task **Owns**, plus the `mod` line and `Cargo.toml` entries it
   needs. A change needed elsewhere is logged for the task that owns that file.
5. Run the standard checks and the task's **Done when** checks.
6. Set the status (`done`, `blocked (reason)` or `needs-human`) and append one log entry at
   the end of the roadmap, newest last, in the format shown there.
7. Open one pull request per task.

**Keep the plans true.** A change to a constant, decision or signature updates the plan in
the same pull request (canvas rule 12, wiring rule 16).

## Standard checks

Every pull request passes these, as CI runs them (`.github/workflows/ci.yml`):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p omnia-bench --all-targets --features scene-counts,readback -- -D warnings
cargo test --workspace
cargo deny check
```

Say in the pull request which checks you ran locally and which you left to CI.

## Code standards

- **Toolchain and GPUI are pinned.** Rust is pinned by `rust-toolchain.toml` (edition 2024).
  GPUI is pinned to one revision in `Cargo.toml`. Changing either is its own task.
- **Read GPUI, don't recall it.** Open the API at the pinned revision before you use it.
- **Lanes.** `omnia-doc` has no UI framework dependency and stays that way. `omnia-canvas`
  and `omnia-app` own drawing and input. `omnia-bench` owns measurement. `omnia-theme` owns
  every colour: there are no colour literals in `omnia-canvas` (INV6).
- **Determinism.** The same inputs give the same document, route, snap and plan. Break ties
  by geometry and then lowest id. Never let hash-map or input order decide. A property test
  that rebuilds the input in another order is the usual proof.
- **Hot paths do not allocate** once their buffers are warm, where the plan says so. Prove it
  with the allocation-counting tests and benches, not by inspection.
- **Commands are the only way to edit a document.** Validation, index and connectivity
  updates, and the inverse all happen inside `Doc::apply`, and a refused command changes
  nothing. Tests may build fixtures through crate-private inserts. Measurement code uses
  public APIs only and never widens visibility to measure something.
- **Tests state the specified result.** Use property tests for invariants and unit tests for
  `TC` scenarios and edge cases. Cite the invariant or scenario id in the test name or
  comment.
- **GUI runs end by themselves.** Use `omnia-app --frames N` or a `bench` subcommand, never a
  window that waits for a person.
- **Dependencies.** Add no new crate outside the plan's dependency list without logging its
  licence first. `cargo deny` must stay green.

## Measurements

- Write results to `docs/measurements/<task-id>.md` with CPU, GPU, OS, resolution, scale
  factor, refresh rate, power source and governor.
- Follow canvas plan section 2.1 for every timed run: ten interleaved, rotating repetitions;
  the gate reads the median of run p99s and reports the range.
- Fix the schedule, the case list and the binary before the first timed run. Do not retry
  until green, discard runs, or swap cases after seeing results. An aborted campaign is
  reported as incomplete.
- Keep raw data (JSON or CSV), the run script, the order file and before/after condition
  snapshots next to the summary.
- Diagnostic runs (tracing, extra logging) are evidence-gathering, not a gate verdict.
- Golden images render on Windows: Linux has no `render_to_image`.

## Pull requests and review

- Title: `<task id>: <short summary>`, for example `W1.3: snap rules`.
- The body covers what changed, the ids it implements, any open-question defaults relied on,
  deviations from the plan and why, the checks you ran, and measured numbers with their
  hardware.
- Get an independent review before merge, from a different model or person than the author.
  A reviewer reports findings and does not rewrite the change.
- Before merge, update the branch from `main`. Roadmap log entries conflict on almost every
  merge: keep both entries, in date order. Never drop someone else's entry.
- Merge only when CI is green and the task's done-when checks hold. A `needs-human` gate
  merges with its numbers recorded and its status left as `needs-human`.

## Working in parallel

- Tasks in different lanes, or owning different files, can run at the same time. One owner
  per shared file at a time: do not start a task that extends a file whose creating task is
  not yet `done` and merged.
- Use one git worktree per task. Do not edit another task's worktree or branch.
- **Watch disk space.** Every worktree's `target/` grows to several GB. Before starting
  builds, check free space, and delete `target/` in worktrees whose branches are merged. A
  full disk shows up as unexplained build or test failures.
- Do not leave untracked files in the main checkout that a branch might also add: they
  block `git switch`.

## Machines

| Machine | Role |
| --- | --- |
| Orion laptop (Linux, Wayland) | Reference machine; the only one that certifies a budget |
| Windows development machines | Builds, tests, golden images and diagnostics; certify nothing |
| CI (`windows-latest`) | Standard checks; window tests skip themselves on the headless runner |

Reference runs need the owner at the desk with other apps closed. Prepare the scripts so
that a run is one command, and ask for the run rather than approximating it elsewhere.
