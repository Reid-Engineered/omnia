#!/usr/bin/env python3
"""Run or replay the T7.3 instrument window stub gate on Orion (canvas plan 2.1)."""

import argparse
from collections import defaultdict
from datetime import datetime, timezone
import hashlib
import io
import json
import math
import os
from pathlib import Path
import platform
import signal
from statistics import median
import subprocess
import sys
from contextlib import redirect_stdout

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "bench-baseline"))
from compare_orion import FRAMES, REPETITIONS, SCALES, check_conditions, read_report
from reference import PANEL, command, dump, set_scale, snapshot

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "T1.6"))
from attribute import attribute, describe

CASES = ("10k-pan-inst60", "10k-pan-inst30")
TARGET_P99_MS = 16.0
B5_MAX_MS = 50.0
B9_MAX_MB = 300.0
SCENE_COUNTS = (10_000, 20_000)
CPU = "i5-7300U"


def case_hz(case):
    if case == "10k-pan-inst60":
        return 60
    if case == "10k-pan-inst30":
        return 30
    raise ValueError(f"unknown T7.3 case: {case!r}")


def schedule():
    order = []
    for tag in SCALES:
        for index in range(REPETITIONS):
            pair = CASES if index % 2 == 0 else CASES[::-1]
            for case in pair:
                order.append([tag, index + 1, case])
    return order


def args_for(case, size):
    hz = case_hz(case)
    return [
        "run",
        "--sheet",
        "10k",
        "--script",
        "pan",
        "--instrument",
        "--instrument-hz",
        str(hz),
        "--warmup",
        str(FRAMES),
        "--frames",
        str(FRAMES),
        "--size",
        size,
    ], True


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def fingerprint(binary):
    with binary.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return {
        "sha256": digest,
        "commit": command("git", "-C", ROOT, "rev-parse", "HEAD"),
        "tracked_status": command(
            "git", "-C", ROOT, "status", "--porcelain", "--untracked-files=no"
        ),
    }


def active_panel():
    outputs = json.loads(command("kscreen-doctor", "--json"))["outputs"]
    native = next((output for output in outputs if output["name"] == PANEL), None)
    if native is None or not native["enabled"]:
        raise ValueError(f"expected the native {PANEL} panel to be enabled")
    return native


def capture():
    active_panel()
    conditions = snapshot()
    conditions["time_utc"] = datetime.now(timezone.utc).isoformat()
    conditions["dpms"] = command("kscreen-doctor", "--dpms", "show")
    return conditions


def validate_conditions(conditions, scale):
    check_conditions(conditions, scale)
    governors = conditions["governor"]
    if len(governors) != 4 or not conditions["processes"] or not conditions["time_utc"]:
        raise ValueError("expected all four CPU governors and a timestamped process snapshot")
    if set(governors.values()) != {"performance"}:
        raise ValueError(f"expected performance CPU governor on all four CPUs: {governors}")
    if not any(
        line.strip().lower() == "dpms mode for screen edp-1: on"
        for line in conditions["dpms"].splitlines()
    ):
        raise ValueError(f"expected active eDP-1 panel: {conditions['dpms']}")


def collect(binary, output):
    if platform.system() != "Linux" or os.environ.get("XDG_SESSION_TYPE") != "wayland":
        raise ValueError("run on Orion in its local Wayland session")
    cpu = next(
        line.split(":", 1)[1].strip()
        for line in Path("/proc/cpuinfo").read_text().splitlines()
        if line.startswith("model name")
    )
    if CPU not in cpu or not binary.is_file() or not os.access(binary, os.X_OK):
        raise ValueError("expected Orion's CPU and a prebuilt release bench executable")
    source_status = command("git", "-C", ROOT, "status", "--porcelain")
    if source_status:
        raise ValueError("run from a clean source checkout")
    initial_governors = capture()["governor"]
    if len(initial_governors) != 4 or set(initial_governors.values()) != {"performance"}:
        raise ValueError(
            f"expected performance CPU governor on all four CPUs: {initial_governors}"
        )
    original_scale = active_panel()["scale"]
    order = schedule()
    output.mkdir(parents=True)
    (output / "status.txt").write_text("incomplete\n", encoding="utf-8")
    for tag in SCALES:
        (output / tag).mkdir()
    dump(
        output / "hardware.json",
        {"cpu": cpu, "os": command("uname", "-a"), "session": "wayland", "panel": "eDP-1"},
    )
    dump(output / "source.json", {"binary": str(binary), "source_status": source_status})
    dump(output / "order.json", order)
    dump(output / "binary-before.json", fingerprint(binary))

    env = {**os.environ, "OMNIA_BENCH_FULLSCREEN": "1"}
    env.pop("WAYLAND_DEBUG", None)
    env.pop("OMNIA_BENCH_TRACE", None)
    previous_tag = None
    try:
        with (output / "runs.jsonl").open("x", encoding="utf-8") as journal:
            for tag, repetition, case in order:
                scale, viewport = SCALES[tag]
                if tag != previous_tag:
                    set_scale(scale)
                    previous_tag = tag
                stem = output / tag / f"{case}-{repetition}"
                size = f"{viewport[0]}x{viewport[1]}"
                path = stem.with_suffix(".json")
                args, traced_run = args_for(case, size)
                run_env = {**env, "OMNIA_BENCH_TRACE": "1"} if traced_run else env
                print(f"{tag} {repetition}/{REPETITIONS} {case}", flush=True)
                identity = [tag, repetition, case]
                before = capture()
                write_record(journal, identity, "before", before, None)
                validate_conditions(before, scale)
                with stem.with_suffix(".log").open("xb") as log:
                    result = subprocess.run(
                        [binary, *args, "--json", path],
                        env=run_env,
                        stdout=log,
                        stderr=subprocess.STDOUT,
                    )
                after = capture()
                write_record(journal, identity, "after", after, result.returncode)
                result.check_returncode()
                validate_conditions(after, scale)
    finally:
        try:
            set_scale(original_scale)
        finally:
            dump(output / "binary-after.json", fingerprint(binary))


def write_record(journal, case, phase, conditions, exit_code):
    record = {"case": case, "phase": phase, "conditions": conditions, "exit": exit_code}
    journal.write(json.dumps(record) + "\n")
    journal.flush()


def metric(value):
    if type(value) not in (int, float) or not math.isfinite(value) or value < 0:
        raise ValueError(f"invalid metric: {value!r}")
    return value


def integer_count(value):
    if type(value) is not int or value < 0:
        raise ValueError(f"invalid count: {value!r}")
    return value


def motion(report, case, scale, viewport):
    hz = case_hz(case)
    expected_stride = 60 // hz
    expected_paints = FRAMES // expected_stride
    config = {
        "sheet": "10k",
        "script": "pan",
        "frames": FRAMES,
        "warmup": FRAMES,
        "window_logical": viewport,
        "scene_counts": False,
        "diagnostic_trace": True,
        "instrument": True,
        "instrument_hz": hz,
    }
    if report["config"] != config or report["scene_details"] != {
        "seed": 42,
        "parts": SCENE_COUNTS[0],
        "wires": SCENE_COUNTS[1],
    }:
        raise ValueError("unexpected motion configuration or stress sheet")

    environment = report["environment"]
    gpu = environment["gpu"]
    if (
        environment["fullscreen"] is not True
        or environment["scale_factor"] != scale
        or environment["viewport_logical"] != viewport
        or environment["viewport_device"] != [1920, 1080]
        or not 59.9 <= environment["refresh_hz"] <= 60.1
        or gpu["is_software_emulated"] is not False
        or "HD Graphics 620" not in gpu["device_name"]
        or "Mesa" not in gpu["driver_name"]
    ):
        raise ValueError(
            f"expected the reference viewport, refresh and Vulkan adapter: {environment}"
        )

    instrument = report.get("instrument") or {}
    if (
        instrument.get("hz") != hz
        or integer_count(instrument.get("stride")) != expected_stride
        or integer_count(instrument.get("paints")) != expected_paints
        or integer_count(instrument.get("ticks")) != expected_paints
        or integer_count(instrument.get("renders")) != expected_paints
    ):
        raise ValueError(f"incomplete or unexpected instrument exercise: {instrument}")

    frames = report["frames"]
    metrics = report["metrics"]
    if (
        report["warnings"]
        or frames["drawn"] != FRAMES
        or frames["unmeasured"] != 0
        or frames["on_time"] + frames["late"] != FRAMES
        or metrics["paint_sampling"] != "every-frame"
        or metrics["paint_samples"] != FRAMES
    ):
        raise ValueError(f"incomplete frame sampling: {frames}, warnings {report['warnings']}")

    p99 = metric(metrics["frame_ms"]["p99"])
    maximum = metric(metrics["frame_ms"]["max"])
    if not 0 < p99 <= maximum:
        raise ValueError(f"invalid frame p99 or maximum: {p99}, {maximum}")
    ours = metric(metrics["allocs_per_frame"]["ours"]["max"])
    memory = metric(metrics["memory_mb"])
    if memory == 0:
        raise ValueError("expected positive private memory")
    traced_b4 = attribute(report, 2.5)
    traced_b3 = attribute(report, 1.5)
    return (p99, maximum, ours, memory, traced_b4, traced_b3)


def med_range(values):
    return f"{median(values):.3f} [{min(values):.3f}, {max(values):.3f}]"


def summarize(output):
    order = schedule()
    if load(output / "order.json") != order:
        raise ValueError("recorded order does not match the frozen schedule")
    records = [
        json.loads(line)
        for line in (output / "runs.jsonl").read_text(encoding="utf-8").splitlines()
    ]
    expected_records = [(case, phase) for case in order for phase in ("before", "after")]
    if [(record["case"], record["phase"]) for record in records] != expected_records:
        raise ValueError("incomplete campaign or run journal out of order")
    before = load(output / "binary-before.json")
    if before != load(output / "binary-after.json") or before["tracked_status"]:
        raise ValueError("benchmark executable or source checkout changed")
    hardware = load(output / "hardware.json")
    if (
        CPU not in hardware["cpu"]
        or not hardware["os"].startswith("Linux ")
        or hardware["session"] != "wayland"
        or hardware["panel"] != "eDP-1"
    ):
        raise ValueError("unexpected reference machine or session")
    print(
        f"Orion: {hardware['cpu']}, {hardware['os']}, {hardware['session']}, "
        f"{hardware['panel']}, source {before['commit']}, binary SHA-256 {before['sha256']}"
    )
    runs = defaultdict(list)
    attribution = []
    conditions = defaultdict(set)
    adapters = set()
    run_pairs = zip(records[::2], records[1::2], strict=True)
    for (pre, post), (tag, repetition, case) in zip(run_pairs, order, strict=True):
        scale, viewport = SCALES[tag]
        stem = output / tag / f"{case}-{repetition}"
        try:
            if pre["exit"] is not None or post["exit"] != 0:
                raise ValueError(
                    f"unexpected benchmark exit records: {pre['exit']}, {post['exit']}"
                )
            for record in (pre, post):
                state = record["conditions"]
                validate_conditions(state, scale)
                conditions[tag].add(
                    (
                        tuple(sorted(set(state["governor"].values()))),
                        state["power_profile"],
                        tuple(state["platform_profile"].values()),
                    )
                )
            report = read_report(stem)
            if (
                report["schema"] != 1
                or report["kind"] != "run"
                or report["environment"]["os"] != "linux"
            ):
                raise ValueError("unexpected report schema, kind or OS")
            runs[tag, case].append(motion(report, case, scale, viewport))
            adapters.add(
                (
                    report["environment"]["gpu"]["device_name"],
                    report["environment"]["gpu"]["driver_name"],
                )
            )
            attribution.extend(describe(f"{tag}/{stem.name}", runs[tag, case][-1][-2]))
        except (OSError, TypeError, KeyError, ValueError) as error:
            raise ValueError(f"{stem}: {error}") from error

    print(f"Vulkan device and driver: {sorted(adapters)}")
    t7_3_60 = []
    t7_3_30 = []
    extra_60 = []
    extra_30 = []

    def verdict(target_list, passed, summary):
        target_list.append(passed)
        print(f"{'PASS' if passed else 'FAIL'} {summary}")

    for tag, (scale, viewport) in SCALES.items():
        print(f"\n{tag}: scale {scale}, {viewport}, native 1920x1080 at 60 Hz, AC power")
        print(
            f"CPU governors, power profile and platform profile: {sorted(conditions[tag], key=str)}"
        )
        for case in CASES:
            t7_3_target = t7_3_60 if case == "10k-pan-inst60" else t7_3_30
            extra_target = extra_60 if case == "10k-pan-inst60" else extra_30
            p99s, maxes, allocs, memory, traced_b4, traced_b3 = zip(
                *runs[tag, case], strict=True
            )
            verdict(
                t7_3_target,
                median(p99s) < TARGET_P99_MS,
                f"T7.3 {case} frame p99 {med_range(p99s)} ms (< {TARGET_P99_MS})",
            )
            passed_b4 = sum(result["passed"] for result in traced_b4)
            verdict(
                extra_target,
                passed_b4 == REPETITIONS,
                f"B4 {case} 2.5R attribution {passed_b4}/{REPETITIONS} passed",
            )
            passed_b3 = sum(result["passed"] for result in traced_b3)
            print(f"INFO {case} 1.5R attribution {passed_b3}/{REPETITIONS} passed")
            verdict(
                extra_target,
                max(maxes) <= B5_MAX_MS,
                f"B5 {case} worst frame {max(maxes):.3f} ms (<= {B5_MAX_MS})",
            )
            verdict(
                extra_target,
                max(allocs) == 0,
                f"B10 {case} max Omnia allocations/frame {max(allocs):g} (= 0)",
            )
            verdict(
                extra_target,
                max(memory) < B9_MAX_MB,
                f"B9 {case} private memory {med_range(memory)} MB (all < {B9_MAX_MB})",
            )
    print("\nMissed-refresh attribution (2.5R)")
    print("\n".join(attribution))
    t7_3_60_ok = all(t7_3_60)
    t7_3_30_ok = all(t7_3_30)
    extra_60_ok = all(extra_60)
    extra_30_ok = all(extra_30)
    print(f"\nT7.3 done-when (60 Hz p99 < {TARGET_P99_MS:g} ms): {'PASS' if t7_3_60_ok else 'FAIL'}")
    print(f"T7.3 done-when (30 Hz p99 < {TARGET_P99_MS:g} ms): {'PASS' if t7_3_30_ok else 'FAIL'}")
    print(f"Additional budgets (B4/B5/B9/B10, 60 Hz): {'PASS' if extra_60_ok else 'FAIL'}")
    print(f"Additional budgets (B4/B5/B9/B10, 30 Hz): {'PASS' if extra_30_ok else 'FAIL'}")
    if t7_3_60_ok and extra_60_ok:
        return True
    if not t7_3_60_ok and t7_3_30_ok:
        if extra_30_ok:
            print(
                "SUMMARY: 60 Hz missed T7.3 done-when; 30 Hz fallback passed T7.3 done-when and all additional budgets."
            )
        else:
            print(
                "SUMMARY: 60 Hz missed T7.3 done-when; 30 Hz fallback passed T7.3 done-when but missed an additional budget."
            )
    elif t7_3_60_ok and not extra_60_ok:
        print(
            "SUMMARY: 60 Hz passed T7.3 done-when (p99 < 16 ms) but missed an additional budget (B4/B5/B9/B10)."
        )
    return False


def interrupted(_signum, _frame):
    raise KeyboardInterrupt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    run = sub.add_parser("run", help="capture and summarize a fresh Orion campaign")
    run.add_argument("output", type=Path)
    run.add_argument("--bench", type=Path, default=ROOT / "target/release/omnia-bench")
    replay = sub.add_parser(
        "summarize", help="verify an existing campaign without running benchmarks"
    )
    replay.add_argument("output", type=Path)
    args = parser.parse_args()
    try:
        output = args.output.resolve()
        if args.action == "run":
            signal.signal(signal.SIGTERM, interrupted)
            collect(args.bench.resolve(), output)
            summary = io.StringIO()
            with redirect_stdout(summary):
                passed = summarize(output)
            (output / "summary.txt").write_text(summary.getvalue(), encoding="utf-8")
            (output / "status.txt").write_text(
                "pass\n" if passed else "fail\n", encoding="utf-8"
            )
            print(f"{'pass' if passed else 'fail'}: {output / 'summary.txt'}")
        else:
            passed = summarize(output)
        return int(not passed)
    except KeyboardInterrupt:
        print("campaign interrupted", file=sys.stderr)
        return 130
    except StopIteration:
        print("incomplete: could not identify the reference CPU or panel mode", file=sys.stderr)
        return 2
    except (OSError, subprocess.CalledProcessError, KeyError, TypeError, ValueError) as error:
        print(f"incomplete: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
