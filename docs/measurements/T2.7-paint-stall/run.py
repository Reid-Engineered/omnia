#!/usr/bin/env python3
"""Diagnostic Orion campaign for the T2.7 paint stalls. Evidence only; never a gate verdict.

Runs `bench run` with the diagnostic trace and the per-paint probe
(`OMNIA_BENCH_PAINT_PROBE`, crates/omnia-bench/src/paint_probe.rs) at 100% scale: ten
repetitions of 1k pan and 10k zoom, interleaved, the pair reversed on even repetitions.
The schedule, cases and binary are fixed before the first run. Each run is bracketed by a
condition snapshot (panel, AC, governors, processes) and the kernel's compaction and THP
counters from /proc/vmstat.

Usage, on Orion in its Wayland session, from a clean checkout with a release build:
    python3 -B docs/measurements/T2.7-paint-stall/run.py OUTPUT_DIR
"""

import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "bench-baseline"))
from reference import command, dump, panel, set_scale, snapshot

FRAMES = 600
REPETITIONS = 10
SCALE = 1.0
CASES = (("1k", "pan"), ("10k", "zoom"))
VMSTAT = ("compact_stall", "compact_fail", "compact_success", "compact_migrate_scanned",
          "compact_free_scanned", "thp_fault_alloc", "thp_fault_fallback", "thp_collapse_alloc",
          "pgfault", "pgmajfault", "pgscan_kswapd", "pgscan_direct", "allocstall_normal",
          "allocstall_movable")


def schedule():
    order = []
    for rep in range(1, REPETITIONS + 1):
        cases = CASES if rep % 2 else CASES[::-1]
        order.extend([rep, sheet, script] for sheet, script in cases)
    return order


def vmstat():
    values = dict(line.split() for line in Path("/proc/vmstat").read_text().splitlines())
    return {key: int(values[key]) for key in VMSTAT if key in values}


def fingerprint(binary):
    with binary.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return {"sha256": digest, "commit": command("git", "-C", ROOT, "rev-parse", "HEAD"),
            "tracked_status": command("git", "-C", ROOT, "status", "--porcelain",
                                      "--untracked-files=no")}


def main():
    output = Path(sys.argv[1]).resolve()
    binary = ROOT / "target/release/omnia-bench"
    if os.environ.get("XDG_SESSION_TYPE") != "wayland" or not binary.is_file():
        sys.exit("run on Orion in its Wayland session after a release build")
    if command("git", "-C", ROOT, "status", "--porcelain"):
        sys.exit("run from a clean checkout")
    output.mkdir(parents=True)
    order = schedule()
    dump(output / "order.json", order)
    dump(output / "binary-before.json", fingerprint(binary))
    (output / "status.txt").write_text("incomplete\n", encoding="utf-8")
    original = panel()["scale"]
    env = {**os.environ, "OMNIA_BENCH_FULLSCREEN": "1", "OMNIA_BENCH_TRACE": "1",
           "OMNIA_BENCH_PAINT_PROBE": "1"}
    env.pop("WAYLAND_DEBUG", None)
    failures = 0
    try:
        set_scale(SCALE)
        with (output / "runs.jsonl").open("w", encoding="utf-8") as journal:
            for rep, sheet, script in order:
                stem = output / f"{sheet}-{script}-{rep}"
                print(f"{rep}/{REPETITIONS} {sheet}-{script}", flush=True)
                before = {"conditions": snapshot(), "vmstat": vmstat()}
                with stem.with_suffix(".log").open("w", encoding="utf-8") as log:
                    code = subprocess.run(
                        [str(binary), "run", "--sheet", sheet, "--script", script,
                         "--warmup", str(FRAMES), "--frames", str(FRAMES),
                         "--json", str(stem.with_suffix(".json"))],
                        env=env, stdout=log, stderr=subprocess.STDOUT).returncode
                after = {"conditions": snapshot(), "vmstat": vmstat()}
                failures += code != 0
                journal.write(json.dumps({"rep": rep, "case": f"{sheet}-{script}", "exit": code,
                                          "before": before, "after": after}) + "\n")
                journal.flush()
    finally:
        set_scale(original)
    dump(output / "binary-after.json", fingerprint(binary))
    status = "complete" if failures == 0 else f"complete with {failures} failed runs"
    (output / "status.txt").write_text(status + "\n", encoding="utf-8")
    print(status)


if __name__ == "__main__":
    main()
