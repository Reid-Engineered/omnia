# T1.6 Orion gate evidence, 2026-11-17

The campaign ran the [merged gate script](../gate.sh) from a clean worktree
at the commit in [source-commit.txt](source-commit.txt). The owner was at
Orion and closed the other apps. The script was launched over SSH into the
active local Wayland session with `kde-inhibit --power --screenSaver` and the
release binary selected through `BENCH`. The panel was set to 100% before the
campaign and restored to its previous 125% afterwards.

- [summary.txt](summary.txt) contains the protocol and budget verdicts and
  the timeline for the one over-limit present interval.
- [raw-reports.tar.xz](raw-reports.tar.xz) contains the 80 original JSON
  reports and their 80 output logs.
- [order.txt](order.txt) records the rotating case order. The
  [100%](s100/runs.jsonl) and [125%](s125/runs.jsonl) condition snapshots
  record the panel, AC state, governor, and profile before and after each run.
- Process snapshots at [100%](s100/processes.txt) and
  [125%](s125/processes.txt) retain PID, owner, and command name. The original
  snapshots with full command arguments are on Orion.
- The [source status](source-status.txt) was captured after the script
  created its output directory. The [before](binary.sha256) and
  [after](binary-after.sha256) binary hashes match.

To reproduce the verdict from the archived reports, run from the repository
root with Bash, tar, and Python 3.

```sh
evidence=docs/measurements/T1.6/attributed-2026-11-17
replay=$(mktemp -d)
cp -R "$evidence/s100" "$evidence/s125" "$replay/"
tar -xJf "$evidence/raw-reports.tar.xz" -C "$replay"
python3 -B docs/measurements/T1.6/summarize.py "$replay"
```
