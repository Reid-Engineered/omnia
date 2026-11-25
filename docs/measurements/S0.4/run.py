#!/usr/bin/env python3
"""Run the frozen S0.4 cadence comparison and retain the reports and conditions."""

import argparse
from collections import defaultdict
from datetime import datetime, timezone
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import signal
from statistics import median
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "bench-baseline"))
from compare_orion import FRAMES, REPETITIONS, SCALES, check_conditions
from reference import command, dump, set_scale, snapshot

sys.path.insert(0, str(ROOT / "docs/measurements/T1.6"))
from attribute import attribute


def schedule(reference):
    scales = SCALES if reference else {"windows": (None, [1536, 864])}
    order = []
    for tag, (scale, viewport) in scales.items():
        for repetition in range(1, REPETITIONS + 1):
            rates = (30, 60) if repetition % 2 else (60, 30)
            for hz in rates:
                order.append(dict(
                    scale_tag=tag, scale=scale, size=viewport, repetition=repetition, hz=hz,
                    frames=FRAMES if reference else 120, warmup=FRAMES if reference else 60,
                ))
    return order


def capture(reference, scale=None):
    if reference:
        state = snapshot()
        state["dpms"] = command("kscreen-doctor", "--dpms", "show")
        if set(state["governor"].values()) != {"performance"} or len(state["governor"]) != 4:
            raise ValueError(f"Orion requires four performance governors: {state['governor']}")
        if "dpms mode for screen edp-1: on" not in state["dpms"].lower().splitlines():
            raise ValueError(f"Orion's eDP-1 panel must remain awake: {state['dpms']}")
        check_conditions(state, state["panel"]["scale"] if scale is None else scale)
    else:
        state = json.loads(command(
            "powershell.exe", "-NoProfile", "-File", Path(__file__).with_name("windows-conditions.ps1")
        ))
    state["captured_utc"] = datetime.now(timezone.utc).isoformat()
    return state


def fingerprint(binary):
    with binary.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    bench = ROOT / "crates/omnia-bench/src"
    return {
        "sha256": digest,
        "source_head": command("git", "-C", ROOT, "rev-parse", "HEAD"),
        "source_sha256": {
            name: hashlib.sha256((bench / name).read_bytes()).hexdigest()
            for name in ("main.rs", "spike/mod.rs", "spike/sim_scope.rs")
        },
    }


def validate_report(report, case, reference):
    env = report["environment"]
    cfg = report["config"]
    frames = report["frames"]
    if (
        report["kind"] != "spike-sim-scope"
        or cfg["sheet"] != "1k"
        or cfg["sample_rate_per_channel"] != 100_000
        or cfg["capacity_per_channel"] != 65_536
        or any(cfg[key] != case[key] for key in ("hz", "frames", "warmup"))
        or cfg["window_logical"] != case["size"]
        or any(frames[key] != case["frames"] for key in ("drawn", "presents", "work_samples"))
        or env["os"] != ("linux" if reference else "windows")
    ):
        raise ValueError("report does not match the frozen workload")
    if reference:
        gpu = env["gpu"] or {}
        names = " ".join(str(gpu.get(k, "")) for k in ("device_name", "driver_name")).lower()
        if (
            not env["fullscreen"]
            or env["scale_factor"] != case["scale"]
            or env["viewport_logical"] != case["size"]
            or env["viewport_device"] != [1920, 1080]
            or not 59.9 <= (env["refresh_hz"] or 0) <= 60.1
            or gpu.get("is_software_emulated") is not False
            or not all(word in names for word in ("intel", "620", "mesa"))
        ):
            raise ValueError(f"unexpected Orion display or Vulkan adapter: {env}")


def summarize(runs):
    rows = []
    for (tag, hz), results in runs.items():
        p99 = [r["metrics"]["frame_ms"]["p99"] for r, _ in results]
        metrics = [r["metrics"] for r, _ in results]
        streams = [r["stream"] for r, _ in results]
        checks = [check for _, check in results]
        rows.append({
            "scale_tag": tag, "hz": hz, "runs": len(results),
            "frame_p99_median_ms": median(p99), "frame_p99_range_ms": [min(p99), max(p99)],
            "max_frame_ms": max(m["frame_ms"]["max"] for m in metrics),
            "total_raw_misses": sum(m["missed_refreshes"] for m in metrics),
            "b3_counted_misses": sum(c["counted"] for c in checks)
            if all(c["usable"] for c in checks) else None,
            "unusable_traces": [c["error"] for c in checks if not c["usable"]],
            "max_sheet_paints": max(m["sheet_paints"] for m in metrics),
            "max_ours_allocs_per_frame": max(m["allocs_per_frame"]["ours"]["max"] for m in metrics),
            "median_total_allocs_per_frame": median(m["allocs_per_frame"]["total"]["mean"] for m in metrics),
            "max_memory_mib": max(m["memory_mb"] for m in metrics),
            "max_working_set_mib": max(m["working_set_mb"] for m in metrics),
            "stream_hz_range": [
                min(s["measured_scope_hz"] for s in streams), max(s["measured_scope_hz"] for s in streams)
            ],
            "sample_rate_range": [
                min(s["attempted_pairs_per_second"] for s in streams),
                max(s["attempted_pairs_per_second"] for s in streams),
            ],
            "total_dropped_pairs": sum(s["dropped_pairs"] for s in streams),
            "total_empty_updates": sum(s["empty_scope_updates"] for s in streams),
            "unexpected_scope_paint_runs": sum(
                s["scope_paints"] != s["scheduled_scope_updates"] for s in streams
            ),
        })
    return rows


def collect(binary, output, reference):
    output.mkdir(parents=True)
    status = output / "status.txt"
    status.write_text("incomplete\n", encoding="utf-8")
    order = schedule(reference)
    dump(output / "order.json", order)
    binary_before = fingerprint(binary)
    dump(output / "binary-before.json", binary_before)
    before = capture(reference)
    if reference:
        before["cpu"] = next(
            line.split(":", 1)[1].strip()
            for line in Path("/proc/cpuinfo").read_text().splitlines()
            if line.startswith("model name")
        )
        before["os"] = command("uname", "-a")
        before["session"] = os.environ["XDG_SESSION_TYPE"]
    dump(output / "conditions-before.json", before)
    original_scale = before["panel"]["scale"] if reference else None
    env = {**os.environ, "OMNIA_BENCH_TRACE": "1"}
    if reference:
        env["OMNIA_BENCH_FULLSCREEN"] = "1"
        env.pop("WAYLAND_DEBUG", None)
    runs = defaultdict(list)
    scale = original_scale
    try:
        for case in order:
            tag, repetition, hz = case["scale_tag"], case["repetition"], case["hz"]
            stem = output / f"{tag}-{repetition:02d}-{hz}hz"
            if reference:
                if case["scale"] != scale:
                    scale = case["scale"]
                    set_scale(scale)
                dump(stem.with_suffix(".before.json"), capture(True, scale))
            args = [
                str(binary), "spike", "sim-scope", "--hz", str(hz),
                "--frames", str(case["frames"]), "--warmup", str(case["warmup"]),
                "--size", "x".join(map(str, case["size"])),
            ]
            print(stem.name, flush=True)
            # Capture even failed invocations. The campaign retains its incomplete status
            # and ends if a case cannot produce a report under the scheduled conditions.
            with (stem.with_suffix(".json")).open("xb") as raw, stem.with_suffix(".log").open("xb") as log:
                subprocess.run(args, stdout=raw, stderr=log, env=env, check=True, timeout=180)
            if reference:
                dump(stem.with_suffix(".after.json"), capture(True, scale))
            raw = stem.with_suffix(".json")
            payload = raw.read_bytes()
            report = json.loads(payload)
            validate_report(report, case, reference)
            attribution = attribute(report, 1.5)
            runs[(tag, hz)].append((
                {key: report[key] for key in ("metrics", "stream")},
                {key: attribution[key] for key in ("counted", "usable", "error")},
            ))
            if reference:
                dump(stem.with_suffix(".b3.json"), attribution)
            raw.with_suffix(".json.gz").write_bytes(gzip.compress(payload, mtime=0))
            raw.unlink()
            log_path = stem.with_suffix(".log")
            if log_path.stat().st_size == 0:
                log_path.unlink()
    finally:
        try:
            dump(output / "conditions-after.json", capture(reference, scale))
        finally:
            try:
                if reference and scale != original_scale:
                    set_scale(original_scale)
            finally:
                binary_after = fingerprint(binary)
                dump(output / "binary-after.json", binary_after)
    if binary_before != binary_after:
        raise ValueError("the binary or source head changed during the campaign")
    summary = summarize(runs)
    dump(output / "summary.json", summary)
    for row in summary:
        print(json.dumps(row), flush=True)
    status.write_text("complete\n" if reference else "complete (Windows diagnostic)\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("orion", "windows"))
    parser.add_argument("output", type=Path, help="new directory for raw reports")
    parser.add_argument("--binary", type=Path, help="use a prebuilt Windows diagnostic binary")
    args = parser.parse_args()
    reference = args.mode == "orion"
    if reference:
        if (
            platform.system() != "Linux"
            or os.environ.get("XDG_SESSION_TYPE") != "wayland"
            or os.environ.get("SSH_CONNECTION")
            or "i5-7300U" not in Path("/proc/cpuinfo").read_text()
        ):
            parser.error("run at Orion's desk in its local Wayland session")
        if args.binary or command("git", "-C", ROOT, "status", "--porcelain"):
            parser.error("the Orion campaign builds from a clean checkout")
        capture(True)
        # Run the same cleanup on terminal interruption and on Ctrl-C.
        signal.signal(signal.SIGTERM, signal.default_int_handler)
    elif platform.system() != "Windows":
        parser.error("Windows diagnostics require a Windows desktop session")
    if args.output.exists():
        parser.error("choose a new output directory to preserve prior runs")
    binary = args.binary
    if binary is None:
        subprocess.run(["cargo", "build", "--locked", "--release", "-p", "omnia-bench"], cwd=ROOT, check=True)
        metadata = json.loads(subprocess.check_output(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], cwd=ROOT
        ))
        name = "omnia-bench" if reference else "omnia-bench.exe"
        binary = Path(metadata["target_directory"]) / "release" / name
    collect(binary.resolve(strict=True), args.output.resolve(), reference)


if __name__ == "__main__":
    main()
