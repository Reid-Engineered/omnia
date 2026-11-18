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
- slow, no faults: cpu carries the excess without that, so execution itself was slow. When
  the hardware counters are present (user-space cycles, instructions and reference cycles,
  not multiplexed), it is split against the local medians of those counters:
  - more work: instructions >= 1.5x local;
  - low clock: cycles < 1.5x local (same work, same cycles, slower cycles);
  - low IPC: otherwise (same work, many more cycles: the core was starved, e.g. by the SMT
    sibling or by memory and cache contention with the GPU).
  The clock column is cycles / reference cycles x NOMINAL_GHZ (the i5-7300U's 2.6 GHz).

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
           "end_cpu_cur_freq_khz", "user_cycles", "user_instructions", "user_ref_cycles",
           "counters_multiplexed"]


NOMINAL_GHZ = 2.6


def neighbours(measured, index):
    return measured[max(0, index - 10):index] + measured[index + 1:index + 11]


def local_wall(measured, index):
    return median(r["wall_ns"] for r in neighbours(measured, index))


def local(measured, index, key):
    values = [r[key] for r in neighbours(measured, index) if r.get(key) is not None]
    return median(values) if values else None


def counters(row, measured, index):
    """(instructions ratio, cycles ratio, clock GHz, IPC, local clock GHz), or None."""
    keys = ("user_cycles", "user_instructions", "user_ref_cycles")
    if row.get("counters_multiplexed") or any(not row.get(k) for k in keys):
        return None
    cycles, instructions, ref = (local(measured, index, k) for k in keys)
    if not (cycles and instructions and ref):
        return None
    return (row["user_instructions"] / instructions, row["user_cycles"] / cycles,
            NOMINAL_GHZ * row["user_cycles"] / row["user_ref_cycles"],
            row["user_instructions"] / row["user_cycles"], NOMINAL_GHZ * cycles / ref)


def classify(row, local, base, hw):
    excess = row["wall_ns"] - local
    off_cpu = row["wall_ns"] - row["cpu_ns"]
    faults = row["minor_faults"] + row["major_faults"]
    if off_cpu >= excess / 2:
        if row["involuntary_switches"] or row["runqueue_wait_ns"] >= off_cpu / 2:
            return "preempted"
        return "blocked"
    if faults >= 10 * max(base["faults"], 1):
        return "slow, faulting"
    if hw is None:
        return "slow, no faults"
    instructions, cycles = hw[0], hw[1]
    if instructions >= 1.5:
        return "slow: more work"
    if cycles < 1.5:
        return "slow: low clock"
    return "slow: low IPC"


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
            hw = counters(r, measured, index)
            kind = classify(r, local, base, hw)
            totals[kind] = totals.get(kind, 0) + 1
            interval = next((ns for i, ns in late if i == index), None)
            details.append(
                f"  {name} frame {index}: {kind}; wall {r['wall_ns'] / 1e6:.2f} ms (local "
                f"{local / 1e6:.2f}), cpu "
                f"{r['cpu_ns'] / 1e6:.2f}, run-queue wait {r['runqueue_wait_ns'] / 1e6:.2f}, "
                f"switches {r['voluntary_switches']}v/{r['involuntary_switches']}i, faults {r['minor_faults']}+{r['major_faults']}, cpu "
                f"{r['cpu_start']}->{r['cpu_end']}, freq {r['end_cpu_cur_freq_khz'] / 1e3:.0f} MHz"
                + (f"; instructions x{hw[0]:.2f}, cycles x{hw[1]:.2f}, clock {hw[2]:.2f} GHz "
                   f"(local {hw[4]:.2f}), IPC {hw[3]:.2f}" if hw else "; no hardware counters")
                + (f"; present interval {interval / 1e6:.2f} ms" if interval else ""))
    print("\nStalls")
    print("\n".join(details) or "  none")
    print("\nTotals:", totals or "no stalls")


if __name__ == "__main__":
    main()
