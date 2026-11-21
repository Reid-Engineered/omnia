"""A1.1: summarize the fork_10k campaign written by run.ps1 (canvas-plan.md section 2.1).

Per pass: p50 and p99 (nearest rank) of the 100 observations, plus heap bytes and
allocations of one live fork. Across passes: the median of run p99s and its range.
Usage: python summarize.py <raw directory>
"""

import csv
import math
import statistics
import sys
from pathlib import Path


def nearest_rank(sorted_ns, pct):
    idx = max(math.ceil(len(sorted_ns) * pct / 100) - 1, 0)
    return sorted_ns[idx]


def main(raw):
    raw = Path(raw)
    rows = []
    for path in sorted(raw.glob("*-fork_10k.csv")):
        with path.open(newline="") as f:
            data = list(csv.DictReader(f))
        ns = sorted(int(r["ns"]) for r in data)
        rows.append(
            (
                path.stem,
                len(ns),
                nearest_rank(ns, 50) / 1e6,
                nearest_rank(ns, 99) / 1e6,
                ns[-1] / 1e6,
                int(data[0]["fork_heap_bytes"]),
                int(data[0]["fork_allocs"]),
            )
        )
    if len(rows) != 10:
        print(f"INCOMPLETE: {len(rows)} of 10 passes")
    print("pass,n,p50_ms,p99_ms,max_ms,fork_heap_bytes,fork_allocs")
    for r in rows:
        print(f"{r[0]},{r[1]},{r[2]:.3f},{r[3]:.3f},{r[4]:.3f},{r[5]},{r[6]}")
    p99s = [r[3] for r in rows]
    p50s = [r[2] for r in rows]
    sizes = sorted({r[5] for r in rows})
    allocs = sorted({r[6] for r in rows})
    print(f"median_of_run_p50_ms={statistics.median(p50s):.3f} range=[{min(p50s):.3f}, {max(p50s):.3f}]")
    print(f"median_of_run_p99_ms={statistics.median(p99s):.3f} range=[{min(p99s):.3f}, {max(p99s):.3f}]")
    print(f"fork_heap_bytes={sizes} fork_allocs={allocs}")


if __name__ == "__main__":
    main(sys.argv[1])
