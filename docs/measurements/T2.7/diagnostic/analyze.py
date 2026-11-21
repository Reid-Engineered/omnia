"""Read the extracted Orion diagnostic and match sheet spans to measured frame ordinals."""
from collections import defaultdict
import json
from pathlib import Path
from statistics import median
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "T1.6"))
from attribute import timeline, attribute

out = Path(sys.argv[1])
fields = ["query", "item-sort", "wire-prep", "wire-emit", "boxes", "symbols-dots",
          "other-draw", "present", "signal-gap"]
rows = defaultdict(dict)
misses = []
for tag, rep, case in json.loads((out / "order.json").read_text()):
    stem = out / f"{tag}-{case}-{rep}"
    report = json.loads(stem.with_suffix(".json").read_text())
    samples = [list(map(int, line.split()[1:])) for line in stem.with_suffix(".log").read_text().splitlines()
               if line.startswith("PHASE ")]
    warmup, frames = report["config"]["warmup"], report["config"]["frames"]
    # RunView also paints the reporting frame after collecting the trace.
    if len(samples) != warmup + frames + 1:
        raise ValueError(f"{stem}: expected {warmup + frames + 1} phase records, got {len(samples)}")
    for index, draw, present, previous, _ in timeline(report):
        parts = [v / 1e6 for v in samples[warmup + index][1:]]
        draw_ms = (draw["end_ns"] - draw["start_ns"]) / 1e6
        residual = draw_ms - sum(parts)
        if residual < 0:
            raise ValueError(f"{stem}/{index}: sheet spans exceed draw")
        rows[tag, case, index][rep] = parts + [residual, (present["end_ns"] - present["start_ns"]) / 1e6,
                                               (draw["signal_ns"] - previous["end_ns"]) / 1e6]
    result = attribute(report, 2.5 if case == "10k-zoom" else 1.5)
    for miss in result["misses"]:
        if miss["verdict"] == "counted":
            misses.append((tag, case, rep, miss["index"], miss["interval_ms"]))
print("Units: ms. Deltas use the median of the other nine runs at the same scale and frame.")
for tag, case, rep, index, interval in misses:
    peers = rows[tag, case, index]
    actual = peers[rep]
    typical = [median(row[i] for n, row in peers.items() if n != rep) for i in range(len(fields))]
    print(f"{tag} {case} run {rep} frame {index} interval {interval:.3f}")
    print("  observed " + " ".join(f"{f}={v:.3f}" for f, v in zip(fields, actual)))
    print("  excess   " + " ".join(f"{f}={a-b:+.3f}" for f, a, b in zip(fields, actual, typical)))
print(f"Counted intervals: {len(misses)}")
for tag in ("s100", "s125"):
    far = [sample for (scale, case, index), runs in rows.items()
           if scale == tag and case == "10k-zoom" and 440 <= index < 500 for sample in runs.values()]
    print(f"{tag} 10k-zoom frames 440..499 across ten runs, median over {len(far)} frames")
    print("  " + " ".join(f"{f}={median(r[i] for r in far):.3f}" for i, f in enumerate(fields)))
for tag in ("s100", "s125"):
    for case in ("10k-zoom", "1k-pan"):
        draw, index, rep, observed = max(
            (sum(r[:7]), i, n, r) for (s, c, i), runs in rows.items()
            if (s, c) == (tag, case) and (case == "1k-pan" or 440 <= i < 500)
            for n, r in runs.items())
        peers = rows[tag, case, index]
        excess = [v - median(r[i] for n, r in peers.items() if n != rep)
                  for i, v in enumerate(observed)]
        print(f"{tag} {case} slowest diagnostic draw in inspected window: run {rep} frame {index} {draw:.3f} ms")
        print("  excess   " + " ".join(f"{f}={v:+.3f}" for f, v in zip(fields, excess)))
