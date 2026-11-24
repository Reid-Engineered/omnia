#!/usr/bin/env python3
"""Classify the slow draws in a T4.6 stall diagnostic (run.py). Evidence only.

For every run: frame p99 and maximum, T1.6's attribution at the run's gate limit (1.5
refreshes on 1k, 2.5 on 10k) and the count over 1.5 refreshes, and the CPU time every other
process used during the run (from the journal's /proc snapshots, in seconds). For every
measured draw over 10 ms: the probe's sheet paint wall and CPU time, run-queue wait,
involuntary switches, effective clock (user cycles / reference cycles x 2.6 GHz) and CPU.
A draw whose paint wall is mostly run-queue wait was preempted: the thread was runnable but
had no CPU. Standard library only.

Usage: python3 -B analyze.py OUTPUT_DIR
"""

from collections import Counter
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "T1.6"))
from attribute import attribute

NOMINAL_GHZ = 2.6
TICKS = 100  # USER_HZ
UNAVAILABLE = 2**64 - 1


def other_cpu(entry):
    before, after = entry["before"]["cpu_ticks"], entry["after"]["cpu_ticks"]
    used = Counter()
    for pid, (comm, ticks) in after.items():
        if "omnia-bench" not in comm:
            used[comm] += max(0, ticks - before.get(pid, [comm, 0])[1])
    return used


def main():
    root = Path(sys.argv[1])
    journal = [json.loads(line) for line in (root / "runs.jsonl").read_text().splitlines()]
    totals = Counter()
    for entry in journal:
        stem = f"{entry['case']}-{entry['rep']}"
        report = json.loads((root / f"{stem}.json").read_text())
        limit = 1.5 if entry["case"].startswith("1k") else 2.5
        gate, wide = attribute(report, limit), attribute(report, 1.5)
        used = other_cpu(entry)
        top = ", ".join(f"{comm} {ticks / TICKS:.2f}" for comm, ticks in used.most_common(4))
        frame = report["metrics"]["frame_ms"]
        print(f"{stem}: p99 {frame['p99']:.3f} ms, max {frame['max']:.3f} ms; over {limit} R: "
              f"{gate['aggregate']}, counted {gate['counted']}; over 1.5 R: {wide['aggregate']}; "
              f"other CPU {sum(used.values()) / TICKS:.2f} s ({top})")
        probe = report["paint_probe"]
        rows = [dict(zip(probe["columns"], row)) for row in probe["rows"]]
        draws = [e for e in report["diagnostic_trace"]["events"] if e["event"] == "draw"]
        start = len(draws) - report["config"]["frames"]
        for index, draw in enumerate(draws[start:]):
            ms = (draw["end_ns"] - draw["start_ns"]) / 1e6
            if ms <= 10:
                continue
            row = rows[start + index]
            clock = ("n/a" if row["user_ref_cycles"] in (0, UNAVAILABLE) else
                     f"{row['user_cycles'] / row['user_ref_cycles'] * NOMINAL_GHZ:.2f} GHz")
            preempted = row["runqueue_wait_ns"] >= (row["wall_ns"] - row["cpu_ns"]) / 2 > 0
            totals["preempted" if preempted else "other"] += 1
            print(f"  frame {index}: draw {ms:.2f} ms; paint wall {row['wall_ns'] / 1e6:.2f}, "
                  f"CPU {row['cpu_ns'] / 1e6:.2f}, run-queue wait "
                  f"{row['runqueue_wait_ns'] / 1e6:.2f} ms; {row['involuntary_switches']} "
                  f"involuntary switches; clock {clock}; CPU {row['cpu_start']}->{row['cpu_end']}; "
                  f"{'preempted' if preempted else 'not preempted'}")
    print(f"draws over 10 ms: {sum(totals.values())} ({totals['preempted']} preempted, "
          f"{totals['other']} other)")


if __name__ == "__main__":
    main()
