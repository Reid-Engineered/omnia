"""Recompute run p99s (nearest rank), median of ten, and diagnostic tails."""
import csv
import io
import statistics
import tarfile
from pathlib import Path

root = Path(__file__).parent
groups = {}
with tarfile.open(root / "raw.tar.gz") as archive:
    for member in sorted(archive.getmembers(), key=lambda m: m.name):
        if not member.name.endswith(".csv"):
            continue
        rows = list(csv.DictReader(io.TextIOWrapper(archive.extractfile(member))))
        assert len(rows) == 1000
        case = Path(member.name).stem[3:]
        for field in rows[0]:
            if not field.endswith("_ns"):
                continue
            values = sorted(int(row[field]) for row in rows)
            p99 = values[989] / 1e6
            maximum = values[-1] / 1e6
            misses = sum(ns >= 1_000_000 for ns in values)
            key = f"{case}/{field[:-3]}"
            groups.setdefault(key, []).append((p99, maximum, misses, statistics.mean(values) / 1e6))
            print(f"{Path(member.name).stem}/{field[:-3]}: p99_ms={p99:.6f} max_ms={maximum:.6f} >=1ms={misses}")
    print("\nMedian of 10 run p99s; range; all-observation mean/max/misses (diagnostic):")
    for key, runs in groups.items():
        assert len(runs) == 10
        p99s = [r[0] for r in runs]
        print(f"{key}: median_p99_ms={statistics.median(p99s):.6f} range_ms={min(p99s):.6f}..{max(p99s):.6f} mean_ms={statistics.mean(r[3] for r in runs):.6f} max_ms={max(r[1] for r in runs):.6f} >=1ms={sum(r[2] for r in runs)}/10000")
