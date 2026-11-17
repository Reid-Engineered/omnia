"""Recompute run p99s (nearest rank), median of ten, and diagnostic tails.

Reads ``raw.tar.gz`` beside this file (the archive of the fixed ten-pass
campaign in ``run.sh``). The gate statistic is the median of the ten run p99s
against the unchanged strict 1 ms target; ``>=1 ms`` counts are diagnostics
only, never an all-samples gate. ``index_10k/query`` has no 1 ms target and is
reported without a verdict. This script never marks a budget as met: only a
complete campaign on the reference machine under plan section 2.1 can do that,
in a separate roadmap/gate decision.
"""
import csv
import io
import statistics
import tarfile
from pathlib import Path

root = Path(__file__).parent
TARGET_MS = 1.0

# Frozen case list: key is "<case>/<operation>" as printed in the summary.
EXPECTED_KEYS = {
    "real_10k_leaf/add",
    "real_10k_leaf/remove",
    "label_5k_gnd_vcc_bridge/add",
    "label_5k_gnd_vcc_bridge/remove",
    "chain_20k_middle/add",
    "chain_20k_middle/remove",
    "index_10k/query",
    "doc_10k_wire_add/apply",
    "doc_10k_wire_delete/apply",
    "doc_10k_move_100/apply",
    "doc_chain_20k_add/apply",
    "doc_chain_20k_delete/apply",
    "doc_label_bridge_add/apply",
    "doc_label_bridge_delete/apply",
}
NO_TARGET_KEYS = {"index_10k/query"}

groups = {}
with tarfile.open(root / "raw.tar.gz") as archive:
    for member in sorted(archive.getmembers(), key=lambda m: m.name):
        if not member.name.endswith(".csv"):
            continue
        rows = list(csv.DictReader(io.TextIOWrapper(archive.extractfile(member))))
        assert len(rows) == 1000, f"{member.name}: expected 1000 rows, got {len(rows)}"
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

assert set(groups) == EXPECTED_KEYS, (
    f"frozen case set mismatch: missing={sorted(EXPECTED_KEYS - set(groups))} "
    f"extra={sorted(set(groups) - EXPECTED_KEYS)}"
)
print("\nMedian of 10 run p99s; range; all-observation mean/max/misses (diagnostic):")
print(f"Target: median_p99_ms < {TARGET_MS:.1f} ms (strict, unchanged). "
      ">=1ms counts are diagnostics, not an all-samples gate.")
for key, runs in sorted(groups.items()):
    assert len(runs) == 10, f"{key}: expected 10 runs, got {len(runs)}"
    p99s = [r[0] for r in runs]
    median_p99 = statistics.median(p99s)
    verdict = "n/a (no 1 ms target)" if key in NO_TARGET_KEYS else (
        "below target" if median_p99 < TARGET_MS else "at/above target"
    )
    print(f"{key}: median_p99_ms={median_p99:.6f} range_ms={min(p99s):.6f}..{max(p99s):.6f} mean_ms={statistics.mean(r[3] for r in runs):.6f} max_ms={max(r[1] for r in runs):.6f} >=1ms={sum(r[2] for r in runs)}/10000 verdict={verdict}")
