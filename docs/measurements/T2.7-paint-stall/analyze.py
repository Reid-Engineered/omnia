#!/usr/bin/env python3
"""Classify paint stalls in a T2.7 paint-stall campaign (run.py). Evidence only.

A stall is a measured frame whose paint wall time exceeds both 2x its local baseline and the
baseline plus 4 ms. The baseline is the median paint wall of the 10 measured frames on each
side (the zoom sweep's normal paint ranges from about 3 to 7.5 ms). Its excess (wall minus
the baseline) is split by the probe:

- preempted: wall - cpu >= half the excess, and an involuntary context switch or run-queue
  wait >= half of (wall - cpu);
- blocked: wall - cpu >= half the excess otherwise (a voluntary switch: asleep, not runnable);
- slow, faulting: cpu carries the excess and minor + major faults >= 10x the run's median;
- slow, no faults: cpu carries the excess without that, so execution itself was slow
  (clocks, SMT sibling or memory contention); the end CPU's cur freq is shown against the
  run's median.

Also prints each run's /proc/vmstat deltas and every present interval over 1.5 refreshes with
its paint. Standard library only.

Usage: python3 -B analyze.py OUTPUT_DIR
"""

import json
from pathlib import Path
from statistics import median
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "T1.6"))
from attribute import timeline

COLUMNS = ["wall_ns", "cpu_ns", "runqueue_wait_ns", "timeslices", "minor_faults",
           "major_faults", "voluntary_switches", "involuntary_switches", "cpu_start", "cpu_end",
           "end_cpu_cur_freq_khz"]


def local_wall(measured, index):
    window = measured[max(0, index - 10):index] + measured[index + 1:index + 11]
    return median(r["wall_ns"] for r in window)


def classify(row, local, base):
    excess = row["wall_ns"] - local
    off_cpu = row["wall_ns"] - row["cpu_ns"]
    faults = row["minor_faults"] + row["major_faults"]
    if off_cpu >= excess / 2:
        if row["involuntary_switches"] or row["runqueue_wait_ns"] >= off_cpu / 2:
            return "preempted"
        return "blocked"
    if faults >= 10 * max(base["faults"], 1):
        return "slow, faulting"
    return "slow, no faults"


def main():
    output = Path(sys.argv[1])
    order = json.loads((output / "order.json").read_text())
    journal = [json.loads(line) for line in (output / "runs.jsonl").read_text().splitlines()]
    totals = {}
    print("run | paints | median paint wall/cpu ms | median freq MHz | stalls | intervals > 1.5 R | "
          "vmstat deltas (non-zero)")
    details = []
    for (rep, sheet, script), record in zip(order, journal, strict=True):
        name = f"{sheet}-{script}-{rep}"
        report = json.loads((output / f"{name}.json").read_text())
        probe = report["paint_probe"]
        assert probe["columns"] == COLUMNS and probe["dropped"] == 0, name
        rows = [dict(zip(COLUMNS, values)) for values in probe["rows"]]
        frames = timeline(report)
        warmup = report["config"]["warmup"]
        if len(rows) != warmup + report["config"]["frames"]:
            raise SystemExit(f"{name}: {len(rows)} paints, expected one per draw")
        measured = rows[warmup:]
        base = {
            "wall_ns": median(r["wall_ns"] for r in measured),
            "cpu_ns": median(r["cpu_ns"] for r in measured),
            "faults": median(r["minor_faults"] + r["major_faults"] for r in measured),
            "freq": median(r["end_cpu_cur_freq_khz"] for r in measured),
        }
        refresh = 1e9 / report["environment"]["refresh_hz"]
        stalls = []
        for i, r in enumerate(measured):
            local = local_wall(measured, i)
            if r["wall_ns"] > max(2 * local, local + 4e6):
                stalls.append((i, r, local))
        late = [(index, present["animation_interval_ns"]) for index, _, present, _, _ in frames
                if present["animation_interval_ns"] > 1.5 * refresh]
        delta = {key: record["after"]["vmstat"][key] - record["before"]["vmstat"][key]
                 for key in record["before"]["vmstat"]}
        print(f"{name} | {len(rows)} | {base['wall_ns'] / 1e6:.2f}/{base['cpu_ns'] / 1e6:.2f} | "
              f"{base['freq'] / 1e3:.0f} | {len(stalls)} | {len(late)} | "
              f"{ {k: v for k, v in delta.items() if v} }")
        for index, r, local in stalls:
            kind = classify(r, local, base)
            totals[kind] = totals.get(kind, 0) + 1
            interval = next((ns for i, ns in late if i == index), None)
            details.append(
                f"  {name} frame {index}: {kind}; wall {r['wall_ns'] / 1e6:.2f} ms (local "
                f"{local / 1e6:.2f}), cpu "
                f"{r['cpu_ns'] / 1e6:.2f}, run-queue wait {r['runqueue_wait_ns'] / 1e6:.2f}, "
                f"switches {r['voluntary_switches']}v/{r['involuntary_switches']}i, faults {r['minor_faults']}+{r['major_faults']}, cpu "
                f"{r['cpu_start']}->{r['cpu_end']}, freq {r['end_cpu_cur_freq_khz'] / 1e3:.0f} MHz"
                + (f"; present interval {interval / 1e6:.2f} ms" if interval else ""))
    print("\nStalls")
    print("\n".join(details) or "  none")
    print("\nTotals:", totals or "no stalls")


if __name__ == "__main__":
    main()
