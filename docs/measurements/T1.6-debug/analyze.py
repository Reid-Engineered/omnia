"""Validate diagnostic timelines and print the longest measured interval's phases.

Usage: python analyze.py RESULTS. Accepts plain JSON or archived JSON.gz reports.
The trace includes warmup; histogram comparison uses only the following measured frames.
"""
import gzip
import json
import pathlib
import sys

for path in sorted(pathlib.Path(sys.argv[1]).glob("pan-*.json*")):
    if path.name.endswith(".log") or ".log." in path.name:
        continue
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rt") as stream:
        report = json.load(stream)
    assert not report["warnings"], (path, report["warnings"])
    assert report["frames"]["drawn"] == report["config"]["frames"]
    m = report["metrics"]
    print(path.name, "misses", m["missed_refreshes"], "max_ms", m["max_present_interval_ms"])
    trace = report.get("diagnostic_trace")
    if not trace:
        continue
    events = trace["events"]
    warmup, frames = report["config"]["warmup"], report["config"]["frames"]
    draws = [e for e in events if e["event"] == "draw"]
    assert len(draws) == warmup + frames, (path, len(draws))
    # Startup can draw twice before presenting: GPUI replaces its pending frame.
    pairs = []
    pending, previous, ordinal = None, None, -1
    for e in events:
        if e["event"] == "draw":
            ordinal += 1
            pending = e
        else:
            assert pending is not None
            pairs.append((ordinal, pending, e, previous))
            pending, previous = None, e
    measured = [row for row in pairs if row[0] >= warmup]
    assert len(measured) == frames
    threshold = 1.5e9 / report["environment"]["refresh_hz"]
    # Cases here are far from the HDR bucket threshold; rounding cannot change the count.
    assert sum(p["animation_interval_ns"] > threshold for _, d, p, _ in measured) == m["missed_refreshes"]
    index, d, p, previous = max(measured, key=lambda row: row[2]["animation_interval_ns"] )
    interval = p["end_ns"] - previous["end_ns"]
    assert interval == p["animation_interval_ns"]
    # HDR reports the bucket's upper bound, not the raw value.
    assert abs(max(p["animation_interval_ns"] for _, d, p, _ in measured) / 1e6 - m["max_present_interval_ms"]) < .1
    phases = dict(
        since_previous_present_to_draw=(d["start_ns"] - previous["end_ns"]),
        draw=d["end_ns"] - d["start_ns"],
        draw_end_to_present_start=p["start_ns"] - d["end_ns"],
        platform_present=p["end_ns"] - p["start_ns"],
    )
    assert sum(phases.values()) == interval
    print("  frame", index, "measured_index", index - warmup,
          "interval_ms", interval / 1e6,
          "phases_ms", {k: round(v / 1e6, 6) for k, v in phases.items()},
          "signal_to_draw_ms", None if d["signal_ns"] is None else (d["start_ns"] - d["signal_ns"]) / 1e6)
